use crate::bytecode::*;

pub fn optimize_module(module: &mut HlModule) {
    pass_constant_folding(module);
    pass_nop_elimination(module);
    pass_source_line_strip(module);
    // Deduplacja stałych jest już wbudowana w ConstPool
}

/// Constant folding: dwa LoadNum + Add/Sub/Mul/Div → jeden LoadNum
fn pass_constant_folding(module: &mut HlModule) {
    // Śledź jakie rejestry są wynikiem LoadNum i ich wartości
    // (prosty jednoprzebiegowy model — bez analizy flow)
    use std::collections::HashMap;
    let mut reg_consts: HashMap<Reg, f64> = HashMap::new();

    let len = module.instructions.len();
    for i in 0..len {
        match module.instructions[i].clone() {
            Instruction::LoadNum { dst, idx } => {
                let val = module.consts.numbers.get(idx as usize).copied().unwrap_or(0.0);
                reg_consts.insert(dst, val);
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
            // Peephole: ToString(LoadStr src) → LoadStr src (już jest stringiem)
            Instruction::ToString { dst, src } => {
                // Jeśli src jest znanym num-rejestrem, zamień na formatowanie
                // (nie robimy tutaj — to wymagałoby bardziej skomplikowanej analizy)
                let _ = (dst, src);
            }
            _ => {}
        }
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
        use std::path::Path;
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
}
