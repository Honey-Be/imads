# Policy Contract Checklist

Dieses Dokument beschreibt die **Contracts**, die bei der Anpassung der Policy-Schicht des **Integrated MADS Frameworks** als Patch-Sets oder Plugins eingehalten werden muessen.

> Ziel: Sicherer Austausch von Scheduling-, Such- und Statistik-Policies bei gleichzeitiger Wahrung von **Korrektheit, Reproduzierbarkeit (Determinismus) und Cache-Konsistenz**.

---

## 1) Absolute Invarianten (standardmaessig versiegelt)

Die folgenden Punkte sind **keine Anpassungsziele** — ein Verstoss kann leicht Konvergenz, Korrektheit oder Determinismus brechen.

- **Nur TRUTH darf ein endgueltiges Accept/Reject erzeugen**
  - `PARTIAL`-Ergebnisse werden ausschliesslich als **Prioritaets-/Pruning-Hinweise** verwendet.
  - Machbarkeitsbestaetigung oder Filter-Einfuegung erfolgt nur bei TRUTH (τ_L, S_L).
- **Poll/Mesh-Update-Regeln sind versiegelt**
  - Kern der MADS-Konvergenztheorie.
- **Cache-Key-Komponenten duerfen nicht veraendert werden**
  - **EvalCache (aufwendige Evaluierungsartefakte)** Mindestschluessel: `(x_mesh, phi=(tau,S), env_rev)`
  - **DecisionCache (Policy-abhaengige Entscheidungen/Ergebnisse)** Mindestschluessel: `(x_mesh, phi=(tau,S), env_rev, policy_rev, tag)`

---

## 2) Determinism Contract

Ein Verstoss gegen eine der folgenden Regeln bricht die Anforderung der *reorderably deterministic* Eigenschaft (oder uebergeordnete Reproduzierbarkeitsgarantien).

### 2.1 Policy-Funktionen muessen pur sein
- Gleiche Eingaben → gleiche Ausgaben
- Folgendes darf nicht direkt verwendet werden (oder muss **in Env ueberfuehrt** werden):
  - Wall-Clock-Zeit
  - Betriebssystem-Zufallszahlen
  - Globaler Zustand, der von Thread-Races abhaengt

### 2.2 Audit-Auswahl muss deterministisch sein
- Empfohlen: Modulare Auswahl basierend auf `hash(x_mesh, phi, env_rev, ...)`
- Verboten: Laufzeit-Zufaelligkeit wie `rand::thread_rng()`

### 2.3 Batch-Boundary-Updates muessen einer deterministischen Reihenfolge folgen
- Die asynchrone Abschlussreihenfolge variiert zwischen Laufzeitumgebungen
- Die Engine muss Ereignisse sortieren, bevor sie an Policies uebergeben werden (empfohlener Schluessel):
  - `(candidate_id, phi, tag)` lexikographisch

---

## 3) (τ, S) 2-Axis Fidelity Ladder Contract

- Die Ladder muss **monotone Praezisionsverfeinerung** erfuellen
  - Typischerweise nimmt τ ab (enger), S nimmt zu
- MC muss **Prefix-Reuse** unterstuetzen
  - Wenn S steigt, werden bestehende Samples 1..S_i unveraendert wiederverwendet
- Bei Aenderung der Ladder muss `policy_rev` inkrementiert und in Cache-Keys widergespiegelt werden

---

## 4) Scheduler (SchedulerPolicy) Contract

- Der Scheduler **darf die Effizienz aendern, aber nicht die Korrektheit**
- Verboten / Vorsicht:
  - Zeitbasierte Entscheidungen ("N Sekunden sind vergangen, also ...")
  - Nicht-deterministische Entscheidungen, die von OS-/Thread-Scheduling abhaengen
- Empfohlen:
  - `W` (Worker-Anzahl) sollte nur "Concurrency-Limit / Batch-Groesse" beeinflussen
  - Die Kandidatenauswahl muss ueber die Eingabeliste deterministisch sein

---

## 5) SearchPolicy Contract

- Die Suche ist frei, aber **finale Einreichungen muessen auf das Mesh quantisiert werden**
- Die Reduzierung doppelter Kandidaten verbessert die Cache-Effizienz
- Scores/Prioritaeten muessen deterministisch sein

### 5.1) StratifiedSearch Contract

`StratifiedSearch` ist eine eingebaute `SearchPolicy`, die drei Kandidatenerzeugungsmodi
kombiniert: Koordinatenschritt, gerichtete Suche und Halton-quasi-zufaellige Exploration.

- Das Modus-Zuteilungsverhaeltnis wird durch die Dimensionalitaet bei der Engine-Konstruktion bestimmt
  und darf sich waehrend des Laufs nicht aendern (andernfalls wuerde der Determinismus gebrochen)
- Der Koordinatenschritt darf pro Iteration hoechstens `min(dim, 6)` Richtungen abfragen
- Die gerichtete Suche muss den Verbesserungsvektor aus demselben deterministischen
  Verlaufsfenster ableiten; sie darf nicht von Wall-Clock-Zeit oder Thread-Abschlussreihenfolge abhaengen
- Halton-Punkte muessen einen deterministischen Sequenzindex verwenden, der an `env_rev` gebunden ist
- Alle erzeugten Kandidaten muessen auf das Mesh quantisiert werden (wie bei jeder `SearchPolicy`)

---

## 5.5) Executor (Batch Runner) Contract

Ersetzen Sie den `Executor`, wenn Sie eine echte parallele/asynchrone Laufzeitumgebung integrieren moechten.

- Der Executor behandelt **ausschliesslich die Evaluierungsausfuehrung**
  - Fuehrt nur Arbeit auf der Ebene `(WorkItem) -> Estimates / cache hit` aus
  - **Early Stop / Accept / Reject / Calibration** — alle *Policy-Entscheidungen* erfolgen ausschliesslich im Engine-Thread
- **Batch-Barrier-Disziplin wird empfohlen**
  - `run_batch(items)` gibt Ergebnisse erst zurueck, nachdem der gesamte Batch abgeschlossen ist
  - Die Engine sortiert zurueckgegebene Ergebnisse nach `cand_id` fuer deterministische Verarbeitung
  - Selbst wenn der Worker-Pool **persistente Threads** verwendet:
    - Der Ausfuehrungskontext (ExecCtx), der pro Batch uebergeben wird, ist **nur waehrend des jeweiligen run_batch-Aufrufs gueltig**
    - Der Executor/die Worker **duerfen keine Referenzen/Pointer auf ExecCtx ueber den Batch hinaus halten**
- Determinismus (Reproduzierbarkeit) Einschraenkungen:
  - Der Executor darf Tasks nicht basierend auf Wall-Clock-Zeit auswaehlen/umordnen
  - Cancellation erschwert die Ergebnis-Reproduzierbarkeit; die Standardempfehlung lautet "kein Intra-Batch-Cancellation"

Die Engine ruft `executor.configure_params(ExecutorParams{..})` vor dem Batch-Dispatch auf, um Performance-Parameter zu uebergeben.

- `ExecutorParams.chunk_base`: Obergrenze fuer Tasks, die ein Worker auf einmal aus der globalen Queue zieht
- `ExecutorParams.spin_limit`: Spin-Iterationen, bevor auf die Condvar in der Batch-Barrier zurueckgefallen wird
- `chunk_base` kann online basierend auf der Batch-Kosten-Varianz (CV) automatisch angepasst werden, wenn `EngineConfig.executor_chunk_auto_tune=true`
- Diese Parameter **duerfen die Korrektheit nicht beeinflussen** und muessen unabhaengig von Cache-Keys / Accept-Regeln sein

Die Engine ruft ausserdem `executor.configure(W)` zu Beginn von `run(..., workers=W)` auf, um den Executor mit der Worker-Anzahl zu synchronisieren.

- `Executor::configure(&mut self, W)` kann davon ausgehen, dass der Aufruf **nur vor Beginn des Batch-Dispatch** erfolgt
- Die Verwendung von `configured(W)` (Owned/Builder-Stil) ist in Ordnung, wenn dies bequemer ist

Die Engine kann zusaetzlich `Executor::configure_params(ExecutorParams)` aufrufen, um Performance-Parameter zu uebergeben.

- Im aktuellen Skeleton bildet `EngineConfig.executor_chunk_base` auf `ExecutorParams.chunk_base` ab
- `chunk_base` ist die Obergrenze fuer Tasks, die aus der globalen Queue in den lokalen Ring gezogen werden
  - Da es kein Work-Stealing gibt, wird der effektive Chunk zusaetzlich durch `ceil(batch_size / W)` begrenzt
- Dieser Wert **beeinflusst nur die Performance** und darf Korrektheit oder Determinismus nicht beeintraechtigen

---

## 5.6) Run-Global Resume Configuration Contract

Das Setzen von `EngineConfig.max_steps_per_iter` auf `Some(k)` bewirkt, dass pro Iteration nur `k` `WorkItem`s ausgefuehrt werden;
verbleibende Ready-Kandidaten **werden in der naechsten Iteration fortgesetzt**.

- `None`: Alle Ready-Kandidaten pro Iteration abarbeiten (v3-Verhalten)
- `Some(k)`: **Erzeugt einen Resume-Pfad**, wodurch Fairness-Policies (Anti-Starvation) wichtig werden
  - z. B. `audit_required`-Kandidaten priorisieren, altersbasierte Scoring-Boni hinzufuegen

---

## 5.7) AnisotropicMeshGeometry Contract

Wenn `EngineConfig.mesh_base_steps` auf `Some(steps)` gesetzt ist, verwendet die Engine dimensionsweise
Mesh-Schrittgroessen anstelle einer einzelnen skalaren Schrittgroesse.

- `base_steps.len()` muss der Suchdimension entsprechen; eine Abweichung ist ein Konfigurationsfehler
- `mesh_muls` verfolgt dimensionsweise Mesh-Multiplikatoren unabhaengig
- `SearchContext::mesh_steps` (Plural) ersetzt das fruehere skalare `mesh_step`
  - Der rueckwaertskompatible `mesh_step()`-Accessor gibt `mesh_steps[0]` zurueck
- Alle Mesh-Quantisierungen in Search und Poll muessen die dimensionsweise Schrittgroesse fuer die
  entsprechende Koordinate verwenden
- `env_rev_with_steps()` muss den `base_steps`-Vektor in den Cache-Key-Hash einschliessen, damit
  verschiedene anisotrope Konfigurationen unterschiedliche Cache-Keys erzeugen
- Poll/Mesh-Update-Regeln gelten dimensionsweise: Jede Dimension verfeinert oder vergroebert ihren eigenen
  Mesh-Multiplikator unabhaengig

---

## 5.8) AcceptancePolicy Contract

`AcceptancePolicy` ist ein oeffentlicher Trait (zuvor als `AcceptanceEngine` versiegelt).
`DefaultAcceptance` implementiert ihn und bleibt der Standard.

- Die Akzeptanz-Policy empfaengt nur TRUTH-Ergebnisse; sie darf niemals PARTIAL-Ergebnisse akzeptieren
- `AcceptancePolicy::accept(candidate, filter, barrier)` muss bei gleichen Eingaben eine deterministische
  Accept/Reject-Entscheidung zurueckgeben
- Benutzerdefinierte Implementierungen (z. B. Pareto-basiert fuer Multi-Objective) muessen erfuellen:
  - Filter-Dominanz: Ein Kandidat wird nur akzeptiert, wenn er nicht vom aktuellen
    Filterinhalt dominiert wird (oder die Barriere verbessert)
  - Barriere-Monotonie: Der progressive Barriere-Schwellenwert darf nicht steigen
  - Determinismus: Die Entscheidung darf nicht von Wall-Clock-Zeit, Thread-Reihenfolge oder
    OS-Zufaelligkeit abhaengen
- Wenn eine benutzerdefinierte `AcceptancePolicy` ihren internen Zustand aendert (z. B. Pareto-Front-
  Zugehoerigkeit), muss `policy_rev` inkrementiert werden

---

## 6) DIDS Policy (DidsPolicy) Contract

- `a` (Assignment-Vektor) ist ein **Early-Stop-Effizienzwerkzeug**
- Darf die Machbarkeitsbestaetigung / Accept-Regeln nicht aendern (nur TRUTH)
- `a`-Updates erfolgen **nur an Batch-Grenzen**

---

## 7) Margin / Calibrator / Audit Policy Contract

### 7.1 Early Infeasible muss konservativ sein
- Die Unterdrueckung falscher Infeasibles hat Vorrang
- Grenzpunkte werden ueber Audit/Promotion korrigiert

### 7.2 Calibrator-Updates erfolgen nur an Batch-Grenzen
- Eingabe-Ereignisse werden nur als sortierte Listen empfangen
- Wenn ein Update die Policy aendert, muss `policy_rev` inkrementiert werden

### 7.3 Calibrator-Parameter werden ueber EngineConfig bereitgestellt
- Ziel-Quote fuer falsche Infeasibles (`calibrator_target_false`), minimale Audit-Samples (`calibrator_min_audits`),
  Update-Schrittweite (`calibrator_eta_delta`), Clamp-Bereich (`calibrator_delta_min/max`) muessen alle
  ueber `EngineConfig` konfigurierbar sein.

---

## 8) Cache (EvalCache / DecisionCache) Contract

- Caching ist eine **Performance-Optimierung**; ein Cache-Miss darf die Korrektheit niemals veraendern
- Cache-Key-Komponenten duerfen nicht veraendert werden:
  - EvalCacheKey: `(x_mesh, phi, env_rev)`
  - DecisionCacheKey: `(x_mesh, phi, env_rev, policy_rev, tag)`
- Wenn Policy-Zustand (δ/K/a etc.) Ergebnisse beeinflusst, **muss `policy_rev` erhoeht werden**, um DecisionCache-Kontamination zu verhindern

---

## 9) Erforderliche Tests (Custom Bundle Verification)

Wenn Sie ein benutzerdefiniertes PolicyBundle hinzufuegen, wird das Bestehen der folgenden Tests dringend empfohlen:

1. **Determinism Replay**: Wiederholte Durchlaeufe mit identischen Eingaben erzeugen identische Ergebnisse
2. **Completion Event Order Independence**: Unterschiedliche Abschlussreihenfolgen liefern identische Ergebnisse
3. **Cache Consistency**: Ergebnisse sind identisch mit Cache ein/aus (warm/cold)
4. (Falls moeglich) Verifikation im Stil eines **Reorderable Call Multiset**

---

## 10) Zusammenfassung der Anpassungsmoeglichkeiten

Sicher anpassbar:
- SchedulerPolicy / SearchPolicy (inkl. StratifiedSearch) / LadderPolicy / DidsPolicy
- MarginPolicy / CalibratorPolicy / AuditPolicy
- AcceptancePolicy (z. B. Pareto-basiert fuer Multi-Objective)
- CacheBackend / Telemetry

Bedingt (fortgeschritten):
- Solver Warm-Start / Solver-interne Stop-Resume-Policies
- Erweiterung des PARTIAL-Ergebnis-Nutzungsbereichs (Akzeptieren wird nicht empfohlen)
- AnisotropicMeshGeometry (dimensionsweise Schritte ueber EngineConfig.mesh_base_steps)

Standardmaessig versiegelt (konvergenz-/korrektheitskritisch):
- Poll/Mesh-Update-Regeln

## 7.4 Objective Pruning Contract

- Objective Pruning ist lediglich ein **Kandidaten-Promotion-Stopp**; es darf die finale Accept/Reject-Semantik nicht veraendern
- `audit_required`-Kandidaten muessen bei Bedarf das Pruning umgehen koennen
- Das Pruning-Gate muss ueber `EngineConfig`/Presets konfigurierbar und deterministisch sein
- Empfohlene Parameter:
  - `objective_prune_min_smc_rank`
  - `objective_prune_min_level`
  - `objective_prune_require_back_half`
  - `objective_prune_disable_for_audit`
