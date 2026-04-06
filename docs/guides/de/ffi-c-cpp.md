# C/C++ FFI Guide

## Erstellen

```bash
cargo build -p imads-ffi --release
```

Dies erzeugt:
- `target/release/libimads_ffi.so` (Linux) / `.dylib` (macOS) / `.dll` (Windows)
- `target/release/libimads_ffi.a` (static)

Der C-Header befindet sich unter `imads-ffi/include/imads.h`.

## Grundlegende Verwendung (C)

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

Kompilieren:
```bash
gcc example.c -L target/release -limads_ffi -lpthread -ldl -lm -o example
```

## Benutzerdefinierter Evaluator

Implementieren Sie die `ImadsEvaluatorVTable` mit Ihren Zielfunktions- und Constraint-Funktionen:

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
    .search_dim = 0,        // 0 = aus Config oder Incumbent ableiten
    .user_data = NULL,
};

ImadsOutput out = imads_engine_run_with_evaluator(engine, cfg, &env, 4, vtable);
```

Setzen Sie `search_dim` auf 0, damit die Engine die Dimension aus der Config oder dem Incumbent ableitet.

## Thread Safety

Das Engine-Handle ist nicht thread-safe. Rufen Sie `imads_engine_run` nicht gleichzeitig auf demselben Engine-Objekt auf. Der interne Executor verwaltet seine eigenen Worker-Threads.
