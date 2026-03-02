/*
 * arena_base.h — hacker-lang arena allocator — shared types & API
 *
 * Dwa tryby kompilacji:
 *   arena_compiler.c  (-DHL_COMPILER) — AOT binarka, agresywna optymalizacja
 *   arena_runtime.c   (-DHL_RUNTIME)  — VM/JIT, thread-safe, segment recycling
 *
 * Wspólne cechy obu trybów:
 *   - grow przez mremap/realloc (wskaźniki zostają ważne*)
 *   - guard pages (mmap PROT_NONE) — overflow → SIGSEGV zamiast korupcji
 *   - canary values — wykrywanie nadpisania granic
 *   - stack aren dla rekurencji :: (ARENA_STACK_MAX poziomów)
 *   - szczegółowe komunikaty błędów z plikiem i linią
 *   - arena_reset() — reuse bez free/alloc
 *
 * *mremap na Linux zachowuje adresy jeśli jest miejsce w VA space.
 *  Na innych platformach fallback do memcpy (wskaźniki nieważne po grow!).
 */
#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdatomic.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── tunables ───────────────────────────────────────────────── */
#define ARENA_STACK_MAX      64           /* max zagnieżdżeń ::        */
#define ARENA_ALIGN          16           /* SIMD alignment (SSE/AVX)  */
#define ARENA_GUARD_SIZE     4096         /* guard page size           */
#define ARENA_CANARY         0xDEADC0DEUL
#define ARENA_DEFAULT_CAP    (1u << 20)   /* 1 MB default              */
#define ARENA_MAX_CAP        (1u << 31)   /* 2 GB hard cap per arena   */
#define ARENA_GROW_FACTOR    2            /* podwój przy grow          */

/* ── błędy ──────────────────────────────────────────────────── */
typedef enum {
    ARENA_OK              = 0,
    ARENA_ERR_STACK_FULL  = 1,
    ARENA_ERR_OOM         = 2,
    ARENA_ERR_OVERFLOW    = 3,
    ARENA_ERR_CANARY      = 4,
    ARENA_ERR_UNDERFLOW   = 5,
    ARENA_ERR_NULL        = 6,
    ARENA_ERR_LOCK        = 7,
} ArenaError;

typedef void (*ArenaErrorHandler)(ArenaError err, const char *msg,
                                  const char *file, int line);

/* ── statystyki ─────────────────────────────────────────────── */
typedef struct {
    size_t   total_alloc;   /* łączne bajty zaalokowane      */
    size_t   peak_usage;    /* szczytowe użycie              */
    size_t   grow_count;    /* ile razy arena urosła         */
    size_t   current_cap;   /* aktualna pojemność            */
    size_t   wasted;        /* bajty stracone na alignment   */
    uint32_t depth;         /* poziom zagnieżdżenia ::       */
} ArenaStats;

/* ── public API (wspólne dla obu trybów) ────────────────────── */

/* lifecycle */
void  arena_enter(size_t cap);
void  arena_exit(void);
void  arena_reset(void);

/* alokacja */
void *arena_alloc(size_t size);
void *arena_alloc_zero(size_t size);
char *arena_strdup(const char *s);
void *arena_memdup(const void *ptr, size_t size);

/* unified — arena lub GC zależnie od kontekstu */
void *hl_alloc(size_t size);
void *hl_alloc_zero(size_t size);
char *hl_strdup(const char *s);

/* diagnostyka */
int        arena_active(void);
int        arena_get_depth(void);
size_t     arena_remaining(void);
void       arena_get_stats(ArenaStats *out);
void       arena_print_stats(void);

/* error handler */
void arena_set_error_handler(ArenaErrorHandler h);

/* ── makra pomocnicze ───────────────────────────────────────── */
#define ARENA_NEW(T)        ((T *)arena_alloc(sizeof(T)))
#define ARENA_NEW_Z(T)      ((T *)arena_alloc_zero(sizeof(T)))
#define ARENA_ARRAY(T, n)   ((T *)arena_alloc((n) * sizeof(T)))
#define ARENA_ARRAY_Z(T, n) ((T *)arena_alloc_zero((n) * sizeof(T)))

/* ── compiler hints ─────────────────────────────────────────── */
#if defined(__GNUC__) || defined(__clang__)
#  define HL_LIKELY(x)       __builtin_expect(!!(x), 1)
#  define HL_UNLIKELY(x)     __builtin_expect(!!(x), 0)
#  define HL_INLINE          __attribute__((always_inline)) inline
#  define HL_PREFETCH(p)     __builtin_prefetch((p), 1, 3)
#  define HL_ALIGNED(n)      __attribute__((aligned(n)))
#  define HL_COLD            __attribute__((cold, noinline))
#else
#  define HL_LIKELY(x)       (x)
#  define HL_UNLIKELY(x)     (x)
#  define HL_INLINE          inline
#  define HL_PREFETCH(p)     ((void)(p))
#  define HL_ALIGNED(n)
#  define HL_COLD
#endif

#ifdef __cplusplus
}
#endif
