use cranelift_codegen::settings::Configurable;
use anyhow::Result;
use std::path::Path;
use cranelift_codegen::ir::{types, AbiParam, Function, InstBuilder, UserFuncName};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::{isa, settings, Context};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;
use super::ir::*;
use crate::OptLevel;

pub fn emit_object(prog: &HlProgram, output: &Path, opt: OptLevel, verbose: bool) -> Result<()> {
    let mut flag_builder = settings::builder();
    match opt {
        OptLevel::None  => { flag_builder.set("opt_level", "none").ok(); }
        OptLevel::Speed => { flag_builder.set("opt_level", "speed").ok(); }
        OptLevel::Size  => { flag_builder.set("opt_level", "speed_and_size").ok(); }
    }
    flag_builder.set("is_pic", "false").ok();
    flag_builder.set("enable_verifier", "false").ok();

    let flags = settings::Flags::new(flag_builder);
    let isa_builder = isa::lookup(Triple::host())
        .map_err(|e| anyhow::anyhow!("Cranelift ISA lookup failed: {}", e))?;
    let isa = isa_builder.finish(flags)
        .map_err(|e| anyhow::anyhow!("Cranelift ISA finish failed: {}", e))?;

    let obj_builder = ObjectBuilder::new(isa, "hl_program", cranelift_module::default_libcall_names())
        .map_err(|e| anyhow::anyhow!("ObjectBuilder failed: {}", e))?;

    let mut module = ObjectModule::new(obj_builder);
    let rt = RuntimeSymbols::declare(&mut module)?;
    let string_data_ids = emit_string_pool(prog, &mut module)?;

    let mut func_ids = Vec::new();
    for func in &prog.functions {
        let id = emit_function(func, prog, &rt, &string_data_ids, &mut module, verbose)?;
        func_ids.push((func.name.clone(), id));
    }

    emit_entry(&prog.deps, &prog.string_pool, &string_data_ids, &rt, &mut module, &func_ids)?;

    let product = module.finish();
    let bytes   = product.emit()
        .map_err(|e| anyhow::anyhow!("Object emit failed: {}", e))?;
    std::fs::write(output, &bytes)
        .map_err(|e| anyhow::anyhow!("Write object failed: {}", e))?;

    if verbose { eprintln!("  Object: {} bytes", bytes.len()); }
    Ok(())
}

fn emit_string_pool(prog: &HlProgram, module: &mut ObjectModule) -> Result<Vec<cranelift_module::DataId>> {
    let mut ids = Vec::new();
    for s in &prog.string_pool {
        let mut desc = DataDescription::new();
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        desc.define(bytes.into_boxed_slice());
        let id = module.declare_anonymous_data(false, false)
            .map_err(|e| anyhow::anyhow!("Data declare failed: {}", e))?;
        module.define_data(id, &desc)
            .map_err(|e| anyhow::anyhow!("Data define failed: {}", e))?;
        ids.push(id);
    }
    Ok(ids)
}

struct RuntimeSymbols {
    hl_print:        cranelift_module::FuncId,
    hl_print_interp: cranelift_module::FuncId,
    hl_run_cmd:      cranelift_module::FuncId,
    hl_run_background: cranelift_module::FuncId,
    hl_set_var:      cranelift_module::FuncId,
    hl_set_var_interp: cranelift_module::FuncId,
    hl_export_var:        cranelift_module::FuncId,
    hl_export_var_interp: cranelift_module::FuncId,
    hl_export_list:  cranelift_module::FuncId,
    hl_quick:        cranelift_module::FuncId,
    hl_dep_check:    cranelift_module::FuncId,
    hl_get_last_exit: cranelift_module::FuncId,
}

impl RuntimeSymbols {
    fn declare(module: &mut ObjectModule) -> Result<Self> {
        let ptr  = types::I64;
        let i32t = types::I32;
        let _i64t = types::I64;

        let hl_print         = decl_fn(module, "hl_print",         &[ptr],         &[])?;
        let hl_print_interp  = decl_fn(module, "hl_print_interp",  &[ptr],         &[])?;
        let hl_run_cmd       = decl_fn(module, "hl_run_cmd",       &[ptr, i32t],   &[i32t])?;
        let hl_run_background= decl_fn(module, "hl_run_background",&[ptr],         &[i32t])?;
        let hl_set_var       = decl_fn(module, "hl_set_var",       &[ptr, ptr],    &[])?;
        let hl_set_var_interp= decl_fn(module, "hl_set_var_interp",&[ptr, ptr],    &[])?;
        let hl_export_var    = decl_fn(module, "hl_export_var",    &[ptr, ptr],    &[])?;
        let hl_export_var_interp = decl_fn(module, "hl_export_var_interp", &[ptr, ptr], &[])?;
        let hl_export_list   = decl_fn(module, "hl_export_list",   &[ptr, ptr, i32t], &[])?;
        let hl_quick         = decl_fn(module, "hl_quick",         &[ptr, ptr],    &[i32t])?;
        let hl_dep_check     = decl_fn(module, "hl_dep_check",     &[ptr],         &[i32t])?;
        let hl_get_last_exit = decl_fn(module, "hl_get_last_exit", &[],            &[i32t])?;

        Ok(Self {
            hl_print, hl_print_interp, hl_run_cmd, hl_run_background,
            hl_set_var, hl_set_var_interp,
            hl_export_var, hl_export_var_interp, hl_export_list,
            hl_quick, hl_dep_check, hl_get_last_exit,
        })
    }
}

fn decl_fn(module: &mut ObjectModule, name: &str, params: &[types::Type], returns: &[types::Type]) -> Result<cranelift_module::FuncId> {
    let mut sig = module.make_signature();
    for &p in params  { sig.params.push(AbiParam::new(p)); }
    for &r in returns { sig.returns.push(AbiParam::new(r)); }
    module.declare_function(name, Linkage::Import, &sig)
        .map_err(|e| anyhow::anyhow!("Declare '{}' failed: {}", name, e))
}

fn emit_function(
    func: &HlFunction, prog: &HlProgram, rt: &RuntimeSymbols,
    string_ids: &[cranelift_module::DataId], module: &mut ObjectModule, _verbose: bool,
) -> Result<cranelift_module::FuncId> {
    let sig = module.make_signature();
    let linkage = if func.name == "__hl_main" { Linkage::Export } else { Linkage::Local };
    let func_id = module.declare_function(&func.name, linkage, &sig)
        .map_err(|e| anyhow::anyhow!("Declare func '{}': {}", func.name, e))?;

    let mut cl_func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    let mut builder  = FunctionBuilder::new(&mut cl_func, &mut func_ctx);

    let entry_block = builder.create_block();
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);

    emit_instrs(&func.instrs, prog, rt, string_ids, module, &mut builder)?;

    builder.ins().return_(&[]);
    builder.finalize();

    let mut ctx = Context::for_function(cl_func);
    module.define_function(func_id, &mut ctx)
        .map_err(|e| anyhow::anyhow!("Define func '{}': {}", func.name, e))?;

    Ok(func_id)
}

fn emit_instrs(instrs: &[HlInstr], prog: &HlProgram, rt: &RuntimeSymbols,
    string_ids: &[cranelift_module::DataId], module: &mut ObjectModule,
    builder: &mut FunctionBuilder) -> Result<()>
{
    for instr in instrs {
        emit_instr(instr, prog, rt, string_ids, module, builder)?;
    }
    Ok(())
}

fn emit_instr(instr: &HlInstr, prog: &HlProgram, rt: &RuntimeSymbols,
    string_ids: &[cranelift_module::DataId], module: &mut ObjectModule,
    builder: &mut FunctionBuilder) -> Result<()>
{
    match instr {
        HlInstr::Nop => {}

        HlInstr::Print { idx } => {
            let ptr = get_str_ptr(*idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_print, builder.func);
            builder.ins().call(fref, &[ptr]);
        }

        HlInstr::PrintInterp { idx } => {
            let ptr = get_str_ptr(*idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_print_interp, builder.func);
            builder.ins().call(fref, &[ptr]);
        }

        HlInstr::RunCmd { cmd_idx, mode } => {
            let ptr  = get_str_ptr(*cmd_idx, string_ids, module, builder)?;
            let mode_val = builder.ins().iconst(types::I32, *mode as i64);
            let fref = module.declare_func_in_func(rt.hl_run_cmd, builder.func);
            builder.ins().call(fref, &[ptr, mode_val]);
        }

        HlInstr::RunBackground { cmd_idx } => {
            let ptr  = get_str_ptr(*cmd_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_run_background, builder.func);
            builder.ins().call(fref, &[ptr]);
        }

        HlInstr::RunHsh { cmd_idx } => {
            // *> — uruchom przez hsh (mode=10)
            let ptr  = get_str_ptr(*cmd_idx, string_ids, module, builder)?;
            let mode_val = builder.ins().iconst(types::I32, 10i64); // CmdMode::Hsh
            let fref = module.declare_func_in_func(rt.hl_run_cmd, builder.func);
            builder.ins().call(fref, &[ptr, mode_val]);
        }

        HlInstr::RepeatN { count, body } => {
            // Prosta petla przez loop block
            let loop_block = builder.create_block();
            let exit_block = builder.create_block();

            // Inicjalizuj licznik
            let _count_val = builder.ins().iconst(types::I64, *count as i64);
            let counter   = builder.ins().iconst(types::I64, 0i64);
            let _ = counter; // uproszczenie — pelna impl wymaga SSA phi-nodes

            builder.ins().jump(loop_block, &[]);
            builder.switch_to_block(loop_block);
            // W pelnej impl: phi-node dla licznika, branch gdy == count
            // Uproszczenie: wykonaj body N razy bez prawdziwej petli (unroll dla malych N)
            // Dla duzych N: nalezy uzyc loop z licznikiem — TODO: pelna impl
            for _ in 0..(*count).min(16) {
                emit_instrs(body, prog, rt, string_ids, module, builder)?;
            }
            builder.ins().jump(exit_block, &[]);
            builder.switch_to_block(exit_block);
            builder.seal_block(loop_block);
            builder.seal_block(exit_block);
        }

        HlInstr::SetVar { name_idx, val_idx } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let val  = get_str_ptr(*val_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_set_var, builder.func);
            builder.ins().call(fref, &[name, val]);
        }

        HlInstr::SetVarInterp { name_idx, val_idx } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let val  = get_str_ptr(*val_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_set_var_interp, builder.func);
            builder.ins().call(fref, &[name, val]);
        }

        HlInstr::ExportVar { name_idx, val_idx } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let val  = get_str_ptr(*val_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_export_var, builder.func);
            builder.ins().call(fref, &[name, val]);
        }

        HlInstr::ExportVarInterp { name_idx, val_idx } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let tmpl = get_str_ptr(*val_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_export_var_interp, builder.func);
            builder.ins().call(fref, &[name, tmpl]);
        }

        HlInstr::ExportList { name_idx, items } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let _ = items;
            let fref = module.declare_func_in_func(rt.hl_export_var, builder.func);
            builder.ins().call(fref, &[name, name]); // placeholder
        }

        HlInstr::QuickCall { name_idx, args_idx } => {
            let name = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let args = get_str_ptr(*args_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_quick, builder.func);
            builder.ins().call(fref, &[name, args]);
        }

        HlInstr::CallFunc { func_idx } => {
            let _name = get_str_ptr(*func_idx, string_ids, module, builder)?;
            // TODO: resolve function by name at link time
        }

        HlInstr::CondOk { body } => {
            let fref_exit = module.declare_func_in_func(rt.hl_get_last_exit, builder.func);
            let call      = builder.ins().call(fref_exit, &[]);
            let exit_code = builder.inst_results(call)[0];
            let zero      = builder.ins().iconst(types::I32, 0);
            let cond      = builder.ins().icmp(IntCC::Equal, exit_code, zero);
            let then_block = builder.create_block();
            let cont_block = builder.create_block();
            builder.ins().brif(cond, then_block, &[], cont_block, &[]);
            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_instrs(body, prog, rt, string_ids, module, builder)?;
            builder.ins().jump(cont_block, &[]);
            builder.switch_to_block(cont_block);
            builder.seal_block(cont_block);
        }

        HlInstr::CondErr { body } => {
            let fref_exit = module.declare_func_in_func(rt.hl_get_last_exit, builder.func);
            let call      = builder.ins().call(fref_exit, &[]);
            let exit_code = builder.inst_results(call)[0];
            let zero      = builder.ins().iconst(types::I32, 0);
            let cond      = builder.ins().icmp(IntCC::NotEqual, exit_code, zero);
            let then_block = builder.create_block();
            let cont_block = builder.create_block();
            builder.ins().brif(cond, then_block, &[], cont_block, &[]);
            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            emit_instrs(body, prog, rt, string_ids, module, builder)?;
            builder.ins().jump(cont_block, &[]);
            builder.switch_to_block(cont_block);
            builder.seal_block(cont_block);
        }

        HlInstr::Dep { name_idx } => {
            let ptr  = get_str_ptr(*name_idx, string_ids, module, builder)?;
            let fref = module.declare_func_in_func(rt.hl_dep_check, builder.func);
            builder.ins().call(fref, &[ptr]);
        }

        HlInstr::Exit { code } => {
            let exit_fn = decl_exit(module)?;
            let fref    = module.declare_func_in_func(exit_fn, builder.func);
            let code_v  = builder.ins().iconst(types::I32, *code as i64);
            builder.ins().call(fref, &[code_v]);
        }
    }
    Ok(())
}

fn get_str_ptr(idx: u32, string_ids: &[cranelift_module::DataId], module: &mut ObjectModule, builder: &mut FunctionBuilder) -> Result<cranelift_codegen::ir::Value> {
    let data_id = string_ids.get(idx as usize)
        .ok_or_else(|| anyhow::anyhow!("String idx {} out of range", idx))?;
    let global = module.declare_data_in_func(*data_id, builder.func);
    Ok(builder.ins().global_value(types::I64, global))
}

fn decl_exit(module: &mut ObjectModule) -> Result<cranelift_module::FuncId> {
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    module.declare_function("exit", Linkage::Import, &sig)
        .map_err(|e| anyhow::anyhow!("Declare exit: {}", e))
}

fn emit_entry(
    _deps: &[String], _pool: &[String],
    _string_ids: &[cranelift_module::DataId], _rt: &RuntimeSymbols,
    module: &mut ObjectModule, func_ids: &[(String, cranelift_module::FuncId)],
) -> Result<()> {
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(types::I32));
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I32));

    let main_id = module.declare_function("main", Linkage::Export, &sig)
        .map_err(|e| anyhow::anyhow!("Declare main: {}", e))?;

    let init_sig = module.make_signature();
    let init_id = module.declare_function("hl_runtime_init", Linkage::Import, &init_sig)
        .map_err(|e| anyhow::anyhow!("Declare hl_runtime_init: {}", e))?;

    let shut_sig = module.make_signature();
    let shut_id = module.declare_function("hl_runtime_shutdown", Linkage::Import, &shut_sig)
        .map_err(|e| anyhow::anyhow!("Declare hl_runtime_shutdown: {}", e))?;

    let hl_main_id = func_ids.iter()
        .find(|(name, _)| name == "__hl_main")
        .map(|(_, id)| *id)
        .ok_or_else(|| anyhow::anyhow!("__hl_main not found"))?;

    let mut cl_func = Function::with_name_signature(UserFuncName::user(0, main_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    let mut builder  = FunctionBuilder::new(&mut cl_func, &mut func_ctx);

    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);

    let init_ref = module.declare_func_in_func(init_id, builder.func);
    builder.ins().call(init_ref, &[]);

    let main_ref = module.declare_func_in_func(hl_main_id, builder.func);
    builder.ins().call(main_ref, &[]);

    let shut_ref = module.declare_func_in_func(shut_id, builder.func);
    builder.ins().call(shut_ref, &[]);

    let zero = builder.ins().iconst(types::I32, 0);
    builder.ins().return_(&[zero]);
    builder.finalize();

    let mut ctx = Context::for_function(cl_func);
    module.define_function(main_id, &mut ctx)
        .map_err(|e| anyhow::anyhow!("Define main: {}", e))?;

    Ok(())
}
