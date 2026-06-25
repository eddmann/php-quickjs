<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

// --- Wall-clock timeout --------------------------------------------------
$js = new QuickJS(timeoutMs: 100);

$t0 = microtime(true);
throws(fn() => $js->eval('while (true) {}'), 'QuickJSTimeoutException', 'infinite loop trips the timeout');
$elapsed = (microtime(true) - $t0) * 1000;
ok($elapsed < 1000, "timeout fired promptly ({$elapsed}ms < 1000ms)");

// The timeout is a QuickJSException too (catchable by the base type).
throws(fn() => $js->eval('for (;;) {}'), 'QuickJSException', 'timeout is a QuickJSException');

// Engine recovers and is usable after a timeout.
eq(4, $js->eval('2 + 2'), 'engine usable after timeout');

// A quick eval well within budget is unaffected.
eq(499500, $js->eval('let s=0; for (let i=0;i<1000;i++) s+=i; s'), 'fast eval is not interrupted');

// A long-running host-callback loop is also bounded by the deadline.
$js->register('noop', fn() => null);
throws(fn() => $js->eval('while (true) { php.noop(); }'), 'QuickJSTimeoutException', 'host-call loop is bounded too');
eq(1, $js->eval('1'), 'engine usable after host-call timeout');

// --- Memory limit --------------------------------------------------------
$mem = new QuickJS(memoryLimit: 2 * 1024 * 1024);
throws(
    fn() => $mem->eval('let a = []; while (true) { a.push(new Array(100000).fill(0)); }'),
    'QuickJSMemoryException',
    'alloc bomb trips the memory limit'
);

// --- Unbounded by default ------------------------------------------------
$free = new QuickJS();
eq(500500, $free->eval('let s=0; for (let i=0;i<=1000;i++) s+=i; s'), 'no limits by default');

done();
