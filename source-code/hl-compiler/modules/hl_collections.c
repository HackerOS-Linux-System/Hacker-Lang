/*
 * hl_collections.c — hacker-lang collections runtime Level 2
 *
 * HlList — dynamiczna lista stringów
 *   Wewnętrznie: tablica wskaźników char** zarządzana przez gc_malloc.
 *   Wzrost: x2 przy pełnym buforze (amortyzowane O(1) push).
 *
 * HlMap — hash mapa string → string
 *   Wewnętrznie: tablica otwartego adresowania z liniowym próbkowaniem.
 *   Load factor < 0.75 — rehash przy przekroczeniu.
 *   Klucze i wartości: wskaźniki do gc_malloc stringów.
 *
 * Struktury alokowane przez gc_malloc — GC sprząta całość.
 * hl_list_free / hl_map_free: zwalniają wewnętrzne tablice (nie elementy).
 */

#include "hl_runtime.h"

#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stdbool.h>

/* ── Helpers ─────────────────────────────────────────────── */
static inline const char* safe_str(const char* s) {
    return s ? s : "";
}

/* Kopiuje string przez gc_malloc — GC-owned */
static char* gc_str_copy(const char* s) {
    if (!s) return (char*)"";
    size_t n   = strlen(s);
    char*  dst = (char*)gc_malloc(n + 1);
    if (!dst) return (char*)"";
    memcpy(dst, s, n + 1);
    return dst;
}

/* ══════════════════════════════════════════════════════════════
 * HlList
 * ══════════════════════════════════════════════════════════════ */

#define HL_LIST_INIT_CAP 8

struct HlList {
    char**  data;
    int64_t len;
    int64_t cap;
};

HlList* hl_list_new(void) {
    HlList* l = (HlList*)gc_malloc(sizeof(HlList));
    if (!l) return NULL;
    l->data = (char**)gc_malloc(sizeof(char*) * HL_LIST_INIT_CAP);
    if (!l->data) { return NULL; }
    l->len = 0;
    l->cap = HL_LIST_INIT_CAP;
    return l;
}

static bool hl_list_grow(HlList* l) {
    int64_t  new_cap  = l->cap * 2;
    char**   new_data = (char**)gc_malloc(sizeof(char*) * (size_t)new_cap);
    if (!new_data) return false;
    memcpy(new_data, l->data, sizeof(char*) * (size_t)l->len);
    /* Stara tablica zostanie zebrana przez GC przy następnym sweep */
    l->data = new_data;
    l->cap  = new_cap;
    return true;
}

void hl_list_push(HlList* l, const char* val) {
    if (!l) return;
    if (l->len >= l->cap) {
        if (!hl_list_grow(l)) return;
    }
    l->data[l->len++] = gc_str_copy(val);
}

char* hl_list_pop(HlList* l) {
    if (!l || l->len <= 0) return (char*)"";
    return l->data[--l->len];
}

char* hl_list_get(HlList* l, int64_t idx) {
    if (!l) return (char*)"";
    /* Ujemne indeksy od końca */
    if (idx < 0) idx = l->len + idx;
    if (idx < 0 || idx >= l->len) return (char*)"";
    return l->data[idx] ? l->data[idx] : (char*)"";
}

void hl_list_set(HlList* l, int64_t idx, const char* val) {
    if (!l) return;
    if (idx < 0) idx = l->len + idx;
    if (idx < 0 || idx >= l->len) return;
    l->data[idx] = gc_str_copy(val);
}

int64_t hl_list_len(HlList* l) {
    return l ? l->len : 0;
}

void hl_list_free(HlList* l) {
    /* GC sprząta elementy — tutaj tylko zerujemy strukturę
     * żeby uniknąć dangling pointers przy ewentualnym reuse */
    if (!l) return;
    l->data = NULL;
    l->len  = 0;
    l->cap  = 0;
    /* Sam HlList zostanie zebrany przez GC */
}

/* ══════════════════════════════════════════════════════════════
 * HlMap — otwarte adresowanie, liniowe próbkowanie
 * ══════════════════════════════════════════════════════════════ */

#define HL_MAP_INIT_CAP  16
#define HL_MAP_LOAD_NUM   3   /* load factor = 3/4 */
#define HL_MAP_LOAD_DEN   4

/* Sentinel dla usuniętych wpisów (tombstone) */
static const char HL_MAP_DELETED_KEY[] = "\x01__hl_deleted__";

typedef struct {
    char* key;
    char* val;
} HlMapEntry;

struct HlMap {
    HlMapEntry* entries;
    int64_t     len;    /* liczba aktywnych wpisów */
    int64_t     cap;    /* rozmiar tablicy (zawsze potęga 2) */
};

/* FNV-1a hash — szybki, dobra dystrybucja dla krótkich kluczy */
static uint64_t hl_map_hash(const char* key) {
    uint64_t h = 14695981039346656037ULL;
    for (const uint8_t* p = (const uint8_t*)key; *p; p++) {
        h ^= (uint64_t)*p;
        h *= 1099511628211ULL;
    }
    return h;
}

static HlMap* hl_map_alloc(int64_t cap) {
    HlMap* m = (HlMap*)gc_malloc(sizeof(HlMap));
    if (!m) return NULL;
    m->entries = (HlMapEntry*)gc_malloc(sizeof(HlMapEntry) * (size_t)cap);
    if (!m->entries) return NULL;
    memset(m->entries, 0, sizeof(HlMapEntry) * (size_t)cap);
    m->len = 0;
    m->cap = cap;
    return m;
}

HlMap* hl_map_new(void) {
    return hl_map_alloc(HL_MAP_INIT_CAP);
}

static void hl_map_rehash(HlMap* m) {
    int64_t     old_cap     = m->cap;
    HlMapEntry* old_entries = m->entries;
    int64_t     new_cap     = old_cap * 2;

    HlMapEntry* new_entries = (HlMapEntry*)gc_malloc(
        sizeof(HlMapEntry) * (size_t)new_cap);
    if (!new_entries) return;
    memset(new_entries, 0, sizeof(HlMapEntry) * (size_t)new_cap);

    int64_t new_len = 0;
    for (int64_t i = 0; i < old_cap; i++) {
        HlMapEntry* e = &old_entries[i];
        if (!e->key || e->key == HL_MAP_DELETED_KEY) continue;

        uint64_t h   = hl_map_hash(e->key);
        int64_t  idx = (int64_t)(h & (uint64_t)(new_cap - 1));
        while (new_entries[idx].key != NULL) {
            idx = (idx + 1) & (new_cap - 1);
        }
        new_entries[idx].key = e->key;
        new_entries[idx].val = e->val;
        new_len++;
    }

    m->entries = new_entries;
    m->cap     = new_cap;
    m->len     = new_len;
    /* old_entries zostanie zebrana przez GC */
}

void hl_map_set(HlMap* m, const char* key, const char* val) {
    if (!m || !key) return;

    /* Rehash jeśli load factor > 3/4 */
    if (m->len * HL_MAP_LOAD_DEN >= m->cap * HL_MAP_LOAD_NUM) {
        hl_map_rehash(m);
    }

    uint64_t h   = hl_map_hash(key);
    int64_t  idx = (int64_t)(h & (uint64_t)(m->cap - 1));
    int64_t  first_tombstone = -1;

    for (;;) {
        HlMapEntry* e = &m->entries[idx];

        if (!e->key) {
            /* Pusta pozycja — wstaw */
            int64_t insert_idx = (first_tombstone >= 0) ? first_tombstone : idx;
            m->entries[insert_idx].key = gc_str_copy(key);
            m->entries[insert_idx].val = gc_str_copy(safe_str(val));
            m->len++;
            return;
        }

        if (e->key == HL_MAP_DELETED_KEY) {
            if (first_tombstone < 0) first_tombstone = idx;
            idx = (idx + 1) & (m->cap - 1);
            continue;
        }

        if (strcmp(e->key, key) == 0) {
            /* Aktualizacja istniejącego */
            e->val = gc_str_copy(safe_str(val));
            return;
        }

        idx = (idx + 1) & (m->cap - 1);
    }
}

char* hl_map_get(HlMap* m, const char* key) {
    if (!m || !key) return (char*)"";

    uint64_t h   = hl_map_hash(key);
    int64_t  idx = (int64_t)(h & (uint64_t)(m->cap - 1));

    for (;;) {
        HlMapEntry* e = &m->entries[idx];
        if (!e->key) return (char*)"";
        if (e->key != HL_MAP_DELETED_KEY && strcmp(e->key, key) == 0)
            return e->val ? e->val : (char*)"";
        idx = (idx + 1) & (m->cap - 1);
    }
}

bool hl_map_has(HlMap* m, const char* key) {
    if (!m || !key) return false;

    uint64_t h   = hl_map_hash(key);
    int64_t  idx = (int64_t)(h & (uint64_t)(m->cap - 1));

    for (;;) {
        HlMapEntry* e = &m->entries[idx];
        if (!e->key) return false;
        if (e->key != HL_MAP_DELETED_KEY && strcmp(e->key, key) == 0)
            return true;
        idx = (idx + 1) & (m->cap - 1);
    }
}

void hl_map_del(HlMap* m, const char* key) {
    if (!m || !key) return;

    uint64_t h   = hl_map_hash(key);
    int64_t  idx = (int64_t)(h & (uint64_t)(m->cap - 1));

    for (;;) {
        HlMapEntry* e = &m->entries[idx];
        if (!e->key) return;
        if (e->key != HL_MAP_DELETED_KEY && strcmp(e->key, key) == 0) {
            /* Tombstone — zachowuje spójność próbkowania */
            e->key = (char*)HL_MAP_DELETED_KEY;
            e->val = NULL;
            m->len--;
            return;
        }
        idx = (idx + 1) & (m->cap - 1);
    }
}

int64_t hl_map_len(HlMap* m) {
    return m ? m->len : 0;
}

void hl_map_free(HlMap* m) {
    if (!m) return;
    m->entries = NULL;
    m->len     = 0;
    m->cap     = 0;
}
