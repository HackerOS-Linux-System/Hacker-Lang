pub mod ast;
pub mod gen;
pub mod lexer;
pub mod parser;
pub mod shebang;
pub mod import_spec;

pub use ast::*;
pub use gen::{Gen, GenError, GenFeature, extract_gen, parse_gen_declaration, HL_MAX_GEN, HL_DEFAULT_GEN};
pub use shebang::{ShebangInfo, PreprocessResult, preprocess};
pub use lexer::{Lexer, Token, LexError};
pub use parser::{Parser, ParseError, parse_source, parse_source_with_meta};
pub use import_spec::{parse_import_line, ImportDecl};

/// Wynik parsowania z pelna meta-informacja
#[derive(Debug)]
pub struct ParseMeta {
    pub nodes:   Vec<Node>,
    pub gen:     Gen,
    pub shebang: Option<ShebangInfo>,
}
