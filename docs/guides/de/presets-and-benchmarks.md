# Presets und kleiner Benchmark-Workflow

Dieses Projekt liefert derzeit fünf Presets in `src/presets.rs` mit:

- `legacy_baseline`: reines Vergleichs-Preset, das das Verhalten vor Upgrade 5 annähert
- `balanced`: **empfohlener Standard**; erreicht eine Qualität auf throughput-Niveau mit einem kleineren partial-step-Budget
- `conservative`: sicherer / vorsichtiger, wenn das Risiko falsch-negativer Infeasible-Bewertungen am wichtigsten ist
- `throughput`: qualitätsorientiertes Preset; gibt mehr partial-step-Budget aus, um eine schnellere Anpassung zu erreichen

> **Hinweis:** Alle Presets setzen `search_dim` jetzt auf `None`. Die Engine fragt zur Laufzeit die `search_dim()`-Methode des Evaluators ab, um die Dimensionalitaet des Suchraums zu bestimmen.

## Empfohlene Verwendung

Verwenden Sie die Presets mit folgender Absicht:

- **Standard / die meisten Nutzer**: `balanced`
- **Sicherheit / Debugging / verrauschte Evaluatoren**: `conservative`
- **Qualitätsorientierte Sweeps**: `throughput`
- **Nur Vorher/Nachher-Vergleich**: `legacy_baseline`

Ein aktueller Bericht (`imads/reports/preset_report.csv`) zeigte:

- `balanced` und `throughput` erreichten denselben `f_best` im Toy-Benchmark
- `balanced` benötigte dafür weniger partial steps als `throughput`
- `conservative` opferte zu viel Lösungsqualität zugunsten der Vorsicht, um als Standard zu dienen

## Rust-Toolchain

Führen Sie die Tests mit Rust 1.94.0 aus:

```bash
cargo +1.94.0 test
```

## Kleiner Vergleichs-Benchmark

Führen Sie den kleinen Vergleichs-Benchmark aus:

```bash
cargo +1.94.0 bench --bench preset_compare
```

Das benutzerdefinierte Bench-Target gibt CSV-ähnliche Zeilen mit Laufzeit und wichtigen Engine-Statistiken für jedes Preset aus.
Der explizite Vorher/Nachher-Vergleich ist nun:

- `legacy_baseline` vs `balanced`

Dieses Paar ist der aussagekräftigste Vergleich zwischen „altem Verhalten" und „empfohlenem Standard".

## Lightweight-Report

Führen Sie den Lightweight-Report aus (einmaliges Timing + Engine-Statistiken):

```bash
cargo +1.94.0 run --release --example preset_report
```

Verwenden Sie die resultierende CSV-Datei, um mindestens Folgendes zu vergleichen:

- `truth_evals`
- `partial_steps`
- `invalid_eval_rejects`
- `f_best`

Ein gutes Standard-Preset sollte `partial_steps` deutlich unter dem Wert von `throughput` halten und dabei den Großteil der `f_best`-Verbesserung bewahren.


## Objective-Pruning-Parameter

Objective Pruning ist über `EngineConfig` und Presets konfigurierbar. Die aktuellen Presets verwenden dieses Gate, um das Verhalten von balanced/throughput/conservative zu unterscheiden:

- `objective_prune_min_smc_rank`: 1-basierter Rang unter den verschiedenen SMC-Levels, der erreicht werden muss
- `objective_prune_min_level`: minimales 1-basiertes Ladder-Level, das erreicht sein muss, bevor Pruning ausgelöst werden kann
- `objective_prune_require_back_half`: wenn true, wird Pruning zusätzlich auf die hintere Hälfte der Ladder beschränkt
- `objective_prune_disable_for_audit`: wenn true, umgehen audit-pflichtige Kandidaten das Objective Pruning

Empfohlene Interpretationen:

- `balanced`: moderates Pruning, beginnt ab dem 2. SMC-Rang und Level 2
- `throughput`: früheres Pruning, beginnt ab dem 1. SMC-Rang
- `conservative`: verzögertes Pruning, beginnt später und nur in der hinteren Hälfte
