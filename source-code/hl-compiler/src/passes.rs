use colored::*;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;
use std::process::exit;

/// Buduje TargetMachine z pełnymi informacjami o CPU hosta.
/// Przekazanie prawdziwego CPU (zamiast pustego "") pozwala LLVM
/// emitować AVX2/AVX-512/BMI2 i inne rozszerzenia — identycznie jak
/// rustc z -C target-cpu=native.
pub fn build_target_machine(opt: u8, pie: bool, verbose: bool) -> TargetMachine {
    let opt_level = match opt {
        0 => OptimizationLevel::None,
        1 => OptimizationLevel::Less,
        3 => OptimizationLevel::Aggressive,
        _ => OptimizationLevel::Default,
    };

    let triple   = TargetMachine::get_default_triple();
    // Pobierz nazwę CPU hosta — np. "znver3", "skylake", "alderlake"
    // Dzięki temu LLVM dobiera właściwe instrukcje (AVX2, BMI2 itd.)
    let cpu      = TargetMachine::get_host_cpu_name();
    // Pobierz pełny zestaw feature'ów hosta (+avx2,+bmi2,+aes,…)
    let features = TargetMachine::get_host_cpu_features();

    if verbose {
        eprintln!(
            "{} Triple:   {}",
            "[i]".blue(),
                  triple.as_str().to_str().unwrap_or("unknown")
        );
        eprintln!(
            "{} CPU:      {}",
            "[i]".blue(),
                  cpu.to_str().unwrap_or("unknown")
        );
        eprintln!(
            "{} Features: {}",
            "[i]".blue(),
                  features.to_str().unwrap_or("unknown")
        );
    }

    let reloc_mode = if pie {
        RelocMode::PIC
    } else {
        RelocMode::Default
    };

    let target = Target::from_triple(&triple).unwrap_or_else(|e| {
        eprintln!("{} Target error: {}", "[x]".red(), e);
        exit(1);
    });

    target
    .create_target_machine(
        &triple,
        cpu.to_str().unwrap_or(""),
                           features.to_str().unwrap_or(""),
                           opt_level,
                           reloc_mode,
                           CodeModel::Default,
    )
    .unwrap_or_else(|| {
        eprintln!("{} Nie można utworzyć TargetMachine", "[x]".red());
        exit(1);
    })
}

/// Uruchamia pełny LLVM pass pipeline — odpowiednik `opt -O3`.
///
/// Kluczowe passy których brakuje w samym OptimizationLevel:
///   • mem2reg          — promocja alloca → SSA registers (eliminuje load/store)
///   • inline           — inlining małych funkcji
///   • instcombine      — algebraiczne uproszczenia IR
///   • reassociate      — przestawienie operacji dla lepszego CSE
///   • gvn              — Global Value Numbering (eliminacja redundancji)
///   • sccp             — Sparse Conditional Constant Propagation
///   • loop-vectorize   — auto-wektoryzacja pętli (SIMD)
///   • slp-vectorize    — SLP vectorizer (skalarne → SIMD)
///   • loop-unroll      — rozwijanie pętli
///   • dce / adce       — eliminacja martwego kodu
///   • tailcallelim     — optymalizacja wywołań ogonowych
///   • simplifycfg      — uproszczenie CFG (merge bloków, fold branches)
///   • licm             — Loop Invariant Code Motion
///   • indvars          — uproszczenie indukcji pętli
pub fn run_passes(module: &Module<'_>, tm: &TargetMachine, opt: u8, verbose: bool) {
    let pipeline = match opt {
        0 => {
            // O0: tylko mem2reg żeby IR był poprawny
            "mem2reg"
        }
        1 => {
            // O1: podstawowe passy bez agresywnego inliningu
            "mem2reg,instcombine,simplifycfg,\
reassociate,gvn,sccp,\
dce,tailcallelim"
        }
        3 => {
            // O3: pełny agresywny pipeline jak rustc release
            "mem2reg,\
always-inline,\
inline,\
instcombine,\
simplifycfg,\
reassociate,\
gvn,\
sccp,\
dce,\
adce,\
tailcallelim,\
licm,\
loop-rotate,\
loop-unroll,\
indvars,\
loop-vectorize,\
slp-vectorize,\
jump-threading,\
correlated-propagation,\
memcpyopt,\
dse,\
instcombine,\
simplifycfg"
        }
        _ => {
            // O2: domyślny balans między czasem kompilacji a wydajnością
            "mem2reg,\
always-inline,\
inline,\
instcombine,\
simplifycfg,\
reassociate,\
gvn,\
sccp,\
dce,\
tailcallelim,\
licm,\
loop-rotate,\
loop-unroll,\
indvars,\
loop-vectorize,\
slp-vectorize,\
dse,\
instcombine,\
simplifycfg"
        }
    };

    if verbose {
        eprintln!(
            "{} Pass pipeline (O{}): {}",
                  "[*]".green(),
                  opt,
                  pipeline
        );
    }

    let opts = PassBuilderOptions::create();

    // Agresywne ustawienia pass managera
    opts.set_verify_each(false);           // brak verify po każdym passie (szybsze)
    opts.set_debug_logging(false);
    opts.set_loop_interleaving(opt >= 2);  // interleaving pętli dla ILP
    opts.set_loop_vectorization(opt >= 2); // auto-wektoryzacja pętli
    opts.set_loop_slp_vectorization(opt >= 2); // SLP vectorizer
    opts.set_loop_unrolling(opt >= 1);     // rozwijanie pętli
    opts.set_merge_functions(opt >= 2);    // mergowanie identycznych funkcji
    opts.set_call_graph_profile(opt >= 2); // profil grafu wywołań dla inlinera

    if let Err(e) = module.run_passes(pipeline, tm, opts) {
        // Nie przerywamy kompilacji — pass może nie być dostępny w tej wersji LLVM
        if verbose {
            eprintln!(
                "{} Pass pipeline warning (niekrytyczny): {}",
                      "[!]".yellow(),
                      e
            );
        }
    } else if verbose {
        eprintln!("{} Optymalizacje zakończone", "[+]".green());
    }
}
