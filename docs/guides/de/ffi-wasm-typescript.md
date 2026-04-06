# WASM / TypeScript FFI Guide

## npm-Paket: `imads-wasm`

Die WASM-Bindings werden als einzelnes npm-Paket mit drei Build-Targets ausgeliefert:

| Target | Anwendungsfall | `init()` erforderlich? | Module format |
|--------|----------------|:----------------------:|:-------------:|
| `bundler` | Webpack 5+, Vite, Rollup | Nein | ESM |
| `web` | `<script type="module">` | Ja (`await init()`) | ESM |
| `nodejs` | Node.js ohne Bundler | Nein | CJS |

### package.json exports

```jsonc
{
  "exports": {
    ".":         { "import": "./bundler/...", "require": "./nodejs/..." },
    "./web":     { "import": "./web/..." },
    "./bundler": { "import": "./bundler/..." },
    "./nodejs":  { "require": "./nodejs/..." }
  }
}
```

Bundler, die `package.json` exports unterstuetzen (Webpack 5+, Vite, Rollup mit `@rollup/plugin-node-resolve`), waehlen automatisch das `bundler`-Target aus.

## Build-Anleitung

```bash
# All three targets (recommended)
make wasm-npm

# Individual targets
make wasm-bundler    # for bundlers (Webpack, Vite)
make wasm-web        # for direct browser use
make wasm-nodejs     # for Node.js

# Or use the script directly
cd imads-wasm && ./build-npm.sh
```

Ausgabe: `imads-wasm/npm/` mit den Unterverzeichnissen `bundler/`, `web/`, `nodejs/`.

## Verwendung nach Umgebung

### Mit einem Bundler (Webpack 5+, Vite)

```typescript
// Bundler resolves "imads-wasm" → npm/bundler/imads_wasm.js
// .wasm file is loaded automatically by the bundler — no init() needed
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

### Ohne Bundler (Browser)

```html
<script type="module">
  // Use the ./web subpath export
  import init, { Engine, EngineConfig, Env } from "imads-wasm/web";

  await init();  // must call init() first
  const cfg = EngineConfig.fromPreset("balanced");
  const output = new Engine().run(cfg, new Env(1, 2, 3, 4));
  console.log(output.fBest);
</script>
```

### Node.js

```javascript
// CommonJS — uses ./nodejs subpath
const { Engine, EngineConfig, Env } = require("imads-wasm/nodejs");

const cfg = EngineConfig.fromPreset("balanced");
const output = new Engine().run(cfg, new Env(1, 2, 3, 4));
console.log(output.fBest);
```

### ESM in Node.js

```javascript
// Uses the default export (bundler target, works with Node 18+ ESM)
import { Engine, EngineConfig, Env } from "imads-wasm";
```

## Verwendung ueber Framework-Bindings

| Framework | Import-Stil | Verwendetes Target |
|-----------|-------------|:------------------:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | `bundler` |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | `bundler` |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | `bundler` |
| **TypeScript** (Bundler) | `import { ... } from "imads-wasm"` | `bundler` |
| **TypeScript** (Browser) | `import init, { ... } from "imads-wasm/web"` | `web` |

Alle JS-Target-Framework-Bindings (Kotlin/JS, Scala.js, CLJS) setzen das `bundler`-Target voraus.
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

## Build fuer WASI (mit Threads)

```bash
cargo build -p imads-wasm --target wasm32-wasip1 --release          # single-threaded
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # multi-threaded
cargo build -p imads-wasm --target wasm32-wasip3 --release          # component model
```

## Threading-Modell

| Target | Workers | Hinweise |
|--------|---------|----------|
| `wasm32-unknown-unknown` (Browser) | nur 1 | Kein `std::thread` |
| `wasm32-wasip1` | nur 1 | Keine Thread-Unterstuetzung |
| `wasm32-wasip1-threads` | N | Vollstaendiger Thread-Pool |
| `wasm32-wasip3` | N | Component Model + Threads |

`AdaptiveExecutor` waehlt automatisch den korrekten Modus basierend auf dem Build-Target aus.
