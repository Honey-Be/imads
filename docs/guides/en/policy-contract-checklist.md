# Policy Contract Checklist

This document describes the **contracts** that must be upheld when customizing the Policy layer of the **Integrated MADS framework** as patch-sets or plugins.

> Goal: enable safe replacement of scheduling, search, and statistical policies while preserving **correctness, reproducibility (determinism), and cache consistency**.

---

## 1) Absolute Invariants (Sealed by Default)

The following are **not customization targets** — violating them easily breaks convergence, correctness, or determinism.

- **Only TRUTH may produce a final accept/reject**
  - `PARTIAL` results are used only as **priority/pruning hints**.
  - Feasibility confirmation or filter insertion happens only at TRUTH (τ_L, S_L).
- **Poll/mesh update rules are sealed**
  - Core of MADS convergence theory.
- **Cache key components must not be altered**
  - **EvalCache (expensive evaluation artifacts)** minimum key: `(x_mesh, phi=(tau,S), env_rev)`
  - **DecisionCache (policy-dependent decisions/results)** minimum key: `(x_mesh, phi=(tau,S), env_rev, policy_rev, tag)`

---

## 2) Determinism Contract

Violating any of the following breaks the *reorderably deterministic* requirement (or higher-level reproducibility guarantees).

### 2.1 Policy functions must be pure
- Same inputs → same outputs
- The following must not be used directly (or must be **promoted to Env**):
  - Wall-clock time
  - OS randomness
  - Global state that depends on thread races

### 2.2 Audit selection must be deterministic
- Recommended: modular selection based on `hash(x_mesh, phi, env_rev, ...)`
- Forbidden: runtime randomness like `rand::thread_rng()`

### 2.3 Batch-boundary updates must follow a deterministic order
- Asynchronous completion order varies across runtimes
- The engine must sort events before passing them to policies (recommended key):
  - `(candidate_id, phi, tag)` lexicographic

---

## 3) (τ, S) 2-Axis Fidelity Ladder Contract

- The ladder must satisfy **monotone precision refinement**
  - Typically τ decreases (tighter), S increases
- MC must support **prefix reuse**
  - When S increases, existing samples 1..S_i are reused as-is
- When the ladder changes, `policy_rev` must be incremented and reflected in cache keys

---

## 4) Scheduler (SchedulerPolicy) Contract

- The scheduler **may change efficiency but must not change correctness**
- Forbidden / caution:
  - Time-based decisions ("N seconds have passed, so…")
  - Non-deterministic choices that depend on OS/thread scheduling
- Recommended:
  - `W` (worker count) should only affect "concurrency limit / batch size"
  - Candidate selection must be deterministic over the input list

---

## 5) SearchPolicy Contract

- Search is free, but **final submissions must be quantized onto the mesh**
- Reducing duplicate candidates improves cache efficiency
- Scores/priorities must be deterministic

### 5.1) StratifiedSearch Contract

`StratifiedSearch` is a built-in `SearchPolicy` that combines three candidate generation
modes: coordinate step, directional search, and Halton quasi-random exploration.

- The mode allocation ratio is determined by dimensionality at engine construction time
  and must not change mid-run (doing so would break determinism)
- Coordinate step must poll at most `min(dim, 6)` directions per iteration
- Directional search must derive the improvement vector from the same deterministic
  history window; it must not depend on wall-clock time or thread completion order
- Halton points must use a deterministic sequence index tied to `env_rev`
- All generated candidates must be quantized onto the mesh (same as any `SearchPolicy`)

---

## 5.5) Executor (Batch Runner) Contract

Replace the `Executor` if you want to integrate a real parallel/async runtime.

- The executor handles **evaluation execution only**
  - Performs only `(WorkItem) -> Estimates / cache hit` level work
  - **Early stop / accept / reject / calibration** — all *policy decisions* happen on the engine thread only
- **Batch barrier discipline is recommended**
  - `run_batch(items)` returns results only after the entire batch completes
  - The engine sorts returned results by `cand_id` for deterministic processing
  - Even if the worker pool uses **persistent threads**:
    - The execution context (ExecCtx) passed per batch is **valid only during that run_batch call**
    - The executor/workers **must not retain references/pointers to ExecCtx beyond the batch**
- Determinism (reproducibility) constraints:
  - The executor must not select/reorder tasks based on wall-clock time
  - Cancellation makes result reproducibility difficult; the default recommendation is "no intra-batch cancellation"

The engine calls `executor.configure_params(ExecutorParams{..})` before batch dispatch to pass performance parameters.

- `ExecutorParams.chunk_base`: upper bound on tasks a worker pulls from the global queue at once
- `ExecutorParams.spin_limit`: spin iterations before falling back to the condvar in the batch barrier
- `chunk_base` may be auto-tuned online based on batch cost variance (CV) when `EngineConfig.executor_chunk_auto_tune=true`
- These parameters **must not affect correctness** and must be unrelated to cache keys / accept rules

The engine also calls `executor.configure(W)` at the start of `run(..., workers=W)` to synchronize the executor with the worker count.

- `Executor::configure(&mut self, W)` can assume it is called **only before batch dispatch begins**
- Using `configured(W)` (owned/builder style) is fine if more convenient

The engine may additionally call `Executor::configure_params(ExecutorParams)` to pass performance parameters.

- In the current skeleton, `EngineConfig.executor_chunk_base` maps to `ExecutorParams.chunk_base`
- `chunk_base` is the upper bound on tasks pulled from the global queue into the local ring at once
  - Since there is no work-stealing, the effective chunk is further capped by `ceil(batch_size / W)`
- This value **only affects performance** and must not affect correctness or determinism

---

## 5.6) Run-Global Resume Configuration Contract

Setting `EngineConfig.max_steps_per_iter` to `Some(k)` causes only `k` `WorkItem`s to execute per iteration;
remaining Ready candidates **resume** in the next iteration.

- `None`: exhaust all Ready candidates per iteration (v3 behavior)
- `Some(k)`: **creates a resume path**, making fairness (anti-starvation) policies important
  - e.g., prioritize `audit_required` candidates, add age-based scoring bonuses

---

## 5.7) AnisotropicMeshGeometry Contract

When `EngineConfig.mesh_base_steps` is `Some(steps)`, the engine uses per-dimension mesh
step sizes instead of a single scalar step.

- `base_steps.len()` must equal the search dimension; a mismatch is a configuration error
- `mesh_muls` tracks per-dimension mesh multipliers independently
- `SearchContext::mesh_steps` (plural) replaces the former scalar `mesh_step`
  - The backward-compatible `mesh_step()` accessor returns `mesh_steps[0]`
- All mesh quantization in search and poll must use the per-dimension step for the
  corresponding coordinate
- `env_rev_with_steps()` must include the `base_steps` vector in the cache key hash so
  that different anisotropic configurations produce distinct cache keys
- Poll/mesh update rules apply per-dimension: each dimension refines or coarsens its own
  mesh multiplier independently

---

## 5.8) AcceptancePolicy Contract

`AcceptancePolicy` is a public trait (previously sealed as `AcceptanceEngine`).
`DefaultAcceptance` implements it and remains the default.

- The acceptance policy receives TRUTH results only; it must never accept PARTIAL results
- `AcceptancePolicy::accept(candidate, filter, barrier)` must return a deterministic
  accept/reject decision given the same inputs
- Custom implementations (e.g., Pareto-based for multi-objective) must satisfy:
  - Filter dominance: a candidate is accepted only if it is not dominated by the current
    filter contents (or improves the barrier)
  - Barrier monotonicity: the progressive barrier threshold must not increase
  - Determinism: the decision must not depend on wall-clock time, thread ordering, or
    OS randomness
- When a custom `AcceptancePolicy` changes its internal state (e.g., Pareto front
  membership), `policy_rev` must be incremented

---

## 6) DIDS Policy (DidsPolicy) Contract

- `a` (assignment vector) is an **early-stop efficiency tool**
- Must not change feasibility confirmation / accept rules (TRUTH only)
- `a` updates happen **only at batch boundaries**

---

## 7) Margin / Calibrator / Audit Policy Contract

### 7.1 Early infeasible must be conservative
- Suppressing false infeasibles takes priority
- Boundary points are corrected via audit/promotion

### 7.2 Calibrator updates happen only at batch boundaries
- Input events are received only as sorted lists
- If an update changes policy, `policy_rev` must be incremented

### 7.3 Calibrator parameters are exposed via EngineConfig
- Target false infeasible rate (`calibrator_target_false`), minimum audit samples (`calibrator_min_audits`),
  update step size (`calibrator_eta_delta`), clamp range (`calibrator_delta_min/max`) must all be
  adjustable through `EngineConfig`.

---

## 8) Cache (EvalCache / DecisionCache) Contract

- Caching is a **performance optimization**; a cache miss must never change correctness
- Cache key components must not be altered:
  - EvalCacheKey: `(x_mesh, phi, env_rev)`
  - DecisionCacheKey: `(x_mesh, phi, env_rev, policy_rev, tag)`
- If policy state (δ/K/a etc.) affects results, **`policy_rev` must be bumped** to prevent DecisionCache contamination

---

## 9) Required Tests (Custom Bundle Verification)

If you add a custom PolicyBundle, passing the following tests is strongly recommended:

1. **Determinism replay**: repeated runs with identical inputs produce identical results
2. **Completion event order independence**: different completion orders yield identical results
3. **Cache consistency**: results are identical with cache on/off (warm/cold)
4. (If possible) **Reorderable call multiset** style verification

---

## 10) Customization Summary

Safe to customize:
- SchedulerPolicy / SearchPolicy (incl. StratifiedSearch) / LadderPolicy / DidsPolicy
- MarginPolicy / CalibratorPolicy / AuditPolicy
- AcceptancePolicy (e.g., Pareto-based multi-objective)
- CacheBackend / Telemetry

Conditional (advanced):
- Solver warm-start / solver internal stop-resume policies
- Extending PARTIAL result usage scope (accepting is not recommended)
- AnisotropicMeshGeometry (per-dimension steps via EngineConfig.mesh_base_steps)

Sealed by default (convergence/correctness critical):
- Poll/mesh update rules

## 7.4 Objective Pruning Contract

- Objective pruning is merely a **candidate promotion stop**; it must not change final accept/reject semantics
- `audit_required` candidates must be able to bypass pruning when needed
- The pruning gate must be adjustable via `EngineConfig`/presets and must be deterministic
- Recommended parameters:
  - `objective_prune_min_smc_rank`
  - `objective_prune_min_level`
  - `objective_prune_require_back_half`
  - `objective_prune_disable_for_audit`
