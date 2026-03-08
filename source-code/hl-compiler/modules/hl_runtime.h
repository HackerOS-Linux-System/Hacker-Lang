/*
 * hl_runtime.h — hacker-lang runtime Level 2
 *
 * Wspólny header dla:
 *   hl_runtime.c    — print, log, env, exit
 *   hl_string.c     — operacje na stringach
 *   hl_collections.c — HlList, HlMap
 *
 * Wszystkie funkcje są thread-safe (używają własnych buforów
 * lub gc_malloc z gc.c który jest thread-safe przez atomic bump ptr).
 *
 * Linkowanie do skompilowanych programów .hl:
 *   -lhl_runtime -lhl_string -lhl_collections -lgc -laa
 */

#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
    #endif

    /* ══════════════════════════════════════════════════════════════
     * Alokator — gc_malloc z gc.c
     * Wszystkie stringi zwracane przez runtime są alokowane przez GC.
     * Wywołujący NIE zwalnia ich ręcznie — GC sprząta przy gc_sweep().
     * ══════════════════════════════════════════════════════════════ */
    extern void* gc_malloc(size_t size);

    /* ══════════════════════════════════════════════════════════════
     * hl_runtime.c — output, env, log, exit
     * ══════════════════════════════════════════════════════════════ */

    /* Drukuje string + '\n' na stdout (zero fork) */
    void  hl_print(const char* s);

    /* Drukuje i64 + '\n' na stdout */
    void  hl_print_i64(int64_t v);

    /* Drukuje double + '\n' na stdout */
    void  hl_print_f64(double v);

    /* Loguje string na stderr (z prefiksem "[hl] ") */
    void  hl_log(const char* s);

    /* Loguje string na stderr (z prefiksem "[hl:err] ") */
    void  hl_log_err(const char* s);

    /* Ustawia zmienną środowiskową */
    void  hl_setenv(const char* key, const char* val);

    /* Ustawia zmienną środowiskową jako i64 (bez alokacji bufora po stronie LLVM) */
    void  hl_setenv_i64(const char* key, int64_t val);

    /* Ustawia zmienną środowiskową jako double */
    void  hl_setenv_f64(const char* key, double val);

    /* Zwraca wartość zmiennej środowiskowej lub "" jeśli nie istnieje */
    const char* hl_getenv(const char* key);

    /* ══════════════════════════════════════════════════════════════
     * hl_string.c — operacje na stringach
     *
     * Wszystkie funkcje zwracające char* zwracają wskaźnik do pamięci
     * zarządzanej przez GC. Nie zwalniaj ręcznie.
     * ══════════════════════════════════════════════════════════════ */

    /* Konkatenacja — zwraca nowy string */
    char* hl_str_concat(const char* a, const char* b);

    /* Długość stringa */
    int64_t hl_str_len(const char* s);

    /* Zamiana na wielkie litery */
    char* hl_str_upper(const char* s);

    /* Zamiana na małe litery */
    char* hl_str_lower(const char* s);

    /* Usunięcie białych znaków z początku i końca */
    char* hl_str_trim(const char* s);

    /* Sprawdzenie czy s zawiera needle */
    bool hl_str_contains(const char* s, const char* needle);

    /* Zamiana wszystkich wystąpień 'from' na 'to' */
    char* hl_str_replace(const char* s, const char* from, const char* to);

    /* Wycinek [start, end) — ujemne indeksy od końca */
    char* hl_str_slice(const char* s, int64_t start, int64_t end);

    /* Porównanie — zwraca true jeśli a == b */
    bool hl_str_eq(const char* a, const char* b);

    /* Konwersja i64 → string (w pamięci GC) */
    char* hl_i64_to_str(int64_t v);

    /* Konwersja double → string (w pamięci GC) */
    char* hl_f64_to_str(double v);

    /* Konwersja string → i64 (0 jeśli błąd) */
    int64_t hl_str_to_i64(const char* s);

    /* Konwersja string → double (0.0 jeśli błąd) */
    double hl_str_to_f64(const char* s);

    /* Czy string zaczyna się od prefix */
    bool hl_str_starts(const char* s, const char* prefix);

    /* Czy string kończy się na suffix */
    bool hl_str_ends(const char* s, const char* suffix);

    /* Powtórzenie stringa n razy */
    char* hl_str_repeat(const char* s, int64_t n);

    /* Odwrócenie stringa */
    char* hl_str_rev(const char* s);

    /* Indeks pierwszego wystąpienia needle (-1 jeśli brak) */
    int64_t hl_str_index(const char* s, const char* needle);

    /* ══════════════════════════════════════════════════════════════
     * hl_collections.c — HlList i HlMap
     *
     * HlList — dynamiczna lista stringów (char*)
     * HlMap  — hash mapa string → string
     *
     * Obie struktury używają gc_malloc — nie zwalniaj ręcznie elementów.
     * hl_list_free / hl_map_free zwalniają samą strukturę (nie elementy).
     * ══════════════════════════════════════════════════════════════ */

    /* ─── HlList ─────────────────────────────────────────────── */
    typedef struct HlList HlList;

    HlList* hl_list_new(void);
    void    hl_list_push(HlList* l, const char* val);
    char*   hl_list_pop(HlList* l);
    char*   hl_list_get(HlList* l, int64_t idx);
    void    hl_list_set(HlList* l, int64_t idx, const char* val);
    int64_t hl_list_len(HlList* l);
    void    hl_list_free(HlList* l);

    /* ─── HlMap ──────────────────────────────────────────────── */
    typedef struct HlMap HlMap;

    HlMap*  hl_map_new(void);
    void    hl_map_set(HlMap* m, const char* key, const char* val);
    char*   hl_map_get(HlMap* m, const char* key);
    bool    hl_map_has(HlMap* m, const char* key);
    void    hl_map_del(HlMap* m, const char* key);
    int64_t hl_map_len(HlMap* m);
    void    hl_map_free(HlMap* m);

    #ifdef __cplusplus
}
#endif
