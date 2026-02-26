use anyhow::{bail, Context, Result};
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

// ─────────────────────────────────────────────────────────────
// Publiczne API
// ─────────────────────────────────────────────────────────────

/// Wynik transpilacji jednego pliku
#[derive(Debug)]
pub struct TranspileResult {
    /// Ścieżka pliku wyjściowego (.rs)
    pub output_path: PathBuf,
    /// Liczba wygenerowanych linii
    pub lines:       usize,
    /// Ostrzeżenia transpilacji
    pub warnings:    Vec<String>,
}

/// Transpiluj cały projekt src/ → out_dir/
pub fn transpile_project(src_dir: &Path, out_dir: &Path) -> Result<Vec<TranspileResult>> {
    fs::create_dir_all(out_dir)
    .with_context(|| format!("Nie można utworzyć katalogu wyjściowego: {}", out_dir.display()))?;

    let mut results = Vec::new();

    for entry in WalkDir::new(src_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "hl"))
        {
            let hl_path = entry.path();

            // Oblicz ścieżkę wyjściową zachowując strukturę katalogów
            let rel = hl_path.strip_prefix(src_dir)?;
            let rs_path = out_dir.join(rel).with_extension("rs");

            if let Some(parent) = rs_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let result = transpile_file(hl_path, &rs_path)?;
            results.push(result);
        }

        // Wygeneruj Cargo.toml dla transpilatora
        generate_rust_cargo(out_dir)?;

        Ok(results)
}

/// Transpiluj jeden plik .hl → .rs
pub fn transpile_file(input: &Path, output: &Path) -> Result<TranspileResult> {
    let source = fs::read_to_string(input)
    .with_context(|| format!("Nie można odczytać: {}", input.display()))?;

    let (rust_code, warnings) = transpile_source(&source, input)?;
    let lines = rust_code.lines().count();

    fs::write(output, &rust_code)
    .with_context(|| format!("Nie można zapisać: {}", output.display()))?;

    Ok(TranspileResult {
        output_path: output.to_path_buf(),
       lines,
       warnings,
    })
}

// ─────────────────────────────────────────────────────────────
// Rdzeń transpilacji
// ─────────────────────────────────────────────────────────────

/// Tokenizer — prosta linia-po-linii dla hacker-lang
#[derive(Debug, Clone)]
enum HlStatement {
    /// Komentarz
    Comment(String),
    /// Czysta komenda shell
    Shell(String),
    /// Przypisanie zmiennej lokalnej: key = val
    LocalVar { key: String, val: String },
    /// Przypisanie zmiennej środowiskowej: @key = val
    EnvVar { key: String, val: String },
    /// If: ?warunek → ciało
    If { cond: String, body: Vec<HlStatement> },
    /// Elif
    Elif { cond: String, body: Vec<HlStatement> },
    /// Else
    Else { body: Vec<HlStatement> },
    /// While
    While { cond: String, body: Vec<HlStatement> },
    /// For
    For { var: String, iter: String, body: Vec<HlStatement> },
    /// Loop N razy
    Loop { count: String, body: Vec<HlStatement> },
    /// Definicja funkcji
    FnDef { name: String, body: Vec<HlStatement> },
    /// Wywołanie funkcji
    FnCall(String),
    /// Log (echo)
    Log(String),
    /// Try/catch
    Try { try_body: Vec<HlStatement>, catch_body: Vec<HlStatement> },
    /// End z kodem
    End(i32),
    /// Importy (#<type/name>)
    Import { lib_type: String, name: String },
    /// Puste
    Empty,
}

fn transpile_source(source: &str, path: &Path) -> Result<(String, Vec<String>)> {
    let mut warnings = Vec::new();
    let stmts = parse_hl(source, &mut warnings)?;
    let rust_code = emit_rust(&stmts, path, &mut warnings)?;
    Ok((rust_code, warnings))
}

/// Parser .hl — przetwarza linia po linii
fn parse_hl(source: &str, warnings: &mut Vec<String>) -> Result<Vec<HlStatement>> {
    let lines: Vec<&str> = source.lines().collect();
    let mut idx = 0;
    let mut stmts = Vec::new();

    while idx < lines.len() {
        let line = lines[idx].trim();
        let stmt = parse_line(line, &lines, &mut idx, warnings)?;
        stmts.push(stmt);
        idx += 1;
    }

    Ok(stmts)
}

fn parse_line(
    line:     &str,
    lines:    &[&str],
    idx:      &mut usize,
    warnings: &mut Vec<String>,
) -> Result<HlStatement> {
    // Puste linie
    if line.is_empty() {
        return Ok(HlStatement::Empty);
    }

    // Komentarze (// lub !!)
    if line.starts_with("//") || line.starts_with("!!") {
        return Ok(HlStatement::Comment(line[2..].trim().to_string()));
    }

    // Importy: #<bytes/obsidian>
    if line.starts_with('#') {
        return parse_import(line);
    }

    // Definicja funkcji: ;;NazwaKlasy / :nazwa_funkcji
    if line.starts_with(";;") || line.starts_with(':') {
        return Ok(HlStatement::Comment(format!("fn_marker: {}", line)));
    }

    // Log: @log "wiadomość"
    if let Some(rest) = line.strip_prefix("@log") {
        return Ok(HlStatement::Log(rest.trim().to_string()));
    }
    if let Some(rest) = line.strip_prefix("@echo") {
        return Ok(HlStatement::Log(rest.trim().to_string()));
    }

    // Zmienna środowiskowa: @key = val
    if line.starts_with('@') && line.contains('=') && !line.starts_with("@log") {
        let rest = &line[1..];
        if let Some((k, v)) = rest.split_once('=') {
            return Ok(HlStatement::EnvVar {
                key: k.trim().to_string(),
                      val: v.trim().to_string(),
            });
        }
    }

    // Zmienna lokalna: $key = val lub key = val (bez @)
    if line.starts_with('$') && line.contains('=') {
        let rest = &line[1..];
        if let Some((k, v)) = rest.split_once('=') {
            return Ok(HlStatement::LocalVar {
                key: k.trim().to_string(),
                      val: v.trim().to_string(),
            });
        }
    }

    // Wywołanie funkcji: .NazwaKlasy.metoda lub .metoda
    if line.starts_with('.') && !line.contains(' ') {
        return Ok(HlStatement::FnCall(line[1..].replace('.', "_")));
    }

    // End: ~~N lub ~~ (koniec z kodem)
    if line.starts_with("~~") {
        let code: i32 = line[2..].trim().parse().unwrap_or(0);
        return Ok(HlStatement::End(code));
    }

    // Czysta komenda shell (fallback)
    if !line.is_empty() {
        // Ostrzeż o komendach sudo
        if line.starts_with('^') {
            warnings.push(format!("Komenda sudo: {}", &line[1..]));
            return Ok(HlStatement::Shell(format!("sudo {}", &line[1..])));
        }
        return Ok(HlStatement::Shell(line.to_string()));
    }

    Ok(HlStatement::Empty)
}

fn parse_import(line: &str) -> Result<HlStatement> {
    // Format: #<bytes/obsidian> lub #<core/utils>
    let inner = line.trim_start_matches('#')
    .trim_start_matches('<')
    .trim_end_matches('>');
    if let Some((lib_type, name)) = inner.split_once('/') {
        Ok(HlStatement::Import {
            lib_type: lib_type.to_string(),
           name:     name.to_string(),
        })
    } else {
        Ok(HlStatement::Comment(format!("unknown_import: {}", line)))
    }
}

// ─────────────────────────────────────────────────────────────
// Emiter Rust
// ─────────────────────────────────────────────────────────────

fn emit_rust(
    stmts:    &[HlStatement],
    src_path: &Path,
    warnings: &mut Vec<String>,
) -> Result<String> {
    let mut out = String::new();

    // Nagłówek
    writeln!(out, "// Wygenerowano automatycznie przez hl-transpiler")?;
    writeln!(out, "// Źródło: {}", src_path.display())?;
    writeln!(out, "// NIE EDYTUJ RĘCZNIE — edytuj plik .hl")?;
    writeln!(out)?;
    writeln!(out, "#![allow(unused_variables, dead_code, non_snake_case)]")?;
    writeln!(out)?;
    writeln!(out, "use std::process::{{Command, exit}};")?;
    writeln!(out, "use std::env;")?;
    writeln!(out)?;

    // Sprawdź czy jest main_body (main.hl)
    let is_main = src_path.file_name()
    .map_or(false, |n| n.to_str().map_or(false, |s| s == "main.hl"));

    // Zbierz importy
    let imports: Vec<_> = stmts.iter().filter_map(|s| {
        if let HlStatement::Import { lib_type, name } = s {
            Some((lib_type.as_str(), name.as_str()))
        } else { None }
    }).collect();

    // Komentarze importów
    if !imports.is_empty() {
        writeln!(out, "// ── Biblioteki hl ─────────────────────────────────────")?;
        for (lt, name) in &imports {
            writeln!(out, "// #<{}/{}> — linked via hl-compiler", lt, name)?;
        }
        writeln!(out)?;
    }

    // Funkcje pomocnicze
    writeln!(out, "/// Wykonaj komendę shell — odpowiednik system() z C")?;
    writeln!(out, "fn hl_exec(cmd: &str) -> i32 {{")?;
    writeln!(out, "    let status = Command::new(\"sh\")")?;
    writeln!(out, "        .args([\"-c\", cmd])")?;
    writeln!(out, "        .status()")?;
    writeln!(out, "        .expect(\"Nie można uruchomić powłoki\");")?;
    writeln!(out, "    status.code().unwrap_or(1)")?;
    writeln!(out, "}}")?;
    writeln!(out)?;
    writeln!(out, "/// Wykonaj komendę sudo")?;
    writeln!(out, "fn hl_exec_sudo(cmd: &str) -> i32 {{")?;
    writeln!(out, "    hl_exec(&format!(\"sudo sh -c '{{}}'\", cmd))")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    // Emituj ciało
    emit_stmts(&mut out, stmts, if is_main { "fn main()" } else { "" }, 0, warnings)?;

    Ok(out)
}

fn emit_stmts(
    out:      &mut String,
    stmts:    &[HlStatement],
    fn_wrap:  &str,
    indent:   usize,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let ind = "    ".repeat(indent);

    // Jeśli opakowujemy w funkcję main
    let in_fn = !fn_wrap.is_empty();
    if in_fn {
        writeln!(out, "{}{} {{", ind, fn_wrap)?;
    }

    let inner = if in_fn { indent + 1 } else { indent };
    let ii    = "    ".repeat(inner);

    for stmt in stmts {
        emit_stmt(out, stmt, inner, warnings)?;
    }

    // Zamknij funkcję
    if in_fn {
        writeln!(out, "{}}}", ind)?;
        writeln!(out)?;
    }

    Ok(())
}

fn emit_stmt(
    out:      &mut String,
    stmt:     &HlStatement,
    indent:   usize,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let ii = "    ".repeat(indent);

    match stmt {
        HlStatement::Empty => {}

        HlStatement::Comment(c) => {
            writeln!(out, "{}// {}", ii, c)?;
        }

        HlStatement::Shell(cmd) => {
            // Escapuj cudzysłowy wewnątrz stringa Rust
            let escaped = cmd.replace('\\', "\\\\").replace('"', "\\\"");
            writeln!(out, "{}hl_exec(\"{}\");", ii, escaped)?;
        }

        HlStatement::LocalVar { key, val } => {
            // Próbuj wykryć typ
            if val == "true" || val == "false" {
                writeln!(out, "{}let {}: bool = {};", ii, sanitize_ident(key), val)?;
            } else if let Ok(n) = val.parse::<i64>() {
                writeln!(out, "{}let {}: i64 = {};", ii, sanitize_ident(key), n)?;
            } else if let Ok(f) = val.parse::<f64>() {
                writeln!(out, "{}let {}: f64 = {};", ii, sanitize_ident(key), f)?;
            } else {
                let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
                writeln!(out, "{}let {} = \"{}\".to_string();", ii, sanitize_ident(key), escaped)?;
            }
        }

        HlStatement::EnvVar { key, val } => {
            let escaped = val.replace('\\', "\\\\").replace('"', "\\\"");
            writeln!(out, "{}env::set_var(\"{}\", \"{}\");", ii, key, escaped)?;
        }

        HlStatement::Log(msg) => {
            // Usuń zewnętrzne cudzysłowy jeśli są
            let clean = msg.trim_matches('"');
            writeln!(out, "{}println!(\"{}\");", ii, clean)?;
        }

        HlStatement::FnCall(name) => {
            writeln!(out, "{}{}();", ii, sanitize_ident(name))?;
        }

        HlStatement::End(code) => {
            writeln!(out, "{}exit({});", ii, code)?;
        }

        HlStatement::Import { .. } => {
            // Obsługiwane w nagłówku
        }

        HlStatement::If { cond, body } => {
            let rs_cond = shell_cond_to_rust(cond, warnings);
            writeln!(out, "{}if {} {{", ii, rs_cond)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::Elif { cond, body } => {
            let rs_cond = shell_cond_to_rust(cond, warnings);
            writeln!(out, "{}else if {} {{", ii, rs_cond)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::Else { body } => {
            writeln!(out, "{}else {{", ii)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::While { cond, body } => {
            let rs_cond = shell_cond_to_rust(cond, warnings);
            writeln!(out, "{}while {} {{", ii, rs_cond)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::For { var, iter, body } => {
            let rs_iter = shell_iter_to_rust(iter);
            writeln!(out, "{}for {} in {} {{", ii, sanitize_ident(var), rs_iter)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::Loop { count, body } => {
            writeln!(out, "{}for _hl_i in 0..{} {{", ii, count)?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
        }

        HlStatement::FnDef { name, body } => {
            writeln!(out, "{}fn {}() {{", ii, sanitize_ident(name))?;
            for s in body {
                emit_stmt(out, s, indent + 1, warnings)?;
            }
            writeln!(out, "{}}}", ii)?;
            writeln!(out)?;
        }

        HlStatement::Try { try_body, catch_body } => {
            // Rust nie ma try/catch jak shell — emitujemy blok z checkstatus
            writeln!(out, "{}{{  // try", ii)?;
            writeln!(out, "{}    let _try_ok = (|| -> bool {{", ii)?;
            for s in try_body {
                emit_stmt(out, s, indent + 2, warnings)?;
            }
            writeln!(out, "{}        true", ii)?;
            writeln!(out, "{}    }})();", ii)?;
            writeln!(out, "{}    if !_try_ok {{  // catch", ii)?;
            for s in catch_body {
                emit_stmt(out, s, indent + 2, warnings)?;
            }
            writeln!(out, "{}    }}", ii)?;
            writeln!(out, "{}}}", ii)?;
        }
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Pomocnicze konwertery
// ─────────────────────────────────────────────────────────────

/// Konwertuj shell condition do wyrażenia Rust (uproszczone)
fn shell_cond_to_rust(cond: &str, warnings: &mut Vec<String>) -> String {
    let c = cond.trim();

    // -f plik → Path::new("plik").is_file()
    if let Some(rest) = c.strip_prefix("[ -f ") {
        let f = rest.trim_end_matches(']').trim().trim_matches('"');
        return format!("std::path::Path::new(\"{}\").is_file()", f);
    }
    // -d katalog → Path::new("katalog").is_dir()
    if let Some(rest) = c.strip_prefix("[ -d ") {
        let f = rest.trim_end_matches(']').trim().trim_matches('"');
        return format!("std::path::Path::new(\"{}\").is_dir()", f);
    }
    // -z "$var" → var.is_empty()
    if let Some(rest) = c.strip_prefix("[ -z ") {
        let v = rest.trim_end_matches(']').trim().trim_matches('"').trim_start_matches('$');
        return format!("{}.is_empty()", sanitize_ident(v));
    }
    // [ "$a" = "$b" ] → a == b
    if c.starts_with('[') && c.ends_with(']') && c.contains(" = ") {
        let inner = c.trim_start_matches('[').trim_end_matches(']').trim();
        if let Some((l, r)) = inner.split_once(" = ") {
            let l = l.trim().trim_matches('"').trim_start_matches('$');
            let r = r.trim().trim_matches('"').trim_start_matches('$');
            return format!("{} == {}", sanitize_ident(l), sanitize_ident(r));
        }
    }

    // Fallback: emituj jako exec i sprawdź wynik
    warnings.push(format!(
        "Złożony warunek shell '{}' — transpilowany do exec check",
        cond
    ));
    format!("hl_exec(\"{}\") == 0", c.replace('"', "\\\""))
}

/// Konwertuj shell iter do Rust iter
fn shell_iter_to_rust(iter: &str) -> String {
    let i = iter.trim();
    // $(seq 1 N) lub 1..N
    if let Some(inner) = i.strip_prefix("$(seq ").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.len() == 2 {
            return format!("{}..={}", parts[0], parts[1]);
        }
    }
    // Tablica literalna: "a b c"
    let items: Vec<&str> = i.split_whitespace().collect();
    if items.len() > 1 {
        let quoted: Vec<String> = items.iter()
        .map(|s| format!("\"{}\"", s.trim_matches('"')))
        .collect();
        return format!("[{}]", quoted.join(", "));
    }
    // Fallback
    format!("/* shell_iter: {} */ 0..1", i)
}

/// Sanityzuj identyfikator Rust
fn sanitize_ident(s: &str) -> String {
    let clean: String = s.chars()
    .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
    .collect();

    // Rust keywords
    match clean.as_str() {
        "type" | "move" | "ref" | "use" | "mod" | "fn" | "let" | "mut"
        | "if" | "else" | "while" | "for" | "loop" | "return" | "match"
        | "impl" | "struct" | "enum" | "trait" | "pub" | "in" | "as" => {
            format!("hl_{}", clean)
        }
        _ => clean,
    }
}

/// Generuj Cargo.toml dla wygenerowanego projektu Rust
fn generate_rust_cargo(out_dir: &Path) -> Result<()> {
    let cargo_path = out_dir.parent()
    .unwrap_or(out_dir)
    .join("Cargo-transpiled.toml");

    let content = r#"[package]
    name    = "hl-transpiled"
    version = "0.1.0"
    edition = "2021"

    [[bin]]
    name = "main"
    path = "src/main.rs"

    [dependencies]
    # Dodaj tu zależności Rust odpowiadające bibliotekom .hl
    "#;

    fs::write(cargo_path, content)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Testy
// ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transpile_echo() {
        let src = "@log \"Hej świecie\"";
        let (code, _) = transpile_source(src, Path::new("test.hl")).unwrap();
        assert!(code.contains("println!"));
    }

    #[test]
    fn test_transpile_local_var() {
        let src = "$x = 42";
        let (code, _) = transpile_source(src, Path::new("test.hl")).unwrap();
        assert!(code.contains("let x: i64 = 42"));
    }

    #[test]
    fn test_transpile_shell() {
        let src = "apt update";
        let (code, _) = transpile_source(src, Path::new("test.hl")).unwrap();
        assert!(code.contains("hl_exec(\"apt update\")"));
    }
}
