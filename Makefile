# php-quickjs — build & test
#
# The extension is a plain cargo cdylib (no phpize). Load the built .so by
# absolute path with `php -d extension=...`.

PROFILE ?= debug
ifeq ($(PROFILE),release)
CARGO_FLAGS := --release
else
CARGO_FLAGS :=
endif

EXT := $(CURDIR)/target/$(PROFILE)/libphp_quickjs.so
PHP := php -d extension=$(EXT)

.PHONY: all build release test test-rust test-php stubs example clean fmt

all: build

build:
	cargo build $(CARGO_FLAGS)

release:
	$(MAKE) build PROFILE=release

# Rust unit tests (marshaling, manifest, facade) + the PHP integration suite.
test: build test-rust test-php

test-rust:
	cargo test --lib

test-php: build
	@fail=0; \
	for t in tests/php/0*.php; do \
	  printf '\n=== %s ===\n' "$$t"; \
	  $(PHP) "$$t" || fail=1; \
	done; \
	exit $$fail

# Regenerate the IDE stub for the PHP-facing classes (requires cargo-php:
#   cargo install cargo-php).
stubs:
	cargo php stubs --stdout > stubs/php_quickjs.stubs.php || \
	  echo "cargo-php not installed; run 'cargo install cargo-php'"

example: build
	$(PHP) examples/usage.php

fmt:
	cargo fmt

clean:
	cargo clean
