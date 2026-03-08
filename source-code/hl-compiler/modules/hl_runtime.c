/*
 * hl_runtime.c — hacker-lang runtime Level 2
 *
 * Zastępuje:
 *   system("echo ...")          → hl_print / hl_print_i64 / hl_print_f64
 *   system("echo ... >&2")      → hl_log / hl_log_err
 *   system("export KEY=VAL")    → hl_setenv / hl_setenv_i64 / hl_setenv_f64
 *   getenv(key)                 → hl_getenv
 *
 * Zero fork(), zero execve() dla podstawowych operacji IO.
 * Wszystkie funkcje są oznaczone __attribute__((hot)) lub cold
 * stosownie do częstotliwości wywołań.
 */

#include "hl_runtime.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>
#include <unistd.h>

/* ── Wewnętrzny bufor dla konwersji liczb ──────────────────
 * Używamy statycznego bufora per-thread — w programach
 * single-thread (typowy output .hl) to jest bezpieczne.
 * Przy multi-thread wartości powinny być kopiowane od razu.
 * ─────────────────────────────────────────────────────────── */
#define HL_NUM_BUF_SIZE 64

/* ══════════════════════════════════════════════════════════════
 * OUTPUT
 * ══════════════════════════════════════════════════════════════ */

__attribute__((hot))
void hl_print(const char* s) {
    if (!s) {
        write(STDOUT_FILENO, "\n", 1);
        return;
    }
    size_t len = strlen(s);
    /* Używamy write() zamiast printf() — unikamy buforowania stdio
     * i mutex na FILE*. Dla małych stringów to jest szybsze. */
    if (len > 0) {
        write(STDOUT_FILENO, s, len);
    }
    write(STDOUT_FILENO, "\n", 1);
}

__attribute__((hot))
void hl_print_i64(int64_t v) {
    char buf[HL_NUM_BUF_SIZE];
    int  n = snprintf(buf, sizeof(buf), "%lld\n", (long long)v);
    if (n > 0) write(STDOUT_FILENO, buf, (size_t)n);
}

__attribute__((hot))
void hl_print_f64(double v) {
    char buf[HL_NUM_BUF_SIZE];
    int  n = snprintf(buf, sizeof(buf), "%g\n", v);
    if (n > 0) write(STDOUT_FILENO, buf, (size_t)n);
}

/* ══════════════════════════════════════════════════════════════
 * LOG — na stderr z prefiksem
 * ══════════════════════════════════════════════════════════════ */

__attribute__((cold))
void hl_log(const char* s) {
    if (!s) return;
    /* [hl] msg\n */
    write(STDERR_FILENO, "[hl] ", 5);
    write(STDERR_FILENO, s, strlen(s));
    write(STDERR_FILENO, "\n", 1);
}

__attribute__((cold))
void hl_log_err(const char* s) {
    if (!s) return;
    /* [hl:err] msg\n */
    write(STDERR_FILENO, "[hl:err] ", 9);
    write(STDERR_FILENO, s, strlen(s));
    write(STDERR_FILENO, "\n", 1);
}

/* ══════════════════════════════════════════════════════════════
 * ENV — setenv / getenv bez forka
 * ══════════════════════════════════════════════════════════════ */

__attribute__((hot))
void hl_setenv(const char* key, const char* val) {
    if (!key) return;
    if (!val) val = "";
    /* overwrite=1 — zawsze nadpisuj */
    setenv(key, val, 1);
}

__attribute__((hot))
void hl_setenv_i64(const char* key, int64_t val) {
    if (!key) return;
    char buf[HL_NUM_BUF_SIZE];
    snprintf(buf, sizeof(buf), "%lld", (long long)val);
    setenv(key, buf, 1);
}

__attribute__((hot))
void hl_setenv_f64(const char* key, double val) {
    if (!key) return;
    char buf[HL_NUM_BUF_SIZE];
    snprintf(buf, sizeof(buf), "%g", val);
    setenv(key, buf, 1);
}

__attribute__((hot))
const char* hl_getenv(const char* key) {
    if (!key) return "";
    const char* val = getenv(key);
    return val ? val : "";
}
