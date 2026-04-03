use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::{DecisionCacheKey, Estimates, EvalCacheKey, JobResult};

/// Evaluation cache backend (stores expensive `Estimates`). Safe to customize.
///
/// # Contract
/// - Cache miss must not change correctness.
/// - Keys must include (x, phi, env_rev).
pub trait EvalCacheBackend: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &EvalCacheKey) -> Option<Estimates>;
    fn put(&self, key: EvalCacheKey, value: Estimates);
}

/// Decision cache backend (stores *policy-dependent* decisions/results). Safe to customize.
///
/// # Contract
/// - Cache miss must not change correctness.
/// - Keys must include (x, phi, env_rev, policy_rev, tag).
pub trait DecisionCacheBackend: std::fmt::Debug + Send + Sync {
    fn get(&self, key: &DecisionCacheKey) -> Option<JobResult>;
    fn put(&self, key: DecisionCacheKey, value: JobResult);
}

#[derive(Debug, Clone, Default)]
pub struct MemoryEvalCache {
    inner: Arc<Mutex<HashMap<EvalCacheKey, Estimates>>>,
}

impl EvalCacheBackend for MemoryEvalCache {
    fn get(&self, key: &EvalCacheKey) -> Option<Estimates> {
        self.inner.lock().ok().and_then(|m| m.get(key).cloned())
    }

    fn put(&self, key: EvalCacheKey, value: Estimates) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(key, value);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryDecisionCache {
    inner: Arc<Mutex<HashMap<DecisionCacheKey, JobResult>>>,
}

impl DecisionCacheBackend for MemoryDecisionCache {
    fn get(&self, key: &DecisionCacheKey) -> Option<JobResult> {
        self.inner.lock().ok().and_then(|m| m.get(key).cloned())
    }

    fn put(&self, key: DecisionCacheKey, value: JobResult) {
        if let Ok(mut m) = self.inner.lock() {
            m.insert(key, value);
        }
    }
}
