use thiserror::Error;

pub type CompileResult<T> = Result<T, CompileError>;

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("Plik nie istnieje: {0}")]
    InputNotFound(String),
    #[error("Plik musi miec rozszerzenie .hl")]
    InvalidExtension,
    #[error("Nieprawidlowe wejscie: {0}")]
    InvalidInput(String),
    #[error("Blad parsowania: {0}")]
    Parse(String),
    #[error("Blad generowania kodu: {0}")]
    Codegen(String),
    #[error("Blad kompilacji runtime: {0}")]
    Runtime(String),
    #[error("Blad linkowania: {0}")]
    Link(String),
    #[error("Blad I/O: {0}")]
    Io(String),
}
