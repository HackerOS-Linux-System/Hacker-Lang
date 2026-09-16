use colored::Colorize;
use hl_hlib::{ExportedSymbol, ExportedSymbolKind, HlibArchive, HlibBuilder, Language};
use hl_parser::Node;
use std::path::{Path, PathBuf};

use crate::cli::HlibAction;

pub fn run(action: HlibAction) {
    match action {
        HlibAction::Build { file, output, lib_version, with_bytecode, sign } => {
            cmd_build(file, output, lib_version, with_bytecode, sign)
        }
        HlibAction::Inspect { file } => cmd_inspect(&file),
        HlibAction::Verify { file, pubkey } => cmd_verify(&file, pubkey.as_deref()),
        HlibAction::Keygen { out } => cmd_keygen(&out),
        HlibAction::Bind { file, into } => cmd_bind(&file, &into),
    }
}

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("{} {}", "BŁĄD:".red().bold(), msg.as_ref());
    std::process::exit(1);
}

/// Top-level function names this library exports — every `FuncDef` and
/// `ArenaFuncDef` node directly in the file (HL has no nested
/// namespaces to walk into, and no `pub`/private distinction — a `.hl`
/// file's top level is its whole public interface by construction).
fn exported_functions(nodes: &[Node]) -> Vec<ExportedSymbol> {
    nodes
        .iter()
        .filter_map(|n| match n {
            Node::FuncDef { name, .. } => Some(ExportedSymbol {
                name: name.clone(),
                kind: ExportedSymbolKind::Function,
                signature: format!(": {} def ... done", name),
                params: Vec::new(), // HL calls are string-marshalled — see HLIB_FORMAT.md
                returns: None,
                generic: false,
                is_macro: false,
            }),
            Node::ArenaFuncDef { name, arena_size, .. } => Some(ExportedSymbol {
                name: name.clone(),
                kind: ExportedSymbolKind::Function,
                signature: format!(":: {} {} def ... done", name, arena_size),
                params: Vec::new(),
                returns: None,
                generic: false,
                is_macro: false,
            }),
            _ => None,
        })
        .collect()
}

fn cmd_build(file: PathBuf, output: Option<String>, lib_version: String, with_bytecode: bool, sign: Option<PathBuf>) {
    if !file.exists() {
        die(format!("plik nie istnieje: {}", file.display()));
    }
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("mylib").to_string();
    let out_hlib = output.unwrap_or_else(|| format!("build/{}.hlib", stem));
    if let Some(parent) = Path::new(&out_hlib).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let source = std::fs::read_to_string(&file).unwrap_or_else(|e| die(format!("nie mogę odczytać `{}`: {}", file.display(), e)));
    let meta = hl_parser::parse_source_with_meta(&source).unwrap_or_else(|e| die(format!("parsowanie nie powiodło się: {}", e)));

    let symbols = exported_functions(&meta.nodes);
    if symbols.is_empty() {
        eprintln!(
            "{} brak `FuncDef`/`ArenaFuncDef` na najwyższym poziomie w `{}` — .hlib będzie miał puste `exports`.",
            "!".yellow().bold(),
            file.display()
        );
    }

    let ast_json = serde_json::to_vec_pretty(&meta.nodes).unwrap_or_else(|e| die(format!("serializacja AST nie powiodła się: {e}")));
    let header_json = serde_json::to_vec_pretty(&symbols).unwrap_or_else(|e| die(format!("serializacja nagłówka nie powiodła się: {e}")));

    let mut builder = HlibBuilder::new(stem.clone(), lib_version, Language::Hackerlang);
    builder.set_language_version(env!("CARGO_PKG_VERSION"));
    builder.add_ast(&ast_json).unwrap_or_else(|e| die(format!("{e}")));
    builder.add_header(&header_json).unwrap_or_else(|e| die(format!("{e}")));
    for sym in &symbols {
        builder.add_export(sym.clone());
    }

    if with_bytecode {
        let tmp_bc = std::env::temp_dir().join(format!("hlib-{}.bc", stem));
        match hl_compiler::compile_hl_to_bc(&file, Some(&tmp_bc)) {
            Ok(bc_path) => {
                let bc_bytes = std::fs::read(&bc_path).unwrap_or_else(|e| die(format!("nie mogę odczytać `{}`: {}", bc_path.display(), e)));
                builder.add_bytecode(&bc_bytes).unwrap_or_else(|e| die(format!("{e}")));
                let _ = std::fs::remove_file(&bc_path);
            }
            Err(e) => die(format!("kompilacja do bytecode nie powiodła się: {e}")),
        }
    }

    let signing_key_hex = sign.map(|p| {
        std::fs::read_to_string(&p)
            .unwrap_or_else(|e| die(format!("nie mogę odczytać klucza `{}`: {}", p.display(), e)))
            .trim()
            .to_string()
    });

    let manifest = builder
        .finish(Path::new(&out_hlib), signing_key_hex.as_deref())
        .unwrap_or_else(|e| die(format!("{e}")));

    println!(
        "{} {} → {} ({} eksport(ów), {} artefakt(ów){})",
        "✓".green().bold(),
        file.display(),
        out_hlib.bold(),
        manifest.exports.len(),
        manifest.artifacts.len(),
        if manifest.signature.is_some() { ", podpisane" } else { ", niepodpisane" },
    );
}

fn cmd_inspect(file: &Path) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| die(format!("{e}")));
    let m = &archive.manifest;
    println!("{} {} v{} ({})", "hlib:".bold(), m.name, m.version, m.language.as_str());
    println!("  spec:      {}   abi: {}", m.hlib_spec_version, m.abi_version);
    println!("  zbudowano: {} {}  ({})", m.language.as_str(), m.language_version, m.created_at);
    println!("  podpisane: {}", if m.signature.is_some() { "tak" } else { "nie" });
    println!("  artefakty ({}):", m.artifacts.len());
    for a in &m.artifacts {
        println!("    {:<10} {:<40} {} B  sha256:{}", format!("{:?}", a.kind), a.path, a.size, &a.sha256[..16]);
    }
    println!("  eksporty ({}):", m.exports.len());
    for e in &m.exports {
        println!("    {}", e.signature);
    }
}

fn cmd_verify(file: &Path, pubkey: Option<&str>) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| die(format!("{e}")));
    archive.verify_checksums().unwrap_or_else(|e| die(format!("weryfikacja sum nie powiodła się: {e}")));
    println!("{} sumy sha256 OK ({} wpisów)", "✓".green().bold(), archive.manifest.artifacts.len());
    if let Some(pk) = pubkey {
        archive.verify_signature(pk).unwrap_or_else(|e| die(format!("weryfikacja podpisu nie powiodła się: {e}")));
        println!("{} podpis OK", "✓".green().bold());
    }
}

fn cmd_keygen(out: &Path) {
    let (signing_hex, verifying_hex) = hl_hlib::sign::generate_keypair();
    std::fs::write(out, &signing_hex).unwrap_or_else(|e| die(format!("nie mogę zapisać `{}`: {}", out.display(), e)));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(out) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(out, perms);
        }
    }
    println!("{} klucz prywatny zapisany do {} (nie udostępniaj go!)", "✓".green().bold(), out.display());
    println!("  klucz publiczny: {}", verifying_hex.bold());
}

fn cmd_bind(file: &Path, into: &Path) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| die(format!("{e}")));
    archive.verify_checksums().unwrap_or_else(|e| die(format!("weryfikacja sum nie powiodła się: {e}")));
    std::fs::create_dir_all(into).ok();
    archive.extract_to_dir(into).unwrap_or_else(|e| die(format!("{e}")));

    let m = &archive.manifest;
    // The AST artifact IS directly executable HL source (as Node JSON) —
    // but `<<` file-import expects real `.hl` text, so we don't try to
    // regenerate `.hl` syntax from the AST here. Instead the bind stub
    // documents the extracted paths and, for the common case, wraps the
    // ast.json artifact behind a tiny loader using HL's own JSON
    // decoding — see HLIB_FORMAT.md's Hacker Lang section for the full
    // pattern this expands to.
    let ast_artifact = m.artifacts.iter().find(|a| matches!(a.kind, hl_hlib::ArtifactKind::Ast));
    let mut stub = String::new();
    stub.push_str(&format!("#!/usr/bin/hl\n/// Auto-wygenerowane przez `hl hlib bind` z {}\n", file.display()));
    stub.push_str(&format!("/// {} v{} ({}) — nie edytuj ręcznie, uruchom ponownie `hl hlib bind`.\n\n", m.name, m.version, m.language.as_str()));
    stub.push_str("using <gen 2>\n\n");
    if let Some(ast) = ast_artifact {
        stub.push_str(&format!("// AST tej biblioteki wypakowano do: {}\n", into.join(&ast.path).display()));
        stub.push_str("// Funkcje dostępne w tej bibliotece:\n");
        for e in &m.exports {
            stub.push_str(&format!("//   {}\n", e.signature));
        }
        stub.push_str(&format!(
            "\n// TODO: podmień to na `<< {}` gdy plik źródłowy .hl producenta\n// jest też dostępny obok archiwum — to najprostsza ścieżka importu.\n",
            into.join(format!("{}.hl", m.name)).display()
        ));
    }
    let stub_path = into.join(format!("{}_hlib_bind.hl", m.name));
    std::fs::write(&stub_path, stub).unwrap_or_else(|e| die(format!("nie mogę zapisać `{}`: {}", stub_path.display(), e)));

    println!("{} wypakowano {} artefakt(ów) do {}", "✓".green().bold(), m.artifacts.len(), into.display());
    println!("  wygenerowano: {}", stub_path.display());
}
