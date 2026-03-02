/*
 * arena_runtime.c — arena allocator dla hl-runtime (VM + JIT)
 *
 * Kompilacja:
 *   gcc -DHL_RUNTIME -O2 -pthread arena_runtime.c -o arena_rt.o
 *
 * Priorytety:
 *   1. Thread-safety dla spawn wewnątrz :: (rwlock zamiast mutex)
 *   2. Recycling segmentów — eliminacja mmap/munmap overhead
 *   3. Grow przez mremap (Linux) lub nowy segment + memcpy (inne)
 *   4. Metryki IPC: alloc_count, grow_count, recycle_hits
 *   5. Canary + guard pages — identyczne z trybem kompilatora
 *
 * Różnice od arena_compiler.c:
 *   - pthread_rwlock zamiast mutex (wielu czytających, jeden piszący)
 *   - pula segmentów (ARENA_RECYCLE_MAX) — recycle zamiast munmap
 *   - atomic counters dla metryk IPC (zero locków dla odczytu stats)
 *   - destructor wątku czyszczy tl_stack automatycznie
 *   - arena_snapshot / arena_restore dla JIT trampolines
 */

#include "arena_base.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <pthread.h>

#if defined(__linux__)
#  include <sys/mman.h>
#  include <unistd.h>
#  define HL_HAVE_MMAP    1
#  define HL_HAVE_MREMAP  1
#elif defined(__APPLE__)
#  include <sys/mman.h>
#  include <unistd.h>
#  define HL_HAVE_MMAP    1
#  define HL_HAVE_MREMAP  0
#else
#  define HL_HAVE_MMAP    0
#  define HL_HAVE_MREMAP  0
#endif

#define ARENA_RECYCLE_MAX  16   /* pula segmentów między wywołaniami */

/* ── forward decl z gc.c ────────────────────────────────────── */
extern void *gc_malloc(size_t size);

/* ── error handler ──────────────────────────────────────────── */
static const char *err_str(ArenaError e) {
    switch (e) {
        case ARENA_OK:             return "OK";
        case ARENA_ERR_STACK_FULL: return "[runtime] za dużo zagnieżdżonych :: (max 64)";
        case ARENA_ERR_OOM:        return "[runtime] brak pamięci systemowej";
        case ARENA_ERR_OVERFLOW:   return "[runtime] grow areny nie powiódł się";
        case ARENA_ERR_CANARY:     return "[runtime] KRYTYCZNE: nadpisano granicę areny";
        case ARENA_ERR_UNDERFLOW:  return "[runtime] arena_exit bez arena_enter";
        case ARENA_ERR_NULL:       return "[runtime] arena_alloc poza funkcją ::";
        case ARENA_ERR_LOCK:       return "[runtime] błąd rwlock areny";
        default:                   return "[runtime] nieznany błąd";
    }
}

HL_COLD
static void default_handler(ArenaError err, const char *msg,
                             const char *file, int line) {
    fprintf(stderr,
        "\n\033[1;31m[arena/runtime] BŁĄD %d\033[0m — %s\n"
        "  lokacja: %s:%d\n\n",
        (int)err, msg ? msg : err_str(err),
        file ? file : "?", line);
}

static ArenaErrorHandler g_handler = default_handler;
void arena_set_error_handler(ArenaErrorHandler h) {
    g_handler = h ? h : default_handler;
}
#define AERR(e) g_handler((e), err_str(e), __FILE__, __LINE__)

/* ════════════════════════════════════════════════════════════
 * Pula segmentów — globalna, thread-safe przez mutex
 * Recykling: zamiast munmap → wróć do puli
 * ════════════════════════════════════════════════════════════ */
typedef struct RecycleEntry {
    uint8_t *base;
    size_t   cap;
} RecycleEntry;

typedef struct {
    RecycleEntry entries[ARENA_RECYCLE_MAX];
    int          count;
    pthread_mutex_t lock;
} HL_ALIGNED(64) RecyclePool;

static RecyclePool g_pool;

static void pool_init(void) {
    static int done = 0;
    if (__atomic_test_and_set(&done, __ATOMIC_SEQ_CST)) return;
    pthread_mutex_init(&g_pool.lock, NULL);
    g_pool.count = 0;
}

/* tryb wyjścia procesu — wyczyść pulę */
__attribute__((destructor))
static void pool_destroy(void) {
    pthread_mutex_lock(&g_pool.lock);
    for (int i = 0; i < g_pool.count; i++) {
#if HL_HAVE_MMAP
        munmap(g_pool.entries[i].base,
               g_pool.entries[i].cap + ARENA_GUARD_SIZE);
#else
        free(g_pool.entries[i].base);
#endif
    }
    g_pool.count = 0;
    pthread_mutex_unlock(&g_pool.lock);
    pthread_mutex_destroy(&g_pool.lock);
}

/* ── mmap z guard page ──────────────────────────────────────── */
static uint8_t *seg_mmap(size_t cap) {
#if HL_HAVE_MMAP
    size_t total = cap + ARENA_GUARD_SIZE;
    uint8_t *mem = (uint8_t *)mmap(NULL, total,
                                    PROT_READ | PROT_WRITE,
                                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mem == MAP_FAILED) return NULL;
    mprotect(mem + cap, ARENA_GUARD_SIZE, PROT_NONE);
    *(uint32_t *)(mem + cap - sizeof(uint32_t)) = ARENA_CANARY;
    return mem;
#else
    uint8_t *mem = (uint8_t *)aligned_alloc(ARENA_ALIGN, cap);
    if (mem) *(uint32_t *)(mem + cap - sizeof(uint32_t)) = ARENA_CANARY;
    return mem;
#endif
}

/* próbuj wziąć z puli, albo zrób nowy */
static uint8_t *seg_acquire(size_t cap, size_t *actual_cap) {
    pool_init();

    pthread_mutex_lock(&g_pool.lock);
    /* szukaj segmentu wystarczająco dużego */
    for (int i = 0; i < g_pool.count; i++) {
        if (g_pool.entries[i].cap >= cap) {
            uint8_t *base = g_pool.entries[i].base;
            *actual_cap   = g_pool.entries[i].cap;
            /* usuń z puli (swap z ostatnim) */
            g_pool.entries[i] = g_pool.entries[--g_pool.count];
            pthread_mutex_unlock(&g_pool.lock);

            /* reset canary i offset */
            *(uint32_t *)(base + *actual_cap - sizeof(uint32_t)) = ARENA_CANARY;
            return base;
        }
    }
    pthread_mutex_unlock(&g_pool.lock);

    /* brak w puli — nowy mmap */
    *actual_cap = cap;
    return seg_mmap(cap);
}

/* oddaj segment do puli lub zwolnij */
static void seg_release(uint8_t *base, size_t cap) {
    pthread_mutex_lock(&g_pool.lock);
    if (g_pool.count < ARENA_RECYCLE_MAX) {
        g_pool.entries[g_pool.count].base = base;
        g_pool.entries[g_pool.count].cap  = cap;
        g_pool.count++;
        pthread_mutex_unlock(&g_pool.lock);
        return;
    }
    pthread_mutex_unlock(&g_pool.lock);
#if HL_HAVE_MMAP
    munmap(base, cap + ARENA_GUARD_SIZE);
#else
    free(base);
#endif
}

/* ════════════════════════════════════════════════════════════
 * Arena — jeden poziom na thread-local stosie
 * Używa rwlock: wielu spawn() może czytać, arena_alloc pisze
 * ════════════════════════════════════════════════════════════ */
typedef struct {
    /* hot (pierwsza cache-line) */
    uint8_t          *base;
    _Atomic size_t    offset;       /* atomic dla lockless read  */
    size_t            cap;
    pthread_rwlock_t  rwlock;

    /* cold — metryki przez atomics (czytanie bez locka) */
    _Atomic size_t    total_alloc;
    _Atomic size_t    peak_usage;
    _Atomic size_t    grow_count;
    _Atomic size_t    recycle_hits;
    size_t            wasted;
} HL_ALIGNED(64) Arena;

/* ── thread-local stack ─────────────────────────────────────── */
static __thread Arena tl_stack[ARENA_STACK_MAX];
static __thread int   tl_depth = -1;

/* destructor wątku — wyczyść jeśli wątek kończy się w trakcie :: */
static void thread_cleanup(void *arg) {
    (void)arg;
    while (tl_depth >= 0) arena_exit();
}

static __thread pthread_key_t tl_cleanup_key;
static __thread int           tl_cleanup_init = 0;

static void ensure_cleanup_key(void) {
    if (!tl_cleanup_init) {
        pthread_key_create(&tl_cleanup_key, thread_cleanup);
        pthread_setspecific(tl_cleanup_key, (void *)1);
        tl_cleanup_init = 1;
    }
}

/* ════════════════════════════════════════════════════════════
 * Grow — powiększ arenę zachowując dane
 * ════════════════════════════════════════════════════════════ */
HL_COLD
static int arena_grow(Arena *a, size_t needed) {
    size_t old_cap = a->cap;
    size_t new_cap = old_cap * ARENA_GROW_FACTOR;
    while (new_cap < needed + sizeof(uint32_t) + ARENA_ALIGN)
        new_cap *= ARENA_GROW_FACTOR;
    if (new_cap > ARENA_MAX_CAP) { AERR(ARENA_ERR_OVERFLOW); return 0; }

#if HL_HAVE_MREMAP
    mprotect(a->base + old_cap, ARENA_GUARD_SIZE, PROT_READ | PROT_WRITE);
    uint8_t *nb = (uint8_t *)mremap(a->base,
                                     old_cap + ARENA_GUARD_SIZE,
                                     new_cap + ARENA_GUARD_SIZE,
                                     MREMAP_MAYMOVE);
    if (nb == MAP_FAILED) {
        mprotect(a->base + old_cap, ARENA_GUARD_SIZE, PROT_NONE);
        AERR(ARENA_ERR_OOM);
        return 0;
    }
    a->base = nb;
    a->cap  = new_cap;
    mprotect(a->base + new_cap, ARENA_GUARD_SIZE, PROT_NONE);

#elif HL_HAVE_MMAP
    /* macOS: nowy segment + memcpy */
    fprintf(stderr,
        "[arena/runtime] UWAGA: grow na macOS unieważnia wskaźniki "
        "(%zu → %zu MB)\n", old_cap>>20, new_cap>>20);

    uint8_t *nb = seg_mmap(new_cap);
    if (!nb) { AERR(ARENA_ERR_OOM); return 0; }
    size_t cur_off = atomic_load_explicit(&a->offset, memory_order_relaxed);
    memcpy(nb, a->base, cur_off);
    munmap(a->base, old_cap + ARENA_GUARD_SIZE);
    a->base = nb;
    a->cap  = new_cap;

#else
    uint8_t *nb = (uint8_t *)realloc(a->base, new_cap);
    if (!nb) { AERR(ARENA_ERR_OOM); return 0; }
    a->base = nb;
    a->cap  = new_cap;
#endif

    *(uint32_t *)(a->base + a->cap - sizeof(uint32_t)) = ARENA_CANARY;
    atomic_fetch_add_explicit(&a->grow_count, 1, memory_order_relaxed);
    return 1;
}

/* ════════════════════════════════════════════════════════════
 * PUBLIC API
 * ════════════════════════════════════════════════════════════ */

void arena_enter(size_t cap) {
    ensure_cleanup_key();

    if (HL_UNLIKELY(tl_depth + 1 >= ARENA_STACK_MAX)) {
        AERR(ARENA_ERR_STACK_FULL);
        return;
    }

    long   pgsz   = sysconf(_SC_PAGESIZE);
    size_t actual = cap ? cap : ARENA_DEFAULT_CAP;
    actual = (actual + (size_t)pgsz - 1) & ~((size_t)pgsz - 1);

    size_t real_cap;
    uint8_t *base = seg_acquire(actual, &real_cap);
    if (HL_UNLIKELY(!base)) { AERR(ARENA_ERR_OOM); return; }

    tl_depth++;
    Arena *a = &tl_stack[tl_depth];

    a->base = base;
    a->cap  = real_cap;
    atomic_store(&a->offset,       0);
    atomic_store(&a->total_alloc,  0);
    atomic_store(&a->peak_usage,   0);
    atomic_store(&a->grow_count,   0);
    atomic_store(&a->recycle_hits, real_cap >= actual ? 1 : 0);
    a->wasted = 0;

    pthread_rwlockattr_t attr;
    pthread_rwlockattr_init(&attr);
    pthread_rwlock_init(&a->rwlock, &attr);
    pthread_rwlockattr_destroy(&attr);

    HL_PREFETCH(base);
    HL_PREFETCH(base + 64);
}

void arena_exit(void) {
    if (HL_UNLIKELY(tl_depth < 0)) { AERR(ARENA_ERR_UNDERFLOW); return; }

    Arena *a = &tl_stack[tl_depth];

    pthread_rwlock_wrlock(&a->rwlock);

    uint32_t canary = *(uint32_t *)(a->base + a->cap - sizeof(uint32_t));
    if (HL_UNLIKELY(canary != ARENA_CANARY))
        AERR(ARENA_ERR_CANARY);

    pthread_rwlock_unlock(&a->rwlock);
    pthread_rwlock_destroy(&a->rwlock);

    seg_release(a->base, a->cap);
    a->base = NULL;
    tl_depth--;
}

void arena_reset(void) {
    if (HL_UNLIKELY(tl_depth < 0)) return;
    Arena *a = &tl_stack[tl_depth];

    pthread_rwlock_wrlock(&a->rwlock);
    atomic_store(&a->offset,      0);
    atomic_store(&a->total_alloc, 0);
    atomic_store(&a->grow_count,  0);
    a->wasted = 0;
    *(uint32_t *)(a->base + a->cap - sizeof(uint32_t)) = ARENA_CANARY;
    pthread_rwlock_unlock(&a->rwlock);
}

HL_INLINE
void *arena_alloc(size_t size) {
    if (HL_UNLIKELY(tl_depth < 0)) { AERR(ARENA_ERR_NULL); return NULL; }

    Arena  *a       = &tl_stack[tl_depth];
    size_t  aligned = (size + (ARENA_ALIGN-1)) & ~(size_t)(ARENA_ALIGN-1);

    pthread_rwlock_wrlock(&a->rwlock);

    size_t cur = atomic_load_explicit(&a->offset, memory_order_relaxed);
    size_t avail = a->cap - cur - sizeof(uint32_t);

    if (HL_UNLIKELY(aligned > avail)) {
        if (HL_UNLIKELY(!arena_grow(a, aligned))) {
            pthread_rwlock_unlock(&a->rwlock);
            return NULL;
        }
        cur = atomic_load_explicit(&a->offset, memory_order_relaxed);
    }

    void *ptr = a->base + cur;
    atomic_store_explicit(&a->offset, cur + aligned, memory_order_relaxed);

    size_t total = atomic_fetch_add_explicit(
        &a->total_alloc, aligned, memory_order_relaxed) + aligned;

    size_t peak = atomic_load_explicit(&a->peak_usage, memory_order_relaxed);
    if (HL_UNLIKELY(total > peak))
        atomic_store_explicit(&a->peak_usage, total, memory_order_relaxed);

    a->wasted += aligned - size;

    pthread_rwlock_unlock(&a->rwlock);

    HL_PREFETCH((uint8_t *)ptr + aligned);
    return ptr;
}

void *arena_alloc_zero(size_t size) {
    void *p = arena_alloc(size);
    if (HL_LIKELY(p)) memset(p, 0, size);
    return p;
}

char *arena_strdup(const char *s) {
    if (!s) return NULL;
    size_t len = strlen(s) + 1;
    char  *dst = (char *)arena_alloc(len);
    if (HL_LIKELY(dst)) memcpy(dst, s, len);
    return dst;
}

void *arena_memdup(const void *ptr, size_t size) {
    void *dst = arena_alloc(size);
    if (HL_LIKELY(dst)) memcpy(dst, ptr, size);
    return dst;
}

/* ── JIT support: snapshot / restore ───────────────────────── */
typedef struct { size_t offset; } ArenaSnapshot;

ArenaSnapshot arena_snapshot(void) {
    ArenaSnapshot s = {0};
    if (tl_depth >= 0) {
        Arena *a = &tl_stack[tl_depth];
        s.offset = atomic_load_explicit(&a->offset, memory_order_relaxed);
    }
    return s;
}

void arena_restore(ArenaSnapshot s) {
    if (tl_depth < 0) return;
    Arena *a = &tl_stack[tl_depth];
    pthread_rwlock_wrlock(&a->rwlock);
    atomic_store_explicit(&a->offset, s.offset, memory_order_relaxed);
    pthread_rwlock_unlock(&a->rwlock);
}

/* ── diagnostyka ────────────────────────────────────────────── */
int    arena_active(void)    { return tl_depth >= 0; }
int    arena_get_depth(void) { return tl_depth; }

size_t arena_remaining(void) {
    if (tl_depth < 0) return 0;
    Arena *a = &tl_stack[tl_depth];
    size_t cur = atomic_load_explicit(&a->offset, memory_order_acquire);
    return a->cap - cur - sizeof(uint32_t);
}

void arena_get_stats(ArenaStats *out) {
    if (!out) return;
    if (tl_depth < 0) { memset(out, 0, sizeof(*out)); return; }
    Arena *a = &tl_stack[tl_depth];
    /* lockless read przez atomics */
    out->total_alloc = atomic_load_explicit(&a->total_alloc, memory_order_relaxed);
    out->peak_usage  = atomic_load_explicit(&a->peak_usage,  memory_order_relaxed);
    out->grow_count  = atomic_load_explicit(&a->grow_count,  memory_order_relaxed);
    out->current_cap = a->cap;
    out->wasted      = a->wasted;
    out->depth       = (uint32_t)tl_depth;
}

void arena_print_stats(void) {
    ArenaStats s;
    arena_get_stats(&s);
    size_t recycle = (tl_depth >= 0)
        ? atomic_load(&tl_stack[tl_depth].recycle_hits) : 0;
    fprintf(stderr,
        "[arena/runtime] głębokość=%-2u  "
        "zaalok=%-8zu  peak=%-8zu  "
        "cap=%-8zu  grow=%-3zu  "
        "recycle=%-3zu  waste=%-6zu  wolne=%-8zu\n",
        s.depth, s.total_alloc, s.peak_usage,
        s.current_cap, s.grow_count,
        recycle, s.wasted, arena_remaining());
}

/* ── unified allocator ──────────────────────────────────────── */
void *hl_alloc(size_t size) {
    return HL_LIKELY(tl_depth >= 0) ? arena_alloc(size) : gc_malloc(size);
}
void *hl_alloc_zero(size_t size) {
    if (HL_LIKELY(tl_depth >= 0)) return arena_alloc_zero(size);
    void *p = gc_malloc(size); if (p) memset(p, 0, size); return p;
}
char *hl_strdup(const char *s) {
    if (HL_LIKELY(tl_depth >= 0)) return arena_strdup(s);
    if (!s) return NULL;
    size_t len = strlen(s)+1;
    char *d = (char *)gc_malloc(len);
    if (d) memcpy(d,s,len);
    return d;
}
