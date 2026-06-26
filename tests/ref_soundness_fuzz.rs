//! E2-M5 soundness property target (feeds the E2-M17 audit).
//!
//! The crown-jewel invariant for tier-2 references: **sema-accepted ⇒
//! rustc-accepted**. If the front end lets a `view` return or a stored `ref`
//! field through, the generated Rust must compile — never an ICE (I2), never a
//! rustc rejection of code sema blessed.
//!
//! This is a bounded, combinatorial generator (not an unbounded fuzzer) so it
//! stays fast enough for CI while covering the matrix cross-product: every
//! source kind × every reference construct. Each generated program is run
//! through sema; the ones sema *accepts* are then compiled by rustc, and a
//! rustc rejection fails the test loudly as a front-end soundness bug.
//!
//! Scope note: the generator varies the dimensions that drove the matrix
//! (source = param / local / field-of-param / index-of-param / fresh literal;
//! construct = `view` return / stored `ref` field; through a generic wrapper
//! or not). It does not mutate arbitrary token streams — the goal is to pin the
//! soundness boundary of the reference rules, not to fuzz the parser.

use std::process::Command;

/// One generated reference program plus a human label for failure messages.
struct Case {
    label: String,
    src: String,
}

/// How the borrowed value is sourced — the dimension that decides soundness.
#[derive(Clone, Copy)]
enum Source {
    Param,
    Local,
    FieldOfParam,
    IndexOfParam,
    FreshLiteral,
}

impl Source {
    fn all() -> &'static [Source] {
        &[
            Source::Param,
            Source::Local,
            Source::FieldOfParam,
            Source::IndexOfParam,
            Source::FreshLiteral,
        ]
    }
}

/// Build a `-> view` return program for the given source.
fn view_return_case(src: Source) -> Case {
    let (label, body) = match src {
        Source::Param => (
            "view_return/param",
            "fn make(p: String) -> &String {\n    return p;\n}\n".to_string(),
        ),
        Source::Local => (
            "view_return/local",
            "fn make(p: String) -> &String {\n    val local: String = \"x\";\n    return local;\n}\n"
                .to_string(),
        ),
        Source::FieldOfParam => (
            "view_return/field_of_param",
            "struct Bin { v: String; }\nfn make(b: Bin) -> &String {\n    return b.v;\n}\n"
                .to_string(),
        ),
        Source::IndexOfParam => (
            "view_return/index_of_param",
            "fn make(xs: [String]) -> &String {\n    return xs[0];\n}\n".to_string(),
        ),
        Source::FreshLiteral => (
            "view_return/fresh_literal",
            "fn make(p: String) -> &String {\n    return \"fresh\";\n}\n".to_string(),
        ),
    };
    Case {
        label: label.to_string(),
        src: format!("{}fn main() {{\n    print(0);\n}}\n", body),
    }
}

/// Build a stored-`ref`-field program for the given source.
fn ref_field_case(src: Source) -> Case {
    let (label, body) = match src {
        Source::Param => (
            "ref_field/param",
            "struct R { ref v: String; }\nfn make(p: String) -> R {\n    return R.{ v: p };\n}\n"
                .to_string(),
        ),
        Source::Local => (
            "ref_field/local",
            "struct R { ref v: String; }\nfn make() {\n    val local: String = \"x\";\n    val r: R = R.{ v: local };\n}\n"
                .to_string(),
        ),
        Source::FieldOfParam => (
            "ref_field/field_of_param",
            "struct Bin { v: String; }\nstruct R { ref v: String; }\nfn make(b: Bin) -> R {\n    return R.{ v: b.v };\n}\n"
                .to_string(),
        ),
        Source::IndexOfParam => (
            "ref_field/index_of_param",
            "struct R { ref v: String; }\nfn make(xs: [String]) -> R {\n    return R.{ v: xs[0] };\n}\n"
                .to_string(),
        ),
        Source::FreshLiteral => (
            "ref_field/fresh_literal",
            "struct R { ref v: String; }\nfn make() {\n    val r: R = R.{ v: \"fresh\" };\n}\n"
                .to_string(),
        ),
    };
    Case {
        label: label.to_string(),
        src: format!("{}fn main() {{\n    print(0);\n}}\n", body),
    }
}

/// Generic-wrapper variants of the `view` return (the generic matrix cell):
/// `-> &T` through a `Wrap<T>` parameter / local.
fn generic_view_cases() -> Vec<Case> {
    vec![
        Case {
            label: "view_return/generic_field_of_param".to_string(),
            src: "struct Wrap<T> { item: T; }\nfn make<T>(w: Wrap<T>) -> &T {\n    return w.item;\n}\nfn main() {\n    print(0);\n}\n".to_string(),
        },
        Case {
            label: "view_return/generic_field_of_local".to_string(),
            src: "struct Wrap<T> { item: T; }\nfn make<T>(x: T) -> &T {\n    val w: Wrap<T> = Wrap<T>.{ item: x };\n    return w.item;\n}\nfn main() {\n    print(0);\n}\n".to_string(),
        },
    ]
}

fn all_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for &s in Source::all() {
        cases.push(view_return_case(s));
        cases.push(ref_field_case(s));
    }
    cases.extend(generic_view_cases());
    cases
}

#[test]
fn sema_accepted_view_ref_programs_are_rustc_accepted() {
    let have_rustc = Command::new("rustc").arg("--version").output().is_ok();
    if !have_rustc {
        eprintln!("note: rustc not found; running sema-only (the ⇒ rustc half is skipped)");
    }

    let cases = all_cases();
    assert!(
        cases.len() >= 12,
        "expected the matrix cross-product, found {}",
        cases.len()
    );

    let mut accepted = 0;
    let mut rejected = 0;

    for case in &cases {
        // `compile_with_path` loads the program from disk (it ignores the
        // in-memory string), so each generated case is written to a temp file
        // and compiled by path — exactly how the golden harness drives it.
        let stem = case.label.replace(['/', '.'], "_");
        let jet_path = std::env::temp_dir().join(format!("jet_refsound_{}.jet", stem));
        std::fs::write(&jet_path, &case.src).unwrap();
        let shown = jet_path.to_string_lossy().into_owned();
        match jet::compile_with_path(&case.src, &shown) {
            Err(_) => {
                // Sema rejected this cell — that's a valid outcome (a rejected
                // matrix cell). The invariant only constrains accepted programs.
                rejected += 1;
            }
            Ok(compiled) => {
                accepted += 1;
                // I1: an accepted program never lowers to `unsafe`.
                assert!(
                    !compiled.rust.contains("unsafe"),
                    "I1 violated: sema-accepted {} lowered to `unsafe`",
                    case.label
                );
                if !have_rustc {
                    continue;
                }
                let dir = std::env::temp_dir();
                let rs = dir.join(format!("jet_refsound_{}.rs", stem));
                let bin = dir.join(format!("jet_refsound_{}", stem));
                std::fs::write(&rs, &compiled.rust).unwrap();
                let mut cmd = Command::new("rustc");
                cmd.args(["--edition", "2021"]).arg(&rs).arg("-o").arg(&bin);
                if let Some(link) = &compiled.ffi {
                    cmd.arg("--extern").arg(format!(
                        "{}={}",
                        link.crate_name,
                        link.rlib_path.display()
                    ));
                }
                let out = cmd.output().unwrap();
                assert!(
                    out.status.success(),
                    "SOUNDNESS HOLE: sema accepted `{}` but rustc rejected the generated Rust \
                     (I2 — a front-end bug). The reference rules let an unsound program through.\n\
                     --- jet source ---\n{}\n--- rustc said ---\n{}",
                    case.label,
                    case.src,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }
    }

    // Sanity: the generator must exercise *both* outcomes, or it isn't probing
    // the boundary. At least one accepted (e.g. view-into-param) and at least
    // one rejected (e.g. view-of-local) must occur.
    assert!(
        accepted >= 1 && rejected >= 1,
        "generator did not straddle the soundness boundary: {} accepted, {} rejected",
        accepted,
        rejected
    );
}
