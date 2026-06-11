pub mod interpreter;
pub mod jit_engine;
pub mod runtime;
pub mod runner;

pub use runner::{run_bc_file, run_bc_module, run_hl_file};
pub use interpreter::BytecodeInterpreter;

use anyhow::Result;
use std::path::Path;

/// Uruchom plik — automatycznie wybiera ścieżkę:
///  - .bc → JIT bezpośrednio
///  - .hl → kompiluj do cache → JIT
pub fn run_file(path: &Path, args: &[String]) -> Result<i32> {
    runner::run_hl_file(path, args)
}
