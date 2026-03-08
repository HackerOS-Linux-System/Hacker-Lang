/*
 * hl_string.c — hacker-lang string runtime Level 2
 *
 * Wszystkie funkcje zwracające char* alokują przez gc_malloc().
 * Caller NIE zwalnia — GC sprząta przy gc_sweep() / gc_collect_full().
 *
 * Zastępuje system("echo $(..._hl_str_upper $x)") itp.
 * Zero fork(), zero bash.
 */

#include "hl_runtime.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <ctype.h>

/* ── Helpers ─────────────────────────────────────────────── */

/* Alokuje n+1 bajtów przez GC i kopiuje src[0..n] */
static char* gc_strdup_n(const char* src, size_t n) {
    char* dst = (char*)gc_malloc(n + 1);
    if (!dst) return (char*)"";
    memcpy(dst, src, n);
    dst[n] = '\0';
    return dst;
}

static inline const char* safe(const char* s) {
    return s ? s : "";
}

/* ══════════════════════════════════════════════════════════════
 * PODSTAWOWE
 * ══════════════════════════════════════════════════════════════ */

__attribute__((hot))
char* hl_str_concat(const char* a, const char* b) {
    a = safe(a); b = safe(b);
    size_t la = strlen(a);
    size_t lb = strlen(b);
    char*  r  = (char*)gc_malloc(la + lb + 1);
    if (!r) return (char*)"";
    memcpy(r,      a, la);
    memcpy(r + la, b, lb);
    r[la + lb] = '\0';
    return r;
}

__attribute__((hot))
int64_t hl_str_len(const char* s) {
    return s ? (int64_t)strlen(s) : 0;
}

char* hl_str_upper(const char* s) {
    s = safe(s);
    size_t n = strlen(s);
    char*  r = (char*)gc_malloc(n + 1);
    if (!r) return (char*)"";
    for (size_t i = 0; i <= n; i++)
        r[i] = (char)toupper((unsigned char)s[i]);
    return r;
}

char* hl_str_lower(const char* s) {
    s = safe(s);
    size_t n = strlen(s);
    char*  r = (char*)gc_malloc(n + 1);
    if (!r) return (char*)"";
    for (size_t i = 0; i <= n; i++)
        r[i] = (char)tolower((unsigned char)s[i]);
    return r;
}

char* hl_str_trim(const char* s) {
    s = safe(s);
    /* Przeskocz białe znaki z przodu */
    while (*s && isspace((unsigned char)*s)) s++;
    size_t n = strlen(s);
    /* Przytnij z tyłu */
    while (n > 0 && isspace((unsigned char)s[n - 1])) n--;
    return gc_strdup_n(s, n);
}

/* ══════════════════════════════════════════════════════════════
 * WYSZUKIWANIE
 * ══════════════════════════════════════════════════════════════ */

bool hl_str_contains(const char* s, const char* needle) {
    s = safe(s); needle = safe(needle);
    return strstr(s, needle) != NULL;
}

int64_t hl_str_index(const char* s, const char* needle) {
    s = safe(s); needle = safe(needle);
    const char* found = strstr(s, needle);
    if (!found) return -1;
    return (int64_t)(found - s);
}

bool hl_str_starts(const char* s, const char* prefix) {
    s = safe(s); prefix = safe(prefix);
    size_t lp = strlen(prefix);
    return strncmp(s, prefix, lp) == 0;
}

bool hl_str_ends(const char* s, const char* suffix) {
    s = safe(s); suffix = safe(suffix);
    size_t ls = strlen(s);
    size_t lx = strlen(suffix);
    if (lx > ls) return false;
    return strcmp(s + ls - lx, suffix) == 0;
}

bool hl_str_eq(const char* a, const char* b) {
    a = safe(a); b = safe(b);
    return strcmp(a, b) == 0;
}

/* ══════════════════════════════════════════════════════════════
 * TRANSFORMACJE
 * ══════════════════════════════════════════════════════════════ */

char* hl_str_replace(const char* s, const char* from, const char* to) {
    s = safe(s); from = safe(from); to = safe(to);

    size_t ls   = strlen(s);
    size_t lf   = strlen(from);
    size_t lt   = strlen(to);

    if (lf == 0) return gc_strdup_n(s, ls);

    /* Policz wystąpienia żeby zaalokować odpowiedni bufor */
    size_t count = 0;
    const char* p = s;
    while ((p = strstr(p, from)) != NULL) { count++; p += lf; }

    if (count == 0) return gc_strdup_n(s, ls);

    size_t new_len = ls + count * (lt - lf);  /* może być <ls jeśli lt<lf */
    if ((int64_t)new_len < 0) new_len = 0;

    char* r   = (char*)gc_malloc(new_len + 1);
    if (!r) return (char*)"";

    char*       dst = r;
    const char* src = s;
    const char* found;
    while ((found = strstr(src, from)) != NULL) {
        size_t chunk = (size_t)(found - src);
        memcpy(dst, src, chunk);
        dst += chunk;
        memcpy(dst, to, lt);
        dst += lt;
        src  = found + lf;
    }
    strcpy(dst, src);  /* reszta po ostatnim wystąpieniu */
    return r;
}

char* hl_str_slice(const char* s, int64_t start, int64_t end) {
    s = safe(s);
    int64_t len = (int64_t)strlen(s);

    /* Normalizacja ujemnych indeksów */
    if (start < 0) start = len + start;
    if (end   < 0) end   = len + end;

    /* Clamp */
    if (start < 0)   start = 0;
    if (end   > len) end   = len;
    if (start >= end) return (char*)"";

    return gc_strdup_n(s + start, (size_t)(end - start));
}

char* hl_str_repeat(const char* s, int64_t n) {
    s = safe(s);
    if (n <= 0) return (char*)"";
    size_t ls  = strlen(s);
    size_t tot = ls * (size_t)n;
    char*  r   = (char*)gc_malloc(tot + 1);
    if (!r) return (char*)"";
    for (int64_t i = 0; i < n; i++)
        memcpy(r + (size_t)i * ls, s, ls);
    r[tot] = '\0';
    return r;
}

char* hl_str_rev(const char* s) {
    s = safe(s);
    size_t n = strlen(s);
    char*  r = (char*)gc_malloc(n + 1);
    if (!r) return (char*)"";
    for (size_t i = 0; i < n; i++)
        r[i] = s[n - 1 - i];
    r[n] = '\0';
    return r;
}

/* ══════════════════════════════════════════════════════════════
 * KONWERSJE
 * ══════════════════════════════════════════════════════════════ */

char* hl_i64_to_str(int64_t v) {
    char buf[32];
    int  n = snprintf(buf, sizeof(buf), "%lld", (long long)v);
    if (n <= 0) return (char*)"0";
    return gc_strdup_n(buf, (size_t)n);
}

char* hl_f64_to_str(double v) {
    char buf[64];
    int  n = snprintf(buf, sizeof(buf), "%g", v);
    if (n <= 0) return (char*)"0";
    return gc_strdup_n(buf, (size_t)n);
}

int64_t hl_str_to_i64(const char* s) {
    if (!s || *s == '\0') return 0;
    char* end;
    long long v = strtoll(s, &end, 10);
    return (int64_t)v;
}

double hl_str_to_f64(const char* s) {
    if (!s || *s == '\0') return 0.0;
    char* end;
    double v = strtod(s, &end);
    return v;
}
