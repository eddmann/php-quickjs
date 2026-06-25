<?php

declare(strict_types=1);

// Run with:
//   php -d extension=target/release/libphp_quickjs.so examples/usage.php

$js = new QuickJS(memoryLimit: 64 * 1024 * 1024, timeoutMs: 1000);

$js->register('log.info', fn(string $m) => fwrite(STDERR, "[js] $m\n"));
$js->register('fetchUser', fn(int $id) => [
    'id' => $id,
    'name' => 'Ada',
    'orders' => [1, 2, 3],
]);

$result = $js->eval(<<<'JS'
    php.log.info("starting");
    const u = php.fetchUser(42);            // reenters PHP
    `${u.name} has ${u.orders.length} orders`;
JS);

echo $result, "\n";   // => "Ada has 3 orders"

// Capability handle for a live, stateful object.
$pdo = new ArrayObject(['rows' => 0]);
$h = $js->grant($pdo);
$js->register('db.insert', fn(int $handle) => ++$js->resolve($handle)['rows']);
echo $js->eval("php.db.insert($h); php.db.insert($h);"), "\n";  // => 2

// Bidirectional functions: PHP higher-order capability driving a JS callback.
$js->register('map', fn(array $xs, callable $fn) => array_map($fn, $xs));
print_r($js->eval('php.map([1, 2, 3], n => n * n)'));            // => [1, 4, 9]
