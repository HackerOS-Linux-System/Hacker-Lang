use clap::Parser as ClapParser;
use miette::{miette, IntoDiagnostic, Result, NamedSource};
use std::fs::{read_to_string, write};
use std::path::PathBuf;
use std::process::Command;
use indextree::Arena;
use logos::Logos;
use chumsky::prelude::*;
use chumsky::Stream;

mod ast_parser;
mod codegen;

use ast_parser::{parser, build_arena, Token};
use codegen::generate_c;

#[derive(ClapParser)]
#[command(name = "hl-advanced", about = "Transpilator HLA -> C")]
struct Cli {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
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

    // 4. Generowanie C
    let c_code = generate_c(&arena, root)?;

    // 5. Zapis wyniku
    let out_path = cli.output.unwrap_or_else(|| cli.input.with_extension("c"));
    write(&out_path, &c_code).into_diagnostic()?;

    println!("Transpilacja zakończona sukcesem: {}", out_path.display());

    // 6. Uruchomienie (opcjonalne)
    if cli.run {
        let bin_path = cli.input.with_extension("out");
        let status = Command::new("gcc")
        .arg(&out_path)
        .arg("-o")
        .arg(&bin_path)
        .status()
        .into_diagnostic()?;

        if status.success() {
            println!("Uruchamianie...\n");
            Command::new(bin_path).status().into_diagnostic()?;
        } else {
            return Err(miette!("Błąd kompilacji C (gcc)"));
        }
    }

    Ok(())
}
