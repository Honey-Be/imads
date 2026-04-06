use crate::types::{Phi, Smc, Tau};

/// Ladder policy. Safe to customize.
///
/// # Contract
/// - Prefer monotone-increasing cost.
/// - Prefer monotone refinement (tau decreases, smc increases).
/// - For MC prefix reuse, do not decrease `Smc` along the ladder.
pub trait LadderPolicy: Send + Sync {
    fn build_ladder(&self, tau_levels: &[Tau], smc_levels: &[Smc]) -> Vec<Phi>;

    fn estimate_cost(&self, _phi: Phi) -> f64 {
        0.0
    }
}

/// Default: staircase ladder.
/// Example: (tau1,S1)->(tau1,S2)->(tau2,S2)->...
#[derive(Default)]
pub struct StaircaseLadder;

impl LadderPolicy for StaircaseLadder {
    fn build_ladder(&self, tau_levels: &[Tau], smc_levels: &[Smc]) -> Vec<Phi> {
        assert!(!tau_levels.is_empty());
        assert!(!smc_levels.is_empty());

        let mut tau = tau_levels.to_vec();
        let mut smc = smc_levels.to_vec();
        // Expect tau sorted from loose->tight: larger numeric value means looser tolerance.
        tau.sort();
        tau.reverse();
        smc.sort();

        let mut ladder = Vec::new();
        let mut i_tau = 0usize;
        let mut i_s = 0usize;
        ladder.push(Phi {
            tau: tau[i_tau],
            smc: smc[i_s],
        });

        // Alternate: increase Smc then tighten tau, repeating.
        while i_tau + 1 < tau.len() || i_s + 1 < smc.len() {
            if i_s + 1 < smc.len() {
                i_s += 1;
                ladder.push(Phi {
                    tau: tau[i_tau],
                    smc: smc[i_s],
                });
            }
            if i_tau + 1 < tau.len() {
                i_tau += 1;
                ladder.push(Phi {
                    tau: tau[i_tau],
                    smc: smc[i_s],
                });
            }
        }
        ladder
    }
}
