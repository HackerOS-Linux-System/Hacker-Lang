use hl_parser::ast::*;
use crate::bytecode::*;
use std::path::Path;

/// Stan lowering — trzyma kontekst kompilacji
struct Lowerer {
    module:    HlModule,
    reg_alloc: u32,
}

impl Lowerer {
    fn new(source_path: &str, gen: u32) -> Self {
        Self {
            module:    HlModule::new(source_path, gen),
            reg_alloc: 0,
        }
    }

    fn alloc_reg(&mut self) -> Reg {
        let r = self.reg_alloc;
        self.reg_alloc += 1;
        r
    }

    fn emit(&mut self, insn: Instruction) {
        self.module.instructions.push(insn);
    }

    fn current_offset(&self) -> InsnOff {
        self.module.instructions.len() as InsnOff
    }

    /// Placeholder skok — wróć i wstaw offset po skompilowaniu docelowego bloku
    fn emit_jump_placeholder(&mut self, cond: Option<Reg>) -> InsnOff {
        let off = self.current_offset();
        match cond {
            Some(r) => self.emit(Instruction::JumpIfFalse { cond: r, offset: 0 }),
            None    => self.emit(Instruction::Jump { offset: 0 }),
        }
        off
    }

    fn patch_jump(&mut self, placeholder: InsnOff, target: InsnOff) {
        match &mut self.module.instructions[placeholder as usize] {
            Instruction::JumpIfFalse { offset, .. } => *offset = target,
            Instruction::JumpIfTrue  { offset, .. } => *offset = target,
            Instruction::Jump        { offset }     => *offset = target,
            _ => panic!("patch_jump: nie ma skoku pod {}", placeholder),
        }
    }

    // ── Kompilacja węzłów ────────────────────────────────────────

    fn lower_nodes(&mut self, nodes: &[Node]) {
        for node in nodes {
            self.lower_node(node);
        }
    }

    fn lower_node(&mut self, node: &Node) {
        match node {
            Node::LineComment(_) | Node::DocComment(_) | Node::BlockComment(_) => {}

            Node::Print { parts } => {
                let dst = self.lower_string_parts(parts);
                self.emit(Instruction::Print { src: dst });
            }

            Node::VarDecl { name, value, .. } => {
                let src = self.lower_var_value(value);
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::SetVar { name: name_idx, src });
            }

            Node::VarRef(name) => {
                let dst = self.alloc_reg();
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::GetVar { dst, name: name_idx });
                self.emit(Instruction::Print { src: dst });
            }

            Node::Export { name, value } => {
                let src = match value {
                    ExportValue::Single(parts) => self.lower_string_parts(parts),
                    ExportValue::List(items) => {
                        let regs: Vec<Reg> = items.iter()
                        .map(|p| self.lower_string_parts(p))
                        .collect();
                        let sep_reg = self.alloc_reg();
                        let sep_idx = self.module.consts.add_str(":");
                        self.emit(Instruction::LoadStr { dst: sep_reg, idx: sep_idx });
                        // Concat z separatorem — prostsze niż join: robimy flat concat przez separator
                        let dst = self.alloc_reg();
                        let mut all_parts = Vec::new();
                        for (i, r) in regs.iter().enumerate() {
                            if i > 0 { all_parts.push(sep_reg); }
                            all_parts.push(*r);
                        }
                        self.emit(Instruction::Concat { dst, parts: all_parts });
                        dst
                    }
                };
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::SetEnv { name: name_idx, src });
            }

            Node::Arithmetic { expr, assign_to } => {
                let dst = self.lower_arithmetic(expr);
                if let Some(var) = assign_to {
                    let name_idx = self.module.consts.add_str(var.as_str());
                    self.emit(Instruction::SetVar { name: name_idx, src: dst });
                } else {
                    self.emit(Instruction::Print { src: dst });
                }
            }

            Node::Command { raw, mode, .. } => {
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(raw.as_str());
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst = self.alloc_reg();
                let mode = lower_cmd_mode(mode);
                self.emit(Instruction::ExecCmd { cmd: cmd_reg, mode, dst });
                // exit code trafia do last_exit przez SetVar
                let le_idx = self.module.consts.add_str("_last_exit_code");
                self.emit(Instruction::SetVar { name: le_idx, src: dst });
            }

            Node::PipeToVar { command, mode, var_name } => {
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(command.as_str());
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst_ec  = self.alloc_reg();
                let dst_out = self.alloc_reg();
                let mode = lower_cmd_mode(mode);
                self.emit(Instruction::ExecCapture { cmd: cmd_reg, mode, dst_ec, dst_out });
                let name_idx = self.module.consts.add_str(var_name.as_str());
                self.emit(Instruction::SetVar { name: name_idx, src: dst_out });
            }

            Node::Conditional { condition, body } => {
                // ? ok / ? err — sprawdź last_exit_code
                let last_ec_reg = self.alloc_reg();
                let le_idx = self.module.consts.add_str("_last_exit_code");
                self.emit(Instruction::GetVar { dst: last_ec_reg, name: le_idx });

                let zero_reg = self.alloc_reg();
                let zero_idx = self.module.consts.add_num(0.0);
                self.emit(Instruction::LoadNum { dst: zero_reg, idx: zero_idx });

                let cond_reg = self.alloc_reg();
                match condition {
                    ConditionKind::Ok  => self.emit(Instruction::CmpEq { dst: cond_reg, a: last_ec_reg, b: zero_reg }),
                    ConditionKind::Err => self.emit(Instruction::CmpNe { dst: cond_reg, a: last_ec_reg, b: zero_reg }),
                }

                let jump_ph = self.emit_jump_placeholder(Some(cond_reg));
                self.lower_nodes(body);
                let after = self.current_offset();
                self.patch_jump(jump_ph, after);
            }

            Node::ForIn { var, iterable, body } => {
                let src = self.lower_string_parts(iterable);
                let iter_reg = self.alloc_reg();
                self.emit(Instruction::ForInStart { iter_reg, src });

                let loop_start = self.current_offset();
                let item_reg = self.alloc_reg();
                // placeholder dla końca pętli — patched po kompilacji body
                let end_ph_off = self.current_offset();
                self.emit(Instruction::ForInNext { iter_reg, dst: item_reg, end_off: 0 });

                let var_idx = self.module.consts.add_str(var.as_str());
                self.emit(Instruction::SetVar { name: var_idx, src: item_reg });

                self.lower_nodes(body);
                // Skok z powrotem na początek iteratora
                self.emit(Instruction::Jump { offset: loop_start });

                let after_loop = self.current_offset();
                // Patch ForInNext.end_off
                if let Instruction::ForInNext { end_off, .. } =
                    &mut self.module.instructions[end_ph_off as usize]
                    {
                        *end_off = after_loop;
                    }
            }

            Node::WhileLoop { condition, body } => {
                let loop_start = self.current_offset();
                let cond_reg = self.lower_string_parts(condition);
                // ewaluacja warunku — truthy check
                let bool_reg = self.alloc_reg();
                self.emit(Instruction::Truthy { dst: bool_reg, src: cond_reg });
                let exit_ph = self.emit_jump_placeholder(Some(bool_reg));
                self.lower_nodes(body);
                self.emit(Instruction::Jump { offset: loop_start });
                let after = self.current_offset();
                self.patch_jump(exit_ph, after);
            }

            Node::MatchExpr { subject, arms } => {
                let subj_reg = self.lower_string_parts(subject);
                let mut exit_jumps: Vec<InsnOff> = Vec::new();

                // Zbierz wildcard na koniec
                let (wildcards, normals): (Vec<_>, Vec<_>) =
                arms.iter().partition(|a| a.pattern.trim() == "*");

                for arm in &normals {
                    let pat_reg = self.alloc_reg();
                    let pat_idx = self.module.consts.add_str(arm.pattern.trim());
                    self.emit(Instruction::LoadStr { dst: pat_reg, idx: pat_idx });
                    let match_reg = self.alloc_reg();
                    self.emit(Instruction::CmpEq { dst: match_reg, a: subj_reg, b: pat_reg });
                    let skip_ph = self.emit_jump_placeholder(Some(match_reg));
                    self.lower_nodes(&arm.body);
                    exit_jumps.push(self.current_offset());
                    self.emit(Instruction::Jump { offset: 0 }); // placeholder exit
                    let after_body = self.current_offset();
                    self.patch_jump(skip_ph, after_body);
                }

                for wc in &wildcards {
                    self.lower_nodes(&wc.body);
                }

                let after_match = self.current_offset();
                for off in exit_jumps {
                    if let Instruction::Jump { offset } =
                        &mut self.module.instructions[off as usize]
                        {
                            *offset = after_match;
                        }
                }
            }

            Node::FuncDef { name, body } => {
                let start = self.current_offset();
                self.lower_nodes(body);
                self.emit(Instruction::Return { src: None });
                let end = self.current_offset();
                self.module.funcs.entries.push(FuncEntry {
                    name:       name.clone(),
                                               start_insn: start,
                                               insn_count: end - start,
                });
            }

            Node::FuncCall { name } => {
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::CallFunc { name: name_idx });
            }

            Node::QuickCall { name, args } => {
                let arg_reg = self.lower_string_parts(args);
                let dst     = self.alloc_reg();
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::CallQuick { name: name_idx, arg: arg_reg, dst });
            }

            Node::HackerOsApi { tool, args } => {
                let args_reg = self.lower_string_parts(args);
                let dst      = self.alloc_reg();
                let tool_name = tool.binary_name();
                let tool_idx = self.module.consts.add_str(tool_name);
                self.emit(Instruction::HackerOsCall { tool: tool_idx, args: args_reg, dst });
            }

            Node::RepeatN { count, body } => {
                // Unroll małych pętli (≤4), resztę kompiluj jako loop
                if *count <= 4 {
                    for _ in 0..*count {
                        self.lower_nodes(body);
                    }
                } else {
                    let counter_reg = self.alloc_reg();
                    let limit_reg   = self.alloc_reg();
                    let limit_idx   = self.module.consts.add_num(*count as f64);
                    let zero_idx    = self.module.consts.add_num(0.0);
                    let one_idx     = self.module.consts.add_num(1.0);
                    self.emit(Instruction::LoadNum { dst: counter_reg, idx: zero_idx });
                    self.emit(Instruction::LoadNum { dst: limit_reg,   idx: limit_idx });

                    let loop_start = self.current_offset();
                    let cmp_reg    = self.alloc_reg();
                    self.emit(Instruction::CmpLt { dst: cmp_reg, a: counter_reg, b: limit_reg });
                    let exit_ph = self.emit_jump_placeholder(Some(cmp_reg));

                    self.lower_nodes(body);

                    let one_reg = self.alloc_reg();
                    self.emit(Instruction::LoadNum { dst: one_reg, idx: one_idx });
                    self.emit(Instruction::Add { dst: counter_reg, a: counter_reg, b: one_reg });
                    self.emit(Instruction::Jump { offset: loop_start });

                    let after = self.current_offset();
                    self.patch_jump(exit_ph, after);
                }
            }

            Node::HshCommand { raw } => {
                // hsh -c "cmd" — traktuj jak zwykłą komendę z trybem Plain
                let cmd_str = format!("hsh -c {}", raw);
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(&cmd_str);
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst = self.alloc_reg();
                self.emit(Instruction::ExecCmd { cmd: cmd_reg, mode: CmdMode::Plain, dst });
                let le_idx = self.module.consts.add_str("_last_exit_code");
                self.emit(Instruction::SetVar { name: le_idx, src: dst });
            }

            Node::Background { raw } => {
                let cmd_str = format!("& {}", raw);
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(&cmd_str);
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst = self.alloc_reg();
                self.emit(Instruction::ExecCmd { cmd: cmd_reg, mode: CmdMode::Plain, dst });
            }

            Node::FileImport { path, .. } => {
                // Inline import — w kompilatorze oznaczamy placeholder
                // Runtime JIT obsługuje FileImport bezpośrednio
                let cmd_str = format!("__hl_import__ {}", path);
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(&cmd_str);
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst = self.alloc_reg();
                self.emit(Instruction::ExecCmd { cmd: cmd_reg, mode: CmdMode::Plain, dst });
            }

            Node::Import { .. } | Node::Dependency { .. } => {
                // Resolved at load-time przez JIT runtime
            }

            Node::Goroutine { name: _, body } => {
                // Goroutines są emitowane jako osobne bloki — JIT spawnuje wątek
                self.lower_nodes(body);
            }

            Node::Channel { name } => {
                let chan_reg = self.alloc_reg();
                let nil_idx = self.module.consts.add_str("");
                self.emit(Instruction::LoadStr { dst: chan_reg, idx: nil_idx });
                let chan_var = format!("__chan_{}", name);
                let name_idx = self.module.consts.add_str(&chan_var);
                self.emit(Instruction::SetVar { name: name_idx, src: chan_reg });
            }

            Node::ChannelOp { name, value } => {
                let chan_var = format!("__chan_{}", name);
                let name_idx = self.module.consts.add_str(&chan_var);
                if let Some(parts) = value {
                    let src = self.lower_string_parts(parts);
                    self.emit(Instruction::SetVar { name: name_idx, src });
                } else {
                    let dst = self.alloc_reg();
                    self.emit(Instruction::GetVar { dst, name: name_idx });
                    self.emit(Instruction::Print { src: dst });
                }
            }

            Node::Block(nodes) => self.lower_nodes(nodes),

            // ── Arena functions (gen 2) ──────────────────────────────────────
            //
            // ArenaFuncDef: kompilujemy ciało jak zwykłą funkcję.
            // Arena allocation dzieje się w runtime (executor), nie w bytecode.
            Node::ArenaFuncDef { name, body, .. } => {
                let start = self.current_offset();
                self.lower_nodes(body);
                self.emit(Instruction::Return { src: None });
                let end = self.current_offset();
                self.module.funcs.entries.push(FuncEntry {
                    name:       format!("__arena__{}", name),
                                               start_insn: start,
                                               insn_count: end - start,
                });
            }

            // ArenaFuncCall: wywołujemy skompilowane ciało jak funkcję.
            // Prefiks __arena__ odróżnia od zwykłych funkcji (dla JIT/runtime).
            Node::ArenaFuncCall { name, args } => {
                let arg_reg = self.lower_string_parts(args);
                let args_idx = self.module.consts.add_str("_arena_args");
                self.emit(Instruction::SetVar { name: args_idx, src: arg_reg });
                let fn_name = format!("__arena__{}", name);
                let name_idx = self.module.consts.add_str(&fn_name);
                self.emit(Instruction::CallFunc { name: name_idx });
            }
        }
    }

    // ── Pomocniki ────────────────────────────────────────────────

    fn lower_string_parts(&mut self, parts: &[StringPart]) -> Reg {
        if parts.is_empty() {
            let dst = self.alloc_reg();
            let idx = self.module.consts.add_str("");
            self.emit(Instruction::LoadStr { dst, idx });
            return dst;
        }
        if parts.len() == 1 {
            return self.lower_string_part(&parts[0]);
        }
        // Wieloczęściowy — concat
        let part_regs: Vec<Reg> = parts.iter()
        .map(|p| self.lower_string_part(p))
        .collect();
        let dst = self.alloc_reg();
        self.emit(Instruction::Concat { dst, parts: part_regs });
        dst
    }

    fn lower_string_part(&mut self, part: &StringPart) -> Reg {
        match part {
            StringPart::Literal(s) => {
                let dst = self.alloc_reg();
                let idx = self.module.consts.add_str(s.as_str());
                self.emit(Instruction::LoadStr { dst, idx });
                dst
            }
            StringPart::Var(name) => {
                let dst = self.alloc_reg();
                let name_idx = self.module.consts.add_str(name.as_str());
                self.emit(Instruction::GetVar { dst, name: name_idx });
                let str_dst = self.alloc_reg();
                self.emit(Instruction::ToString { dst: str_dst, src: dst });
                str_dst
            }
        }
    }

    fn lower_var_value(&mut self, value: &VarValue) -> Reg {
        match value {
            VarValue::String(s) => {
                let dst = self.alloc_reg();
                let idx = self.module.consts.add_str(s.as_str());
                self.emit(Instruction::LoadStr { dst, idx });
                dst
            }
            VarValue::Int(n) => {
                let dst = self.alloc_reg();
                let idx = self.module.consts.add_num(*n as f64);
                self.emit(Instruction::LoadNum { dst, idx });
                dst
            }
            VarValue::Float(n) | VarValue::Number(n) => {
                let dst = self.alloc_reg();
                let idx = self.module.consts.add_num(*n);
                self.emit(Instruction::LoadNum { dst, idx });
                dst
            }
            VarValue::Bool(b) => {
                let dst = self.alloc_reg();
                self.emit(Instruction::LoadBool { dst, val: *b });
                dst
            }
            VarValue::Interpolated(parts) => {
                self.lower_string_parts(parts)
            }
            VarValue::Arithmetic(expr) => {
                self.lower_arithmetic(expr)
            }
            VarValue::CmdOutput(cmd) => {
                let cmd_reg = self.alloc_reg();
                let cmd_idx = self.module.consts.add_str(cmd.as_str());
                self.emit(Instruction::LoadStr { dst: cmd_reg, idx: cmd_idx });
                let dst_ec  = self.alloc_reg();
                let dst_out = self.alloc_reg();
                self.emit(Instruction::ExecCapture {
                    cmd: cmd_reg, mode: CmdMode::Plain, dst_ec, dst_out,
                });
                dst_out
            }
            VarValue::List(_) | VarValue::Map(_) => {
                // Uproszczenie: listy/mapy traktuj jak pusty string w BC (nie używane w gen 2 skryptach)
                let dst = self.alloc_reg();
                let idx = self.module.consts.add_str("");
                self.emit(Instruction::LoadStr { dst, idx });
                dst
            }
        }
    }

    /// Kompiluj wyrażenie arytmetyczne do rejestru wynikowego
    /// Obsługuje: liczby, @zmienne, +, -, *, /, %, nawiasy
    fn lower_arithmetic(&mut self, expr: &str) -> Reg {
        let expr = expr.trim();
        // Spróbuj skompilować wyrażenie do instrukcji arytmetycznych
        if let Some(reg) = self.try_compile_arith_expr(expr) {
            return reg;
        }
        // Fallback: traktuj jako string (shell fallback w runtime)
        let dst = self.alloc_reg();
        let idx = self.module.consts.add_str(expr);
        self.emit(Instruction::LoadStr { dst, idx });
        dst
    }

    fn try_compile_arith_expr(&mut self, expr: &str) -> Option<Reg> {
        let expr = expr.trim();

        // Liczba literalna
        if let Ok(n) = expr.parse::<f64>() {
            let dst = self.alloc_reg();
            let idx = self.module.consts.add_num(n);
            self.emit(Instruction::LoadNum { dst, idx });
            return Some(dst);
        }

        // Zmienna @name
        if expr.starts_with('@') {
            let name = &expr[1..];
            let dst = self.alloc_reg();
            let name_idx = self.module.consts.add_str(name);
            self.emit(Instruction::GetVar { dst, name: name_idx });
            let num_dst = self.alloc_reg();
            self.emit(Instruction::ToNumber { dst: num_dst, src: dst });
            return Some(num_dst);
        }

        // Nawiasy
        if expr.starts_with('(') && expr.ends_with(')') {
            return self.try_compile_arith_expr(&expr[1..expr.len()-1]);
        }

        // Szukaj operatora na najniższym poziomie priorytetu (additive)
        if let Some((left, op, right)) = find_binary_op_split(expr) {
            let l = self.try_compile_arith_expr(left)?;
            let r = self.try_compile_arith_expr(right)?;
            let dst = self.alloc_reg();
            let insn = match op {
                '+' => Instruction::Add { dst, a: l, b: r },
                '-' => Instruction::Sub { dst, a: l, b: r },
                '*' => Instruction::Mul { dst, a: l, b: r },
                '/' => Instruction::Div { dst, a: l, b: r },
                '%' => Instruction::Mod { dst, a: l, b: r },
                _   => return None,
            };
            self.emit(insn);
            return Some(dst);
        }

        None
    }
}

/// Znajdź binarny operator z uwzględnieniem priorytetu i nawiasów
fn find_binary_op_split(expr: &str) -> Option<(&str, char, &str)> {
    let bytes = expr.as_bytes();
    let mut depth = 0i32;

    // Additive (najniższy priorytet — szukamy od końca)
    let mut best_add: Option<usize> = None;
    let mut best_sub: Option<usize> = None;

    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'+' if depth == 0 && i > 0 => { best_add = Some(i); break; }
            b'-' if depth == 0 && i > 0 => { best_sub = Some(i); break; }
            _ => {}
        }
    }

    let additive = match (best_add, best_sub) {
        (Some(a), Some(b)) => if a > b { Some((a, '+')) } else { Some((b, '-')) },
        (Some(a), None)    => Some((a, '+')),
        (None, Some(b))    => Some((b, '-')),
        _                  => None,
    };

    if let Some((pos, op)) = additive {
        return Some((&expr[..pos], op, &expr[pos+1..]));
    }

    // Multiplicative
    depth = 0;
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            b'*' if depth == 0 => return Some((&expr[..i], '*', &expr[i+1..])),
            b'/' if depth == 0 => return Some((&expr[..i], '/', &expr[i+1..])),
            b'%' if depth == 0 => return Some((&expr[..i], '%', &expr[i+1..])),
            _ => {}
        }
    }

    None
}

fn lower_cmd_mode(mode: &CommandMode) -> CmdMode {
    match mode {
        CommandMode::Plain            => CmdMode::Plain,
        CommandMode::Sudo             => CmdMode::Sudo,
        CommandMode::Isolated         => CmdMode::Isolated,
        CommandMode::IsolatedSudo     => CmdMode::IsolatedSudo,
        CommandMode::WithVars         => CmdMode::WithVars,
        CommandMode::WithVarsSudo     => CmdMode::WithVarsSudo,
        CommandMode::WithVarsIsolated => CmdMode::WithVarsIsolated,
    }
}

/// Główna funkcja lowering
pub fn lower_ast(nodes: &[Node], source_path: &Path, gen: u32) -> HlModule {
    let path_str = source_path.display().to_string();
    let mut lowerer = Lowerer::new(&path_str, gen);
    lowerer.lower_nodes(nodes);
    lowerer.emit(Instruction::Return { src: None });
    lowerer.module.main_regs = lowerer.reg_alloc;
    lowerer.module
}
