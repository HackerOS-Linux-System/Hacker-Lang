use anyhow::{bail, Result};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    types, AbiParam, Block, Function, InstBuilder, MemFlags, Signature, Type, UserFuncName, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::{settings, Context};
use cranelift_codegen::settings::Configurable;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module};
use hl_compiler::bytecode::*;
use std::collections::{HashMap, HashSet};

/// Górny limit rozmiaru CIAŁA FUNKCJI (w instrukcjach) dla whole-function
/// JIT (patrz `compile_function_entry` niżej). Trasy pętli (Trace JIT) mają
/// osobny, mniejszy limit pilnowany przez `interpreter.rs` (pętle są
/// zazwyczaj gorące i małe — kompilowanie ich całych jest tanie i szybko
/// się zwraca). Funkcje bywają większe, ale wciąż chcemy uniknąć
/// wielosekundowej kompilacji Cranelift dla patologicznie dużego ciała.
pub const MAX_FUNC_JIT_INSNS: u32 = 512;

/// Wszystkie skompilowane fragmenty (trasy I całe funkcje) mają TĘ SAMĄ
/// sygnaturę ABI — pięć parametrów, jeden wynik:
///   fn(regs: *mut u64, vars: *mut u64, reg_count: u32, var_count: u32,
///      link_table: *const usize) -> i32
/// `link_table` to wskaźnik do tablicy (o długości = liczba instrukcji w
/// module) wypełnianej przez interpreter: `link_table[i] != 0` oznacza
/// "pod offsetem `i` jest już skompilowana natywnie TRASA — jej wskaźnik
/// funkcji to `link_table[i]`". Whole-function JIT dostaje ten parametr
/// identycznie (dla jednorodności ABI — jeden typ wskaźnika funkcji wszędzie
/// w całym module), ale NIGDY go nie odczytuje w generowanym kodzie (patrz
/// `Ctx::enable_link_table` i "Trace linking" niżej) — chaining ma sens
/// tylko między trasami pętli (offset = konkretne miejsce w strumieniu
/// bytecode, na które można "wrócić"), nie między wywołaniami funkcji
/// (offset zwracany przez whole-function JIT to zawsze koniec ciała
/// funkcji, którego wołający i tak nie używa jako celu skoku).
pub type CompiledFnPtr = unsafe extern "C" fn(*mut u64, *mut u64, u32, u32, *const usize) -> i32;

/// Skompilowany JIT fragment — wskaźnik do kodu maszynowego.
pub struct JitFragment {
    pub fn_ptr: CompiledFnPtr,
}

/// Menedżer JIT — właściciel JEDNEGO trwałego `JITModule` używanego przez
/// CAŁY czas życia interpretera, dla WSZYSTKICH kompilowanych fragmentów
/// (zarówno tras pętli, jak i całych funkcji — patrz `compile_trace_entry`
/// / `compile_function_entry` niżej).
///
/// Wcześniejsza wersja tworzyła NOWY `JITModule` (a więc też nowe
/// wyszukiwanie ISA i nowy `JITBuilder`) przy KAŻDEJ pojedynczej kompilacji,
/// po czym `std::mem::forget`owała go, żeby wygenerowany kod maszynowy
/// przeżył poza zakresem funkcji — to był i czysty wyciek pamięci (moduł
/// nigdy nie jest zwalniany), i marnowanie czasu (ISA/JITBuilder budowane od
/// zera za każdym razem). Trzymając jeden `JitEngine` w `BytecodeInterpreter`
/// przez cały czas wykonania skryptu:
///   - ISA/JITBuilder budowane są RAZ, nie za każdym skompilowanym hotspotem,
///   - zero wycieku — moduł jest poprawnie zwalniany razem z interpreterem,
///   - kolejne kompilacje są szybsze (JITModule nie odtwarza swojej
///     wewnętrznej księgowości od zera),
///   - generowany kod używa `opt_level=speed`, więc sam backend Cranelift
///     emituje lepszej jakości kod maszynowy niż domyślne `opt_level=none`.
pub struct JitEngine {
    module: JITModule,
    /// Kontekst kompilacji Cranelift, WIELOKROTNEGO UŻYTKU między
    /// kompilacjami (czyszczony przez `ctx.clear()` przed każdym użyciem —
    /// to jest udokumentowany, zalecany wzorzec Cranelift dla JIT-ów, które
    /// kompilują wiele fragmentów w czasie życia jednego procesu). Unika
    /// ponownej alokacji wewnętrznych struktur IR przy KAŻDYM pojedynczym
    /// skompilowanym fragmencie — ma to szczególne znaczenie odkąd trasy i
    /// funkcje mogą być próbowane kilkukrotnie (retry-with-backoff).
    ctx:    Context,
    /// Analogicznie: kontekst budowniczego funkcji Cranelift, też
    /// wielokrotnego użytku — `FunctionBuilder::new` sam go czyści/
    /// przygotowuje przy każdym wywołaniu.
    fn_ctx: FunctionBuilderContext,
    /// Liczba fragmentów skompilowanych w tym uruchomieniu (statystyki,
    /// patrz `HL_JIT_STATS` w interpreter.rs/runner.rs).
    pub compiled_count: u32,
    /// Suma instrukcji bytecode pokrytych przez skompilowane fragmenty.
    pub compiled_insns: u32,
}

impl JitEngine {
    /// Zbuduj silnik JIT: wykryj natywne ISA i zainicjalizuj trwały
    /// `JITModule`. Może się nie powieść na platformach bez wsparcia
    /// Cranelift dla natywnej architektury — wołający (interpreter.rs)
    /// traktuje to jako sygnał do bezpiecznego działania w trybie czysto
    /// interpretowanym (JIT wyłączony), nie jako błąd fatalny.
    pub fn new() -> Result<Self> {
        let mut flag_builder = settings::builder();
        // opt_level=speed: Cranelift stosuje pełny zestaw optymalizacji
        // backendu przy emisji kodu maszynowego, zamiast domyślnego "none".
        // Dowód bezpieczeństwa tego modułu dotyczy DOBORU instrukcji
        // dopuszczonych do kompilacji (patrz `is_jit_eligible`/
        // `audit_for_inlining`), nie zachowania samego backendu.
        flag_builder
            .set("opt_level", "speed")
            .map_err(|e| anyhow::anyhow!("nie można ustawić opt_level=speed: {}", e))?;
        let flags = settings::Flags::new(flag_builder);

        let isa = cranelift_native::builder()
            .map_err(|e| anyhow::anyhow!("Brak ISA: {}", e))?
            .finish(flags)?;

        let jit_builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        let module = JITModule::new(jit_builder);

        Ok(Self {
            module,
            ctx:    Context::new(),
            fn_ctx: FunctionBuilderContext::new(),
            compiled_count: 0,
            compiled_insns: 0,
        })
    }

    /// Zbuduj sygnaturę WSPÓLNĄ dla wszystkich skompilowanych fragmentów —
    /// patrz doc `CompiledFnPtr`.
    fn shared_signature(&self) -> Signature {
        let mut sig = Signature::new(CallConv::SystemV);
        let ptr_type = self.module.target_config().pointer_type();
        sig.params.push(AbiParam::new(ptr_type));    // regs
        sig.params.push(AbiParam::new(ptr_type));    // vars
        sig.params.push(AbiParam::new(types::I32));  // reg_count
        sig.params.push(AbiParam::new(types::I32));  // var_count
        sig.params.push(AbiParam::new(ptr_type));    // link_table
        sig.returns.push(AbiParam::new(types::I32));
        sig
    }

    /// Skompiluj JEDEN fragment (trasa pętli LUB całe ciało funkcji, ew.
    /// z jednym wklejonym wywołaniem — patrz `inline`) do natywnego kodu
    /// maszynowego w trwałym module tego silnika.
    ///
    /// `logical_name` służy wyłącznie do generowania unikalnej nazwy symbolu
    /// wewnątrz `JITModule` (i do logów) — łączymy go z monotonicznym
    /// licznikiem, żeby dwie kompilacje o tej samej logicznej nazwie nigdy
    /// nie kolidowały w przestrzeni nazw modułu.
    ///
    /// `var_slots`: name_idx → slot w vars_flat, WCZEŚNIEJ rozwiązany i
    /// zweryfikowany jako bezpieczny przez wołającego (interpreter.rs) —
    /// dla WSZYSTKICH zmiennych używanych zarówno w głównym ciele, jak i w
    /// ewentualnym wklejonym callee.
    ///
    /// `enable_link_table`: czy wygenerowany kod ma przy KAŻDYM wyjściu
    /// sprawdzać tablicę linków i doskakiwać natywnie do innej już
    /// skompilowanej trasy zamiast wracać do interpretera (patrz "Trace
    /// linking" przy `emit_store_and_return`). Prawda TYLKO dla tras.
    ///
    /// `inline`: `Some((offset_wywołania, callee))`, gdy w ciele jest
    /// dokładnie jedno bezpieczne do wklejenia `CallFunc` (patrz
    /// `audit_for_inlining`) — `None` w przeciwnym razie.
    fn compile(
        &mut self,
        logical_name: &str,
        module_bc: &HlModule,
        entry: &FuncEntry,
        var_slots: &HashMap<u32, u32>,
        enable_link_table: bool,
        inline: Option<&(u32, FuncEntry)>,
    ) -> Result<JitFragment> {
        let unique_name = format!("{}__{}", logical_name, self.compiled_count);
        let sig = self.shared_signature();
        let ptr_type = self.module.target_config().pointer_type();

        let func_id = self.module.declare_function(&unique_name, Linkage::Local, &sig)?;

        // Wyczyść (NIE zaalokuj od nowa) trwały kontekst kompilacji
        // pozostały po poprzednim wywołaniu `compile` — patrz doc `ctx`
        // wyżej przy definicji struktury.
        self.ctx.clear();
        self.ctx.func = Function::with_name_signature(UserFuncName::user(0, 0), sig.clone());

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.fn_ctx);
            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);

            let params       = builder.block_params(entry_block);
            let regs_ptr     = params[0];
            let vars_ptr     = params[1];
            let reg_count_val = params[2];
            let var_count_val = params[3];
            let link_table_ptr = params[4];

            compile_func_body(
                &mut builder, module_bc, entry,
                regs_ptr, vars_ptr, reg_count_val, var_count_val, link_table_ptr,
                ptr_type, enable_link_table,
                var_slots, inline,
                entry_block,
            )?;

            // seal_all_blocks() zamiast ręcznego seal_block per blok — udokumentowana
            // metoda Cranelift dla przypadków z cyklami (pętle wstecz), gdzie
            // sekwencyjne sealowanie w trakcie tłumaczenia jest niepraktyczne.
            builder.seal_all_blocks();
            builder.finalize();
        }

        self.module.define_function(func_id, &mut self.ctx)?;
        // Bezpieczne wołanie wielokrotne: finalizuje WSZYSTKIE dotąd
        // zdefiniowane-a-niezfinalizowane funkcje w module, więc kolejne
        // kompilacje w tym samym trwałym module poprawnie się kumulują.
        self.module.finalize_definitions()?;

        let fn_ptr = self.module.get_finalized_function(func_id);
        let fn_typed: CompiledFnPtr = unsafe { std::mem::transmute(fn_ptr) };

        self.compiled_count += 1;
        self.compiled_insns += entry.insn_count;

        Ok(JitFragment { fn_ptr: fn_typed })
    }
}

/// Sprawdź czy blok instrukcji kwalifikuje się do JIT.
/// Kwalifikuje się: czysta arytmetyka + ładowanie stałych + SetVar/GetVar +
/// porównania + kontrola przepływu. NIGDY `LoadStr`/`ExecCmd`/`CallQuick`/
/// `CallFunc`/itp. — to jest fundament dowodu bezpieczeństwa opisanego w
/// doku modułu.
///
/// `pub(crate)`: używane przez `is_trace_safe` (Trace JIT), przez
/// `audit_for_inlining` (whole-function JIT — jako reguła dla CALLEE, który
/// ma być wklejony: musi być w pełni jit-eligible SAM W SOBIE, a więc bez
/// CallFunc — to jest właśnie to, co gwarantuje, że inlining nigdy nie
/// tworzy łańcuchów ani cykli, patrz `audit_for_inlining`), i bezpośrednio
/// przez `interpreter.rs::exec_func_by_name_idx` dla funkcji BEZ żadnego
/// CallFunc (najczęstszy przypadek — ścieżka bez inliningu).
pub(crate) fn is_jit_eligible(module: &HlModule, entry: &FuncEntry) -> bool {
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;

    for insn in module.instructions.get(start..end).unwrap_or(&[]) {
        match insn {
            Instruction::LoadNum { .. } |
            Instruction::LoadBool { .. } |
            Instruction::LoadNil { .. } |
            Instruction::GetVar { .. } |
            Instruction::SetVar { .. } |
            Instruction::Add { .. } |
            Instruction::Sub { .. } |
            Instruction::Mul { .. } |
            Instruction::Div { .. } |
            Instruction::Mod { .. } |
            Instruction::Neg { .. } |
            Instruction::CmpEq { .. } |
            Instruction::CmpNe { .. } |
            Instruction::CmpLt { .. } |
            Instruction::CmpLe { .. } |
            Instruction::CmpGt { .. } |
            Instruction::CmpGe { .. } |
            Instruction::ToNumber { .. } |
            Instruction::Jump { .. } |
            Instruction::JumpIfFalse { .. } |
            Instruction::JumpIfTrue { .. } |
            Instruction::Return { .. } |
            Instruction::Nop => {}

            _ => return false,
        }
    }
    true
}

/// Dodatkowa reguła bezpieczeństwa TYLKO dla tras (nie whole-function JIT):
/// `Return` w ŚRODKU trasy pętli (nie jako jej ostatnia instrukcja) oznacza
/// wyjście z całej otaczającej funkcji, nie tylko z pętli — to wymagałoby
/// osobnej obsługi sygnału powrotu, której ten kompilator nie implementuje.
/// Zamiast ryzykować złą semantykę, po prostu odrzucamy taką trasę.
///
/// Ta sama reguła jest też wymaganiem na CALLEE przy inliningu (patrz
/// `audit_for_inlining`) — z dokładnie tego samego powodu: wklejane ciało
/// musi mieć JEDEN, KOŃCOWY punkt powrotu, żeby dało się go bezpiecznie
/// przełożyć na zwykły skok do miejsca "zaraz po wywołaniu".
pub fn is_trace_safe(module: &HlModule, entry: &FuncEntry) -> bool {
    if !is_jit_eligible(module, entry) { return false; }
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;
    let insns = match module.instructions.get(start..end) { Some(s) => s, None => return false };
    for (i, insn) in insns.iter().enumerate() {
        if matches!(insn, Instruction::Return { .. }) && i != insns.len() - 1 {
            return false;
        }
    }
    true
}

/// Wynik audytu ciała funkcji pod kątem whole-function JIT Z INLININGIEM.
#[derive(Clone)]
pub(crate) enum InlineAudit {
    /// Funkcja NIE kwalifikuje się do whole-function JIT w ogóle —
    /// dokładnie jak dotychczasowe samo `is_jit_eligible`: niebezpieczna
    /// instrukcja, albo `CallFunc`, którego nie da się bezpiecznie wkleić
    /// (więcej niż jedno wystąpienie, nieznana/niekwalifikująca się funkcja
    /// docelowa, albo wywołanie samej siebie).
    Ineligible,
    /// Kwalifikuje się. `inline` to `Some((offset_wywołania, callee))`, gdy
    /// w ciele jest DOKŁADNIE JEDNO `CallFunc` nadające się do wklejenia —
    /// `None`, gdy w ogóle nie ma CallFunc (zachowanie identyczne jak przed
    /// wprowadzeniem inliningu).
    Eligible { inline: Option<(u32, FuncEntry)> },
}

/// Rozszerzona reguła kwalifikacji dla whole-function JIT: jak
/// `is_jit_eligible`, ale DODATKOWO pozwala na DOKŁADNIE JEDNO wystąpienie
/// `CallFunc` w ciele funkcji, pod warunkiem że wywoływana funkcja:
///   1. istnieje i daje się rozwiązać w tym samym module,
///   2. NIE jest samą funkcją `entry` (obrona w głąb — punkt 3 by to i tak
///      wykluczył, bo `entry` z definicji zawiera to samo CallFunc, więc
///      nie mogłaby przejść `is_trace_safe`/`is_jit_eligible`),
///   3. spełnia `is_trace_safe` — czyli jest SAMA W SOBIE w pełni
///      jit-eligible (a więc z DEFINICJI nie zawiera ŻADNEGO CallFunc) i ma
///      dokładnie jeden, końcowy `Return`.
///
/// Punkt 3 jest kluczowy dla bezpieczeństwa: skoro wklejana funkcja
/// (callee) sama nigdy nie zawiera CallFunc, inlining jest z góry
/// ograniczony do JEDNEGO POZIOMU głębokości — nie ma możliwości łańcucha
/// A→B→C ani cyklu A→B→A, więc NIE trzeba wykrywać cykli ani ograniczać
/// głębokości rekursji osobnym licznikiem: sama reguła to gwarantuje z
/// definicji, statycznie, bez potrzeby przechodzenia grafu wywołań.
///
/// Ograniczenie do JEDNEGO miejsca wywołania (nie więcej) upraszcza
/// generowanie kodu: ciało callee jest wklejane raz, z jednym miejscem
/// powrotu (patrz `Ctx::inline_call_off`/`inline_callee_range` w
/// `compile_func_body`) — obsługa wielu niezależnych miejsc wklejenia tej
/// samej (lub różnych) funkcji w jednej trasie wymagałaby osobnych kopii
/// bloków Cranelift per miejsce wywołania, co znacznie zwiększa złożoność
/// bez proporcjonalnej korzyści: najczęstszy i najbardziej wartościowy
/// wzorzec — wywołanie pomocniczej funkcji WEWNĄTRZ gorącej pętli — to i
/// tak JEDNA instrukcja CallFunc w bytecode, niezależnie od tego, ile razy
/// pętla wykona się w praktyce.
pub(crate) fn audit_for_inlining(module: &HlModule, entry: &FuncEntry) -> InlineAudit {
    let start = entry.start_insn as usize;
    let end   = start + entry.insn_count as usize;
    let insns = match module.instructions.get(start..end) {
        Some(s) => s,
        None => return InlineAudit::Ineligible,
    };

    let mut call_site: Option<(u32, FuncEntry)> = None;

    for (i, insn) in insns.iter().enumerate() {
        match insn {
            Instruction::LoadNum { .. } |
            Instruction::LoadBool { .. } |
            Instruction::LoadNil { .. } |
            Instruction::GetVar { .. } |
            Instruction::SetVar { .. } |
            Instruction::Add { .. } |
            Instruction::Sub { .. } |
            Instruction::Mul { .. } |
            Instruction::Div { .. } |
            Instruction::Mod { .. } |
            Instruction::Neg { .. } |
            Instruction::CmpEq { .. } |
            Instruction::CmpNe { .. } |
            Instruction::CmpLt { .. } |
            Instruction::CmpLe { .. } |
            Instruction::CmpGt { .. } |
            Instruction::CmpGe { .. } |
            Instruction::ToNumber { .. } |
            Instruction::Jump { .. } |
            Instruction::JumpIfFalse { .. } |
            Instruction::JumpIfTrue { .. } |
            Instruction::Return { .. } |
            Instruction::Nop => {}

            Instruction::CallFunc { name } => {
                if call_site.is_some() {
                    return InlineAudit::Ineligible; // więcej niż jedno wywołanie
                }
                let callee_name = match module.consts.strings.get(*name as usize) {
                    Some(s) => s,
                    None => return InlineAudit::Ineligible,
                };
                if callee_name == &entry.name {
                    return InlineAudit::Ineligible; // wywołanie samej siebie
                }
                let callee = match module.funcs.find(callee_name) {
                    Some(e) => e.clone(),
                    None => return InlineAudit::Ineligible,
                };
                if !is_trace_safe(module, &callee) {
                    return InlineAudit::Ineligible; // callee sam się nie kwalifikuje
                }
                call_site = Some((start as u32 + i as u32, callee));
            }

            _ => return InlineAudit::Ineligible,
        }
    }

    InlineAudit::Eligible { inline: call_site }
}

/// Kontekst współdzielony między compile_func_body i compile_insn — jeden
/// obiekt zamiast rosnącej listy parametrów.
struct Ctx<'a> {
    module:      &'a HlModule,
    start:       u32,
    end_excl:    u32,
    reg_vars:    HashMap<u32, Variable>,
    var_vars:    HashMap<u32, Variable>,
    var_slots:   HashMap<u32, u32>,
    blocks:      HashMap<u32, Block>,
    exit_blocks: HashMap<u32, Block>,
    regs_ptr:    Value,
    vars_ptr:    Value,

    // ── Trace linking (patrz `emit_store_and_return`) ──────────────────
    reg_count_val:      Value,
    var_count_val:      Value,
    link_table_ptr:      Value,
    ptr_type:            Type,
    /// Prawda TYLKO dla tras (nigdy dla whole-function JIT) — patrz doc
    /// `CompiledFnPtr`.
    enable_link_table:   bool,

    // ── Inlining pojedynczego CallFunc (patrz `audit_for_inlining`) ────
    /// Offset instrukcji CallFunc, którą wklejamy (jeśli jakaś jest).
    inline_call_off:     Option<u32>,
    /// Zakres [start, end) wklejonego callee (jeśli jakiś jest).
    inline_callee_range: Option<(u32, u32)>,
}

impl<'a> Ctx<'a> {
    /// Offset jest "wewnętrzny" (nie wymaga wyjścia z powrotem do
    /// interpretera), jeśli leży w głównym kompilowanym zakresie ALBO w
    /// zakresie wklejonego callee — oba mają swoje bloki w `self.blocks`.
    fn is_internal(&self, off: u32) -> bool {
        (off >= self.start && off < self.end_excl)
            || self.inline_callee_range.map_or(false, |(cs, ce)| off >= cs && off < ce)
    }
}

/// Zapisz wszystkie śledzone rejestry i zmienne z powrotem do pamięci.
///
/// ## Trace linking
/// Jeśli `ctx.enable_link_table` (tylko trasy pętli — nigdy whole-function
/// JIT, patrz doc `CompiledFnPtr`), przed zwróceniem sterowania do
/// interpretera SPRAWDZAMY w wygenerowanym kodzie, czy pod docelowym
/// offsetem jest już skompilowana natywnie INNA trasa — jeśli tak, wołamy
/// ją PROSTO STĄD (bez przechodzenia przez pętlę dispatchu w Rust) i
/// zwracamy JEJ wynik zamiast wracać do interpretera.
///
/// To sprawdzenie dzieje się w WYGENEROWANYM KODZIE, przy KAŻDYM wyjściu z
/// trasy — dlatego złapie też trasy skompilowane PO tej: nie trzeba niczego
/// łatać/patchować w już wyemitowanym kodzie maszynowym, wystarczy że
/// `interpreter.rs` wpisze wskaźnik nowo skompilowanej trasy do
/// `link_table[jej_start]` — każda trasa, której wyjście akurat tam
/// prowadzi, zobaczy to przy SWOIM następnym uruchomieniu.
///
/// Bezpieczeństwo tego mechanizmu nie wprowadza ŻADNEGO nowego założenia
/// ponad to, co już obowiązuje dla zwykłego "trasa już skompilowana →
/// wykonaj natywnie" w interpreter.rs: chaining po prostu pomija powrót do
/// Rust między dwoma wywołaniami, które i tak by nastąpiły.
fn emit_store_and_return(builder: &mut FunctionBuilder, ctx: &Ctx, offset: u32) {
    // MemFlags::new(): "puste" flagi (bez notrap/aligned/readonly) —
    // najbezpieczniejszy, najbardziej konserwatywny wybór. Świadomie NIE
    // korzystamy z hintów wydajnościowych typu "trusted"/"aligned" — ten
    // dostęp do pamięci jest i tak trywialnie tani (pojedynczy load/store),
    // a błędne założenie o wyrównaniu w kodzie, którego nie da się tu
    // przetestować, to zysk niewart ryzyka.
    for (&reg_idx, &var) in &ctx.reg_vars {
        let val = builder.use_var(var);
        builder.ins().store(MemFlags::new(), val, ctx.regs_ptr, (reg_idx as i32) * 8);
    }
    for (&name_idx, &var) in &ctx.var_vars {
        let slot = ctx.var_slots[&name_idx];
        let val  = builder.use_var(var);
        builder.ins().store(MemFlags::new(), val, ctx.vars_ptr, (slot as i32) * 8);
    }

    if ctx.enable_link_table {
        let ptr_bytes = ctx.ptr_type.bytes() as i64;
        let entry_addr = builder.ins().iadd_imm(ctx.link_table_ptr, (offset as i64) * ptr_bytes);
        let target = builder.ins().load(ctx.ptr_type, MemFlags::new(), entry_addr, 0);
        let zero = builder.ins().iconst(ctx.ptr_type, 0);
        let has_link = builder.ins().icmp(IntCC::NotEqual, target, zero);

        let call_block  = builder.create_block();
        let plain_block = builder.create_block();
        builder.ins().brif(has_link, call_block, &[], plain_block, &[]);

        builder.switch_to_block(call_block);
        let mut callee_sig = Signature::new(CallConv::SystemV);
        callee_sig.params.push(AbiParam::new(ctx.ptr_type));
        callee_sig.params.push(AbiParam::new(ctx.ptr_type));
        callee_sig.params.push(AbiParam::new(types::I32));
        callee_sig.params.push(AbiParam::new(types::I32));
        callee_sig.params.push(AbiParam::new(ctx.ptr_type));
        callee_sig.returns.push(AbiParam::new(types::I32));
        let sig_ref = builder.import_signature(callee_sig);
        let call = builder.ins().call_indirect(
            sig_ref, target,
            &[ctx.regs_ptr, ctx.vars_ptr, ctx.reg_count_val, ctx.var_count_val, ctx.link_table_ptr],
        );
        let result = builder.inst_results(call)[0];
        builder.ins().return_(&[result]);

        builder.switch_to_block(plain_block);
        // Spadamy do zwykłego "return offset" poniżej — plain_block jest
        // teraz bieżącym blokiem, więc kod poniżej dopisuje się do niego.
    }

    let off_val = builder.ins().iconst(types::I32, offset as i64);
    builder.ins().return_(&[off_val]);
}

/// Zwróć (tworząc przy pierwszym użyciu) blok-stub dla ZEWNĘTRZNEGO celu —
/// blok, który tylko zapisuje stan i zwraca `offset`, bez dalszej logiki.
fn get_or_make_exit_block(builder: &mut FunctionBuilder, ctx: &mut Ctx, offset: u32) -> Block {
    if let Some(&b) = ctx.exit_blocks.get(&offset) { return b; }
    let b = builder.create_block();
    ctx.exit_blocks.insert(offset, b);
    let saved = builder.current_block();
    builder.switch_to_block(b);
    emit_store_and_return(builder, ctx, offset);
    if let Some(prev) = saved { builder.switch_to_block(prev); }
    b
}

/// Rejestry użyte przez zakres instrukcji `[start, start+insns.len())` oraz
/// wewnętrzne cele skoków w tym zakresie (dopisywane do `block_starts`,
/// przekazanego przez wołającego, bo jeden zbiór obejmuje ciało główne I
/// ewentualny wklejony callee).
fn collect_regs_and_blocks(
    insns: &[Instruction],
    start: u32,
    end_excl: u32,
    reg_set: &mut HashSet<u32>,
    block_starts: &mut HashSet<u32>,
) {
    for (i, insn) in insns.iter().enumerate() {
        let off = start + i as u32;
        for r in insn_regs(insn) { reg_set.insert(r); }
        match insn {
            Instruction::Jump { offset } => {
                if *offset >= start && *offset < end_excl { block_starts.insert(*offset); }
            }
            Instruction::JumpIfFalse { offset, .. } | Instruction::JumpIfTrue { offset, .. } => {
                if *offset >= start && *offset < end_excl { block_starts.insert(*offset); }
                if off + 1 < end_excl { block_starts.insert(off + 1); }
            }
            _ => {}
        }
    }
}

/// Zbiór rejestrów, które w BLOKU WEJŚCIOWYM (entry_block — jedyny możliwy
/// punkt wejścia do skompilowanej funkcji) są zapisywane (jako dst) ZANIM
/// zostaną odczytane (jako src) — dla takich rejestrów początkowa wartość
/// z pamięci jest z definicji niezauważalna (zawsze nadpisana, zanim
/// ktokolwiek ją przeczyta), więc pomijamy kosztowny `load` na starcie i
/// podstawiamy tanią stałą 0.0 zamiast niego.
///
/// ## Dowód bezpieczeństwa
/// entry_block jest JEDYNYM punktem wejścia do skompilowanej funkcji —
/// interpreter wywołuje skompilowany kod WYŁĄCZNIE przez jego wskaźnik
/// funkcji (zawsze zaczyna się w entry_block), a WEWNĄTRZ funkcji jedyna
/// droga do jakiegokolwiek innego bloku prowadzi PRZEZ entry_block: żaden
/// skok, ani zewnętrzny (interpreter nigdy nie wskakuje w środek
/// skompilowanego kodu), ani wewnętrzny skok wsteczny pętli (dla trasy
/// pętli entry_block to WŁAŚNIE cel skoku wstecznego — wykonuje się więc na
/// KAŻDEJ iteracji, nie tylko raz), nie omija entry_block.
///
/// entry_block to pojedynczy blok bazowy — kończy się dopiero na pierwszym
/// terminatorze (kolejnym `block_starts`) — więc "zapis przed odczytem w
/// kolejności programu w TYM JEDNYM bloku" oznacza zapis BEZWARUNKOWY, który
/// zawsze wykona się przed jakimkolwiek możliwym odczytem gdziekolwiek w
/// całej funkcji.
///
/// Analiza jest CELOWO ograniczona do jednego, prostego przebiegu liniowego
/// (bez ogólnego dataflow z punktem stałym po grafie z cyklami) — to
/// wystarcza, żeby bezpiecznie (i w sposób łatwy do zweryfikowania przez
/// inspekcję) złapać najbardziej wartościowy i najczęstszy przypadek:
/// rejestry pomocnicze/tymczasowe przeliczane na nowo na starcie KAŻDEJ
/// iteracji pętli, zanim zostaną gdziekolwiek odczytane.
///
/// Siatka bezpieczeństwa: nawet gdyby ten dowód miał defekt, `def_var`
/// dostaje tanią stałą 0.0 zamiast pozostawienia zmiennej SSA
/// niezdefiniowaną — skutkiem błędu byłby co najwyżej zły WYNIK, nigdy
/// odczyt niezainicjalizowanej zmiennej ani cofnięcie się do
/// niezdefiniowanego zachowania.
///
/// Celowo NIE stosowane do zmiennych HL (`var_vars`/GetVar/SetVar) — te
/// reprezentują stan na poziomie programu, nie lokalne rejestry robocze;
/// margines bezpieczeństwa dla nich zostaje węższy, więc zostawiamy je
/// zawsze ładowane, tak jak dotychczas.
fn entry_block_write_before_read_regs(
    insns: &[Instruction],
    start: u32,
    block_starts: &HashSet<u32>,
) -> HashSet<u32> {
    let mut written_first: HashSet<u32> = HashSet::new();
    let mut disqualified: HashSet<u32> = HashSet::new();
    for (i, insn) in insns.iter().enumerate() {
        let off = start + i as u32;
        if off != start && block_starts.contains(&off) {
            break; // opuściliśmy blok wejściowy — koniec analizy
        }
        let (srcs, dst) = insn_src_dst(insn);
        for s in &srcs {
            if !written_first.contains(s) {
                disqualified.insert(*s);
            }
        }
        if let Some(d) = dst {
            if !disqualified.contains(&d) {
                written_first.insert(d);
            }
        }
    }
    written_first
}

/// Kompiluj ciało trasy/funkcji do IR Cranelift z prawdziwym CFG — patrz
/// `Ctx` dla wyjaśnienia pól i `audit_for_inlining` dla `inline`.
#[allow(clippy::too_many_arguments)]
fn compile_func_body(
    builder: &mut FunctionBuilder,
    module: &HlModule,
    entry: &FuncEntry,
    regs_ptr: Value,
    vars_ptr: Value,
    reg_count_val: Value,
    var_count_val: Value,
    link_table_ptr: Value,
    ptr_type: Type,
    enable_link_table: bool,
    var_slots: &HashMap<u32, u32>,
    inline: Option<&(u32, FuncEntry)>,
    entry_block: Block,
) -> Result<()> {
    let start = entry.start_insn;
    let end_excl = entry.start_insn + entry.insn_count;
    let insns = match module.instructions.get(start as usize..end_excl as usize) {
        Some(s) => s,
        None    => bail!("Nieprawidłowy zakres instrukcji"),
    };

    // Zakres wklejonego callee, jeśli jakiś jest (patrz `audit_for_inlining`
    // — bezpieczeństwo tego zostało już zweryfikowane PRZED wywołaniem tej
    // funkcji, tu tylko konsumujemy wynik).
    let callee_range: Option<(u32, u32)> = inline.map(|(_, callee)| {
        (callee.start_insn, callee.start_insn + callee.insn_count)
    });
    let callee_insns: &[Instruction] = match callee_range {
        Some((cs, ce)) => module.instructions.get(cs as usize..ce as usize).unwrap_or(&[]),
        None => &[],
    };

    let mut reg_set: HashSet<u32> = HashSet::new();
    let mut block_starts: HashSet<u32> = HashSet::new();
    block_starts.insert(start);

    collect_regs_and_blocks(insns, start, end_excl, &mut reg_set, &mut block_starts);
    // Miejsce wywołania (jeśli jest) MUSI mieć osobny blok od "off+1" —
    // Return wklejonego callee będzie tam skakał bezpośrednio, więc granica
    // bloku jest wymagana niezależnie od tego, czy coś INNEGO też tam skacze.
    if let Some((call_off, _)) = inline {
        if call_off + 1 < end_excl { block_starts.insert(call_off + 1); }
    }
    if let Some((cs, ce)) = callee_range {
        block_starts.insert(cs);
        collect_regs_and_blocks(callee_insns, cs, ce, &mut reg_set, &mut block_starts);
    }

    let mut blocks: HashMap<u32, Block> = HashMap::new();
    blocks.insert(start, entry_block);
    for &off in &block_starts {
        if off == start { continue; }
        blocks.insert(off, builder.create_block());
    }

    let mut reg_vars: HashMap<u32, Variable> = HashMap::new();
    for &reg in &reg_set {
        let var = builder.declare_var(types::F64);
        reg_vars.insert(reg, var);
    }
    let mut var_vars: HashMap<u32, Variable> = HashMap::new();
    for (&name_idx, _slot) in var_slots {
        let var = builder.declare_var(types::F64);
        var_vars.insert(name_idx, var);
    }

    let skip_load_regs = entry_block_write_before_read_regs(insns, start, &block_starts);
    for (&reg_idx, &var) in &reg_vars {
        let val = if skip_load_regs.contains(&reg_idx) {
            builder.ins().f64const(0.0)
        } else {
            builder.ins().load(types::F64, MemFlags::new(), regs_ptr, (reg_idx as i32) * 8)
        };
        builder.def_var(var, val);
    }
    for (&name_idx, &var) in &var_vars {
        let slot = var_slots[&name_idx];
        let val  = builder.ins().load(types::F64, MemFlags::new(), vars_ptr, (slot as i32) * 8);
        builder.def_var(var, val);
    }

    let mut ctx = Ctx {
        module, start, end_excl,
        reg_vars, var_vars,
        var_slots: var_slots.clone(),
        blocks,
        exit_blocks: HashMap::new(),
        regs_ptr, vars_ptr,
        reg_count_val, var_count_val, link_table_ptr, ptr_type, enable_link_table,
        inline_call_off: inline.map(|(off, _)| *off),
        inline_callee_range: callee_range,
    };

    // ── Ciało główne ─────────────────────────────────────────────────────
    let mut last_was_terminator = false;
    for (i, insn) in insns.iter().enumerate() {
        let off = start + i as u32;
        if off != start && block_starts.contains(&off) {
            if !last_was_terminator {
                let target = ctx.blocks[&off];
                builder.ins().jump(target, &[]);
            }
            builder.switch_to_block(ctx.blocks[&off]);
        }
        last_was_terminator = compile_insn(builder, insn, off, &mut ctx)?;
    }
    if !last_was_terminator {
        emit_store_and_return(builder, &ctx, end_excl);
    }

    // ── Ciało wklejonego callee, jeśli jest ─────────────────────────────
    // Jego `Return` przekierowuje się na blok kontynuacji ciała głównego
    // (patrz `ctx.inline_call_off`/`inline_callee_range` w `compile_insn`),
    // NIE na wyjście z całej skompilowanej funkcji.
    if let Some((cs, ce)) = callee_range {
        // KRYTYCZNE: musimy jawnie przełączyć się na blok wejściowy callee —
        // w przeciwieństwie do ciała głównego (gdzie `entry_block` jest już
        // bieżącym blokiem builder-a od momentu wywołania `compile()`), tutaj
        // builder wciąż wskazuje na OSTATNI blok ciała głównego (już
        // zaterminowany przez skok/return powyżej). Bez tego przełączenia
        // pierwsza instrukcja callee trafiłaby do bloku, który ma już
        // terminator — nieprawidłowy IR.
        builder.switch_to_block(ctx.blocks[&cs]);
        let mut last_was_terminator = false;
        for (i, insn) in callee_insns.iter().enumerate() {
            let off = cs + i as u32;
            if off != cs && block_starts.contains(&off) {
                if !last_was_terminator {
                    let target = ctx.blocks[&off];
                    builder.ins().jump(target, &[]);
                }
                builder.switch_to_block(ctx.blocks[&off]);
            }
            last_was_terminator = compile_insn(builder, insn, off, &mut ctx)?;
        }
        // `is_trace_safe` (sprawdzone w `audit_for_inlining`) zagwarantował,
        // że callee kończy się DOKŁADNIE instrukcją Return — więc powyższa
        // pętla zawsze kończy się terminatorem. Fallback niżej to czysta
        // obrona w głąb, nie oczekiwana ścieżka.
        if !last_was_terminator {
            emit_store_and_return(builder, &ctx, ce);
        }
    }

    Ok(())
}

/// Zapisz wynik porównania (i8 z fcmp) jako f64 0.0/1.0 do rejestru docelowego.
fn store_bool_result(builder: &mut FunctionBuilder, ctx: &Ctx, dst: u32, cmp: Value) {
    let one_f  = builder.ins().f64const(1.0);
    let zero_f = builder.ins().f64const(0.0);
    let r      = builder.ins().select(cmp, one_f, zero_f);
    if let Some(&v) = ctx.reg_vars.get(&dst) { builder.def_var(v, r); }
}

/// Kompiluje pojedynczą instrukcję. Zwraca `true`, jeśli wyemitowała
/// terminator bloku (jump/brif/return) — wołający MUSI wtedy przełączyć się
/// na kolejny blok przed kompilacją następnej instrukcji.
fn compile_insn(builder: &mut FunctionBuilder, insn: &Instruction, off: u32, ctx: &mut Ctx) -> Result<bool> {
    macro_rules! gv {
        ($r:expr) => {
            if let Some(&v) = ctx.reg_vars.get(&$r) { builder.use_var(v) }
            else { builder.ins().f64const(0.0) }
        };
    }
    macro_rules! dv {
        ($r:expr, $val:expr) => {
            if let Some(&v) = ctx.reg_vars.get(&$r) { builder.def_var(v, $val); }
        };
    }

    match insn {
        Instruction::LoadNum { dst, idx } => {
            let n   = ctx.module.consts.numbers.get(*idx as usize).copied().unwrap_or(0.0);
            let val = builder.ins().f64const(n);
            dv!(*dst, val);
            Ok(false)
        }
        Instruction::LoadBool { dst, val } => {
            let n = if *val { 1.0f64 } else { 0.0f64 };
            let v = builder.ins().f64const(n);
            dv!(*dst, v);
            Ok(false)
        }
        Instruction::LoadNil { dst } => {
            let v = builder.ins().f64const(0.0);
            dv!(*dst, v);
            Ok(false)
        }

        Instruction::GetVar { dst, name } => {
            if let Some(&var) = ctx.var_vars.get(name) {
                let val = builder.use_var(var);
                dv!(*dst, val);
            } else {
                let v = builder.ins().f64const(0.0);
                dv!(*dst, v);
            }
            Ok(false)
        }
        Instruction::SetVar { name, src } => {
            if let Some(&var) = ctx.var_vars.get(name) {
                let v = gv!(*src);
                builder.def_var(var, v);
            }
            Ok(false)
        }

        Instruction::Add { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fadd(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Sub { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fsub(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Mul { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let r=builder.ins().fmul(va,vb); dv!(*dst,r); Ok(false) }
        Instruction::Div { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let zero = builder.ins().f64const(0.0);
            let is_zero = builder.ins().fcmp(FloatCC::Equal, vb, zero);
            let div     = builder.ins().fdiv(va, vb);
            let r       = builder.ins().select(is_zero, zero, div);
            dv!(*dst, r);
            Ok(false)
        }
        Instruction::Mod { dst, a, b } => {
            let va = gv!(*a); let vb = gv!(*b);
            let ia = builder.ins().fcvt_to_sint_sat(types::I64, va);
            let ib = builder.ins().fcvt_to_sint_sat(types::I64, vb);
            let is_zero = builder.ins().icmp_imm(IntCC::Equal, ib, 0);
            let one_i   = builder.ins().iconst(types::I64, 1);
            let safe_ib = builder.ins().select(is_zero, one_i, ib);
            let rem     = builder.ins().srem(ia, safe_ib);
            let zero_i  = builder.ins().iconst(types::I64, 0);
            let rem_or_zero = builder.ins().select(is_zero, zero_i, rem);
            let r = builder.ins().fcvt_from_sint(types::F64, rem_or_zero);
            dv!(*dst, r);
            Ok(false)
        }
        Instruction::Neg { dst, src } => { let v=gv!(*src); let r=builder.ins().fneg(v); dv!(*dst,r); Ok(false) }

        Instruction::CmpEq { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::Equal, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpNe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::NotEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpLt { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::LessThan, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpLe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::LessThanOrEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpGt { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::GreaterThan, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }
        Instruction::CmpGe { dst, a, b } => { let va=gv!(*a); let vb=gv!(*b); let c=builder.ins().fcmp(FloatCC::GreaterThanOrEqual, va, vb); store_bool_result(builder, ctx, *dst, c); Ok(false) }

        Instruction::ToNumber { dst, src } => { let v=gv!(*src); dv!(*dst,v); Ok(false) }

        Instruction::Nop => Ok(false),

        Instruction::Jump { offset } => {
            let target = resolve_block(builder, ctx, *offset);
            builder.ins().jump(target, &[]);
            Ok(true)
        }
        Instruction::JumpIfFalse { cond, offset } => {
            let cond_f = gv!(*cond);
            let zero = builder.ins().f64const(0.0);
            let cond_bool = builder.ins().fcmp(FloatCC::NotEqual, cond_f, zero);
            let fallthrough_off = off + 1;
            let true_block  = resolve_block(builder, ctx, fallthrough_off);
            let false_block = resolve_block(builder, ctx, *offset);
            builder.ins().brif(cond_bool, true_block, &[], false_block, &[]);
            Ok(true)
        }
        Instruction::JumpIfTrue { cond, offset } => {
            let cond_f = gv!(*cond);
            let zero = builder.ins().f64const(0.0);
            let cond_bool = builder.ins().fcmp(FloatCC::NotEqual, cond_f, zero);
            let fallthrough_off = off + 1;
            let true_block  = resolve_block(builder, ctx, *offset);
            let false_block = resolve_block(builder, ctx, fallthrough_off);
            builder.ins().brif(cond_bool, true_block, &[], false_block, &[]);
            Ok(true)
        }
        Instruction::Return { .. } => {
            // Jeśli to KOŃCOWY Return WKLEJONEGO callee (is_trace_safe w
            // audit_for_inlining zagwarantował, że to jego JEDYNY i OSTATNI
            // Return) — "powrót" to zwykły skok do kontynuacji ciała
            // głównego zaraz po miejscu wywołania, NIE wyjście z całej
            // skompilowanej funkcji.
            if let (Some(call_off), Some((cs, ce))) = (ctx.inline_call_off, ctx.inline_callee_range) {
                if off >= cs && off < ce {
                    let cont_off = call_off + 1;
                    let target = match ctx.blocks.get(&cont_off) {
                        Some(&b) => b,
                        None     => get_or_make_exit_block(builder, ctx, cont_off),
                    };
                    builder.ins().jump(target, &[]);
                    return Ok(true);
                }
            }
            emit_store_and_return(builder, ctx, ctx.end_excl);
            Ok(true)
        }

        Instruction::CallFunc { .. } => {
            if let (Some(call_off), Some((cs, _ce))) = (ctx.inline_call_off, ctx.inline_callee_range) {
                if off == call_off {
                    let target = ctx.blocks[&cs];
                    builder.ins().jump(target, &[]);
                    return Ok(true);
                }
            }
            // Nie powinno się zdarzyć: `audit_for_inlining` już wykluczyło
            // CallFunc poza dokładnie jednym, zweryfikowanym miejscu — ale
            // gdyby jednak coś tu trafiło, odmawiamy JAWNIE (błąd kompilacji,
            // bezpieczny fallback na interpretację) zamiast po cichu
            // traktować to jak Nop, co byłoby CICHYM pominięciem realnego
            // wywołania funkcji — dokładnie klasa błędu, przed którą ten
            // moduł ma chronić.
            bail!("CallFunc poza zweryfikowanym miejscem wklejenia — odmowa kompilacji");
        }

        _ => Ok(false),
    }
}

/// Rozwiąż blok docelowy dla danego offsetu: wewnętrzny → istniejący blok
/// z mapy; zewnętrzny → stub zwracający ten offset.
fn resolve_block(builder: &mut FunctionBuilder, ctx: &mut Ctx, off: u32) -> Block {
    if ctx.is_internal(off) {
        ctx.blocks[&off]
    } else {
        get_or_make_exit_block(builder, ctx, off)
    }
}

/// Zbierz wszystkie rejestry użyte w instrukcji (dla deklaracji Variable) —
/// bez rozróżniania roli źródło/cel, patrz `insn_src_dst` gdy rozróżnienie
/// jest potrzebne.
fn insn_regs(insn: &Instruction) -> Vec<u32> {
    match insn {
        Instruction::LoadNum  { dst, .. } => vec![*dst],
        Instruction::LoadBool { dst, .. } => vec![*dst],
        Instruction::LoadNil  { dst }     => vec![*dst],
        Instruction::GetVar    { dst, .. } => vec![*dst],
        Instruction::GetVarDyn { dst, name } => vec![*dst, *name],
        Instruction::SetVar    { src, .. } => vec![*src],
        Instruction::Add { dst, a, b } |
        Instruction::Sub { dst, a, b } |
        Instruction::Mul { dst, a, b } |
        Instruction::Div { dst, a, b } |
        Instruction::Mod { dst, a, b } |
        Instruction::CmpEq { dst, a, b } |
        Instruction::CmpNe { dst, a, b } |
        Instruction::CmpLt { dst, a, b } |
        Instruction::CmpLe { dst, a, b } |
        Instruction::CmpGt { dst, a, b } |
        Instruction::CmpGe { dst, a, b } => vec![*dst, *a, *b],
        Instruction::Neg { dst, src } |
        Instruction::ToString { dst, src } |
        Instruction::ToNumber { dst, src } |
        Instruction::Truthy   { dst, src } => vec![*dst, *src],
        Instruction::JumpIfFalse { cond, .. } |
        Instruction::JumpIfTrue  { cond, .. } => vec![*cond],
        _ => vec![],
    }
}

/// Rozbij rejestry instrukcji na (źródła, cel) — w przeciwieństwie do
/// `insn_regs`, który tylko zbiera WSZYSTKIE zaangażowane rejestry bez
/// rozróżniania roli. Potrzebne wyłącznie przez
/// `entry_block_write_before_read_regs`. Obejmuje TYLKO instrukcje
/// dopuszczone przez `is_jit_eligible`/`audit_for_inlining` — inne nigdy
/// nie trafiają do skompilowanego zakresu.
fn insn_src_dst(insn: &Instruction) -> (Vec<u32>, Option<u32>) {
    match insn {
        Instruction::LoadNum  { dst, .. } => (vec![], Some(*dst)),
        Instruction::LoadBool { dst, .. } => (vec![], Some(*dst)),
        Instruction::LoadNil  { dst }     => (vec![], Some(*dst)),
        Instruction::GetVar    { dst, .. } => (vec![], Some(*dst)),
        Instruction::SetVar    { src, .. } => (vec![*src], None),
        Instruction::Add { a, b, dst } |
        Instruction::Sub { a, b, dst } |
        Instruction::Mul { a, b, dst } |
        Instruction::Div { a, b, dst } |
        Instruction::Mod { a, b, dst } |
        Instruction::CmpEq { a, b, dst } |
        Instruction::CmpNe { a, b, dst } |
        Instruction::CmpLt { a, b, dst } |
        Instruction::CmpLe { a, b, dst } |
        Instruction::CmpGt { a, b, dst } => (vec![*a, *b], Some(*dst)),
        Instruction::CmpGe { a, b, dst } => (vec![*a, *b], Some(*dst)),
        Instruction::Neg { src, dst } |
        Instruction::ToNumber { src, dst } => (vec![*src], Some(*dst)),
        Instruction::JumpIfFalse { cond, .. } |
        Instruction::JumpIfTrue  { cond, .. } => (vec![*cond], None),
        // Return{src} MOŻE czytać rejestr (wartość zwracana) — zgłaszamy to
        // jawnie zamiast polegać na tym, że Return jest zawsze ostatnią
        // osiągalną instrukcją w bloku (co jest prawdą przy obecnym kształcie
        // lower.rs, ale niepotrzebnie kruche założenie do trzymania w
        // milczeniu w kodzie bezpieczeństwa).
        Instruction::Return { src: Some(r) } => (vec![*r], None),
        _ => (vec![], None),
    }
}

// ── compile_trace_entry / compile_function_entry — entry pointy dla JIT ───────
//
// Obie funkcje dzielą ten sam trwały `JitEngine` (przekazany przez `&mut`
// przez wołającego — `interpreter.rs`, który posiada go przez cały czas
// życia `BytecodeInterpreter`) — patrz doc `JitEngine` wyżej. Różnią się
// regułą bezpieczeństwa (`is_trace_safe` vs `audit_for_inlining`) i tym,
// czy generowany kod uczestniczy w trace linkingu (tylko trasy).

/// Skompiluj trasę pętli (Trace JIT). `engine`: trwały silnik JIT właściciela
/// (interpretera) — patrz `JitEngine`.
pub fn compile_trace_entry(
    engine: &mut JitEngine,
    module_bc: &HlModule,
    entry: &FuncEntry,
    var_slots: &HashMap<u32, u32>,
) -> anyhow::Result<crate::interpreter::CompiledTrace> {
    if !is_trace_safe(module_bc, entry) {
        anyhow::bail!("trasa niekwalifikująca się do bezpiecznej kompilacji JIT");
    }
    engine.compile(&entry.name, module_bc, entry, var_slots, /* enable_link_table */ true, /* inline */ None)
        .map(|frag| crate::interpreter::CompiledTrace {
            fn_ptr:      frag.fn_ptr,
            exit_offset: entry.start_insn + entry.insn_count,
        })
}

/// Skompiluj CAŁE ciało funkcji HL do natywnego kodu (whole-function JIT),
/// z ewentualnym wklejeniem jednego bezpiecznego wywołania funkcji — patrz
/// `audit_for_inlining`.
///
/// `inline`: wynik `audit_for_inlining` dla `entry`, WCZEŚNIEJ obliczony
/// przez wołającego (interpreter.rs, który go też cache'uje per funkcja —
/// patrz `func_eligible`), żeby nie liczyć go dwa razy.
pub fn compile_function_entry(
    engine: &mut JitEngine,
    module_bc: &HlModule,
    entry: &FuncEntry,
    var_slots: &HashMap<u32, u32>,
    inline: Option<&(u32, FuncEntry)>,
) -> anyhow::Result<crate::interpreter::CompiledTrace> {
    if entry.insn_count > MAX_FUNC_JIT_INSNS {
        anyhow::bail!(
            "funkcja '{}' zbyt duża do whole-function JIT ({} > {} instrukcji)",
            entry.name, entry.insn_count, MAX_FUNC_JIT_INSNS
        );
    }
    engine.compile(&entry.name, module_bc, entry, var_slots, /* enable_link_table */ false, inline)
        .map(|frag| crate::interpreter::CompiledTrace {
            fn_ptr:      frag.fn_ptr,
            exit_offset: entry.start_insn + entry.insn_count,
        })
}
