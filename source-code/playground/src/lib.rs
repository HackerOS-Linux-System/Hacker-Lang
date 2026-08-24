use wasm_bindgen::prelude::*;
use std::path::Path;

/// Wynik uruchomienia skryptu, zwracany do JS. Pola dostępne jako gettery
/// (`result.output`, `result.error`, `result.exitCode`, `result.ok` po
/// stronie JS — wasm-bindgen tłumaczy `exit_code` na `exitCode`
/// automatycznie wg konwencji camelCase).
#[wasm_bindgen]
pub struct RunResult {
    output:    String,
    error:     String,
    exit_code: i32,
}

#[wasm_bindgen]
impl RunResult {
    #[wasm_bindgen(getter)]
    pub fn output(&self) -> String {
        self.output.clone()
    }

    /// Puste, jeśli `ok` — komunikat błędu parsowania/wykonania w
    /// przeciwnym razie. `output` może być NIEPUSTE nawet gdy `error` też
    /// nie jest — skrypt mógł zdążyć coś wypisać, zanim napotkał błąd.
    #[wasm_bindgen(getter)]
    pub fn error(&self) -> String {
        self.error.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    #[wasm_bindgen(getter)]
    pub fn ok(&self) -> bool {
        self.error.is_empty()
    }
}

/// Uruchom kod źródłowy Hacker Lang i zwróć zebrane wyjście.
///
/// Przechodzi DOKŁADNIE tę samą ścieżkę co natywne `hl run skrypt.hl`:
/// parsowanie (`hl_parser::parse_source_with_meta`) → lowering do bytecode
/// (`hl_compiler::lower_ast`) → optymalizacja (`hl_compiler::optimize_module`)
/// → interpretacja (`hl_jit::BytecodeInterpreter`).
///
/// Wyjście (`Instruction::Print`, czyli `~>`/`print`) jest zbierane do
/// `state.output` — patrz doc tego pola w `source-code/jit/src/runtime.rs`:
/// to bufor DODATKOWY do zwykłego `println!`, który tu i tak nie ma dokąd
/// trafić (brak prawdziwego stdout w przeglądarce bez powłoki WASI).
#[wasm_bindgen]
pub fn run_hl(source: &str) -> RunResult {
    // Ścieżka syntetyczna: hl_compiler::lower_ast wymaga &Path (używanej
    // tylko do komunikatów błędów/metadanych), ale playground nie ma
    // prawdziwego pliku źródłowego — nazwa jest czysto kosmetyczna.
    let synthetic_path = Path::new("playground.hl");

    let meta = match hl_parser::parse_source_with_meta(source) {
        Ok(m) => m,
        Err(e) => {
            return RunResult {
                output: String::new(),
                error: format!("błąd parsowania: {}", e),
                exit_code: 1,
            }
        }
    };

    let mut module = hl_compiler::lower_ast(&meta.nodes, synthetic_path, meta.gen.number());
    hl_compiler::optimize_module(&mut module);

    let mut interp = hl_jit::BytecodeInterpreter::new(&module);
    match interp.run() {
        Ok(code) => RunResult {
            output: interp.state.output.clone(),
            error: String::new(),
            exit_code: code,
        },
        Err(e) => RunResult {
            output: interp.state.output.clone(),
            error: format!("błąd wykonania: {}", e),
            exit_code: 1,
        },
    }
}

/// Wersja hl-playground (z Cargo.toml) — przydatne np. w stopce strony.
#[wasm_bindgen]
pub fn playground_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
