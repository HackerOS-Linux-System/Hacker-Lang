use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

const HL_KEYWORDS: &[&str] = &[
    // Wyjscie
    "~>",
    // Quick functions
    "::", "::upper", "::lower", "::len", "::trim", "::rev", "::split",
    "::lines", "::words", "::replace", "::contains", "::startswith", "::endswith",
    "::repeat", "::abs", "::ceil", "::floor", "::round", "::max", "::min", "::rand",
    "::env", "::date", "::time", "::pid", "::which", "::exists", "::isdir",
    "::isfile", "::basename", "::dirname", "::read", "::set", "::get", "::type",
    "::unset", "::nl", "::hr", "::bold", "::red", "::green", "::yellow", "::cyan",
    // Komendy
    ">", "^>", "->", "^->", ">>", "^>>", "->>",
    // Nowe (gen 1)
    "&",    // background
    "*>",   // hsh command
    ":*",   // goroutine
    ":**",  // channel
    "*--",  // channel op
    "_",    // repeat N times (np. _10)
    "<<",   // file import
    // Zmienne
    "%", "@", "=>",
    // Definicje
    "//", "#", ";;", "///", ":", "--", "? ok", "? err", "done", "def",
    // Importy
    "# <main/>", "# <bit/>", "# <github/>",
    // Wartosci
    "true", "false",
    // Gen
    "using",
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
        let (color, reset) = if line.starts_with("~>")                            { ("\x1b[32m", "\x1b[0m") }
            else if line.starts_with("::")                                         { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with(";;") || line.starts_with("///")             { ("\x1b[90m", "\x1b[0m") }
            else if line.starts_with(":*") || line.starts_with(":**")             { ("\x1b[35m", "\x1b[0m") } // goroutine/channel
            else if line.starts_with("*>")                                         { ("\x1b[33m", "\x1b[0m") } // hsh
            else if line.starts_with("*--")                                        { ("\x1b[35m", "\x1b[0m") } // channel op
            else if line.starts_with('&')                                          { ("\x1b[36m", "\x1b[0m") } // background
            else if line.starts_with("<<")                                         { ("\x1b[36m", "\x1b[0m") } // file import
            else if line.starts_with('_') && line.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) { ("\x1b[33m", "\x1b[0m") } // _N
            else if line.starts_with("^->") || line.starts_with("->")             { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with("^>") || line.starts_with('>')               { ("\x1b[34m", "\x1b[0m") }
            else if line.starts_with("=>")                                         { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with('%')                                          { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with("using")                                      { ("\x1b[36m", "\x1b[0m") }
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
            "#"     => Some(" <main/lib>  -- importuj biblioteke".into()),
            ":"     => Some(" <n> def  -- zdefiniuj funkcje".into()),
            "--"    => Some(" <n>  -- wywolaj funkcje".into()),
            "using" => Some(" <gen 1>  -- deklaruj gen HL".into()),
            "&"     => Some(" <cmd>  -- uruchom w tle".into()),
            "*>"    => Some(" <cmd>  -- uruchom przez hsh".into()),
            ":*"    => Some("  -- goroutine (blok + done)".into()),
            ":**"   => Some(" <nazwa>  -- zadeklaruj channel".into()),
            "*--"   => Some(" <nazwa>  -- operacja na channel".into()),
            "<<"    => Some(" <plik.hl>  -- importuj plik".into()),
            _       => {
                // _N hint
                if line.starts_with('_') && line.len() > 1 && line.chars().skip(1).all(|c| c.is_ascii_digit()) {
                    Some(format!(" > <cmd>  -- powtorz {} razy", &line[1..]))
                } else {
                    None
                }
            }
        }
    }
}

impl Validator for HlCompleter {}
impl Helper for HlCompleter {}
