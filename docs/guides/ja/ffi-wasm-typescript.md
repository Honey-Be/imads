# WASM / TypeScript FFI ガイド

## npm パッケージ: `imads-wasm`

WASM バインディングは、3つのビルドターゲットを持つ単一の npm パッケージとして配布されます。

| Target | Use case | `init()` needed? | Module format |
|--------|----------|:-----------------:|:-------------:|
| `bundler` | Webpack 5+, Vite, Rollup | No | ESM |
| `web` | `<script type="module">` | Yes (`await init()`) | ESM |
| `nodejs` | Node.js without bundler | No | CJS |

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

`package.json` exports をサポートするバンドラー（Webpack 5+、Vite、`@rollup/plugin-node-resolve` を使用した Rollup）は、自動的に `bundler` ターゲットを選択します。

## ビルド

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

出力先: `imads-wasm/npm/` 内の `bundler/`、`web/`、`nodejs/` サブディレクトリです。

## 環境別の使い方

### バンドラーを使用する場合（Webpack 5+、Vite）

```typescript
// Bundler resolves "imads-wasm" → npm/bundler/imads_wasm.js
// .wasm file is loaded automatically by the bundler — no init() needed
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const env = new Env(1, 2, 3, 4);
const output = new Engine().run(cfg, env);
console.log(output.fBest, output.xBest);
```

**Webpack 5** の設定 — `asyncWebAssembly` を有効にします:
```javascript
// webpack.config.js
module.exports = {
  experiments: { asyncWebAssembly: true },
  // ...
};
```

**Vite** — `vite-plugin-wasm` またはトップレベル await を使用すれば、そのまま動作します:
```javascript
// vite.config.js
import wasm from "vite-plugin-wasm";
export default { plugins: [wasm()] };
```

### バンドラーなしの場合（ブラウザ）

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

### Node.js での ESM

```javascript
// Uses the default export (bundler target, works with Node 18+ ESM)
import { Engine, EngineConfig, Env } from "imads-wasm";
```

## フレームワークバインディングからの使用

| Framework | Import style | Target used |
|-----------|-------------|:-----------:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | `bundler` |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | `bundler` |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | `bundler` |
| **TypeScript** (bundler) | `import { ... } from "imads-wasm"` | `bundler` |
| **TypeScript** (browser) | `import init, { ... } from "imads-wasm/web"` | `web` |

すべての JS ターゲットフレームワークバインディング（Kotlin/JS、Scala.js、CLJS）は `bundler` ターゲットを前提としています。
それぞれのビルドツール（Gradle+Webpack、sbt+Scala.js linker、shadow-cljs）がバンドラーとして機能します。

## カスタム Evaluator

```typescript
function mcSample(x: Float64Array, tau: number, k: number): number[] {
    let sumSq = 0;
    for (let i = 0; i < x.length; i++) sumSq += x[i] * x[i];
    return [sumSq, x[0] - 1, x[1] - 2];
}

function cheapConstraints(x: Float64Array): boolean {
    return true;
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints);
```

## WASI 向けビルド（スレッド対応）

```bash
cargo build -p imads-wasm --target wasm32-wasip1 --release          # single-threaded
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # multi-threaded
cargo build -p imads-wasm --target wasm32-wasip3 --release          # component model
```

## スレッディングモデル

| Target | Workers | Notes |
|--------|---------|-------|
| `wasm32-unknown-unknown` (browser) | 1 only | No `std::thread` |
| `wasm32-wasip1` | 1 only | No thread support |
| `wasm32-wasip1-threads` | N | Full thread pool |
| `wasm32-wasip3` | N | Component model + threads |

`AdaptiveExecutor` はビルドターゲットに基づいて、正しいモードを自動的に選択します。
