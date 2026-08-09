use colored::Colorize;
use hl_core::diagnostics::{parse_error_to_diag, lint_source, lint_gen, DiagRenderer, DiagSummary};
use hl_core::env::Env;
use hl_core::{check_source, run_source, parse_source_with_meta};
use std::path::Path;

/// Uruchom plik .bc bezpośrednio przez JIT (bez kompilacji)
pub fn run_bc_direct(file: &Path, args: &[String]) -> i32 {
    match hl_jit::run_bc_file(file, args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{} {}", "BŁĄD .bc:".red().bold(), e);
            1
        }
    }
}

/// Uruchom plik przez JIT pipeline (eksperymentalny)
pub fn run_file_jit(file: &Path, args: &[String], _verbose: bool) -> i32 {
    // Parytet lintera z domyślną ścieżką (hl_shell::run_file /
    // run_source_with_diag): wcześniej `hl run --jit` w ogóle nie wołał
    // lint_source/lint_gen, więc skrypty z zabronionymi konstrukcjami (np.
    // `> echo ... |> @var` — patrz lint_source w hl-core/diagnostics.rs)
    // były cicho kompilowane do bytecode i URUCHAMIANE, mimo że domyślne
    // `hl run` (bez --jit) poprawnie je odrzuca z czytelnym błędem. Robimy
    // dokładnie to samo sprawdzenie, w tym samym miejscu w przepływie
    // (przed jakąkolwiek kompilacją/wykonaniem), z tym samym formatem
    // komunikatu co reszta CLI.
    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} {}: {}", "BŁĄD".red().bold(), file.display(), e);
            return 1;
        }
    };
    let fname = file.file_name().and_then(|n| n.to_str()).unwrap_or("<unknown>");
    let renderer = DiagRenderer::new(fname, &source);
    let mut lint_diags = lint_source(&source);
    lint_diags.extend(lint_gen(&source));
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        sum.print();
        if sum.has_errors() {
            return 2;
        }
    }

    match hl_jit::run_file(file, args) {
        Ok(code) => code,
        Err(e) => {
            // JIT zawiódł — fallback do tree-walk
            tracing::warn!("JIT error: {}, fallback do interpretera", e);
            let mut env = Env::new();
            inject_args(&mut env, args);
            run_file_with_diag(file, &mut env, false)
        }
    }
}

pub fn run_file_with_diag(file: &Path, env: &mut Env, verbose: bool) -> i32 {
    if !file.exists() {
        eprintln!("{} Plik nie istnieje: {}", "BŁĄD".red().bold(), file.display());
        return 1;
    }

    if verbose {
        if let Ok(source) = std::fs::read_to_string(file) {
            if let Ok(meta) = parse_source_with_meta(&source) {
                eprintln!("  Gen: {}  Shebang: {}",
                          format!("gen {}", meta.gen.number()).bright_magenta(),
                              meta.shebang.map(|s| s.raw).unwrap_or_else(|| "(brak)".into()).bright_black());
            }
        }
    }

    env.set_var("HL_SCRIPT", hl_core::Value::String(file.display().to_string()));

    match hl_shell::run_file(file, env) {
        Ok(code) => code,
        Err(e)   => { eprintln!("{} {}", "BŁĄD".red().bold(), e); 1 }
    }
}

pub fn run_source_with_diag(fname: &str, source: &str, env: &mut Env) -> i32 {
    let renderer = DiagRenderer::new(fname, source);
    let mut lint_diags = lint_source(source);
    lint_diags.extend(lint_gen(source));
    if !lint_diags.is_empty() {
        renderer.emit_all(&lint_diags);
        let sum = DiagSummary::from_diags(&lint_diags);
        sum.print();
        if sum.has_errors() { return 2; }
    }
    if let Err(e) = check_source(source) { renderer.emit(&parse_error_to_diag(&e)); return 2; }
    match run_source(source, env) {
        Ok(r)  => r.exit_code,
        Err(e) => { renderer.emit(&hl_core::Diag::error(e.to_string())); 1 }
    }
}

pub fn inject_args(env: &mut Env, args: &[String]) {
    env.set_var("argc", hl_core::Value::Number(args.len() as f64));
    for (i, arg) in args.iter().enumerate() {
        env.set_var(&format!("arg{}", i), hl_core::Value::String(arg.clone()));
    }
}

pub fn inject_args_env(env: &mut Env, args: &[String]) {
    inject_args(env, args);
}
