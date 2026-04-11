# Python FFI Guide (CPython + GraalPython)

The `imads` Python package provides a **single unified API** that works on both CPython and GraalPython.

| Runtime | Backend | How |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → FFM (`imads-jvm`) | JDK 22+, `java.library.path` |

The correct backend is selected automatically at import time.

## Installation

### CPython

```bash
cd imads-py
# if not installed
pip install maturin
maturin develop --release
```

### GraalPython

```bash
# Build native library (shared)
cargo build -p imads-ffi --release

# Compile FFM Java bridge (requires JDK 22+)
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# Run with GraalPython
graalpy --jvm \
    --vm.Djava.library.path=target/release \
    --vm.cp=imads-jvm/target \
    your_script.py
```

## API (identical on both runtimes)

```python
import imads

# Basic run with built-in evaluator
cfg = imads.EngineConfig.from_preset("balanced")
env = imads.Env(run_id=1, config_hash=2, data_snapshot_id=3, rng_master_seed=4)
engine = imads.Engine()
output = engine.run(cfg, env, workers=4)
print(output.f_best, output.x_best)

# Available presets
print(imads.EngineConfig.preset_names())
# ['legacy_baseline', 'balanced', 'conservative', 'throughput']
```

## Custom Evaluator (identical on both runtimes)

```python
class MyEvaluator:
    def mc_sample(self, x: list[float], tau: int, smc: int, k: int) -> tuple[float, list[float]]:
        f = sum(xi ** 2 for xi in x)
        c = [sum(x) - (j + 1) for j in range(2)]
        return f, c

    def cheap_constraints(self, x: list[float]) -> bool:
        return True

    def search_dim(self) -> int:
        """Optional: return the number of search dimensions.
        When provided, the engine uses this instead of EngineConfig.search_dim."""
        return 4

evaluator = MyEvaluator()
output = engine.run(cfg, env, workers=4, evaluator=evaluator, num_constraints=2)
```

> **Note:** `search_dim()` is optional. When the evaluator provides it, the engine automatically discovers the search space dimensionality. If omitted, the engine falls back to `EngineConfig.search_dim` (if set) or the incumbent's length. Presets now default to `search_dim=None`, expecting the evaluator to provide it.

## Multi-Objective Evaluator

For multi-objective optimization, return a list of objective values instead of a single
float:

```python
class MyMultiEvaluator:
    def mc_sample(self, x: list[float], tau: int, smc: int, k: int) -> tuple[list[float], list[float]]:
        f1 = sum(xi ** 2 for xi in x)
        f2 = sum((xi - 1) ** 2 for xi in x)
        c = [sum(x) - 1.0]
        return [f1, f2], c

    def num_objectives(self) -> int:
        return 2

evaluator = MyMultiEvaluator()
output = engine.run(cfg, env, workers=4, evaluator=evaluator, num_constraints=1)

# Access all objective values for the best solution
print(output.f_best_all)   # e.g. [0.123, 0.456]
print(output.f_best)       # first objective (backward compat): 0.123
print(output.num_objectives)  # 2
```

`output.f_best_all` returns the full `Vec<f64>` of best objective values.
`output.f_best` remains available as a convenience accessor for the first objective.
`output.num_objectives` reports the number of objectives.

## Architecture

```
imads/__init__.py          ← auto-detects runtime
├── imads/_cpython.py      ← wraps PyO3 _imads native extension
└── imads/_graalpy.py      ← wraps FFM via GraalPython java interop (JDK 22+)
```

## Performance Notes

- **CPython**: Each `mc_sample` call crosses the Python/Rust boundary via the GIL.
- **GraalPython**: Each `mc_sample` call crosses the Python/Java/Rust boundary via FFM.
- For compute-heavy evaluators, keep the Python side lightweight.
- Multi-worker execution parallelizes between GIL/JVM acquisitions.
