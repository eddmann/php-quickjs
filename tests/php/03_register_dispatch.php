<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();

// Flat, dotted registration -> namespaced `php.*` facade in JS.
$log = [];
$js->register('log.info', function (string $msg) use (&$log) {
    $log[] = $msg;
    return null;
});
$js->register('math.add', fn(int $a, int $b) => $a + $b);
$js->register('fetchUser', fn(int $id) => ['id' => $id, 'name' => 'Ada', 'orders' => [1, 2, 3]]);
$js->register('echo', fn($x) => $x);

// JS -> PHP -> JS through the msgpack __host bridge.
eq(7, $js->eval('php.math.add(3, 4)'), 'math.add reenters PHP');
eq('Ada has 3 orders', $js->eval('
    const u = php.fetchUser(42);
    `${u.name} has ${u.orders.length} orders`;
'), 'object result from PHP consumed in JS');

$js->eval('php.log.info("hello from js")');
eq(['hello from js'], $log, 'side-effecting callback ran in PHP');

// Every marshalable type survives the msgpack round-trip JS->PHP->JS.
eq(true, $js->eval('php.echo(true)'), 'bool through bridge');
eq(-12345, $js->eval('php.echo(-12345)'), 'negative int through bridge');
eq(1 << 40, $js->eval('php.echo(' . (1 << 40) . ')'), 'large int through bridge');
eq(3.5, $js->eval('php.echo(3.5)'), 'float through bridge');
eq('héllo', $js->eval('php.echo("héllo")'), 'utf-8 string through bridge');
eq([1, 2, 3], $js->eval('php.echo([1,2,3])'), 'array through bridge');
eq(['a' => 1, 'b' => [2, 3]], $js->eval('php.echo({a:1, b:[2,3]})'), 'nested object through bridge');
eq(null, $js->eval('php.echo(null)'), 'null through bridge');
eq("\x00\xff", $js->eval('php.echo(new Uint8Array([0,255]))'), 'bytes through bridge');

// The facade is frozen: guests cannot shadow capabilities.
eq(true, $js->eval('Object.isFrozen(php)'), 'php is frozen');
eq(true, $js->eval('Object.isFrozen(php.math)'), 'namespace is frozen');
$js->eval('try { php.math.add = () => 999; } catch (e) {}');
eq(7, $js->eval('php.math.add(3, 4)'), 'frozen capability cannot be overwritten');

// Unknown capabilities are rejected at the trust boundary.
ok(
    (bool) $js->eval('(() => { try { php.echo; return true; } catch(e){ return false; } })()'),
    'registered capability is reachable'
);
$threw = $js->eval('(() => { try { __host("not.registered", __mp.encode([])); return false; } catch(e){ return true; } })()');
eq(true, $threw, 'unknown capability throws in JS');

done();
