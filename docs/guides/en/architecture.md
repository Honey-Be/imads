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
| `EvalCacheBackend` | Estimates cache | Yes |
| `DecisionCacheBackend` | Decision cache | Yes |
| `Executor` | Work batch execution | Yes |

**Sealed (non-customizable):**
- Poll/mesh updates (`DefaultPoll`) — convergence-critical
- Acceptance logic (`DefaultAcceptance`) — filter + progressive barrier

## AdaptiveExecutor

`DefaultBundle` uses `AdaptiveExecutor` which auto-selects:

- **workers = 1** → `InlineExecutor` (sequential, zero overhead)
- **workers > 1** → `WorkerPoolExecutor` (thread pool with batch barrier)

On WASM targets without thread support, only `InlineExecutor` is available.
On `wasm32-wasip1-threads` and `wasm32-wasip3`, the pool variant is enabled.

### Evaluator Trait

The `Evaluator` trait defines the black-box interface:

| Method | Required | Description |
|--------|----------|-------------|
| `mc_sample(x, phi, env, k)` | Yes | Deterministic MC sample of objective + constraints |
| `cheap_constraints(x, env)` | No | Fast rejection gate (default: accept all) |
| `solver_bias(x, tau, env)` | No | Tau-dependent bias term (default: zero) |
| `num_constraints()` | Yes | Number of constraint values |
| `search_dim()` | No | Search space dimension; when `Some(d)`, overrides `EngineConfig.search_dim` |

The engine resolves dimension as: `config.search_dim` > `evaluator.search_dim()` > incumbent length > fallback 1.

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
