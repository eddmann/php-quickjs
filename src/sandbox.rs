//! In-engine resource containment: memory limit, native stack size, and a
//! wall-clock deadline enforced through QuickJS's interrupt handler.
//!
//! These are *resource* guards (infinite loops, alloc bombs), not a defence
//! against QuickJS memory-corruption bugs — for hostile code, nest the whole
//! extension inside an outer microVM/gVisor boundary.

use rquickjs::Runtime;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

/// Apply the static limits. A value of `0` means "leave unbounded" (QuickJS
/// treats a literal 0 limit as zero bytes, so we simply skip the call).
pub fn apply_limits(rt: &Runtime, memory_limit: usize, max_stack: usize) {
    if memory_limit > 0 {
        rt.set_memory_limit(memory_limit);
    }
    if max_stack > 0 {
        rt.set_max_stack_size(max_stack);
    }
}

/// Install the wall-clock interrupt handler. It fires between QuickJS
/// operations; returning `true` aborts execution. When it trips on the
/// deadline it records that fact so the eval boundary can raise a timeout
/// (rather than a generic) exception.
pub fn install_interrupt(
    rt: &Runtime,
    deadline: Rc<Cell<Option<Instant>>>,
    timed_out: Rc<Cell<bool>>,
) {
    rt.set_interrupt_handler(Some(Box::new(move || match deadline.get() {
        Some(dl) if Instant::now() >= dl => {
            timed_out.set(true);
            true
        }
        _ => false,
    })));
}
