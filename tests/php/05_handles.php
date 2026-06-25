<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

// A live, stateful PHP object we never want serialized into JS.
class Counter
{
    private int $n = 0;
    public function bump(int $by = 1): int
    {
        return $this->n += $by;
    }
    public function value(): int
    {
        return $this->n;
    }
}

$js = new QuickJS();
$counter = new Counter();

// grant() hands JS an opaque int; the live object stays host-side.
$h = $js->grant($counter);
ok(is_int($h), 'grant() returns an integer handle');

// Capabilities resolve the handle back to the live object.
$js->register('counter.bump', fn(int $handle, int $by = 1) => $js->resolve($handle)->bump($by));
$js->register('counter.value', fn(int $handle) => $js->resolve($handle)->value());

// JS drives the live object purely through the handle.
eq(1, $js->eval("php.counter.bump($h)"), 'mutate live object via handle');
eq(5, $js->eval("php.counter.bump($h, 4)"), 'second mutation accumulates');
eq(5, $js->eval("php.counter.value($h)"), 'state persisted host-side');

// The same object is referenced (not a copy): PHP sees the mutations.
eq(5, $counter->value(), 'host object reflects JS-driven mutations');

// A typed wrapper makes the handle ergonomic in JS (the spec pattern).
eq(8, $js->eval("
    class CounterHandle {
        #id;
        constructor(id){ this.#id = id; }
        bump(by){ return php.counter.bump(this.#id, by); }
        value(){ return php.counter.value(this.#id); }
    }
    const c = new CounterHandle($h);
    c.bump(3);
    c.value();
"), 'opaque handle wrapped in a JS class');

// The handle is opaque: it is just an int, carrying no object data.
eq('number', $js->eval("typeof $h"), 'handle is an opaque number in JS');

// Unknown / revoked handles are rejected.
ok($js->revoke($h), 'revoke() succeeds for a live handle');
ok(!$js->revoke($h), 'revoke() is idempotent');
throws(fn() => $js->eval("php.counter.value($h)"), \Throwable::class, 'resolving a revoked handle throws');

// References survive PHP garbage collection while granted.
$h2 = $js->grant(new Counter());
gc_collect_cycles();
$js->eval("php.counter.bump($h2, 7)");
eq(7, $js->eval("php.counter.value($h2)"), 'granted object survives gc_collect_cycles()');

done();
