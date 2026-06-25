<?php
// Minimal test harness: assertion helpers shared by the PHP test scripts.
// Each test script exits non-zero on the first failure so `make test` can
// detect failures by exit code.

declare(strict_types=1);

$GLOBALS['__tests'] = 0;
$GLOBALS['__fails'] = 0;

function ok(bool $cond, string $msg): void
{
    $GLOBALS['__tests']++;
    if ($cond) {
        echo "  ok   - $msg\n";
    } else {
        $GLOBALS['__fails']++;
        echo "  FAIL - $msg\n";
    }
}

function eq($expected, $actual, string $msg): void
{
    $cond = $expected === $actual;
    if (!$cond) {
        $msg .= sprintf(' (expected %s, got %s)', var_export($expected, true), var_export($actual, true));
    }
    ok($cond, $msg);
}

function throws(callable $fn, string $class, string $msg): void
{
    try {
        $fn();
        ok(false, "$msg (no exception thrown)");
    } catch (\Throwable $e) {
        ok($e instanceof $class, "$msg (got " . get_class($e) . ': ' . $e->getMessage() . ')');
    }
}

function done(): void
{
    $t = $GLOBALS['__tests'];
    $f = $GLOBALS['__fails'];
    echo sprintf("\n%d assertions, %d failures\n", $t, $f);
    exit($f === 0 ? 0 : 1);
}
