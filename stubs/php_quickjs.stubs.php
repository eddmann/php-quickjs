<?php

// Stubs for the php-quickjs extension (IDE / static-analysis aid only).
// These declarations describe the native classes; they are not loaded at
// runtime. Regenerate with `make stubs` (requires cargo-php).

/**
 * An embedded QuickJS sandbox with a typed, bidirectional PHP bridge.
 */
class QuickJS
{
    /**
     * @param int|null $memoryLimit Max heap bytes (0/null = unbounded).
     * @param int|null $timeoutMs   Per-eval wall-clock budget in ms (0/null = unbounded).
     * @param int|null $maxStack    Max native stack bytes (0/null = engine default).
     */
    public function __construct(?int $memoryLimit = null, ?int $timeoutMs = null, ?int $maxStack = null) {}

    /**
     * Register a PHP callable under a flat, dotted capability name, callable
     * from JS as `php.<dotted.name>(...)`.
     *
     * @param string      $name     e.g. "db.query"
     * @param callable    $callable
     * @param string|null $types    Optional TypeScript signature for `dts()`.
     */
    public function register(string $name, callable $callable, ?string $types = null): void {}

    /** Evaluate JS source and marshal the result back to a PHP value. */
    public function eval(string $code): mixed {}

    /** The registration manifest: a list of `['name' => string, 'types' => ?string]`. */
    public function manifest(): array {}

    /** Generate a TypeScript `.d.ts` declaration for the `php` global. */
    public function dts(): string {}

    /** Grant JS an opaque integer handle to a live PHP value. */
    public function grant(mixed $resource): int {}

    /** Resolve a handle back to its live PHP value (throws if unknown). */
    public function resolve(int $handle): mixed {}

    /** Revoke a handle, releasing the host-side reference. */
    public function revoke(int $handle): bool {}

    /** Round-trip a PHP value through JS and back (testing/diagnostics). */
    public function roundtrip(mixed $value): mixed {}
}

namespace Js {
    /**
     * A JS function handed to PHP. Invoke it like any callable: `$cb(...$args)`.
     */
    class Callback
    {
        public function __invoke(mixed ...$args): mixed {}
        public function call(mixed ...$args): mixed {}
    }
}

namespace {
    /** Base class for every exception thrown by the extension. */
    class QuickJSException extends \Exception {}

    /** A JavaScript error escaped `eval`. */
    class QuickJSEvalException extends QuickJSException {}

    /** The wall-clock deadline tripped during `eval`. */
    class QuickJSTimeoutException extends QuickJSException {}

    /** The memory limit tripped during `eval`. */
    class QuickJSMemoryException extends QuickJSException {}
}
