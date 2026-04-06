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
    .user_data = NULL,
};

ImadsOutput out = imads_engine_run_with_evaluator(engine, cfg, &env, 4, vtable);
```

## 스레드 안전성

엔진 핸들은 스레드에 안전하지 않습니다. 동일한 엔진에 대해 `imads_engine_run`을 동시에 호출하지 마십시오. 내부 executor가 자체적으로 워커 스레드를 관리합니다.
