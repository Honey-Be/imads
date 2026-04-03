use crate::types::{Env, EnvRev, Phi, XMesh, env_rev, stable_hash_u64};

/// Audit policy. Safe to customize.
///
/// # Contract
/// - Must be deterministic (hash-based).
pub trait AuditPolicy: Send + Sync {
    fn should_audit(&self, x: &XMesh, phi: Phi, env_rev: EnvRev) -> bool;

    /// Optional boundary oversampling (score near 0 => near boundary).
    fn boundary_boost(&self, _score: f64) -> f64 {
        1.0
    }
}

#[derive(Clone, Debug)]
pub struct DefaultAudit {
    /// Base audit probability in [0,1].
    pub p_audit: f64,
    /// Deterministic modulus.
    pub modulus: u64,
}

impl Default for DefaultAudit {
    fn default() -> Self {
        Self {
            p_audit: 0.02,
            modulus: 10_000,
        }
    }
}

impl DefaultAudit {
    pub fn env_rev(env: &Env) -> EnvRev {
        env_rev(env)
    }
}

impl AuditPolicy for DefaultAudit {
    fn should_audit(&self, x: &XMesh, phi: Phi, env_rev: EnvRev) -> bool {
        let key = (stable_hash_u64(x), phi, env_rev);
        let h = stable_hash_u64(&key);
        let thresh = (self.p_audit.clamp(0.0, 1.0) * self.modulus as f64) as u64;
        (h % self.modulus) < thresh
    }
}
