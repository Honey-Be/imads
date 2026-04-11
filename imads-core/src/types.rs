//! Core type definitions for the IMADS framework.
//!
//! Includes mesh geometry, fidelity parameters, candidate lifecycle, and evaluation results.

use std::hash::{Hash, Hasher};

use typed_floats::{InvalidNumber, NonNaNFinite};

/// Number of parallel workers for the executor.
pub type WorkerCount = usize;

/// Unique identifier for a candidate point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateId(pub u64);

/// Mesh coordinates in base-lattice units (integer).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct XMesh(pub Vec<i64>);

/// Continuous-space coordinates guaranteed to be non-NaN and finite.
#[derive(Clone, Debug, PartialEq, Hash)]
pub struct XReal(pub(crate) Vec<NonNaNFinite<f64>>);

impl XReal {
    pub fn new(xr: impl Iterator<Item = f64>) -> Result<Self, InvalidNumber> {
        let mut out = Vec::new();
        for x in xr {
            match NonNaNFinite::<f64>::new(x) {
                Ok(v) => out.push(v),
                Err(e) => return Err(e),
            }
        }
        Ok(Self(out))
    }

    /// Returns the dimension count.
    pub fn dim(&self) -> usize {
        self.0.len()
    }

    /// Returns the coordinate values as a slice of `f64`.
    pub fn as_f64_slice(&self) -> Vec<f64> {
        self.0.iter().map(|v| f64::from(*v)).collect()
    }
}

/// Mesh geometry in base-lattice units.
///
/// - `base_step` is the size of one base lattice unit (Δ₀) in continuous space.
/// - `mesh_mul` is the current mesh spacing multiplier (Δ = Δ₀ * mesh_mul).
///
/// We refine the mesh by decreasing `mesh_mul` using integer division by `refine_div`.
/// This preserves nested meshes when `mesh_mul` is chosen to be divisible by `refine_div` repeatedly.
#[derive(Clone, Debug)]
pub struct MeshGeometry {
    pub base_step: f64,
    pub mesh_mul: i64,
    pub mesh_mul_min: i64,
    pub refine_div: i64,
    pub poll_step_mult: i64,
}

impl MeshGeometry {
    pub fn current_step(&self) -> f64 {
        self.base_step * (self.mesh_mul as f64)
    }

    /// Refine (shrink) the mesh spacing. This is the only adaptive mesh update in the
    /// conservative variant of the framework.
    pub fn refine(&mut self) {
        let div = self.refine_div.max(2);
        let minm = self.mesh_mul_min.max(1);
        if self.mesh_mul > minm {
            // Integer-only refinement.
            let next = self.mesh_mul / div;
            self.mesh_mul = next.max(minm);
        }
    }

    /// Poll step in base-lattice units.
    pub fn poll_step_units(&self) -> i64 {
        let mult = self.poll_step_mult.max(1);
        self.mesh_mul * mult
    }
}

/// Map a canonical mesh point into continuous space: `x_real = base_step * x_mesh`.
///
/// This is checked: if multiplication produces a non-finite value, the caller gets an
/// `InvalidNumber` instead of panicking.
pub fn mesh_to_real(x: &XMesh, base_step: f64) -> Result<XReal, InvalidNumber> {
    XReal::new(x.0.iter().map(|&u| (u as f64) * base_step))
}

/// Quantize a continuous point onto the current mesh, returning a canonical base-lattice coordinate.
///
/// The result coordinates are integer multiples of `mesh_mul` in base-lattice units.
pub fn quantize_real_to_mesh(x: &XReal, geo: &MeshGeometry) -> XMesh {
    let inv = 1.0 / geo.base_step;
    let m = geo.mesh_mul.max(1) as f64;
    let step = geo.mesh_mul.max(1);
    let mut out = Vec::with_capacity(x.0.len());
    for &xi in &x.0 {
        let z = f64::from(xi) * inv;
        let q = (z / m).round();
        let zi = (q as i64) * step;
        out.push(zi);
    }
    XMesh(out)
}

// --------------------------------
// Multi-objective support
// --------------------------------

/// Trait for objective function value containers.
///
/// Enables single-objective (`f64`), fixed multi-objective (`[f64; N]`),
/// and dynamic multi-objective (`Vec<f64>`) through a unified interface.
pub trait ObjectiveValues: Sized + Clone + Send + Sync + std::fmt::Debug {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn get(&self, i: usize) -> Option<f64>;
    /// Primary objective value (index 0). Used for single-objective compatibility.
    fn primary(&self) -> f64 {
        self.get(0).unwrap_or(f64::INFINITY)
    }
    fn as_slice(&self) -> &[f64];
    /// Convert to a Vec for internal engine use (avoids pervasive generics).
    fn to_vec(&self) -> Vec<f64> {
        self.as_slice().to_vec()
    }

    /// Construct a zero-valued instance with the given number of objectives.
    ///
    /// Used by [`Evaluator::solver_bias`]'s default implementation so that
    /// evaluators without a meaningful tau-dependent residual term don't
    /// need to override `solver_bias` manually. For fixed-arity types
    /// (`f64`, `[f64; N]`), the `n` argument is ignored — the arity is
    /// determined by the type itself.
    fn zero(n: usize) -> Self;
}

impl ObjectiveValues for f64 {
    fn len(&self) -> usize {
        1
    }
    fn get(&self, i: usize) -> Option<f64> {
        if i == 0 { Some(*self) } else { None }
    }
    fn primary(&self) -> f64 {
        *self
    }
    fn as_slice(&self) -> &[f64] {
        std::slice::from_ref(self)
    }
    fn to_vec(&self) -> Vec<f64> {
        vec![*self]
    }
    fn zero(_n: usize) -> Self {
        0.0
    }
}

impl ObjectiveValues for Vec<f64> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
    fn get(&self, i: usize) -> Option<f64> {
        self.as_slice().get(i).copied()
    }
    fn as_slice(&self) -> &[f64] {
        self
    }
    fn zero(n: usize) -> Self {
        vec![0.0; n]
    }
}

macro_rules! impl_objective_values_array {
    ($($n:literal),*) => {
        $(
            impl ObjectiveValues for [f64; $n] {
                fn len(&self) -> usize { $n }
                fn get(&self, i: usize) -> Option<f64> {
                    if i < $n { Some(self[i]) } else { None }
                }
                fn as_slice(&self) -> &[f64] { &self[..] }
                fn zero(_n: usize) -> Self { [0.0; $n] }
            }
        )*
    };
}

impl_objective_values_array!(1, 2, 3, 4, 5, 6, 7, 8);

/// Per-dimension (anisotropic) mesh geometry.
///
/// Each dimension has its own `base_step`, while `mesh_muls` are refined uniformly
/// to preserve nested mesh structure. The absolute step for dimension `i` is
/// `base_steps[i] * mesh_muls[i]`.
#[derive(Clone, Debug)]
pub struct AnisotropicMeshGeometry {
    pub base_steps: Vec<f64>,
    pub mesh_muls: Vec<i64>,
    pub mesh_mul_min: i64,
    pub refine_div: i64,
    pub poll_step_mult: i64,
}

impl AnisotropicMeshGeometry {
    /// Current step per dimension in continuous units.
    pub fn current_steps(&self) -> Vec<f64> {
        self.base_steps
            .iter()
            .zip(&self.mesh_muls)
            .map(|(&bs, &mm)| bs * (mm as f64))
            .collect()
    }

    /// Scalar current step (maximum across dimensions). Useful for search policies.
    pub fn current_step_max(&self) -> f64 {
        self.current_steps()
            .into_iter()
            .fold(0.0f64, f64::max)
    }

    /// Refine (shrink) all mesh multipliers uniformly by integer division.
    pub fn refine(&mut self) {
        let div = self.refine_div.max(2);
        let minm = self.mesh_mul_min.max(1);
        for mm in &mut self.mesh_muls {
            if *mm > minm {
                let next = *mm / div;
                *mm = next.max(minm);
            }
        }
    }

    /// Poll step in base-lattice units per dimension.
    pub fn poll_step_units(&self) -> Vec<i64> {
        let mult = self.poll_step_mult.max(1);
        self.mesh_muls.iter().map(|&mm| mm * mult).collect()
    }

    /// Dimension count.
    pub fn dim(&self) -> usize {
        self.base_steps.len()
    }
}

/// Convert an isotropic `MeshGeometry` into `AnisotropicMeshGeometry` by broadcasting
/// the scalar values to `dim` dimensions.
impl MeshGeometry {
    pub fn to_anisotropic(&self, dim: usize) -> AnisotropicMeshGeometry {
        AnisotropicMeshGeometry {
            base_steps: vec![self.base_step; dim],
            mesh_muls: vec![self.mesh_mul; dim],
            mesh_mul_min: self.mesh_mul_min,
            refine_div: self.refine_div,
            poll_step_mult: self.poll_step_mult,
        }
    }
}

/// Map a canonical mesh point into continuous space with per-dimension base steps.
pub fn mesh_to_real_aniso(x: &XMesh, base_steps: &[f64]) -> Result<XReal, InvalidNumber> {
    XReal::new(
        x.0.iter()
            .zip(base_steps)
            .map(|(&u, &bs)| (u as f64) * bs),
    )
}

/// Quantize a continuous point onto the current anisotropic mesh.
///
/// Each dimension is quantized independently using its own `base_step` and `mesh_mul`.
pub fn quantize_real_to_aniso_mesh(x: &XReal, geo: &AnisotropicMeshGeometry) -> XMesh {
    let mut out = Vec::with_capacity(x.0.len());
    for (i, &xi) in x.0.iter().enumerate() {
        let bs = geo.base_steps[i];
        let mm = geo.mesh_muls[i].max(1);
        let inv = 1.0 / bs;
        let z = f64::from(xi) * inv;
        let q = (z / (mm as f64)).round();
        let zi = (q as i64) * mm;
        out.push(zi);
    }
    XMesh(out)
}

/// Compute environment revision including per-dimension base steps.
///
/// This ensures cache keys are invalidated when anisotropic geometry changes.
pub fn env_rev_with_steps(env: &Env, base_steps: &[f64]) -> EnvRev {
    let mut h = Fnv1aHasher::new();
    env.run_id.hash(&mut h);
    env.config_hash.hash(&mut h);
    env.data_snapshot_id.hash(&mut h);
    env.rng_master_seed.hash(&mut h);
    for &s in base_steps {
        s.to_bits().hash(&mut h);
    }
    EnvRev(h.finish() as u128)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tau(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Smc(pub u32);

/// Fidelity step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Phi {
    pub tau: Tau,
    pub smc: Smc,
}

#[derive(Clone, Debug)]
pub struct Env {
    pub run_id: u128,
    pub config_hash: u128,
    pub data_snapshot_id: u128,
    pub rng_master_seed: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct EnvRev(pub u128);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PolicyRev(pub u64);

#[derive(Clone, Debug)]
pub struct EvalMeta {
    pub phi: Phi,
    pub env_rev: EnvRev,
    pub policy_rev: PolicyRev,
    pub runtime_cost: f64,
}

#[derive(Clone, Debug)]
pub struct Estimates {
    /// Objective estimate(s). Single-objective: `vec![f]`. Multi-objective: `vec![f0, f1, ...]`.
    pub f_hat: Vec<f64>,
    /// Objective standard error(s), same length as `f_hat`.
    pub f_se: Vec<f64>,
    pub c_hat: Vec<f64>,
    pub c_se: Vec<f64>,
    /// Exact fidelity bucket that produced this estimate.
    ///
    /// This intentionally duplicates `EvalMeta::phi` so policies that only receive
    /// `Estimates` can still use `(tau, S)` bucket information.
    pub phi: Phi,
    /// Tolerance scale used to model solver-bias terms.
    ///
    /// In the default margin policy, the effective bias bound is `K * tau_scale`.
    pub tau_scale: f64,
    /// Number of objectives (for convenience).
    pub num_objectives: usize,
}

impl Estimates {
    /// Primary objective estimate (index 0). Backward-compatible accessor.
    pub fn f_hat_primary(&self) -> f64 {
        self.f_hat.first().copied().unwrap_or(f64::INFINITY)
    }
    /// Primary objective SE (index 0).
    pub fn f_se_primary(&self) -> f64 {
        self.f_se.first().copied().unwrap_or(0.0)
    }
}

#[derive(Clone, Debug)]
pub enum JobResult {
    /// Rejected by cheap constraints (Stage A). No black-box evaluation performed.
    RejectedCheap { meta: EvalMeta },

    /// Rejected because evaluator input/output was invalid (e.g. non-finite values or
    /// mesh-to-real overflow). This is distinct from a cheap-constraint rejection.
    RejectedInvalidEval { meta: EvalMeta },

    EarlyInfeasible {
        violated_j: usize,
        estimates: Estimates,
        meta: EvalMeta,
    },

    Partial {
        estimates: Estimates,
        meta: EvalMeta,
    },

    Truth {
        /// Objective value(s). Single-objective: `vec![f]`.
        f: Vec<f64>,
        c: Vec<f64>,
        feasible: bool,
        v: f64,
        meta: EvalMeta,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CacheTag {
    Cheap,
    Partial,
    Truth,
}

/// Cache key for expensive evaluation outputs (`Estimates`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EvalCacheKey {
    pub x: XMesh,
    pub phi: Phi,
    pub env_rev: EnvRev,
}

/// Cache key for policy-dependent decisions/results (`JobResult`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DecisionCacheKey {
    pub x: XMesh,
    pub phi: Phi,
    pub env_rev: EnvRev,
    pub policy_rev: PolicyRev,
    pub tag: CacheTag,
}

/// Candidate lifecycle.
#[derive(Clone, Debug)]
pub enum CandidateStatus {
    Ready,
    InFlight { phi_idx: u32 },
    DoneRejectedCheap,
    DoneRejectedInvalidEval,
    DoneEarlyInfeasible { violated_j: usize, at_phi_idx: u32 },
    DoneStoppedPartial { at_phi_idx: u32 },
    DoneTruth,
}

#[derive(Clone, Debug)]
pub struct CandidateAuditOrigin {
    pub violated_j: usize,
    pub phi_at_cut: Phi,
    pub phi_idx_at_cut: u32,
    /// Same-S tighter-tau checkpoints collected along the audit path.
    ///
    /// These are stored in ladder order and let the engine/calibrator accumulate
    /// multiple paired-K samples instead of just a single checkpoint.
    pub paired_phi_indices: Vec<u32>,
}

/// Candidate state for resumable, step-wise evaluation.
#[derive(Clone, Debug)]
pub struct CandidateStageState {
    pub id: CandidateId,
    pub x: XMesh,

    /// Search-provided base priority (lower is better).
    pub base_score: f64,

    /// Monotone creation epoch (typically engine iteration index).
    ///
    /// Used for fairness/anti-starvation scoring in `ReadyCandidateView`.
    pub created_epoch: u64,

    pub next_phi_idx: u32,
    pub last_estimates_phi_idx: Option<u32>,
    pub last_estimates: Option<Estimates>,

    /// Sticky: once true, stays true.
    pub audit_required: bool,
    pub audit_origin: Option<CandidateAuditOrigin>,
    pub audit_cut_estimates: Option<Estimates>,

    pub cheap_checked: bool,
    pub cheap_ok: bool,

    /// Snapshot bookkeeping to keep step decisions reproducible.
    pub submitted_policy_rev: PolicyRev,
    pub submitted_incumbent_id: u64,

    pub status: CandidateStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadyKind {
    New,
    Resume,
}

/// View used by the scheduler to pick which candidates to dispatch next.
#[derive(Clone, Debug)]
pub struct ReadyCandidateView {
    pub id: CandidateId,
    pub kind: ReadyKind,
    pub x: XMesh,
    pub next_phi_idx: u32,
    pub score: f64,
    pub audit_required: bool,
}

/// A single unit of work: evaluate *one* fidelity step for one candidate.
#[derive(Clone, Debug)]
pub struct WorkItem {
    pub cand_id: CandidateId,
    pub x: XMesh,
    pub phi_idx: u32,
    pub phi: Phi,
    pub env_rev: EnvRev,
    pub policy_rev: PolicyRev,
    pub incumbent_id: u64,
}

// ------------------------
// Deterministic hashing
// ------------------------

/// Deterministic FNV-1a hasher.
#[derive(Default)]
struct Fnv1aHasher(u64);

impl Fnv1aHasher {
    fn new() -> Self {
        // FNV offset basis
        Self(0xcbf29ce484222325u64)
    }
}

impl Hasher for Fnv1aHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        const FNV_PRIME: u64 = 0x00000100000001B3;
        let mut hash = self.0;
        for b in bytes {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        self.0 = hash;
    }
}

pub fn env_rev(env: &Env) -> EnvRev {
    let mut h = Fnv1aHasher::new();
    env.run_id.hash(&mut h);
    env.config_hash.hash(&mut h);
    env.data_snapshot_id.hash(&mut h);
    env.rng_master_seed.hash(&mut h);
    EnvRev(h.finish() as u128)
}

/// Deterministic audit selector helper.
pub fn stable_hash_u64<T: Hash>(value: &T) -> u64 {
    let mut h = Fnv1aHasher::new();
    value.hash(&mut h);
    h.finish()
}
