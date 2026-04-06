# Policy Contract Checklist

Dieses Dokument beschreibt die **Contracts**, die bei der Anpassung der Policy-Schicht des **Integrated MADS Frameworks** als Patch-Sets oder Plugins eingehalten werden müssen.

> Ziel: Sicherer Austausch von Scheduling-, Such- und Statistik-Policies bei gleichzeitiger Wahrung von **Korrektheit, Reproduzierbarkeit (Determinismus) und Cache-Konsistenz**.

---

## 1) Absolute Invarianten (standardmäßig versiegelt)

Die folgenden Punkte sind **keine Anpassungsziele** — ein Verstoß kann leicht Konvergenz, Korrektheit oder Determinismus brechen.

- **Nur TRUTH darf ein endgültiges Accept/Reject erzeugen**
  - `PARTIAL`-Ergebnisse werden ausschließlich als **Prioritäts-/Pruning-Hinweise** verwendet.
  - Machbarkeitsbestätigung oder Filter-Einfügung erfolgt nur bei TRUTH (τ_L, S_L).
- **Poll/Mesh-Update-Regeln sind versiegelt**
  - Kern der MADS-Konvergenztheorie.
- **Cache-Key-Komponenten dürfen nicht verändert werden**
  - **EvalCache (aufwendige Evaluierungsartefakte)** Mindestschlüssel: `(x_mesh, phi=(tau,S), env_rev)`
  - **DecisionCache (Policy-abhängige Entscheidungen/Ergebnisse)** Mindestschlüssel: `(x_mesh, phi=(tau,S), env_rev, policy_rev, tag)`

---

## 2) Determinism Contract

Ein Verstoß gegen eine der folgenden Regeln bricht die Anforderung der *reorderably deterministic* Eigenschaft (oder übergeordnete Reproduzierbarkeitsgarantien).

### 2.1 Policy-Funktionen müssen pur sein
- Gleiche Eingaben → gleiche Ausgaben
- Folgendes darf nicht direkt verwendet werden (oder muss **in Env überführt** werden):
  - Wall-Clock-Zeit
  - Betriebssystem-Zufallszahlen
  - Globaler Zustand, der von Thread-Races abhängt

### 2.2 Audit-Auswahl muss deterministisch sein
- Empfohlen: Modulare Auswahl basierend auf `hash(x_mesh, phi, env_rev, ...)`
- Verboten: Laufzeit-Zufälligkeit wie `rand::thread_rng()`

### 2.3 Batch-Boundary-Updates müssen einer deterministischen Reihenfolge folgen
- Die asynchrone Abschlussreihenfolge variiert zwischen Laufzeitumgebungen
- Die Engine muss Ereignisse sortieren, bevor sie an Policies übergeben werden (empfohlener Schlüssel):
  - `(candidate_id, phi, tag)` lexikographisch

---

## 3) (τ, S) 2-Axis Fidelity Ladder Contract

- Die Ladder muss **monotone Präzisionsverfeinerung** erfüllen
  - Typischerweise nimmt τ ab (enger), S nimmt zu
- MC muss **Prefix-Reuse** unterstützen
  - Wenn S steigt, werden bestehende Samples 1..S_i unverändert wiederverwendet
- Bei Änderung der Ladder muss `policy_rev` inkrementiert und in Cache-Keys widergespiegelt werden

---

## 4) Scheduler (SchedulerPolicy) Contract

- Der Scheduler **darf die Effizienz ändern, aber nicht die Korrektheit**
- Verboten / Vorsicht:
  - Zeitbasierte Entscheidungen („N Sekunden sind vergangen, also …")
  - Nicht-deterministische Entscheidungen, die von OS-/Thread-Scheduling abhängen
- Empfohlen:
  - `W` (Worker-Anzahl) sollte nur „Concurrency-Limit / Batch-Größe" beeinflussen
  - Die Kandidatenauswahl muss über die Eingabeliste deterministisch sein

---

## 5) SearchPolicy Contract

- Die Suche ist frei, aber **finale Einreichungen müssen auf das Mesh quantisiert werden**
- Die Reduzierung doppelter Kandidaten verbessert die Cache-Effizienz
- Scores/Prioritäten müssen deterministisch sein

---

## 5.5) Executor (Batch Runner) Contract

Ersetzen Sie den `Executor`, wenn Sie eine echte parallele/asynchrone Laufzeitumgebung integrieren möchten.

- Der Executor behandelt **ausschließlich die Evaluierungsausführung**
  - Führt nur Arbeit auf der Ebene `(WorkItem) -> Estimates / cache hit` aus
  - **Early Stop / Accept / Reject / Calibration** — alle *Policy-Entscheidungen* erfolgen ausschließlich im Engine-Thread
- **Batch-Barrier-Disziplin wird empfohlen**
  - `run_batch(items)` gibt Ergebnisse erst zurück, nachdem der gesamte Batch abgeschlossen ist
  - Die Engine sortiert zurückgegebene Ergebnisse nach `cand_id` für deterministische Verarbeitung
  - Selbst wenn der Worker-Pool **persistente Threads** verwendet:
    - Der Ausführungskontext (ExecCtx), der pro Batch übergeben wird, ist **nur während des jeweiligen run_batch-Aufrufs gültig**
    - Der Executor/die Worker **dürfen keine Referenzen/Pointer auf ExecCtx über den Batch hinaus halten**
- Determinismus (Reproduzierbarkeit) Einschränkungen:
  - Der Executor darf Tasks nicht basierend auf Wall-Clock-Zeit auswählen/umordnen
  - Cancellation erschwert die Ergebnis-Reproduzierbarkeit; die Standardempfehlung lautet „kein Intra-Batch-Cancellation"

Die Engine ruft `executor.configure_params(ExecutorParams{..})` vor dem Batch-Dispatch auf, um Performance-Parameter zu übergeben.

- `ExecutorParams.chunk_base`: Obergrenze für Tasks, die ein Worker auf einmal aus der globalen Queue zieht
- `ExecutorParams.spin_limit`: Spin-Iterationen, bevor auf die Condvar in der Batch-Barrier zurückgefallen wird
- `chunk_base` kann online basierend auf der Batch-Kosten-Varianz (CV) automatisch angepasst werden, wenn `EngineConfig.executor_chunk_auto_tune=true`
- Diese Parameter **dürfen die Korrektheit nicht beeinflussen** und müssen unabhängig von Cache-Keys / Accept-Regeln sein

Die Engine ruft außerdem `executor.configure(W)` zu Beginn von `run(..., workers=W)` auf, um den Executor mit der Worker-Anzahl zu synchronisieren.

- `Executor::configure(&mut self, W)` kann davon ausgehen, dass der Aufruf **nur vor Beginn des Batch-Dispatch** erfolgt
- Die Verwendung von `configured(W)` (Owned/Builder-Stil) ist in Ordnung, wenn dies bequemer ist

Die Engine kann zusätzlich `Executor::configure_params(ExecutorParams)` aufrufen, um Performance-Parameter zu übergeben.

- Im aktuellen Skeleton bildet `EngineConfig.executor_chunk_base` auf `ExecutorParams.chunk_base` ab
- `chunk_base` ist die Obergrenze für Tasks, die aus der globalen Queue in den lokalen Ring gezogen werden
  - Da es kein Work-Stealing gibt, wird der effektive Chunk zusätzlich durch `ceil(batch_size / W)` begrenzt
- Dieser Wert **beeinflusst nur die Performance** und darf Korrektheit oder Determinismus nicht beeinträchtigen

---

## 5.6) Run-Global Resume Configuration Contract

Das Setzen von `EngineConfig.max_steps_per_iter` auf `Some(k)` bewirkt, dass pro Iteration nur `k` `WorkItem`s ausgeführt werden;
verbleibende Ready-Kandidaten **werden in der nächsten Iteration fortgesetzt**.

- `None`: Alle Ready-Kandidaten pro Iteration abarbeiten (v3-Verhalten)
- `Some(k)`: **Erzeugt einen Resume-Pfad**, wodurch Fairness-Policies (Anti-Starvation) wichtig werden
  - z. B. `audit_required`-Kandidaten priorisieren, altersbasierte Scoring-Boni hinzufügen

---

## 6) DIDS Policy (DidsPolicy) Contract

- `a` (Assignment-Vektor) ist ein **Early-Stop-Effizienzwerkzeug**
- Darf die Machbarkeitsbestätigung / Accept-Regeln nicht ändern (nur TRUTH)
- `a`-Updates erfolgen **nur an Batch-Grenzen**

---

## 7) Margin / Calibrator / Audit Policy Contract

### 7.1 Early Infeasible muss konservativ sein
- Die Unterdrückung falscher Infeasibles hat Vorrang
- Grenzpunkte werden über Audit/Promotion korrigiert

### 7.2 Calibrator-Updates erfolgen nur an Batch-Grenzen
- Eingabe-Ereignisse werden nur als sortierte Listen empfangen
- Wenn ein Update die Policy ändert, muss `policy_rev` inkrementiert werden

### 7.3 Calibrator-Parameter werden über EngineConfig bereitgestellt
- Ziel-Quote für falsche Infeasibles (`calibrator_target_false`), minimale Audit-Samples (`calibrator_min_audits`),
  Update-Schrittweite (`calibrator_eta_delta`), Clamp-Bereich (`calibrator_delta_min/max`) müssen alle
  über `EngineConfig` konfigurierbar sein.

---

## 8) Cache (EvalCache / DecisionCache) Contract

- Caching ist eine **Performance-Optimierung**; ein Cache-Miss darf die Korrektheit niemals verändern
- Cache-Key-Komponenten dürfen nicht verändert werden:
  - EvalCacheKey: `(x_mesh, phi, env_rev)`
  - DecisionCacheKey: `(x_mesh, phi, env_rev, policy_rev, tag)`
- Wenn Policy-Zustand (δ/K/a etc.) Ergebnisse beeinflusst, **muss `policy_rev` erhöht werden**, um DecisionCache-Kontamination zu verhindern

---

## 9) Erforderliche Tests (Custom Bundle Verification)

Wenn Sie ein benutzerdefiniertes PolicyBundle hinzufügen, wird das Bestehen der folgenden Tests dringend empfohlen:

1. **Determinism Replay**: Wiederholte Durchläufe mit identischen Eingaben erzeugen identische Ergebnisse
2. **Completion Event Order Independence**: Unterschiedliche Abschlussreihenfolgen liefern identische Ergebnisse
3. **Cache Consistency**: Ergebnisse sind identisch mit Cache ein/aus (warm/cold)
4. (Falls möglich) Verifikation im Stil eines **Reorderable Call Multiset**

---

## 10) Zusammenfassung der Anpassungsmöglichkeiten

✅ Sicher anpassbar
- SchedulerPolicy / SearchPolicy / LadderPolicy / DidsPolicy
- MarginPolicy / CalibratorPolicy / AuditPolicy
- CacheBackend / Telemetry

⚠️ Bedingt (fortgeschritten)
- Solver Warm-Start / Solver-interne Stop-Resume-Policies
- Erweiterung des PARTIAL-Ergebnis-Nutzungsbereichs (Akzeptieren wird nicht empfohlen)

🚫 Standardmäßig versiegelt (konvergenz-/korrektheitskritisch)
- Poll/Mesh-Update-Regeln
- Filter/Barrier finale Accept/Reject-Regeln

## 7.4 Objective Pruning Contract

- Objective Pruning ist lediglich ein **Kandidaten-Promotion-Stopp**; es darf die finale Accept/Reject-Semantik nicht verändern
- `audit_required`-Kandidaten müssen bei Bedarf das Pruning umgehen können
- Das Pruning-Gate muss über `EngineConfig`/Presets konfigurierbar und deterministisch sein
- Empfohlene Parameter:
  - `objective_prune_min_smc_rank`
  - `objective_prune_min_level`
  - `objective_prune_require_back_half`
  - `objective_prune_disable_for_audit`
