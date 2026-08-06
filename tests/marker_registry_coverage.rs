//! D-VERDICT-1455-1 (law zero): a marker exists if and only if it is a registry
//! row, and every active row must be alive end to end. This walks the registry
//! itself, so a row that nothing can parse — `#Authority` was one — fails here
//! instead of sitting in the table looking implemented.
//!
//! The renderer builds each program from the row's own signature and site list.
//! Nothing in this file names a marker, so a new row is covered the moment it
//! is added and cannot be forgotten.

use jet_foundation::Policy::{
    self, AppliedRule, RuleArgType, RuleSite, RuleStatus,
};

/// Sites that describe a position with no source spelling of its own.
/// `Package` is `package.jet` manifest scope, `Impl` and `Operation` are
/// interior positions the writer reaches through some other construct. A row
/// that lives *only* at one of these is reported, not skipped.
fn site_is_source_renderable(site: RuleSite) -> bool {
    !matches!(
        site,
        RuleSite::Package | RuleSite::Impl | RuleSite::Operation
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
    format!("#{}({})", row.name, args.join(", "))
}

/// The smallest whole program that puts `marker` at `site`.
fn program_at(marker: &str, site: RuleSite) -> Option<String> {
    Some(match site {
        RuleSite::File => format!("{marker}\nfn run() {{\n    print(\"ok\")\n}}\n"),
        // A module rule attaches to a `module` declaration, not to the file.
        RuleSite::Module => {
            format!("{marker} module c.lib {{\n}}\n\nfn run() {{\n    print(\"ok\")\n}}\n")
        }
        RuleSite::Function => format!("{marker}\nfn helper() {{\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Method => format!(
            "W :: struct {{\n    a: Int\n\n    {marker}\n    fn helper(self) {{\n    }}\n}}\n\nfn run() {{\n}}\n"
        ),
        RuleSite::Type => format!("{marker}\nstruct W {{\n    a: Int\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Field => format!("struct W {{\n    {marker} a: Int\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Variant => {
            format!("enum E {{\n    {marker} A\n    B\n}}\n\nfn run() {{\n}}\n")
        }
        RuleSite::Block => format!("fn run() {{\n    {marker} {{\n        print(\"ok\")\n    }}\n}}\n"),
        RuleSite::Statement => format!("fn run() {{\n    {marker} print(\"ok\")\n}}\n"),
        RuleSite::Declaration => format!("{marker} value :: 1\n\nfn run() {{\n}}\n"),
        RuleSite::Constant => format!("{marker} comptime value :: 1\n\nfn run() {{\n}}\n"),
        RuleSite::Parameter => {
            format!("fn helper({marker} a: Int) {{\n}}\n\nfn run() {{\n}}\n")
        }
        RuleSite::Expression => format!("fn run() {{\n    value :: {marker}\n}}\n"),
        RuleSite::Test => format!("{marker} {{\n    print(\"ok\")\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Bench => format!("{marker} {{\n    print(\"ok\")\n}}\n\nfn run() {{\n}}\n"),
        RuleSite::Package | RuleSite::Impl | RuleSite::Operation => return None,
    })
}

/// Diagnostics that mean "this marker did not reach its rule": the closed
/// vocabulary rejected the name, or the site table rejected the position.
/// Any other diagnostic is the row's own business and is not this guard's.
fn rejected_the_marker(source: &str) -> Vec<String> {
    let (tokens, lex_diagnostics) = jet::Lexer::lex(source);
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

/// Law zero, the reachable half: every active row can be written at one of its
/// own declared sites and is recognized there. A row nothing can spell is a
/// ghost and must be retired instead.
#[test]
fn every_active_row_is_reachable_at_a_declared_site() {
    let mut ghosts = Vec::new();
    for row in Policy::APPLIED_RULES {
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

/// Law zero, the other half: a retired row teaches its replacement and applies
/// nothing, so every retired row must name a non-empty replacement.
#[test]
fn every_retired_row_teaches_a_replacement() {
    for row in Policy::APPLIED_RULES {
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
    let repeatable: Vec<&str> = Policy::APPLIED_RULES
        .iter()
        .filter(|row| row.repeatable)
        .map(|row| row.name)
        .collect();
    assert_eq!(
        repeatable,
        vec!["Pre", "Post", "allow"],
        "D-MARK-REPEAT1=A names exactly these repeatable rows"
    );
}
