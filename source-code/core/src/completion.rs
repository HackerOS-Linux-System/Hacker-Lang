use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

/// HL shell keywords for completion
const HL_KEYWORDS: &[&str] = &[
    // Output
    "~>",       // print (główny operator wyjścia)
    "::",       // quick-call prefix
// Quick functions
"::upper",  "::lower",  "::len",    "::trim",   "::rev",
"::split",  "::lines",  "::words",  "::replace","::contains",
"::startswith", "::endswith", "::repeat",
"::abs",    "::ceil",   "::floor",  "::round",  "::max",    "::min",    "::rand",
"::env",    "::date",   "::time",   "::pid",    "::which",
"::exists", "::isdir",  "::isfile", "::basename","::dirname","::read",
"::set",    "::get",    "::type",   "::unset",
"::nl",     "::hr",     "::bold",   "::red",    "::green",  "::yellow", "::cyan",
// Commands
">",    "^>",   "->",   "^->",  ">>",   "^>>",  "->>",
// Variables
"%",    "@",
// Dependencies / imports
"//",   "#",
// Comments
";;",   "///",
// Functions
":",    "--",
// Logic
"? ok", "? err", "done", "def",
// Literals
"true", "false",
];

pub struct HlCompleter {
    file_completer: FilenameCompleter,
}

impl HlCompleter {
    pub fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
        }
    }
}

impl Default for HlCompleter {
    fn default() -> Self {
        Self::new()
    }
}

impl Completer for HlCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Try HL keywords first
        let word_start = line[..pos]
        .rfind(|c: char| c.is_whitespace())
        .map(|i| i + 1)
        .unwrap_or(0);
        let current_word = &line[word_start..pos];

        let kw_matches: Vec<Pair> = HL_KEYWORDS
        .iter()
        .filter(|kw| kw.starts_with(current_word))
        .map(|kw| Pair {
            display: kw.to_string(),
             replacement: kw.to_string(),
        })
        .collect();

        if !kw_matches.is_empty() {
            return Ok((word_start, kw_matches));
        }

        // Fall back to file completion
        self.file_completer.complete(line, pos, ctx)
    }
}

impl Highlighter for HlCompleter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        // Simple syntax highlighting for the prompt line
        let mut result = String::new();
        let line = line;

        // Highlight operators
        if line.starts_with("~>") {
            result.push_str("\x1b[32m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with("::") {
            result.push_str("\x1b[35m"); // purple for quick-calls
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with(";;") {
            result.push_str("\x1b[90m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with("///") {
            result.push_str("\x1b[36m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with("^->") || line.starts_with("->") {
            result.push_str("\x1b[35m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with("^>") || line.starts_with('>') {
            result.push_str("\x1b[34m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }
        if line.starts_with('%') {
            result.push_str("\x1b[33m");
            result.push_str(line);
            result.push_str("\x1b[0m");
            return Cow::Owned(result);
        }

        Cow::Borrowed(line)
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }
}

impl Hinter for HlCompleter {
    type Hint = String;

    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        // Show hints for common operators
        match line.trim() {
            "~>"  => Some(" <message>  -- wypisz tekst".to_string()),
            "::"  => Some(" <fn> [args]  -- quick-function".to_string()),
            ">"   => Some(" <cmd>  -- uruchom komende".to_string()),
            "%"   => Some(" <n>=<v>  -- deklaruj zmienna".to_string()),
            "//"  => Some(" <pkg>  -- zaleznosc".to_string()),
            "#"   => Some(" <lib>  -- importuj biblioteke".to_string()),
            ":"   => Some(" <n> def  -- zdefiniuj funkcje".to_string()),
            "--"  => Some(" <n>  -- wywolaj funkcje".to_string()),
            _ => None,
        }
    }
}

impl Validator for HlCompleter {}

impl Helper for HlCompleter {}
