use cranelift_codegen::ir::{types, AbiParam, Endianness, InstBuilder, MemFlags, Signature, Value};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::Codegen::TIR::{
    self, JitProgram, TFunc, TFuncKind, TJitSpawnBody, TJitSpawnLambda, TLambda, TLambdaBody, TStmt,
};
use jet_foundation::AST::Type;
use std::collections::HashMap;

use super::lower_ctx::LowerCtx;
use super::runtime_host::HostFns;
use super::types_meta::{
    clif_ty, fn_value_signature, func_has_receiver, func_signature, jit_fn_name, JitMeta,
};
use super::JitRuntime;
use crate::{Cell, Collections};

fn register_packed_enum_show_table(meta: &JitMeta<'_>) {
    Collections::clear_packed_enum_show();
    for enum_name in meta.enum_names() {
        if !meta.enum_packed_showable(enum_name) {
            continue;
        }
        let Some(variants) = meta.enum_variant_names(enum_name) else {
            continue;
        };
        let mut rows = Vec::new();
        for variant in variants {
            let vname = variant.strip_prefix("user_").unwrap_or(variant.as_str());
            let payloads = meta.enum_variant_payload_types(enum_name, vname).unwrap_or(&[]);
            let (kind, nested) = match payloads {
                [] => (0u8, String::new()),
                [Type::Int] => (1u8, String::new()),
                [Type::Named(inner)] => (2u8, inner.clone()),
                [Type::String] => (3u8, String::new()),
                _ => continue,
            };
            // I2: print the Jet-source variant name, not the mangled `user_` form.
            rows.push((vname.to_string(), kind, nested));
        }
        Collections::register_packed_enum_show(enum_name, rows);
    }
}

fn pack_spawn_return(
    b: &mut FunctionBuilder,
    val: Value,
    ret_ty: &Type,
) -> Result<Value, String> {
    match clif_ty(ret_ty) {
        Some(ty) if ty == types::F64 => Ok(b.ins().bitcast(
            types::I64,
            MemFlags::new().with_endianness(Endianness::Little),
            val,
        )),
        Some(ty) if ty == types::I8 || ty == types::I32 => Ok(b.ins().uextend(types::I64, val)),
        Some(ty) if ty == types::I64 => Ok(val),
        None => Ok(val),
        other => Err(format!("jit spawn return unsupported: {ret_ty:?} ({other:?})")),
    }
}

fn spawn_lambda_signature(module: &JITModule, lam: &TJitSpawnLambda) -> Signature {
    let cc = module.target_config().default_call_conv;
    let mut sig = Signature::new(cc);
    for _ in &lam.captures {
        sig.params.push(AbiParam::new(types::I64));
    }
    for (_, ty) in &lam.params {
        sig.params
            .push(AbiParam::new(clif_ty(ty).unwrap_or(types::I64)));
    }
    if clif_ty(&lam.ret).is_some() {
        sig.returns.push(AbiParam::new(types::I64));
    }
    sig
}

fn lower_spawn_function(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
    lam: &TJitSpawnLambda,
    func_id: FuncId,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    runtime: &mut JitRuntime,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = spawn_lambda_signature(module, lam);
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    let mut var_tys = HashMap::new();
    let mut spawn_site = 0usize;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let param_vals = b.block_params(entry).to_vec();
        let mut idx = 0usize;
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            var_tys: &mut var_tys,
            raw_slots: HashMap::new(),
            func_ids,
            spawn_site: &mut spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct: None,
            ret_clif: if clif_ty(&lam.ret).is_some() {
                // Spawn bodies use a packed i64 ABI (Float → bitcast bits).
                Some(types::I64)
            } else {
                None
            },
            ret_range: false,
            ret_cell_layout: 0,
            cell_frame: false,
            shield_depth: 0,
            deadline_depth: 0,
            switch_subject: None,
            yield_sender: None,
            in_shared_transaction: false,
            shared_transaction_depth: 0,
            unsafe_depth: 0,
            scope_guards: Vec::new(),
            deferred_closes: Vec::new(),
            deferred_shared_guards: Vec::new(),
            task_groups: Vec::new(),
            in_lexical_exit: false,
            txn_stack: Vec::new(),
        };
        for cap in &lam.captures {
            let var = lctx.fresh_var(types::I64);
            lctx.b.def_var(var, param_vals[idx]);
            lctx.vars.insert(TIR::local_place(&cap.name), var);
            lctx.var_tys
                .insert(TIR::local_place(&cap.name), cap.ty.clone());
            idx += 1;
        }
        for (name, ty) in &lam.params {
            let clif = clif_ty(ty).unwrap_or(types::I64);
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[idx]);
            lctx.vars.insert(TIR::local_place(name), var);
            lctx.var_tys.insert(TIR::local_place(name), ty.clone());
            idx += 1;
        }
        match &lam.body {
            TJitSpawnBody::Expr(e) => {
                let val = lctx.lower_expr(e)?;
                if clif_ty(&lam.ret).is_some() {
                    let packed = pack_spawn_return(lctx.b, val, &lam.ret)?;
                    lctx.emit_lexical_exit(Some(packed), false, lctx.shield_depth)?;
                } else {
                    let _ = val;
                    lctx.emit_lexical_exit(None, false, lctx.shield_depth)?;
                }
            }
            TJitSpawnBody::Block { prefix, tail } => {
                lctx.lower_stmts(prefix)?;
                if let Some(t) = tail {
                    let val = lctx.lower_expr(t)?;
                    if clif_ty(&lam.ret).is_some() {
                        let packed = pack_spawn_return(lctx.b, val, &lam.ret)?;
                        lctx.emit_lexical_exit(Some(packed), false, lctx.shield_depth)?;
                    } else {
                        let _ = val;
                        lctx.emit_lexical_exit(None, false, lctx.shield_depth)?;
                    }
                } else {
                    lctx.emit_lexical_exit(None, false, lctx.shield_depth)?;
                }
            }
        }
        b.finalize();
    }
    if let Err(e) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
        return Err(format!("spawn body verifier: {e:?}"));
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("define spawn body: {e:?}"))?;
    // Spawn bodies are named jet_jit_spawn_body_N at declare time; recover via FuncId scan.
    let export = (0..64)
        .find_map(|i| {
            let name = format!("jet_jit_spawn_body_{i}");
            match module.get_name(&name) {
                Some(cranelift_module::FuncOrDataId::Func(id)) if id == func_id => Some(name),
                _ => None,
            }
        })
        .unwrap_or_else(|| "jet_jit_spawn_body".to_string());
    super::tier_cache::note_defined(&export, &ctx);
    module.clear_context(&mut ctx);
    Ok(())
}

fn block_has_valued_return(stmts: &[TStmt]) -> bool {
    fn walk(stmts: &[TStmt]) -> bool {
        for stmt in stmts {
            match stmt {
                TStmt::Return(Some(_)) => return true,
                TStmt::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if walk(then_body) || else_body.as_ref().is_some_and(|b| walk(b)) {
                        return true;
                    }
                }
                TStmt::EnumMatch {
                    arms,
                    else_body,
                    ..
                } => {
                    if arms.iter().any(|arm| walk(&arm.body))
                        || else_body.as_ref().is_some_and(|b| walk(b))
                    {
                        return true;
                    }
                }
                TStmt::Loop { body, .. } | TStmt::While { body, .. } => {
                    if walk(body) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    walk(stmts)
}

pub(crate) fn lower_callable_lambda(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
    lam: &TLambda,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    spawn_site: &mut usize,
    runtime: &mut JitRuntime,
) -> Result<FuncId, String> {
    let capturing = !lam.captures.is_empty();
    // Capturing callables are supported when every capture is an i64 handle/scalar
    // (HTTPHandler middleware closures). Prep is AOT-only Rust clone text.
    if !lam.prep.is_empty() && !capturing {
        return Err("jit callable captures unsupported".to_string());
    }
    let fn_ty = Type::Fn {
        params: lam.param_types.clone(),
        ret: lam.ret.clone().map(Box::new),
        effect_bound: None,
        return_view_provenance: None,
    };
    let block_returns_value = match &lam.executable {
        TLambdaBody::Block(stmts) => block_has_valued_return(stmts),
        TLambdaBody::Expr(_) => false,
    };
    let sig = if capturing {
        let mut sig = fn_value_signature(module, &fn_ty, meta)?;
        sig.params.insert(0, AbiParam::new(types::I64)); // env
        // Block-bodied lambdas often type as Unit when arms `return` Result;
        // Cranelift still needs the real return ABI.
        if (lam.ret.is_some() || block_returns_value) && sig.returns.is_empty() {
            sig.returns.push(AbiParam::new(types::I64));
        }
        sig
    } else {
        fn_value_signature(module, &fn_ty, meta)?
    };
    let id = module
        .declare_function(&lam.jit_name, Linkage::Local, &sig)
        .map_err(|error| error.to_string())?;
    let mut ctx = module.make_context();
    ctx.func.signature = sig;
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    let mut var_tys = HashMap::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let values = b.block_params(entry).to_vec();
        let ret_clif = lam
            .ret
            .as_ref()
            .and_then(|ret| meta.clif_ty(ret))
            .or_else(|| {
                if block_returns_value {
                    Some(types::I64)
                } else {
                    None
                }
            });
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            var_tys: &mut var_tys,
            raw_slots: HashMap::new(),
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct: None,
            ret_clif,
            ret_range: false,
            ret_cell_layout: 0,
            cell_frame: false,
            shield_depth: 0,
            deadline_depth: 0,
            switch_subject: None,
            yield_sender: None,
            in_shared_transaction: false,
            shared_transaction_depth: 0,
            unsafe_depth: 0,
            scope_guards: Vec::new(),
            deferred_closes: Vec::new(),
            deferred_shared_guards: Vec::new(),
            task_groups: Vec::new(),
            in_lexical_exit: false,
            txn_stack: Vec::new(),
        };
        let mut arg_i = 0usize;
        if capturing {
            let env = values[arg_i];
            arg_i += 1;
            let line = lctx.b.ins().iconst(types::I32, 0);
            for (idx, (_outer, place, ty)) in lam.captures.iter().enumerate() {
                let idx_v = lctx.b.ins().iconst(types::I64, idx as i64);
                let host = lctx
                    .module
                    .declare_func_in_func(lctx.host.coll.list_get, lctx.b.func);
                let call = lctx.b.ins().call(host, &[env, idx_v, line]);
                let val = lctx.b.inst_results(call)[0];
                let clif = meta.clif_ty(ty).unwrap_or(types::I64);
                let var = lctx.fresh_var(clif);
                lctx.b.def_var(var, val);
                lctx.vars.insert(place.clone(), var);
                lctx.var_tys.insert(place.clone(), ty.clone());
            }
        }
        for (name, ty) in lam.source_params.iter().zip(&lam.param_types) {
            let clif = meta
                .clif_ty(ty)
                .ok_or_else(|| format!("jit callable param unsupported: {ty:?}"))?;
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, values[arg_i]);
            arg_i += 1;
            let place = TIR::local_place(name);
            lctx.vars.insert(place.clone(), var);
            lctx.var_tys.insert(place, ty.clone());
        }
        match &lam.executable {
            TLambdaBody::Expr(expr) => {
                let value = lctx.lower_expr(expr)?;
                if lam.ret.is_some() {
                    lctx.emit_lexical_exit(Some(value), false, lctx.shield_depth)?;
                } else {
                    lctx.emit_lexical_exit(None, false, lctx.shield_depth)?;
                }
            }
            TLambdaBody::Block(stmts) => {
                lctx.lower_stmts(stmts)?;
                if !lctx.dead {
                    if lam.ret.is_some() {
                        return Err("jit callable block missing return".to_string());
                    }
                    lctx.emit_lexical_exit(None, false, lctx.shield_depth)?;
                }
            }
        }
        b.finalize();
    }
    cranelift_codegen::verify_function(&ctx.func, module.isa()).map_err(|error| {
        format!(
            "{}: verifier: {error:?} (ret={:?}, captures={}, sig_returns={})",
            lam.jit_name,
            lam.ret,
            lam.captures.len(),
            ctx.func.signature.returns.len()
        )
    })?;
    module
        .define_function(id, &mut ctx)
        .map_err(|error| error.to_string())?;
    module.clear_context(&mut ctx);
    Ok(id)
}

fn lower_function(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
    tir: &TFunc,
    func_id: FuncId,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    spawn_site: &mut usize,
    runtime: &mut JitRuntime,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = func_signature(module, tir, meta)?;
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    let mut var_tys = HashMap::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);

        let param_vals = b.block_params(entry).to_vec();
        let mut param_idx = 0usize;
        let (method_struct, self_type) = match &tir.kind {
            TFuncKind::Method { owner_type, .. } => (
                owner_type.base_name().map(str::to_string),
                Some(owner_type.clone()),
            ),
            TFuncKind::TraitMethod { .. } => {
                let owner = tir.name.split_once("::").map(|(t, _)| t.to_string());
                (owner.clone(), owner.map(Type::Named))
            }
            _ => (None, None),
        };
        let ret_cell_layout = tir
            .ret
            .as_ref()
            .map(|ret| Cell::CellGuardLayout::from_type(ret, meta))
            .transpose()?
            .flatten()
            .map(|layout| runtime.cells.register_guard_layout(layout))
            .unwrap_or(0);
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            var_tys: &mut var_tys,
            raw_slots: HashMap::new(),
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct,
            ret_clif: tir.ret.as_ref().and_then(|ret| meta.clif_ty(ret)),
            ret_range: tir.ret.as_ref().is_some_and(|ret| {
                matches!(ret, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE)
            }),
            ret_cell_layout,
            cell_frame: true,
            shield_depth: 0,
            deadline_depth: 0,
            switch_subject: None,
            yield_sender: None,
            in_shared_transaction: false,
            shared_transaction_depth: 0,
            unsafe_depth: 0,
            scope_guards: Vec::new(),
            deferred_closes: Vec::new(),
            deferred_shared_guards: Vec::new(),
            task_groups: Vec::new(),
            in_lexical_exit: false,
            txn_stack: Vec::new(),
        };
        let enter = lctx
            .module
            .declare_func_in_func(lctx.host.cell.frame_enter, lctx.b.func);
        lctx.b.ins().call(enter, &[]);
        if func_has_receiver(tir) {
            let self_var = lctx.fresh_var(types::I64);
            lctx.b.def_var(self_var, param_vals[0]);
            lctx.vars.insert("self".to_string(), self_var);
            if let Some(owner_type) = self_type {
                lctx.var_tys.insert("self".to_string(), owner_type);
            }
            param_idx = 1;
        }
        for (name, ty, convention) in &tir.params {
            if matches!(ty, Type::Named(range) if range == jet_foundation::Syntax::TYPE_RANGE) {
                let values = [
                    param_vals[param_idx],
                    param_vals[param_idx + 1],
                    param_vals[param_idx + 2],
                ];
                lctx.bind_range_local(name, values);
                param_idx += 3;
                continue;
            }
            let scalar_write = matches!(
                convention,
                jet_foundation::AST::AccessConvention::Write
            ) && matches!(
                ty,
                Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::Bool
                    | Type::Char
            );
            let clif = if scalar_write {
                types::I64
            } else {
                meta.clif_ty(ty).ok_or("jit param clif type")?
            };
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[param_idx]);
            param_idx += 1;
            lctx.vars.insert(name.clone(), var);
            let stored_ty = if scalar_write {
                Type::Apply {
                    name: "__JetScalarMut".to_string(),
                    args: vec![ty.clone()],
                }
            } else {
                ty.clone()
            };
            lctx.var_tys.insert(name.clone(), stored_ty);
        }

        lctx.lower_stmts(&tir.body)?;
        if !lctx.dead {
            let value = if let Some(ret) = &tir.ret {
                if matches!(ret, Type::Result { ok, err }
                    if matches!(ok.as_ref(), Type::Named(n) if n == "Void" || n == "Unit")
                        && matches!(err.as_ref(), Type::Named(n) if n == "Error"))
                {
                    let tag = lctx.b.ins().iconst(types::I8, 1);
                    let unit = lctx.b.ins().iconst(types::I64, 0);
                    let host_ref = lctx
                        .module
                        .declare_func_in_func(lctx.host.result_new_i64, lctx.b.func);
                    let call = lctx.b.ins().call(host_ref, &[tag, unit]);
                    Some(lctx.b.inst_results(call)[0])
                } else if clif_ty(ret).is_some()
                    || matches!(ret, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE)
                {
                    return Err("jit function missing return".to_string());
                } else {
                    None
                }
            } else {
                None
            };
            lctx.emit_lexical_exit(value, false, lctx.shield_depth)?;
        }
        b.finalize();
    }
    if let Err(e) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
        return Err(format!("{}: verifier: {e:?}", tir.name));
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| format!("define {}: {e:?}", tir.name))?;
    let export_name = if matches!(
        module.get_name("jet_jit_main"),
        Some(cranelift_module::FuncOrDataId::Func(id)) if id == func_id
    ) {
        "jet_jit_main".to_string()
    } else {
        super::types_meta::jit_fn_name(&tir.name)
    };
    super::tier_cache::note_defined(&export_name, &ctx);
    module.clear_context(&mut ctx);
    Ok(())
}

fn is_generator(tir: &TFunc) -> bool {
    matches!(&tir.ret, Some(Type::Apply { name, .. }) if name == "Stream")
}

fn generator_body_signature(module: &JITModule, tir: &TFunc) -> Result<Signature, String> {
    if func_has_receiver(tir) {
        return Err("jit generator methods unsupported".to_string());
    }
    let mut sig = Signature::new(module.target_config().default_call_conv);
    for (_, ty, _) in &tir.params {
        let clif = clif_ty(ty)
            .ok_or_else(|| format!("jit generator param unsupported: {ty:?}"))?;
        if clif != types::I64 {
            return Err(format!("jit generator param ABI unsupported: {ty:?}"));
        }
        sig.params.push(AbiParam::new(clif));
    }
    sig.params.push(AbiParam::new(types::I64));
    sig.returns.push(AbiParam::new(types::I64));
    Ok(sig)
}

fn lower_generator_body(
    module: &mut JITModule,
    host: &HostFns,
    meta: &JitMeta<'_>,
    tir: &TFunc,
    func_id: FuncId,
    func_ids: &HashMap<String, FuncId>,
    spawn_func_ids: &[FuncId],
    spawn_lambdas: &[TJitSpawnLambda],
    spawn_site: &mut usize,
    runtime: &mut JitRuntime,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = generator_body_signature(module, tir)?;
    let mut fbcx = FunctionBuilderContext::new();
    let mut vars = HashMap::new();
    let mut var_tys = HashMap::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let values = b.block_params(entry).to_vec();
        let sender = *values.last().ok_or("jit generator sender missing")?;
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            var_tys: &mut var_tys,
            raw_slots: HashMap::new(),
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct: None,
            ret_clif: Some(types::I64),
            ret_range: false,
            ret_cell_layout: 0,
            cell_frame: false,
            shield_depth: 0,
            deadline_depth: 0,
            switch_subject: None,
            yield_sender: Some(sender),
            in_shared_transaction: false,
            shared_transaction_depth: 0,
            unsafe_depth: 0,
            scope_guards: Vec::new(),
            deferred_closes: Vec::new(),
            deferred_shared_guards: Vec::new(),
            task_groups: Vec::new(),
            in_lexical_exit: false,
            txn_stack: Vec::new(),
        };
        for (index, (name, ty, _)) in tir.params.iter().enumerate() {
            let var = lctx.fresh_var(types::I64);
            lctx.b.def_var(var, values[index]);
            lctx.vars.insert(name.clone(), var);
            lctx.var_tys.insert(name.clone(), ty.clone());
        }
        lctx.lower_stmts(&tir.body)?;
        if !lctx.dead {
            let zero = lctx.b.ins().iconst(types::I64, 0);
            lctx.emit_lexical_exit(Some(zero), false, lctx.shield_depth)?;
        }
        b.finalize();
    }
    cranelift_codegen::verify_function(&ctx.func, module.isa())
        .map_err(|error| format!("{} generator: verifier: {error:?}", tir.name))?;
    module
        .define_function(func_id, &mut ctx)
        .map_err(|error| error.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
}

fn lower_generator_wrapper(
    module: &mut JITModule,
    host: &HostFns,
    tir: &TFunc,
    func_id: FuncId,
    body_id: FuncId,
    meta: &JitMeta<'_>,
) -> Result<(), String> {
    let mut ctx = module.make_context();
    ctx.func.signature = func_signature(module, tir, meta)?;
    let mut fbcx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        b.seal_block(entry);
        let params = b.block_params(entry).to_vec();
        if params.len() + 1 > 4 {
            return Err("jit generator capture count unsupported".to_string());
        }
        if params
            .iter()
            .any(|value| b.func.dfg.value_type(*value) != types::I64)
        {
            return Err("jit generator parameter ABI unsupported".to_string());
        }
        let new = module.declare_func_in_func(host.conc.generator_channel_new, b.func);
        let call = b.ins().call(new, &[]);
        let channel = b.inst_results(call)[0];
        let sender_fn = module.declare_func_in_func(host.conc.channel_sender, b.func);
        let call = b.ins().call(sender_fn, &[channel]);
        let sender = b.inst_results(call)[0];
        let body_ref = module.declare_func_in_func(body_id, b.func);
        let body_ptr = b.ins().func_addr(types::I64, body_ref);
        let mut spawn_args = vec![body_ptr];
        spawn_args.extend(params);
        spawn_args.push(sender);
        let spawn_id = match spawn_args.len() - 1 {
            1 => host.conc.spawn1,
            2 => host.conc.spawn2,
            3 => host.conc.spawn3,
            4 => host.conc.spawn4,
            _ => return Err("jit generator capture count unsupported".to_string()),
        };
        let spawn = module.declare_func_in_func(spawn_id, b.func);
        b.ins().call(spawn, &spawn_args);
        b.ins().return_(&[channel]);
        b.finalize();
    }
    cranelift_codegen::verify_function(&ctx.func, module.isa())
        .map_err(|error| format!("{} wrapper: verifier: {error:?}", tir.name))?;
    module
        .define_function(func_id, &mut ctx)
        .map_err(|error| error.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
}

pub(crate) fn compile_program(
    module: &mut JITModule,
    host: &HostFns,
    program: &JitProgram,
    runtime: &mut JitRuntime,
    existing_main: Option<FuncId>,
) -> Result<FuncId, String> {
    compile_program_tiered(module, host, program, runtime, existing_main, &HashMap::new())
}

/// Compile with optional per-function interpreter deopt stubs (#778).
/// `deopt_index` maps function name → packed host index.
pub(crate) fn compile_program_tiered(
    module: &mut JITModule,
    host: &HostFns,
    program: &JitProgram,
    runtime: &mut JitRuntime,
    existing_main: Option<FuncId>,
    deopt_index: &HashMap<String, i64>,
) -> Result<FuncId, String> {
    if !deopt_index.is_empty() {
        super::tier_cache::abort_capture();
    }
    runtime.source_file = program.source_file.clone();
    let meta = JitMeta::from_program(program);
    register_packed_enum_show_table(&meta);

    let spawn_lambdas = &program.spawn_lambdas;
    let mut spawn_func_ids: Vec<FuncId> = Vec::new();
    for (i, lam) in spawn_lambdas.iter().enumerate() {
        let name = format!("jet_jit_spawn_body_{i}");
        let sig = spawn_lambda_signature(module, lam);
        let id = module
            .declare_function(&name, Linkage::Export, &sig)
            .map_err(|e| e.to_string())?;
        spawn_func_ids.push(id);
    }

    let cli_entry = program.entry == "__jet_cli_main";
    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    let mut cli_import_id: Option<FuncId> = None;
    if cli_entry {
        cli_import_id = Some(crate::CLI::declare_cli_main_import(module)?);
        let main_id = match existing_main {
            Some(id) => id,
            None => {
                let cc = module.target_config().default_call_conv;
                let sig = Signature::new(cc);
                module
                    .declare_function("jet_jit_main", Linkage::Export, &sig)
                    .map_err(|e| e.to_string())?
            }
        };
        func_ids.insert(program.entry.clone(), main_id);
    }
    for f in &program.funcs {
        if cli_entry && f.name == program.entry {
            continue;
        }
        let sig = func_signature(module, f, &meta)?;
        let id = if !cli_entry && f.name == program.entry {
            match existing_main {
                Some(id) => id,
                None => module
                    .declare_function("jet_jit_main", Linkage::Export, &sig)
                    .map_err(|e| e.to_string())?,
            }
        } else {
            module
                .declare_function(&jit_fn_name(&f.name), Linkage::Export, &sig)
                .map_err(|e| e.to_string())?
        };
        func_ids.insert(f.name.clone(), id);
    }
    let mut generator_body_ids = HashMap::new();
    for f in &program.funcs {
        if !is_generator(f) {
            continue;
        }
        let name = format!("{}__generator", jit_fn_name(&f.name));
        let sig = generator_body_signature(module, f)?;
        let id = module
            .declare_function(&name, Linkage::Local, &sig)
            .map_err(|error| error.to_string())?;
        generator_body_ids.insert(f.name.clone(), id);
    }

    for (i, lam) in spawn_lambdas.iter().enumerate() {
        lower_spawn_function(
            module,
            host,
            &meta,
            lam,
            spawn_func_ids[i],
            &func_ids,
            &spawn_func_ids,
            spawn_lambdas,
            runtime,
        )?;
    }

    let mut spawn_site = 0usize;
    for f in &program.funcs {
        if cli_entry && f.name == program.entry {
            continue;
        }
        let id = func_ids[&f.name];
        if let Some(&idx) = deopt_index.get(&f.name) {
            super::deopt::lower_deopt_stub(module, host, &meta, f, id, idx)
                .map_err(|e| format!("{}: {e}", f.name))?;
        } else if let Some(&body_id) = generator_body_ids.get(&f.name) {
            lower_generator_body(
                module,
                host,
                &meta,
                f,
                body_id,
                &func_ids,
                &spawn_func_ids,
                spawn_lambdas,
                &mut spawn_site,
                runtime,
            )
            .map_err(|error| format!("{}: {error}", f.name))?;
            lower_generator_wrapper(module, host, f, id, body_id, &meta)
                .map_err(|error| format!("{}: {error}", f.name))?;
        } else {
            lower_function(
                module,
                host,
                &meta,
                f,
                id,
                &func_ids,
                &spawn_func_ids,
                spawn_lambdas,
                &mut spawn_site,
                runtime,
            )
            .map_err(|e| format!("{}: {e}", f.name))?;
        }
    }

    if cli_entry {
        // Export `jet_jit_main` as a thin wrapper around the host trampoline.
        // Cranelift cannot `get_finalized_function` an Import for direct invoke.
        let main_id = func_ids[&program.entry];
        let import_id = cli_import_id.expect("cli import");
        let mut ctx = module.make_context();
        let cc = module.target_config().default_call_conv;
        ctx.func.signature = Signature::new(cc);
        let mut fbcx = FunctionBuilderContext::new();
        {
            let mut b = FunctionBuilder::new(&mut ctx.func, &mut fbcx);
            let entry = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            b.seal_block(entry);
            let callee = module.declare_func_in_func(import_id, b.func);
            b.ins().call(callee, &[]);
            b.ins().return_(&[]);
            b.finalize();
        }
        module
            .define_function(main_id, &mut ctx)
            .map_err(|e| e.to_string())?;
        module.clear_context(&mut ctx);
    }

    module.finalize_definitions().map_err(|e| e.to_string())?;
    crate::Data::bind_lazy_callables(module);
    if cli_entry {
        let run_name = "run";
        let run_id = func_ids
            .get(run_name)
            .copied()
            .ok_or_else(|| "jit CLI entry missing `run`".to_string())?;
        let code = module.get_finalized_function(run_id);
        crate::CLI::install_cli_run_ptr(code);
    }
    Ok(func_ids
        .get(&program.entry)
        .copied()
        .ok_or_else(|| format!("jit program missing selected entry `{}`", program.entry))?)
}
