# php-quickjs

Run untrusted **JavaScript or TypeScript inside PHP** — safely, with a typed,
bidirectional bridge.

`php-quickjs` embeds the [QuickJS-NG](https://github.com/quickjs-ng/quickjs) engine
directly in your PHP process. Guest code runs in an isolated context with memory,
time, and stack limits; PHP exposes a controlled allowlist of capabilities into JS;
and values, functions, and errors cross the boundary both ways. Guest code may be
TypeScript — it's transpiled in-process and runtime errors map back to the original
TS source.

Built in Rust with [`ext-php-rs`](https://github.com/davidcole1340/ext-php-rs) and
[`rquickjs`](https://github.com/DelSkayn/rquickjs). QuickJS-NG is bundled — no system
library required.

## Features

- **Isolated guest** — memory, time, and stack limits contain runaway code.
- **Capability allowlist** — JS only sees the PHP functions you expose, as a frozen
  `php.module.fn()` SDK.
- **Bidirectional** — JS calls back into PHP, functions pass both ways, and opaque
  handles wrap live PHP objects.
- **TypeScript built in** — transpiled with [`oxc`](https://github.com/oxc-project/oxc);
  errors map to the original TS line and column.
- **Typed exceptions** — guest failures surface as `QuickJSEvalException` with a
  JS-like message and stack.

## Quick example

PHP exposes a narrow capability surface; the guest — JavaScript or TypeScript — runs
sandboxed and calls back in.

```php
<?php
$js = new QuickJS(memoryLimit: 32 * 1024 * 1024, timeoutMs: 500);
$js->register('fetchUser', fn(int $id) => ['name' => 'Ada', 'roles' => ['admin', 'dev']]);

echo $js->eval(<<<'TS'
    interface User { name: string; roles: string[] }   // types erase in-process
    const u: User = php.fetchUser(42);                  // re-enters PHP
    `${u.name} has ${u.roles.length} roles`;
TS);
// => "Ada has 2 roles"
```

A tour of the rest of the bridge:

```php
// Functions cross both ways — a JS callback arrives in PHP as a Js\Callback.
$js->register('map', fn(array $xs, callable $fn) => array_map($fn, $xs));
$js->eval('php.map([1, 2, 3], (n: number) => n * n)');         // => [1, 4, 9]

// Live objects stay host-side; JS only ever holds an opaque handle.
$h = $js->grant(new ArrayObject(['hits' => 0]));
$js->register('bump', fn(int $h) => ++$js->resolve($h)['hits']);
$js->eval("php.bump($h); php.bump($h);");                      // => 2

// Guest failures surface as typed exceptions, located in the original TS.
try {
    $js->eval("const x: any = null;\nx.field;");
} catch (QuickJSEvalException $e) {
    echo $e->getJsName(), ' @ line ', $e->getLine();          // => "TypeError @ line 2"
}

// Resource abuse is contained, and the engine recovers afterwards.
$g = new QuickJS(timeoutMs: 100);
try { $g->eval('while (true) {}'); } catch (QuickJSTimeoutException $e) {}
$g->eval('1 + 1');                                             // => 2
```

Run with `php -d extension=/path/to/libphp_quickjs.so script.php`. Fuller programs —
typed capabilities, `dts()`, isolated realms — in [`examples/`](examples):
`kitchen_sink.php`, `modes.php`, `usage.php`.

## Installation

Prebuilt binaries are attached to each
[release](https://github.com/eddmann/php-quickjs/releases) for PHP 8.4 / 8.5 —
self-hosted Linux, AWS Lambda (a ready Bref layer), and macOS (Apple Silicon). Enable
the one matching your platform:

```ini
; php.ini
extension=/path/to/php-quickjs-...so
```

Or build from source (Rust 1.96+, clang, PHP dev headers — a plain cargo `cdylib`, no
`phpize`):

```sh
git clone https://github.com/eddmann/php-quickjs && cd php-quickjs
make build
```

→ Full platform matrix, Docker, and AWS Lambda / Bref instructions:
**[docs/install.md](docs/install.md)**.

## How it works

```
PHP (trusted)  ──ext-php-rs──►  Rust bridge  ──rquickjs──►  QuickJS (untrusted)
   register()                  dispatch table                php.module.fn()
   eval()                      __host(name, bytes)           frozen php.* facade
```

Everything the guest reaches goes through a single `__host` import and a flat dispatch
table; the namespaced `php.*` tree is frozen JS built from your registrations. Values
cross as MessagePack, functions as references backed by registries, and errors bridge
both ways — remapping to TS coordinates on the way out.

→ **[docs/architecture.md](docs/architecture.md)** for the full design.

## Scope

This is an *embedder*, not a standalone defence against hostile code. The capability
model contains *what JS can reach*; the resource limits contain *abuse* (infinite
loops, alloc bombs). QuickJS C memory-corruption bugs are **not** contained — for
attacker-controlled code, nest the extension inside an outer microVM / gVisor boundary.

## Documentation

- [Installation](docs/install.md) — prebuilt binaries, Docker, AWS Lambda (Bref), and
  building from source.
- [API reference](docs/api.md) — the `QuickJS` class and every method.
- [Architecture](docs/architecture.md) — the bridge, marshaling, function passing, and
  security model.
- [Execution modes](docs/execution-modes.md) — shared vs. isolated realms and the
  callback lifecycle.
- [Errors](docs/errors.md) — typed exceptions, both-way bridging, and TypeScript
  remapping.

## License

MIT
