use std::path::PathBuf;

/// Every way building, reading, verifying or storing a `.hlib` archive can fail.
///
/// Warianty są stabilne (`#[non_exhaustive]` nie jest użyte celowo — loader i CLI
/// dopasowują je wprost), nowe dopisywane wyłącznie na końcu.
#[derive(thiserror::Error, Debug)]
pub enum HlibError {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("i/o error: {0}")]
    IoPlain(#[from] std::io::Error),

    #[error("manifest (de)serialization failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("`{0}` is not a valid .hlib archive (not a zstd-compressed tar / bad header)")]
    NotAnArchive(PathBuf),

    #[error("archive is missing required entry `{0}`")]
    MissingEntry(String),

    #[error("archive entry `{path}` failed checksum verification (expected {expected}, got {actual})")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("CHECKSUMS.sha256 line is malformed: `{0}`")]
    MalformedChecksumLine(String),

    #[error("archive is not signed (no SIGNATURE.ed25519 entry) — cannot verify")]
    Unsigned,

    #[error("ed25519 signature verification failed for this archive")]
    BadSignature,

    #[error("invalid ed25519 key material: {0}")]
    BadKey(String),

    #[error("hlib spec version {found} is newer than the highest version this reader understands ({supported})")]
    UnsupportedSpecVersion { found: u32, supported: u32 },

    #[error("unknown artifact kind `{0}`")]
    UnknownArtifactKind(String),

    #[error("unknown target language `{0}` (expected one of: hsharp, hackerlang, hackerscript)")]
    UnknownLanguage(String),

    #[error("duplicate archive entry path `{0}`")]
    DuplicatePath(String),

    // ── dopisane w wersji produkcyjnej ──────────────────────────────────────
    #[error("invalid library name `{0}` (allowed: 1-64 chars of [A-Za-z0-9_-], must start with a letter or digit)")]
    InvalidName(String),

    #[error("invalid version `{0}` (expected semver: MAJOR.MINOR.PATCH[-pre][+build])")]
    InvalidVersion(String),

    #[error("invalid version requirement `{0}`")]
    InvalidVersionReq(String),

    #[error("invalid target triple `{0}`")]
    InvalidTarget(String),

    #[error("unsafe or invalid archive path `{0}` (absolute, `..`, backslash, NUL or empty component)")]
    UnsafePath(String),

    #[error("archive entry `{0}` has an unsupported type (only regular files are allowed — no symlinks/hardlinks/devices)")]
    UnsupportedEntryType(String),

    #[error("limit exceeded: {what} ({actual} > {limit})")]
    LimitExceeded {
        what: &'static str,
        actual: u64,
        limit: u64,
    },

    #[error("archive entry `{0}` is not covered by CHECKSUMS.sha256")]
    UntrackedEntry(String),

    #[error("manifest is inconsistent with the archive: {0}")]
    ManifestMismatch(String),

    #[error("archive is signed by an untrusted key (fingerprint {0})")]
    UntrustedKey(String),

    #[error("trust policy requires a signed archive, but no trusted keys are configured")]
    NoTrustedKeys,

    #[error("artifact `{0}` is listed in the manifest but not present in the archive")]
    ArtifactMissing(String),

    #[error("refusing to write through a symlink: {0}")]
    SymlinkRefused(PathBuf),

    #[error("hlib `{name}` not found for requirement `{req}` (searched {searched} directories)")]
    NotFound {
        name: String,
        req: String,
        searched: usize,
    },

    #[error("archive contains `{found}` v{found_version}, but `{expected}` was requested")]
    NameMismatch {
        expected: String,
        found: String,
        found_version: String,
    },

    #[error("archive version {found} does not satisfy requirement `{req}`")]
    VersionUnsatisfied { found: String, req: String },

    #[error("native ABI version {found} is newer than the one this reader supports ({supported})")]
    UnsupportedAbiVersion { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, HlibError>;

impl HlibError {
    /// Stabilny, maszynowo-czytelny kod błędu (używany przez `--json` w CLI
    /// i przez bibliotekę `main/hlib.hl`).
    pub fn code(&self) -> &'static str {
        use HlibError::*;
        match self {
            Io { .. } | IoPlain(_) => "io",
            Json(_) => "json",
            NotAnArchive(_) => "not_an_archive",
            MissingEntry(_) => "missing_entry",
            ChecksumMismatch { .. } => "checksum_mismatch",
            MalformedChecksumLine(_) => "malformed_checksums",
            Unsigned => "unsigned",
            BadSignature => "bad_signature",
            BadKey(_) => "bad_key",
            UnsupportedSpecVersion { .. } => "unsupported_spec",
            UnknownArtifactKind(_) => "unknown_artifact_kind",
            UnknownLanguage(_) => "unknown_language",
            DuplicatePath(_) => "duplicate_path",
            InvalidName(_) => "invalid_name",
            InvalidVersion(_) => "invalid_version",
            InvalidVersionReq(_) => "invalid_version_req",
            InvalidTarget(_) => "invalid_target",
            UnsafePath(_) => "unsafe_path",
            UnsupportedEntryType(_) => "unsupported_entry_type",
            LimitExceeded { .. } => "limit_exceeded",
            UntrackedEntry(_) => "untracked_entry",
            ManifestMismatch(_) => "manifest_mismatch",
            UntrustedKey(_) => "untrusted_key",
            NoTrustedKeys => "no_trusted_keys",
            ArtifactMissing(_) => "artifact_missing",
            SymlinkRefused(_) => "symlink_refused",
            NotFound { .. } => "not_found",
            NameMismatch { .. } => "name_mismatch",
            VersionUnsatisfied { .. } => "version_unsatisfied",
            UnsupportedAbiVersion { .. } => "unsupported_abi",
        }
    }
}
