<?php

declare(strict_types=1);

// A tour of every feature of the php-quickjs extension.
//
//   make build   # or: cargo build
//   php -d extension=$(pwd)/target/debug/libphp_quickjs.so examples/kitchen_sink.php

function section(string $title): void
{
    echo "\n== $title ==\n";
}

// ── 1. Construction with sandbox limits ─────────────────────────────────
section('1. Construction with sandbox limits');
$js = new QuickJS(memoryLimit: 64 * 1024 * 1024, timeoutMs: 1000);
echo 'created: ', get_class($js), "\n";

// ── 2. register() — expose PHP capabilities under flat, dotted names ─────
// The optional `types` string is surfaced by dts() (see section 8). The flat
// dotted-name registry is the entire trust boundary.
section('2. register() — expose PHP capabilities');
$js->register('log.info', fn(string $m) => print("   [js] $m\n"), types: 'info(msg: string): void');
$js->register('math.add', fn(int $a, int $b) => $a + $b, types: 'add(a: number, b: number): number');
$js->register('users.find', fn(int $id) => ['id' => $id, 'name' => 'Ada', 'roles' => ['admin', 'dev']]);
echo 'registered: ', implode(', ', array_column($js->manifest(), 'name')), "\n";

// ── 3. eval() runs TypeScript — types are erased, the rest runs as JS ────
section('3. eval() runs TypeScript');
$out = $js->eval(<<<'TS'
    interface User { id: number; name: string; roles: string[]; }
    const u: User = php.users.find(42);            // reenters PHP
    php.log.info(`loaded ${u.name}`);
    const sum: number = php.math.add(u.id, u.roles.length);
    `${u.name} #${u.id} has ${u.roles.length} roles, sum=${sum}`;
TS);
echo "result: $out\n";

// ── 4. Capability handles — live objects stay host-side, JS gets an int ──
section('4. Capability handles');
class Counter
{
    private int $n = 0;
    public function bump(int $by = 1): int
    {
        return $this->n += $by;
    }
}
$counter = new Counter();
$handle = $js->grant($counter);                    // -> opaque int
$js->register('counter.bump', fn(int $h, int $by) => $js->resolve($h)->bump($by));
echo "handle id: $handle (opaque in JS)\n";
echo 'JS drives the live object: ', $js->eval("php.counter.bump($handle, 5); php.counter.bump($handle, 3);"), "\n";
echo 'PHP sees the same object:  ', $counter->bump(0), "\n";

// ── 5. Bidirectional functions ──────────────────────────────────────────
section('5. Bidirectional functions');
// 5a. A PHP capability returns a closure; JS calls it.
$js->register('makeAdder', fn(int $n) => fn(int $x) => $x + $n);
echo '5a PHP closure -> JS:    ', $js->eval('const add10 = php.makeAdder(10); add10(5);'), "\n";
// 5b. JS passes a function to PHP (arrives as Js\Callback); PHP invokes it.
$js->register('mapEach', fn(array $xs, callable $fn) => array_map($fn, $xs));
echo '5b JS fn -> PHP:         ', json_encode($js->eval('php.mapEach([1,2,3], (n: number) => n * n)')), "\n";
// 5c. A JS function stored in PHP, invoked across later evals.
$handler = null;
$js->register('onEvent', function (callable $cb) use (&$handler) {
    $handler = $cb;
});
$js->eval('php.onEvent((name: string) => `handled:${name}`)');
$js->eval('1 + 1');                                // a later, unrelated eval
echo '5c stored callback:      ', $handler('click'), ' (', get_class($handler), ")\n";

// ── 6. Errors — typed, TS-located, structured ───────────────────────────
section('6. Errors');
// 6a. A JS runtime error, remapped to the original TS line.
try {
    $js->eval("const a: number = 1;\nconst o: any = null;\no.field;");
} catch (QuickJSEvalException $e) {
    printf("6a %s | %s | %s:%d | jsName=%s\n", get_class($e), $e->getMessage(), $e->getFile(), $e->getLine(), $e->getJsName());
}
// 6b. A PHP exception thrown inside a callback, caught in JS with e.phpClass.
$js->register('risky', fn() => throw new \RuntimeException('db down'));
echo '6b PHP error caught in JS: ', $js->eval('(() => { try { php.risky(); } catch (e) { return e.message + " [" + e.phpClass + "]"; } })()'), "\n";
// 6c. A syntax error, located at its TS line.
try {
    $js->eval("const a = 1;\nconst = ;");
} catch (QuickJSEvalException $e) {
    printf("6c %s:%d jsName=%s msg=%s\n", $e->getFile(), $e->getLine(), $e->getJsName(), $e->getMessage());
}

// ── 7. Sandbox limits enforce, engine recovers ──────────────────────────
section('7. Sandbox limits');
$guarded = new QuickJS(timeoutMs: 100, memoryLimit: 4 * 1024 * 1024);
try {
    $guarded->eval('while (true) {}');
} catch (QuickJSTimeoutException $e) {
    echo '7a timeout: ', get_class($e), "\n";
}
try {
    $guarded->eval('const a: any[] = []; while (true) { a.push(new Array(50000).fill(0)); }');
} catch (QuickJSMemoryException $e) {
    echo '7b memory:  ', get_class($e), "\n";
}
echo '7c recovers: ', $guarded->eval('1 + 2'), "\n";

// ── 8. dts() — TypeScript declarations generated from the manifest ──────
section('8. dts() — generated TypeScript declarations');
echo $js->dts();
