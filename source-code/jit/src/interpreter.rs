use anyhow::{bail, Result};
use hl_compiler::bytecode::*;
use crate::runtime::{RuntimeState, NanVal};
use std::process::{Command, Stdio};
use std::collections::HashMap;

// ── Dispatch signal ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ExecSignal {
    Next,
    Jump(u32),
    Return,
    FuncCall(u32),   // name_idx
    Exit(i32),
}

// ── JIT thresholds ────────────────────────────────────────────────────────────

// Trace JIT NAPRAWIONY i ponownie włączony. Historia buga i naprawy — patrz
// obszerny komentarz na górze jit_engine.rs (dwa niezależne braki: brak
// prawdziwego CFG dla Jump/JumpIfFalse/JumpIfTrue, i GetVar/SetVar jako
// no-opy zamiast dostępu do vars_ptr). Naprawa zweryfikowana:
//   - cały tests/*.hl z aktywnym trace JIT (próg=5, agresywne wyzwalanie):
//     zero crashy, zero nowych rozbieżności PASS/FAIL względem trybu
//     domyślnego (wszystkie pętle w tym korpusie poprawnie i bezpiecznie
//     odrzucają się z kompilacji, bo zawierają ExecCmd/stringi — dokładnie
//     zgodnie z projektem bezpieczeństwa, patrz jit_engine.rs).
//   - osobne testy syntetyczne dla przypadków, które FAKTYCZNIE się
//     kompilują: prosty licznik, akumulator z dwiema zmiennymi nazwanymi,
//     modulo, pętle zagnieżdżone (10x10), pętla zero-iteracji, odejmowanie
//     z wynikiem ujemnym — wszystkie identyczne z trybem domyślnym, zero
//     crashy.
// Próg domyślny to wartość produkcyjna (50, jak przed wyłączeniem) — teraz
// dostrajalna przez zmienne środowiskowe (patrz `env_threshold` niżej), a
// dla CIASNYCH pętli (<= TIGHT_LOOP_SIZE instrukcji) efektywnie obniżana:
// mała pętla wykonuje dużo więcej iteracji na jednostkę "rozgrzewki" niż
// duża, więc szybsze przejście na kod natywny szybciej się zwraca.
const TRACE_THRESHOLD: u32 = 50;
const FUNC_JIT_THRESHOLD: u32 = 50;
const TIGHT_LOOP_SIZE: usize = 8;
const TIGHT_LOOP_THRESHOLD_DIVISOR: u32 = 2;
const MIN_ADAPTIVE_THRESHOLD: u32 = 4;
const MAX_TRACE_LOOP_SIZE: usize = 64;
/// Ile razy WOLNO ponowić próbę kompilacji trasy/funkcji po niepowodzeniu
/// spowodowanym przyczyną DYNAMICZNĄ (np. zmienna jeszcze nie ma stabilnie
/// numerycznej wartości), zanim JIT trwale się podda. Niepowodzenia
/// STRUKTURALNE (instrukcje niekwalifikujące się do JIT — patrz
/// `is_jit_eligible`) nigdy się same nie naprawią, więc dla nich nie ma
/// sensu w ogóle liczyć prób — blokujemy od razu i na zawsze (patrz
/// `func_eligible`/`func_jit_blocked` niżej).
const MAX_JIT_RETRIES: u8 = 4;

/// Odczytaj próg z env (np. `HL_JIT_TRACE_THRESHOLD`), z fallbackiem na
/// wartość domyślną, jeśli zmienna nie jest ustawiona / nie parsuje się /
/// jest zerem (próg zerowy nie ma sensu — kompilowałby wszystko od razu,
/// zanim interpreter zdąży w ogóle ustalić stabilne typy zmiennych).
fn env_threshold(var: &str, default: u32) -> u32 {
    std::env::var(var)
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

// ── Główny interpreter ────────────────────────────────────────────────────────

pub struct BytecodeInterpreter<'a> {
    pub module:      &'a HlModule,
    pub state:       RuntimeState,
    /// Liczniki wykonań per instrukcja (dla trace JIT) — indeksowane
    /// BEZPOŚREDNIO przez `pc` (Vec, nie mapa: rozmiar = liczba instrukcji,
    /// dostęp bez hashowania na najgorętszej możliwej ścieżce).
    exec_counts:     Vec<u32>,
    /// Skompilowane trasy pętli — indeksowane BEZPOŚREDNIO przez offset
    /// startu pętli (`target`). Wcześniej `FxHashMap<u32, CompiledTrace>`:
    /// każde przejście przez KAŻDĄ pętlę wsteczną (skompilowaną czy nie —
    /// to sprawdzenie wykonuje się na WIĘKSZOŚCI iteracji każdej pętli w
    /// programie) hashowało klucz. `pc`/`target` są gęstymi indeksami w
    /// [0, instructions.len()) z definicji (to offsety w tym samym
    /// strumieniu bytecode) — `Vec<Option<_>>` daje identyczną semantykę
    /// przy zerowym koszcie hashowania.
    compiled_traces:   Vec<Option<CompiledTrace>>,
    /// Licznik ponownych prób kompilacji per miejsce pętli (indeksowany
    /// przez `target`) — patrz `MAX_JIT_RETRIES`.
    trace_retry_count: Vec<u8>,

    // ── Whole-function JIT ──────────────────────────────────────────────
    // Uzupełnienie Trace JIT: pętla kompiluje SIEBIE, gdy jest gorąca, ale
    // funkcja czysto obliczeniowa wołana wielokrotnie z top-level (bez
    // gorącej pętli WEWNĄTRZ niej) wcześniej nigdy nie trafiała do JIT —
    // `jit_compile`/`record_call` w jit_engine.rs istniały, ale nic ich nie
    // wołało (patrz historia w komentarzu tamtego pliku). Teraz: śledzimy
    // liczbę wywołań PER FUNKCJA i po przekroczeniu progu próbujemy
    // skompilować całe jej ciało.
    //
    // Wszystkie poniższe pola są indeksowane BEZPOŚREDNIO przez `name_idx`
    // (indeks do puli stałych stringowych modułu — patrz `const_str`), z
    // tego samego powodu co `compiled_traces` wyżej: `exec_func_by_name_idx`
    // jest wołane na KAŻDE wywołanie funkcji w skrypcie, skompilowanej czy
    // nie, więc to jest równie gorąca ścieżka jak trasy pętli.
    /// Liczba wywołań per funkcja
    func_call_counts: Vec<u32>,
    /// Cache wyniku audytu whole-function JIT per funkcja (patrz
    /// `jit_engine::InlineAudit`/`audit_for_inlining`) — liczony raz, bo
    /// zbiór instrukcji funkcji (i to, czy ma bezpieczne miejsce do
    /// wklejenia wywołania — patrz "Inlining" niżej) się nie zmienia między
    /// wywołaniami.
    func_eligible:    Vec<Option<crate::jit_engine::InlineAudit>>,
    /// Licznik ponownych prób kompilacji per funkcja (patrz `MAX_JIT_RETRIES`).
    func_retry_count: Vec<u8>,
    /// Funkcje, dla których JIT jest trwale wykluczony w tym uruchomieniu:
    /// albo audyt zwrócił `Ineligible` (nigdy się nie zmieni), albo pula
    /// ponownych prób (`MAX_JIT_RETRIES`) się wyczerpała.
    func_jit_blocked: Vec<bool>,
    /// Skompilowane CAŁE funkcje
    compiled_funcs:   Vec<Option<CompiledTrace>>,

    /// Trwały silnik JIT (patrz doc `JitEngine` w jit_engine.rs) — `None`,
    /// jeśli `HL_NO_JIT` jest ustawione, albo jeśli budowa silnika (np.
    /// wykrycie natywnego ISA) się nie powiodła; w obu przypadkach
    /// interpreter bezpiecznie działa w trybie czysto interpretowanym.
    jit_engine: Option<crate::jit_engine::JitEngine>,
    trace_threshold: u32,
    func_threshold:  u32,

    /// ── Trace linking ───────────────────────────────────────────────────
    /// Tablica przekazywana jako 5. parametr do KAŻDEGO skompilowanego
    /// fragmentu (patrz `jit_engine::CompiledFnPtr`): `link_table[i] != 0`
    /// oznacza "pod offsetem `i` jest już skompilowana natywnie TRASA — jej
    /// wskaźnik funkcji to `link_table[i]`". Wygenerowany kod trasy
    /// SPRAWDZA tę tablicę przy KAŻDYM wyjściu i, jeśli cel jest już
    /// skompilowany, doskakuje do niego BEZPOŚREDNIO w kodzie natywnym —
    /// bez przechodzenia przez pętlę dispatchu w Rust. Rozmiar ustalony RAZ
    /// (= liczba instrukcji) i NIGDY nie zmieniany — indeksy muszą zostać
    /// stabilne przez cały czas życia interpretera, bo wskaźnik do tej
    /// tablicy jest przekazywany w głąb potencjalnie długich łańcuchów
    /// zagnieżdżonych wywołań natywnych.
    link_table: Vec<usize>,
}

/// Skompilowana trasa (wynik trace JIT) ALBO całej funkcji (whole-function
/// JIT) — oba przypadki mają identyczny kształt: wskaźnik do kodu
/// natywnego i offset, na którym interpreter widzi stan jako w pełni
/// zapisany po zakończeniu wykonania natywnego (patrz
/// `jit_engine.rs::emit_store_and_return`). `Copy`: zawiera tylko wskaźnik
/// funkcji i u32 — potrzebne, żeby `vec![None; n]` mogło inicjalizować
/// gęste tablice `Vec<Option<CompiledTrace>>` (patrz `BytecodeInterpreter`).
#[derive(Clone, Copy)]
pub struct CompiledTrace {
    /// Patrz `jit_engine::CompiledFnPtr` — WSPÓLNA sygnatura dla tras i
    /// całych funkcji (piąty parametr to `link_table`, patrz wyżej;
    /// whole-function JIT dostaje go dla jednorodności ABI, ale nigdy go
    /// nie odczytuje w wygenerowanym kodzie).
    pub fn_ptr: crate::jit_engine::CompiledFnPtr,
    /// Offset wyjścia (dokąd skakać po wykonaniu trasy pętli; dla
    /// whole-function JIT jest to zawsze koniec funkcji i jest ignorowany
    /// przez wołającego — patrz `exec_func_by_name_idx`).
    pub exit_offset: u32,
}

/// Zbiorcze statystyki JIT dla jednego uruchomienia — patrz `HL_JIT_STATS`
/// (drukowane przez `runner.rs::run_bc_module`, gdy zmienna jest ustawiona).
pub struct JitStats {
    pub compiled_traces: usize,
    pub compiled_funcs:  usize,
    pub engine_compiles: u32,
    pub engine_insns:    u32,
}

impl<'a> BytecodeInterpreter<'a> {
    pub fn new(module: &'a HlModule) -> Self {
        let n = module.instructions.len();
        // name_idx (indeks do puli stałych stringowych — GetVar/SetVar/
        // CallFunc wszystkie się do niej odwołują) jest zawsze < ten rozmiar
        // z definicji, więc Vec o tej długości pokrywa KAŻDY możliwy
        // name_idx bez potrzeby hashowania na ścieżce wywołania funkcji.
        let name_pool = module.consts.strings.len();

        let jit_engine = if std::env::var("HL_NO_JIT").is_ok() {
            None
        } else {
            match crate::jit_engine::JitEngine::new() {
                Ok(engine) => Some(engine),
                Err(e) => {
                    tracing::warn!(
                        "[jit] silnik JIT niedostępny ({}), praca w trybie czysto interpretowanym",
                        e
                    );
                    None
                }
            }
        };

        Self {
            module,
            state:             RuntimeState::new(module.main_regs as usize, &module.consts.strings),
            exec_counts:       vec![0u32; n],
            compiled_traces:   vec![None; n],
            trace_retry_count: vec![0u8; n],
            func_call_counts:  vec![0u32; name_pool],
            func_eligible:     vec![None; name_pool],
            func_retry_count:  vec![0u8; name_pool],
            func_jit_blocked:  vec![false; name_pool],
            compiled_funcs:    vec![None; name_pool],
            trace_threshold:   env_threshold("HL_JIT_TRACE_THRESHOLD", TRACE_THRESHOLD),
            func_threshold:    env_threshold("HL_JIT_FUNC_THRESHOLD", FUNC_JIT_THRESHOLD),
            jit_engine,
            // Rozmiar ustalony RAZ, nigdy nie zmieniany — patrz doc pola.
            link_table: vec![0usize; n],
        }
    }

    /// Statystyki JIT dla tego uruchomienia (patrz `JitStats`).
    pub fn jit_stats(&self) -> JitStats {
        JitStats {
            compiled_traces: self.compiled_traces.iter().filter(|o| o.is_some()).count(),
            compiled_funcs:  self.compiled_funcs.iter().filter(|o| o.is_some()).count(),
            engine_compiles: self.jit_engine.as_ref().map(|e| e.compiled_count).unwrap_or(0),
            engine_insns:    self.jit_engine.as_ref().map(|e| e.compiled_insns).unwrap_or(0),
        }
    }

    /// Inicjalizuj zmienne HL_VERSION itp.
    ///
    /// BUG (naprawiony): poprzednio używało self.state.interner.intern(...)
    /// do wyliczenia name_idx — ale to CAŁKOWICIE ODDZIELNY interner od
    /// module.consts (którego indeksów używa skompilowany bytecode we
    /// WSZYSTKICH instrukcjach GetVar/SetVar). var_slots/var_cache trzymają
    /// te indeksy jako nieprzezroczyste klucze — jeśli runtime-interner
    /// przypadkiem zwróci tę samą liczbę co jakiś INNY, kompletnie
    /// niezwiązany indeks w module.consts (np. dla zmiennej `_name`),
    /// zapis do HL_VERSION cicho nadpisywał slot tamtej zmiennej. Objawiało
    /// się to losowym pojawianiem się "gen 2" w miejscu zupełnie innych
    /// wartości. Teraz: ustawiamy te zmienne TYLKO jeśli skrypt faktycznie
    /// się do nich odwołuje (czyli ich nazwa już jest w module.consts) —
    /// wtedy używamy DOKŁADNIE tego samego indeksu, więc zero kolizji.
    /// Skrypt, który nigdy nie czyta @HL_VERSION/@HL_GEN i tak nie mógłby
    /// ich odczytać bez tego indeksu w puli, więc nie tracimy funkcjonalności.
    pub fn init_hl_vars(&mut self) {
        if let Some(k) = self.module.consts.find_str("HL_VERSION") {
            let v = self.state.intern_str("gen 2");
            self.state.set_var(k, v);
        }
        if let Some(k2) = self.module.consts.find_str("HL_GEN") {
            let v2 = self.state.intern_str("2");
            self.state.set_var(k2, v2);
        }
    }

    /// Uruchom główny blok
    pub fn run(&mut self) -> Result<i32> {
        self.init_hl_vars();
        // BUG (naprawiony): wcześniej top-level wykonanie było obcinane do
        // `main_end` = offset PIERWSZEJ zdefiniowanej funkcji, co zakładało,
        // że wszystkie `: nazwa def ... done` są na KOŃCU pliku. W realnych
        // skryptach HL funkcje są definiowane najpierw, a top-level kod
        // (dispatch, wywołania) idzie PO nich — więc main_end obcinał
        // wykonanie niemal natychmiast, zanim cokolwiek realnego się
        // wykonało. Teraz ciała funkcji mają własny skip-jump (patrz
        // lower.rs: Node::FuncDef/ArenaFuncDef), więc bezpiecznie
        // wykonujemy CAŁY strumień instrukcji od 0 do końca — sekwencyjny
        // przepływ sam poprawnie omija ciała funkcji, wchodząc do nich
        // tylko przez explicit CallFunc (exec_func_by_name_idx).
        self.exec_block(0, self.module.instructions.len())?;
        Ok(self.state.last_exit)
    }

    fn exec_block(&mut self, start: usize, end: usize) -> Result<ExecSignal> {
        let mut pc = start;
        while pc < end {
            // ── Trace JIT check ───────────────────────────────────────────
            // Przy JumpIfFalse/Jump które wracają do wcześniejszego offsetu
            // (pętla) — zliczamy i kompilujemy po przekroczeniu progu
            if let Some(Instruction::Jump { offset }) = self.module.instructions.get(pc) {
                let target = *offset as usize;
                if target < pc {
                    // Już skompilowana? Wykonaj natywnie od razu — dostęp
                    // O(1) przez Vec (bez hashowania), sprawdzane na TEJ
                    // SAMEJ ścieżce co każda inna iteracja tej pętli.
                    if let Some(trace) = self.compiled_traces.get(target).and_then(|o| o.as_ref()) {
                        let fn_ptr = trace.fn_ptr;
                        let result = self.exec_native_fn_ptr(fn_ptr)?;
                        pc = result as usize;
                        continue;
                    }

                    // Pętla wsteczna, jeszcze nieskompilowana — kandydat do trace JIT.
                    // Guard: kompiluj tylko małe pętle (<= MAX_TRACE_LOOP_SIZE instrukcji)
                    let loop_size = pc - target;
                    // Ciasne pętle (mało instrukcji, więc dużo iteracji na
                    // jednostkę "rozgrzewki") rozgrzewają się o połowę
                    // szybciej niż domyślny próg — patrz komentarz przy
                    // TRACE_THRESHOLD.
                    let effective_threshold = if loop_size <= TIGHT_LOOP_SIZE {
                        (self.trace_threshold / TIGHT_LOOP_THRESHOLD_DIVISOR).max(MIN_ADAPTIVE_THRESHOLD)
                    } else {
                        self.trace_threshold
                    };
                    let count = self.exec_counts.get_mut(pc).map(|c| { *c += 1; *c }).unwrap_or(0);
                    if count == effective_threshold && loop_size <= MAX_TRACE_LOOP_SIZE {
                        // Próbuj skompilować pętlę [target..pc+1]
                        match self.try_compile_trace(target as u32, pc as u32) {
                            Ok(trace) => {
                                tracing::debug!("[trace jit] skompilowano pętlę @ {} (size={})", target, loop_size);
                                // Trace linking: udostępnij tę trasę WSZYSTKIM
                                // innym już skompilowanym trasom, których
                                // wyjście akurat tu prowadzi — patrz doc pola
                                // `link_table` i `jit_engine.rs::emit_store_and_return`.
                                // Retroaktywne: nie trzeba niczego łatać w
                                // już wyemitowanym kodzie tamtych tras, bo
                                // sprawdzają tę tablicę w RUNTIME przy
                                // każdym swoim wyjściu.
                                if let Some(slot) = self.link_table.get_mut(target) { *slot = trace.fn_ptr as usize; }
                                if let Some(slot) = self.compiled_traces.get_mut(target) { *slot = Some(trace); }
                            }
                            Err(e) => {
                                tracing::debug!("[trace jit] pominięto pętlę @ {} (size={}): {}", target, loop_size, e);
                                // Niepowodzenie MOŻE być dynamiczne (np.
                                // zmienna jeszcze nie jest stabilnie
                                // numeryczna po pierwszym przejściu) i
                                // naprawić się samo później — dajemy
                                // ograniczoną liczbę ponownych prób zamiast
                                // trwale się poddawać po pierwszej. Reset
                                // licznika pozwala mu ponownie wspiąć się do
                                // `effective_threshold` po kolejnych
                                // iteracjach tej samej pętli.
                                if let Some(r) = self.trace_retry_count.get_mut(target) {
                                    if *r < MAX_JIT_RETRIES {
                                        *r += 1;
                                        if let Some(c) = self.exec_counts.get_mut(pc) { *c = 0; }
                                    }
                                    // Pula wyczerpana: zostawiamy licznik jak
                                    // jest — nigdy więcej nie trafi dokładnie
                                    // w `effective_threshold`, więc trasa
                                    // jest odtąd trwale interpretowana.
                                }
                            }
                        }
                        if let Some(trace) = self.compiled_traces.get(target).and_then(|o| o.as_ref()) {
                            let fn_ptr = trace.fn_ptr;
                            let result = self.exec_native_fn_ptr(fn_ptr)?;
                            pc = result as usize;
                            continue;
                        }
                    }
                }
            }

            match self.exec_insn(pc)? {
                ExecSignal::Next          => pc += 1,
                ExecSignal::Jump(off)     => pc = off as usize,
                ExecSignal::Return        => return Ok(ExecSignal::Return),
                ExecSignal::Exit(code)    => { self.state.last_exit = code; return Ok(ExecSignal::Return); }
                ExecSignal::FuncCall(ni)  => {
                    self.exec_func_by_name_idx(ni)?;
                    pc += 1;
                }
            }
        }
        Ok(ExecSignal::Next)
    }

    /// Wykonaj DOWOLNY skompilowany natywny fragment (trasa pętli LUB całe
    /// ciało funkcji — patrz `exec_func_by_name_idx`) na bieżącym stanie
    /// rejestrów/zmiennych i zwróć offset, na którym interpreter widzi stan
    /// jako w pełni zapisany po zakończeniu (dla whole-function JIT jest to
    /// zawsze koniec funkcji; wołający wtedy tę wartość ignoruje).
    fn exec_native_fn_ptr(
        &mut self,
        fn_ptr: crate::jit_engine::CompiledFnPtr,
    ) -> Result<u32> {
        let reg_count = self.state.regs.len() as u32;
        let var_count = self.state.vars_flat.len() as u32;

        // SAFETY: NanVal jest #[repr(transparent)] u64 — bezpośredni cast.
        // Skompilowany kod (jit_engine.rs::compile_func_body) czyta/pisze
        // WYŁĄCZNIE do slotów zweryfikowanych w try_compile_trace /
        // try_compile_function jako aktualnie-liczbowe, w zakresie
        // [0, var_count) i [0, reg_count) — patrz komentarz bezpieczeństwa
        // w jit_engine.rs. `link_table.as_ptr()` (patrz doc pola
        // `link_table`) jest ważny przez cały czas trwania tego wywołania —
        // rozmiar Vec jest ustalony raz w `new()` i nigdy nie zmieniany, więc
        // wskaźnik nie staje się nieważny nawet w głąb zagnieżdżonych
        // natywnych wywołań łańcuchowanych przez trace linking.
        let result = unsafe {
            (fn_ptr)(
                self.state.regs.as_mut_ptr() as *mut u64,
                     self.state.vars_flat.as_mut_ptr() as *mut u64,
                     reg_count,
                     var_count,
                     self.link_table.as_ptr(),
            )
        };

        // Naprawione: poprzednio ten wynik był ignorowany na rzecz sztywnego
        // trace.exit_offset, co miało sens tylko dopóki skompilowany kod i
        // tak nigdy nie pętlił się poprawnie (patrz historia w jit_engine.rs).
        // Teraz jit_engine.rs generuje kod, który zwraca DOKŁADNY offset
        // instrukcji, na którym interpreter ma wznowić wykonanie — po
        // wyjściu z pętli (zewnętrzny cel JumpIfFalse/JumpIfTrue) albo po
        // naturalnym końcu skompilowanego zakresu.
        if result < 0 {
            bail!("natywny JIT zwrócił nieprawidłowy offset: {}", result);
        }
        let next_pc = result as u32;
        if next_pc as usize > self.module.instructions.len() {
            bail!("natywny JIT zwrócił offset poza zakresem instrukcji: {}", next_pc);
        }
        Ok(next_pc)
    }

    /// Zbierz i zweryfikuj var_slots dla zakresu instrukcji [start..end)
    /// (bez końca) LUB [start..=end] (z końcem, dla tras pętli — patrz
    /// wywołania niżej). Współdzielona logika bezpieczeństwa między
    /// `try_compile_trace` i `try_compile_function`: KAŻDA zmienna
    /// odwoływana w zakresie musi (a) mieć już przydzielony slot w
    /// vars_flat (czyli być choć raz ustawiona), (b) jej AKTUALNA wartość
    /// musi być `is_num()` — czysta liczba. Jeśli którykolwiek warunek
    /// zawiedzie, w ogóle nie próbujemy kompilować — bezpieczny fallback to
    /// po prostu dalsze interpretowanie.
    fn collect_verified_var_slots(&self, names: &std::collections::HashSet<u32>) -> Result<HashMap<u32, u32>> {
        let mut var_slots: HashMap<u32, u32> = HashMap::new();
        for name in names {
            let slot = match self.state.var_slots.get(name) {
                Some(&s) => s,
                None => bail!("zmienna (idx {}) nie ma jeszcze slotu — JIT pomija", name),
            };
            let val = self.state.vars_flat.get(slot as usize).copied().unwrap_or(NanVal::nil());
            if !val.is_num() {
                bail!("zmienna (idx {}) nie jest obecnie liczbą — JIT pomija (bezpieczny fallback)", name);
            }
            var_slots.insert(*name, slot);
        }
        Ok(var_slots)
    }

    /// Zbierz wszystkie nazwane zmienne (GetVar/SetVar) odwoływane w zakresie
    /// instrukcji [start..end] (WŁĄCZNIE z `end`).
    fn named_vars_in_range_inclusive(&self, start: u32, end: u32) -> std::collections::HashSet<u32> {
        let mut names = std::collections::HashSet::new();
        for i in start..=end {
            if let Some(insn) = self.module.instructions.get(i as usize) {
                match insn {
                    Instruction::GetVar { name, .. } | Instruction::SetVar { name, .. } => {
                        names.insert(*name);
                    }
                    _ => {}
                }
            }
        }
        names
    }

    /// Próbuj skompilować trasę pętli [start..end] do kodu maszynowego.
    /// Bezpieczeństwo: patrz `collect_verified_var_slots`.
    fn try_compile_trace(&mut self, start: u32, end: u32) -> Result<CompiledTrace> {
        let entry = hl_compiler::bytecode::FuncEntry {
            name:       format!("__trace_{}_{}", start, end),
            start_insn: start,
            insn_count: end - start + 1,
        };

        let names = self.named_vars_in_range_inclusive(start, end);
        let var_slots = self.collect_verified_var_slots(&names)?;

        let engine = self.jit_engine.as_mut().ok_or_else(|| anyhow::anyhow!("silnik JIT niedostępny"))?;
        crate::jit_engine::compile_trace_entry(engine, self.module, &entry, &var_slots)
    }

    /// Próbuj skompilować CAŁE ciało funkcji `entry` do kodu maszynowego
    /// (whole-function JIT — patrz `compile_function_entry` w
    /// jit_engine.rs), ewentualnie z jednym wklejonym wywołaniem funkcji
    /// (patrz `inline` — wynik `audit_for_inlining`, WCZEŚNIEJ obliczony i
    /// scache'owany przez wołającego w `func_eligible`).
    ///
    /// Bezpieczeństwo: sama kwalifikacja instrukcji (i miejsca do wklejenia,
    /// jeśli jest) jest już zagwarantowana przez `audit_for_inlining`
    /// (sprawdzone wcześniej w `exec_func_by_name_idx`); tutaj dodatkowo
    /// weryfikujemy zmienne — identycznie jak dla tras, ale gdy `inline`
    /// jest `Some`, zakres skanowania obejmuje TAKŻE ciało wklejanego
    /// callee (patrz `named_vars_in_range_inclusive` wywołane dla jego
    /// własnego zakresu) — inaczej JIT mógłby błędnie założyć, że zmienna
    /// używana WYŁĄCZNIE przez callee jest bezpieczna, nigdy jej nie
    /// sprawdziwszy.
    fn try_compile_function(
        &mut self,
        entry: &FuncEntry,
        inline: Option<&(u32, FuncEntry)>,
    ) -> Result<CompiledTrace> {
        let start = entry.start_insn;
        let end_excl = entry.start_insn + entry.insn_count;
        let mut names = if entry.insn_count == 0 {
            std::collections::HashSet::new()
        } else {
            self.named_vars_in_range_inclusive(start, end_excl - 1)
        };
        if let Some((_, callee)) = inline {
            if callee.insn_count > 0 {
                let callee_end_excl = callee.start_insn + callee.insn_count;
                names.extend(self.named_vars_in_range_inclusive(callee.start_insn, callee_end_excl - 1));
            }
        }
        let var_slots = self.collect_verified_var_slots(&names)?;

        let engine = self.jit_engine.as_mut().ok_or_else(|| anyhow::anyhow!("silnik JIT niedostępny"))?;
        crate::jit_engine::compile_function_entry(engine, self.module, entry, &var_slots, inline)
    }

    // ── Dispatch instrukcji ───────────────────────────────────────────────────

    fn exec_insn(&mut self, pc: usize) -> Result<ExecSignal> {
        let insn = match self.module.instructions.get(pc) {
            Some(i) => i.clone(),
            None    => return Ok(ExecSignal::Return),
        };

        // Dispatch — Rust kompilator generuje jump table dla gęstego match
        // Używamy jawnego match zamiast fn ptr table bo Rust optymalizuje to dobrze
        match insn {
            Instruction::Nop | Instruction::SourceLine { .. } => Ok(ExecSignal::Next),

            // ── Ładowanie stałych ─────────────────────────────────────────────
            Instruction::LoadStr { dst, idx } => {
                let s   = self.module.consts.strings.get(idx as usize).map(|s| s.as_str()).unwrap_or("");
                let val = self.state.intern_str(s);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::LoadNum { dst, idx } => {
                let n = self.module.consts.numbers.get(idx as usize).copied().unwrap_or(0.0);
                self.state.set_reg(dst, NanVal::num(n));
                Ok(ExecSignal::Next)
            }
            Instruction::LoadBool { dst, val } => {
                self.state.set_reg(dst, NanVal::bool(val));
                Ok(ExecSignal::Next)
            }
            Instruction::LoadNil { dst } => {
                self.state.set_reg(dst, NanVal::nil());
                Ok(ExecSignal::Next)
            }

            // ── Zmienne ───────────────────────────────────────────────────────
            Instruction::GetVar { dst, name } => {
                // Inline cache hot path — O(1)
                let val = self.state.get_var(name);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            // GetVarDyn: @{arg@_i} — dynamiczna nazwa zmiennej z rejestru
            // Rejestr `name` zawiera string (nazwę zmiennej), np. "arg0".
            // Rozdzielamy borrow: najpierw kopiujemy string (owned), potem internujemy i robimy lookup.
            Instruction::GetVarDyn { dst, name } => {
                // 1. Pobierz wartość rejestru name → string (owned, by uniknąć borrow conflict)
                let name_str: String = {
                    let name_val = self.state.get_reg(name);
                    name_val.to_str_val(&self.state.interner)
                };
                // 2. Intern string → idx (wymaga &mut interner — osobny blok)
                let name_idx = self.state.interner.intern(&name_str);
                // 3. Lookup zmiennej przez idx
                let val = self.state.get_var(name_idx);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::SetVar { name, src } => {
                let val = self.state.get_reg(src);
                self.state.set_var(name, val);
                // Synchronizuj last_exit jeśli to _last_exit_code
                let le_name = self.const_str_idx("_last_exit_code");
                if name == le_name {
                    self.state.last_exit = val.as_f64() as i32;
                }
                Ok(ExecSignal::Next)
            }
            Instruction::SetEnv { name, src } => {
                let val = self.state.get_reg(src);
                self.state.export_var(name, val);
                Ok(ExecSignal::Next)
            }

            // ── Arytmetyka — bezpośrednio na f64, zero alokacji ───────────────
            Instruction::Add { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() + self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Sub { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() - self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Mul { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() * self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }
            Instruction::Div { dst, a, b } => {
                let va = self.state.get_reg(a).as_f64();
                let vb = self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::num(if vb == 0.0 { 0.0 } else { va / vb }));
                Ok(ExecSignal::Next)
            }
            Instruction::Mod { dst, a, b } => {
                let va = self.state.get_reg(a).as_f64() as i64;
                let vb = self.state.get_reg(b).as_f64() as i64;
                let r  = if vb == 0 { 0 } else { va % vb };
                self.state.set_reg(dst, NanVal::num(r as f64));
                Ok(ExecSignal::Next)
            }
            Instruction::Neg { dst, src } => {
                let r = -self.state.get_reg(src).as_f64();
                self.state.set_reg(dst, NanVal::num(r));
                Ok(ExecSignal::Next)
            }

            // ── Porównania — fast path dla liczb ─────────────────────────────
            Instruction::CmpEq { dst, a, b } => {
                let va = self.state.get_reg(a);
                let vb = self.state.get_reg(b);
                self.state.set_reg(dst, NanVal::bool(va.eq_val(&vb, &self.state.interner)));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpNe { dst, a, b } => {
                let va = self.state.get_reg(a);
                let vb = self.state.get_reg(b);
                self.state.set_reg(dst, NanVal::bool(!va.eq_val(&vb, &self.state.interner)));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpLt { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() < self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpLe { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() <= self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpGt { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() > self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }
            Instruction::CmpGe { dst, a, b } => {
                let r = self.state.get_reg(a).as_f64() >= self.state.get_reg(b).as_f64();
                self.state.set_reg(dst, NanVal::bool(r));
                Ok(ExecSignal::Next)
            }

            // ── Konwersje ─────────────────────────────────────────────────────
            Instruction::ToString { dst, src } => {
                let s   = self.state.get_reg(src).to_str_val(&self.state.interner);
                let val = self.state.intern_str_owned(s);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }
            Instruction::ToNumber { dst, src } => {
                let n = self.state.get_reg(src).as_f64();
                self.state.set_reg(dst, NanVal::num(n));
                Ok(ExecSignal::Next)
            }
            Instruction::Truthy { dst, src } => {
                let val = self.state.get_reg(src);
                let b   = match val.as_str_idx() {
                    Some(idx) => {
                        // Warunek while — ewaluuj wyrażenie porównania
                        let s = self.state.interner.get(idx).to_string();
                        eval_condition_str(&s, &mut self.state)
                    }
                    None => val.is_truthy(&self.state.interner),
                };
                self.state.set_reg(dst, NanVal::bool(b));
                Ok(ExecSignal::Next)
            }

            // ── Concat — internuje wynik ──────────────────────────────────────
            Instruction::Concat { dst, parts } => {
                let mut buf = String::new();
                for &r in &parts {
                    let s = self.state.get_reg(r).to_str_val(&self.state.interner);
                    buf.push_str(&s);
                }
                let val = self.state.intern_str_owned(buf);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }

            // ── Output ────────────────────────────────────────────────────────
            Instruction::Print { src } => {
                println!("{}", self.state.get_reg(src).to_str_val(&self.state.interner));
                Ok(ExecSignal::Next)
            }

            // ── Sterowanie ────────────────────────────────────────────────────
            Instruction::JumpIfFalse { cond, offset } => {
                if !self.state.get_reg(cond).is_truthy(&self.state.interner) {
                    Ok(ExecSignal::Jump(offset))
                } else {
                    Ok(ExecSignal::Next)
                }
            }
            Instruction::JumpIfTrue { cond, offset } => {
                if self.state.get_reg(cond).is_truthy(&self.state.interner) {
                    Ok(ExecSignal::Jump(offset))
                } else {
                    Ok(ExecSignal::Next)
                }
            }
            Instruction::Jump { offset } => Ok(ExecSignal::Jump(offset)),
            Instruction::Return { .. }   => Ok(ExecSignal::Return),

            // ── Wywołania ─────────────────────────────────────────────────────
            Instruction::CallFunc { name } => Ok(ExecSignal::FuncCall(name)),

            Instruction::CallQuick { name, arg, dst } => {
                let arg_str  = self.state.get_reg(arg).to_str_val(&self.state.interner);
                let name_str = self.const_str(name);
                let result   = exec_quick_fn(&name_str, &arg_str, &mut self.state);
                let val      = self.state.intern_str_owned(result);
                self.state.set_reg(dst, val);
                Ok(ExecSignal::Next)
            }

            // ── Komendy systemowe ─────────────────────────────────────────────
            Instruction::ExecCmd { cmd, mode, dst } => {
                let cmd_str   = self.state.get_reg(cmd).to_str_val(&self.state.interner);
                let exit_code = exec_system_cmd(&cmd_str, mode, &mut self.state)?;
                self.state.set_reg(dst, NanVal::num(exit_code as f64));
                self.state.last_exit = exit_code;
                // Ustaw _last_exit_code w zmiennych
                let le_idx = self.state.interner.intern("_last_exit_code");
                self.state.set_var(le_idx, NanVal::num(exit_code as f64));
                Ok(ExecSignal::Next)
            }

            Instruction::ExecCapture { cmd, mode, dst_ec, dst_out } => {
                let cmd_str = self.state.get_reg(cmd).to_str_val(&self.state.interner);
                let (exit_code, stdout) = exec_system_cmd_capture(&cmd_str, mode)?;
                self.state.set_reg(dst_ec, NanVal::num(exit_code as f64));
                let out_val = self.state.intern_str_owned(stdout);
                self.state.set_reg(dst_out, out_val);
                self.state.last_exit = exit_code;
                Ok(ExecSignal::Next)
            }

            // ── For-in ────────────────────────────────────────────────────────
            Instruction::ForInStart { iter_reg, src } => {
                let src_str = self.state.get_reg(src).to_str_val(&self.state.interner);
                // Intern każde słowo — szybsze porównania w pętli
                let words: Vec<u32> = src_str.split_whitespace()
                .map(|w| self.state.interner.intern(w))
                .collect();
                self.state.iters.insert(iter_reg, (words, 0));
                Ok(ExecSignal::Next)
            }

            Instruction::ForInNext { iter_reg, dst, end_off } => {
                let should_jump = if let Some((words, idx)) = self.state.iters.get_mut(&iter_reg) {
                    if *idx >= words.len() {
                        true
                    } else {
                        let word_idx = words[*idx];
                        *idx += 1;
                        self.state.set_reg(dst, NanVal::str_interned(word_idx));
                        false
                    }
                } else { true };

                if should_jump {
                    self.state.iters.remove(&iter_reg);
                    Ok(ExecSignal::Jump(end_off))
                } else {
                    Ok(ExecSignal::Next)
                }
            }

            // ── HackerOS API ──────────────────────────────────────────────────
            Instruction::HackerOsCall { tool, args, dst } => {
                let tool_str = self.const_str(tool);
                let args_str = self.state.get_reg(args).to_str_val(&self.state.interner);
                let cmd = if args_str.is_empty() {
                    tool_str.clone()
                } else {
                    format!("{} {}", tool_str, args_str)
                };
                if which::which(&tool_str).is_err() {
                    eprintln!("\x1b[33m[hl ||]\x1b[0m Narzędzie '{}' nie jest zainstalowane.", tool_str);
                    self.state.set_reg(dst, NanVal::num(127.0));
                } else {
                    let ec = exec_system_cmd(&cmd, CmdMode::Plain, &mut self.state)?;
                    self.state.set_reg(dst, NanVal::num(ec as f64));
                }
                Ok(ExecSignal::Next)
            }
        }
    }

    fn exec_func_by_name_idx(&mut self, name_idx: u32) -> Result<()> {
        self.state.check_call_depth()?;
        let idx = name_idx as usize;

        // ── Whole-function JIT: funkcja już skompilowana → wykonaj natywnie ──
        // Nie trzeba tu ruszać call_depth: funkcje kwalifikujące się do JIT
        // (patrz is_jit_eligible) z definicji nie zawierają CallFunc, więc
        // wykonanie natywne nigdy nie rekurencyjnie woła z powrotem
        // exec_func_by_name_idx — nie ma czego liczyć. Dostęp O(1) przez
        // Vec — to sprawdzenie wykonuje się na KAŻDE wywołanie funkcji.
        if let Some(trace) = self.compiled_funcs.get(idx).and_then(|o| o.as_ref()) {
            let fn_ptr = trace.fn_ptr;
            self.exec_native_fn_ptr(fn_ptr)?;
            return Ok(());
        }

        let name = self.const_str(name_idx);
        let entry = match self.module.funcs.find(&name) {
            Some(e) => e.clone(),
            None    => bail!("Niezdefiniowana funkcja: '{}'", name),
        };

        // ── Whole-function JIT: śledzenie gorących funkcji ──────────────
        // Uzupełnienie Trace JIT (który kompiluje tylko GORĄCE PĘTLE
        // wewnątrz funkcji): funkcja czysto obliczeniowa wołana wiele razy
        // z top-level, bez żadnej gorącej pętli w środku, wcześniej nigdy
        // nie trafiała do JIT. Kwalifikacja używa `audit_for_inlining` w
        // jit_engine.rs — jak dotychczasowe `is_jit_eligible`, ale
        // DODATKOWO rozpoznaje jedno bezpieczne miejsce do wklejenia
        // wywołania funkcji (patrz doc `InlineAudit`) — liczymy ją raz per
        // funkcja (cache w `func_eligible`), bo zbiór instrukcji funkcji
        // się nie zmienia między wywołaniami.
        let blocked = self.func_jit_blocked.get(idx).copied().unwrap_or(true);
        if self.jit_engine.is_some() && !blocked {
            let audit = match self.func_eligible.get(idx).and_then(|o| o.clone()) {
                Some(a) => a,
                None => {
                    let a = crate::jit_engine::audit_for_inlining(self.module, &entry);
                    if let Some(slot) = self.func_eligible.get_mut(idx) { *slot = Some(a.clone()); }
                    a
                }
            };
            let inline = match audit {
                crate::jit_engine::InlineAudit::Ineligible => {
                    // Strukturalne — nigdy się nie zmieni. Blokuj trwale bez
                    // liczenia prób ponownych (patrz MAX_JIT_RETRIES).
                    if let Some(slot) = self.func_jit_blocked.get_mut(idx) { *slot = true; }
                    None
                }
                crate::jit_engine::InlineAudit::Eligible { inline } => inline,
            };
            if !self.func_jit_blocked.get(idx).copied().unwrap_or(true) {
                let count = self.func_call_counts.get_mut(idx).map(|c| { *c += 1; *c }).unwrap_or(0);
                if count == self.func_threshold {
                    match self.try_compile_function(&entry, inline.as_ref()) {
                        Ok(trace) => {
                            if inline.is_some() {
                                tracing::debug!("[func jit] skompilowano funkcję '{}' (z wklejonym wywołaniem)", name);
                            } else {
                                tracing::debug!("[func jit] skompilowano funkcję '{}'", name);
                            }
                            let fn_ptr = trace.fn_ptr;
                            if let Some(slot) = self.compiled_funcs.get_mut(idx) { *slot = Some(trace); }
                            self.exec_native_fn_ptr(fn_ptr)?;
                            return Ok(());
                        }
                        Err(e) => {
                            tracing::debug!("[func jit] pominięto funkcję '{}': {}", name, e);
                            // Niepowodzenie DYNAMICZNE (np. zmienna używana
                            // przez funkcję — lub przez wklejane callee —
                            // wciąż nie jest stabilnie numeryczna) może się
                            // naprawić samo w kolejnych wywołaniach — dajemy
                            // ograniczoną liczbę ponownych prób (reset
                            // licznika wywołań) zamiast trwale się poddawać
                            // po pierwszej porażce.
                            let retries_left = self.func_retry_count.get(idx).copied().unwrap_or(u8::MAX);
                            if retries_left < MAX_JIT_RETRIES {
                                if let Some(r) = self.func_retry_count.get_mut(idx) { *r += 1; }
                                if let Some(c) = self.func_call_counts.get_mut(idx) { *c = 0; }
                            } else {
                                if let Some(slot) = self.func_jit_blocked.get_mut(idx) { *slot = true; }
                            }
                        }
                    }
                }
            }
        }

        // ── Ścieżka interpretowana (rozgrzewka albo trwały fallback) ────
        self.state.call_depth += 1;
        let start = entry.start_insn as usize;
        let end   = start + entry.insn_count as usize;
        let mut pc = start;
        loop {
            if pc >= end { break; }
            match self.exec_insn(pc)? {
                ExecSignal::Next             => pc += 1,
                ExecSignal::Jump(off)        => pc = off as usize,
                ExecSignal::Return           => break,
                ExecSignal::Exit(code)       => { self.state.last_exit = code; break; }
                ExecSignal::FuncCall(ni)     => { self.exec_func_by_name_idx(ni)?; pc += 1; }
            }
        }
        self.state.call_depth -= 1;
        Ok(())
    }

    #[inline]
    fn const_str(&self, idx: u32) -> String {
        self.module.consts.strings.get(idx as usize).cloned().unwrap_or_default()
    }

    #[inline]
    fn const_str_idx(&mut self, s: &str) -> u32 {
        self.state.interner.intern(s)
    }
}

// ── Komendy systemowe ─────────────────────────────────────────────────────────

fn exec_system_cmd(cmd: &str, mode: CmdMode, _state: &mut RuntimeState) -> Result<i32> {
    // Specjalne prefiksy z lowera
    if let Some(path) = cmd.strip_prefix("__hl_import__ ") {
        // Import w czasie wykonania — placeholder, obsługiwany przez tree-walk executor
        // JIT nie ładuje importów bezpośrednio (brak hl_core w zależnościach jit crate)
        tracing::debug!("[jit import] pomijam: {}", path);
        return Ok(0);
    }
    if let Some(rest) = cmd.strip_prefix("& ") {
        let _ = Command::new("sh").args(["-c", rest])
        .stdin(Stdio::null()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .spawn();
        return Ok(0);
    }

    // `cd` MUSI być obsłużone jako wbudowane, w BIEŻĄCYM procesie hl —
    // nigdy przez spawnowanie zewnętrznego programu:
    //   1. "cd" nie istnieje jako samodzielny plik wykonywalny na dysku —
    //      próba `Command::new("cd")` kończy się ENOENT ("No such file or
    //      directory"), mimo że polecenie jest z punktu widzenia użytkownika
    //      całkowicie poprawne.
    //   2. Nawet gdyby "cd" istniało jako binarka, zmiana katalogu roboczego
    //      SPAWNOWANEGO procesu i tak nigdy nie przetrwałaby do KOLEJNEJ
    //      linii `> ...` w skrypcie — każda taka linia to OSOBNY proces,
    //      dziedziczący cwd macierzystego procesu hl (niezmieniony), a nie
    //      cwd poprzedniej komendy. Trwała zmiana katalogu obejmująca CAŁY
    //      dalszy ciąg skryptu wymaga `std::env::set_current_dir` wykonanego
    //      TU, w tym samym procesie co interpreter — dokładnie tak, jak
    //      powłoki (bash/zsh/...) traktują `cd` jako komendę wbudowaną, a
    //      nie zewnętrzny program, z tego samego powodu.
    // Ograniczone do Plain/WithVars — Sudo/Isolated uruchamiają polecenie w
    // innym kontekście uprawnień/przestrzeni nazw, gdzie "zmień katalog w
    // procesie hl" nie ma tego samego znaczenia; te tryby nadal idą starą
    // ścieżką (spawn), tak jak dotychczas.
    if matches!(mode, CmdMode::Plain | CmdMode::WithVars) {
        if let Some(target) = parse_cd_builtin(cmd) {
            return exec_cd_builtin(&target);
        }
    }

    let (prog, args, needs_sh) = build_cmd_parts(cmd, mode);
    let status = if needs_sh {
        Command::new("sh").args(["-c", cmd])
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status()
    } else {
        Command::new(&prog).args(&args)
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status()
    };

    match status {
        Ok(s)  => Ok(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Błąd komendy: {}", e);
            Ok(1)
        }
    }
}

/// Rozpoznaj `cd` / `cd <ścieżka>` jako CAŁĄ komendę (nie fragment większej,
/// np. `cd foo && bar` — to zawiera `&`, więc `build_cmd_parts` i tak
/// skierowałby je przez `sh -c`, gdzie `cd` DZIAŁA poprawnie jako builtin
/// powłoki; specjalnie obsługujemy tu tylko przypadek, który inaczej
/// próbowałby spawnować nieistniejący plik "cd"). Zwraca docelową ścieżkę —
/// pusty string oznacza `cd` bez argumentu, czyli $HOME (jak w bash).
fn parse_cd_builtin(cmd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    if trimmed == "cd" {
        return Some(String::new());
    }
    trimmed.strip_prefix("cd ").map(|rest| rest.trim().to_string())
}

fn exec_cd_builtin(target: &str) -> Result<i32> {
    let path = if target.is_empty() {
        match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => {
                eprintln!("\x1b[31m[hl jit]\x1b[0m cd: brak zmiennej środowiskowej HOME");
                return Ok(1);
            }
        }
    } else {
        target.to_string()
    };
    match std::env::set_current_dir(&path) {
        Ok(())  => Ok(0),
        Err(e)  => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m cd: {}: {}", path, e);
            Ok(1)
        }
    }
}

fn exec_system_cmd_capture(cmd: &str, mode: CmdMode) -> Result<(i32, String)> {
    let (prog, args, needs_sh) = build_cmd_parts(cmd, mode);
    let out = if needs_sh {
        Command::new("sh").args(["-c", cmd])
        .stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .output()
    } else {
        Command::new(&prog).args(&args)
        .stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .output()
    };
    match out {
        Ok(o)  => Ok((o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stdout).trim().to_string())),
        Err(e) => { eprintln!("\x1b[31m[hl jit]\x1b[0m Capture error: {}", e); Ok((1, String::new())) }
    }
}

fn build_cmd_parts(cmd: &str, mode: CmdMode) -> (String, Vec<String>, bool) {
    let needs_sh = cmd.contains('|') || cmd.contains(';') || cmd.contains('&')
    || cmd.contains('>') || cmd.contains('<') || cmd.contains('$') || cmd.contains('`')
    || cmd.contains('*') || cmd.contains('~');

    match mode {
        CmdMode::Sudo | CmdMode::WithVarsSudo => {
            if needs_sh {
                ("sudo".into(), vec!["sh".into(), "-c".into(), cmd.into()], false)
            } else {
                let parts = split_cmd(cmd);
                ("sudo".into(), parts, false)
            }
        }
        CmdMode::Isolated | CmdMode::WithVarsIsolated => {
            let a = vec!["--mount","--pid","--net","--fork","--","sh","-c",cmd]
            .into_iter().map(|s| s.to_string()).collect();
            ("unshare".into(), a, false)
        }
        CmdMode::IsolatedSudo => {
            let a = vec!["unshare","--mount","--pid","--net","--fork","--","sh","-c",cmd]
            .into_iter().map(|s| s.to_string()).collect();
            ("sudo".into(), a, false)
        }
        _ => {
            if needs_sh {
                (String::new(), vec![], true) // caller uses sh -c
            } else {
                let mut parts = split_cmd(cmd);
                let prog = if parts.is_empty() { String::new() } else { parts.remove(0) };
                (prog, parts, false)
            }
        }
    }
}

fn split_cmd(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_sq = false;
    let mut in_dq = false;
    for c in cmd.chars() {
        match c {
            '\'' if !in_dq => in_sq = !in_sq,
            '"'  if !in_sq => in_dq = !in_dq,
            ' ' | '\t' if !in_sq && !in_dq => {
                if !cur.is_empty() { parts.push(std::mem::take(&mut cur)); }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() { parts.push(cur); }
    parts
}

// ── Quick functions ───────────────────────────────────────────────────────────

fn exec_quick_fn(name: &str, arg: &str, state: &mut RuntimeState) -> String {
    match name {
        "upper"    => arg.to_uppercase(),
        "lower"    => arg.to_lowercase(),
        "len"      => arg.len().to_string(),
        "trim"     => arg.trim().to_string(),
        "rev"      => arg.chars().rev().collect(),
        "abs"      => arg.parse::<f64>().unwrap_or(0.0).abs().to_string(),
        "ceil"     => arg.parse::<f64>().unwrap_or(0.0).ceil().to_string(),
        "floor"    => arg.parse::<f64>().unwrap_or(0.0).floor().to_string(),
        "round"    => arg.parse::<f64>().unwrap_or(0.0).round().to_string(),
        "basename" => std::path::Path::new(arg).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string(),
        "dirname"  => std::path::Path::new(arg).parent().and_then(|p| p.to_str()).unwrap_or(".").to_string(),
        "pid"      => { println!("{}", std::process::id()); String::new() }
        "nl"       => { println!(); String::new() }
        "hr"       => {
            let w: usize = arg.parse().unwrap_or(60);
            println!("{}", "─".repeat(w)); String::new()
        }
        "bold"     => { println!("\x1b[1m{}\x1b[0m", arg); String::new() }
        "red"      => { println!("\x1b[31m{}\x1b[0m", arg); String::new() }
        "green"    => { println!("\x1b[32m{}\x1b[0m", arg); String::new() }
        "yellow"   => { println!("\x1b[33m{}\x1b[0m", arg); String::new() }
        "cyan"     => { println!("\x1b[36m{}\x1b[0m", arg); String::new() }
        "exists"   => {
            let e = std::path::Path::new(arg).exists();
            let k = state.interner.intern("_last_bool");
            state.set_var(k, NanVal::bool(e));
            if !e { String::from("false") } else { String::new() }
        }
        "isdir"    => { let e = std::path::Path::new(arg).is_dir();  e.to_string() }
        "isfile"   => { let e = std::path::Path::new(arg).is_file(); e.to_string() }
        "which"    => which::which(arg).map(|p| p.display().to_string()).unwrap_or_default(),
        "env" | "getenv" => std::env::var(arg).unwrap_or_default(),
        // ::env-path — ścieżka aktywnego środowiska z config.hk, zero subprocess
        "env-path" => {
            use hl_core::config::get_active_env;
            get_active_env()
                .map(|(_n, p)| p.display().to_string())
                .unwrap_or_default()
        }
        "read"     => std::fs::read_to_string(arg).unwrap_or_default(),
        "set"      => {
            if let Some((name, val)) = arg.splitn(2, ' ').collect::<Vec<_>>().as_slice().split_first() {
                let k = state.interner.intern(name);
                let v = state.intern_str(val.first().copied().unwrap_or(""));
                state.set_var(k, v);
            }
            String::new()
        }
        "get"      => {
            let k = state.interner.intern(arg);
            state.get_var(k).to_str_val(&state.interner)
        }
        "unset"    => {
            let k = state.interner.intern(arg);
            state.var_cache.invalidate(k);
            state.var_slots.remove(&k);
            String::new()
        }
        "rand"     => {
            let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64;
            let r = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) % 100;
            r.to_string()
        }
        "date"     => { let o = Command::new("date").arg("+%Y-%m-%d").output().ok(); o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default() }
        "time"     => { let o = Command::new("date").arg("+%H:%M:%S").output().ok(); o.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default() }
        _          => {
            eprintln!("\x1b[31m[hl jit]\x1b[0m Nieznana quick-funkcja '::{}'", name);
            String::new()
        }
    }
}

// ── Ewaluacja warunków while ──────────────────────────────────────────────────

fn eval_condition_str(cond: &str, state: &mut RuntimeState) -> bool {
    let cond = cond.trim();
    if cond.is_empty() { return false; }
    if cond == "true"  { return true;  }
    if cond == "false" { return false; }

    const OPS: &[&str] = &["==", "!=", ">=", "<=", ">", "<"];
    for op in OPS {
        if let Some(pos) = find_op(cond, op) {
            let left_raw  = cond[..pos].trim();
            let right_raw = cond[pos + op.len()..].trim().trim_matches('"');

            let lv = if let Some(name) = left_raw.strip_prefix('@') {
                let k = state.interner.intern(name);
                state.get_var(k).to_str_val(&state.interner)
            } else {
                left_raw.to_string()
            };

            return match *op {
                "==" => lv == right_raw,
                "!=" => lv != right_raw,
                ">=" => lv.parse::<f64>().unwrap_or(0.0) >= right_raw.parse::<f64>().unwrap_or(0.0),
                "<=" => lv.parse::<f64>().unwrap_or(0.0) <= right_raw.parse::<f64>().unwrap_or(0.0),
                ">"  => lv.parse::<f64>().unwrap_or(0.0) >  right_raw.parse::<f64>().unwrap_or(0.0),
                "<"  => lv.parse::<f64>().unwrap_or(0.0) <  right_raw.parse::<f64>().unwrap_or(0.0),
                _    => false,
            };
        }
    }

    // Fallback shell
    Command::new("sh").args(["-c", cond]).status().map(|s| s.success()).unwrap_or(false)
}

fn find_op(s: &str, op: &str) -> Option<usize> {
    let b = s.as_bytes(); let ob = op.as_bytes(); let ol = ob.len();
    let mut i = 0;
    while i + ol <= b.len() {
        if &b[i..i+ol] == ob {
            let ok = match op {
                ">" => i + 1 >= b.len() || b[i+1] != b'=',
                "<" => i + 1 >= b.len() || b[i+1] != b'=',
                _   => true,
            };
            if ok { return Some(i); }
        }
        i += 1;
    }
    None
}
