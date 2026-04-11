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

## Multi-Objective C API

Fuer Multi-Objective-Probleme verwenden Sie die dedizierte Multi-Objective-API. Aktivieren Sie
feste Zielfunktionsanzahlen mit Feature-Flags fuer allokationsfreie Hot Paths:

```bash
# Dynamische Zielfunktionsanzahl (Standard)
cargo build -p imads-ffi --release

# Feste 3-Zielfunktions-Evaluatoren (Compile-Zeit-Optimierung)
cargo build -p imads-ffi --release --features fixed-n3

# Feste 8-Zielfunktions-Evaluatoren
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
    size_t search_dim;       // 0 = aus Config oder Incumbent ableiten
    uint8_t *user_data;
} ImadsMultiEvaluatorVTable;
```

`mc_sample` schreibt `num_objectives` Werte in `f_out` (anstelle eines einzelnen `double`)
und `m` Constraint-Werte in `c_out`.

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

### Ausfuehren einer Multi-Objective-Optimierung

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

### Feature-Flags

| Flag | Wirkung |
|------|---------|
| `fixed-n3` | Kompiliert mit `num_objectives` fest auf 3; vermeidet Heap-Allokation fuer `f_out` |
| `fixed-n8` | Kompiliert mit `num_objectives` fest auf 8 |

Wenn ein `fixed-n*`-Flag aktiv ist, wird `ImadsMultiEvaluatorVTable.num_objectives` zur
Compile-Zeit geprueft. Nicht uebereinstimmende Werte loesen zur Laufzeit einen Panic bei
der Engine-Konstruktion aus.

## Thread Safety

Das Engine-Handle ist nicht thread-safe. Rufen Sie `imads_engine_run` nicht gleichzeitig auf demselben Engine-Objekt auf. Der interne Executor verwaltet seine eigenen Worker-Threads.
