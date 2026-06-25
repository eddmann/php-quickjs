<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();

// --- Direction 1: PHP callable -> JS function ---------------------------
// A capability returns a closure; JS calls it, reentering PHP.
$js->register('makeAdder', fn(int $n) => fn(int $x) => $x + $n);
eq(15, $js->eval('
    const add10 = php.makeAdder(10);
    add10(5);
'), 'PHP-returned closure is callable from JS');

// A capability that returns an object containing callbacks.
$js->register('handlers', fn() => [
    'double' => fn(int $x) => $x * 2,
    'greet' => fn(string $n) => "hi $n",
]);
eq(['v' => 8, 'g' => 'hi Ada'], $js->eval('
    const h = php.handlers();
    ({ v: h.double(4), g: h.greet("Ada") });
'), 'callbacks nested in a returned object');

// --- Direction 2: JS function -> PHP (Js\Callback) ----------------------
// PHP receives a JS function as an argument and invokes it synchronously.
$js->register('apply', fn(callable $fn, $arg) => $fn($arg));
eq(20, $js->eval('php.apply(x => x * 2, 10)'), 'PHP invokes a JS callback synchronously');

// PHP higher-order capability calling a JS callback multiple times.
$js->register('mapEach', function (array $items, callable $fn) {
    return array_map($fn, $items);
});
eq([1, 4, 9], $js->eval('php.mapEach([1,2,3], n => n * n)'), 'JS callback invoked repeatedly from PHP');

// The Js\Callback is a real object of class Js\Callback.
$js->register('inspect', fn(callable $fn) => get_class($fn));
eq('Js\\Callback', $js->eval('php.inspect(() => 1)'), 'JS function arrives as Js\\Callback');

// --- Round trip: JS fn -> PHP -> back to JS unchanged --------------------
$js->register('identity', fn($x) => $x);
eq(42, $js->eval('
    const f = () => 42;
    const g = php.identity(f);   // f -> PHP Js\\Callback -> back to JS as f
    g();
'), 'JS function survives a round trip through PHP');

// --- Mutual recursion / nesting (PHP -> JS -> PHP -> JS) -----------------
$js->register('callTwice', fn(callable $fn, $x) => $fn($fn($x)));
eq(12, $js->eval('php.callTwice(n => php.makeAdder(3)(n), 6)'),
    'nested JS->PHP->JS->PHP callbacks');

// --- Depth guard --------------------------------------------------------
// Unbounded mutual recursion is stopped with an exception, not a crash.
$js->register('bounce', fn(callable $fn) => $fn());
throws(
    fn() => $js->eval('
        function rec(){ return php.bounce(rec); }
        rec();
    '),
    \Throwable::class,
    'runaway re-entrancy is bounded by the depth guard'
);

// Engine still usable after the guard tripped.
eq(3, $js->eval('1 + 2'), 'engine usable after depth guard');

done();
