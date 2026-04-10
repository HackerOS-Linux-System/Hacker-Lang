#[derive(Debug, Clone, PartialEq)]
pub enum ImportSource {
    /// Biblioteka standardowa: std/net, std/fs, std/sys, ...
    Std {
        lib:     String,          // np. "net", "fs", "sys"
        detail:  Option<String>,  // np. "ports", "dns" (po <- w starej składni)
        version: Option<String>,
    },
    /// Społecznościowy plik .hl lub repo GitHub/GitLab
    Community {
        path:    String,          // np. "plik.hl" lub "github.com/owner/repo"
        version: Option<String>,
    },
    /// Biblioteka .so z ekosystemu Virus
    Virus {
        name:    String,
        version: Option<String>,
    },
}

/// Parsuje string wewnątrz # <...>
/// Przykłady:
///   "std/net"                    → Std { lib: "net", detail: None, version: None }
///   "std/net:1.2"                → Std { lib: "net", detail: None, version: Some("1.2") }
///   "community/plik.hl"          → Community { path: "plik.hl", ... }
///   "community/github.com/o/r"   → Community { path: "github.com/o/r", ... }
///   "virus/hashlib:2.1"          → Virus { name: "hashlib", version: Some("2.1") }
pub fn parse_import_spec(raw: &str) -> Option<ImportSource> {
    let raw = raw.trim();

    // Rozdziel ewentualną wersję po ':'
    // Ale uwaga: "community/github.com/owner/repo:v1" — tylko ostatni ':' to wersja
    let (body, version) = if let Some(colon_pos) = raw.rfind(':') {
        // Sprawdź czy to nie port w URL (np. "community/host:8080/path")
        // Heurystyka: jeśli po ':' nie ma '/', to to wersja
        let after = &raw[colon_pos + 1..];
        if !after.contains('/') {
            (&raw[..colon_pos], Some(after.to_string()))
        } else {
            (raw, None)
        }
    } else {
        (raw, None)
    };

    // Podziel po pierwszym '/'
    let slash = body.find('/')?;
    let namespace = &body[..slash];
    let rest      = &body[slash + 1..];

    match namespace {
        "std" => {
            // rest może być "net" lub "net/ports" (stary styl z detail)
            let (lib, detail) = if let Some(s) = rest.find('/') {
                (rest[..s].to_string(), Some(rest[s+1..].to_string()))
            } else {
                (rest.to_string(), None)
            };
            Some(ImportSource::Std { lib, detail, version })
        }
        "community" => {
            Some(ImportSource::Community {
                path: rest.to_string(),
                 version,
            })
        }
        "virus" => {
            Some(ImportSource::Virus {
                name: rest.to_string(),
                 version,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_std() {
        let s = parse_import_spec("std/net").unwrap();
        assert!(matches!(s, ImportSource::Std { lib, .. } if lib == "net"));
    }

    #[test]
    fn test_std_version() {
        let s = parse_import_spec("std/net:1.2").unwrap();
        assert!(matches!(s, ImportSource::Std { version: Some(v), .. } if v == "1.2"));
    }

    #[test]
    fn test_community_file() {
        let s = parse_import_spec("community/utils.hl").unwrap();
        assert!(matches!(s, ImportSource::Community { path, .. } if path == "utils.hl"));
    }

    #[test]
    fn test_community_github() {
        let s = parse_import_spec("community/github.com/owner/repo:v2").unwrap();
        assert!(matches!(s, ImportSource::Community { path, version: Some(v) }
        if path == "github.com/owner/repo" && v == "v2"));
    }

    #[test]
    fn test_virus() {
        let s = parse_import_spec("virus/hashlib:2.1").unwrap();
        assert!(matches!(s, ImportSource::Virus { name, version: Some(v) }
        if name == "hashlib" && v == "2.1"));
    }
}
