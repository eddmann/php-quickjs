//! The trust boundary: one `__host` import into JS, a flat dispatch table of
//! registered PHP callables, and the frozen `php.*` facade generated from the
//! manifest.
//!
//! The `__host(name, argsBytes)` ABI is byte-based: the guest encodes its
//! argument array to msgpack, the host decodes it, dispatches to the PHP
//! callable, and returns the msgpack-encoded result. Adding a capability never
//! changes this ABI.

use crate::manifest::ManifestEntry;
use crate::marshal::{middle_to_zval, zval_to_middle, MiddleValue};
use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::types::{ZendCallable, Zval};
use rquickjs::{Ctx, Exception, TypedArray, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The msgpack codec injected into every sandbox context.
const MSGPACK_JS: &str = include_str!("js/msgpack.js");

/// Shared host-side state behind the bridge. Single-threaded (PHP NTS), so
/// `Rc`/`RefCell` interior mutability is sufficient and correct.
#[derive(Default)]
pub struct BridgeState {
    /// Dotted name -> PHP callable (the trust boundary allowlist).
    dispatch: RefCell<HashMap<String, Zval>>,
    /// Ordered registration manifest (drives the facade and the `.d.ts`).
    manifest: RefCell<Vec<ManifestEntry>>,
}

impl BridgeState {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Register a PHP callable under a flat, dotted name.
    pub fn register(&self, name: String, callable: &Zval, types: Option<String>) -> Result<(), String> {
        if !callable.is_callable() {
            return Err(format!("value registered as '{name}' is not callable"));
        }
        self.dispatch
            .borrow_mut()
            .insert(name.clone(), callable.shallow_clone());
        let mut manifest = self.manifest.borrow_mut();
        // Replace an existing entry with the same name, else append.
        if let Some(existing) = manifest.iter_mut().find(|e| e.name == name) {
            existing.types = types;
        } else {
            manifest.push(ManifestEntry { name, types });
        }
        Ok(())
    }

    pub fn manifest_snapshot(&self) -> Vec<ManifestEntry> {
        self.manifest.borrow().clone()
    }

    fn names(&self) -> Vec<String> {
        self.manifest.borrow().iter().map(|e| e.name.clone()).collect()
    }
}

/// Dispatch a host call: resolve the name, marshal args to PHP, invoke, and
/// marshal the result back. Returns `Err(message)` for an unknown capability
/// or a failed call (refined into typed JS errors in a later step).
fn host_call(state: &BridgeState, name: &str, args: Vec<MiddleValue>) -> Result<MiddleValue, String> {
    let dispatch = state.dispatch.borrow();
    let callable_zv = dispatch
        .get(name)
        .ok_or_else(|| format!("unknown capability: {name}"))?;

    let zvals: Vec<Zval> = args
        .iter()
        .map(middle_to_zval)
        .collect::<Result<_, _>>()?;
    let params: Vec<&dyn IntoZvalDyn> = zvals.iter().map(|z| z as &dyn IntoZvalDyn).collect();

    let callable = ZendCallable::new(callable_zv).map_err(|e| e.to_string())?;
    let ret = callable.try_call(params).map_err(|e| e.to_string())?;
    zval_to_middle(&ret)
}

/// Install the bridge into a context: the `__host` native import, the msgpack
/// codec, and the frozen `php.*` facade. Call once per `eval`, before guest
/// code runs.
pub fn install<'js>(ctx: &Ctx<'js>, state: Rc<BridgeState>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // The single JS -> host entry point.
    let host_state = state.clone();
    let host = rquickjs::Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>, name: String, args_bytes: TypedArray<'js, u8>| -> rquickjs::Result<Value<'js>> {
            let bytes = args_bytes
                .as_bytes()
                .ok_or_else(|| Exception::throw_type(&ctx, "__host args must be a Uint8Array"))?;
            let decoded = MiddleValue::from_msgpack(bytes)
                .map_err(|e| Exception::throw_type(&ctx, &format!("invalid msgpack payload: {e}")))?;
            let args = match decoded {
                MiddleValue::Array(a) => a,
                other => vec![other],
            };
            match host_call(&host_state, &name, args) {
                Ok(result) => {
                    let out = result
                        .to_msgpack()
                        .map_err(|e| Exception::throw_type(&ctx, &format!("encode failed: {e}")))?;
                    Ok(TypedArray::new(ctx.clone(), out)?.into_value())
                }
                Err(msg) => Err(Exception::throw_message(&ctx, &msg)),
            }
        },
    )?;
    globals.set("__host", host)?;

    // The msgpack codec, then the frozen facade.
    ctx.eval::<(), _>(MSGPACK_JS)?;
    ctx.eval::<(), _>(build_facade(&state.names()))?;
    Ok(())
}

/// Generate the JS that builds the frozen `php.*` tree from the dotted names.
fn build_facade(names: &[String]) -> String {
    let mut src = String::from(
        "(function(){\n\
         var host = globalThis.__host, mp = globalThis.__mp;\n\
         function call(name, args){ return mp.decode(host(name, mp.encode(args))); }\n\
         var php = {};\n",
    );

    for name in names {
        let parts: Vec<&str> = name.split('.').collect();
        // Ensure each intermediate namespace object exists.
        let mut path = String::from("php");
        for part in &parts[..parts.len() - 1] {
            let next = format!("{path}[{}]", js_string(part));
            src.push_str(&format!("{next} = {next} || {{}};\n"));
            path = next;
        }
        let leaf = format!("{path}[{}]", js_string(parts[parts.len() - 1]));
        src.push_str(&format!(
            "{leaf} = function(){{ return call({}, Array.prototype.slice.call(arguments)); }};\n",
            js_string(name)
        ));
    }

    src.push_str(
        "(function deepFreeze(o){ Object.keys(o).forEach(function(k){ var v=o[k]; if(v && (typeof v==='object'||typeof v==='function')) deepFreeze(v); }); Object.freeze(o); })(php);\n\
         globalThis.php = php;\n\
         })();\n",
    );
    src
}

/// Produce a valid JS string literal for `s`.
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_builds_nested_paths() {
        let src = build_facade(&["db.query".into(), "log.info".into()]);
        assert!(src.contains("php[\"db\"] = php[\"db\"] || {};"));
        assert!(src.contains("php[\"db\"][\"query\"] = function()"));
        assert!(src.contains("call(\"db.query\""));
        assert!(src.contains("Object.freeze"));
    }
}
