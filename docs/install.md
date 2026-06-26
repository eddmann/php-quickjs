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
| `php-quickjs-vX-php8.4-macos-arm64.dylib` / `-x86_64.dylib` | local development on macOS |

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

The `lambda-bref-*.zip` is a **Lambda layer**: it contains the extension plus a
`conf.d` ini that enables it. Add it as a layer alongside the Bref runtime layer
and the extension loads automatically.

In `serverless.yml`:

```yaml
functions:
  api:
    handler: index.php
    runtime: php-84              # Bref PHP 8.4 runtime
    layers:
      - arn:aws:lambda:<region>:<account>:layer:php-quickjs-php84-arm64:<n>
```

To create the layer from the released zip:

```sh
aws lambda publish-layer-version \
  --layer-name php-quickjs-php84-arm64 \
  --compatible-architectures arm64 \
  --zip-file fileb://php-quickjs-vX-php8.4-lambda-bref-arm64.zip
```

Match the **architecture** (`arm64` for Graviton functions, `x86_64` otherwise)
and the **PHP version** to your Bref runtime. The layer's internal layout
(`/opt/php-quickjs/quickjs.so` + `/opt/bref/etc/php/conf.d/quickjs.ini`) follows
Bref's ini scan path; if a future Bref changes that path, prefer the raw `.so`
with your own `php/conf.d` entry in the project.

Alternatively, skip the layer and vendor the raw `.so` in your project, enabling
it via Bref's per-project config (`php/conf.d/php.ini`):

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
# --security-opt seccomp=unconfined: AL2023's dnf segfaults under Docker's
# default seccomp profile.
docker run --rm --security-opt seccomp=unconfined \
  -v "$PWD":/src --entrypoint /bin/bash bref/build-php-84 -lc '
  dnf install -y clang clang-devel
  curl --proto "=https" -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
  export PATH="$HOME/.cargo/bin:/opt/bin:$PATH" CARGO_TARGET_DIR=/tmp/target
  export LIBCLANG_PATH=/usr/lib64
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
