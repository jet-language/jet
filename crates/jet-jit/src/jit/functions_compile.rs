use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::Codegen::TIR::{self, JitProgram, TFunc, TFuncKind, TJitSpawnBody, TJitSpawnLambda};
use jet_foundation::AST::Type;
use std::collections::HashMap;

use super::lower_ctx::LowerCtx;
use super::runtime_host::HostFns;
use super::types_meta::{clif_ty, func_signature, jit_fn_name, JitMeta};
use super::JitRuntime;

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
            func_ids,
            spawn_site: &mut spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct: None,
            ret_clif: clif_ty(&lam.ret),
            shield_depth: 0,
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
                    b.ins().return_(&[val]);
                } else {
                    let _ = val;
                    b.ins().return_(&[]);
                }
            }
            TJitSpawnBody::Block { prefix, tail } => {
                lctx.lower_stmts(prefix)?;
                if let Some(t) = tail {
                    let val = lctx.lower_expr(t)?;
                    if clif_ty(&lam.ret).is_some() {
                        b.ins().return_(&[val]);
                    } else {
                        let _ = val;
                        b.ins().return_(&[]);
                    }
                } else if clif_ty(&lam.ret).is_some() {
                    let zero = b.ins().iconst(types::I64, 0);
                    b.ins().return_(&[zero]);
                } else {
                    b.ins().return_(&[]);
                }
            }
        }
        b.finalize();
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| e.to_string())?;
    module.clear_context(&mut ctx);
    Ok(())
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
    ctx.func.signature = func_signature(module, tir)?;
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
        let method_struct = match &tir.kind {
            TFuncKind::Method { .. } => tir.name.split_once("::").map(|(t, _)| t.to_string()),
            _ => None,
        };
        let mut lctx = LowerCtx {
            b: &mut b,
            module,
            host,
            runtime,
            meta,
            vars: &mut vars,
            var_tys: &mut var_tys,
            func_ids,
            spawn_site,
            spawn_func_ids,
            spawn_lambdas,
            loop_stack: Vec::new(),
            dead: false,
            next_var: 0,
            method_struct,
            ret_clif: tir.ret.as_ref().and_then(clif_ty),
            shield_depth: 0,
        };
        if matches!(tir.kind, TFuncKind::Method { self_conv: Some(_) }) {
            let self_var = lctx.fresh_var(types::I64);
            lctx.b.def_var(self_var, param_vals[0]);
            lctx.vars.insert("self".to_string(), self_var);
            if let Some(struct_name) = &lctx.method_struct {
                lctx.var_tys
                    .insert("self".to_string(), Type::Named(struct_name.clone()));
            }
            param_idx = 1;
        }
        for (i, (name, ty, _)) in tir.params.iter().enumerate() {
            let clif = clif_ty(ty).ok_or("jit param clif type")?;
            let var = lctx.fresh_var(clif);
            lctx.b.def_var(var, param_vals[param_idx + i]);
            lctx.vars.insert(name.clone(), var);
            lctx.var_tys.insert(name.clone(), ty.clone());
        }

        lctx.lower_stmts(&tir.body)?;
        if !lctx.dead {
            if let Some(ret) = &tir.ret {
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
                    let result = lctx.b.inst_results(call)[0];
                    lctx.b.ins().return_(&[result]);
                } else if clif_ty(ret).is_some() {
                    return Err("jit function missing return".to_string());
                } else {
                    lctx.b.ins().return_(&[]);
                }
            } else {
                lctx.b.ins().return_(&[]);
            }
        }
        b.finalize();
    }
    if let Err(e) = cranelift_codegen::verify_function(&ctx.func, module.isa()) {
        return Err(format!("{}: verifier: {e:?}", tir.name));
    }
    module
        .define_function(func_id, &mut ctx)
        .map_err(|e| e.to_string())?;
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
    runtime.source_file = program.source_file.clone();
    let meta = JitMeta::from_program(program);

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

    let mut func_ids: HashMap<String, FuncId> = HashMap::new();
    for f in &program.funcs {
        let sig = func_signature(module, f)?;
        let id = if f.name == "run" {
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
        let id = func_ids[&f.name];
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

    module.finalize_definitions().map_err(|e| e.to_string())?;
    Ok(func_ids
        .get("run")
        .copied()
        .ok_or_else(|| "jit program missing run".to_string())?)
}
