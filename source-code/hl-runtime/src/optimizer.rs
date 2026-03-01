use crate::bytecode::{BytecodeProgram, OpCode};
use colored::*;
use std::collections::HashMap;

const INLINE_THRESHOLD: usize = 8;

// ─────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────
pub fn optimize(prog: &mut BytecodeProgram, verbose: bool) {
    let before = prog.ops.len();

    constant_fold_conds(prog, verbose);
    nop_strip(prog);

    dead_store_elim(prog, verbose);
    nop_strip(prog);

    tail_call_opt(prog, verbose);
    nop_strip(prog);

    inline_small_funcs(prog, verbose);
    nop_strip(prog);

    let after = prog.ops.len();
    if verbose {
        let removed = before.saturating_sub(after);
        let pct = if before > 0 { removed as f64 / before as f64 * 100.0 } else { 0.0 };
        eprintln!(
            "{} Optimizer: {} → {} ops ({} removed, {:.1}%)",
                  "[opt]".magenta(), before, after, removed, pct
        );
    }
}

// ─────────────────────────────────────────────────────────────
// [1] Constant folding
// ─────────────────────────────────────────────────────────────
fn constant_fold_conds(prog: &mut BytecodeProgram, verbose: bool) {
    let mut folded = 0usize;
    let len = prog.ops.len();
    let mut i = 0;

    while i < len {
        let (cond_id, target) = match prog.ops[i] {
            OpCode::JumpIfFalse { cond_id, target } => (cond_id, target),
            _ => { i += 1; continue; }
        };

        let cond = prog.str(cond_id).to_string();

        match eval_static(&cond) {
            Some(true) => {
                // Zawsze TRUE → JIF nigdy nie skacze → Nop
                prog.ops[i] = OpCode::Nop;
                folded += 1;
                if verbose {
                    eprintln!("{} fold TRUE:  [{}] {}", "[opt]".magenta(), i, cond);
                }
            }
            Some(false) => {
                // Zawsze FALSE → body staje się martwym kodem
                let end = target.min(len);
                for j in i..end {
                    prog.ops[j] = OpCode::Nop;
                }
                folded += 1;
                if verbose {
                    eprintln!("{} fold FALSE: [{}] {} → Nop [{}-{}]",
                              "[opt]".magenta(), i, cond, i, end);
                }
            }
            None => {}
        }
        i += 1;
    }

    if verbose && folded > 0 {
        eprintln!("{} constant_fold: {} conditions folded", "[opt]".magenta(), folded);
    }
}

fn eval_static(cond: &str) -> Option<bool> {
    let t = cond.trim();

    let inner = if t.starts_with("[[") && t.ends_with("]]") {
        t[2..t.len()-2].trim()
    } else if t.starts_with('[') && t.ends_with(']') {
        t[1..t.len()-1].trim()
    } else {
        return None;
    };

    if let Some(val) = inner.strip_prefix("-n ") {
        let v = unquote(val.trim());
        if !v.contains('$') { return Some(!v.is_empty()); }
    }
    if let Some(val) = inner.strip_prefix("-z ") {
        let v = unquote(val.trim());
        if !v.contains('$') { return Some(v.is_empty()); }
    }

    for op in &[" == ", " = ", " != ", " -eq ", " -ne ",
        " -lt ", " -le ", " -gt ", " -ge "] {
            if let Some(pos) = inner.find(op) {
                let lhs = unquote(inner[..pos].trim());
                let rhs = unquote(inner[pos + op.len()..].trim());
                if lhs.contains('$') || rhs.contains('$') { return None; }
                return Some(match *op {
                    " == " | " = " => lhs == rhs,
                    " != "         => lhs != rhs,
                    " -eq " => lhs.parse::<i64>().ok()? == rhs.parse::<i64>().ok()?,
                            " -ne " => lhs.parse::<i64>().ok()? != rhs.parse::<i64>().ok()?,
                            " -lt " => lhs.parse::<i64>().ok()? <  rhs.parse::<i64>().ok()?,
                            " -le " => lhs.parse::<i64>().ok()? <= rhs.parse::<i64>().ok()?,
                            " -gt " => lhs.parse::<i64>().ok()? >  rhs.parse::<i64>().ok()?,
                            " -ge " => lhs.parse::<i64>().ok()? >= rhs.parse::<i64>().ok()?,
                            _ => return None,
                });
            }
        }
        None
}

fn unquote(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('"')  && s.ends_with('"'))
        || (s.starts_with('\'') && s.ends_with('\'')))
        {
            &s[1..s.len()-1]
        } else {
            s
        }
}

// ─────────────────────────────────────────────────────────────
// [2] Dead store elimination
// ─────────────────────────────────────────────────────────────
fn dead_store_elim(prog: &mut BytecodeProgram, verbose: bool) {
    let len = prog.ops.len();
    let mut removed = 0usize;
    let mut i = 0;

    while i < len {
        let key_id = match prog.ops[i] {
            OpCode::SetLocal { key_id, .. } => key_id,
            _ => { i += 1; continue; }
        };

        let key_name = prog.str(key_id).to_string();
        let mut safe = false;
        let mut j    = i + 1;

        while j < len {
            match &prog.ops[j] {
                OpCode::SetLocal { key_id: k2, .. } if *k2 == key_id => {
                    safe = true;
                    break;
                }
                OpCode::Exec { cmd_id, .. } => {
                    let s = prog.str(*cmd_id);
                    if s.contains(&format!("${}", key_name))
                        || s.contains(&format!("${{{}}}", key_name))
                        {
                            break;
                        }
                        j += 1;
                }
                OpCode::CallFunc { .. } | OpCode::Plugin { .. } => break,
                OpCode::Return | OpCode::Exit(_) => break,
                _ => { j += 1; }
            }
        }

        if safe {
            prog.ops[i] = OpCode::Nop;
            removed += 1;
            if verbose {
                eprintln!("{} dead_store: [{}] ${}", "[opt]".magenta(), i, key_name);
            }
        }
        i += 1;
    }

    if verbose && removed > 0 {
        eprintln!("{} dead_store: {} removed", "[opt]".magenta(), removed);
    }
}

// ─────────────────────────────────────────────────────────────
// [3] Tail Call Optimization
// ─────────────────────────────────────────────────────────────
fn tail_call_opt(prog: &mut BytecodeProgram, verbose: bool) {
    let len = prog.ops.len();

    let fn_addrs: HashMap<u32, usize> = prog.functions.iter()
    .filter_map(|(name, &addr)| {
        prog.pool.index.get(name).map(|&id| (id, addr))
    })
    .collect();

    let mut count = 0usize;
    let mut i = 0;

    while i + 1 < len {
        let func_id = match prog.ops[i] {
            OpCode::CallFunc { func_id } => func_id,
            _ => { i += 1; continue; }
        };

        // Następny nieNopowy opcode
        let mut j = i + 1;
        while j < len && matches!(prog.ops[j], OpCode::Nop) { j += 1; }

        if j < len && matches!(prog.ops[j], OpCode::Return) {
            if let Some(&target) = fn_addrs.get(&func_id) {
                let fname = prog.str(func_id).to_string();
                prog.ops[i] = OpCode::Jump { target };
                prog.ops[j] = OpCode::Nop;
                count += 1;
                if verbose {
                    eprintln!("{} TCO: [{}] .{} → Jump {}", "[opt]".magenta(), i, fname, target);
                }
            }
        }
        i += 1;
    }

    if verbose && count > 0 {
        eprintln!("{} tail_call_opt: {} calls converted", "[opt]".magenta(), count);
    }
}

// ─────────────────────────────────────────────────────────────
// [4] Function inlining
// ─────────────────────────────────────────────────────────────
fn inline_small_funcs(prog: &mut BytecodeProgram, verbose: bool) {
    let candidates = find_inline_candidates(prog);
    if candidates.is_empty() { return; }

    let mut count = 0usize;
    let old_ops: Vec<OpCode> = std::mem::take(&mut prog.ops);
    let mut new_ops: Vec<OpCode> = Vec::with_capacity(old_ops.len() * 2);

    for op in old_ops {
        match &op {
            OpCode::CallFunc { func_id } => {
                if let Some(body) = candidates.get(func_id) {
                    for b in body { new_ops.push(b.clone()); }
                    count += 1;
                    if verbose {
                        let fname = prog.pool.get(*func_id);
                        eprintln!("{} inline: .{} ({} ops)", "[opt]".magenta(), fname, body.len());
                    }
                } else {
                    new_ops.push(op);
                }
            }
            other => new_ops.push(other.clone()),
        }
    }

    prog.ops = new_ops;
    if verbose && count > 0 {
        eprintln!("{} inline: {} call sites inlined", "[opt]".magenta(), count);
    }
}

fn find_inline_candidates(prog: &BytecodeProgram) -> HashMap<u32, Vec<OpCode>> {
    let mut result = HashMap::new();

    'outer: for (name, &start) in &prog.functions {
        let func_id = match prog.pool.index.get(name) {
            Some(&id) => id,
            None      => continue,
        };

        let mut body: Vec<OpCode> = Vec::new();
        let mut j = start;

        while j < prog.ops.len() {
            match &prog.ops[j] {
                OpCode::Return  => break,
                OpCode::Exit(_) => continue 'outer,  // nie inlinuj — zmienia semantykę
                OpCode::CallFunc { func_id: callee } if *callee == func_id => {
                    continue 'outer;                 // rekurencja
                }
                OpCode::Nop => { j += 1; continue; }
                op => body.push(op.clone()),
            }
            j += 1;
            if body.len() > INLINE_THRESHOLD { continue 'outer; }
        }

        if !body.is_empty() {
            result.insert(func_id, body);
        }
    }

    result
}

// ─────────────────────────────────────────────────────────────
// [5] NOP strip + adres patch
// ─────────────────────────────────────────────────────────────
fn nop_strip(prog: &mut BytecodeProgram) {
    // Buduj remapę: stary indeks → nowy indeks (usize::MAX = Nop)
    let mut remap: Vec<usize> = Vec::with_capacity(prog.ops.len());
    let mut new_idx = 0usize;
    for op in &prog.ops {
        if matches!(op, OpCode::Nop) {
            remap.push(usize::MAX);
        } else {
            remap.push(new_idx);
            new_idx += 1;
        }
    }

    // Usuń Nopy
    let filtered: Vec<OpCode> = std::mem::take(&mut prog.ops)
    .into_iter()
    .filter(|op| !matches!(op, OpCode::Nop))
    .collect();

    // Patch targets
    prog.ops = filtered
    .into_iter()
    .map(|op| match op {
        OpCode::Jump { target } =>
        OpCode::Jump { target: patch_target(target, &remap) },
         OpCode::JumpIfFalse { cond_id, target } =>
         OpCode::JumpIfFalse { cond_id, target: patch_target(target, &remap) },
         other => other,
    })
    .collect();

    // Patch function addresses
    for addr in prog.functions.values_mut() {
        *addr = patch_target(*addr, &remap);
    }
}

fn patch_target(old: usize, remap: &[usize]) -> usize {
    let mut t = old;
    while t < remap.len() {
        if remap[t] != usize::MAX { return remap[t]; }
        t += 1;
    }
    // Za końcem tablicy = koniec programu
    remap.iter().filter(|&&v| v != usize::MAX).max().map(|&v| v + 1).unwrap_or(0)
}
