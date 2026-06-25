<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();
$js->register('db.query', fn(int $h, string $sql) => [], types: 'query(handle: number, sql: string): unknown[]');
$js->register('db.execute', fn(int $h, string $sql) => 0);
$js->register('log.info', fn(string $m) => null, types: 'info(msg: string): void');

// --- manifest() is the single source of truth ----------------------------
$m = $js->manifest();
eq(3, count($m), 'manifest lists every registration');
eq('db.query', $m[0]['name'], 'manifest preserves dotted name');
eq('query(handle: number, sql: string): unknown[]', $m[0]['types'], 'manifest carries the signature');
eq(null, $m[1]['types'], 'untyped registration has null signature');

// --- dts() is generated from the same manifest ---------------------------
$dts = $js->dts();
ok(str_contains($dts, 'declare const php:'), 'dts declares the php global');
ok(str_contains($dts, 'db: {'), 'dts nests dotted names into namespaces');
ok(str_contains($dts, 'query(handle: number, sql: string): unknown[];'), 'typed signature emitted verbatim');
ok(str_contains($dts, 'execute(...args: any[]): any;'), 'untyped leaf falls back to any');
ok(str_contains($dts, 'info(msg: string): void;'), 'second namespace emitted');

// Types and runtime cannot drift: every manifest name is reachable in JS.
foreach ($m as $entry) {
    $name = $entry['name'];
    $reachable = $js->eval("(() => {
        const parts = " . json_encode(explode('.', $name)) . ";
        let node = php;
        for (const p of parts) { if (!node) return false; node = node[p]; }
        return typeof node === 'function';
    })()");
    ok($reachable, "manifest name '$name' is a callable on the php facade");
}

done();
