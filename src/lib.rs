use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use std::rc::Rc;

mod bridge;
mod callback;
mod engine;
mod error;
mod exceptions;
mod handles;
mod manifest;
mod marshal;

use engine::Engine;
use exceptions::QuickJSEvalException;
use marshal::{js_to_middle, middle_to_js, middle_to_zval, zval_to_middle};

/// An embedded QuickJS sandbox with a typed, bidirectional PHP bridge.
#[php_class]
#[php(name = "QuickJS")]
pub struct QuickJS {
    engine: Rc<Engine>,
}

#[php_impl]
impl QuickJS {
    pub fn __construct() -> PhpResult<Self> {
        let engine = Engine::new().map_err(to_php_err)?;
        Ok(QuickJS { engine })
    }

    /// Register a PHP callable under a flat, dotted capability name, optionally
    /// with a TypeScript signature used for `.d.ts` generation. The name
    /// becomes callable from JS as `php.<dotted.name>(...)`.
    pub fn register(
        &mut self,
        name: String,
        callable: &Zval,
        types: Option<String>,
    ) -> PhpResult<()> {
        self.engine
            .state
            .register(name, callable, types)
            .map_err(PhpException::default)
    }

    /// Evaluate JS source and marshal the result back to a PHP value. The
    /// `php.*` facade is installed fresh from the current manifest first.
    pub fn eval(&self, code: String) -> PhpResult<Zval> {
        let state = self.engine.state.clone();
        self.engine.ctx.with(|ctx| {
            let eval_err = |e| {
                PhpException::from_class::<QuickJSEvalException>(error::js_error_message(&ctx, e))
            };
            bridge::install(&ctx, state.clone()).map_err(eval_err)?;
            let value: rquickjs::Value = ctx.eval(code).map_err(eval_err)?;
            let middle = js_to_middle(&ctx, value, &state).map_err(eval_err)?;
            middle_to_zval(&middle, &state).map_err(PhpException::default)
        })
    }

    /// Grant JS access to a live PHP value (e.g. a database connection) as an
    /// opaque integer handle. The value is kept host-side; JS only ever sees
    /// the id and must pass it back to a capability to use it.
    pub fn grant(&self, resource: &Zval) -> i64 {
        self.engine.state.handles.grant(resource)
    }

    /// Resolve a handle previously returned by `grant()` back to its live PHP
    /// value. Throws if the handle is unknown (e.g. already revoked).
    pub fn resolve(&self, handle: i64) -> PhpResult<Zval> {
        self.engine
            .state
            .handles
            .resolve(handle)
            .ok_or_else(|| PhpException::default(format!("unknown handle: {handle}")))
    }

    /// Revoke a handle, releasing the host-side reference. Returns whether the
    /// handle existed.
    pub fn revoke(&self, handle: i64) -> bool {
        self.engine.state.handles.revoke(handle)
    }

    /// Round-trip a PHP value through the JS engine and back, exercising every
    /// leg of the marshaling pipeline (PHP -> middle -> JS -> middle -> PHP).
    pub fn roundtrip(&self, value: &Zval) -> PhpResult<Zval> {
        let state = self.engine.state.clone();
        let middle = zval_to_middle(value, &state).map_err(PhpException::default)?;
        self.engine.ctx.with(|ctx| {
            // Runtime support must exist for any function reconstruction.
            bridge::install(&ctx, state.clone())
                .map_err(|e| PhpException::default(error::js_error_message(&ctx, e)))?;
            let js = middle_to_js(&ctx, &middle, &state).map_err(to_php_err)?;
            let back = js_to_middle(&ctx, js, &state).map_err(to_php_err)?;
            middle_to_zval(&back, &state).map_err(PhpException::default)
        })
    }
}

fn to_php_err<E: std::fmt::Display>(e: E) -> PhpException {
    PhpException::default(e.to_string())
}

#[php_module]
pub fn module(module: ModuleBuilder) -> ModuleBuilder {
    module
        // Exceptions first so subclasses can resolve their parent class entry.
        .class::<exceptions::QuickJSException>()
        .class::<exceptions::QuickJSEvalException>()
        .class::<exceptions::QuickJSTimeoutException>()
        .class::<exceptions::QuickJSMemoryException>()
        .class::<QuickJS>()
        .class::<callback::JsCallback>()
}
