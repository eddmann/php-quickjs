<?php

declare(strict_types=1);

require __DIR__ . '/_harness.php';

// roundtrip() pushes a PHP value through the full pipeline:
//   PHP zval -> MiddleValue -> JS Value -> MiddleValue -> PHP zval
$js = new QuickJS();

eq(null, $js->roundtrip(null), 'null');
eq(true, $js->roundtrip(true), 'bool true');
eq(false, $js->roundtrip(false), 'bool false');
eq(0, $js->roundtrip(0), 'int zero');
eq(-123, $js->roundtrip(-123), 'negative int');
eq(1 << 40, $js->roundtrip(1 << 40), 'large int (beyond i32)');
eq(3.5, $js->roundtrip(3.5), 'float');
eq('', $js->roundtrip(''), 'empty string');
eq('héllo', $js->roundtrip('héllo'), 'utf-8 string');

eq([1, 2, 3], $js->roundtrip([1, 2, 3]), 'indexed array');
eq(['a' => 1, 'b' => 'two'], $js->roundtrip(['a' => 1, 'b' => 'two']), 'assoc array');
eq(
    ['x' => [1, 2], 'y' => ['z' => true]],
    $js->roundtrip(['x' => [1, 2], 'y' => ['z' => true]]),
    'nested structure'
);

// Empty array stays an (empty) array.
eq([], $js->roundtrip([]), 'empty array');

// Binary bytes survive via Uint8Array <-> binary string.
$bytes = "\x00\x01\x02\xff";
eq($bytes, $js->eval('new Uint8Array([0,1,2,255])'), 'Uint8Array -> binary string');

done();
