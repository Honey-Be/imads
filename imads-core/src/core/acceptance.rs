//! Acceptance logic (Filter + Progressive barrier).
//!
//! The `AcceptancePolicy` trait is now a public extension point. Implementors
//! **must** maintain the progressive barrier invariant: `barrier_h()` must be
//! non-increasing across successful iterations. Violating this breaks convergence.
//!
//! In this framework, **only TRUTH results** can be accepted into the filter.
//! PARTIAL results may influence scheduling but never acceptance.

use crate::types::XMesh;

#[derive(Clone, Debug)]
pub struct AcceptanceConfig {
    /// Initial progressive barrier threshold h0 (allowed violation).
    pub h0: f64,
    /// Minimum barrier threshold.
    pub h_min: f64,
    /// Multiplicative shrink factor applied when we decide to tighten the barrier.
    ///
    /// Typical values: 0.5 .. 0.9.
    pub h_shrink: f64,
    /// Filter dominance slack in objective space.
    pub eps_f: f64,
    /// Filter dominance slack in violation space.
    pub eps_v: f64,
    /// Max number of non-dominated points stored in the filter.
    ///
    /// If exceeded, we conservatively keep the best-by-f points among those with
    /// smallest violations. (A heuristic cap to bound memory.)
    pub filter_cap: usize,
}

impl Default for AcceptanceConfig {
    fn default() -> Self {
        Self {
            // Conservative defaults: feasible-only unless user widens h0.
            h0: 0.0,
            h_min: 0.0,
            h_shrink: 0.5,
            eps_f: 1e-12,
            eps_v: 1e-12,
            filter_cap: 64,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BarrierState {
    /// Progressive barrier threshold h_k.
    pub h: f64,
}

impl BarrierState {
    pub fn new(cfg: &AcceptanceConfig) -> Self {
        let mut h0 = cfg.h0;
        if !h0.is_finite() || h0 < 0.0 {
            h0 = 0.0;
        }
        Self {
            h: h0.max(cfg.h_min.max(0.0)),
        }
    }

    pub fn tighten_on_poll_fail(&mut self, cfg: &AcceptanceConfig) {
        let shrink = cfg.h_shrink.clamp(0.0, 1.0);
        let next = self.h * shrink;
        self.h = next.max(cfg.h_min.max(0.0));
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FilterPoint {
    pub f: f64,
    pub v: f64,
}

fn dominates(a: FilterPoint, b: FilterPoint, eps_f: f64, eps_v: f64) -> bool {
    let eps_f = eps_f.max(0.0);
    let eps_v = eps_v.max(0.0);

    let f_le = a.f <= b.f + eps_f;
    let v_le = a.v <= b.v + eps_v;

    let f_lt = a.f < b.f - eps_f;
    let v_lt = a.v < b.v - eps_v;

    f_le && v_le && (f_lt || v_lt)
}

fn filter_insert(points: &mut Vec<FilterPoint>, p: FilterPoint, cfg: &AcceptanceConfig) -> bool {
    for &q in points.iter() {
        if dominates(q, p, cfg.eps_f, cfg.eps_v) {
            return false;
        }
    }
    points.retain(|&q| !dominates(p, q, cfg.eps_f, cfg.eps_v));
    points.push(p);

    if points.len() > cfg.filter_cap.max(1) {
        points.sort_by(|a, b| {
            a.v.partial_cmp(&b.v)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.f.partial_cmp(&b.f).unwrap_or(std::cmp::Ordering::Equal))
        });
        points.truncate(cfg.filter_cap.max(1));
    }
    true
}

#[derive(Clone, Debug)]
pub struct FilterState {
    pub points: Vec<FilterPoint>,
    /// Best feasible incumbent (v == 0) objective value.
    pub incumbent_feasible_f: Option<f64>,
}

#[allow(clippy::derivable_impls)]
impl Default for FilterState {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            incumbent_feasible_f: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AcceptanceState {
    pub barrier: BarrierState,
    pub filter: FilterState,
}

impl AcceptanceState {
    pub fn new(cfg: &AcceptanceConfig) -> Self {
        Self {
            barrier: BarrierState::new(cfg),
            filter: FilterState::default(),
        }
    }
}

/// Accept/Reject decision for a TRUTH result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TruthDecision {
    Reject,
    Accept,
}

/// Acceptance policy trait. Customizable extension point.
///
/// # Contract
/// Implementors **must** maintain the progressive barrier invariant:
/// - `barrier_h()` must be non-increasing across successful iterations.
/// - Violating this invalidates convergence guarantees.
pub trait AcceptancePolicy: Send + Sync {
    /// Decide whether to accept a TRUTH result.
    ///
    /// `f` is the primary objective value, `v` is the constraint violation.
    fn decide_truth(&mut self, x: &XMesh, f: f64, v: f64) -> TruthDecision;

    /// Called once per engine iteration at a deterministic boundary.
    fn on_iteration_end(&mut self, poll_attempted: bool, iter_improved: bool);
}

/// Default acceptance: single-objective progressive barrier + filter.
pub struct DefaultAcceptance {
    cfg: AcceptanceConfig,
    pub state: AcceptanceState,
}

impl Default for DefaultAcceptance {
    fn default() -> Self {
        let cfg = AcceptanceConfig::default();
        Self::new(cfg)
    }
}

impl DefaultAcceptance {
    pub fn new(cfg: AcceptanceConfig) -> Self {
        let state = AcceptanceState::new(&cfg);
        Self { cfg, state }
    }
}

impl AcceptancePolicy for DefaultAcceptance {
    fn decide_truth(&mut self, _x: &XMesh, f: f64, v: f64) -> TruthDecision {
        // Barrier gate.
        if v > self.state.barrier.h {
            return TruthDecision::Reject;
        }

        // Update feasible incumbent.
        if v <= 0.0 {
            match self.state.filter.incumbent_feasible_f {
                None => self.state.filter.incumbent_feasible_f = Some(f),
                Some(best) => {
                    if f < best {
                        self.state.filter.incumbent_feasible_f = Some(f)
                    }
                }
            }
        }

        // Filter update.
        let accepted = filter_insert(
            &mut self.state.filter.points,
            FilterPoint { f, v },
            &self.cfg,
        );
        if accepted {
            TruthDecision::Accept
        } else {
            TruthDecision::Reject
        }
    }

    fn on_iteration_end(&mut self, poll_attempted: bool, iter_improved: bool) {
        if poll_attempted && !iter_improved {
            self.state.barrier.tighten_on_poll_fail(&self.cfg);
        }
    }
}
