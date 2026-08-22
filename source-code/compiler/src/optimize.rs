use crate::bytecode::*;
use std::collections::{HashMap, HashSet};

pub fn optimize_module(module: &mut HlModule) {
    pass_constant_folding(module);
    pass_dead_branch_elimination(module);
    // Po dead_branch_elimination (który zamienia znane-stałe JumpIfFalse/
    // JumpIfTrue na Jump) — żeby wątkowanie objęło też te NOWO powstałe
    // bezwarunkowe skoki, nie tylko te napisane wprost w źródle.
    pass_jump_threading(module);
    pass_dead_store_elimination(module);
    pass_nop_elimination(module);
    pass_source_line_strip(module);
    // Deduplacja stałych jest już wbudowana w ConstPool
}

/// Constant folding: dwa LoadNum + Add/Sub/Mul/Div/Mod → jeden LoadNum,
/// dwa LoadNum + porównanie → jeden LoadBool, ToNumber/Truthy ze znanej
/// stałej → LoadNum/LoadBool bezpośrednio (patrz komentarz przy gałęzi
/// `Truthy` niżej — to również poszerza zasięg whole-function/trace JIT,
/// bo `Truthy` sam w sobie nie jest jit-eligible, a `LoadBool` jest).
fn pass_constant_folding(module: &mut HlModule) {
    // Śledź jakie rejestry są wynikiem LoadNum/LoadBool i ich wartości
    // (prosty jednoprzebiegowy model — bez analizy flow)
    use std::collections::HashMap;
    let mut reg_consts: HashMap<Reg, f64> = HashMap::new();
    let mut reg_bools:  HashMap<Reg, bool> = HashMap::new();

    let len = module.instructions.len();
    for i in 0..len {
        match module.instructions[i].clone() {
            Instruction::LoadNum { dst, idx } => {
                let val = module.consts.numbers.get(idx as usize).copied().unwrap_or(0.0);
                reg_consts.insert(dst, val);
                reg_bools.remove(&dst);
            }
            Instruction::LoadBool { dst, val } => {
                reg_bools.insert(dst, val);
                reg_consts.remove(&dst);
            }
            Instruction::Add { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va + vb;
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            Instruction::Sub { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va - vb;
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            Instruction::Mul { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va * vb;
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            Instruction::Div { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = if vb == 0.0 { 0.0 } else { va / vb };
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            Instruction::Mod { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = if vb == 0.0 { 0.0 } else { (va as i64 % vb as i64) as f64 };
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            Instruction::Neg { dst, src } => {
                if let Some(&v) = reg_consts.get(&src) {
                    let result = -v;
                    let new_idx = module.consts.add_num(result);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, result);
                } else {
                    reg_consts.remove(&dst);
                }
            }
            // Porównania liczbowe ze znanymi stałymi → LoadBool (unikamy
            // porównania w runtime, a przy okazji odsłania to kolejne
            // martwe gałęzie dla pass_dead_branch_elimination).
            Instruction::CmpEq { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va == vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            Instruction::CmpNe { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va != vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            Instruction::CmpLt { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va < vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            Instruction::CmpLe { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va <= vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            Instruction::CmpGt { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va > vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            Instruction::CmpGe { dst, a, b } => {
                if let (Some(&va), Some(&vb)) = (reg_consts.get(&a), reg_consts.get(&b)) {
                    let result = va >= vb;
                    module.instructions[i] = Instruction::LoadBool { dst, val: result };
                    reg_bools.insert(dst, result);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            // Peephole: ToString(LoadStr src) → LoadStr src (już jest stringiem)
            Instruction::ToString { dst, src } => {
                // Jeśli src jest znanym num-rejestrem, zamień na formatowanie
                // (nie robimy tutaj — to wymagałoby bardziej skomplikowanej analizy)
                let _ = (dst, src);
                reg_consts.remove(&dst);
                reg_bools.remove(&dst);
            }
            // ToNumber ze znanej stałej liczbowej → LoadNum bezpośrednio
            // (jeden krok zamiast dwóch w interpreterze/JIT).
            Instruction::ToNumber { dst, src } => {
                if let Some(&v) = reg_consts.get(&src) {
                    let new_idx = module.consts.add_num(v);
                    module.instructions[i] = Instruction::LoadNum { dst, idx: new_idx };
                    reg_consts.insert(dst, v);
                    reg_bools.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            // Truthy ze znanej stałej (bool LUB liczbowej) → LoadBool.
            //
            // To NIE jest tylko "jedna instrukcja mniej": `Truthy` wywołuje w
            // interpreterze specjalną ścieżkę dla stringów-warunków
            // (`eval_condition_str`) i dlatego CELOWO nie jest na liście
            // bezpiecznych instrukcji dla JIT (patrz `is_jit_eligible` w
            // jit_engine.rs). Foldując `Truthy` znanej stałej na wprost
            // `LoadBool` — co JEST jit-eligible — poszerzamy zakres pętli i
            // funkcji, które kwalifikują się do kompilacji natywnej, bez
            // zmiany semantyki: src pochodzi tu WYŁĄCZNIE z LoadNum/LoadBool
            // (śledzonych w reg_consts/reg_bools), nigdy ze stringa, więc
            // nie ma ryzyka pominięcia ścieżki eval_condition_str.
            Instruction::Truthy { dst, src } => {
                if let Some(&b) = reg_bools.get(&src) {
                    module.instructions[i] = Instruction::LoadBool { dst, val: b };
                    reg_bools.insert(dst, b);
                    reg_consts.remove(&dst);
                } else if let Some(&v) = reg_consts.get(&src) {
                    let b = v != 0.0;
                    module.instructions[i] = Instruction::LoadBool { dst, val: b };
                    reg_bools.insert(dst, b);
                    reg_consts.remove(&dst);
                } else {
                    reg_consts.remove(&dst);
                    reg_bools.remove(&dst);
                }
            }
            // ExecCapture zapisuje DWA rejestry (dst_ec i dst_out) — trzeba
            // unieważnić oba, inaczej dalsze foldowanie mogłoby użyć
            // nieaktualnej "stałej" wartości w jednym z nich.
            Instruction::ExecCapture { dst_ec, dst_out, .. } => {
                reg_consts.remove(&dst_ec);
                reg_bools.remove(&dst_ec);
                reg_consts.remove(&dst_out);
                reg_bools.remove(&dst_out);
            }
            // Każda inna instrukcja z `dst` unieważnia wiedzę o tym rejestrze —
            // bez tego dalsze foldowanie mogłoby użyć nieaktualnej wartości po
            // tym, jak rejestr został nadpisany czymś innym niż stała.
            other => {
                if let Some(d) = instruction_dst(&other) {
                    reg_consts.remove(&d);
                    reg_bools.remove(&d);
                }
            }
        }
    }
}

/// Dead branch elimination: jeśli warunek skoku jest znaną stałą bool
/// (LoadBool bezpośrednio przed JumpIfFalse/JumpIfTrue, bez nadpisania
/// rejestru pomiędzy), zamień na bezwarunkowy Jump albo usuń skok całkiem —
/// oszczędza jedno porównanie i jeden branch w runtime na każde trafienie.
///
/// Celowo NIE usuwamy martwego kodu w nieosiągalnej gałęzi (wymagałoby to
/// pełnej analizy przepływu sterowania i przenumerowania wszystkich skoków
/// wskazujących do środka tej gałęzi z zewnątrz) — tylko upraszczamy sam
/// warunek. To bezpieczna, lokalna optymalizacja bez ryzyka zepsucia
/// poprawności programu.
fn pass_dead_branch_elimination(module: &mut HlModule) {
    use std::collections::HashMap;
    let mut reg_bools: HashMap<Reg, bool> = HashMap::new();

    let len = module.instructions.len();
    for i in 0..len {
        match module.instructions[i].clone() {
            Instruction::LoadBool { dst, val } => {
                reg_bools.insert(dst, val);
            }
            Instruction::JumpIfFalse { cond, offset } => {
                if let Some(&known) = reg_bools.get(&cond) {
                    module.instructions[i] = if known {
                        // warunek zawsze prawdziwy → nigdy nie skacz
                        Instruction::Nop
                    } else {
                        // warunek zawsze fałszywy → zawsze skacz
                        Instruction::Jump { offset }
                    };
                }
            }
            Instruction::JumpIfTrue { cond, offset } => {
                if let Some(&known) = reg_bools.get(&cond) {
                    module.instructions[i] = if known {
                        Instruction::Jump { offset }
                    } else {
                        Instruction::Nop
                    };
                }
            }
            Instruction::ExecCapture { dst_ec, dst_out, .. } => {
                reg_bools.remove(&dst_ec);
                reg_bools.remove(&dst_out);
            }
            other => {
                if let Some(d) = instruction_dst(&other) {
                    reg_bools.remove(&d);
                }
            }
        }
    }
}

/// Wątkowanie skoków (jump threading): jeśli bezwarunkowy `Jump{offset: X}`
/// wskazuje na INNĄ bezwarunkową instrukcję `Jump{offset: Y}`, przekierowuje
/// go bezpośrednio na `Y`, pomijając pośredni "doskok" — i tak dalej
/// łańcuchowo, aż trafi na coś innego niż `Jump` (albo osiągnie limit
/// `MAX_HOPS`, co gwarantuje zakończenie nawet dla patologicznych /
/// cyklicznych łańcuchów).
///
/// Bezpieczeństwo: ta transformacja NIGDY nie przesuwa ani nie usuwa żadnej
/// instrukcji — zmienia WYŁĄCZNIE pole `offset` samego skoku źródłowego.
/// Dzięki temu żadne INNE miejsce w module, które skacze gdzie indziej
/// (albo w ogóle nie dotyczy tego łańcucha), nie wymaga żadnej korekty —
/// w przeciwieństwie do przesuwania/usuwania instrukcji, gdzie trzeba by
/// przeliczyć WSZYSTKIE offsety w całym module.
///
/// Korzyść dla JIT: `try_compile_trace` liczy `loop_size = pc - target` i
/// odrzuca trasy dłuższe niż `MAX_TRACE_LOOP_SIZE` — wątkowanie skoków
/// zmniejsza tę odległość (mniej pośrednich "doskoków" po drodze), więc
/// więcej pętli mieści się pod limitem. Krócej też znaczy mniej bloków w
/// CFG budowanym przez Cranelift (`compile_func_body`) — mniejszy koszt
/// kompilacji za każdym razem, gdy retry-with-backoff próbuje ponownie.
fn pass_jump_threading(module: &mut HlModule) {
    const MAX_HOPS: usize = 64;
    let len = module.instructions.len();
    for i in 0..len {
        let mut target = match module.instructions[i] {
            Instruction::Jump { offset } => offset,
            _ => continue,
        };
        let mut hops = 0;
        // Podążaj łańcuchem Jump→Jump→Jump… aż trafisz na coś innego.
        // Czytamy AKTUALNY (być może już częściowo powątkowany w tym samym
        // przebiegu) stan instrukcji — to bezpieczne i tylko przyspiesza
        // zbieżność: wynik końcowy jest tym samym punktem stałym niezależnie
        // od kolejności przetwarzania, bo wątkowanie jest przechodnie.
        while hops < MAX_HOPS {
            match module.instructions.get(target as usize) {
                Some(&Instruction::Jump { offset: next }) if next != target => {
                    target = next;
                    hops += 1;
                }
                _ => break,
            }
        }
        if let Instruction::Jump { offset } = &mut module.instructions[i] {
            *offset = target;
        }
    }
}


/// kiedy dotychczasowa wiedza o "ten rejestr to znana stała" się dezaktualizuje.
fn instruction_dst(insn: &Instruction) -> Option<Reg> {
    match *insn {
        Instruction::LoadStr  { dst, .. } => Some(dst),
        Instruction::LoadNum  { dst, .. } => Some(dst),
        Instruction::LoadBool { dst, .. } => Some(dst),
        Instruction::LoadNil  { dst }     => Some(dst),
        Instruction::GetVar   { dst, .. } => Some(dst),
        Instruction::GetVarDyn{ dst, .. } => Some(dst),
        Instruction::Add  { dst, .. } => Some(dst),
        Instruction::Sub  { dst, .. } => Some(dst),
        Instruction::Mul  { dst, .. } => Some(dst),
        Instruction::Div  { dst, .. } => Some(dst),
        Instruction::Mod  { dst, .. } => Some(dst),
        Instruction::Neg  { dst, .. } => Some(dst),
        Instruction::CmpEq { dst, .. } => Some(dst),
        Instruction::CmpNe { dst, .. } => Some(dst),
        Instruction::CmpLt { dst, .. } => Some(dst),
        Instruction::CmpLe { dst, .. } => Some(dst),
        Instruction::CmpGt { dst, .. } => Some(dst),
        Instruction::CmpGe { dst, .. } => Some(dst),
        Instruction::ToString { dst, .. } => Some(dst),
        Instruction::ToNumber { dst, .. } => Some(dst),
        Instruction::Truthy   { dst, .. } => Some(dst),
        Instruction::Concat   { dst, .. } => Some(dst),
        Instruction::CallQuick { dst, .. } => Some(dst),
        Instruction::ExecCmd    { dst, .. } => Some(dst),
        Instruction::ExecCapture { dst_ec, .. } => Some(dst_ec),
        _ => None,
    }
}

/// Czy instrukcja jest CZYSTYM producentem wartości (dst = f(operandy)) bez
/// żadnych efektów ubocznych poza zapisem do `dst`? Tylko takie instrukcje
/// wolno usunąć, gdy ich wynik nigdy nie jest czytany — np. ExecCmd/Print/
/// CallQuick/HackerOsCall NIGDY nie trafiają tutaj, bo mają efekty uboczne
/// (uruchamiają proces, piszą na stdout, itd.) niezależne od tego, czy ktoś
/// przeczyta ich `dst`.
fn is_pure_producer(insn: &Instruction) -> bool {
    matches!(insn,
        Instruction::LoadStr { .. } | Instruction::LoadNum { .. } |
        Instruction::LoadBool { .. } | Instruction::LoadNil { .. } |
        Instruction::GetVar { .. } | Instruction::GetVarDyn { .. } |
        Instruction::Add { .. } | Instruction::Sub { .. } | Instruction::Mul { .. } |
        Instruction::Div { .. } | Instruction::Mod { .. } | Instruction::Neg { .. } |
        Instruction::CmpEq { .. } | Instruction::CmpNe { .. } | Instruction::CmpLt { .. } |
        Instruction::CmpLe { .. } | Instruction::CmpGt { .. } | Instruction::CmpGe { .. } |
        Instruction::ToString { .. } | Instruction::ToNumber { .. } | Instruction::Truthy { .. } |
        Instruction::Concat { .. }
    )
}

/// Zwraca wszystkie rejestry ODCZYTYWANE (nie zapisywane) przez instrukcję.
fn instruction_reads(insn: &Instruction) -> Vec<Reg> {
    match insn {
        Instruction::GetVarDyn { name, .. }        => vec![*name],
        Instruction::Add { a, b, .. } | Instruction::Sub { a, b, .. } |
        Instruction::Mul { a, b, .. } | Instruction::Div { a, b, .. } |
        Instruction::Mod { a, b, .. } |
        Instruction::CmpEq { a, b, .. } | Instruction::CmpNe { a, b, .. } |
        Instruction::CmpLt { a, b, .. } | Instruction::CmpLe { a, b, .. } |
        Instruction::CmpGt { a, b, .. } | Instruction::CmpGe { a, b, .. }
            => vec![*a, *b],
        Instruction::Neg { src, .. } | Instruction::ToString { src, .. } |
        Instruction::ToNumber { src, .. } | Instruction::Truthy { src, .. }
            => vec![*src],
        Instruction::Concat { parts, .. }          => parts.clone(),
        Instruction::SetVar { src, .. }             => vec![*src],
        Instruction::SetEnv { src, .. }             => vec![*src],
        Instruction::JumpIfFalse { cond, .. } | Instruction::JumpIfTrue { cond, .. }
            => vec![*cond],
        Instruction::Return { src: Some(r) }        => vec![*r],
        Instruction::CallQuick { arg, .. }          => vec![*arg],
        Instruction::ExecCmd { cmd, .. }            => vec![*cmd],
        Instruction::ExecCapture { cmd, .. }        => vec![*cmd],
        Instruction::Print { src }                  => vec![*src],
        Instruction::ForInStart { src, .. }         => vec![*src],
        Instruction::HackerOsCall { args, .. }      => vec![*args],
        _ => vec![],
    }
}

/// Dead store elimination: czysta instrukcja produkująca wartość w `dst`,
/// której NIKT nigdy nie odczytuje przed kolejnym zapisem do tego samego
/// rejestru (albo przed końcem funkcji/bloku), jest martwa — zamień na Nop.
///
/// Bezpieczeństwo: jednoprzebiegowy model per-rejestr. Przy KAŻDYM
/// potencjalnym celu skoku (Jump/JumpIfFalse/JumpIfTrue offset, początek
/// funkcji) i przy CallFunc czyścimy całą wiedzę o "oczekujących" zapisach
/// — to konserwatywne, ale gwarantuje, że nigdy nie usuniemy zapisu, który
/// mógłby być odczytany z innej ścieżki przepływu sterowania (np. wejście
/// do pętli z zewnątrz, skok warunkowy, wywołanie funkcji o nieznanym dla
/// tego passu zachowaniu).
fn pass_dead_store_elimination(module: &mut HlModule) {
    // Zbiór wszystkich potencjalnych celów skoków/wejść — "granice bloków".
    let mut boundaries: HashSet<InsnOff> = HashSet::new();
    for insn in &module.instructions {
        match insn {
            Instruction::Jump { offset } |
            Instruction::JumpIfFalse { offset, .. } |
            Instruction::JumpIfTrue { offset, .. } => { boundaries.insert(*offset); }
            Instruction::ForInNext { end_off, .. } => { boundaries.insert(*end_off); }
            _ => {}
        }
    }
    for f in &module.funcs.entries {
        boundaries.insert(f.start_insn);
    }

    let mut pending: HashMap<Reg, usize> = HashMap::new();
    let len = module.instructions.len();

    for i in 0..len {
        if boundaries.contains(&(i as InsnOff)) {
            pending.clear();
        }

        // Osobno wyciągnij informacje o bieżącej instrukcji, żeby nie
        // trzymać pożyczki na module.instructions[i] podczas mutacji.
        let insn = module.instructions[i].clone();

        // 1. Przetwórz odczyty PRZED zapisem — instrukcja typu `x = x + 1`
        //    najpierw CZYTA stary rejestr, dopiero potem go nadpisuje.
        for r in instruction_reads(&insn) {
            pending.remove(&r);
        }

        // 2. CallFunc / Return — nieznany wpływ na rejestry z zewnątrz
        //    (wywołanie funkcji) albo koniec zasięgu (Return) — w obu
        //    przypadkach zapisy, które PRZETRWAŁY do tego punktu i tak są
        //    już martwe (nic po nich ich nie przeczyta w tym zasięgu), więc
        //    można je bezpiecznie skasować teraz, zamiast tylko czyścić.
        if matches!(insn, Instruction::CallFunc { .. } | Instruction::Return { .. }) {
            for (_, &idx) in pending.iter() {
                module.instructions[idx] = Instruction::Nop;
            }
            pending.clear();
        }

        // 3. Zapis do rejestru (dst) — jeśli poprzedni "oczekujący" zapis do
        //    TEGO SAMEGO rejestru nigdy nie został odczytany (bo inaczej
        //    krok 1 by go usunął z `pending`), to on jest martwy.
        if let Some(dst) = instruction_dst(&insn) {
            if let Some(&old_idx) = pending.get(&dst) {
                module.instructions[old_idx] = Instruction::Nop;
            }
            if is_pure_producer(&insn) {
                pending.insert(dst, i);
            } else {
                // Instrukcja ma efekty uboczne (CallQuick/ExecCmd/...) —
                // nie wolno jej usunąć nawet jeśli dst jest martwy, więc nie
                // wchodzi do `pending` (nic tu nie będziemy kasować), ale
                // POPRZEDNI martwy zapis do dst już został wyżej obsłużony.
                pending.remove(&dst);
            }
        }
    }

    // Cokolwiek zostało w `pending` na końcu strumienia instrukcji (np.
    // main bez jawnego Return na samym końcu) jest również martwe.
    for (_, &idx) in pending.iter() {
        module.instructions[idx] = Instruction::Nop;
    }
}

/// Usuń Nop — przepisz instrukcje pomijając Nopy i popraw offsety skoków
fn pass_nop_elimination(module: &mut HlModule) {
    // Zbuduj mapę starych offsetów → nowych offsetów
    let old_len = module.instructions.len();
    let mut offset_map = vec![0u32; old_len + 1];
    let mut new_instructions = Vec::with_capacity(old_len);

    for (old_off, insn) in module.instructions.iter().enumerate() {
        offset_map[old_off] = new_instructions.len() as u32;
        if !matches!(insn, Instruction::Nop) {
            new_instructions.push(insn.clone());
        }
    }
    offset_map[old_len] = new_instructions.len() as u32;

    // Przepisz offsety w instrukcjach skoku
    for insn in &mut new_instructions {
        match insn {
            Instruction::JumpIfFalse { offset, .. } => {
                *offset = offset_map[(*offset as usize).min(old_len)];
            }
            Instruction::JumpIfTrue { offset, .. } => {
                *offset = offset_map[(*offset as usize).min(old_len)];
            }
            Instruction::Jump { offset } => {
                *offset = offset_map[(*offset as usize).min(old_len)];
            }
            Instruction::ForInNext { end_off, .. } => {
                *end_off = offset_map[(*end_off as usize).min(old_len)];
            }
            _ => {}
        }
    }

    // Zaktualizuj wpisy funkcji
    for entry in &mut module.funcs.entries {
        entry.start_insn = offset_map[(entry.start_insn as usize).min(old_len)];
    }

    module.instructions = new_instructions;
}

/// Usuń SourceLine markers — nie potrzebne w release
fn pass_source_line_strip(module: &mut HlModule) {
    for insn in &mut module.instructions {
        if matches!(insn, Instruction::SourceLine { .. }) {
            *insn = Instruction::Nop;
        }
    }
    // Drugi pass: usuń właśnie wstawione Nopy (Nop elimination już to ogarnie
    // ale wywołamy go ponownie jeśli cokolwiek zmieniliśmy)
    let had_source_lines = module.instructions.iter()
    .any(|i| matches!(i, Instruction::Nop));
    if had_source_lines {
        pass_nop_elimination(module);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::HlModule;

    fn make_module_with(insns: Vec<Instruction>, nums: Vec<f64>) -> HlModule {
        let mut m = HlModule::new("test.hl", 2);
        for n in nums { m.consts.add_num(n); }
        m.instructions = insns;
        m
    }

    #[test]
    fn test_constant_folding_add() {
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 }, // 2.0
            Instruction::LoadNum { dst: 1, idx: 1 }, // 3.0
            Instruction::Add { dst: 2, a: 0, b: 1 },
        ], vec![2.0, 3.0]);

        optimize_module(&mut m);

        // Po constant folding: Add powinien być zastąpiony przez LoadNum(5.0)
        let has_five = m.consts.numbers.contains(&5.0);
        assert!(has_five, "Powinno być 5.0 w puli stałych po constant folding");
        assert!(!m.instructions.iter().any(|i| matches!(i, Instruction::Add { .. })),
                "Add powinien być wyeliminowany");
    }

    #[test]
    fn test_constant_folding_chained() {
        // (2+3)*4 = 20 — sprawdza, że foldowanie łańcuchowe działa (wynik
        // jednego foldu jest dalej traktowany jako stała dla kolejnego)
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 }, // 2.0
            Instruction::LoadNum { dst: 1, idx: 1 }, // 3.0
            Instruction::Add { dst: 2, a: 0, b: 1 }, // reg2 = 5.0
            Instruction::LoadNum { dst: 3, idx: 2 }, // 4.0
            Instruction::Mul { dst: 4, a: 2, b: 3 }, // reg4 = 20.0
        ], vec![2.0, 3.0, 4.0]);

        optimize_module(&mut m);

        assert!(m.consts.numbers.contains(&20.0), "Powinno być 20.0 po foldowaniu łańcuchowym");
        assert!(!m.instructions.iter().any(|i| matches!(i, Instruction::Add { .. } | Instruction::Mul { .. })),
                "Add i Mul powinny być wyeliminowane");
    }

    #[test]
    fn test_constant_folding_comparison() {
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 }, // 5.0
            Instruction::LoadNum { dst: 1, idx: 1 }, // 3.0
            Instruction::CmpGt { dst: 2, a: 0, b: 1 }, // 5 > 3 = true
            Instruction::Return { src: Some(2) }, // "użyj" reg2, żeby dead store elimination go nie skasowało
        ], vec![5.0, 3.0]);

        optimize_module(&mut m);

        match &m.instructions.iter().find(|i| matches!(i, Instruction::LoadBool { dst: 2, .. })) {
            Some(Instruction::LoadBool { val, .. }) => assert!(*val, "5 > 3 powinno dać true"),
            _ => panic!("Oczekiwano LoadBool po foldowaniu CmpGt"),
        }
        assert!(!m.instructions.iter().any(|i| matches!(i, Instruction::CmpGt { .. })),
                "CmpGt powinien być wyeliminowany");
    }

    #[test]
    fn test_stale_register_not_folded() {
        // reg 0 jest najpierw stałą (2.0), potem nadpisywana przez GetVar —
        // Add na reg 0 PO nadpisaniu nie może użyć starej wartości 2.0.
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 },       // reg0 = 2.0 (stała) — martwy zapis (nadpisany zaraz przez GetVar), OK że zniknie
            Instruction::GetVar  { dst: 0, name: 0 },       // reg0 = env["x"] (JUŻ NIE stała!)
            Instruction::LoadNum { dst: 1, idx: 1 },        // reg1 = 3.0
            Instruction::Add     { dst: 2, a: 0, b: 1 },    // reg2 = reg0 + 3.0 — NIE wolno foldować
            Instruction::Return { src: Some(2) },           // "użyj" reg2, żeby dead store elimination go nie skasowało
        ], vec![2.0, 3.0]);

        optimize_module(&mut m);

        assert!(m.instructions.iter().any(|i| matches!(i, Instruction::Add { .. })),
                "Add NIE powinien być foldowany — reg0 był nadpisany przez GetVar, nie jest już znaną stałą");
    }

    #[test]
    fn test_dead_branch_elimination_always_true() {
        // JumpIfFalse z warunkiem = zawsze true → nigdy nie skacz → Nop
        let mut m = make_module_with(vec![
            Instruction::LoadBool { dst: 0, val: true },
            Instruction::JumpIfFalse { cond: 0, offset: 5 },
            Instruction::Return { src: None },
        ], vec![]);

        optimize_module(&mut m);

        // Po dead branch elimination + nop elimination: JumpIfFalse zniknął
        // całkowicie (był Nop, potem wyczyszczony), zostaje tylko Return.
        assert!(!m.instructions.iter().any(|i| matches!(i, Instruction::JumpIfFalse { .. })),
                "JumpIfFalse ze znanym warunkiem=true powinien zniknąć (nigdy nie skacze)");
    }

    #[test]
    fn test_dead_branch_elimination_always_false() {
        // JumpIfFalse z warunkiem = zawsze false → zawsze skacz → Jump bezwarunkowy
        let mut m = make_module_with(vec![
            Instruction::LoadBool { dst: 0, val: false },
            Instruction::JumpIfFalse { cond: 0, offset: 5 },
            Instruction::Return { src: None },
        ], vec![]);

        optimize_module(&mut m);

        let has_unconditional_jump = m.instructions.iter().any(|i| matches!(i, Instruction::Jump { .. }));
        let has_conditional = m.instructions.iter().any(|i| matches!(i, Instruction::JumpIfFalse { .. }));
        assert!(has_unconditional_jump, "JumpIfFalse ze znanym warunkiem=false powinien stać się bezwarunkowym Jump");
        assert!(!has_conditional, "Nie powinno już być JumpIfFalse");
    }

    #[test]
    fn test_dead_branch_preserves_offset() {
        // Upewnij się, że offset skoku jest poprawnie przenoszony i przemapowany
        // po nop_elimination (czyli end-to-end przez cały optimize_module).
        let mut m = make_module_with(vec![
            Instruction::LoadBool { dst: 0, val: false },      // 0
            Instruction::JumpIfFalse { cond: 0, offset: 3 },   // 1 → Jump{offset:3}
            Instruction::LoadNum { dst: 1, idx: 0 },            // 2 (pomijane w runtime, ale nadal obecne — brak DCE gałęzi)
            Instruction::Return { src: None },                  // 3 ← cel skoku
        ], vec![1.0]);

        optimize_module(&mut m);

        let jump = m.instructions.iter().find_map(|i| match i {
            Instruction::Jump { offset } => Some(*offset),
            _ => None,
        }).expect("powinien być Jump");
        match &m.instructions[jump as usize] {
            Instruction::Return { .. } => {}
            other => panic!("Jump powinien wskazywać na Return, wskazuje na {:?}", other),
        }
    }

    #[test]
    fn test_dynamic_condition_untouched() {
        // Warunek NIE jest znaną stałą (pochodzi z GetVar) — JumpIfFalse
        // musi pozostać nietknięty, bo nie wiemy nic o jego wartości.
        let mut m = make_module_with(vec![
            Instruction::GetVar { dst: 0, name: 0 },
            Instruction::JumpIfFalse { cond: 0, offset: 5 },
            Instruction::Return { src: None },
        ], vec![]);

        optimize_module(&mut m);

        assert!(m.instructions.iter().any(|i| matches!(i, Instruction::JumpIfFalse { .. })),
                "JumpIfFalse z dynamicznym (nieznanym) warunkiem nie powinien być ruszany");
    }

    #[test]
    fn test_nop_elimination_preserves_jumps() {
        let mut m = make_module_with(vec![
            Instruction::Nop,                             // 0 → usunięte
            Instruction::LoadBool { dst: 0, val: true },  // 1 → 0
            Instruction::JumpIfFalse { cond: 0, offset: 3 }, // 2 → 1, offset 3→2
            Instruction::Nop,                             // 3 → usunięte
            Instruction::Return { src: None },            // 4 → 2
        ], vec![]);

        pass_nop_elimination(&mut m);

        assert_eq!(m.instructions.len(), 3);
        if let Instruction::JumpIfFalse { offset, .. } = m.instructions[1] {
            assert_eq!(offset, 2, "offset skoku powinien wskazywać na Return (idx 2)");
        } else {
            panic!("Oczekiwano JumpIfFalse");
        }
    }

    #[test]
    fn test_dead_store_basic_overwrite() {
        // reg0 = 1.0, potem NATYCHMIAST nadpisane przez reg0 = 2.0 bez
        // żadnego odczytu pomiędzy — pierwszy LoadNum jest czystym martwym
        // zapisem i powinien zniknąć.
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 }, // 1.0 — MARTWE
            Instruction::LoadNum { dst: 0, idx: 1 }, // 2.0 — to przetrwa (czytane niżej)
            Instruction::Print { src: 0 },
        ], vec![1.0, 2.0]);

        pass_dead_store_elimination(&mut m);

        // Pierwszy LoadNum powinien być Nop, drugi nietknięty.
        assert!(matches!(m.instructions[0], Instruction::Nop), "pierwszy zapis powinien być martwy");
        assert!(matches!(m.instructions[1], Instruction::LoadNum { idx: 1, .. }), "drugi zapis musi przetrwać");
    }

    #[test]
    fn test_dead_store_preserves_read_before_overwrite() {
        // reg0 = 1.0, ODCZYTANE (Print), potem nadpisane — pierwszy zapis
        // NIE jest martwy, bo ktoś go przeczytał zanim reg0 dostał nową wartość.
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 }, // 1.0 — używane niżej
            Instruction::Print { src: 0 },            // odczyt reg0
            Instruction::LoadNum { dst: 0, idx: 1 },  // 2.0 — nowa wartość
            Instruction::Print { src: 0 },            // odczyt nowej wartości
        ], vec![1.0, 2.0]);

        pass_dead_store_elimination(&mut m);

        assert!(matches!(m.instructions[0], Instruction::LoadNum { idx: 0, .. }),
                "pierwszy zapis był odczytany — NIE wolno go usunąć");
        assert!(matches!(m.instructions[2], Instruction::LoadNum { idx: 1, .. }));
    }

    #[test]
    fn test_dead_store_never_touches_side_effects() {
        // ExecCmd/Print mają efekty uboczne — nigdy nie wolno ich usunąć,
        // nawet jeśli ich `dst`/wynik nigdy nie jest odczytany.
        let mut m = make_module_with(vec![
            Instruction::LoadStr { dst: 0, idx: 0 },
            Instruction::ExecCmd { cmd: 0, mode: CmdMode::Plain, dst: 1 }, // dst=1 nigdy nieużyty
        ], vec![]);
        m.consts.strings.push("echo hi".to_string());

        pass_dead_store_elimination(&mut m);

        assert!(m.instructions.iter().any(|i| matches!(i, Instruction::ExecCmd { .. })),
                "ExecCmd ma efekt uboczny — nie wolno go usunąć nawet z martwym dst");
    }

    #[test]
    fn test_dead_store_respects_jump_target_boundary() {
        // reg0 jest zapisywane PRZED skokiem warunkowym, ale odczytywane
        // dopiero W CELU skoku (czyli z innej ścieżki kontrolnej niż
        // sekwencyjny fallthrough) — pass MUSI to zauważyć przez granicę
        // bloku i NIE wolno mu skasować zapisu tylko dlatego, że "lokalnie"
        // między zapisem a skokiem nikt go nie czyta.
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 },        // 0: reg0 = 1.0 — używane w celu skoku (idx 3)
            Instruction::LoadBool { dst: 1, val: true },     // 1
            Instruction::JumpIfFalse { cond: 1, offset: 4 }, // 2: (nie skacze, warunek dynamiczny by nie foldował)
            Instruction::Jump { offset: 4 },                 // 3: bezwarunkowy skok do celu 4
            Instruction::Print { src: 0 },                   // 4: CEL SKOKU — czyta reg0
        ], vec![1.0]);

        pass_dead_store_elimination(&mut m);

        assert!(matches!(m.instructions[0], Instruction::LoadNum { .. }),
                "zapis czytany z innej ścieżki (przez cel skoku) NIE może zostać usunięty");
    }

    #[test]
    fn test_dead_store_end_to_end_via_optimize_module() {
        // Pełny optimize_module na realistycznym wzorcu: policz coś, wypisz,
        // policz coś innego w tym samym rejestrze bez odczytu pierwszego —
        // sprawdza że DSE faktycznie działa przez pełny pipeline (po
        // constant foldingu, przed nop eliminaton).
        let mut m = make_module_with(vec![
            Instruction::LoadNum { dst: 0, idx: 0 },  // 1.0 — MARTWE (nadpisane niżej bez odczytu)
            Instruction::LoadNum { dst: 0, idx: 1 },  // 2.0 — używane
            Instruction::ToString { dst: 1, src: 0 },
            Instruction::Print { src: 1 },
        ], vec![1.0, 2.0]);

        let before = m.instructions.len();
        optimize_module(&mut m);
        let after = m.instructions.len();

        assert!(after < before, "dead store elimination + nop elimination powinno skrócić kod");
    }

    #[test]
    fn test_jump_threading_collapses_chain() {
        // Jump{1} → Jump{2} → Jump{3} → Return  ⇒  po wątkowaniu WSZYSTKIE
        // trzy skoki powinny wskazywać bezpośrednio na Return (idx 3), bez
        // pośrednich "doskoków". Wołamy pass w izolacji (jak inne testy
        // pojedynczych przebiegów w tym pliku) — pełny optimize_module
        // zawierałby też dead_store/nop_elimination, które przenumerowałyby
        // indeksy po ewentualnym usunięciu martwych instrukcji i
        // zaciemniłyby to, co faktycznie testujemy.
        let mut m = make_module_with(vec![
            Instruction::Jump { offset: 1 },   // 0 → 1 → 2 → 3
            Instruction::Jump { offset: 2 },   // 1 → 2 → 3
            Instruction::Jump { offset: 3 },   // 2 → 3
            Instruction::Return { src: None }, // 3
        ], vec![]);

        pass_jump_threading(&mut m);

        for (i, insn) in m.instructions.iter().enumerate() {
            if let Instruction::Jump { offset } = insn {
                assert_eq!(*offset, 3, "instrukcja {} powinna wskazywać bezpośrednio na Return (idx 3), nie na pośredni doskok", i);
            }
        }
    }

    #[test]
    fn test_jump_threading_after_dead_branch_elimination() {
        // Warunek znany na etapie kompilacji → JumpIfFalse zwija się w Jump
        // (dead_branch_elimination) → ten NOWO powstały Jump też powinien
        // zostać powątkowany, jeśli sam wskazuje na kolejny Jump. Oba passy
        // wołane bezpośrednio (bez nop/dead-store elimination), żeby
        // uniknąć przenumerowania indeksów przez niepowiązane przebiegi.
        let mut m = make_module_with(vec![
            Instruction::LoadBool { dst: 0, val: false },    // 0
            Instruction::JumpIfFalse { cond: 0, offset: 2 }, // 1 → (cond=false) → Jump{2}
            Instruction::Jump { offset: 3 },                 // 2
            Instruction::Return { src: None },               // 3
        ], vec![]);

        pass_dead_branch_elimination(&mut m);
        pass_jump_threading(&mut m);

        match m.instructions[1] {
            Instruction::Jump { offset } => assert_eq!(
                offset, 3,
                "Jump powstały z foldowania warunku powinien być powątkowany do celu ostatecznego"
            ),
            ref other => panic!("Oczekiwano Jump, jest {:?}", other),
        }
    }

    #[test]
    fn test_jump_threading_self_loop_terminates() {
        // Jump do samego siebie (odpowiednik `while true do done` bez ciała)
        // — pass NIE MOŻE wpaść w nieskończoną pętlę KOMPILATORA, nawet
        // jeśli sam program źródłowy opisuje nieskończoną pętlę wykonania.
        let mut m = make_module_with(vec![
            Instruction::Jump { offset: 0 }, // 0 → 0 (sam do siebie)
        ], vec![]);

        pass_jump_threading(&mut m); // nie powinno się zawiesić

        match m.instructions[0] {
            Instruction::Jump { offset } => assert_eq!(offset, 0),
            _ => panic!("Oczekiwano Jump"),
        }
    }
}
