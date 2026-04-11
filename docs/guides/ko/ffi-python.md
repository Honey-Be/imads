# Python FFI 가이드 (CPython + GraalPython)

`imads` Python 패키지는 CPython과 GraalPython 모두에서 동작하는 **단일 통합 API**를 제공합니다.

| Runtime | Backend | How |
|---------|---------|-----|
| **CPython** | PyO3 native extension (`_imads.so`) | `maturin develop` |
| **GraalPython** | Java interop → FFM (`imads-jvm`) | JDK 22+, `java.library.path` |

올바른 백엔드는 import 시점에 자동으로 선택됩니다.

## 설치

### CPython

```bash
cd imads-py
# if not installed
pip install maturin
maturin develop --release
```

### GraalPython

```bash
# 네이티브 라이브러리 빌드 (공유 라이브러리)
cargo build -p imads-ffi --release

# FFM Java 브릿지 컴파일 (JDK 22+ 필요)
javac -d imads-jvm/target imads-jvm/src/main/java/io/imads/*.java

# GraalPython으로 실행
graalpy --jvm \
    --vm.Djava.library.path=target/release \
    --vm.cp=imads-jvm/target \
    your_script.py
```

## API (두 런타임에서 동일)

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

## 사용자 정의 Evaluator (두 런타임에서 동일)

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

> **참고:** `search_dim()`은 선택 사항입니다. evaluator가 이를 제공하면, 엔진이 자동으로 탐색 공간의 차원 수를 파악합니다. 생략하면, 엔진은 `EngineConfig.search_dim`(설정된 경우) 또는 incumbent의 길이를 사용합니다. 프리셋은 이제 기본적으로 `search_dim=None`이며, evaluator가 이를 제공할 것을 기대합니다.

## 다목적 Evaluator

다목적 최적화의 경우, 단일 float 대신 목적 함수 값의 리스트를 반환합니다:

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

# 최적 해의 모든 목적 함수 값에 접근
print(output.f_best_all)   # 예: [0.123, 0.456]
print(output.f_best)       # 첫 번째 목적 함수 (하위 호환): 0.123
print(output.num_objectives)  # 2
```

`output.f_best_all`은 최적 목적 함수 값의 전체 `Vec<f64>`를 반환합니다.
`output.f_best`는 첫 번째 목적 함수에 대한 편의 접근자로 계속 사용할 수 있습니다.
`output.num_objectives`는 목적 함수의 수를 보고합니다.

## 아키텍처

```
imads/__init__.py          ← 런타임 자동 감지
├── imads/_cpython.py      ← PyO3 _imads native extension 래핑
└── imads/_graalpy.py      ← GraalPython java interop을 통한 FFM 래핑 (JDK 22+)
```

## 성능 참고 사항

- **CPython**: 각 `mc_sample` 호출은 GIL을 통해 Python/Rust 경계를 넘습니다.
- **GraalPython**: 각 `mc_sample` 호출은 FFM을 통해 Python/Java/Rust 경계를 넘습니다.
- 연산이 무거운 evaluator의 경우, Python 측 코드를 가볍게 유지하십시오.
- 다중 워커 실행은 GIL/JVM 획득 사이에서 병렬화됩니다.
