pub fn cranelift_host_supported() -> bool {
    // cranelift-jit 0.112's PLT path panics on non-x86_64 hosts. Keep the
    // default dev path safe by delegating to the tier-0 backend there.
    cfg!(target_arch = "x86_64")
}

fn try_resident(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    if !cranelift_host_supported() {
        return None;
    }
    let program = TIR::lower_jit_program(bundle)?;
    Some(catch_jit_panic("resident run", || {
        resident_run_fresh(&program)
    }))
}

fn try_resident_hot_swap(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    if !cranelift_host_supported() {
        return None;
    }
    let program = TIR::lower_jit_program(bundle)?;
    if !resident_safe_program(&program) {
        return None;
    }
    Some(resident_hot_swap(&program))
}

fn try_resident_restart(bundle: &ProgramBundle) -> Option<Result<RunOutcome, String>> {
    if !cranelift_host_supported() {
        return None;
    }
    let program = TIR::lower_jit_program(bundle)?;
    if !resident_safe_program(&program) {
        return None;
    }
    Some(resident_run_fresh(&program))
}

/// Test hook: MixedSwitch arm condition strings from lowered `main`.
#[doc(hidden)]
pub fn jit_dump_mixed_switch_conds(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    let mut out = Vec::new();
    for f in &program.funcs {
        for s in &f.body {
            if let TStmt::MixedSwitch { arms, .. } = s {
                for (c, _) in arms {
                    out.push(format!("{}: {:?}", f.name, c.ty));
                }
            }
        }
    }
    out
}

/// Test hook: try JIT-compile a checked bundle; surfaces lowering errors.
#[doc(hidden)]
pub fn try_compile_bundle(bundle: &ProgramBundle) -> Result<(), String> {
    if !cranelift_host_supported() {
        return Err("cranelift-jit host path unsupported on this architecture".to_string());
    }
    let program = TIR::lower_jit_program(bundle).ok_or_else(|| {
        format!(
            "lower_jit_program returned None ({})",
            TIR::lower_jit_program_fail_reason(bundle)
        )
    })?;
    catch_jit_panic("compile", || {
        resident_teardown();
        RESIDENT_RUNTIME.with(|slot| *slot.borrow_mut() = Some(fresh_runtime()));
        ensure_resident_module(&program)
    })
}

/// Test hook: lowered function names in the JIT program.
#[doc(hidden)]
pub fn jit_program_func_names(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    program.funcs.iter().map(|f| f.name.clone()).collect()
}

/// Test hook: per-function resident safety detail (`None` = covered).
#[doc(hidden)]
pub fn resident_jit_func_safety_detail(bundle: &ProgramBundle, name: &str) -> Option<String> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let f = program.funcs.iter().find(|f| f.name == name)?;
    resident_safe_func_detail(f, &names)
}

/// Test hook: dump lowered run stmt tags.
#[doc(hidden)]
pub fn jit_dump_main_stmts(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["<no program>".into()];
    };
    let Some(m) = program.funcs.iter().find(|f| f.name == "run") else {
        return vec!["<no run>".into()];
    };
    m.body
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{i}:{}", jit_stmt_tag(s)))
        .collect()
}

/// Test hook: observed TIR statement/expression tags in lowered `run`.
#[doc(hidden)]
pub fn jit_dump_main_ops(bundle: &ProgramBundle) -> Vec<String> {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return vec!["TStmt::<no program>".into()];
    };
    let Some(m) = program.funcs.iter().find(|f| f.name == "run") else {
        return vec!["TStmt::<no run>".into()];
    };
    let mut out = Vec::new();
    collect_stmt_ops(&m.body, &mut out);
    out.sort();
    out
}

fn collect_stmt_ops(stmts: &[TStmt], out: &mut Vec<String>) {
    for stmt in stmts {
        out.push(format!("TStmt::{}", jit_stmt_tag(stmt)));
        match stmt {
            TStmt::Let { init, .. }
            | TStmt::TupleDestructure { init, .. }
            | TStmt::StructDestructure { init, .. }
            | TStmt::ListDestructure { init, .. } => collect_expr_ops(init, out),
            TStmt::Assign { value, .. } | TStmt::Return(Some(value)) | TStmt::ExprStmt(value) => {
                collect_expr_ops(value, out)
            }
            TStmt::If {
                cond,
                then_body,
                else_body,
                ..
            } => {
                collect_if_cond_ops(cond, out);
                collect_stmt_ops(then_body, out);
                if let Some(body) = else_body {
                    collect_stmt_ops(body, out);
                }
            }
            TStmt::Loop { body, .. }
            | TStmt::Region(body)
            | TStmt::Unsafe(body)
            | TStmt::Inline(body)
            | TStmt::DebugOnly(body)
            | TStmt::Live { body }
            | TStmt::Shield { body }
            | TStmt::ScopeMember { body, .. } => collect_stmt_ops(body, out),
            TStmt::While { cond, body, .. } => {
                collect_expr_ops(cond, out);
                collect_stmt_ops(body, out);
            }
            TStmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                collect_stmt_ops(std::slice::from_ref(init.as_ref()), out);
                collect_expr_ops(cond, out);
                collect_stmt_ops(std::slice::from_ref(step.as_ref()), out);
                collect_stmt_ops(body, out);
            }
            TStmt::Range {
                start,
                end,
                step,
                body,
                ..
            } => {
                collect_expr_ops(start, out);
                collect_expr_ops(end, out);
                if let Some(step) = step {
                    collect_expr_ops(step, out);
                }
                collect_stmt_ops(body, out);
            }
            TStmt::IndexAssign {
                base, index, value, ..
            }
            | TStmt::IndexHookAssign {
                base, index, value, ..
            } => {
                collect_expr_ops(base, out);
                collect_expr_ops(index, out);
                collect_expr_ops(value, out);
            }
            TStmt::MathSwizzleAssign { base, value, .. } => {
                collect_expr_ops(base, out);
                collect_expr_ops(value, out);
            }
            TStmt::ForIn { body, .. }
            | TStmt::EnumMatch {
                else_body: Some(body),
                ..
            }
            | TStmt::Layout { body, .. }
            | TStmt::ContextBlock { body, .. }
            | TStmt::Transact { body, .. } => collect_stmt_ops(body, out),
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                for (cond, body) in arms {
                    collect_expr_ops(cond, out);
                    collect_stmt_ops(body, out);
                }
                if let Some(body) = else_body {
                    collect_stmt_ops(body, out);
                }
            }
            TStmt::RangeSwitch {
                arms, else_body, ..
            } => {
                for (_, _, body) in arms {
                    collect_stmt_ops(body, out);
                }
                collect_stmt_ops(else_body, out);
            }
            TStmt::Return(None)
            | TStmt::Break(_)
            | TStmt::Continue(_)
            | TStmt::EnumMatch {
                else_body: None, ..
            }
            | TStmt::Reactive { .. }
            | TStmt::LineMarker(_) => {}
        }
    }
}

fn collect_if_cond_ops(cond: &TIfCond, out: &mut Vec<String>) {
    match cond {
        TIfCond::Plain(e) => collect_expr_ops(e, out),
        TIfCond::IfLet { subj, .. }
        | TIfCond::IsNone { subj, .. }
        | TIfCond::Matches { subj, .. } => collect_expr_ops(subj, out),
    }
}

fn collect_expr_ops(expr: &TExpr, out: &mut Vec<String>) {
    out.push(format!("TExprKind::{}", jit_expr_tag(expr)));
    match &expr.kind {
        TExprKind::Print(inner)
        | TExprKind::Clone(inner)
        | TExprKind::MaterializeView(inner)
        | TExprKind::DistinctRaw(inner)
        | TExprKind::Present(inner)
        | TExprKind::Ok(inner)
        | TExprKind::Err(inner)
        | TExprKind::Deref(inner)
        | TExprKind::RawOf(inner)
        | TExprKind::LayoutLit { inner } => collect_expr_ops(inner, out),
        TExprKind::Unary { operand, .. } => collect_expr_ops(operand, out),
        TExprKind::Binary { lhs, rhs, .. } | TExprKind::LayoutCompare { lhs, rhs, .. } => {
            collect_expr_ops(lhs, out);
            collect_expr_ops(rhs, out);
        }
        TExprKind::CompareChain { operands, .. } => {
            for operand in operands {
                collect_expr_ops(operand, out);
            }
        }
        TExprKind::StrLit(parts) => {
            for part in parts {
                if let TStrPart::Interp(e, _) = part {
                    collect_expr_ops(e, out);
                }
            }
        }
        TExprKind::Call { args, .. }
        | TExprKind::MethodCall { args, .. }
        | TExprKind::FnFieldCall { args, .. }
        | TExprKind::StaticCall { args, .. } => {
            for arg in args {
                collect_expr_ops(&arg.value, out);
            }
        }
        TExprKind::StructLit { fields, .. } => {
            for field in fields {
                collect_expr_ops(&field.1, out);
            }
        }
        TExprKind::TupleLit { fields, .. } => {
            for field in fields {
                collect_expr_ops(&field.1, out);
            }
        }
        TExprKind::EnumLit { payload, .. } => match payload {
            TEnumPayload::Unit => {}
            TEnumPayload::Positional(vals) => {
                for v in vals {
                    collect_expr_ops(&v.value, out);
                }
            }
            TEnumPayload::Named(vals) => {
                for (_, v) in vals {
                    collect_expr_ops(&v.value, out);
                }
            }
        },
        TExprKind::ListLit(elems) => {
            for elem in elems {
                collect_expr_ops(elem, out);
            }
        }
        TExprKind::Index { base, index, .. }
        | TExprKind::IndexHook { base, index, .. }
        | TExprKind::MathLaneIndex { base, index, .. }
        | TExprKind::ColumnarGather { base, index, .. } => {
            collect_expr_ops(base, out);
            collect_expr_ops(index, out);
        }
        TExprKind::Slice {
            base, start, end, ..
        } => {
            collect_expr_ops(base, out);
            collect_expr_ops(start, out);
            collect_expr_ops(end, out);
        }
        TExprKind::BuiltinMethod { recv, args, .. } => {
            collect_expr_ops(recv, out);
            for arg in args {
                collect_expr_ops(arg, out);
            }
        }
        TExprKind::CoreCall { args, .. } => {
            for arg in args {
                collect_expr_ops(arg, out);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            collect_expr_ops(cond, out);
            collect_stmt_ops(then_body, out);
            collect_expr_ops(then_value, out);
            collect_stmt_ops(else_body, out);
            collect_expr_ops(else_value, out);
        }
        _ => {}
    }
}

/// Test hook: count select recv/timer arms on the first `SelectWait` in `run`.
#[doc(hidden)]
pub fn jit_select_arm_counts(bundle: &ProgramBundle) -> Option<(usize, usize)> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let m = program.funcs.iter().find(|f| f.name == "run")?;
    for s in &m.body {
        if let TStmt::Region(body) = s {
            for inner in body {
                if let TStmt::Let { init, .. } = inner {
                    if let TExprKind::SelectWait { builder } = &init.kind {
                        let (r, a) = collect_select_arms_jit(builder);
                        let _ = &names;
                        return Some((r.len(), a.len()));
                    }
                }
            }
        }
    }
    None
}
#[doc(hidden)]
pub fn jit_main_uncovered_detail(bundle: &ProgramBundle) -> Option<String> {
    let program = TIR::lower_jit_program(bundle)?;
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let m = program.funcs.iter().find(|f| f.name == "run")?;
    for (i, s) in m.body.iter().enumerate() {
        if resident_safe_stmt(s, &names) {
            continue;
        }
        if let TStmt::Region(body) = s {
            for (j, inner) in body.iter().enumerate() {
                if !resident_safe_stmt(inner, &names) {
                    let extra = if let TStmt::Let { init, .. } = inner {
                        if let TExprKind::TaskGroupAll { tasks } = &init.kind {
                            format!(
                                ", init=TaskGroupAll tasks={} list_ok={} tasks_ok={}",
                                jit_expr_tag(tasks),
                                jit_list_task_int_type(&tasks.ty),
                                resident_safe_expr(tasks, &names)
                            )
                        } else {
                            format!(", init={}", jit_expr_tag(init))
                        }
                    } else if let TStmt::ExprStmt(e) = inner {
                        format!(", expr={}", jit_expr_tag(e))
                    } else {
                        String::new()
                    };
                    return Some(format!(
                        "main[{i}] region[{j}]={}{extra}",
                        jit_stmt_tag(inner)
                    ));
                }
            }
        }
        return Some(format!("main[{i}]={}", jit_stmt_tag(s)));
    }
    None
}

/// Test hook: label a lowered expr for diagnostics.
#[doc(hidden)]
pub fn jit_expr_tag(expr: &TExpr) -> &'static str {
    match &expr.kind {
        TExprKind::Print(_) => "Print",
        TExprKind::Call { .. } => "Call",
        TExprKind::CoreCall { .. } => "CoreCall",
        TExprKind::CoreClosureCall { .. } => "CoreClosureCall",
        TExprKind::HandleMethod { .. } => "HandleMethod",
        TExprKind::ListLit(_) => "ListLit",
        TExprKind::TaskGroupAll { .. } => "TaskGroupAll",
        TExprKind::TaskGroupRace { .. } => "TaskGroupRace",
        TExprKind::TaskGroupAny { .. } => "TaskGroupAny",
        TExprKind::SelectStart => "SelectStart",
        TExprKind::SelectRecv { .. } => "SelectRecv",
        TExprKind::SelectAfter { .. } => "SelectAfter",
        TExprKind::SelectRead { .. } => "SelectRead",
        TExprKind::SelectWait { .. } => "SelectWait",
        TExprKind::MethodCall { .. } => "MethodCall",
        TExprKind::Local(_) => "Local",
        TExprKind::Binary { .. } => "Binary",
        TExprKind::Index { .. } => "Index",
        _ => "Other",
    }
}

/// Test hook: label a lowered stmt for diagnostics.
#[doc(hidden)]
pub fn jit_stmt_tag(stmt: &TStmt) -> &'static str {
    match stmt {
        TStmt::Let { .. } => "Let",
        TStmt::Assign { .. } => "Assign",
        TStmt::Return(_) => "Return",
        TStmt::ExprStmt(_) => "ExprStmt",
        TStmt::If { .. } => "If",
        TStmt::Loop { .. } => "Loop",
        TStmt::While { .. } => "While",
        TStmt::CountedLoop { .. } => "CountedLoop",
        TStmt::Range { .. } => "Range",
        TStmt::ForIn { .. } => "ForIn",
        TStmt::Break(_) => "Break",
        TStmt::Continue(_) => "Continue",
        _ => "Other",
    }
}

/// Test hook: spawn site vs lambda counts for a bundle.
#[doc(hidden)]
pub fn jit_spawn_stats(bundle: &ProgramBundle) -> (usize, usize) {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return (0, 0);
    };
    (count_spawn_sites(&program), program.spawn_lambdas.len())
}

/// Test hook: whether TIR lowers this bundle for JIT (`lower_jit_program` gate).
#[doc(hidden)]
pub fn tir_lowers_bundle(bundle: &ProgramBundle) -> bool {
    TIR::lower_jit_program(bundle).is_some()
}

/// Test hook: why `lower_jit_program` returned `None`.
#[doc(hidden)]
pub fn tir_lower_fail_reason(bundle: &ProgramBundle) -> String {
    TIR::lower_jit_program_fail_reason(bundle)
}

/// Test hook: whether the bundle's entry module is inside `resident_jit_safe`.
#[doc(hidden)]
pub fn resident_jit_safe_bundle(bundle: &ProgramBundle) -> bool {
    resident_jit_safe_bundle_detail(bundle).is_empty()
}

/// Test hook: empty string when covered; otherwise a short failure reason.
#[doc(hidden)]
pub fn resident_jit_safe_bundle_detail(bundle: &ProgramBundle) -> String {
    let Some(program) = TIR::lower_jit_program(bundle) else {
        return format!(
            "lower_jit_program returned None ({})",
            TIR::lower_jit_program_fail_reason(bundle)
        );
    };
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = program.funcs.iter().any(|f| {
        f.name == "run" && f.params.is_empty() && f.ret.is_none() && resident_safe_func(f, &names)
    });
    if !main_ok {
        for f in &program.funcs {
            if f.name == "run" {
                if let Some(d) = resident_safe_func_detail(f, &names) {
                    return format!("run not resident-safe: {d}");
                }
            }
        }
        return "run not resident-safe".to_string();
    }
    for f in &program.funcs {
        if !resident_safe_func(f, &names) {
            return format!("func `{}` not resident-safe", f.name);
        }
    }
    let spawn_sites = count_spawn_sites(&program);
    if spawn_sites != program.spawn_lambdas.len() {
        return format!(
            "spawn site count {spawn_sites} != lambda count {}",
            program.spawn_lambdas.len()
        );
    }
    for (i, lam) in program.spawn_lambdas.iter().enumerate() {
        if !resident_safe_spawn_lambda(lam, &names) {
            return format!("spawn lambda {i} not resident-safe");
        }
    }
    String::new()
}

/// Test hook: how many times resident `main` ran without a clean restart.
#[doc(hidden)]
pub fn resident_invocations_for_test() -> u64 {
    RESIDENT_RUNTIME.with(|slot| slot.borrow().as_ref().map(|r| r.invocations).unwrap_or(0))
}
