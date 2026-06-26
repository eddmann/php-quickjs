//! `Js\Callback`: a PHP-facing wrapper around a JS function handed to PHP.
//!
//! The JS function itself never leaves the engine; the callback holds only an
//! integer id into the JS-side registry plus a handle back to the engine. When
//! invoked from PHP it re-enters JS — reusing the live context if a host call
//! is already in flight, else acquiring the runtime lock afresh.

use crate::engine::{current_ctx_ptr, Engine};
use crate::marshal::{middle_to_zval, zval_to_middle, MiddleValue};
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use rquickjs::{Ctx, Function, TypedArray, Value};
use std::rc::Rc;

#[php_class]
#[php(name = "Js\\Callback")]
pub struct JsCallback {
    pub id: u64,
    pub engine: Rc<Engine>,
}

impl JsCallback {
    pub fn new(id: u64, engine: Rc<Engine>) -> Self {
        JsCallback { id, engine }
    }

    /// Invoke the underlying JS function with the given (already PHP-side) args.
    fn invoke_inner(&self, args: &[&Zval]) -> PhpResult<Zval> {
        let _guard = self.engine.enter().map_err(PhpException::default)?;

        let mut middle_args = Vec::with_capacity(args.len());
        for a in args {
            middle_args.push(zval_to_middle(a, &self.engine.state).map_err(PhpException::default)?);
        }
        let payload = MiddleValue::Array(middle_args)
            .to_msgpack()
            .map_err(|e| PhpException::default(e.to_string()))?;
        let id = self.id;
        let engine = self.engine.clone();

        let run = move |ctx: &Ctx<'_>| -> PhpResult<Zval> {
            let globals = ctx.globals();
            let invoke: Function = globals
                .get("__invokeJs")
                .map_err(|e| PhpException::default(format!("__invokeJs missing: {e}")))?;
            let arg_bytes = TypedArray::new(ctx.clone(), payload.clone())
                .map_err(|e| PhpException::default(e.to_string()))?;
            // A JS error here re-surfaces a host exception (unwrapped to its
            // original PHP class) or becomes a QuickJSEvalException.
            let ret: Value = invoke
                .call((id as f64, arg_bytes))
                .map_err(|e| crate::error::js_error_to_php(ctx, e))?;
            let ta = TypedArray::<u8>::from_value(ret)
                .map_err(|e| PhpException::default(format!("JS callback did not return bytes: {e}")))?;
            let bytes = ta
                .as_bytes()
                .ok_or_else(|| PhpException::default("detached result buffer".to_owned()))?;
            let mv = MiddleValue::from_msgpack(bytes).map_err(|e| PhpException::default(e.to_string()))?;
            middle_to_zval(&mv, &engine.state).map_err(PhpException::default)
        };

        // Reuse the live context if we are nested inside a host call; otherwise
        // acquire the runtime lock. Reusing avoids a deadlock from re-locking.
        match current_ctx_ptr() {
            Some(ptr) => {
                let ctx = unsafe { Ctx::from_raw(ptr) };
                run(&ctx)
            }
            None => self.engine.ctx.with(|ctx| run(&ctx)),
        }
    }
}

#[php_impl]
impl JsCallback {
    /// Invoke the JS callback: `$cb(...$args)`.
    pub fn __invoke(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.invoke_inner(args)
    }

    /// Explicit form: `$cb->call([...$args])`.
    pub fn call(&self, args: &[&Zval]) -> PhpResult<Zval> {
        self.invoke_inner(args)
    }
}
