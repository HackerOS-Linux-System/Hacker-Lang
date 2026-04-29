#[derive(Debug, Clone, PartialEq)]
pub struct ShebangInfo {
    pub raw:         String,
    pub interpreter: String,
    pub args:        Vec<String>,
}

impl ShebangInfo {
    pub fn is_shebang(line: &str) -> bool { line.starts_with("#!") }

    pub fn parse(line: &str) -> Option<ShebangInfo> {
        if !line.starts_with("#!") { return None; }
        let rest = line[2..].trim();
        let mut parts = rest.split_whitespace();
        let interpreter = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = parts.map(|s| s.to_string()).collect();
        if interpreter.is_empty() { return None; }
        Some(ShebangInfo { raw: line.to_string(), interpreter, args })
    }

    pub fn is_hl_shebang(&self) -> bool {
        self.interpreter.ends_with("hl")
            || self.interpreter.ends_with("env") && self.args.first().map(|s| s == "hl").unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct PreprocessResult {
    pub source:  String,
    pub shebang: Option<ShebangInfo>,
    pub offset:  usize,
}

pub fn preprocess(source: &str) -> PreprocessResult {
    let mut lines = source.splitn(2, '\n');
    let first = lines.next().unwrap_or("");
    let rest  = lines.next().unwrap_or("");

    if ShebangInfo::is_shebang(first) {
        let shebang = ShebangInfo::parse(first);
        let new_source = format!("\n{}", rest);
        PreprocessResult { source: new_source, shebang, offset: 1 }
    } else {
        PreprocessResult { source: source.to_string(), shebang: None, offset: 0 }
    }
}
