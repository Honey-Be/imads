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
    fn recency_weight(age: usize, half_life: f64) -> f64 {
        if half_life <= 0.0 {
            return 1.0;
        }
        (-std::f64::consts::LN_2 * (age as f64) / half_life).exp()
    }

    fn effective_sample_size(weighted: &[(usize, f64)]) -> f64 {
        let sum_w: f64 = weighted.iter().map(|(_, w)| *w).sum();
        let sum_w2: f64 = weighted.iter().map(|(_, w)| w * w).sum();
        if sum_w2 <= 0.0 {
            0.0
        } else {
            (sum_w * sum_w) / sum_w2
        }
    }

    fn weighted_quantile(weighted: &[(usize, f64)], q: f64) -> Option<usize> {
        if weighted.is_empty() {
            return None;
        }
        let mut items = weighted.to_vec();
        items.sort_unstable_by_key(|(level, _)| *level);
        let total_w: f64 = items.iter().map(|(_, w)| *w).sum();
        if total_w <= 0.0 {
            return None;
        }
        let target = q.clamp(0.0, 1.0) * total_w;
        let mut acc = 0.0;
        for (level, w) in items {
            acc += w;
            if acc + 1e-12 >= target {
                return Some(level);
            }
        }
        weighted.iter().map(|(level, _)| *level).max()
    }

    fn wilson_interval(success: usize, total: usize, z: f64) -> (f64, f64) {
        if total == 0 {
            return (0.0, 1.0);
        }
        let n = total as f64;
        let p = (success as f64) / n;
        let z2 = z * z;
        let denom = 1.0 + z2 / n;
        let center = (p + z2 / (2.0 * n)) / denom;
        let radius = (z / denom) * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).max(0.0)).sqrt();
        (
            (center - radius).clamp(0.0, 1.0),
            (center + radius).clamp(0.0, 1.0),
        )
    }

    fn compute_assignment(&self, ladder_len: usize, calib: &CalibState) -> Vec<usize> {
        let l = ladder_len.max(1);

        // Step 1: recency-weighted base rule from observed early-cut levels.
        let mut per_j: Vec<Vec<(usize, f64)>> = vec![Vec::new(); self.m];
        let hist_len = self.history.records.len();
        let half_life = ((hist_len.max(8) as f64) / 3.0).max(6.0);
        for (idx, r) in self.history.records.iter().enumerate() {
            if r.outcome == OutcomeTag::EarlyInfeasible
                && let Some(j) = r.violated_j
                && j < self.m
            {
                let level = ((r.phi_idx as usize) + 1).clamp(1, l);
                let age = hist_len.saturating_sub(idx + 1);
                let w = Self::recency_weight(age, half_life);
                per_j[j].push((level, w));
            }
        }

        let q = 0.80f64;
        let min_eff_count = 6.0f64;
        let mut out = vec![1usize; self.m];
        for j in 0..self.m {
            let eff_n = Self::effective_sample_size(&per_j[j]);
            if eff_n < min_eff_count {
                out[j] = 1;
                continue;
            }
            let aj = Self::weighted_quantile(&per_j[j], q)
                .unwrap_or(1)
                .clamp(1, l);
            out[j] = if aj == 1 && eff_n < 18.0 {
                2.min(l)
            } else {
                aj
            };
        }

        // Step 2: confidence-aware audit refinement using false-infeasible rate and precision.
        let t = if calib.target_false.is_finite() && calib.target_false > 0.0 {
            calib.target_false.clamp(0.0, 1.0)
        } else {
            0.01
        };
        let min_audits = calib.min_audits.max(1);
        let relax_min_audits = (min_audits * 2).max(8);
        let z = 1.96;

        #[allow(clippy::needless_range_loop)]
        for j in 0..self.m {
            let aud_by_phi = calib.audit_n_by_phi_idx.get(j);
            let false_by_phi = calib.false_infeas_n_by_phi_idx.get(j);
            let confirmed_by_phi = calib.confirmed_violation_n_by_phi_idx.get(j);
            let precision_enabled =
                confirmed_by_phi.is_some() || calib.confirmed_violation_n.get(j).is_some();

            if let (Some(aud), Some(fals)) = (aud_by_phi, false_by_phi) {
                for lvl0 in 0..l {
                    let aud_n = aud.get(lvl0).copied().unwrap_or(0);
                    if aud_n == 0 {
                        continue;
                    }
                    let fals_n = fals.get(lvl0).copied().unwrap_or(0);
                    let confirmed_n = confirmed_by_phi
                        .and_then(|v| v.get(lvl0).copied())
                        .unwrap_or(0);

                    let (_, false_hi) = Self::wilson_interval(fals_n, aud_n, z);
                    let (precision_lo, _) = if precision_enabled {
                        Self::wilson_interval(confirmed_n, aud_n, z)
                    } else {
                        (0.0, 1.0)
                    };

                    let false_alarm = aud_n >= min_audits && false_hi > t * 1.15;
                    let precision_alarm =
                        precision_enabled && aud_n >= min_audits && precision_lo < 0.55;
                    let severe_low_n_false =
                        aud_n >= (min_audits / 2).max(3) && fals_n > 0 && false_hi > t * 1.50;

                    if false_alarm || precision_alarm || severe_low_n_false {
                        out[j] = out[j].max((lvl0 + 2).min(l));
                    }
                }
            }

            // Step 2b: relax only when the immediately lower level has strong evidence.
            if out[j] > 1 {
                let lower_lvl0 = out[j] - 2;
                let aud_n = aud_by_phi
                    .and_then(|v| v.get(lower_lvl0).copied())
                    .unwrap_or(0);
                let fals_n = false_by_phi
                    .and_then(|v| v.get(lower_lvl0).copied())
                    .unwrap_or(0);
                let confirmed_n = confirmed_by_phi
                    .and_then(|v| v.get(lower_lvl0).copied())
                    .unwrap_or(0);

                let (_, false_hi) = Self::wilson_interval(fals_n, aud_n, z);
                let (precision_lo, _) = if precision_enabled {
                    Self::wilson_interval(confirmed_n, aud_n, z)
                } else {
                    (0.0, 1.0)
                };

                let overall_aud = calib.audit_n.get(j).copied().unwrap_or(0);
                let overall_false = calib.false_infeas_n.get(j).copied().unwrap_or(0);
                let overall_confirmed = calib.confirmed_violation_n.get(j).copied().unwrap_or(0);
                let (_, overall_false_hi) = Self::wilson_interval(overall_false, overall_aud, z);
                let (overall_precision_lo, _) = if precision_enabled {
                    Self::wilson_interval(overall_confirmed, overall_aud, z)
                } else {
                    (0.0, 1.0)
                };

                let precision_ok =
                    !precision_enabled || (precision_lo > 0.70 && overall_precision_lo > 0.65);
                if aud_n >= relax_min_audits
                    && overall_aud >= relax_min_audits
                    && false_hi < t * 0.60
                    && overall_false_hi < t * 0.75
                    && precision_ok
                {
                    out[j] -= 1;
                }
            }
        }

        // Step 3: fallback guard from `delta_rel`.
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
            if ratio <= 1.25 {
                continue;
            }
            let bumps = (ratio.ln() / 2f64.ln()).ceil().max(0.0) as usize;
            if bumps > 0 {
                *out_item = (*out_item + bumps).min(l);
            }
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
        if let Some(d0) = calib.delta_rel.first()
            && d0.is_finite()
            && *d0 > 0.0
        {
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
