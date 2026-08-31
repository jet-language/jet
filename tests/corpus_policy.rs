//! Focused proof for the semantic canonical-corpus adapter.

mod common;

use common::corpus_policy::{CorpusPolicy, RuleScopeRow, SemanticViolation, SourceRole};

#[test]
fn checked_manifest_has_all_audited_findings_and_provenance() {
    let policy = CorpusPolicy::load().expect("checked-in corpus manifest is valid");
    assert_eq!(policy.manifest().findings.len(), 38);
    assert_eq!(policy.manifest().producers.len(), 26);
    assert!(policy
        .manifest()
        .artifacts
        .iter()
        .any(|artifact| artifact.selector == "family:lua-binding"));
    let lua = policy
        .manifest()
        .producers
        .iter()
        .find(|producer| producer.selector.contains("LuaBind.rs"))
        .expect("Lua renderer is inventoried");
    assert_eq!(
        lua.protocol,
        "override:lua-raw-json:differs-from-ordinary-envelope"
    );
}

#[test]
fn inventory_is_manifest_scoped() {
    let policy = CorpusPolicy::load().unwrap();
    let inventory = policy.inventory().unwrap();
    assert!(inventory
        .files
        .iter()
        .any(|entry| entry.path == "docs/first-hour.md"));
    assert_eq!(
        inventory.provenance.len(),
        inventory.artifacts.len(),
        "every artifact must retain producer provenance"
    );
    assert!(!inventory
        .files
        .iter()
        .any(|entry| entry.path == "scratch-parser-call.jet"));
}

#[test]
fn cli_recipe_inventory_is_manifest_owned_and_ast_checked() {
    let policy = CorpusPolicy::load().unwrap();
    policy
        .check_cli_recipe_inventory()
        .expect("the ratified CLI recipe inventory must stay checked");
    let source_profile = |selector: &str| {
        policy
            .manifest()
            .sources
            .iter()
            .find(|source| source.selector == selector)
            .map(|source| source.profile.as_str())
            .unwrap_or_else(|| panic!("missing CLI recipe source row: {selector}"))
    };
    for selector in [
        "file:adoption/fixtures/clean-project/run.jet",
        "file:examples/features/basics/first_hour.jet",
        "file:examples/features/basics/onboarding/run.jet",
        "file:docs/first-hour.md",
        "root:gauntlet/entries",
        "root:tests/agent_workloads/adapters",
        "root:tests/compiled_workloads/adapters/jet",
        "file:dogfood/jetpack/src/cli/main.jet",
    ] {
        assert!(
            matches!(source_profile(selector), "typed-cli" | "typed-cli-doc"),
            "canonical CLI source is not typed: {selector}"
        );
    }
    for selector in [
        "file:examples/features/io/args_spec.jet",
        "file:examples/features/io/args_audit.jet",
    ] {
        assert_eq!(source_profile(selector), "builder-cli", "builder boundary drifted: {selector}");
    }
    for selector in [
        "file:examples/features/io/cli_args.jet",
        "file:examples/features/io/watcher.jet",
        "file:gauntlet/entries/taskfile-cli/jet/main.jet",
    ] {
        assert_eq!(source_profile(selector), "raw-cli", "raw boundary drifted: {selector}");
    }
    assert_eq!(
        source_profile("file:dogfood/tower/run.jet"),
        "non-cli",
        "domain-policy dogfood is not a CLI recipe"
    );
    for selector in [
        "producer:Source/CmdCompile.rs#run_new.native_run_src",
        "producer:crates/jet-pkg-model/src/Package/Convert.rs#new_template",
    ] {
        assert!(
            policy
                .manifest()
                .producers
                .iter()
                .any(|producer| producer.selector == selector),
            "missing CLI starter producer row: {selector}"
        );
    }
}

#[test]
fn fixture_roles_and_approved_builder_boundary_are_explicit() {
    let policy = CorpusPolicy::load().unwrap();
    policy
        .check_fixture_inventory()
        .expect("fixture roots and profiles must agree");

    let fixture = policy
        .evaluate_source(
            "tests/ui/args_spec_bad_parse_arity.jet",
            "fn run() { process.argv() }",
        )
        .unwrap();
    assert!(fixture.is_empty(), "negative fixtures are role-gated: {fixture:?}");

    let builder = policy
        .evaluate_source(
            "dogfood/jetpack/src/cli/main.jet",
            "fn run() { args.spec(); process.argv() }",
        )
        .unwrap();
    assert!(builder.iter().all(|violation| {
        violation.rule != "raw-cli-fixed-shape" && violation.rule != "raw-cli-builder-shape"
    }));
}

#[test]
fn inventory_rejects_new_source_or_producer_without_a_row() {
    let policy = CorpusPolicy::load().unwrap();
    let error = policy
        .audit_inventory(&["unclassified/new-maintained-program.jet"], &[])
        .expect_err("a synthetic source without a classification must fail");
    assert!(error.contains("unclassified eligible source"));
    assert!(error.contains("rule=corpus-inventory"));
    assert!(error.contains("site=inventory:source@0..0"));
    assert!(error.contains("replacement="));

    let error = policy
        .audit_inventory(&["examples/features/basics/hello.jet"], &["producer:new.rs#render"])
        .expect_err("a synthetic producer without provenance must fail");
    assert!(error.contains("unclassified generated-source producer"));
}

#[test]
fn manifest_rejects_a_scope_without_source_coverage() {
    let policy = CorpusPolicy::load().unwrap();
    let mut manifest = policy.manifest().clone();
    manifest.scopes.push(RuleScopeRow {
        rule: "entry-implicit".to_string(),
        selector: "file:unclassified/entry.jet".to_string(),
        reason: "synthetic uncovered scope".to_string(),
    });
    let error = manifest
        .validate()
        .expect_err("a rule scope must be covered by a source row");
    assert!(error.contains("unclassified source selector"), "{error}");
}

#[test]
fn manifest_rejects_a_finding_without_an_executable_guard() {
    let policy = CorpusPolicy::load().unwrap();
    let mut manifest = policy.manifest().clone();
    manifest.scopes.retain(|scope| scope.rule != "entry-implicit");
    let error = manifest
        .validate()
        .expect_err("every finding must retain a corpus or domain guard");
    assert!(error.contains("entry-implicit has no executable corpus or domain guard"), "{error}");
}

#[test]
fn exception_matching_is_occurrence_scoped() {
    let policy = CorpusPolicy::load().unwrap();
    let site = "fn:run@0..0".to_string();
    let candidate = SemanticViolation::new(
        "tests/ui/E-CALL-VALUE_call_result_not_called.jet",
        "entry-implicit",
        site,
    );
    policy
        .validate_occurrences(
            "tests/ui/E-CALL-VALUE_call_result_not_called.jet",
            std::slice::from_ref(&candidate),
        )
        .expect("the recorded single occurrence is accepted");
    let duplicate = vec![candidate.clone(), candidate];
    let error = policy
        .validate_occurrences(
            "tests/ui/E-CALL-VALUE_call_result_not_called.jet",
            &duplicate,
        )
        .expect_err("a second occurrence cannot reuse one exception");
    assert!(error.contains("expected 1"));
}

#[test]
fn exceptions_are_limited_to_expert_or_negative_sources() {
    let policy = CorpusPolicy::load().unwrap();
    for exception in &policy.manifest().exceptions {
        let path = exception
            .selector
            .strip_prefix("file:")
            .expect("exceptions use exact file selectors");
        let source = policy
            .manifest()
            .sources
            .iter()
            .filter(|source| {
                let selector_path = source
                    .selector
                    .strip_prefix("file:")
                    .or_else(|| source.selector.strip_prefix("root:"))
                    .expect("manifest source selector has a path");
                source.selector.starts_with("file:")
                    && selector_path == path
                    || source.selector.starts_with("root:")
                        && (path == selector_path
                            || path
                                .strip_prefix(selector_path)
                                .is_some_and(|rest| rest.starts_with('/')))
            })
            .max_by_key(|source| {
                source
                    .selector
                    .strip_prefix("file:")
                    .map_or_else(|| source.selector.len(), str::len)
            })
            .expect("exception source is classified");
        assert!(
            exception.site.starts_with("allow:")
                || matches!(source.role, SourceRole::ExpertLesson | SourceRole::NegativeDiagnostic),
            "{} is classified as {:?} without an allowance site",
            exception.selector,
            source.role
        );
    }
}

#[test]
fn semantic_rules_use_ast_shapes_not_source_text() {
    let policy = CorpusPolicy::load().unwrap();
    let positive = policy
        .evaluate_source(
            "examples/features/basics/first_hour.jet",
            "fn run() { args :: process.argv() }",
        )
        .unwrap();
    assert!(positive.iter().any(|violation| violation.rule == "raw-cli-fixed-shape"));
    assert!(positive
        .iter()
        .all(|violation| violation.file == "examples/features/basics/first_hour.jet"));

    let literal = policy
        .evaluate_source(
            "examples/features/basics/first_hour.jet",
            "fn run() { print(\"process.argv()\") }",
        )
        .unwrap();
    assert!(!literal.iter().any(|violation| violation.rule == "raw-cli-fixed-shape"));
}

#[test]
fn cli_boundary_rules_cover_builder_views_and_repeated_process_reads() {
    let policy = CorpusPolicy::load().unwrap();
    let builder_view = policy
        .evaluate_source(
            "examples/features/basics/first_hour.jet",
            "fn run() { args :: process.argv().skip(1) }",
        )
        .unwrap();
    assert!(builder_view
        .iter()
        .any(|violation| violation.rule == "raw-cli-builder-shape"));

    let repeated = policy
        .evaluate_source(
            "examples/features/basics/first_hour.jet",
            "fn run() {
    first :: process.argv()
    second :: process.argv()
}",
        )
        .unwrap();
    assert!(repeated
        .iter()
        .any(|violation| violation.rule == "raw-cli-process-boundary"));

    let single = policy
        .evaluate_source(
            "examples/features/basics/first_hour.jet",
            "fn run() { args :: process.argv() }",
        )
        .unwrap();
    assert!(!single
        .iter()
        .any(|violation| violation.rule == "raw-cli-process-boundary"));
}

#[test]
fn maintained_guidance_allow_requires_an_occurrence_manifest_row() {
    let policy = CorpusPolicy::load().unwrap();
    let error = policy
        .evaluate_source(
            "examples/features/text/unreviewed_allow.jet",
            "fn run() { #allow(unit_scalar_rewrap) print(\"kept\") }",
        )
        .expect_err("maintained guidance allowance must be recorded");
    assert!(error.contains("occurrence-scoped manifest reason"), "{error}");
}

#[test]
fn card_2375_rules_reject_each_reintroduced_ceremony() {
    let policy = CorpusPolicy::load().unwrap();
    let cases = [
        (
            "examples/features/basics/hello.jet",
            r#"fn run() { print("hello") }"#,
            "entry-implicit",
        ),
        (
            "docs/first-hour.md",
            "jet build examples/features/basics/first_hour.jet",
            "first-hour-doc-recipe",
        ),
        (
            "examples/features/types/typed_literal_forms.jet",
            r#"fn run() { print("jet build examples/features/types/typed_literal_forms.jet") }"#,
            "first-hour-doc-recipe",
        ),
        (
            "examples/features/serde/reintroduced.jet",
            r#"#Codable
struct Sample {
    value: Int
}
fn run() {}"#,
            "codable-structural",
        ),
        (
            "examples/features/memory/reintroduced.jet",
            r#"fn run() {
    delay :: Duration.hours(2) ?? return Err("duration")
}"#,
            "duration-constant-safe",
        ),
        (
            "examples/features/basics/reintroduced_effect.jet",
            r#"use core.files as fs
fn run() -[FS, IO]> {
    path :: "input.txt"
    text :: fs.read(path)
    print(text)
}"#,
            "effect-row-inference",
        ),
        (
            "examples/features/concurrency/parallel_scan.jet",
            r#"use core.files as fs
fn run() {
    path :: "input.txt"
    text :: fs.read(~path)
}"#,
            "readonly-copy",
        ),
        (
            "examples/features/io/reintroduced.jet",
            r#"use core.files as fs
fn run() {
    path :: "input.txt"
    text :: fs.read(path) ?? return Err("read")
}"#,
            "error-identity",
        ),
        (
            "examples/features/concurrency/reintroduced.jet",
            r#"fn run() {
    task.group g {
        task.race {1, 2}
    }
}"#,
            "task-one-child",
        ),
        (
            "examples/features/concurrency/reintroduced.jet",
            r#"fn run() {
    task.group g {
        task.any {1, 2}
    }
}"#,
            "task-one-child",
        ),
        (
            "tests/agent_workloads/adapters/reintroduced.jet",
            r#"fn run() {
    line_number := 0
    loop item in items {
        line_number += 1
        seen :: line_number
    }
}"#,
            "indexed-sequence",
        ),
        (
            "gauntlet/entries/bulkrename/jet/main.jet",
            r#"fn run() {
    value :: Regex{"IMG"}
}"#,
            "regex-capture-recipe",
        ),
    ];

    for (path, source, rule) in cases {
        let violations = policy.evaluate_source(path, source).unwrap();
        let violation = violations
            .iter()
            .find(|violation| violation.rule == rule)
            .unwrap_or_else(|| panic!("{path} did not reject {rule}: {violations:?}"));
        let rendered = violation.to_string();
        assert!(rendered.contains(&format!("file={path}")), "{rendered}");
        assert!(rendered.contains(&format!("rule={rule}")), "{rendered}");
        assert!(rendered.contains("site="), "{rendered}");
        assert!(rendered.contains("why="), "{rendered}");
        assert!(rendered.contains("replacement="), "{rendered}");
    }
}

#[test]
fn generated_protocol_check_requires_one_decoder_for_response_protocols() {
    let policy = CorpusPolicy::load().unwrap();
    let violations = policy
        .evaluate_generated(
            "producer:crates/jet-pkg-model/src/LuaBind.rs#render_jet",
            "fn run() {}",
        )
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "bindgen-one-decoder");
    assert!(violations[0].to_string().contains("replacement="));

    let no_response = policy
        .evaluate_generated(
            "producer:crates/jet-pkg-model/src/CBind.rs#render_jet",
            "fn run() {}",
        )
        .unwrap();
    assert!(no_response.is_empty());
}

#[test]
fn every_response_binding_renderer_uses_the_shared_decoder_policy() {
    let policy = CorpusPolicy::load().unwrap();
    let response_producers = policy
        .manifest()
        .producers
        .iter()
        .filter(|producer| {
            producer.selector.contains("Bind.rs#render_jet")
                && producer.protocol != "no-response-protocol"
        })
        .collect::<Vec<_>>();
    assert_eq!(response_producers.len(), 7);

    let source = "fn decode_response(raw: String, code: Int) DataTree !Err -> { return Ok(DataTree.Null) }";
    for producer in response_producers {
        let violations = policy
            .evaluate_generated(&producer.selector, source)
            .unwrap_or_else(|error| panic!("{}: {error}", producer.selector));
        assert!(
            violations.is_empty(),
            "{} did not accept one shared response decoder: {violations:?}",
            producer.selector
        );
    }
}

#[test]
fn walk_policy_distinguishes_file_only_filters_from_directory_tree_walks() {
    let policy = CorpusPolicy::load().unwrap();
    let file_only = policy
        .evaluate_source(
            "dogfood/jetpack/tests/transcript.jet",
            "use core.files as fs
fn run() {
    entries :: fs.walk(root)
    loop entry in entries {
        if !entry.is_dir -> print(entry.path)
    }
}",
        )
        .unwrap();
    assert!(file_only
        .iter()
        .any(|violation| violation.rule == "walk-files-filter"));

    let unrelated_walk_and_filter = policy
        .evaluate_source(
            "dogfood/jetpack/tests/transcript.jet",
            "use core.files as fs
fn walk() {
    entries :: fs.walk(root)
}
fn filter(entry: Entry) {
    if !entry.is_dir -> print(entry.path)
}",
        )
        .unwrap();
    assert!(!unrelated_walk_and_filter
        .iter()
        .any(|violation| violation.rule == "walk-files-filter"));

    let tree = policy
        .evaluate_source(
            "dogfood/jetpack/tests/transcript.jet",
            "use core.files as fs
fn run() {
    entries :: fs.walk(root)
    loop entry in entries {
        if entry.is_dir -> print(entry.relative) else -> print(entry.path)
    }
}",
        )
        .unwrap();
    assert!(!tree
        .iter()
        .any(|violation| violation.rule == "walk-files-filter"));
}

#[test]
fn semantic_migration_rules_match_only_their_structural_shapes() {
    let policy = CorpusPolicy::load().unwrap();

    let duration = policy
        .evaluate_source(
            "examples/features/memory/reintroduced.jet",
            "fn run() { delay :: Duration.seconds(1.5) }",
        )
        .unwrap();
    assert!(duration
        .iter()
        .any(|violation| violation.rule == "duration-constant-safe"));
    let runtime_duration = policy
        .evaluate_source(
            "examples/features/memory/reintroduced.jet",
            "fn run(seconds: Float) { delay :: Duration.seconds(seconds) }",
        )
        .unwrap();
    assert!(!runtime_duration
        .iter()
        .any(|violation| violation.rule == "duration-constant-safe"));

    let unit = policy
        .evaluate_source(
            "examples/features/text/reintroduced.jet",
            "fn run() { value :: Meter.from_float(source.raw() * 2) }",
        )
        .unwrap();
    assert!(unit
        .iter()
        .any(|violation| violation.rule == "unit-scalar-rewrap"));
    let converted_unit = policy
        .evaluate_source(
            "examples/features/types/unit_family.jet",
            "fn subtotal(price: Usd, qty: Int) Usd -> { Usd.from_float(price.raw() * Float.from_int(qty)) }",
        )
        .unwrap();
    assert!(converted_unit
        .iter()
        .any(|violation| violation.rule == "unit-scalar-rewrap"));
    let helper_unit = policy
        .evaluate_source(
            "examples/features/text/reintroduced.jet",
            "fn run() { value :: Meter.from_float(source.raw() * calibrate(2.0)) }",
        )
        .unwrap();
    assert!(!helper_unit
        .iter()
        .any(|violation| violation.rule == "unit-scalar-rewrap"));

    let format = policy
        .evaluate_source(
            "gauntlet/entries/bulkrename/jet/main.jet",
            "fn run() { print(\"{value:Fixed(2)}\".replace(\",\", \"\")) }",
        )
        .unwrap();
    assert!(format
        .iter()
        .any(|violation| violation.rule == "plain-format-fact"));
    let grouped = policy
        .evaluate_source(
            "gauntlet/entries/bulkrename/jet/main.jet",
            "fn run() { print(\"{value:Grouped(2)}\".replace(\",\", \"\")) }",
        )
        .unwrap();
    assert!(!grouped
        .iter()
        .any(|violation| violation.rule == "plain-format-fact"));

    let directory = policy
        .evaluate_source(
            "site/generate.jet",
            "fn run() { files.create_dir(\"dist\") }",
        )
        .unwrap();
    assert!(directory
        .iter()
        .any(|violation| violation.rule == "dogfood-directory-setup"));
    let idempotent = policy
        .evaluate_source(
            "site/generate.jet",
            "fn run() { files.create_dir_all(\"dist\") }",
        )
        .unwrap();
    assert!(!idempotent
        .iter()
        .any(|violation| violation.rule == "dogfood-directory-setup"));

    let generic_policy = policy
        .evaluate_source(
            "dogfood/tower/run.jet",
            "fn duplicate_equal(left: DataTree, right: DataTree) Bool -> { return true }",
        )
        .unwrap();
    assert!(generic_policy
        .iter()
        .any(|violation| violation.rule == "datatree-domain-policy"));
    let named_policy = policy
        .evaluate_source(
            "dogfood/tower/run.jet",
            "fn javascript_truthy(value: DataTree) Bool -> { return true }",
        )
        .unwrap();
    assert!(!named_policy
        .iter()
        .any(|violation| violation.rule == "datatree-domain-policy"));

    let mut lower_ladder = String::from("\"text\"");
    for (from, to) in ('A'..='Z').zip('a'..='z') {
        lower_ladder.push_str(&format!(".replace(\"{from}\", \"{to}\")"));
    }
    let ladder = policy
        .evaluate_source(
            "dogfood/jetpack/src/store/path_laws.jet",
            &format!("fn run() {{ value :: {lower_ladder} }}"),
        )
        .unwrap();
    assert!(ladder
        .iter()
        .any(|violation| violation.rule == "dogfood-ascii"));
    let partial = policy
        .evaluate_source(
            "dogfood/jetpack/src/store/path_laws.jet",
            "fn run() { value :: \"text\".replace(\"A\", \"a\").replace(\"B\", \"b\") }",
        )
        .unwrap();
    assert!(!partial
        .iter()
        .any(|violation| violation.rule == "dogfood-ascii"));

    let unrelated_regex_recipe = policy
        .evaluate_source(
            "gauntlet/entries/bulkrename/jet/main.jet",
            "fn pattern() { value :: Regex{\"^x$\"} }
fn run(value: String) {
    matched :: re.match(Regex{\"x\"}, value)
    print(matched.group(1) ?? \"\")
}",
        )
        .unwrap();
    assert!(unrelated_regex_recipe
        .iter()
        .any(|violation| violation.rule == "regex-capture-recipe"));

    let docs_without_runner = policy
        .evaluate_source(
            "docs/first-hour.md",
            "```jet\n#CLI\nstruct Args { name: String }\n```\n```bash\njet build\n```",
        )
        .unwrap();
    assert!(docs_without_runner
        .iter()
        .any(|violation| violation.rule == "first-hour-doc-recipe"));
}

#[test]
fn crypto_digest_rule_uses_the_resolved_core_module() {
    let policy = CorpusPolicy::load().unwrap();
    let legacy = policy
        .evaluate_source(
            "examples/features/crypto/reintroduced.jet",
            "use core.crypto as crypto
fn run() { crypto.sha256_bytes(\"abc\".bytes()) }",
        )
        .unwrap();
    assert!(legacy
        .iter()
        .any(|violation| violation.rule == "crypto-naming-fact"));

    let canonical = policy
        .evaluate_source(
            "examples/features/crypto/reintroduced.jet",
            "use core.crypto as crypto
fn run() { crypto.sha256(\"abc\".bytes()).hex() }",
        )
        .unwrap();
    assert!(!canonical
        .iter()
        .any(|violation| violation.rule == "crypto-naming-fact"));

    let unrelated = policy
        .evaluate_source(
            "examples/features/crypto/reintroduced.jet",
            "fn run() { sha256_bytes(\"abc\".bytes()) }",
        )
        .unwrap();
    assert!(!unrelated
        .iter()
        .any(|violation| violation.rule == "crypto-naming-fact"));
}

#[test]
fn scoped_dogfood_and_workload_recipes_have_positive_and_negative_shapes() {
    let policy = CorpusPolicy::load().unwrap();

    let delimited = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/incident_report.jet",
            "fn run() { rows :: input.split(\"\\t\") }",
        )
        .unwrap();
    assert!(delimited
        .iter()
        .any(|violation| violation.rule == "delimited-reader-config"));
    let other_delimiter = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/incident_report.jet",
            "fn run() { rows :: input.split(\"|\") }",
        )
        .unwrap();
    assert!(!other_delimiter
        .iter()
        .any(|violation| violation.rule == "delimited-reader-config"));

    let http = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn run(req: HTTPRequest) { payload :: req.body(Json) }",
        )
        .unwrap();
    assert!(http
        .iter()
        .any(|violation| violation.rule == "http-wrapper-json"));
    assert!(http
        .iter()
        .any(|violation| violation.rule == "http-typed-json"));
    let bounded_http = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn run() { answer :: response.body().text(100) }",
        )
        .unwrap();
    assert!(bounded_http
        .iter()
        .any(|violation| violation.rule == "http-message-text"));
    let direct_http = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn run() { answer :: response.text() }",
        )
        .unwrap();
    assert!(!direct_http
        .iter()
        .any(|violation| violation.rule == "http-message-text"));
    let canonical_http = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn run(req: HTTPRequest) { payload :: req.json(Json) }",
        )
        .unwrap();
    assert!(!canonical_http
        .iter()
        .any(|violation| matches!(
            violation.rule.as_str(),
            "http-wrapper-json" | "http-typed-json"
        )));

    let unused_request = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn handle(req: HTTPRequest) HTTPResponse -> { return response }",
        )
        .unwrap();
    assert!(unused_request
        .iter()
        .any(|violation| violation.rule == "http-unused-request"));
    let used_request = policy
        .evaluate_source(
            "tests/agent_workloads/adapters/http_api.jet",
            "fn handle(req: HTTPRequest) HTTPResponse -> { return req.response() }",
        )
        .unwrap();
    assert!(!used_request
        .iter()
        .any(|violation| violation.rule == "http-unused-request"));

    let path = policy
        .evaluate_source(
            "dogfood/jetpack/src/store/ingest.jet",
            "fn run() { allowed :: candidate.starts_with(\"root/\") }",
        )
        .unwrap();
    assert!(path
        .iter()
        .any(|violation| violation.rule == "dogfood-path-containment"));
    let path_without_prefix = policy
        .evaluate_source(
            "dogfood/jetpack/src/store/ingest.jet",
            "fn run() { allowed :: candidate == root }",
        )
        .unwrap();
    assert!(!path_without_prefix
        .iter()
        .any(|violation| violation.rule == "dogfood-path-containment"));

    let json = policy
        .evaluate_source(
            "dogfood/jetpack/src/cli/main.jet",
            r#"fn run() { wire :: "{{\"key\": {value}}}" }"#,
        )
        .unwrap();
    assert!(json
        .iter()
        .any(|violation| violation.rule == "dogfood-json"));
    let typed_json = policy
        .evaluate_source(
            "dogfood/jetpack/src/cli/main.jet",
            "fn run() { wire :: json.to_string(value) }",
        )
        .unwrap();
    assert!(!typed_json
        .iter()
        .any(|violation| violation.rule == "dogfood-json"));

    let query = policy
        .evaluate_source(
            "dogfood/tower/run.jet",
            "fn run() { fields :: query.split(\"&\") }",
        )
        .unwrap();
    assert!(query
        .iter()
        .any(|violation| violation.rule == "dogfood-url-query"));
    let query_api = policy
        .evaluate_source(
            "dogfood/tower/run.jet",
            "fn run() { fields :: url.query() }",
        )
        .unwrap();
    assert!(!query_api
        .iter()
        .any(|violation| violation.rule == "dogfood-url-query"));

    let equality = policy
        .evaluate_source(
            "dogfood/jetpack/src/plan/plan.jet",
            "fn run() { same_strings(left, right) }",
        )
        .unwrap();
    assert!(equality
        .iter()
        .any(|violation| violation.rule == "dogfood-list-equality"));
    let direct_equality = policy
        .evaluate_source(
            "dogfood/jetpack/src/plan/plan.jet",
            "fn run() { left == right }",
        )
        .unwrap();
    assert!(!direct_equality
        .iter()
        .any(|violation| violation.rule == "dogfood-list-equality"));
}

#[test]
fn generated_decoder_policy_requires_the_canonical_decoder_symbol() {
    let policy = CorpusPolicy::load().unwrap();
    let violations = policy
        .evaluate_generated(
            "producer:crates/jet-pkg-model/src/LuaBind.rs#render_jet",
            "fn decode_other(raw: String, code: Int) DataTree !Err -> { return Ok(DataTree.Null) }",
        )
        .unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "bindgen-one-decoder");
}

#[test]
fn generated_protocol_override_requires_a_distinct_envelope_shape() {
    let policy = CorpusPolicy::load().unwrap();
    let raw_json = policy
        .evaluate_generated(
            "producer:crates/jet-pkg-model/src/LuaBind.rs#render_jet",
            "fn decode_response(raw: String, code: Int) DataTree !Err -> { return Ok(json.parse(raw)) }",
        )
        .unwrap();
    assert!(raw_json.is_empty(), "raw JSON override is distinct: {raw_json:?}");

    let ordinary = policy
        .evaluate_generated(
            "producer:crates/jet-pkg-model/src/LuaBind.rs#render_jet",
            "fn decode_response(raw: String, code: Int) DataTree !Err -> {
    response :: json.parse(raw)
    ok :: response.field(\"ok\")
    return Ok(response)
}",
        )
        .unwrap();
    assert!(ordinary
        .iter()
        .any(|violation| violation.rule == "generated-protocol-override"));
}
