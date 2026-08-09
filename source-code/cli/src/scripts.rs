use crate::guard::HL_SCRIPTS_DIR;
use crate::run::{inject_args, run_file_with_diag};
use colored::Colorize;
use hl_core::env::Env;
use std::path::{Path, PathBuf};

pub fn cmd_exec(name: &str, args: &[String], verbose: bool) -> i32 {
    let scripts_dir = Path::new(HL_SCRIPTS_DIR);

    let candidates = [
        scripts_dir.join(format!("{}.hl", name)),
        scripts_dir.join(name),
    ];

    let script_path = candidates.iter().find(|p| p.exists());

    match script_path {
        Some(path) => {
            if verbose {
                eprintln!("{} {}", "hl exec:".bright_magenta().bold(),
                          path.display().to_string().bright_white());
            }
            let mut env = Env::new();
            inject_args(&mut env, args);
            env.set_var("HL_EXEC_NAME", hl_core::Value::String(name.to_string()));
            run_file_with_diag(path, &mut env, verbose)
        }
        None => {
            eprintln!("{} Skrypt '{}' nie znaleziony w {}",
                      "BŁĄD".red().bold(), name.bright_white(), HL_SCRIPTS_DIR.bright_black());
            eprintln!("  Użyj {} aby zobaczyć dostępne skrypty.", "hl search all".bright_cyan());
            1
        }
    }
}

pub fn cmd_search(query: &str) {
    let scripts_dir = Path::new(HL_SCRIPTS_DIR);

    if !scripts_dir.exists() {
        eprintln!("{} Katalog skryptów nie istnieje: {}", "BŁĄD".red().bold(), HL_SCRIPTS_DIR.bright_black());
        return;
    }

    let mut scripts: Vec<(String, PathBuf)> = match std::fs::read_dir(scripts_dir) {
        Ok(entries) => entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) == Some("hl") {
                let name = path.file_stem()?.to_str()?.to_string();
                Some((name, path))
            } else {
                None
            }
        })
        .collect(),
        Err(e) => {
            eprintln!("{} Nie można odczytać katalogu: {}", "BŁĄD".red().bold(), e);
            return;
        }
    };

    scripts.sort_by(|a, b| a.0.cmp(&b.0));

    let show_all = query.eq_ignore_ascii_case("all");
    let query_lc = query.to_lowercase();

    let matched: Vec<&(String, PathBuf)> = if show_all {
        scripts.iter().collect()
    } else {
        scripts.iter().filter(|(name, _)| name.to_lowercase().contains(&query_lc)).collect()
    };

    if matched.is_empty() {
        println!("{} Brak skryptów pasujących do '{}'",
                 "hl search:".bright_magenta().bold(), query.bright_yellow());
        return;
    }

    println!("{} {} — {}",
             "hl search:".bright_magenta().bold(),
             HL_SCRIPTS_DIR.bright_black(),
             if show_all {
                 format!("{} skryptów", matched.len()).bright_white().to_string()
             } else {
                 format!("{} wyników dla '{}'", matched.len(), query).bright_white().to_string()
             });
    println!();

    for (name, path) in &matched {
        let description = read_script_description(path);
        let exec_hint = format!("hl exec {}", name).bright_cyan().to_string();
        println!("  {} {}", format!("{:<35}", name).bright_white().bold(), exec_hint.bright_black());
        if let Some(desc) = description {
            println!("  {}  {}", " ".repeat(35), desc.bright_black().italic());
        }
        println!();
    }
}

fn read_script_description(path: &Path) -> Option<String> {
    let source = std::fs::read_to_string(path).ok()?;
    for line in source.lines().take(8) {
        let t = line.trim();
        if t.starts_with("///") {
            let desc = t.trim_start_matches('/').trim().to_string();
            if !desc.is_empty() { return Some(desc); }
        }
        if t.starts_with(";;") {
            let desc = t.trim_start_matches(';').trim().to_string();
            if !desc.is_empty() && !desc.starts_with('=') { return Some(desc); }
        }
        if !t.is_empty() && !t.starts_with('#') && !t.starts_with(';') && !t.starts_with("using") { break; }
    }
    None
}
