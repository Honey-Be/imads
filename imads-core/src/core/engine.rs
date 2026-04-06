use crate::backends::cache::{DecisionCacheBackend, EvalCacheBackend};
use crate::core::acceptance::{
    AcceptanceConfig, AcceptanceEngine, DefaultAcceptance, TruthDecision,
};
use crate::core::evaluator::{Evaluator, ToyEvaluator};
use crate::core::executor::{AdaptiveExecutor, ExecCtx, Executor, ExecutorParams, WorkOutcome};
use crate::core::poll::DefaultPoll;
use crate::policies::{
    AuditOf, AuditPolicy, CalibEvent, CalibratorConfig, CalibratorPolicy, DidsPolicy, LadderPolicy,
    MarginPolicy, SchedulerPolicy, SearchPolicy,
};
use crate::types::{
    CacheTag, CandidateAuditOrigin, CandidateId, CandidateStageState, CandidateStatus,
    DecisionCacheKey, Env, Estimates, EvalMeta, JobResult, MeshGeometry, Phi, PolicyRev,
    ReadyCandidateView, ReadyKind, Smc, Tau, WorkItem, XMesh, env_rev, mesh_to_real,
};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use bitflags::bitflags;

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub tau_levels: Vec<Tau>,
    pub smc_levels: Vec<Smc>,
    pub mesh_base_step: f64,
    /// Initial mesh multiplier in base-lattice units (Δ = Δ₀ * mesh_mul).
    pub mesh_mul_init: i64,
    /// Minimum mesh multiplier (finest mesh).
    pub mesh_mul_min: i64,
    /// Refinement divisor (integer-only): on failure mesh_mul <- max(mesh_mul_min, mesh_mul / refine_div).
    pub mesh_refine_div: i64,
    /// Poll step multiplier in base-lattice units.
    pub poll_step_mult: i64,
    pub max_iters: u64,
    pub candidates_per_iter: usize,
    /// Search space dimension (length of x in continuous space).
    ///
    /// When `None`, the engine obtains the dimension from the evaluator's
    /// `search_dim()` method or from the incumbent's length.
    pub search_dim: Option<usize>,
    /// Maximum number of one-step work items executed per engine iteration.
    ///
    /// - `None`: drain all Ready candidates each iteration (v3 behavior).
    /// - `Some(k)`: execute at most `k` `WorkItem`s per iteration, leaving the rest to resume.
    pub max_steps_per_iter: Option<usize>,
    pub num_constraints: usize,

    // Acceptance (Filter + progressive barrier) parameters.
    pub accept_h0: f64,
    pub accept_h_min: f64,
    pub accept_h_shrink: f64,
    pub accept_eps_f: f64,
    pub accept_eps_v: f64,
    pub accept_filter_cap: usize,
    // Calibrator (delta controller) parameters.
    pub calibrator_target_false: f64,
    pub calibrator_min_audits: usize,
    pub calibrator_eta_delta: f64,
    pub calibrator_delta_min: f64,
    pub calibrator_delta_max: f64,
    /// K-learning: window size for stored tau-normalized samples.
    pub calibrator_k_window: usize,
    /// Minimum number of paired audit samples before K updates are enabled.
    pub calibrator_k_min_pairs: usize,
    /// Quantile in \[0,1\] used for conservative K updates.
    pub calibrator_k_quantile: f64,
    /// EMA step size for K updates.
    pub calibrator_k_eta: f64,

    // Objective pruning gate parameters.
    /// 1-based rank among distinct SMC levels required before objective pruning is allowed.
    pub objective_prune_min_smc_rank: usize,
    /// Minimum 1-based ladder level before objective pruning is allowed.
    pub objective_prune_min_level: usize,
    /// If true, objective pruning is additionally restricted to the back half of the ladder.
    pub objective_prune_require_back_half: bool,
    /// If true, audit-required candidates bypass objective pruning.
    pub objective_prune_disable_for_audit: bool,
    /// Batch boundary size for policy updates (calibrator/DIDS assignment).
    ///
    /// If `None`, the engine uses the scheduler's `batch_size()`.
    pub batch_boundary: Option<usize>,

    /// Executor chunking base (performance knob).
    ///
    /// The worker pool caps the effective chunk to `ceil(batch_size / workers)` to avoid
    /// hoarding when no work-stealing is used.
    pub executor_chunk_base: usize,

    /// Minimum chunk base for the worker pool.
    pub executor_chunk_min: usize,
    /// Maximum chunk base for the worker pool.
    pub executor_chunk_max: usize,
    /// Spin iterations before condvar barrier wait in the executor.
    pub executor_spin_limit: usize,
    /// Enable online auto-tuning of `executor_chunk_base` based on batch cost variance.
    pub executor_chunk_auto_tune: bool,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ConfigError : u32 {
        const CalibratorTargetFalse = 0b000000000000000000000001;
        const CalibratorMinAudits = 0b000000000000000000000010;
        const CalibratorEtaDelta = 0b000000000000000000000100;
        const CalibratorDeltaRange = 0b000000000000000000001000;
        const CalibratorKWindow = 0b000000000000000000010000;
        const CalibratorKMinPairs = 0b000000000000000000100000;
        const CalibratorKQuantile = 0b000000000000000001000000;
        const CalibratorKEta = 0b000000000000000010000000;
        const AcceptH0 = 0b000000000000000100000000;
        const AcceptHMin = 0b000000000000001000000000;
        const AcceptHShrink = 0b000000000000010000000000;
        const AcceptEpsF = 0b000000000000100000000000;
        const AcceptEpsV = 0b000000000001000000000000;
        const MeshBaseStep = 0b000000000010000000000000;
        const MeshMulInit = 0b000000000100000000000000;
        const MeshMulMin = 0b000000001000000000000000;
        const MeshRefineDiv = 0b000000010000000000000000;
        const PollStepMult = 0b000000100000000000000000;
        const ExecutorChunkBaseRange = 0b000001000000000000000000;
        const SearchDim = 0b000010000000000000000000;
        const ObjectivePruneMinSmcRank = 0b000100000000000000000000;
        const ObjectivePruneMinLevel = 0b001000000000000000000000;
    }
}

impl EngineConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tau_levels: Vec<Tau>,
        smc_levels: Vec<Smc>,
        mesh_base_step: f64,
        mesh_mul_init: i64,
        mesh_mul_min: i64,
        mesh_refine_div: i64,
        poll_step_mult: i64,
        max_iters: u64,
        candidates_per_iter: usize,
        search_dim: Option<usize>,
        max_steps_per_iter: Option<usize>,
        num_constraints: usize,
        accept_h0: f64,
        accept_h_min: f64,
        accept_h_shrink: f64,
        accept_eps_f: f64,
        accept_eps_v: f64,
        accept_filter_cap: usize,
        calibrator_target_false: f64,
        calibrator_min_audits: usize,
        calibrator_eta_delta: f64,
        calibrator_delta_min: f64,
        calibrator_delta_max: f64,
        calibrator_k_window: usize,
        calibrator_k_min_pairs: usize,
        calibrator_k_quantile: f64,
        calibrator_k_eta: f64,
        objective_prune_min_smc_rank: usize,
        objective_prune_min_level: usize,
        objective_prune_require_back_half: bool,
        objective_prune_disable_for_audit: bool,
        batch_boundary: Option<usize>,
        executor_chunk_base: usize,
        executor_chunk_min: usize,
        executor_chunk_max: usize,
        executor_spin_limit: usize,
        executor_chunk_auto_tune: bool,
    ) -> Result<Self, ConfigError> {
        let mut err: ConfigError = ConfigError::empty();
        if !(0.0..=1.0).contains(&calibrator_target_false) {
            err |= ConfigError::CalibratorTargetFalse;
        }
        if calibrator_min_audits < 1 {
            err |= ConfigError::CalibratorMinAudits;
        }
        if !(calibrator_eta_delta.is_finite() && calibrator_eta_delta > 0.0) {
            err |= ConfigError::CalibratorEtaDelta;
        }
        if !(calibrator_delta_min.is_finite()
            && calibrator_delta_max.is_finite()
            && calibrator_delta_min >= 0.0
            && calibrator_delta_min <= calibrator_delta_max)
        {
            err |= ConfigError::CalibratorDeltaRange;
        }
        if calibrator_k_window < 1 {
            err |= ConfigError::CalibratorKWindow;
        }
        if !(calibrator_k_min_pairs >= 1 && calibrator_k_min_pairs <= calibrator_k_window.max(1)) {
            err |= ConfigError::CalibratorKMinPairs;
        }
        if !(calibrator_k_quantile.is_finite() && (0.0..=1.0).contains(&calibrator_k_quantile)) {
            err |= ConfigError::CalibratorKQuantile;
        }
        if !(calibrator_k_eta.is_finite() && calibrator_k_eta > 0.0) {
            err |= ConfigError::CalibratorKEta;
        }
        if objective_prune_min_smc_rank < 1 {
            err |= ConfigError::ObjectivePruneMinSmcRank;
        }
        if objective_prune_min_level < 1 {
            err |= ConfigError::ObjectivePruneMinLevel;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(accept_h0 >= 0.0) {
            err |= ConfigError::AcceptH0;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(accept_h_min >= 0.0) {
            err |= ConfigError::AcceptHMin;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(accept_h_min <= accept_h0) {
            err |= ConfigError::AcceptH0 | ConfigError::AcceptHMin;
        }
        if !(0.0..=1.0).contains(&accept_h_shrink) {
            err |= ConfigError::AcceptHShrink;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(accept_eps_f >= 0.0) {
            err |= ConfigError::AcceptEpsF;
        }
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(accept_eps_v >= 0.0) {
            err |= ConfigError::AcceptEpsV;
        }

        if !(mesh_base_step.is_finite() && mesh_base_step > 0.0) {
            err |= ConfigError::MeshBaseStep;
        }
        if mesh_mul_min < 1 {
            err |= ConfigError::MeshMulMin;
        }
        if mesh_mul_init < mesh_mul_min.max(1i64) {
            err |= ConfigError::MeshMulInit;
        }
        if mesh_refine_div < 2 {
            err |= ConfigError::MeshRefineDiv;
        }
        if poll_step_mult < 1 {
            err |= ConfigError::PollStepMult;
        }
        if search_dim == Some(0) {
            err |= ConfigError::SearchDim;
        }
        if !(executor_chunk_min..=executor_chunk_max).contains(&executor_chunk_base) {
            err |= ConfigError::ExecutorChunkBaseRange;
        }

        if err.is_empty() {
            Ok(Self {
                tau_levels,
                smc_levels,
                mesh_base_step,
                mesh_mul_init,
                mesh_mul_min,
                mesh_refine_div,
                poll_step_mult,
                max_iters,
                candidates_per_iter,
                search_dim,
                max_steps_per_iter,
                num_constraints,
                accept_h0,
                accept_h_min,
                accept_h_shrink,
                accept_eps_f,
                accept_eps_v,
                accept_filter_cap,
                calibrator_target_false,
                calibrator_min_audits,
                calibrator_eta_delta,
                calibrator_delta_min,
                calibrator_delta_max,
                calibrator_k_window,
                calibrator_k_min_pairs,
                calibrator_k_quantile,
                calibrator_k_eta,
                objective_prune_min_smc_rank,
                objective_prune_min_level,
                objective_prune_require_back_half,
                objective_prune_disable_for_audit,
                batch_boundary,
                executor_chunk_base,
                executor_chunk_min,
                executor_chunk_max,
                executor_spin_limit,
                executor_chunk_auto_tune,
            })
        } else {
            Err(err)
        }
    }
}

// ----------------------------
// Ladder defaults / automation
// ----------------------------

fn default_tau_levels(dim: usize) -> Vec<Tau> {
    // Heuristic: 3-level tau ladder (loose -> tight).
    // Larger tau => looser solver tolerance (higher bias but (usually) cheaper).
    // For higher-dimensional problems, we add one extra very-loose level to improve screening.
    if dim >= 64 {
        vec![Tau(1000), Tau(100), Tau(10), Tau(1)]
    } else {
        vec![Tau(100), Tau(10), Tau(1)]
    }
}

fn default_smc_levels(dim: usize) -> Vec<Smc> {
    // Heuristic: 3-level MC ladder (low -> high).
    // Scale up the base sample count with dimension to keep estimator variance in check.
    let d = dim.max(1);
    let base: u32 = if d <= 8 {
        8
    } else if d <= 32 {
        16
    } else {
        32
    };
    let mid = (base.saturating_mul(4)).max(base + 1);
    let high = (mid.saturating_mul(4)).max(mid + 1);
    vec![Smc(base), Smc(mid), Smc(high)]
}

fn collect_paired_phi_indices(ladder: &[Phi], cut_idx: u32) -> Vec<u32> {
    let i = cut_idx as usize;
    if i >= ladder.len() {
        return Vec::new();
    }
    let cur = ladder[i];
    ladder
        .iter()
        .enumerate()
        .skip(i + 1)
        .filter_map(|(j, p)| {
            if p.smc == cur.smc && p.tau.0 < cur.tau.0 {
                Some(j as u32)
            } else {
                None
            }
        })
        .collect()
}

fn is_paired_checkpoint(origin: Option<&CandidateAuditOrigin>, phi_idx: u32) -> bool {
    origin
        .map(|o| o.paired_phi_indices.binary_search(&phi_idx).is_ok())
        .unwrap_or(false)
}

fn next_audit_target_idx(
    origin: Option<&CandidateAuditOrigin>,
    current_idx: u32,
    ladder_len: usize,
) -> u32 {
    if ladder_len == 0 {
        return 0;
    }
    if let Some(origin) = origin {
        for &idx in origin.paired_phi_indices.iter() {
            if idx > current_idx {
                return idx;
            }
        }
    }
    (ladder_len - 1) as u32
}

fn audit_of_from_origin(origin: &CandidateAuditOrigin) -> AuditOf {
    AuditOf {
        violated_j: origin.violated_j,
        phi_at_cut: origin.phi_at_cut,
        phi_idx_at_cut: origin.phi_idx_at_cut,
    }
}

fn truth_as_estimates(phi: Phi, f: f64, c: &[f64]) -> Estimates {
    Estimates {
        f_hat: f,
        f_se: 0.0,
        c_hat: c.to_vec(),
        c_se: vec![0.0; c.len()],
        phi,
        tau_scale: phi.tau.0 as f64,
    }
}

fn paired_audit_sample_from_estimates(
    origin: Option<&CandidateAuditOrigin>,
    phi_idx: u32,
    phi: Phi,
    estimates: &Estimates,
) -> Option<crate::policies::PairedAuditSample> {
    if is_paired_checkpoint(origin, phi_idx) {
        Some(crate::policies::PairedAuditSample {
            paired_phi: phi,
            paired_phi_idx: phi_idx,
            estimates: estimates.clone(),
        })
    } else {
        None
    }
}

fn resolve_ladder_levels(cfg: &EngineConfig, dim: usize) -> (Vec<Tau>, Vec<Smc>) {
    let mut tau = if cfg.tau_levels.is_empty() {
        default_tau_levels(dim)
    } else {
        cfg.tau_levels.clone()
    };
    let mut smc = if cfg.smc_levels.is_empty() {
        default_smc_levels(dim)
    } else {
        cfg.smc_levels.clone()
    };

    tau.retain(|t| t.0 > 0);
    smc.retain(|s| s.0 > 0);

    if tau.is_empty() {
        tau.push(Tau(1));
    }
    if smc.is_empty() {
        smc.push(Smc(1));
    }

    (tau, smc)
}

pub(crate) fn objective_pruning_allowed(
    phi_idx: usize,
    ladder: &[Phi],
    audit_required: bool,
    cfg: &EngineConfig,
) -> bool {
    if ladder.is_empty() || phi_idx >= ladder.len() {
        return false;
    }

    if audit_required && cfg.objective_prune_disable_for_audit {
        return false;
    }

    let level = phi_idx + 1;
    if level < cfg.objective_prune_min_level.max(1) {
        return false;
    }

    let current = ladder[phi_idx];
    let mut smc_levels: Vec<u32> = ladder.iter().map(|p| p.smc.0).collect();
    smc_levels.sort_unstable();
    smc_levels.dedup();

    let rank_idx = cfg.objective_prune_min_smc_rank.max(1) - 1;
    let smc_threshold = smc_levels
        .get(rank_idx)
        .copied()
        .unwrap_or_else(|| *smc_levels.last().unwrap_or(&current.smc.0));
    let smc_ok = current.smc.0 >= smc_threshold;
    if !smc_ok {
        return false;
    }

    if cfg.objective_prune_require_back_half {
        let ladder_mid = ladder.len() / 2 + 1;
        if level < ladder_mid.max(2) {
            return false;
        }
    }

    true
}

#[derive(Clone, Debug, Default)]
pub struct EngineStats {
    pub truth_evals: u64,
    pub truth_decision_cache_hits: u64,
    pub truth_eval_cache_hits: u64,

    pub partial_steps: u64,
    pub partial_decision_cache_hits: u64,
    pub partial_eval_cache_hits: u64,

    pub cheap_rejects: u64,
    pub invalid_eval_rejects: u64,
}

#[derive(Clone, Debug)]
pub struct EngineOutput {
    pub x_best: Option<XMesh>,
    pub f_best: Option<f64>,
    pub stats: EngineStats,
}

/// Bundle of policies/backends; intended to be customized at build time.
pub trait PolicyBundle {
    type Scheduler: SchedulerPolicy;
    type Search: SearchPolicy;
    type Ladder: LadderPolicy;
    type Dids: DidsPolicy;
    type Margin: MarginPolicy;
    type Calibrator: CalibratorPolicy;
    type Audit: AuditPolicy;
    type EvalCache: EvalCacheBackend + Clone;
    type DecisionCache: DecisionCacheBackend + Clone;
    type Executor: Executor<Self::EvalCache, Self::DecisionCache>;
}

pub struct Engine<P: PolicyBundle> {
    pub scheduler: P::Scheduler,
    pub search: P::Search,
    pub ladder: P::Ladder,
    pub dids: P::Dids,
    pub margin: P::Margin,
    pub calibrator: P::Calibrator,
    pub audit: P::Audit,
    pub eval_cache: P::EvalCache,
    pub decision_cache: P::DecisionCache,
    pub executor: P::Executor,
}

impl<P: PolicyBundle> Engine<P> {
    pub fn run(&mut self, cfg: &EngineConfig, env: &Env, workers: usize) -> EngineOutput {
        let evaluator: Arc<dyn Evaluator> = Arc::new(ToyEvaluator {
            m: cfg.num_constraints,
            dim: cfg.search_dim.unwrap_or(4),
        });
        self.run_with_evaluator(cfg, env, workers, evaluator)
    }

    pub fn run_with_evaluator(
        &mut self,
        cfg: &EngineConfig,
        env: &Env,
        workers: usize,
        evaluator: Arc<dyn Evaluator>,
    ) -> EngineOutput {
        let env_rev = env_rev(env);
        let env_arc = Arc::new(env.clone());
        let eval_cache_arc: Arc<P::EvalCache> = Arc::new(self.eval_cache.clone());
        let decision_cache_arc: Arc<P::DecisionCache> = Arc::new(self.decision_cache.clone());

        self.scheduler.configure(workers);
        self.executor.configure(workers);
        // Executor params are configured per-batch to support online tuning.
        let mut chunk_base = cfg.executor_chunk_base;
        chunk_base = chunk_base
            .max(cfg.executor_chunk_min.max(1))
            .min(cfg.executor_chunk_max.max(1));
        self.executor.configure_params(ExecutorParams {
            chunk_base,
            spin_limit: cfg.executor_spin_limit,
        });
        self.search.reset(env);
        self.dids.init(cfg.num_constraints);
        self.calibrator.init(cfg.num_constraints);
        self.calibrator.configure(&CalibratorConfig {
            target_false: cfg.calibrator_target_false,
            min_audits: cfg.calibrator_min_audits,
            eta_delta: cfg.calibrator_eta_delta,
            delta_min: cfg.calibrator_delta_min,
            delta_max: cfg.calibrator_delta_max,
            k_window: cfg.calibrator_k_window,
            k_min_pairs: cfg.calibrator_k_min_pairs,
            k_quantile: cfg.calibrator_k_quantile,
            k_eta: cfg.calibrator_k_eta,
        });

        // Resolve search dimension: config override > evaluator > fallback 1.
        let resolved_dim: usize = cfg
            .search_dim
            .or_else(|| evaluator.search_dim())
            .unwrap_or(1);

        let (tau_levels, smc_levels) = resolve_ladder_levels(cfg, resolved_dim);
        let ladder = self.ladder.build_ladder(&tau_levels, &smc_levels);
        let ladder_len = ladder.len();
        assert!(ladder_len > 0, "ladder must be non-empty");

        // Mesh geometry: canonical base lattice (Δ₀) + current mesh multiplier.
        assert!(
            cfg.mesh_base_step.is_finite(),
            "mesh_base_step must be finite"
        );
        assert!(cfg.mesh_base_step > 0.0, "mesh_base_step must be positive");
        let mut geo = MeshGeometry {
            base_step: cfg.mesh_base_step,
            mesh_mul: cfg.mesh_mul_init.max(1),
            mesh_mul_min: cfg.mesh_mul_min.max(1),
            refine_div: cfg.mesh_refine_div.max(2),
            poll_step_mult: cfg.poll_step_mult.max(1),
        };

        // DIDS assignment vector a_j in 1..=L.
        let cal_state0 = self.calibrator.state();
        let (mut assignment, _a_delta0) = self.dids.update_assignment(ladder_len, &cal_state0);

        let mut state = crate::policies::search::SearchState { iter: 0 };
        let mut stats = EngineStats::default();

        let mut policy_rev = self.calibrator.state().policy_rev;

        let accept_cfg = AcceptanceConfig {
            h0: cfg.accept_h0,
            h_min: cfg.accept_h_min,
            h_shrink: cfg.accept_h_shrink,
            eps_f: cfg.accept_eps_f,
            eps_v: cfg.accept_eps_v,
            filter_cap: cfg.accept_filter_cap,
        };
        let mut accept = DefaultAcceptance::new(accept_cfg);

        let mut x_best: Option<XMesh> = None;
        let mut f_best: Option<f64> = None;
        let mut incumbent_id: u64 = 0;

        // Run-global candidate store.
        let mut cands: Vec<CandidateStageState> = Vec::new();
        let mut index: HashMap<CandidateId, usize> = HashMap::new();

        let boundary = cfg
            .batch_boundary
            .unwrap_or_else(|| self.scheduler.batch_size())
            .max(1);

        for iter in 0..cfg.max_iters {
            let mut iter_improved = false;
            let mut poll_generated = false;
            state.iter = iter;

            // 1) Propose candidates and ingest into the run-global resumable store.

            let dim: usize = if let Some(xb) = x_best.as_ref() {
                xb.0.len()
            } else {
                resolved_dim
            };
            let incumbent_x: Option<Vec<f64>> = x_best
                .as_ref()
                .map(|xb| xb.0.iter().map(|&u| (u as f64) * geo.base_step).collect());
            self.search
                .set_context(&crate::policies::search::SearchContext {
                    dim,
                    incumbent_x,
                    mesh_step: geo.current_step(),
                });

            let raw = self.search.propose(&state, cfg.candidates_per_iter);

            // Deterministic order: sort raw by (search_score, id).
            let mut scored: Vec<(CandidateId, XMesh, f64)> = raw
                .iter()
                .map(|rc| {
                    let x = crate::policies::search::project_to_mesh(rc, &geo);
                    let score = self.search.score(
                        rc,
                        &crate::policies::search::SearchHints {
                            incumbent_score: f_best,
                        },
                    );
                    (rc.id, x, score)
                })
                .collect();
            scored.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap().then(a.0.cmp(&b.0)));

            for (id, x, base_score) in scored.into_iter() {
                if index.contains_key(&id) {
                    continue;
                }
                let pos = cands.len();
                index.insert(id, pos);
                cands.push(CandidateStageState {
                    id,
                    x,
                    base_score,
                    created_epoch: iter,
                    next_phi_idx: 0,
                    last_estimates_phi_idx: None,
                    last_estimates: None,
                    audit_required: false,
                    audit_origin: None,
                    cheap_checked: false,
                    cheap_ok: false,
                    submitted_policy_rev: policy_rev,
                    submitted_incumbent_id: incumbent_id,
                    status: CandidateStatus::Ready,
                    audit_cut_estimates: None,
                });
            }

            // 2) Execute up to `max_steps_per_iter` one-step work items.
            let mut steps_left = cfg.max_steps_per_iter.unwrap_or(usize::MAX);
            while steps_left > 0 {
                let th = self.margin.thresholds(&self.calibrator.state());

                let mut ready_view = self.make_ready_view(&cands, &th, f_best, iter);
                if ready_view.is_empty() {
                    // Search exhaustion: inject Poll points once per iteration, then retry.
                    if !poll_generated
                        && !iter_improved
                        && let Some(center) = x_best.as_ref()
                    {
                        let step = geo.poll_step_units();
                        let poll_pts = DefaultPoll::generate_points(center, step);
                        // Deterministic ID namespace for poll points (high bit set).
                        let base: u64 = 0x8000_0000_0000_0000u64 | (iter << 32);
                        for (d, x) in poll_pts.into_iter().enumerate() {
                            let id = CandidateId(base | (d as u64));
                            if index.contains_key(&id) {
                                continue;
                            }
                            let pos = cands.len();
                            index.insert(id, pos);
                            cands.push(CandidateStageState {
                                id,
                                x,
                                base_score: f64::INFINITY,
                                created_epoch: iter,
                                next_phi_idx: 0,
                                last_estimates_phi_idx: None,
                                last_estimates: None,
                                audit_required: false,
                                audit_origin: None,
                                cheap_checked: false,
                                cheap_ok: false,
                                submitted_policy_rev: policy_rev,
                                submitted_incumbent_id: incumbent_id,
                                status: CandidateStatus::Ready,
                                audit_cut_estimates: None,
                            });
                        }
                        poll_generated = true;
                        continue;
                    }
                    break;
                }

                // Deterministic input to scheduler.
                ready_view
                    .sort_by(|a, b| a.score.partial_cmp(&b.score).unwrap().then(a.id.cmp(&b.id)));

                let picked = self.scheduler.select_next(&ready_view);
                if picked.is_empty() {
                    break;
                }

                // Dispatch: convert picked candidates to one-step WorkItems.
                let mut work: Vec<WorkItem> = Vec::new();
                let take_n = picked.len().min(boundary).min(steps_left);
                for id in picked.into_iter().take(take_n) {
                    if let Some(&pos) = index.get(&id) {
                        let cand = &mut cands[pos];
                        if !matches!(cand.status, CandidateStatus::Ready) {
                            continue;
                        }
                        let phi_idx = cand.next_phi_idx;
                        if (phi_idx as usize) >= ladder_len {
                            continue;
                        }
                        let phi = ladder[phi_idx as usize];
                        cand.status = CandidateStatus::InFlight { phi_idx };
                        cand.submitted_policy_rev = policy_rev;
                        cand.submitted_incumbent_id = incumbent_id;

                        work.push(WorkItem {
                            cand_id: id,
                            x: cand.x.clone(),
                            phi_idx,
                            phi,
                            env_rev,
                            policy_rev,
                            incumbent_id,
                        });
                    }
                }

                if work.is_empty() {
                    break;
                }

                // Dispatch+join through the executor (may be parallel).
                let exec_ctx = Arc::new(ExecCtx {
                    evaluator: evaluator.clone(),
                    env: env_arc.clone(),
                    env_rev,
                    eval_cache: eval_cache_arc.clone(),
                    decision_cache: decision_cache_arc.clone(),
                    ladder_len,
                    base_step: geo.base_step,
                });
                self.executor.configure_params(ExecutorParams {
                    chunk_base,
                    spin_limit: cfg.executor_spin_limit,
                });
                let mut outcomes: Vec<WorkOutcome> = self.executor.run_batch(work, exec_ctx);

                // Join: deterministic completion processing.
                outcomes.sort_by_key(|o| o.item.cand_id);

                // Update stats deterministically on the engine thread.
                for out in outcomes.iter() {
                    self.update_stats_from_outcome(&mut stats, out, ladder_len);
                }

                let mut events: Vec<CalibEvent> = Vec::with_capacity(outcomes.len());
                let mut new_incumbent = false;

                for out in outcomes.iter() {
                    let id = out.item.cand_id;
                    let pos = *index.get(&id).expect("candidate exists");
                    let cand = &mut cands[pos];

                    // Note: decisions are evaluated with the *current* policy state for this batch.
                    // Policy updates happen at the batch boundary after processing all completions.
                    let job = self.materialize_job_result(
                        cfg,
                        evaluator.as_ref(),
                        env,
                        geo.base_step,
                        &ladder,
                        &assignment,
                        &th,
                        cand,
                        out,
                        policy_rev,
                        f_best,
                        &mut stats,
                    );

                    // Scheduler completion hook.
                    self.scheduler.on_complete(id, &job.result);

                    // DIDS history.
                    self.dids
                        .record(cand.x.clone(), out.item.phi, out.item.phi_idx, &job.result);

                    // Acceptance: TRUTH only (sealed).
                    if let JobResult::Truth { f, v, .. } = &job.result {
                        match accept.decide_truth(&cand.x, *f, *v) {
                            TruthDecision::Accept => {
                                // Update incumbent.
                                iter_improved = true;
                                new_incumbent = true;
                                incumbent_id += 1;
                                f_best = Some(*f);
                                x_best = Some(cand.x.clone());
                            }
                            TruthDecision::Reject => {}
                        }
                    }

                    // Collect calibration event.
                    events.push(CalibEvent {
                        id,
                        result: job.result.clone(),
                        audited: job.audited,
                        audit_of: job.audit_of,
                        paired_sample: job.paired_sample,
                    });
                }

                // Batch boundary policy update: deterministic order required.
                // Ensure events are sorted.
                events.sort_by_key(|e| e.id);
                let delta = self.calibrator.update(&events);
                if delta.0 != 0 {
                    policy_rev = PolicyRev(policy_rev.0 + delta.0);
                }
                let cal_state = self.calibrator.state();
                let (new_a, a_delta) = self.dids.update_assignment(ladder_len, &cal_state);
                assignment = new_a;
                if a_delta.0 != 0 {
                    policy_rev = PolicyRev(policy_rev.0 + a_delta.0);
                }

                // Online chunk tuning (performance only).
                if cfg.executor_chunk_auto_tune {
                    let mut costs: Vec<f64> = Vec::new();
                    for o in outcomes.iter() {
                        if o.did_compute && o.runtime_cost > 0.0 {
                            costs.push(o.runtime_cost);
                        }
                    }
                    if costs.len() >= 4 {
                        let mean: f64 = costs.iter().sum::<f64>() / (costs.len() as f64);
                        if mean > 0.0 {
                            let var: f64 = costs
                                .iter()
                                .map(|c| {
                                    let d = c - mean;
                                    d * d
                                })
                                .sum::<f64>()
                                / (costs.len() as f64);
                            let sd = var.sqrt();
                            let cv = sd / mean.max(1e-12);

                            let mut target = chunk_base;
                            if cv < 0.25 {
                                target = (chunk_base.saturating_mul(2))
                                    .min(cfg.executor_chunk_max.max(1));
                            } else if cv > 0.75 {
                                target = chunk_base.div_ceil(2).max(cfg.executor_chunk_min.max(1));
                            }

                            chunk_base = target
                                .max(cfg.executor_chunk_min.max(1))
                                .min(cfg.executor_chunk_max.max(1));
                        }
                    }
                }

                // Optional cancellation policy hook.
                let _ = self.scheduler.should_cancel_inflight(new_incumbent);

                steps_left = steps_left.saturating_sub(outcomes.len());
            }

            // Acceptance/Barrier schedule update (sealed, deterministic boundary).
            accept.on_iteration_end(poll_generated, iter_improved);

            // Conservative mesh update: refine (shrink) mesh only when Poll was attempted and failed.
            if !iter_improved && poll_generated {
                geo.refine();
            }
        }

        EngineOutput {
            x_best,
            f_best,
            stats,
        }
    }

    fn update_stats_from_outcome(
        &self,
        stats: &mut EngineStats,
        out: &WorkOutcome,
        ladder_len: usize,
    ) {
        let is_truth = (out.item.phi_idx as usize) + 1 == ladder_len;

        if matches!(
            out.cached_decision,
            Some(JobResult::RejectedInvalidEval { .. })
        ) {
            stats.invalid_eval_rejects += 1;
            return;
        }

        if out.hit_decision_cache {
            if is_truth {
                stats.truth_decision_cache_hits += 1;
            } else {
                stats.partial_decision_cache_hits += 1;
            }
            return;
        }

        if out.hit_eval_cache {
            if is_truth {
                stats.truth_eval_cache_hits += 1;
            } else {
                stats.partial_eval_cache_hits += 1;
            }
            return;
        }

        if out.did_compute {
            if is_truth {
                stats.truth_evals += 1;
            } else {
                stats.partial_steps += 1;
            }
        }
    }

    fn make_ready_view(
        &self,
        cands: &[CandidateStageState],
        th: &crate::policies::margin::Thresholds,
        incumbent_f: Option<f64>,
        epoch: u64,
    ) -> Vec<ReadyCandidateView> {
        let mut out: Vec<ReadyCandidateView> = Vec::new();
        for c in cands.iter() {
            if !matches!(c.status, CandidateStatus::Ready) {
                continue;
            }
            let kind = if c.next_phi_idx == 0 {
                ReadyKind::New
            } else {
                ReadyKind::Resume
            };

            // Priority score: audit-required first, then optimistic objective bound if available,
            // otherwise fall back to search base score.
            let mut score = c.base_score;
            if c.audit_required {
                score -= 1e12;
            }
            // Anti-starvation: older candidates receive a tiny priority boost.
            let age = epoch.saturating_sub(c.created_epoch);
            score -= (age as f64) * 1e-9;
            if let Some(est) = &c.last_estimates {
                let (lcb, _ucb) = self.margin.objective_bounds(est, th);
                score = score.min(lcb);
            }

            // Encourage finishing ladder (small tie-breaker).
            score -= (c.next_phi_idx as f64) * 1e-6;

            // If incumbent is known and LCB is already hopeless, we may still keep it ready;
            // the stop rule is applied after step completion to keep behavior deterministic.
            let _ = incumbent_f;

            out.push(ReadyCandidateView {
                id: c.id,
                kind,
                x: c.x.clone(),
                next_phi_idx: c.next_phi_idx,
                score,
                audit_required: c.audit_required,
            });
        }
        out
    }

    /// Convert a completed `WorkOutcome` into a `JobResult` and update candidate state.
    #[allow(clippy::too_many_arguments)]
    fn materialize_job_result(
        &mut self,
        cfg: &EngineConfig,
        evaluator: &dyn Evaluator,
        env: &Env,
        base_step: f64,
        ladder: &[Phi],
        assignment: &[usize],
        th: &crate::policies::margin::Thresholds,
        cand: &mut CandidateStageState,
        out: &WorkOutcome,
        policy_rev: PolicyRev,
        incumbent_f: Option<f64>,
        stats: &mut EngineStats,
    ) -> MaterializedJob {
        let phi = out.item.phi;
        let env_rev = out.item.env_rev;
        let is_truth = (out.item.phi_idx as usize) + 1 == ladder.len();
        let tag = if is_truth {
            CacheTag::Truth
        } else {
            CacheTag::Partial
        };

        // If a decision cache hit happened, use it, but still update state coherently.
        if let Some(cached) = &out.cached_decision {
            match cached {
                JobResult::Partial { estimates, meta } => {
                    cand.last_estimates = Some(estimates.clone());
                    cand.last_estimates_phi_idx = Some(out.item.phi_idx);

                    let audited = cand.audit_required;
                    let audit_of = cand.audit_origin.as_ref().map(audit_of_from_origin);
                    let paired_sample = paired_audit_sample_from_estimates(
                        cand.audit_origin.as_ref(),
                        out.item.phi_idx,
                        meta.phi,
                        estimates,
                    );

                    self.apply_state_transition_from_result(
                        cand,
                        out.item.phi_idx,
                        cached,
                        ladder.len(),
                    );

                    return MaterializedJob {
                        result: cached.clone(),
                        audited,
                        audit_of,
                        paired_sample,
                    };
                }
                JobResult::EarlyInfeasible {
                    violated_j,
                    estimates,
                    meta,
                } => {
                    cand.last_estimates = Some(estimates.clone());
                    cand.last_estimates_phi_idx = Some(out.item.phi_idx);

                    let level = (out.item.phi_idx as usize) + 1;
                    let aj = assignment.get(*violated_j).copied().unwrap_or(ladder.len());

                    if !cand.audit_required && level >= aj {
                        let audit = self.audit.should_audit(&cand.x, phi, env_rev);
                        if audit {
                            cand.audit_required = true;
                            cand.audit_cut_estimates = Some(estimates.clone());
                            cand.audit_origin = Some(CandidateAuditOrigin {
                                violated_j: *violated_j,
                                phi_at_cut: meta.phi,
                                phi_idx_at_cut: out.item.phi_idx,
                                paired_phi_indices: collect_paired_phi_indices(
                                    ladder,
                                    out.item.phi_idx,
                                ),
                            });
                        }
                    }

                    if cand.audit_required {
                        let audit_of = cand.audit_origin.as_ref().map(audit_of_from_origin);
                        cand.next_phi_idx = next_audit_target_idx(
                            cand.audit_origin.as_ref(),
                            out.item.phi_idx,
                            ladder.len(),
                        );
                        cand.status = CandidateStatus::Ready;
                        let paired_sample = paired_audit_sample_from_estimates(
                            cand.audit_origin.as_ref(),
                            out.item.phi_idx,
                            meta.phi,
                            estimates,
                        );
                        return MaterializedJob {
                            result: cached.clone(),
                            audited: true,
                            audit_of,
                            paired_sample,
                        };
                    }

                    cand.status = CandidateStatus::DoneEarlyInfeasible {
                        violated_j: *violated_j,
                        at_phi_idx: out.item.phi_idx,
                    };
                    return MaterializedJob {
                        result: cached.clone(),
                        audited: false,
                        audit_of: None,
                        paired_sample: None,
                    };
                }
                JobResult::Truth { f, c, meta, .. } => {
                    cand.status = CandidateStatus::DoneTruth;
                    let paired_sample =
                        if is_paired_checkpoint(cand.audit_origin.as_ref(), out.item.phi_idx) {
                            let est = truth_as_estimates(meta.phi, *f, c);
                            Some(crate::policies::PairedAuditSample {
                                paired_phi: meta.phi,
                                paired_phi_idx: out.item.phi_idx,
                                estimates: est,
                            })
                        } else {
                            None
                        };
                    return MaterializedJob {
                        result: cached.clone(),
                        audited: false,
                        audit_of: None,
                        paired_sample,
                    };
                }
                JobResult::RejectedCheap { .. } => {
                    cand.status = CandidateStatus::DoneRejectedCheap;
                    return MaterializedJob {
                        result: cached.clone(),
                        audited: false,
                        audit_of: None,
                        paired_sample: None,
                    };
                }
                JobResult::RejectedInvalidEval { .. } => {
                    cand.status = CandidateStatus::DoneRejectedInvalidEval;
                    return MaterializedJob {
                        result: cached.clone(),
                        audited: false,
                        audit_of: None,
                        paired_sample: None,
                    };
                }
            }
        }

        // Sticky cheap constraints.
        if !cand.cheap_checked {
            cand.cheap_checked = true;
            match mesh_to_real(&cand.x, base_step) {
                Ok(x_real) => {
                    cand.cheap_ok = evaluator.cheap_constraints(&x_real, env);
                }
                Err(_) => {
                    stats.invalid_eval_rejects += 1;
                    cand.status = CandidateStatus::DoneRejectedInvalidEval;
                    let meta = EvalMeta {
                        phi,
                        env_rev,
                        policy_rev,
                        runtime_cost: 0.0,
                    };
                    let jr = JobResult::RejectedInvalidEval { meta };
                    let dkey = DecisionCacheKey {
                        x: cand.x.clone(),
                        phi,
                        env_rev,
                        policy_rev,
                        tag: CacheTag::Cheap,
                    };
                    self.decision_cache.put(dkey, jr.clone());
                    return MaterializedJob {
                        result: jr,
                        audited: false,
                        audit_of: None,
                        paired_sample: None,
                    };
                }
            };
        }
        if !cand.cheap_ok {
            stats.cheap_rejects += 1;
            cand.status = CandidateStatus::DoneRejectedCheap;
            let meta = EvalMeta {
                phi,
                env_rev,
                policy_rev,
                runtime_cost: 0.0,
            };
            let jr = JobResult::RejectedCheap { meta };
            let dkey = DecisionCacheKey {
                x: cand.x.clone(),
                phi,
                env_rev,
                policy_rev,
                tag: CacheTag::Cheap,
            };
            self.decision_cache.put(dkey, jr.clone());
            return MaterializedJob {
                result: jr,
                audited: false,
                audit_of: None,
                paired_sample: None,
            };
        }

        let estimates = out
            .estimates
            .as_ref()
            .expect("estimates must exist if decision cache miss and cheap ok")
            .clone();

        // Keep last estimates for scoring.
        cand.last_estimates = Some(estimates.clone());
        cand.last_estimates_phi_idx = Some(out.item.phi_idx);

        let meta = EvalMeta {
            phi,
            env_rev,
            policy_rev,
            runtime_cost: out.runtime_cost,
        };

        // Non-truth step: early infeasible / stop / continue.
        if !is_truth {
            // Early infeasible?
            if let Some(j) = self.margin.early_infeasible(&estimates, th) {
                let aj = assignment.get(j).copied().unwrap_or(ladder.len());
                let level = (out.item.phi_idx as usize) + 1; // 1-based
                if level >= aj {
                    if !cand.audit_required {
                        let audit = self.audit.should_audit(&cand.x, phi, env_rev);
                        if audit {
                            cand.audit_required = true;
                            cand.audit_cut_estimates = Some(estimates.clone());
                            cand.audit_origin = Some(CandidateAuditOrigin {
                                violated_j: j,
                                phi_at_cut: phi,
                                phi_idx_at_cut: out.item.phi_idx,
                                paired_phi_indices: collect_paired_phi_indices(
                                    ladder,
                                    out.item.phi_idx,
                                ),
                            });
                        }
                    }

                    if cand.audit_required {
                        let audit_of = cand.audit_origin.as_ref().map(audit_of_from_origin);
                        cand.next_phi_idx = next_audit_target_idx(
                            cand.audit_origin.as_ref(),
                            out.item.phi_idx,
                            ladder.len(),
                        );
                        cand.status = CandidateStatus::Ready;

                        let jr = JobResult::EarlyInfeasible {
                            violated_j: j,
                            estimates: estimates.clone(),
                            meta: meta.clone(),
                        };
                        let dkey = DecisionCacheKey {
                            x: cand.x.clone(),
                            phi,
                            env_rev,
                            policy_rev,
                            tag,
                        };
                        self.decision_cache.put(dkey, jr.clone());

                        return MaterializedJob {
                            result: jr,
                            audited: true,
                            audit_of,
                            paired_sample: paired_audit_sample_from_estimates(
                                cand.audit_origin.as_ref(),
                                out.item.phi_idx,
                                phi,
                                &estimates,
                            ),
                        };
                    }

                    // No audit: finalize as early infeasible.
                    cand.status = CandidateStatus::DoneEarlyInfeasible {
                        violated_j: j,
                        at_phi_idx: out.item.phi_idx,
                    };
                    let jr = JobResult::EarlyInfeasible {
                        violated_j: j,
                        estimates: estimates.clone(),
                        meta: meta.clone(),
                    };
                    let dkey = DecisionCacheKey {
                        x: cand.x.clone(),
                        phi,
                        env_rev,
                        policy_rev,
                        tag,
                    };
                    self.decision_cache.put(dkey, jr.clone());
                    return MaterializedJob {
                        result: jr,
                        audited: false,
                        audit_of: None,
                        paired_sample: None,
                    };
                }
            }

            // Promotion stop: if even optimistic bound can't beat incumbent, stop here.
            if let Some(best) = incumbent_f {
                // Ladder-aware / SMC-aware gating controlled by EngineConfig.
                if objective_pruning_allowed(
                    out.item.phi_idx as usize,
                    ladder,
                    cand.audit_required,
                    cfg,
                ) {
                    let (lcb, _ucb) = self.margin.objective_bounds(&estimates, th);
                    if lcb >= best - th.eps_f {
                        cand.status = CandidateStatus::DoneStoppedPartial {
                            at_phi_idx: out.item.phi_idx,
                        };
                        let jr = JobResult::Partial {
                            estimates: estimates.clone(),
                            meta: meta.clone(),
                        };
                        let dkey = DecisionCacheKey {
                            x: cand.x.clone(),
                            phi,
                            env_rev,
                            policy_rev,
                            tag,
                        };
                        self.decision_cache.put(dkey, jr.clone());
                        return MaterializedJob {
                            result: jr,
                            audited: false,
                            audit_of: None,
                            paired_sample: None,
                        };
                    }
                }
            }

            // Continue to next step.
            if cand.audit_required {
                cand.next_phi_idx = next_audit_target_idx(
                    cand.audit_origin.as_ref(),
                    out.item.phi_idx,
                    ladder.len(),
                );
            } else {
                cand.next_phi_idx = out.item.phi_idx + 1;
            }
            cand.status = CandidateStatus::Ready;

            let jr = JobResult::Partial {
                estimates: estimates.clone(),
                meta: meta.clone(),
            };
            let dkey = DecisionCacheKey {
                x: cand.x.clone(),
                phi,
                env_rev,
                policy_rev,
                tag,
            };
            self.decision_cache.put(dkey, jr.clone());
            return MaterializedJob {
                result: jr,
                audited: cand.audit_required,
                audit_of: cand.audit_origin.as_ref().map(audit_of_from_origin),
                paired_sample: paired_audit_sample_from_estimates(
                    cand.audit_origin.as_ref(),
                    out.item.phi_idx,
                    phi,
                    &estimates,
                ),
            };
        }

        // TRUTH
        cand.status = CandidateStatus::DoneTruth;
        let c = estimates.c_hat.clone();
        let f = estimates.f_hat;
        let feasible = c.iter().all(|&cj| cj <= 0.0);
        let v = c.iter().map(|&cj| cj.max(0.0)).fold(0.0, f64::max);
        let jr = JobResult::Truth {
            f,
            c,
            feasible,
            v,
            meta: meta.clone(),
        };
        let dkey = DecisionCacheKey {
            x: cand.x.clone(),
            phi,
            env_rev,
            policy_rev,
            tag: CacheTag::Truth,
        };
        self.decision_cache.put(dkey, jr.clone());

        let paired_sample = paired_audit_sample_from_estimates(
            cand.audit_origin.as_ref(),
            out.item.phi_idx,
            phi,
            &estimates,
        );

        MaterializedJob {
            result: jr,
            audited: false,
            audit_of: None,
            paired_sample,
        }
    }

    fn apply_state_transition_from_result(
        &self,
        cand: &mut CandidateStageState,
        phi_idx: u32,
        result: &JobResult,
        ladder_len: usize,
    ) {
        match result {
            JobResult::RejectedCheap { .. } => {
                cand.status = CandidateStatus::DoneRejectedCheap;
            }
            JobResult::RejectedInvalidEval { .. } => {
                cand.status = CandidateStatus::DoneRejectedInvalidEval;
            }
            JobResult::EarlyInfeasible { violated_j, .. } => {
                if cand.audit_required {
                    cand.next_phi_idx =
                        next_audit_target_idx(cand.audit_origin.as_ref(), phi_idx, ladder_len);
                    cand.status = CandidateStatus::Ready;
                } else {
                    cand.status = CandidateStatus::DoneEarlyInfeasible {
                        violated_j: *violated_j,
                        at_phi_idx: phi_idx,
                    };
                }
            }
            JobResult::Partial { .. } => {
                if (phi_idx as usize) + 1 >= ladder_len {
                    cand.status = CandidateStatus::DoneTruth;
                } else if cand.audit_required {
                    cand.next_phi_idx =
                        next_audit_target_idx(cand.audit_origin.as_ref(), phi_idx, ladder_len);
                    cand.status = CandidateStatus::Ready;
                } else {
                    cand.next_phi_idx = phi_idx + 1;
                    cand.status = CandidateStatus::Ready;
                }
            }
            JobResult::Truth { .. } => {
                cand.status = CandidateStatus::DoneTruth;
            }
        }
    }

    #[allow(unused)]
    pub(crate) fn calibrator_state(&self) -> crate::policies::CalibState {
        self.calibrator.state()
    }
}

#[derive(Clone, Debug)]
struct MaterializedJob {
    result: JobResult,
    audited: bool,
    audit_of: Option<AuditOf>,
    paired_sample: Option<crate::policies::PairedAuditSample>,
}

/// Default policy bundle.
///
/// Uses [`AdaptiveExecutor`] which automatically selects between inline (single-threaded)
/// and pooled (multi-threaded) execution based on the `workers` parameter passed to
/// [`Engine::run`].
pub struct DefaultBundle;

impl PolicyBundle for DefaultBundle {
    type Scheduler = crate::policies::scheduler::DefaultScheduler;
    type Search = crate::policies::search::DefaultSearch;
    type Ladder = crate::policies::ladder::StaircaseLadder;
    type Dids = crate::policies::dids::DefaultDids;
    type Margin = crate::policies::margin::DefaultMargin;
    type Calibrator = crate::policies::calibrator::DeltaKCalibrator;
    type Audit = crate::policies::audit::DefaultAudit;
    type EvalCache = crate::backends::cache::MemoryEvalCache;
    type DecisionCache = crate::backends::cache::MemoryDecisionCache;
    type Executor = AdaptiveExecutor<
        crate::backends::cache::MemoryEvalCache,
        crate::backends::cache::MemoryDecisionCache,
    >;
}

impl<P: PolicyBundle> Default for Engine<P>
where
    P::Scheduler: Default,
    P::Search: Default,
    P::Ladder: Default,
    P::Dids: Default,
    P::Margin: Default,
    P::Calibrator: Default,
    P::Audit: Default,
    P::EvalCache: Default,
    P::DecisionCache: Default,
    P::Executor: Default,
{
    fn default() -> Self {
        Self {
            scheduler: P::Scheduler::default(),
            search: P::Search::default(),
            ladder: P::Ladder::default(),
            dids: P::Dids::default(),
            margin: P::Margin::default(),
            calibrator: P::Calibrator::default(),
            audit: P::Audit::default(),
            eval_cache: P::EvalCache::default(),
            decision_cache: P::DecisionCache::default(),
            executor: P::Executor::default(),
        }
    }
}

/// Example: customize only the scheduler while keeping all other policies default.
pub struct CustomSchedulerBundle<S: SchedulerPolicy>(PhantomData<S>);

impl<S: SchedulerPolicy> PolicyBundle for CustomSchedulerBundle<S> {
    type Scheduler = S;
    type Search = crate::policies::search::DefaultSearch;
    type Ladder = crate::policies::ladder::StaircaseLadder;
    type Dids = crate::policies::dids::DefaultDids;
    type Margin = crate::policies::margin::DefaultMargin;
    type Calibrator = crate::policies::calibrator::DeltaKCalibrator;
    type Audit = crate::policies::audit::DefaultAudit;
    type EvalCache = crate::backends::cache::MemoryEvalCache;
    type DecisionCache = crate::backends::cache::MemoryDecisionCache;
    type Executor = AdaptiveExecutor<
        crate::backends::cache::MemoryEvalCache,
        crate::backends::cache::MemoryDecisionCache,
    >;
}

impl<S: SchedulerPolicy + Default> Engine<CustomSchedulerBundle<S>> {
    pub fn with_custom_scheduler() -> Self {
        Self {
            scheduler: S::default(),
            search: crate::policies::search::DefaultSearch::default(),
            ladder: crate::policies::ladder::StaircaseLadder,
            dids: crate::policies::dids::DefaultDids::default(),
            margin: crate::policies::margin::DefaultMargin,
            calibrator: crate::policies::calibrator::DeltaKCalibrator::default(),
            audit: crate::policies::audit::DefaultAudit::default(),
            eval_cache: crate::backends::cache::MemoryEvalCache::default(),
            decision_cache: crate::backends::cache::MemoryDecisionCache::default(),
            executor: AdaptiveExecutor::default(),
        }
    }
}

/// Example: customize only the executor while keeping all other policies default.
///
/// This is convenient for swapping `InlineExecutor` with a real worker pool executor.
pub struct CustomExecutorBundle<
    E: Executor<<Self as PolicyBundle>::EvalCache, <Self as PolicyBundle>::DecisionCache>,
>(PhantomData<E>)
where
    Self: PolicyBundle;

impl<
    E: Executor<crate::backends::cache::MemoryEvalCache, crate::backends::cache::MemoryDecisionCache>,
> PolicyBundle for CustomExecutorBundle<E>
{
    type Scheduler = crate::policies::scheduler::DefaultScheduler;
    type Search = crate::policies::search::DefaultSearch;
    type Ladder = crate::policies::ladder::StaircaseLadder;
    type Dids = crate::policies::dids::DefaultDids;
    type Margin = crate::policies::margin::DefaultMargin;
    type Calibrator = crate::policies::calibrator::DeltaKCalibrator;
    type Audit = crate::policies::audit::DefaultAudit;
    type EvalCache = crate::backends::cache::MemoryEvalCache;
    type DecisionCache = crate::backends::cache::MemoryDecisionCache;
    type Executor = E;
}

impl<
    E: Executor<crate::backends::cache::MemoryEvalCache, crate::backends::cache::MemoryDecisionCache>,
> Engine<CustomExecutorBundle<E>>
{
    pub fn with_executor(executor: E) -> Self {
        Self {
            scheduler: crate::policies::scheduler::DefaultScheduler::default(),
            search: crate::policies::search::DefaultSearch::default(),
            ladder: crate::policies::ladder::StaircaseLadder,
            dids: crate::policies::dids::DefaultDids::default(),
            margin: crate::policies::margin::DefaultMargin,
            calibrator: crate::policies::calibrator::DeltaKCalibrator::default(),
            audit: crate::policies::audit::DefaultAudit::default(),
            eval_cache: crate::backends::cache::MemoryEvalCache::default(),
            decision_cache: crate::backends::cache::MemoryDecisionCache::default(),
            executor,
        }
    }
}
