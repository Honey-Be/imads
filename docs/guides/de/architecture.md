# Architekturuebersicht

## Engine und PolicyBundle

Die Engine (`Engine<P: PolicyBundle>`) orchestriert die Optimierung ueber eine austauschbare
Policy-Oberflaeche. Jeder Policy-Slot ist ein assoziierter Typ auf `PolicyBundle`:

| Policy | Rolle | Anpassbar? |
|--------|-------|:---:|
| `SchedulerPolicy` | Batch-Dispatch-Reihenfolge | Ja |
| `SearchPolicy` | Kandidatenerzeugung und -bewertung | Ja |
| `LadderPolicy` | Konstruktion der (tau, S)-Genauigkeitsleiter | Ja |
| `DidsPolicy` | Strategie fuer dynamischen Unzulaessigkeitsgrad | Ja |
| `MarginPolicy` | Frueher Schwellenwert fuer Unzulaessigkeit/Zielfunktion | Ja |
| `CalibratorPolicy` | Delta-Regler und K-Learning | Ja |
| `AuditPolicy` | Hash-basierte Audit-Auswahl | Ja |
| `AcceptancePolicy` | Filter + progressive Barriere-Akzeptanz | Ja |
| `EvalCacheBackend` | Schaetzungs-Cache | Ja |
| `DecisionCacheBackend` | Entscheidungs-Cache | Ja |
| `Executor` | Ausfuehrung von Arbeitspaketen | Ja |

**Versiegelt (nicht anpassbar):**
- Poll-/Mesh-Aktualisierungen (`DefaultPoll`) — konvergenzkritisch

> **Hinweis:** `AcceptancePolicy` war zuvor als `AcceptanceEngine` versiegelt. Es ist nun ein
> oeffentlicher Trait. `DefaultAcceptance` implementiert `AcceptancePolicy` und bleibt der Standard.
> Benutzer koennen eigene Akzeptanz-Policies implementieren (z. B. Pareto-basiert fuer Multi-Objective).

## AdaptiveExecutor

`DefaultBundle` verwendet `AdaptiveExecutor`, der automatisch auswaehlt:

- **workers = 1** → `InlineExecutor` (sequentiell, kein Overhead)
- **workers > 1** → `WorkerPoolExecutor` (Thread-Pool mit Batch-Barriere)

Auf WASM-Zielen ohne Thread-Unterstuetzung ist nur `InlineExecutor` verfuegbar.
Auf `wasm32-wasip1-threads` und `wasm32-wasip3` ist die Pool-Variante aktiviert.

### Evaluator-Trait

Der `Evaluator`-Trait definiert die Black-Box-Schnittstelle:

| Methode / Typ | Erforderlich | Beschreibung |
|---------------|----------|-------------|
| `type Objectives: ObjectiveValues` | Ja | Assoziierter Typ fuer Zielfunktionswerte (f64, [f64;N] oder Vec<f64>) |
| `mc_sample(x, phi, env, k)` | Ja | Deterministische MC-Stichprobe von Zielfunktion + Constraints |
| `cheap_constraints(x, env)` | Nein | Schnelles Ablehnungsgate (Standard: alle akzeptieren) |
| `solver_bias(x, tau, env)` | Nein | Tau-abhaengiger Bias-Term (Standard: Null) |
| `num_constraints()` | Ja | Anzahl der Constraint-Werte |
| `num_objectives()` | Ja | Anzahl der Zielfunktionswerte (1 fuer Single-Objective) |
| `search_dim()` | Nein | Suchraum-Dimension; bei `Some(d)` wird `EngineConfig.search_dim` ueberschrieben |

Die Engine loest die Dimension wie folgt auf: `config.search_dim` > `evaluator.search_dim()` > Laenge des aktuellen Kandidaten > Fallback 1.

### ObjectiveValues und Multi-Objective-Unterstuetzung

Der `ObjectiveValues`-Trait abstrahiert ueber Single- und Multi-Objective-Evaluatoren. Er ist
implementiert fuer `f64` (Single-Objective), `[f64; N]` (feste Anzahl) und `Vec<f64>`
(dynamische Anzahl).

- `Estimates.f_hat` und `f_se` sind `Vec<f64>` (ein Eintrag pro Zielfunktion).
- `Estimates.num_objectives` gibt die Anzahl an.
- `JobResult::Truth.f` ist `Vec<f64>`.
- `EngineOutput.f_best` ist `Option<Vec<f64>>`.

Der Marker-Sub-Trait `SingleObjectiveEvaluator` hat eine Blanket-Implementierung fuer jeden Evaluator
mit `Objectives = f64`, wodurch die Rueckwaertskompatibilitaet gewahrt bleibt.

### EvaluatorErased

`EvaluatorErased` ist ein typgeloeschter Trait-Object-Wrapper, der intern von der Engine verwendet wird.
Er vermeidet generische Infizierung: Der Engine-Kern operiert auf `&dyn EvaluatorErased` anstatt
ueber den konkreten Evaluator-Typ parametrisiert zu sein. Benutzercode muss nicht direkt
mit diesem Trait interagieren.

## StratifiedSearch

`StratifiedSearch` ist ein Drop-in-Ersatz fuer `DefaultSearch` (ueber `PolicyBundle::Search`).
Es ist definiert in `imads-core/src/policies/stratified_search.rs` und kombiniert drei
Kandidatenerzeugungsmodi:

1. **Koordinatenschritt** — pollt bis zu `min(dim, 6)` Koordinatenrichtungen mit mesh-ausgerichteten
   Stoerungen.
2. **Gerichtete Suche** — extrapoliert entlang eines Verbesserungsvektors, der aus juengsten
   erfolgreichen Schritten abgeleitet wird.
3. **Halton-quasi-zufaellige globale Exploration** — erzeugt niedrig-diskrepante Punkte ueber
   den gesamten Suchraum fuer globale Abdeckung.

Das Zuteilungsverhaeltnis zwischen diesen Modi wird dynamisch basierend auf der
Problemdimensionalitaet angepasst:

| Dimensionalitaet | Koordinate | Gerichtet | Halton |
|:----------------:|:----------:|:---------:|:------:|
| dim <= 8         | 60%        | 20%       | 20%    |
| 8 < dim < 32     | 45%        | 25%       | 30%    |
| dim >= 32        | 30%        | 30%       | 40%    |

Hoeher-dimensionale Probleme erhalten mehr globales Explorationsbudget, da
Koordinatenschritte zunehmend ineffizient werden.

## AnisotropicMeshGeometry

`AnisotropicMeshGeometry` ermoeglicht **dimensionsweise Mesh-Schrittgroessen**. Anstelle einer
einzelnen skalaren Mesh-Schrittgroesse fuer alle Dimensionen hat jede Dimension ihre eigene `base_step`
und `mesh_mul`:

- `base_steps: Vec<f64>` — initiale Schrittgroesse pro Dimension.
- `mesh_muls: Vec<f64>` — Mesh-Groessen-Multiplikator pro Dimension.

`EngineConfig` enthaelt nun `mesh_base_steps: Option<Vec<f64>>`. Bei `Some(steps)` konstruiert die
Engine eine `AnisotropicMeshGeometry` anstelle der standardmaessigen isotropen Geometrie.

`SearchContext::mesh_step` wurde durch `mesh_steps: Vec<f64>` ersetzt (ein Vektor von
dimensionsweisen Schritten). Ein rueckwaertskompatibler `mesh_step()`-Accessor gibt das erste
Element zurueck fuer Code, der isotropes Mesh annimmt.

Die Funktion `env_rev_with_steps()` schliesst `base_steps` in den Cache-Key-Hash ein, wodurch
sichergestellt wird, dass verschiedene anisotrope Konfigurationen nicht im Evaluierungs-Cache kollidieren.

## Dreistufiger Entscheidungsfluss

1. **Stufe A (guenstig)** — `Evaluator::cheap_constraints()`. Ablehnung ohne Black-Box-Auswertung.
2. **PARTIAL** — Zwischenstufe mit (tau, S)-Genauigkeit. Kann fruehzeitige Unzulaessigkeit ausloesen oder abbrechen.
3. **TRUTH** — Endgueltige Auswertung auf hoechster Genauigkeitsstufe. Nur TRUTH kann in den Filter aufgenommen werden.

## Genauigkeitsleiter (Fidelity Ladder)

Die 2-Achsen-Leiter wird durch `tau_levels` (Toleranz, locker→streng) und `smc_levels`
(MC-Stichprobenzahl, niedrig→hoch) definiert. Die `LadderPolicy` kombiniert diese zu einer
geordneten Folge von `Phi = (Tau, Smc)`-Schritten. MC-Praefix-Wiederverwendung stellt sicher,
dass Stichproben aus Schritt i in Schritt i+1 wiederverwendet werden.

## Determinismus-Vertrag

Alle Policy-Entscheidungen sind reine Funktionen von (inputs, env_rev, policy_rev). Keine
Wanduhrzeit, Thread-Races oder Betriebssystem-Zufaelligkeit in Entscheidungspfaden. Dies ermoeglicht:
- Reproduzierbare Laeufe ueber verschiedene Maschinen hinweg
- 1-Worker und N-Worker liefern identische Ergebnisse
- Cache-Korrektheit durch deterministische Schluessel

## Calibrator-Rueckkopplungsschleife

Der Calibrator verfolgt:
- Falsch-unzulaessig-Rate pro Constraint pro Genauigkeitsstufe
- K (Bias-Schranke), gelernt aus gepaarten Audit-Stichproben
- Delta-Schwellenwert, angepasst ueber EWMA in Richtung Ziel-Falschrate

Aktualisierungen erfolgen an Batch-Grenzen in deterministischer Reihenfolge.
