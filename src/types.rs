use std::hash::{Hash, Hasher};

use typed_floats::{InvalidNumber, NonNaNFinite};

pub type WorkerCount = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CandidateId(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct XMesh(pub Vec<i64>);

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
    pub f_hat: f64,
    pub f_se: f64,
    pub c_hat: Vec<f64>,
    pub c_se: Vec<f64>,
    /// Tolerance scale used to model solver-bias terms.
    ///
    /// In the default margin policy, the effective bias bound is `K * tau_scale`.
    pub tau_scale: f64,
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
        f: f64,
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
    pub paired_phi_idx: Option<u32>,
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
