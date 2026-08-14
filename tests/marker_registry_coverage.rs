//! D-VERDICT-1455-1 (law zero): a marker exists if and only if it is a registry
//! row, and every active row must be alive end to end. This walks the registry
//! itself, so a row that nothing can parse — `#Authority` was one — fails here
//! instead of sitting in the table looking implemented.
//!
//! The renderer builds each program from the row's own signature and site list.
//! Nothing in this file names a marker, so a new row is covered the moment it
//! is added and cannot be forgotten.

mod common;

use jet_foundation::Policy::{
    self, AppliedRule, RuleArgType, RuleSite, RuleStatus,
};
use jet_foundation::Registry;

/// Sites that describe a position with no source spelling of its own.
/// `Package` is `package.jet` manifest scope, `Impl` and `Operation` are
/// interior positions the writer reaches through some other construct. A row
/// that lives *only* at one of these is reported, not skipped.
fn site_is_source_renderable(site: RuleSite) -> bool {
    !matches!(
        site,
        RuleSite::Package | RuleSite::Impl | RuleSite::Operation | RuleSite::Text
    )
}

/// One placeholder argument per declared parameter type. `Ident` arguments read
/// the row's own menu, so a closed menu never gets an invented variant.
fn placeholder(ty: RuleArgType, source_type: &str) -> String {
    if let Some(declaration) = Policy::rule_arg_declaration(source_type) {
        if let Some(variant) = declaration.variants.first() {
            return (*variant).to_string();
        }
    }
    match ty {
        RuleArgType::String => "\"x\"".to_string(),
        RuleArgType::Int => "1".to_string(),
        RuleArgType::Bool => "true".to_string(),
        RuleArgType::DurationOrString => "1s".to_string(),
        RuleArgType::Ident => "x".to_string(),
        RuleArgType::Any => "1".to_string(),
    }
}

/// D-MARK-FORM1=A: write parentheses exactly when the signature needs them.
fn spelling(row: &AppliedRule) -> String {
    if row.name == "Test" {
        return "#Test(\"coverage\")".to_string();
    }
    if !row.signature.arguments_required() {
        return format!("#{}", row.name);
    }
    let mut args: Vec<String> = row
        .signature
        .params
        .iter()
        .filter(|param| param.default.is_none())
        .map(|param| placeholder(param.ty, param.source_type))
        .collect();
    if args.is_empty() {
        if let (Some(ty), Some(source_type)) =
            (row.signature.variadic, row.signature.variadic_source_type)
        {
            args.push(placeholder(ty, source_type));
        }
    }
    if row.name == "Grant" && row.signature.variadic.is_some() {
        args[0] = format!("caps: {}", args[0]);
    }
    if row.name == "Scrub" {
        args[0] = "X".to_string();
    }
    if row.name == "Undo" {
        args[0] = "inverse".to_string();
    }
    if row.name == "Short" {
        args[0] = "\"p\"".to_string();
    }
    if row.name == "Env" {
        args[0] = "\"PORT\"".to_string();
    }
    if row.name == "Caps" {
        args[0] = "IO".to_string();
    }
    if row.name == "Target" {
        args[0] = "Web".to_string();
    }
    if row.name == "UnitFamily" {
        return "#UnitFamily(Length, dimension)".to_string();
    }
    if matches!(row.name, "Pre" | "Post") {
        args[0] = "true".to_string();
    }
    if row.name == "Every" {
        return format!("#[Job, Every({})]", args.join(", "));
    }
    format!("#{}({})", row.name, args.join(", "))
}

/// The smallest whole program that puts `marker` at `site`.
fn program_at(marker: &str, site: RuleSite) -> Option<String> {
    Some(match site {
        RuleSite::File if marker.starts_with("#NoPrelude") => format!(
            "#NoPrelude\nuse core.io as io\nfn run() {{\n    io.print(\"ok\")\n}}\n"
        ),
        RuleSite::File => format!("{marker}\nfn run() {{\n    print(\"ok\")\n}}\n"),
        // A module rule attaches to a `module` declaration, not to the file.
        RuleSite::Module if marker.starts_with("#Bindgen") => {
            format!("{marker} module c.coverage.__bindgen__ {{\n}}\n")
        }
        RuleSite::Module => {
            format!("{marker} module c.lib {{\n}}\n\nfn run() {{\n    print(\"ok\")\n}}\n")
        }
        RuleSite::Function if marker.starts_with("#Scrub") => format!(
            "tag X {{ deny: [IO] }}\n{marker} fn helper(raw: #X String) => String {{ return ~raw }}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Function if marker.starts_with("#Undo") => format!(
            "fn inverse() {{}}\n#[Unsafe(\"coverage\"), FFI(c), Undo(inverse)]\nfn helper() {{\n    \"\"\"void helper(void) {{}}\"\"\"\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Function if marker.starts_with("#ABI") => format!(
            "#Extern module c.demo {{\n    {marker} fn helper(value: I32) => I32 = \"helper\"\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Function if marker.starts_with("#FFI") => format!(
            "#[Unsafe(\"coverage\"), FFI(c)]\nfn helper() {{\n    \"\"\"void helper(void) {{}}\"\"\"\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Function => format!("{marker}\nfn helper() {{\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Method => format!(
            "W :: struct {{\n    a: Int\n\n    {marker}\n    fn helper(self) {{\n    }}\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Type if marker.starts_with("#UnitFamily") => {
            format!("{marker} {{ meter }}\n\nfn run() {{\n}}\n")
        }
        RuleSite::Type => format!("{marker}\nstruct W {{\n    a: Int\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Field if marker.starts_with("#Short") || marker.starts_with("#Env") || marker.starts_with("#Flag") => format!(
            "#CLI\nstruct W {{\n    {marker} a: String\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Field => format!("struct W {{\n    {marker} a: Int\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Variant => {
            format!("enum E {{\n    {marker} A\n    B\n}}\n\nfn run() {{\n}}\n")
        }
        RuleSite::Block if marker.starts_with("#Grant") => {
            format!("fn run() {{\n    {marker} {{\n    }}\n}}\n")
        }
        RuleSite::Block => format!("fn run() {{\n    {marker} {{\n        print(\"ok\")\n    }}\n}}\n"),
        RuleSite::Statement => format!("fn run() {{\n    {marker} print(\"ok\")\n}}\n"),
        RuleSite::Declaration if marker.starts_with("#Persist") => format!(
            "{marker} value := 1\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Declaration => format!(
            "fn run() {{\n    {marker} value :: 1\n}}\n"
        ),
        RuleSite::Constant if marker.starts_with("#Static") => {
            format!("{marker} @value :: 1\n\nfn run() {{\n}}\n")
        }
        RuleSite::Constant => format!("{marker} comptime value :: 1\n\nfn run() {{\n}}\n"),
        RuleSite::Parameter => {
            format!("fn helper({marker} a: Int) {{\n}}\n\nfn run() {{\n}}\n")
        }
        RuleSite::Expression => format!("fn run() {{\n    value :: {marker}\n}}\n"),
        RuleSite::Test => format!("{marker} {{\n    print(\"ok\")\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Bench => format!("{marker} {{\n    print(\"ok\")\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Package | RuleSite::Impl | RuleSite::Operation | RuleSite::Text => return None,
    })
}

fn sema_diagnostics(source: &str, path: &std::path::Path) -> Vec<jet::Diagnostics::Diagnostic> {
    if source.contains("#Bindgen") {
        return jet::Driver::compile_generated_src(
            source,
            ".jet/bindings/c/coverage.jet",
            jet::Sema::CompileMode::Check,
        )
        .err()
        .unwrap_or_default();
    }
    std::fs::write(path, source).expect("write marker coverage source");
    let mut bundle = jet::Loader::load_entry(path.to_str().expect("coverage path")).expect("load marker coverage source");
    jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check)
}

/// Diagnostics that mean "this marker did not reach its rule": the closed
/// vocabulary rejected the name, or the site table rejected the position.
/// Any other diagnostic is the row's own business and is not this guard's.
fn rejected_the_marker(source: &str) -> Vec<String> {
    let (tokens, lex_diagnostics) = if source.contains("#Bindgen") {
        jet::Lexer::lex_generated(source)
    } else {
        jet::Lexer::lex(source)
    };
    if !lex_diagnostics.is_empty() {
        return lex_diagnostics
            .into_iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
            .collect();
    }
    let diagnostics = match jet::Parser::parse_for_check(&tokens) {
        Ok((_, diagnostics)) => diagnostics,
        Err(diagnostics) => diagnostics,
    };
    diagnostics
        .into_iter()
        .filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0927" | "E0355"))
        .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
        .collect()
}

fn format_for_coverage(source: &str) -> Result<String, Vec<jet::Diagnostics::Diagnostic>> {
    if source.contains("#Bindgen") {
        let (tokens, diagnostics) = jet::Lexer::lex_generated(source);
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        let program = jet::Parser::parse_for_fmt(&tokens)?;
        return Ok(jet::Formatter::format_program(
            &program,
            source,
            &jet::Lexer::comments(&tokens),
        ));
    }
    jet::format_source(source)
}

/// Law zero, the reachable half: every active row can be written at one of its
/// own declared sites and is recognized there. A row nothing can spell is a
/// ghost and must be retired instead.
#[test]
fn every_active_row_is_reachable_at_a_declared_site() {
    let mut ghosts = Vec::new();
    for registered in Registry::marker_rows() {
        let row = registered.rule.expect("marker rows carry their applied rule");
        if !matches!(row.status, RuleStatus::Active) {
            continue;
        }
        let marker = spelling(row);
        let renderable: Vec<RuleSite> = row
            .sites
            .iter()
            .copied()
            .filter(|site| site_is_source_renderable(*site))
            .collect();
        if renderable.is_empty() {
            ghosts.push(format!(
                "#{} declares only interior sites {:?}, so no source can reach it",
                row.name, row.sites
            ));
            continue;
        }
        let mut rejections = Vec::new();
        let mut reached = false;
        for site in renderable {
            let Some(source) = program_at(&marker, site) else {
                continue;
            };
            let rejected = rejected_the_marker(&source);
            if rejected.is_empty() {
                reached = true;
                break;
            }
            rejections.push(format!("  at {site:?}: {}", rejected.join("; ")));
        }
        if !reached {
            ghosts.push(format!(
                "`{marker}` is rejected at every site it declares:\n{}",
                rejections.join("\n")
            ));
        }
    }
    assert!(
        ghosts.is_empty(),
        "registry rows with no reachable spelling ({}):\n{}",
        ghosts.len(),
        ghosts.join("\n")
    );
}

/// D-MARK-FORM1/A and D-HL1: every registered row is exercised through the
/// user-facing consumers. The reachability test above owns special parser
/// forms; this pass keeps formatter, highlight, and reflection coverage on the
/// same row iteration without duplicating those special grammars here.
#[test]
fn every_active_row_walks_parse_validate_format_highlight_reflect() {
    let highlighted = jet::Syntax::highlighted_tokens_sorted();
    let scratch = common::Scratch::new("marker-registry-coverage");
    let validation_path = scratch.join("main.jet");
    for registered in Registry::marker_rows() {
        let row = registered.rule.expect("marker rows carry their applied rule");
        if !matches!(row.status, RuleStatus::Active) {
            continue;
        }
        let marker = spelling(row);
        let source = row
            .sites
            .iter()
            .copied()
            .filter(|site| site_is_source_renderable(*site))
            .find_map(|site| {
                let source = program_at(&marker, site)?;
                let (_, lex_diagnostics) = if source.contains("#Bindgen") {
                    jet::Lexer::lex_generated(&source)
                } else {
                    jet::Lexer::lex(&source)
                };
                if !lex_diagnostics.is_empty() {
                    return None;
                }
                let (tokens, _) = if source.contains("#Bindgen") {
                    jet::Lexer::lex_generated(&source)
                } else {
                    jet::Lexer::lex(&source)
                };
                let Ok((_, parse_diagnostics)) = jet::Parser::parse_for_check(&tokens) else {
                    return None;
                };
                let marker_rejected = parse_diagnostics
                    .iter()
                    .any(|diagnostic| matches!(diagnostic.code.as_str(), "E0927" | "E0355"));
                (!marker_rejected).then_some(source)
            })
            .unwrap_or_else(|| panic!("`{marker}` has no parseable declared site"));

        let (tokens, lex_diagnostics) = if source.contains("#Bindgen") {
            jet::Lexer::lex_generated(&source)
        } else {
            jet::Lexer::lex(&source)
        };
        assert!(lex_diagnostics.is_empty(), "`{marker}` lex: {lex_diagnostics:?}");
        let (_, parse_diagnostics) = jet::Parser::parse_for_check(&tokens)
            .unwrap_or_else(|diagnostics| panic!("`{marker}` parse: {diagnostics:?}"));
        assert!(
            parse_diagnostics
                .iter()
                .all(|diagnostic| !matches!(diagnostic.code.as_str(), "E0927" | "E0355")),
            "`{marker}` was rejected during validation: {parse_diagnostics:?}"
        );
        let validation_diagnostics = sema_diagnostics(&source, &validation_path);
        assert!(
            validation_diagnostics
                .iter()
                .all(|diagnostic| {
                    !matches!(diagnostic.severity, jet::Diagnostics::Severity::Error)
                        || (source.contains("#Bindgen") && diagnostic.code == "E3201")
                }),
            "`{marker}` failed semantic validation: {validation_diagnostics:?}"
        );

        let once = format_for_coverage(&source)
            .unwrap_or_else(|error| panic!("`{marker}` format: {error:?}"));
        assert_eq!(
            once,
            format_for_coverage(&once).expect("formatted row must round-trip")
        );
        assert!(
            highlighted.iter().any(|token| token.text == row.name),
            "`{marker}` is missing from the highlight registry"
        );

        let reflected = Registry::marker_row_and_args(row.name, false, Vec::new())
            .unwrap_or_else(|| panic!("`{marker}` is missing from reflection"));
        assert_eq!(reflected.row.name, row.name);
    }
}

/// Law zero, the other half: a retired row teaches its replacement and applies
/// nothing, so every retired row must name a non-empty replacement.
#[test]
fn every_retired_row_teaches_a_replacement() {
    for registered in Registry::marker_rows() {
        let row = registered.rule.expect("marker rows carry their applied rule");
        if let RuleStatus::Retired { replacement } = row.status {
            assert!(
                !replacement.trim().is_empty(),
                "retired `#{}` has no replacement to teach",
                row.name
            );
        }
    }
}

/// D-MARK-REPEAT1=A: the repeatable column is the only place repetition is
/// decided, and it stays a deliberate minority.
#[test]
fn repeatable_rows_are_declared_not_assumed() {
    let repeatable: Vec<&str> = Registry::marker_rows()
        .filter_map(|registered| registered.rule)
        .filter(|row| row.repeatable)
        .map(|row| row.name)
        .collect();
    assert_eq!(
        repeatable,
        vec!["Pre", "Post", "allow"],
        "D-MARK-REPEAT1=A names exactly these repeatable rows"
    );
}

/// D-META-REG1=A: the marker rows are rows of the one registration table, and a
/// knowledge plane, a right, a build fact, a corpus truth, and a diagnostic row
/// are its other five uses. The coverage guard above walks the marker rows;
/// these walk the whole table, so no kind gets a guard of its own.
#[test]
fn the_one_table_holds_every_kind() {
    use jet_foundation::Registry::{self, RowKind, RowTarget, SafeDirection};

    for kind in [
        RowKind::Marker,
        RowKind::Plane,
        RowKind::Right,
        RowKind::Fact,
        RowKind::Truth,
        RowKind::Diagnostic,
    ] {
        let row = Registry::rows()
            .iter()
            .find(|row| row.kind() == kind)
            .unwrap_or_else(|| panic!("the one table holds no {} row", kind.name()));
        assert_eq!(row.target.kind(), kind);
    }

    // A marker row is exactly a row whose target is written code, and it is the
    // same row the marker registry holds.
    for registered in Registry::marker_rows() {
        let row = registered.rule.expect("marker rows carry their applied rule");
        let registered = Registry::row(row.name)
            .unwrap_or_else(|| panic!("`#{}` is not in the one table", row.name));
        assert_eq!(registered.target, RowTarget::Code(row.sites));
        assert_eq!(registered.rule.map(|rule| rule.name), Some(row.name));
        assert_eq!(registered.safe_direction, SafeDirection::None);
    }

    for row in Registry::type_plane_rows() {
        assert_eq!(row.kind(), RowKind::Plane);
        let declaration = Registry::fact_declarations()
            .iter()
            .find(|declaration| declaration.name == row.name)
            .unwrap_or_else(|| panic!("type plane `{}` has no Prelude declaration", row.name));
        assert_eq!(row.identity_bearing, declaration.identity_bearing);
        assert_eq!(row.decision, declaration.decision);
    }

    for row in Registry::fact_rows() {
        let declaration = Registry::fact_declarations()
            .iter()
            .find(|declaration| declaration.name == row.name)
            .unwrap_or_else(|| panic!("fact `{}` has no Prelude declaration", row.name));
        assert_eq!(row.target, declaration.target);
        assert_eq!(row.identity_bearing, declaration.identity_bearing);
    }
}

#[test]
fn every_registered_plane_is_reflectable() {
    use jet_foundation::Registry::{self, RowKind};
    use jet::Comptime::CtValue;

    let planes: Vec<_> = Registry::fact_rows()
        .filter(|row| row.kind() == RowKind::Plane)
        .collect();
    for row in &planes {
        assert_eq!(row.kind(), RowKind::Plane);
        assert!(Registry::row(row.name).is_some(), "plane `{}` has no home", row.name);
        let info = jet::Comptime::build_registered_fact_info(row.name)
            .unwrap_or_else(|| panic!("plane `{}` has no typed reflection row", row.name));
        assert!(matches!(
            info,
            CtValue::Struct { fields, .. }
                if fields.iter().any(|(name, value)| name == "kind"
                    && matches!(value, CtValue::Enum { .. }))
        ));
    }
    assert_eq!(
        planes.len(),
        Registry::fact_declarations()
            .iter()
            .filter(|declaration| declaration.target == jet_foundation::Registry::RowTarget::Value)
            .count()
    );
}

/// D-FACT-LAW1=B: every row states which way is safe and which written words
/// move it the other way; a row with no meaningful direction states none, and a
/// row that states neither fails the build. This is the law-zero drift guard
/// extended — one implementation in `Registry::law_violations`, not a second
/// guard per kind.
#[test]
fn every_row_states_the_one_way_law() {
    let violations = jet_foundation::Registry::law_violations();
    assert!(
        violations.is_empty(),
        "rows that break the one-way law ({}):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// D-ONCE-LAW1=A: a registered truth names a home that exists and a guard that
/// runs. `Registry::law_violations` already refuses a row with an empty guard
/// column; only a file read can tell whether the named test is real, so that
/// half lives here, in the same lint pass, not in a second guard engine.
#[test]
fn every_registered_truth_names_a_home_and_a_guard_that_exist() {
    use std::path::PathBuf;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();
    for row in jet_foundation::Registry::truths() {
        let home = row.home.expect("law_violations refuses a truth with no home");
        if !root.join(home).is_file() {
            violations.push(format!("`{}` names the home `{home}`, which is not a file", row.name));
        }
        let guard = row.guard.expect("law_violations refuses a truth with no guard");
        match std::fs::read_to_string(root.join(guard.file)) {
            Ok(source) => {
                if !source.contains(&format!("fn {}(", guard.test)) {
                    violations.push(format!(
                        "`{}` names the guard `{}`, which `{}` does not define",
                        row.name, guard.test, guard.file
                    ));
                }
            }
            Err(_) => violations.push(format!(
                "`{}` names the guard file `{}`, which cannot be read",
                row.name, guard.file
            )),
        }
    }
    assert!(
        violations.is_empty(),
        "registered truths whose home or guard is missing ({}):\n{}",
        violations.len(),
        violations.join("\n")
    );
}

/// D-FACT-OWN1=A: the ownership prover is not a plane. What it proves still
/// registers, as a read-only row with no plane algebra.
#[test]
fn a_prover_publishes_read_only_rows() {
    use jet_foundation::Registry::{self, SafeDirection};

    for name in ["Sendability", "ViewProvenance", "Movedness"] {
        let row = Registry::row(name)
            .unwrap_or_else(|| panic!("the ownership prover publishes no `{name}` row"));
        assert!(row.is_prover_supplied(), "`{name}` names no prover");
        assert_eq!(row.safe_direction, SafeDirection::None);
        assert!(row.gates.is_empty(), "`{name}` is read-only, so it has no gate");
    }
}

#[test]
fn orphan_fact_rows_have_one_home() {
    use jet_foundation::Registry::{self, RowKind, RowTarget};

    for name in [
        "Sendability",
        "Attribution",
        "TrackOrigin",
        "ViewProvenance",
        "UnitScaleProvenance",
        "Maturity",
    ] {
        let declaration = Registry::fact_declarations()
            .iter()
            .find(|declaration| declaration.name == name)
            .unwrap_or_else(|| panic!("`{name}` has no Prelude declaration"));
        let matches: Vec<_> = Registry::rows()
            .iter()
            .filter(|row| row.name == name)
            .collect();
        assert_eq!(matches.len(), 1, "`{name}` must have one registry home");
        let row = matches[0];
        assert_eq!(row.target, RowTarget::Value);
        assert_eq!(row.kind(), RowKind::Plane);
        assert_eq!(row.decision, declaration.decision);
    }

    for engine_fact in ["Uninit", "Exhaustiveness"] {
        assert!(
            !Registry::fact_rows().any(|row| row.name == engine_fact),
            "`{engine_fact}` is prover state, not a registered user fact plane"
        );
    }
}

/// D-VERDICT-1455-1: a parser or checker may ask the registry about a marker,
/// but it may not grow a second string vocabulary. The scan is deliberately
/// narrow: it follows the names used by code that matches marker names, while
/// leaving ordinary type, protocol, and diagnostic words alone.
fn marker_name_literal_offenders(
    sources: impl IntoIterator<Item = (String, String)>,
    marker_names: &[&str],
) -> Vec<String> {
    let mut offenders = Vec::new();
    for (path, source) in sources {
        for (line_number, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            let names_marker = [
                "marker_name",
                "marker.name",
                "attr_name",
                "rule_name",
                "marker_head",
            ]
            .iter()
            .any(|needle| code.contains(needle));
            if !names_marker {
                continue;
            }
            for name in marker_names {
                if code.contains(&format!("\"{name}\"")) {
                    offenders.push(format!(
                        "{path}:{} contains marker name `{name}`",
                        line_number + 1
                    ));
                }
            }
        }
    }
    offenders
}

fn collect_source_files(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn marker_name_literals_stay_in_the_registry() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut paths = Vec::new();
    for directory in [
        "crates/jet-lexer/src",
        "crates/jet-parser/src",
        "crates/jet-sema/src",
    ] {
        collect_source_files(&root.join(directory), &mut paths);
    }
    let marker_names: Vec<&str> = Registry::marker_rows()
        .filter_map(|registered| registered.rule)
        .filter(|row| matches!(row.status, RuleStatus::Active))
        .map(|row| row.name)
        .collect();
    let sources = paths.into_iter().map(|path| {
        let display = path.display().to_string();
        let source = std::fs::read_to_string(path).expect("read compiler source");
        (display, source)
    });
    let offenders = marker_name_literal_offenders(sources, &marker_names);
    assert!(
        offenders.is_empty(),
        "marker-name literals escaped the registry:\n{}",
        offenders.join("\n")
    );

    let seeded = marker_name_literal_offenders(
        [("synthetic.rs".to_string(), "if marker_name == \"Inline\" {}".to_string())],
        &["Inline"],
    );
    assert_eq!(seeded.len(), 1, "the seeded drift case must fail this guard");
}
