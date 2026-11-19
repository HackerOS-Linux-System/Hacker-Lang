use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{HlibError, Result};
use crate::manifest::{
    Artifact, ArtifactKind, Dependency, ExportedSymbol, Language, Manifest, SignatureInfo,
    HLIB_SPEC_VERSION,
};
use crate::fsutil;
use crate::limits::Limits;
use crate::read::{sha256_hex, CHECKSUMS_ENTRY, MANIFEST_ENTRY, SIGNATURE_ENTRY};
use crate::sign;
use crate::validate::{validate_entry_path, validate_name, validate_target};
use crate::version::{Version, VersionReq};

/// Znacznik czasu builda: `SOURCE_DATE_EPOCH` (reproducible builds), inaczej zegar.
fn now_rfc3339() -> String {
    let epoch_override = std::env::var("SOURCE_DATE_EPOCH").ok().and_then(|v| v.trim().parse::<u64>().ok());
    rfc3339_from_unix(epoch_override.unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
    }))
}

fn rfc3339_from_unix(secs: u64) -> String {
    // Deliberately dependency-light: hand-rolled RFC 3339 UTC timestamp
    // instead of pulling in a full date/time crate just for this one
    // call. Good enough for "when was this archive built" provenance —
    // not used for anything that requires calendar arithmetic.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    // Civil-from-days (Howard Hinnant's algorithm) — turns a day count
    // since 1970-01-01 into a proleptic-Gregorian (y, m, d) triple
    // without needing a chrono/time dependency.
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m2 = if mp < 10 { mp + 3 } else { mp - 9 };
    let y2 = if m2 <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y2, m2, d, h, m, s
    )
}

/// A single entry queued up to go into the archive before `finish()`
/// writes everything out in one pass.
struct PendingEntry {
    path: String,
    data: Vec<u8>,
}

/// Incrementally assembles a `.hlib` archive: a `tar` stream (manifest +
/// checksums + optional signature + every artifact file), whole-stream
/// compressed with `zstd`.
///
/// Typical use from a language's own CLI:
/// ```ignore
/// let mut b = HlibBuilder::new("mylib", "1.0.0", Language::Hsharp);
/// b.set_language_version("0.9.0");
/// b.add_shared_object("x86_64-unknown-linux-gnu", &so_bytes);
/// b.add_header(&header_json_bytes);
/// b.add_ast(&ast_json_bytes);
/// b.add_export(export_symbol);
/// b.finish(Path::new("mylib.hlib"), signing_key.as_ref())?;
/// ```
pub struct HlibBuilder {
    name: String,
    version: String,
    language: Language,
    language_version: String,
    description: String,
    authors: Vec<String>,
    dependencies: Vec<Dependency>,
    exports: Vec<ExportedSymbol>,
    entries: Vec<PendingEntry>,
    artifacts: Vec<Artifact>,
    seen_paths: BTreeMap<String, ()>,
    created_at: Option<String>,
    compression_level: i32,
    abi_version: u32,
    limits: Limits,
}

impl HlibBuilder {
    pub fn new(name: impl Into<String>, version: impl Into<String>, language: Language) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            language,
            language_version: String::new(),
            description: String::new(),
            authors: Vec::new(),
            dependencies: Vec::new(),
            exports: Vec::new(),
            entries: Vec::new(),
            artifacts: Vec::new(),
            seen_paths: BTreeMap::new(),
            created_at: None,
            compression_level: 19,
            abi_version: crate::manifest::HLIB_ABI_VERSION,
            limits: Limits::DEFAULT,
        }
    }

    /// Jak `new`, ale od razu waliduje nazwę i wersję.
    pub fn try_new(name: impl Into<String>, version: impl Into<String>, language: Language) -> Result<Self> {
        let (name, version) = (name.into(), version.into());
        validate_name(&name)?;
        Version::parse(&version)?;
        Ok(Self::new(name, version, language))
    }

    /// Nadpisuje `created_at` (RFC 3339 UTC) — dla reprodukowalnych buildów.
    pub fn set_created_at(&mut self, ts: impl Into<String>) -> &mut Self {
        self.created_at = Some(ts.into());
        self
    }

    /// Poziom zstd 1..=22 (domyślnie 19).
    pub fn set_compression_level(&mut self, level: i32) -> &mut Self {
        self.compression_level = level.clamp(1, 22);
        self
    }

    pub fn set_abi_version(&mut self, v: u32) -> &mut Self {
        self.abi_version = v;
        self
    }

    /// Limity rozmiaru *budowanego* archiwum (domyślnie takie same jak przy czytaniu,
    /// żeby nie zbudować pliku, którego własny czytnik by nie otworzył).
    pub fn set_limits(&mut self, limits: Limits) -> &mut Self {
        self.limits = limits;
        self
    }

    pub fn set_language_version(&mut self, v: impl Into<String>) -> &mut Self {
        self.language_version = v.into();
        self
    }

    pub fn set_description(&mut self, d: impl Into<String>) -> &mut Self {
        self.description = d.into();
        self
    }

    pub fn add_author(&mut self, a: impl Into<String>) -> &mut Self {
        self.authors.push(a.into());
        self
    }

    pub fn add_dependency(&mut self, name: impl Into<String>, version_req: impl Into<String>) -> &mut Self {
        self.dependencies.push(Dependency {
            name: name.into(),
            version_req: version_req.into(),
        });
        self
    }

    pub fn add_export(&mut self, sym: ExportedSymbol) -> &mut Self {
        self.exports.push(sym);
        self
    }

    /// Registers one raw archive entry under `kind`, at a path this
    /// builder chooses by convention (see the `add_*` helpers below for
    /// the concrete layout every reader can rely on). Returns an error
    /// if `rel_path` was already added — every path must be unique.
    fn add_entry(&mut self, rel_path: String, kind: ArtifactKind, target: Option<String>, data: Vec<u8>) -> Result<&mut Self> {
        validate_name(&self.name)?;
        if let Some(t) = &target {
            validate_target(t)?;
        }
        validate_entry_path(&rel_path)?;
        if data.len() as u64 > self.limits.max_entry_bytes {
            return Err(HlibError::LimitExceeded {
                what: "entry size",
                actual: data.len() as u64,
                limit: self.limits.max_entry_bytes,
            });
        }
        if self.entries.len() >= self.limits.max_entries {
            return Err(HlibError::LimitExceeded {
                what: "entry count",
                actual: self.entries.len() as u64 + 1,
                limit: self.limits.max_entries as u64,
            });
        }
        if self.seen_paths.insert(rel_path.clone(), ()).is_some() {
            return Err(HlibError::DuplicatePath(rel_path));
        }
        let sha256 = sha256_hex(&data);
        let size = data.len() as u64;
        self.artifacts.push(Artifact {
            path: rel_path.clone(),
            kind,
            target,
            sha256,
            size,
        });
        self.entries.push(PendingEntry { path: rel_path, data });
        Ok(self)
    }

    /// Adds a native shared object built for `target` (a target triple,
    /// e.g. `x86_64-unknown-linux-gnu`). Stored at
    /// `native/<target>/<name>.so` (extension kept as `.so` regardless
    /// of host OS — consumers rename on extraction if they need
    /// `.dylib`/`.dll`, since the archive itself is platform-neutral).
    pub fn add_shared_object(&mut self, target: &str, data: &[u8]) -> Result<&mut Self> {
        let path = format!("native/{}/{}.so", target, self.name);
        self.add_entry(path, ArtifactKind::SharedObject, Some(target.to_string()), data.to_vec())
    }

    /// Adds Hacker Lang bytecode (target-independent).
    pub fn add_bytecode(&mut self, data: &[u8]) -> Result<&mut Self> {
        let path = format!("bytecode/{}.hlbc", self.name);
        self.add_entry(path, ArtifactKind::Bytecode, None, data.to_vec())
    }

    /// Adds the serialized AST blob (JSON) covering every exported
    /// generic/macro definition.
    pub fn add_ast(&mut self, json_data: &[u8]) -> Result<&mut Self> {
        let path = format!("ast/{}.ast.json", self.name);
        self.add_entry(path, ArtifactKind::Ast, None, json_data.to_vec())
    }

    /// Adds the language-agnostic interface header (JSON) — usually the
    /// same data as `self.exports`, but kept as its own standalone file
    /// too so a consumer can grab just the header without parsing the
    /// whole manifest (handy for editor tooling / LSPs).
    pub fn add_header(&mut self, json_data: &[u8]) -> Result<&mut Self> {
        let path = format!("headers/{}.iface.json", self.name);
        self.add_entry(path, ArtifactKind::Header, None, json_data.to_vec())
    }

    /// Adds an arbitrary extra file (docs, license, …) at a
    /// caller-chosen path under `extra/`.
    pub fn add_extra(&mut self, rel_name: &str, data: &[u8]) -> Result<&mut Self> {
        let path = format!("extra/{}", rel_name.trim_start_matches('/'));
        self.add_entry(path, ArtifactKind::Extra, None, data.to_vec())
    }

    /// Jak `add_extra`, ale z pliku na dysku.
    pub fn add_extra_file(&mut self, rel_name: &str, file: &Path) -> Result<&mut Self> {
        let data = std::fs::read(file).map_err(|e| HlibError::Io { path: file.to_path_buf(), source: e })?;
        self.add_extra(rel_name, &data)
    }

    /// Finalizes the archive: builds `manifest.json`, computes
    /// `CHECKSUMS.sha256` over every artifact + the manifest itself,
    /// optionally signs it with `signing_key_hex` (32-byte Ed25519
    /// signing key, lowercase hex — see `sign::generate_keypair`), tars
    /// everything, zstd-compresses the tar, and writes the result to
    /// `output_path`. Returns the `Manifest` that was embedded, so
    /// callers can print a summary without re-opening the file.
    pub fn finish(self, output_path: &Path, signing_key_hex: Option<&str>) -> Result<Manifest> {
        let (bytes, manifest) = self.finish_to_vec(signing_key_hex)?;
        fsutil::write_atomic(output_path, &bytes, 0o644)?;
        Ok(manifest)
    }

    /// Jak `finish`, ale zwraca gotowe bajty `.hlib` (bez dotykania dysku) —
    /// przydatne do serwerów rejestru, testów i `HlibStore::install_bytes`.
    pub fn finish_to_vec(mut self, signing_key_hex: Option<&str>) -> Result<(Vec<u8>, Manifest)> {
        validate_name(&self.name)?;
        Version::parse(&self.version)?;
        for d in &self.dependencies {
            validate_name(&d.name)?;
            VersionReq::parse(&d.version_req)?;
        }
        let mut seen_exports = std::collections::BTreeSet::new();
        for e in &self.exports {
            if e.name.trim().is_empty() {
                return Err(HlibError::ManifestMismatch("export with an empty name".into()));
            }
            if !seen_exports.insert((e.name.clone(), format!("{:?}", e.kind))) {
                return Err(HlibError::DuplicatePath(format!("export `{}`", e.name)));
            }
        }

        // CHECKSUMS.sha256 covers every *artifact* entry added via the
        // `add_*` helpers — deliberately NOT `manifest.json` itself.
        // `manifest.json` already embeds each artifact's own sha256 (see
        // `Artifact::sha256`), and excluding it here sidesteps a
        // circularity: the final manifest bytes depend on whether a
        // `signature` block is present, which itself must be computed
        // from CHECKSUMS.sha256 — so the manifest cannot also be a
        // hashed input to that same checksums file. A reader that wants
        // to confirm `manifest.json` matches the archive it shipped with
        // simply re-checks each `artifacts[].sha256` against
        // `CHECKSUMS.sha256`'s lines, which is exactly what
        // `HlibArchive::verify_checksums` does.
        let mut lines: Vec<(String, String)> = self
            .entries
            .iter()
            .map(|e| (e.path.clone(), sha256_hex(&e.data)))
            .collect();
        lines.sort_by(|a, b| a.0.cmp(&b.0));
        let checksums_text = lines
            .iter()
            .map(|(path, digest)| format!("{}  {}\n", digest, path))
            .collect::<String>();
        let checksums_bytes = checksums_text.into_bytes();

        // Optional signature over the exact bytes of CHECKSUMS.sha256.
        let signature = if let Some(key_hex) = signing_key_hex {
            let signing_key = sign::signing_key_from_hex(key_hex)?;
            let sig_bytes = sign::sign(&signing_key, &checksums_bytes);
            let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
            self.entries.push(PendingEntry {
                path: SIGNATURE_ENTRY.to_string(),
                data: sig_bytes.to_vec(),
            });
            Some(SignatureInfo {
                algorithm: "ed25519".to_string(),
                public_key: public_key_hex,
                signed_entry: CHECKSUMS_ENTRY.to_string(),
            })
        } else {
            None
        };

        self.entries.push(PendingEntry {
            path: CHECKSUMS_ENTRY.to_string(),
            data: checksums_bytes,
        });

        let final_manifest = Manifest {
            hlib_spec_version: HLIB_SPEC_VERSION,
            name: self.name.clone(),
            version: self.version.clone(),
            language: self.language,
            language_version: self.language_version.clone(),
            abi_version: self.abi_version,
            description: self.description.clone(),
            authors: self.authors.clone(),
            created_at: self.created_at.clone().unwrap_or_else(now_rfc3339),
            dependencies: self.dependencies.clone(),
            artifacts: self.artifacts.clone(),
            exports: self.exports.clone(),
            signature,
        };
        let final_manifest_bytes = serde_json::to_vec_pretty(&final_manifest)?;
        self.entries.push(PendingEntry {
            path: MANIFEST_ENTRY.to_string(),
            data: final_manifest_bytes,
        });

        // Tar it all up, in a stable order (manifest first, then
        // checksums/signature, then everything else sorted by path) so
        // two builds from identical inputs produce byte-identical
        // archives.
        self.entries.sort_by(|a, b| entry_sort_key(&a.path).cmp(&entry_sort_key(&b.path)));

        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            builder.mode(tar::HeaderMode::Deterministic);
            for entry in &self.entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(entry.data.len() as u64);
                header.set_mode(0o644);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                header.set_cksum();
                builder.append_data(&mut header, &entry.path, entry.data.as_slice())?;
            }
            builder.finish()?;
        }
        if tar_bytes.len() as u64 > self.limits.max_unpacked_bytes {
            return Err(HlibError::LimitExceeded {
                what: "unpacked size",
                actual: tar_bytes.len() as u64,
                limit: self.limits.max_unpacked_bytes,
            });
        }

        let compressed = zstd::stream::encode_all(tar_bytes.as_slice(), self.compression_level)?;
        if compressed.len() as u64 > self.limits.max_archive_bytes {
            return Err(HlibError::LimitExceeded {
                what: "archive size",
                actual: compressed.len() as u64,
                limit: self.limits.max_archive_bytes,
            });
        }
        Ok((compressed, final_manifest))
    }
}

/// Sort key giving `manifest.json`, `CHECKSUMS.sha256`, `SIGNATURE.ed25519`
/// a fixed position at the front of the archive (in that order), with
/// everything else after, alphabetically — purely cosmetic (so `tar tf`
/// on a `.hlib` reads sensibly) but also makes archives reproducible.
fn entry_sort_key(path: &str) -> (u8, &str) {
    match path {
        MANIFEST_ENTRY => (0, path),
        CHECKSUMS_ENTRY => (1, path),
        SIGNATURE_ENTRY => (2, path),
        _ => (3, path),
    }
}
