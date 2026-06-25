//! Owns the QuickJS runtime/context and the re-entrancy machinery.

use crate::bridge::BridgeState;
use rquickjs::{Context, Ctx, Runtime};
use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

/// Maximum nesting depth across the JS<->PHP boundary, guarding runaway mutual
/// recursion (which would otherwise overflow the native stack).
pub const MAX_DEPTH: usize = 200;

/// The QuickJS engine: runtime + context + the shared bridge state.
pub struct Engine {
    pub rt: Runtime,
    pub ctx: Context,
    pub state: Rc<BridgeState>,
    depth: Cell<usize>,
}

impl Engine {
    pub fn new() -> rquickjs::Result<Rc<Self>> {
        let rt = Runtime::new()?;
        let ctx = Context::full(&rt)?;
        let state = BridgeState::new();
        let engine = Rc::new(Engine {
            rt,
            ctx,
            state: state.clone(),
            depth: Cell::new(0),
        });
        // Close the cycle so the bridge can reach back into the engine when it
        // needs to invoke JS callbacks held by PHP.
        state.set_engine(Rc::downgrade(&engine));
        Ok(engine)
    }

    /// Enter one level of cross-boundary nesting; errors if the cap is hit.
    pub fn enter(&self) -> Result<DepthGuard<'_>, String> {
        let d = self.depth.get();
        if d >= MAX_DEPTH {
            return Err(format!(
                "maximum bridge re-entrancy depth ({MAX_DEPTH}) exceeded"
            ));
        }
        self.depth.set(d + 1);
        Ok(DepthGuard { depth: &self.depth })
    }
}

/// RAII guard that decrements the depth counter on drop.
pub struct DepthGuard<'a> {
    depth: &'a Cell<usize>,
}

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.depth.set(self.depth.get() - 1);
    }
}

// ---------------------------------------------------------------------------
// current-context stack
//
// While a host call runs, the live `Ctx` is valid but its `'js` lifetime
// cannot be named in PHP-facing code. We stash the raw pointer so a PHP-held JS
// callback can be invoked *synchronously* during a host call by reusing the
// already-locked context instead of re-locking the runtime (which would
// deadlock). Single-threaded (PHP NTS), so a thread-local stack is sufficient.
// ---------------------------------------------------------------------------

thread_local! {
    static CTX_STACK: std::cell::RefCell<Vec<NonNull<rquickjs::qjs::JSContext>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Push the current context for the duration of a host call.
pub fn push_ctx(ctx: &Ctx<'_>) {
    CTX_STACK.with(|s| s.borrow_mut().push(ctx.as_raw()));
}

/// Pop the current context when a host call returns.
pub fn pop_ctx() {
    CTX_STACK.with(|s| {
        s.borrow_mut().pop();
    });
}

/// The innermost active context, if a host call is currently on the stack.
pub fn current_ctx_ptr() -> Option<NonNull<rquickjs::qjs::JSContext>> {
    CTX_STACK.with(|s| s.borrow().last().copied())
}
