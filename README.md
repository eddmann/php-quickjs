# php-quickjs

A PHP extension that embeds a [QuickJS-NG](https://github.com/quickjs-ng/quickjs)
sandbox with **typed, bidirectional** communication. PHP runs untrusted JS in an
isolated context, exposes a controlled allowlist of PHP capabilities into JS as a
frozen, namespaced `php.module.fn()` SDK, and JS can call back into PHP
mid-execution.

Guest code may be **TypeScript**: it is transpiled to JavaScript in-process with
[`oxc`](https://github.com/oxc-project/oxc) before QuickJS ever sees it, and
runtime errors are mapped back to the original TS line/column.

Written in Rust using [`ext-php-rs`](https://github.com/davidcole1340/ext-php-rs)
(Zend side), [`rquickjs`](https://github.com/DelSkayn/rquickjs) (QuickJS-NG is
bundled — no system library needed), and `oxc` for the TypeScript fast path.

> **Scope.** This is an *embedder*, not a security boundary against hostile code
> on its own. The capability model contains *what JS can reach*; the memory/CPU
> limits contain *resource abuse* (infinite loops, alloc bombs). QuickJS C
> memory-corruption bugs are **not** contained — for attacker-controlled code,
> nest the whole extension inside an outer microVM/gVisor boundary.

## Quick start

```php
$js = new QuickJS(memoryLimit: 64 * 1024 * 1024, timeoutMs: 1000);

$js->register('log.info',  fn(string $m) => error_log("[js] $m"));
$js->register('fetchUser', fn(int $id) => ['name' => 'Ada', 'orders' => [1, 2, 3]]);

echo $js->eval(<<<'JS'
    php.log.info("starting");
    const u = php.fetchUser(42);            // reenters PHP
    `${u.name} has ${u.orders.length} orders`;
JS);
// => "Ada has 3 orders"
```

## Building

Requires Rust (1.96+, for oxc), clang, and the PHP dev headers (`php-config`,
`phpize`). The extension is a plain cargo `cdylib` — no `phpize` step.

```sh
make build                 # debug -> target/debug/libphp_quickjs.so
make release               # optimized -> target/release/libphp_quickjs.so
make test                  # Rust unit tests + PHP integration suite
make example               # run examples/usage.php

# Load it:
php -d extension=$(pwd)/target/release/libphp_quickjs.so script.php
```

## API

### `new QuickJS(?int $memoryLimit = null, ?int $timeoutMs = null, ?int $maxStack = null, bool $isolated = false)`
All limits default to unbounded; pass non-zero values to contain resource abuse.
`isolated: true` runs each `eval()` in a fresh realm (see [Execution scope](#execution-scope)).

### `register(string $name, callable $fn, ?string $types = null): void`
Expose a PHP callable to JS under a flat, dotted name. The name becomes
`php.<dotted.name>(...)` in the guest. `$types` is an optional TypeScript
signature surfaced by `dts()`. The dotted-name registry is the **entire** trust
boundary — flat and greppable.

### `eval(string $code): mixed`
Run **TypeScript or JavaScript** and marshal the result back. The source is
transpiled to JS with oxc (types erased, esnext target) before QuickJS sees it;
the frozen `php.*` facade is (re)built from the current manifest before the guest
runs. Runtime errors raise a `QuickJSEvalException` whose message and stack are
remapped to the original TS line/column. See [TypeScript](#typescript) below.

### `grant(mixed $resource): int` / `resolve(int $h): mixed` / `revoke(int $h): bool`
Capability handles for live, stateful objects (DB connections, file handles).
The object stays host-side; JS only ever sees an opaque integer it can pass back
to a capability. The handle **is** the capability.

```php
$pdo = new PDO('sqlite:app.db');
$h   = $js->grant($pdo);
$js->register('db.query', fn(int $handle, string $sql) => $js->resolve($handle)->query($sql)->fetchAll());
```

### `manifest(): array` / `dts(): string`
The manifest (`[['name' => ..., 'types' => ...], ...]`) and a generated
TypeScript `.d.ts` for the `php` global, both from the same source of truth.

## How it works

```
PHP (trusted)  ──ext-php-rs──►  Rust bridge  ──rquickjs──►  QuickJS (untrusted)
   register()                  dispatch table                php.module.fn()
   eval()                      __host(name, bytes)           frozen php.* facade
```

- **One host import.** Everything JS reaches goes through a single
  `__host(name, argsBytes)` function and a flat dispatch table. The namespaced
  `php.*` tree is cosmetic JS built from the manifest and **frozen** so guests
  cannot shadow a capability.
- **msgpack wire format.** Values cross the boundary as MessagePack payloads. A
  neutral `MiddleValue` (de)serializes to *native* msgpack and converts to JS
  values / PHP zvals; a small pure-JS codec (`src/js/msgpack.js`) interoperates
  with it.
- **Functions both ways.** A PHP callable handed to JS becomes a callable that
  routes back through the host; a JS function handed to PHP becomes a
  `Js\Callback` object whose `__invoke` re-enters JS. A depth guard bounds
  runaway mutual recursion.
- **Errors both ways.** A JS error past `eval` becomes a typed
  `QuickJSEvalException` (or `QuickJSTimeoutException` / `QuickJSMemoryException`);
  a PHP exception inside a callback becomes a JS `Error` exposing `e.phpClass`.

## TypeScript

`eval()` accepts TypeScript. The fast path — the Bun model — is transpile-and-go,
**no type-checking on the hot path**:

```
eval(tsSource)
  ├─ oxc transform  → js + source map   (types erased, esnext target)
  │     cache: hash(source) → (js, map)  — the map stays host-side
  ├─ rquickjs runs the JS (QuickJS only ever sees JS)
  └─ on throw → remap stack JS→TS coords → QuickJSEvalException
```

- **Always transpile.** Plain JS is valid TS, so it round-trips unchanged. The
  content-hash LRU cache makes re-running the same guest free.
- **esnext target.** A near-identity transform — just strip types. QuickJS-NG
  natively supports private fields, nullish, optional chaining, etc., so nothing
  is downleveled and source maps stay tight.
- **Full TS transform.** Type annotations, `interface`, `type`, generics and
  `as` casts erase to nothing; constructs that emit runtime code — `enum`,
  `namespace`, decorators — are transformed by oxc and work (the esbuild/Bun
  model). There is no separate type-erasure-only mode.
- **Errors map back to TS.** A guest throw becomes a `QuickJSEvalException`
  whose `getFile()`/`getLine()` are the original TS location and whose
  `getJsStack()` is the remapped, guest-only stack — even when type erasure
  shifted the generated JS:

  ```php
  try {
      $js->eval("interface Foo { a: number }\n\nthrow new Error('boom');");
  } catch (QuickJSEvalException $e) {
      $e->getMessage();   // "boom"
      $e->getLine();      // 3   (original TS line, not generated JS line 1)
      $e->getJsName();    // "Error"
      $e->getJsStack();   // "    at <eval> (guest.ts:3:7)"
  }
  ```

  Syntax errors are located too (`getLine()` + `getJsName() === 'SyntaxError'`),
  and a non-`Error` `throw` (object/array/number) is JSON-rendered into the
  message rather than dropped.
- **Source maps never enter the sandbox** — they are kept host-side, keyed by
  content hash, and used only when remapping an error.

Type-*checking* (e.g. a bundled `tsgo`) is intentionally absent and can be slotted
in later without reshaping this pipeline.

### Execution scope

By default, all `eval()` calls on one `QuickJS` instance share a single,
persistent global realm (like a REPL session): top-level
`var`/`let`/`const`/`function` and `globalThis` carry over between calls, and a
JS function handed to PHP stays callable for the instance's lifetime (its
registry entry is freed when the PHP `Js\Callback` is garbage-collected).

Pass `isolated: true` to run **each `eval()` in its own fresh realm** — a
stateless script runner:

```php
$js = new QuickJS(isolated: true);
$js->eval('const k = 1;');
$js->eval('const k = 2;');     // no redeclaration clash — different world
$js->eval('typeof k;');        // "undefined"
```

In isolated mode, registered capabilities, handles, marshaling and
*synchronous* callbacks all work as normal, but a JS callback **cannot outlive
the `eval()` that created it** (its realm is discarded) — invoking a stored one
later throws. Choose per intent: the instance is the unit of isolation, and
`isolated: true` shrinks it to a single eval.

## Value marshaling

| JS                 | PHP                        |
|--------------------|----------------------------|
| null / undefined   | null                       |
| boolean            | bool                       |
| number (integer)   | int                        |
| number (float)     | float                      |
| string             | string (UTF-8)             |
| Uint8Array         | binary string              |
| Array              | indexed array              |
| Object             | associative array          |
| function           | `Js\Callback` ⇄ callable   |

Non-UTF-8 PHP strings cross as bytes (`Uint8Array`). Integers beyond 2^53 lose
precision as JS numbers.

## Sandbox knobs

| Limit              | Guards                                   |
|--------------------|------------------------------------------|
| `memoryLimit`      | allocation bombs (`QuickJSMemoryException`) |
| `timeoutMs`        | infinite loops, wall-clock (`QuickJSTimeoutException`) |
| `maxStack`         | native stack exhaustion                  |
| frozen `php.*`     | what JS can name / reach                 |
| capability handles | which live objects JS can use            |

These are **resource** guards. For hostile code, add an outer VM boundary.

## Project layout

```
src/lib.rs        QuickJS class + module
src/engine.rs     runtime/context, depth guard, current-ctx stack
src/bridge.rs     __host dispatch, frozen facade, registries
src/marshal.rs    MiddleValue <-> JS / PHP, native-msgpack serde
src/callback.rs   Js\Callback (JS function -> PHP)
src/handles.rs    capability handle table
src/sandbox.rs    memory/stack limits + wall-clock interrupt
src/transpile.rs  oxc TS->JS transpile + content-hash cache
src/error.rs      exception bridging both ways + TS stack remapping
src/exceptions.rs typed PHP exception classes
src/manifest.rs   manifest + .d.ts generation
src/js/*.js       msgpack codec + function-ref runtime support
tests/php/*.php   integration suite
```

## License

MIT
