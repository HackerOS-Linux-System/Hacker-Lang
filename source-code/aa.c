/*
 * aa.c — hacker-lang arena allocator
 *
 * Jeden plik zrodlowy, dwa tryby kompilacji:
 *
 *   -DHL_ARENA_MODE_COMPILER
 *     Uzywa kompilator AOT przy generowaniu kodu dla :: blokow.
 *     Dostarcza funkcje emit_* do wstrzykniecia IR/pseudokodu.
 *
 *   -DHL_ARENA_MODE_JIT
 *     Uzywa interpreter/JIT do ewaluacji :: blokow w locie.
 *     Dostarcza HlJitArenaScope + stos aren per ramka wywolania.
 *
 * Wspolne API (dostepne w obu trybach):
 *   HlArena* hl_arena_new(size_t bytes)
 *   void*    hl_arena_alloc(HlArena* a, size_t n)
 *   void     hl_arena_reset(HlArena* a)
 *   void     hl_arena_free(HlArena* a)
 *   size_t   hl_arena_used(const HlArena* a)
 *   size_t   hl_arena_capacity(const HlArena* a)
 *   size_t   hl_arena_parse_size(const char* spec)
 *
 * Kompilacja przez build.rs:
 *   aa.c -DHL_ARENA_MODE_COMPILER -> libaa.a  (dla kompilatora AOT)
 *   aa.c -DHL_ARENA_MODE_JIT      -> libaa.a  (dla interpretera/JIT + output)
 *
 * Instalacja: ~/.hackeros/hacker-lang/libs/libaa.a
 */

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ── Wyrownanie do 16 bajtow (SSE/AVX bezpieczne) ──────────────────── */
#define HL_ARENA_ALIGN      16
#define HL_ALIGN_UP(n, a)   (((n) + (a) - 1) & ~((a) - 1))
#define HL_ARENA_MIN_SIZE   64

/* ── Debug logging ─────────────────────────────────────────────────── */
#ifdef HL_ARENA_DEBUG
#  define HL_DBG(fmt, ...) fprintf(stderr, "[aa] " fmt "\n", ##__VA_ARGS__)
#else
#  define HL_DBG(fmt, ...)
#endif

/* ══════════════════════════════════════════════════════════════════════
 * HlArena — bufor z bump-pointer allocatorem
 *
 *   base                              end
 *    |<──────── capacity ─────────────>|
 *    |<── used ──>|<── wolne ─────────>|
 *                  ^
 *                 ptr  (bump pointer)
 *
 * Alokacja : ptr += align(n)   O(1)
 * Reset    : ptr = base        O(1), pamiec zachowana
 * Free     : free(base)        O(1), jednorazowe zwolnienie
 * ══════════════════════════════════════════════════════════════════════ */
typedef struct HlArena {
    uint8_t* base;
    uint8_t* ptr;
    uint8_t* end;
    size_t   peak;   /* maks. uzycie — do profilowania/debugowania */
} HlArena;

/* ══════════════════════════════════════════════════════════════════════
 * Wspolne API
 * ══════════════════════════════════════════════════════════════════════ */

/*
 * hl_arena_parse_size — zamien string rozmiaru na bajty.
 *
 * Obslugiwane sufiksy (case-insensitive):
 *   "b"  lub brak  — bajty
 *   "kb"           — kilobajty (1024)
 *   "mb"           — megabajty (1024^2)
 *   "gb"           — gigabajty (1024^3)
 *
 * Przyklady: "512kb" -> 524288, "1mb" -> 1048576, "256" -> 256
 * Zwraca 0 przy bledzie parsowania.
 */
size_t hl_arena_parse_size(const char* spec) {
    if (!spec || *spec == '\0') return 0;

    char buf[32];
    size_t len = strlen(spec);
    if (len >= sizeof(buf)) return 0;

    for (size_t i = 0; i <= len; i++)
        buf[i] = (char)((spec[i] >= 'A' && spec[i] <= 'Z')
        ? spec[i] + 32 : spec[i]);

    char*    endp  = NULL;
    uint64_t value = (uint64_t)strtoull(buf, &endp, 10);
    if (endp == buf) return 0;

    while (*endp == ' ') endp++;

    uint64_t mul = 1;
    if      (strncmp(endp, "gb", 2) == 0) mul = 1024ULL * 1024 * 1024;
    else if (strncmp(endp, "mb", 2) == 0) mul = 1024ULL * 1024;
    else if (strncmp(endp, "kb", 2) == 0) mul = 1024ULL;
    else if (strncmp(endp, "b",  1) == 0) mul = 1;
    else if (*endp != '\0')               return 0;

    uint64_t result = value * mul;
    if (result > (uint64_t)SIZE_MAX) return 0;
    return (size_t)result;
}

/*
 * hl_arena_new — alokuj nowa arene o pojemnosci `size_bytes`.
 * Zwraca NULL jesli malloc zawiedzie.
 */
HlArena* hl_arena_new(size_t size_bytes) {
    if (size_bytes < HL_ARENA_MIN_SIZE) size_bytes = HL_ARENA_MIN_SIZE;
    size_bytes = HL_ALIGN_UP(size_bytes, HL_ARENA_ALIGN);

    HlArena* a = (HlArena*)malloc(sizeof(HlArena));
    if (!a) return NULL;

    a->base = (uint8_t*)malloc(size_bytes);
    if (!a->base) { free(a); return NULL; }

    a->ptr  = a->base;
    a->end  = a->base + size_bytes;
    a->peak = 0;

    HL_DBG("new %p [%zu bytes]", (void*)a, size_bytes);
    return a;
}

/*
 * hl_arena_alloc — alokuj `n` bajtow z areny (bump pointer, O(1)).
 * Zero-inicjalizuje przydzielona pamiec.
 * Zwraca NULL jesli brak miejsca.
 */
void* hl_arena_alloc(HlArena* a, size_t n) {
    if (!a || n == 0) return NULL;

    size_t aligned = HL_ALIGN_UP(n, HL_ARENA_ALIGN);
    if (a->ptr + aligned > a->end) {
        HL_DBG("OOM: need %zu, have %zu", aligned, (size_t)(a->end - a->ptr));
        return NULL;
    }

    void* p = a->ptr;
    a->ptr += aligned;
    memset(p, 0, aligned);

    size_t used = (size_t)(a->ptr - a->base);
    if (used > a->peak) a->peak = used;

    HL_DBG("alloc %zu -> %p (used %zu/%zu)",
           n, p, used, (size_t)(a->end - a->base));
    return p;
}

/*
 * hl_arena_reset — cofnij bump pointer do poczatku.
 * Pamiec zachowana, wskaznik cofniety — O(1).
 */
void hl_arena_reset(HlArena* a) {
    if (!a) return;
    HL_DBG("reset %p (used %zu, peak %zu)",
           (void*)a, (size_t)(a->ptr - a->base), a->peak);
    a->ptr = a->base;
}

/*
 * hl_arena_free — zwolnij cala arene jednorazowo — O(1).
 */
void hl_arena_free(HlArena* a) {
    if (!a) return;
    HL_DBG("free %p (peak %zu)", (void*)a, a->peak);
    free(a->base);
    free(a);
}

/* Rozmiar uzytej pamieci. */
size_t hl_arena_used(const HlArena* a) {
    return a ? (size_t)(a->ptr - a->base) : 0;
}

/* Calkowita pojemnosc areny. */
size_t hl_arena_capacity(const HlArena* a) {
    return a ? (size_t)(a->end - a->base) : 0;
}

/* ══════════════════════════════════════════════════════════════════════
 * TRYB KOMPILATORA — HL_ARENA_MODE_COMPILER
 *
 * Uzywany przez hl-compiler AOT przy generowaniu kodu IR
 * dla blokow :: name [size] def...done.
 *
 * Kompilator wywoluje emit_prologue / emit_epilogue w czasie
 * kompilacji (build-time) — NIE podczas wykonania programu.
 *
 * Schemat wstrzykniecia w skompilowany kod:
 *
 *   [prologue]  %arena.cache = hl_arena_new(524288)
 *               ... cialo :: bloku ...
 *               hl_arena_alloc(%arena.cache, n)
 *   [epilogue]  hl_arena_free(%arena.cache)
 * ══════════════════════════════════════════════════════════════════════ */
#ifdef HL_ARENA_MODE_COMPILER

/* Kontekst jednego :: bloku podczas emisji kodu. */
typedef struct HlArenaEmitCtx {
    char   arena_var[64];  /* nazwa zmiennej IR, np. "%arena.cache" */
    size_t size_bytes;     /* rozmiar po parsowaniu size_spec        */
} HlArenaEmitCtx;

/*
 * hl_arena_emit_prologue — wygeneruj instrukcje inicjalizacji areny.
 *
 * Wypelnia `ctx` i zapisuje IR do `out`.
 * Przyklad wyjscia:
 *   ; arena prologue: cache [512kb = 524288 bytes]
 *   %arena.cache = call HlArena* @hl_arena_new(i64 524288)
 *
 * Zwraca liczbe zapisanych znakow lub -1 przy bledzie.
 */
int hl_arena_emit_prologue(
    HlArenaEmitCtx* ctx,
    const char*     fn_name,
    const char*     size_spec,
    char*           out,
    size_t          out_len
) {
    if (!ctx || !fn_name || !size_spec || !out || out_len == 0) return -1;

    size_t bytes = hl_arena_parse_size(size_spec);
    if (bytes == 0) { HL_DBG("prologue: zly size_spec '%s'", size_spec); return -1; }

    snprintf(ctx->arena_var, sizeof(ctx->arena_var), "%%arena.%s", fn_name);
    ctx->size_bytes = bytes;

    int n = snprintf(out, out_len,
                     "; arena prologue: %s [%s = %zu bytes]\n"
                     "%s = call HlArena* @hl_arena_new(i64 %zu)\n",
                     fn_name, size_spec, bytes,
                     ctx->arena_var, bytes);

    HL_DBG("emit_prologue: %s [%zu bytes]", fn_name, bytes);
    return (n > 0 && (size_t)n < out_len) ? n : -1;
}

/*
 * hl_arena_emit_epilogue — wygeneruj instrukcje sprzatania areny.
 *
 * Przyklad wyjscia:
 *   ; arena epilogue: cache
 *   call void @hl_arena_free(HlArena* %arena.cache)
 */
int hl_arena_emit_epilogue(
    const HlArenaEmitCtx* ctx,
    char*                 out,
    size_t                out_len
) {
    if (!ctx || !out || out_len == 0) return -1;

    int n = snprintf(out, out_len,
                     "; arena epilogue: %s\n"
                     "call void @hl_arena_free(HlArena* %s)\n",
                     ctx->arena_var, ctx->arena_var);

    HL_DBG("emit_epilogue: %s", ctx->arena_var);
    return (n > 0 && (size_t)n < out_len) ? n : -1;
}

/*
 * hl_arena_emit_alloc — wygeneruj instrukcje alokacji z areny.
 *
 * Przyklad wyjscia:
 *   %ptr.0 = call i8* @hl_arena_alloc(HlArena* %arena.cache, i64 128)
 */
int hl_arena_emit_alloc(
    const HlArenaEmitCtx* ctx,
    const char*           result_var,
    size_t                n_bytes,
    char*                 out,
    size_t                out_len
) {
    if (!ctx || !result_var || !out || out_len == 0) return -1;

    int n = snprintf(out, out_len,
                     "%s = call i8* @hl_arena_alloc(HlArena* %s, i64 %zu)\n",
                     result_var, ctx->arena_var, n_bytes);

    return (n > 0 && (size_t)n < out_len) ? n : -1;
}

/*
 * hl_arena_emit_reset — wygeneruj instrukcje resetu (dla petli w :: bloku).
 */
int hl_arena_emit_reset(
    const HlArenaEmitCtx* ctx,
    char*                 out,
    size_t                out_len
) {
    if (!ctx || !out || out_len == 0) return -1;

    int n = snprintf(out, out_len,
                     "; arena reset\n"
                     "call void @hl_arena_reset(HlArena* %s)\n",
                     ctx->arena_var);

    return (n > 0 && (size_t)n < out_len) ? n : -1;
}

#endif /* HL_ARENA_MODE_COMPILER */


/* ══════════════════════════════════════════════════════════════════════
 * TRYB JIT — HL_ARENA_MODE_JIT
 *
 * Uzywany przez interpreter do ewaluacji :: blokow w locie
 * oraz przez skompilowane programy .hl zawierajace :: bloki.
 *
 * Interpreter wywoluje:
 *   hl_jit_arena_enter(&scope, "cache", "512kb")  // :: cache [512kb] def
 *   hl_jit_arena_alloc(&scope, n)                 // alokacje w ciele
 *   hl_jit_arena_exit(&scope)                     // done
 *
 * Stos obsługuje zagniezdzone :: bloki do HL_JIT_MAX_DEPTH poziomow.
 * ══════════════════════════════════════════════════════════════════════ */
#ifdef HL_ARENA_MODE_JIT

#define HL_JIT_MAX_DEPTH 64

typedef struct {
    HlArena* arena;
    char     name[64];
} HlJitFrame;

typedef struct {
    HlJitFrame frames[HL_JIT_MAX_DEPTH];
    int        depth;
} HlJitArenaScope;

/*
 * hl_jit_arena_enter — wejdz w :: blok.
 * Alokuje nowa arene i wrzuca ja na stos scope.
 * Zwraca 0 przy sukcesie, -1 przy bledzie.
 */
int hl_jit_arena_enter(
    HlJitArenaScope* scope,
    const char*      name,
    const char*      size_spec
) {
    if (!scope || !name || !size_spec) return -1;
    if (scope->depth >= HL_JIT_MAX_DEPTH) {
        HL_DBG("enter: za gleboko (%d)", scope->depth);
        return -1;
    }

    size_t bytes = hl_arena_parse_size(size_spec);
    if (bytes == 0) { HL_DBG("enter: zly size_spec '%s'", size_spec); return -1; }

    HlArena* a = hl_arena_new(bytes);
    if (!a) { HL_DBG("enter: OOM przy new(%zu)", bytes); return -1; }

    HlJitFrame* f = &scope->frames[scope->depth++];
    f->arena = a;
    snprintf(f->name, sizeof(f->name), "%s", name);

    HL_DBG("enter :: %s [%zu bytes] depth=%d", name, bytes, scope->depth);
    return 0;
}

/*
 * hl_jit_arena_exit — wyjdz z :: bloku.
 * Zwalnia arene na szczycie stosu.
 * Zwraca 0 przy sukcesie, -1 jesli stos pusty.
 */
int hl_jit_arena_exit(HlJitArenaScope* scope) {
    if (!scope || scope->depth <= 0) { HL_DBG("exit: pusty scope"); return -1; }

    HlJitFrame* f = &scope->frames[--scope->depth];
    HL_DBG("exit :: %s (peak %zu) depth=%d",
           f->name, f->arena ? f->arena->peak : 0, scope->depth);

    hl_arena_free(f->arena);
    f->arena = NULL;
    return 0;
}

/*
 * hl_jit_arena_alloc — alokuj `n` bajtow z biezacej areny.
 * Zwraca NULL jesli scope pusty lub brak miejsca.
 */
void* hl_jit_arena_alloc(HlJitArenaScope* scope, size_t n) {
    if (!scope || scope->depth <= 0) return NULL;
    return hl_arena_alloc(scope->frames[scope->depth - 1].arena, n);
}

/*
 * hl_jit_arena_reset — resetuj biezaca arene (dla petli w :: bloku).
 */
void hl_jit_arena_reset(HlJitArenaScope* scope) {
    if (!scope || scope->depth <= 0) return;
    hl_arena_reset(scope->frames[scope->depth - 1].arena);
    HL_DBG("reset :: %s", scope->frames[scope->depth - 1].name);
}

/*
 * hl_jit_arena_cleanup — zwolnij wszystkie areny (np. przy panic/unwind).
 */
void hl_jit_arena_cleanup(HlJitArenaScope* scope) {
    if (!scope) return;
    while (scope->depth > 0) hl_jit_arena_exit(scope);
    HL_DBG("cleanup done");
}

/*
 * hl_jit_arena_current — zwroc wskaznik do biezacej areny (debug).
 */
HlArena* hl_jit_arena_current(const HlJitArenaScope* scope) {
    if (!scope || scope->depth <= 0) return NULL;
    return scope->frames[scope->depth - 1].arena;
}

#endif /* HL_ARENA_MODE_JIT */
