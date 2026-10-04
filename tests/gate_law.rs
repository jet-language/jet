//! D-GATE-LAW1=A: one policy vocabulary, real source gates, and pre-write refusals.
mod common;
use std::fs;
use jet::Policy::{PolicyDeclaration, PolicyKey, PolicyScope, PolicyValue};

fn declaration(key: PolicyKey, value: PolicyValue, scope: PolicyScope, source: &str) -> PolicyDeclaration {
    PolicyDeclaration { key, value, scope, span: jet::Diagnostics::Span::new(0, 0), target: None, source: source.into() }
}

fn ledger(source: &str) -> jet::Sema::GateLedger::GateLedger {
    let scratch = common::Scratch::new("gate-law-source");
    let entry = scratch.join("main.jet");
    fs::write(&entry, source).unwrap();
    let mut bundle = jet::Loader::load_entry(entry.to_str().unwrap()).unwrap();
    let diagnostics = jet::Sema::check_bundle(&mut bundle, jet::Sema::CompileMode::Check);
    assert!(!diagnostics.iter().any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error), "{diagnostics:#?}");
    jet::Sema::GateLedger::GateLedger::collect(&bundle, jet::Policy::GateSet::default())
}

#[test]
fn all_fourteen_kinds_have_one_policy_key_and_authoritative_floor() {
    assert_eq!(jet::Policy::AUDITED_GATE_KEYS.len(), 14);
    for &key in jet::Policy::AUDITED_GATE_KEYS {
        let kind = jet::Sema::GateLedger::GateKind::parse(key.name()).unwrap();
        assert_eq!(kind.policy_key(), key);
        let declarations = [declaration(key, PolicyValue::Forbid, PolicyScope::Organization, "org-policy.jet"), declaration(key, PolicyValue::GateOnly, PolicyScope::Package, "package.jet")];
        let refusal = jet::Policy::gate_refusal(key, "main.jet:4..9", None, &declarations).unwrap();
        assert_eq!(refusal.code, "E3415");
        assert!(refusal.what.contains(key.name()));
        assert!(refusal.what.contains("main.jet:4..9"));
        assert!(refusal.what.contains("org-policy.jet"));
        assert!(jet::Policy::resolve_with_gates(key, declarations, &jet::Policy::GateSet::allow(key)).is_err());
        if !matches!(key, PolicyKey::Unsafe | PolicyKey::Impure | PolicyKey::Nondeterministic) {
            assert_eq!(jet::Policy::default_gate_value(key), PolicyValue::GateOnly);
            assert!(jet::Policy::parse_value(key, ".Default").is_err());
            assert!(jet::Policy::parse_value(key, ".Obligations").is_err());
            assert_eq!(jet::Policy::parse_value(key, ".GateOnly").unwrap(), PolicyValue::GateOnly);
        }
    }
}

#[test]
fn unused_plain_name_is_not_a_structure_gate_but_written_suppression_is() {
    let plain = ledger("fn run() { unused_value :: 1 print(\"done\") }\n");
    assert!(!plain.entries().iter().any(|entry| entry.kind == jet::Sema::GateLedger::GateKind::Structure));
    let suppressed = ledger("fn run() { _unused_value :: 1 print(\"done\") }\n");
    assert!(suppressed.entries().iter().any(|entry| entry.kind == jet::Sema::GateLedger::GateKind::Structure && entry.subject.contains("_unused_value")));
}

#[test]
fn lint_gate_keeps_the_source_reason() {
    let gates = ledger("fn run() { #allow(unit_scalar_rewrap, \"reviewed source reason\") print(\"done\") }\n");
    let entry = gates.entries().iter().find(|entry| entry.kind == jet::Sema::GateLedger::GateKind::LintAllow).unwrap();
    assert_eq!(entry.reason.as_deref(), Some("reviewed source reason"));
    assert_eq!(entry.status.as_deref(), Some("recorded"));
}

#[test]
fn allow_reason_cannot_suppress_itself() {
    let diagnostics = jet::compile("fn run() { #allow(allow_reason, \"cannot disable the audit\") print(\"done\") }").unwrap_err();
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E0355"));
}

#[test]
fn trust_writers_refuse_before_touching_the_store() {
    let scratch = common::Scratch::new("gate-law-trust");
    fs::write(scratch.join("package.jet"), "name: \"law\"\nversion: \"0.1.0\"\npolicy: { trust_grant: .Forbid }\n").unwrap();
    let store = scratch.join("trust");
    let grant = jetpack::Trust::parse_grant_selector("build:audited", "repo").unwrap();
    for result in [jetpack::Trust::add_grant(&store, &scratch.path, &grant), jetpack::Trust::grant_hash(&store, &scratch.path, "audited"), jetpack::Trust::add_pattern(&store, &scratch.path, "/reviewed/*")] {
        let diagnostics = result.unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3415" && diagnostic.what.contains("trust_grant") && diagnostic.what.contains("package.jet")), "{diagnostics:#?}");
    }
    assert!(!store.exists());
}

#[test]
fn cli_choices_use_their_own_gate_kind() {
    for (key, argument) in [(PolicyKey::BuildFlag, "--allow=Net"), (PolicyKey::DependencyGrant, "--allow=Net"), (PolicyKey::SessionFlag, "--trust"), (PolicyKey::ForcePin, "--force")] {
        let scratch = common::Scratch::new("gate-law-flags");
        fs::write(scratch.join("package.jet"), format!("name: \"law\"\nversion: \"0.1.0\"\npolicy: {{ {}: .Forbid }}\n", key.name())).unwrap();
        let diagnostics = jet::GateWriters::check_invocation_flags(&scratch.path, &[argument.into()]).unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3415" && diagnostic.what.contains(key.name()) && diagnostic.what.contains(argument)));
    }
}

#[test]
fn workspace_force_and_dependency_grants_refuse_before_lock_publication() {
    for key in [PolicyKey::ForcePin, PolicyKey::DependencyGrant] {
        let scratch = common::Scratch::new("gate-law-workspace");
        fs::write(scratch.join("package.jet"), format!("name: \"law\"\nversion: \"0.1.0\"\npolicy: {{ {}: .Forbid }}\n", key.name())).unwrap();
        let mut plan = jetpack::WorkspaceFile::WorkspacePlan::default();
        if key == PolicyKey::ForcePin {
            plan.overlay_policy = jetpack::Overlay::parse_workspace_policy("module workspace { overlay audit { package(\"app\").version: Force(\"2\") } }\n").unwrap();
        } else {
            plan.overlay_policy.build_grants.push(("app".into(), vec!["Exec".into()]));
        }
        let diagnostics = jetpack::WorkspaceLock::write(&scratch.path, &plan).unwrap_err();
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == "E3415" && diagnostic.what.contains(key.name())), "{diagnostics:#?}");
        assert!(!scratch.join(jet::Syntax::UNIFIED_LOCK_FILE).exists());
    }
}
