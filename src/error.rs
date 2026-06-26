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

/// Render an rquickjs error into a human-readable `Name: message` string.
pub fn js_error_message(ctx: &Ctx<'_>, err: JsError) -> String {
    js_error_detail(ctx, err).0
}

/// Like [`js_error_message`], but also returns the JS stack (in generated-JS
/// coordinates) when the error carries one. The bare `Error` name is elided
/// from the message to avoid a redundant prefix.
pub fn js_error_detail(ctx: &Ctx<'_>, err: JsError) -> (String, Option<String>) {
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
                let message = match (name.as_str(), msg.is_empty()) {
                    (_, true) => name,
                    ("Error", false) => msg,
                    (_, false) => format!("{name}: {msg}"),
                };
                (message, ex.stack())
            } else {
                let message = val
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .unwrap_or_else(|| "uncaught JS exception".to_owned());
                (message, None)
            }
        }
        other => (other.to_string(), None),
    }
}

/// Remap a JS stack (generated coordinates) back to TypeScript coordinates
/// using the module's source map. Frames referencing `module_id` have their
/// `:line:col` rewritten to the original TS position; other lines pass through.
/// Returns `None` if the map cannot be parsed.
pub fn remap_stack(stack: &str, map_json: &str, module_id: &str) -> Option<String> {
    let sm = sourcemap::SourceMap::from_slice(map_json.as_bytes()).ok()?;
    let out = stack
        .lines()
        .map(|line| remap_line(line, &sm, module_id))
        .collect::<Vec<_>>()
        .join("\n");
    Some(out)
}

/// The original-TS `(line, col)` of the top stack frame that references the
/// guest module, if any — used to enrich the exception message.
pub fn top_frame_location(remapped_stack: &str, module_id: &str) -> Option<(u32, u32)> {
    remapped_stack.lines().find_map(|line| {
        let after = line.split_once(module_id).map(|(_, rest)| rest)?;
        parse_line_col(after).map(|(l, c, _)| (l, c))
    })
}

fn remap_line(line: &str, sm: &sourcemap::SourceMap, module_id: &str) -> String {
    let Some(pos) = line.find(module_id) else {
        return line.to_owned();
    };
    let head_end = pos + module_id.len();
    let after = &line[head_end..];
    let Some((l, c, consumed)) = parse_line_col(after) else {
        return line.to_owned();
    };
    // QuickJS positions are 1-based; source maps are 0-based.
    match sm.lookup_token(l.saturating_sub(1), c.saturating_sub(1)) {
        Some(tok) => format!(
            "{}:{}:{}{}",
            &line[..head_end],
            tok.get_src_line() + 1,
            tok.get_src_col() + 1,
            &after[consumed..]
        ),
        None => line.to_owned(),
    }
}

/// Parse a leading `:<line>:<col>` from `s`, returning `(line, col, bytes_consumed)`.
fn parse_line_col(s: &str) -> Option<(u32, u32, usize)> {
    let rest = s.strip_prefix(':')?;
    let (line_str, rest) = take_digits(rest);
    let line: u32 = line_str.parse().ok()?;
    let rest = rest.strip_prefix(':')?;
    let (col_str, _) = take_digits(rest);
    let col: u32 = col_str.parse().ok()?;
    let consumed = 1 + line_str.len() + 1 + col_str.len();
    Some((line, col, consumed))
}

fn take_digits(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    s.split_at(end)
}
