# C/C++ FFI ガイド

## ビルド

```bash
cargo build -p imads-ffi --release
```

以下が生成されます:
- `target/release/libimads_ffi.so` (Linux) / `.dylib` (macOS) / `.dll` (Windows)
- `target/release/libimads_ffi.a` (静的ライブラリ)

C ヘッダーは `imads-ffi/include/imads.h` にあります。

## 基本的な使い方 (C)

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

コンパイル:
```bash
gcc example.c -L target/release -limads_ffi -lpthread -ldl -lm -o example
```

## カスタム Evaluator

`ImadsEvaluatorVTable` に目的関数と制約関数を実装します:

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
    .search_dim = 0,        // 0 = config または incumbent から推論
    .user_data = NULL,
};

ImadsOutput out = imads_engine_run_with_evaluator(engine, cfg, &env, 4, vtable);
```

`search_dim` を 0 に設定すると、エンジンが config または incumbent から次元を推論します。

## 多目的 C API

多目的最適化問題では、専用の多目的 API を使用します。ゼロアロケーションのホットパスのために、
フィーチャーフラグで固定目的関数数を有効にできます:

```bash
# 動的目的関数数（デフォルト）
cargo build -p imads-ffi --release

# 固定 3 目的 evaluator（コンパイル時最適化）
cargo build -p imads-ffi --release --features fixed-n3

# 固定 8 目的 evaluator
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
    size_t search_dim;       // 0 = config または incumbent から推論
    uint8_t *user_data;
} ImadsMultiEvaluatorVTable;
```

`mc_sample` は単一の `double` ではなく、`num_objectives` 個の値を `f_out` に書き込み、
`m` 個の制約値を `c_out` に書き込みます。

### ImadsMultiOutput

```c
typedef struct {
    const double *f_best_ptr;   // num_objectives 個の値の配列、または NULL
    size_t        f_best_len;   // 有効な場合は num_objectives、それ以外は 0
    int           f_best_valid; // 最適解が見つかった場合は非ゼロ
    const int64_t *x_best_ptr;
    size_t         x_best_len;
    uint64_t truth_evals;
    uint64_t partial_steps;
} ImadsMultiOutput;
```

### 多目的最適化の実行

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

### フィーチャーフラグ

| フラグ | 効果 |
|------|--------|
| `fixed-n3` | `num_objectives` を 3 に固定してコンパイルします。`f_out` のヒープアロケーションを回避します |
| `fixed-n8` | `num_objectives` を 8 に固定してコンパイルします |

`fixed-n*` フラグが有効な場合、`ImadsMultiEvaluatorVTable.num_objectives` はコンパイル時に
一致がチェックされます。不一致の値はエンジン構築時にランタイムパニックを引き起こします。

## スレッド安全性

engine ハンドルはスレッドセーフではありません。同一の engine に対して `imads_engine_run` を同時に呼び出さないでください。内部の executor が独自のワーカースレッドを管理します。
