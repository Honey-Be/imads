use crate::policies::calibrator::{CalibState, PolicyRevDelta};
use crate::types::{JobResult, Phi, XMesh};

#[derive(Default, Clone, Debug)]
pub struct DidsHistory {
    pub records: Vec<DidsRecord>,
}

#[derive(Clone, Debug)]
pub struct DidsRecord {
    pub x: XMesh,
    pub phi: Phi,
    pub phi_idx: u32,
    pub outcome: OutcomeTag,
    pub violated_j: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutcomeTag {
    RejectedCheap,
    RejectedInvalidEval,
    EarlyInfeasible,
    Partial,
    Truth,
}

/// DIDS policy. Safe to customize.
///
/// # Contract
/// - `record` is called deterministically for completed work items.
/// - `update_assignment` must be called only at batch boundaries.
/// - Assignment affects *early termination efficiency*, not feasibility confirmation.
pub trait DidsPolicy: Send + Sync {
    fn init(&mut self, m: usize);

    fn record(&mut self, x: XMesh, phi: Phi, phi_idx: u32, result: &JobResult);

    /// Returns (assignment vector a_j in 1..=L, policy rev delta).
    ///
    /// Semantics (conservative, MADS-friendly):
    /// - `a_j` is the *minimum* fidelity level (1-based) at which we allow early infeasible
    ///   termination for constraint `j`.
    /// - Thus, larger `a_j` is **more conservative** (requires higher fidelity before cutting).
    ///
    /// If the returned delta is non-zero, the engine will bump the global `policy_rev`
    /// so that decision-cache entries do not get reused across assignment changes.
    fn update_assignment(
        &mut self,
        ladder_len: usize,
        calib: &CalibState,
    ) -> (Vec<usize>, PolicyRevDelta);
}

#[derive(Default)]
pub struct DefaultDids {
    pub m: usize,
    pub history: DidsHistory,
    pub assignment: Vec<usize>,
    pub max_records: usize,
    /// Baseline delta used to interpret false-infeasible severity.
    pub delta_baseline: f64,
}

impl DefaultDids {
    fn compute_assignment(&self, ladder_len: usize, calib: &CalibState) -> Vec<usize> {
        // Step 1: data-driven base rule from observed early-cut levels.
        let l = ladder_len.max(1);
        let mut per_j: Vec<Vec<usize>> = vec![Vec::new(); self.m];
        for r in self.history.records.iter() {
            if r.outcome == OutcomeTag::EarlyInfeasible
                && let Some(j) = r.violated_j
                && j < self.m
            {
                let level = ((r.phi_idx as usize) + 1).clamp(1, l);
                per_j[j].push(level);
            }
        }

        let min_count = 10usize;
        let q = 0.80f64;
        let mut out = vec![1usize; self.m];
        for j in 0..self.m {
            let mut v = per_j[j].clone();
            if v.len() < min_count {
                out[j] = 1;
                continue;
            }
            v.sort_unstable();
            let n = v.len();
            let idx = ((q * (n as f64)).ceil() as isize - 1).clamp(0, (n as isize) - 1) as usize;
            let aj = v[idx].clamp(1, l);
            out[j] = if aj == 1 && n < 50 { 2.min(l) } else { aj };
        }

        // Step 2: incorporate *measured* audit performance using per-phi buckets.
        // `audit_n_by_phi_idx[j][k]` / `false_infeas_n_by_phi_idx[j][k]` corresponds to ladder level k+1.
        let t = calib.target_false;
        let min_audits = calib.min_audits.max(1);
        if t.is_finite() && t > 0.0 && t <= 1.0 {
            for (j, out_item) in out.iter_mut().enumerate() {
                let aud_by_phi = calib.audit_n_by_phi_idx.get(j);
                let false_by_phi = calib.false_infeas_n_by_phi_idx.get(j);
                if let (Some(aud), Some(fals)) = (aud_by_phi, false_by_phi) {
                    for lvl0 in 0..l {
                        let aud_n = aud.get(lvl0).copied().unwrap_or(0);
                        let fals_n = fals.get(lvl0).copied().unwrap_or(0);
                        if aud_n < min_audits {
                            continue;
                        }
                        let p_false = (fals_n as f64 + 1.0) / (aud_n as f64 + 2.0);
                        if !p_false.is_finite() {
                            continue;
                        }
                        // If level-specific false rate is high, forbid cutting here by pushing a_j above this level.
                        if p_false > t * 1.25 {
                            *out_item = (*out_item).max((lvl0 + 2).min(l));
                        }
                    }
                }

                // Gentle relaxation only if every well-audited level at/above current a_j is extremely safe.
                let overall_aud = calib.audit_n.get(j).copied().unwrap_or(0);
                let overall_false = calib.false_infeas_n.get(j).copied().unwrap_or(0);
                if overall_aud >= (min_audits * 5) {
                    let p_false = (overall_false as f64 + 1.0) / (overall_aud as f64 + 2.0);
                    if p_false < (t / 10.0) && *out_item > 1 {
                        *out_item -= 1;
                    }
                }
            }
        }

        // Step 3: incorporate false-infeasible proxy signal from `delta_rel` (fallback / extra guard).
        let base = if self.delta_baseline.is_finite() && self.delta_baseline > 0.0 {
            self.delta_baseline
        } else {
            0.005
        };
        for (j, out_item) in out.iter_mut().enumerate() {
            let dj = calib.delta_rel.get(j).copied().unwrap_or(base);
            if !(dj.is_finite() && dj > 0.0) {
                continue;
            }
            let ratio = dj / base;
            if ratio <= 1.2 {
                continue;
            }
            let bumps = (ratio.ln() / 2f64.ln()).floor().max(0.0) as usize;
            if bumps == 0 {
                continue;
            }
            *out_item = (*out_item + bumps).min(l);
        }

        for aj in out.iter_mut() {
            *aj = (*aj).clamp(1, l);
        }

        out
    }
}

impl DidsPolicy for DefaultDids {
    fn init(&mut self, m: usize) {
        self.m = m;
        self.history = DidsHistory::default();
        self.assignment = vec![1; m];
        self.max_records = 5000;
        self.delta_baseline = 0.005;
    }

    fn record(&mut self, x: XMesh, phi: Phi, phi_idx: u32, result: &JobResult) {
        let (outcome, violated_j) = match result {
            JobResult::RejectedCheap { .. } => (OutcomeTag::RejectedCheap, None),
            JobResult::RejectedInvalidEval { .. } => (OutcomeTag::RejectedInvalidEval, None),
            JobResult::EarlyInfeasible { violated_j, .. } => {
                (OutcomeTag::EarlyInfeasible, Some(*violated_j))
            }
            JobResult::Partial { .. } => (OutcomeTag::Partial, None),
            JobResult::Truth { .. } => (OutcomeTag::Truth, None),
        };
        self.history.records.push(DidsRecord {
            x,
            phi,
            phi_idx,
            outcome,
            violated_j,
        });
        if self.history.records.len() > self.max_records.max(1) {
            let excess = self.history.records.len() - self.max_records.max(1);
            self.history.records.drain(0..excess);
        }
    }

    fn update_assignment(
        &mut self,
        ladder_len: usize,
        calib: &CalibState,
    ) -> (Vec<usize>, PolicyRevDelta) {
        // Keep baseline updated to the initial delta if available.
        if let Some(d0) = calib.delta_rel.first()
            && d0.is_finite()
            && *d0 > 0.0
        {
            // Use the median/typical scale by taking the minimum across constraints.
            let mut minv = f64::INFINITY;
            for d in calib.delta_rel.iter() {
                if d.is_finite() && *d > 0.0 {
                    minv = minv.min(*d);
                }
            }
            if minv.is_finite() {
                self.delta_baseline = minv;
            }
        }

        let new_a = self.compute_assignment(ladder_len, calib);
        let changed = new_a != self.assignment;
        self.assignment = new_a.clone();
        (
            new_a,
            if changed {
                PolicyRevDelta(1)
            } else {
                PolicyRevDelta(0)
            },
        )
    }
}
