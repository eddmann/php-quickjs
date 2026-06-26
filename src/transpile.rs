//! The TypeScript fast path. Guest source is transpiled to JavaScript with oxc
//! (types erased, esnext target — a near-identity transform) before it ever
//! reaches QuickJS, and the resulting source map is kept host-side so guest
//! errors can be remapped from generated-JS back to original-TS coordinates.
//!
//! Transpilation is content-addressed and cached: re-evaluating the same guest
//! is free, and the source map never enters the sandbox.

use lru::LruCache;
use oxc::codegen::CodegenReturn;
use oxc::diagnostics::Diagnostics;
use oxc::parser::ParseOptions;
use oxc::span::SourceType;
use oxc::transformer::TransformOptions;
use oxc::CompilerInterface;
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::Path;
use std::rc::Rc;

/// A transpiled guest module: the JS QuickJS will run, plus the source map
/// (as JSON) for remapping errors. `map` is `None` only if codegen emitted none.
#[derive(Clone)]
pub struct Transpiled {
    pub module_id: String,
    pub js: Rc<str>,
    pub map_json: Option<Rc<str>>,
}

/// Transpile TS -> JS. `module_id` names the module (used as the codegen source
/// path and the QuickJS filename, so stack frames are attributable). Returns a
/// formatted diagnostic string on parse/transform failure.
pub fn transpile(source: &str, module_id: &str) -> Result<(String, Option<String>), String> {
    let options = TransformOptions::from_target("esnext")
        .map_err(|e| format!("invalid transform target: {e}"))?;
    let mut compiler = TsCompiler {
        options,
        code: None,
        map_json: None,
        errors: Vec::new(),
    };
    compiler.compile(source, SourceType::ts(), Path::new(module_id));

    if !compiler.errors.is_empty() {
        return Err(compiler.errors.join("\n"));
    }
    let code = compiler
        .code
        .ok_or_else(|| "transpiler produced no output".to_owned())?;
    Ok((code, compiler.map_json))
}

/// An oxc compiler pipeline configured for type-stripping + source maps. The
/// trait runs parse -> semantic -> transform -> codegen; we capture the
/// generated code, the map, and any diagnostics.
struct TsCompiler {
    options: TransformOptions,
    code: Option<String>,
    map_json: Option<String>,
    errors: Vec<String>,
}

impl CompilerInterface for TsCompiler {
    fn parse_options(&self) -> ParseOptions {
        ParseOptions::default()
    }

    fn transform_options(&self) -> Option<&TransformOptions> {
        Some(&self.options)
    }

    fn enable_sourcemap(&self) -> bool {
        true
    }

    fn handle_errors(&mut self, errors: Diagnostics) {
        for e in errors {
            self.errors.push(e.to_string());
        }
    }

    fn after_codegen(&mut self, ret: CodegenReturn<'_>) {
        self.code = Some(ret.code);
        self.map_json = ret.map.map(|m| m.to_json_string());
    }
}

// ---------------------------------------------------------------------------
// content-addressed cache
// ---------------------------------------------------------------------------

struct CachedModule {
    source: String,
    transpiled: Transpiled,
}

/// A small LRU mapping `hash(source)` -> transpiled output. Single-threaded
/// (PHP NTS), so a `RefCell` is sufficient.
pub struct TranspileCache {
    inner: RefCell<LruCache<u64, CachedModule>>,
}

impl TranspileCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        TranspileCache {
            inner: RefCell::new(LruCache::new(cap)),
        }
    }

    /// Return the transpiled form of `source`, transpiling and caching on miss.
    /// The cache key is a content hash; the stored source is compared on a hit
    /// to rule out a hash collision returning the wrong JS.
    pub fn get_or_transpile(&self, source: &str) -> Result<Transpiled, String> {
        let key = hash(source);
        if let Some(hit) = self.inner.borrow_mut().get(&key) {
            if hit.source == source {
                return Ok(hit.transpiled.clone());
            }
        }
        // A clean, stable label for stack frames (the hash is the cache key,
        // not user-facing). Single-source guests share one filename.
        let module_id = "guest.ts".to_owned();
        let (js, map_json) = transpile(source, &module_id)?;
        let transpiled = Transpiled {
            module_id,
            js: Rc::from(js.as_str()),
            map_json: map_json.map(|m| Rc::from(m.as_str())),
        };
        self.inner.borrow_mut().put(
            key,
            CachedModule {
                source: source.to_owned(),
                transpiled: transpiled.clone(),
            },
        );
        Ok(transpiled)
    }
}

fn hash(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_types() {
        let (js, map) = transpile("const x: number = 41;\nconst y = x + 1;", "m.ts").unwrap();
        assert!(!js.contains(": number"), "type annotation not stripped: {js}");
        assert!(js.contains("41"));
        assert!(map.is_some(), "source map should be emitted");
    }

    #[test]
    fn keeps_private_fields_native() {
        // esnext target must NOT downlevel #private to WeakMaps.
        let (js, _) = transpile("class C { #id = 1; get(){ return this.#id; } }", "m.ts").unwrap();
        assert!(js.contains("#id"), "private field downleveled: {js}");
    }

    #[test]
    fn syntax_error_is_reported() {
        let err = transpile("const = ;", "m.ts").unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn cache_hits_return_same_output() {
        let cache = TranspileCache::new(8);
        let a = cache.get_or_transpile("const a: number = 1; a;").unwrap();
        let b = cache.get_or_transpile("const a: number = 1; a;").unwrap();
        assert_eq!(a.js, b.js);
        assert_eq!(a.module_id, b.module_id);
    }
}
