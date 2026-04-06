//! Opinionated engine presets for quick experimentation and benchmarking.
//!
//! The presets are intentionally small in number and cover distinct operating modes:
//! - `LegacyBaseline`: approximates the pre-upgrade5 behavior for before/after comparisons.
//! - `Balanced`: recommended default for general use.
//! - `Conservative`: lower false-infeasible risk, more audit/estimation caution.
//! - `Throughput`: favors more candidate churn and faster adaptation.

use crate::core::engine::EngineConfig;
use crate::types::{Smc, Tau};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Preset {
    LegacyBaseline,
    Balanced,
    Conservative,
    Throughput,
}

impl Preset {
    pub const ALL: [Preset; 4] = [
        Preset::LegacyBaseline,
        Preset::Balanced,
        Preset::Conservative,
        Preset::Throughput,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Preset::LegacyBaseline => "legacy_baseline",
            Preset::Balanced => "balanced",
            Preset::Conservative => "conservative",
            Preset::Throughput => "throughput",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Preset::LegacyBaseline => {
                "Approximate pre-upgrade5 behavior; useful as a before-comparison baseline."
            }
            Preset::Balanced => {
                "Recommended default: near-throughput solution quality with moderate partial budget and moderately aggressive objective pruning."
            }
            Preset::Conservative => {
                "Minimize false infeasible risk; slower adaptation, more cautious K updates, and delayed objective pruning."
            }
            Preset::Throughput => {
                "Favor candidate throughput and faster adaptation for quick sweeps, including earlier objective pruning."
            }
        }
    }

    pub fn config(self) -> EngineConfig {
        match self {
            Preset::LegacyBaseline => legacy_baseline(),
            Preset::Balanced => balanced(),
            Preset::Conservative => conservative(),
            Preset::Throughput => throughput(),
        }
    }
}

fn base() -> EngineConfig {
    EngineConfig::new(
        vec![], // let ladder resolve defaults for most presets
        vec![],
        1.0,     // mesh_base_step
        4,       // mesh_mul_init
        1,       // mesh_mul_min
        2,       // mesh_refine_div
        1,       // poll_step_mult
        6,       // max_iters
        8,       // candidates_per_iter
        None,    // search_dim (resolved from evaluator)
        Some(8), // max_steps_per_iter
        2,       // num_constraints
        0.0,     // accept_h0
        0.0,     // accept_h_min
        0.5,     // accept_h_shrink
        1e-12,   // accept_eps_f
        1e-12,   // accept_eps_v
        64,      // accept_filter_cap
        0.01,    // calibrator_target_false
        20,      // calibrator_min_audits
        0.1,     // calibrator_eta_delta
        0.0,     // calibrator_delta_min
        0.05,    // calibrator_delta_max
        4096,    // calibrator_k_window
        25,      // calibrator_k_min_pairs
        0.90,    // calibrator_k_quantile
        0.2,     // calibrator_k_eta
        2,       // objective_prune_min_smc_rank
        2,       // objective_prune_min_level
        false,   // objective_prune_require_back_half
        true,    // objective_prune_disable_for_audit
        Some(4), // batch_boundary
        32,      // executor_chunk_base
        1,       // executor_chunk_min
        256,     // executor_chunk_max
        2_000,   // executor_spin_limit
        true,    // executor_chunk_auto_tune
    )
    .expect("preset config must be valid")
}

pub fn legacy_baseline() -> EngineConfig {
    let mut cfg = base();
    cfg.tau_levels = vec![Tau(100), Tau(10), Tau(1)];
    cfg.smc_levels = vec![Smc(16), Smc(64)];
    cfg.calibrator_min_audits = 64;
    cfg.calibrator_k_window = 64;
    cfg.calibrator_k_min_pairs = 64; // effectively disables K on small runs
    cfg.calibrator_k_eta = 0.05;
    cfg.batch_boundary = Some(4);
    cfg.executor_chunk_auto_tune = false;
    cfg.max_steps_per_iter = Some(6);
    cfg
}

pub fn balanced() -> EngineConfig {
    let mut cfg = base();

    // Search / scheduling
    cfg.candidates_per_iter = 12; // balanced 8 → 12
    cfg.max_steps_per_iter = Some(12); // balanced 8 → 12
    cfg.batch_boundary = Some(6); // balanced 4 → 6

    // Delta calibrator: balanced보다 약간 더 빠르게 적응
    cfg.calibrator_min_audits = 15; // balanced 20 → 15
    cfg.calibrator_eta_delta = 0.12; // balanced 0.10 → 0.12

    // K calibrator: throughput보다 덜 공격적이지만 balanced보다 빠름
    cfg.calibrator_k_min_pairs = 20; // balanced 25 → 20
    cfg.calibrator_k_quantile = 0.85; // balanced 0.90 → 0.85
    cfg.calibrator_k_eta = 0.28; // balanced 0.20 → 0.28

    // Objective pruning: moderate default gate.
    cfg.objective_prune_min_smc_rank = 2;
    cfg.objective_prune_min_level = 2;
    cfg.objective_prune_require_back_half = false;
    cfg.objective_prune_disable_for_audit = true;

    // Worker-pool tuning
    cfg.executor_chunk_base = 48; // balanced 32 → 48
    cfg.executor_chunk_auto_tune = true;

    cfg
}

pub fn conservative() -> EngineConfig {
    let mut cfg = base();
    cfg.candidates_per_iter = 6;
    cfg.max_steps_per_iter = Some(4);
    cfg.calibrator_target_false = 0.005;
    cfg.calibrator_min_audits = 40;
    cfg.calibrator_eta_delta = 0.05;
    cfg.calibrator_delta_max = 0.08;
    cfg.calibrator_k_min_pairs = 64;
    cfg.calibrator_k_quantile = 0.95;
    cfg.calibrator_k_eta = 0.10;
    cfg.objective_prune_min_smc_rank = 3;
    cfg.objective_prune_min_level = 3;
    cfg.objective_prune_require_back_half = true;
    cfg.objective_prune_disable_for_audit = true;
    cfg.batch_boundary = Some(4);
    cfg.executor_chunk_auto_tune = false;
    cfg
}

pub fn throughput() -> EngineConfig {
    let mut cfg = base();
    cfg.max_iters = 5;
    cfg.candidates_per_iter = 16;
    cfg.max_steps_per_iter = Some(16);
    cfg.batch_boundary = Some(8);
    cfg.calibrator_min_audits = 10;
    cfg.calibrator_eta_delta = 0.15;
    cfg.calibrator_k_min_pairs = 16;
    cfg.calibrator_k_quantile = 0.80;
    cfg.calibrator_k_eta = 0.35;
    cfg.executor_chunk_base = 64;
    cfg.executor_chunk_auto_tune = true;
    cfg.objective_prune_min_smc_rank = 1;
    cfg.objective_prune_min_level = 2;
    cfg.objective_prune_require_back_half = false;
    cfg.objective_prune_disable_for_audit = true;
    cfg
}
