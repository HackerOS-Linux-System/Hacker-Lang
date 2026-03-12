use clap::Parser as ClapParser;
use colored::*;
use std::collections::HashSet;
use std::process::exit;

mod ast;
mod analysis;
mod lib_resolver;
mod parser;

use crate::ast::AnalysisResult;
use crate::analysis::{check_plugins, print_errors, print_summary};
use crate::parser::parse_file;

// ─────────────────────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────────────────────
#[derive(ClapParser, Debug)]
#[command(
name    = "hl-plsa",
author  = "HackerOS Team",
version = env!("CARGO_PKG_VERSION"),
          about   = "hacker-lang static analyser"
)]
struct Args {
    /// Plik .hl do analizy
    file: String,

    /// Szczegółowe wyjście (verbose summary)
    #[arg(long, short)]
    verbose: bool,

    /// Wypisz AST jako JSON
    #[arg(long, short)]
    json: bool,

    /// Rekurencyjnie parsuj biblioteki source/core
    #[arg(long)]
    resolve_libs: bool,

    /// Sprawdź czy pliki pluginów istnieją
    #[arg(long)]
    check_plugins: bool,

    /// Wypisz listę modułów (=;)
    #[arg(long)]
    list_modules: bool,

    /// Wypisz tylko publiczne funkcje
    #[arg(long)]
    list_pub: bool,

    /// Wypisz flagi kompilacji warunkowej (*{})
    #[arg(long)]
    list_cond: bool,
}

// ─────────────────────────────────────────────────────────────
// main
// ─────────────────────────────────────────────────────────────
fn main() {
    let args = Args::parse();
    let mut seen = HashSet::new();

    match parse_file(&args.file, args.resolve_libs, args.verbose, &mut seen) {
        Ok(res) => {
            if args.verbose        { print_summary(&res); }
            if args.check_plugins  { check_plugins(&res); }
            if args.list_modules   { print_modules(&res); }
            if args.list_pub       { print_pub_api(&res); }
            if args.list_cond      { print_cond_flags(&res); }
            if args.json {
                match serde_json::to_string_pretty(&res) {
                    Ok(j)  => println!("{}", j),
                    Err(e) => { eprintln!("{} {e}", "[!]".red()); exit(1); },
                }
            }
            if !args.verbose && !args.json && !args.list_modules
                && !args.list_pub && !args.list_cond && !args.check_plugins {
                    print_quick_summary(&res, &args.file);
                }
        },
        Err(errors) => {
            print_errors(&errors, &args.file);
            exit(1);
        },
    }
}

// ─────────────────────────────────────────────────────────────
// Szybkie podsumowanie (domyślne wyjście)
// ─────────────────────────────────────────────────────────────
fn print_quick_summary(res: &AnalysisResult, file: &str) {
    eprintln!("{} {} {} fn:{} mod:{} lib:{} dep:{}",
              "✓".green().bold(),
              file.dimmed(),
              "|".dimmed(),
              res.functions.len().to_string().yellow(),
              res.modules.len().to_string().cyan(),
              res.libs.len().to_string().magenta(),
              res.deps.len().to_string().blue(),
    );
    if res.is_potentially_unsafe {
        eprintln!("  {} {} linia(e) sudo (^)",
                  "⚠".yellow().bold(),
                  res.safety_warnings.len().to_string().yellow()
        );
    }
}

// ─────────────────────────────────────────────────────────────
// Wypisz moduły
// ─────────────────────────────────────────────────────────────
fn print_modules(res: &AnalysisResult) {
    use crate::ast::Visibility;
    if res.modules.is_empty() {
        eprintln!("{}", "(brak modułów)".dimmed());
        return;
    }
    eprintln!("{}", "━━━ Moduły (=;) ━━━━━━━━━━━━━━━━━━━━━━━━━━".cyan());
    let mut mods: Vec<_> = res.modules.iter().collect();
    mods.sort_by_key(|(n, _)| n.as_str());
    for (name, meta) in &mods {
        let vis_tag = if meta.visibility == Visibility::Public {
            " [pub]".green().to_string()
        } else {
            " [prv]".dimmed().to_string()
        };
        eprintln!("  {}{}", name.cyan().bold(), vis_tag);
        for sub in &meta.submodules {
            eprintln!("    {} sub: {}", "↳".dimmed(), sub.yellow());
        }
        for f in &meta.functions {
            let fmeta = res.functions.get(f);
            let vis_f = fmeta.map(|m| &m.visibility);
            let pub_tag = if vis_f == Some(&Visibility::Public) { " pub".green().to_string() } else { String::new() };
            let sig_tag = fmeta.and_then(|m| m.sig.as_deref())
            .map(|s| format!(" {}", s.dimmed()))
            .unwrap_or_default();
            eprintln!("    {} fn:{}{}{}", "·".dimmed(), f.blue(), pub_tag, sig_tag);
        }
    }
    eprintln!();
}

// ─────────────────────────────────────────────────────────────
// Wypisz publiczne API
// ─────────────────────────────────────────────────────────────
fn print_pub_api(res: &AnalysisResult) {
    use crate::ast::Visibility;
    let pub_fns: Vec<_> = res.functions.iter()
    .filter(|(_, m)| m.visibility == Visibility::Public)
    .collect();
    if pub_fns.is_empty() {
        eprintln!("{}", "(brak publicznych funkcji)".dimmed());
        return;
    }
    eprintln!("{}", "━━━ Publiczne API ━━━━━━━━━━━━━━━━━━━━━━━━".green());
    let mut sorted: Vec<_> = pub_fns;
    sorted.sort_by_key(|(n, _)| n.as_str());
    for (name, meta) in sorted {
        let sig = meta.sig.as_deref().unwrap_or("");
        let module = if meta.module_path.is_empty() {
            String::new()
        } else {
            format!(" [{}]", meta.module_path.join("::").dimmed())
        };
        let attrs: Vec<String> = meta.attrs.iter()
        .map(|a| {
            if let Some(arg) = &a.arg { format!("|] {} \"{}\"", a.name, arg) }
            else { format!("|] {}", a.name) }
        })
        .collect();
        let attr_str = if attrs.is_empty() { String::new() } else {
            format!("  {}", attrs.join(", ").yellow())
        };
        eprintln!("  pub {}{} {}{}", name.cyan().bold(), module, sig.yellow(), attr_str);
    }
    eprintln!();
}

// ─────────────────────────────────────────────────────────────
// Wypisz flagi kompilacji warunkowej
// ─────────────────────────────────────────────────────────────
fn print_cond_flags(res: &AnalysisResult) {
    use crate::ast::CommandType;
    let cond_nodes: Vec<_> = res.main_body.iter()
    .chain(res.functions.values().flat_map(|m| m.body.iter()))
    .filter(|n| matches!(&n.content, CommandType::CondComp { .. } | CommandType::CondCompEnd))
    .collect();
    if cond_nodes.is_empty() {
        eprintln!("{}", "(brak bloków kompilacji warunkowej)".dimmed());
        return;
    }
    eprintln!("{}", "━━━ Kompilacja warunkowa (*{}) ━━━━━━━━━━━━".yellow());
    for node in cond_nodes {
        match &node.content {
            CommandType::CondComp { flag } =>
            eprintln!("  linia {:>4} — *{{{}}}",  node.line_num, flag.yellow()),
            CommandType::CondCompEnd =>
            eprintln!("  linia {:>4} — *{{end}}", node.line_num),
            _ => {}
        }
    }
    eprintln!();
}
