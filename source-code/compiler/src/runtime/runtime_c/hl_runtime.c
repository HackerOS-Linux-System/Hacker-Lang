/*
 * hl_runtime.c — Hacker Lang Runtime (C)
 *
 * Kompilowany i linkowany do kazdej binarki/biblioteki skompilowanej przez hl-compiler.
 * Uzytkownik nigdy nie widzi tego pliku — jest on automatycznie dolaczany przez kompilator.
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/types.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <errno.h>
#include <ctype.h>

#include "hl_runtime.h"

/* ── Tablica zmiennych HL ─────────────────────────────────────────────────── */

#define HL_VAR_MAX     512
#define HL_VAR_NAME_SZ 128
#define HL_VAR_VAL_SZ  4096
#define HL_INTERP_SZ   8192
#define HL_CMD_SZ      8192

typedef struct {
    char name[HL_VAR_NAME_SZ];
    char value[HL_VAR_VAL_SZ];
    int  used;
} HlVar;

static HlVar  g_vars[HL_VAR_MAX];
static int    g_var_count  = 0;
static int    g_last_exit  = 0;

/* ── Init / Shutdown ─────────────────────────────────────────────────────── */

void hl_runtime_init(void) {
    memset(g_vars, 0, sizeof(g_vars));
    g_var_count = 0;
    g_last_exit = 0;

    /* Wbudowane zmienne srodowiskowe */
    hl_set_var("HL_VERSION", "0.4.0");
    hl_set_var("HL_OS",      "HackerOS");

    /* Przekaz zmienne srodowiskowe do HL */
    const char *env_keys[] = {
        "HOME", "USER", "PATH", "SHELL", "LANG",
        "DISPLAY", "XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS",
        NULL
    };
    for (int i = 0; env_keys[i] != NULL; i++) {
        const char *v = getenv(env_keys[i]);
        if (v) hl_set_var(env_keys[i], v);
    }
}

void hl_runtime_shutdown(void) {
    /* Mozna tu dodac cleanup jesli potrzebny */
}

/* ── Zmienne ─────────────────────────────────────────────────────────────── */

static HlVar *hl_find_var(const char *name) {
    for (int i = 0; i < g_var_count; i++) {
        if (g_vars[i].used && strcmp(g_vars[i].name, name) == 0)
            return &g_vars[i];
    }
    return NULL;
}

static HlVar *hl_alloc_var(const char *name) {
    HlVar *v = hl_find_var(name);
    if (v) return v;
    if (g_var_count >= HL_VAR_MAX) {
        fprintf(stderr, "[hl runtime] FATAL: za duzo zmiennych (max %d)\n", HL_VAR_MAX);
        exit(1);
    }
    v = &g_vars[g_var_count++];
    v->used = 1;
    strncpy(v->name, name, HL_VAR_NAME_SZ - 1);
    v->name[HL_VAR_NAME_SZ - 1] = '\0';
    return v;
}

void hl_set_var(const char *name, const char *value) {
    if (!name || !value) return;
    HlVar *v = hl_alloc_var(name);
    strncpy(v->value, value, HL_VAR_VAL_SZ - 1);
    v->value[HL_VAR_VAL_SZ - 1] = '\0';
}

static const char *hl_get_var(const char *name) {
    if (!name) return "";
    HlVar *v = hl_find_var(name);
    if (v) return v->value;
    /* Sprawdz srodowisko */
    const char *env = getenv(name);
    return env ? env : "";
}

/* ── Interpolacja @zmiennych ─────────────────────────────────────────────── */

static void hl_interpolate(const char *tmpl, char *out, size_t out_sz) {
    size_t ti = 0, oi = 0;
    size_t tlen = strlen(tmpl);

    while (ti < tlen && oi + 1 < out_sz) {
        if (tmpl[ti] == '@' && ti + 1 < tlen
            && (isalpha((unsigned char)tmpl[ti+1]) || tmpl[ti+1] == '_'))
        {
            /* Parsuj nazwe zmiennej */
            ti++;
            char varname[HL_VAR_NAME_SZ] = {0};
            size_t vi = 0;
            while (ti < tlen && vi + 1 < HL_VAR_NAME_SZ
                   && (isalnum((unsigned char)tmpl[ti]) || tmpl[ti] == '_'))
            {
                varname[vi++] = tmpl[ti++];
            }
            varname[vi] = '\0';
            const char *val = hl_get_var(varname);
            size_t vlen = strlen(val);
            if (oi + vlen < out_sz) {
                memcpy(out + oi, val, vlen);
                oi += vlen;
            }
        } else {
            out[oi++] = tmpl[ti++];
        }
    }
    out[oi] = '\0';
}

/* ── Print ───────────────────────────────────────────────────────────────── */

void hl_print(const char *msg) {
    if (!msg) return;
    puts(msg);
}

void hl_print_interp(const char *tmpl) {
    if (!tmpl) return;
    char buf[HL_INTERP_SZ];
    hl_interpolate(tmpl, buf, sizeof(buf));
    puts(buf);
}

/* ── Uruchamianie komend ─────────────────────────────────────────────────── */

/*
 * Tryby:
 *   0 = plain
 *   1 = sudo
 *   2 = isolated (unshare)
 *   3 = isolated + sudo
 *   4 = z interpolacja @vars (plain)
 *   5 = z interpolacja + sudo
 *   6 = z interpolacja + isolated
 */
int hl_run_cmd(const char *cmd, int mode) {
    if (!cmd) return 1;

    char expanded[HL_CMD_SZ];

    /* Interpolacja dla trybów >= 4 */
    if (mode >= 4) {
        hl_interpolate(cmd, expanded, sizeof(expanded));
    } else {
        strncpy(expanded, cmd, HL_CMD_SZ - 1);
        expanded[HL_CMD_SZ - 1] = '\0';
    }

    int sudo     = (mode == 1 || mode == 3 || mode == 5);
    int isolated = (mode == 2 || mode == 3 || mode == 6);

    /* Buduj ostateczna komende */
    char full_cmd[HL_CMD_SZ * 2];
    if (isolated && sudo) {
        snprintf(full_cmd, sizeof(full_cmd),
                 "sudo unshare --mount --pid --net --fork -- sh -c %s", expanded);
    } else if (isolated) {
        snprintf(full_cmd, sizeof(full_cmd),
                 "unshare --mount --pid --net --fork -- sh -c %s", expanded);
    } else if (sudo) {
        snprintf(full_cmd, sizeof(full_cmd), "sudo sh -c '%s'", expanded);
    } else {
        strncpy(full_cmd, expanded, sizeof(full_cmd) - 1);
        full_cmd[sizeof(full_cmd) - 1] = '\0';
    }

    int ret = system(full_cmd);
    if (ret == -1) {
        g_last_exit = 127;
    } else if (WIFEXITED(ret)) {
        g_last_exit = WEXITSTATUS(ret);
    } else if (WIFSIGNALED(ret)) {
        g_last_exit = 128 + WTERMSIG(ret);
    } else {
        g_last_exit = ret;
    }

    return g_last_exit;
}

/* ── Export ──────────────────────────────────────────────────────────────── */

void hl_export_var(const char *name, const char *value) {
    if (!name || !value) return;
    hl_set_var(name, value);
    setenv(name, value, 1);
}

void hl_export_var_interp(const char *name, const char *tmpl) {
    if (!name || !tmpl) return;
    char buf[HL_VAR_VAL_SZ];
    hl_interpolate(tmpl, buf, sizeof(buf));
    hl_export_var(name, buf);
}

void hl_export_list(const char *name, const char **items, int count) {
    if (!name || !items || count <= 0) return;

    char joined[HL_VAR_VAL_SZ] = {0};
    size_t off = 0;

    for (int i = 0; i < count && off < HL_VAR_VAL_SZ - 1; i++) {
        if (!items[i]) continue;
        size_t len = strlen(items[i]);
        if (off + len < HL_VAR_VAL_SZ - 1) {
            memcpy(joined + off, items[i], len);
            off += len;
        }
        if (i < count - 1 && off < HL_VAR_VAL_SZ - 1) {
            joined[off++] = ':';
        }
    }
    joined[off] = '\0';

    hl_export_var(name, joined);
}

void hl_set_var_interp(const char *name, const char *tmpl) {
    if (!name || !tmpl) return;
    char buf[HL_VAR_VAL_SZ];
    hl_interpolate(tmpl, buf, sizeof(buf));
    hl_set_var(name, buf);
}

/* ── Zaleznosci ──────────────────────────────────────────────────────────── */

int hl_dep_check(const char *name) {
    if (!name) return 1;

    /* Sprawdz czy narzedzie jest w PATH */
    char cmd[256];
    snprintf(cmd, sizeof(cmd), "command -v '%s' >/dev/null 2>&1", name);
    int ret = system(cmd);

    if (ret == 0) return 0;

    /* Proba instalacji */
    fprintf(stderr, "\033[33m[hl dep]\033[0m '%s' nie znalezione. Instaluje...\n", name);

    snprintf(cmd, sizeof(cmd), "sudo apt-get install -y '%s' >/dev/null 2>&1", name);
    ret = system(cmd);
    if (ret == 0) {
        fprintf(stderr, "\033[32m[hl dep]\033[0m '%s' zainstalowane.\n", name);
        return 0;
    }

    fprintf(stderr, "\033[31m[hl dep]\033[0m Nie udalo sie zainstalowac '%s'.\n", name);
    return 1;
}

/* ── Quick functions ─────────────────────────────────────────────────────── */

int hl_quick(const char *name, const char *args) {
    if (!name) return 1;

    const char *a = args ? args : "";

    /* Kolory i formatowanie */
    if (strcmp(name, "red")    == 0) { printf("\033[31m%s\033[0m\n", a); return 0; }
    if (strcmp(name, "green")  == 0) { printf("\033[32m%s\033[0m\n", a); return 0; }
    if (strcmp(name, "yellow") == 0) { printf("\033[33m%s\033[0m\n", a); return 0; }
    if (strcmp(name, "cyan")   == 0) { printf("\033[36m%s\033[0m\n", a); return 0; }
    if (strcmp(name, "bold")   == 0) { printf("\033[1m%s\033[0m\n",  a); return 0; }
    if (strcmp(name, "nl")     == 0) { puts(""); return 0; }

    if (strcmp(name, "hr") == 0) {
        int w = (*a != '\0') ? atoi(a) : 60;
        if (w <= 0 || w > 300) w = 60;
        for (int i = 0; i < w; i++) putchar('-');
        putchar('\n');
        return 0;
    }

    /* String operations */
    if (strcmp(name, "upper") == 0) {
        for (size_t i = 0; a[i]; i++) putchar(toupper((unsigned char)a[i]));
        putchar('\n'); return 0;
    }
    if (strcmp(name, "lower") == 0) {
        for (size_t i = 0; a[i]; i++) putchar(tolower((unsigned char)a[i]));
        putchar('\n'); return 0;
    }
    if (strcmp(name, "len") == 0) { printf("%zu\n", strlen(a)); return 0; }
    if (strcmp(name, "trim") == 0) {
        while (isspace((unsigned char)*a)) a++;
        size_t len = strlen(a);
        while (len > 0 && isspace((unsigned char)a[len-1])) len--;
        printf("%.*s\n", (int)len, a); return 0;
    }
    if (strcmp(name, "rev") == 0) {
        size_t len = strlen(a);
        for (size_t i = len; i > 0; i--) putchar(a[i-1]);
        putchar('\n'); return 0;
    }

    /* Filesystem */
    if (strcmp(name, "exists") == 0) { return (access(a, F_OK) == 0) ? 0 : 1; }
    if (strcmp(name, "isdir")  == 0) {
        struct stat st;
        return (stat(a, &st) == 0 && S_ISDIR(st.st_mode)) ? 0 : 1;
    }
    if (strcmp(name, "isfile") == 0) {
        struct stat st;
        return (stat(a, &st) == 0 && S_ISREG(st.st_mode)) ? 0 : 1;
    }
    if (strcmp(name, "read") == 0) {
        FILE *f = fopen(a, "r");
        if (!f) { fprintf(stderr, ":: read: nie mozna otworzyc '%s'\n", a); return 1; }
        char buf[4096];
        while (fgets(buf, sizeof(buf), f)) fputs(buf, stdout);
        fclose(f); return 0;
    }

    /* System */
    if (strcmp(name, "pid") == 0) { printf("%d\n", (int)getpid()); return 0; }
    if (strcmp(name, "env") == 0) {
        const char *v = getenv(a);
        if (v) { puts(v); return 0; }
        puts(""); return 1;
    }
    if (strcmp(name, "which") == 0) {
        char cmd2[256];
        snprintf(cmd2, sizeof(cmd2), "which '%s' 2>/dev/null", a);
        int r = system(cmd2);
        return WIFEXITED(r) ? WEXITSTATUS(r) : 1;
    }

    /* Interp set/get */
    if (strcmp(name, "set") == 0) {
        /* "name value" */
        char nm[HL_VAR_NAME_SZ]; const char *sp = strchr(a, ' ');
        if (!sp) { hl_set_var(a, ""); return 0; }
        size_t nlen = (size_t)(sp - a);
        if (nlen >= HL_VAR_NAME_SZ) nlen = HL_VAR_NAME_SZ - 1;
        memcpy(nm, a, nlen); nm[nlen] = '\0';
        hl_set_var(nm, sp + 1); return 0;
    }
    if (strcmp(name, "get") == 0) { puts(hl_get_var(a)); return 0; }

    /* Math */
    if (strcmp(name, "abs")   == 0) { double n = atof(a); printf("%.10g\n", n < 0 ? -n : n); return 0; }
    if (strcmp(name, "ceil")  == 0) { double n = atof(a); printf("%lld\n",  (long long)((n == (long long)n) ? (long long)n : (long long)n + (n > 0 ? 1 : 0))); return 0; }
    if (strcmp(name, "floor") == 0) { double n = atof(a); printf("%lld\n",  (long long)n); return 0; }
    if (strcmp(name, "round") == 0) { double n = atof(a); printf("%lld\n",  (long long)(n + 0.5)); return 0; }

    fprintf(stderr, "\033[31m[hl runtime]\033[0m Nieznana quick-funkcja '::%s'\n", name);
    return 1;
}

/* ── Last exit ───────────────────────────────────────────────────────────── */

int hl_get_last_exit(void) {
    return g_last_exit;
}
