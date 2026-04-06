# Python FFI Guide

## Installation

Requires Python >= 3.9 and [maturin](https://github.com/PyO3/maturin).

```bash
cd imads-py
pip install maturin
maturin develop --release
```

## Basic Usage

```python
import imads

cfg = imads.EngineConfig.from_preset("balanced")
env = imads.Env(run_id=1, config_hash=2, data_snapshot_id=3, rng_master_seed=4)
engine = imads.Engine()
output = engine.run(cfg, env, workers=4)

print(f"f_best = {output.f_best}")
print(f"x_best = {output.x_best}")
print(f"truth_evals = {output.truth_evals}")
print(f"partial_steps = {output.partial_steps}")
```

## Available Presets

```python
print(imads.EngineConfig.preset_names())
# ['legacy_baseline', 'balanced', 'conservative', 'throughput']
```

## Custom Evaluator

Implement a Python class with a `mc_sample` method:

```python
class MyEvaluator:
    def mc_sample(self, x: list[float], tau: int, smc: int, k: int) -> tuple[float, list[float]]:
        """Return (objective, [constraint_0, constraint_1, ...])."""
        f = sum(xi ** 2 for xi in x)
        c = [sum(x) - (j + 1) for j in range(2)]
        return f, c

    def cheap_constraints(self, x: list[float]) -> bool:
        """Optional: return False to reject without evaluation."""
        return True

evaluator = MyEvaluator()
output = engine.run_with_evaluator(cfg, env, evaluator, num_constraints=2, workers=4)
```

## Performance Notes

- Each `mc_sample` call crosses the Python/Rust boundary via the GIL.
- For compute-heavy evaluators, keep the Python side lightweight or use numpy.
- Multi-worker execution still parallelizes between GIL acquisitions.
