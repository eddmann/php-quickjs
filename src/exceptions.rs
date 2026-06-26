//! Typed PHP exception classes thrown across the bridge.
//!
//! ```text
//! \Exception
//!   └─ QuickJSException                (base for everything this ext throws)
//!        ├─ QuickJSEvalException        (a JS error escaped eval)
//!        ├─ QuickJSTimeoutException     (the wall-clock deadline tripped)
//!        └─ QuickJSMemoryException      (the memory limit tripped)
//! ```

use ext_php_rs::convert::IntoZval;
use ext_php_rs::ffi::{zend_class_entry, zend_object, zval};
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendClassObject, Zval};
use ext_php_rs::zend::ce;

extern "C" {
    // Writes a property honouring `scope`'s visibility — required to set the
    // protected `message`/`file`/`line` slots that `\Exception` reads (the safe
    // `ZendObject::set_property` passes a null scope and so can only create a
    // dynamic property, which `getMessage()` does not see).
    fn zend_update_property(
        scope: *mut zend_class_entry,
        object: *mut zend_object,
        name: *const std::os::raw::c_char,
        name_length: usize,
        value: *mut zval,
    );
}

#[php_class]
#[php(extends(ce = ce::exception, stub = "\\Exception"))]
#[derive(Default)]
#[php(name = "QuickJSException")]
pub struct QuickJSException;

#[php_impl]
impl QuickJSException {}

/// A JS/TS error that escaped `eval`. Beyond the inherited `getMessage()` /
/// `getFile()` / `getLine()` (set to the original TS location), it exposes the
/// JS error name and the remapped, guest-only stack trace.
#[php_class]
#[php(extends(ce = quickjs_exception_ce, stub = "\\QuickJSException"))]
#[derive(Default)]
#[php(name = "QuickJSEvalException")]
pub struct QuickJSEvalException {
    js_name: String,
    js_stack: String,
}

#[php_impl]
impl QuickJSEvalException {
    /// The JS error constructor name, e.g. `"TypeError"` (or the originating
    /// PHP exception class for a re-surfaced host error).
    #[php(name = "getJsName")]
    pub fn get_js_name(&self) -> String {
        self.js_name.clone()
    }

    /// The stack trace, remapped to TypeScript coordinates and filtered to
    /// guest frames (host bridge frames removed).
    #[php(name = "getJsStack")]
    pub fn get_js_stack(&self) -> String {
        self.js_stack.clone()
    }
}

/// Build a fully-populated `QuickJSEvalException`: a clean `message`, the TS
/// `file`/`line`, and the structured JS name + remapped stack. Falls back to a
/// plain typed exception if object construction fails.
pub fn eval_exception(
    js_name: String,
    message: String,
    file: &str,
    line: u32,
    js_stack: String,
) -> PhpException {
    let mut obj = ZendClassObject::new(QuickJSEvalException { js_name, js_stack });
    {
        // Set the inherited \Exception properties with the Exception class as
        // scope so the protected slots `getMessage()`/`getFile()`/`getLine()`
        // read are actually written.
        let scope = std::ptr::from_ref(ce::exception()).cast_mut();
        let zo: *mut zend_object = obj.get_mut_zend_obj();
        set_prop_str(scope, zo, c"message", &message);
        set_prop_str(scope, zo, c"file", file);
        let mut line_zv = Zval::new();
        line_zv.set_long(i64::from(line));
        unsafe { zend_update_property(scope, zo, c"line".as_ptr(), 4, &mut line_zv) };
    }
    match obj.into_zval(false) {
        Ok(zval) => {
            let mut ex = PhpException::default(message);
            ex.set_object(Some(zval));
            ex
        }
        Err(_) => PhpException::from_class::<QuickJSEvalException>(message),
    }
}

/// Write a string property `name` on `obj` using `scope` for visibility.
fn set_prop_str(scope: *mut zend_class_entry, obj: *mut zend_object, name: &std::ffi::CStr, value: &str) {
    let mut zv = Zval::new();
    if zv.set_string(value, false).is_ok() {
        unsafe {
            zend_update_property(scope, obj, name.as_ptr(), name.to_bytes().len(), &mut zv);
        }
    }
}

#[php_class]
#[php(extends(ce = quickjs_exception_ce, stub = "\\QuickJSException"))]
#[derive(Default)]
#[php(name = "QuickJSTimeoutException")]
pub struct QuickJSTimeoutException;

#[php_impl]
impl QuickJSTimeoutException {}

#[php_class]
#[php(extends(ce = quickjs_exception_ce, stub = "\\QuickJSException"))]
#[derive(Default)]
#[php(name = "QuickJSMemoryException")]
pub struct QuickJSMemoryException;

#[php_impl]
impl QuickJSMemoryException {}

/// Class-entry accessor for the base exception, used as the parent of the
/// specialised subclasses above.
fn quickjs_exception_ce() -> &'static ext_php_rs::zend::ClassEntry {
    <QuickJSException as ext_php_rs::class::RegisteredClass>::get_metadata().ce()
}
