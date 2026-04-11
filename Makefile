# ──────────────────────────────────────────────
# IMADS — top-level Makefile
# ──────────────────────────────────────────────

# Paths / tools
CBINDGEN   ?= $(HOME)/.cargo/bin/cbindgen
JVM_KOTLIN := imads-jvm/kotlin
WASM_OUT   := imads-wasm/pkg

# ──── Core (Rust workspace) ──────────────────

.PHONY: test clippy fmt fmt-check doc

test:
	cargo test --workspace

clippy: test
	cargo clippy --workspace --all-targets -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

doc:
	cargo doc --no-deps --workspace

# ──── Release builds ─────────────────────────

.PHONY: build-core build-ffi build-jvm build-py build-wasm build-all

build-core:
	cargo build -p imads-core --release

build-ffi: build-core
	cargo build -p imads-ffi --release

build-jvm: build-core
	cargo build -p imads-jvm --release

build-py: build-core
	cargo build -p imads-py --release

build-wasm: build-core
	cargo component build -p imads-wasm --release

build-all: build-ffi build-jvm build-py build-wasm

# ──── FFI artifacts ──────────────────────────

.PHONY: cbindgen py-wheel py-develop \
        wasm-component wasm-transpile ffi-all

## Regenerate C header from imads-ffi
cbindgen: build-ffi
	$(CBINDGEN) --config imads-ffi/cbindgen.toml --crate imads-ffi \
		--output imads-ffi/include/imads.h

## Build Python wheel (CPython, via maturin)
py-wheel:
	cd imads-py && maturin build --release

## Build Python dev install (CPython, via maturin)
py-develop:
	cd imads-py && maturin develop --release

## ---- WASM targets (Component Model) ----

## Build WASM component (wasm32-wasip2)
wasm-component: build-wasm

## Transpile WASM component to ESM for Node.js/Deno/Bun
wasm-transpile: wasm-component
	mkdir -p $(WASM_OUT)/esm
	jco transpile target/wasm32-wasip2/release/imads_wasm.wasm \
		-o $(WASM_OUT)/esm

## All FFI artifacts
ffi-all: cbindgen build-jvm wasm-transpile

# ──── JVM (FFM, JDK 22+) ────────────────────

.PHONY: jvm-kotlin

## Build Kotlin JVM wrapper (requires JDK 22+)
jvm-kotlin: build-jvm
	cd $(JVM_KOTLIN) && ./gradlew build

# ──── Benchmarks ─────────────────────────────

.PHONY: preset-compare preset-report

preset-compare: test
	mkdir -p reports
	cargo bench -p imads-core --bench preset_compare > reports/preset_compare.csv

preset-report: test
	mkdir -p reports
	cargo run -p imads-core --release --example preset_report > reports/preset_report.csv

# ──── Housekeeping ───────────────────────────

.PHONY: clean clean-all

## Remove reports + Rust target
clean:
	rm -rf reports
	cargo clean

## Remove everything including generated FFI artifacts
clean-all: clean
	rm -rf $(WASM_OUT)
	rm -rf imads-ffi/include/imads.h
