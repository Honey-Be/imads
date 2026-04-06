# ──────────────────────────────────────────────
# IMADS — top-level Makefile
# ──────────────────────────────────────────────

# Paths / tools
CBINDGEN   ?= $(HOME)/.cargo/bin/cbindgen
JNI_JAVA   := imads-jni/java/src/main/java/io/imads
JNI_CLS    := imads-jni/java/target
WASM_NPM   := imads-wasm/npm

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

.PHONY: build-core build-ffi build-jni build-py build-wasm build-all

build-core:
	cargo build -p imads-core --release

build-ffi: build-core
	cargo build -p imads-ffi --release

build-jni: build-core
	cargo build -p imads-jni --release

build-py: build-core
	cargo build -p imads-py --release

build-wasm: build-core
	cargo build -p imads-wasm --release

build-all: build-ffi build-jni build-py build-wasm

# ──── FFI artifacts ──────────────────────────

.PHONY: cbindgen java-bridge py-wheel py-develop \
        wasm-bundler wasm-web wasm-nodejs wasm-npm ffi-all

## Regenerate C header from imads-ffi
cbindgen: build-ffi
	$(CBINDGEN) --config imads-ffi/cbindgen.toml --crate imads-ffi \
		--output imads-ffi/include/imads.h

## Compile JNI Java bridge classes
java-bridge: build-jni
	mkdir -p $(JNI_CLS)
	javac -d $(JNI_CLS) $(JNI_JAVA)/*.java

## Build Python wheel (CPython, via maturin)
py-wheel:
	cd imads-py && maturin build --release

## Build Python dev install (CPython, via maturin)
py-develop:
	cd imads-py && maturin develop --release

## ---- WASM targets ----

## Build WASM for bundlers (Webpack 5+, Vite, Rollup+plugin).
## Primary target for Kotlin/JS, Scala.js, ClojureScript, and TS/JS with bundlers.
## No init() required — bundler handles .wasm loading.
wasm-bundler:
	cd imads-wasm && wasm-pack build --target bundler --release \
		--out-dir $(CURDIR)/$(WASM_NPM)/bundler --out-name imads_wasm
	rm -f $(WASM_NPM)/bundler/package.json $(WASM_NPM)/bundler/.gitignore

## Build WASM for direct browser use (ESM, requires await init()).
## For <script type="module"> without a bundler.
wasm-web:
	cd imads-wasm && wasm-pack build --target web --release \
		--out-dir $(CURDIR)/$(WASM_NPM)/web --out-name imads_wasm
	rm -f $(WASM_NPM)/web/package.json $(WASM_NPM)/web/.gitignore

## Build WASM for Node.js (CommonJS, synchronous).
## For Node.js without a bundler.
wasm-nodejs:
	cd imads-wasm && wasm-pack build --target nodejs --release \
		--out-dir $(CURDIR)/$(WASM_NPM)/nodejs --out-name imads_wasm
	rm -f $(WASM_NPM)/nodejs/package.json $(WASM_NPM)/nodejs/.gitignore

## Build all WASM targets and assemble the npm package.
wasm-npm: wasm-bundler wasm-web wasm-nodejs

## All FFI artifacts
ffi-all: cbindgen java-bridge wasm-npm

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
	rm -rf $(JNI_CLS)
	rm -rf $(WASM_NPM)/bundler $(WASM_NPM)/web $(WASM_NPM)/nodejs
	rm -rf imads-wasm/pkg
