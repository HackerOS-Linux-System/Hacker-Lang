/*
 * arena_compiler.c — arena allocator dla hl-compiler (AOT binarka)
 *
 * Kompilacja:
 *   gcc -DHL_COMPILER -O3 -march=native -flto arena_compiler.c -o arena_cc.o
 *
 * Priorytety:
 *   1. Maksymalna wydajność ścieżki gorącej (bump pointer inline)
 *   2. Grow przez mremap — wskaźniki zostają ważne na Linux
 *   3. Thread-safe przez mutex per poziom areny (nie per alokację)
 *   4. Guard pages przez mmap — overflow = natychmiastowy SIGSEGV
 *   5. Canary przy exit — wykrycie korupcji przed zwolnieniem pamięci
 *
 * Różnice od arena_runtime.c:
 *   - brak recyclingu segmentów (każda :: funkcja = fresh mmap)
 *   - agresywniejsze prefetch (zakładamy sekwencyjny dostęp)
 *   - grow factor 2x bez limitu recyklingu
 *   - brak metryki IPC — tylko arena_stats
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
#  define HL_HAVE_MREMAP  0   /* macOS nie ma mremap */
#else
#  define HL_HAVE_MMAP    0
#  define HL_HAVE_MREMAP  0
#endif

/* ── forward decl z gc.c ────────────────────────────────────── */
extern void *gc_malloc(size_t size);

/* ── error handler ──────────────────────────────────────────── */
static const char *err_str(ArenaError e) {
    switch (e) {
        case ARENA_OK:             return "OK";
        case ARENA_ERR_STACK_FULL: return "[compiler] za dużo zagnieżdżonych :: (max 64)";
        case ARENA_ERR_OOM:        return "[compiler] brak pamięci systemowej";
        case ARENA_ERR_OVERFLOW:   return "[compiler] grow areny nie powiódł się";
        case ARENA_ERR_CANARY:     return "[compiler] KRYTYCZNE: nadpisano granicę areny (buffer overflow)";
        case ARENA_ERR_UNDERFLOW:  return "[compiler] arena_exit bez arena_enter";
        case ARENA_ERR_NULL:       return "[compiler] arena_alloc poza funkcją ::";
        case ARENA_ERR_LOCK:       return "[compiler] błąd mutex areny";
        default:                   return "[compiler] nieznany błąd";
    }
}

HL_COLD
static void default_handler(ArenaError err, const char *msg,
                             const char *file, int line) {
    fprintf(stderr,
        "\n\033[1;31m[arena/compiler] BŁĄD %d\033[0m — %s\n"
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
 * Arena struct — jeden poziom na stosie
 * ════════════════════════════════════════════════════════════ */
typedef struct {
    /* ── hot fields (pierwsza cache-line) ────────────────── */
    uint8_t        *base;         /* początek danych             */
    size_t          offset;       /* bump pointer                */
    size_t          cap;          /* aktualna pojemność          */
    pthread_mutex_t lock;         /* mutex dla thread-safety     */

    /* ── cold fields (statystyki) ────────────────────────── */
    size_t          total_alloc;
    size_t          peak_usage;
    size_t          grow_count;
    size_t          wasted;
    uint8_t         uses_mmap;
} HL_ALIGNED(64) Arena;           /* cache-line aligned          */

/* ── thread-local stack ─────────────────────────────────────── */
static __thread Arena tl_stack[ARENA_STACK_MAX];
static __thread int   tl_depth = -1;

/* ════════════════════════════════════════════════════════════
 * Alokacja początkowa przez mmap z guard page
 * ════════════════════════════════════════════════════════════ */
static uint8_t *arena_mmap_alloc(size_t cap) {
#if HL_HAVE_MMAP
    long   pgsz  = sysconf(_SC_PAGESIZE);
    size_t total = cap + ARENA_GUARD_SIZE;

    uint8_t *mem = (uint8_t *)mmap(NULL, total,
                                    PROT_READ | PROT_WRITE,
                                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (HL_UNLIKELY(mem == MAP_FAILED)) return NULL;

    /* guard page — zapis za końcem = SIGSEGV natychmiast */
    mprotect(mem + cap, ARENA_GUARD_SIZE, PROT_NONE);

    /* ustaw canary tuż przed guard page */
    *(uint32_t *)(mem + cap - sizeof(uint32_t)) = ARENA_CANARY;
    return mem;
#else
    uint8_t *mem = (uint8_t *)aligned_alloc(ARENA_ALIGN, cap);
    if (mem)
        *(uint32_t *)(mem + cap - sizeof(uint32_t)) = ARENA_CANARY;
    return mem;
#endif
}

/* ════════════════════════════════════════════════════════════
 * Grow — powiększ arenę zachowując istniejące dane
 *
 * Na Linux: mremap → wskaźniki ZOSTAJĄ WAŻNE (VA remapping)
 * Na macOS/inne: mmap nowy blok + memcpy + munmap stary
 *                (wskaźniki nieważne po grow — ostrzegamy)
 * ════════════════════════════════════════════════════════════ */
HL_COLD
static int arena_grow(Arena *a, size_t needed) {
    size_t new_cap = a->cap * ARENA_GROW_FACTOR;
    while (new_cap < needed + sizeof(uint32_t) + ARENA_ALIGN)
        new_cap *= ARENA_GROW_FACTOR;
    if (new_cap > ARENA_MAX_CAP) {
        AERR(ARENA_ERR_OVERFLOW);
        return 0;
    }

#if HL_HAVE_MREMAP
    /* ── Linux: mremap in-place ──────────────────────────── */
    size_t old_total = a->cap + ARENA_GUARD_SIZE;
    size_t new_total = new_cap + ARENA_GUARD_SIZE;

    /* najpierw zdejmij guard page żeby mremap mógł rosnąć */
    mprotect(a->base + a->cap, ARENA_GUARD_SIZE, PROT_READ | PROT_WRITE);

    uint8_t *new_base = (uint8_t *)mremap(a->base, old_total,
                                           new_total, MREMAP_MAYMOVE);
    if (HL_UNLIKELY(new_base == MAP_FAILED)) {
        /* przywróć guard i zgłoś błąd */
        mprotect(a->base + a->cap, ARENA_GUARD_SIZE, PROT_NONE);
        AERR(ARENA_ERR_OOM);
        return 0;
    }

    a->base = new_base;
    a->cap  = new_cap;

    /* guard page w nowym miejscu */
    mprotect(a->base + new_cap, ARENA_GUARD_SIZE, PROT_NONE);

#elif HL_HAVE_MMAP
    /* ── macOS: nowy mmap + memcpy ───────────────────────── */
    fprintf(stderr,
        "[arena/compiler] UWAGA: grow na macOS unieważnia wskaźniki "
        "(cap: %zu → %zu)\n", a->cap, new_cap);

    uint8_t *new_base = arena_mmap_alloc(new_cap);
    if (HL_UNLIKELY(!new_base)) { AERR(ARENA_ERR_OOM); return 0; }

    memcpy(new_base, a->base, a->offset);
    munmap(a->base, a->cap + ARENA_GUARD_SIZE);
    a->base = new_base;
    a->cap  = new_cap;

#else
    /* ── fallback: realloc ───────────────────────────────── */
    uint8_t *new_base = (uint8_t *)realloc(a->base, new_cap);
    if (HL_UNLIKELY(!new_base)) { AERR(ARENA_ERR_OOM); return 0; }
    a->base = new_base;
    a->cap  = new_cap;
#endif

    /* odnów canary w nowej lokalizacji */
    *(uint32_t *)(a->base + a->cap - sizeof(uint32_t)) = ARENA_CANARY;

    a->grow_count++;
    return 1;
}

/* ════════════════════════════════════════════════════════════
 * PUBLIC API
 * ════════════════════════════════════════════════════════════ */

void arena_enter(size_t cap) {
    if (HL_UNLIKELY(tl_depth + 1 >= ARENA_STACK_MAX)) {
        AERR(ARENA_ERR_STACK_FULL);
        return;
    }

    size_t actual = cap ? cap : ARENA_DEFAULT_CAP;
    /* zaokrąglij do strony */
    long pgsz = sysconf(_SC_PAGESIZE);
    actual = (actual + (size_t)pgsz - 1) & ~((size_t)pgsz - 1);

    tl_depth++;
    Arena *a = &tl_stack[tl_depth];

    a->base = arena_mmap_alloc(actual);
    if (HL_UNLIKELY(!a->base)) {
        tl_depth--;
        AERR(ARENA_ERR_OOM);
        return;
    }

    a->cap         = actual;
    a->offset      = 0;
    a->total_alloc = 0;
    a->peak_usage  = 0;
    a->grow_count  = 0;
    a->wasted      = 0;
    a->uses_mmap   = HL_HAVE_MMAP;

    pthread_mutexattr_t attr;
    pthread_mutexattr_init(&attr);
    /* ERRORCHECK w debug, FAST w release */
#ifdef NDEBUG
    pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_DEFAULT);
#else
    pthread_mutexattr_settype(&attr, PTHREAD_MUTEX_ERRORCHECK);
#endif
    pthread_mutex_init(&a->lock, &attr);
    pthread_mutexattr_destroy(&attr);

    /* prefetch pierwszych cache-line */
    HL_PREFETCH(a->base);
    HL_PREFETCH(a->base + 64);
    HL_PREFETCH(a->base + 128);
}

void arena_exit(void) {
    if (HL_UNLIKELY(tl_depth < 0)) {
        AERR(ARENA_ERR_UNDERFLOW);
        return;
    }

    Arena *a = &tl_stack[tl_depth];

    pthread_mutex_lock(&a->lock);

    /* weryfikacja canary przed zwolnieniem */
    uint32_t canary = *(uint32_t *)(a->base + a->cap - sizeof(uint32_t));
    if (HL_UNLIKELY(canary != ARENA_CANARY)) {
        AERR(ARENA_ERR_CANARY);
        /* kontynuujemy — zwalniamy żeby nie leakować */
    }

    pthread_mutex_unlock(&a->lock);
    pthread_mutex_destroy(&a->lock);

#if HL_HAVE_MMAP
    if (a->uses_mmap)
        munmap(a->base, a->cap + ARENA_GUARD_SIZE);
    else
#endif
        free(a->base);

    a->base = NULL;
    tl_depth--;
}

void arena_reset(void) {
    if (HL_UNLIKELY(tl_depth < 0)) return;
    Arena *a = &tl_stack[tl_depth];

    pthread_mutex_lock(&a->lock);
    a->offset      = 0;
    a->total_alloc = 0;
    a->grow_count  = 0;
    a->wasted      = 0;
    /* odnów canary */
    *(uint32_t *)(a->base + a->cap - sizeof(uint32_t)) = ARENA_CANARY;
    pthread_mutex_unlock(&a->lock);
}

HL_INLINE
void *arena_alloc(size_t size) {
    if (HL_UNLIKELY(tl_depth < 0)) {
        AERR(ARENA_ERR_NULL);
        return NULL;
    }

    Arena *a = &tl_stack[tl_depth];

    /* wyrównanie SIMD */
    size_t aligned = (size + (ARENA_ALIGN - 1)) & ~(size_t)(ARENA_ALIGN - 1);
    size_t waste   = aligned - size;

    pthread_mutex_lock(&a->lock);

    /* zostaw 4 bajty na canary */
    size_t avail = a->cap - a->offset - sizeof(uint32_t);

    if (HL_UNLIKELY(aligned > avail)) {
        /* grow — cold path */
        if (HL_UNLIKELY(!arena_grow(a, aligned))) {
            pthread_mutex_unlock(&a->lock);
            return NULL;
        }
        /* po grow avail się zmieniło */
    }

    void *ptr     = a->base + a->offset;
    a->offset    += aligned;
    a->total_alloc += aligned;
    a->wasted    += waste;

    if (HL_UNLIKELY(a->total_alloc > a->peak_usage))
        a->peak_usage = a->total_alloc;

    pthread_mutex_unlock(&a->lock);

    /* prefetch kolejnych cache-lines dla kompilatora (sekwencyjny dostęp) */
    HL_PREFETCH((uint8_t *)ptr + aligned);
    HL_PREFETCH((uint8_t *)ptr + aligned + 64);

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

/* ── diagnostyka ────────────────────────────────────────────── */
int    arena_active(void)    { return tl_depth >= 0; }
int    arena_get_depth(void) { return tl_depth; }

size_t arena_remaining(void) {
    if (tl_depth < 0) return 0;
    Arena *a = &tl_stack[tl_depth];
    return a->cap - a->offset - sizeof(uint32_t);
}

void arena_get_stats(ArenaStats *out) {
    if (!out) return;
    if (tl_depth < 0) { memset(out, 0, sizeof(*out)); return; }
    Arena *a = &tl_stack[tl_depth];
    pthread_mutex_lock(&a->lock);
    out->total_alloc = a->total_alloc;
    out->peak_usage  = a->peak_usage;
    out->grow_count  = a->grow_count;
    out->current_cap = a->cap;
    out->wasted      = a->wasted;
    out->depth       = (uint32_t)tl_depth;
    pthread_mutex_unlock(&a->lock);
}

void arena_print_stats(void) {
    ArenaStats s;
    arena_get_stats(&s);
    fprintf(stderr,
        "[arena/compiler] głębokość=%-2u  "
        "zaalok=%-8zu  peak=%-8zu  "
        "cap=%-8zu  grow=%-3zu  "
        "waste=%-6zu  wolne=%-8zu\n",
        s.depth, s.total_alloc, s.peak_usage,
        s.current_cap, s.grow_count,
        s.wasted, arena_remaining());
}

/* ── unified allocator ──────────────────────────────────────── */
void *hl_alloc(size_t size) {
    return HL_LIKELY(tl_depth >= 0) ? arena_alloc(size) : gc_malloc(size);
}

void *hl_alloc_zero(size_t size) {
    if (HL_LIKELY(tl_depth >= 0)) return arena_alloc_zero(size);
    void *p = gc_malloc(size);
    if (p) memset(p, 0, size);
    return p;
}

char *hl_strdup(const char *s) {
    if (HL_LIKELY(tl_depth >= 0)) return arena_strdup(s);
    if (!s) return NULL;
    size_t len = strlen(s) + 1;
    char  *dst = (char *)gc_malloc(len);
    if (dst) memcpy(dst, s, len);
    return dst;
}
