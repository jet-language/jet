//! Capability-claim acceptance lanes (UL0 / pre-push ledger).
//!
//! Each `#[test]` named in `docs/spec/capability-claim-manifest.json` must
//! contain the exact lane marker `CAPABILITY_CLAIM: <claim-id> / <lane-id>`
//! in its body. Proven claims run these via
//! `check-capability-ledger.mjs --verify-focused`.

mod common;

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

fn lex_probe(source: &str) -> Vec<jet::Lexer::Token> {
    let (tokens, diagnostics) = jet::Lexer::lex(source);
    assert!(
        diagnostics.is_empty(),
        "capability probe source must lex cleanly: {diagnostics:#?}\n{source}"
    );
    tokens
}

fn token_stream_has_executable_drop(tokens: &[jet::Lexer::Token]) -> bool {
    let code: Vec<_> = tokens
        .iter()
        .filter(|token| !jet::Lexer::is_comment(&token.kind))
        .collect();
    let here = code.windows(5).any(|window| {
        matches!(
            (
                &window[0].kind,
                &window[1].kind,
                &window[2].kind,
                &window[3].kind,
                &window[4].kind,
            ),
            (
                jet::Lexer::TokKind::Dot,
                jet::Lexer::TokKind::Ident(method),
                jet::Lexer::TokKind::LParen,
                jet::Lexer::TokKind::Str(parts),
                jet::Lexer::TokKind::RParen,
            ) if method == "drop"
                && matches!(
                    parts.as_slice(),
                    [jet::Lexer::StrTokPart::Lit(reason)] if !reason.is_empty()
                )
        )
    });
    here || tokens.iter().any(|token| match &token.kind {
        jet::Lexer::TokKind::Str(parts) => parts.iter().any(|part| match part {
            jet::Lexer::StrTokPart::Interp(inner) => token_stream_has_executable_drop(inner),
            jet::Lexer::StrTokPart::Lit(_) => false,
        }),
        _ => false,
    })
}

fn has_executable_drop_with_reason(source: &str) -> bool {
    token_stream_has_executable_drop(&lex_probe(source))
}

fn token_stream_has_active_suppress(tokens: &[jet::Lexer::Token]) -> bool {
    let code: Vec<_> = tokens
        .iter()
        .filter(|token| !jet::Lexer::is_comment(&token.kind))
        .collect();
    let here = code.windows(2).any(|window| {
        matches!(
            (&window[0].kind, &window[1].kind),
            (
                jet::Lexer::TokKind::Hash,
                jet::Lexer::TokKind::Ident(marker),
            ) if marker == "Suppress"
        )
    });
    here || tokens.iter().any(|token| match &token.kind {
        jet::Lexer::TokKind::Str(parts) => parts.iter().any(|part| match part {
            jet::Lexer::StrTokPart::Interp(inner) => token_stream_has_active_suppress(inner),
            jet::Lexer::StrTokPart::Lit(_) => false,
        }),
        _ => false,
    })
}

fn has_active_suppress(source: &str) -> bool {
    token_stream_has_active_suppress(&lex_probe(source))
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

/// claim.metaprogramming / source-reentry — D-METADERIVE1.
#[test]
fn derive_source_reentry() {
    // CAPABILITY_CLAIM: claim.metaprogramming / source-reentry
    let serde_registration = read("crates/jet-sema/src/Sema/Registration/Serde.rs");
    let codegen_items = read("crates/jet-codegen/src/Codegen/Items.rs");
    assert!(
        serde_registration.contains("source.push_str(&format!(\"impl {}.Encode")
            && serde_registration.contains("source.push_str(&format!(\"impl {}.Decode")
            && serde_registration.contains("parse_generated_fragment(\n            &source")
            && serde_registration.contains("crate::Parser::parse(&tokens)"),
        "built-in codecs must emit ordinary Jet impl fragments and parse them"
    );
    for retired in [
        "__JetSerdeCarrier",
        "__JetSerdeGenerated",
        "trait_impls.extend",
    ] {
        assert!(
            !serde_registration.contains(retired),
            "retired AST transplant path remains: {retired}"
        );
    }
    assert!(
        !codegen_items.contains("emit_struct_serde")
            && !codegen_items.contains("emit_enum_serde"),
        "direct Rust serde synthesis must stay deleted"
    );

    let source = r#"
use core.encoding.json as json
#Codable
struct Point { x: Int }
fn run() {
    p := Point.{ x: 7 }
    print(json.to_string(p))
}
"#;
    let dir = std::env::temp_dir().join(format!(
        "jet_capability_derive_reentry_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create derive acceptance dir");
    let path = dir.join("main.jet");
    fs::write(&path, source).expect("write derive acceptance source");
    let compiled = jet::compile_with_path(source, path.to_str().unwrap()).unwrap_or_else(|diags| {
        panic!("generated codec did not re-enter the front end: {diags:#?}")
    });
    assert!(
        compiled.rust.contains("impl __jet_Encode for __jet_Point")
            && compiled.rust.contains("impl __jet_Decode for __jet_Point"),
        "ordinary parsed codec impls must reach TIR/codegen"
    );
    let _ = fs::remove_dir_all(&dir);
}

/// claim.discard-control / audited-discard — D-IGNORERET2=A + D-MARK-DISCARD1=A.
#[test]
fn audited_discard() {
    // CAPABILITY_CLAIM: claim.discard-control / audited-discard
    let syntax = read("crates/jet-foundation/src/Syntax/package_files.rs");
    assert!(
        syntax.contains("METHOD_DROP") || syntax.contains("\"drop\""),
        "Syntax must register the `.drop` discard terminal"
    );
    assert!(
        !syntax.contains("pub const MARKER_SUPPRESS"),
        "D-MARK-DISCARD1 retired `#Suppress(MustUse)`"
    );

    let example = read("examples/features/errors/discard_fallible.jet");
    for fake in [
        "// value.drop(\"line comment\")",
        "/* value.drop(\"block comment\") */",
        "/* outer /* value.drop(\"nested comment\") */ still comment */",
        "print(\"value.drop(\\\"string contents\\\")\")",
        "\"\"\"\nordinary \"quotes\" and value.drop(\"triple contents\")\n\"\"\"",
        "value.drop(\"\")",
        "value.drop(reason)",
        "value.dropper(\"not the method\")",
    ] {
        assert!(
            !has_executable_drop_with_reason(fake),
            "discard probe accepted inactive or non-literal call: {fake}"
        );
    }
    assert!(
        has_executable_drop_with_reason("value.drop(\"理由\")"),
        "discard probe must accept an active nonempty Unicode reason"
    );
    assert!(
        has_executable_drop_with_reason(
            "\"\"\"\ndiscarded: {value.drop(\"interpolated reason\")}\n\"\"\""
        ),
        "discard probe must inspect active triple-string interpolations"
    );
    assert!(
        has_active_suppress(
            "#Suppress /* outer /* nested */ comment */ (\n MustUse /* gap */ ) { }"
        ),
        "discard probe must detect active retired blocks across trivia"
    );
    assert!(
        has_active_suppress(
            "\"\"\"\nretired: {#Suppress(MustUse) { value }}\n\"\"\""
        ),
        "discard probe must inspect active marker syntax in string interpolations"
    );
    for fake in [
        "// #Suppress(MustUse) { }",
        "/* outer /* #Suppress(MustUse) { } */ still comment */",
        "print(\"#Suppress(MustUse) {{ }}\")",
        "\"\"\"\nordinary \"quotes\" and #Suppress(MustUse) {{ }}\n\"\"\"",
    ] {
        assert!(
            !has_active_suppress(fake),
            "discard probe treated comments or string contents as active: {fake}"
        );
    }
    assert!(
        has_executable_drop_with_reason(&example)
            && !has_active_suppress(&example),
        "I5 example must exercise the sole discard channel"
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

/// claim.maturity-tags / maturity-convention — D-MARK-META1=B.
#[test]
fn maturity_convention() {
    // CAPABILITY_CLAIM: claim.maturity-tags / maturity-convention
    let docs = read("docs/reference/maturity-tags.md");
    assert!(
        docs.contains("#Meta(maturity: .Experimental)")
            && docs.contains(".Tested")
            && docs.contains(".Hardened"),
        "reference docs must name all three maturity metadata values"
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
        syntax.contains("MARKER_EXPERIMENTAL")
            && syntax.contains("MARKER_TESTED")
            && syntax.contains("MARKER_HARDENED"),
        "Syntax.rs must register maturity values (I7)"
    );

    let example = read("examples/features/syntax/maturity_tags.jet");
    assert!(
        example.contains("#Meta(maturity: .Experimental)")
            && example.contains("#Meta(maturity: .Tested)")
            && example.contains("#Meta(maturity: .Hardened)"),
        "I5 example must use all three maturity metadata values"
    );
    let expected = read("examples/features/expected/syntax/maturity_tags.out");
    let got = run_example("examples/features/syntax/maturity_tags.jet");
    assert_eq!(
        got.trim(),
        expected.trim(),
        "maturity_tags golden mismatch"
    );

    // Zero sema effect: no diagnostic/codegen policy keyed on maturity.
    let retired_at = read("tests/ui/marker_experimental_at.stderr");
    assert!(
        retired_at.contains("isn't a known marker") && retired_at.contains("Experimental"),
        "retired standalone `#Experimental` must be an ordinary unknown marker"
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
        .args(["inspect", "graph", entry.to_str().unwrap(), "--json"])
        .current_dir(root())
        .output()
        .expect("jet inspect graph");
    assert!(
        graph.status.success(),
        "jet inspect graph failed:\n{}",
        String::from_utf8_lossy(&graph.stderr)
    );
    let graph_json = String::from_utf8_lossy(&graph.stdout);
    assert!(
        graph_json.contains("programmable_build") && graph_json.contains("\"targets\""),
        "jet inspect graph must expose typed targets: {graph_json}"
    );

    let query = Command::new(jet_bin())
        .args(["inspect", "query", "build", entry.to_str().unwrap(), "--json"])
        .current_dir(root())
        .output()
        .expect("jet inspect query build");
    assert!(
        query.status.success(),
        "jet inspect query build failed:\n{}",
        String::from_utf8_lossy(&query.stderr)
    );
    let query_json = String::from_utf8_lossy(&query.stdout);
    assert!(
        query_json.contains("programmable_build"),
        "jet inspect query build must share graph facts: {query_json}"
    );

    let explain = Command::new(jet_bin())
        .args([
            "inspect",
            "explain-build",
            "programmable_build",
            entry.to_str().unwrap(),
            "--json",
        ])
        .current_dir(root())
        .output()
        .expect("jet inspect explain-build");
    assert!(
        explain.status.success(),
        "jet inspect explain-build failed:\n{}",
        String::from_utf8_lossy(&explain.stderr)
    );
    let explain_json = String::from_utf8_lossy(&explain.stdout);
    assert!(
        explain_json.contains("\"provenance\"")
            && (explain_json.contains("actions=") || explain_json.contains("sources=")),
        "explain-build must emit provenance: {explain_json}"
    );

    let cli = read("crates/jet-cli/src/CLI.rs");
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

#[Pre(n >= 0, "n non-negative"), Post(result >= 0, "result non-negative")]
fn absish(n: Int) => Int {
    return n
}

#Scrub(Input) fn clean(raw: #Input String) => String {
    return raw
}

#Replayable fn add(a: Int, b: Int) => Int {
    return a + b
}

fn stamp(path: String) =[FS]=> String ? {
    return path
}

fn pick(xs: [String#4], i: Index4) => String {
    return xs[i]
}

fn run() {
    dirty :: #Input "x"
    safe := clean(dirty)
    words :: [String#4].{ "a", "b", "c", "d" }
    print(pick(words, Index4.from_int(1)))
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
        read("examples/features/contracts/pre_post.jet").contains("#Pre"),
        "I5 contracts example must remain"
    );
    assert!(
        read("examples/features/effects/taint.jet").contains("#Input")
            && read("examples/features/effects/taint.jet").contains("#Scrub(Input)"),
        "I5 taint/IFC slice example must remain"
    );
    assert!(
        read("examples/features/packages/effect_budget/package.jet").contains("effects:"),
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
