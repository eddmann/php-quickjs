use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use rquickjs::{Context, Runtime};

mod marshal;

use marshal::{js_to_middle, middle_to_js, middle_to_zval, zval_to_middle};

/// An embedded QuickJS sandbox.
#[php_class]
pub struct QuickJS {
    _rt: Runtime,
    ctx: Context,
}

#[php_impl]
impl QuickJS {
    pub fn __construct() -> PhpResult<Self> {
        let rt = Runtime::new().map_err(to_php_err)?;
        let ctx = Context::full(&rt).map_err(to_php_err)?;
        Ok(QuickJS { _rt: rt, ctx })
    }

    /// Evaluate JS source and marshal the result back to a PHP value.
    pub fn eval(&self, code: String) -> PhpResult<Zval> {
        self.ctx.with(|ctx| {
            let value: rquickjs::Value = ctx.eval(code).map_err(to_php_err)?;
            let middle = js_to_middle(&ctx, value).map_err(to_php_err)?;
            middle_to_zval(&middle).map_err(PhpException::default)
        })
    }

    /// Round-trip a PHP value through the JS engine and back, exercising every
    /// leg of the marshaling pipeline (PHP -> middle -> JS -> middle -> PHP).
    pub fn roundtrip(&self, value: &Zval) -> PhpResult<Zval> {
        let middle = zval_to_middle(value).map_err(PhpException::default)?;
        self.ctx.with(|ctx| {
            let js = middle_to_js(&ctx, &middle).map_err(to_php_err)?;
            let back = js_to_middle(&ctx, js).map_err(to_php_err)?;
            middle_to_zval(&back).map_err(PhpException::default)
        })
    }
}

fn to_php_err<E: std::fmt::Display>(e: E) -> PhpException {
    PhpException::default(e.to_string())
}

#[php_module]
pub fn module(module: ModuleBuilder) -> ModuleBuilder {
    module.class::<QuickJS>()
}
