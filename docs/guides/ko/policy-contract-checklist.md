# Policy 계약 체크리스트

이 문서는 **통합 MADS 프레임워크**의 Policy 레이어를 패치셋이나 플러그인 형태로 커스터마이즈할 때 반드시 지켜야 하는 **계약(contracts)** 을 기술합니다.

> 목표: **정확성(정합성), 재현성(결정성), 캐시 일관성**을 유지하면서, 스케줄링/서치/통계 정책을 안전하게 교체 가능하게 만들기.

---

## 1) 절대 불변(기본 봉인) 규칙

다음은 기본적으로 **커스터마이즈 대상이 아니며**, 위반하면 수렴성, 정합성, 또는 결정성이 쉽게 깨집니다.

- **TRUTH에서만 최종 accept/reject**
  - `PARTIAL` 결과는 **우선순위/프루닝 힌트**로만 사용.
  - feasible 확정 또는 filter 삽입은 TRUTH(τ_L, S_L)에서만.
- **Poll/mesh 업데이트 규칙 봉인**
  - MADS 수렴 이론의 핵심.
- **캐시 키 구성요소 변경 금지**
  - **EvalCache(비싼 평가 산출물)** 최소 키: `(x_mesh, phi=(tau,S), env_rev)`
  - **DecisionCache(정책 의존 판정/결과)** 최소 키: `(x_mesh, phi=(tau,S), env_rev, policy_rev, tag)`

---

## 2) 결정성(Determinism) 계약

아래 조건이 깨지면 *reorderably deterministic* 요구(또는 그 상위 개념의 재현성 보장)가 붕괴합니다.

### 2.1 정책 함수는 순수해야 함
- 동일 입력 → 동일 출력
- 다음을 직접 사용 금지(또는 **Env로 승격**):
  - wall-clock time
  - OS randomness
  - 스레드 레이스에 의존하는 전역 상태

### 2.2 audit 선택은 **반드시 결정적**이어야 함
- 권장: `hash(x_mesh, phi, env_rev, ...)` 기반 모듈러 선택
- 금지: `rand::thread_rng()` 같은 실행 시 난수

### 2.3 배치 경계 업데이트는 **결정적 순서**로만
- 비동기 완료 순서는 런타임에 따라 바뀜
- 따라서 엔진은 정책에 전달하는 이벤트를 정렬해야 함(권장 키):
  - `(candidate_id, phi, tag)` lexicographic

---

## 3) (τ, S) 2축 Fidelity ladder 계약

- ladder는 **단조 정밀도 정제**를 만족해야 함
  - 일반적으로 τ는 감소(더 엄격), S는 증가
- MC는 **prefix 재사용**을 지원해야 함
  - S가 증가할 때 기존 샘플 1..S_i를 그대로 재사용
- ladder가 바뀌면 `policy_rev`를 증가시키고 캐시 키에 반영

---

## 4) 스케줄러(SchedulerPolicy) 계약

- 스케줄러는 **효율을 바꾸되 정합성을 바꾸면 안 됨**
- 금지/주의:
  - 시간 기반("N초가 지났으니…") 의사결정
  - OS/스레드 스케줄에 따라 달라지는 비결정적 선택
- 권장:
  - `W`(워커 수)는 "동시 실행 한도/배치 크기"에만 반영
  - 후보 선택은 입력 리스트에 대해 결정적으로

---

## 5) SearchPolicy 계약

- Search는 자유지만, **최종 제출은 mesh 위로 quantize**되어야 함
- 중복 후보를 줄일수록 캐시 효율이 증가
- 점수/우선순위는 결정적이어야 함

### 5.1) StratifiedSearch 계약

`StratifiedSearch`는 좌표 스텝, 방향 탐색, Halton 준난수 탐색의 세 가지 후보 생성
모드를 결합한 내장 `SearchPolicy`입니다.

- 모드 할당 비율은 엔진 생성 시 차원에 따라 결정되며, 실행 도중 변경되면
  안 됩니다 (변경 시 결정성이 깨집니다)
- 좌표 스텝은 반복당 최대 `min(dim, 6)` 방향을 폴링해야 합니다
- 방향 탐색은 동일한 결정론적 히스토리 윈도우에서 개선 벡터를 도출해야 하며,
  벽시계 시간이나 스레드 완료 순서에 의존하면 안 됩니다
- Halton 포인트는 `env_rev`에 연결된 결정론적 시퀀스 인덱스를 사용해야 합니다
- 생성된 모든 후보는 mesh 위로 quantize되어야 합니다 (모든 `SearchPolicy`와 동일)

---

## 5.5) Executor(배치 실행기) 계약

실제 병렬/비동기 런타임을 넣고 싶다면 `Executor`를 교체하게 됩니다.

- Executor는 **평가 실행만** 담당
  - `(WorkItem) -> Estimates/캐시 히트` 수준의 일만 수행
  - **조기 중단/accept/reject/캘리브레이션** 등 *정책 결정*은 엔진 스레드에서만
- **배치 배리어(batch barrier) 규율 유지 권장**
  - `run_batch(items)`는 배치 전체가 끝난 뒤 결과를 반환
  - 엔진은 반환된 결과를 `cand_id`로 정렬하여 결정적으로 처리
  - 워커풀이 **영구 스레드(persistent threads)** 를 유지하더라도,
    - 배치마다 전달되는 실행 컨텍스트(ExecCtx)는 **해당 run_batch 호출 동안에만 유효**
    - Executor/워커는 ExecCtx에 대한 참조/포인터를 **배치 밖으로 보관하면 안 됨**
- 결정성(재현성) 관점에서 금지/주의
  - Executor가 wall-clock 기반으로 작업을 선택/재배치하면 안 됨
  - 취소(cancellation)는 결과 재현성을 어렵게 하므로 기본값은 "배치 내부 취소 없음" 권장

엔진은 배치 디스패치 직전에 `executor.configure_params(ExecutorParams{..})`를 호출하여 성능 파라미터를 전달합니다.

- `ExecutorParams.chunk_base`: 워커가 global queue에서 한 번에 가져오는 작업 묶음 상한
- `ExecutorParams.spin_limit`: 배치 배리어에서 condvar로 넘어가기 전 스핀 반복 횟수
- `chunk_base`는 `EngineConfig.executor_chunk_auto_tune=true`일 때 배치 비용 분산(CV)에 기반해 온라인으로 자동 조정될 수 있습니다
- 위 파라미터들은 **정확성에 영향을 주면 안 되며**, 캐시 키/accept 규칙과 무관해야 합니다

추가로, 엔진은 `run(..., workers=W)` 호출 시 `executor.configure(W)`를 호출하여 실행기와 워커 수를 동기화합니다.

- `Executor::configure(&mut self, W)`는 **배치 디스패치 시작 전**에만 호출됨을 가정해도 됨
- 구현이 편하면 `configured(W)`(owned/builder 스타일)를 사용해도 됨

엔진은 추가로 `Executor::configure_params(ExecutorParams)`를 호출해 성능 파라미터를 전달할 수 있습니다.

- 현재 스켈레톤에서는 `EngineConfig.executor_chunk_base`가 `ExecutorParams.chunk_base`로 전달됩니다
- `chunk_base`는 글로벌 큐에서 로컬 링으로 한 번에 가져오는 작업 수의 상한입니다
  - 단, work-stealing이 없으므로 실제 적용 chunk는 `ceil(batch_size / W)`로 한 번 더 캡됩니다
- 이 값은 **성능만** 바꾸며 결과의 정합성/결정성에는 영향을 주면 안 됩니다

---

## 5.6) Run-global resume 설정 계약

`EngineConfig.max_steps_per_iter`를 `Some(k)`로 두면, 한 iteration에서 `k`개의 `WorkItem`만 실행하고
나머지 Ready 후보는 다음 iteration에서 **resume** 됩니다.

- `None`: 매 iteration마다 Ready 후보를 전부 소진(v3 동작)
- `Some(k)`: **resume 경로가 생기므로** 공정성(anti-starvation) 정책이 중요
  - 예: `audit_required` 후보 우선, 오래된 후보(age) 가산 등

---

## 5.7) AnisotropicMeshGeometry 계약

`EngineConfig.mesh_base_steps`가 `Some(steps)`일 때, 엔진은 단일 스칼라 스텝 대신
차원별 메시 스텝 크기를 사용합니다.

- `base_steps.len()`은 탐색 차원과 일치해야 합니다; 불일치는 구성 오류입니다
- `mesh_muls`는 차원별 메시 배율을 독립적으로 추적합니다
- `SearchContext::mesh_steps` (복수형)이 이전의 스칼라 `mesh_step`을 대체합니다
  - 하위 호환성을 위한 `mesh_step()` 접근자는 `mesh_steps[0]`을 반환합니다
- 검색 및 폴의 모든 메시 양자화는 해당 좌표에 대한 차원별 스텝을 사용해야 합니다
- `env_rev_with_steps()`는 `base_steps` 벡터를 캐시 키 해시에 포함하여, 서로 다른
  이방성 구성이 고유한 캐시 키를 생성하도록 해야 합니다
- 폴/메시 업데이트 규칙은 차원별로 적용됩니다: 각 차원이 자체 메시 배율을
  독립적으로 정제하거나 조대화합니다

---

## 5.8) AcceptancePolicy 계약

`AcceptancePolicy`는 공개 트레이트입니다 (이전에는 `AcceptanceEngine`으로 봉인됨).
`DefaultAcceptance`가 이를 구현하며 기본값으로 유지됩니다.

- 수락 정책은 TRUTH 결과만 받습니다; PARTIAL 결과를 수락해서는 안 됩니다
- `AcceptancePolicy::accept(candidate, filter, barrier)`는 동일한 입력이 주어지면
  결정론적 accept/reject 결정을 반환해야 합니다
- 커스텀 구현(예: 다목적을 위한 Pareto 기반)은 다음을 만족해야 합니다:
  - 필터 지배: 후보는 현재 필터 내용에 의해 지배되지 않을 때만(또는 배리어를
    개선할 때만) 수락됩니다
  - 배리어 단조성: 점진적 배리어 임계값은 증가하면 안 됩니다
  - 결정성: 결정은 벽시계 시간, 스레드 순서, OS 난수에 의존하면 안 됩니다
- 커스텀 `AcceptancePolicy`가 내부 상태(예: Pareto 프런트 멤버십)를 변경할 때,
  `policy_rev`를 증가시켜야 합니다

---

## 6) DIDS 정책(DidsPolicy) 계약

- `a`(assignment vector)는 **조기 중단 효율화 도구**
- feasible 확정/accept 규칙을 바꾸면 안 됨(TRUTH only)
- `a` 업데이트는 **배치 경계**에서만 수행

---

## 7) Margin/Calibrator/Audit 정책 계약

### 7.1 Early infeasible은 보수적이어야 함
- 거짓 infeasible(false infeasible) 억제가 우선
- 경계점은 audit/승격으로 보정

### 7.2 Calibrator 업데이트는 배치 경계에서만
- 입력 이벤트는 정렬된 리스트로만 받기
- 업데이트가 정책을 바꾸면 `policy_rev` 증가

### 7.3 Calibrator 파라미터는 EngineConfig로 노출
- 목표 false infeasible rate(`calibrator_target_false`), 최소 audit 표본(`calibrator_min_audits`),
  업데이트 step(`calibrator_eta_delta`), clamp 범위(`calibrator_delta_min/max`)는
  `EngineConfig`를 통해 조정 가능해야 합니다.

---

## 8) 캐시(EvalCache/DecisionCache) 계약

- 캐시는 **성능 최적화**여야 하며, cache miss가 정확성을 바꾸면 안 됨
- 캐시 키 구성요소 변경 금지
  - EvalCacheKey: `(x_mesh, phi, env_rev)`
  - DecisionCacheKey: `(x_mesh, phi, env_rev, policy_rev, tag)`
- 정책(δ/K/a 등)이 결과에 영향을 주면 **`policy_rev`를 반드시 bump**하여 DecisionCache가 오염되지 않게 할 것

---

## 9) 필수 테스트(커스텀 번들 검증)

커스텀 PolicyBundle을 추가했다면 아래 테스트를 통과시키는 것을 강력히 권장합니다.

1. **결정성 리플레이**: 동일 입력에서 실행을 여러 번 반복해 결과 동일
2. **완료 이벤트 순서 무관성**: completion order가 달라도 결과 동일
3. **캐시 정합성**: 캐시 on/off(또는 warm/cold)에서도 결과 동일
4. (가능하면) **reorderable call multiset** 유사 검증

---

## 10) 커스터마이즈 권장/금지 요약

안전하게 커스터마이즈 가능:
- SchedulerPolicy / SearchPolicy (StratifiedSearch 포함) / LadderPolicy / DidsPolicy
- MarginPolicy / CalibratorPolicy / AuditPolicy
- AcceptancePolicy (예: 다목적을 위한 Pareto 기반)
- CacheBackend / Telemetry

조건부 (고급):
- solver warm-start / solver 내부 중단·재개 정책
- PARTIAL 결과의 활용 범위 확장(accept는 비권장)
- AnisotropicMeshGeometry (EngineConfig.mesh_base_steps를 통한 차원별 스텝)

기본 봉인(수렴/정합성 핵심):
- Poll/mesh 업데이트 규칙

## 7.4 Objective pruning 계약

- objective pruning은 **candidate promotion stop**일 뿐이며, 최종 accept/reject semantics를 바꾸면 안 됨
- `audit_required` 후보는 필요 시 pruning을 우회할 수 있어야 함
- pruning gate는 `EngineConfig`/preset으로 조절 가능하되, 결정적이어야 함
- 권장 파라미터:
  - `objective_prune_min_smc_rank`
  - `objective_prune_min_level`
  - `objective_prune_require_back_half`
  - `objective_prune_disable_for_audit`
