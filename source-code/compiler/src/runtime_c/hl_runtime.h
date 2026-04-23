/*
 * hl_runtime.h — Hacker Lang Runtime (C)
 *
 * Ten plik jest WEWNETRZNY — uzytkownik nigdy go nie widzi.
 * Kompilowany przez hl-compiler jako obiekt i linkowany do kazdej binarki HL.
 *
 * ABI (wywolywane z kodu Cranelift):
 *   hl_runtime_init()          — inicjalizacja (wywolywana z main)
 *   hl_runtime_shutdown()      — sprzatanie (wywolywana z main)
 *   hl_print(msg)              — wypisz string na stdout
 *   hl_print_interp(tmpl)      — wypisz string z interpolacja @zmiennych
 *   hl_run_cmd(cmd, mode)      — uruchom komende (mode: 0=plain, 1=sudo, 2=iso, 3=iso+sudo, 4=vars, ...)
 *   hl_set_var(name, val)      — ustaw zmienna lokalna HL
 *   hl_set_var_interp(n, tmpl) — ustaw zmienna z interpolacja
 *   hl_export_var(name, val)   — setenv() + zmienna HL
 *   hl_export_list(n, items, c)— export listy (dolaczone ':')
 *   hl_quick(name, args)       — wywolaj quick-funkcje (::name args)
 *   hl_dep_check(name)         — sprawdz zaleznosc
 *   hl_get_last_exit()         — zwroc ostatni exit code
 */

#ifndef HL_RUNTIME_H
#define HL_RUNTIME_H

#ifdef __cplusplus
extern "C" {
#endif

void hl_runtime_init(void);
void hl_runtime_shutdown(void);

void hl_print(const char *msg);
void hl_print_interp(const char *tmpl);

int  hl_run_cmd(const char *cmd, int mode);

void hl_set_var(const char *name, const char *value);
void hl_set_var_interp(const char *name, const char *tmpl);
void hl_export_var(const char *name, const char *value);
void hl_export_var_interp(const char *name, const char *tmpl);
void hl_export_list(const char *name, const char **items, int count);

int  hl_quick(const char *name, const char *args);
int  hl_dep_check(const char *name);
int  hl_get_last_exit(void);

#ifdef __cplusplus
}
#endif

#endif /* HL_RUNTIME_H */
