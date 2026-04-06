# WASM / TypeScript FFI Guide

## Building for Browser

Requires [wasm-pack](https://rustwasm.github.io/wasm-pack/):

```bash
cd imads-wasm
wasm-pack build --target web --release
```

This produces a `pkg/` directory with `.wasm`, `.js`, and `.d.ts` files.

## Building for WASI (with threads)

```bash
# Single-threaded (WASI P1)
cargo build -p imads-wasm --target wasm32-wasip1 --release

# Multi-threaded (WASI P1 with threads)
cargo build -p imads-wasm --target wasm32-wasip1-threads --release

# WASI P3 (component model + threads)
cargo build -p imads-wasm --target wasm32-wasip3 --release
```

## TypeScript Usage (Browser)

```typescript
import init, { Engine, EngineConfig, Env } from "./pkg/imads_wasm.js";

async function main() {
    await init();

    const cfg = EngineConfig.fromPreset("balanced");
    const env = new Env(1, 2, 3, 4);  // run_id, config_hash, data_snapshot_id, rng_master_seed

    const engine = new Engine();
    const output = engine.run(cfg, env);

    console.log("f_best:", output.fBest);
    console.log("x_best:", output.xBest);
    console.log("truth_evals:", output.truthEvals);
    console.log("partial_steps:", output.partialSteps);
}

main();
```

## Available Presets

```typescript
const names = EngineConfig.presetNames();
// ["legacy_baseline", "balanced", "conservative", "throughput"]
```

## Custom Evaluator

```typescript
function mcSample(x: Float64Array, tau: number, k: number): number[] {
    // Return [objective, constraint_0, constraint_1, ...]
    let sumSq = 0;
    for (let i = 0; i < x.length; i++) sumSq += x[i] * x[i];
    return [sumSq, x[0] - 1, x[1] - 2];
}

function cheapConstraints(x: Float64Array): boolean {
    return true;  // accept all
}

const output = engine.runWithEvaluator(cfg, env, mcSample, 2, cheapConstraints);
```

## Threading Model

| Target | Workers | Notes |
|--------|---------|-------|
| `wasm32-unknown-unknown` (browser) | 1 only | No `std::thread` |
| `wasm32-wasip1` | 1 only | No thread support |
| `wasm32-wasip1-threads` | N | Full thread pool |
| `wasm32-wasip3` | N | Component model + threads |

The `AdaptiveExecutor` automatically selects the correct mode based on build target.
