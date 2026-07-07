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
    let mut lines_iter = source.splitn(2, '\n');
    let first = lines_iter.next().unwrap_or("");
    let rest  = lines_iter.next().unwrap_or("");

    let (shebang, body) = if ShebangInfo::is_shebang(first) {
        (ShebangInfo::parse(first), rest)
    } else {
        (None, source)
    };

    // Usuń linie `using <...>` i `using <ROLLING>` ze source zanim trafi do lexera.
    // extract_gen() już je odczytał; lexer nie rozumie składni <gen N>.
    // Zamieniamy takie linie na puste (zachowując numery linii dla diagnostyki).
    let cleaned: String = body
        .lines()
        .map(|line| {
            let t = line.trim();
            if t.starts_with("using") {
                let after = t["using".len()..].trim();
                // using <gen N>  lub  using <ROLLING>  lub  using <gen N+future>
                if after.starts_with('<') && after.ends_with('>') {
                    return "";  // Zastąp pustą linią — numer linii zachowany
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n");

    if shebang.is_some() {
        PreprocessResult { source: format!("\n{}", cleaned), shebang, offset: 1 }
    } else {
        PreprocessResult { source: cleaned, shebang: None, offset: 0 }
    }
}
