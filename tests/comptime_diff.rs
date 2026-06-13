//! M9.5 differential battery (permanent CI). For each expression, the same
//! code is evaluated twice — once as `comptime C = e;` (the sema
//! tree-walking interpreter) and once as a runtime `val r = e;` (generated
//! Rust). The program prints both; the two lines MUST be byte-identical.
//!
//! Divergence is a P0 miscompile-class bug (S26 rule 6: comptime implements
//! runtime semantics exactly — i64 Int, IEEE f64 Float with S21 display,
//! char-counted Strings (S41), BTreeMap ordering (S38)).

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Expressions whose comptime and runtime evaluation must agree. Each is
/// inlined verbatim on both sides, so it must be a self-contained
/// expression with an inferable type.
const CASES: &[&str] = &[
    // Int arithmetic + operator semantics
    "2 + 3 * 4",
    "100 / 7",
    "100 % 7",
    "7 / 2",
    "(0 - 17) % 5",
    "(0 - 17) / 5",
    "1 << 10",
    "255 & 15",
    "12 | 3",
    "6 ^ 3",
    "1000000 * 1000000",
    // Float rounding + S21 "always a decimal" display
    "3.0 / 2.0",
    "10.0 / 4.0",
    "1.0 / 3.0",
    "5.0",
    "2.0 * 2.0",
    "0.1 + 0.2",
    // Bool / comparison
    "3 < 5 && 2 == 2",
    "10 >= 10 || false",
    // String + Char ops (char-counted, S41)
    "\"Hello\".to_upper()",
    "\"WORLD\".to_lower()",
    "\"héllo\".len()",
    "\"  trim me  \".trim()",
    "\"ab\".repeat(3)",
    "\"a,b,c\".split(\",\")",
    "\"hello world\".replace(\"o\", \"0\")",
    // List values, ordering, and methods
    "[1, 2, 3]",
    "[3, 1, 2]",
    "[10, 20, 30][1]",
    "[\"x\", \"y\", \"z\"]",
    // Map ordering via derived lists (BTreeMap is sorted by key)
    "[\"b\": 2, \"a\": 1, \"c\": 3].keys()",
    "[\"b\": 2, \"a\": 1, \"c\": 3].values()",
    "[2: \"two\", 1: \"one\"].keys()",
];

#[test]
fn comptime_matches_runtime() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime differential battery");
        return;
    }
    let dir = std::env::temp_dir();
    for (i, expr) in CASES.iter().enumerate() {
        let src = format!(
            "comptime C = {e};\n\nfn main() {{\n    val r = {e};\n    print(\"{{C}}\");\n    print(\"{{r}}\");\n}}\n",
            e = expr
        );
        let compiled = match jet::compile(&src) {
            Ok(c) => c,
            Err(diags) => panic!(
                "case {} `{}` failed the front end:\n{}",
                i,
                expr,
                jet::render_diagnostics("comptime_diff.jet", &src, &diags)
            ),
        };
        assert!(
            !compiled.rust.contains("unsafe"),
            "case `{}` generated unsafe",
            expr
        );

        let rs = dir.join(format!("jet_ctdiff_{}.rs", i));
        let bin = dir.join(format!("jet_ctdiff_{}", i));
        fs::write(&rs, &compiled.rust).unwrap();
        let out = Command::new("rustc")
            .args(["--edition", "2021"])
            .arg(&rs)
            .arg("-o")
            .arg(&bin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "I2 violated: rustc rejected generated code for `{}`:\n{}",
            expr,
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().unwrap();
        assert!(run.status.success(), "case `{}` panicked at runtime", expr);
        let stdout = String::from_utf8_lossy(&run.stdout);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(
            lines.len(),
            2,
            "case `{}` printed {} lines, expected 2",
            expr,
            lines.len()
        );
        assert_eq!(
            lines[0], lines[1],
            "DIVERGENCE for `{}`: comptime gave {:?}, runtime gave {:?} — this is a P0 miscompile",
            expr, lines[0], lines[1]
        );
    }
    let _ = PathBuf::new();
}
