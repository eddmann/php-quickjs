<?php

declare(strict_types=1);

// The two execution modes side by side.
//
//   php -d extension=$(pwd)/target/debug/libphp_quickjs.so examples/modes.php
//
// A QuickJS *instance* is the unit of isolation. `isolated: true` shrinks that
// unit to a single eval(). See docs/execution-modes.md.

// ── Shared (default): one persistent realm for the instance's lifetime ───
echo "===== SHARED MODE (default) =====\n";
$shared = new QuickJS();
$shared->register('store', function ($cb) {
    $GLOBALS['cb'] = $cb;
});

$shared->eval('var visits = 0; function visit(){ return ++visits; }');
echo "eval #1 defined `visits` + `visit()`\n";
echo 'eval #2 SEES them:                  ', $shared->eval('visit(); visit(); visits;'), "\n";

$shared->eval('php.store((n: number) => n * 100)');
$shared->eval('1 + 1');                         // an unrelated later eval
echo 'stored callback survives evals:    ', $GLOBALS['cb'](2), "\n";

try {
    $shared->eval('const C = 1;');
    $shared->eval('const C = 2;');              // same realm -> clash
} catch (\Throwable $e) {
    echo 're-declaring top-level `const C`:  ', $e->getMessage(), "\n";
}

// ── Isolated: a fresh realm per eval(); each eval is its own world ────────
echo "\n===== ISOLATED MODE =====\n";
$iso = new QuickJS(isolated: true);
$iso->register('store', function ($cb) {
    $GLOBALS['cb2'] = $cb;
});

$iso->eval('var visits = 0;');
echo "eval #1 defined `visits`\n";
echo 'eval #2 does NOT see it:            ', $iso->eval('typeof visits;'), "\n";

$iso->eval('const C = 1;');
echo 're-declaring `const C` is fine:     ', $iso->eval('const C = 2; C;'), "\n";

$iso->register('add', fn(int $a, int $b) => $a + $b);
echo 'capabilities still work:           ', $iso->eval('php.add(2, 3);'), "\n";

$iso->register('apply', fn(callable $f) => $f(7));
echo 'synchronous callback within eval:  ', $iso->eval('php.apply((x: number) => x * x);'), "\n";

$iso->eval('php.store(() => 1)');               // store a callback...
try {
    echo $GLOBALS['cb2']();                      // ...invoke it after its eval
} catch (\Throwable $e) {
    echo 'stored callback after its eval:    ', $e->getMessage(), "\n";
}
