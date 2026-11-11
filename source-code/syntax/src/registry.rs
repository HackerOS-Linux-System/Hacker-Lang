use crate::version::{ToolVersion, Version};

/// Najwyższy `gen`, który rozumie parser (`hl_parser::HL_MAX_GEN`).
/// Pilnuje tego test w hl-core (`syntax_gen_matches_parser`).
pub const MAX_GEN: u32 = 2;
/// Gen domyślny, gdy plik nie ma deklaracji (`hl_parser::HL_DEFAULT_GEN`).
pub const DEFAULT_GEN: u32 = 2;

/// Wersja narzędzia `hl` (to, z czym porównywane jest `gen <wersja>`).
pub const TOOL_VERSION: ToolVersion = ToolVersion(Version::new(1, 1));

/// Najstarsza składnia, którą da się zadeklarować (`using` / `take`).
pub const OLDEST_SYNTAX: Version = Version::new(4, 0);
/// Ostatnia wersja składni z rodziny „gen 1" (`using <HackerOS V4.7>` ≡ gen 1).
pub const GEN1_FINAL: Version = Version::new(4, 7);
/// Pierwsza wersja składni z rodziny „gen 2".
pub const GEN2_START: Version = Version::new(5, 0);
/// Najnowsza znana składnia (`using <HackerOS V5.1>` ≡ gen 2).
pub const LATEST_SYNTAX: Version = Version::new(5, 1);

/// Który `gen` parsera obsługuje daną wersję składni.
pub fn gen_for_version(v: Version) -> u32 {
    if v < GEN2_START { 1 } else { 2 }
}

/// Odpowiednik starej deklaracji `using <gen N>` w nowych wersjach.
pub fn version_for_legacy_gen(gen: u32) -> Version {
    if gen <= 1 { GEN1_FINAL } else { LATEST_SYNTAX }
}

/// Wynik dopasowania wersji składni do zakresu znanego `hl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedVersion {
    /// Wersja, według reguł której faktycznie działamy.
    pub effective: Version,
    /// Gen parsera dla tej wersji.
    pub gen: u32,
    /// `true`, gdy zadeklarowana wersja jest nowsza niż `LATEST_SYNTAX`
    /// i została zredukowana do `LATEST_SYNTAX`.
    pub clamped: bool,
}

/// Dopasuj zadeklarowaną wersję składni. Błąd tylko dla wersji starszych niż
/// `OLDEST_SYNTAX`; nowsze niż `LATEST_SYNTAX` są akceptowane i redukowane
/// (wywołujący sam decyduje, czy to ostrzeżenie, czy błąd — patrz `take`).
pub fn resolve_version(v: Version) -> Result<ResolvedVersion, String> {
    if v < OLDEST_SYNTAX {
        return Err(format!(
            "składnia {} jest starsza niż najstarsza obsługiwana ({})",
            v, OLDEST_SYNTAX
        ));
    }
    if v > LATEST_SYNTAX {
        return Ok(ResolvedVersion {
            effective: LATEST_SYNTAX,
            gen: gen_for_version(LATEST_SYNTAX),
            clamped: true,
        });
    }
    Ok(ResolvedVersion { effective: v, gen: gen_for_version(v), clamped: false })
}

/// Jeden element składni, który można „pożyczyć" z danej wersji przez `take`.
#[derive(Debug, Clone, Copy)]
pub struct Feature {
    /// Nazwa kanoniczna (małe litery, `-` zamiast spacji).
    pub name: &'static str,
    /// Alternatywne zapisy (w tym historyczne literówki, np. `corountimes`).
    pub aliases: &'static [&'static str],
    /// Minimalny gen parsera potrzebny, by ten element w ogóle się sparsował.
    pub gen: u32,
    /// Od której wersji składni element istnieje.
    pub since: Version,
    /// Operator/składnia + jedno zdanie opisu (do `hl version` i komunikatów).
    pub summary: &'static str,
}

const V4_0: Version = Version::new(4, 0);
const V5_0: Version = Version::new(5, 0);

/// Wszystkie znane elementy składni. Kolejność = kolejność w wypisie.
pub const FEATURES: &[Feature] = &[
    // ── rodzina gen 1 (V4.x) ─────────────────────────────────────────────
    Feature { name: "goroutines", aliases: &["coroutines", "corountimes", "async"],
        gen: 1, since: V4_0, summary: "`:*` — anonimowa goroutine (współbieżność)" },
    Feature { name: "channels", aliases: &["channel"],
        gen: 1, since: V4_0, summary: "`:** kanal`, `*-- kanal` — kanały między goroutines" },
    Feature { name: "background", aliases: &["bg"],
        gen: 1, since: V4_0, summary: "`&` — komenda w tle, PID w @_bg_pid" },
    Feature { name: "hsh", aliases: &["shell-run"],
        gen: 1, since: V4_0, summary: "`*>` — komenda przez hsh -c" },
    Feature { name: "repeat", aliases: &["times"],
        gen: 1, since: V4_0, summary: "`_N` — powtórz N razy" },
    Feature { name: "import", aliases: &["include"],
        gen: 1, since: V4_0, summary: "`<<` — import pliku .hl" },
    // ── rodzina gen 2 (V5.x) ─────────────────────────────────────────────
    Feature { name: "typed-vars", aliases: &["types", "typed"],
        gen: 2, since: V5_0, summary: "`% x: int = 1` — zmienne z typem" },
    Feature { name: "arith", aliases: &["arithmetic", "math"],
        gen: 2, since: V5_0, summary: "`$( a + b ) -> @x` — arytmetyka natywna" },
    Feature { name: "pipe-var", aliases: &["pipe"],
        gen: 2, since: V5_0, summary: "`> cmd |> @x` — wynik komendy do zmiennej" },
    Feature { name: "for-in", aliases: &["foreach"],
        gen: 2, since: V5_0, summary: "`@ x in ...` — pętla for-in" },
    Feature { name: "while", aliases: &["while-loop"],
        gen: 2, since: V5_0, summary: "`?~ warunek` — pętla while" },
    Feature { name: "switch", aliases: &["match"],
        gen: 2, since: V5_0, summary: "`? switch @x` — switch/case" },
    Feature { name: "hackeros-api", aliases: &["api"],
        gen: 2, since: V5_0, summary: "`|| narzędzie` — wywołania narzędzi HackerOS" },
    Feature { name: "named-goroutines", aliases: &["named-coroutines"],
        gen: 2, since: V5_0, summary: "`:* nazwa def` — goroutine z nazwą" },
    Feature { name: "arena", aliases: &["arena-functions"],
        gen: 2, since: V5_0, summary: "`:: nazwa <rozmiar> def` — funkcje areny" },
    Feature { name: "coreutils", aliases: &["builtin-coreutils"],
        gen: 2, since: V5_0, summary: "`/> narzędzie` — wbudowane coreutils" },
    Feature { name: "multiline-string", aliases: &["multiline"],
        gen: 2, since: V5_0, summary: "napisy wieloliniowe" },
    Feature { name: "extern", aliases: &["extern-blocks"],
        gen: 2, since: V5_0, summary: "bloki extern (shell, python, java, elf, so)" },
    Feature { name: "import-dir", aliases: &["include-dir"],
        gen: 2, since: V5_0, summary: "`<* katalog` — import katalogu" },
];

/// Znajdź element po nazwie lub aliasie (bez rozróżniania wielkości liter).
pub fn find_feature(name: &str) -> Option<&'static Feature> {
    let n = name.trim().to_ascii_lowercase();
    FEATURES
        .iter()
        .find(|f| f.name == n || f.aliases.iter().any(|a| *a == n))
}

/// Najbliższa nazwa do `name` (do podpowiedzi „czy chodziło o…").
pub fn suggest_feature(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    if n.is_empty() {
        return None;
    }
    FEATURES
        .iter()
        .filter(|f| f.name.contains(&n) || n.contains(f.name) || common_prefix(f.name, &n) >= 3)
        .map(|f| f.name)
        .next()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}
