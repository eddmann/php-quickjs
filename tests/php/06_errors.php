<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();

// --- Exception class hierarchy ------------------------------------------
ok(class_exists('QuickJSException'), 'QuickJSException exists');
ok(is_subclass_of('QuickJSEvalException', 'QuickJSException'), 'Eval extends base');
ok(is_subclass_of('QuickJSException', 'Exception'), 'base extends \\Exception');
ok(is_subclass_of('QuickJSTimeoutException', 'QuickJSException'), 'Timeout extends base');
ok(is_subclass_of('QuickJSMemoryException', 'QuickJSException'), 'Memory extends base');

// --- JS throw -> typed PHP exception ------------------------------------
throws(fn() => $js->eval('throw new Error("boom")'), 'QuickJSEvalException', 'JS Error -> QuickJSEvalException');
throws(fn() => $js->eval('null.field'), 'QuickJSException', 'JS runtime error is a QuickJSException');
throws(fn() => $js->eval('this is not valid js'), 'QuickJSException', 'JS syntax error is a QuickJSException');

// The PHP message starts with the JS error name + message (no redundant
// prefix); a remapped TS location is appended (see 09_typescript.php).
try {
    $js->eval('throw new TypeError("bad arg")');
} catch (QuickJSException $e) {
    ok(str_starts_with($e->getMessage(), 'TypeError: bad arg'), 'typed JS error message preserved');
}
try {
    $js->eval('throw new Error("plain")');
} catch (QuickJSException $e) {
    ok(str_starts_with($e->getMessage(), 'plain'), 'bare Error name elided from message');
}

// --- PHP throw inside a callback -> JS -----------------------------------
$js->register('boom', fn() => throw new \RuntimeException('kaboom'));

// JS can catch it, and inspect the originating PHP class.
eq('kaboom', $js->eval('(() => {
    try { php.boom(); return "unreached"; }
    catch (e) { return e.message; }
})()'), 'PHP exception is catchable in JS');
eq('RuntimeException', $js->eval('(() => {
    try { php.boom(); } catch (e) { return e.phpClass; }
})()'), 'PHP exception class exposed to JS as e.phpClass');

// If JS does NOT catch it, it re-surfaces to PHP with the message intact.
try {
    $js->eval('php.boom()');
} catch (QuickJSException $e) {
    ok(str_starts_with($e->getMessage(), 'RuntimeException: kaboom'), 'uncaught PHP exception re-surfaces cleanly');
}

// --- PHP exception thrown through a JS callback (Direction 2) -------------
$js->register('apply', fn(callable $fn) => $fn());
throws(
    fn() => $js->eval('php.apply(() => { throw new RangeError("oops"); })'),
    'QuickJSException',
    'JS error inside a PHP-invoked callback surfaces to PHP'
);

// --- Engine remains usable after errors ---------------------------------
eq(3, $js->eval('1 + 2'), 'engine still works after exceptions');

done();
