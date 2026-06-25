//! Capability handles for live, stateful PHP objects.
//!
//! A live PHP object (PDO connection, file handle, ...) must never be
//! serialized into JS. Instead it is kept host-side in this table and JS is
//! handed an opaque integer. The handle *is* the capability: JS can do nothing
//! with it but pass it back to a host function willing to honour it.

use ext_php_rs::types::Zval;
use std::cell::Cell;
use std::collections::HashMap;
use std::cell::RefCell;

#[derive(Default)]
pub struct HandleTable {
    next: Cell<i64>,
    map: RefCell<HashMap<i64, Zval>>,
}

impl HandleTable {
    /// Store a live value, returning its opaque handle id. The value's refcount
    /// is bumped (via `shallow_clone`) so PHP will not free it while granted.
    pub fn grant(&self, value: &Zval) -> i64 {
        let id = self.next.get() + 1;
        self.next.set(id);
        self.map.borrow_mut().insert(id, value.shallow_clone());
        id
    }

    /// Resolve a handle back to the live value (refcount-bumped clone).
    pub fn resolve(&self, id: i64) -> Option<Zval> {
        self.map.borrow().get(&id).map(Zval::shallow_clone)
    }

    /// Drop a handle, releasing the host-side reference. Returns whether it
    /// existed.
    pub fn revoke(&self, id: i64) -> bool {
        self.map.borrow_mut().remove(&id).is_some()
    }

    pub fn len(&self) -> usize {
        self.map.borrow().len()
    }
}
