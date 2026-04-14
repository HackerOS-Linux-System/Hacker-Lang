/// Reprezentacja po sparsowaniu linii `# ...`
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    /// Główna specyfikacja: `std/net:1.2`, `virus/hashlib`, itp.
    pub spec:   String,
    /// Opcjonalny detail po `|`: `utils`, `keyboard`, `ports`
    pub detail: Option<String>,
}

/// Parsuj całą linię importu (wszystko po `#`)
///
/// Przykłady wejścia:
///   " <std/net>"                → spec="std/net", detail=None
///   " <std/net:1.2>"            → spec="std/net:1.2", detail=None
///   " <std/sys> | <utils>"      → spec="std/sys", detail=Some("utils")
///   " <std/io:0.1> | <keyboard>"→ spec="std/io:0.1", detail=Some("keyboard")
///   " <community/repo:v2>"      → spec="community/repo:v2", detail=None
pub fn parse_import_line(line: &str) -> Option<ImportDecl> {
    let line = line.trim();

    // Sprawdź czy zaczyna się od '<'
    if !line.starts_with('<') {
        // Stara składnia (kompatybilność): # std/net lub # std/net <- detail
        return parse_legacy(line);
    }

    // Wyciągnij pierwszą część <...>
    let close1 = line.find('>')?;
    let spec = line[1..close1].trim().to_string();

    // Sprawdź czy jest część po `|`
    let rest = line[close1 + 1..].trim();
    let detail = if rest.starts_with('|') {
        let after_pipe = rest[1..].trim();
        if after_pipe.starts_with('<') && after_pipe.ends_with('>') {
            Some(after_pipe[1..after_pipe.len()-1].trim().to_string())
        } else if after_pipe.starts_with('<') {
            // '<' bez '>' — weź do końca
            let inner = after_pipe.trim_start_matches('<').trim();
            if inner.is_empty() { None } else { Some(inner.to_string()) }
        } else {
            // Bez < > — weź wprost
            if after_pipe.is_empty() { None } else { Some(after_pipe.to_string()) }
        }
    } else {
        None
    };

    if spec.is_empty() { return None; }

    Some(ImportDecl { spec, detail })
}

/// Stara składnia dla kompatybilności:
///   std/net           → spec="std/net", detail=None
///   std/net <- ports  → spec="std/net", detail=Some("ports")
///   owner/repo        → spec="community/owner/repo", detail=None  (GitHub)
fn parse_legacy(line: &str) -> Option<ImportDecl> {
    if let Some(arrow_pos) = line.find("<-") {
        let spec   = line[..arrow_pos].trim().to_string();
        let detail = line[arrow_pos + 2..].trim().to_string();
        Some(ImportDecl {
            spec,
            detail: if detail.is_empty() { None } else { Some(detail) },
        })
    } else {
        // Sprawdź czy to GitHub (owner/repo bez std/)
        let lib = line.trim();
        if !lib.starts_with("std/") && lib.contains('/') && !lib.is_empty() {
            // Opakuj jako community
            Some(ImportDecl {
                spec:   format!("community/{}", lib),
                 detail: None,
            })
        } else {
            Some(ImportDecl { spec: lib.to_string(), detail: None })
        }
    }
}

// ── Resolver (łączy z libs.rs) ────────────────────────────────────────────────

pub use inner::ImportSource;
pub use inner::resolve_spec;

mod inner {
    use super::ImportDecl;

    #[derive(Debug, Clone, PartialEq)]
    pub enum ImportSource {
        Std       { lib: String, detail: Option<String>, version: Option<String> },
        Community { path: String, version: Option<String> },
        Virus     { name: String, version: Option<String> },
    }

    /// Przetłumacz ImportDecl → ImportSource
    pub fn resolve_spec(decl: &ImportDecl) -> Option<ImportSource> {
        let spec = decl.spec.trim();

        // Rozdziel wersję — ostatni ':' jeśli po nim nie ma '/'
        let (body, version) = if let Some(pos) = spec.rfind(':') {
            let after = &spec[pos + 1..];
            if !after.contains('/') {
                (&spec[..pos], Some(after.to_string()))
            } else {
                (spec, None)
            }
        } else {
            (spec, None)
        };

        let slash = body.find('/')?;
        let ns    = &body[..slash];
        let rest  = &body[slash + 1..];

        match ns {
            "std" => {
                // rest może być "net" lub "net/ports" (stary styl)
                let (lib, inline_detail) = if let Some(s) = rest.find('/') {
                    (rest[..s].to_string(), Some(rest[s+1..].to_string()))
                } else {
                    (rest.to_string(), None)
                };
                // detail z `| <...>` ma priorytet nad inline detail
                let detail = decl.detail.clone().or(inline_detail);
                Some(ImportSource::Std { lib, detail, version })
            }
            "community" => Some(ImportSource::Community {
                path:    rest.to_string(),
                                version,
            }),
            "virus" => Some(ImportSource::Virus {
                name:    rest.to_string(),
                            version,
            }),
            _ => None,
        }
    }
}

// ── Testy ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_simple() {
        let d = parse_import_line("<std/net>").unwrap();
        assert_eq!(d.spec, "std/net");
        assert_eq!(d.detail, None);
    }

    #[test]
    fn test_new_version() {
        let d = parse_import_line("<std/net:1.2>").unwrap();
        assert_eq!(d.spec, "std/net:1.2");
        assert_eq!(d.detail, None);
    }

    #[test]
    fn test_new_with_detail() {
        let d = parse_import_line("<std/sys> | <utils>").unwrap();
        assert_eq!(d.spec, "std/sys");
        assert_eq!(d.detail, Some("utils".to_string()));
    }

    #[test]
    fn test_new_version_with_detail() {
        let d = parse_import_line("<std/io:0.1> | <keyboard>").unwrap();
        assert_eq!(d.spec, "std/io:0.1");
        assert_eq!(d.detail, Some("keyboard".to_string()));
    }

    #[test]
    fn test_new_virus() {
        let d = parse_import_line("<virus/hashlib:2.0>").unwrap();
        assert_eq!(d.spec, "virus/hashlib:2.0");
        assert_eq!(d.detail, None);
    }

    #[test]
    fn test_new_community() {
        let d = parse_import_line("<community/github.com/owner/repo:v2>").unwrap();
        assert_eq!(d.spec, "community/github.com/owner/repo:v2");
    }

    #[test]
    fn test_legacy_arrow() {
        let d = parse_import_line("std/net <- ports").unwrap();
        assert_eq!(d.spec, "std/net");
        assert_eq!(d.detail, Some("ports".to_string()));
    }

    #[test]
    fn test_legacy_simple() {
        let d = parse_import_line("std/sys").unwrap();
        assert_eq!(d.spec, "std/sys");
        assert_eq!(d.detail, None);
    }

    #[test]
    fn test_resolve_std_with_detail() {
        let d = parse_import_line("<std/net> | <ports>").unwrap();
        let src = inner::resolve_spec(&d).unwrap();
        assert!(matches!(src, inner::ImportSource::Std { lib, detail: Some(det), .. }
        if lib == "net" && det == "ports"));
    }

    #[test]
    fn test_resolve_virus() {
        let d = parse_import_line("<virus/hashlib:2.0>").unwrap();
        let src = inner::resolve_spec(&d).unwrap();
        assert!(matches!(src, inner::ImportSource::Virus { name, version: Some(v) }
        if name == "hashlib" && v == "2.0"));
    }
}
