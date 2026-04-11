use crate::policies::calibrator::{CalibState, KByPhiState};
use crate::types::{Estimates, Phi};

#[derive(Clone, Debug)]
pub struct Thresholds {
    pub delta_rel: Vec<f64>,
    pub k_c: Vec<f64>,
    pub k_f: f64,
    pub k_by_phi: Vec<KByPhiState>,
    pub eps_f: f64,
}

impl Thresholds {
    pub fn k_f_for(&self, phi: Phi) -> f64 {
        self.k_by_phi
            .iter()
            .find(|st| st.phi == phi)
            .map(|st| st.k_f)
            .unwrap_or(self.k_f)
    }

    pub fn k_c_for(&self, phi: Phi, j: usize) -> f64 {
        self.k_by_phi
            .iter()
            .find(|st| st.phi == phi)
            .and_then(|st| st.k_c.get(j).copied())
            .unwrap_or_else(|| self.k_c.get(j).copied().unwrap_or(0.0))
    }
}

/// Margin policy. Safe to customize.
///
/// # Contract
/// - Deterministic given inputs.
/// - Must not declare feasibility from PARTIAL; only provide bounds for priority.
pub trait MarginPolicy: Send + Sync {
    fn alpha_c(&self) -> f64;
    fn alpha_f(&self) -> f64;
    fn beta_f(&self) -> f64;

    fn thresholds(&self, cal: &CalibState) -> Thresholds;

    fn early_infeasible(&self, est: &Estimates, th: &Thresholds) -> Option<usize>;

    fn objective_bounds(&self, est: &Estimates, th: &Thresholds) -> (f64, f64);
}

#[derive(Default)]
pub struct DefaultMargin;

fn z_for(alpha: f64) -> f64 {
    if (alpha - 0.05).abs() < 1e-12 {
        1.645
    } else if (alpha - 0.10).abs() < 1e-12 {
        1.282
    } else {
        1.96
    }
}

impl MarginPolicy for DefaultMargin {
    fn alpha_c(&self) -> f64 {
        0.05
    }

    fn alpha_f(&self) -> f64 {
        0.10
    }

    fn beta_f(&self) -> f64 {
        0.05
    }

    fn thresholds(&self, cal: &CalibState) -> Thresholds {
        Thresholds {
            delta_rel: cal.delta_rel.clone(),
            k_c: cal.k_c.clone(),
            k_f: cal.k_f,
            k_by_phi: cal.k_by_phi.clone(),
            eps_f: cal.eps_f,
        }
    }

    fn early_infeasible(&self, est: &Estimates, th: &Thresholds) -> Option<usize> {
        let zc = z_for(self.alpha_c());
        for (j, (&c_hat, &c_se)) in est.c_hat.iter().zip(est.c_se.iter()).enumerate() {
            let delta = th.delta_rel.get(j).copied().unwrap_or(0.0);
            let bias = th.k_c_for(est.phi, j) * est.tau_scale;
            let lhs = c_hat - zc * c_se - bias;
            if lhs > delta {
                return Some(j);
            }
        }
        None
    }

    fn objective_bounds(&self, est: &Estimates, th: &Thresholds) -> (f64, f64) {
        let z_l = z_for(self.alpha_f());
        let z_u = z_for(self.beta_f());
        let k_f = th.k_f_for(est.phi);
        let f_hat = est.f_hat_primary();
        let f_se = est.f_se_primary();
        let lcb = f_hat - z_l * f_se - k_f * est.tau_scale;
        let ucb = f_hat + z_u * f_se + k_f * est.tau_scale;
        (lcb, ucb)
    }
}
