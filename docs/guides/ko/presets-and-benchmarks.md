# 프리셋 및 소규모 벤치마크 워크플로우

이 프로젝트는 현재 `src/presets.rs`에 다섯 가지 프리셋을 제공합니다:

- `legacy_baseline`: 업그레이드5 이전 동작을 근사하는 비교 전용 프리셋
- `balanced`: **권장 기본값**; 더 작은 partial-step 예산으로 throughput 수준의 품질에 도달합니다
- `conservative`: false infeasible 위험이 가장 중요할 때 더 안전하고 신중한 프리셋
- `throughput`: 품질 우선 프리셋; 더 빠른 적응을 위해 더 많은 partial-step 예산을 사용합니다

> **참고:** 모든 프리셋은 이제 `search_dim`을 `None`으로 설정합니다. 엔진은 런타임에 evaluator의 `search_dim()` 메서드를 조회하여 탐색 공간의 차원을 결정합니다.

## 권장 사용법

다음과 같은 목적에 따라 프리셋을 사용하십시오:

- **기본값 / 대부분의 사용자**: `balanced`
- **안전성 / 디버깅 / 노이즈가 많은 평가기**: `conservative`
- **품질 우선 탐색**: `throughput`
- **전후 비교 전용**: `legacy_baseline`

최근 보고서(`imads/reports/preset_report.csv`)에서 다음과 같은 결과가 나타났습니다:

- `balanced`와 `throughput`는 토이 벤치마크에서 동일한 `f_best`에 도달했습니다
- `balanced`는 `throughput`보다 더 적은 partial step으로 이를 달성했습니다
- `conservative`는 기본값으로 사용하기에는 솔루션 품질을 너무 많이 희생했습니다

## Rust 툴체인

Rust 1.94.0으로 테스트를 실행하십시오:

```bash
cargo +1.94.0 test
```

## 소규모 비교 벤치

소규모 비교 벤치를 실행하십시오:

```bash
cargo +1.94.0 bench --bench preset_compare
```

커스텀 벤치 타겟은 각 프리셋에 대해 경과 시간과 주요 엔진 통계를 CSV 형식의 행으로 출력합니다.
명시적인 전후 비교는 현재 다음과 같습니다:

- `legacy_baseline` vs `balanced`

이 쌍은 "이전 동작 vs 권장 기본값"을 비교하는 데 가장 유용합니다.

## 경량 보고서

경량 보고서(단일 실행 타이밍 + 엔진 통계)를 실행하십시오:

```bash
cargo +1.94.0 run --release --example preset_report
```

생성된 CSV를 사용하여 최소한 다음 항목을 비교하십시오:

- `truth_evals`
- `partial_steps`
- `invalid_eval_rejects`
- `f_best`

좋은 기본 프리셋은 `partial_steps`를 `throughput`보다 충분히 낮게 유지하면서 `f_best` 개선의 대부분을 보존해야 합니다.


## Objective pruning 매개변수

Objective pruning은 `EngineConfig`와 프리셋을 통해 구성할 수 있습니다. 현재 프리셋은 다음 게이트를 사용하여 balanced/throughput/conservative 동작을 구분합니다:

- `objective_prune_min_smc_rank`: 도달해야 하는 고유 SMC 수준 중 1 기반 순위
- `objective_prune_min_level`: pruning이 트리거되기 전에 필요한 최소 1 기반 래더 레벨
- `objective_prune_require_back_half`: true인 경우, pruning이 래더의 후반부로 추가 제한됩니다
- `objective_prune_disable_for_audit`: true인 경우, 감사가 필요한 후보는 objective pruning을 우회합니다

권장 해석:

- `balanced`: 2번째 SMC 순위와 레벨 2에서 시작하는 중간 수준의 pruning
- `throughput`: 1번째 SMC 순위에서 시작하는 조기 pruning
- `conservative`: 후반부에서만 시작하는 지연된 pruning
