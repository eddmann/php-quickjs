# API reference

The extension exposes a single `QuickJS` class. For the bigger picture see
[architecture](architecture.md); for realms and the callback lifecycle see
[execution modes](execution-modes.md).

### `new QuickJS(?int $memoryLimit = null, ?int $timeoutMs = null, ?int $maxStack = null, bool $isolated = false)`

Limits default to unbounded; pass non-zero values to contain resource abuse.
`isolated: true` runs each `eval()` in a fresh realm (see
[execution modes](execution-modes.md)).

### `register(string $name, callable $fn, ?string $types = null): void`

Expose a PHP callable to JS under a flat, dotted name — it becomes
`php.<dotted.name>(...)` in the guest. `$types` is an optional TypeScript signature
surfaced by `dts()`. This flat registry is the **entire** trust boundary.

### `eval(string $code): mixed`

Run TypeScript or JavaScript and marshal the result back to PHP. Errors raise a
`QuickJSEvalException` located at the original TS line/column (see [errors](errors.md)).

### `grant(mixed $resource): int` / `resolve(int $h): mixed` / `revoke(int $h): bool`

Capability handles for live, stateful objects (DB connections, file handles). The
object stays host-side; JS only ever sees an opaque integer it can pass back to a
capability. The handle **is** the capability.

```php
$pdo = new PDO('sqlite:app.db');
$h   = $js->grant($pdo);
$js->register('db.query', fn(int $handle, string $sql) => $js->resolve($handle)->query($sql)->fetchAll());
```

### `manifest(): array` / `dts(): string`

The registration manifest and a generated TypeScript `.d.ts` for the `php` global,
both from the same source of truth.
