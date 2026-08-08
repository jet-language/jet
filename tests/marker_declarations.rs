//! D-META-ONE1=A: the marker vocabulary and the effect roots are written as
//! Jet declarations in the Prelude, and Rust keeps no copy.
//!
//! Two guards. The first proves the real Jet parser reads the same file the
//! compiler's registry reader reads, and agrees on every name, every parameter,
//! and every site — so the small reader in `jet-foundation` (which sits below
//! the parser in the crate graph and cannot call it) can never drift from the
//! language. The second proves no Rust file holds a second copy of either
//! vocabulary.

use jet::AST::{Item, MarkerDecl};
use jet_foundation::Facts;
use jet_foundation::Policy::{self, RuleSite};

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
    let registry = Policy::applied_rule_registry();
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
        let row = Policy::applied_rule(&declaration.name)
            .unwrap_or_else(|| panic!("`{}` is written but not registered", declaration.name));

        let facts: Vec<&str> = declaration
            .params
            .iter()
            .filter(|param| param.name.starts_with('$'))
            .map(|param| param.name.as_str())
            .collect();
        assert!(
            facts.contains(&"$sites"),
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

/// Rust files under the compiler, scanned for a second copy of either
/// vocabulary. A marker row may be built only where the declarations are read;
/// the retired effect-root array may not come back anywhere.
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
