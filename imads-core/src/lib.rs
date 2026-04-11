//! # IMADS — Integrated Mesh Adaptive Direct Search
//!
//! A deterministic, multi-fidelity optimization framework built around pluggable policies.
//! Supports single-objective and multi-objective optimization via the [`ObjectiveValues`]
//! trait.
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
//!   ├── SchedulerPolicy     — batch scheduling
//!   ├── SearchPolicy         — candidate generation (DefaultSearch or StratifiedSearch)
//!   ├── LadderPolicy         — (τ, S) fidelity ladder
//!   ├── DidsPolicy           — dynamic infeasibility degree
//!   ├── MarginPolicy         — threshold decisions
//!   ├── CalibratorPolicy     — delta/K controller
//!   ├── AuditPolicy          — verification selection
//!   ├── AcceptancePolicy     — filter + progressive barrier (customizable)
//!   ├── EvalCacheBackend     — estimates cache
//!   ├── DecisionCacheBackend — decision cache
//!   └── AdaptiveExecutor     — inline (1 worker) or pool (N workers)
//! ```
//!
//! ## Key Features
//!
//! - **Multi-objective support**: the [`Evaluator`] trait has an associated type
//!   `Objectives: ObjectiveValues` (impl for `f64`, `[f64; N]`, `Vec<f64>`).
//!   [`EvaluatorErased`] provides type-erased access for the engine internals.
//!   `SingleObjectiveEvaluator` is a marker sub-trait with a blanket impl for
//!   `Objectives = f64`.
//! - **StratifiedSearch**: a drop-in replacement for `DefaultSearch` that combines
//!   coordinate stepping, directional search, and Halton quasi-random exploration
//!   with dimensionality-adaptive ratios.
//! - **AnisotropicMeshGeometry**: per-dimension mesh step sizes via
//!   `EngineConfig::mesh_base_steps`. Replaces the scalar `mesh_step` with a
//!   per-dimension `mesh_steps: Vec<f64>`.
//! - **AcceptancePolicy**: now a public trait (was sealed). Users can implement custom
//!   acceptance logic (e.g., Pareto-based for multi-objective).
//!
//! ## Design Principles
//!
//! - **Deterministic execution**: all policies are pure functions of their inputs.
//! - **Sealed core**: poll/mesh updates cannot be overridden.
//! - **Pluggable policies**: every policy trait can be customized independently.
//! - **Three-stage flow**: Stage A (cheap) → PARTIAL (multi-fidelity) → TRUTH (final).

pub mod types;

pub mod backends;
pub mod core;
pub mod policies;
pub mod presets;

#[cfg(test)]
mod tests;
