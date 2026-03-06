use serde::Deserialize;
use std::collections::HashMap;

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
    AssignEnv   { key: String, val: String },
    /// key = val  — zmienna lokalna
    /// ~key = val — przypisanie bez quote-stripping (is_raw=true)
    AssignLocal { key: String, val: String, is_raw: bool },
    /// key = 2 + 3 * 4 / key = [$a, $b] / key = {k: "v"}
    /// Wyrażenia arytmetyczne, logiczne, listy, mapy, interpolacje
    AssignExpr  { key: String, expr: String, is_raw: bool, is_global: bool },
    /// % KEY = val — stała (konwencja niezmienności)
    Const       { key: String, val: String },

    // ── Przepływ sterowania ───────────────────────────────────

    /// = N > cmd  — pętla N razy
    Loop        { count: u64, cmd: String },
    /// ? cond > cmd — if
    If          { cond: String, cmd: String },
    /// ?? cond > cmd — elif
    Elif        { cond: String, cmd: String },
    /// ?: > cmd   — else
    Else        { cmd: String },
    /// while cond > cmd
    While       { cond: String, cmd: String },
    /// for x in cond > cmd
    For         { var: String, in_: String, cmd: String },
    /// end [code] — wyjście z programu
    End         { code: i32 },
    /// out [val]  — output wartości (return z main)
    Out(String),

    // ── Wywołania ─────────────────────────────────────────────

    /// .path args — wywołanie funkcji/metody
    Call        { path: String, args: String },
    /// module.method args — wywołanie metody modułu (;;Moduł def)
    ModuleCall  { path: String, args: String },
    /// \\ plugin [args] — uruchom plugin z plugins/
    Plugin      { name: String, args: String, is_super: bool },
    /// -- [static] path — linkuj zewnętrzną bibliotekę
    Extern      { path: String, static_link: bool },

    // ── Import i zależności ───────────────────────────────────

    /// // dep — zależność systemowa (apt/dnf/pacman)
    SysDep(String),   // nie jest w CommandType głównym — obsługiwane przez deps Vec
    /// << "path" [in ns] — import pliku/modułu
    Import      { resource: String, namespace: Option<String> },

    // ── Typy i struktury ──────────────────────────────────────

    /// == Name [V1, V2, V3] — enum
    Enum        { name: String, variants: Vec<String> },
    /// struct Name [field: type, ...] — struktura
    Struct      { name: String, fields: Vec<(String, String)> },
    /// ==type Shape [Circle [r: float], Rect [w: float, h: float]] — ADT
    AdtDef      { name: String, variants: Vec<(String, Vec<(String, String)>)> },
    /// ==interface Name [method1, method2] — protokół/interfejs
    Interface   { name: String, methods: Vec<String> },
    /// ;;Class impl Interface def — deklaracja implementacji
    ImplDef     { class: String, interface: String },

    // ── Funkcje i klasy ───────────────────────────────────────
    // Uwaga: FuncDef/ClassDef nie są węzłami w main_body —
    // są kluczami w HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>
    // (is_arena_fn, type_sig, body)
    //
    // Generics: :serialize [T impl Serializable -> str] def
    FuncDefGeneric { name: String, sig: String },

    // ── Arena allocator — :: name [size] def...done ───────────
    //
    // :: cache [512kb] def
    //   .do_work $data
    // done
    //
    // Semantyka:
    //   - Przy wejściu do bloku: hl_arena_init(&arena, size_bytes)
    //   - Alokacje wewnątrz przez hl_arena_alloc (bump pointer, zero GC)
    //   - Przy wyjściu: hl_arena_free(&arena) — jednorazowe zwolnienie
    //   - NIGDY nie miesza się z GC (gc.c) wewnątrz bloku areny

    /// :: name [size_spec] def...done — funkcja z dedykowaną areną
    /// size_spec: "512b" | "4kb" | "1mb" | "2gb"
    /// body: węzły wewnątrz bloku (kompilowane bez GC)
    ArenaDef    { name: String, size_spec: String, body: Vec<ProgramNode> },
    /// key = arena.name.alloc size_bytes — alokacja z areny (wynik to wskaźnik)
    ArenaAlloc  { key: String, arena_name: String, size: u64 },
    /// arena.name.reset — przywróć wskaźnik do początku (zachowaj pamięć)
    ArenaReset  { arena_name: String },
    /// arena.name.free — zwolnij całą arenę (hl_arena_free)
    ArenaFree   { arena_name: String },

    // ── Kolekcje — natywna mutacja ────────────────────────────

    /// $list.push val / $map.set key val / $list.pop / $map.del key
    CollectionMut { var: String, method: String, args: String },

    // ── Error handling ────────────────────────────────────────

    /// try body catch handler
    Try         { try_cmd: String, catch_cmd: String },
    /// expr ?! "komunikat" — unwrap lub panik (jak Rust ?)
    ResultUnwrap { expr: String, msg: String },

    // ── Async — spawn / await ─────────────────────────────────

    /// spawn rest — uruchom zadanie (fire & forget)
    Spawn(String),
    /// await rest — czekaj (bez przypisania)
    Await(String),
    /// key = spawn rest — uruchom, przypisz handle
    AssignSpawn { key: String, task: String },
    /// key = await rest — czekaj, przypisz wynik
    AssignAwait { key: String, expr: String },

    // ── Pattern matching ──────────────────────────────────────

    /// match cond |> — nagłówek bloku dopasowania
    Match       { cond: String },
    /// val > cmd — ramię match
    MatchArm    { val: String, cmd: String },

    // ── Pipe ──────────────────────────────────────────────────

    /// .a |> .b |> .c — łańcuch wywołań (inline)
    Pipe(Vec<String>),
    /// | .step args — krok wieloliniowego potoku
    PipeLine    { step: String },

    // ── Lambdy / domknięcia ───────────────────────────────────

    /// { $x -> $x * 2 } — lambda jako argument inline
    Lambda      { params: Vec<String>, body: String },
    /// key = { $x -> $x * 2 } — lambda przypisana do zmiennej
    AssignLambda { key: String, params: Vec<String>, body: String, is_raw: bool, is_global: bool },

    // ── Rekurencja ogonowa ────────────────────────────────────

    /// recur args — tail call bieżącej funkcji (stack-safe)
    Recur       { args: String },

    // ── Destrukturyzacja ──────────────────────────────────────

    /// [head | tail] = $lista — destrukturyzacja listy
    DestructList { head: String, tail: String, source: String },
    /// {name, age} = $user — destrukturyzacja mapy/struktury
    DestructMap  { fields: Vec<String>, source: String },

    // ── Zasięg leksykalny ─────────────────────────────────────

    /// ;;scope def...done — anonimowy zakres leksykalny
    ScopeDef,

    // ── Do-notacja ────────────────────────────────────────────

    /// key = do...done — blok sekwencyjny (monadic-style)
    DoBlock     { key: String, body: Vec<ProgramNode> },

    // ── Testy jednostkowe ─────────────────────────────────────

    /// ==test "opis" [ assert ... ] — blok testowy jako pierwsza klasa
    TestBlock   { desc: String, body: Vec<ProgramNode> },

    // ── Walidacja ─────────────────────────────────────────────

    /// assert cond ["komunikat"] — walidacja w miejscu, exit 1 przy błędzie
    Assert      { cond: String, msg: Option<String> },

    // ── Inne ──────────────────────────────────────────────────

    /// log "wiadomość" — log do stderr
    Log(String),
    /// lock $key = val — mutex lock
    Lock        { key: String, val: String },
    /// unlock $key — mutex unlock
    Unlock      { key: String },
    /// & cmd — uruchom w tle
    Background(String),
    /// defer expr — sprzątanie przy wyjściu ze scope (Go-style)
    Defer       { expr: String },
}

// ─────────────────────────────────────────────────────────────
// ProgramNode — pojedynczy węzeł AST
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    /// (byte_offset, byte_len) w pliku źródłowym
    pub span:          (usize, usize),
}

// ─────────────────────────────────────────────────────────────
// AnalysisResult — wynik analizy hl-plsa
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Deserialize)]
pub struct AnalysisResult {
    /// Zależności systemowe (//)
    pub deps:  Vec<String>,
    /// Biblioteki (#<lib_type/name:version>)
    pub libs:  Vec<LibRef>,
    /// Funkcje i metody klas:
    ///   key   = "NazwaKlasy.nazwa_funkcji" lub "nazwa_funkcji"
    ///   value = (is_arena_fn, Option<type_sig>, Vec<ProgramNode>)
    ///
    /// is_arena_fn=true gdy funkcja zdefiniowana przez :: name [size] def
    /// type_sig np. "[int int -> int]" lub "[T impl Serializable -> str]"
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    /// Kod poza funkcjami (globalny main body)
    pub main_body:             Vec<ProgramNode>,
    /// true jeśli jakikolwiek węzeł ma is_sudo=true (^ prefix)
    pub is_potentially_unsafe: bool,
    /// Lista ostrzeżeń sudo, np. "Linia 42: sudo (^)"
    pub safety_warnings:       Vec<String>,
}

// ─────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────
impl AnalysisResult {
    /// Zwraca wszystkie węzły (main_body + wnętrza funkcji) jako flat iterator.
    pub fn all_nodes(&self) -> impl Iterator<Item = &ProgramNode> {
        self.main_body
        .iter()
        .chain(self.functions.values().flat_map(|(_, _, nodes)| nodes.iter()))
    }

    /// Zwraca true jeśli w programie jest jakikolwiek :: blok areny.
    pub fn uses_arena(&self) -> bool {
        self.functions.values().any(|(is_arena, _, _)| *is_arena)
        || self.all_nodes().any(|n| matches!(&n.content, CommandType::ArenaDef { .. }))
    }

    /// Zwraca true jeśli program używa spawn/await (wymaga -lpthread).
    pub fn uses_async(&self) -> bool {
        self.all_nodes().any(|n| matches!(
            &n.content,
            CommandType::Spawn(_)
            | CommandType::Await(_)
            | CommandType::AssignSpawn { .. }
            | CommandType::AssignAwait { .. }
        ))
    }

    /// Zwraca listę extern libs: (path, is_static).
    pub fn extern_libs(&self) -> Vec<(String, bool)> {
        self.all_nodes()
        .filter_map(|n| match &n.content {
            CommandType::Extern { path, static_link } => Some((path.clone(), *static_link)),
                    _ => None,
        })
        .collect()
    }
}
