use std::os::raw::c_void;

// ─────────────────────────────────────────────────────────────
// Raw FFI — extern "C" bindowania do gc.c
// ─────────────────────────────────────────────────────────────
extern "C" {
    // ── System GC [A] ─────────────────────────────────────────
    /// Alokuj w young generation (bump pointer, O(1))
    pub fn gc_malloc(size: usize) -> *mut c_void;
    /// Alokuj bezpośrednio w old generation (malloc+header)
    pub fn gc_alloc_old(size: usize) -> *mut c_void;
    /// Oznacz wskaźnik jako live (nie zbieraj)
    pub fn gc_mark(ptr: *mut c_void);
    /// Usuń wszystkie znaczniki przed nowym cyklem mark-sweep
    pub fn gc_unmark_all();
    /// Minor GC: przesuń survivors do old, sweep young
    pub fn gc_sweep();
    /// Full GC: minor + major sweep old generation
    pub fn gc_collect_full();
    /// Wypisz statystyki GC na stderr
    pub fn gc_stats_print();
    /// Pobierz statystyki GC do zmiennych
    pub fn gc_stats_get(
        minor_out:     *mut u64,
        major_out:     *mut u64,
        promoted_out:  *mut u64,
        total_out:     *mut u64,
    );

    // ── Arena allocator [B] ───────────────────────────────────
    /// Inicjalizuj arenę z initial_size bajtów (mmap)
    pub fn arena_init(arena: *mut ArenaRaw, initial_size: usize);
    /// Alokuj size bajtów w arenie (bump pointer, O(1))
    pub fn arena_alloc(arena: *mut ArenaRaw, size: usize) -> *mut c_void;
    /// Alokuj i zeruj (memset 0)
    pub fn arena_alloc_zeroed(arena: *mut ArenaRaw, size: usize) -> *mut c_void;
    /// Duplikuj string do areny
    pub fn arena_strdup(arena: *mut ArenaRaw, s: *const u8) -> *mut u8;
    /// Duplikuj n bajtów stringa do areny
    pub fn arena_strndup(arena: *mut ArenaRaw, s: *const u8, n: usize) -> *mut u8;
    /// Reset areny: uwolnij wszystko oprócz pierwszego bloku (O(1))
    pub fn arena_reset(arena: *mut ArenaRaw);
    /// Zwolnij całą arenę (munmap wszystkich bloków)
    pub fn arena_free(arena: *mut ArenaRaw);
    /// Wypisz statystyki areny
    pub fn arena_stats_print(arena: *const ArenaRaw, name: *const u8);
    /// Zapisz punkt powrotu
    pub fn arena_save(arena: *const ArenaRaw) -> ArenaSavepointRaw;
    /// Przywróć stan do savepoint
    pub fn arena_restore(arena: *mut ArenaRaw, sp: ArenaSavepointRaw);
}

// ─────────────────────────────────────────────────────────────
// Arena types (repr(C) — dokładne dopasowanie do gc.c)
// ─────────────────────────────────────────────────────────────

/// Opaque handle dla ArenaChunk linked list
/// Rzeczywista struktura w gc.c — Rust tylko przechowuje wskaźnik
#[repr(C)]
pub struct ArenaChunkRaw {
    _private: [u8; 0],
}

/// Odpowiada struct Arena w gc.c
#[repr(C)]
pub struct ArenaRaw {
    pub head:         *mut ArenaChunkRaw,
    pub first:        *mut ArenaChunkRaw,
    pub chunk_size:   usize,
    pub total_allocs: usize,
    pub total_bytes:  usize,
}

impl ArenaRaw {
    pub const fn zeroed() -> Self {
        Self {
            head:         std::ptr::null_mut(),
            first:        std::ptr::null_mut(),
            chunk_size:   0,
            total_allocs: 0,
            total_bytes:  0,
        }
    }
}

/// Odpowiada struct ArenaSavepoint w gc.c
#[repr(C)]
pub struct ArenaSavepointRaw {
    pub head: *mut ArenaChunkRaw,
    pub top:  *mut u8,
}

// ─────────────────────────────────────────────────────────────
// Safe Rust wrapper: Arena
// ─────────────────────────────────────────────────────────────
/// Bezpieczny wrapper na arena allocator z gc.c
/// Implementuje RAII — arena_free() w Drop
pub struct Arena {
    raw: ArenaRaw,
}

impl Arena {
    /// Utwórz arenę z initial_size bajtów
    pub fn new(initial_size: usize) -> Self {
        let mut a = Self { raw: ArenaRaw::zeroed() };
        unsafe { arena_init(&mut a.raw, initial_size) };
        a
    }

    /// Alokuj T w arenie (niezainicjalizowane)
    /// SAFETY: Zwrócony wskaźnik żyje tyle co arena
    pub unsafe fn alloc<T>(&mut self) -> *mut T {
        arena_alloc(&mut self.raw, std::mem::size_of::<T>()) as *mut T
    }

    /// Alokuj T i zeruj pamięć
    pub unsafe fn alloc_zeroed<T>(&mut self) -> *mut T {
        arena_alloc_zeroed(&mut self.raw, std::mem::size_of::<T>()) as *mut T
    }

    /// Alokuj bufor bajtów
    pub unsafe fn alloc_bytes(&mut self, size: usize) -> *mut u8 {
        arena_alloc(&mut self.raw, size) as *mut u8
    }

    /// Zduplikuj string do areny
    pub fn strdup(&mut self, s: &str) -> *mut u8 {
        let bytes = s.as_bytes();
        unsafe {
            arena_strndup(&mut self.raw, bytes.as_ptr(), bytes.len())
        }
    }

    /// Reset areny — zwolnij wszystkie alokacje (O(1))
    pub fn reset(&mut self) {
        unsafe { arena_reset(&mut self.raw) };
    }

    /// Zapisz savepoint dla backtrackingu
    pub fn save(&self) -> ArenaSavepointRaw {
        unsafe { arena_save(&self.raw) }
    }

    /// Przywróć stan do savepoint
    pub fn restore(&mut self, sp: ArenaSavepointRaw) {
        unsafe { arena_restore(&mut self.raw, sp) };
    }

    /// Wypisz statystyki
    pub fn print_stats(&self, name: &str) {
        let c_name = std::ffi::CString::new(name).unwrap_or_default();
        unsafe { arena_stats_print(&self.raw, c_name.as_ptr() as *const u8) };
    }

    /// Całkowita liczba alokacji
    pub fn total_allocs(&self) -> usize {
        self.raw.total_allocs
    }

    /// Całkowita liczba zaalokowanych bajtów
    pub fn total_bytes(&self) -> usize {
        self.raw.total_bytes
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe { arena_free(&mut self.raw) };
    }
}

// SAFETY: Arena jest single-threaded w HL runtime
unsafe impl Send for Arena {}

// ─────────────────────────────────────────────────────────────
// GcStats — zebrane statystyki GC
// ─────────────────────────────────────────────────────────────
#[derive(Debug, Default, Clone, Copy)]
pub struct GcStats {
    pub minor_count: u64,
    pub major_count: u64,
    pub promoted:    u64,
    pub total_allocs: u64,
}

impl GcStats {
    /// Pobierz aktualne statystyki z gc.c
    pub fn collect() -> Self {
        let (mut minor, mut major, mut promoted, mut total) = (0u64, 0u64, 0u64, 0u64);
        unsafe {
            gc_stats_get(&mut minor, &mut major, &mut promoted, &mut total);
        }
        Self { minor_count: minor, major_count: major, promoted, total_allocs: total }
    }
}

// ─────────────────────────────────────────────────────────────
// Wygodne funkcje do użycia w VM
// ─────────────────────────────────────────────────────────────

/// Wykonaj pełny cykl GC (minor + major)
pub fn full_gc() {
    unsafe { gc_collect_full() };
}

/// Wypisz szczegółowe statystyki GC na stderr
pub fn print_gc_stats() {
    unsafe { gc_stats_print() };
}
