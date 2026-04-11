# Python FFI ガイド (CPython + GraalPython)

`imads` Python パッケージは、CPython と GraalPython の両方で動作する**単一の統合 API** を提供します。

| Runtime | Backend | How |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → FFM (`imads-jvm`) | JDK 22+, `java.library.path` |

適切なバックエンドは、インポート時に自動的に選択されます。

## インストール

### CPython

```bash
cd imads-py
# if not installed
pip install maturin
maturin develop --release
```

### GraalPython

```bash
# ネイティブライブラリ（共有）のビルド
cargo build -p imads-ffi --release

# FFM Java ブリッジのコンパイル（JDK 22+ が必要）
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# GraalPython で実行
graalpy --jvm \
    --vm.Djava.library.path=target/release \
    --vm.cp=imads-jvm/target \
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

## カスタム Evaluator (両ランタイムで共通)

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

> **注意:** `search_dim()` はオプションです。evaluator がこれを提供すると、エンジンは自動的に探索空間の次元数を検出します。省略した場合、エンジンは `EngineConfig.search_dim`（設定されている場合）または incumbent の長さにフォールバックします。プリセットはデフォルトで `search_dim=None` であり、evaluator からの提供を期待します。

## 多目的 Evaluator

多目的最適化では、単一の float の代わりに目的関数値のリストを返します:

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

# 最良解のすべての目的関数値にアクセス
print(output.f_best_all)   # 例: [0.123, 0.456]
print(output.f_best)       # 最初の目的関数（後方互換）: 0.123
print(output.num_objectives)  # 2
```

`output.f_best_all` は最良の目的関数値の完全な `Vec<f64>` を返します。
`output.f_best` は最初の目的関数の便利なアクセサとして引き続き利用可能です。
`output.num_objectives` は目的関数の数を報告します。

## アーキテクチャ

```
imads/__init__.py          ← auto-detects runtime
├── imads/_cpython.py      ← wraps PyO3 _imads native extension
└── imads/_graalpy.py      ← wraps FFM via GraalPython java interop (JDK 22+)
```

## パフォーマンスに関する注意事項

- **CPython**: `mc_sample` の各呼び出しは、GIL を介して Python/Rust の境界を越えます。
- **GraalPython**: `mc_sample` の各呼び出しは、FFM を介して Python/Java/Rust の境界を越えます。
- 計算負荷の高い evaluator の場合、Python 側はできるだけ軽量に保ってください。
- マルチワーカー実行は、GIL/JVM の取得間で並列化されます。
