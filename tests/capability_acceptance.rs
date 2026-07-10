//! Capability-claim acceptance lanes (UL0 / pre-push ledger).
//!
//! Each `#[test]` named in `docs/spec/capability-claim-manifest.json` must
//! contain the exact lane marker `CAPABILITY_CLAIM: <claim-id> / <lane-id>`
//! in its body. Proven claims run these via
//! `check-capability-ledger.mjs --verify-focused`.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn jet_bin() -> PathBuf {
    let from_env = std::env::var_os("CARGO_BIN_EXE_jet").map(PathBuf::from);
    if let Some(p) = from_env {
        if p.exists() {
            return p;
        }
    }
    // Dev-shell / integration: prefer freshly built debug jet next to tests.
    let candidates = [
        root().join("target/debug/jet"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/debug/jet"),
    ];
    for c in candidates {
        if c.exists() {
            return c;
        }
    }
    PathBuf::from("jet")
}

fn run_example(rel: &str) -> String {
    let path = root().join(rel);
    let out = Command::new(jet_bin())
        .arg("run")
        .arg(&path)
        .current_dir(root())
        .output()
        .unwrap_or_else(|e| panic!("jet run {rel}: {e}"));
    assert!(
        out.status.success(),
        "jet run {rel} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Strip optional effects banner lines.
    stdout
        .lines()
        .filter(|l| !l.starts_with("effects:"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// claim.discard-control / audited-discard — D-IGNORERET2=A + D-MUSTUSE1.
#[test]
fn audited_discard() {
    // CAPABILITY_CLAIM: claim.discard-control / audited-discard
    let syntax = read("crates/jet-foundation/src/Syntax/package_files.rs");
    assert!(
        syntax.contains("METHOD_DROP") || syntax.contains("\"drop\""),
        "Syntax must register the `.drop` discard terminal"
    );
    assert!(
        syntax.contains("ATTR_SUPPRESS") || syntax.contains("\"Suppress\""),
        "Syntax must register `#Suppress(MustUse)`"
    );

    let example = read("examples/features/errors/discard_fallible.jet");
    assert!(
        example.contains(".drop(\"") && example.contains("#Suppress(MustUse)"),
        "I5 example must exercise both discard channels"
    );
    let expected = read("examples/features/expected/errors/discard_fallible.out");
    let got = run_example("examples/features/errors/discard_fallible.jet");
    assert_eq!(
        got.trim(),
        expected.trim(),
        "discard_fallible golden mismatch"
    );

    let empty = read("tests/ui/drop_method_empty_reason.stderr");
    assert!(
        empty.contains("E0407") && empty.contains(".drop()"),
        "empty-reason discard must be E0407"
    );
    let silence = read("tests/ui/drop_method_silences_e0402.stderr");
    assert!(
        silence.trim().is_empty() || !silence.contains("E0402"),
        "`.drop(\"…\")` must silence bare-ignore E0402"
    );
}

/// claim.prelude-control / prelude-opt-out — D-PRELUDEX1=A.
#[test]
fn prelude_opt_out() {
    // CAPABILITY_CLAIM: claim.prelude-control / prelude-opt-out
    let syntax = read("crates/jet-foundation/src/Syntax/core_surface.rs");
    let package = read("crates/jet-foundation/src/Syntax/package_files.rs");
    assert!(
        syntax.contains("MARKER_NO_PRELUDE")
            || package.contains("MARKER_NO_PRELUDE")
            || syntax.contains("NoPrelude")
            || package.contains("NoPrelude"),
        "Syntax must register `#NoPrelude`"
    );

    let example = read("examples/features/io/no_prelude.jet");
    assert!(
        example.starts_with("#NoPrelude") || example.contains("\n#NoPrelude\n") || example.lines().next() == Some("#NoPrelude"),
        "I5 example must declare `#NoPrelude`"
    );
    assert!(
        example.contains("use core.io"),
        "opt-out example must use explicit core.io"
    );
    let expected = read("examples/features/expected/io/no_prelude.out");
    let got = run_example("examples/features/io/no_prelude.jet");
    assert_eq!(got.trim(), expected.trim(), "no_prelude golden mismatch");

    let bare = read("tests/ui/no_prelude_print.stderr");
    assert!(
        bare.contains("E0429") && bare.contains("#NoPrelude"),
        "bare `print` under `#NoPrelude` must be E0429"
    );
    let dup = read("tests/ui/no_prelude_duplicate.stderr");
    assert!(
        dup.contains("E0428") || dup.contains("NoPrelude"),
        "duplicate `#NoPrelude` must diagnose"
    );
}

/// claim.maturity-tags / maturity-convention — D-MATURITY1=B.
#[test]
fn maturity_convention() {
    // CAPABILITY_CLAIM: claim.maturity-tags / maturity-convention
    let docs = read("docs/reference/maturity-tags.md");
    assert!(
        docs.contains("@Experimental")
            && docs.contains("@Tested")
            && docs.contains("@Hardened"),
        "reference docs must name all three maturity tags"
    );
    assert!(
        docs.contains("do not propagate")
            || docs.contains("does not warn")
            || docs.contains("zero semantic")
            || docs.contains("without changing compiler behavior"),
        "docs must state non-semantic / no-propagation contract"
    );

    let syntax = read("crates/jet-foundation/src/Syntax/package_files.rs");
    assert!(
        syntax.contains("ATTR_EXPERIMENTAL")
            && syntax.contains("ATTR_TESTED")
            && syntax.contains("ATTR_HARDENED"),
        "Syntax.rs must register maturity tag constants (I7)"
    );

    let example = read("examples/features/syntax/maturity_tags.jet");
    assert!(
        example.contains("@Experimental")
            && example.contains("@Tested")
            && example.contains("@Hardened"),
        "I5 example must use all three @ maturity tags"
    );
    let expected = read("examples/features/expected/syntax/maturity_tags.out");
    let got = run_example("examples/features/syntax/maturity_tags.jet");
    assert_eq!(
        got.trim(),
        expected.trim(),
        "maturity_tags golden mismatch"
    );

    // Zero sema effect: no diagnostic/codegen policy keyed on maturity.
    let e0062 = read("tests/ui/marker_experimental_hash.stderr");
    assert!(
        e0062.contains("E0062") && e0062.contains("@Experimental"),
        "retired `#Experimental` must teach `@Experimental` (E0062)"
    );

    let sema_hits = Command::new("rg")
        .args([
            "-n",
            r"\.maturity\b|MaturityTag",
            "crates/jet-sema/src",
        ])
        .current_dir(root())
        .output();
    if let Ok(out) = sema_hits {
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            text.trim().is_empty(),
            "sema must not read maturity tags (zero semantic effect):\n{text}"
        );
    }
}

/// claim.package-build / public-build-product — D-BUILDENTRY1 + D-BUILDQUERY1.
#[test]
fn public_build_product() {
    // CAPABILITY_CLAIM: claim.package-build / public-build-product
    let example = read("examples/features/tooling/programmable_build/main.jet");
    assert!(
        example.contains("fn build(b: BuildContext)")
            && example.contains("b.generate")
            && example.contains("b.add_executable")
            && example.contains("b.plan"),
        "I5 example must be a public root fn build graph"
    );

    let entry = root().join("examples/features/tooling/programmable_build/main.jet");
    let compiled = jet::compile_programmable_build(entry.to_str().unwrap(), &[])
        .unwrap_or_else(|diags| panic!("fn build path failed: {diags:#?}"));
    assert!(
        compiled.rust.contains("generated_build_message") && compiled.rust.contains("fn main"),
        "programmable build must re-enter generated source into codegen"
    );

    // I5 golden: execute path materializes generated Jet, then rustc runs it.
    let expected = read("examples/features/expected/tooling/programmable_build.out");
    let dir = std::env::temp_dir();
    let rs = dir.join(format!("jet_cap_build_{}.rs", std::process::id()));
    let bin = dir.join(format!("jet_cap_build_{}", std::process::id()));
    fs::write(&rs, &compiled.rust).expect("write generated rust");
    let rustc = Command::new("rustc")
        .args(["--edition", "2021"])
        .arg(&rs)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("rustc");
    assert!(
        rustc.status.success(),
        "I2: rustc rejected programmable build codegen:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().expect("run programmable build bin");
    assert!(
        run.status.success(),
        "programmable build binary failed:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        expected.trim(),
        "programmable_build golden mismatch"
    );
    let _ = fs::remove_file(&rs);
    let _ = fs::remove_file(&bin);

    let graph = Command::new(jet_bin())
        .args(["graph", entry.to_str().unwrap(), "--json"])
        .current_dir(root())
        .output()
        .expect("jet graph");
    assert!(
        graph.status.success(),
        "jet graph failed:\n{}",
        String::from_utf8_lossy(&graph.stderr)
    );
    let graph_json = String::from_utf8_lossy(&graph.stdout);
    assert!(
        graph_json.contains("programmable_build") && graph_json.contains("\"targets\""),
        "jet graph must expose typed targets: {graph_json}"
    );

    let query = Command::new(jet_bin())
        .args(["query", "build", entry.to_str().unwrap(), "--json"])
        .current_dir(root())
        .output()
        .expect("jet query build");
    assert!(
        query.status.success(),
        "jet query build failed:\n{}",
        String::from_utf8_lossy(&query.stderr)
    );
    let query_json = String::from_utf8_lossy(&query.stdout);
    assert!(
        query_json.contains("programmable_build"),
        "jet query build must share graph facts: {query_json}"
    );

    let explain = Command::new(jet_bin())
        .args([
            "explain-build",
            "programmable_build",
            entry.to_str().unwrap(),
            "--json",
        ])
        .current_dir(root())
        .output()
        .expect("jet explain-build");
    assert!(
        explain.status.success(),
        "jet explain-build failed:\n{}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let explain_json = String::from_utf8_lossy(&explain.stdout);
    assert!(
        explain_json.contains("\"provenance\"")
            && (explain_json.contains("actions=") || explain_json.contains("sources=")),
        "explain-build must emit provenance: {explain_json}"
    );

    let cli = read("Source/CLI.rs");
    assert!(
        cli.contains("name: \"graph\"")
            && cli.contains("name: \"query\"")
            && cli.contains("name: \"explain-build\"")
            && cli.contains("claim.package-build"),
        "public build query commands must be owned by claim.package-build"
    );

    let sandbox = read("tests/ui/build_action_failed.stderr");
    assert!(
        sandbox.contains("E3505") && sandbox.contains("build sandbox"),
        "sandboxed action failure must teach E3505"
    );
    let build_entry = read("tests/build_entry.rs");
    assert!(
        build_entry.contains("sandbox_refuses_output_parent_symlink_escape")
            && build_entry.contains("RestoredFromCache"),
        "build_entry must prove sandbox confinement and action-cache restore"
    );

    let dispatch = read("Source/EngineDispatch.rs");
    assert!(
        dispatch.contains("D-JPK-DISPATCH1")
            && dispatch.contains("find_engine_binary")
            && dispatch.contains("jetpack"),
        "product dispatch must exec jetpack engine verbs out-of-process"
    );
    let main = read("Source/main.rs");
    assert!(
        main.contains("EngineDispatch::dispatch")
            && main.contains("JETPACK_BINARY_NAME"),
        "jet front door must dispatch package/env verbs to jetpack"
    );
}
