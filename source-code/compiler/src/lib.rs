pub mod codegen;
pub mod runtime;
pub mod linker;
pub mod error;

pub use error::{CompileError, CompileResult};

use std::path::{Path, PathBuf};
use colored::Colorize;
use hl_parser::parse_source;

#[derive(Debug, Clone, PartialEq)]
pub enum CompileMode {
    /// Statyczna binarka ELF x86_64 (domyslna)
    Binary,
    /// Biblioteka .so (ekosystem Virus)
    Shared,
}

pub struct CompileOptions {
    pub input:   PathBuf,
    pub output:  Option<PathBuf>,
    pub verbose: bool,
    pub mode:    CompileMode,
    pub opt:     OptLevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OptLevel {
    None,   // -O0
    Speed,  // -O2 (domyslna)
    Size,   // -Os
}

impl Default for OptLevel {
    fn default() -> Self { OptLevel::Speed }
}

pub struct CompileOutput {
    pub output_path: PathBuf,
    pub mode:        CompileMode,
    pub object_size: u64,
}

/// Glowna funkcja kompilatora Hacker Lang
///
/// Pipeline:
///   1. Parsowanie .hl  (hl-parser)
///   2. Lowering AST -> HlIR (wewnetrzna reprezentacja)
///   3. Codegen Cranelift -> obiekt ELF (.o)
///   4. Kompilacja runtime C -> obiekt (.o)
///   5. Linkowanie -> binarka lub .so
pub fn compile(opts: CompileOptions) -> CompileResult<CompileOutput> {
    let input = &opts.input;

    if !input.exists() {
        return Err(CompileError::InputNotFound(input.display().to_string()));
    }
    if input.extension().and_then(|e| e.to_str()) != Some("hl") {
        return Err(CompileError::InvalidExtension);
    }

    let stem = input.file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CompileError::InvalidInput("zla nazwa pliku".into()))?;

    let output_path = opts.output.clone().unwrap_or_else(|| {
        let ext = match opts.mode { CompileMode::Binary => "", CompileMode::Shared => ".so" };
        input.parent().unwrap_or(Path::new(".")).join(format!("{}{}", stem, ext))
    });

    // ── 1. Parsowanie ────────────────────────────────────────────────────────
    log_step("PARSE", &input.display().to_string(), opts.verbose);
    let source = std::fs::read_to_string(input)
        .map_err(|e| CompileError::Io(e.to_string()))?;
    let ast = parse_source(&source)
        .map_err(|e| CompileError::Parse(e.to_string()))?;

    // ── 2. Lowering AST -> HlIR ──────────────────────────────────────────────
    log_step("LOWER", "AST -> HlIR", opts.verbose);
    let ir = codegen::lower::lower_ast(&ast)
        .map_err(|e| CompileError::Codegen(e.to_string()))?;

    // ── 3. Codegen Cranelift ─────────────────────────────────────────────────
    log_step("CODEGEN", "Cranelift -> ELF object", opts.verbose);
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| CompileError::Io(e.to_string()))?;

    let obj_path = tmp_dir.path().join(format!("{}_hl.o", stem));
    codegen::emit::emit_object(&ir, &obj_path, opts.opt, opts.verbose)
        .map_err(|e| CompileError::Codegen(e.to_string()))?;

    // ── 4. Runtime C -> obiekt ───────────────────────────────────────────────
    log_step("RUNTIME", "C runtime -> object", opts.verbose);
    let rt_obj = tmp_dir.path().join("hl_runtime.o");
    runtime::compile_runtime(&rt_obj, opts.verbose)
        .map_err(|e| CompileError::Runtime(e.to_string()))?;

    // ── 5. Linkowanie ────────────────────────────────────────────────────────
    log_step("LINK", &output_path.display().to_string(), opts.verbose);
    linker::link(&obj_path, &rt_obj, &output_path, &opts.mode, opts.verbose)
        .map_err(|e| CompileError::Link(e.to_string()))?;

    let object_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    log_step("OK", &format!("{} ({} KB)", output_path.display(), object_size / 1024), opts.verbose);

    Ok(CompileOutput { output_path, mode: opts.mode, object_size })
}

fn log_step(step: &str, msg: &str, verbose: bool) {
    if verbose {
        eprintln!("{} {}", format!("[{}]", step).bright_cyan().bold(), msg.bright_white());
    }
}
