//! D-GATE-LAW1=A: manifest, lock and invocation gate projections shared by builds and inspection.
use crate as jet;
use crate::Sema::GateLedger::{GateEntry, GateKind, GateLedger};

pub fn append_package_writers(ledger: &mut GateLedger, bundle: &crate::AST::ProgramBundle) {
    let root = &bundle.project_root;
    if let Ok(Some(facts)) = jet::Loader::package_facts_for_bundle(bundle) {
        let source = facts.origin.as_str();
        append_authority_entries(ledger, &facts.authority, source);
        for effect in &facts.build_allow {
            let mut provenance = facts.field_provenance("build_allow").to_vec();
            if provenance.is_empty() {
                provenance.push(format!("{source}:build.allow"));
            }
            ledger.push(external_entry(
                GateKind::BuildFlag,
                "security",
                "package",
                source,
                &format!("build:{effect}"),
                "package build authority",
                provenance,
            ));
        }
    }

    if let Some(lock) = jet::Lock::load(root) {
        if let Some(authority) = &lock.authority {
            append_authority_entries(ledger, authority, jet::Syntax::UNIFIED_LOCK_FILE);
        } else {
            for package in &lock.packages {
                if !package.effect_grants.is_empty() || !package.granted_effects.is_empty() {
                    ledger.push(external_entry(
                        GateKind::DependencyGrant,
                "security",
                "package",
                jet::Syntax::UNIFIED_LOCK_FILE,
                &package.name,
                        &format!(
                            "required effects: {}; granted effects: {}; denied effects: {}; authority: {}",
                            effect_names(&package.required_effects, &package.effects),
                            effect_names(&package.granted_effects, &package.effect_grants),
                            effect_names(&package.denied_effects, &[]),
                            package
                                .effect_authority
                                .as_deref()
                                .unwrap_or(".jet/lock effect provenance"),
                        ),
                        vec![format!(
                            "{}:dependency.effect-grants",
                            jet::Syntax::UNIFIED_LOCK_FILE
                        )],
                    ));
                }
            }
        }
        for (subject, effects) in &lock.workspace_overlay_policy.build_grants {
            ledger.push(external_entry(
                GateKind::DependencyGrant,
                "security",
                "package",
                jet::Syntax::UNIFIED_LOCK_FILE,
                &format!("build:{subject}"),
                &format!(
                    "required effects: not evaluated; granted effects: {}; denied effects: none; authority: .jet/lock workspace authority",
                    effects.join(",")
                ),
                vec![format!(
                    "{}:workspace.build-grants",
                    jet::Syntax::UNIFIED_LOCK_FILE
                )],
            ));
        }
        for overlay in &lock.workspace_overlay_policy.overlays {
            for package in &overlay.packages {
                let forced = is_forced_override(package);
                if !forced {
                    continue;
                }
                let fields = package
                    .field_priorities
                    .iter()
                    .filter(|(_, priority)| **priority >= 100)
                    .map(|(field, _)| field.as_str())
                    .collect::<Vec<_>>();
                let detail = if fields.is_empty() {
                    "workspace override".to_string()
                } else {
                    format!("workspace override fields: {}", fields.join(","))
                };
                ledger.push(external_entry(
                    GateKind::ForcePin,
                    "security",
                    "package",
                    jet::Syntax::UNIFIED_LOCK_FILE,
                    &package.package,
                    &detail,
                    vec![format!(
                        "{}:overlay {} priority=Force",
                        jet::Syntax::UNIFIED_LOCK_FILE,
                        overlay.name
                    )],
                ));
            }
        }
    }
}

pub fn is_forced_override(package: &jet_pkg_model::Overlay::PackageOverride) -> bool {
    package.priority >= 100 || package.field_priorities.values().any(|priority| *priority >= 100)
}
fn append_authority_entries(
    ledger: &mut GateLedger,
    authority: &jet::Package::PackageAuthority,
    source: &str,
) {
    let provenance = |field: &str| vec![format!("{source}:{field}")];
    let holds_authority = format!("{source} authority.holds");
    let grants_authority = format!("{source} authority.grants");
    if let Some(allow) = &authority.holds.allow {
        ledger.push(external_entry(
            GateKind::BuildFlag,
            "security",
            "package",
            source,
            "authority.holds.allow",
            &format!(
                "required effects: not evaluated; granted effects: {}; denied effects: none; authority: {holds_authority}",
                allow.join(",")
            ),
            provenance("authority.holds.allow"),
        ));
    }
    if let Some(deny) = &authority.holds.deny {
        ledger.push(external_entry(
            GateKind::BuildFlag,
            "security",
            "package",
            source,
            "authority.holds.deny",
            &format!(
                "required effects: not evaluated; granted effects: none; denied effects: {}; authority: {holds_authority}",
                deny.join(",")
            ),
            provenance("authority.holds.deny"),
        ));
    }
    for (dependency, rights) in &authority.grants {
        ledger.push(external_entry(
            GateKind::DependencyGrant,
            "security",
            "package",
            source,
            dependency,
            &format!(
                "required effects: not evaluated; granted effects: {}; denied effects: none; authority: {grants_authority}",
                rights.join(",")
            ),
            provenance("authority.grants"),
        ));
    }
    if let Some(trust) = &authority.trust {
        if let Some(default) = trust.default {
            ledger.push(external_entry(
                GateKind::TrustGrant,
                "security",
                "package",
                source,
                "authority.trust.default",
                &format!("default: {}", trust_decision_label(default)),
                provenance("authority.trust.default"),
            ));
        }
        if let Some(ci) = trust.ci_prompt {
            ledger.push(external_entry(
                GateKind::TrustGrant,
                "security",
                "package",
                source,
                "authority.trust.ci",
                &format!("prompt: {}", trust_decision_label(ci)),
                provenance("authority.trust.ci"),
            ));
        }
        for (service, decision) in &trust.services {
            ledger.push(external_entry(
                GateKind::TrustGrant,
                "security",
                "package",
                source,
                &format!("authority.trust.services.{service}"),
                &format!("{}", trust_decision_label(*decision)),
                provenance("authority.trust.services"),
            ));
        }
        if let Some(require) = trust.require {
            ledger.push(external_entry(
                GateKind::TrustGrant,
                "security",
                "package",
                source,
                "authority.trust.require",
                &format!("require: {}", require.label()),
                provenance("authority.trust.require"),
            ));
        }
    }
    for provider in &authority.providers {
        let mut detail = format!("registry: {}", provider.registry);
        if !provider.allow.is_empty() {
            detail.push_str(&format!(", allow: {}", provider.allow.join(",")));
        }
        if !provider.deny.is_empty() {
            detail.push_str(&format!(", deny: {}", provider.deny.join(",")));
        }
        ledger.push(external_entry(
            GateKind::TrustGrant,
            "security",
            "package",
            source,
            &format!("authority.providers.{}", provider.provider),
            &detail,
            provenance("authority.providers"),
        ));
    }
}

fn trust_decision_label(decision: jet::Package::TrustDecision) -> &'static str {
    match decision {
        jet::Package::TrustDecision::Allow => "allow",
        jet::Package::TrustDecision::Prompt => "prompt",
        jet::Package::TrustDecision::Deny => "deny",
    }
}
fn effect_names(primary: &[String], fallback: &[String]) -> String {
    let effects = if primary.is_empty() {
        fallback
    } else {
        primary
    };
    if effects.is_empty() {
        "none".to_string()
    } else {
        effects.join(",")
    }
}

pub fn invocation_gate_kind(argument: &str) -> Option<GateKind> {
    if argument == "--force" {
        Some(GateKind::ForcePin)
    } else if argument == "--gate" || argument.starts_with("--gate=")
        || argument == "--allow" || argument.starts_with("--allow=")
        || matches!(argument, "--release" | "--target" | "--profile")
        || argument.starts_with("--target=") || argument.starts_with("--profile=")
    {
        Some(GateKind::BuildFlag)
    } else if argument == "--deny" || argument.starts_with("--deny=")
        || matches!(argument, "--trust" | "--online" | "--try-anyway" | "--interpret" | "--offline" | "--locked")
    {
        Some(GateKind::SessionFlag)
    } else {
        None
    }
}

pub fn check_invocation_flags(root: &std::path::Path, args: &[String]) -> Result<(), Vec<crate::Diagnostics::Diagnostic>> {
    if !args.iter().any(|argument| invocation_gate_kind(argument).is_some()) {
        return Ok(());
    }
    let declarations = crate::Loader::project_gate_declarations(root)?;
    let mut diagnostics = Vec::new();
    for argument in args {
        let Some(kind) = invocation_gate_kind(argument) else { continue };
        if let Some(diagnostic) = crate::Policy::gate_refusal(
            kind.policy_key(), &format!("command line ({argument})"), None, &declarations,
        ) {
            diagnostics.push(diagnostic);
        }
        if argument == "--allow" || argument.starts_with("--allow=") {
            if let Some(diagnostic) = crate::Policy::gate_refusal(
                crate::Policy::PolicyKey::DependencyGrant,
                &format!("command line ({argument})"), None, &declarations,
            ) {
                diagnostics.push(diagnostic);
            }
        }
    }
    if diagnostics.is_empty() { Ok(()) } else { Err(diagnostics) }
}

pub fn append_invocation_flags(ledger: &mut GateLedger, args: &[String]) {
    for (index, argument) in args.iter().enumerate() {
        let Some(kind) = invocation_gate_kind(argument) else { continue };
        let subject = if matches!(argument.as_str(), "--gate" | "--allow" | "--deny" | "--target" | "--profile") {
            args.get(index + 1).map_or_else(|| argument.clone(), |value| format!("{argument} {value}"))
        } else {
            argument.clone()
        };
        ledger.push(external_entry(
            kind, "security",
            if kind == GateKind::SessionFlag { "session" } else { "build" },
            "command line", &subject, "written invocation choice",
            vec!["command line".to_string()],
        ));
        if argument == "--allow" || argument.starts_with("--allow=") {
            ledger.push(external_entry(
                GateKind::DependencyGrant, "security", "build", "command line",
                &subject, "invocation authority grant", vec!["command line".to_string()],
            ));
        }
    }
}

pub fn external_entry(
    kind: GateKind,
    domain: &str,
    scope: &str,
    source: &str,
    subject: &str,
    detail: &str,
    provenance: Vec<String>,
) -> GateEntry {
    GateEntry {
        kind,
        domain: domain.to_string(),
        scope: scope.to_string(),
        source: source.to_string(),
        span: None,
        subject: subject.to_string(),
        reason: None,
        status: Some("recorded".to_string()),
        detail: detail.to_string(),
        provenance,
        operations: Vec::new(),
    }
}
