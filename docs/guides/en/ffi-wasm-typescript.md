# WASM / TypeScript FFI Guide

## WASI Component Model

The `imads-wasm` crate has been rewritten to use the **WASI Component Model** with
`wit-bindgen`. It replaces the previous `wasm-bindgen`-based approach. The WIT
(WebAssembly Interface Type) interface defines the contract between the host and the
component.

### WIT Interface

The WIT interface is defined in `imads-wasm/wit/imads.wit` and exposes:

- Engine creation and lifecycle
- Configuration from presets
- Single-objective and multi-objective `run` functions
- Custom evaluator callbacks

### Building

```bash
# Build the WASI component (requires cargo-component)
cd imads-wasm && cargo component build --release
```

This produces a `.wasm` component in `target/wasm32-wasip2/release/`.

### Generating JavaScript/TypeScript bindings

Use `jco` to transpile the component into JavaScript/TypeScript modules:

```bash
# Install jco (if not already installed)
npm install -g @bytecodealliance/jco

# Transpile to JS/TS
jco transpile target/wasm32-wasip2/release/imads_wasm.wasm \
    --out-dir npm/dist \
    --map 'imads:core/*=./imads-*.js'
```

This generates:
- `npm/dist/imads_wasm.js` — ES module with all exports
- `npm/dist/imads_wasm.d.ts` — TypeScript type declarations
- `npm/dist/*.wasm` — core module(s)

### npm Package: `imads-wasm`

The transpiled output is distributed as a standard npm package:

```jsonc
{
  "exports": {
    ".": { "import": "./dist/imads_wasm.js", "types": "./dist/imads_wasm.d.ts" }
  }
}
```

## Usage

### With a bundler (Webpack 5+, Vite)

```typescript
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const env = new Env(1, 2, 3, 4);
const output = new Engine().run(cfg, env);
console.log(output.fBest, output.xBest);
```

**Webpack 5** config — enable `asyncWebAssembly`:
```javascript
// webpack.config.js
module.exports = {
  experiments: { asyncWebAssembly: true },
  // ...
};
```

**Vite** — works out of the box with `vite-plugin-wasm` or top-level await:
```javascript
// vite.config.js
import wasm from "vite-plugin-wasm";
export default { plugins: [wasm()] };
```

### Node.js (ESM)

```javascript
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const output = new Engine().run(cfg, new Env(1, 2, 3, 4));
console.log(output.fBest);
```

Node.js 18+ with ESM support is required. The component is loaded via the standard
ESM import.

## Usage from Framework Bindings

| Framework | Import style | Notes |
|-----------|-------------|:-----:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | via bundler |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | via bundler |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | via bundler |
| **TypeScript** (bundler) | `import { ... } from "imads-wasm"` | bundler |

All JS-target framework bindings (Kotlin/JS, Scala.js, CLJS) assume a bundler is present.
Their build tools (Gradle+Webpack, sbt+Scala.js linker, shadow-cljs) act as bundlers.

## Custom Evaluator

```typescript
function mcSample(x: Float64Array, tau: number, k: number): number[] {
    let sumSq = 0;
    for (let i = 0; i < x.length; i++) sumSq += x[i] * x[i];
    return [sumSq, x[0] - 1, x[1] - 2];
}

function cheapConstraints(x: Float64Array): boolean {
    return true;
}

function searchDim(): number | undefined {
    return 3;  // return undefined to let the engine infer from config or incumbent
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints, searchDim);
```

## Building for WASI Targets

```bash
# Component model (recommended)
cargo component build -p imads-wasm --release

# Legacy WASI targets (still supported)
cargo build -p imads-wasm --target wasm32-wasip1 --release          # single-threaded
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # multi-threaded
```

## Threading Model

| Target | Workers | Notes |
|--------|---------|-------|
| `wasm32-wasip2` (component) | N | Component model + threads |
| `wasm32-wasip1` | 1 only | No thread support |
| `wasm32-wasip1-threads` | N | Full thread pool |
| `wasm32-unknown-unknown` (browser) | 1 only | No `std::thread` |

`AdaptiveExecutor` automatically selects the correct mode based on build target.
