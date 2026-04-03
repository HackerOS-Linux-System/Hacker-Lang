//! Hacker Lang — system diagnostyczny
//!
//! Obsługuje błędy, ostrzeżenia i podpowiedzi z dokładnym wskazaniem
//! miejsca w kodzie źródłowym oraz kolorowymi komunikatami.

use colored::Colorize;
use std::fmt;

// ─── Poziomy diagnostyki ─────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum DiagLevel {
    Error,
    Warning,
    Hint,
    Note,
}

impl DiagLevel {
    fn label(&self) -> colored::ColoredString {
        match self {
            DiagLevel::Error   => "error".red().bold(),
            DiagLevel::Warning => "warning".yellow().bold(),
            DiagLevel::Hint    => "hint".cyan().bold(),
            DiagLevel::Note    => "note".bright_black().bold(),
        }
    }

    fn gutter_color(&self) -> &'static str {
        match self {
            DiagLevel::Error   => "\x1b[31m", // red
            DiagLevel::Warning => "\x1b[33m", // yellow
            DiagLevel::Hint    => "\x1b[36m", // cyan
            DiagLevel::Note    => "\x1b[90m", // dark grey
        }
    }

    fn marker(&self) -> &'static str {
        match self {
            DiagLevel::Error   => "^",
            DiagLevel::Warning => "~",
            DiagLevel::Hint    => "-",
            DiagLevel::Note    => "·",
        }
    }
}

// ─── Span — pozycja w źródle ─────────────────────────────────────────────────

/// Pozycja fragmentu kodu (linia 1-based, kolumna 1-based, długość)
#[derive(Debug, Clone)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

impl Span {
    pub fn new(line: usize, col: usize, len: usize) -> Self {
        Self { line, col, len }
    }

    /// Span wskazujący na całą linię
    pub fn line_only(line: usize) -> Self {
        Self { line, col: 1, len: 0 }
    }
}

// ─── Pojedynczy komunikat diagnostyczny ─────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Diag {
    pub level: DiagLevel,
    pub message: String,
    pub span: Option<Span>,
    /// Sugestia zamiany (wyświetlana po głównym komunikacie)
    pub suggestion: Option<String>,
    /// Dodatkowe uwagi (np. "see also")
    pub notes: Vec<String>,
}

impl Diag {
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Error,
            message: msg.into(),
            span: None,
            suggestion: None,
            notes: vec![],
        }
    }

    pub fn warning(msg: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Warning,
            message: msg.into(),
            span: None,
            suggestion: None,
            notes: vec![],
        }
    }

    pub fn hint(msg: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Hint,
            message: msg.into(),
            span: None,
            suggestion: None,
            notes: vec![],
        }
    }

    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_suggestion(mut self, s: impl Into<String>) -> Self {
        self.suggestion = Some(s.into());
        self
    }

    pub fn with_note(mut self, n: impl Into<String>) -> Self {
        self.notes.push(n.into());
        self
    }
}

// ─── Renderer ────────────────────────────────────────────────────────────────

/// Renderuje diagnostykę do stderr z kolorami i fragmentem kodu
pub struct DiagRenderer<'a> {
    /// Nazwa pliku / źródła (np. "update.hl" lub "<repl>")
    pub filename: &'a str,
    /// Pełna zawartość pliku źródłowego (linie)
    pub lines: Vec<&'a str>,
}

impl<'a> DiagRenderer<'a> {
    pub fn new(filename: &'a str, source: &'a str) -> Self {
        Self {
            filename,
            lines: source.lines().collect(),
        }
    }

    /// Wypisz jeden komunikat diagnostyczny na stderr
    pub fn emit(&self, diag: &Diag) {
        let gc = diag.level.gutter_color();
        let reset = "\x1b[0m";

        // ── Nagłówek: error[E]: wiadomość ──
        eprintln!(
            "{}: {}",
            diag.level.label(),
                  diag.message.white().bold()
        );

        // ── Lokalizacja pliku ──
        if let Some(ref span) = diag.span {
            eprintln!(
                "  {} {}:{}:{}",
                "-->".bright_black(),
                      self.filename.bright_white(),
                      span.line,
                      span.col
            );

            // ── Kontekst kodu ──
            let line_idx = span.line.saturating_sub(1);
            let line_num_w = format!("{}", span.line).len().max(2);

            // Linia poprzednia (kontekst)
            if line_idx > 0 {
                if let Some(prev) = self.lines.get(line_idx - 1) {
                    eprintln!(
                        "{}{:>w$} │{} {}",
                        gc,
                        span.line - 1,
                        reset,
                        prev.bright_black(),
                              w = line_num_w
                    );
                }
            }

            // Linia błędu
            if let Some(src_line) = self.lines.get(line_idx) {
                eprintln!(
                    "{}{:>w$} │{} {}",
                    gc,
                    span.line,
                    reset,
                    src_line,
                    w = line_num_w
                );

                // Marker podkreślenia
                let col0 = span.col.saturating_sub(1);
                let marker_len = if span.len == 0 {
                    src_line.trim_start().len().max(1)
                } else {
                    span.len
                };
                let spaces = " ".repeat(line_num_w + 3 + col0);
                let markers = diag.level.marker().repeat(marker_len);
                eprintln!(
                    "{}{}{}{}",
                    spaces,
                    gc,
                    markers,
                    reset
                );
            }

            // Linia następna (kontekst)
            if let Some(next) = self.lines.get(line_idx + 1) {
                let next_num = span.line + 1;
                eprintln!(
                    "{}{:>w$} │{} {}",
                    gc,
                    next_num,
                    reset,
                    next.bright_black(),
                          w = line_num_w
                );
            }

            eprintln!("{}{:>w$} │{}", gc, "", reset, w = line_num_w);
        } else {
            eprintln!("  {} {}", "-->".bright_black(), self.filename.bright_white());
        }

        // ── Sugestia ──
        if let Some(ref sug) = diag.suggestion {
            eprintln!(
                "  {} {}",
                "help:".bright_cyan().bold(),
                      sug.bright_white()
            );
        }

        // ── Uwagi dodatkowe ──
        for note in &diag.notes {
            eprintln!(
                "  {} {}",
                "note:".bright_black().bold(),
                      note.bright_black()
            );
        }

        eprintln!(); // pusta linia dla czytelności
    }

    /// Wypisz wiele komunikatów
    pub fn emit_all(&self, diags: &[Diag]) {
        for d in diags {
            self.emit(d);
        }
    }
}

// ─── Analiza ostrzeżeń i podpowiedzi ─────────────────────────────────────────

/// Sprawdza źródło HL pod kątem typowych anty-wzorców i zwraca listę ostrzeżeń
pub fn lint_source(source: &str) -> Vec<Diag> {
    let mut diags = Vec::new();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();

        // ── Wykryj `> sudo ...` — sugestia użycia `^>` ──────────────────────
        if let Some(rest) = strip_cmd_prefix(trimmed, ">") {
            let rest = rest.trim();
            if rest.starts_with("sudo ") {
                let actual_cmd = rest.trim_start_matches("sudo").trim();
                let col = raw_line.find('>').map(|c| c + 1).unwrap_or(1);
                diags.push(
                    Diag::warning(format!(
                        "użycie `> sudo {}` — zamiast tego użyj operatora `^>`",
                        actual_cmd
                    ))
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(format!(
                        "zamień na: `^> {}`",
                        actual_cmd
                    ))
                    .with_note("operator `^>` jest natywnym odpowiednikiem sudo w Hacker Lang"),
                );
            }
        }

        // ── Wykryj `>> sudo ...` — sugestia `^>>` ───────────────────────────
        if let Some(rest) = strip_cmd_prefix(trimmed, ">>") {
            let rest = rest.trim();
            if rest.starts_with("sudo ") {
                let actual_cmd = rest.trim_start_matches("sudo").trim();
                let col = raw_line.find(">>").map(|c| c + 1).unwrap_or(1);
                diags.push(
                    Diag::warning(format!(
                        "użycie `>> sudo {}` — zamiast tego użyj operatora `^>>`",
                        actual_cmd
                    ))
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(format!("zamień na: `^>> {}`", actual_cmd))
                    .with_note("operator `^>>` łączy interpolację zmiennych z uprawnieniami sudo"),
                );
            }
        }

        // ── Wykryj `-> sudo ...` — sugestia `^->` ───────────────────────────
        if let Some(rest) = strip_cmd_prefix(trimmed, "->") {
            let rest = rest.trim();
            if rest.starts_with("sudo ") {
                let actual_cmd = rest.trim_start_matches("sudo").trim();
                let col = raw_line.find("->").map(|c| c + 1).unwrap_or(1);
                diags.push(
                    Diag::warning(format!(
                        "użycie `-> sudo {}` — zamiast tego użyj `^->`",
                        actual_cmd
                    ))
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(format!("zamień na: `^-> {}`", actual_cmd))
                    .with_note("`^->` uruchamia komendę izolowanej przestrzeni nazw z sudo"),
                );
            }
        }

        // ── Wykryj `> echo ...` — twarde ostrzeżenie, echo zakazane ─────────
        if let Some(rest) = strip_cmd_prefix(trimmed, ">") {
            let rest = rest.trim();
            if rest.starts_with("echo ") || rest == "echo" {
                let msg = rest.trim_start_matches("echo").trim();
                let col = raw_line.find('>').map(|c| c + 1).unwrap_or(1);
                diags.push(
                    Diag::error("`echo` jest zabronione w blokach komend Hacker Lang")
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(if msg.is_empty() {
                        "użyj: `:: <wiadomość>`".to_string()
                    } else {
                        format!("zamień na: `:: {}`", msg)
                    })
                    .with_note("operator `::` to jedyny sposób wypisywania tekstu w HL"),
                );
            }
        }

        // ── Wykryj `>> echo ...` ─────────────────────────────────────────────
        if let Some(rest) = strip_cmd_prefix(trimmed, ">>") {
            let rest = rest.trim();
            if rest.starts_with("echo ") || rest == "echo" {
                let msg = rest.trim_start_matches("echo").trim();
                let col = raw_line.find(">>").map(|c| c + 1).unwrap_or(1);
                diags.push(
                    Diag::error("`echo` jest zabronione w blokach komend Hacker Lang")
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(format!("zamień na: `:: {}`", msg))
                    .with_note("zmienne (@var) są automatycznie interpolowane przez `::`"),
                );
            }
        }

        // ── Wykryj zmienną bez @ ─── @var w stringu bez cudzysłowu ──────────
        // Jeśli linia to `% name = coś` a wartość wygląda jak zmienna bez @
        if trimmed.starts_with('%') {
            if let Some(eq_pos) = trimmed.find('=') {
                let value = trimmed[eq_pos + 1..].trim();
                // Jeśli wartość zaczyna się od @ — OK. Ostrzeżenie gdy brak @ a wygląda jak ref
                // (heurystyka: wartość to samo słowo pisane wielką literą == prawdopodobna zmienna)
                let _ = value; // zarezerwowane na przyszłe heurystyki
            }
        }

        // ── Ostrzeżenie: pusta funkcja ────────────────────────────────────────
        // `: name def` od razu `done` na następnej linii
        if trimmed.starts_with(':') && trimmed.ends_with("def") {
            if let Some(next_line) = source.lines().nth(line_no) {
                if next_line.trim() == "done" {
                    let col = raw_line.find(':').map(|c| c + 1).unwrap_or(1);
                    diags.push(
                        Diag::warning("pusta definicja funkcji")
                        .with_span(Span::new(line_no, col, trimmed.len()))
                        .with_suggestion("dodaj ciało funkcji przed `done` lub usuń definicję")
                        .with_note("puste funkcje są dozwolone, ale prawdopodobnie to błąd"),
                    );
                }
            }
        }

        // ── Podpowiedź: brak // dla popularnych narzędzi sieciowych ──────────
        lazy_check_missing_dep(trimmed, line_no, source, &mut diags);
    }

    diags
}

/// Sprawdza czy użyto narzędzia bez wcześniejszej deklaracji `// narzędzie`
fn lazy_check_missing_dep(line: &str, line_no: usize, source: &str, diags: &mut Vec<Diag>) {
    // Narzędzia które warto deklarować
    const WATCHED: &[&str] = &["nmap", "curl", "wget", "whois", "john", "hydra",
    "sqlmap", "nikto", "masscan", "aircrack-ng", "hashcat"];

    let cmd_content = if let Some(r) = strip_cmd_prefix(line, ">>") { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, ">") { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, "->") { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, "^>") { r.to_string() }
    else { return };

    let first_word = cmd_content.trim().split_whitespace().next().unwrap_or("");

    if let Some(&tool) = WATCHED.iter().find(|&&t| t == first_word) {
        // Sprawdź czy `// tool` pojawia się gdzieś wcześniej w source
        let declared = source.lines().enumerate().any(|(i, l)| {
            i + 1 < line_no && {
                let t = l.trim();
                // `// nmap` (dep) ale nie `// komentarz blokowy \\`
                t.starts_with("//") && !t.ends_with("\\\\") && t.contains(tool)
            }
        });

        if !declared {
            diags.push(
                Diag::hint(format!(
                    "narzędzie `{}` użyte bez deklaracji zależności",
                    tool
                ))
                .with_span(Span::new(line_no, 1, line.len()))
                .with_suggestion(format!(
                    "dodaj na początku pliku: `// {}`",
                    tool
                ))
                .with_note("deklaracja `//` pozwala HL automatycznie zainstalować brakujące narzędzie"),
            );
        }
    }
}

/// Pomocnik: zdejmij prefix komendy i zwróć resztę (bez prefixu)
fn strip_cmd_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    // Kolejność ważna: najpierw dłuższe prefixy
    let line = line.trim();
    if prefix == ">>" && (line.starts_with("^>>") || line.starts_with("->>")) {
        return None; // unikaj fałszywych dopasowań
    }
    if prefix == ">" && (line.starts_with(">>") || line.starts_with("^>") || line.starts_with("->")) {
        return None;
    }
    if prefix == "->" && line.starts_with("^->") {
        return None;
    }
    if line.starts_with(prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
}

// ─── Licznik błędów / ostrzeżeń ──────────────────────────────────────────────

#[derive(Default)]
pub struct DiagSummary {
    pub errors: usize,
    pub warnings: usize,
    pub hints: usize,
}

impl DiagSummary {
    pub fn from_diags(diags: &[Diag]) -> Self {
        let mut s = Self::default();
        for d in diags {
            match d.level {
                DiagLevel::Error   => s.errors += 1,
                DiagLevel::Warning => s.warnings += 1,
                DiagLevel::Hint | DiagLevel::Note => s.hints += 1,
            }
        }
        s
    }

    pub fn has_errors(&self) -> bool { self.errors > 0 }

    pub fn print(&self) {
        if self.errors == 0 && self.warnings == 0 && self.hints == 0 {
            return;
        }
        let mut parts = vec![];
        if self.errors > 0 {
            parts.push(format!("{} błąd(y)", self.errors).red().bold().to_string());
        }
        if self.warnings > 0 {
            parts.push(format!("{} ostrzeżenie(a)", self.warnings).yellow().bold().to_string());
        }
        if self.hints > 0 {
            parts.push(format!("{} podpowiedź(zi)", self.hints).cyan().to_string());
        }
        eprintln!("{} {}", "hl:".bright_black().bold(), parts.join(", "));
    }
}

impl fmt::Display for DiagSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} errors, {} warnings, {} hints",
               self.errors, self.warnings, self.hints)
    }
}

// ─── Konwersja błędów parsera na Diag ────────────────────────────────────────

use crate::parser::ParseError;
use crate::lexer::LexError;

/// Konwertuje ParseError na ładny Diag z podpowiedzią
pub fn parse_error_to_diag(err: &ParseError) -> Diag {
    match err {
        ParseError::Lex(lex_err) => lex_error_to_diag(lex_err),

        ParseError::UnexpectedToken(pos, tok) => {
            Diag::error(format!("nieoczekiwany token `{}` (pozycja {})", tok, pos))
            .with_suggestion("sprawdź składnię HL — każda linia powinna zaczynać się od operatora (::, >, %, :, --, ? ok, ? err)")
            .with_note("użyj `hl check plik.hl` aby zobaczyć listę błędów")
        }

        ParseError::MissingDone => {
            Diag::error("brakujące `done` — blok nie jest zamknięty")
            .with_suggestion("dodaj `done` na końcu bloku funkcji lub warunkowego")
            .with_note("każdy blok `: nazwa def` oraz `? ok`/`? err` musi kończyć się słowem `done`")
        }

        ParseError::MissingDef => {
            Diag::error("brakujące `def` po nazwie funkcji")
            .with_suggestion("poprawna składnia: `: nazwa_funkcji def`")
            .with_note("np. `: scan def` ... `done`")
        }
    }
}

pub fn lex_error_to_diag(err: &LexError) -> Diag {
    match err {
        LexError::UnexpectedChar(ch, line, col) => {
            Diag::error(format!("nieoczekiwany znak `{}` w linii {}:{}", ch, line, col))
            .with_span(Span::new(*line, *col, 1))
            .with_suggestion("usuń lub zastąp nieznany znak — HL akceptuje tylko operatory ASCII i identyfikatory")
        }

        LexError::UnterminatedString(line) => {
            Diag::error("niezamknięty string — brakuje cudzysłowu zamykającego `\"`")
            .with_span(Span::line_only(*line))
            .with_suggestion("dodaj `\"` na końcu stringa")
            .with_note("stringi w HL: `\"zawartość\"` — mogą zawierać @zmienne")
        }

        LexError::UnterminatedBlockComment => {
            Diag::error("niezamknięty komentarz blokowy")
            .with_suggestion("zamknij komentarz dwoma backslashami: `// treść \\\\`")
            .with_note("komentarze blokowe: `// wieloliniowa treść \\\\`")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_sudo_in_cmd() {
        let src = "> sudo apt-get update";
        let diags = lint_source(src);
        assert!(!diags.is_empty());
        assert_eq!(diags[0].level, DiagLevel::Warning);
        assert!(diags[0].suggestion.as_ref().unwrap().contains("^>"));
    }

    #[test]
    fn test_lint_echo_blocked() {
        let src = "> echo hello world";
        let diags = lint_source(src);
        assert!(diags.iter().any(|d| d.level == DiagLevel::Error));
    }

    #[test]
    fn test_lint_clean() {
        let src = ":: hello\n% x = 1\n> ls -la";
        let diags = lint_source(src);
        // ls nie jest w WATCHED — no diags expected
        assert!(diags.is_empty());
    }
}
