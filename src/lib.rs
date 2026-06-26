// The constructor exposes camelCase PHP named arguments (`memoryLimit`,
// `timeoutMs`, `maxStack`) to match the public API; ext-php-rs derives PHP arg
// names from the Rust idents, so the non-snake-case lint is allowed here.
#![allow(non_snake_case)]

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
mod transpile;

use engine::Engine;
use exceptions::{QuickJSMemoryException, QuickJSTimeoutException};
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
    /// - `memoryLimit`: max heap bytes (alloc-bomb guard)
    /// - `timeoutMs`: wall-clock budget per `eval` (infinite-loop guard)
    /// - `maxStack`: max native stack bytes
    /// - `isolated`: when true, each `eval()` runs in a fresh global realm (its
    ///   own world); cross-eval globals and persistent JS callbacks are not
    ///   kept. Defaults to false (one shared, persistent realm per instance).
    #[php(defaults(memoryLimit = None, timeoutMs = None, maxStack = None, isolated = false))]
    pub fn __construct(
        memoryLimit: Option<i64>,
        timeoutMs: Option<i64>,
        maxStack: Option<i64>,
        isolated: bool,
    ) -> PhpResult<Self> {
        let engine = Engine::new(
            memoryLimit.unwrap_or(0).max(0) as usize,
            timeoutMs.unwrap_or(0).max(0) as u64,
            maxStack.unwrap_or(0).max(0) as usize,
            isolated,
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
        // TypeScript fast path: transpile to JS (types erased, esnext) before
        // QuickJS ever sees the source. Transpile/syntax errors surface here,
        // located at their original TS line/column.
        let module = self.engine.transpile.get_or_transpile(&code).map_err(|e| {
            let stack = if e.line > 0 {
                format!("    at guest.ts:{}:{}", e.line, e.col)
            } else {
                String::new()
            };
            exceptions::eval_exception(
                "SyntaxError".to_owned(),
                e.message,
                "guest.ts",
                e.line,
                stack,
            )
        })?;

        let state = self.engine.state.clone();
        self.engine.arm_deadline();
        let outcome = self.engine.eval_in(|ctx| {
            let map = module.map_json.clone();
            let eval_err = |e| self.classify_js_error(ctx, e, map.as_deref(), &module.module_id);
            bridge::install(ctx, state.clone()).map_err(&eval_err)?;
            // Name the module so stack frames are attributable and remappable.
            let mut opts = rquickjs::context::EvalOptions::default();
            opts.filename = Some(module.module_id.clone());
            let value: rquickjs::Value = ctx
                .eval_with_options(module.js.as_bytes(), opts)
                .map_err(&eval_err)?;
            let middle = js_to_middle(ctx, value, &state).map_err(&eval_err)?;
            middle_to_zval(&middle, &state).map_err(PhpException::default)
        });
        self.engine.disarm_deadline();
        match outcome {
            Ok(r) => r,
            Err(e) => Err(to_php_err(e)),
        }
    }

    /// Return the registration manifest as an array of `['name'=>..., 'types'=>...]`.
    pub fn manifest(&self) -> PhpResult<Zval> {
        let state = &self.engine.state;
        let entries = state.manifest_snapshot();
        let mv = marshal::MiddleValue::Array(
            entries
                .iter()
                .map(|e| {
                    marshal::MiddleValue::Map(vec![
                        ("name".to_owned(), marshal::MiddleValue::Str(e.name.clone())),
                        (
                            "types".to_owned(),
                            e.types
                                .clone()
                                .map_or(marshal::MiddleValue::Null, marshal::MiddleValue::Str),
                        ),
                    ])
                })
                .collect(),
        );
        marshal::middle_to_zval(&mv, state).map_err(PhpException::default)
    }

    /// Generate a TypeScript `.d.ts` declaration for the `php` global from the
    /// current manifest. Guests can author against these types; they are erased
    /// at runtime (the host still validates at the boundary).
    pub fn dts(&self) -> String {
        manifest::to_dts(&self.engine.state.manifest_snapshot())
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
        let outcome = self.engine.eval_in(|ctx| {
            // Runtime support must exist for any function reconstruction.
            bridge::install(ctx, state.clone())
                .map_err(|e| PhpException::default(error::js_error_message(ctx, e)))?;
            let js = middle_to_js(ctx, &middle, &state).map_err(to_php_err)?;
            let back = js_to_middle(ctx, js, &state).map_err(to_php_err)?;
            middle_to_zval(&back, &state).map_err(PhpException::default)
        });
        match outcome {
            Ok(r) => r,
            Err(e) => Err(to_php_err(e)),
        }
    }
}

impl QuickJS {
    /// Map a JS-side failure to the most specific PHP exception class: timeout
    /// (deadline tripped), memory (heap limit), else a generic eval error. For
    /// the eval case the JS stack is remapped to TypeScript coordinates via the
    /// module's source map and surfaced in the message.
    fn classify_js_error(
        &self,
        ctx: &rquickjs::Ctx<'_>,
        err: rquickjs::Error,
        map_json: Option<&str>,
        module_id: &str,
    ) -> PhpException {
        let parts = error::js_error_parts(ctx, err);
        let message = parts.display_message();
        if self.engine.timed_out() {
            return PhpException::from_class::<QuickJSTimeoutException>(message);
        }
        if message.to_lowercase().contains("out of memory") {
            return PhpException::from_class::<QuickJSMemoryException>(message);
        }
        // Remap the guest stack to TypeScript coordinates (guest frames only),
        // and surface it as a structured, JS-error-like exception.
        let remapped = parts
            .stack
            .as_deref()
            .zip(map_json)
            .and_then(|(stack, map)| error::remap_stack(stack, map, module_id));
        let (line, _) = remapped
            .as_deref()
            .and_then(|s| error::top_frame_location(s, module_id))
            .unwrap_or((0, 0));
        exceptions::eval_exception(
            parts.name,
            message,
            module_id,
            line,
            remapped.unwrap_or_default(),
        )
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
