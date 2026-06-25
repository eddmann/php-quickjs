//! Typed PHP exception classes thrown across the bridge.
//!
//! ```text
//! \Exception
//!   └─ QuickJSException                (base for everything this ext throws)
//!        ├─ QuickJSEvalException        (a JS error escaped eval)
//!        ├─ QuickJSTimeoutException     (the wall-clock deadline tripped)
//!        └─ QuickJSMemoryException      (the memory limit tripped)
//! ```

use ext_php_rs::prelude::*;
use ext_php_rs::zend::ce;

#[php_class]
#[php(extends(ce = ce::exception, stub = "\\Exception"))]
#[derive(Default)]
#[php(name = "QuickJSException")]
pub struct QuickJSException;

#[php_impl]
impl QuickJSException {}

#[php_class]
#[php(extends(ce = quickjs_exception_ce, stub = "\\QuickJSException"))]
#[derive(Default)]
#[php(name = "QuickJSEvalException")]
pub struct QuickJSEvalException;

#[php_impl]
impl QuickJSEvalException {}

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
