# php-quickjs on AWS Lambda (Bref)

Three ways to ship the extension to Lambda, all using the artifacts attached to a
[php-quickjs release](https://github.com/eddmann/php-quickjs/releases). Pick the
`.so` / `.zip` that matches your **PHP minor** (8.4 / 8.5) and **architecture**
(`arm64` for Graviton, `x86_64` otherwise). All builds are NTS, built inside the
Bref Amazon Linux 2023 image so they load on Lambda (glibc 2.34).

This folder contains a runnable skeleton: [`index.php`](index.php) (a Bref function
handler that runs a TS guest), a [`Dockerfile`](Dockerfile), and a
[`serverless.yml`](serverless.yml).

## How Bref loads it

A Bref function is the runtime layer mounted at `/opt` plus your code at
`/var/task`. PHP scans `*.ini` from `/opt/bref/etc/php/conf.d/` (layers) and your
project's `php/conf.d/`. Bref's `extension_dir` is `/opt/bref/extensions`. The
release artifacts follow that convention exactly, so the same `.so` + ini work
whether you go the layer or the Docker route.

## 1. Docker image (recommended for custom binaries)

Bake the `.so` into a `FROM bref/php-XX:3` image (see [`Dockerfile`](Dockerfile)):

```dockerfile
FROM bref/php-84:3
ARG EXT
COPY ${EXT} /opt/bref/extensions/quickjs.so
RUN echo 'extension=quickjs.so' > /opt/bref/etc/php/conf.d/ext-quickjs.ini
COPY . /var/task
```

```sh
composer require bref/bref
serverless deploy        # builds the image (buildArg EXT=...), pushes to ECR, deploys
```

## 2. Lambda layer (the released `.zip`)

Publish the layer once, then reference its ARN alongside the Bref runtime:

```sh
aws lambda publish-layer-version \
  --layer-name php-quickjs-php84-arm64 \
  --compatible-architectures arm64 \
  --zip-file fileb://php-quickjs-v0.0.1-php8.4-lambda-bref-arm64.zip
```

```yaml
functions:
  demo:
    handler: index.php
    runtime: php-84
    architecture: arm64
    layers:
      - arn:aws:lambda:<region>:<account>:layer:php-quickjs-php84-arm64:1
```

(AWS allows at most **5 layers** per function.) See the commented block at the
bottom of [`serverless.yml`](serverless.yml).

## 3. Vendor the raw `.so`

Skip layers entirely: drop the `.so` in your project and enable it via Bref's
per-project config.

```ini
; php/conf.d/quickjs.ini
extension=/var/task/quickjs.so
```

---

See [`../../docs/install.md`](../../docs/install.md) for the full platform matrix
and how the Lambda binaries are built.
