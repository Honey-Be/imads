# WASM / TypeScript FFI 가이드

## npm 패키지: `imads-wasm`

WASM 바인딩은 세 가지 빌드 타겟을 포함한 단일 npm 패키지로 배포됩니다.

| Target | 사용 사례 | `init()` 필요 여부 | Module format |
|--------|----------|:-----------------:|:-------------:|
| `bundler` | Webpack 5+, Vite, Rollup | 아니요 | ESM |
| `web` | `<script type="module">` | 예 (`await init()`) | ESM |
| `nodejs` | 번들러 없이 Node.js 사용 | 아니요 | CJS |

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

`package.json` exports를 지원하는 번들러(Webpack 5+, Vite, `@rollup/plugin-node-resolve`를 사용하는 Rollup)는 자동으로 `bundler` 타겟을 선택합니다.

## 빌드

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

출력 경로: `imads-wasm/npm/` 하위에 `bundler/`, `web/`, `nodejs/` 디렉터리가 생성됩니다.

## 환경별 사용법

### 번들러 사용 시 (Webpack 5+, Vite)

```typescript
// Bundler resolves "imads-wasm" → npm/bundler/imads_wasm.js
// .wasm file is loaded automatically by the bundler — no init() needed
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const env = new Env(1, 2, 3, 4);
const output = new Engine().run(cfg, env);
console.log(output.fBest, output.xBest);
```

**Webpack 5** 설정 — `asyncWebAssembly`를 활성화하세요:
```javascript
// webpack.config.js
module.exports = {
  experiments: { asyncWebAssembly: true },
  // ...
};
```

**Vite** — `vite-plugin-wasm` 또는 top-level await를 사용하면 별도 설정 없이 바로 동작합니다:
```javascript
// vite.config.js
import wasm from "vite-plugin-wasm";
export default { plugins: [wasm()] };
```

### 번들러 없이 사용 시 (브라우저)

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

### Node.js에서 ESM 사용

```javascript
// Uses the default export (bundler target, works with Node 18+ ESM)
import { Engine, EngineConfig, Env } from "imads-wasm";
```

## 프레임워크 바인딩을 통한 사용

| Framework | Import 방식 | 사용되는 Target |
|-----------|-------------|:-----------:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | `bundler` |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | `bundler` |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | `bundler` |
| **TypeScript** (bundler) | `import { ... } from "imads-wasm"` | `bundler` |
| **TypeScript** (browser) | `import init, { ... } from "imads-wasm/web"` | `web` |

모든 JS 타겟 프레임워크 바인딩(Kotlin/JS, Scala.js, CLJS)은 `bundler` 타겟을 사용합니다.
해당 빌드 도구(Gradle+Webpack, sbt+Scala.js linker, shadow-cljs)가 번들러 역할을 수행합니다.

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

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints);
```

## WASI용 빌드 (스레드 지원)

```bash
cargo build -p imads-wasm --target wasm32-wasip1 --release          # single-threaded
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # multi-threaded
cargo build -p imads-wasm --target wasm32-wasip3 --release          # component model
```

## 스레딩 모델

| Target | Workers | 비고 |
|--------|---------|-------|
| `wasm32-unknown-unknown` (browser) | 1개만 | `std::thread` 사용 불가 |
| `wasm32-wasip1` | 1개만 | 스레드 미지원 |
| `wasm32-wasip1-threads` | N | 전체 스레드 풀 사용 가능 |
| `wasm32-wasip3` | N | Component model + 스레드 |

`AdaptiveExecutor`는 빌드 타겟에 따라 자동으로 올바른 모드를 선택합니다.
