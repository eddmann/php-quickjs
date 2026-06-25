//! Error bridging between JS and PHP.

use rquickjs::{Ctx, Error as JsError};

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
