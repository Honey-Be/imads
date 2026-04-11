# Presets und kleiner Benchmark-Workflow

Dieses Projekt liefert derzeit fuenf Presets in `src/presets.rs` mit:

- `legacy_baseline`: reines Vergleichs-Preset, das das Verhalten vor Upgrade 5 annaehernd nachbildet
- `balanced`: **empfohlener Standard**; erreicht eine Qualitaet auf throughput-Niveau mit einem kleineren partial-step-Budget
- `conservative`: sicherer / vorsichtiger, wenn das Risiko falsch-negativer Infeasible-Bewertungen am wichtigsten ist
- `throughput`: qualitaetsorientiertes Preset; gibt mehr partial-step-Budget aus, um eine schnellere Anpassung zu erreichen

> **Hinweis:** Alle Presets setzen `search_dim` jetzt auf `None`. Die Engine fragt zur Laufzeit die `search_dim()`-Methode des Evaluators ab, um die Dimensionalitaet des Suchraums zu bestimmen.

Alle Presets verwenden standardmaessig `DefaultSearch`. `StratifiedSearch` ist als
Drop-in-Ersatz ueber `PolicyBundle::Search` verfuegbar fuer Benutzer, die die kombinierte
Koordinatenschritt-/Gerichtete-/Halton-Explorationsstrategie wuenschen (siehe
[Architektur](architecture.md#stratifiedsearch)).

Anisotrope Mesh-Geometrie kann bei jedem Preset aktiviert werden, indem
`EngineConfig.mesh_base_steps` auf `Some(vec![...])` mit dimensionsweisen Schrittgroessen gesetzt wird.
Wenn nicht gesetzt, wird das standardmaessige isotrope Mesh verwendet.

## Empfohlene Verwendung

Verwenden Sie die Presets mit folgender Absicht:

- **Standard / die meisten Nutzer**: `balanced`
- **Sicherheit / Debugging / verrauschte Evaluatoren**: `conservative`
- **Qualitaetsorientierte Sweeps**: `throughput`
- **Nur Vorher/Nachher-Vergleich**: `legacy_baseline`

Ein aktueller Bericht (`imads/reports/preset_report.csv`) zeigte:

- `balanced` und `throughput` erreichten denselben `f_best` im Toy-Benchmark
- `balanced` benoetigte dafuer weniger partial steps als `throughput`
- `conservative` opferte zu viel Loesungsqualitaet zugunsten der Vorsicht, um als Standard zu dienen

## Rust-Toolchain

Fuehren Sie die Tests mit Rust 1.94.0 aus:

```bash
cargo +1.94.0 test
```

## Kleiner Vergleichs-Benchmark

Fuehren Sie den kleinen Vergleichs-Benchmark aus:

```bash
cargo +1.94.0 bench --bench preset_compare
```

Das benutzerdefinierte Bench-Target gibt CSV-aehnliche Zeilen mit Laufzeit und wichtigen Engine-Statistiken fuer jedes Preset aus.
Der explizite Vorher/Nachher-Vergleich ist nun:

- `legacy_baseline` vs `balanced`

Dieses Paar ist der aussagekraeftigste Vergleich zwischen "altem Verhalten" und "empfohlenem Standard".

## Lightweight-Report

Fuehren Sie den Lightweight-Report aus (einmaliges Timing + Engine-Statistiken):

```bash
cargo +1.94.0 run --release --example preset_report
```

Verwenden Sie die resultierende CSV-Datei, um mindestens Folgendes zu vergleichen:

- `truth_evals`
- `partial_steps`
- `invalid_eval_rejects`
- `f_best`

Ein gutes Standard-Preset sollte `partial_steps` deutlich unter dem Wert von `throughput` halten und dabei den Grossteil der `f_best`-Verbesserung bewahren.


## Objective-Pruning-Parameter

Objective Pruning ist ueber `EngineConfig` und Presets konfigurierbar. Die aktuellen Presets verwenden dieses Gate, um das Verhalten von balanced/throughput/conservative zu unterscheiden:

- `objective_prune_min_smc_rank`: 1-basierter Rang unter den verschiedenen SMC-Levels, der erreicht werden muss
- `objective_prune_min_level`: minimales 1-basiertes Ladder-Level, das erreicht sein muss, bevor Pruning ausgeloest werden kann
- `objective_prune_require_back_half`: wenn true, wird Pruning zusaetzlich auf die hintere Haelfte der Ladder beschraenkt
- `objective_prune_disable_for_audit`: wenn true, umgehen audit-pflichtige Kandidaten das Objective Pruning

Empfohlene Interpretationen:

- `balanced`: moderates Pruning, beginnt ab dem 2. SMC-Rang und Level 2
- `throughput`: frueheres Pruning, beginnt ab dem 1. SMC-Rang
- `conservative`: verzoegertes Pruning, beginnt spaeter und nur in der hinteren Haelfte
