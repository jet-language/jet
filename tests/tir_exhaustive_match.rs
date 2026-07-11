//! Card #455 (durability W4): the TIR quadruple (subset gate / AOT emit / JIT
//! lower / tier-0 interpreter fallback) is memory-coupled — a missed match arm
//! over a `TIR` enum variant is a runtime ICE, not a compile error, unless the
//! match is genuinely exhaustive (no bare `_ =>` catch-all). Rust's own
//! exhaustiveness check already gives us the machine enforcement (R12): once a
//! match has no wildcard, adding a new enum variant is a compile error in
//! every function that matches it. This test pins that the two real
//! TIR-variant dispatch points (`TIR/emit/*.rs`'s AOT emitter and `jet-jit`'s
//! JIT lowerer) stay wildcard-free at the exact indent level their own
//! `TStmt`/`TExprKind`/`TBuiltinOp`/`THandleOp` match arms live at — a nested
//! match over an ancillary type (`Type`, `BinOp`, a string key) one level
//! deeper is a legitimate combinatorial fallback, not a hidden `TIR` variant,
//! so this check is indent-anchored rather than a blanket `_ =>` ban.
//!
//! Finding recorded here (not a syntax gate, informational): of the four
//! consumers named on card #455, only two actually dispatch on a `TIR` enum.
//! `TIR/subset.rs` (`jet-codegen`) gates JIT-eligibility by matching the
//! pre-TIR AST (`Expr`/`Stmt`), not `TIR`. The tier-0 interpreter
//! (`Source/JitBackend.rs::InterpreterBackend` → `Source/Interpreter.rs::
//! run_checked`) re-runs the parsed AST directly; `jet-comptime` has no
//! `jet-codegen` dependency at all. Both are real R12 fallback participants
//! but neither is a `TIR`-enum consumer, so they are out of scope for an
//! exhaustive-`TIR`-match pin.
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
    // Brace-balance, skipping the contents of string literals (so a format
    // string like `"{}"` doesn't desync the depth count) and single-char
    // literals (`'x'`, `'\n'`) — but NOT lifetimes (`'f`, `'static`), which
    // are just a quote followed by an identifier with no closing quote.
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
            // Simple char literal: `'x'` (quote, one char, quote).
            if i + 2 < bytes.len() && bytes[i + 2] as char == '\'' {
                i += 3;
                continue;
            }
            // Escaped char literal: `'\n'`, `'\''`, `'\\'`, ...
            if i + 1 < bytes.len() && bytes[i + 1] as char == '\\' {
                if let Some(close) = source[body_start + i + 2..]
                    .find('\'')
                    .filter(|&off| off <= 2)
                {
                    i += 3 + close;
                    continue;
                }
            }
            // Otherwise a lifetime (`'f`, `'a`, `'static`) — not a literal,
            // just consume the quote and keep scanning normally.
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

/// Assert `body` has no bare `_ =>` arm at exactly `indent` spaces — the
/// indent level of that function's own top-level `match` over a `TIR` enum.
/// A wildcard at a deeper indent belongs to a nested match over an ancillary
/// type and is out of scope (see module doc).
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
    // impl-block method: fn body at 8 spaces, its `match stmt { ... }` arms at 12.
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
    // The nested `match cond { TIfCond::... }` inside the `TStmt::If` arm sits
    // one level deeper than the outer `TStmt` match (16 spaces).
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
    // free fn: fn body at 4 spaces, its top-level `match s { ... }` arms at 8.
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
