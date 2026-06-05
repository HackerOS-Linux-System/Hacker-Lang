use colored::Colorize;
use std::fmt;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum DiagLevel { Error, Warning, Hint, Note }

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
            DiagLevel::Error   => "\x1b[31m",
            DiagLevel::Warning => "\x1b[33m",
            DiagLevel::Hint    => "\x1b[36m",
            DiagLevel::Note    => "\x1b[90m",
        }
    }
    fn marker(&self) -> &'static str {
        match self { DiagLevel::Error=>"^", DiagLevel::Warning=>"~", DiagLevel::Hint=>"-", DiagLevel::Note=>"." }
    }
}

#[derive(Debug, Clone)]
pub struct Span { pub line: usize, pub col: usize, pub len: usize }
impl Span {
    pub fn new(line: usize, col: usize, len: usize) -> Self { Self { line, col, len } }
    pub fn line_only(line: usize) -> Self { Self { line, col: 1, len: 0 } }
}

#[derive(Debug, Clone)]
pub struct Diag {
    pub level: DiagLevel, pub message: String,
    pub span: Option<Span>, pub suggestion: Option<String>, pub notes: Vec<String>,
}
impl Diag {
    pub fn error(msg: impl Into<String>)   -> Self { Self { level: DiagLevel::Error,   message: msg.into(), span: None, suggestion: None, notes: vec![] } }
    pub fn warning(msg: impl Into<String>) -> Self { Self { level: DiagLevel::Warning, message: msg.into(), span: None, suggestion: None, notes: vec![] } }
    pub fn hint(msg: impl Into<String>)    -> Self { Self { level: DiagLevel::Hint,    message: msg.into(), span: None, suggestion: None, notes: vec![] } }
    pub fn with_span(mut self, span: Span) -> Self { self.span = Some(span); self }
    pub fn with_suggestion(mut self, s: impl Into<String>) -> Self { self.suggestion = Some(s.into()); self }
    pub fn with_note(mut self, n: impl Into<String>) -> Self { self.notes.push(n.into()); self }
}

pub struct DiagRenderer<'a> { pub filename: &'a str, pub lines: Vec<&'a str> }
impl<'a> DiagRenderer<'a> {
    pub fn new(filename: &'a str, source: &'a str) -> Self {
        Self { filename, lines: source.lines().collect() }
    }
    pub fn emit(&self, diag: &Diag) {
        let gc = diag.level.gutter_color(); let reset = "\x1b[0m";
        eprintln!("{}: {}", diag.level.label(), diag.message.white().bold());
        if let Some(ref span) = diag.span {
            eprintln!("  {} {}:{}:{}", "-->".bright_black(), self.filename.bright_white(), span.line, span.col);
            let line_idx = span.line.saturating_sub(1);
            let line_num_w = format!("{}", span.line).len().max(2);
            if line_idx > 0 { if let Some(prev) = self.lines.get(line_idx - 1) { eprintln!("{}{:>w$} |{} {}", gc, span.line-1, reset, prev.bright_black(), w=line_num_w); } }
            if let Some(src_line) = self.lines.get(line_idx) {
                eprintln!("{}{:>w$} |{} {}", gc, span.line, reset, src_line, w=line_num_w);
                let col0 = span.col.saturating_sub(1);
                let marker_len = if span.len == 0 { src_line.trim_start().len().max(1) } else { span.len };
                let spaces = " ".repeat(line_num_w + 3 + col0);
                eprintln!("{}{}{}{}", spaces, gc, diag.level.marker().repeat(marker_len), reset);
            }
            if let Some(next) = self.lines.get(line_idx + 1) { eprintln!("{}{:>w$} |{} {}", gc, span.line+1, reset, next.bright_black(), w=line_num_w); }
            eprintln!("{}{:>w$} |{}", gc, "", reset, w=line_num_w);
        } else {
            eprintln!("  {} {}", "-->".bright_black(), self.filename.bright_white());
        }
        if let Some(ref sug) = diag.suggestion { eprintln!("  {} {}", "help:".bright_cyan().bold(), sug.bright_white()); }
        for note in &diag.notes { eprintln!("  {} {}", "note:".bright_black().bold(), note.bright_black()); }
        eprintln!();
    }
    pub fn emit_all(&self, diags: &[Diag]) { for d in diags { self.emit(d); } }
}

pub fn lint_source(source: &str) -> Vec<Diag> {
    let mut diags = Vec::new();

    // ── OPTYMALIZACJA: jednorazowy pre-pass dla deklaracji narzedzi ────────────
    // Poprzednio: O(n^2) - dla kazdej linii komendy iterowalo wszystkie linie
    // Teraz: O(n) - jeden HashSet zbierany na poczatku
    let declared_tools: HashSet<&str> = source.lines()
    .filter_map(|l| {
        let t = l.trim();
        // linia // narzedzie (nie ///, nie blok komentarz z \\)
        if t.starts_with("//") && !t.starts_with("///") && !t.ends_with("\\\\") {
            let tool = t[2..].trim();
            if !tool.is_empty() && !tool.contains(' ') { Some(tool) } else { None }
        } else { None }
    })
    .collect();

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();

        // echo zakazane w blokach >
        if let Some(rest) = strip_cmd_prefix(trimmed, ">") {
            let rest = rest.trim();
            if rest.starts_with("echo ") || rest == "echo" {
                let msg = rest.trim_start_matches("echo").trim();
                let col = raw_line.find('>').map(|c| c+1).unwrap_or(1);
                diags.push(Diag::error("`echo` jest zabronione w blokach komend HL")
                .with_span(Span::new(line_no, col, trimmed.len()))
                .with_suggestion(if msg.is_empty() { "uzyj: `~>`".into() } else { format!("zamien na: `~> {}`", msg) })
                .with_note("operator `~>` to jedyny sposob wypisywania tekstu w HL"));
            }
            // sudo zamiast ^>
            if rest.starts_with("sudo ") {
                let actual_cmd = rest.trim_start_matches("sudo").trim();
                let col = raw_line.find('>').map(|c| c+1).unwrap_or(1);
                diags.push(Diag::warning("`> sudo` — uzyj operatora `^>`".to_string())
                .with_span(Span::new(line_no, col, trimmed.len()))
                .with_suggestion(format!("zamien na: `^> {}`", actual_cmd))
                .with_note("`^>` to natywny odpowiednik sudo w HL"));
            }
        }

        // % PATH zamiast =>
        if trimmed.starts_with('%') {
            if let Some(eq_pos) = trimmed.find('=') {
                let varname = trimmed[1..eq_pos].trim().trim_end_matches(':')
                .split(':').next().unwrap_or("").trim();
                const ENV_VARS: &[&str] = &["PATH","HOME","USER","SHELL","LANG","LD_LIBRARY_PATH",
                "JAVA_HOME","GOPATH","CARGO_HOME","PYTHONPATH"];
                if ENV_VARS.contains(&varname) {
                    let col = raw_line.find('%').map(|c| c+1).unwrap_or(1);
                    diags.push(Diag::hint(format!("`%{}` to zmienna lokalna HL — uzyj `=>` dla exportu", varname))
                    .with_span(Span::new(line_no, col, trimmed.len()))
                    .with_suggestion(format!("zamien na: `=> {} = <wartosc>`", varname)));
                }
            }
        }

        // Sprawdz narzedzia — uzywa pre-obliczonego HashSet (O(1) lookup)
        check_missing_dep_fast(trimmed, line_no, &declared_tools, &mut diags);
    }
    diags
}

/// Sprawdz czy narzedzie jest uzywane bez deklaracji //
/// Uzywa przekazanego HashSet zamiast skanowac cale zrodlo (O(1) vs O(n))
fn check_missing_dep_fast(line: &str, line_no: usize, declared: &HashSet<&str>, diags: &mut Vec<Diag>) {
    const WATCHED: &[&str] = &["nmap","curl","wget","whois","john","hydra","sqlmap",
    "nikto","masscan","aircrack-ng","hashcat","git","python3"];
    let cmd_content = if let Some(r) = strip_cmd_prefix(line, ">>") { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, ">")   { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, "->")  { r.to_string() }
    else if let Some(r) = strip_cmd_prefix(line, "^>")  { r.to_string() }
    else { return };

    let first_word = cmd_content.trim().split_whitespace().next().unwrap_or("");
    if let Some(&tool) = WATCHED.iter().find(|&&t| t == first_word) {
        if !declared.contains(tool) {
            diags.push(Diag::hint(format!("narzedzie `{}` uzyte bez `// {}`", tool, tool))
            .with_span(Span::new(line_no, 1, line.len()))
            .with_suggestion(format!("dodaj: `// {}`", tool)));
        }
    }
}

fn strip_cmd_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let line = line.trim();
    if prefix == ">>" && (line.starts_with("^>>") || line.starts_with("->>")) { return None; }
    if prefix == ">"  && (line.starts_with(">>") || line.starts_with("^>") || line.starts_with("->") || line.starts_with("*>")) { return None; }
    if prefix == "->" && line.starts_with("^->") { return None; }
    if line.starts_with(prefix) { Some(&line[prefix.len()..]) } else { None }
}

#[derive(Default)]
pub struct DiagSummary { pub errors: usize, pub warnings: usize, pub hints: usize }
impl DiagSummary {
    pub fn from_diags(diags: &[Diag]) -> Self {
        let mut s = Self::default();
        for d in diags { match d.level { DiagLevel::Error=>s.errors+=1, DiagLevel::Warning=>s.warnings+=1, _=>s.hints+=1 } }
        s
    }
    pub fn has_errors(&self) -> bool { self.errors > 0 }
    pub fn print(&self) {
        if self.errors==0 && self.warnings==0 && self.hints==0 { return; }
        let mut parts = vec![];
        if self.errors>0   { parts.push(format!("{} blad(y)", self.errors).red().bold().to_string()); }
        if self.warnings>0 { parts.push(format!("{} ostrzezenie(a)", self.warnings).yellow().bold().to_string()); }
        if self.hints>0    { parts.push(format!("{} podpowiedz(zi)", self.hints).cyan().to_string()); }
        eprintln!("{} {}", "hl:".bright_black().bold(), parts.join(", "));
    }
}
impl fmt::Display for DiagSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} errors, {} warnings, {} hints", self.errors, self.warnings, self.hints)
    }
}

use hl_parser::parser::ParseError;
use hl_parser::lexer::LexError;

pub fn parse_error_to_diag(err: &ParseError) -> Diag {
    match err {
        ParseError::Lex(e) => lex_error_to_diag(e),
        ParseError::UnexpectedToken(pos, tok) => Diag::error(format!("nieoczekiwany token `{}` (pozycja {})", tok, pos))
        .with_suggestion("sprawdz skladnie — kazda linia powinna zaczynac sie od operatora"),
        ParseError::MissingDone => Diag::error("brakujace `done` — blok nie jest zamkniety")
        .with_suggestion("dodaj `done` na koncu bloku"),
        ParseError::MissingDef  => Diag::error("brakujace `def` po nazwie funkcji")
        .with_suggestion("poprawna skladnia: `: nazwa_funkcji def`"),
        ParseError::MissingExportListEnd => Diag::error("brakujace `]` — lista eksportu nie jest zamknieta")
        .with_suggestion("dodaj `]` na koncu listy"),
        ParseError::Gen(gen_err) => Diag::error(format!("blad deklaracji gena: {}", gen_err))
        .with_suggestion("poprawna skladnia: `using <gen 2>`"),
    }
}

pub fn lex_error_to_diag(err: &LexError) -> Diag {
    match err {
        LexError::UnexpectedChar(ch, line, col) => Diag::error(format!("nieoczekiwany znak `{}` w linii {}:{}", ch, line, col))
        .with_span(Span::new(*line, *col, 1))
        .with_suggestion("usun lub zastap nieznany znak"),
        LexError::UnterminatedString(line) => Diag::error("niezamkniety string")
        .with_span(Span::line_only(*line))
        .with_suggestion("dodaj `\"` na koncu stringa"),
        LexError::UnterminatedBlockComment => Diag::error("niezamkniety komentarz blokowy")
        .with_suggestion("zamknij komentarz: `//  tresc  \\\\`"),
    }
}

pub fn lint_gen(source: &str) -> Vec<Diag> {
    use hl_parser::gen::{extract_gen, HL_MAX_GEN};
    let mut diags = Vec::new();
    let (_gen, gen_err) = extract_gen(source);
    if let Some(err) = gen_err {
        diags.push(Diag::error(format!("nieprawidlowa deklaracja gena: {}", err))
        .with_suggestion(format!("poprawna skladnia: `using <gen 2>`  (max gen: {})", HL_MAX_GEN)));
        return diags;
    }
    let mut seen_code = false;
    for (idx, raw_line) in source.lines().enumerate() {
        let t = raw_line.trim();
        if t.starts_with("#!") || t.starts_with(";;") || t.starts_with("///") || t.starts_with("//") || t.is_empty() { continue; }
            if t.starts_with("using") {
                if seen_code {
                    diags.push(Diag::warning("deklaracja `using` po kodzie — gen moze nie byc uwzgledniony")
                    .with_span(Span::new(idx+1, 1, t.len()))
                    .with_suggestion("umies `using <gen N>` na samym poczatku pliku"));
                }
                continue;
            }
            seen_code = true;
    }
    diags
}
