pub mod bytecode;
pub mod lower;
pub mod optimize;
pub mod serialize;
pub mod cache;

pub use bytecode::{HlModule, HlBcHeader, Instruction, ConstPool, FuncTable};
pub use lower::lower_ast;
pub use optimize::optimize_module;
pub use serialize::{write_bc_file, read_bc_file, BC_MAGIC, BC_VERSION};
pub use cache::{bc_cache_path, ensure_cache_dir, cache_cleanup_if_needed, CACHE_MAX_FILES};

use anyhow::Result;
use hl_parser::{parse_source_with_meta, ParseMeta};
use std::path::Path;

/// Główna funkcja: .hl → .bc
/// Kompiluje plik źródłowy do zoptymalizowanego bytecode.
/// Zwraca ścieżkę do pliku .bc.
pub fn compile_hl_to_bc(source_path: &Path, out_path: Option<&Path>) -> Result<std::path::PathBuf> {
    let source = std::fs::read_to_string(source_path)?;
    compile_source_to_bc(&source, source_path, out_path)
}

/// Kompiluj kod źródłowy (string) do .bc
pub fn compile_source_to_bc(
    source: &str,
    source_path: &Path,
    out_path: Option<&Path>,
) -> Result<std::path::PathBuf> {
    // 1. Parse
    let meta: ParseMeta = parse_source_with_meta(source)?;

    // 2. Lower AST → HlModule (nasz IR bytecode)
    let mut module = lower_ast(&meta.nodes, source_path, meta.gen.number());

    // 3. Optymalizuj
    optimize_module(&mut module);

    // 4. Wyznacz ścieżkę wyjściową
    let bc_path = match out_path {
        Some(p) => p.to_path_buf(),
        None => {
            let stem = source_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
            source_path.with_file_name(format!("{}.bc", stem))
        }
    };

    // 5. Serializuj do pliku
    write_bc_file(&module, &bc_path)?;

    Ok(bc_path)
}

/// Kompiluj do cache (~/.hackeros/hacker-lang/cache/<hash>.bc)
/// Zwraca ścieżkę do pliku cache.
pub fn compile_to_cache(source: &str, source_path: &Path) -> Result<std::path::PathBuf> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    ensure_cache_dir()?;
    cache_cleanup_if_needed()?;

    // Hash: zawartość + ścieżka + mtime
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    source_path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(source_path) {
        if let Ok(mtime) = meta.modified() {
            mtime.hash(&mut hasher);
        }
    }
    let hash = hasher.finish();

    let cache_path = bc_cache_path(&format!("{:016x}", hash));

    // Jeśli cache trafiony i plik aktualny — zwróć od razu
    if cache_path.exists() {
        if let Ok(bc_meta) = std::fs::metadata(&cache_path) {
            if let (Ok(src_m), Ok(bc_m)) = (
                std::fs::metadata(source_path).and_then(|m| m.modified()),
                                            bc_meta.modified(),
            ) {
                if bc_m >= src_m {
                    tracing::debug!("cache hit: {:?}", cache_path);
                    return Ok(cache_path);
                }
            }
        }
    }

    tracing::debug!("cache miss, kompiluje: {:?}", source_path);
    compile_source_to_bc(source, source_path, Some(&cache_path))?;
    Ok(cache_path)
}
