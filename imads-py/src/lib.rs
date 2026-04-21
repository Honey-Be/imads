//! Python bindings for IMADS via PyO3.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use imads_core::core::engine::{Engine, EngineConfig, EngineOutput};
use imads_core::core::evaluator::{Evaluator, EvaluatorErased};
use imads_core::core::DefaultBundle;
use imads_core::presets::Preset;
use imads_core::types::{Env, Phi, XReal};

// ---------------------------------------------------------------------------
// PyEngineConfig
// ---------------------------------------------------------------------------


#[pyclass(name = "EngineConfig", from_py_object)]
#[derive(Clone)]
struct PyEngineConfig {
    inner: EngineConfig,
}

#[pymethods]
impl PyEngineConfig {
    /// Create an EngineConfig from a preset name.
    ///
    /// Valid names: "legacy_baseline", "balanced", "conservative", "throughput".
    #[staticmethod]
    fn from_preset(name: &str) -> PyResult<Self> {
        let preset = match name {
            "legacy_baseline" => Preset::LegacyBaseline,
            "balanced" => Preset::Balanced,
            "conservative" => Preset::Conservative,
            "throughput" => Preset::Throughput,
            _ => return Err(PyValueError::new_err(format!("Unknown preset: {name}"))),
        };
        Ok(Self {
            inner: preset.config(),
        })
    }

    /// List all available preset names.
    #[staticmethod]
    fn preset_names() -> Vec<String> {
        Preset::ALL.iter().map(|p| p.name().to_owned()).collect()
    }

    /// Maximum number of engine iterations (the underlying budget cap).
    ///
    /// One iteration may execute several truth evaluations, so this is an
    /// upper bound on iterations rather than a strict eval count. Higher
    /// layers (e.g. ``imads_hpo.minimize``) typically map a user-facing
    /// ``max_evals`` value to this field.
    #[getter]
    fn max_iters(&self) -> u64 {
        self.inner.max_iters
    }

    #[setter]
    fn set_max_iters(&mut self, value: u64) {
        self.inner.max_iters = value;
    }
}

// ---------------------------------------------------------------------------
// PyEnv
// ---------------------------------------------------------------------------

#[pyclass(name = "Env", from_py_object)]
#[derive(Clone)]
struct PyEnv {
    inner: Env,
}

#[pymethods]
impl PyEnv {
    #[new]
    fn new(run_id: u128, config_hash: u128, data_snapshot_id: u128, rng_master_seed: u128) -> Self {
        Self {
            inner: Env {
                run_id,
                config_hash,
                data_snapshot_id,
                rng_master_seed,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// PyEngineOutput
// ---------------------------------------------------------------------------

#[pyclass(name = "EngineOutput")]
struct PyEngineOutput {
    inner: EngineOutput,
}

#[pymethods]
impl PyEngineOutput {
    /// Primary (first) objective best value. For single-objective, this is the only value.
    #[getter]
    fn f_best(&self) -> Option<f64> {
        self.inner.f_best.as_ref().map(|f| f[0])
    }

    /// All objective best values (for multi-objective optimization).
    #[getter]
    fn f_best_all(&self) -> Option<Vec<f64>> {
        self.inner.f_best.clone()
    }

    #[getter]
    fn x_best(&self) -> Option<Vec<i64>> {
        self.inner.x_best.as_ref().map(|xm| xm.0.clone())
    }

    #[getter]
    fn truth_evals(&self) -> u64 {
        self.inner.stats.truth_evals
    }

    #[getter]
    fn partial_steps(&self) -> u64 {
        self.inner.stats.partial_steps
    }

    #[getter]
    fn cheap_rejects(&self) -> u64 {
        self.inner.stats.cheap_rejects
    }

    #[getter]
    fn invalid_eval_rejects(&self) -> u64 {
        self.inner.stats.invalid_eval_rejects
    }

    fn __repr__(&self) -> String {
        format!(
            "EngineOutput(f_best={:?}, truth_evals={}, partial_steps={})",
            self.inner.f_best.as_ref().map(|f| f[0]), self.inner.stats.truth_evals, self.inner.stats.partial_steps,
        )
    }
}

// ---------------------------------------------------------------------------
// PyEvaluator — wraps a Python object implementing the evaluator protocol
// ---------------------------------------------------------------------------

struct PyEvaluator {
    obj: Py<PyAny>,
    m: usize,
    n_obj: usize,
}

impl std::fmt::Debug for PyEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyEvaluator")
            .field("num_constraints", &self.m)
            .field("num_objectives", &self.n_obj)
            .finish()
    }
}

// Safety: GIL protects all Python interactions.
unsafe impl Send for PyEvaluator {}
unsafe impl Sync for PyEvaluator {}

impl Evaluator for PyEvaluator {
    // `Vec<f64>` is the most permissive `ObjectiveValues` impl: a
    // single-objective Python evaluator returns a Python ``float`` which
    // we wrap into ``vec![x]`` below; a multi-objective evaluator returns
    // a ``list[float]`` that we extract as ``Vec<f64>`` directly. The
    // ``EvaluatorErased`` blanket impl then flattens either via
    // ``to_vec()`` before handing the engine its final ``Vec<f64>``.
    type Objectives = Vec<f64>;

    fn cheap_constraints(&self, x: &XReal, _env: &Env) -> bool {
        Python::attach(|py| {
            let vals = x.as_f64_slice();
            match self.obj.call_method(py, "cheap_constraints", (vals,), None) {
                Ok(result) => result.extract::<bool>(py).unwrap_or(true),
                Err(_) => true,
            }
        })
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &Env, k: u32) -> (Vec<f64>, Vec<f64>) {
        Python::attach(|py| {
            let vals = x.as_f64_slice();
            let result = self
                .obj
                .call_method(py, "mc_sample", (vals, phi.tau.0, phi.smc.0, k), None)
                .expect("mc_sample call failed");
            // Accept either ``(float, list[float])`` (single-objective) or
            // ``(list[float], list[float])`` (multi-objective). Extract the
            // first tuple element as an opaque PyAny and attempt ``f64``
            // first, falling back to ``Vec<f64>``.
            let tup = result
                .cast_bound::<pyo3::types::PyTuple>(py)
                .expect("mc_sample must return a 2-tuple")
                .clone();
            if tup.len() != 2 {
                panic!(
                    "mc_sample must return a 2-tuple (objectives, constraints); got arity {}",
                    tup.len()
                );
            }
            let obj_any = tup.get_item(0).expect("mc_sample tuple missing objectives");
            let cons_any = tup.get_item(1).expect("mc_sample tuple missing constraints");
            let objs: Vec<f64> = if let Ok(single) = obj_any.extract::<f64>() {
                vec![single]
            } else {
                obj_any.extract::<Vec<f64>>().expect(
                    "mc_sample objective must be float or list[float]",
                )
            };
            let cons: Vec<f64> = cons_any
                .extract::<Vec<f64>>()
                .expect("mc_sample constraints must be list[float]");
            (objs, cons)
        })
    }

    fn num_objectives(&self) -> usize {
        self.n_obj
    }

    fn num_constraints(&self) -> usize {
        self.m
    }

    fn search_dim(&self) -> Option<usize> {
        Python::attach(|py| {
            match self.obj.call_method(py, "search_dim", (), None) {
                Ok(result) => result.extract::<usize>(py).ok(),
                Err(_) => None,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// PyEngine
// ---------------------------------------------------------------------------

#[pyclass(name = "Engine")]
struct PyEngine {
    inner: Engine<DefaultBundle>,
}

#[pymethods]
impl PyEngine {
    #[new]
    fn new() -> Self {
        Self {
            inner: Engine::<DefaultBundle>::default(),
        }
    }

    /// Run the engine with the built-in toy evaluator.
    #[pyo3(signature = (cfg, env, workers = 1))]
    fn run(&mut self, cfg: &PyEngineConfig, env: &PyEnv, workers: usize) -> PyEngineOutput {
        let out = self.inner.run(&cfg.inner, &env.inner, workers);
        PyEngineOutput { inner: out }
    }

    /// Run the engine with a custom Python evaluator object.
    ///
    /// The evaluator must implement:
    ///   - `mc_sample(x: list[float], tau: int, smc: int, k: int) -> (float | list[float], list[float])`
    ///     (single-objective → float, multi-objective → list[float])
    ///   - Optionally: `cheap_constraints(x: list[float]) -> bool`
    ///
    /// ``num_objectives`` defaults to 1 so existing single-objective call
    /// sites remain source-compatible with v2.0.1. Pass ``num_objectives > 1``
    /// to enable the multi-objective path, in which case ``mc_sample`` must
    /// return a ``list[float]`` of the appropriate length as its first tuple
    /// element.
    #[pyo3(signature = (cfg, env, evaluator, num_constraints, workers = 1, num_objectives = 1))]
    fn run_with_evaluator(
        &mut self,
        cfg: &PyEngineConfig,
        env: &PyEnv,
        evaluator: Py<PyAny>,
        num_constraints: usize,
        workers: usize,
        num_objectives: usize,
    ) -> PyEngineOutput {
        let eval: Arc<dyn EvaluatorErased> = Arc::new(PyEvaluator {
            obj: evaluator,
            m: num_constraints,
            n_obj: num_objectives,
        });
        let out =
            self.inner
                .run_with_evaluator(&cfg.inner, &env.inner, workers, eval);
        PyEngineOutput { inner: out }
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

#[pymodule]
fn _imads(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyEngineConfig>()?;
    m.add_class::<PyEnv>()?;
    m.add_class::<PyEngine>()?;
    m.add_class::<PyEngineOutput>()?;
    Ok(())
}
