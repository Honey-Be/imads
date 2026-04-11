use crate::types::{CandidateId, Estimates, JobResult, Phi, PolicyRev, Smc, Tau};

use std::collections::hash_map::Entry as HashEntry;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug)]
pub struct PolicyRevDelta(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KIntervalKey {
    pub tau_loose: Tau,
    pub tau_tight: Tau,
    pub smc: Smc,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KIntervalState {
    pub key: KIntervalKey,
    pub k_f: f64,
    pub k_c: Vec<f64>,
    pub sample_count_f: usize,
    pub sample_count_c: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KByPhiState {
    pub phi: Phi,
    pub k_f: f64,
    pub k_c: Vec<f64>,
}

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
    /// Number of audited early-cuts whose originally flagged constraint `j` remained
    /// violated at TRUTH.
    pub confirmed_violation_n: Vec<usize>,
    /// Phi-index-bucketed counts (same ordering as ladder indices, 0-based inside vectors).
    pub cut_n_by_phi_idx: Vec<Vec<usize>>,
    pub audit_n_by_phi_idx: Vec<Vec<usize>>,
    pub false_infeas_n_by_phi_idx: Vec<Vec<usize>>,
    pub confirmed_violation_n_by_phi_idx: Vec<Vec<usize>>,
    /// Interval-bucketed K statistics keyed by `(tau_loose, tau_tight, S)`.
    pub k_interval_stats: Vec<KIntervalState>,
    /// Effective K statistics keyed by the current `(tau, S)` bucket.
    pub k_by_phi: Vec<KByPhiState>,
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
    /// Whether this completion was part of an audit path.
    pub audited: bool,
    /// If present, this event is associated with the original early-cut being audited.
    pub audit_of: Option<AuditOf>,
    /// Optional same-x / same-S / tighter-tau checkpoint collected on this event.
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
            confirmed_violation_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
            confirmed_violation_n_by_phi_idx: vec![Vec::new(); m],
            k_interval_stats: Vec::new(),
            k_by_phi: Vec::new(),
        };
    }

    fn configure(&mut self, cfg: &CalibratorConfig) {
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
    confirmed_violation: usize,
    cut_by_phi_idx: Vec<usize>,
    audit_by_phi_idx: Vec<usize>,
    false_infeas_by_phi_idx: Vec<usize>,
    confirmed_violation_by_phi_idx: Vec<usize>,
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
        if self.confirmed_violation_by_phi_idx.len() <= idx {
            self.confirmed_violation_by_phi_idx.resize(idx + 1, 0);
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
        if self.state.confirmed_violation_n.len() != m {
            self.state.confirmed_violation_n = vec![0; m];
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
        if self.state.confirmed_violation_n_by_phi_idx.len() != m {
            self.state.confirmed_violation_n_by_phi_idx = vec![Vec::new(); m];
        }
        for j in 0..m {
            self.state.cut_n[j] = self.counts.get(j).map(|c| c.cut).unwrap_or(0);
            self.state.audit_n[j] = self.counts.get(j).map(|c| c.audit).unwrap_or(0);
            self.state.false_infeas_n[j] = self.counts.get(j).map(|c| c.false_infeas).unwrap_or(0);
            self.state.confirmed_violation_n[j] = self
                .counts
                .get(j)
                .map(|c| c.confirmed_violation)
                .unwrap_or(0);
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
            self.state.confirmed_violation_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.confirmed_violation_by_phi_idx.clone())
                .unwrap_or_default();
        }
    }

    fn update_delta(&mut self) -> bool {
        let m = self.state.delta_rel.len();
        let mut changed = false;
        for j in 0..m {
            let c = &self.counts[j];
            if c.audit < self.state.min_audits.max(1) {
                continue;
            }
            let p_false = (c.false_infeas as f64 + 1.0) / (c.audit as f64 + 2.0);
            let t = self.state.target_false.clamp(0.0, 1.0);
            let mut s = 0i32;
            if p_false > t {
                s = 1;
            } else if p_false < (t / 3.0) {
                s = -1;
            }
            if s == 0 {
                continue;
            }
            let old = self.state.delta_rel[j];
            let eta = self.state.eta_delta.max(0.0);
            let mult = if s > 0 { eta.exp() } else { (-eta).exp() };
            let next = (old * mult).clamp(self.state.delta_min, self.state.delta_max);
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
            confirmed_violation_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
            confirmed_violation_n_by_phi_idx: vec![Vec::new(); m],
            k_interval_stats: Vec::new(),
            k_by_phi: Vec::new(),
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
                JobResult::EarlyInfeasible { .. } | JobResult::Partial { .. } => {
                    if e.audited
                        && let Some(audit_of) = &e.audit_of
                        && let HashEntry::Vacant(v) = self.pending.entry(e.id)
                    {
                        let j = audit_of.violated_j;
                        if j < self.counts.len() {
                            self.counts[j].cut += 1;
                            let lvl = audit_of.phi_idx_at_cut as usize;
                            self.counts[j].ensure_level(lvl);
                            self.counts[j].cut_by_phi_idx[lvl] += 1;
                        }
                        v.insert(audit_of.clone());
                    }
                }
                JobResult::Truth { feasible, c, .. } => {
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
                            if c.get(j).copied().unwrap_or(0.0) > 0.0 {
                                self.counts[j].confirmed_violation += 1;
                                self.counts[j].confirmed_violation_by_phi_idx[lvl] += 1;
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
    paired_samples: Vec<PairedAuditSample>,
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

    // Global K samples (tau-normalized).
    k_samples_f: Vec<f64>,
    k_samples_c: Vec<Vec<f64>>,
    // Same-(tau,S) bucketed K samples keyed by the loose endpoint phi.
    k_phi_samples_f: BTreeMap<Phi, Vec<f64>>,
    k_phi_samples_c: BTreeMap<Phi, Vec<Vec<f64>>>,
    // Interval-bucketed K samples keyed by (tau_loose, tau_tight, S).
    k_interval_samples_f: BTreeMap<KIntervalKey, Vec<f64>>,
    k_interval_samples_c: BTreeMap<KIntervalKey, Vec<Vec<f64>>>,

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
            k_phi_samples_f: BTreeMap::new(),
            k_phi_samples_c: BTreeMap::new(),
            k_interval_samples_f: BTreeMap::new(),
            k_interval_samples_c: BTreeMap::new(),
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
        if self.state.confirmed_violation_n.len() != m {
            self.state.confirmed_violation_n = vec![0; m];
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
        if self.state.confirmed_violation_n_by_phi_idx.len() != m {
            self.state.confirmed_violation_n_by_phi_idx = vec![Vec::new(); m];
        }
        for j in 0..m {
            self.state.cut_n[j] = self.counts.get(j).map(|c| c.cut).unwrap_or(0);
            self.state.audit_n[j] = self.counts.get(j).map(|c| c.audit).unwrap_or(0);
            self.state.false_infeas_n[j] = self.counts.get(j).map(|c| c.false_infeas).unwrap_or(0);
            self.state.confirmed_violation_n[j] = self
                .counts
                .get(j)
                .map(|c| c.confirmed_violation)
                .unwrap_or(0);
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
            self.state.confirmed_violation_n_by_phi_idx[j] = self
                .counts
                .get(j)
                .map(|c| c.confirmed_violation_by_phi_idx.clone())
                .unwrap_or_default();
        }
    }

    fn update_delta(&mut self) -> bool {
        let m = self.state.delta_rel.len();
        let mut changed = false;
        for j in 0..m {
            let c = &self.counts[j];
            if c.audit < self.state.min_audits.max(1) {
                continue;
            }
            let p_false = (c.false_infeas as f64 + 1.0) / (c.audit as f64 + 2.0);
            let t = self.state.target_false.clamp(0.0, 1.0);
            let mut s = 0i32;
            if p_false > t {
                s = 1;
            } else if p_false < (t / 3.0) {
                s = -1;
            }
            if s == 0 {
                continue;
            }
            let old = self.state.delta_rel[j];
            let eta = self.state.eta_delta.max(0.0);
            let mult = if s > 0 { eta.exp() } else { (-eta).exp() };
            let next = (old * mult).clamp(self.state.delta_min, self.state.delta_max);
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

    fn push_nested_k_sample(vecs: &mut Vec<Vec<f64>>, j: usize, v: f64, cap: usize) {
        if vecs.len() <= j {
            vecs.resize_with(j + 1, Vec::new);
        }
        Self::push_k_sample(&mut vecs[j], v, cap);
    }

    fn quantile_of(samples: &mut [f64], q: f64) -> Option<f64> {
        if samples.is_empty() {
            return None;
        }
        samples.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let qq = q.clamp(0.0, 1.0);
        let n = samples.len();
        let idx = ((qq * ((n - 1) as f64)).round() as usize).min(n - 1);
        Some(samples[idx])
    }

    fn ema_quantile_update(&self, old: f64, samples: &[f64]) -> (f64, bool) {
        if samples.len() < self.k_min_pairs.max(1) {
            return (old, false);
        }
        let mut tmp = samples.to_vec();
        if let Some(qv) = Self::quantile_of(&mut tmp[..], self.k_quantile) {
            let next = ((1.0 - self.k_eta) * old + self.k_eta * qv).max(0.0);
            return (next, (next - old).abs() > 1e-15);
        }
        (old, false)
    }

    fn append_paired_sample(pending: &mut PendingAudit, ps: PairedAuditSample) {
        if let Some(existing) = pending
            .paired_samples
            .iter_mut()
            .find(|cur| cur.paired_phi_idx == ps.paired_phi_idx)
        {
            *existing = ps;
        } else {
            pending.paired_samples.push(ps);
            pending
                .paired_samples
                .sort_by_key(|sample| sample.paired_phi_idx);
        }
    }

    fn note_audit_checkpoint(
        &mut self,
        id: CandidateId,
        audit_of: &AuditOf,
        est: &Estimates,
        paired_sample: Option<&PairedAuditSample>,
    ) {
        match self.pending.entry(id) {
            HashEntry::Vacant(v) => {
                let j = audit_of.violated_j;
                if j < self.counts.len() {
                    self.counts[j].cut += 1;
                    let lvl = audit_of.phi_idx_at_cut as usize;
                    self.counts[j].ensure_level(lvl);
                    self.counts[j].cut_by_phi_idx[lvl] += 1;
                }
                let mut pending = PendingAudit {
                    audit_of: audit_of.clone(),
                    est_at_cut: est.clone(),
                    paired_samples: Vec::new(),
                };
                if est.phi == audit_of.phi_at_cut {
                    pending.est_at_cut = est.clone();
                }
                if let Some(ps) = paired_sample.cloned() {
                    Self::append_paired_sample(&mut pending, ps);
                }
                v.insert(pending);
            }
            HashEntry::Occupied(mut o) => {
                let pending = o.get_mut();
                if est.phi == audit_of.phi_at_cut {
                    pending.est_at_cut = est.clone();
                }
                if let Some(ps) = paired_sample.cloned() {
                    Self::append_paired_sample(pending, ps);
                }
            }
        }
    }

    fn truth_as_estimates(phi: Phi, f: &[f64], c: &[f64], m: usize) -> Estimates {
        let mut c_hat = vec![0.0; m];
        for (j, ch) in c_hat.iter_mut().enumerate() {
            *ch = c.get(j).copied().unwrap_or(0.0);
        }
        let num_objectives = f.len();
        Estimates {
            f_hat: f.to_vec(),
            f_se: vec![0.0; num_objectives],
            c_hat,
            c_se: vec![0.0; m],
            phi,
            tau_scale: phi.tau.0 as f64,
            num_objectives,
        }
    }

    fn register_interval_sample(
        &mut self,
        loose_phi: Phi,
        loose_est: &Estimates,
        tight_phi: Phi,
        tight_est: &Estimates,
    ) {
        if loose_phi.smc != tight_phi.smc || tight_phi.tau.0 >= loose_phi.tau.0 {
            return;
        }
        let dtau = (loose_phi.tau.0 as f64 - tight_phi.tau.0 as f64).max(1.0);
        let key = KIntervalKey {
            tau_loose: loose_phi.tau,
            tau_tight: tight_phi.tau,
            smc: loose_phi.smc,
        };

        let df = (loose_est.f_hat_primary() - tight_est.f_hat_primary()).max(0.0) / dtau;
        Self::push_k_sample(&mut self.k_samples_f, df, self.k_window);
        Self::push_k_sample(
            self.k_phi_samples_f.entry(loose_phi).or_default(),
            df,
            self.k_window,
        );
        Self::push_k_sample(
            self.k_interval_samples_f.entry(key).or_default(),
            df,
            self.k_window,
        );

        let m = self.state.k_c.len();
        for j in 0..m {
            let loose_c = loose_est.c_hat.get(j).copied().unwrap_or(0.0);
            let tight_c = tight_est.c_hat.get(j).copied().unwrap_or(0.0);
            let dc = (loose_c - tight_c).max(0.0) / dtau;
            if j < self.k_samples_c.len() {
                Self::push_k_sample(&mut self.k_samples_c[j], dc, self.k_window);
            }
            let phi_entry = self
                .k_phi_samples_c
                .entry(loose_phi)
                .or_insert_with(|| vec![Vec::new(); m]);
            Self::push_nested_k_sample(phi_entry, j, dc, self.k_window);
            let interval_entry = self
                .k_interval_samples_c
                .entry(key)
                .or_insert_with(|| vec![Vec::new(); m]);
            Self::push_nested_k_sample(interval_entry, j, dc, self.k_window);
        }
    }

    fn register_cut_to_truth_fallback(
        &mut self,
        cut_phi: Phi,
        cut_est: &Estimates,
        truth_phi: Phi,
        truth_est: &Estimates,
    ) {
        let tau_scale = (cut_phi.tau.0 as f64).max(1.0);
        let df = (cut_est.f_hat_primary() - truth_est.f_hat_primary()).max(0.0) / tau_scale;
        Self::push_k_sample(&mut self.k_samples_f, df, self.k_window);
        Self::push_k_sample(
            self.k_phi_samples_f.entry(cut_phi).or_default(),
            df,
            self.k_window,
        );

        let m = self.state.k_c.len();
        for j in 0..m {
            let cut_c = cut_est.c_hat.get(j).copied().unwrap_or(0.0);
            let truth_c = truth_est.c_hat.get(j).copied().unwrap_or(0.0);
            let dc = (cut_c - truth_c).max(0.0) / tau_scale;
            if j < self.k_samples_c.len() {
                Self::push_k_sample(&mut self.k_samples_c[j], dc, self.k_window);
            }
            let phi_entry = self
                .k_phi_samples_c
                .entry(cut_phi)
                .or_insert_with(|| vec![Vec::new(); m]);
            Self::push_nested_k_sample(phi_entry, j, dc, self.k_window);
        }

        let _ = truth_phi;
    }

    fn update_global_k(&mut self) -> bool {
        let mut changed = false;
        let (next_kf, ch_f) = self.ema_quantile_update(self.state.k_f, &self.k_samples_f);
        if ch_f {
            self.state.k_f = next_kf;
            changed = true;
        }
        for j in 0..self.state.k_c.len() {
            let samples = self.k_samples_c.get(j).map(Vec::as_slice).unwrap_or(&[]);
            let old = self.state.k_c[j];
            let (next, ch) = self.ema_quantile_update(old, samples);
            if ch {
                self.state.k_c[j] = next;
                changed = true;
            }
        }
        changed
    }

    fn update_interval_k(&mut self) -> bool {
        let m = self.state.k_c.len();
        let old_states = self.state.k_interval_stats.clone();
        let old_map: BTreeMap<KIntervalKey, KIntervalState> =
            old_states.iter().cloned().map(|st| (st.key, st)).collect();

        let mut keys: Vec<KIntervalKey> = self.k_interval_samples_f.keys().copied().collect();
        for key in self.k_interval_samples_c.keys() {
            if !keys.contains(key) {
                keys.push(*key);
            }
        }
        for key in old_map.keys() {
            if !keys.contains(key) {
                keys.push(*key);
            }
        }
        keys.sort();

        let mut changed = false;
        let mut next_states = Vec::with_capacity(keys.len());
        for key in keys {
            let old = old_map.get(&key);
            let old_f = old.map(|st| st.k_f).unwrap_or(0.0);
            let f_samples = self
                .k_interval_samples_f
                .get(&key)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let (k_f, ch_f) = self.ema_quantile_update(old_f, f_samples);
            changed |= ch_f;

            let mut k_c = old.map(|st| st.k_c.clone()).unwrap_or_else(|| vec![0.0; m]);
            if k_c.len() < m {
                k_c.resize(m, 0.0);
            }
            let mut sample_count_c = vec![0usize; m];
            if let Some(samples_by_j) = self.k_interval_samples_c.get(&key) {
                if sample_count_c.len() < samples_by_j.len() {
                    sample_count_c.resize(samples_by_j.len(), 0);
                }
                for (j, samples) in samples_by_j.iter().enumerate() {
                    sample_count_c[j] = samples.len();
                    let (next, ch) = self.ema_quantile_update(*k_c.get(j).unwrap_or(&0.0), samples);
                    if j >= k_c.len() {
                        k_c.resize(j + 1, 0.0);
                    }
                    if ch {
                        k_c[j] = next;
                        changed = true;
                    }
                }
            }

            next_states.push(KIntervalState {
                key,
                k_f,
                k_c,
                sample_count_f: f_samples.len(),
                sample_count_c,
            });
        }

        self.state.k_interval_stats = next_states;
        changed
    }

    fn update_phi_bucket_k(&mut self) -> bool {
        let m = self.state.k_c.len();
        let old_states = self.state.k_by_phi.clone();
        let old_map: BTreeMap<Phi, KByPhiState> =
            old_states.iter().cloned().map(|st| (st.phi, st)).collect();

        let mut phis: Vec<Phi> = self.k_phi_samples_f.keys().copied().collect();
        for phi in self.k_phi_samples_c.keys() {
            if !phis.contains(phi) {
                phis.push(*phi);
            }
        }
        for phi in old_map.keys() {
            if !phis.contains(phi) {
                phis.push(*phi);
            }
        }
        phis.sort();

        let mut changed = false;
        let mut next_states = Vec::with_capacity(phis.len());
        for phi in phis {
            let old = old_map.get(&phi);
            let old_f = old.map(|st| st.k_f).unwrap_or(0.0);
            let f_samples = self
                .k_phi_samples_f
                .get(&phi)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let (k_f, ch_f) = self.ema_quantile_update(old_f, f_samples);
            changed |= ch_f;

            let mut k_c = old.map(|st| st.k_c.clone()).unwrap_or_else(|| vec![0.0; m]);
            if k_c.len() < m {
                k_c.resize(m, 0.0);
            }
            if let Some(samples_by_j) = self.k_phi_samples_c.get(&phi) {
                for (j, samples) in samples_by_j.iter().enumerate() {
                    let old_j = *k_c.get(j).unwrap_or(&0.0);
                    let (next, ch) = self.ema_quantile_update(old_j, samples);
                    if j >= k_c.len() {
                        k_c.resize(j + 1, 0.0);
                    }
                    if ch {
                        k_c[j] = next;
                        changed = true;
                    }
                }
            }

            next_states.push(KByPhiState { phi, k_f, k_c });
        }

        if old_states != next_states {
            changed = true;
        }
        self.state.k_by_phi = next_states;
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
            confirmed_violation_n: vec![0; m],
            cut_n_by_phi_idx: vec![Vec::new(); m],
            audit_n_by_phi_idx: vec![Vec::new(); m],
            false_infeas_n_by_phi_idx: vec![Vec::new(); m],
            confirmed_violation_n_by_phi_idx: vec![Vec::new(); m],
            k_interval_stats: Vec::new(),
            k_by_phi: Vec::new(),
        };
        self.counts = vec![Counts::default(); m];
        self.pending.clear();
        self.k_samples_f.clear();
        self.k_samples_c = vec![Vec::new(); m];
        self.k_phi_samples_f.clear();
        self.k_phi_samples_c.clear();
        self.k_interval_samples_f.clear();
        self.k_interval_samples_c.clear();
    }

    fn configure(&mut self, cfg: &CalibratorConfig) {
        self.apply_cfg(cfg);
        for v in self.state.k_c.iter_mut() {
            if !v.is_finite() {
                *v = 0.0;
            }
        }
        if !self.state.k_f.is_finite() {
            self.state.k_f = 0.0;
        }
        for st in self.state.k_by_phi.iter_mut() {
            if !st.k_f.is_finite() {
                st.k_f = 0.0;
            }
            if st.k_c.len() < self.state.k_c.len() {
                st.k_c.resize(self.state.k_c.len(), 0.0);
            }
            for v in st.k_c.iter_mut() {
                if !v.is_finite() {
                    *v = 0.0;
                }
            }
        }
    }

    fn update(&mut self, events: &[CalibEvent]) -> PolicyRevDelta {
        let mut any_change = false;

        for e in events {
            match &e.result {
                JobResult::EarlyInfeasible { estimates, .. }
                | JobResult::Partial { estimates, .. } => {
                    if e.audited
                        && let Some(audit_of) = &e.audit_of
                    {
                        self.note_audit_checkpoint(
                            e.id,
                            audit_of,
                            estimates,
                            e.paired_sample.as_ref(),
                        );
                    }
                }
                JobResult::Truth {
                    feasible,
                    f,
                    c,
                    meta,
                    ..
                } => {
                    if let Some(mut pending) = self.pending.remove(&e.id) {
                        if let Some(ps) = e.paired_sample.clone() {
                            Self::append_paired_sample(&mut pending, ps);
                        }

                        let j0 = pending.audit_of.violated_j;
                        if j0 < self.counts.len() {
                            self.counts[j0].audit += 1;
                            let lvl = pending.audit_of.phi_idx_at_cut as usize;
                            self.counts[j0].ensure_level(lvl);
                            self.counts[j0].audit_by_phi_idx[lvl] += 1;
                            if *feasible {
                                self.counts[j0].false_infeas += 1;
                                self.counts[j0].false_infeas_by_phi_idx[lvl] += 1;
                            }
                            if c.get(j0).copied().unwrap_or(0.0) > 0.0 {
                                self.counts[j0].confirmed_violation += 1;
                                self.counts[j0].confirmed_violation_by_phi_idx[lvl] += 1;
                            }
                        }

                        let truth_est =
                            Self::truth_as_estimates(meta.phi, f, c, self.state.k_c.len());

                        let mut checkpoints: Vec<(Phi, Estimates)> = Vec::new();
                        checkpoints.push((pending.audit_of.phi_at_cut, pending.est_at_cut.clone()));
                        pending
                            .paired_samples
                            .sort_by_key(|sample| sample.paired_phi_idx);
                        for ps in pending.paired_samples.into_iter() {
                            checkpoints.push((ps.paired_phi, ps.estimates));
                        }

                        let mut structured_pairs = 0usize;
                        for window in checkpoints.windows(2) {
                            let (loose_phi, loose_est) = (&window[0].0, &window[0].1);
                            let (tight_phi, tight_est) = (&window[1].0, &window[1].1);
                            if loose_phi.smc == tight_phi.smc && tight_phi.tau.0 < loose_phi.tau.0 {
                                self.register_interval_sample(
                                    *loose_phi, loose_est, *tight_phi, tight_est,
                                );
                                structured_pairs += 1;
                            }
                        }

                        if structured_pairs == 0 {
                            self.register_cut_to_truth_fallback(
                                pending.audit_of.phi_at_cut,
                                &pending.est_at_cut,
                                meta.phi,
                                &truth_est,
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        if self.update_delta() {
            any_change = true;
        }
        if self.update_global_k() {
            any_change = true;
        }
        if self.update_interval_k() {
            any_change = true;
        }
        if self.update_phi_bucket_k() {
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
