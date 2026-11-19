use colored::Colorize;
use hl_hlib::{
    sign, ArtifactKind, ExportedSymbol, ExportedSymbolKind, HlibArchive, HlibBuilder, HlibError, HlibStore,
    InstallOptions, Language, TrustPolicy, TrustStatus, Version, VersionReq,
};
use hl_parser::Node;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::cli::HlibAction;

pub fn run(action: HlibAction) {
    match action {
        HlibAction::Build { file, output, lib_version, with_bytecode, sign, description, authors, deps, extras, level } => {
            cmd_build(file, output, lib_version, with_bytecode, sign, description, authors, deps, extras, level)
        }
        HlibAction::Inspect { file, json } => cmd_inspect(&file, json),
        HlibAction::Verify { file, pubkeys, require_signed, json } => cmd_verify(&file, &pubkeys, require_signed, json),
        HlibAction::Keygen { out, force } => cmd_keygen(&out, force),
        HlibAction::Pubkey { keyfile } => cmd_pubkey(&keyfile),
        HlibAction::Fingerprint { pubkey } => cmd_fingerprint(&pubkey),
        HlibAction::Bind { file, into } => cmd_bind(&file, &into),
        HlibAction::Extract { file, into, entry } => cmd_extract(&file, &into, entry.as_deref()),
        HlibAction::List { json } => cmd_list(json),
        HlibAction::Find { name, req, json } => cmd_find(&name, &req, json),
        HlibAction::Install { file, dir, pubkeys, require_signed, force, json } => {
            cmd_install(&file, dir, &pubkeys, require_signed, force, json)
        }
        HlibAction::Remove { name, version, dir } => cmd_remove(&name, &version, dir),
    }
}

fn die(msg: impl AsRef<str>) -> ! {
    eprintln!("{} {}", "BŁĄD:".red().bold(), msg.as_ref());
    std::process::exit(1);
}

/// Błąd biblioteki → komunikat (tryb zwykły) albo `{"ok":false,"code":..}` (JSON), exit 1.
fn fail(e: &HlibError, json: bool) -> ! {
    if json {
        println!("{}", json!({ "ok": false, "code": e.code(), "error": e.to_string() }));
        std::process::exit(1);
    }
    die(e.to_string())
}

fn policy_from(pubkeys: &[String], require_signed: bool, json: bool) -> TrustPolicy {
    let mut p = TrustPolicy::from_env().unwrap_or_else(|e| fail(&e, json));
    for k in pubkeys {
        p = p.with_key(k.clone()).unwrap_or_else(|e| fail(&e, json));
    }
    if require_signed {
        p.require_signed = true;
    }
    p
}

fn default_user_dir() -> PathBuf {
    HlibStore::user_dir().unwrap_or_else(|| PathBuf::from("hlibs"))
}

fn standard_store() -> HlibStore {
    HlibStore::standard(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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

#[allow(clippy::too_many_arguments)]
fn cmd_build(
    file: PathBuf,
    output: Option<String>,
    lib_version: String,
    with_bytecode: bool,
    sign_key: Option<PathBuf>,
    description: Option<String>,
    authors: Vec<String>,
    deps: Vec<String>,
    extras: Vec<PathBuf>,
    level: i32,
) {
    if !file.exists() {
        die(format!("plik nie istnieje: {}", file.display()));
    }
    let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("mylib").to_string();
    let out_hlib = output.unwrap_or_else(|| format!("build/{}.hlib", stem));

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

    let mut builder = HlibBuilder::try_new(stem.clone(), lib_version, Language::Hackerlang).unwrap_or_else(|e| die(format!("{e}")));
    builder.set_language_version(env!("CARGO_PKG_VERSION"));
    builder.set_compression_level(level);
    if let Some(d) = description {
        builder.set_description(d);
    }
    for a in authors {
        builder.add_author(a);
    }
    for d in deps {
        match d.split_once('=') {
            Some((n, r)) => builder.add_dependency(n.trim(), r.trim()),
            None => die(format!("zły format --dep `{}` (oczekiwano nazwa=wymaganie)", d)),
        };
    }
    for ex in &extras {
        let name = ex.file_name().and_then(|f| f.to_str()).unwrap_or_else(|| die(format!("zła ścieżka --extra: {}", ex.display())));
        builder.add_extra_file(name, ex).unwrap_or_else(|e| die(format!("{e}")));
    }
    builder.add_ast(&ast_json).unwrap_or_else(|e| die(format!("{e}")));
    builder.add_header(&header_json).unwrap_or_else(|e| die(format!("{e}")));
    for sym in &symbols {
        builder.add_export(sym.clone());
    }

    if with_bytecode {
        let tmp_bc = std::env::temp_dir().join(format!("hlib-{}-{}.bc", stem, std::process::id()));
        match hl_compiler::compile_hl_to_bc(&file, Some(&tmp_bc)) {
            Ok(bc_path) => {
                let bc_bytes = std::fs::read(&bc_path).unwrap_or_else(|e| die(format!("nie mogę odczytać `{}`: {}", bc_path.display(), e)));
                builder.add_bytecode(&bc_bytes).unwrap_or_else(|e| die(format!("{e}")));
                let _ = std::fs::remove_file(&bc_path);
            }
            Err(e) => die(format!("kompilacja do bytecode nie powiodła się: {e}")),
        }
    }

    let signing_key_hex = sign_key.map(|p| {
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

fn manifest_json(archive: &HlibArchive, path: &Path) -> Value {
    let m = &archive.manifest;
    json!({
        "ok": true,
        "path": path.display().to_string(),
        "name": m.name,
        "version": m.version,
        "language": m.language.as_str(),
        "language_version": m.language_version,
        "spec": m.hlib_spec_version,
        "abi": m.abi_version,
        "description": m.description,
        "authors": m.authors,
        "created_at": m.created_at,
        "signed": archive.is_signed(),
        "signer_fingerprint": m.signature.as_ref().and_then(|s| sign::fingerprint(&s.public_key).ok()),
        "archive_sha256": archive.archive_sha256(),
        "packed_size": archive.packed_size(),
        "unpacked_size": archive.unpacked_size(),
        "dependencies": m.dependencies.iter().map(|d| json!({"name": d.name, "version_req": d.version_req})).collect::<Vec<_>>(),
        "artifacts": m.artifacts.iter().map(|a| json!({
            "path": a.path, "kind": format!("{:?}", a.kind), "target": a.target, "sha256": a.sha256, "size": a.size,
        })).collect::<Vec<_>>(),
        "exports": m.exports.iter().map(|e| json!({"name": e.name, "signature": e.signature})).collect::<Vec<_>>(),
    })
}

fn cmd_inspect(file: &Path, as_json: bool) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| fail(&e, as_json));
    if as_json {
        println!("{}", manifest_json(&archive, file));
        return;
    }
    let m = &archive.manifest;
    println!("{} {} v{} ({})", "hlib:".bold(), m.name, m.version, m.language.as_str());
    if !m.description.is_empty() {
        println!("  opis:      {}", m.description);
    }
    println!("  spec:      {}   abi: {}", m.hlib_spec_version, m.abi_version);
    println!("  zbudowano: {} {}  ({})", m.language.as_str(), m.language_version, m.created_at);
    match &m.signature {
        Some(s) => println!("  podpisane: tak (klucz {})", sign::fingerprint(&s.public_key).unwrap_or_else(|_| "?".into())),
        None => println!("  podpisane: nie"),
    }
    println!("  sha256:    {}", archive.archive_sha256());
    if !m.dependencies.is_empty() {
        println!("  zależności ({}):", m.dependencies.len());
        for d in &m.dependencies {
            println!("    {} {}", d.name, d.version_req);
        }
    }
    println!("  artefakty ({}):", m.artifacts.len());
    for a in &m.artifacts {
        println!("    {:<10} {:<40} {} B  sha256:{}", format!("{:?}", a.kind), a.path, a.size, &a.sha256[..16]);
    }
    println!("  eksporty ({}):", m.exports.len());
    for e in &m.exports {
        println!("    {}", e.signature);
    }
}

fn cmd_verify(file: &Path, pubkeys: &[String], require_signed: bool, as_json: bool) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| fail(&e, as_json));
    let policy = policy_from(pubkeys, require_signed, as_json);
    let status = archive.verify(&policy).unwrap_or_else(|e| fail(&e, as_json));
    if as_json {
        let (st, fp) = match &status {
            TrustStatus::Unsigned => ("unsigned", None),
            TrustStatus::SignedUnpinned => ("signed_unpinned", None),
            TrustStatus::Trusted(fp) => ("trusted", Some(fp.clone())),
        };
        println!("{}", json!({ "ok": true, "checksums": true, "trust": st, "fingerprint": fp, "entries": archive.entry_count() }));
        return;
    }
    println!("{} sumy sha256 OK ({} wpisów)", "✓".green().bold(), archive.entry_count());
    match status {
        TrustStatus::Trusted(fp) => println!("{} podpis OK, zaufany klucz {}", "✓".green().bold(), fp),
        TrustStatus::SignedUnpinned => println!(
            "{} archiwum jest podpisane, ale nie przypięto klucza (--pubkey / HL_HLIB_TRUSTED_KEYS) — autentyczność NIE zweryfikowana",
            "!".yellow().bold()
        ),
        TrustStatus::Unsigned => println!("{} archiwum niepodpisane", "!".yellow().bold()),
    }
}

fn cmd_keygen(out: &Path, force: bool) {
    let (signing_hex, verifying_hex) = sign::generate_keypair();
    // klucz powstaje od razu z prawami 0600 (bez okna, w którym jest czytelny dla innych)
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    if force { opts.create(true).truncate(true); } else { opts.create_new(true); }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    let mut f = opts.open(out).unwrap_or_else(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            die(format!("`{}` już istnieje — użyj --force, by nadpisać", out.display()))
        }
        die(format!("nie mogę zapisać `{}`: {}", out.display(), e))
    });
    f.write_all(signing_hex.as_bytes()).and_then(|_| f.write_all(b"\n")).unwrap_or_else(|e| die(format!("zapis klucza: {e}")));
    println!("{} klucz prywatny zapisany do {} (nie udostępniaj go!)", "✓".green().bold(), out.display());
    println!("  klucz publiczny: {}", verifying_hex.bold());
    println!("  odcisk:          {}", sign::fingerprint(&verifying_hex).unwrap_or_default());
}

fn cmd_pubkey(keyfile: &Path) {
    let hex = std::fs::read_to_string(keyfile).unwrap_or_else(|e| die(format!("nie mogę odczytać `{}`: {}", keyfile.display(), e)));
    println!("{}", sign::public_key_of(&hex).unwrap_or_else(|e| die(format!("{e}"))));
}

fn cmd_fingerprint(pubkey: &str) {
    println!("{}", sign::fingerprint(pubkey).unwrap_or_else(|e| die(format!("{e}"))));
}

fn cmd_extract(file: &Path, into: &Path, entry: Option<&str>) {
    let archive = HlibArchive::open(file).unwrap_or_else(|e| die(format!("{e}")));
    archive.verify_checksums().unwrap_or_else(|e| die(format!("weryfikacja sum nie powiodła się: {e}")));
    match entry {
        Some(e) => {
            let dest = archive.extract_entry(e, into).unwrap_or_else(|e| die(format!("{e}")));
            println!("{}", dest.display());
        }
        None => {
            archive.extract_to_dir(into).unwrap_or_else(|e| die(format!("{e}")));
            println!("{} wypakowano {} wpisów do {}", "✓".green().bold(), archive.entry_count(), into.display());
        }
    }
}

fn cmd_list(as_json: bool) {
    let store = standard_store();
    let entries = store.list();
    if as_json {
        let arr: Vec<Value> = entries
            .iter()
            .map(|e| json!({"name": e.name, "version": e.version.as_ref().map(|v| v.to_string()), "path": e.path.display().to_string()}))
            .collect();
        println!("{}", json!({"ok": true, "libraries": arr}));
        return;
    }
    if entries.is_empty() {
        println!("(brak bibliotek .hlib w: {})", store.dirs().iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", "));
    }
    for e in entries {
        println!("{:<24} {:<12} {}", e.name, e.version.map(|v| v.to_string()).unwrap_or_else(|| "-".into()), e.path.display());
    }
}

fn cmd_find(name: &str, req: &str, as_json: bool) {
    let req = VersionReq::parse_import(req).unwrap_or_else(|e| fail(&e, as_json));
    let e = standard_store().find(name, &req).unwrap_or_else(|e| fail(&e, as_json));
    if as_json {
        println!("{}", json!({"ok": true, "name": e.name, "version": e.version.map(|v| v.to_string()), "path": e.path.display().to_string()}));
    } else {
        println!("{}", e.path.display());
    }
}

fn cmd_install(file: &Path, dir: Option<PathBuf>, pubkeys: &[String], require_signed: bool, force: bool, as_json: bool) {
    let dest_dir = dir.unwrap_or_else(default_user_dir);
    let opts = InstallOptions { policy: policy_from(pubkeys, require_signed, as_json), overwrite: force };
    let dest = HlibStore::install(file, &dest_dir, &opts).unwrap_or_else(|e| fail(&e, as_json));
    if as_json {
        println!("{}", json!({"ok": true, "path": dest.display().to_string()}));
        return;
    }
    println!("{} zainstalowano → {}", "✓".green().bold(), dest.display());
}

fn cmd_remove(name: &str, version: &str, dir: Option<PathBuf>) {
    let v = Version::parse(version).unwrap_or_else(|e| fail(&e, false));
    let dest_dir = dir.unwrap_or_else(default_user_dir);
    match HlibStore::remove(&dest_dir, name, &v) {
        Ok(true) => println!("{} usunięto {}-{}", "✓".green().bold(), name, v),
        Ok(false) => die(format!("nie znaleziono {}-{} w {}", name, v, dest_dir.display())),
        Err(e) => fail(&e, false),
    }
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
    // documents the extracted paths. The recommended path is
    // `# <hlib/name>`, which loads the AST directly with full verification.
    let ast_artifact = m.artifacts.iter().find(|a| matches!(a.kind, ArtifactKind::Ast));
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
            "\n// Zalecany import (z weryfikacją sum i podpisu) — odkomentuj:\n// # <hlib/{}>\n",
            m.name
        ));
    }
    let stub_path = into.join(format!("{}_hlib_bind.hl", m.name));
    std::fs::write(&stub_path, stub).unwrap_or_else(|e| die(format!("nie mogę zapisać `{}`: {}", stub_path.display(), e)));

    println!("{} wypakowano {} artefakt(ów) do {}", "✓".green().bold(), m.artifacts.len(), into.display());
    println!("  wygenerowano: {}", stub_path.display());
}
