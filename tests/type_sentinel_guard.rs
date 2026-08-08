//! Card #1662: `\0`-prefixed fake-name sentinels are a phantom-type anti-pattern
//! (D-FACT-HOME1 extension) — a compiler fact smuggled through a `String` field
//! that pretends to hold a real name, spellable/comparable/serializable, when
//! it should be a typed variant instead.
//!
//! Retired by card #1662: `APPROX_NUMERIC_WIDEN_MARKER` (real
//! `Call::widen_approx` field), `\0Quantity` (real `Type::Quantity` variant),
//! `\0compute.dimension.N` (real `Type::ComputeDim(u64)` variant),
//! `\0numeric.checked_widen` (real `Expr::MethodCall::checked_widen` field),
//! and the eight `Type::Tagged` provenance/access markers `\0core.crypto`,
//! `\0clock.deterministic`, `\0clock.system`, `\0expiring_secret.loan`,
//! `\0shared_guard.read`, `\0shared_guard.edit`, `\0terminal.fact_set`, and
//! `\0cpp.callback_abi` (real `TagMarker::Internal(InternalTag)` field,
//! replacing the `TagMarker::User(String)` piggy-back).
//!
//! The scan covers every `.rs` file under `crates/jet-foundation/src` — a
//! sentinel constant declared in any Syntax fragment or AST module is caught
//! (review bd11-rev: scanning only two files let `\0jet.defer.close` hide in
//! `Syntax/core_surface.rs`). The allowlist may only shrink: never add a row
//! for a NEW marker (I8, I9 — every fake-name sentinel is exactly the
//! mechanism card #1662 exists to delete).
//!
//! Diagnostic/fixture files are not scanned — only the compiler's own
//! foundation sources, where a sentinel constant is declared.

use std::fs;
use std::path::PathBuf;

/// Every `\0`-prefixed compiler-private marker still allowed to exist. This
/// list only shrinks. The one survivor predates the card's enumerated scope
/// and has its own retirement card — see each row's comment.
const ALLOWED_SENTINELS: &[&str] = &[
    // `defer close(^r)` still lowers to a fake `Expr::Call` named
    // `\0jet.defer.close` (5 consumer sites across parser/formatter/sema/
    // codegen). Homing it as a real statement variant fans out into every
    // exhaustive `Stmt` match — card #1778 tracks the retirement.
    "\\0jet.defer.close",
];

/// The AST and Syntax trees plus the module roots — every home where a
/// type/marker sentinel constant has ever been declared. Binary-format modules
/// (`CLISchema.rs` holds WASM/ELF magic strings) are deliberately outside the
/// scan: a `\0asm` file magic is not a fake-name type sentinel.
fn foundation_sources() -> Vec<PathBuf> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/jet-foundation/src");
    let mut out = vec![src.join("lib.rs"), src.join("Syntax.rs")];
    let mut stack = vec![src.join("AST"), src.join("Syntax")];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.retain(|p| p.exists());
    out.sort();
    out
}

fn scan(path: &PathBuf) -> Vec<String> {
    let text =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut found = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find("\"\\0") {
            let tail = &rest[at + 1..];
            let Some(end) = tail.find('"') else { break };
            let literal = &tail[..end];
            found.push(format!("{}:{}: \\0{}", path.display(), line_no + 1, &literal[2..]));
            rest = &tail[end + 1..];
        }
    }
    found
}

#[test]
fn nul_prefixed_type_sentinels_stay_on_the_allowlist() {
    let mut offenders = Vec::new();
    let mut survivors_seen = 0usize;
    for path in foundation_sources() {
        for hit in scan(&path) {
            let marker = hit.splitn(3, ": \\0").nth(1).map(|m| format!("\\0{m}"));
            let allowed = marker
                .as_deref()
                .is_some_and(|m| ALLOWED_SENTINELS.iter().any(|a| m.starts_with(a)));
            if allowed {
                survivors_seen += 1;
            } else {
                offenders.push(hit);
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "new \\0-prefixed compiler sentinel(s) outside the card #1662 allowlist \
         (add a real typed field/variant instead, per D-FACT-HOME1):\n{}",
        offenders.join("\n")
    );
    assert!(
        survivors_seen <= ALLOWED_SENTINELS.len(),
        "allowlisted sentinel declared in more than one place — the ratchet only shrinks"
    );
}

#[test]
fn approx_and_quantity_sentinels_are_retired() {
    for path in foundation_sources() {
        // Code lines only: a doc comment naming a retired constant for
        // provenance ("Was `X`: …") is history, not a reintroduction.
        let text = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let shown = path.display();
        assert!(
            !text.contains("APPROX_NUMERIC_WIDEN_MARKER"),
            "{shown}: APPROX_NUMERIC_WIDEN_MARKER must stay retired (card #1662) — \
             use Call::widen_approx"
        );
        assert!(
            !text.contains("\\0Quantity"),
            "{shown}: the \\0Quantity string encoding must stay retired (card #1662) — \
             use Type::Quantity"
        );
        assert!(
            !text.contains("CHECKED_NUMERIC_WIDEN_MARKER"),
            "{shown}: CHECKED_NUMERIC_WIDEN_MARKER must stay retired (card #1662) — \
             use Expr::MethodCall::checked_widen"
        );
        assert!(
            !text.contains("COMPUTE_DIMENSION_PREFIX"),
            "{shown}: COMPUTE_DIMENSION_PREFIX must stay retired (card #1662) — \
             use Type::ComputeDim(u64)"
        );
        for retired_marker_const in [
            "CORE_CRYPTO_NOMINAL_MARKER",
            "DETERMINISTIC_CLOCK_MARKER",
            "SYSTEM_CLOCK_MARKER",
            "EXPIRING_SECRET_LOAN_MARKER",
            "SHARED_GUARD_READ_MARKER",
            "SHARED_GUARD_EDIT_MARKER",
            "TERMINAL_FACT_SET_MARKER",
            "CPP_CALLBACK_ABI_MARKER",
        ] {
            assert!(
                !text.contains(retired_marker_const),
                "{shown}: {retired_marker_const} must stay retired (card #1662) — \
                 use Type::Tagged with TagMarker::Internal(InternalTag)"
            );
        }
    }
}
