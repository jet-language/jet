//! M9.5 differential battery (permanent CI). For each expression, the same
//! code is evaluated twice — once as `comptime C = e;` (the sema
//! tree-walking interpreter) and once as a runtime `r :: e` (generated
//! Rust). The program prints both; the two lines MUST be byte-identical.
//!
//! Divergence is a P0 miscompile-class bug (S26 rule 6: comptime implements
//! runtime semantics exactly — i64 Int, IEEE f64 Float with S21 display,
//! char-counted Strings (S41), BTreeMap ordering (S38)).

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod common;
use common::{have_rustc, panic_message, test_worker_count};

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
    // D-BIGINT1 (card #392): arbitrary-precision arithmetic — no overflow,
    // no auto-promotion. comptime must match AOT's limb-based `JetBigInt`
    // byte-for-byte (R12 parity).
    "BigInt(9223372036854775807) + BigInt(1)",
    "BigInt(\"999999999999999999999999999999\") + BigInt(\"999999999999999999999999999999\")",
    "BigInt(100) - BigInt(1)",
    "BigInt(7) * BigInt(6)",
    "BigInt(5).sub(BigInt(3))",
    "BigInt(3).neg()",
];

#[test]
fn comptime_matches_runtime() {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime differential battery");
        return;
    }
    let next = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(Mutex::new(Vec::new()));
    let workers = test_worker_count(16).min(CASES.len().max(1));
    let mut handles = Vec::new();
    for _ in 0..workers {
        let next = Arc::clone(&next);
        let failures = Arc::clone(&failures);
        handles.push(std::thread::spawn(move || {
            loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= CASES.len() {
                    break;
                }
                if let Err(payload) = std::panic::catch_unwind(|| check_comptime_case(i, CASES[i]))
                {
                    failures.lock().unwrap().push(panic_message(payload));
                }
            }
        }));
    }
    for handle in handles {
        handle.join().unwrap();
    }
    let failures = failures.lock().unwrap();
    if !failures.is_empty() {
        panic!("{}", failures.join("\n\n"));
    }
}

fn check_comptime_case(i: usize, expr: &str) {
    let src = format!(
        "comptime C = {e}\n\nfn run() {{\n    r :: {e}\n    print(\"{{C}}\")\n    print(\"{{r}}\")\n}}\n",
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

    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_ctdiff_{}_{}.rs", std::process::id(), i));
    let bin = dir.join(format!("jet_ctdiff_{}_{}", std::process::id(), i));
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

#[test]
fn local_comptime_is_literal_data() {
    let stdout = compile_and_run(
        r#"
fn build() -> [Int] {
    xs: [Int] := []
    loop i in 1..3 {
        xs.push(i * 10)
    }
    return xs
}

fn run() {
    comptime xs = build()
    print("{xs}")
    print("{xs[1]}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["[10, 20, 30]", "20"]
    );
}

#[test]
fn struct_and_enum_comptime_values_round_trip() {
    let stdout = compile_and_run(
        r#"
struct Pair {
    left: Int
    right: String
}

enum Light {
    Red
    Green
}

comptime P = Pair.{left: 7, right: "seven"}
comptime L = Light.Green

fn run() {
    p :: Pair.{left: 7, right: "seven"}
    l :: Light.Green
    print("{P.left}")
    print("{p.left}")
    print("{P.right}")
    print("{p.right}")
    print("{L == Light.Green}")
    print("{l == Light.Green}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["7", "7", "seven", "seven", "true", "true"]
    );
}

#[test]
fn if_expr_comptime_matches_runtime() {
    let stdout = compile_and_run(
        r#"
comptime C = if 3 > 2 { 10 } else { 20 }
comptime D = if 1 > 2 { 10 } else { 20 }

fn run() {
    c :: if 3 > 2 { 10 } else { 20 }
    d :: if 1 > 2 { 10 } else { 20 }
    print("{C}")
    print("{c}")
    print("{D}")
    print("{d}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["10", "10", "20", "20"]
    );
}

#[test]
fn fan_out_comptime_matches_runtime() {
    let stdout = compile_and_run(
        r#"
fn double(x: Int) -> Int {
    return x * 2
}

comptime C = double.[1, 2, 3]

fn run() {
    c :: double.[1, 2, 3]
    print("{C}")
    print("{c}")
}
"#,
    );
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        vec!["[2, 4, 6]", "[2, 4, 6]"]
    );
}

fn compile_and_run(src: &str) -> String {
    let have_rustc = have_rustc();
    if !have_rustc {
        eprintln!("note: rustc not found; skipping comptime run fixture");
        return String::new();
    }
    let compiled = match jet::compile(src) {
        Ok(c) => c,
        Err(diags) => panic!(
            "fixture failed the front end:\n{}",
            jet::render_diagnostics("comptime_fixture.jet", src, &diags)
        ),
    };
    let dir = std::env::temp_dir();
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let rs = dir.join(format!("jet_ct_fixture_{}.rs", id));
    let bin = dir.join(format!("jet_ct_fixture_{}", id));
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
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    assert!(run.status.success(), "fixture panicked at runtime");
    String::from_utf8(run.stdout).unwrap()
}
