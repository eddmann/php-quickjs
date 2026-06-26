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

Bake the `.so` into a Bref runtime image (see [`Dockerfile`](Dockerfile)):

```dockerfile
ARG BREF_IMAGE=bref/arm-php-84:3   # arm64 default; bref/php-84:3 for x86_64
FROM ${BREF_IMAGE}
ARG EXT
COPY ${EXT} /opt/bref/extensions/quickjs.so
RUN echo 'extension=quickjs.so' > /opt/bref/etc/php/conf.d/ext-quickjs.ini
COPY . /var/task
```

> **Match the base image to the arch.** Bref ships a separate runtime per
> architecture — `bref/arm-php-84` for arm64 (Graviton) and `bref/php-84` for
> x86_64 — so the base image, the `--platform`, and the `.so` arch must all
> agree. Copying an arm64 `.so` into an x86_64 base (or vice-versa) builds an
> image that won't load the extension on Lambda.

```sh
composer require bref/bref
serverless deploy        # builds the image (buildArg EXT=...), pushes to ECR, deploys
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
  --zip-file fileb://php-quickjs-v0.0.2-php8.4-lambda-bref-arm64.zip
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

## Test locally

You can exercise the whole thing on your machine before deploying — Bref's
runtime images bundle the AWS Runtime Interface Emulator (RIE). Use the image and
`.so` that match your host arch (`bref/arm-php-84:3` + `arm64.so` on Apple
Silicon; `bref/php-84:3` + `x86_64.so` on Intel/`--platform linux/amd64`).

**Quick smoke test** — does the extension load and run a guest?

```sh
docker run --rm \
  -v "$PWD/quickjs.so":/opt/bref/extensions/quickjs.so:ro \
  --entrypoint /bin/sh bref/arm-php-84:3 -c '
    echo "extension=quickjs.so" > /opt/bref/etc/php/conf.d/ext-quickjs.ini
    /opt/bin/php -r "var_dump(class_exists(\"QuickJS\"));"          # bool(true)
    /opt/bin/php -r "echo (new QuickJS())->eval(\"41+1\"), PHP_EOL;" # 42
  '
```

**Full Lambda invoke** — build the image and call it through the RIE:

```sh
composer require bref/bref           # the handler needs vendor/autoload.php
cp /path/to/quickjs.so ./quickjs.so  # into the build context
docker build --build-arg EXT=quickjs.so -t php-quickjs-demo .
docker run --rm -e BREF_RUNTIME=function -p 9000:8080 php-quickjs-demo index.php
# in another shell:
curl -s "http://localhost:9000/2015-03-31/functions/function/invocations" \
  -d '{"name":"Lambda"}'
# => {"message":"Hello, Lambda! 2 ** 10 = 1024"}
```

> **`BREF_RUNTIME=function` is required locally.** Bref's deploy tooling injects
> it for you on real Lambda, but a bare `docker run` of the image fatals with
> *"The environment variable `BREF_RUNTIME` is not set"* until you pass it.

---

See [`../../docs/install.md`](../../docs/install.md) for the full platform matrix
and how the Lambda binaries are built.
