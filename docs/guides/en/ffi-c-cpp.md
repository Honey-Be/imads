# C/C++ FFI Guide

## Building

```bash
cargo build -p imads-ffi --release
```

This produces:
- `target/release/libimads_ffi.so` (Linux) / `.dylib` (macOS) / `.dll` (Windows)
- `target/release/libimads_ffi.a` (static)

The C header is at `imads-ffi/include/imads.h`.

## Basic Usage (C)

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

Compile:
```bash
gcc example.c -L target/release -limads_ffi -lpthread -ldl -lm -o example
```

## Custom Evaluator

Implement the `ImadsEvaluatorVTable` with your objective/constraint functions:

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
    .search_dim = 0,        // 0 = infer from config or incumbent
    .user_data = NULL,
};

ImadsOutput out = imads_engine_run_with_evaluator(engine, cfg, &env, 4, vtable);
```

Set `search_dim` to 0 to let the engine infer from config or incumbent.

## Multi-Objective C API

For multi-objective problems, use the dedicated multi-objective API. Enable fixed
objective counts with feature flags for zero-allocation hot paths:

```bash
# Dynamic objective count (default)
cargo build -p imads-ffi --release

# Fixed 3-objective evaluators (compile-time optimization)
cargo build -p imads-ffi --release --features fixed-n3

# Fixed 8-objective evaluators
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
    size_t search_dim;       // 0 = infer from config or incumbent
    uint8_t *user_data;
} ImadsMultiEvaluatorVTable;
```

`mc_sample` writes `num_objectives` values into `f_out` (instead of a single `double`)
and `m` constraint values into `c_out`.

### ImadsMultiOutput

```c
typedef struct {
    const double *f_best_ptr;   // array of num_objectives values, or NULL
    size_t        f_best_len;   // num_objectives when valid, 0 otherwise
    int           f_best_valid; // non-zero if an optimum was found
    const int64_t *x_best_ptr;
    size_t         x_best_len;
    uint64_t truth_evals;
    uint64_t partial_steps;
} ImadsMultiOutput;
```

### Running a multi-objective optimization

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

### Feature Flags

| Flag | Effect |
|------|--------|
| `fixed-n3` | Compiles with `num_objectives` fixed at 3; avoids heap allocation for `f_out` |
| `fixed-n8` | Compiles with `num_objectives` fixed at 8 |

When a `fixed-n*` flag is active, `ImadsMultiEvaluatorVTable.num_objectives` is
compile-time checked to match. Mismatched values cause a runtime panic at engine
construction.

## Thread Safety

The engine handle is not thread-safe. Do not call `imads_engine_run` concurrently
on the same engine. The internal executor manages its own worker threads.
