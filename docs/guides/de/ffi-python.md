# Python FFI Guide (CPython + GraalPython)

Das Python-Paket `imads` bietet eine **einheitliche API**, die sowohl auf CPython als auch auf GraalPython funktioniert.

| Runtime | Backend | Vorgehensweise |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → JNI (`libimads_jni`) | `java.library.path` + classpath |

Das passende Backend wird beim Import automatisch ausgewählt.

## Installation

### CPython

```bash
cd imads-py
pip install maturin
maturin develop --release
```

### GraalPython

```bash
# Build JNI native library
cargo build -p imads-jni --release

# Compile Java bridge
javac -d imads-jni/java/target imads-jni/java/src/main/java/io/imads/*.java

# Run with GraalPython
graalpy --jvm \
    --vm.Djava.library.path=target/release \
    --vm.cp=imads-jni/java/target \
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

evaluator = MyEvaluator()
output = engine.run(cfg, env, workers=4, evaluator=evaluator, num_constraints=2)
```

## Architektur

```
imads/__init__.py          ← erkennt die Runtime automatisch
├── imads/_cpython.py      ← wraps PyO3 _imads native extension
└── imads/_graalpy.py      ← wraps JNI via GraalPython java interop
```

## Hinweise zur Performance

- **CPython**: Jeder `mc_sample`-Aufruf überquert die Python/Rust-Grenze über den GIL.
- **GraalPython**: Jeder `mc_sample`-Aufruf überquert die Python/Java/Rust-Grenze.
- Halten Sie bei rechenintensiven Evaluatoren die Python-Seite schlank.
- Die Multi-Worker-Ausführung parallelisiert zwischen GIL/JVM-Akquisitionen.
