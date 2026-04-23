use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

const HL_KEYWORDS: &[&str] = &[
    "~>", "::", "::upper", "::lower", "::len", "::trim", "::rev", "::split",
    "::lines", "::words", "::replace", "::contains", "::startswith", "::endswith",
    "::repeat", "::abs", "::ceil", "::floor", "::round", "::max", "::min", "::rand",
    "::env", "::date", "::time", "::pid", "::which", "::exists", "::isdir",
    "::isfile", "::basename", "::dirname", "::read", "::set", "::get", "::type",
    "::unset", "::nl", "::hr", "::bold", "::red", "::green", "::yellow", "::cyan",
    ">", "^>", "->", "^->", ">>", "^>>", "->>", "%", "@", "=>",
    "//", "#", ";;", "///", ":", "--", "? ok", "? err", "done", "def",
    "true", "false", "using",
];

pub struct HlCompleter { file: FilenameCompleter }

impl HlCompleter {
    pub fn new() -> Self { Self { file: FilenameCompleter::new() } }
}

impl Default for HlCompleter { fn default() -> Self { Self::new() } }

impl Completer for HlCompleter {
    type Candidate = Pair;
    fn complete(&self, line: &str, pos: usize, ctx: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let word_start = line[..pos].rfind(|c: char| c.is_whitespace()).map(|i| i + 1).unwrap_or(0);
        let current_word = &line[word_start..pos];
        let kw_matches: Vec<Pair> = HL_KEYWORDS.iter()
            .filter(|kw| kw.starts_with(current_word))
            .map(|kw| Pair { display: kw.to_string(), replacement: kw.to_string() })
            .collect();
        if !kw_matches.is_empty() { return Ok((word_start, kw_matches)); }
        self.file.complete(line, pos, ctx)
    }
}

impl Highlighter for HlCompleter {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        let mut result = String::new();
        let (color, reset) = if line.starts_with("~>")        { ("\x1b[32m", "\x1b[0m") }
            else if line.starts_with("::")                     { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with(";;") || line.starts_with("///") { ("\x1b[90m", "\x1b[0m") }
            else if line.starts_with("^->") || line.starts_with("->") { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with("^>") || line.starts_with('>')   { ("\x1b[34m", "\x1b[0m") }
            else if line.starts_with("=>")                     { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with('%')                      { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with("using")                  { ("\x1b[36m", "\x1b[0m") }
            else { ("", "") };
        result.push_str(color);
        result.push_str(line);
        result.push_str(reset);
        if color.is_empty() { Cow::Borrowed(line) } else { Cow::Owned(result) }
    }
    fn highlight_char(&self, _line: &str, _pos: usize) -> bool { true }
}

impl Hinter for HlCompleter {
    type Hint = String;
    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        match line.trim() {
            "~>"    => Some(" <message>  -- wypisz tekst".into()),
            "::"    => Some(" <fn> [args]  -- quick-function".into()),
            ">"     => Some(" <cmd>  -- uruchom komende".into()),
            "%"     => Some(" <n>=<v>  -- deklaruj zmienna".into()),
            "=>"    => Some(" <n>=<v>  -- export do srodowiska".into()),
            "//"    => Some(" <pkg>  -- zaleznosc".into()),
            "#"     => Some(" <lib>  -- importuj biblioteke".into()),
            ":"     => Some(" <n> def  -- zdefiniuj funkcje".into()),
            "--"    => Some(" <n>  -- wywolaj funkcje".into()),
            "using" => Some(" <gen N>  -- deklaruj gen HL".into()),
            _       => None,
        }
    }
}

impl Validator for HlCompleter {}
impl Helper for HlCompleter {}
