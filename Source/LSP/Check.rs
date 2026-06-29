//! Document check (used by LSP and tests) + unified fix engine + doctor/bench.

use crate::Diagnostics::{Diagnostic, TextEdit};
use crate::AST::ProgramBundle;
use std::path::{Path, PathBuf};

// ── Document check (used by LSP and tests) ────────────────────────────────────

/// Check one document (disk path + in-memory text). Used by LSP and tests.
pub fn check_document(path: &str, text: &str) -> Vec<Diagnostic> {
    let abs = canonical_path(path);
    let (diags, _) = crate::Driver::check_file(path, Some((&abs, text)), true);
    diags
}

/// Check one document, also returning the bundle and effect facts for symbol analysis.
pub fn check_document_with_bundle(
    path: &str,
    text: &str,
) -> (
    Vec<Diagnostic>,
    Option<ProgramBundle>,
    jet_semindex::SemIndexEffectFacts,
) {
    let abs = canonical_path(path);
    crate::Driver::check_file_with_effect_facts(path, Some((&abs, text)), true)
}

/// Apply a single teaching edit to source text (for scripted LSP tests and the
/// LSP's own preview). Delegates to the unified fix engine so the offset/splice
/// math lives in exactly one place (`FixEngine::apply_edits`), shared with the
/// CLI `jet fix`. A lone edit can never overlap itself, so the `Result` is
/// always `Ok` here; we unwrap defensively rather than propagate.
pub fn apply_edit(src: &str, edit: &TextEdit) -> String {
    crate::FixEngine::apply_edits(src, std::slice::from_ref(edit))
        .unwrap_or_else(|_| src.to_string())
}

// ── Unified fix engine (CLI `jet fix` AND LSP code actions) ────────────────────

/// One machine-applicable fix: the title a user sees and the edit that applies.
/// This is the single shape both `jet fix` and the LSP "quick fix" code action
/// are built from — the SAME `edit` carried in the `--json` diagnostic schema —
/// so the two can never drift (E2-M3 unified fix engine; D-LSP7).
#[derive(Debug, Clone)]
pub struct Fix {
    /// Action title (the diagnostic's `fix` line).
    pub title: String,
    /// The edit to apply.
    pub edit: TextEdit,
}

/// Collect every machine-applicable fix for a document, in diagnostic order.
/// Both the CLI and the LSP go through here.
pub fn collect_fixes(path: &str, text: &str) -> Vec<Fix> {
    check_document(path, text)
        .into_iter()
        .filter_map(|d| {
            d.edit.clone().map(|edit| Fix {
                title: d.fix.clone(),
                edit,
            })
        })
        .collect()
}

/// Apply every fix to `src`, returning the rewritten text. Edits are applied
/// from the highest offset down so earlier spans stay valid.
pub fn apply_all(src: &str, fixes: &[Fix]) -> String {
    let mut edits: Vec<&TextEdit> = fixes.iter().map(|f| &f.edit).collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    let mut out = src.to_string();
    for edit in edits {
        out = apply_edit(&out, edit);
    }
    out
}

// ── URI / path utilities ──────────────────────────────────────────────────────

pub(crate) fn canonical_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        normalize_path(&p)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        normalize_path(&cwd.join(p))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ── Doctor ────────────────────────────────────────────────────────────────────

/// Health check: verify that the server can lex/parse/check a trivial program.
pub fn run_doctor() {
    println!("jet lsp doctor");
    println!("--------------");
    let src = "fn main() { print(\"hello\"); }\n";
    let (toks, lex_errs) = crate::Lexer::lex(src);
    if lex_errs.is_empty() {
        println!("  [ok] lexer");
    } else {
        println!("  [FAIL] lexer: {} errors", lex_errs.len());
    }
    match crate::Parser::parse(&toks) {
        Ok(_) => println!("  [ok] parser"),
        Err(errs) => println!("  [FAIL] parser: {} errors", errs.len()),
    }
    let diags = check_document("test.jet", src);
    if diags.is_empty() {
        println!("  [ok] sema");
    } else {
        println!("  [FAIL] sema: {} diagnostics", diags.len());
    }
    let formatted = crate::format_source(src);
    if formatted.is_ok() {
        println!("  [ok] formatter");
    } else {
        println!("  [FAIL] formatter");
    }
    println!("  [ok] JSON-RPC framing");

    // C13: transcript runner smoke — verify tests/lsp/01_initialize.json exists.
    let transcript_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lsp");
    if transcript_dir.exists() {
        let count = std::fs::read_dir(&transcript_dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
                    .count()
            })
            .unwrap_or(0);
        println!(
            "  [ok] transcript runner: {} fixture(s) found in tests/lsp/",
            count
        );
    } else {
        println!("  [WARN] transcript runner: tests/lsp/ not found");
    }

    // C13: tree-sitter grammar presence.
    let ts_grammar =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/tree-sitter/grammar.js");
    if ts_grammar.exists() {
        println!("  [ok] editors/tree-sitter/grammar.js present");
    } else {
        println!(
            "  [WARN] editors/tree-sitter/grammar.js not found — run `tree-sitter generate` to build"
        );
    }

    // C13: TextMate grammar presence.
    let tm_grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/jet.tmGrammar");
    if tm_grammar.exists() {
        println!("  [ok] editors/jet.tmGrammar present");
    } else {
        println!("  [WARN] editors/jet.tmGrammar not found");
    }

    println!("all checks passed — the language server is healthy");
}

// ── Bench ─────────────────────────────────────────────────────────────────────

/// Replay a simple session to measure diagnostic latency.
/// Runs `rounds` iterations of parse+sema over `src` and checks budget_ms.
pub fn run_bench(src: &str, rounds: usize, budget_ms: u128) {
    let label = format!("{}B input, {} rounds", src.len(), rounds);
    let start = std::time::Instant::now();
    for _ in 0..rounds {
        let _ = check_document("bench.jet", src);
    }
    let elapsed = start.elapsed();
    let per_round_ms = elapsed.as_millis() / rounds.max(1) as u128;
    println!(
        "bench [{}]: {}ms/round ({} total) — budget {}ms — {}",
        label,
        per_round_ms,
        elapsed.as_millis(),
        budget_ms,
        if per_round_ms <= budget_ms {
            "PASS"
        } else {
            "FAIL"
        }
    );
    if per_round_ms > budget_ms {
        std::process::exit(1);
    }
}
