//! # IMADS — Integrated Mesh Adaptive Direct Search
//!
//! A deterministic, multi-fidelity optimization framework built around pluggable policies.
//!
//! ## Quick Start
//!
//! ```rust
//! use imads_core::core::{DefaultBundle, Engine};
//! use imads_core::presets::Preset;
//! use imads_core::types::Env;
//!
//! let cfg = Preset::Balanced.config();
//! let env = Env { run_id: 1, config_hash: 2, data_snapshot_id: 3, rng_master_seed: 4 };
//! let mut engine = Engine::<DefaultBundle>::default();
//! let output = engine.run(&cfg, &env, 4); // 4 workers → auto multi-thread
//! println!("f_best = {:?}", output.f_best);
//! ```
//!
//! ## Architecture
//!
//! ```text
//! Engine<PolicyBundle>
//!   ├── SchedulerPolicy   — batch scheduling
//!   ├── SearchPolicy       — candidate generation
//!   ├── LadderPolicy       — (τ, S) fidelity ladder
//!   ├── DidsPolicy         — dynamic infeasibility degree
//!   ├── MarginPolicy       — threshold decisions
//!   ├── CalibratorPolicy   — delta/K controller
//!   ├── AuditPolicy        — verification selection
//!   ├── EvalCacheBackend   ��� estimates cache
//!   ├── DecisionCacheBackend — decision cache
//!   └── AdaptiveExecutor   — inline (1 worker) or pool (N workers)
//! ```
//!
//! ## Design Principles
//!
//! - **Deterministic execution**: all policies are pure functions of their inputs.
//! - **Sealed core**: poll/mesh updates and acceptance logic cannot be overridden.
//! - **Pluggable policies**: every policy trait can be customized independently.
//! - **Three-stage flow**: Stage A (cheap) → PARTIAL (multi-fidelity) → TRUTH (final).

pub mod types;

pub mod backends;
pub mod core;
pub mod policies;
pub mod presets;

#[cfg(test)]
mod tests;
