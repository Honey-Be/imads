# Python FFI Guide (CPython + GraalPython)

`imads` Python パッケージは、CPython と GraalPython の両方で動作する**単一の統合 API** を提供します。

| Runtime | Backend | How |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → JNI (`libimads_jni`) | `java.library.path` + classpath |

適切なバックエンドは、インポート時に自動的に選択されます。

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

## API (両ランタイムで共通)

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

## Custom Evaluator (両ランタイムで共通)

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

## Architecture

```
imads/__init__.py          ← auto-detects runtime
├── imads/_cpython.py      ← wraps PyO3 _imads native extension
└── imads/_graalpy.py      ← wraps JNI via GraalPython java interop
```

## パフォーマンスに関する注意事項

- **CPython**: `mc_sample` の各呼び出しは、GIL を介して Python/Rust の境界を越えます。
- **GraalPython**: `mc_sample` の各呼び出しは、Python/Java/Rust の境界を越えます。
- 計算負荷の高い evaluator の場合、Python 側はできるだけ軽量に保ってください。
- マルチワーカー実行は、GIL/JVM の取得間で並列化されます。
