# Installation

A PHP extension binary is tied to an exact combination of **OS · CPU arch · PHP
minor version · thread-safety**. Pick the artifact that matches your target, or
build from source.

Releases attach these artifacts (per PHP 8.4 / 8.5, NTS):

| Artifact | For |
|----------|-----|
| `php-quickjs-vX-php8.4-linux-x86_64.so` / `-aarch64.so` | self-hosted Linux / Docker (glibc ≥ 2.35) |
| `php-quickjs-vX-php8.4-lambda-bref-x86_64.zip` / `-arm64.zip` | AWS Lambda via [Bref](https://bref.sh) (a ready Lambda layer) |
| `php-quickjs-vX-php8.4-lambda-bref-*.so` | Lambda/Amazon Linux 2023, if you prefer the raw `.so` (glibc ≥ 2.34) |
| `php-quickjs-vX-php8.4-macos-arm64.dylib` | local development on macOS (Apple Silicon) |

> The Lambda build is made **inside the Bref Amazon Linux 2023 image** so it links
> against glibc 2.34 and loads on Lambda; a binary built on Ubuntu links against a
> newer glibc and will fail to load there.

## Self-hosted (Linux / macOS / Docker)

Download the `.so`/`.dylib` matching your PHP version and arch, then enable it:

```ini
; php.ini  (find it with: php --ini)
extension=/path/to/php-quickjs-vX-php8.4-linux-x86_64.so
```

Verify:

```sh
php -d extension=/path/to/...so -r 'var_dump(class_exists("QuickJS"));'
# bool(true)
```

In Docker, copy the `.so` into the image and add the `extension=` line to a
`conf.d` ini:

```dockerfile
COPY php-quickjs-vX-php8.4-linux-x86_64.so /usr/local/lib/php/quickjs.so
RUN echo 'extension=/usr/local/lib/php/quickjs.so' > /usr/local/etc/php/conf.d/quickjs.ini
```

## AWS Lambda (Bref)

A Bref function is the runtime layer mounted at `/opt` plus your code at
`/var/task`. PHP scans `*.ini` from `/opt/bref/etc/php/conf.d/` (layers) and your
project's `php/conf.d/`; Bref's `extension_dir` is `/opt/bref/extensions`. The
release artifacts follow that convention, so the **same `.so` + ini work whether
you go via a layer or a Docker image**.

Match the **architecture** (`arm64` for Graviton, `x86_64` otherwise) and the
**PHP version** to your Bref runtime. There's a runnable skeleton (handler +
`Dockerfile` + `serverless.yml`) in [`examples/lambda/`](../examples/lambda).

### Docker image (recommended for custom binaries)

Bake the released `.so` into a `FROM bref/php-XX:3` image:

```dockerfile
FROM bref/php-84:3
COPY php-quickjs-vX-php8.4-lambda-bref-arm64.so /opt/bref/extensions/quickjs.so
RUN echo 'extension=quickjs.so' > /opt/bref/etc/php/conf.d/ext-quickjs.ini
COPY . /var/task
```

Deploy it as a container image (`provider.ecr.images` + `functions.*.image` in
`serverless.yml` — see the example).

### Lambda layer (the `lambda-bref-*.zip`)

The zip is a ready layer: it contains `bref/extensions/quickjs.so` and
`bref/etc/php/conf.d/ext-quickjs.ini` (which simply says `extension=quickjs.so`).
Publish it, then reference its ARN alongside the Bref runtime:

```sh
aws lambda publish-layer-version \
  --layer-name php-quickjs-php84-arm64 \
  --compatible-architectures arm64 \
  --zip-file fileb://php-quickjs-vX-php8.4-lambda-bref-arm64.zip
```

```yaml
functions:
  api:
    handler: index.php
    runtime: php-84              # Bref PHP 8.4 runtime
    architecture: arm64
    layers:
      - arn:aws:lambda:<region>:<account>:layer:php-quickjs-php84-arm64:<n>
```

AWS allows at most **5 layers** per function.

### Vendor the raw `.so`

Skip layers entirely — drop the `.so` in your project and enable it via Bref's
per-project config:

```ini
; php/conf.d/php.ini
extension=/var/task/quickjs.so
```

## Build from source

Requires Rust 1.96+, clang, and PHP 8.4/8.5 dev headers (`php-config`).

```sh
git clone https://github.com/eddmann/php-quickjs && cd php-quickjs
make release      # -> target/release/libphp_quickjs.so (or .dylib on macOS)
make test         # optional: Rust unit tests + PHP suite
```

To build a Lambda-compatible binary locally, build inside the Bref image so it
links against Amazon Linux's glibc. Use `bref/build-php-8x` for x86_64 and
`bref/arm-build-php-8x` for arm64 (and the matching PHP version):

```sh
# The Bref build image exports LD_LIBRARY_PATH/LD_PRELOAD to its /opt libs,
# which makes dnf (Python) segfault; strip them for the dnf call only.
docker run --rm --security-opt seccomp=unconfined \
  -v "$PWD":/src --entrypoint /bin/bash bref/build-php-84 -lc '
  env -u LD_LIBRARY_PATH -u LD_PRELOAD dnf install -y clang clang-devel
  curl --proto "=https" -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  export PATH="$HOME/.cargo/bin:/opt/bin:$PATH" CARGO_TARGET_DIR=/tmp/target
  export LIBCLANG_PATH=/usr/lib64
  # The minimal image has no `which`; point ext-php-rs at PHP directly.
  export PHP=/opt/bin/php PHP_CONFIG=/opt/bin/php-config
  cd /src && cargo build --release
  cp /tmp/target/release/libphp_quickjs.so /src/quickjs.so
'
```

## Choosing the right binary

- **PHP version** must match exactly (an 8.4 extension won't load in 8.5).
- **Architecture** must match (`x86_64` vs `arm64`/`aarch64`).
- **glibc**: the Lambda/Bref build (glibc 2.34) is the most portable on Linux; the
  generic Linux build (glibc 2.35) needs a reasonably recent distro.
- All builds are **NTS** (non-thread-safe) — what CLI, FPM, and Bref use. The
  design assumes a single thread; do not use under a ZTS SAPI.
