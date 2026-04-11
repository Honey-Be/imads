# Python FFI Guide (CPython + GraalPython)

Das Python-Paket `imads` bietet eine **einheitliche API**, die sowohl auf CPython als auch auf GraalPython funktioniert.

| Runtime | Backend | Vorgehensweise |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → FFM (`imads-jvm`) | JDK 22+, `java.library.path` |

Das passende Backend wird beim Import automatisch ausgewaehlt.

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

## API (identisch auf beiden Runtimes)

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

## Custom Evaluator (identisch auf beiden Runtimes)

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

> **Hinweis:** `search_dim()` ist optional. Wenn der Evaluator es bereitstellt, erkennt die Engine die Suchraum-Dimensionalitaet automatisch. Wenn weggelassen, greift die Engine auf `EngineConfig.search_dim` (falls gesetzt) oder die Laenge des Incumbents zurueck. Presets verwenden jetzt standardmaessig `search_dim=None` und erwarten, dass der Evaluator dies bereitstellt.

## Multi-Objective Evaluator

Fuer Multi-Objective-Optimierung geben Sie eine Liste von Zielfunktionswerten anstelle eines
einzelnen Float zurueck:

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

`output.f_best_all` gibt den vollstaendigen `Vec<f64>` der besten Zielfunktionswerte zurueck.
`output.f_best` bleibt als komfortabler Accessor fuer die erste Zielfunktion verfuegbar.
`output.num_objectives` gibt die Anzahl der Zielfunktionen an.

## Architektur

```
imads/__init__.py          ← erkennt die Runtime automatisch
├── imads/_cpython.py      ← wraps PyO3 _imads native extension
└── imads/_graalpy.py      ← wraps FFM ueber GraalPython java interop (JDK 22+)
```

## Hinweise zur Performance

- **CPython**: Jeder `mc_sample`-Aufruf ueberquert die Python/Rust-Grenze ueber den GIL.
- **GraalPython**: Jeder `mc_sample`-Aufruf ueberquert die Python/Java/Rust-Grenze ueber FFM.
- Halten Sie bei rechenintensiven Evaluatoren die Python-Seite schlank.
- Die Multi-Worker-Ausfuehrung parallelisiert zwischen GIL/JVM-Akquisitionen.
