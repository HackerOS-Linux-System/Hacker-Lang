#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub spec:   String,
    pub detail: Option<String>,
}

pub fn parse_import_line(line: &str) -> Option<ImportDecl> {
    let line = line.trim();
    if !line.starts_with('<') { return parse_legacy(line); }
    let close1 = line.find('>')?;
    let spec_raw = line[1..close1].trim().to_string();
    let rest = line[close1 + 1..].trim();
    let detail = if rest.starts_with('|') {
        let after_pipe = rest[1..].trim();
        if after_pipe.starts_with('<') && after_pipe.ends_with('>') {
            Some(after_pipe[1..after_pipe.len()-1].trim().to_string())
        } else if !after_pipe.is_empty() {
            Some(after_pipe.trim_start_matches('<').trim().to_string())
        } else { None }
    } else { None };

    if spec_raw.is_empty() { return None; }

    // Normalizuj stare przestrzenie nazw na nowe
    let spec = normalize_import_spec(&spec_raw);

    Some(ImportDecl { spec, detail })
}

/// Mapuj stare nazwy przestrzeni na nowe
fn normalize_import_spec(raw: &str) -> String {
    // std/* -> main/*
    if let Some(rest) = raw.strip_prefix("std/") {
        return format!("main/{}", rest);
    }
    // virus/* -> bit/*
    if let Some(rest) = raw.strip_prefix("virus/") {
        return format!("bit/{}", rest);
    }
    // community/* -> github/*
    if let Some(rest) = raw.strip_prefix("community/") {
        return format!("github/{}", rest);
    }
    raw.to_string()
}

fn parse_legacy(line: &str) -> Option<ImportDecl> {
    // Stara skladnia: std/net <- ports
    if let Some(arrow_pos) = line.find("<-") {
        let spec_raw = line[..arrow_pos].trim().to_string();
        let detail   = line[arrow_pos + 2..].trim().to_string();
        let spec = normalize_import_spec(&spec_raw);
        return Some(ImportDecl { spec, detail: if detail.is_empty() { None } else { Some(detail) } });
    }
    let lib = line.trim();
    if !lib.is_empty() {
        let spec = normalize_import_spec(lib);
        Some(ImportDecl { spec, detail: None })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_main_lib() {
        let d = parse_import_line("<main/net>").unwrap();
        assert_eq!(d.spec, "main/net");
    }

    #[test]
    fn test_std_maps_to_main() {
        let d = parse_import_line("<std/net>").unwrap();
        assert_eq!(d.spec, "main/net");
    }

    #[test]
    fn test_bit_lib() {
        let d = parse_import_line("<bit/hashlib>").unwrap();
        assert_eq!(d.spec, "bit/hashlib");
    }

    #[test]
    fn test_virus_maps_to_bit() {
        let d = parse_import_line("<virus/hashlib>").unwrap();
        assert_eq!(d.spec, "bit/hashlib");
    }

    #[test]
    fn test_github_lib() {
        let d = parse_import_line("<github/user/repo>").unwrap();
        assert_eq!(d.spec, "github/user/repo");
    }

    #[test]
    fn test_community_maps_to_github() {
        let d = parse_import_line("<community/user/repo>").unwrap();
        assert_eq!(d.spec, "github/user/repo");
    }

    #[test]
    fn test_legacy_arrow() {
        let d = parse_import_line("std/net <- ports").unwrap();
        assert_eq!(d.spec, "main/net");
        assert_eq!(d.detail.as_deref(), Some("ports"));
    }

    #[test]
    fn test_main_with_detail() {
        let d = parse_import_line("<main/net> | <ports>").unwrap();
        assert_eq!(d.spec, "main/net");
        assert_eq!(d.detail.as_deref(), Some("ports"));
    }

    #[test]
    fn test_main_progress_bar() {
        let d = parse_import_line("<main/progress-bar>").unwrap();
        assert_eq!(d.spec, "main/progress-bar");
    }
}
