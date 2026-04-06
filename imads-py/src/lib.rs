//! Python bindings for IMADS via PyO3.

use std::sync::Arc;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use imads_core::core::engine::{Engine, EngineConfig, EngineOutput};
use imads_core::core::evaluator::Evaluator;
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
    #[getter]
    fn f_best(&self) -> Option<f64> {
        self.inner.f_best
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
            self.inner.f_best, self.inner.stats.truth_evals, self.inner.stats.partial_steps,
        )
    }
}

// ---------------------------------------------------------------------------
// PyEvaluator — wraps a Python object implementing the evaluator protocol
// ---------------------------------------------------------------------------

struct PyEvaluator {
    obj: Py<PyAny>,
    m: usize,
}

impl std::fmt::Debug for PyEvaluator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PyEvaluator")
            .field("num_constraints", &self.m)
            .finish()
    }
}

// Safety: GIL protects all Python interactions.
unsafe impl Send for PyEvaluator {}
unsafe impl Sync for PyEvaluator {}

impl Evaluator for PyEvaluator {
    fn cheap_constraints(&self, x: &XReal, _env: &Env) -> bool {
        Python::attach(|py| {
            let vals = x.as_f64_slice();
            match self.obj.call_method(py, "cheap_constraints", (vals,), None) {
                Ok(result) => result.extract::<bool>(py).unwrap_or(true),
                Err(_) => true,
            }
        })
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, _env: &Env, k: u32) -> (f64, Vec<f64>) {
        Python::attach(|py| {
            let vals = x.as_f64_slice();
            let result = self
                .obj
                .call_method(py, "mc_sample", (vals, phi.tau.0, phi.smc.0, k), None)
                .expect("mc_sample call failed");
            let (f, c): (f64, Vec<f64>) = result.extract(py).expect("mc_sample return type error");
            (f, c)
        })
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
    ///   - `mc_sample(x: list[float], tau: int, smc: int, k: int) -> (float, list[float])`
    ///   - Optionally: `cheap_constraints(x: list[float]) -> bool`
    #[pyo3(signature = (cfg, env, evaluator, num_constraints, workers = 1))]
    fn run_with_evaluator(
        &mut self,
        cfg: &PyEngineConfig,
        env: &PyEnv,
        evaluator: Py<PyAny>,
        num_constraints: usize,
        workers: usize,
    ) -> PyEngineOutput {
        let eval: Arc<dyn Evaluator> = Arc::new(PyEvaluator {
            obj: evaluator,
            m: num_constraints,
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
