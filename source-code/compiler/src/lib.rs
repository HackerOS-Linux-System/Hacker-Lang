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
    /// Statyczna binarka ELF x86_64 (z .bc)
    Binary,
    /// Biblioteka .so (ekosystem bit)
    Shared,
    /// Bytecode HL (.bc) — etap posredni .hl -> .bc
    Bytecode,
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
/// Pipeline dla .hl -> .bc:
///   1. Parsowanie .hl  (hl-parser)
///   2. Serializacja AST -> JSON bytecode (.bc)
///   3. Dodaj shebang #!/usr/bin/env -S /usr/bin/hl run
///
/// Pipeline dla .bc -> ELF/.so:
///   1. Wczytaj .bc (JSON AST)
///   2. Lowering AST -> HlIR
///   3. Codegen Cranelift -> obiekt ELF (.o)
///   4. Kompilacja runtime C -> obiekt (.o)
///   5. Linkowanie -> binarka lub .so
pub fn compile(opts: CompileOptions) -> CompileResult<CompileOutput> {
    let input = &opts.input;

    if !input.exists() {
        return Err(CompileError::InputNotFound(input.display().to_string()));
    }

    let ext = input.extension().and_then(|e| e.to_str()).unwrap_or("");

    match ext {
        "hl" => compile_hl_to_bc(opts),
        "bc" => compile_bc_to_elf(opts),
        _    => Err(CompileError::InvalidExtension),
    }
}

/// Kompiluj .hl -> .bc (bytecode JSON + shebang)
fn compile_hl_to_bc(opts: CompileOptions) -> CompileResult<CompileOutput> {
    let input = &opts.input;

    let stem = input.file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CompileError::InvalidInput("zla nazwa pliku".into()))?;

    let output_path = opts.output.clone().unwrap_or_else(|| {
        input.parent().unwrap_or(Path::new(".")).join(format!("{}.bc", stem))
    });

    log_step("PARSE", &input.display().to_string(), opts.verbose);
    let source = std::fs::read_to_string(input)
        .map_err(|e| CompileError::Io(e.to_string()))?;

    let ast = parse_source(&source)
        .map_err(|e| CompileError::Parse(e.to_string()))?;

    log_step("SERIALIZE", "AST -> bytecode JSON", opts.verbose);
    let ast_json = serde_json::to_string(&ast)
        .map_err(|e| CompileError::Codegen(e.to_string()))?;

    // Plik .bc: shebang + marker + JSON AST
    let bc_content = format!(
        "#!/usr/bin/env -S /usr/bin/hl run\n# HL-BC gen 1\n# source: {}\n{}\n",
        input.display(),
        ast_json
    );

    std::fs::write(&output_path, bc_content.as_bytes())
        .map_err(|e| CompileError::Io(e.to_string()))?;

    // Ustaw bit wykonywalny
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&output_path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&output_path, perms);
        }
    }

    let object_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    log_step("OK", &format!("{} ({} KB)", output_path.display(), object_size / 1024), opts.verbose);

    Ok(CompileOutput { output_path, mode: CompileMode::Bytecode, object_size })
}

/// Kompiluj .bc -> ELF / .so
fn compile_bc_to_elf(opts: CompileOptions) -> CompileResult<CompileOutput> {
    let input = &opts.input;

    let stem = input.file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| CompileError::InvalidInput("zla nazwa pliku".into()))?;

    let output_path = opts.output.clone().unwrap_or_else(|| {
        let ext = match opts.mode { CompileMode::Shared => ".so", _ => "" };
        input.parent().unwrap_or(Path::new(".")).join(format!("{}{}", stem, ext))
    });

    // Wczytaj JSON AST z .bc
    log_step("LOAD-BC", &input.display().to_string(), opts.verbose);
    let bc_content = std::fs::read_to_string(input)
        .map_err(|e| CompileError::Io(e.to_string()))?;

    let ast_json_line = bc_content.lines()
        .find(|l| l.starts_with('[') || l.starts_with('{'))
        .ok_or_else(|| CompileError::InvalidInput("Nieprawidlowy format .bc — brak JSON AST".into()))?;

    let ast: Vec<hl_parser::Node> = serde_json::from_str(ast_json_line)
        .map_err(|e| CompileError::Parse(format!("Blad deserializacji .bc: {}", e)))?;

    // Lowering
    log_step("LOWER", "AST -> HlIR", opts.verbose);
    let ir = codegen::lower::lower_ast(&ast)
        .map_err(|e| CompileError::Codegen(e.to_string()))?;

    // Codegen Cranelift
    log_step("CODEGEN", "Cranelift -> ELF object", opts.verbose);
    let tmp_dir = tempfile::tempdir()
        .map_err(|e| CompileError::Io(e.to_string()))?;

    let obj_path = tmp_dir.path().join(format!("{}_hl.o", stem));
    codegen::emit::emit_object(&ir, &obj_path, opts.opt, opts.verbose)
        .map_err(|e| CompileError::Codegen(e.to_string()))?;

    // Runtime C
    log_step("RUNTIME", "C runtime -> object", opts.verbose);
    let rt_obj = tmp_dir.path().join("hl_runtime.o");
    runtime::compile_runtime(&rt_obj, opts.verbose)
        .map_err(|e| CompileError::Runtime(e.to_string()))?;

    // Linkowanie
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
