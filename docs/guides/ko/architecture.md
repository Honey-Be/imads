# 아키텍처 개요

## Engine과 PolicyBundle

엔진(`Engine<P: PolicyBundle>`)은 플러그 가능한 정책 표면을 통해 최적화를 조율합니다. 각 정책 슬롯은 `PolicyBundle`의 연관 타입(associated type)으로 정의됩니다.

| Policy | 역할 | 커스터마이징 가능 여부 |
|--------|------|:---:|
| `SchedulerPolicy` | 배치 디스패치 순서 결정 | Yes |
| `SearchPolicy` | 후보 생성 및 스코어링 | Yes |
| `LadderPolicy` | (tau, S) 충실도 래더 구성 | Yes |
| `DidsPolicy` | 동적 비실행 가능도 전략 | Yes |
| `MarginPolicy` | 조기 비실행 가능/목적 함수 임계값 | Yes |
| `CalibratorPolicy` | 델타 컨트롤러 및 K-학습 | Yes |
| `AuditPolicy` | 해시 기반 감사 선택 | Yes |
| `AcceptancePolicy` | 필터 + 점진적 장벽 수락 | Yes |
| `EvalCacheBackend` | 추정값 캐시 | Yes |
| `DecisionCacheBackend` | 의사결정 캐시 | Yes |
| `Executor` | 작업 배치 실행 | Yes |

**봉인됨 (커스터마이징 불가):**
- 폴/메시 업데이트(`DefaultPoll`) — 수렴에 핵심적인 요소

> **참고:** `AcceptancePolicy`는 이전에 `AcceptanceEngine`으로 봉인되어 있었습니다. 현재는
> 공개 트레이트입니다. `DefaultAcceptance`가 `AcceptancePolicy`를 구현하며 기본값으로 유지됩니다.
> 사용자는 커스텀 수락 정책을 구현할 수 있습니다 (예: 다목적 최적화를 위한 Pareto 기반).

## AdaptiveExecutor

`DefaultBundle`은 `AdaptiveExecutor`를 사용하며, 다음과 같이 자동 선택됩니다:

- **workers = 1** → `InlineExecutor` (순차 실행, 오버헤드 없음)
- **workers > 1** → `WorkerPoolExecutor` (배치 배리어를 갖춘 스레드 풀)

스레드를 지원하지 않는 WASM 타겟에서는 `InlineExecutor`만 사용 가능합니다.
`wasm32-wasip1-threads` 및 `wasm32-wasip3`에서는 풀 변형이 활성화됩니다.

### Evaluator 트레이트

`Evaluator` 트레이트는 블랙박스 인터페이스를 정의합니다:

| 메서드 / 타입 | 필수 여부 | 설명 |
|---------------|----------|-------------|
| `type Objectives: ObjectiveValues` | 예 | 목적 함수 값의 연관 타입 (f64, [f64;N], 또는 Vec<f64>) |
| `mc_sample(x, phi, env, k)` | 예 | 목적 함수 + 제약 조건의 결정론적 MC 샘플 |
| `cheap_constraints(x, env)` | 아니요 | 빠른 거부 게이트 (기본값: 모두 수락) |
| `solver_bias(x, tau, env)` | 아니요 | Tau 의존 편향 항 (기본값: 0) |
| `num_constraints()` | 예 | 제약 조건 값의 수 |
| `num_objectives()` | 예 | 목적 함수 값의 수 (단일 목적일 경우 1) |
| `search_dim()` | 아니요 | 탐색 공간 차원; `Some(d)`일 때 `EngineConfig.search_dim`을 오버라이드 |

엔진은 차원을 다음 순서로 결정합니다: `config.search_dim` > `evaluator.search_dim()` > 현재 해의 길이 > 폴백 값 1.

### ObjectiveValues와 다목적 최적화 지원

`ObjectiveValues` 트레이트는 단일 목적 및 다목적 evaluator를 추상화합니다. `f64`(단일 목적),
`[f64; N]`(고정 개수), `Vec<f64>`(동적 개수)에 대해 구현되어 있습니다.

- `Estimates.f_hat`과 `f_se`는 `Vec<f64>`입니다 (목적 함수당 하나의 항목).
- `Estimates.num_objectives`는 개수를 보고합니다.
- `JobResult::Truth.f`는 `Vec<f64>`입니다.
- `EngineOutput.f_best`는 `Option<Vec<f64>>`입니다.

마커 서브 트레이트 `SingleObjectiveEvaluator`는 `Objectives = f64`인 모든 evaluator에 대한
블랭킷 구현을 가지고 있어 하위 호환성을 유지합니다.

### EvaluatorErased

`EvaluatorErased`는 엔진 내부에서 사용되는 타입 소거 트레이트 오브젝트 래퍼입니다.
제네릭 감염을 방지합니다: 엔진 코어는 구체적인 evaluator 타입으로 파라미터화되지 않고
`&dyn EvaluatorErased`에 대해 동작합니다. 사용자 코드에서는 이 트레이트와 직접 상호작용할
필요가 없습니다.

## StratifiedSearch

`StratifiedSearch`는 `DefaultSearch`의 드롭인 대체재입니다 (`PolicyBundle::Search`를 통해 사용).
`imads-core/src/policies/stratified_search.rs`에 정의되어 있으며 세 가지 후보 생성 모드를
결합합니다:

1. **좌표 스텝** — 메시에 정렬된 교란으로 최대 `min(dim, 6)`개의 좌표 방향을 폴링합니다.
2. **방향 탐색** — 최근 성공한 스텝에서 도출된 개선 벡터를 따라 외삽합니다.
3. **Halton 준난수 전역 탐색** — 전역 커버리지를 위해 전체 탐색 공간에 걸쳐 저불일치
   점을 생성합니다.

이 모드 간의 할당 비율은 문제의 차원에 따라 동적으로 조정됩니다:

| 차원 | 좌표 | 방향 | Halton |
|:----:|:----:|:----:|:------:|
| dim <= 8       | 60%        | 20%         | 20%    |
| 8 < dim < 32   | 45%        | 25%         | 30%    |
| dim >= 32      | 30%        | 30%         | 40%    |

고차원 문제에서는 좌표 스테핑의 효율이 점점 떨어지므로 전역 탐색 예산이 더 많이
할당됩니다.

## AnisotropicMeshGeometry

`AnisotropicMeshGeometry`는 **차원별 메시 스텝 크기**를 가능하게 합니다. 모든 차원이
공유하는 단일 스칼라 메시 스텝 대신, 각 차원이 자체적인 `base_step`과 `mesh_mul`을
가집니다:

- `base_steps: Vec<f64>` — 차원별 초기 스텝 크기.
- `mesh_muls: Vec<f64>` — 차원별 메시 크기 배율.

`EngineConfig`에는 이제 `mesh_base_steps: Option<Vec<f64>>`가 포함됩니다. `Some(steps)`일 때,
엔진은 기본 등방성 기하 대신 `AnisotropicMeshGeometry`를 구성합니다.

`SearchContext::mesh_step`은 `mesh_steps: Vec<f64>` (차원별 스텝의 벡터)로 대체되었습니다.
하위 호환성을 위한 `mesh_step()` 접근자는 등방성 메시를 가정하는 코드를 위해 첫 번째
요소를 반환합니다.

`env_rev_with_steps()` 함수는 `base_steps`를 캐시 키 해시에 포함하여, 서로 다른 이방성
구성이 평가 캐시에서 충돌하지 않도록 보장합니다.

## 3단계 의사결정 흐름

1. **Stage A (저비용)** — `Evaluator::cheap_constraints()`. 블랙박스 평가 없이 거부합니다.
2. **PARTIAL** — 중간 (tau, S) 충실도. 조기 비실행 가능 판정 또는 중단이 발생할 수 있습니다.
3. **TRUTH** — 최고 충실도에서의 최종 평가. TRUTH만이 필터에 수락될 수 있습니다.

## 충실도 래더

2축 래더는 `tau_levels`(허용 오차, 느슨함→엄격함)와 `smc_levels`(몬테카를로 샘플 수, 적음→많음)로 정의됩니다. `LadderPolicy`는 이들을 `Phi = (Tau, Smc)` 단계의 정렬된 시퀀스로 결합합니다. MC 접두사 재사용을 통해 i단계의 샘플이 i+1단계에서 재사용됩니다.

## 결정론 계약

모든 정책 결정은 (inputs, env_rev, policy_rev)의 순수 함수입니다. 의사결정 경로에 벽시계 시간, 스레드 경쟁, OS 난수가 개입하지 않습니다. 이를 통해 다음이 보장됩니다:
- 머신 간 재현 가능한 실행
- 1-worker와 N-worker가 동일한 결과를 생성
- 결정론적 키를 통한 캐시 정합성

## 캘리브레이터 피드백 루프

캘리브레이터는 다음을 추적합니다:
- 제약 조건별, 충실도 수준별 거짓 비실행 가능 비율
- 쌍을 이룬 감사 샘플로부터 학습된 K (편향 한계)
- 목표 거짓 비율을 향해 EWMA로 조정되는 델타 임계값

업데이트는 배치 경계에서 결정론적 순서로 수행됩니다.
