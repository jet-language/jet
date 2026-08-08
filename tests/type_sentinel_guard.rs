//! Card #1662: `\0`-prefixed fake-name sentinels are a phantom-type anti-pattern
//! (D-FACT-HOME1 extension) — a compiler fact smuggled through a `String` field
//! that pretends to hold a real name, spellable/comparable/serializable, when
//! it should be a typed variant instead. `APPROX_NUMERIC_WIDEN_MARKER` and
//! `\0Quantity` are retired (real `Call::widen_approx` field and
//! `Type::Quantity` variant). This is a ratchet over the remaining ones: it
//! never grows, and every entry removed here is a small win.
//!
//! Diagnostic/fixture files are not scanned — only the compiler's own AST/type
//! definitions, where a sentinel constant is declared.

use std::fs;
use std::path::PathBuf;

/// Every `\0`-prefixed compiler-private marker still allowed to exist. Remove
/// a row when its marker becomes a real typed field or enum variant; never add
/// a new row for a NEW marker without owner sign-off (I8, I9 — every fake-name
/// sentinel is exactly the mechanism card #1662 exists to delete).
const ALLOWED_SENTINELS: &[&str] = &[
    "\\0core.crypto",
    "\\0clock.deterministic",
    "\\0clock.system",
    "\\0expiring_secret.loan",
    "\\0shared_guard.read",
    "\\0shared_guard.edit",
    "\\0terminal.fact_set",
    "\\0cpp.callback_abi",
    "\\0compute.dimension.",
    "\\0numeric.checked_widen",
];

fn scan(path: &str) -> Vec<String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let text = fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut found = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(at) = rest.find("\"\\0") {
            let tail = &rest[at + 1..];
            let Some(end) = tail.find('"') else { break };
            let literal = &tail[..end];
            found.push(format!("{path}:{}: \\0{}", line_no + 1, &literal[2..]));
            rest = &tail[end + 1..];
        }
    }
    found
}

#[test]
fn nul_prefixed_type_sentinels_stay_on_the_allowlist() {
    let mut offenders = Vec::new();
    for path in [
        "crates/jet-foundation/src/AST/types.rs",
        "crates/jet-foundation/src/Syntax.rs",
    ] {
        for hit in scan(path) {
            let marker = hit.splitn(3, ": \\0").nth(1).map(|m| format!("\\0{m}"));
            let allowed = marker
                .as_deref()
                .is_some_and(|m| ALLOWED_SENTINELS.iter().any(|a| m.starts_with(a)));
            if !allowed {
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
}

#[test]
fn approx_and_quantity_sentinels_are_retired() {
    for path in [
        "crates/jet-foundation/src/AST/types.rs",
        "crates/jet-foundation/src/Syntax.rs",
    ] {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let text = fs::read_to_string(root.join(path)).unwrap();
        assert!(
            !text.contains("APPROX_NUMERIC_WIDEN_MARKER"),
            "{path}: APPROX_NUMERIC_WIDEN_MARKER must stay retired (card #1662) — \
             use Call::widen_approx"
        );
        assert!(
            !text.contains("\\0Quantity"),
            "{path}: the \\0Quantity string encoding must stay retired (card #1662) — \
             use Type::Quantity"
        );
    }
}
