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
| `EvalCacheBackend` | 추정값 캐시 | Yes |
| `DecisionCacheBackend` | 의사결정 캐시 | Yes |
| `Executor` | 작업 배치 실행 | Yes |

**봉인됨 (커스터마이징 불가):**
- 폴/메시 업데이트(`DefaultPoll`) — 수렴에 핵심적인 요소
- 수락 로직(`DefaultAcceptance`) — 필터 + 점진적 장벽

## AdaptiveExecutor

`DefaultBundle`은 `AdaptiveExecutor`를 사용하며, 다음과 같이 자동 선택됩니다:

- **workers = 1** → `InlineExecutor` (순차 실행, 오버헤드 없음)
- **workers > 1** → `WorkerPoolExecutor` (배치 배리어를 갖춘 스레드 풀)

스레드를 지원하지 않는 WASM 타겟에서는 `InlineExecutor`만 사용 가능합니다.
`wasm32-wasip1-threads` 및 `wasm32-wasip3`에서는 풀 변형이 활성화됩니다.

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
