use crate::backends::cache::{DecisionCacheBackend, EvalCacheBackend};
use crate::core::evaluator::Evaluator;
use crate::types::{
    CacheTag, DecisionCacheKey, Env, EnvRev, Estimates, EvalCacheKey, EvalMeta, JobResult, Phi,
    WorkItem, XReal, mesh_to_real,
};
use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::mem::MaybeUninit;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Executor configuration parameters.
///
/// These are *performance* knobs and must not affect correctness.
#[derive(Clone, Copy, Debug)]
pub struct ExecutorParams {
    /// Upper bound for how many tasks a worker pulls from the global queue per refill.
    ///
    /// Effective chunk size is additionally capped by `ceil(batch_size / workers)` to avoid
    /// one worker hoarding the entire batch (no work-stealing).
    pub chunk_base: usize,

    /// Spin iterations before falling back to the condvar barrier wait.
    ///
    /// This is a performance knob for very short batches.
    pub spin_limit: usize,
}

impl Default for ExecutorParams {
    fn default() -> Self {
        Self {
            chunk_base: 32,
            spin_limit: 2_000,
        }
    }
}

/// Execution context passed to executors.
///
/// This is intentionally **owned** (via `Arc`) so it can be transferred safely to worker threads
/// without raw pointers.
///
/// # Contract
/// - Executors must treat this as read-only. All *policy* decisions happen on the engine thread.
/// - Implementations placed behind trait objects must be `Send + Sync`.
#[derive(Debug, Clone)]
pub struct ExecCtx<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> {
    pub evaluator: Arc<dyn Evaluator>,
    pub env: Arc<Env>,
    pub env_rev: EnvRev,
    pub eval_cache: Arc<E>,
    pub decision_cache: Arc<D>,
    pub ladder_len: usize,
    pub base_step: f64,
}

/// Output of executing one `WorkItem`.
///
/// This intentionally contains *only* evaluation artifacts. Policy-dependent materialization
/// (early infeasible / stop / truth) is handled by the engine in deterministic order.
#[derive(Clone, Debug)]
pub struct WorkOutcome {
    pub item: WorkItem,
    pub cached_decision: Option<JobResult>,
    pub estimates: Option<Estimates>,
    pub runtime_cost: f64,

    // Stats signals (so we can update EngineStats on the engine thread deterministically).
    pub hit_decision_cache: bool,
    pub hit_eval_cache: bool,
    pub did_compute: bool,
}

/// Executor for running batches of `WorkItem`s.
///
/// # Contract
/// - Must not mutate policy objects.
/// - May evaluate items concurrently.
/// - Must return a `Vec<WorkOutcome>` with one outcome per input item.
/// - Ordering is irrelevant; the engine will sort by `cand_id` before materializing.
pub trait Executor<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone>:
    Send + Sync
{
    fn run_batch(&self, items: Vec<WorkItem>, ctx: Arc<ExecCtx<E, D>>) -> Vec<WorkOutcome>;

    /// Configure the executor's concurrency to match the engine's `workers` parameter.
    ///
    /// # Contract
    /// - Called by the engine **before** it starts dispatching batches.
    /// - Must not depend on wall-clock time or other hidden nondeterminism.
    /// - Implementations may recreate internal worker pools.
    fn configure(&mut self, _workers: usize) {}

    /// Configure performance parameters.
    ///
    /// # Contract
    /// - Must not affect correctness.
    /// - Called by the engine before dispatch.
    fn configure_params(&mut self, _params: ExecutorParams) {}

    /// Owned/builder-style convenience.
    ///
    /// This keeps the ergonomics of `fn configure(mut self, workers) -> Self` without
    /// requiring the engine to move out of `self.executor`.
    fn configured(mut self, workers: usize) -> Self
    where
        Self: Sized,
    {
        self.configure(workers);
        self
    }

    /// Owned/builder-style convenience for params.
    fn configured_params(mut self, params: ExecutorParams) -> Self
    where
        Self: Sized,
    {
        self.configure_params(params);
        self
    }
}

/// Default executor: runs items sequentially on the calling thread.
#[derive(Clone, Debug, Default)]
pub struct InlineExecutor;

impl<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> Executor<E, D>
    for InlineExecutor
{
    fn run_batch(&self, items: Vec<WorkItem>, ctx: Arc<ExecCtx<E, D>>) -> Vec<WorkOutcome> {
        items
            .into_iter()
            .map(|wi| execute_one(&wi, ctx.as_ref()))
            .collect()
    }

    fn configure(&mut self, _workers: usize) {}

    fn configure_params(&mut self, _params: ExecutorParams) {}
}

/// Fixed-size worker pool executor with a global queue + worker-local batch rings.
///
/// - Persistent worker threads: fixed `W` (no work-stealing).
/// - Global queue: `Mutex<VecDeque<Task>> + Condvar`.
/// - Worker-local ring: `VecDeque<Task>` filled in **chunks** to reduce lock contention.
/// - Batch barrier semantics: `run_batch` blocks until all items complete.
///
/// # Result collection
/// We avoid per-task channels by using a batch-local preallocated slot array:
/// `Vec<MaybeUninit<WorkOutcome>> + AtomicUsize` (done counter) + Condvar barrier.
/// This reduces allocations and synchronization overhead.
///
/// # Safety
/// Result slots use `UnsafeCell<MaybeUninit<WorkOutcome>>` to avoid per-item allocation.
/// Each slot is written by exactly one task, and the engine thread reads them only after
/// the batch barrier completes.
#[derive(Debug)]
pub struct WorkerPoolExecutor<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> {
    inner: Arc<Inner<E, D>>,
    workers: usize,
    params: ExecutorParams,
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static>
    WorkerPoolExecutor<E, D>
{
    pub fn new(workers: usize) -> Self {
        let w = workers.max(1);
        let inner = Arc::new(Inner {
            queue: Mutex::new(VecDeque::new()),
            cv: Condvar::new(),
            shutdown: AtomicBool::new(false),
            handles: Mutex::new(Vec::with_capacity(w)),
            current_chunk: AtomicUsize::new(1),
        });

        // Spawn persistent workers.
        {
            let mut handles = inner.handles.lock().unwrap();
            for _ in 0..w {
                let inner_cloned = inner.clone();
                handles.push(std::thread::spawn(move || worker_loop(inner_cloned)));
            }
        }

        Self {
            inner,
            workers: w,
            params: ExecutorParams::default(),
        }
    }

    pub fn workers(&self) -> usize {
        self.workers
    }
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static> Default
    for WorkerPoolExecutor<E, D>
{
    fn default() -> Self {
        Self::new(1)
    }
}

impl<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> Clone
    for WorkerPoolExecutor<E, D>
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            workers: self.workers,
            params: self.params,
        }
    }
}

impl<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> Drop
    for WorkerPoolExecutor<E, D>
{
    fn drop(&mut self) {
        // Only shut down and join threads on last drop.
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        self.inner.shutdown.store(true, Ordering::SeqCst);
        self.inner.cv.notify_all();

        let mut handles = self.inner.handles.lock().unwrap();
        for h in handles.drain(..) {
            let _ = h.join();
        }
    }
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static>
    Executor<E, D> for WorkerPoolExecutor<E, D>
{
    fn run_batch(&self, items: Vec<WorkItem>, ctx: Arc<ExecCtx<E, D>>) -> Vec<WorkOutcome> {
        let n = items.len();
        if n == 0 {
            return Vec::new();
        }

        // Tie effective chunk size to batch size to avoid hoarding when no work-stealing exists.
        let w = self.workers.max(1);
        let per_worker_share = n.div_ceil(w);
        let base = self.params.chunk_base.max(1);
        let chunk_eff = base.min(per_worker_share.max(1));
        self.inner.current_chunk.store(chunk_eff, Ordering::Relaxed);

        // Batch-local result sink.
        let sink = Arc::new(BatchSink::new(n));

        {
            let mut q = self.inner.queue.lock().unwrap();
            for (i, wi) in items.into_iter().enumerate() {
                q.push_back(Task {
                    wi,
                    ctx: ctx.clone(),
                    batch_idx: i,
                    sink: sink.clone(),
                });
            }
        }

        // Wake workers.
        self.inner.cv.notify_all();

        // Barrier wait.
        //
        // First do a short spin phase to avoid the mutex/condvar path for very short batches.
        let mut spins = 0usize;
        while spins < self.params.spin_limit && sink.done.load(Ordering::Acquire) < n {
            std::hint::spin_loop();
            spins += 1;
        }
        if sink.done.load(Ordering::Acquire) < n {
            let mut guard = sink.mu.lock().unwrap();
            while sink.done.load(Ordering::Acquire) < n {
                guard = sink.cv.wait(guard).unwrap();
            }
            drop(guard);
        }

        // Collect outcomes in submission order (engine will sort later anyway).
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            // SAFETY: barrier ensures every slot is initialized.
            unsafe {
                let mi = &*sink.slots[i].0.get();
                out.push(mi.assume_init_read());
            }
        }
        out
    }

    fn configure(&mut self, workers: usize) {
        let w = workers.max(1);
        if w == self.workers {
            return;
        }
        *self = WorkerPoolExecutor::new(w);
    }

    fn configure_params(&mut self, params: ExecutorParams) {
        // Only affects performance; safe to update live.
        self.params = params;
    }
}

#[derive(Debug)]
struct Inner<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> {
    queue: Mutex<VecDeque<Task<E, D>>>,
    cv: Condvar,
    shutdown: AtomicBool,
    handles: Mutex<Vec<std::thread::JoinHandle<()>>>,
    current_chunk: AtomicUsize,
}

#[derive(Debug, Clone)]
struct Task<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone> {
    wi: WorkItem,
    ctx: Arc<ExecCtx<E, D>>,
    batch_idx: usize,
    sink: Arc<BatchSink>,
}

#[derive(Debug)]
struct Slot(UnsafeCell<MaybeUninit<WorkOutcome>>);
unsafe impl Sync for Slot {}
unsafe impl Send for Slot {}

#[derive(Debug)]
struct BatchSink {
    total: usize,
    done: AtomicUsize,
    slots: Box<[Slot]>,
    mu: Mutex<()>,
    cv: Condvar,
}

impl BatchSink {
    fn new(total: usize) -> Self {
        let mut v = Vec::with_capacity(total);
        for _ in 0..total {
            v.push(Slot(UnsafeCell::new(MaybeUninit::uninit())));
        }
        Self {
            total,
            done: AtomicUsize::new(0),
            slots: v.into_boxed_slice(),
            mu: Mutex::new(()),
            cv: Condvar::new(),
        }
    }
}

fn worker_loop<'a, E: EvalCacheBackend + Clone + 'a, D: DecisionCacheBackend + Clone + 'a>(
    inner: Arc<Inner<E, D>>,
) {
    // Worker-local ring buffer. No work-stealing.
    let mut local: VecDeque<Task<E, D>> = VecDeque::new();

    loop {
        let task = if let Some(t) = local.pop_front() {
            Some(t)
        } else {
            let mut q = inner.queue.lock().unwrap();
            loop {
                let chunk = inner.current_chunk.load(Ordering::Relaxed).max(1);

                // Drain up to `chunk` tasks into local.
                while local.len() < chunk {
                    if let Some(t) = q.pop_front() {
                        local.push_back(t);
                    } else {
                        break;
                    }
                }

                if let Some(t) = local.pop_front() {
                    break Some(t);
                }

                if inner.shutdown.load(Ordering::SeqCst) {
                    break None;
                }

                q = inner.cv.wait(q).unwrap();
            }
        };

        let Some(task) = task else {
            return;
        };

        // Execute inside a panic boundary so one panicking evaluation doesn't kill the worker.
        // We explicitly assert unwind-safety: policies are applied on the engine thread, and
        // evaluators/caches are expected to uphold their own invariants.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            execute_one(&task.wi, &task.ctx)
        }))
        .unwrap_or_else(|_| WorkOutcome {
            item: task.wi.clone(),
            cached_decision: None,
            estimates: None,
            runtime_cost: 0.0,
            hit_decision_cache: false,
            hit_eval_cache: false,
            did_compute: false,
        });

        // Write outcome into the batch-local slot.
        unsafe {
            let slot = &mut *task.sink.slots[task.batch_idx].0.get();
            slot.write(outcome);
        }

        // Release: ensure slot write is visible before counting completion.
        let done_now = task.sink.done.fetch_add(1, Ordering::Release) + 1;
        if done_now == task.sink.total {
            task.sink.cv.notify_one();
        }
    }
}

fn execute_one<E: EvalCacheBackend + Clone, D: DecisionCacheBackend + Clone>(
    wi: &WorkItem,
    ctx: &ExecCtx<E, D>,
) -> WorkOutcome {
    let is_truth = (wi.phi_idx as usize) + 1 == ctx.ladder_len;
    let tag = if is_truth {
        CacheTag::Truth
    } else {
        CacheTag::Partial
    };

    // 1) Decision cache: if present, we can skip evaluation entirely.
    let dkey = DecisionCacheKey {
        x: wi.x.clone(),
        phi: wi.phi,
        env_rev: wi.env_rev,
        policy_rev: wi.policy_rev,
        tag,
    };
    if let Some(v) = ctx.decision_cache.get(&dkey) {
        return WorkOutcome {
            item: wi.clone(),
            cached_decision: Some(v),
            estimates: None,
            runtime_cost: 0.0,
            hit_decision_cache: true,
            hit_eval_cache: false,
            did_compute: false,
        };
    }

    // 2) Eval cache for expensive estimates.
    let ekey = EvalCacheKey {
        x: wi.x.clone(),
        phi: wi.phi,
        env_rev: wi.env_rev,
    };
    if let Some(est) = ctx.eval_cache.get(&ekey) {
        return WorkOutcome {
            item: wi.clone(),
            cached_decision: None,
            estimates: Some(est),
            runtime_cost: 0.0,
            hit_decision_cache: false,
            hit_eval_cache: true,
            did_compute: false,
        };
    }

    // 3) Compute estimates at this phi.
    let x_real = match mesh_to_real(&wi.x, ctx.base_step) {
        Ok(x_real) => x_real,
        Err(_) => {
            let meta = EvalMeta {
                phi: wi.phi,
                env_rev: wi.env_rev,
                policy_rev: wi.policy_rev,
                runtime_cost: 0.0,
            };
            let jr = JobResult::RejectedInvalidEval { meta };
            ctx.decision_cache.put(dkey, jr.clone());
            return WorkOutcome {
                item: wi.clone(),
                cached_decision: Some(jr),
                estimates: None,
                runtime_cost: 0.0,
                hit_decision_cache: false,
                hit_eval_cache: false,
                did_compute: false,
            };
        }
    };
    let est = match compute_estimates(ctx.evaluator.as_ref(), &x_real, wi.phi, ctx.env.as_ref()) {
        Some(est) => est,
        None => {
            let meta = EvalMeta {
                phi: wi.phi,
                env_rev: wi.env_rev,
                policy_rev: wi.policy_rev,
                runtime_cost: 1.0,
            };
            let jr = JobResult::RejectedInvalidEval { meta };
            ctx.decision_cache.put(dkey, jr.clone());
            return WorkOutcome {
                item: wi.clone(),
                cached_decision: Some(jr),
                estimates: None,
                runtime_cost: 1.0,
                hit_decision_cache: false,
                hit_eval_cache: false,
                did_compute: true,
            };
        }
    };
    ctx.eval_cache.put(ekey, est.clone());

    WorkOutcome {
        item: wi.clone(),
        cached_decision: None,
        estimates: Some(est),
        runtime_cost: 1.0,
        hit_decision_cache: false,
        hit_eval_cache: false,
        did_compute: true,
    }
}

fn compute_estimates(
    evaluator: &dyn Evaluator,
    x: &XReal,
    phi: Phi,
    env: &Env,
) -> Option<Estimates> {
    let s = phi.smc.0.max(1);

    #[derive(Clone, Copy, Debug, Default)]
    struct Welford {
        n: u32,
        mean: f64,
        m2: f64,
    }
    impl Welford {
        fn push(&mut self, x: f64) {
            self.n += 1;
            let delta = x - self.mean;
            self.mean += delta / self.n as f64;
            let delta2 = x - self.mean;
            self.m2 += delta * delta2;
        }
        fn se(&self) -> f64 {
            if self.n < 2 {
                return 0.0;
            }
            let var = self.m2 / ((self.n - 1) as f64);
            (var / (self.n as f64)).sqrt()
        }
    }

    let m = evaluator.num_constraints();
    let mut f_acc = Welford::default();
    let mut c_acc: Vec<Welford> = vec![Welford::default(); m];
    for k in 0..s {
        let (f_s, c_s) = evaluator.mc_sample(x, phi, env, k);
        if !f_s.is_finite() {
            return None;
        }
        f_acc.push(f_s);
        for (j, c_acc_item) in c_acc.iter_mut().enumerate() {
            let cj = *c_s.get(j).unwrap_or(&0.0);
            if !cj.is_finite() {
                return None;
            }
            c_acc_item.push(cj);
        }
    }

    // Deterministic tau-dependent bias.
    let (fb, cb) = evaluator.solver_bias(x, phi.tau, env);
    if !fb.is_finite() || cb.iter().any(|v| !v.is_finite()) {
        return None;
    }

    let f_hat = f_acc.mean + fb;
    let f_se = f_acc.se();
    if !f_hat.is_finite() || !f_se.is_finite() {
        return None;
    }
    let mut c_hat = Vec::with_capacity(m);
    let mut c_se = Vec::with_capacity(m);
    for (j, c_acc_item) in c_acc.iter().enumerate() {
        let b = cb.get(j).copied().unwrap_or(0.0);
        let ch = c_acc_item.mean + b;
        let cs = c_acc_item.se();
        if !ch.is_finite() || !cs.is_finite() {
            return None;
        }
        c_hat.push(ch);
        c_se.push(cs);
    }

    Some(Estimates {
        f_hat,
        f_se,
        c_hat,
        c_se,
        phi,
        tau_scale: phi.tau.0 as f64,
    })
}

// ---------------------------------------------------------------------------
// AdaptiveExecutor — single API for inline / threaded execution
// ---------------------------------------------------------------------------

/// Executor that automatically dispatches to [`InlineExecutor`] when `workers <= 1`
/// and to [`WorkerPoolExecutor`] when `workers > 1` (on targets with thread support).
///
/// On WASM targets without thread support (`wasm32-unknown-unknown`, `wasm32-emscripten`, etc.)
/// only the `Inline` variant is available. On native targets and thread-capable WASM targets
/// (`wasm32-wasip1-threads`, `wasm32-wasip3`), both variants are available and switching
/// happens automatically via [`Executor::configure`].
#[derive(Debug)]
pub enum AdaptiveExecutor<
    E: EvalCacheBackend + Clone + 'static,
    D: DecisionCacheBackend + Clone + 'static,
> {
    Inline(InlineExecutor),
    #[cfg(imads_has_threads)]
    Pool(WorkerPoolExecutor<E, D>),
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static> Default
    for AdaptiveExecutor<E, D>
{
    fn default() -> Self {
        Self::Inline(InlineExecutor)
    }
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static> Clone
    for AdaptiveExecutor<E, D>
{
    fn clone(&self) -> Self {
        match self {
            Self::Inline(i) => Self::Inline(i.clone()),
            #[cfg(imads_has_threads)]
            Self::Pool(p) => Self::Pool(p.clone()),
        }
    }
}

impl<E: EvalCacheBackend + Clone + 'static, D: DecisionCacheBackend + Clone + 'static>
    Executor<E, D> for AdaptiveExecutor<E, D>
{
    fn run_batch(&self, items: Vec<WorkItem>, ctx: Arc<ExecCtx<E, D>>) -> Vec<WorkOutcome> {
        match self {
            Self::Inline(i) => i.run_batch(items, ctx),
            #[cfg(imads_has_threads)]
            Self::Pool(p) => p.run_batch(items, ctx),
        }
    }

    fn configure(&mut self, workers: usize) {
        #[cfg(imads_has_threads)]
        if workers > 1 {
            match self {
                Self::Pool(p) => p.configure(workers),
                Self::Inline(_) => *self = Self::Pool(WorkerPoolExecutor::new(workers)),
            }
            return;
        }

        // workers <= 1 or no thread support: use inline.
        if !matches!(self, Self::Inline(_)) {
            *self = Self::Inline(InlineExecutor);
        }
    }

    fn configure_params(&mut self, params: ExecutorParams) {
        match self {
            Self::Inline(_) => {}
            #[cfg(imads_has_threads)]
            Self::Pool(p) => p.configure_params(params),
        }
    }
}
