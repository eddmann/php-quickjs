//! Error bridging in both directions.
//!
//! - **PHP throws inside a callback** -> a JS `Error` whose `message` is the PHP
//!   message and whose `name`/`phpClass` carry the PHP exception class, so the
//!   guest can `catch (e) { if (e.phpClass === 'HttpError') ... }`.
//! - **JS throws past `eval`** -> a typed PHP exception (`QuickJSEvalException`
//!   et al.) carrying the JS error's name, message and stack.

use ext_php_rs::error::Error as PhpError;
use ext_php_rs::exception::PhpException;
use ext_php_rs::zend::ClassEntry;
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

/// The decomposed parts of a thrown JS error.
pub struct JsErrorParts {
    /// The error constructor name (`TypeError`, `Error`, or a PHP class for a
    /// re-surfaced host exception).
    pub name: String,
    /// The error message, without the name prefix.
    pub message: String,
    /// The JS stack (generated-JS coordinates), if any.
    pub stack: Option<String>,
    /// The originating PHP exception class, if this error re-surfaced a host
    /// exception (set by `throw_host_error`).
    pub php_class: Option<String>,
}

impl JsErrorParts {
    /// `Name: message`, eliding a bare `Error` name to avoid a redundant prefix.
    pub fn display_message(&self) -> String {
        match (self.name.as_str(), self.message.is_empty()) {
            (_, true) => self.name.clone(),
            ("Error", false) => self.message.clone(),
            (_, false) => format!("{}: {}", self.name, self.message),
        }
    }
}

/// Render an rquickjs error into a human-readable `Name: message` string.
pub fn js_error_message(ctx: &Ctx<'_>, err: JsError) -> String {
    js_error_parts(ctx, err).display_message()
}

/// Decompose a thrown JS error into name / message / stack / php_class.
pub fn js_error_parts(ctx: &Ctx<'_>, err: JsError) -> JsErrorParts {
    match err {
        JsError::Exception => {
            let val = ctx.catch();
            if let Some(ex) = val.as_exception() {
                let name = ex
                    .get::<_, Option<String>>("name")
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "Error".to_owned());
                let php_class = ex.get::<_, Option<String>>("phpClass").ok().flatten();
                JsErrorParts {
                    name,
                    message: ex.message().unwrap_or_default(),
                    stack: ex.stack(),
                    php_class,
                }
            } else {
                // A thrown non-Error value: a string as-is, anything else (an
                // object, number, ...) rendered via JSON so it is not lost.
                let message = val
                    .as_string()
                    .and_then(|s| s.to_string().ok())
                    .or_else(|| {
                        ctx.json_stringify(val.clone())
                            .ok()
                            .flatten()
                            .and_then(|s| s.to_string().ok())
                    })
                    .unwrap_or_else(|| "uncaught JS exception".to_owned());
                JsErrorParts {
                    name: "Error".to_owned(),
                    message,
                    stack: None,
                    php_class: None,
                }
            }
        }
        other => JsErrorParts {
            name: "Error".to_owned(),
            message: other.to_string(),
            stack: None,
            php_class: None,
        },
    }
}

/// Convert a JS error caught while invoking a PHP-held JS callback into a PHP
/// exception. If the JS error re-surfaced a host exception (carries
/// `phpClass`), the original PHP class is restored with a clean message;
/// otherwise it becomes a `QuickJSEvalException`. This unwraps the
/// JS->PHP->JS->PHP round trip instead of nesting `Exception:` prefixes.
pub fn js_error_to_php(ctx: &Ctx<'_>, err: JsError) -> PhpException {
    let parts = js_error_parts(ctx, err);
    if let Some(class) = parts.php_class.as_deref() {
        if let Some(ce) = ClassEntry::try_find(class) {
            return PhpException::new(parts.message, 0, ce);
        }
    }
    PhpException::from_class::<crate::exceptions::QuickJSEvalException>(parts.display_message())
}

/// Remap a JS stack back to TypeScript coordinates, keeping only guest frames.
///
/// Frames that reference `module_id` have their `:line:col` rewritten to the
/// original TS position; internal host frames (the msgpack/runtime bootstrap
/// and the `php.*` facade wrappers) are dropped so the trace reads like a plain
/// TS stack. Returns `None` if the map cannot be parsed or no guest frame
/// remains.
pub fn remap_stack(stack: &str, map_json: &str, module_id: &str) -> Option<String> {
    let sm = sourcemap::SourceMap::from_slice(map_json.as_bytes()).ok()?;
    let out: Vec<String> = stack
        .lines()
        .filter(|line| line.contains(module_id))
        .map(|line| remap_line(line, &sm, module_id))
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out.join("\n"))
    }
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
