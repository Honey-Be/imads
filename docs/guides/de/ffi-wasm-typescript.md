# WASM / TypeScript FFI Guide

## WASI Component Model

Das `imads-wasm`-Crate wurde umgeschrieben, um das **WASI Component Model** mit
`wit-bindgen` zu verwenden. Es ersetzt den bisherigen `wasm-bindgen`-basierten Ansatz. Das WIT
(WebAssembly Interface Type) Interface definiert den Vertrag zwischen dem Host und der
Komponente.

### WIT Interface

Das WIT Interface ist in `imads-wasm/wit/imads.wit` definiert und stellt bereit:

- Engine-Erstellung und -Lebenszyklus
- Konfiguration ueber Presets
- Single-Objective- und Multi-Objective-`run`-Funktionen
- Custom-Evaluator-Callbacks

### Erstellen

```bash
# Build the WASI component (requires cargo-component)
cd imads-wasm && cargo component build --release
```

Dies erzeugt eine `.wasm`-Komponente in `target/wasm32-wasip2/release/`.

### JavaScript/TypeScript-Bindings generieren

Verwenden Sie `jco`, um die Komponente in JavaScript/TypeScript-Module zu transpilieren:

```bash
# Install jco (if not already installed)
npm install -g @bytecodealliance/jco

# Transpile to JS/TS
jco transpile target/wasm32-wasip2/release/imads_wasm.wasm \
    --out-dir npm/dist \
    --map 'imads:core/*=./imads-*.js'
```

Dies erzeugt:
- `npm/dist/imads_wasm.js` — ES-Modul mit allen Exports
- `npm/dist/imads_wasm.d.ts` — TypeScript-Typdeklarationen
- `npm/dist/*.wasm` — Kernmodul(e)

### npm-Paket: `imads-wasm`

Die transpilierte Ausgabe wird als Standard-npm-Paket verteilt:

```jsonc
{
  "exports": {
    ".": { "import": "./dist/imads_wasm.js", "types": "./dist/imads_wasm.d.ts" }
  }
}
```

## Verwendung

### Mit einem Bundler (Webpack 5+, Vite)

```typescript
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const env = new Env(1, 2, 3, 4);
const output = new Engine().run(cfg, env);
console.log(output.fBest, output.xBest);
```

**Webpack 5** Konfiguration — aktivieren Sie `asyncWebAssembly`:
```javascript
// webpack.config.js
module.exports = {
  experiments: { asyncWebAssembly: true },
  // ...
};
```

**Vite** — funktioniert sofort mit `vite-plugin-wasm` oder Top-Level Await:
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

Node.js 18+ mit ESM-Unterstuetzung ist erforderlich. Die Komponente wird ueber den Standard-ESM-Import geladen.

## Verwendung ueber Framework-Bindings

| Framework | Import-Stil | Hinweise |
|-----------|-------------|:--------:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | ueber Bundler |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | ueber Bundler |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | ueber Bundler |
| **TypeScript** (Bundler) | `import { ... } from "imads-wasm"` | Bundler |

Alle JS-Target-Framework-Bindings (Kotlin/JS, Scala.js, CLJS) setzen voraus, dass ein Bundler vorhanden ist.
Deren Build-Werkzeuge (Gradle+Webpack, sbt+Scala.js Linker, shadow-cljs) fungieren als Bundler.

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
    return 3;  // undefined zurueckgeben, damit die Engine aus Config oder Incumbent ableitet
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints, searchDim);
```

## Build fuer WASI-Ziele

```bash
# Component model (empfohlen)
cargo component build -p imads-wasm --release

# Legacy-WASI-Ziele (weiterhin unterstuetzt)
cargo build -p imads-wasm --target wasm32-wasip1 --release          # single-threaded
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # multi-threaded
```

## Threading-Modell

| Target | Workers | Hinweise |
|--------|---------|----------|
| `wasm32-wasip2` (Komponente) | N | Component Model + Threads |
| `wasm32-wasip1` | nur 1 | Keine Thread-Unterstuetzung |
| `wasm32-wasip1-threads` | N | Vollstaendiger Thread-Pool |
| `wasm32-unknown-unknown` (Browser) | nur 1 | Kein `std::thread` |

`AdaptiveExecutor` waehlt automatisch den korrekten Modus basierend auf dem Build-Target aus.
