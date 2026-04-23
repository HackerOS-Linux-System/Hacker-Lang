#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub spec:   String,
    pub detail: Option<String>,
}

pub fn parse_import_line(line: &str) -> Option<ImportDecl> {
    let line = line.trim();
    if !line.starts_with('<') { return parse_legacy(line); }
    let close1 = line.find('>')?;
    let spec = line[1..close1].trim().to_string();
    let rest = line[close1 + 1..].trim();
    let detail = if rest.starts_with('|') {
        let after_pipe = rest[1..].trim();
        if after_pipe.starts_with('<') && after_pipe.ends_with('>') {
            Some(after_pipe[1..after_pipe.len()-1].trim().to_string())
        } else if !after_pipe.is_empty() {
            Some(after_pipe.trim_start_matches('<').trim().to_string())
        } else { None }
    } else { None };
    if spec.is_empty() { return None; }
    Some(ImportDecl { spec, detail })
}

fn parse_legacy(line: &str) -> Option<ImportDecl> {
    if let Some(arrow_pos) = line.find("<-") {
        let spec   = line[..arrow_pos].trim().to_string();
        let detail = line[arrow_pos + 2..].trim().to_string();
        Some(ImportDecl { spec, detail: if detail.is_empty() { None } else { Some(detail) } })
    } else {
        let lib = line.trim();
        if !lib.starts_with("std/") && lib.contains('/') && !lib.is_empty() {
            Some(ImportDecl { spec: format!("community/{}", lib), detail: None })
        } else {
            Some(ImportDecl { spec: lib.to_string(), detail: None })
        }
    }
}
