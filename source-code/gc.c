/*
 * gc.c — hacker-lang Unified Memory v2.0
 *
 * Dwa niezależne systemy w jednym pliku:
 *
 *   [A] GC  — dla runtime (hl-runtime)
 *       Generacyjny mark-sweep:
 *         Young: bump-pointer arena 64KB (alokacja = 2 instrukcje)
 *         Old:   linked list + mark-sweep
 *
 *   [B] Arena — dla kompilatora (hl-compiler)
 *       Region allocator: mmap + bump pointer + reset per-faza
 *       Reset całej fazy = 1 instrukcja (top = base)
 *       Brak GC overhead — kompilator sam wie kiedy zwolnić
 *
 * Brak konfliktu: GC i areny używają osobnych pul pamięci.
 * Oba systemu mogą działać w tym samym procesie jednocześnie.
 *
 * Kompilacja:
 *   gcc -O3 -march=native -fomit-frame-pointer -c gc.c -o gc.o
 *   ar rcs libgc.a gc.o
 */

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <stdio.h>
#include <sys/mman.h>
#include <assert.h>
#include <errno.h>

/* ================================================================
 * Wspólna konfiguracja
 * ================================================================ */
#define SLAB_ALIGN        8
#define ALIGN_UP(n, a)    (((n) + (a) - 1) & ~((a) - 1))

/* ================================================================
 * [A] GC — Young / Old Generation
 * ================================================================ */
#define YOUNG_SIZE        (64  * 1024)        /* 64 KB  — mieści się w L2    */
#define OLD_THRESHOLD     (2   * 1024 * 1024) /* 2  MB  — trigger major GC   */
#define TENURING_AGE      2                   /* minor GC przeżyć → old      */

typedef struct GcHeader {
    uint32_t         size;        /* rozmiar danych użytkownika          */
    uint8_t          age;         /* liczba przeżytych minor GC          */
    uint8_t          marked;      /* flaga mark                          */
    uint8_t          generation;  /* 0 = young, 1 = old                  */
    uint8_t          _pad;
    struct GcHeader *next;        /* linked list dla old generation      */
} GcHeader;

#define HDR_SIZE        sizeof(GcHeader)
#define HDR_TO_PTR(h)   ((void*)((GcHeader*)(h) + 1))
#define PTR_TO_HDR(p)   ((GcHeader*)(p) - 1)

/* Young slab — statyczny, wyrównany do linii cache */
static uint8_t  young_slab[YOUNG_SIZE] __attribute__((aligned(64)));
static uint8_t *young_top  = young_slab;
static uint8_t *young_end  = young_slab + YOUNG_SIZE;

/* Old generation */
static GcHeader *old_list   = NULL;
static size_t    old_used   = 0;
static size_t    old_allocs = 0;

/* Statystyki */
typedef struct {
    uint64_t minor_count;
    uint64_t major_count;
    uint64_t promoted;
    uint64_t collected_young;
    uint64_t collected_old;
    uint64_t total_allocs;
} GcStats;

static GcStats gc_stats = {0};

/* ── Prywatna: alokacja w old ─────────────────────────────── */
static void *gc_alloc_old_internal(size_t size) {
    size_t    aligned = ALIGN_UP(size, SLAB_ALIGN);
    GcHeader *h       = (GcHeader*)malloc(HDR_SIZE + aligned);
    if (!h) return NULL;
    h->size       = (uint32_t)aligned;
    h->age        = TENURING_AGE;
    h->marked     = 0;
    h->generation = 1;
    h->next       = old_list;
    old_list      = h;
    old_used     += HDR_SIZE + aligned;
    old_allocs++;
    return HDR_TO_PTR(h);
}

/* ── Minor GC ─────────────────────────────────────────────── */
static void gc_collect_minor(void) {
    gc_stats.minor_count++;

    static uint8_t survivor_buf[YOUNG_SIZE] __attribute__((aligned(64)));
    uint8_t *sur  = survivor_buf;
    uint8_t *p    = young_slab;

    while (p < young_top) {
        GcHeader *h    = (GcHeader*)p;
        size_t    osz  = HDR_SIZE + h->size;

        if (h->marked) {
            h->age++;
            if (h->age >= TENURING_AGE) {
                GcHeader *oh = (GcHeader*)malloc(osz);
                if (oh) {
                    memcpy(oh, h, osz);
                    oh->generation = 1;
                    oh->next       = old_list;
                    old_list       = oh;
                    old_used      += osz;
                    old_allocs++;
                    gc_stats.promoted++;
                }
            } else {
                memcpy(sur, h, osz);
                sur += osz;
            }
        } else {
            gc_stats.collected_young++;
        }
        p += osz;
    }

    size_t sur_sz = (size_t)(sur - survivor_buf);
    memcpy(young_slab, survivor_buf, sur_sz);
    young_top = young_slab + sur_sz;

    #ifdef GC_DEBUG
    memset(young_top, 0xDD, (size_t)(young_end - young_top));
    #endif
}

/* ── Major GC (old generation) ────────────────────────────── */
static void gc_collect_major(void) {
    gc_stats.major_count++;
    GcHeader **pp = &old_list;
    while (*pp) {
        GcHeader *h = *pp;
        if (!h->marked) {
            *pp       = h->next;
            old_used -= HDR_SIZE + h->size;
            old_allocs--;
            gc_stats.collected_old++;
            free(h);
        } else {
            h->marked = 0;
            pp = &h->next;
        }
    }
}

/* ================================================================
 * Publiczne API GC
 * ================================================================ */

/* Alokacja — fast path: bump pointer w young */
__attribute__((hot))
void *gc_malloc(size_t size) {
    if (size == 0) size = 1;
    size_t aligned = ALIGN_UP(size, SLAB_ALIGN);
    size_t total   = HDR_SIZE + aligned;
    gc_stats.total_allocs++;

    if (__builtin_expect(young_top + total <= young_end, 1)) {
        GcHeader *h   = (GcHeader*)young_top;
        young_top    += total;
        h->size       = (uint32_t)aligned;
        h->age        = 0;
        h->marked     = 0;
        h->generation = 0;
        h->next       = NULL;
        return HDR_TO_PTR(h);
    }

    /* Young pełny — minor GC */
    gc_collect_minor();

    if (__builtin_expect(young_top + total <= young_end, 1)) {
        GcHeader *h   = (GcHeader*)young_top;
        young_top    += total;
        h->size       = (uint32_t)aligned;
        h->age        = 0;
        h->marked     = 0;
        h->generation = 0;
        h->next       = NULL;
        return HDR_TO_PTR(h);
    }

    /* Nadal brak miejsca — alokuj w old */
    return gc_alloc_old_internal(size);
}

/* Alokacja bezpośrednio w old (publiczne dla Rust FFI) */
void *gc_alloc_old(size_t size) {
    return gc_alloc_old_internal(size);
}

__attribute__((hot))
void gc_mark(void *ptr) {
    if (!ptr) return;
    PTR_TO_HDR(ptr)->marked = 1;
}

void gc_unmark_all(void) {
    uint8_t *p = young_slab;
    while (p < young_top) {
        GcHeader *h = (GcHeader*)p;
        h->marked   = 0;
        p          += HDR_SIZE + h->size;
    }
    for (GcHeader *h = old_list; h; h = h->next)
        h->marked = 0;
}

void gc_sweep(void) {
    gc_collect_minor();
    if (old_used > OLD_THRESHOLD)
        gc_collect_major();
}

void gc_collect_full(void) {
    gc_collect_minor();
    gc_collect_major();
    young_top = young_slab;
}

void gc_stats_print(void) {
    fprintf(stderr,
            "[GC] allocs=%llu  minor=%llu  major=%llu\n"
            "     promoted=%llu  collected(y=%llu o=%llu)\n"
            "     old_live=%zu KB  young=%zu/%d KB\n",
            (unsigned long long)gc_stats.total_allocs,
            (unsigned long long)gc_stats.minor_count,
            (unsigned long long)gc_stats.major_count,
            (unsigned long long)gc_stats.promoted,
            (unsigned long long)gc_stats.collected_young,
            (unsigned long long)gc_stats.collected_old,
            old_used / 1024,
            (size_t)(young_top - young_slab) / 1024,
            YOUNG_SIZE / 1024
    );
}

void gc_stats_get(
    uint64_t *minor_out, uint64_t *major_out,
    uint64_t *promoted_out, uint64_t *total_out
) {
    if (minor_out)    *minor_out    = gc_stats.minor_count;
    if (major_out)    *major_out    = gc_stats.major_count;
    if (promoted_out) *promoted_out = gc_stats.promoted;
    if (total_out)    *total_out    = gc_stats.total_allocs;
}

/* ================================================================
 * [B] ARENA ALLOCATOR — dla hl-compiler
 *
 * Wzorzec użycia w kompilatorze:
 *
 *   Arena tokens, ast_arena, ir_arena;
 *   arena_init(&tokens,   512 * 1024);  // 512KB dla tokenizera
 *   arena_init(&ast_arena, 4  * 1024 * 1024); // 4MB dla AST
 *   arena_init(&ir_arena,  8  * 1024 * 1024); // 8MB dla IR
 *
 *   // Faza 1: tokenizer
 *   Token *t = arena_alloc(&tokens, sizeof(Token));
 *   // ...
 *   arena_reset(&tokens);  // uwolnij WSZYSTKO jednym krokiem
 *
 *   // Faza 2: AST
 *   AstNode *n = arena_alloc(&ast_arena, sizeof(AstNode));
 *
 *   // Koniec kompilacji
 *   arena_free(&tokens);
 *   arena_free(&ast_arena);
 *   arena_free(&ir_arena);
 *
 * String interning w arenie:
 *   char *s = arena_strdup(&ast_arena, "identifier");
 *
 * ================================================================ */

#define ARENA_DEFAULT_ALIGN 8

typedef struct ArenaChunk {
    uint8_t          *base;    /* początek bloku mmap               */
    uint8_t          *top;     /* aktualny wskaźnik alokacji        */
    size_t            cap;     /* pojemność bloku w bajtach         */
    struct ArenaChunk *next;   /* następny blok (jeśli overflow)    */
} ArenaChunk;

typedef struct Arena {
    ArenaChunk *head;          /* aktualny blok (first-fit)         */
    ArenaChunk *first;         /* pierwszy blok (do reset)          */
    size_t      chunk_size;    /* rozmiar kolejnych bloków          */
    size_t      total_allocs;  /* liczba alokacji (statystyki)      */
    size_t      total_bytes;   /* suma zaalokowanych bajtów         */
} Arena;

/* Alokacja nowego bloku przez mmap */
static ArenaChunk *arena_new_chunk(size_t size) {
    size_t aligned_size = ALIGN_UP(size, 4096); /* strona */
    void *mem = mmap(
        NULL, sizeof(ArenaChunk) + aligned_size,
                     PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS,
                     -1, 0
    );
    if (mem == MAP_FAILED) {
        perror("[arena] mmap failed");
        return NULL;
    }
    ArenaChunk *c = (ArenaChunk*)mem;
    c->base = (uint8_t*)mem + sizeof(ArenaChunk);
    c->top  = c->base;
    c->cap  = aligned_size;
    c->next = NULL;
    return c;
}

/* ── arena_init ───────────────────────────────────────────── */
void arena_init(Arena *a, size_t initial_size) {
    if (!a) return;
    ArenaChunk *c = arena_new_chunk(initial_size);
    a->head        = c;
    a->first       = c;
    a->chunk_size  = initial_size;
    a->total_allocs = 0;
    a->total_bytes  = 0;
}

/* ── arena_alloc — O(1) alokacja ─────────────────────────── */
__attribute__((hot))
void *arena_alloc(Arena *a, size_t size) {
    if (!a || size == 0) return NULL;
    size_t aligned = ALIGN_UP(size, ARENA_DEFAULT_ALIGN);
    a->total_allocs++;
    a->total_bytes += aligned;

    ArenaChunk *c = a->head;

    /* Fast path: jest miejsce w aktualnym bloku */
    if (__builtin_expect(c->top + aligned <= c->base + c->cap, 1)) {
        void *ptr  = c->top;
        c->top    += aligned;
        return ptr;
    }

    /* Slow path: potrzebny nowy blok */
    size_t new_cap = (aligned > a->chunk_size) ? aligned * 2 : a->chunk_size;
    ArenaChunk *nc = arena_new_chunk(new_cap);
    if (!nc) return NULL;

    nc->next = a->head;
    a->head  = nc;

    void *ptr = nc->top;
    nc->top  += aligned;
    return ptr;
}

/* ── arena_alloc_zeroed ───────────────────────────────────── */
void *arena_alloc_zeroed(Arena *a, size_t size) {
    void *p = arena_alloc(a, size);
    if (p) memset(p, 0, size);
    return p;
}

/* ── arena_strdup — string interning ─────────────────────── */
char *arena_strdup(Arena *a, const char *s) {
    if (!s) return NULL;
    size_t len = strlen(s) + 1;
    char  *dst = (char*)arena_alloc(a, len);
    if (dst) memcpy(dst, s, len);
    return dst;
}

/* ── arena_strndup ────────────────────────────────────────── */
char *arena_strndup(Arena *a, const char *s, size_t n) {
    if (!s) return NULL;
    char *dst = (char*)arena_alloc(a, n + 1);
    if (!dst) return NULL;
    memcpy(dst, s, n);
    dst[n] = '\0';
    return dst;
}

/* ── arena_reset — zwolnij WSZYSTKO bez munmap (O(1)) ─────── */
void arena_reset(Arena *a) {
    if (!a) return;
    /* Zwolnij wszystkie bloki oprócz pierwszego */
    ArenaChunk *c = a->head;
    while (c && c != a->first) {
        ArenaChunk *next = c->next;
        size_t total = sizeof(ArenaChunk) + c->cap;
        munmap(c, total);
        c = next;
    }
    /* Zresetuj pierwszy blok */
    if (a->first) {
        a->first->top  = a->first->base;
        a->first->next = NULL;
    }
    a->head         = a->first;
    a->total_allocs = 0;
    a->total_bytes  = 0;
}

/* ── arena_free — zwolnij całkowicie ─────────────────────── */
void arena_free(Arena *a) {
    if (!a) return;
    ArenaChunk *c = a->head;
    while (c) {
        ArenaChunk *next = c->next;
        size_t total = sizeof(ArenaChunk) + c->cap;
        munmap(c, total);
        c = next;
    }
    memset(a, 0, sizeof(Arena));
}

/* ── arena_stats_print ────────────────────────────────────── */
void arena_stats_print(const Arena *a, const char *name) {
    if (!a) return;
    size_t chunks = 0;
    size_t cap    = 0;
    for (ArenaChunk *c = a->head; c; c = c->next) {
        chunks++;
        cap += c->cap;
    }
    fprintf(stderr,
            "[Arena:%s] allocs=%zu  bytes=%zu KB  chunks=%zu  cap=%zu KB\n",
            name ? name : "?",
            a->total_allocs,
            a->total_bytes / 1024,
            chunks,
            cap / 1024
    );
}

/* ================================================================
 * Savepoint — powrót do poprzedniego stanu areny (bez alokacji)
 * Przydatne dla spekulatywnego parsowania (backtracking).
 * ================================================================ */
typedef struct ArenaSavepoint {
    ArenaChunk *head;
    uint8_t    *top;
} ArenaSavepoint;

ArenaSavepoint arena_save(const Arena *a) {
    ArenaSavepoint sp = {0};
    if (a && a->head) {
        sp.head = a->head;
        sp.top  = a->head->top;
    }
    return sp;
}

void arena_restore(Arena *a, ArenaSavepoint sp) {
    if (!a || !sp.head) return;
    /* Zwolnij bloki alokowane po savepoint */
    ArenaChunk *c = a->head;
    while (c && c != sp.head) {
        ArenaChunk *next = c->next;
        munmap(c, sizeof(ArenaChunk) + c->cap);
        c = next;
    }
    a->head      = sp.head;
    if (sp.head) sp.head->top = sp.top;
}
