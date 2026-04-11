# C/C++ FFI 가이드

## 빌드

```bash
cargo build -p imads-ffi --release
```

위 명령을 실행하면 다음 파일이 생성됩니다:
- `target/release/libimads_ffi.so` (Linux) / `.dylib` (macOS) / `.dll` (Windows)
- `target/release/libimads_ffi.a` (정적 라이브러리)

C 헤더 파일은 `imads-ffi/include/imads.h`에 위치합니다.

## 기본 사용법 (C)

```c
#include "imads.h"
#include <stdio.h>

int main() {
    ImadsConfig *cfg = imads_config_from_preset("balanced");
    if (!cfg) return 1;

    ImadsEnv env = { .run_id = 1, .config_hash = 2,
                     .data_snapshot_id = 3, .rng_master_seed = 4 };

    ImadsEngine *engine = imads_engine_new();
    ImadsOutput out = imads_engine_run(engine, cfg, &env, 4);

    if (out.f_best_valid) {
        printf("f_best = %f\n", out.f_best);
        printf("x_best = [");
        for (size_t i = 0; i < out.x_best_len; i++)
            printf("%s%ld", i ? ", " : "", out.x_best_ptr[i]);
        printf("]\n");
    }

    imads_output_free(&out);
    imads_engine_free(engine);
    imads_config_free(cfg);
    return 0;
}
```

컴파일 명령:
```bash
gcc example.c -L target/release -limads_ffi -lpthread -ldl -lm -o example
```

## 사용자 정의 Evaluator

목적 함수 및 제약 조건 함수를 포함하는 `ImadsEvaluatorVTable`을 구현하십시오:

```c
void my_mc_sample(const double *x, size_t dim,
                  uint64_t tau, uint32_t smc, uint32_t k,
                  double *f_out, double *c_out, size_t m,
                  uint8_t *user_data) {
    double sum_sq = 0;
    for (size_t i = 0; i < dim; i++) sum_sq += x[i] * x[i];
    *f_out = sum_sq;
    for (size_t j = 0; j < m; j++) c_out[j] = 0.0;
}

ImadsEvaluatorVTable vtable = {
    .cheap_constraints = NULL,  // optional
    .mc_sample = my_mc_sample,
    .num_constraints = 2,
    .search_dim = 0,        // 0 = config 또는 incumbent에서 추론
    .user_data = NULL,
};

ImadsOutput out = imads_engine_run_with_evaluator(engine, cfg, &env, 4, vtable);
```

`search_dim`을 0으로 설정하면 엔진이 config 또는 incumbent에서 차원을 추론합니다.

## 다목적 C API

다목적 최적화 문제에는 전용 다목적 API를 사용하십시오. 제로 할당 핫 패스를 위해
피처 플래그로 고정 목적 함수 개수를 활성화할 수 있습니다:

```bash
# 동적 목적 함수 개수 (기본값)
cargo build -p imads-ffi --release

# 고정 3-목적 evaluator (컴파일 타임 최적화)
cargo build -p imads-ffi --release --features fixed-n3

# 고정 8-목적 evaluator
cargo build -p imads-ffi --release --features fixed-n8
```

### ImadsMultiEvaluatorVTable

```c
typedef struct {
    void (*mc_sample)(const double *x, size_t dim,
                      uint64_t tau, uint32_t smc, uint32_t k,
                      double *f_out, size_t num_objectives,
                      double *c_out, size_t m,
                      uint8_t *user_data);
    int (*cheap_constraints)(const double *x, size_t dim, uint8_t *user_data);
    size_t num_constraints;
    size_t num_objectives;
    size_t search_dim;       // 0 = config 또는 incumbent에서 추론
    uint8_t *user_data;
} ImadsMultiEvaluatorVTable;
```

`mc_sample`은 단일 `double` 대신 `num_objectives`개의 값을 `f_out`에 쓰고,
`m`개의 제약 조건 값을 `c_out`에 씁니다.

### ImadsMultiOutput

```c
typedef struct {
    const double *f_best_ptr;   // num_objectives개의 값 배열, 또는 NULL
    size_t        f_best_len;   // 유효할 때 num_objectives, 그렇지 않으면 0
    int           f_best_valid; // 최적값이 발견되면 0이 아닌 값
    const int64_t *x_best_ptr;
    size_t         x_best_len;
    uint64_t truth_evals;
    uint64_t partial_steps;
} ImadsMultiOutput;
```

### 다목적 최적화 실행

```c
ImadsMultiEvaluatorVTable vtable = {
    .mc_sample = my_multi_mc_sample,
    .cheap_constraints = NULL,
    .num_constraints = 2,
    .num_objectives = 3,
    .search_dim = 0,
    .user_data = NULL,
};

ImadsMultiOutput out = imads_engine_run_multi(engine, cfg, &env, 4, vtable);

if (out.f_best_valid) {
    printf("f_best = [");
    for (size_t i = 0; i < out.f_best_len; i++)
        printf("%s%f", i ? ", " : "", out.f_best_ptr[i]);
    printf("]\n");
}

imads_multi_output_free(&out);
```

### 피처 플래그

| 플래그 | 효과 |
|--------|------|
| `fixed-n3` | `num_objectives`를 3으로 고정하여 컴파일; `f_out`에 대한 힙 할당을 방지합니다 |
| `fixed-n8` | `num_objectives`를 8로 고정하여 컴파일합니다 |

`fixed-n*` 플래그가 활성화된 경우, `ImadsMultiEvaluatorVTable.num_objectives`는
컴파일 타임에 일치 여부가 검사됩니다. 불일치하는 값은 엔진 생성 시 런타임 패닉을
발생시킵니다.

## 스레드 안전성

엔진 핸들은 스레드에 안전하지 않습니다. 동일한 엔진에 대해 `imads_engine_run`을 동시에 호출하지 마십시오. 내부 executor가 자체적으로 워커 스레드를 관리합니다.
