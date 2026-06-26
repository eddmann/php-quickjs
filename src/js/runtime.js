// Runtime support for bidirectional function passing across the bridge.
//
// Functions cannot be msgpack-encoded, so they cross as tagged refs:
//   a JS function   -> { "$__jsfn": id }   (id into the JS-side registry)
//   a PHP callable   -> { "$__phpfn": id }  (id into the host-side registry)
//
// `wrap` replaces functions with refs before encoding; `unwrap` replaces refs
// with callables after decoding. The host (Rust) sees only the tagged refs.
//
// Installed once per realm: the guard keeps the JS-side function registry
// (`jsFns`) alive across re-installs, so a JS function handed to PHP survives
// for the life of the realm. Entries are freed when their Js\Callback is
// garbage-collected (host calls `__deleteJsFn`).
if (!globalThis.__rt) {
  globalThis.__rt = (function () {
  "use strict";
  var mp = globalThis.__mp;
  var JSFN = "$__jsfn";
  var PHPFN = "$__phpfn";

  var jsFns = {};
  var nextId = 1;

  function registerFn(fn) {
    var id = nextId++;
    jsFns[id] = fn;
    return id;
  }
  function getFn(id) {
    return jsFns[id];
  }
  function deleteFn(id) {
    delete jsFns[id];
  }

  function isPlainContainer(v) {
    return v && typeof v === "object" && !(v instanceof Uint8Array);
  }

  // Replace JS functions with refs (outgoing: JS -> host).
  function wrap(v) {
    if (typeof v === "function") {
      var o = {};
      o[JSFN] = registerFn(v);
      return o;
    }
    if (Array.isArray(v)) return v.map(wrap);
    if (isPlainContainer(v)) {
      var out = {};
      for (var k in v) {
        if (Object.prototype.hasOwnProperty.call(v, k)) out[k] = wrap(v[k]);
      }
      return out;
    }
    return v;
  }

  // Replace refs with callables (incoming: host -> JS).
  function unwrap(v) {
    if (isPlainContainer(v)) {
      if (Object.prototype.hasOwnProperty.call(v, PHPFN)) {
        return makePhpFn(v[PHPFN]);
      }
      if (Object.prototype.hasOwnProperty.call(v, JSFN)) {
        return getFn(v[JSFN]);
      }
      if (Array.isArray(v)) return v.map(unwrap);
      var out = {};
      for (var k in v) {
        if (Object.prototype.hasOwnProperty.call(v, k)) out[k] = unwrap(v[k]);
      }
      return out;
    }
    return v;
  }

  function makePhpFn(id) {
    return function () {
      return callPhp(id, Array.prototype.slice.call(arguments));
    };
  }

  // JS -> host capability dispatch.
  function callHost(name, args) {
    return unwrap(mp.decode(globalThis.__host(name, mp.encode(wrap(args)))));
  }

  // JS -> host: invoke a PHP callable previously handed to JS.
  function callPhp(id, args) {
    return unwrap(mp.decode(globalThis.__php_invoke(id, mp.encode(wrap(args)))));
  }

  // host -> JS: invoke a JS function previously handed to PHP (called by Rust).
  function invokeJs(id, argsBytes) {
    var fn = jsFns[id];
    if (!fn) throw new Error("unknown JS callback id " + id);
    var args = unwrap(mp.decode(argsBytes));
    var r = fn.apply(null, args);
    return mp.encode(wrap(r));
  }

  globalThis.__invokeJs = invokeJs;
  // Used by the host to register a bare JS function value (e.g. an eval result
  // that is a function) so it can be handed to PHP as a Js\Callback.
  globalThis.__registerJsFn = registerFn;
  // Used by the host to reconstruct callables when marshaling MiddleValue ->
  // JS directly (e.g. QuickJS::roundtrip of a function).
  globalThis.__getJsFn = getFn;
  globalThis.__makePhpFn = makePhpFn;
  // Called when a PHP-side Js\Callback is garbage-collected, to release its
  // entry from the registry.
  globalThis.__deleteJsFn = deleteFn;
  // Test/diagnostic helper: number of live JS callbacks held for PHP.
  globalThis.__jsFnCount = function () {
    return Object.keys(jsFns).length;
  };

  return {
    registerFn: registerFn,
    getFn: getFn,
    deleteFn: deleteFn,
    wrap: wrap,
    unwrap: unwrap,
    callHost: callHost,
    callPhp: callPhp,
    invokeJs: invokeJs,
  };
  })();
}
