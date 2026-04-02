use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::borrow::Cow;

/// HL shell keywords for completion
const HL_KEYWORDS: &[&str] = &[
    "::",   // print
">",    // command
"^>",   // sudo command
"->",   // isolated command
"^->",  // isolated + sudo
">>",   // command with vars
"^>>",  // command with vars + sudo
"%",    // var decl
"@",    // var ref
"//",   // dependency
";;",   // line comment
"///",  // doc comment
":",    // function def
"--",   // function call
"? ok", // if ok
"? err",// if err
"done", // end block
"def",
"true",
"false",
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
        if line.starts_with("::") {
            result.push_str("\x1b[32m");
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
            "::" => Some(" <message>  – print to stdout".to_string()),
            ">" => Some(" <command>  – run command".to_string()),
            "%" => Some(" <name> = <value>  – declare variable".to_string()),
            "//" => Some(" <package>  – require dependency".to_string()),
            ":" => Some(" <name> def  – define function".to_string()),
            "--" => Some(" <name>  – call function".to_string()),
            _ => None,
        }
    }
}

impl Validator for HlCompleter {}

impl Helper for HlCompleter {}
