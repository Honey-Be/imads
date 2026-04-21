"""CPython backend via PyO3 native extension."""

from imads._imads import (
    Engine as _Engine,
    EngineConfig as _EngineConfig,
    EngineOutput as _EngineOutput,
    Env as _Env,
)
from typing import Protocol, Optional

__all__ = ["Engine", "EngineConfig", "EngineOutput", "Env", "Evaluator"]


class Evaluator(Protocol):
    """Custom evaluator protocol. All methods must be deterministic.

    ``mc_sample`` returns ``(objective, constraints)``:

    * Single-objective: ``objective`` is a ``float``.
    * Multi-objective:  ``objective`` is a ``list[float]`` of length
      ``num_objectives`` (supply this count via
      :meth:`Engine.run(num_objectives=...)`).

    ``constraints`` is always a ``list[float]`` of length
    ``num_constraints`` — each entry is a "slack" value treated as
    feasible iff it is ``<= 0``.
    """

    def mc_sample(
        self, x: list[float], tau: int, smc: int, k: int
    ) -> "tuple[float | list[float], list[float]]":
        """Return ``(objective, [c0, c1, ...])``."""
        ...

    def cheap_constraints(self, x: list[float]) -> bool:
        """Optional: return False to reject without evaluation."""
        ...


class Env:
    """Environment descriptor."""

    __slots__ = ("_inner",)

    def __init__(
        self,
        run_id: int = 1,
        config_hash: int = 0,
        data_snapshot_id: int = 0,
        rng_master_seed: int = 0,
    ):
        self._inner = _Env(run_id, config_hash, data_snapshot_id, rng_master_seed)

    @property
    def _native(self):
        return self._inner


class EngineConfig:
    """Engine configuration."""

    __slots__ = ("_inner",)

    def __init__(self, inner):
        self._inner = inner

    @staticmethod
    def from_preset(name: str) -> "EngineConfig":
        return EngineConfig(_EngineConfig.from_preset(name))

    @staticmethod
    def preset_names() -> list[str]:
        return _EngineConfig.preset_names()

    @property
    def max_iters(self) -> int:
        """Maximum number of engine iterations.

        One iteration may execute multiple truth evaluations, so this is
        an upper bound on iterations rather than a strict eval count.
        Higher-level wrappers (e.g. ``imads_hpo.minimize``) typically
        forward a user-facing ``max_evals`` value into this field.
        """
        return self._inner.max_iters

    @max_iters.setter
    def max_iters(self, value: int) -> None:
        self._inner.max_iters = int(value)


class EngineOutput:
    """Result of an engine run."""

    __slots__ = ("_inner",)

    def __init__(self, inner):
        self._inner = inner

    @property
    def f_best(self) -> Optional[float]:
        """Primary (index-0) best objective value.

        For single-objective runs this is the only best value; for
        multi-objective runs it is the first component of ``f_best_all``.
        """
        return self._inner.f_best

    @property
    def f_best_all(self) -> Optional[list[float]]:
        """All best objective values — length ``num_objectives``.

        For single-objective runs this is a list of length 1; for
        multi-objective runs it is the full vector returned by the engine.
        """
        return self._inner.f_best_all

    @property
    def x_best(self) -> Optional[list[int]]:
        return self._inner.x_best

    @property
    def truth_evals(self) -> int:
        return self._inner.truth_evals

    @property
    def partial_steps(self) -> int:
        return self._inner.partial_steps

    @property
    def cheap_rejects(self) -> int:
        return self._inner.cheap_rejects

    @property
    def invalid_eval_rejects(self) -> int:
        return self._inner.invalid_eval_rejects

    def __repr__(self) -> str:
        return f"EngineOutput(f_best={self.f_best}, truth_evals={self.truth_evals}, partial_steps={self.partial_steps})"


class Engine:
    """IMADS optimization engine."""

    __slots__ = ("_inner",)

    def __init__(self):
        self._inner = _Engine()

    def run(
        self,
        cfg: EngineConfig,
        env: Env,
        workers: int = 1,
        evaluator: Optional[Evaluator] = None,
        num_constraints: Optional[int] = None,
        num_objectives: int = 1,
    ) -> EngineOutput:
        """Run the engine.

        Parameters
        ----------
        cfg, env, workers: see :class:`EngineConfig` / :class:`Env`.
        evaluator: if given, the engine calls ``evaluator.mc_sample`` for
            every evaluation instead of the built-in toy evaluator.
            Requires ``num_constraints``.
        num_constraints: number of constraint slack values returned by
            the evaluator per call (must be given alongside ``evaluator``).
        num_objectives: number of objectives returned by the evaluator.
            Defaults to ``1`` (single-objective). Supply ``>1`` for
            multi-objective runs, in which case the evaluator must return
            ``list[float]`` as the first element of the ``mc_sample`` tuple.
        """
        if evaluator is not None:
            assert num_constraints is not None, "num_constraints required with evaluator"
            out = self._inner.run_with_evaluator(
                cfg._inner,
                env._native,
                evaluator,
                num_constraints,
                workers,
                num_objectives,
            )
        else:
            out = self._inner.run(cfg._inner, env._native, workers)
        return EngineOutput(out)
