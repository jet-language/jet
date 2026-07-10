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

/// claim.static-guarantees / shared-facts-engine — one PolicyFactGraph over
/// refinements, contracts, taint/IFC, budgets, bounds, and replay.
#[test]
fn static_guarantees_shared_engine() {
    // CAPABILITY_CLAIM: claim.static-guarantees / shared-facts-engine
    let src = r#"
#Invariant("value >= 0 && value < 4")
Index4 :: distinct Int

@Pre(n >= 0, "n non-negative") @Post(result >= 0, "result non-negative")
fn absish(n: Int) -> Int {
    return n
}

#Sanitizer fn clean(raw: String) -> String {
    return raw
}

#Replayable fn add(a: Int, b: Int) -> Int {
    return a + b
}

fn stamp(path: String) #(Fs) -> String ? {
    return path
}

fn pick(xs: [String#4], i: Index4) -> String {
    return xs[i]
}

fn run() {
    dirty :: #Tainted "x"
    safe := clean(dirty)
    words: [String#4] :: ["a", "b", "c", "d"]
    print(pick(words, Index4(1)))
    print(absish(3))
    print(add(1, 2))
    print(safe)
}
"#;
    let graph = jet::Sema::collect_policy_facts(src)
        .unwrap_or_else(|diags| panic!("policy fact collect failed: {diags:#?}"));
    for domain in [
        jet::Sema::PolicyDomain::Refinement,
        jet::Sema::PolicyDomain::Contract,
        jet::Sema::PolicyDomain::Taint,
        jet::Sema::PolicyDomain::Budget,
        jet::Sema::PolicyDomain::Bounds,
        jet::Sema::PolicyDomain::Replay,
    ] {
        assert!(
            graph.has_domain(domain),
            "shared fact graph missing {:?}; facts={:#?}",
            domain,
            graph.facts()
        );
    }

    // Existing verticals still ship through the same engine surface.
    assert!(
        read("examples/features/types/refinements.jet").contains("#Invariant"),
        "I5 refinements example must remain"
    );
    assert!(
        read("examples/features/contracts/pre_post.jet").contains("@Pre"),
        "I5 contracts example must remain"
    );
    assert!(
        read("examples/features/effects/taint.jet").contains("#Tainted")
            && read("examples/features/effects/taint.jet").contains("#Sanitizer"),
        "I5 taint/IFC slice example must remain"
    );
    assert!(
        read("examples/features/packages/effect_budget/pkg.jet").contains("effects:"),
        "I5 effect-budget example must remain"
    );
    let replay_ui = read("tests/ui/replayable_reaches_io.stderr");
    assert!(
        replay_ui.contains("E0725") && replay_ui.contains("Replayable"),
        "replay soundness must teach E0725"
    );
    let bounds_ui = read("tests/ui/fixed_list_index_bounds.stderr");
    assert!(
        bounds_ui.contains("E0965"),
        "bounds proof miss must teach E0965"
    );
    let engine = read("crates/jet-sema/src/Sema/PolicyFacts.rs");
    assert!(
        engine.contains("PolicyFactGraph") && engine.contains("claim.static-guarantees"),
        "shared PolicyFactGraph must own claim.static-guarantees"
    );
}

/// claim.format-test / project-format-test — D-FMTPROJECT1 + D-TOOL4.
#[test]
fn project_format_and_test() {
    // CAPABILITY_CLAIM: claim.format-test / project-format-test
    let cmd = read("Source/CmdCompile.rs");
    assert!(
        cmd.contains("JET_UPDATE_SNAPSHOTS")
            && cmd.contains("if update_snapshots")
            && !cmd.contains("_update_snapshots: bool"),
        "run_test_cov must honor update_snapshots (no ignored _update_snapshots)"
    );
    assert!(
        read("tests/fmt_project.rs").contains("D-FMTPROJECT1"),
        "project formatter workflow must have dedicated tests"
    );

    let dir = std::env::temp_dir().join(format!(
        "jet_cap_format_test_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).expect("mkdir");
    // Unformatted project file — `jet fmt` must rewrite it.
    let unformatted = "fn run()  {\n  print( \"hi\" )\n}\n";
    let main = dir.join("src/main.jet");
    fs::write(&main, unformatted).expect("write main");
    let fmt = Command::new(jet_bin())
        .args(["fmt", "src"])
        .current_dir(&dir)
        .output()
        .expect("jet fmt");
    assert!(
        fmt.status.success(),
        "jet fmt project failed:\n{}",
        String::from_utf8_lossy(&fmt.stderr)
    );
    let formatted = fs::read_to_string(&main).expect("read formatted");
    assert_ne!(formatted, unformatted, "project fmt must rewrite dirty files");
    let check = Command::new(jet_bin())
        .args(["fmt", "--check", "src"])
        .current_dir(&dir)
        .output()
        .expect("jet fmt --check");
    assert_eq!(
        check.status.code(),
        Some(0),
        "fmt --check must be clean after format:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // D-TOOL4: seed a stale snap, prove `-u` rewrites it via expect().snapshot().
    let test_src = r#"
fn run() {}

#Test("snapshot updates") {
    expect("fresh-value").snapshot()
}
"#;
    let test_file = dir.join("snap.jet");
    fs::write(&test_file, test_src).expect("write snap.jet");
    // First run with -u creates the golden.
    let create = Command::new(jet_bin())
        .args(["test", "--update-snapshots", "snap.jet"])
        .current_dir(&dir)
        .output()
        .expect("jet test -u create");
    assert!(
        create.status.success(),
        "jet test -u create failed:\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );
    let snap_dir = dir.join("snapshots");
    let snaps: Vec<_> = fs::read_dir(&snap_dir)
        .expect("snapshots dir after -u")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("snap"))
        .collect();
    assert_eq!(snaps.len(), 1, "expected one .snap under {snap_dir:?}");
    let snap_path = &snaps[0];
    assert_eq!(
        fs::read_to_string(snap_path).unwrap().trim(),
        "fresh-value",
        "created snap must hold the expected value"
    );

    // Stale golden without -u must fail.
    fs::write(snap_path, "stale-value").expect("seed stale");
    let stale = Command::new(jet_bin())
        .args(["test", "snap.jet"])
        .current_dir(&dir)
        .output()
        .expect("jet test stale");
    assert!(
        !stale.status.success(),
        "stale snapshot must fail without -u:\n{}",
        String::from_utf8_lossy(&stale.stdout)
    );

    // -u must rewrite the golden and pass.
    let update = Command::new(jet_bin())
        .args(["test", "-u", "snap.jet"])
        .current_dir(&dir)
        .output()
        .expect("jet test -u");
    assert!(
        update.status.success(),
        "jet test -u must pass:\nstdout:{}\nstderr:{}",
        String::from_utf8_lossy(&update.stdout),
        String::from_utf8_lossy(&update.stderr)
    );
    assert_eq!(
        fs::read_to_string(snap_path).unwrap().trim(),
        "fresh-value",
        "-u must rewrite the stale snapshot"
    );

    // testing.snap also honors JET_UPDATE_SNAPSHOTS=1 via the same flag.
    let helper = r#"
use core.testing as testing

fn run() {}

#Test("testing.snap updates") {
    require(testing.snap("helper-case", "helper-fresh"))
}
"#;
    fs::write(dir.join("helper.jet"), helper).expect("write helper");
    fs::create_dir_all(dir.join("__snapshots__")).expect("mkdir __snapshots__");
    fs::write(dir.join("__snapshots__/helper-case.snap"), "helper-stale").expect("seed helper");
    let helper_stale = Command::new(jet_bin())
        .args(["test", "helper.jet"])
        .current_dir(&dir)
        .output()
        .expect("helper stale");
    assert!(
        !helper_stale.status.success(),
        "testing.snap stale must fail without -u"
    );
    let helper_update = Command::new(jet_bin())
        .args(["test", "--update-snapshots", "helper.jet"])
        .current_dir(&dir)
        .output()
        .expect("helper -u");
    assert!(
        helper_update.status.success(),
        "testing.snap -u must pass:\n{}",
        String::from_utf8_lossy(&helper_update.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.join("__snapshots__/helper-case.snap"))
            .unwrap()
            .trim(),
        "helper-fresh"
    );

    let _ = fs::remove_dir_all(&dir);
}
