# WASM / TypeScript FFI 가이드

## WASI Component Model

`imads-wasm` 크레이트는 `wit-bindgen`을 사용하는 **WASI Component Model**로 재작성되었습니다.
이전의 `wasm-bindgen` 기반 방식을 대체합니다. WIT(WebAssembly Interface Type) 인터페이스는
호스트와 컴포넌트 간의 계약을 정의합니다.

### WIT 인터페이스

WIT 인터페이스는 `imads-wasm/wit/imads.wit`에 정의되어 있으며 다음을 노출합니다:

- 엔진 생성 및 생명주기
- 프리셋으로부터의 구성
- 단일 목적 및 다목적 `run` 함수
- 사용자 정의 evaluator 콜백

### 빌드

```bash
# WASI 컴포넌트 빌드 (cargo-component 필요)
cd imads-wasm && cargo component build --release
```

위 명령은 `target/wasm32-wasip2/release/`에 `.wasm` 컴포넌트를 생성합니다.

### JavaScript/TypeScript 바인딩 생성

`jco`를 사용하여 컴포넌트를 JavaScript/TypeScript 모듈로 트랜스파일합니다:

```bash
# jco 설치 (아직 설치하지 않은 경우)
npm install -g @bytecodealliance/jco

# JS/TS로 트랜스파일
jco transpile target/wasm32-wasip2/release/imads_wasm.wasm \
    --out-dir npm/dist \
    --map 'imads:core/*=./imads-*.js'
```

위 명령은 다음을 생성합니다:
- `npm/dist/imads_wasm.js` — 모든 export를 포함한 ES 모듈
- `npm/dist/imads_wasm.d.ts` — TypeScript 타입 선언
- `npm/dist/*.wasm` — 코어 모듈

### npm 패키지: `imads-wasm`

트랜스파일된 결과물은 표준 npm 패키지로 배포됩니다:

```jsonc
{
  "exports": {
    ".": { "import": "./dist/imads_wasm.js", "types": "./dist/imads_wasm.d.ts" }
  }
}
```

## 사용법

### 번들러 사용 시 (Webpack 5+, Vite)

```typescript
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

### Node.js (ESM)

```javascript
import { Engine, EngineConfig, Env } from "imads-wasm";

const cfg = EngineConfig.fromPreset("balanced");
const output = new Engine().run(cfg, new Env(1, 2, 3, 4));
console.log(output.fBest);
```

ESM을 지원하는 Node.js 18+가 필요합니다. 컴포넌트는 표준 ESM import를 통해
로드됩니다.

## 프레임워크 바인딩을 통한 사용

| Framework | Import 방식 | 비고 |
|-----------|-------------|:----:|
| **Kotlin/JS** | `@JsModule("imads-wasm")` | 번들러 사용 |
| **Scala.js** | `@JSImport("imads-wasm", Namespace)` | 번들러 사용 |
| **ClojureScript** | `(:require ["imads-wasm" :as wasm])` | 번들러 사용 |
| **TypeScript** (번들러) | `import { ... } from "imads-wasm"` | 번들러 사용 |

모든 JS 타겟 프레임워크 바인딩(Kotlin/JS, Scala.js, CLJS)은 번들러가 있다고 가정합니다.
해당 빌드 도구(Gradle+Webpack, sbt+Scala.js linker, shadow-cljs)가 번들러 역할을 수행합니다.

## 사용자 정의 Evaluator

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
    return 3;  // undefined를 반환하면 엔진이 config 또는 incumbent에서 추론합니다
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints, searchDim);
```

## WASI 타겟용 빌드

```bash
# Component model (권장)
cargo component build -p imads-wasm --release

# 레거시 WASI 타겟 (계속 지원)
cargo build -p imads-wasm --target wasm32-wasip1 --release          # 싱글 스레드
cargo build -p imads-wasm --target wasm32-wasip1-threads --release  # 멀티 스레드
```

## 스레딩 모델

| Target | Workers | 비고 |
|--------|---------|------|
| `wasm32-wasip2` (component) | N | Component model + 스레드 |
| `wasm32-wasip1` | 1개만 | 스레드 미지원 |
| `wasm32-wasip1-threads` | N | 전체 스레드 풀 사용 가능 |
| `wasm32-unknown-unknown` (브라우저) | 1개만 | `std::thread` 사용 불가 |

`AdaptiveExecutor`는 빌드 타겟에 따라 자동으로 올바른 모드를 선택합니다.
