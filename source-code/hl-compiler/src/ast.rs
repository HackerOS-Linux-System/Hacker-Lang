use serde::Deserialize;
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────
// Helpers: serde default functions
// ─────────────────────────────────────────────────────────────

fn default_false() -> bool { false }
fn default_empty_string() -> String { String::new() }
fn default_empty_vec<T>() -> Vec<T> { Vec::new() }
fn default_span() -> (usize, usize) { (0, 0) }
fn default_arena_size() -> Option<String> { None }
fn default_zero_u64() -> u64 { 0 }
fn default_zero_i32() -> i32 { 0 }

// ─────────────────────────────────────────────────────────────
// Typy bibliotek (musi pasować do hl-plsa LibType)
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    /// vira  → ~/.hackeros/hacker-lang/libs/.virus/ (repo git)
    Vira,
    /// virus → ~/.hackeros/hacker-lang/libs/.virus/ (alias vira)
    Virus,
    /// bytes → ~/.hackeros/hacker-lang/libs/bytes/  (.so / .a)
    Bytes,
    /// core  → ~/.hackeros/hacker-lang/libs/core/   (.hl)
    Core,
    /// github → ~/.hackeros/hacker-lang/libs/github/ (repo gh)
    Github,
    /// source → pliki .hl w projekcie (import lokalny)
    Source,
}

// ─────────────────────────────────────────────────────────────
// Referencja do biblioteki (#<lib_type/name:version>)
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

// ─────────────────────────────────────────────────────────────
// CommandType — pełne AST hacker-lang
//
// Zasady deserializacji:
//   - Wszystkie pola String mogą mieć #[serde(default)] gdzie
//     brakujące pole z hl-plsa nie powinno blokować kompilacji.
//   - Vec<T> zawsze #[serde(default)] = pusty Vec.
//   - bool zawsze #[serde(default)] = false.
//   - Option<T> obsługuje null i brak pola automatycznie przez serde.
//   - (usize, usize) span ma #[serde(default)] = (0, 0).
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {

    // ── Komendy wykonywalne ───────────────────────────────────

    /// >> cmd — surowa komenda bez podstawień zmiennych
    RawNoSub(String),
    /// |> cmd / > cmd — komenda z podstawieniem
    RawSub(String),
    /// >>> cmd — izolowana komenda (osobny podpowłok)
    Isolated(String),

    // ── Zmienne i przypisania ─────────────────────────────────

    /// @key = val — zmienna globalna (env)
    AssignEnv {
        key: String,
        val: String,
    },
    /// key = val  — zmienna lokalna
    /// ~key = val — przypisanie bez quote-stripping (is_raw=true)
    AssignLocal {
        key:    String,
        val:    String,
        #[serde(default = "default_false")]
        is_raw: bool,
    },
    /// key = 2 + 3 * 4 / key = [$a, $b] / key = {k: "v"}
    AssignExpr {
        key:       String,
        expr:      String,
        #[serde(default = "default_false")]
        is_raw:    bool,
        #[serde(default = "default_false")]
        is_global: bool,
    },
    /// % KEY = val — stała
    Const {
        key: String,
        val: String,
    },

    // ── Przepływ sterowania ───────────────────────────────────

    /// = N > cmd  — pętla N razy
    Loop {
        #[serde(default = "default_zero_u64")]
        count: u64,
        cmd:   String,
    },
    /// ? cond > cmd — if
    If   { cond: String, cmd: String },
    /// ?? cond > cmd — elif
    Elif { cond: String, cmd: String },
    /// ?: > cmd   — else
    Else { cmd: String },
    /// while cond > cmd
    While { cond: String, cmd: String },
    /// for x in cond > cmd
    For   { var: String, in_: String, cmd: String },
    /// end [code]
    End   {
        #[serde(default = "default_zero_i32")]
        code: i32,
    },
    /// out [val]
    Out(String),

    // ── Wywołania ─────────────────────────────────────────────

    /// .path args — wywołanie funkcji/metody
    Call {
        path: String,
        #[serde(default = "default_empty_string")]
        args: String,
    },
    /// module.method args — wywołanie metody modułu
    ModuleCall {
        path: String,
        #[serde(default = "default_empty_string")]
        args: String,
    },
    /// \\ plugin [args]
    Plugin {
        name:     String,
        #[serde(default = "default_empty_string")]
        args:     String,
        #[serde(default = "default_false")]
        is_super: bool,
    },
    /// -- [static] path
    Extern {
        path:        String,
        #[serde(default = "default_false")]
        static_link: bool,
    },

    // ── Import i zależności ───────────────────────────────────

    /// // dep — zależność systemowa
    SysDep(String),
    /// << "path" [in ns]
    Import {
        resource:  String,
        namespace: Option<String>,
    },

    // ── Typy i struktury ──────────────────────────────────────

    /// == Name [V1, V2] — enum
    Enum {
        name:     String,
        #[serde(default = "default_empty_vec")]
        variants: Vec<String>,
    },
    /// struct Name [field: type, ...]
    Struct {
        name:   String,
        #[serde(default = "default_empty_vec")]
        fields: Vec<(String, String)>,
    },
    /// ==type Shape [...] — ADT
    AdtDef {
        name:     String,
        #[serde(default = "default_empty_vec")]
        variants: Vec<(String, Vec<(String, String)>)>,
    },
    /// ==interface Name [methods]
    Interface {
        name:    String,
        #[serde(default = "default_empty_vec")]
        methods: Vec<String>,
    },
    /// ;;Class impl Interface def
    ImplDef { class: String, interface: String },

    // ── Funkcje i klasy ───────────────────────────────────────

    FuncDefGeneric {
        name: String,
        sig:  String,
    },

    // ── Arena allocator ───────────────────────────────────────
    //
    // FIX: size_spec jest Option<String> z #[serde(default)].
    //
    // Dlaczego: hl-plsa emituje ArenaDef na dwa sposoby:
    //
    //   1. Standalone w main_body / ciele funkcji:
    //      { "type": "ArenaDef", "data": { "name": "cache", "size_spec": "512kb" } }
    //      Tutaj size_spec jest w JSON.
    //
    //   2. Jako marker że funkcja ma własną arenę (is_arena_fn=true w functions[]):
    //      { "type": "ArenaDef", "data": { "name": "cache" } }
    //      size_spec NIE jest w JSON — jest zakodowane w sygnaturze "[arena:512kb]".
    //      To powoduje błąd "missing field size_spec" w starym ast.rs.
    //
    //   Rozwiązanie: size_spec: Option<String>, body: Vec<ProgramNode> z default.
    //   ir.rs/codegen.rs powinny użyć resolve_arena_size() żeby wyciągnąć
    //   rozmiar z sygnatury funkcji jeśli size_spec jest None.

    /// :: name [size_spec] def...done — blok z dedykowaną areną
    ArenaDef {
        name: String,

        /// Rozmiar areny: "512b" | "4kb" | "1mb" | "2gb"
        /// None gdy hl-plsa emituje ArenaDef jako marker is_arena_fn —
        /// rozmiar jest wtedy w sygnaturze functions[name].1 = "[arena:512kb]"
        #[serde(default = "default_arena_size")]
        size_spec: Option<String>,

        /// Ciało bloku areny — węzły wewnątrz def...done.
        /// Puste gdy ArenaDef jest markerem w functions[] (ciało jest osobno).
        #[serde(default = "default_empty_vec")]
        body: Vec<ProgramNode>,
    },

    /// key = arena.name.alloc size_bytes
    ArenaAlloc {
        key:        String,
        arena_name: String,
        #[serde(default = "default_zero_u64")]
        size:       u64,
    },
    /// arena.name.reset
    ArenaReset {
        arena_name: String,
    },
    /// arena.name.free
    ArenaFree {
        arena_name: String,
    },

    // ── Kolekcje ─────────────────────────────────────────────

    CollectionMut {
        var:    String,
        method: String,
        #[serde(default = "default_empty_string")]
        args:   String,
    },

    // ── Error handling ────────────────────────────────────────

    Try {
        try_cmd:   String,
        catch_cmd: String,
    },
    ResultUnwrap {
        expr: String,
        msg:  String,
    },

    // ── Async ─────────────────────────────────────────────────

    Spawn(String),
    Await(String),
    AssignSpawn {
        key:  String,
        task: String,
    },
    AssignAwait {
        key:  String,
        expr: String,
    },

    // ── Pattern matching ──────────────────────────────────────

    Match    { cond: String },
    MatchArm { val: String, cmd: String },

    // ── Pipe ──────────────────────────────────────────────────

    Pipe(Vec<String>),
    PipeLine { step: String },

    // ── Lambdy ────────────────────────────────────────────────

    Lambda {
        #[serde(default = "default_empty_vec")]
        params: Vec<String>,
        body:   String,
    },
    AssignLambda {
        key:       String,
        #[serde(default = "default_empty_vec")]
        params:    Vec<String>,
        body:      String,
        #[serde(default = "default_false")]
        is_raw:    bool,
        #[serde(default = "default_false")]
        is_global: bool,
    },

    // ── Rekurencja ogonowa ────────────────────────────────────

    Recur {
        #[serde(default = "default_empty_string")]
        args: String,
    },

    // ── Destrukturyzacja ──────────────────────────────────────

    DestructList {
        head:   String,
        tail:   String,
        source: String,
    },
    DestructMap {
        #[serde(default = "default_empty_vec")]
        fields: Vec<String>,
        source: String,
    },

    // ── Zasięg leksykalny ─────────────────────────────────────

    ScopeDef,

    // ── Do-notacja ────────────────────────────────────────────

    DoBlock {
        key:  String,
        #[serde(default = "default_empty_vec")]
        body: Vec<ProgramNode>,
    },

    // ── Testy jednostkowe ─────────────────────────────────────

    TestBlock {
        desc: String,
        #[serde(default = "default_empty_vec")]
        body: Vec<ProgramNode>,
    },

    // ── Walidacja ─────────────────────────────────────────────

    Assert {
        cond: String,
        msg:  Option<String>,
    },

    // ── Inne ──────────────────────────────────────────────────

    Log(String),
    Lock   { key: String, val: String },
    Unlock { key: String },
    Background(String),
    Defer  { expr: String },
}

// ─────────────────────────────────────────────────────────────
// ProgramNode — pojedynczy węzeł AST
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
pub struct ProgramNode {
    #[serde(default)]
    pub line_num:      usize,
    #[serde(default = "default_false")]
    pub is_sudo:       bool,
    pub content:       CommandType,
    #[serde(default = "default_empty_string")]
    pub original_text: String,
    /// (byte_offset, byte_len) w pliku źródłowym.
    /// FIX: #[serde(default)] bo hl-plsa może emitować span jako []
    /// lub w ogóle nie emitować tego pola dla starszych węzłów.
    #[serde(default = "default_span")]
    pub span:          (usize, usize),
}

// ─────────────────────────────────────────────────────────────
// AnalysisResult — wynik analizy hl-plsa
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    /// Zależności systemowe (//)
    #[serde(default = "default_empty_vec")]
    pub deps:  Vec<String>,
    /// Biblioteki (#<lib_type/name:version>)
    #[serde(default = "default_empty_vec")]
    pub libs:  Vec<LibRef>,
    /// Funkcje i metody klas:
    ///   key   = "NazwaKlasy.nazwa_funkcji" lub "nazwa_funkcji"
    ///   value = (is_arena_fn, Option<type_sig>, Vec<ProgramNode>)
    #[serde(default)]
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    /// Kod poza funkcjami (globalny main body)
    #[serde(default = "default_empty_vec")]
    pub main_body:             Vec<ProgramNode>,
    /// true jeśli jakikolwiek węzeł ma is_sudo=true
    #[serde(default = "default_false")]
    pub is_potentially_unsafe: bool,
    /// Lista ostrzeżeń sudo
    #[serde(default = "default_empty_vec")]
    pub safety_warnings:       Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Helpers — metody na AnalysisResult
// ─────────────────────────────────────────────────────────────
impl AnalysisResult {
    /// Wszystkie węzły (main_body + wnętrza funkcji) — flat iterator.
    pub fn all_nodes(&self) -> impl Iterator<Item = &ProgramNode> {
        self.main_body
        .iter()
        .chain(self.functions.values().flat_map(|(_, _, nodes)| nodes.iter()))
    }

    /// true jeśli program używa arena (:: blok).
    pub fn uses_arena(&self) -> bool {
        self.functions.values().any(|(is_arena, _, _)| *is_arena)
        || self.all_nodes().any(|n| matches!(&n.content, CommandType::ArenaDef { .. }))
    }

    /// true jeśli program używa spawn/await (wymaga -lpthread).
    pub fn uses_async(&self) -> bool {
        self.all_nodes().any(|n| matches!(
            &n.content,
            CommandType::Spawn(_)
            | CommandType::Await(_)
            | CommandType::AssignSpawn { .. }
            | CommandType::AssignAwait { .. }
        ))
    }

    /// Lista extern libs: (path, is_static).
    pub fn extern_libs(&self) -> Vec<(String, bool)> {
        self.all_nodes()
        .filter_map(|n| match &n.content {
            CommandType::Extern { path, static_link } => Some((path.clone(), *static_link)),
                    _ => None,
        })
        .collect()
    }
}

// ─────────────────────────────────────────────────────────────
// resolve_arena_size — wyciągnij rozmiar areny
//
// FIX: ArenaDef.size_spec może być None gdy hl-plsa emituje
// ArenaDef jako marker is_arena_fn. Rozmiar jest wtedy
// zakodowany w sygnaturze funkcji: "[arena:512kb]".
//
// Używane przez ir.rs i codegen.rs zamiast bezpośredniego
// dostępu do size_spec.
// ─────────────────────────────────────────────────────────────
pub fn resolve_arena_size(
    size_spec:   Option<&str>,
    func_sig:    Option<&str>,
    default_val: &str,
) -> String {
    // 1. Bezpośredni size_spec z węzła ArenaDef
    if let Some(s) = size_spec {
        if !s.is_empty() {
            return s.to_string();
        }
    }

    // 2. Wyciągnij z sygnatury funkcji "[arena:512kb]"
    if let Some(sig) = func_sig {
        if let Some(inner) = sig.strip_prefix("[arena:") {
            if let Some(size) = inner.strip_suffix(']') {
                if !size.is_empty() {
                    return size.to_string();
                }
            }
        }
        // Alternatywna forma: "arena:512kb" bez nawiasów
        if let Some(rest) = sig.strip_prefix("arena:") {
            let size = rest.trim_end_matches(']').trim();
            if !size.is_empty() {
                return size.to_string();
            }
        }
    }

    // 3. Fallback
    default_val.to_string()
}

// ─────────────────────────────────────────────────────────────
// Testy jednostkowe
// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arena_def_without_size_spec() {
        // hl-plsa emituje ArenaDef bez size_spec — nie powinno panikować
        let json = r#"{
        "type": "ArenaDef",
        "data": { "name": "cache" }
    }"#;
    let ct: CommandType = serde_json::from_str(json).expect("deserializacja ArenaDef bez size_spec");
    match ct {
        CommandType::ArenaDef { name, size_spec, body } => {
            assert_eq!(name, "cache");
            assert!(size_spec.is_none());
            assert!(body.is_empty());
        }
        _ => panic!("Zły wariant"),
    }
    }

    #[test]
    fn test_arena_def_with_size_spec() {
        let json = r#"{
        "type": "ArenaDef",
        "data": { "name": "buf", "size_spec": "512kb" }
    }"#;
    let ct: CommandType = serde_json::from_str(json).expect("deserializacja ArenaDef z size_spec");
    match ct {
        CommandType::ArenaDef { name, size_spec, .. } => {
            assert_eq!(name, "buf");
            assert_eq!(size_spec.as_deref(), Some("512kb"));
        }
        _ => panic!("Zły wariant"),
    }
    }

    #[test]
    fn test_program_node_without_span() {
        // hl-plsa może nie emitować span
        let json = r#"{
        "line_num": 10,
        "is_sudo": false,
        "content": { "type": "Log", "data": "hello" },
        "original_text": "log hello"
    }"#;
    let node: ProgramNode = serde_json::from_str(json).expect("ProgramNode bez span");
    assert_eq!(node.span, (0, 0));
    }

    #[test]
    fn test_resolve_arena_size_from_spec() {
        assert_eq!(resolve_arena_size(Some("512kb"), None, "64kb"), "512kb");
    }

    #[test]
    fn test_resolve_arena_size_from_sig() {
        assert_eq!(resolve_arena_size(None, Some("[arena:1mb]"), "64kb"), "1mb");
    }

    #[test]
    fn test_resolve_arena_size_fallback() {
        assert_eq!(resolve_arena_size(None, None, "64kb"), "64kb");
        assert_eq!(resolve_arena_size(Some(""), Some(""), "64kb"), "64kb");
    }

    #[test]
    fn test_assign_local_without_is_raw() {
        // is_raw powinno defaultować do false
        let json = r#"{
        "type": "AssignLocal",
        "data": { "key": "x", "val": "42" }
    }"#;
    let ct: CommandType = serde_json::from_str(json).unwrap();
    match ct {
        CommandType::AssignLocal { is_raw, .. } => assert!(!is_raw),
        _ => panic!(),
    }
    }
}
