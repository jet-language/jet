//! D-META-ONE1=A / D-FACT-LAW1=B: marker vocabulary, effect roots, and fact law
//! rows are written as Prelude declarations, and Rust keeps no copy.
//!
//! The marker guard proves the real Jet parser reads the same file the
//! compiler's registry reader reads, and agrees on every name, every parameter,
//! and every site — so the small reader in `jet-foundation` (which sits below
//! the parser in the crate graph and cannot call it) can never drift from the
//! language. The fact guard checks both law columns against the one registry.
//! The source scan rejects second Rust copies.

mod common;

use std::collections::BTreeSet;

use jet::AST::{Item, MarkerDecl};
use jet_foundation::Facts;
use jet_foundation::Policy::{self, RuleSite};
use jet_foundation::Registry;

fn declarations(source: &str) -> Vec<Item> {
    let (tokens, lex_diagnostics) = jet::Lexer::lex(source);
    assert!(lex_diagnostics.is_empty(), "lex: {lex_diagnostics:?}");
    jet::Parser::parse(&tokens)
        .expect("Prelude declarations must parse as ordinary Jet")
        .items
}

fn marker_declarations() -> Vec<MarkerDecl> {
    declarations(Policy::MARKER_SOURCE)
        .into_iter()
        .filter_map(|item| match item {
            Item::MarkerDecl(declaration) => Some(declaration),
            _ => None,
        })
        .collect()
}

/// The whole file is `marker` declarations and comments — nothing else sneaks
/// in, and every row the compiler serves came from one of them.
#[test]
fn every_registry_row_is_one_written_declaration() {
    let written = marker_declarations();
    let registry: Vec<_> = Registry::marker_rows()
        .map(|row| row.rule.expect("marker rows carry their applied rule"))
        .collect();
    assert_eq!(
        written.len(),
        registry.len(),
        "the registry and the file must hold the same rows"
    );
    for (declaration, row) in written.iter().zip(registry) {
        assert_eq!(declaration.name, row.name, "rows are served in written order");
    }
}

/// The real parser and the registry reader agree on what each row says.
#[test]
fn the_parser_and_the_registry_read_the_same_rows() {
    for declaration in marker_declarations() {
        let row = Registry::marker_rows()
            .find(|registered| registered.name == declaration.name)
            .and_then(|registered| registered.rule)
            .unwrap_or_else(|| panic!("`{}` is written but not registered", declaration.name));

        let facts: Vec<&str> = declaration
            .params
            .iter()
            .filter(|param| param.name.starts_with('$'))
            .map(|param| param.name.as_str())
            .collect();
        assert!(
            facts.contains(&"@sites"),
            "`{}` must say where it may be written",
            declaration.name
        );

        let written_arguments: Vec<&str> = declaration
            .params
            .iter()
            .filter(|param| !param.name.starts_with('$'))
            .map(|param| param.name.as_str())
            .collect();
        let registered_arguments: Vec<&str> =
            row.signature.params.iter().map(|param| param.name).collect();
        let registered_variadic =
            declaration.params.iter().filter(|param| param.variadic).count();
        assert_eq!(
            registered_variadic,
            usize::from(row.signature.variadic.is_some()),
            "`{}` variadic list",
            declaration.name
        );
        assert_eq!(
            written_arguments.len(),
            registered_arguments.len() + registered_variadic,
            "`{}` argument count",
            declaration.name
        );
        for (index, name) in registered_arguments.iter().enumerate() {
            assert_eq!(&written_arguments[index], name, "`{}` argument order", declaration.name);
        }
    }
}

/// A row's sites are read off the declaration, not off a Rust table.
#[test]
fn a_written_site_list_reaches_the_registry() {
    assert!(Policy::rule_allows("Unsafe", RuleSite::Block));
    assert!(Policy::rule_allows("Redact", RuleSite::Field));
    assert!(!Policy::rule_allows("Redact", RuleSite::Type));
    assert!(
        Policy::applied_rule("Known").is_some_and(|row| row.sites.is_empty()),
        "a retired row keeps no legal site"
    );
}

/// The effect roots are `effect Name` declarations, read by the same law.
#[test]
fn every_effect_root_is_one_written_declaration() {
    let written: Vec<String> = declarations(Facts::EFFECT_SOURCE)
        .into_iter()
        .filter_map(|item| match item {
            Item::EffectDecl(declaration) => Some(declaration.name),
            _ => None,
        })
        .collect();
    assert!(!written.is_empty(), "the effect file must declare roots");
    assert_eq!(written, *Facts::EFFECT_ROOTS, "written roots are the served roots");
}

/// D-FACT-LAW1=B: every non-code row writes both law columns in Prelude. The
/// Rust reader and the one registry must preserve those values, including an
/// empty gate list for a row that has no written escape.
#[test]
fn every_fact_row_carries_its_law_columns() {
    let parsed: Vec<_> = declarations(Registry::FACT_SOURCE)
        .into_iter()
        .filter_map(|item| match item {
            Item::FactDecl(declaration) => Some(declaration),
            _ => None,
        })
        .collect();
    let written: Vec<&str> = Registry::FACT_SOURCE
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("fact "))
        .collect();
    let declarations = Registry::fact_declarations();
    assert_eq!(
        parsed.iter().map(|declaration| declaration.name.as_str()).collect::<Vec<_>>(),
        declarations.iter().map(|declaration| declaration.name).collect::<Vec<_>>(),
        "the real parser and the foundation reader must see the same fact rows"
    );
    assert_eq!(declarations.len(), written.len(), "every fact declaration is read");

    let declaration_names: BTreeSet<_> = declarations.iter().map(|declaration| declaration.name).collect();
    let rows: Vec<_> = Registry::rows()
        .iter()
        .filter(|row| declaration_names.contains(row.name))
        .collect();
    let row_names: BTreeSet<_> = rows.iter().map(|row| row.name).collect();
    assert_eq!(
        row_names, declaration_names,
        "parsed fact declarations and registry rows must have the same names"
    );
    assert_eq!(rows.len(), declarations.len(), "every fact declaration serves one row");

    for declaration in declarations {
        let source = written
            .iter()
            .find(|source| source.starts_with(&format!("fact {}(", declaration.name)))
            .unwrap_or_else(|| panic!("fact `{}` is not written", declaration.name));
        let row = rows
            .iter()
            .find(|row| row.name == declaration.name)
            .unwrap_or_else(|| panic!("fact `{}` has no registry row", declaration.name));
        assert!(source.starts_with(&format!("fact {}(", declaration.name)));
        for column in ["@holds:", "@safe:", "@gates:"] {
            assert!(source.contains(column), "`{}` must write `{column}`", declaration.name);
        }
        assert_eq!(row.name, declaration.name);
        assert_eq!(row.target, declaration.target);
        assert_eq!(row.safe_direction, declaration.safe_direction);
        assert_eq!(row.gates, declaration.gates);
    }
}

/// D-META-USER1=A: source rows join the same checked vocabulary that drives
/// marker legality, and a body can return an additive member through the
/// existing generated-fragment path.
#[test]
fn source_rules_record_facts_and_add_checked_members() {
    let source = r#"
marker Recorded($sites: [.Type])

marker AddGreeting($sites: [.Type]) {
    tname :: target.name
    emit("""
impl $tname {{
    fn greeting(self) => String {{
        return "hello"
    }}
}}
""")
}

#[Recorded, AddGreeting]
struct Person { name: String }

fn run() {
    person :: Person.{name: "ada"}
    print(person.greeting())
}
"#;
    jet::compile(source).expect("source marker facts and additive bodies should compile");

    let parsed = declarations(source);
    let recorded = parsed
        .iter()
        .find_map(|item| match item {
            Item::MarkerDecl(declaration) if declaration.name == "Recorded" => Some(declaration),
            _ => None,
        })
        .expect("bodyless source rule");
    let additive = parsed
        .iter()
        .find_map(|item| match item {
            Item::MarkerDecl(declaration) if declaration.name == "AddGreeting" => Some(declaration),
            _ => None,
        })
        .expect("bodyful source rule");
    assert!(recorded.body.is_none());
    assert!(additive.body.is_some());

    let source_declarations: Vec<_> = parsed
        .iter()
        .filter_map(|item| match item {
            Item::MarkerDecl(declaration) => Some(declaration.clone()),
            _ => None,
        })
        .collect();
    let vocabulary = Policy::MarkerVocabulary::with_derives_and_declarations(
        std::iter::empty(),
        source_declarations,
    );
    let person = parsed
        .iter()
        .find_map(|item| match item {
            Item::Struct(definition) => Some(definition),
            _ => None,
        })
        .expect("reflected target type");
    let reflection = jet::Comptime::build_struct_type_info_with_path_and_vocabulary(
        person,
        &[],
        "Person",
        Some(&vocabulary),
    );
    let jet::AST::CtValue::Struct { fields, .. } = reflection else {
        panic!("TypeInfo must be a struct value");
    };
    let Some((_, jet::AST::CtValue::List(markers))) =
        fields.iter().find(|(name, _)| name == "markers")
    else {
        panic!("TypeInfo must expose marker facts");
    };
    assert!(markers.iter().any(|marker| {
        matches!(
            marker,
            jet::AST::CtValue::Struct { fields, .. }
                if fields.iter().any(|(name, value)| {
                    name == "name"
                        && matches!(value, jet::AST::CtValue::Str(value) if value == "Recorded")
                })
        )
    }));
}

/// D-MARK-FORM1=A: a source rule keeps typed signature checking rather than
/// accepting an arity-only marker call.
#[test]
fn source_rule_rejects_a_wrong_typed_argument() {
    let diagnostics = jet::compile(
        "marker NeedsName(name: String, $sites: [.Type])\n#NeedsName(7)\nstruct Person { id: Int }\n",
    )
    .expect_err("a source rule must check its typed argument");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E0930"),
        "expected E0930: {diagnostics:?}"
    );
}

#[test]
fn unknown_callable_rule_is_checked_against_the_bundle_registry() {
    let diagnostics = jet::compile(
        "#MissingRule fn work() {}\nfn run() {}\n",
    )
    .expect_err("an undeclared callable rule must not remain a silent marker");
    assert_eq!(
        diagnostics.iter().filter(|diagnostic| diagnostic.code == "E0927").count(),
        1,
        "expected one registered unknown-rule diagnostic: {diagnostics:?}"
    );
}

#[test]
fn source_rule_rejects_a_site_not_in_its_declared_facts() {
    let diagnostics = jet::compile(
        "marker TypeOnly($sites: [.Type])\n#TypeOnly fn work() {}\nfn run() {}\n",
    )
    .expect_err("a source rule must enforce its declared legal sites");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0355"),
        "expected the registered site diagnostic: {diagnostics:?}"
    );
}

#[test]
fn source_rule_rejects_a_non_repeatable_duplicate() {
    let diagnostics = jet::compile(
        "marker Once($sites: [.Type])\n#[Once, Once]\nstruct Person { id: Int }\n",
    )
    .expect_err("a non-repeatable source rule must reject a duplicate");
    let duplicate = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0999")
        .expect("duplicate rule diagnostic");
    let detail = duplicate.detail.as_deref().unwrap_or("");
    assert!(detail.contains("first application span"), "{duplicate:?}");
    assert!(detail.contains("second application span"), "{duplicate:?}");
}

/// D-META-USER1=A: the same body path applies to a callable target, including
/// a registered diagnostic raised by the rule itself.
#[test]
fn source_rule_body_can_reject_a_function_target() {
    let diagnostics = jet::compile(
        r#"
marker RejectsFunction($sites: [.Function]) {
    reject(
        code: "E0927",
        what: "the function is not allowed",
        why: "the test rule rejects this declaration",
        fix: "remove the marker"
    )
}

#RejectsFunction
fn run() {}
"#,
    )
    .expect_err("a function-site rule must run its checked body");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E2710"),
        "expected the function rule wrapper: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.cause.iter().any(|code| code == "E0927")),
        "expected the registered function-rule cause: {diagnostics:?}"
    );
}

/// D-META-USER1=A / Law 1: generated members cannot change or shadow a
/// user-written member, and the error preserves both source locations.
#[test]
fn source_rule_collision_names_generated_and_written_spans() {
    let diagnostics = jet::compile(
        r#"
marker AddGreeting($sites: [.Type]) {
    tname :: target.name
    emit("""
impl $tname {{
    fn greeting(self) => String {{
        return "generated"
    }}
}}
""")
}

#AddGreeting
struct Person { name: String }

impl Person {
    fn greeting(self) => String {
        return "written"
    }
}
"#,
    )
    .expect_err("a source rule must not shadow a written member");
    let collision = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "E0105")
        .expect("collision diagnostic");
    assert!(collision.what.contains("generated member"), "{collision:?}");
    let detail = collision.detail.as_deref().unwrap_or("");
    assert!(detail.contains("generated rule"), "{collision:?}");
    assert!(detail.contains("existing member"), "{collision:?}");
}

/// D-META-USER1=A / I4: a rule body can reject through the registered
/// diagnostic constructor instead of smuggling an unregistered compiler error.
#[test]
fn source_rule_rejection_keeps_the_registered_cause() {
    let diagnostics = jet::compile(
        r#"
marker Rejects($sites: [.Type]) {
    reject(
        code: "E0927",
        what: "the rule rejected this type",
        why: "the test rule is deliberately strict",
        fix: "remove the marker"
    )
}

#Rejects
struct Person { id: Int }

fn run() {}
"#,
    )
    .expect_err("a rule rejection must stop compilation");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == "E2710"),
        "expected the registered rule-body wrapper: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.cause.iter().any(|code| code == "E0927")),
        "expected E0927 as the causal registered diagnostic: {diagnostics:?}"
    );
}

/// Rust files under the compiler, scanned for a second copy of the declaration
/// rows. Marker and fact rows may be built only where their declarations are
/// read; the retired effect-root array may not come back anywhere.
#[test]
fn no_rust_file_keeps_a_second_copy() {
    let mut offenders = Vec::new();
    for path in rust_sources() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let display = path.display().to_string();
        // A hand-written row is the only thing that spells a site list or calls
        // the retired row macros. Matching on the struct name alone would also
        // catch the type's own definition and the patterns that read a row.
        let builds_a_row = text.contains("sites: &[RuleSite::") || text.contains("rule!(");
        if builds_a_row && !display.ends_with("Policy/MarkerSource.rs") {
            offenders.push(format!("{display} builds marker rows in Rust"));
        }
        if text.contains("NON_CODE_ROWS") || text.contains("PRELUDE_GATES") {
            offenders.push(format!("{display} keeps fact law rows in Rust"));
        }
        if text.contains("\"Net\", \"FS\"") {
            offenders.push(format!("{display} keeps a copy of the effect roots"));
        }
    }
    assert_eq!(offenders, Vec::<String>::new());
}

fn rust_sources() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for directory in ["crates", "Source"] {
        collect(&root.join(directory), &mut found);
    }
    found
}

fn collect(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}
