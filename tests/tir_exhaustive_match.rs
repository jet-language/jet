//! Card #455 / #777: TIR quadruple dispatch must stay wildcard-free.
//! Pins AOT emit, JIT lower, and the canonical TIR evaluator (#777).
//!
//! Run: `cargo test --test tir_exhaustive_match`

mod common;

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Extract the brace-balanced body of the first `fn <name>(` in `source`,
/// starting from the line containing the signature.
fn fn_body<'a>(source: &'a str, name: &str) -> &'a str {
    let needle = format!("fn {name}(");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("fn `{name}` not found"));
    let body_start = source[start..]
        .find('{')
        .map(|i| start + i)
        .unwrap_or_else(|| panic!("fn `{name}` has no body"));
    let mut depth = 0i32;
    let bytes = source[body_start..].as_bytes();
    let mut i = 0usize;
    let mut in_string = false;
    while i < bytes.len() {
        let ch = bytes[i] as char;
        if in_string {
            if ch == '\\' {
                i += 2;
                continue;
            }
            if ch == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if ch == '"' {
            in_string = true;
            i += 1;
            continue;
        }
        if ch == '\'' {
            if i + 2 < bytes.len() && bytes[i + 2] as char == '\'' {
                i += 3;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] as char == '\\' {
                if let Some(close) = source[body_start + i + 2..]
                    .find('\'')
                    .filter(|&off| off <= 2)
                {
                    i += 3 + close;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[body_start..body_start + i + 1];
                }
            }
            _ => {}
        }
        i += 1;
    }
    panic!("fn `{name}` body never closes");
}

fn assert_no_wildcard_at_indent(body: &str, indent: usize, ctx: &str) {
    let marker = format!("{}_ =>", " ".repeat(indent));
    let hits: Vec<&str> = body
        .lines()
        .filter(|l| l.starts_with(&marker))
        .collect();
    assert!(
        hits.is_empty(),
        "{ctx}: found bare `_ =>` at indent {indent} (hides new TIR variants): {hits:?}"
    );
}

fn assert_contains_all(body: &str, ctx: &str, required: &[&str]) {
    for needle in required {
        assert!(
            body.contains(needle),
            "{ctx}: canonical execution edge `{needle}` is missing"
        );
    }
}

fn assert_no_execution_fallback(body: &str, ctx: &str) {
    let code = body
        .lines()
        .map(|line| line.split_once("//").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "Command::new(",
        "std::process::Command",
        "compile_generated_src",
        "rustc",
        "jetpack",
        "run_aot",
        "run_release",
        "emit_tir_",
        "AotBackend",
        "eval_ast",
        "exec_ast",
        "legacy_eval",
        "tree_walk",
        "tree-walk",
    ] {
        assert!(
            !code.contains(forbidden),
            "{ctx}: reachable execution body contains forbidden fallback `{forbidden}`"
        );
    }
}

#[test]
fn jit_lower_stmt_is_exhaustive_over_tstmt() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-jit/src/jit/lower_ctx.rs")).unwrap();
    let body = fn_body(&source, "lower_stmt");
    assert_no_wildcard_at_indent(body, 12, "lower_stmt (TStmt)");
}

#[test]
fn jit_lower_expr_is_exhaustive_over_texprkind() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-jit/src/jit/lower_ctx.rs")).unwrap();
    let body = fn_body(&source, "lower_expr");
    assert_no_wildcard_at_indent(body, 12, "lower_expr (TExprKind)");
}

#[test]
fn jit_lower_builtin_method_is_exhaustive_over_tbuiltinop() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-jit/src/jit/lower_ctx.rs")).unwrap();
    let body = fn_body(&source, "lower_builtin_method");
    assert_no_wildcard_at_indent(body, 12, "lower_builtin_method (TBuiltinOp)");
}

#[test]
fn jit_lower_handle_method_is_exhaustive_over_thandleop() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-jit/src/jit/lower_ctx.rs")).unwrap();
    let body = fn_body(&source, "lower_handle_method");
    assert_no_wildcard_at_indent(body, 12, "lower_handle_method (THandleOp)");
}

#[test]
fn jit_lower_stmt_if_cond_is_exhaustive_over_tifcond() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-jit/src/jit/lower_ctx.rs")).unwrap();
    let body = fn_body(&source, "lower_stmt");
    assert_no_wildcard_at_indent(body, 16, "lower_stmt's TIfCond match");
}

#[test]
fn aot_emit_tir_stmt_is_exhaustive_over_tstmt() {
    let root = root();
    let source = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/emit/statements.rs"),
    )
    .unwrap();
    let body = fn_body(&source, "emit_tir_stmt");
    assert_no_wildcard_at_indent(body, 8, "emit_tir_stmt (TStmt)");
}

#[test]
fn aot_emit_tir_expr_is_exhaustive_over_texprkind() {
    let root = root();
    let source = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/emit/expressions.rs"),
    )
    .unwrap();
    let body = fn_body(&source, "emit_tir_expr");
    assert_no_wildcard_at_indent(body, 8, "emit_tir_expr (TExprKind)");
}

#[test]
fn eval_exec_stmt_is_exhaustive_over_tstmt() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/TIR/eval/stmts.rs")).unwrap();
    let body = fn_body(&source, "exec_stmt_inner");
    assert_no_wildcard_at_indent(body, 12, "eval exec_stmt (TStmt)");
}

#[test]
fn eval_expr_is_exhaustive_over_texprkind() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs")).unwrap();
    let body = fn_body(&source, "eval_expr_inner");
    assert_no_wildcard_at_indent(body, 12, "eval_expr (TExprKind)");
}

#[test]
fn eval_builtin_is_exhaustive_over_tbuiltinop() {
    let root = root();
    let source = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/eval/builtins.rs"),
    )
    .unwrap();
    let body = fn_body(&source, "eval_builtin");
    assert_no_wildcard_at_indent(body, 8, "eval_builtin (TBuiltinOp)");
}

#[test]
fn eval_handle_is_exhaustive_over_thandleop() {
    let root = root();
    let source = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/eval/handles.rs"),
    )
    .unwrap();
    let body = fn_body(&source, "eval_handle");
    assert_no_wildcard_at_indent(body, 8, "eval_handle (THandleOp)");
}

#[test]
fn comptime_repl_and_deopt_use_canonical_tir_without_external_fallback() {
    let root = root();
    let repl = fs::read_to_string(root.join("crates/jet-repl/src/lib.rs")).unwrap();
    let execute_line = fn_body(&repl, "execute_line");
    assert_contains_all(
        execute_line,
        "REPL statement execution",
        &[
            "crate::Comptime::run_repl_step_interruptible(",
            "crate::Comptime::run_repl_step(",
        ],
    );

    let comptime = fs::read_to_string(root.join("crates/jet-comptime/src/Comptime/mod.rs")).unwrap();
    let repl_step = fn_body(&comptime, "run_repl_step");
    let repl_step_interruptible = fn_body(&comptime, "run_repl_step_interruptible");
    let repl_inner = fn_body(&comptime, "run_repl_step_inner");
    assert_contains_all(repl_step, "REPL step", &["run_repl_step_inner("]);
    assert_contains_all(
        repl_step_interruptible,
        "interruptible REPL step",
        &["run_repl_step_inner("],
    );
    assert_contains_all(
        repl_inner,
        "REPL semantic execution",
        &[
            "interp.exec_block(head, scope)",
            "interp.eval(&b.init, scope)",
            "interp.eval(e, scope)",
            "interp.exec_stmt(other, scope)",
        ],
    );
    assert_eq!(repl_inner.matches("interp.exec_block(").count(), 1);
    assert_eq!(repl_inner.matches("interp.eval(").count(), 3);
    assert_eq!(repl_inner.matches("interp.exec_stmt(").count(), 1);

    let interpreter =
        fs::read_to_string(root.join("crates/jet-comptime/src/Comptime/Interpreter.rs")).unwrap();
    let interp_block = fn_body(&interpreter, "exec_block");
    let interp_stmt = fn_body(&interpreter, "exec_stmt");
    let interp_expr = fn_body(&interpreter, "eval");
    assert_contains_all(
        interp_block,
        "comptime block adapter",
        &["super::TirBridge::eval_block(&mut req)"],
    );
    assert_contains_all(
        interp_stmt,
        "comptime statement adapter",
        &["self.exec_block(std::slice::from_ref(stmt), scope)"],
    );
    assert_contains_all(
        interp_expr,
        "comptime expression adapter",
        &["super::TirBridge::eval_expr(&mut req)"],
    );

    let bridge =
        fs::read_to_string(root.join("crates/jet-comptime/src/Comptime/TirBridge.rs")).unwrap();
    let bridge_hooks = fn_body(&bridge, "hooks");
    let bridge_bundle = fn_body(&bridge, "run_bundle");
    let bridge_expr = fn_body(&bridge, "eval_expr");
    let bridge_block = fn_body(&bridge, "eval_block");
    assert_contains_all(bridge_hooks, "TIR bridge hook source", &["HOOKS", ".get()"]);
    assert_contains_all(
        bridge_bundle,
        "TIR bundle bridge",
        &["(hooks().run_bundle)(bundle, sink, allow_impure)"],
    );
    assert_contains_all(
        bridge_expr,
        "TIR expression bridge",
        &["(hooks().eval_expr)(req)"],
    );
    assert_contains_all(
        bridge_block,
        "TIR block bridge",
        &["(hooks().eval_block)(req)"],
    );

    let evaluator =
        fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/TIR/eval/mod.rs")).unwrap();
    let install = fn_body(&evaluator, "install_comptime_bridge");
    assert_contains_all(
        install,
        "canonical TIR bridge installation",
        &[
            "run_bundle,",
            "eval_expr: eval_expr_hook",
            "eval_block: eval_block_hook",
        ],
    );
    let run_bundle = fn_body(&evaluator, "run_bundle");
    let eval_expr_hook = fn_body(&evaluator, "eval_expr_hook");
    let eval_block_hook = fn_body(&evaluator, "eval_block_hook");
    let lower_expr = fn_body(&evaluator, "lower_expr_for_eval");
    let lower_stmts = fn_body(&evaluator, "lower_stmts_for_eval");
    let lower_program = fn_body(&evaluator, "lower_interp_program");
    let run_program = fn_body(&evaluator, "run_program_with_structs");
    let run_named = fn_body(&evaluator, "run_named_func");
    let run_func = fn_body(&evaluator, "run_func");
    assert_contains_all(
        run_bundle,
        "installed bundle hook",
        &["lower_interp_program(bundle)", "run_program_with_structs("],
    );
    assert_contains_all(
        eval_expr_hook,
        "installed expression hook",
        &["lower_expr_for_eval(", "ctx.eval_expr(&tir, &mut scope)"],
    );
    assert_contains_all(
        eval_block_hook,
        "installed block hook",
        &["lower_stmts_for_eval(", "ctx.exec_stmts(&tir, &mut scope)"],
    );
    assert_contains_all(
        lower_expr,
        "expression TIR lowering",
        &["TIR::lower_expr(expr, &cx, &mut env)"],
    );
    assert_contains_all(
        lower_stmts,
        "statement TIR lowering",
        &["TIR::lower_stmts(stmts, &cx, &mut env)"],
    );
    assert_contains_all(
        lower_program,
        "program TIR lowering",
        &["TIR::lower_jit_program(bundle)"],
    );
    assert_contains_all(
        run_program,
        "canonical program evaluator",
        &["ctx.run_func(entry, Vec::new(), &mut scope)"],
    );
    assert_contains_all(
        run_named,
        "canonical named-function evaluator",
        &["ctx.run_func(func, args, &mut scope)"],
    );
    assert_contains_all(
        run_func,
        "canonical function evaluator",
        &["self.exec_stmts(&func.body, scope)"],
    );

    let eval_stmts = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/eval/stmts.rs"),
    )
    .unwrap();
    let exec_stmts = fn_body(&eval_stmts, "exec_stmts");
    let exec_stmt = fn_body(&eval_stmts, "exec_stmt_inner");
    assert_contains_all(
        exec_stmts,
        "canonical TIR statement list",
        &["self.exec_stmt(stmt, scope)"],
    );
    assert_contains_all(
        exec_stmt,
        "canonical TIR statement",
        &["self.eval_expr("],
    );
    let eval_exprs = fs::read_to_string(
        root.join("crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs"),
    )
    .unwrap();
    let eval_expr = fn_body(&eval_exprs, "eval_expr_inner");
    assert_contains_all(eval_expr, "canonical TIR expression", &["match &expr.kind"]);

    let deopt = fs::read_to_string(root.join("crates/jet-jit/src/jit/deopt.rs")).unwrap();
    let whole_program = fn_body(&deopt, "run_whole_interp");
    let deopt_call = fn_body(&deopt, "jet_deopt_call");
    let bits_to_ct = fn_body(&deopt, "bits_to_ct");
    let ct_to_bits = fn_body(&deopt, "ct_to_bits");
    assert_contains_all(
        whole_program,
        "whole-program deopt",
        &[
            "TIR::install_comptime_bridge()",
            "Comptime::TirBridge::run_bundle(bundle, &mut sink, true)",
        ],
    );
    assert_contains_all(
        deopt_call,
        "function deopt",
        &[
            "TIR::install_comptime_bridge()",
            "TIR::run_named_func(program, &func_name, args, &mut sink)",
            "bits_to_ct(",
            "ct_to_bits(",
        ],
    );

    for (ctx, body) in [
        ("REPL execute_line", execute_line),
        ("REPL step", repl_step),
        ("REPL interruptible step", repl_step_interruptible),
        ("REPL semantic execution", repl_inner),
        ("comptime block adapter", interp_block),
        ("comptime statement adapter", interp_stmt),
        ("comptime expression adapter", interp_expr),
        ("TIR bridge hooks", bridge_hooks),
        ("TIR bundle bridge", bridge_bundle),
        ("TIR expression bridge", bridge_expr),
        ("TIR block bridge", bridge_block),
        ("TIR hook installation", install),
        ("installed bundle hook", run_bundle),
        ("installed expression hook", eval_expr_hook),
        ("installed block hook", eval_block_hook),
        ("expression lowering", lower_expr),
        ("statement lowering", lower_stmts),
        ("program lowering", lower_program),
        ("canonical program evaluator", run_program),
        ("canonical named-function evaluator", run_named),
        ("canonical function evaluator", run_func),
        ("canonical statement list", exec_stmts),
        ("canonical statement evaluator", exec_stmt),
        ("canonical expression evaluator", eval_expr),
        ("whole-program deopt", whole_program),
        ("function deopt", deopt_call),
        ("deopt argument decode", bits_to_ct),
        ("deopt result encode", ct_to_bits),
    ] {
        assert_no_execution_fallback(body, ctx);
    }
}
