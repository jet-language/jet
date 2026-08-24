//! D-CONF-MERGE1 / D-FACT-LAW1: one resolver owns every build-fact writer.

use jet_foundation::Diagnostics::Span;
use jet_foundation::Policy::{
    self, ContributionLayer, FactContribution, FactError, FactKey, FactValue, SourceScope,
};

fn writer(
    value: &str,
    scope: SourceScope,
    layer: ContributionLayer,
    source: &str,
) -> FactContribution {
    FactContribution::new(
        "Build.Profile",
        FactValue::Text(value.to_string()),
        scope,
        layer,
        source,
    )
}

#[test]
fn source_scope_and_layer_order_produce_one_explainable_chain() {
    let declarations = [
        writer(
            "package",
            SourceScope::Package,
            ContributionLayer::Declaration,
            "package.jet",
        ),
        writer(
            "file",
            SourceScope::File,
            ContributionLayer::Declaration,
            "main.jet",
        ),
        writer(
            "module",
            SourceScope::Module,
            ContributionLayer::Declaration,
            "main.jet",
        ),
        writer(
            "function",
            SourceScope::Function,
            ContributionLayer::Declaration,
            "main.jet",
        ),
        writer(
            "block",
            SourceScope::Block,
            ContributionLayer::Declaration,
            "main.jet",
        ),
        writer(
            "item",
            SourceScope::Item,
            ContributionLayer::Declaration,
            "main.jet",
        ),
        writer(
            "profile",
            SourceScope::Package,
            ContributionLayer::OptimizationBundle,
            "release",
        ),
        writer(
            "cli",
            SourceScope::Package,
            ContributionLayer::CommandLine,
            "command line",
        ),
    ];
    let fact = Policy::resolve(FactKey::new("Build.Profile"), declarations)
        .expect("the canonical resolver accepts cross-layer overrides")
        .expect("the chain has writers");

    assert_eq!(fact.value, FactValue::Text("cli".to_string()));
    assert_eq!(fact.provenance.len(), 8);
    assert_eq!(fact.effective, 7);
    let explanation = Policy::explain(&fact);
    assert_eq!(
        explanation
            .lines()
            .filter(|line| line.contains('['))
            .count(),
        8
    );
    assert!(explanation.contains("[effective] command line / package"));
}

#[test]
fn same_layer_conflict_names_both_sources() {
    let first = writer(
        "debug",
        SourceScope::Package,
        ContributionLayer::Workspace,
        "workspace-a.jet",
    )
    .at(Span::new(10, 16));
    let second = writer(
        "release",
        SourceScope::Package,
        ContributionLayer::Workspace,
        "workspace-b.jet",
    )
    .at(Span::new(24, 31));
    let error = Policy::resolve(FactKey::new("Build.Profile"), [first, second])
        .expect_err("different same-layer values must stop the build");
    assert!(matches!(error, FactError::Conflict { .. }));

    let diagnostic = error
        .diagnostic()
        .expect("conflicts have the UI diagnostic");
    assert_eq!(diagnostic.code, "E3521");
    assert!(diagnostic.why.contains("workspace-a.jet"));
    assert!(diagnostic.why.contains("workspace-b.jet"));
    assert_eq!(diagnostic.span, Some(Span::new(24, 31)));
}

#[test]
fn force_pins_later_layers_and_safety_only_tightens() {
    let force = writer(
        "release",
        SourceScope::Package,
        ContributionLayer::System,
        "system.jet",
    )
    .force_with_reason("certified profile");
    let fact = Policy::resolve(
        FactKey::new("Build.Profile"),
        [
            writer(
                "dev",
                SourceScope::Package,
                ContributionLayer::Declaration,
                "package.jet",
            ),
            force,
            writer(
                "debug",
                SourceScope::Package,
                ContributionLayer::CommandLine,
                "command line",
            ),
        ],
    )
    .expect("system force is valid")
    .expect("the chain has writers");
    assert_eq!(fact.value, FactValue::Text("release".to_string()));
    let explanation = Policy::explain(&fact);
    assert!(explanation.contains("pin=certified profile"));
    assert!(explanation.contains("[shadowed] command line"));

    let safety = FactKey::tighten_only("Build.Settings.tls");
    assert!(Policy::resolve(
        safety.clone(),
        [
            FactContribution::new(
                "Build.Settings.tls",
                FactValue::Bool(true),
                SourceScope::Package,
                ContributionLayer::Declaration,
                "package.jet",
            ),
            FactContribution::new(
                "Build.Settings.tls",
                FactValue::Bool(false),
                SourceScope::Package,
                ContributionLayer::Environment,
                "environment",
            ),
        ],
    )
    .is_ok());
    assert!(matches!(
        Policy::resolve(
            safety,
            [
                FactContribution::new(
                    "Build.Settings.tls",
                    FactValue::Bool(false),
                    SourceScope::Package,
                    ContributionLayer::Declaration,
                    "package.jet",
                ),
                FactContribution::new(
                    "Build.Settings.tls",
                    FactValue::Bool(true),
                    SourceScope::Package,
                    ContributionLayer::Environment,
                    "environment",
                ),
            ],
        ),
        Err(FactError::SafetyWidening { .. })
    ));
}
