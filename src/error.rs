//! Error bridging between JS and PHP.

use ext_php_rs::error::Error as PhpError;
use rquickjs::{Ctx, Error as JsError};

/// Render a PHP-side error (typically a thrown PHP exception captured by
/// `ZendCallable::try_call`) into a clean `Class: message` string. Avoids the
/// default `Display`, which debug-prints the raw object (NUL-laden memory).
pub fn php_error_message(err: PhpError) -> String {
    match err {
        PhpError::Exception(obj) => {
            let class = obj.get_class_name().unwrap_or_else(|_| "Exception".to_owned());
            // `message` is a protected property; getMessage() reads it reliably.
            let msg = obj
                .try_call_method("getMessage", vec![])
                .ok()
                .and_then(|z| z.string())
                .unwrap_or_default();
            if msg.is_empty() {
                class
            } else {
                format!("{class}: {msg}")
            }
        }
        other => other.to_string(),
    }
}

/// Render an rquickjs error into a human-readable message, pulling the pending
/// JS exception (name/message/stack) out of the context when present.
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
                if msg.is_empty() {
                    name
                } else {
                    format!("{name}: {msg}")
                }
            } else {
                // A thrown non-Error value.
                val.as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| "uncaught JS exception".to_owned())
            }
        }
        other => other.to_string(),
    }
}
