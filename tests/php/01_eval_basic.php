<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

$js = new QuickJS();

eq(2, $js->eval('1 + 1'), 'integer arithmetic');
eq(42, $js->eval('let x = 40; x + 2'), 'statements then expression');
eq('hello world', $js->eval('"hello" + " " + "world"'), 'string concat');
eq(true, $js->eval('3 > 2'), 'boolean');
eq(2.5, $js->eval('5 / 2'), 'float division');
eq(null, $js->eval('null'), 'null');
eq(null, $js->eval('undefined'), 'undefined -> null');
eq([1, 2, 3], $js->eval('[1, 2, 3]'), 'array');
eq(['a' => 1, 'b' => 2], $js->eval('({a: 1, b: 2})'), 'object -> assoc array');

// Pure JS computation never leaves the cage.
eq([1, 4, 9], $js->eval('[1,2,3].map(n => n*n)'), 'Array.map runs in JS');

done();
