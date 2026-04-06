"""GraalPython backend via Java interop (JNI).

GraalPython provides direct Java interop via the `java` module.
This backend uses the io.imads.ImadsNative JNI bridge.
"""

import java  # type: ignore  # GraalPython built-in
import struct
from typing import Protocol, Optional

__all__ = ["Engine", "EngineConfig", "EngineOutput", "Env", "Evaluator"]

# Import Java classes
_ImadsNative = java.type("io.imads.ImadsNative")
_ImadsJvmEvaluator = java.type("io.imads.ImadsJvmEvaluator")


class Evaluator(Protocol):
    """Custom evaluator protocol. All methods must be deterministic."""

    def mc_sample(
        self, x: list[float], tau: int, smc: int, k: int
    ) -> tuple[float, list[float]]:
        ...

    def cheap_constraints(self, x: list[float]) -> bool:
        ...


class Env:
    """Environment descriptor."""

    __slots__ = ("run_id", "config_hash", "data_snapshot_id", "rng_master_seed")

    def __init__(
        self,
        run_id: int = 1,
        config_hash: int = 0,
        data_snapshot_id: int = 0,
        rng_master_seed: int = 0,
    ):
        self.run_id = run_id
        self.config_hash = config_hash
        self.data_snapshot_id = data_snapshot_id
        self.rng_master_seed = rng_master_seed


class EngineConfig:
    """Engine configuration (wraps JNI pointer)."""

    __slots__ = ("_ptr",)

    def __init__(self, ptr: int):
        self._ptr = ptr

    @staticmethod
    def from_preset(name: str) -> "EngineConfig":
        p = _ImadsNative.configFromPreset(name)
        if p == 0:
            raise ValueError(f"Unknown preset: {name}")
        return EngineConfig(p)

    @staticmethod
    def preset_names() -> list[str]:
        return list(_ImadsNative.presetNames())

    def close(self):
        if self._ptr != 0:
            _ImadsNative.configFree(self._ptr)
            self._ptr = 0

    def __del__(self):
        self.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def _unpack_output(packed) -> "EngineOutput":
    f_bits = packed[0]
    f = struct.unpack("d", struct.pack("q", f_bits))[0]
    x_len = int(packed[1])
    x_best = [int(packed[6 + i]) for i in range(x_len)]
    import math

    return EngineOutput(
        f_best=None if math.isnan(f) else f,
        x_best=x_best,
        truth_evals=int(packed[2]),
        partial_steps=int(packed[3]),
        cheap_rejects=int(packed[4]),
        invalid_eval_rejects=int(packed[5]),
    )


class EngineOutput:
    """Result of an engine run."""

    __slots__ = (
        "f_best",
        "x_best",
        "truth_evals",
        "partial_steps",
        "cheap_rejects",
        "invalid_eval_rejects",
    )

    def __init__(self, f_best, x_best, truth_evals, partial_steps, cheap_rejects, invalid_eval_rejects):
        self.f_best = f_best
        self.x_best = x_best
        self.truth_evals = truth_evals
        self.partial_steps = partial_steps
        self.cheap_rejects = cheap_rejects
        self.invalid_eval_rejects = invalid_eval_rejects

    def __repr__(self) -> str:
        return f"EngineOutput(f_best={self.f_best}, truth_evals={self.truth_evals}, partial_steps={self.partial_steps})"


class _JvmEvaluatorBridge:
    """Adapts a Python Evaluator to io.imads.ImadsJvmEvaluator for GraalPython."""

    def __init__(self, evaluator, num_constraints):
        self._eval = evaluator
        self._m = num_constraints

    class Java:
        implements = ["io.imads.ImadsJvmEvaluator"]

    def mcSample(self, x, tau, smc, k):
        x_list = list(x)
        f, c = self._eval.mc_sample(x_list, int(tau), int(smc), int(k))
        result = [f] + list(c)
        return java.to_java_array("double", result)

    def cheapConstraints(self, x):
        x_list = list(x)
        if hasattr(self._eval, "cheap_constraints"):
            return bool(self._eval.cheap_constraints(x_list))
        return True


class Engine:
    """IMADS optimization engine."""

    __slots__ = ("_ptr",)

    def __init__(self):
        self._ptr = _ImadsNative.engineNew()

    def run(
        self,
        cfg: EngineConfig,
        env: Env,
        workers: int = 1,
        evaluator: Optional[Evaluator] = None,
        num_constraints: Optional[int] = None,
    ) -> EngineOutput:
        if evaluator is not None:
            assert num_constraints is not None, "num_constraints required with evaluator"
            bridge = _JvmEvaluatorBridge(evaluator, num_constraints)
            packed = _ImadsNative.engineRunWithEvaluator(
                self._ptr, cfg._ptr,
                env.run_id, env.config_hash, env.data_snapshot_id, env.rng_master_seed,
                workers, bridge, num_constraints,
            )
        else:
            packed = _ImadsNative.engineRun(
                self._ptr, cfg._ptr,
                env.run_id, env.config_hash, env.data_snapshot_id, env.rng_master_seed,
                workers,
            )
        return _unpack_output(packed)

    def close(self):
        if self._ptr != 0:
            _ImadsNative.engineFree(self._ptr)
            self._ptr = 0

    def __del__(self):
        self.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()
