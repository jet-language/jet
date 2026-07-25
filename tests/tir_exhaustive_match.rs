//! Card #455 / #777: TIR quadruple dispatch must stay wildcard-free.
//! Pins AOT emit, JIT lower, and the canonical TIR evaluator (#777).
//!
//! Run: `cargo test --test tir_exhaustive_match`

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
    let body = fn_body(&source, "exec_stmt");
    assert_no_wildcard_at_indent(body, 12, "eval exec_stmt (TStmt)");
}

#[test]
fn eval_expr_is_exhaustive_over_texprkind() {
    let root = root();
    let source =
        fs::read_to_string(root.join("crates/jet-codegen/src/Codegen/TIR/eval/exprs.rs")).unwrap();
    let body = fn_body(&source, "eval_expr");
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
