use anyhow::{bail, Result};
use std::fs;
use std::io::{self, Read};

pub mod common;
pub mod tools;

/// Wynik wykonania wbudowanej komendy
pub struct BuiltinResult {
    pub exit_code: i32,
    pub stdout:    Option<String>,  // Some gdy capture=true
}

impl BuiltinResult {
    pub fn ok()                    -> Self { Self { exit_code: 0, stdout: None } }
    pub fn err(code: i32)          -> Self { Self { exit_code: code, stdout: None } }
    pub fn captured(s: String)     -> Self { Self { exit_code: 0, stdout: Some(s) } }
    pub fn is_ok(&self)            -> bool { self.exit_code == 0 }
}

pub fn run_builtin(raw: &str, capture: bool) -> Result<BuiltinResult> {
    let raw = raw.trim();
    if raw.is_empty() { return Ok(BuiltinResult::ok()); }

    let parts = shell_split(raw);
    if parts.is_empty() { return Ok(BuiltinResult::ok()); }

    let cmd  = parts[0].as_str();
    let args = &parts[1..];

    let result = match cmd {
        "echo"     => tools::echo::builtin_echo(args),
        "cat"      => tools::cat::builtin_cat(args),
        "ls"       => tools::ls::builtin_ls(args),
        "grep"     => tools::grep::builtin_grep(args),
        "head"     => tools::head::builtin_head(args),
        "tail"     => tools::tail::builtin_tail(args),
        "wc"       => tools::wc::builtin_wc(args),
        "find"     => tools::find::builtin_find(args),
        "cp"       => tools::cp::builtin_cp(args),
        "mv"       => tools::mv::builtin_mv(args),
        "rm"       => tools::rm::builtin_rm(args),
        "mkdir"    => tools::mkdir::builtin_mkdir(args),
        "touch"    => tools::touch::builtin_touch(args),
        "sort"     => tools::sort::builtin_sort(args),
        "uniq"     => tools::uniq::builtin_uniq(args),
        "cut"      => tools::cut::builtin_cut(args),
        "tr"       => tools::tr::builtin_tr(args),
        "rev"      => tools::rev::builtin_rev(args),
        "basename" => tools::basename::builtin_basename(args),
        "dirname"  => tools::dirname::builtin_dirname(args),
        "stat"     => tools::stat::builtin_stat(args),
        "du"       => tools::du::builtin_du(args),
        "chmod"    => tools::chmod::builtin_chmod(args),
        "pwd"      => tools::pwd::builtin_pwd(),
        "which"    => tools::which::builtin_which(args),
        "env"      => tools::env::builtin_env(args),
        "date"     => tools::date::builtin_date(args),
        "sleep"    => tools::sleep::builtin_sleep(args),
        "true"     => Ok(BuiltinResult::ok()),
        "false"    => Ok(BuiltinResult::err(1)),
        "seq"      => tools::seq::builtin_seq(args),
        "printf"   => tools::printf::builtin_printf(args),
        "yes"      => tools::yes::builtin_yes(args),
        "tee"      => tools::tee::builtin_tee(args),
        "xargs"    => tools::xargs::builtin_xargs(args),
        _          => bail!("[/>] nieznana komenda: '{}'. Użyj > dla zewnętrznych narzędzi.", cmd),
    }?;

    // Jeśli capture=true i brak stdout (komenda pisała do stdout), to zbierz przez bufor
    // Dla uproszczenia: builtin functions zwracają dane w stdout Option
    if capture {
        if let Some(s) = result.stdout {
            return Ok(BuiltinResult { exit_code: result.exit_code, stdout: Some(s) });
        }
        // Komenda nie zwróciła bufora — to normalny wynik bez przechwytywania
        return Ok(BuiltinResult::captured(String::new()));
    }

    Ok(result)
}

// ── echo ──────────────────────────────────────────────────────────────────────

fn shell_split(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur   = String::new();
    let mut in_sq = false;
    let mut in_dq = false;
    for c in s.chars() {
        match c {
            '\'' if !in_dq => in_sq = !in_sq,
            '"'  if !in_sq => in_dq = !in_dq,
            ' ' | '\t' if !in_sq && !in_dq => {
                if !cur.is_empty() { words.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { words.push(cur); }
    words
}

fn read_files_or_stdin(files: Vec<String>) -> Result<String> {
    if files.is_empty() {
        let mut buf = String::new();
        io::stdin().lock().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        let mut all = String::new();
        for f in files { all.push_str(&fs::read_to_string(&f)?); }
        Ok(all)
    }
}
