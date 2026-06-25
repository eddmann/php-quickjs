use ext_php_rs::prelude::*;
use rquickjs::{Context, Runtime};

/// Minimal skeleton proving the ext-php-rs <-> rquickjs integration.
/// This will be fleshed out per the implementation plan.
#[php_class]
pub struct QuickJS {
    _rt: Runtime,
    ctx: Context,
}

#[php_impl]
impl QuickJS {
    pub fn __construct() -> PhpResult<Self> {
        let rt = Runtime::new().map_err(|e| PhpException::default(e.to_string()))?;
        let ctx = Context::full(&rt).map_err(|e| PhpException::default(e.to_string()))?;
        Ok(QuickJS { _rt: rt, ctx })
    }

    /// Evaluate JS source and return the result as a 64-bit integer (skeleton).
    pub fn eval(&self, code: String) -> PhpResult<i64> {
        let result: i64 = self
            .ctx
            .with(|ctx| ctx.eval::<i64, _>(code))
            .map_err(|e| PhpException::default(e.to_string()))?;
        Ok(result)
    }
}

#[php_module]
pub fn module(module: ModuleBuilder) -> ModuleBuilder {
    module.class::<QuickJS>()
}
