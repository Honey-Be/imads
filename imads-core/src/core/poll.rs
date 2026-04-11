//! Sealed Poll / Mesh update logic.
//!
//! This is intentionally **not** exposed as a customizable policy surface
//! in the default build, because it is where MADS' convergence conditions
//! live (positive spanning directions, mesh/poll parameter updates, etc.).
//!
//! You may expose experimental hooks behind `--features unstable-poll-policy`.

#[allow(dead_code)]
pub(crate) mod sealed {
    pub trait Sealed {}
}

/// Placeholder type for a Poll generator.
#[derive(Clone, Debug, Default)]
pub struct DefaultPoll;

impl sealed::Sealed for DefaultPoll {}

impl DefaultPoll {
    /// Generate axis-aligned poll points around `center` with integer step in base-lattice units.
    ///
    /// This is a conservative, deterministic positive-spanning set: {±e_i}.
    pub fn generate_points(center: &crate::types::XMesh, step: i64) -> Vec<crate::types::XMesh> {
        let n = center.0.len();
        let s = step.max(1);
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let mut xp = center.0.clone();
            xp[i] = xp[i].saturating_add(s);
            out.push(crate::types::XMesh(xp));

            let mut xm = center.0.clone();
            xm[i] = xm[i].saturating_sub(s);
            out.push(crate::types::XMesh(xm));
        }
        out
    }

    /// Generate axis-aligned poll points with per-dimension steps.
    pub fn generate_points_aniso(
        center: &crate::types::XMesh,
        steps: &[i64],
    ) -> Vec<crate::types::XMesh> {
        let n = center.0.len();
        let mut out = Vec::with_capacity(n * 2);
        for i in 0..n {
            let s = steps.get(i).copied().unwrap_or(1).max(1);
            let mut xp = center.0.clone();
            xp[i] = xp[i].saturating_add(s);
            out.push(crate::types::XMesh(xp));

            let mut xm = center.0.clone();
            xm[i] = xm[i].saturating_sub(s);
            out.push(crate::types::XMesh(xm));
        }
        out
    }
}
