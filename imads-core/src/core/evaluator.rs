use crate::types::{Env, ObjectiveValues, Phi, Tau, XReal, stable_hash_u64};

/// Black-box evaluator interface with associated objective type.
///
/// In a real integration this would call your expensive simulator/solver.
///
/// # Contract
/// - Must be deterministic w.r.t (x_mesh, phi=(tau,S), env).
/// - If you use RNG, derive seeds deterministically from the inputs (e.g., hash).
/// - Prefer MC **prefix reuse**: results for S_i should be reusable as prefix of S_{i+1}.
pub trait Evaluator: std::fmt::Debug + Send + Sync {
    /// The objective value container type.
    type Objectives: ObjectiveValues;

    /// Cheap constraints gate (Stage A). Return false to reject without black-box evaluation.
    fn cheap_constraints(&self, _x: &XReal, _env: &Env) -> bool {
        true
    }

    /// Return a deterministic sample of the objective and constraint values for a given sample index.
    ///
    /// `k` is 0-based and must be stable across runs.
    fn mc_sample(&self, x: &XReal, phi: Phi, env: &Env, k: u32) -> (Self::Objectives, Vec<f64>);

    /// Optional: deterministic, tau-dependent bias term (e.g., solver residual effects).
    fn solver_bias(&self, _x: &XReal, _tau: Tau, _env: &Env) -> (Self::Objectives, Vec<f64>) {
        // Default: zero bias. For single objective, Objectives must impl Default-like behavior.
        // Since we can't construct Self::Objectives generically, implementors should override.
        unimplemented!("solver_bias requires explicit implementation for multi-objective evaluators")
    }

    /// Number of objectives.
    fn num_objectives(&self) -> usize;

    /// Number of constraints.
    fn num_constraints(&self) -> usize;

    /// Number of continuous search dimensions.
    ///
    /// When the engine config's `search_dim` is `None`, the engine queries
    /// this method to determine the dimensionality of the search space.
    /// The default returns `None`, meaning "use config or incumbent length".
    fn search_dim(&self) -> Option<usize> {
        None
    }
}

/// Marker trait: single-objective evaluator (Objectives = f64).
pub trait SingleObjectiveEvaluator: Evaluator<Objectives = f64> {}
impl<E: Evaluator<Objectives = f64>> SingleObjectiveEvaluator for E {}

/// Type-erased evaluator wrapper for internal engine use.
///
/// The engine works with `Vec<f64>` objectives internally to avoid generic infection.
/// This trait provides a non-generic interface.
pub trait EvaluatorErased: std::fmt::Debug + Send + Sync {
    fn cheap_constraints(&self, x: &XReal, env: &Env) -> bool;
    fn mc_sample(&self, x: &XReal, phi: Phi, env: &Env, k: u32) -> (Vec<f64>, Vec<f64>);
    fn solver_bias(&self, x: &XReal, tau: Tau, env: &Env) -> (Vec<f64>, Vec<f64>);
    fn num_objectives(&self) -> usize;
    fn num_constraints(&self) -> usize;
    fn search_dim(&self) -> Option<usize>;
}

/// Blanket implementation: any concrete Evaluator can be used as EvaluatorErased.
impl<E: Evaluator> EvaluatorErased for E {
    fn cheap_constraints(&self, x: &XReal, env: &Env) -> bool {
        Evaluator::cheap_constraints(self, x, env)
    }
    fn mc_sample(&self, x: &XReal, phi: Phi, env: &Env, k: u32) -> (Vec<f64>, Vec<f64>) {
        let (obj, cons) = Evaluator::mc_sample(self, x, phi, env, k);
        (obj.to_vec(), cons)
    }
    fn solver_bias(&self, x: &XReal, tau: Tau, env: &Env) -> (Vec<f64>, Vec<f64>) {
        let (obj, cons) = Evaluator::solver_bias(self, x, tau, env);
        (obj.to_vec(), cons)
    }
    fn num_objectives(&self) -> usize {
        Evaluator::num_objectives(self)
    }
    fn num_constraints(&self) -> usize {
        Evaluator::num_constraints(self)
    }
    fn search_dim(&self) -> Option<usize> {
        Evaluator::search_dim(self)
    }
}

/// A tiny deterministic toy evaluator used by tests.
///
/// - Objective base: sum(x_i^2)
/// - Constraints base: sum(x_i) - (j+1)
/// - MC noise: deterministic pseudo-random, scaled down by sqrt(S)
/// - Solver bias: proportional to tau
#[derive(Clone, Debug, Default)]
pub struct ToyEvaluator {
    pub m: usize,
    pub dim: usize,
}

impl ToyEvaluator {
    fn prng_u64(seed: u64) -> u64 {
        // SplitMix64
        let mut z = seed.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    fn u01(seed: u64) -> f64 {
        // [0,1)
        let x = Self::prng_u64(seed);
        (x >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

impl Evaluator for ToyEvaluator {
    type Objectives = f64;

    fn cheap_constraints(&self, _x: &XReal, _env: &Env) -> bool {
        true
    }

    fn mc_sample(&self, x: &XReal, phi: Phi, env: &Env, k: u32) -> (f64, Vec<f64>) {
        let sum: f64 = x.0.iter().map(|&xi| f64::from(xi)).sum();
        let f_det =
            x.0.iter()
                .map(|v| f64::from(*v) * f64::from(*v))
                .sum::<f64>();

        // Deterministic noise seed
        let key = (
            stable_hash_u64(x),
            phi,
            env.run_id,
            env.config_hash,
            env.data_snapshot_id,
            env.rng_master_seed,
            k,
        );
        let h = stable_hash_u64(&key);
        let u = Self::u01(h);
        let noise = (u - 0.5) * 2.0; // [-1,1]
        let s = (phi.smc.0.max(1) as f64).sqrt();

        // Objective noise
        let f = f_det + noise / s;
        // Constraint noise shares the same seed deterministically.
        let mut c = Vec::with_capacity(self.m);
        for j in 0..self.m {
            let hj = stable_hash_u64(&(h, j as u64));
            let uj = Self::u01(hj);
            let nj = (uj - 0.5) * 2.0;
            c.push(sum - (j as f64 + 1.0) + nj / s);
        }
        (f, c)
    }

    fn solver_bias(&self, x: &XReal, tau: Tau, _env: &Env) -> (f64, Vec<f64>) {
        // A tiny deterministic tau-dependent bias.
        let t = tau.0 as f64;
        let mag = (x.0.len().max(1) as f64).sqrt();
        let fb = 1e-6 * t * mag;
        let mut cb = Vec::with_capacity(self.m);
        for j in 0..self.m {
            cb.push(1e-6 * t * (j as f64 + 1.0));
        }
        (fb, cb)
    }

    fn num_objectives(&self) -> usize {
        1
    }

    fn num_constraints(&self) -> usize {
        self.m
    }

    fn search_dim(&self) -> Option<usize> {
        if self.dim > 0 { Some(self.dim) } else { None }
    }
}
