<?php

declare(strict_types=1);

// A minimal Bref function handler that runs a JS/TS guest inside Lambda.
//
//   composer require bref/bref
//
// The `QuickJS` class comes from the php-quickjs extension, enabled by the
// Lambda layer or baked into the Docker image (see this folder's README).

require __DIR__ . '/vendor/autoload.php';

return function (array $event): array {
    $js = new QuickJS(memoryLimit: 32 * 1024 * 1024, timeoutMs: 1000);

    // Expose a tiny, controlled capability to the guest.
    $js->register('event.name', fn(): string => (string) ($event['name'] ?? 'world'));

    // The guest is TypeScript; types are erased in-process before it runs.
    $message = $js->eval(<<<'TS'
        const name: string = php.event.name();
        `Hello, ${name}! 2 ** 10 = ${2 ** 10}`;
    TS);

    return ['message' => $message];
};
