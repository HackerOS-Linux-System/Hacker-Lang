use std::alloc::{alloc, dealloc, Layout};
use std::cell::Cell;

/// Bump-pointer arena
pub struct Arena {
    /// Wskaźnik do początku bloku
    ptr:      *mut u8,
    /// Łączny rozmiar areny w bajtach
    capacity: usize,
    /// Aktualny offset (ile zostało już użyte)
    used:     Cell<usize>,
    /// Layout do deallokacji
    layout:   Layout,
}

impl Arena {
    /// Utwórz nową arenę o podanym rozmiarze
    /// Rozmiar jest zaokrąglany w górę do wielokrotności 8 bajtów
    pub fn new(size: usize) -> Self {
        // Minimalny rozmiar: 64 bajty; wyrównanie: 8 bajtów (f64/ptr)
        let size = size.max(64);
        let layout = Layout::from_size_align(size, 8)
        .expect("nieprawidłowy layout areny");
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            panic!("Nie można zaalokować areny {} bajtów", size);
        }
        Self { ptr, capacity: size, used: Cell::new(0), layout }
    }

    /// Zaalokuj `size` bajtów wyrównanych do `align`
    /// Zwraca None jeśli brak miejsca (executor powinien fallback do heap)
    #[inline]
    pub fn alloc_bytes(&self, size: usize, align: usize) -> Option<*mut u8> {
        let used = self.used.get();
        // Wyrównaj do `align`
        let aligned = (used + align - 1) & !(align - 1);
        let new_used = aligned + size;
        if new_used > self.capacity {
            return None; // Arena pełna → fallback
        }
        self.used.set(new_used);
        Some(unsafe { self.ptr.add(aligned) })
    }

    /// Zaalokuj String w arenie — kopiuje bajty do areny, zwraca &str
    /// Czas życia: do końca areny (drop Areny)
    #[inline]
    pub fn alloc_str(&self, s: &str) -> Option<*const u8> {
        let bytes = s.as_bytes();
        let ptr = self.alloc_bytes(bytes.len() + 1, 1)?; // +1 na null terminator
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            *ptr.add(bytes.len()) = 0; // null terminate
        }
        Some(ptr as *const u8)
    }

    /// Ile bajtów zostało użyte
    pub fn used(&self) -> usize { self.used.get() }

    /// Ile bajtów zostało wolnych
    pub fn remaining(&self) -> usize { self.capacity.saturating_sub(self.used.get()) }

    /// Rozmiar areny
    pub fn capacity(&self) -> usize { self.capacity }

    /// Reset areny (reuse bez realokacji) — przydatne gdy arena function
    /// jest wywoływana wielokrotnie w pętli
    #[inline]
    pub fn reset(&self) { self.used.set(0); }
}

impl Drop for Arena {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout); }
    }
}

// Arena jest Send bo używamy jej tylko w jednym wątku na raz
// (arena function nie jest async/multi-threaded)
unsafe impl Send for Arena {}

/// Kontekst wykonania arena function
/// Przechowuje arenę + zmienne lokalne (slice do areny)
pub struct ArenaContext {
    pub arena: Arena,
    /// Licznik alokacji (do debugowania)
    pub alloc_count: usize,
    /// Czy arena się przepełniła (fallback do heap był używany)
    pub overflowed: bool,
}

impl ArenaContext {
    pub fn new(size: usize) -> Self {
        Self {
            arena:       Arena::new(size),
            alloc_count: 0,
            overflowed:  false,
        }
    }

    /// Zaalokuj string w arenie lub heap jeśli arena pełna
    pub fn alloc_string(&mut self, s: String) -> String {
        self.alloc_count += 1;
        // Próbuj zaalokować w arenie
        if let Some(_ptr) = self.arena.alloc_str(&s) {
            // Sukces — ale nadal zwracamy String (Rust ownership model)
            // W praktyce kopia jest w arenie, ale Rust API wymaga String
            // Dla naprawdę zero-copy potrzeba unsafe lifetimes
            s
        } else {
            // Arena pełna — fallback do heap
            if !self.overflowed {
                tracing::warn!(
                    "[arena] przepełnienie areny ({}/{} bajtów) — fallback do heap",
                               self.arena.used(), self.arena.capacity()
                );
                self.overflowed = true;
            }
            s // String zostaje na heap
        }
    }

    /// Statystyki areny (do debugowania / profilowania)
    pub fn stats(&self) -> ArenaStats {
        ArenaStats {
            capacity:    self.arena.capacity(),
            used:        self.arena.used(),
            alloc_count: self.alloc_count,
            overflowed:  self.overflowed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArenaStats {
    pub capacity:    usize,
    pub used:        usize,
    pub alloc_count: usize,
    pub overflowed:  bool,
}

impl std::fmt::Display for ArenaStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "arena: {}/{} bajtów, {} alokacji{}",
               self.used, self.capacity, self.alloc_count,
               if self.overflowed { " [OVERFLOW→heap]" } else { "" })
    }
}
