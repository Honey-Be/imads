# Architekturübersicht

## Engine und PolicyBundle

Die Engine (`Engine<P: PolicyBundle>`) orchestriert die Optimierung über eine austauschbare
Policy-Oberfläche. Jeder Policy-Slot ist ein assoziierter Typ auf `PolicyBundle`:

| Policy | Rolle | Anpassbar? |
|--------|-------|:---:|
| `SchedulerPolicy` | Batch-Dispatch-Reihenfolge | Ja |
| `SearchPolicy` | Kandidatenerzeugung und -bewertung | Ja |
| `LadderPolicy` | Konstruktion der (tau, S)-Genauigkeitsleiter | Ja |
| `DidsPolicy` | Strategie für dynamischen Unzulässigkeitsgrad | Ja |
| `MarginPolicy` | Früher Schwellenwert für Unzulässigkeit/Zielfunktion | Ja |
| `CalibratorPolicy` | Delta-Regler und K-Learning | Ja |
| `AuditPolicy` | Hash-basierte Audit-Auswahl | Ja |
| `EvalCacheBackend` | Schätzungs-Cache | Ja |
| `DecisionCacheBackend` | Entscheidungs-Cache | Ja |
| `Executor` | Ausführung von Arbeitspaketen | Ja |

**Versiegelt (nicht anpassbar):**
- Poll-/Mesh-Aktualisierungen (`DefaultPoll`) — konvergenzkritisch
- Akzeptanzlogik (`DefaultAcceptance`) — Filter + progressive Barriere

## AdaptiveExecutor

`DefaultBundle` verwendet `AdaptiveExecutor`, der automatisch auswählt:

- **workers = 1** → `InlineExecutor` (sequentiell, kein Overhead)
- **workers > 1** → `WorkerPoolExecutor` (Thread-Pool mit Batch-Barriere)

Auf WASM-Zielen ohne Thread-Unterstützung ist nur `InlineExecutor` verfügbar.
Auf `wasm32-wasip1-threads` und `wasm32-wasip3` ist die Pool-Variante aktiviert.

## Dreistufiger Entscheidungsfluss

1. **Stufe A (günstig)** — `Evaluator::cheap_constraints()`. Ablehnung ohne Black-Box-Auswertung.
2. **PARTIAL** — Zwischenstufe mit (tau, S)-Genauigkeit. Kann frühzeitige Unzulässigkeit auslösen oder abbrechen.
3. **TRUTH** — Endgültige Auswertung auf höchster Genauigkeitsstufe. Nur TRUTH kann in den Filter aufgenommen werden.

## Genauigkeitsleiter (Fidelity Ladder)

Die 2-Achsen-Leiter wird durch `tau_levels` (Toleranz, locker→streng) und `smc_levels`
(MC-Stichprobenzahl, niedrig→hoch) definiert. Die `LadderPolicy` kombiniert diese zu einer
geordneten Folge von `Phi = (Tau, Smc)`-Schritten. MC-Präfix-Wiederverwendung stellt sicher,
dass Stichproben aus Schritt i in Schritt i+1 wiederverwendet werden.

## Determinismus-Vertrag

Alle Policy-Entscheidungen sind reine Funktionen von (inputs, env_rev, policy_rev). Keine
Wanduhrzeit, Thread-Races oder Betriebssystem-Zufälligkeit in Entscheidungspfaden. Dies ermöglicht:
- Reproduzierbare Läufe über verschiedene Maschinen hinweg
- 1-Worker und N-Worker liefern identische Ergebnisse
- Cache-Korrektheit durch deterministische Schlüssel

## Calibrator-Rückkopplungsschleife

Der Calibrator verfolgt:
- Falsch-unzulässig-Rate pro Constraint pro Genauigkeitsstufe
- K (Bias-Schranke), gelernt aus gepaarten Audit-Stichproben
- Delta-Schwellenwert, angepasst über EWMA in Richtung Ziel-Falschrate

Aktualisierungen erfolgen an Batch-Grenzen in deterministischer Reihenfolge.
