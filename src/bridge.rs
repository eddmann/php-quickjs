//! The trust boundary: one `__host` import into JS, a flat dispatch table of
//! registered PHP callables, and the frozen `php.*` facade generated from the
//! manifest.
//!
//! The `__host(name, argsBytes)` ABI is byte-based: the guest encodes its
//! argument array to msgpack, the host decodes it, dispatches to the PHP
//! callable, and returns the msgpack-encoded result. Adding a capability never
//! changes this ABI.

use crate::engine::{push_ctx, Engine};
use crate::error::{throw_host_error, HostError};
use crate::handles::HandleTable;
use crate::manifest::ManifestEntry;
use crate::marshal::{middle_to_zval, zval_to_middle, MiddleValue};
use ext_php_rs::convert::IntoZvalDyn;
use ext_php_rs::types::{ZendCallable, Zval};
use rquickjs::{Ctx, Exception, Function, TypedArray, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

/// The msgpack codec and runtime support injected into every sandbox context.
const MSGPACK_JS: &str = include_str!("js/msgpack.js");
const RUNTIME_JS: &str = include_str!("js/runtime.js");

/// Shared host-side state behind the bridge. Single-threaded (PHP NTS), so
/// `Rc`/`RefCell` interior mutability is sufficient and correct.
#[derive(Default)]
pub struct BridgeState {
    /// Dotted name -> PHP callable (the trust boundary allowlist).
    dispatch: RefCell<HashMap<String, Zval>>,
    /// Ordered registration manifest (drives the facade and the `.d.ts`).
    manifest: RefCell<Vec<ManifestEntry>>,
    /// Anonymous PHP callables handed to JS, keyed by id.
    php_funcs: RefCell<HashMap<u64, Zval>>,
    next_php: Cell<u64>,
    /// Live PHP objects granted to JS as opaque handles.
    pub handles: HandleTable,
    /// Back-reference to the owning engine (for invoking JS callbacks).
    engine: RefCell<Weak<Engine>>,
    /// JS-callback ids whose PHP wrapper was dropped, awaiting release from the
    /// JS registry (deferred to the next eval boundary; see `JsCallback::drop`).
    pending_fn_deletions: RefCell<Vec<u64>>,
}

impl BridgeState {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Queue a JS-callback id for release at the next eval boundary.
    pub fn queue_fn_deletion(&self, id: u64) {
        self.pending_fn_deletions.borrow_mut().push(id);
    }

    fn take_pending_deletions(&self) -> Vec<u64> {
        std::mem::take(&mut self.pending_fn_deletions.borrow_mut())
    }

    pub fn set_engine(&self, engine: Weak<Engine>) {
        *self.engine.borrow_mut() = engine;
    }

    pub fn engine(&self) -> Option<Rc<Engine>> {
        self.engine.borrow().upgrade()
    }

    /// Register a PHP callable under a flat, dotted name.
    pub fn register(
        &self,
        name: String,
        callable: &Zval,
        types: Option<String>,
    ) -> Result<(), String> {
        if !callable.is_callable() {
            return Err(format!("value registered as '{name}' is not callable"));
        }
        self.dispatch
            .borrow_mut()
            .insert(name.clone(), callable.shallow_clone());
        let mut manifest = self.manifest.borrow_mut();
        if let Some(existing) = manifest.iter_mut().find(|e| e.name == name) {
            existing.types = types;
        } else {
            manifest.push(ManifestEntry { name, types });
        }
        Ok(())
    }

    /// Register an anonymous PHP callable (handed to JS), returning its id.
    pub fn register_php_fn(&self, callable: &Zval) -> u64 {
        let id = self.next_php.get() + 1;
        self.next_php.set(id);
        self.php_funcs
            .borrow_mut()
            .insert(id, callable.shallow_clone());
        id
    }

    pub fn get_php_fn(&self, id: u64) -> Option<Zval> {
        self.php_funcs.borrow().get(&id).map(Zval::shallow_clone)
    }

    pub fn manifest_snapshot(&self) -> Vec<ManifestEntry> {
        self.manifest.borrow().clone()
    }

    fn names(&self) -> Vec<String> {
        self.manifest
            .borrow()
            .iter()
            .map(|e| e.name.clone())
            .collect()
    }
}

/// Invoke a PHP callable with already-marshaled args, returning its result.
fn call_php(
    callable_zv: &Zval,
    args: &[MiddleValue],
    state: &BridgeState,
) -> Result<MiddleValue, HostError> {
    let zvals: Vec<Zval> = args
        .iter()
        .map(|m| middle_to_zval(m, state))
        .collect::<Result<_, _>>()
        .map_err(HostError::internal)?;
    let params: Vec<&dyn IntoZvalDyn> = zvals.iter().map(|z| z as &dyn IntoZvalDyn).collect();

    let callable =
        ZendCallable::new(callable_zv).map_err(|e| HostError::internal(e.to_string()))?;
    let ret = callable
        .try_call(params)
        .map_err(crate::error::php_exception_info)?;
    zval_to_middle(&ret, state).map_err(HostError::internal)
}

/// Dispatch a named host call. Returns `Err` for an unknown capability (the
/// trust-boundary rejection) or a failed call.
fn host_call(
    state: &BridgeState,
    name: &str,
    args: Vec<MiddleValue>,
) -> Result<MiddleValue, HostError> {
    let callable_zv = state
        .dispatch
        .borrow()
        .get(name)
        .map(Zval::shallow_clone)
        .ok_or_else(|| HostError::internal(format!("unknown capability: {name}")))?;
    call_php(&callable_zv, &args, state)
}

/// Invoke an anonymous PHP callable (one previously handed to JS) by id.
fn php_fn_call(
    state: &BridgeState,
    id: u64,
    args: Vec<MiddleValue>,
) -> Result<MiddleValue, HostError> {
    let callable_zv = state
        .get_php_fn(id)
        .ok_or_else(|| HostError::internal(format!("unknown PHP callable id {id}")))?;
    call_php(&callable_zv, &args, state)
}

/// Decode the msgpack arg payload from a host import into a list of values.
fn decode_args(bytes: &[u8]) -> Result<Vec<MiddleValue>, String> {
    match MiddleValue::from_msgpack(bytes).map_err(|e| e.to_string())? {
        MiddleValue::Array(a) => Ok(a),
        other => Ok(vec![other]),
    }
}

/// Encode a host result back to a msgpack `Uint8Array` for JS.
fn encode_result<'js>(ctx: &Ctx<'js>, result: MiddleValue) -> rquickjs::Result<Value<'js>> {
    let out = result
        .to_msgpack()
        .map_err(|e| Exception::throw_type(ctx, &format!("encode failed: {e}")))?;
    Ok(TypedArray::new(ctx.clone(), out)?.into_value())
}

/// Install the bridge into a context: the `__host`/`__php_invoke` native
/// imports, the msgpack codec, the runtime support, and the frozen `php.*`
/// facade. Call once per `eval`, before guest code runs.
pub fn install<'js>(ctx: &Ctx<'js>, state: Rc<BridgeState>) -> rquickjs::Result<()> {
    let globals = ctx.globals();

    // The single JS -> host capability entry point.
    let host_state = state.clone();
    let host = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              name: String,
              args_bytes: TypedArray<'js, u8>|
              -> rquickjs::Result<Value<'js>> {
            let bytes = args_bytes
                .as_bytes()
                .ok_or_else(|| Exception::throw_type(&ctx, "__host args must be a Uint8Array"))?;
            let args = decode_args(bytes).map_err(|e| Exception::throw_type(&ctx, &e))?;
            let result = {
                let _guard = push_ctx(&ctx);
                host_call(&host_state, &name, args)
            };
            match result {
                Ok(r) => encode_result(&ctx, r),
                Err(err) => Err(throw_host_error(&ctx, &err)),
            }
        },
    )?;
    globals.set("__host", host)?;

    // JS -> host: invoke an anonymous PHP callable handed to JS earlier.
    let php_state = state.clone();
    let php_invoke = Function::new(
        ctx.clone(),
        move |ctx: Ctx<'js>,
              id: f64,
              args_bytes: TypedArray<'js, u8>|
              -> rquickjs::Result<Value<'js>> {
            let bytes = args_bytes.as_bytes().ok_or_else(|| {
                Exception::throw_type(&ctx, "__php_invoke args must be a Uint8Array")
            })?;
            let args = decode_args(bytes).map_err(|e| Exception::throw_type(&ctx, &e))?;
            let result = {
                let _guard = push_ctx(&ctx);
                php_fn_call(&php_state, id as u64, args)
            };
            match result {
                Ok(r) => encode_result(&ctx, r),
                Err(err) => Err(throw_host_error(&ctx, &err)),
            }
        },
    )?;
    globals.set("__php_invoke", php_invoke)?;

    // Codec, runtime support, then the frozen facade.
    ctx.eval::<(), _>(MSGPACK_JS)?;
    ctx.eval::<(), _>(RUNTIME_JS)?;
    ctx.eval::<(), _>(build_facade(&state.names()))?;

    // Release JS callbacks whose PHP wrappers were dropped since the last eval.
    let stale = state.take_pending_deletions();
    if !stale.is_empty() {
        if let Ok(del) = globals.get::<_, Function>("__deleteJsFn") {
            for id in stale {
                let _ = del.call::<_, ()>((id as f64,));
            }
        }
    }
    Ok(())
}

/// Generate the JS that builds the frozen `php.*` tree from the dotted names.
fn build_facade(names: &[String]) -> String {
    let mut src = String::from(
        "(function(){\n\
         var php = {};\n",
    );

    for name in names {
        let parts: Vec<&str> = name.split('.').collect();
        let mut path = String::from("php");
        for part in &parts[..parts.len() - 1] {
            let next = format!("{path}[{}]", js_string(part));
            src.push_str(&format!("{next} = {next} || {{}};\n"));
            path = next;
        }
        let leaf = format!("{path}[{}]", js_string(parts[parts.len() - 1]));
        src.push_str(&format!(
            "{leaf} = function(){{ return globalThis.__rt.callHost({}, Array.prototype.slice.call(arguments)); }};\n",
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
        assert!(src.contains("callHost(\"db.query\""));
        assert!(src.contains("Object.freeze"));
    }
}
