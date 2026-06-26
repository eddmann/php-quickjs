<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();

// --- Type stripping ------------------------------------------------------
eq(42, $js->eval('const x: number = 41; const y = (n: number): number => n + 1; y(x);'),
    'type annotations are stripped and the JS runs');
eq('Ada', $js->eval('type User = { name: string }; const u: User = { name: "Ada" }; u.name;'),
    'type aliases erase, value survives');

// --- Type-only constructs erase cleanly (isolatedModules) ----------------
eq(7, $js->eval('
    interface Shape { sides: number }
    const tri: Shape = { sides: 3 };
    tri.sides + 4;
'), 'interface declaration erases with no runtime trace');

// Generics and `as` casts are erased.
eq(3, $js->eval('function first<T>(xs: T[]): T { return xs[0]; } first<number>([3, 4, 5]);'),
    'generics erase');
eq(5, $js->eval('const v = ("5" as unknown as string); Number(v);'), '`as` casts erase');

// --- Private fields stay native (esnext target, no WeakMap downleveling) --
eq(8, $js->eval('
    class Counter {
        #n = 0;
        bump(by: number): number { return this.#n += by; }
    }
    const c = new Counter();
    c.bump(3);
    c.bump(5);
'), 'class private fields run natively under esnext');

// --- TypeScript reaches the PHP bridge -----------------------------------
$js->register('add', fn(int $a, int $b) => $a + $b);
eq(30, $js->eval('const r: number = php.add(10, 20); r;'), 'typed guest calls a PHP capability');

// --- Error remapping to TS coordinates -----------------------------------
// A fully-erased interface (lines 1-4) shifts the throw: it lands on generated
// JS line 1, but the source map must report the original TS line 5.
try {
    $js->eval("interface Foo {\n  a: number;\n  b: string;\n}\nthrow new Error(\"deep\");");
    ok(false, 'expected throw');
} catch (QuickJSEvalException $e) {
    $m = $e->getMessage();
    ok(str_contains($m, 'deep'), 'error message preserved');
    ok(str_contains($m, 'guest.ts:5'), "error remapped to original TS line 5 (got: " . explode("\n", $m)[0] . ")");
    ok(!str_contains($m, 'guest.ts:1:'), 'not reported at the generated JS line 1');
}

// A runtime TypeError is remapped to the TS line that triggered it.
try {
    $js->eval("const a: number = 1;\nconst obj: any = null;\nobj.field;");
    ok(false, 'expected throw');
} catch (QuickJSEvalException $e) {
    ok(str_contains($e->getMessage(), 'guest.ts:3'), 'runtime error remapped to TS line 3');
}

// --- Transpile / syntax errors ------------------------------------------
throws(fn() => $js->eval('const = ;'), 'QuickJSEvalException', 'invalid syntax surfaces as QuickJSEvalException');
try {
    $js->eval('function ( {');
    ok(false, 'expected throw');
} catch (QuickJSEvalException $e) {
    ok($e->getMessage() !== '', 'transpile error carries a diagnostic message');
}

// --- Cache correctness ---------------------------------------------------
// IIFE-scoped so re-evaluating the same source is idempotent (top-level
// declarations otherwise persist in the shared global scope across evals).
$src = '(() => { const z: number = 99; return z * 2; })();';
eq(198, $js->eval($src), 'first eval (transpile + cache)');
eq(198, $js->eval($src), 'second eval (cache hit) returns identical result');

// --- Plain JS still works through the TS path ---------------------------
eq([1, 4, 9], $js->eval('[1,2,3].map(n => n * n)'), 'plain JS is valid TS and round-trips');

done();
