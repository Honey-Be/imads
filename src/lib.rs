//! Integrated MADS policy surface (skeleton).
//!
//! This crate intentionally focuses on **policy boundaries** and **deterministic execution contracts**.
//! The optimization logic is a minimal scaffold to make the policy interfaces compile and testable.

pub mod types;

pub mod backends;
pub mod core;
pub mod policies;
pub mod presets;

#[cfg(test)]
mod tests;
