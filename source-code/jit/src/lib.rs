pub mod interpreter;
#[cfg(feature = "native-jit")]
pub mod jit_engine;
#[cfg(not(feature = "native-jit"))]
#[path = "jit_stub.rs"]
pub mod jit_engine;
pub mod runtime;
#[cfg(feature = "process-exec")]
pub mod runner;

#[cfg(feature = "process-exec")]
pub use runner::{run_bc_file, run_bc_module, run_hl_file};
pub use interpreter::BytecodeInterpreter;

use anyhow::Result;
use std::path::Path;

/// Uruchom plik — automatycznie wybiera ścieżkę:
///  - .bc → JIT bezpośrednio
///  - .hl → kompiluj do cache → JIT
/// Dostępne tylko z cechą `process-exec` (wymaga dostępu do systemu plików —
/// patrz doc cechy w Cargo.toml). Dla wasm32/playground używaj bezpośrednio
/// `BytecodeInterpreter` na module już skompilowanym z tekstu źródłowego —
/// patrz source-code/playground.
#[cfg(feature = "process-exec")]
pub fn run_file(path: &Path, args: &[String]) -> Result<i32> {
    runner::run_hl_file(path, args)
}
