use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────
// Typy bibliotek — synchronizacja z hl-plsa LibType
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LibType {
    /// vira → ~/.hackeros/hacker-lang/libs/.virus/ (repo git)
    Vira,
    /// virus → ~/.hackeros/hacker-lang/libs/.virus/ (alias vira)
    Virus,
    /// bytes → ~/.hackeros/hacker-lang/libs/bytes/ (pliki .so)
    Bytes,
    /// core → ~/.hackeros/hacker-lang/libs/core/ (pliki .hl)
    Core,
}

impl LibType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LibType::Vira  => "vira",
            LibType::Virus => "virus",
            LibType::Bytes => "bytes",
            LibType::Core  => "core",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibRef {
    pub lib_type: LibType,
    pub name:     String,
    pub version:  Option<String>,
}

// ─────────────────────────────────────────────────────────────
// Węzły komend — pełna składnia hacker-lang
// Synchronizacja z hl-plsa/main.rs CommandType
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum CommandType {

    // ── ISTNIEJĄCE — BEZ ZMIAN ────────────────────────────────

    /// >> cmd — surowa komenda bez podstawienia zmiennych
    RawNoSub(String),
    /// > cmd — surowa komenda z podstawieniem zmiennych
    RawSub(String),
    /// >>> cmd — komenda izolowana w subshell'u
    Isolated(String),
    /// @KEY = val — przypisanie zmiennej środowiskowej
    AssignEnv    { key: String, val: String },
    /// key = val — przypisanie zmiennej lokalnej
    AssignLocal  { key: String, val: String, is_raw: bool },
    /// = N > cmd — pętla N razy
    Loop         { count: u64, cmd: String },
    /// ? cond > cmd — warunek if
    If           { cond: String, cmd: String },
    /// ?? cond > cmd — warunek elif
    Elif         { cond: String, cmd: String },
    /// ?: > cmd — gałąź else
    Else         { cmd: String },
    /// while cond |> cmd — pętla while
    While        { cond: String, cmd: String },
    /// for var in expr |> cmd — pętla for
    For          { var: String, in_: String, cmd: String },
    /// & cmd — tło (background)
    Background(String),
    /// .func_name [args] — wywołanie funkcji HL
    Call         { path: String, args: String },
    /// \\ plugin [args] — uruchamia ~/.hackeros/hacker-lang/plugins/<n>
    Plugin       { name: String, args: String, is_super: bool },
    /// log "msg" — wypisz wiadomość
    Log(String),
    /// lock $key = val — alokuj blok pamięci na stercie GC
    Lock         { key: String, val: String },
    /// unlock $key — zwolnij blok pamięci
    Unlock       { key: String },
    /// -- [static] path — dołącz zewnętrzną bibliotekę .so/.a
    Extern       { path: String, static_link: bool },
    /// << "path" [in ns] — import zasobu
    Import       { resource: String, namespace: Option<String> },
    /// == Name [A, B, C] — deklaracja enum
    Enum         { name: String, variants: Vec<String> },
    /// struct Name [field: type, ...] — deklaracja struct
    Struct       { name: String, fields: Vec<(String, String)> },
    /// try cmd catch cmd — obsługa błędów
    Try          { try_cmd: String, catch_cmd: String },
    /// end [N] — zakończ program z kodem wyjścia
    End          { code: i32 },
    /// out val — zwróć wartość z funkcji
    Out(String),

    // ── NOWE ─────────────────────────────────────────────────

    /// % KEY = val — stała (niezmienne przez konwencję)
    Const        { key: String, val: String },

    /// spawn rest — uruchom zadanie asynchronicznie, zwróć handle
    Spawn(String),

    /// await rest — czekaj na wynik (bez przypisania)
    Await(String),

    /// key = spawn rest — uruchom i przypisz handle do zmiennej
    AssignSpawn  { key: String, task: String },

    /// key = await rest — czekaj i przypisz wynik do zmiennej
    AssignAwait  { key: String, expr: String },

    /// assert cond [msg] — walidacja w miejscu, exit 1 przy błędzie
    Assert       { cond: String, msg: Option<String> },

    /// match cond |> — nagłówek bloku dopasowania wzorców
    Match        { cond: String },

    /// val > cmd — pojedyncze ramię match
    MatchArm     { val: String, cmd: String },

    /// .a |> .b |> .c — łańcuch wywołań
    Pipe(Vec<String>),

    // ── NOWE: system typów i wyrażenia ───────────────────────

    /// key = expr — przypisanie z wyrażeniem arytmetycznym/logicznym
    /// np. x = 2 + 3 * 4  |  y = $x > 10  |  z = [$a, $b]  |  m = {k: "v"}
    /// Obsługuje też interpolację wyrażeń: "Wynik: $(2 + 3)"
    AssignExpr   { key: String, expr: String, is_raw: bool, is_global: bool },

    /// $list.push 42  /  $map.set "key" "val"  — mutacja kolekcji
    CollectionMut { var: String, method: String, args: String },

    // ── NOWE: interfejsy / protokoły ─────────────────────────

    /// ==interface Serializable [to_json, from_json]
    Interface    { name: String, methods: Vec<String> },

    /// ;;Config impl Serializable def
    ImplDef      { class: String, interface: String },

    // ── NOWE: arena allocator ─────────────────────────────────
    //
    // :: name [size] def...done — funkcja z dedykowaną areną
    //
    // Cały runtime nadal używa GC (gc.c) dla zwykłych alokacji.
    // Arena (aa.c, tryb HL_ARENA_MODE_JIT) jest używana WYŁĄCZNIE
    // wewnątrz :: bloków. Po wyjściu z bloku (done) arena jest
    // zwalniana jednym hl_arena_free() zamiast przez GC.
    //
    // Przykład:
    //   :: cache [512kb] def
    //     x = .compute $data
    //   done
    ArenaDef     { name: String, size: String },

    // ── NOWE: error handling jako wartość ─────────────────────

    /// expr ?! "komunikat błędu" — unwrap lub panik z komunikatem (jak Rust ?)
    ResultUnwrap { expr: String, msg: String },

    /// wywołanie metody modułu: http.get "url"
    ModuleCall   { path: String, args: String },

    // ── NOWE: domknięcia / lambdy ─────────────────────────────

    /// { $x -> $x * 2 } — domknięcie standalone (np. jako argument inline)
    Lambda       { params: Vec<String>, body: String },

    /// callback = { $x -> $x * 2 } — przypisanie lambdy do zmiennej
    AssignLambda { key: String, params: Vec<String>, body: String, is_raw: bool, is_global: bool },

    // ── NOWE: rekurencja ogonowa ──────────────────────────────

    /// recur ($1 - 1) — wywołanie ogonowe bieżącej funkcji
    Recur        { args: String },

    // ── NOWE: destrukturyzacja ────────────────────────────────

    /// [head | tail] = $lista — destrukturyzacja listy
    DestructList { head: String, tail: String, source: String },

    /// {name, age} = $user — destrukturyzacja mapy/struktury
    DestructMap  { fields: Vec<String>, source: String },

    // ── NOWE: zasięg leksykalny ──────────────────────────────

    /// ;;scope def — anonimowy zakres leksykalny (traktowany jak klasa)
    ScopeDef,

    // ── NOWE: typy algebraiczne (ADT) ────────────────────────

    /// ==type Shape [ Circle [radius: float], Rect [w: float, h: float], Point ]
    AdtDef       { name: String, variants: Vec<(String, Vec<(String, String)>)> },

    // ── NOWE: do-notacja ─────────────────────────────────────

    /// result = do ... done — blok sekwencyjny (jak do-notacja Haskell)
    DoBlock      { key: String, body: Vec<ProgramNode> },

    /// | .step args — krok wieloliniowego potoku
    PipeLine     { step: String },

    // ── NOWE: testy jednostkowe ──────────────────────────────

    /// ==test "opis" [ assert ... ] — blok testowy jako pierwsza klasa
    TestBlock    { desc: String, body: Vec<ProgramNode> },

    // ── NOWE: defer ──────────────────────────────────────────

    /// defer .file.close $f — sprzątanie zasobów przy wyjściu ze scope
    Defer        { expr: String },

    // ── NOWE: generics z constraints ─────────────────────────

    /// :serialize [T impl Serializable -> str] def — funkcja z ograniczeniem generycznym
    FuncDefGeneric { name: String, sig: String },
}

// ─────────────────────────────────────────────────────────────
// Węzeł programu
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramNode {
    pub line_num:      usize,
    pub is_sudo:       bool,
    pub content:       CommandType,
    pub original_text: String,
    pub span:          (usize, usize),
}

// ─────────────────────────────────────────────────────────────
// Wynik analizy — identyczny z hl-plsa AnalysisResult
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnalysisResult {
    pub deps:                  Vec<String>,
    pub libs:                  Vec<LibRef>,
    /// Trójka: (is_arena_fn, Option<type_sig>, Vec<ProgramNode>)
    /// is_arena_fn = true gdy funkcja zdefiniowana przez :: name [size] def
    pub functions:             HashMap<String, (bool, Option<String>, Vec<ProgramNode>)>,
    pub main_body:             Vec<ProgramNode>,
    pub is_potentially_unsafe: bool,
    pub safety_warnings:       Vec<String>,
}
