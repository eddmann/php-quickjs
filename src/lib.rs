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
mod sandbox;

use engine::Engine;
use exceptions::{QuickJSEvalException, QuickJSMemoryException, QuickJSTimeoutException};
use marshal::{js_to_middle, middle_to_js, middle_to_zval, zval_to_middle};

/// An embedded QuickJS sandbox with a typed, bidirectional PHP bridge.
#[php_class]
#[php(name = "QuickJS")]
pub struct QuickJS {
    engine: Rc<Engine>,
}

#[php_impl]
impl QuickJS {
    /// Construct a sandbox. All limits default to unbounded; pass non-zero
    /// values to contain resource abuse:
    /// - `memory_limit`: max heap bytes (alloc-bomb guard)
    /// - `timeout_ms`: wall-clock budget per `eval` (infinite-loop guard)
    /// - `max_stack`: max native stack bytes
    #[allow(non_snake_case)]
    #[php(defaults(memoryLimit = None, timeoutMs = None, maxStack = None))]
    pub fn __construct(
        memoryLimit: Option<i64>,
        timeoutMs: Option<i64>,
        maxStack: Option<i64>,
    ) -> PhpResult<Self> {
        let engine = Engine::new(
            memoryLimit.unwrap_or(0).max(0) as usize,
            timeoutMs.unwrap_or(0).max(0) as u64,
            maxStack.unwrap_or(0).max(0) as usize,
        )
        .map_err(to_php_err)?;
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
        self.engine.arm_deadline();
        let result = self.engine.ctx.with(|ctx| {
            let eval_err = |e| self.classify_js_error(&ctx, e);
            bridge::install(&ctx, state.clone()).map_err(&eval_err)?;
            let value: rquickjs::Value = ctx.eval(code).map_err(&eval_err)?;
            let middle = js_to_middle(&ctx, value, &state).map_err(&eval_err)?;
            middle_to_zval(&middle, &state).map_err(PhpException::default)
        });
        self.engine.disarm_deadline();
        result
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

impl QuickJS {
    /// Map a JS-side failure to the most specific PHP exception class: timeout
    /// (deadline tripped), memory (heap limit), else a generic eval error.
    fn classify_js_error(&self, ctx: &rquickjs::Ctx<'_>, err: rquickjs::Error) -> PhpException {
        let msg = error::js_error_message(ctx, err);
        if self.engine.timed_out() {
            PhpException::from_class::<QuickJSTimeoutException>(msg)
        } else if msg.to_lowercase().contains("out of memory") {
            PhpException::from_class::<QuickJSMemoryException>(msg)
        } else {
            PhpException::from_class::<QuickJSEvalException>(msg)
        }
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
