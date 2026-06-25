//! Error bridging in both directions.
//!
//! - **PHP throws inside a callback** -> a JS `Error` whose `message` is the PHP
//!   message and whose `name`/`phpClass` carry the PHP exception class, so the
//!   guest can `catch (e) { if (e.phpClass === 'HttpError') ... }`.
//! - **JS throws past `eval`** -> a typed PHP exception (`QuickJSEvalException`
//!   et al.) carrying the JS error's name, message and stack.

use ext_php_rs::error::Error as PhpError;
use rquickjs::{Ctx, Error as JsError, Exception, Object};

/// A host-side failure carried back to JS: the originating class (a PHP
/// exception class, or `"Error"` for internal failures) plus a message.
pub struct HostError {
    pub class: String,
    pub message: String,
}

impl HostError {
    pub fn internal(message: impl Into<String>) -> Self {
        HostError {
            class: "Error".to_owned(),
            message: message.into(),
        }
    }
}

/// Classify a PHP-side error (typically a thrown PHP exception captured by
/// `ZendCallable::try_call`) into class + message. Avoids the default
/// `Display`, which debug-prints the raw object (NUL-laden memory).
pub fn php_exception_info(err: PhpError) -> HostError {
    match err {
        PhpError::Exception(obj) => {
            let class = obj.get_class_name().unwrap_or_else(|_| "Exception".to_owned());
            // `message` is a protected property; getMessage() reads it reliably.
            let message = obj
                .try_call_method("getMessage", vec![])
                .ok()
                .and_then(|z| z.string())
                .unwrap_or_default();
            HostError { class, message }
        }
        other => HostError::internal(other.to_string()),
    }
}

/// Throw a JS error carrying a host failure's class and message. The guest can
/// inspect `e.phpClass` to branch on the originating PHP exception type.
pub fn throw_host_error(ctx: &Ctx<'_>, err: &HostError) -> JsError {
    match Exception::from_message(ctx.clone(), &err.message) {
        Ok(ex) => {
            let obj: &Object = ex.as_object();
            let _ = obj.set("name", err.class.as_str());
            let _ = obj.set("phpClass", err.class.as_str());
            ex.throw()
        }
        Err(e) => e,
    }
}

/// Render an rquickjs error into a human-readable `Name: message` string,
/// pulling the pending JS exception out of the context when present. The bare
/// `Error` name is elided to avoid a redundant prefix.
pub fn js_error_message(ctx: &Ctx<'_>, err: JsError) -> String {
    match err {
        JsError::Exception => {
            let val = ctx.catch();
            if let Some(ex) = val.as_exception() {
                let msg = ex.message().unwrap_or_default();
                let name = ex
                    .get::<_, Option<String>>("name")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "Error".to_owned());
                match (name.as_str(), msg.is_empty()) {
                    (_, true) => name,
                    ("Error", false) => msg,
                    (_, false) => format!("{name}: {msg}"),
                }
            } else {
                val.as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| "uncaught JS exception".to_owned())
            }
        }
        other => other.to_string(),
    }
}
