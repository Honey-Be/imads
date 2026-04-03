use crate::types::{CandidateId, Estimates, JobResult, PolicyRev};

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct PolicyRevDelta(pub u64);

/// Calibration state surfaced to other policies (Margin/DIDS/Search).
///
/// NOTE: This state is intentionally compact and cloneable.
#[derive(Clone, Debug, Default)]
pub struct CalibState {
    pub policy_rev: PolicyRev,
    /// Relative slack for constraints.
    pub delta_rel: Vec<f64>,
    /// Solver bias coefficient K_cj (per-constraint).
    pub k_c: Vec<f64>,
    /// Solver bias coefficient K_f (objective).
    pub k_f: f64,
    /// Objective deprioritize epsilon.
    pub eps_f: f64,
    /// Target false infeasible rate.
    pub target_false: f64,
    /// Min number of audited samples before updating delta for a constraint.
    pub min_audits: usize,
    /// Multiplicative step size for delta updates (exp(±eta)).
    pub eta_delta: f64,
    /// Clamp range for delta_rel.
    pub delta_min: f64,
    pub delta_max: f64,

    /// Number of early-cut attempts that were selected for audit for each constraint.
    pub cut_n: Vec<usize>,
    /// Number of audited early-cut attempts for each constraint.
    pub audit_n: Vec<usize>,
    /// Number of false-infeasible outcomes among audited early-cuts for each constraint.
    pub false_infeas_n: Vec<usize>,
    /// Phi-index-bucketed counts (same ordering as ladder indices, 0-based inside vectors).
    pub cut_n_by_phi_idx: Vec<Vec<usize>>,
    pub audit_n_by_phi_idx: Vec<Vec<usize>>,
    pub false_infeas_n_by_phi_idx: Vec<Vec<usize>>,
}

/// Configuration parameters for the default calibrator (delta controller).
///
/// These are intended to be surfaced to `EngineConfig` so callers can tune
/// the false-infeasible vs. cost tradeoff without changing policy code.
#[derive(Clone, Debug)]
pub struct CalibratorConfig {
    /// Target false infeasible rate (fraction in [0, 1]).
    pub target_false: f64,
    /// Minimum number of audited samples before updating delta for a constraint.
    pub min_audits: usize,
    /// Multiplicative step size for delta updates (exp(±eta)).
    pub eta_delta: f64,
    /// Clamp range for delta_rel.
    pub delta_min: f64,
    pub delta_max: f64,
    /// Max number of K samples kept per objective/constraint stream.
    pub k_window: usize,
    /// Minimum number of paired audit samples before K updates are enabled.
    pub k_min_pairs: usize,
    /// Target quantile for K updates (in [0, 1]).
    pub k_quantile: f64,
    /// EMA step size for K updates.
    pub k_eta: f64,
}

impl Default for CalibratorConfig {
    fn default() -> Self {
        Self {
            target_false: 0.01,
            min_audits: 20,
            eta_delta: 0.1,
            delta_min: 0.0,
            delta_max: 0.05,
            k_window: 4096,
            k_min_pairs: 25,
            k_quantile: 0.90,
            k_eta: 0.2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CalibEvent {
    pub id: CandidateId,
    pub result: JobResult,
    /// Whether this completion was an audit (truth-confirm) path.
    pub audited: bool,
    /// If audited, what early-cut decision was being audited.
    pub audit_of: Option<AuditOf>,
    /// Optional same-S tighter-tau paired sample collected before TRUTH.
    pub paired_sample: Option<PairedAuditSample>,
}

#[derive(Clone, Debug)]
pub struct AuditOf {
    pub violated_j: usize,
    pub phi_at_cut: crate::types::Phi,
    pub phi_idx_at_cut: u32,
}

#[derive(Clone, Debug)]
pub struct PairedAuditSample {
    pub paired_phi: crate::types::Phi,
    pub paired_phi_idx: u32,
    pub estimates: crate::types::Estimates,
}

/// Calibrator policy. Safe to customize.
///
/// # Contract
/// - `update` is called at batch boundary with events in deterministic order.
/// - Must not depend on wall clock.
/// - May keep internal state across batches (e.g., pending audit mappings).
pub trait CalibratorPolicy: Send + Sync {
    fn init(&mut self, m: usize);

    /// Configure calibration parameters.
    ///
    /// Default implementation is a no-op so custom calibrators are not forced
    /// to support this.
    fn configure(&mut self, _cfg: &CalibratorConfig) {}

    fn update(&mut self, events: &[CalibEvent]) -> PolicyRevDelta;

    fn state(&self) -> CalibState;
}

#[derive(Default)]
pub struct NoopCalibrator {
    state: CalibState,
}

impl CalibratorPolicy for NoopCalibrator {
    fn init(&mut self, m: usize) {
        self.state = CalibState {
            policy_rev: PolicyRev(0),
            delta_rel: vec![0.005; m],
            k_c: vec![0.0; m],
            k_f: 0.0,
            eps_f: 1e-3,
            target_false: 0.01,
            min_audits: 20,
            eta_delta: 0.1,
            delta_min: 0.0,
            delta_max: 0.05,
            cut_n: vec![0; m],
            audit_n: vec![0; m],
            false_infeas_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
        };
    }

    fn configure(&mut self, cfg: &CalibratorConfig) {
        // Even though this calibrator does not update, keep the state consistent
        // with user configuration so downstream policies can read thresholds.
        self.state.target_false = cfg.target_false;
        self.state.min_audits = cfg.min_audits;
        self.state.eta_delta = cfg.eta_delta;
        self.state.delta_min = cfg.delta_min;
        self.state.delta_max = cfg.delta_max;
        for d in self.state.delta_rel.iter_mut() {
            *d = d.clamp(self.state.delta_min, self.state.delta_max);
        }
    }

    fn update(&mut self, _events: &[CalibEvent]) -> PolicyRevDelta {
        PolicyRevDelta(0)
    }

    fn state(&self) -> CalibState {
        self.state.clone()
    }
}

#[derive(Clone, Debug, Default)]
struct Counts {
    cut: usize,
    audit: usize,
    false_infeas: usize,
    cut_by_phi_idx: Vec<usize>,
    audit_by_phi_idx: Vec<usize>,
    false_infeas_by_phi_idx: Vec<usize>,
}
impl Counts {
    fn ensure_level(&mut self, idx: usize) {
        if self.cut_by_phi_idx.len() <= idx {
            self.cut_by_phi_idx.resize(idx + 1, 0);
        }
        if self.audit_by_phi_idx.len() <= idx {
            self.audit_by_phi_idx.resize(idx + 1, 0);
        }
        if self.false_infeas_by_phi_idx.len() <= idx {
            self.false_infeas_by_phi_idx.resize(idx + 1, 0);
        }
    }
}

/// Default calibrator: update only `delta_rel` to control false-infeasible rate.
///
/// This is intentionally conservative: it does not attempt to learn solver-bias bounds K.
#[derive(Default)]
pub struct DeltaCalibrator {
    state: CalibState,
    counts: Vec<Counts>,
    pending: HashMap<CandidateId, AuditOf>,
}

impl DeltaCalibrator {
    fn apply_cfg(&mut self, cfg: &CalibratorConfig) {
        self.state.target_false = cfg.target_false;
        self.state.min_audits = cfg.min_audits;
        self.state.eta_delta = cfg.eta_delta;
        self.state.delta_min = cfg.delta_min;
        self.state.delta_max = cfg.delta_max;
        for d in self.state.delta_rel.iter_mut() {
            *d = d.clamp(self.state.delta_min, self.state.delta_max);
        }
    }

    fn sync_counts_to_state(&mut self) {
        let m = self.state.delta_rel.len();
        if self.state.cut_n.len() != m {
            self.state.cut_n = vec![0; m];
        }
        if self.state.audit_n.len() != m {
            self.state.audit_n = vec![0; m];
        }
        if self.state.false_infeas_n.len() != m {
            self.state.false_infeas_n = vec![0; m];
        }
        if self.state.cut_n_by_phi_idx.len() != m {
            self.state.cut_n_by_phi_idx = vec![Vec::new(); m];
        }
        if self.state.audit_n_by_phi_idx.len() != m {
            self.state.audit_n_by_phi_idx = vec![Vec::new(); m];
        }
        if self.state.false_infeas_n_by_phi_idx.len() != m {
            self.state.false_infeas_n_by_phi_idx = vec![Vec::new(); m];
        }
        for j in 0..m {
            self.state.cut_n[j] = self.counts.get(j).map(|c| c.cut).unwrap_or(0);
            self.state.audit_n[j] = self.counts.get(j).map(|c| c.audit).unwrap_or(0);
            self.state.false_infeas_n[j] = self.counts.get(j).map(|c| c.false_infeas).unwrap_or(0);
            self.state.cut_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.cut_by_phi_idx.clone())
                .unwrap_or_default();
            self.state.audit_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.audit_by_phi_idx.clone())
                .unwrap_or_default();
            self.state.false_infeas_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.false_infeas_by_phi_idx.clone())
                .unwrap_or_default();
        }
    }

    fn update_delta(&mut self) -> bool {
        // Returns whether any delta changed.
        let m = self.state.delta_rel.len();
        let mut changed = false;
        for j in 0..m {
            let c = &self.counts[j];
            if c.audit < self.state.min_audits.max(1) {
                continue;
            }
            // Laplace smoothing for stability.
            let p_false = (c.false_infeas as f64 + 1.0) / (c.audit as f64 + 2.0);

            // Hysteresis: only adjust when clearly above target or well below.
            let t = self.state.target_false.clamp(0.0, 1.0);
            let mut s = 0i32;
            if p_false > t {
                s = 1; // too many false infeasible => increase delta
            } else if p_false < (t / 3.0) {
                s = -1; // too conservative => decrease delta
            }

            if s == 0 {
                continue;
            }
            let old = self.state.delta_rel[j];
            let eta = self.state.eta_delta.max(0.0);
            let mult = if s > 0 { (eta).exp() } else { (-eta).exp() };
            let mut next = old * mult;
            next = next.clamp(self.state.delta_min, self.state.delta_max);
            if (next - old).abs() > 1e-15 {
                self.state.delta_rel[j] = next;
                changed = true;
            }
        }
        changed
    }
}

impl CalibratorPolicy for DeltaCalibrator {
    fn init(&mut self, m: usize) {
        self.state = CalibState {
            policy_rev: PolicyRev(0),
            delta_rel: vec![0.005; m],
            k_c: vec![0.0; m],
            k_f: 0.0,
            eps_f: 1e-3,
            target_false: 0.01,
            min_audits: 20,
            eta_delta: 0.1,
            delta_min: 0.0,
            delta_max: 0.05,
            cut_n: vec![0; m],
            audit_n: vec![0; m],
            false_infeas_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
        };
        self.counts = vec![Counts::default(); m];
        self.pending.clear();
    }

    fn configure(&mut self, cfg: &CalibratorConfig) {
        self.apply_cfg(cfg);
    }

    fn update(&mut self, events: &[CalibEvent]) -> PolicyRevDelta {
        let mut any_change = false;

        for e in events {
            match &e.result {
                JobResult::EarlyInfeasible { .. } => {
                    if e.audited {
                        // Record pending audit mapping. Truth will arrive later.
                        if let Some(audit_of) = &e.audit_of {
                            self.pending.insert(e.id, audit_of.clone());
                            let j = audit_of.violated_j;
                            if j < self.counts.len() {
                                self.counts[j].cut += 1;
                                let lvl = audit_of.phi_idx_at_cut as usize;
                                self.counts[j].ensure_level(lvl);
                                self.counts[j].cut_by_phi_idx[lvl] += 1;
                            }
                        }
                    }
                }
                JobResult::Truth { feasible, .. } => {
                    // If this TRUTH corresponds to a prior audited early-cut, update counts.
                    if let Some(audit_of) = self.pending.remove(&e.id) {
                        let j = audit_of.violated_j;
                        if j < self.counts.len() {
                            self.counts[j].audit += 1;
                            let lvl = audit_of.phi_idx_at_cut as usize;
                            self.counts[j].ensure_level(lvl);
                            self.counts[j].audit_by_phi_idx[lvl] += 1;
                            if *feasible {
                                self.counts[j].false_infeas += 1;
                                self.counts[j].false_infeas_by_phi_idx[lvl] += 1;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if self.update_delta() {
            any_change = true;
        }

        self.sync_counts_to_state();

        if any_change {
            self.state.policy_rev = PolicyRev(self.state.policy_rev.0 + 1);
            PolicyRevDelta(1)
        } else {
            PolicyRevDelta(0)
        }
    }

    fn state(&self) -> CalibState {
        self.state.clone()
    }
}

// ----------------------------
// Delta + K calibrator
// ----------------------------

#[derive(Clone, Debug)]
struct PendingAudit {
    audit_of: AuditOf,
    est_at_cut: Estimates,
    paired_sample: Option<PairedAuditSample>,
}

/// Default calibrator (production-ready): update `delta_rel` *and* learn solver-bias bounds `K`.
///
/// - `delta_rel`: controls false-infeasible rate via audited early-cuts.
/// - `k_c`, `k_f`: estimated upper bounds on tau-dependent bias magnitude, used by `MarginPolicy`.
///
/// This calibrator is still conservative by design: K updates use a high quantile of observed
/// tau-normalized deltas, with smoothing.
pub struct DeltaKCalibrator {
    state: CalibState,
    counts: Vec<Counts>,
    pending: HashMap<CandidateId, PendingAudit>,

    // K samples (tau-normalized). Windowed to keep memory bounded.
    k_samples_f: Vec<f64>,
    k_samples_c: Vec<Vec<f64>>,

    // Internal knobs (kept as constants for now).
    k_window: usize,
    k_min_pairs: usize,
    k_quantile: f64,
    k_eta: f64,
}

impl Default for DeltaKCalibrator {
    fn default() -> Self {
        Self {
            state: CalibState::default(),
            counts: Vec::new(),
            pending: HashMap::new(),
            k_samples_f: Vec::new(),
            k_samples_c: Vec::new(),
            k_window: 4096,
            k_min_pairs: 25,
            k_quantile: 0.90,
            k_eta: 0.2,
        }
    }
}

impl DeltaKCalibrator {
    fn apply_cfg(&mut self, cfg: &CalibratorConfig) {
        self.state.target_false = cfg.target_false;
        self.state.min_audits = cfg.min_audits;
        self.state.eta_delta = cfg.eta_delta;
        self.state.delta_min = cfg.delta_min;
        self.state.delta_max = cfg.delta_max;
        self.k_window = cfg.k_window.max(1);
        self.k_min_pairs = cfg.k_min_pairs.max(1).min(self.k_window.max(1));
        self.k_quantile = cfg.k_quantile.clamp(0.0, 1.0);
        self.k_eta = cfg.k_eta.clamp(0.0, 1.0);
        for d in self.state.delta_rel.iter_mut() {
            *d = d.clamp(self.state.delta_min, self.state.delta_max);
        }
    }

    fn sync_counts_to_state(&mut self) {
        let m = self.state.delta_rel.len();
        if self.state.cut_n.len() != m {
            self.state.cut_n = vec![0; m];
        }
        if self.state.audit_n.len() != m {
            self.state.audit_n = vec![0; m];
        }
        if self.state.false_infeas_n.len() != m {
            self.state.false_infeas_n = vec![0; m];
        }
        if self.state.cut_n_by_phi_idx.len() != m {
            self.state.cut_n_by_phi_idx = vec![Vec::new(); m];
        }
        if self.state.audit_n_by_phi_idx.len() != m {
            self.state.audit_n_by_phi_idx = vec![Vec::new(); m];
        }
        if self.state.false_infeas_n_by_phi_idx.len() != m {
            self.state.false_infeas_n_by_phi_idx = vec![Vec::new(); m];
        }
        for j in 0..m {
            self.state.cut_n[j] = self.counts.get(j).map(|c| c.cut).unwrap_or(0);
            self.state.audit_n[j] = self.counts.get(j).map(|c| c.audit).unwrap_or(0);
            self.state.false_infeas_n[j] = self.counts.get(j).map(|c| c.false_infeas).unwrap_or(0);
            self.state.cut_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.cut_by_phi_idx.clone())
                .unwrap_or_default();
            self.state.audit_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.audit_by_phi_idx.clone())
                .unwrap_or_default();
            self.state.false_infeas_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.false_infeas_by_phi_idx.clone())
                .unwrap_or_default();
        }
    }

    fn update_delta(&mut self) -> bool {
        // Returns whether any delta changed.
        let m = self.state.delta_rel.len();
        let mut changed = false;
        for j in 0..m {
            let c = &self.counts[j];
            if c.audit < self.state.min_audits.max(1) {
                continue;
            }
            // Laplace smoothing for stability.
            let p_false = (c.false_infeas as f64 + 1.0) / (c.audit as f64 + 2.0);

            // Hysteresis: only adjust when clearly above target or well below.
            let t = self.state.target_false.clamp(0.0, 1.0);
            let mut s = 0i32;
            if p_false > t {
                s = 1; // too many false infeasible => increase delta
            } else if p_false < (t / 3.0) {
                s = -1; // too conservative => decrease delta
            }

            if s == 0 {
                continue;
            }
            let old = self.state.delta_rel[j];
            let eta = self.state.eta_delta.max(0.0);
            let mult = if s > 0 { (eta).exp() } else { (-eta).exp() };
            let mut next = old * mult;
            next = next.clamp(self.state.delta_min, self.state.delta_max);
            if (next - old).abs() > 1e-15 {
                self.state.delta_rel[j] = next;
                changed = true;
            }
        }
        changed
    }

    fn push_k_sample(vec: &mut Vec<f64>, v: f64, cap: usize) {
        if !(v.is_finite() && v >= 0.0) {
            return;
        }
        vec.push(v);
        if vec.len() > cap.max(1) {
            let excess = vec.len() - cap.max(1);
            vec.drain(0..excess);
        }
    }

    fn quantile_of(samples: &mut [f64], q: f64) -> Option<f64> {
        if samples.is_empty() {
            return None;
        }
        // Caller guarantees all are finite.
        samples.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let qq = q.clamp(0.0, 1.0);
        let n = samples.len();
        let idx = ((qq * ((n - 1) as f64)).round() as usize).min(n - 1);
        Some(samples[idx])
    }

    fn update_k_from_samples(&mut self) -> bool {
        let mut changed = false;

        // Objective K_f.
        if self.k_samples_f.len() >= self.k_min_pairs.max(1) {
            let mut tmp = self.k_samples_f.clone();
            if let Some(qv) = Self::quantile_of(&mut tmp[..], self.k_quantile) {
                let old = self.state.k_f;
                let eta = self.k_eta.clamp(0.0, 1.0);
                let next = (1.0 - eta) * old + eta * qv;
                if (next - old).abs() > 1e-15 {
                    self.state.k_f = next.max(0.0);
                    changed = true;
                }
            }
        }

        // Constraint-wise K_c.
        let m = self.state.k_c.len();
        for j in 0..m {
            if j >= self.k_samples_c.len() {
                continue;
            }
            if self.k_samples_c[j].len() < self.k_min_pairs.max(1) {
                continue;
            }
            let mut tmp = self.k_samples_c[j].clone();
            if let Some(qv) = Self::quantile_of(&mut tmp[..], self.k_quantile) {
                let old = self.state.k_c[j];
                let eta = self.k_eta.clamp(0.0, 1.0);
                let next = (1.0 - eta) * old + eta * qv;
                if (next - old).abs() > 1e-15 {
                    self.state.k_c[j] = next.max(0.0);
                    changed = true;
                }
            }
        }

        changed
    }
}

impl CalibratorPolicy for DeltaKCalibrator {
    fn init(&mut self, m: usize) {
        self.state = CalibState {
            policy_rev: PolicyRev(0),
            delta_rel: vec![0.005; m],
            k_c: vec![0.0; m],
            k_f: 0.0,
            eps_f: 1e-3,
            target_false: 0.01,
            min_audits: 20,
            eta_delta: 0.1,
            delta_min: 0.0,
            delta_max: 0.05,
            cut_n: vec![0; m],
            audit_n: vec![0; m],
            false_infeas_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
        };
        self.counts = vec![Counts::default(); m];
        self.pending.clear();

        self.k_samples_f.clear();
        self.k_samples_c = vec![Vec::new(); m];
    }

    fn configure(&mut self, cfg: &CalibratorConfig) {
        self.apply_cfg(cfg);
        // Defensive: keep K finite.
        for v in self.state.k_c.iter_mut() {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        if !self.state.k_f.is_finite() {
            self.state.k_f = 0.0;
        }
    }

    fn update(&mut self, events: &[CalibEvent]) -> PolicyRevDelta {
        let mut any_change = false;

        for e in events {
            match &e.result {
                JobResult::EarlyInfeasible { estimates, .. } => {
                    if e.audited
                        && let Some(audit_of) = &e.audit_of
                    {
                        let j = audit_of.violated_j;
                        if j < self.counts.len() {
                            self.counts[j].cut += 1;
                            let lvl = audit_of.phi_idx_at_cut as usize;
                            self.counts[j].ensure_level(lvl);
                            self.counts[j].cut_by_phi_idx[lvl] += 1;
                        }
                        self.pending.insert(
                            e.id,
                            PendingAudit {
                                audit_of: audit_of.clone(),
                                est_at_cut: estimates.clone(),
                                paired_sample: e.paired_sample.clone(),
                            },
                        );
                    }
                }
                JobResult::Truth { feasible, f, c, .. } => {
                    if let Some(p) = self.pending.remove(&e.id) {
                        // Delta false-infeasible accounting.
                        let j0 = p.audit_of.violated_j;
                        if j0 < self.counts.len() {
                            self.counts[j0].audit += 1;
                            let lvl = p.audit_of.phi_idx_at_cut as usize;
                            self.counts[j0].ensure_level(lvl);
                            self.counts[j0].audit_by_phi_idx[lvl] += 1;
                            if *feasible {
                                self.counts[j0].false_infeas += 1;
                                self.counts[j0].false_infeas_by_phi_idx[lvl] += 1;
                            }
                        }

                        // Prefer a true paired sample: same x, same S, tighter tau.
                        if let Some(ps) = &p.paired_sample {
                            let tau_loose = p.audit_of.phi_at_cut.tau.0 as f64;
                            let tau_tight = ps.paired_phi.tau.0 as f64;
                            let dtau = (tau_loose - tau_tight).abs().max(1.0);

                            // Objective K.
                            let df = (p.est_at_cut.f_hat - ps.estimates.f_hat).max(0.0);
                            let kf = df / dtau;
                            Self::push_k_sample(&mut self.k_samples_f, kf, self.k_window);

                            // Constraint K.
                            let m = self.state.k_c.len();
                            for jj in 0..m {
                                let cc_cut = p.est_at_cut.c_hat.get(jj).copied().unwrap_or(0.0);
                                let cc_pair = ps.estimates.c_hat.get(jj).copied().unwrap_or(0.0);
                                let dc = (cc_cut - cc_pair).max(0.0);
                                let kc = dc / dtau;
                                if jj < self.k_samples_c.len() {
                                    Self::push_k_sample(
                                        &mut self.k_samples_c[jj],
                                        kc,
                                        self.k_window,
                                    );
                                }
                            }
                        } else {
                            // Fallback: tau-normalized cut-vs-truth difference.
                            let tau_scale = (p.audit_of.phi_at_cut.tau.0 as f64).max(1.0);

                            let df = (p.est_at_cut.f_hat - *f).max(0.0);
                            let kf = df / tau_scale;
                            Self::push_k_sample(&mut self.k_samples_f, kf, self.k_window);

                            let m = self.state.k_c.len();
                            for jj in 0..m {
                                let cc_cut = p.est_at_cut.c_hat.get(jj).copied().unwrap_or(0.0);
                                let cc_truth = c.get(jj).copied().unwrap_or(0.0);
                                let dc = (cc_cut - cc_truth).max(0.0);
                                let kc = dc / tau_scale;
                                if jj < self.k_samples_c.len() {
                                    Self::push_k_sample(
                                        &mut self.k_samples_c[jj],
                                        kc,
                                        self.k_window,
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if self.update_delta() {
            any_change = true;
        }

        if self.update_k_from_samples() {
            any_change = true;
        }

        self.sync_counts_to_state();

        if any_change {
            self.state.policy_rev = PolicyRev(self.state.policy_rev.0 + 1);
            PolicyRevDelta(1)
        } else {
            PolicyRevDelta(0)
        }
    }

    fn state(&self) -> CalibState {
        self.state.clone()
    }
}
