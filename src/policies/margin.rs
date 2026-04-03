use crate::policies::calibrator::CalibState;
use crate::types::Estimates;

#[derive(Clone, Debug)]
pub struct Thresholds {
    pub delta_rel: Vec<f64>,
    pub k_c: Vec<f64>,
    pub k_f: f64,
    pub eps_f: f64,
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
    // Minimal, deterministic approximation.
    // 0.05 one-sided ≈ 1.645, 0.10 one-sided ≈ 1.282.
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
            eps_f: cal.eps_f,
        }
    }

    fn early_infeasible(&self, est: &Estimates, th: &Thresholds) -> Option<usize> {
        let zc = z_for(self.alpha_c());
        for (j, (&c_hat, &c_se)) in est.c_hat.iter().zip(est.c_se.iter()).enumerate() {
            let delta = th.delta_rel.get(j).copied().unwrap_or(0.0);
            let bias = th.k_c.get(j).copied().unwrap_or(0.0) * est.tau_scale;
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
        let lcb = est.f_hat - z_l * est.f_se - th.k_f * est.tau_scale;
        let ucb = est.f_hat + z_u * est.f_se + th.k_f * est.tau_scale;
        (lcb, ucb)
    }
}
