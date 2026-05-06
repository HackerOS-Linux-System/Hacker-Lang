use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

const HL_KEYWORDS: &[&str] = &[
    // Output
    "~>",
    // Quick functions
    "::", "::upper", "::lower", "::len", "::trim", "::rev", "::split",
    "::lines", "::words", "::replace", "::contains", "::startswith", "::endswith",
    "::repeat", "::abs", "::ceil", "::floor", "::round", "::max", "::min", "::rand",
    "::env", "::date", "::time", "::pid", "::which", "::exists", "::isdir",
    "::isfile", "::basename", "::dirname", "::read", "::set", "::get", "::type",
    "::unset", "::nl", "::hr", "::bold", "::red", "::green", "::yellow", "::cyan",
    // Commands gen 1
    ">", "^>", "->", "^->", ">>", "^>>", "->>",
    // Gen 1 new
    "&", "*>", ":*", ":**", "*--",
    // Gen 2 — pipe do zmiennej
    "|>",
    // Gen 2 — arytmetyka
    "$(", "$(( ))",
    // Gen 2 — for-in
    "@ item in",
    // Gen 2 — while
    "?~",
    // Gen 2 — switch
    "? switch",
    // Gen 2 — HackerOS API
    "||",
    "|| hacker", "|| hco", "|| hsh", "|| hpkg", "|| lpm",
    "|| Blue-Environment", "|| hnm", "|| hpm", "|| hedit",
    "|| ngt", "|| eiq", "|| getit", "|| hdev", "|| anvil",
    "|| a", "|| hbuild", "|| chker", "|| isolator",
    "|| hackeros-steam", "|| ulb", "|| gameframe",
    "|| hup", "|| hackeros-builder", "|| H#",
    // Gen 2 — typowane zmienne
    "% name: int =", "% name: float =", "% name: str =", "% name: bool =",
    // Pozostale
    "_", "%", "@", "=>",
    "//", "#", ";;", "///", ":", "--", "? ok", "? err", "done", "def",
    // Importy
    "# <main/>", "# <bit/>", "# <github/>",
    "# <main/net>", "# <main/fs>", "# <main/sys>", "# <main/str>",
    "# <main/colors>", "# <main/cli>", "# <main/progress-bar>",
    "# <main/json>", "# <main/hk-parser>", "# <main/hacker>",
    "# <main/crypto>", "# <main/proc>",
    // Import pliku
    "<<",
    // Wartosci
    "true", "false",
    // Gen
    "using", "using <gen 1>", "using <gen 2>",
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
        let (color, reset) = if line.starts_with("~>")                                              { ("\x1b[32m", "\x1b[0m") }
            else if line.starts_with("::")                                                           { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with(";;") || line.starts_with("///")                               { ("\x1b[90m", "\x1b[0m") }
            // Gen 2
            else if line.starts_with("$(")                                                          { ("\x1b[33m", "\x1b[0m") } // arytmetyka
            else if line.starts_with("||")                                                          { ("\x1b[95m", "\x1b[0m") } // HackerOS API
            else if line.starts_with("?~")                                                          { ("\x1b[36m", "\x1b[0m") } // while
            else if line.starts_with("? switch")                                                    { ("\x1b[36m", "\x1b[0m") } // switch
            else if line.starts_with('|')                                                           { ("\x1b[36m", "\x1b[0m") } // case arm
            else if line.starts_with('@') && line.contains(" in ")                                  { ("\x1b[33m", "\x1b[0m") } // for-in
            // Gen 1
            else if line.starts_with(":*") || line.starts_with(":**")                               { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with("*>")                                                          { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with("*--")                                                         { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with('&')                                                           { ("\x1b[36m", "\x1b[0m") }
            else if line.starts_with("<<")                                                          { ("\x1b[36m", "\x1b[0m") }
            else if line.starts_with('_') && line.chars().nth(1).map_or(false,|c| c.is_ascii_digit()) { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with("^->") || line.starts_with("->")                               { ("\x1b[35m", "\x1b[0m") }
            else if line.starts_with("^>") || line.starts_with('>')                                 { ("\x1b[34m", "\x1b[0m") }
            else if line.starts_with("=>")                                                          { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with('%')                                                           { ("\x1b[33m", "\x1b[0m") }
            else if line.starts_with("using")                                                       { ("\x1b[36m", "\x1b[0m") }
            else { ("", "") };

        if color.is_empty() { Cow::Borrowed(line) }
        else { Cow::Owned(format!("{}{}{}", color, line, reset)) }
    }
    fn highlight_char(&self, _line: &str, _pos: usize) -> bool { true }
}

impl Hinter for HlCompleter {
    type Hint = String;
    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        let t = line.trim();
        match t {
            "~>"     => Some(" <tekst>  -- wypisz tekst".into()),
            "::"     => Some(" <fn> [args]  -- quick-function".into()),
            ">"      => Some(" <cmd>  -- komenda  |  > cmd |> @var".into()),
            "%"      => Some(" <n>=<v>  |  % n: int = v  -- typowana".into()),
            "=>"     => Some(" <n>=<v>  -- export do srodowiska".into()),
            "//"     => Some(" <pkg>  -- zaleznosc".into()),
            "#"      => Some(" <main/lib>  -- importuj biblioteke".into()),
            ":"      => Some(" <n> def  -- zdefiniuj funkcje".into()),
            "--"     => Some(" <n>  -- wywolaj funkcje".into()),
            "using"  => Some(" <gen 2>  -- deklaruj gen HL".into()),
            "&"      => Some(" <cmd>  -- uruchom w tle".into()),
            "*>"     => Some(" <cmd>  -- uruchom przez hsh".into()),
            ":*"     => Some(" [nazwa] def  -- goroutine".into()),
            ":**"    => Some(" <nazwa>  -- zadeklaruj channel".into()),
            "*--"    => Some(" <nazwa>  -- channel op".into()),
            "<<"     => Some(" <plik.hl>  -- importuj plik".into()),
            "$("     => Some(" expr )  -- arytmetyka  |  $( expr ) -> @var".into()),
            "||"     => Some(" <narzedzie> [args]  -- HackerOS API".into()),
            "@"      => Some(" <var> in <lista>  -- for-in loop".into()),
            "?~"     => Some(" <warunek>  -- while loop".into()),
            "? switch" => Some(" <@var>  -- switch/case".into()),
            "|"      => Some(" <pattern>  -- case arm (w switch)".into()),
            _ => {
                if t.starts_with('_') && t.len() > 1 && t[1..].chars().all(|c| c.is_ascii_digit()) {
                    Some(format!(" > <cmd>  -- powtorz {} razy", &t[1..]))
                } else { None }
            }
        }
    }
}

impl Validator for HlCompleter {}
impl Helper for HlCompleter {}
