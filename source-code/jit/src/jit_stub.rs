use anyhow::{bail, Result};
use hl_compiler::bytecode::*;
use std::collections::HashMap;

/// Patrz doc w prawdziwym `jit_engine.rs` — tu bez znaczenia (nic się nigdy
/// nie kompiluje), zachowane dla identyczności API.
pub const MAX_FUNC_JIT_INSNS: u32 = 0;

/// Patrz doc `CompiledFnPtr` w prawdziwym `jit_engine.rs`.
pub type CompiledFnPtr = unsafe extern "C" fn(*mut u64, *mut u64, u32, u32, *const usize) -> i32;

pub struct JitFragment {
    pub fn_ptr: CompiledFnPtr,
}

/// Kikut silnika JIT — zawsze odmawia budowy. `interpreter.rs` traktuje to
/// dokładnie tak samo jak brak wsparcia natywnego ISA na prawdziwym
/// backendzie: łapie błąd, loguje ostrzeżenie i pracuje w trybie czysto
/// interpretowanym.
pub struct JitEngine {
    pub compiled_count: u32,
    pub compiled_insns: u32,
}

impl JitEngine {
    pub fn new() -> Result<Self> {
        bail!(
            "natywny JIT niedostępny w tej konfiguracji (zbudowano bez cechy \
             `native-jit` — np. playground/wasm32); interpreter działa w \
             trybie czysto interpretowanym"
        )
    }
}

/// Patrz doc `InlineAudit` w prawdziwym `jit_engine.rs`.
#[derive(Clone)]
pub(crate) enum InlineAudit {
    Ineligible,
    Eligible { inline: Option<(u32, FuncEntry)> },
}

/// Zawsze `false` — bez silnika JIT nic nigdy nie kwalifikuje się do
/// kompilacji natywnej, więc odpowiedź jest trywialnie poprawna niezależnie
/// od treści `entry`.
pub(crate) fn is_jit_eligible(_module: &HlModule, _entry: &FuncEntry) -> bool {
    false
}

/// Jak wyżej — patrz doc `is_trace_safe` w prawdziwym `jit_engine.rs`.
pub fn is_trace_safe(_module: &HlModule, _entry: &FuncEntry) -> bool {
    false
}

/// Jak wyżej — nigdy nie ma nic do wklejenia, bo nic się nigdy nie
/// kompiluje.
pub(crate) fn audit_for_inlining(_module: &HlModule, _entry: &FuncEntry) -> InlineAudit {
    InlineAudit::Ineligible
}

pub fn compile_trace_entry(
    _engine: &mut JitEngine,
    _module_bc: &HlModule,
    _entry: &FuncEntry,
    _var_slots: &HashMap<u32, u32>,
) -> Result<crate::interpreter::CompiledTrace> {
    bail!("natywny JIT niedostępny w tej konfiguracji")
}

pub fn compile_function_entry(
    _engine: &mut JitEngine,
    _module_bc: &HlModule,
    _entry: &FuncEntry,
    _var_slots: &HashMap<u32, u32>,
    _inline: Option<&(u32, FuncEntry)>,
) -> Result<crate::interpreter::CompiledTrace> {
    bail!("natywny JIT niedostępny w tej konfiguracji")
}
