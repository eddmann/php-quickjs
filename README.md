# php-quickjs

Run **untrusted JavaScript or TypeScript inside PHP**, safely and ergonomically.

PHP applications increasingly need to execute user-supplied logic — rules,
formulas, templates, plugins, AI-generated snippets. Doing that in PHP itself
means `eval()` (no isolation) or a separate service (operational weight).
`php-quickjs` embeds a [QuickJS-NG](https://github.com/quickjs-ng/quickjs) engine
directly in the process and gives you a **typed, bidirectional bridge**:

- The guest runs in an **isolated context** with memory, time, and stack limits.
- PHP exposes a **controlled allowlist** of capabilities into JS as a frozen,
  namespaced `php.module.fn()` SDK — the guest can only reach what you grant.
- JS can **call back into PHP** mid-execution, pass functions both ways, and
  hold opaque handles to live PHP objects.
- Guest code may be **TypeScript**: it is transpiled to JS in-process with
  [`oxc`](https://github.com/oxc-project/oxc), and runtime errors are mapped back
  to the original TS line/column.

Written in Rust with [`ext-php-rs`](https://github.com/davidcole1340/ext-php-rs)
(the Zend side) and [`rquickjs`](https://github.com/DelSkayn/rquickjs) (QuickJS-NG
is bundled — no system library needed).

> **Scope.** This is an *embedder*, not a security boundary against hostile code
> on its own. The capability model contains *what JS can reach*; the resource
> limits contain *abuse* (infinite loops, alloc bombs). QuickJS C
> memory-corruption bugs are **not** contained — for attacker-controlled code,
> nest the whole extension inside an outer microVM/gVisor boundary.

## Getting started

**Requirements:** Rust 1.96+ (for oxc), clang, and PHP 8.4 dev headers
(`php-config`). The extension is a plain cargo `cdylib` — no `phpize` step.

```sh
git clone https://github.com/eddmann/php-quickjs && cd php-quickjs
make build        # -> target/debug/libphp_quickjs.so
make test         # Rust unit tests + PHP integration suite
```

Load it and run your first guest:

```php
<?php
// hello.php — run with:
//   php -d extension=$(pwd)/target/debug/libphp_quickjs.so hello.php

$js = new QuickJS(memoryLimit: 64 * 1024 * 1024, timeoutMs: 1000);

$js->register('log.info',  fn(string $m) => error_log("[js] $m"));
$js->register('fetchUser', fn(int $id) => ['name' => 'Ada', 'orders' => [1, 2, 3]]);

echo $js->eval(<<<'TS'
    php.log.info("starting");
    const u = php.fetchUser(42);            // reenters PHP
    `${u.name} has ${u.orders.length} orders`;
TS);
// => "Ada has 3 orders"
```

More to copy from: [`examples/kitchen_sink.php`](examples/kitchen_sink.php) (every
feature), [`examples/modes.php`](examples/modes.php), and
[`examples/usage.php`](examples/usage.php).

## API

### `new QuickJS(?int $memoryLimit = null, ?int $timeoutMs = null, ?int $maxStack = null, bool $isolated = false)`
Limits default to unbounded; pass non-zero values to contain resource abuse.
`isolated: true` runs each `eval()` in a fresh realm (see
[execution modes](docs/execution-modes.md)).

### `register(string $name, callable $fn, ?string $types = null): void`
Expose a PHP callable to JS under a flat, dotted name — it becomes
`php.<dotted.name>(...)` in the guest. `$types` is an optional TypeScript
signature surfaced by `dts()`. This flat registry is the **entire** trust
boundary.

### `eval(string $code): mixed`
Run TypeScript or JavaScript and marshal the result back to PHP. Errors raise a
`QuickJSEvalException` located at the original TS line/column (see
[errors](docs/errors.md)).

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
The registration manifest and a generated TypeScript `.d.ts` for the `php`
global, both from the same source of truth.

## How it works (in brief)

```
PHP (trusted)  ──ext-php-rs──►  Rust bridge  ──rquickjs──►  QuickJS (untrusted)
   register()                  dispatch table                php.module.fn()
   eval()                      __host(name, bytes)           frozen php.* facade
```

Everything the guest reaches goes through a **single** `__host(name, argsBytes)`
import and a flat dispatch table — the namespaced `php.*` tree is cosmetic JS,
built from the manifest and **frozen**. Values cross as MessagePack; functions
cross both ways as references backed by registries; errors bridge both ways and
remap to TS coordinates.

→ Full details in **[docs/architecture.md](docs/architecture.md)**.

### TypeScript

`eval()` accepts TS (the Bun model: transpile-and-go, no type-checking on the hot
path). Types/`interface`/generics erase; the transpile result is cached by
content hash; the source map stays host-side and is used only to remap errors.
→ [docs/architecture.md#the-typescript-fast-path](docs/architecture.md#the-typescript-fast-path).

### Execution modes

By default, all `eval()` calls on an instance share one **persistent** global
realm (a REPL-like session; state and callbacks carry over). Pass
`isolated: true` to run **each `eval()` in its own fresh realm** (a stateless
script runner). → [docs/execution-modes.md](docs/execution-modes.md).

### Sandbox & security

| Layer | Contains |
|-------|----------|
| frozen `php.*` + flat dispatch table | what JS can *name* / reach |
| capability handles | which live objects JS can *use* |
| `memoryLimit` / `timeoutMs` / `maxStack` | resource abuse (loops, alloc bombs) |
| **outer microVM / gVisor** | QuickJS C memory-corruption → host RCE |

These are resource guards; the extension is the embedder, not a memory-safety
boundary. For hostile code, add an outer VM.

## Documentation

- [Installation](docs/install.md) — prebuilt binaries for self-hosted PHP, AWS
  Lambda (Bref), and macOS; or build from source.
- [Architecture](docs/architecture.md) — the bridge, marshaling, function
  passing, security model.
- [Execution modes](docs/execution-modes.md) — realms, shared vs. isolated,
  callback lifecycle.
- [Errors](docs/errors.md) — typed exceptions, both-way bridging, TS remapping.

## Project layout

```
src/lib.rs        QuickJS class + module          src/handles.rs    capability handle table
src/engine.rs     runtime/realms, re-entrancy     src/sandbox.rs    memory/stack/timeout limits
src/bridge.rs     __host dispatch, frozen facade   src/error.rs      exception bridging + TS remap
src/marshal.rs    value <-> msgpack <-> zval       src/exceptions.rs typed exception classes
src/callback.rs   Js\Callback (JS fn -> PHP)       src/manifest.rs   manifest + .d.ts generation
src/transpile.rs  oxc TS->JS + cache               src/js/*.js       in-sandbox codec + runtime
docs/             implementation guide            examples/         runnable demos
tests/php/        integration suite
```

## License

MIT
