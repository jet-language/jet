//! Stage 1a — `module name { … }` declarations (U3, unified-ecosystem §4).
//! Parser-level: the module shell (many per file, leading-`_` disable) and its
//! typed namespace contributions (`env.dev: Env.{ … }`). Contribution *values*
//! reuse the existing struct-literal expression parser.

mod common;

#[path = "tir_support/mod.rs"]
mod tir_support;

use std::fs;
use std::path::Path;

use jet::AST::{Call, Contribution, Expr, Item, Namespace, StrPart};
use tir_support::{build_and_run_multi, have_rustc, run_default_multi};

fn parse_items(src: &str) -> Vec<Item> {
    let (toks, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    jet::Parser::parse(&toks).expect("parse").items
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (relative, source) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, source).unwrap();
    }
}

fn run_forced_interpreter(
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    let scratch = common::Scratch::new(name);
    write_files(&scratch.path, files);
    let path = scratch.join(entry);
    match jet::Interpreter::dev_iteration(path.to_str().unwrap(), false, true) {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => (exit_code, stdout, stderr),
        jet::Interpreter::RunOutcome::Problems(diags) => {
            panic!("forced interpreter rejected {name}: {diags:?}")
        }
    }
}

fn assert_bodyless_module_runs(name: &str, declaration: &str) {
    let main_src = "module lib;\nuse lib.[helper]\nfn run() { print(helper.value()) }\n";
    let lib_src = format!("{} module helper;\n", declaration);
    let files = [
        ("main.jet", main_src),
        ("lib.jet", lib_src.as_str()),
        (
            "helper.jet",
            "pub fn value() => String { return \"visible\" }\n",
        ),
    ];

    if have_rustc() {
        let (code, stdout) = build_and_run_multi(name, "main.jet", &files);
        assert_eq!(code, 0);
        assert_eq!(stdout, "visible\n");
    }

    let (code, stdout, stderr) = run_default_multi(name, "main.jet", &files);
    assert_eq!(code, 0, "default jet run failed: {stderr}");
    assert_eq!(stdout, "visible\n");

    let (code, stdout, stderr) = run_forced_interpreter(name, "main.jet", &files);
    assert_eq!(code, 0, "forced interpreter failed: {stderr}");
    assert_eq!(stdout, "visible\n");
}

#[test]
fn perf_role_captures_budget_expression_and_spans() {
    let src = r#"module perf.release {
    budgets: [Budget.{
        name: "binary",
        scope: .Target("cli"),
        metric: .BinarySize,
        limit: .AtMost(2MiB),
    }]
}

"#;
    let items = parse_items(src);
    let Item::Module(module) = &items[0] else {
        panic!("expected perf role module");
    };
    assert_eq!(module.name, "perf.release");
    assert_eq!(&src[module.name_span.start..module.name_span.end], "perf.release");
    let contribution = &module.contributions[0];
    assert_eq!(contribution.namespace, Namespace::Perf);
    assert_eq!(contribution.path, "release");
    let jet::AST::ContribValue::Perf(perf) = &contribution.value else {
        panic!("expected typed perf role AST");
    };
    assert_eq!(&src[perf.budgets_span.start..perf.budgets_span.end], "budgets");
    assert_eq!(perf.budgets.len(), 1);
    assert_eq!(perf.budgets[0].fields.len(), 4);
    let limit = perf.budgets[0]
        .fields
        .iter()
        .find(|field| field.name == "limit")
        .expect("typed limit field");
    let Expr::EnumLit { args, .. } = &limit.value else {
        panic!("typed limit enum");
    };
    let jet::AST::EnumLitArg::Positional(Expr::UnitLit { raw, suffix, suffix_span, .. }) = &args[0] else {
        panic!("typed unit literal");
    };
    assert_eq!((raw.as_str(), suffix.as_str()), ("2", "MiB"));
    assert_eq!(&src[suffix_span.start..suffix_span.end], "MiB");
    assert_eq!(
        &src[perf.span.start..perf.span.end],
        src[src.find("budgets").unwrap()..].trim_end()
    );
}

#[test]
fn perf_budget_sema_elaborates_defaults_and_field_spans() {
    let src = r#"module perf.release {
    budgets: [Budget.{
        name: "binary",
        metric: .BinarySize,
        limit: .AtMost(2MiB),
    }]
}

"#;
    let (tokens, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let program = jet::Parser::parse(&tokens).expect("parse");
    let specs = jet::Sema::collect_budget_specs(&program).expect("valid performance budget");
    assert_eq!(specs.len(), 1);
    let spec = &specs[0];
    assert_eq!(spec.role, "release");
    assert_eq!(spec.name, "binary");
    assert_eq!(spec.metric, "BinarySize");
    assert_eq!(spec.comparison, "Absolute");
    assert_eq!(spec.limit, "AtMost");
    assert_eq!(spec.enforcement, "Fail");
    assert_eq!(&src[spec.span.start..spec.span.end], "Budget.{\n        name: \"binary\",\n        metric: .BinarySize,\n        limit: .AtMost(2MiB),\n    }");
    for field in ["name", "metric", "limit"] {
        let span = spec.field_spans.get(field).expect("field span");
        assert_eq!(&src[span.start..span.end], field);
    }
}

fn collect_perf_specs(src: &str) -> Result<Vec<jet::Sema::BudgetSpec>, Vec<jet::Diagnostics::Diagnostic>> {
    let (tokens, lex_diags) = jet::Lexer::lex(src);
    assert!(lex_diags.is_empty(), "lex diagnostics: {lex_diags:?}");
    let program = jet::Parser::parse(&tokens).expect("parse");
    jet::Sema::collect_budget_specs(&program)
}

#[test]
fn perf_budget_sema_rejects_duplicate_identity_and_effective_overlap() {
    let duplicate_name = r#"module perf.release {
    budgets: [
        Budget.{ name: "binary", metric: .BinarySize, limit: .AtMost(2MiB) },
        Budget.{ name: "binary", metric: .ArtifactSize, limit: .AtMost(3MiB) },
    ]
}
"#;
    let diagnostics = collect_perf_specs(duplicate_name).expect_err("duplicate identity");
    assert_eq!(diagnostics.last().expect("diagnostic").code, "E2904");

    let overlap = r#"module perf.release {
    budgets: [
        Budget.{ name: "binary", metric: .BinarySize, limit: .AtMost(2MiB) },
        Budget.{ name: "shipping", metric: .BinarySize, limit: .AtMost(3MiB) },
    ]
}
"#;
    let diagnostics = collect_perf_specs(overlap).expect_err("effective overlap");
    assert_eq!(diagnostics.last().expect("diagnostic").code, "E2904");
}

#[test]
fn perf_budget_sema_accepts_disjoint_target_applicability() {
    let src = r#"module perf.release {
    budgets: [
        Budget.{
            name: "linux",
            metric: .BinarySize,
            limit: .AtMost(2MiB),
            applies: BudgetApplies.{
                targets: .Only([.Triple("x86_64-unknown-linux-gnu")]),
                profiles: .All,
            },
        },
        Budget.{
            name: "windows",
            metric: .BinarySize,
            limit: .AtMost(2MiB),
            applies: BudgetApplies.{
                targets: .Only([.Triple("x86_64-pc-windows-msvc")]),
                profiles: .All,
            },
        },
    ]
}

"#;
    assert_eq!(collect_perf_specs(src).expect("disjoint budgets").len(), 2);
}

#[test]
fn perf_budget_sema_resolves_service_scope_and_provider() {
    let src = r#"module env.dev {
    services: { api: { enable: true } }
}
module perf.release {
    budgets: [Budget.{
        name: "ready",
        scope: .Service("api"),
        metric: .ServiceReadiness,
        provider: .ServiceProbe("api"),
        comparison: .AbsoluteFrom("ci/linux-x64"),
        limit: .AtMost(2s),
        enforcement: .Warn,
    }]
}
"#;
    let specs = collect_perf_specs(src).expect("resolved service budget");
    assert_eq!(specs[0].scope, "Service(api)");
    assert_eq!(specs[0].provider, "ServiceProbe(api)");
    assert_eq!(specs[0].enforcement, "Warn");
}

#[test]
fn perf_budget_sema_rejects_statistical_budget_without_provider() {
    let src = r#"module perf.release {
    budgets: [Budget.{
        name: "startup",
        scope: .Target("cli"),
        metric: .StartupTime,
        comparison: .AbsoluteFrom("ci/linux-x64"),
        limit: .AtMost(500ms),
    }]
}
"#;
    let diagnostics = collect_perf_specs(src).expect_err("statistical provider is required");
    let diagnostic = diagnostics.last().expect("diagnostic");
    assert_eq!(diagnostic.code, "E2903");
    assert_eq!(diagnostic.what, "performance budget startup is not valid");
    assert!(diagnostic.why.contains("provider"), "{diagnostic:?}");
}

#[test]
fn perf_budget_sema_rejects_closed_metric_baseline_and_selector_shapes() {
    let bad_percentile = r#"module perf.release {
    budgets: [Budget.{
        name: "frame",
        scope: .Scene("menu"),
        metric: .FrameTime(.P42),
        provider: .SceneProbe("menu"),
        comparison: .AbsoluteFrom("ci/linux-x64"),
        limit: .AtMost(16ms),
    }]
}

"#;
    assert_eq!(
        collect_perf_specs(bad_percentile).expect_err("closed percentile")[0].code,
        "E2903"
    );

    let bad_baseline = r#"module perf.release {
    budgets: [Budget.{
        name: "startup",
        scope: .Target("cli"),
        metric: .StartupTime,
        provider: .BuildArtifact("cli"),
        comparison: .AbsoluteFrom("CI//linux"),
        limit: .AtMost(500ms),
    }]
}
"#;
    assert_eq!(
        collect_perf_specs(bad_baseline).expect_err("baseline grammar")[0].code,
        "E2903"
    );

    let wrong_axis = r#"module perf.release {
    budgets: [Budget.{
        name: "binary",
        metric: .BinarySize,
        limit: .AtMost(2MiB),
        applies: BudgetApplies.{ profiles: .Only([.Triple("x86_64-unknown-linux-gnu")]) },
    }]
}
"#;
    assert_eq!(
        collect_perf_specs(wrong_axis).expect_err("profile selector family")[0].code,
        "E2903"
    );
}

#[test]
fn perf_budget_sema_normalizes_rate_and_retains_canonical_facts() {
    let source = |baseline: &str, count: &str, seconds: i64| format!(r#"module env.dev {{
    services: {{ api: {{ enable: true }} }}
}}
module perf.release {{
    budgets: [Budget.{{
        name: "throughput",
        scope: .Service("api"),
        metric: .Throughput,
        provider: .ServiceProbe("api"),
        comparison: .AbsoluteFrom("{baseline}"),
        limit: .AtLeast(Rate.{{ count: {count}, per: {seconds}s }}),
        enforcement: .Warn,
    }}]
}}
"#);
    let first = collect_perf_specs(&source("ci/linux-x64", "100", 2)).expect("valid Rate").remove(0);
    assert_eq!(first.comparison, "AbsoluteFrom");
    assert_eq!(first.comparison_fact.baseline.as_deref(), Some("ci/linux-x64"));
    assert_eq!(first.limit_fact.quantity, jet::Sema::BudgetQuantity::Rate { numerator: 1, denominator_ns: 20_000_000 });
    assert_eq!(first.limit_fact.raw, jet::Sema::BudgetRawQuantity::Rate { count_digits: "100".into(), per_digits: "2".into(), per_suffix: "s".into() });
    let second = collect_perf_specs(&source("ci/macos-arm64", "75", 1)).expect("second valid Rate").remove(0);
    assert_ne!(first.comparison_fact, second.comparison_fact);
    assert_ne!(first.limit_fact, second.limit_fact);

    let hostile = collect_perf_specs(&source("ci/linux-x64", "000_100", 2))
        .expect("Rate with legal leading zeroes and separators")
        .remove(0);
    assert_eq!(hostile.limit_fact.quantity, jet::Sema::BudgetQuantity::Rate { numerator: 1, denominator_ns: 20_000_000 });
    assert_eq!(hostile.limit_fact.raw, jet::Sema::BudgetRawQuantity::Rate { count_digits: "000_100".into(), per_digits: "2".into(), per_suffix: "s".into() });
}

#[test]
fn perf_budget_sema_rejects_named_and_extra_limit_arguments() {
    let cases = [
        ("BinarySize", "", "", "Absolute", "AtMost", "2MiB"),
        ("StartupTime", "scope: .Service(\"api\"),", "provider: .ServiceProbe(\"api\"),", "RelativeTo(\"ci/linux-x64\")", "RegressionAtMost", "3pct"),
        ("Throughput", "scope: .Service(\"api\"),", "provider: .ServiceProbe(\"api\"),", "RelativeTo(\"ci/linux-x64\")", "ImprovementAtLeast", "3pct"),
        ("Throughput", "scope: .Service(\"api\"),", "provider: .ServiceProbe(\"api\"),", "AbsoluteFrom(\"ci/linux-x64\")", "AtLeast", "Rate.{ count: 100, per: 1s }"),
    ];
    for (metric, scope, provider, comparison, constructor, value) in cases {
        for limit_expr in [format!(".{constructor}.{{ value: {value} }}"), format!(".{constructor}({value}, {value})")] {
            let src = format!(r#"module env.dev {{ services: {{ api: {{ enable: true }} }} }}
module perf.release {{
    budgets: [Budget.{{
        name: "hostile",
        {scope}
        metric: .{metric},
        {provider}
        comparison: .{comparison},
        limit: {limit_expr},
        enforcement: .Fail,
    }}]
}}
"#);
            let diagnostics = collect_perf_specs(&src).expect_err("limit constructor arity must reject");
            assert_eq!(diagnostics[0].code, "E2903", "{limit_expr}");
            assert!(diagnostics[0].why.contains("exactly one positional"), "{:?}", diagnostics[0]);
        }
    }
}

#[test]
fn parses_module_shell_with_contribution() {
    let src = r#"
module dev {
    env.dev: Env.{
        prompt: "wordstats",
    }
}
"#;
    let items = parse_items(src);
    assert_eq!(items.len(), 1);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item, got {:?}", items[0]);
    };
    assert_eq!(m.name, "dev");
    assert!(m.is_auto_discovered());
    assert_eq!(m.contributions.len(), 1);
    let Contribution {
        namespace, path, ..
    } = &m.contributions[0];
    assert_eq!(*namespace, Namespace::Env);
    assert_eq!(path, "dev");
}

#[test]
fn parses_nested_sources_and_imports() {
    // U8: `sources:` / `imports:` nest inside the module body, as siblings of
    // the `env.dev: Env.{ … }` contribution (owner, 2026-06-16; amends U4).
    let src = r#"
module dev {
    sources: { default: NixOS/nixpkgs/nixos-24.05@github }
    imports: find("./modules")
    env.dev: Env.{
        prompt: "wordstats",
    }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item, got {:?}", items[0]);
    };

    // One named source; its `target@provider` ref is recovered by slicing the
    // source at the recorded span (the parser is token-based, the ref is not a
    // single token — modeval validates it via classify_provider_ref).
    assert_eq!(m.sources.len(), 1);
    assert_eq!(m.sources[0].name, "default");
    assert_eq!(
        &src[m.sources[0].ref_span.start..m.sources[0].ref_span.end],
        "NixOS/nixpkgs/nixos-24.05@github"
    );

    // One import: `find("./modules")`, parsed as an ordinary call expression.
    assert_eq!(m.imports.len(), 1);
    let Expr::Call(Call { name, args, .. }) = &m.imports[0] else {
        panic!("expected a call expression, got {:?}", m.imports[0]);
    };
    assert_eq!(name, "find");
    assert_eq!(args.len(), 1);
    let Expr::Str(parts, _) = &args[0].expr else {
        panic!("expected a string argument, got {:?}", args[0].expr);
    };
    let [StrPart::Lit(path)] = parts.as_slice() else {
        panic!("expected a single literal string part, got {parts:?}");
    };
    assert_eq!(path, "./modules");

    // The typed contribution still parses alongside the new fields.
    assert_eq!(m.contributions.len(), 1);
    assert_eq!(m.contributions[0].namespace, Namespace::Env);
    assert_eq!(m.contributions[0].path, "dev");
}

#[test]
fn module_without_sources_or_imports_has_empty_fields() {
    let src = r#"
module dev {
    env.dev: Env.{ prompt: "x" }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item");
    };
    assert!(m.sources.is_empty());
    assert!(m.imports.is_empty());
    assert_eq!(m.contributions.len(), 1);
}

#[test]
fn leading_underscore_opts_module_out_of_auto_discovery() {
    let src = r#"
module _gaming {
    system.gaming: System.{
        target: linux.x64,
    }
}
"#;
    let items = parse_items(src);
    let Item::Module(m) = &items[0] else {
        panic!("expected a module item");
    };
    assert_eq!(m.name, "_gaming");
    assert!(!m.is_auto_discovered());
    assert_eq!(m.contributions[0].namespace, Namespace::System);
}

#[test]
fn many_modules_per_file() {
    let src = r#"
module laptop {
    system.laptop: System.{ target: linux.x64 }
}
module installer {
    image.installer: Image.{ from: system.laptop, target: linux.arm64 }
}
"#;
    let items = parse_items(src);
    assert_eq!(items.len(), 2);
    let (Item::Module(a), Item::Module(b)) = (&items[0], &items[1]) else {
        panic!("expected two module items");
    };
    assert_eq!(a.name, "laptop");
    assert_eq!(a.contributions[0].namespace, Namespace::System);
    assert_eq!(b.name, "installer");
    assert_eq!(b.contributions[0].namespace, Namespace::Image);
}

#[test]
fn bodyless_public_module_is_visible_through_selective_import() {
    assert_bodyless_module_runs("bodyless_public_module", "pub");
}

#[test]
fn bodyless_package_module_is_visible_inside_package() {
    assert_bodyless_module_runs("bodyless_package_module", "pub(package)");
}

#[test]
fn bodyless_private_module_stays_hidden_outside_owner_file() {
    let scratch = common::Scratch::new("bodyless_private_module");
    write_files(
        &scratch.path,
        &[
            (
                "main.jet",
                "module lib;\nuse lib.[helper]\nfn run() { print(helper.value()) }\n",
            ),
            ("lib.jet", "module helper;\n"),
            (
                "helper.jet",
                "pub fn value() => String { return \"private\" }\n",
            ),
        ],
    );

    let entry = scratch.join("main.jet");
    let diagnostics = jet::check_with_path(entry.to_str().unwrap());
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0609"),
        "private bodyless module unexpectedly visible: {diagnostics:?}"
    );
}

#[test]
fn bodyless_package_module_stays_hidden_across_packages() {
    let scratch = common::Scratch::new("bodyless_package_module_external");
    write_files(
        &scratch.path,
        &[
            (
                "app/package.jet",
                "name: \"app\"\nversion: \"0.1.0\"\ndeps: .{ dep: ../dep }\n",
            ),
            (
                "app/main.jet",
                "use dep\nuse dep.[helper]\nfn run() { print(helper.value()) }\n",
            ),
            ("dep/package.jet", "name: \"dep\"\nversion: \"0.1.0\"\n"),
            ("dep/dep.jet", "pub(package) module helper;\n"),
            (
                "dep/helper.jet",
                "pub fn value() => String { return \"hidden\" }\n",
            ),
        ],
    );

    let entry = scratch.join("app/main.jet");
    let diagnostics = jet::check_with_path(entry.to_str().unwrap());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code.as_str(), "E0609" | "E0611")),
        "package-private bodyless module unexpectedly crossed package boundary: {diagnostics:?}"
    );
}
