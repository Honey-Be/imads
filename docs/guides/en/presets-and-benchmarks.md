# Presets and small benchmark workflow

This project currently ships five presets in `src/presets.rs`:

- `legacy_baseline`: comparison-only preset that approximates pre-upgrade5 behavior
- `balanced`: **recommended default**; reaches throughput-like quality with a smaller partial-step budget
- `conservative`: safer / more cautious when false infeasible risk matters most
- `throughput`: quality-first preset; spends more partial-step budget for faster adaptation

> **Note:** All presets now set `search_dim` to `None`. The engine queries the evaluator's `search_dim()` method at runtime to determine the search space dimensionality.

All presets use `DefaultSearch` by default. `StratifiedSearch` is available as a
drop-in replacement via `PolicyBundle::Search` for users who want the combined
coordinate-step / directional / Halton exploration strategy (see
[Architecture](architecture.md#stratifiedsearch)).

Anisotropic mesh geometry can be enabled on any preset by setting
`EngineConfig.mesh_base_steps` to `Some(vec![...])` with per-dimension step sizes.
When unset, the default isotropic mesh is used.

## Recommended usage

Use the presets with the following intent:

- **Default / most users**: `balanced`
- **Safety / debugging / noisy evaluators**: `conservative`
- **Quality-first sweeps**: `throughput`
- **Before/after comparison only**: `legacy_baseline`

A recent report (`imads/reports/preset_report.csv`) showed:

- `balanced` and `throughput` reached the same `f_best` on the toy benchmark
- `balanced` did so with fewer partial steps than `throughput`
- `conservative` traded too much solution quality for caution to be the default

## Rust toolchain

Run tests with Rust 1.94.0:

```bash
cargo +1.94.0 test
```

## Small comparison bench

Run the small comparison bench:

```bash
cargo +1.94.0 bench --bench preset_compare
```

The custom bench target prints CSV-like rows with elapsed time and key engine stats for each preset.
The explicit before/after compare is now:

- `legacy_baseline` vs `balanced`

That pair is the most informative “old behavior vs recommended default” comparison.

## Lightweight report

Run the lightweight report (single-shot timing + engine stats):

```bash
cargo +1.94.0 run --release --example preset_report
```

Use the resulting CSV to compare at least:

- `truth_evals`
- `partial_steps`
- `invalid_eval_rejects`
- `f_best`

A good default preset should keep `partial_steps` well below `throughput` while preserving most of its `f_best` improvement.


## Objective pruning parameters

Objective pruning is configurable through `EngineConfig` and presets. The current presets use this gate to distinguish balanced/throughput/conservative behavior:

- `objective_prune_min_smc_rank`: 1-based rank among distinct SMC levels that must be reached
- `objective_prune_min_level`: minimum 1-based ladder level required before pruning can trigger
- `objective_prune_require_back_half`: if true, pruning is additionally limited to the back half of the ladder
- `objective_prune_disable_for_audit`: if true, audit-required candidates bypass objective pruning

Recommended interpretations:

- `balanced`: moderate pruning, starts at the 2nd SMC rank and level 2
- `throughput`: earlier pruning, starts at the 1st SMC rank
- `conservative`: delayed pruning, starts later and only in the back half
