# Errors

Errors cross the boundary in both directions, and a JS/TS error that escapes
`eval()` surfaces as a real, typed PHP exception located at its **original
TypeScript** coordinates.

## Exception hierarchy

```
\Exception
  └─ QuickJSException                 (base for everything this extension throws)
       ├─ QuickJSEvalException        (a JS/TS error escaped eval, or a transpile error)
       ├─ QuickJSTimeoutException     (the wall-clock deadline tripped)
       └─ QuickJSMemoryException      (the memory limit tripped)
```

Catch the base to handle everything, or a leaf to be specific:

```php
try {
    $js->eval($code);
} catch (QuickJSTimeoutException $e) {
    // infinite loop / over budget
} catch (QuickJSEvalException $e) {
    // a guest error or a syntax error
} catch (QuickJSException $e) {
    // anything else from the engine
}
```

## JS / TS error → PHP exception

When a guest throws past `eval`, you get a `QuickJSEvalException`. It behaves like
a normal PHP exception *and* exposes the JS specifics:

```php
try {
    // The interface fully erases, so the throw lands on generated JS line 1 —
    // but the source map reports the original TS line.
    $js->eval("interface Foo { a: number }\n\nthrow new TypeError('boom');");
} catch (QuickJSEvalException $e) {
    $e->getMessage();   // "TypeError: boom"   (clean error text)
    $e->getFile();      // "guest.ts"
    $e->getLine();      // 3                   (original TS line, not JS line 1)
    $e->getJsName();    // "TypeError"
    $e->getJsStack();   // "    at <eval> (guest.ts:3:7)"   (guest frames, TS coords)
    (string) $e;        // "QuickJSEvalException: TypeError: boom in guest.ts:3 …"
}
```

What each accessor gives you:

- **`getMessage()`** — the error text only (`"TypeError: boom"`), with a bare
  `Error` name elided to avoid a redundant prefix.
- **`getFile()` / `getLine()`** — the original **TS** location, so the standard
  PHP idioms (`getLine()`, `getTraceAsString()`, `(string) $e`) read naturally.
- **`getJsName()`** — the JS error constructor (`TypeError`, `RangeError`, a
  custom subclass name, …), or the originating PHP class for a re-surfaced host
  error.
- **`getJsStack()`** — the stack **remapped to TS coordinates** and **filtered to
  guest frames**: the internal bridge/bootstrap frames are removed, so it reads
  like a plain TS trace.

### How the remapping works

The module is named `guest.ts` when handed to QuickJS, so stack frames reference
it. On a throw, `error.rs` reads the JS stack (generated-JS coordinates), and for
each frame referencing the guest module it looks the position up in the module's
**source map** (kept host-side from transpilation) and rewrites it to the
original TS `line:col`. Frames that don't reference the guest module — the
`__rt`/facade plumbing — are dropped.

### Non-`Error` throws are surfaced, not lost

```php
$js->eval("throw { code: 42 };");   // message: {"code":42}
$js->eval("throw [1, 2, 3];");      // message: [1,2,3]
$js->eval("throw 'nope';");         // message: nope
```

A thrown object/array/number is `JSON.stringify`-ed into the message rather than
collapsing to a generic "uncaught" string.

### Syntax / transpile errors are located

A guest that doesn't parse surfaces as a `QuickJSEvalException` with
`getJsName() === 'SyntaxError'` and `getLine()` pointing at the offending TS line
(computed from the oxc diagnostic's span).

### Resource limits

An infinite loop or over-budget script raises `QuickJSTimeoutException`; an
allocation bomb raises `QuickJSMemoryException`. These fire from the interrupt
handler / allocator and so carry no meaningful source location. The engine
recovers and remains usable afterward.

## PHP exception → JS

When a PHP capability (or callback) throws, the bridge turns it into a JS `Error`
the guest can catch and inspect via `e.phpClass`:

```php
$js->register('risky', fn() => throw new \RuntimeException('db down'));

$js->eval(<<<'JS'
    try {
        php.risky();
    } catch (e) {
        // e.message  === "db down"
        // e.phpClass === "RuntimeException"
        // e.name     === "RuntimeException"
    }
JS);
```

The PHP message is read via `getMessage()` (it's a protected property, so the
extension reads it through the proper accessor, not the raw object), and the
class name is attached as both `name` and `phpClass`.

### Round trips stay clean

If a PHP exception travels PHP → JS → PHP (for example, thrown inside a JS
callback that PHP invoked, and not caught in JS), it re-surfaces with its
**original class and a clean message** — not a nested `Exception: Exception: …`
wrapper. The `Js\Callback` layer recognizes a re-surfaced host error (it carries
`phpClass`) and restores that PHP class via `ClassEntry::try_find`, so:

```php
$js->register('relay', fn(callable $fn) => $fn());
try {
    $js->eval('php.relay(() => { throw new RangeError("x"); });');
} catch (QuickJSEvalException $e) {
    // a JS RangeError that crossed into PHP and back surfaces as a
    // QuickJSEvalException with jsName "RangeError" — not a doubled wrapper.
}
```

## Implementation note: writing protected exception properties

`message`, `file`, and `line` are *protected* on `\Exception`. ext-php-rs's safe
`set_property` passes a null scope, which can only create an (invisible) dynamic
property that `getMessage()` never reads. So `exceptions.rs` builds the exception
object, then sets those slots via Zend's `zend_update_property` with the
`\Exception` class as the scope — the canonical way internal code populates them
— before throwing the object. The JS name and stack live on the Rust-backed
`QuickJSEvalException` struct and are read by `getJsName()` / `getJsStack()`.
