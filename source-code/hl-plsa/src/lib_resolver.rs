use std::collections::HashSet;
use std::path::PathBuf;
use dirs;
use miette::{NamedSource, SourceSpan};
use crate::ast::{AnalysisResult, LibRef, LibType, ParseError};

pub fn libs_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/libs")
}

pub fn plugins_root() -> PathBuf {
    dirs::home_dir()
    .expect("HOME not set")
    .join(".hackeros/hacker-lang/plugins")
}

pub fn lib_path(lib: &LibRef) -> Option<PathBuf> {
    let r = libs_root();
    match lib.lib_type {
        LibType::Core  => Some(r.join("core").join(&lib.name).with_extension("hl")),
        LibType::Bytes => Some(r.join("bytes").join(&lib.name).with_extension("so")),
        LibType::Virus => Some(r.join(".virus").join(&lib.name).with_extension("a")),
        LibType::Vira  => Some(r.join(".virus").join(&lib.name)),
    }
}

pub fn handle_lib(
    lib_ref:      LibRef,
    path:         &str,
    src:          &str,
    span:         SourceSpan,
    resolve_libs: bool,
    verbose:      bool,
    seen_libs:    &mut HashSet<String>,
    result:       &mut AnalysisResult,
    errors:       &mut Vec<ParseError>,
) {
    result.libs.push(lib_ref.clone());

    match lib_ref.lib_type {
        LibType::Vira | LibType::Virus => {
            if verbose {
                eprintln!(
                    "[lib] {}: {}{}",
                    lib_ref.lib_type.as_str(),
                          lib_ref.name,
                          lib_ref.use_symbols.as_ref()
                          .map(|s| format!(" use [{}]", s.join(", ")))
                          .unwrap_or_default()
                );
            }
        }
        LibType::Bytes => {
            if verbose {
                if let Some(p) = lib_path(&lib_ref) {
                    eprintln!(
                        "[lib] bin: {}{}",
                        p.display(),
                              lib_ref.use_symbols.as_ref()
                              .map(|s| format!(" use [{}]", s.join(", ")))
                              .unwrap_or_default()
                    );
                }
            }
        }
        LibType::Core => {
            if !resolve_libs { return; }
            let key = lib_ref.cache_key();
            if !seen_libs.insert(key.clone()) {
                if verbose { eprintln!("[lib] już widziany: {}", key); }
                return;
            }
            let fp = match lib_path(&lib_ref) { Some(p) => p, None => return };
            if verbose { eprintln!("[lib] parsowanie: {}", fp.display()); }
            if let Some(p) = fp.to_str() {
                match crate::parser::parse_file(p, resolve_libs, verbose, seen_libs) {
                    Ok(lr) => {
                        result.deps.extend(lr.deps);
                        result.libs.extend(lr.libs);
                        result.functions.extend(lr.functions);
                        result.main_body.extend(lr.main_body);
                        result.modules.extend(lr.modules);
                        result.is_potentially_unsafe |= lr.is_potentially_unsafe;
                        result.safety_warnings.extend(lr.safety_warnings);
                    }
                    Err(mut e) => errors.append(&mut e),
                }
            } else {
                errors.push(ParseError::SyntaxError {
                    src:      NamedSource::new(path, src.to_string()),
                            span,
                            line_num: 0,
                            advice:   format!("Nieprawidłowa ścieżka lib: {}", lib_ref.name),
                });
            }
        }
    }
}
