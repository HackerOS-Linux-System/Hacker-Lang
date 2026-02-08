use miette::{Diagnostic, GraphicalReportHandler, Report, SourceSpan};
use thiserror::Error;
use std::fs::File;
use std::io::{self, Read};
use serde::Deserialize;

#[derive(Deserialize)]
struct ErrorJson {
    line: usize,
    column: usize,
    message: String,
    context: String,
}

#[derive(Error, Diagnostic, Debug)]
enum HlaError {
    #[error("Syntax error at line {line}: {message}")]
    #[diagnostic(code(hla::syntax_error))]
    SyntaxError {
        line: usize,
        message: String,
        #[source_code]
        src: String,
        #[label("here")]
        span: SourceSpan,
        #[help]
        suggestion: String,
    },

    #[error("Type mismatch: expected {expected}, found {found}")]
    #[diagnostic(code(hla::type_mismatch))]
    TypeMismatch {
        expected: String,
        found: String,
        #[label("here")]
        span: SourceSpan,
    },
}

fn main() -> io::Result<()> {
    if std::env::args().len() < 2 {
        println!("Usage: hla-errors <error.json>");
        std::process::exit(1);
    }

    let error_file = std::env::args().nth(1).unwrap();
    let mut file = File::open(error_file)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;

    let error_data: ErrorJson = serde_json::from_str(&contents).unwrap();

    let err = HlaError::SyntaxError {
        line: error_data.line,
        message: error_data.message,
        src: error_data.context,
        span: (error_data.column - 1, 1).into(),
        suggestion: "Check your syntax and try again.".to_string(),
    };

    let handler = GraphicalReportHandler::new();
    let report = Report::new(err);
    let mut out = String::new();
    handler.render_report(&mut out, report.as_ref()).unwrap();
    println!("{}", out);

    Ok(())
}
