use clap::Parser;

/// hl-compiler — .hl → native binary via LLVM + gcc
#[derive(Parser, Debug)]
#[command(
author  = "HackerOS Team",
version = "1.7.5",
about   = "hacker-lang compiler — .hl → native binary via LLVM + gcc"
)]
pub struct Args {
    /// Plik .hl do kompilacji
    pub file: String,

    /// Plik wyjściowy (domyślnie: nazwa pliku bez rozszerzenia)
    #[arg(short, long)]
    pub output: Option<String>,

    /// Szczegółowe wyjście
    #[arg(long, short)]
    pub verbose: bool,

    /// Emituj tylko plik obiektowy .o (bez linkowania)
    #[arg(long)]
    pub emit_obj: bool,

    /// Emituj LLVM IR jako .ll (do debugowania)
    #[arg(long)]
    pub emit_ir: bool,

    /// Poziom optymalizacji: 0=brak 1=mało 2=domyślny 3=agresywny
    #[arg(long, default_value = "2")]
    pub opt: u8,

    /// Wymuś PIE (Position Independent Executable) — domyślnie wyłączone
    #[arg(long)]
    pub pie: bool,
}
