"""IMADS — Integrated Mesh Adaptive Direct Search.

Unified API for CPython (PyO3 native) and GraalPython (JNI).
The correct backend is selected automatically at import time.

Usage::

    import imads

    cfg = imads.EngineConfig.from_preset("balanced")
    env = imads.Env(run_id=1, config_hash=2, data_snapshot_id=3, rng_master_seed=4)
    engine = imads.Engine()
    output = engine.run(cfg, env, workers=4)
    print(output.f_best, output.x_best)
"""

import sys as _sys

def _is_graalpy() -> bool:
    return hasattr(_sys, "graal_python_id") or "graalpy" in _sys.version.lower()

if _is_graalpy():
    from imads._graalpy import Engine, EngineConfig, EngineOutput, Env, Evaluator
else:
    from imads._cpython import Engine, EngineConfig, EngineOutput, Env, Evaluator

__all__ = ["Engine", "EngineConfig", "EngineOutput", "Env", "Evaluator"]
