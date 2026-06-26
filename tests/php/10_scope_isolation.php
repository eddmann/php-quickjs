<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

// --- Shared (default): one persistent realm per instance ----------------
$shared = new QuickJS();
$shared->eval('var counter = 0; function bump(){ return ++counter; }');
eq(2, $shared->eval('bump(); bump(); counter;'), 'globals persist across evals (shared)');
eq(42, $shared->eval('globalThis.X = 42; globalThis.X;'), 'globalThis persists');
eq(42, $shared->eval('globalThis.X;'), 'and is visible in a later eval');

// A JS callback handed to PHP survives across evals (registry persists).
$stored = null;
$shared->register('save', function ($cb) use (&$stored) {
    $stored = $cb;
});
$shared->eval('php.save((n) => n * 10)');
$shared->eval('1 + 1');                 // a later eval must not reset the registry
$shared->eval('2 + 2');
eq(70, $stored(7), 'stored JS callback survives later evals');

// A JS function that round-trips PHP->JS within one eval still works.
$shared->register('identity', fn($x) => $x);
eq(5, $shared->eval('const g = php.identity((n) => n + 1); g(4);'),
    'JS function round-trips through PHP within an eval');

// --- Registry cleanup: dropped callbacks are freed ----------------------
$shared->register('drop', fn($cb) => null);   // receives, does not keep
$before = (int) $shared->eval('__jsFnCount()');
for ($i = 0; $i < 4; $i++) {
    $shared->eval("php.drop(() => $i)");
}
gc_collect_cycles();
$shared->eval('1');                    // flush boundary releases dropped entries
eq($before, (int) $shared->eval('__jsFnCount()'), 'unstored callbacks are released');

// --- Isolated: each eval is its own world -------------------------------
$iso = new QuickJS(isolated: true);
$iso->eval('var counter = 0;');
eq('undefined', $iso->eval('typeof counter'), 'globals do NOT persist (isolated)');

// Re-declaring a top-level const across evals is fine (fresh realm each time).
$iso->eval('const k = 1; k;');
eq(2, $iso->eval('const k = 2; k;'), 'no cross-eval redeclaration clash (isolated)');

// Capabilities, marshaling, and synchronous callbacks still work.
$iso->register('add', fn(int $a, int $b) => $a + $b);
eq(5, $iso->eval('php.add(2, 3);'), 'capabilities work in isolated mode');
$iso->register('apply', fn(callable $f, $x) => $f($x));
eq(36, $iso->eval('php.apply((x) => x * x, 6);'), 'synchronous callbacks work in isolated mode');

// A JS callback cannot outlive its eval in isolated mode (realm is gone).
$held = null;
$iso->register('keep', function ($cb) use (&$held) {
    $held = $cb;
});
$iso->eval('php.keep(() => 1)');
throws(fn() => $held(), \Throwable::class, 'stored callback rejected after its eval (isolated)');

done();
