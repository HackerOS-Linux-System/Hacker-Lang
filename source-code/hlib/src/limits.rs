#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maksymalny rozmiar pliku `.hlib` (skompresowany) w bajtach.
    pub max_archive_bytes: u64,
    /// Maksymalny łączny rozmiar po dekompresji (strumień tar).
    pub max_unpacked_bytes: u64,
    /// Maksymalna liczba wpisów w archiwum.
    pub max_entries: usize,
    /// Maksymalny rozmiar pojedynczego wpisu.
    pub max_entry_bytes: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        max_archive_bytes: 256 * 1024 * 1024,
        max_unpacked_bytes: 1024 * 1024 * 1024,
        max_entries: 10_000,
        max_entry_bytes: 512 * 1024 * 1024,
    };

    /// Bez ograniczeń — wyłącznie dla zaufanych, własnych archiwów.
    pub const UNLIMITED: Limits = Limits {
        max_archive_bytes: u64::MAX,
        max_unpacked_bytes: u64::MAX,
        max_entries: usize::MAX,
        max_entry_bytes: u64::MAX,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Limits::DEFAULT
    }
}
