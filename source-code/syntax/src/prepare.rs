use crate::profiles;
use crate::registry::{
    find_feature, resolve_version, suggest_feature, version_for_legacy_gen,
    DEFAULT_GEN, LATEST_SYNTAX, MAX_GEN, TOOL_VERSION,
};
use crate::version::{ToolVersion, Version};
use std::fmt;

// ── Typy publiczne ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Hint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxDiag {
    pub level: Level,
    /// Numer linii liczony od 1.
    pub line: usize,
    /// Kolumna liczona od 1.
    pub col: usize,
    /// Długość podkreślenia (w znakach).
    pub len: usize,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Co plik zadeklarował w `using`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Declared {
    /// Brak deklaracji — składnia domyślna.
    Default,
    /// Stara forma `using <gen N>`.
    Legacy(u32),
    /// `using <ROLLING>` — zawsze najnowsze funkcje (tryb parsera).
    Rolling,
    /// `using <HackerOS Vx.y>`.
    HackerOs(Version),
    /// `using <not-default/nazwa>`.
    NotDefault { name: String, base: Version },
}

/// Jedno `take <HackerOS Vx.y/funkcja>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Take {
    pub version: Version,
    /// Nazwa kanoniczna elementu (alias z pliku został rozwiązany).
    pub feature: &'static str,
    /// Tak, jak napisał to użytkownik.
    pub requested: String,
    pub line: usize,
}

/// Wynik rozwiązania wszystkich dyrektyw nagłówka.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxInfo {
    pub declared: Declared,
    /// Wersja składni, według reguł której działa plik.
    pub effective: Version,
    /// Gen parsera wynikający z samego `using`.
    pub base_gen: u32,
    /// Gen parsera faktycznie użyty (z uwzględnieniem `take`).
    pub effective_gen: u32,
    /// `gen <wersja>` — wersja narzędzia `hl`, jeśli zadeklarowana.
    pub tool: Option<ToolVersion>,
    pub takes: Vec<Take>,
}

impl Default for SyntaxInfo {
    fn default() -> Self {
        SyntaxInfo {
            declared: Declared::Default,
            effective: LATEST_SYNTAX,
            base_gen: DEFAULT_GEN,
            effective_gen: DEFAULT_GEN,
            tool: None,
            takes: Vec::new(),
        }
    }
}

impl SyntaxInfo {
    /// Czytelny opis składni (do `hl check --meta`, `hl gen-info`).
    pub fn label(&self) -> String {
        match &self.declared {
            Declared::Default => format!("HackerOS {} (domyślna)", self.effective),
            Declared::Legacy(n) => format!("gen {} (≡ HackerOS {})", n, self.effective),
            Declared::Rolling => format!("ROLLING (zawsze najnowsze, HackerOS {})", self.effective),
            Declared::HackerOs(v) if *v == self.effective => format!("HackerOS {}", v),
            Declared::HackerOs(v) => format!("HackerOS {} (działa jak {})", v, self.effective),
            Declared::NotDefault { name, base } => {
                format!("not-default/{} (baza: HackerOS {})", name, base)
            }
        }
    }

    /// Krótka wartość do zmiennej `HL_SYNTAX`.
    pub fn short_label(&self) -> String {
        match &self.declared {
            Declared::NotDefault { name, .. } => format!("not-default/{}", name),
            _ => format!("HackerOS {}", self.effective),
        }
    }

    /// `V4.1/goroutines V5.1/arena` — lista `take` w jednej linii.
    pub fn takes_label(&self) -> String {
        self.takes
            .iter()
            .map(|t| format!("{}/{}", t.version, t.feature))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Zmienne środowiskowe, które interpreter ustawia dla skryptu.
    pub fn env_pairs(&self) -> Vec<(&'static str, String)> {
        let mut v = vec![
            ("HL_SYNTAX", self.short_label()),
            ("HL_SYNTAX_GEN", self.effective_gen.to_string()),
        ];
        if let Some(t) = &self.tool {
            v.push(("HL_TOOL_GEN", t.to_string()));
        }
        if !self.takes.is_empty() {
            v.push(("HL_TAKE", self.takes_label()));
        }
        v
    }
}

impl SyntaxInfo {
    /// Egzekwuje `take`: gdy `take` podniosło parser z gen 1 do gen 2, wolno
    /// użyć tylko tych elementów gen 2, które plik jawnie pożyczył.
    /// `used` — elementy gen 2 znalezione w AST (`hl_parser::gen2_features_used`).
    /// Dla plików, w których gen nie został podniesiony, zawsze `Ok` —
    /// ich sprawdzanie zostaje po staremu przy parserze.
    pub fn check_gen2_usage(&self, used: &[&str]) -> Result<(), String> {
        if self.effective_gen <= self.base_gen {
            return Ok(());
        }
        for feature in used {
            if self.takes.iter().any(|t| t.feature == *feature) {
                continue;
            }
            let (summary, since) = match crate::registry::find_feature(feature) {
                Some(f) => (f.summary, f.since),
                None => ("", LATEST_SYNTAX),
            };
            return Err(format!(
                "`{}` ({}) wymaga gen 2, a plik używa {} z `take` — dodaj `take <HackerOS {}/{}>` albo zmień `using`",
                feature, summary, self.label(), since, feature
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Prepared {
    /// Źródło do oddania parserowi (te same numery linii co oryginał).
    pub source: String,
    pub info: SyntaxInfo,
    pub diags: Vec<SyntaxDiag>,
    /// `true`, gdy w pliku są dyrektywy, których sam parser nie zna
    /// (`gen <..>`, `using <HackerOS ..>`, `using <not-default/..>`, `take`)
    /// albo `take` podniosło gen — czyli oryginał NIE nadaje się do parsera.
    pub rewritten: bool,
}

impl Prepared {
    pub fn has_errors(&self) -> bool {
        self.diags.iter().any(|d| d.level == Level::Error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &SyntaxDiag> {
        self.diags.iter().filter(|d| d.level == Level::Error)
    }

    /// `Err`, jeśli któraś dyrektywa jest błędna; w przeciwnym razie `self`.
    pub fn into_result(self) -> Result<Prepared, SyntaxError> {
        if self.has_errors() {
            let diags: Vec<SyntaxDiag> =
                self.diags.into_iter().filter(|d| d.level == Level::Error).collect();
            Err(SyntaxError { diags })
        } else {
            Ok(self)
        }
    }
}

/// Błędy dyrektyw nagłówka (same poziomy `Error`).
#[derive(Debug, Clone)]
pub struct SyntaxError {
    pub diags: Vec<SyntaxDiag>,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.diags.first() {
            Some(d) => {
                write!(f, "błąd deklaracji składni (linia {}): {}", d.line, d.message)?;
                if let Some(s) = &d.suggestion {
                    write!(f, " — {}", s)?;
                }
                if self.diags.len() > 1 {
                    write!(f, " (+{} więcej)", self.diags.len() - 1)?;
                }
                Ok(())
            }
            None => write!(f, "błąd deklaracji składni"),
        }
    }
}

impl std::error::Error for SyntaxError {}

// ── Parsowanie pojedynczej linii ────────────────────────────────────────────

#[derive(Debug)]
enum Directive {
    Gen(ToolVersion),
    /// `using <gen N>`; `None` = numer nie jest liczbą (zostawiamy parserowi).
    UsingLegacy(Option<u32>),
    /// `using <ROLLING>`.
    UsingRolling,
    UsingHackerOs(Version),
    UsingNotDefault(String),
    Take { version: Version, feature: String },
}

type DirErr = (String, Option<String>);

fn err(msg: impl Into<String>, sug: impl Into<String>) -> DirErr {
    (msg.into(), Some(sug.into()))
}

/// `None` — to nie jest dyrektywa naszej warstwy (zwykła linia).
fn parse_directive(trimmed: &str) -> Option<Result<Directive, DirErr>> {
    let line = match trimmed.find(";;") {
        Some(p) => trimmed[..p].trim_end(),
        None => trimmed,
    };
    let kw_end = line
        .find(|c: char| c.is_whitespace() || c == '<')
        .unwrap_or(line.len());
    let kw = &line[..kw_end];
    if !matches!(kw, "gen" | "using" | "take") {
        return None;
    }
    let rest = line[kw_end..].trim();
    // Stare formy (`using <gen N>`, `using <ROLLING>`) parser czyta tolerancyjnie
    // (nawiasy opcjonalne) — zachowujemy to, żeby żaden dotychczasowy plik
    // nie zaczął nagle być błędny.
    if kw == "using" {
        let lenient = rest.trim_start_matches('<').trim_end_matches('>').trim();
        if lenient.eq_ignore_ascii_case("rolling") {
            return Some(Ok(Directive::UsingRolling));
        }
        if let Some(r) = lenient.strip_prefix("gen") {
            if r.is_empty() || r.starts_with(char::is_whitespace) {
                return Some(Ok(Directive::UsingLegacy(r.trim().parse::<u32>().ok())));
            }
        }
    }
    let inner = match rest.strip_prefix('<').and_then(|r| r.strip_suffix('>')) {
        Some(i) => i.trim(),
        None => {
            return Some(Err(err(
                format!("`{}` wymaga nawiasów kątowych", kw),
                match kw {
                    "gen" => "poprawna składnia: `gen <1>` albo `gen <1.1>`",
                    "using" => "poprawna składnia: `using <HackerOS V5.1>`",
                    _ => "poprawna składnia: `take <HackerOS V4.1/goroutines>`",
                },
            )))
        }
    };
    Some(match kw {
        "gen" => parse_gen(inner),
        "using" => parse_using(inner),
        _ => parse_take(inner),
    })
}

fn parse_gen(inner: &str) -> Result<Directive, DirErr> {
    ToolVersion::parse(inner).map(Directive::Gen).map_err(|e| {
        err(format!("`gen <{}>`: {}", inner, e), "poprawna składnia: `gen <1>` albo `gen <1.1>`")
    })
}

/// Zdejmij słowo `HackerOS` (bez rozróżniania wielkości liter) + białe znaki.
fn strip_hackeros(inner: &str) -> Option<&str> {
    const KW: &str = "hackeros";
    if inner.len() > KW.len()
        && inner.is_char_boundary(KW.len())
        && inner[..KW.len()].eq_ignore_ascii_case(KW)
    {
        let rest = &inner[KW.len()..];
        if rest.starts_with(char::is_whitespace) {
            return Some(rest.trim());
        }
    }
    None
}

fn parse_using(inner: &str) -> Result<Directive, DirErr> {
    if let Some(ver) = strip_hackeros(inner) {
        return Version::parse_hackeros(ver).map(Directive::UsingHackerOs).map_err(|e| {
            err(format!("`using <{}>`: {}", inner, e), "poprawna składnia: `using <HackerOS V5.1>`")
        });
    }
    if let Some(name) = inner.strip_prefix("not-default/") {
        let name = name.trim();
        if profiles::is_valid_profile_name(name) {
            return Ok(Directive::UsingNotDefault(name.to_string()));
        }
        return Err(err(
            format!("niepoprawna nazwa profilu `{}`", name),
            "poprawna składnia: `using <not-default/nazwa>` (litery, cyfry, `-`, `_`, `.`)",
        ));
    }
    Err(err(
        format!("nieznana forma deklaracji `using <{}>`", inner),
        "użyj `using <HackerOS V5.1>`, `using <not-default/nazwa>` albo starego `using <gen 2>`",
    ))
}

fn parse_take(inner: &str) -> Result<Directive, DirErr> {
    const SUG: &str = "poprawna składnia: `take <HackerOS V4.1/goroutines>`";
    if inner.starts_with("not-default/") {
        return Err(err("`take` obsługuje tylko wersje HackerOS, nie profile not-default", SUG));
    }
    let spec = strip_hackeros(inner)
        .ok_or_else(|| err(format!("`take <{}>`: oczekiwano `HackerOS Vx.y/funkcja`", inner), SUG))?;
    let (ver, feat) = spec
        .split_once('/')
        .ok_or_else(|| err(format!("`take <{}>`: brak `/funkcja` po wersji", inner), SUG))?;
    let feat = feat.trim();
    if feat.is_empty() || feat.contains('/') {
        return Err(err(format!("`take <{}>`: niepoprawna nazwa funkcji", inner), SUG));
    }
    let version = Version::parse_hackeros(ver)
        .map_err(|e| err(format!("`take <{}>`: {}", inner, e), SUG))?;
    Ok(Directive::Take { version, feature: feat.to_string() })
}

// ── Walidacja `take` ────────────────────────────────────────────────────────

fn resolve_take(version: Version, feature: &str) -> Result<&'static crate::registry::Feature, DirErr> {
    let resolved = resolve_version(version).map_err(|e| {
        err(e, "najstarsza obsługiwana składnia to V4.0")
    })?;
    if resolved.clamped {
        return Err(err(
            format!(
                "`take`: składnia {} nie istnieje (najnowsza znana: {})",
                version, LATEST_SYNTAX
            ),
            "nie można pożyczać funkcji z wersji, która jeszcze nie wyszła",
        ));
    }
    let f = find_feature(feature).ok_or_else(|| {
        let hint = match suggest_feature(feature) {
            Some(s) => format!("czy chodziło o `{}`? (lista: `hl version`)", s),
            None => "lista dostępnych funkcji: `hl version`".to_string(),
        };
        err(format!("nieznana funkcja składni `{}`", feature), hint)
    })?;
    if f.since > version {
        return Err(err(
            format!("funkcja `{}` nie istnieje w {} (jest od {})", f.name, version, f.since),
            format!("użyj `take <HackerOS {}/{}>`", f.since, f.name),
        ));
    }
    Ok(f)
}

/// `hl_parser::extract_gen` szuka deklaracji tylko w tylu pierwszych liniach.
const PARSER_HEADER_LINES: usize = 10;

// ── Główna funkcja ──────────────────────────────────────────────────────────

struct UsingRec {
    out_index: usize,
    eol: String,
    /// Czy linia była już oryginalnie `using <gen N>` (wtedy zostaje, jeśli nic jej nie podnosi).
    legacy: bool,
}

struct TakeRec {
    take: Take,
    gen: u32,
}

fn split_eol(seg: &str) -> (&str, &str) {
    let body = seg.strip_suffix('\n').unwrap_or(seg);
    let body = body.strip_suffix('\r').unwrap_or(body);
    (body, &seg[body.len()..])
}

fn is_non_code(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with("#!")
        || trimmed.starts_with(";;")
        || trimmed.starts_with("//")
}

pub fn prepare(source: &str) -> Prepared {
    let mut diags: Vec<SyntaxDiag> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let mut seen_code = false;
    let mut rewritten = false;

    let mut tool: Option<(ToolVersion, usize)> = None;
    let mut using: Option<UsingRec> = None;
    let mut declared = Declared::Default;
    let mut base_gen = DEFAULT_GEN;
    let mut effective = LATEST_SYNTAX;
    let mut takes: Vec<TakeRec> = Vec::new();

    for (idx, seg) in source.split_inclusive('\n').enumerate() {
        let (body, eol) = split_eol(seg);
        let trimmed = body.trim();
        let line = idx + 1;
        let col = body.len() - body.trim_start().len() + 1;
        let len = trimmed.chars().count().max(1);
        let mut diag = |level: Level, message: String, suggestion: Option<String>| {
            diags.push(SyntaxDiag { level, line, col, len, message, suggestion });
        };

        let parsed = match parse_directive(trimmed) {
            None => {
                if !is_non_code(trimmed) {
                    seen_code = true;
                }
                out.push(seg.to_string());
                continue;
            }
            Some(Err((msg, sug))) => {
                diag(Level::Error, msg, sug);
                out.push(seg.to_string()); // błędna dyrektywa → parser ją odrzuci
                continue;
            }
            Some(Ok(d)) => d,
        };

        // Stare formy (`using <gen N>`, `using <ROLLING>`) zostają po staremu —
        // ostrzeżenie o pozycji wystawia lint_gen w hl-core, a ich kolejne
        // wystąpienia (np. w heredoc generującym inny skrypt) nie są błędem.
        if matches!(parsed, Directive::UsingLegacy(_) | Directive::UsingRolling) {
            if using.is_none() && !seen_code {
                match &parsed {
                    Directive::UsingLegacy(Some(n)) if *n >= 1 && *n <= MAX_GEN => {
                        declared = Declared::Legacy(*n);
                        base_gen = *n;
                        effective = version_for_legacy_gen(*n);
                        using = Some(UsingRec { out_index: out.len(), eol: eol.to_string(), legacy: true });
                    }
                    Directive::UsingRolling => {
                        declared = Declared::Rolling;
                        base_gen = MAX_GEN;
                        effective = LATEST_SYNTAX;
                        using = Some(UsingRec { out_index: out.len(), eol: eol.to_string(), legacy: true });
                    }
                    _ => {}
                }
            }
            out.push(seg.to_string());
            continue;
        }

        // Dyrektywy nowej składni działają tylko w nagłówku.
        if seen_code {
            diag(
                Level::Warning,
                "dyrektywa po kodzie — zignorowana".into(),
                Some("dyrektywy `gen`, `using`, `take` umieść na samym początku pliku".into()),
            );
            out.push(eol.to_string());
            continue;
        }

        match parsed {
            Directive::UsingLegacy(_) | Directive::UsingRolling => unreachable!("obsłużone wyżej"),

            Directive::Gen(tv) => {
                if tool.is_some() {
                    diag(Level::Error, "druga deklaracja `gen <wersja>` w pliku".into(), None);
                    out.push(seg.to_string());
                } else {
                    tool = Some((tv, line));
                    rewritten = true;
                    out.push(eol.to_string());
                }
            }

            Directive::UsingHackerOs(_) | Directive::UsingNotDefault(_) => {
                if using.is_some() {
                    diag(
                        Level::Error,
                        "druga deklaracja `using` w pliku".into(),
                        Some("zostaw jedną deklarację `using` (dodatkowe elementy dołącz przez `take`)".into()),
                    );
                    out.push(seg.to_string());
                    continue;
                }
                // Rozwiąż wersję bazową (dla not-default — przez profil).
                let (decl, ver): (Declared, Version) = match parsed {
                    Directive::UsingHackerOs(v) => (Declared::HackerOs(v), v),
                    Directive::UsingNotDefault(name) => match profiles::find_profile(&name) {
                        Ok(Some(p)) => (Declared::NotDefault { name, base: p.base }, p.base),
                        Ok(None) => {
                            let dirs = profiles::profile_dirs()
                                .iter()
                                .map(|d| d.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            diag(
                                Level::Error,
                                format!("nieznany profil składni `not-default/{}`", name),
                                Some(format!("zainstaluj `{}.syntax` w jednym z: {}", name, dirs)),
                            );
                            out.push(seg.to_string());
                            continue;
                        }
                        Err(e) => {
                            diag(Level::Error, e, None);
                            out.push(seg.to_string());
                            continue;
                        }
                    },
                    _ => unreachable!(),
                };
                match resolve_version(ver) {
                    Ok(r) => {
                        if r.clamped {
                            diag(
                                Level::Warning,
                                format!(
                                    "składnia {} jest nowsza niż najnowsza znana ({}) — używam reguł {}",
                                    ver, LATEST_SYNTAX, LATEST_SYNTAX
                                ),
                                Some("zaktualizuj `hl`, aby użyć nowszej składni".into()),
                            );
                        }
                        if line > PARSER_HEADER_LINES {
                            diag(
                                Level::Warning,
                                format!("deklaracja `using` jest poza pierwszymi {} liniami pliku — parser jej nie zauważy", PARSER_HEADER_LINES),
                                Some("przesuń `using` bliżej początku pliku".into()),
                            );
                        }
                        declared = decl;
                        base_gen = r.gen;
                        effective = r.effective;
                        using = Some(UsingRec { out_index: out.len(), eol: eol.to_string(), legacy: false });
                        rewritten = true;
                        out.push(String::new()); // placeholder, uzupełniany po zebraniu `take`
                    }
                    Err(e) => {
                        diag(Level::Error, e, Some("najstarsza obsługiwana składnia to V4.0".into()));
                        out.push(seg.to_string());
                    }
                }
            }

            Directive::Take { version, feature } => match resolve_take(version, &feature) {
                Ok(f) => {
                    let dup = takes.iter().any(|t| t.take.feature == f.name && t.take.version == version);
                    if dup {
                        diag(Level::Warning, format!("powtórzone `take` funkcji `{}` — pomijam", f.name), None);
                    } else {
                        takes.push(TakeRec {
                            take: Take { version, feature: f.name, requested: feature, line },
                            gen: f.gen,
                        });
                    }
                    rewritten = true;
                    out.push(eol.to_string());
                }
                Err((msg, sug)) => {
                    diag(Level::Error, msg, sug);
                    out.push(seg.to_string());
                }
            },
        }
    }

    // ── Gen parsera = max(gen z `using`, gen każdej pożyczonej funkcji) ─────
    let effective_gen = takes.iter().map(|t| t.gen).fold(base_gen, u32::max);
    if effective_gen > base_gen {
        if let Some(t) = takes.iter().find(|t| t.gen > base_gen) {
            diags.push(SyntaxDiag {
                level: Level::Hint,
                line: t.take.line,
                col: 1,
                len: 1,
                message: format!(
                    "`take <HackerOS {}/{}>` podnosi parser do gen {} — cały plik jest parsowany jako gen {}",
                    t.take.version, t.take.feature, effective_gen, effective_gen
                ),
                suggestion: None,
            });
        }
    }

    // Uzupełnij linię `using` (nowa forma zawsze; stara tylko, gdy została podniesiona).
    if let Some(u) = &using {
        if !u.legacy || effective_gen != base_gen {
            out[u.out_index] = format!("using <gen {}>{}", effective_gen, u.eol);
            rewritten = true;
        }
    }
    debug_assert!(
        using.is_some() || effective_gen == DEFAULT_GEN,
        "podniesienie genu bez deklaracji `using` wymagałoby wstawienia linii"
    );

    // `gen <wersja>` nowsze niż uruchomiony `hl` — tylko ostrzeżenie.
    if let Some((tv, line)) = tool {
        if tv > TOOL_VERSION {
            diags.push(SyntaxDiag {
                level: Level::Warning,
                line,
                col: 1,
                len: 1,
                message: format!(
                    "skrypt deklaruje `gen <{}>`, a uruchomiony `hl` ma gen {}",
                    tv, TOOL_VERSION
                ),
                suggestion: Some("zaktualizuj `hl`; skrypt może używać nieobsługiwanych elementów".into()),
            });
        }
    }
    diags.sort_by_key(|d| d.line);

    Prepared {
        source: out.concat(),
        info: SyntaxInfo {
            declared,
            effective,
            base_gen,
            effective_gen,
            tool: tool.map(|(t, _)| t),
            takes: takes.into_iter().map(|t| t.take).collect(),
        },
        diags,
        rewritten,
    }
}
