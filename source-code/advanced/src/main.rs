use clap::Parser as ClapParser;
use miette::{miette, IntoDiagnostic, Result, NamedSource};
use std::fs::{read_to_string, write, remove_file};
use std::path::PathBuf;
use std::process::Command;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use indextree::Arena;
use logos::Logos;
use chumsky::prelude::*;
use chumsky::Stream;

mod ast_parser;
mod codegen;
mod semantics;
mod optimizer;

use ast_parser::{parser, build_arena, Token};
use codegen::generate_c;
use semantics::analyze_semantics;
use optimizer::optimize_ast;

#[derive(ClapParser)]
#[command(name = "hl-advanced", about = "Kompilator HLA -> Native Binary")]
struct Cli {
    input: PathBuf,
    /// Ścieżka do pliku wyjściowego (binarki)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Uruchom program po kompilacji
    #[arg(short, long)]
    run: bool,
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("Błąd parsowania")]
struct ParseError {
    #[source_code]
    src: NamedSource<String>,
    #[label("Tutaj")]
    span: (usize, usize),
    token: String,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let source_code = read_to_string(&cli.input)
    .into_diagnostic()
    .map_err(|e| miette!("Nie można otworzyć pliku: {}", e))?;

    // 1. Lexing
    let lexer = Token::lexer(&source_code);
    let tokens: Vec<_> = lexer.spanned().map(|(tok, span)| (tok.unwrap_or(Token::Error), span)).collect();

    // 2. Parsing
    let len = tokens.len();
    let stream = Stream::from_iter(len..len + 1, tokens.into_iter());

    let pre_ast = parser()
    .parse(stream)
    .map_err(|e| {
        let err = e.first().unwrap();
        ParseError {
            src: NamedSource::new(cli.input.to_string_lossy(), source_code.clone()),
             span: (err.span().start, err.span().len()),
             token: format!("{:?}", err.found()),
        }
    })?;

    // 3. Budowa drzewa AST
    let mut arena = Arena::new();
    let root = build_arena(pre_ast, &mut arena);

    // 4. Analiza Semantyczna
    println!("Analiza semantyczna...");
    let type_map = analyze_semantics(&arena, root)?;

    // 5. Optymalizacja AST
    println!("Optymalizacja AST...");
    optimize_ast(&mut arena, root);

    // 6. Generowanie C
    println!("Generowanie kodu C...");
    let c_code = generate_c(&arena, root, &type_map)?;

    // 7. Obsługa pliku tymczasowego
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let temp_c_path = env::temp_dir().join(format!("hla_build_{}.c", timestamp));

    // Zapisz kod C do katalogu tymczasowego (niewidoczny dla użytkownika w katalogu roboczym)
    write(&temp_c_path, &c_code).into_diagnostic()?;

    // Określenie ścieżki wyjściowej dla binarki
    let output_bin_path = cli.output.unwrap_or_else(|| {
        let mut p = cli.input.clone();
        p.set_extension(""); // Usuń rozszerzenie .hl, np. script.hl -> script (lub script.exe na Win)

    // Na Windows warto dodać .exe jeśli nie ma
    #[cfg(target_os = "windows")]
    if p.extension().is_none() {
        p.set_extension("exe");
    }
    p
    });

    println!("Kompilacja natywna (GCC) -> {}", output_bin_path.display());

    // 8. Kompilacja GCC
    let gcc_status = Command::new("gcc")
    .arg(&temp_c_path)      // Wejście: plik tymczasowy
    .arg("-o")
    .arg(&output_bin_path)  // Wyjście: binarka
    .arg("-Wall")           // Ostrzeżenia
    .arg("-O2")             // Optymalizacja poziomu 2
    .status()
    .into_diagnostic();

    // 9. Sprzątanie (Usuń plik tymczasowy C)
    if let Err(e) = remove_file(&temp_c_path) {
        // Logujemy tylko jako warning, nie przerywamy programu
        eprintln!("Warning: Nie udało się usunąć pliku tymczasowego {:?}: {}", temp_c_path, e);
    }

    // Sprawdzenie wyniku GCC po posprzątaniu
    let status = gcc_status?;
    if !status.success() {
        return Err(miette!("Błąd kompilacji GCC. Upewnij się, że masz zainstalowane gcc."));
    }

    println!("Sukces! Utworzono plik wykonywalny.");

    // 10. Uruchomienie (opcjonalne)
    if cli.run {
        println!("\n=== URUCHAMIANIE ===\n");
        // Konwersja ścieżki na absolutną lub dodanie ./ dla obecnego katalogu (dla systemów Unix)
        let exec_path = if output_bin_path.is_relative() && !output_bin_path.starts_with(".") {
            std::path::Path::new(".").join(&output_bin_path)
        } else {
            output_bin_path
        };

        let run_status = Command::new(&exec_path)
        .status()
        .into_diagnostic()?;

        if !run_status.success() {
            println!("\nProgram zakończył się błędem (kod: {:?}).", run_status.code());
        }
    }

    Ok(())
}
