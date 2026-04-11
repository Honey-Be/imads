# Architecture Overview

## Engine and PolicyBundle

The engine (`Engine<P: PolicyBundle>`) orchestrates optimization through a pluggable
policy surface. Each policy slot is an associated type on `PolicyBundle`:

| Policy | Role | Customizable? |
|--------|------|:---:|
| `SchedulerPolicy` | Batch dispatch ordering | Yes |
| `SearchPolicy` | Candidate generation and scoring | Yes |
| `LadderPolicy` | (tau, S) fidelity ladder construction | Yes |
| `DidsPolicy` | Dynamic infeasibility degree strategy | Yes |
| `MarginPolicy` | Early infeasible/objective threshold | Yes |
| `CalibratorPolicy` | Delta controller and K-learning | Yes |
| `AuditPolicy` | Hash-based audit selection | Yes |
| `AcceptancePolicy` | Filter + progressive barrier acceptance | Yes |
| `EvalCacheBackend` | Estimates cache | Yes |
| `DecisionCacheBackend` | Decision cache | Yes |
| `Executor` | Work batch execution | Yes |

**Sealed (non-customizable):**
- Poll/mesh updates (`DefaultPoll`) — convergence-critical

> **Note:** `AcceptancePolicy` was previously sealed as `AcceptanceEngine`. It is now a
> public trait. `DefaultAcceptance` implements `AcceptancePolicy` and remains the default.
> Users can implement custom acceptance policies (e.g., Pareto-based for multi-objective).

## AdaptiveExecutor

`DefaultBundle` uses `AdaptiveExecutor` which auto-selects:

- **workers = 1** → `InlineExecutor` (sequential, zero overhead)
- **workers > 1** → `WorkerPoolExecutor` (thread pool with batch barrier)

On WASM targets without thread support, only `InlineExecutor` is available.
On `wasm32-wasip1-threads` and `wasm32-wasip3`, the pool variant is enabled.

### Evaluator Trait

The `Evaluator` trait defines the black-box interface:

| Method / Type | Required | Description |
|---------------|----------|-------------|
| `type Objectives: ObjectiveValues` | Yes | Associated type for objective values (f64, [f64;N], or Vec<f64>) |
| `mc_sample(x, phi, env, k)` | Yes | Deterministic MC sample of objective + constraints |
| `cheap_constraints(x, env)` | No | Fast rejection gate (default: accept all) |
| `solver_bias(x, tau, env)` | No | Tau-dependent bias term (default: zero) |
| `num_constraints()` | Yes | Number of constraint values |
| `num_objectives()` | Yes | Number of objective values (1 for single-objective) |
| `search_dim()` | No | Search space dimension; when `Some(d)`, overrides `EngineConfig.search_dim` |

The engine resolves dimension as: `config.search_dim` > `evaluator.search_dim()` > incumbent length > fallback 1.

### ObjectiveValues and Multi-Objective Support

The `ObjectiveValues` trait abstracts over single and multi-objective evaluators. It is
implemented for `f64` (single objective), `[f64; N]` (fixed-count), and `Vec<f64>`
(dynamic count).

- `Estimates.f_hat` and `f_se` are `Vec<f64>` (one entry per objective).
- `Estimates.num_objectives` reports the count.
- `JobResult::Truth.f` is `Vec<f64>`.
- `EngineOutput.f_best` is `Option<Vec<f64>>`.

The marker sub-trait `SingleObjectiveEvaluator` has a blanket impl for any evaluator
with `Objectives = f64`, preserving backward compatibility.

### EvaluatorErased

`EvaluatorErased` is a type-erased trait object wrapper used internally by the engine.
It avoids generic infection: the engine core operates on `&dyn EvaluatorErased` rather
than being parameterized over the concrete evaluator type. User code does not need to
interact with this trait directly.

## StratifiedSearch

`StratifiedSearch` is a drop-in replacement for `DefaultSearch` (via `PolicyBundle::Search`).
It is defined in `imads-core/src/policies/stratified_search.rs` and combines three
candidate generation modes:

1. **Coordinate step** — polls up to `min(dim, 6)` coordinate directions with mesh-aligned
   perturbations.
2. **Directional search** — extrapolates along an improvement vector derived from recent
   successful steps.
3. **Halton quasi-random global exploration** — generates low-discrepancy points across
   the full search space for global coverage.

The allocation ratio among these modes is dynamically adjusted based on problem
dimensionality:

| Dimensionality | Coordinate | Directional | Halton |
|:--------------:|:----------:|:-----------:|:------:|
| dim <= 8       | 60%        | 20%         | 20%    |
| 8 < dim < 32   | 45%        | 25%         | 30%    |
| dim >= 32      | 30%        | 30%         | 40%    |

Higher-dimensional problems receive more global exploration budget because coordinate
stepping becomes increasingly inefficient.

## AnisotropicMeshGeometry

`AnisotropicMeshGeometry` enables **per-dimension mesh step sizes**. Instead of a single
scalar mesh step shared by all dimensions, each dimension has its own `base_step` and
`mesh_mul`:

- `base_steps: Vec<f64>` — initial step size per dimension.
- `mesh_muls: Vec<f64>` — mesh size multiplier per dimension.

`EngineConfig` now includes `mesh_base_steps: Option<Vec<f64>>`. When `Some(steps)`, the
engine constructs an `AnisotropicMeshGeometry` instead of the default isotropic geometry.

`SearchContext::mesh_step` has been replaced by `mesh_steps: Vec<f64>` (a vector of
per-dimension steps). A backward-compatible `mesh_step()` accessor returns the first
element for code that assumes isotropic mesh.

The `env_rev_with_steps()` function includes `base_steps` in the cache key hash, ensuring
that different anisotropic configurations do not collide in the evaluation cache.

## Three-Stage Decision Flow

1. **Stage A (Cheap)** — `Evaluator::cheap_constraints()`. Reject without black-box evaluation.
2. **PARTIAL** — Intermediate (tau, S) fidelity. May trigger early infeasible or stop.
3. **TRUTH** — Final evaluation at highest fidelity. Only TRUTH can be accepted into the filter.

## Fidelity Ladder

The 2-axis ladder is defined by `tau_levels` (tolerance, loose->tight) and `smc_levels`
(MC sample count, low->high). The `LadderPolicy` combines these into an ordered sequence
of `Phi = (Tau, Smc)` steps. MC prefix reuse ensures samples from step i are reused in step i+1.

## Determinism Contract

All policy decisions are pure functions of (inputs, env_rev, policy_rev). No wall-clock
time, thread races, or OS randomness in decision paths. This enables:
- Reproducible runs across machines
- 1-worker and N-worker produce identical results
- Cache correctness via deterministic keys

## Calibrator Feedback Loop

The calibrator tracks:
- False infeasible rate per constraint per fidelity level
- K (bias bound) learned from paired audit samples
- Delta threshold adjusted via EWMA toward target false rate

Updates happen at batch boundaries in deterministic order.
