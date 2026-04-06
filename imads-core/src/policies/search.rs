use crate::types::{CandidateId, Env, MeshGeometry, XMesh, XReal, quantize_real_to_mesh};

#[derive(Clone, Debug)]
pub struct RawCandidate {
    pub id: CandidateId,
    pub x: Vec<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchHints {
    pub incumbent_score: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub iter: u64,
}

/// Context provided by the engine at each iteration.
///
/// This enables practical search policies without baking engine internals into the trait.
#[derive(Clone, Debug, Default)]
pub struct SearchContext {
    /// Problem dimension (length of `x`).
    pub dim: usize,
    /// Incumbent point in continuous coordinates (base lattice scaled), if available.
    pub incumbent_x: Option<Vec<f64>>,
    /// Current mesh step in continuous units (Δ = Δ₀ * mesh_mul).
    pub mesh_step: f64,
}

/// Search policy. Safe to customize.
///
/// # Contract
/// - Must be deterministic given inputs.
/// - Engine will quantize `RawCandidate` into `XMesh`.
pub trait SearchPolicy: Send + Sync {
    fn reset(&mut self, env: &Env);

    /// Provide iteration-local context (dimension, incumbent, mesh step).
    ///
    /// Default implementation is a no-op so custom policies need not implement it.
    fn set_context(&mut self, _ctx: &SearchContext) {}

    /// Produce `budget` new raw candidates.
    fn propose(&mut self, state: &SearchState, budget: usize) -> Vec<RawCandidate>;

    /// Score a raw candidate for priority ordering. Lower is better.
    fn score(&mut self, cand: &RawCandidate, hints: &SearchHints) -> f64;
}

/// Deterministic SplitMix64 PRNG (good enough for search jitter / exploration).
#[derive(Clone, Debug)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        // splitmix64
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_f64(&mut self) -> f64 {
        // Uniform in [0, 1).
        const DEN: f64 = 1.0 / ((1u64 << 53) as f64);
        let u = self.next_u64() >> 11;
        (u as f64) * DEN
    }

    fn next_i64_range(&mut self, lo: i64, hi_exclusive: i64) -> i64 {
        debug_assert!(hi_exclusive > lo);
        let span = (hi_exclusive - lo) as u64;
        lo + (self.next_u64() % span) as i64
    }

    fn next_sign(&mut self) -> f64 {
        if (self.next_u64() & 1) == 0 {
            -1.0
        } else {
            1.0
        }
    }
}

/// Practical default search policy (1st upgrade):
///
/// - Mixture of local coordinate steps around the incumbent (when available)
/// - Deterministic global exploration with scale increasing slowly over iterations
///
/// This is conservative and deterministic, aimed at good baseline behavior.
pub struct DefaultSearch {
    next: u64,
    rng: SplitMix64,
    ctx: SearchContext,
}

impl Default for DefaultSearch {
    fn default() -> Self {
        Self {
            next: 0,
            rng: SplitMix64::new(0),
            ctx: SearchContext::default(),
        }
    }
}

impl SearchPolicy for DefaultSearch {
    fn reset(&mut self, env: &Env) {
        self.next = 0;
        self.rng = SplitMix64::new(env.rng_master_seed as u64 ^ (env.run_id as u64));
        self.ctx = SearchContext::default();
    }

    fn set_context(&mut self, ctx: &SearchContext) {
        self.ctx = ctx.clone();
    }

    fn propose(&mut self, state: &SearchState, budget: usize) -> Vec<RawCandidate> {
        let dim = self.ctx.dim.max(1);
        let step = if self.ctx.mesh_step.is_finite() && self.ctx.mesh_step > 0.0 {
            self.ctx.mesh_step
        } else {
            1.0
        };

        let mut out = Vec::with_capacity(budget);
        for _ in 0..budget {
            let id = CandidateId((state.iter << 32) | self.next);

            // Mode: exploit (local) vs explore (global).
            let use_local = self.ctx.incumbent_x.is_some() && (self.rng.next_f64() < 0.70);

            let x = if use_local {
                let mut x = self
                    .ctx
                    .incumbent_x
                    .clone()
                    .unwrap_or_else(|| vec![0.0; dim]);
                if x.len() != dim {
                    x.resize(dim, 0.0);
                }

                // 1D coordinate step (occasionally 2D).
                let k = (self.rng.next_u64() as usize) % dim;
                let mult = 1.0 + ((self.rng.next_u64() % 4) as f64); // 1..4 steps
                x[k] += self.rng.next_sign() * mult * step;

                if dim >= 2 && (self.rng.next_f64() < 0.25) {
                    let k2 = ((k as i64 + self.rng.next_i64_range(1, dim as i64)) as usize) % dim;
                    let mult2 = 1.0 + ((self.rng.next_u64() % 3) as f64);
                    x[k2] += self.rng.next_sign() * mult2 * step;
                }

                x
            } else {
                // Global exploration with slowly increasing radius.
                let t = 1.0 + (state.iter as f64).ln_1p();
                let radius = step * 8.0 * t;
                let mut x = vec![0.0; dim];
                for x_item in x.iter_mut() {
                    let u = self.rng.next_f64();
                    *x_item = (2.0 * u - 1.0) * radius;
                }
                x
            };

            out.push(RawCandidate { id, x });
            self.next += 1;
        }
        out
    }

    fn score(&mut self, cand: &RawCandidate, _hints: &SearchHints) -> f64 {
        // Prefer candidates close to incumbent (if present); otherwise near origin.
        let dim = self.ctx.dim.max(1);
        if let Some(ix) = self.ctx.incumbent_x.as_ref() {
            let mut s = 0.0;
            for i in 0..dim {
                let a = cand.x.get(i).copied().unwrap_or(0.0);
                let b = ix.get(i).copied().unwrap_or(0.0);
                let d = a - b;
                s += d * d;
            }
            s
        } else {
            cand.x.iter().map(|v| v * v).sum::<f64>()
        }
    }
}

/// Deterministic quantization into a mesh coordinate (base-lattice units).
///
/// The engine uses a base lattice with step `geo.base_step` and a current mesh multiplier `geo.mesh_mul`.
/// This function snaps `raw.x` onto the current mesh and returns a canonical `XMesh`.
pub fn project_to_mesh(raw: &RawCandidate, geo: &MeshGeometry) -> XMesh {
    let xr = XReal::new(raw.x.clone().into_iter()).unwrap();
    quantize_real_to_mesh(&xr, geo)
}
