# WASM / TypeScript FFI ガイド

## WASI コンポーネントモデル

`imads-wasm` クレートは、`wit-bindgen` を使用した **WASI コンポーネントモデル**を使用するように
書き換えられました。以前の `wasm-bindgen` ベースのアプローチを置き換えます。WIT
（WebAssembly Interface Type）インターフェースが、ホストとコンポーネント間の契約を定義します。

### WIT インターフェース

WIT インターフェースは `imads-wasm/wit/imads.wit` で定義されており、以下を公開します:

- エンジンの作成とライフサイクル
- プリセットからの設定
- 単目的および多目的の `run` 関数
- カスタム evaluator コールバック

### ビルド

```bash
# WASI コンポーネントのビルド（cargo-component が必要）
cd imads-wasm && cargo component build --release
```

`target/wasm32-wasip2/release/` に `.wasm` コンポーネントが生成されます。

### JavaScript/TypeScript バインディングの生成

`jco` を使用してコンポーネントを JavaScript/TypeScript モジュールにトランスパイルします:

```bash
# jco のインストール（未インストールの場合）
npm install -g @bytecodealliance/jco

# JS/TS へのトランスパイル
jco transpile target/wasm32-wasip2/release/imads_wasm.wasm \
    --out-dir npm/dist \
    --map 'imads:core/*=./imads-*.js'
```

以下が生成されます:
- `npm/dist/imads_wasm.js` — すべてのエクスポートを含む ES モジュール
- `npm/dist/imads_wasm.d.ts` — TypeScript 型宣言
- `npm/dist/*.wasm` — コアモジュール

### npm パッケージ: `imads-wasm`

トランスパイルされた出力は、標準の npm パッケージとして配布されます:

```jsonc
{
  "exports": {
    ".": { "import": "./dist/imads_wasm.js", "types": "./dist/imads_wasm.d.ts" }
  }
}
```

## 使い方

### バンドラーを使用する場合（Webpack 5+、Vite）

```typescript
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

### Node.js (ESM)

```javascript
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const output = new Engine().run(cfg, new Env(1, 2, 3, 4));
console.log(output.fBest);
```

ESM サポート付きの Node.js 18+ が必要です。コンポーネントは標準の ESM import を介してロードされます。

## フレームワークバインディングからの使用

| Framework | Import style | 備考 |
|-----------|-------------|:-----:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | バンドラー経由 |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | バンドラー経由 |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | バンドラー経由 |
| **TypeScript** (バンドラー) | `import { ... } from "imads-wasm"` | バンドラー |

すべての JS ターゲットフレームワークバインディング（Kotlin/JS、Scala.js、CLJS）はバンドラーの存在を前提としています。
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

function searchDim(): number | undefined {
    return 3;  // undefined を返すと、エンジンが config または incumbent から推論します
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints, searchDim);
```

## WASI ターゲット向けビルド

```bash
# コンポーネントモデル（推奨）
cargo component build -p imads-wasm --release

# レガシー WASI ターゲット（引き続きサポート）
cargo build -p imads-wasm --target wasm32-wasip1 --release          # シングルスレッド
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # マルチスレッド
```

## スレッディングモデル

| Target | Workers | 備考 |
|--------|---------|-------|
| `wasm32-wasip2` (コンポーネント) | N | コンポーネントモデル + スレッド |
| `wasm32-wasip1` | 1 のみ | スレッドサポートなし |
| `wasm32-wasip1-threads` | N | フルスレッドプール |
| `wasm32-unknown-unknown` (ブラウザ) | 1 のみ | `std::thread` なし |

`AdaptiveExecutor` はビルドターゲットに基づいて、正しいモードを自動的に選択します。
