//! The env-namespace evaluation core: parse a source to a `Program`, evaluate
//! every enabled `Item::Module`, run each `env.*` field through `comptime` (or
//! the text-level Pkg-sugar parser for `packages:`), and merge the resulting
//! contributions through the §6 engine.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::Comptime;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::AST::{
    CallArg, ContribValue, Contribution, CtKey, CtValue, EnvLit, Expr, Func, Item, ModuleDecl,
    Namespace,
};

use super::super::Merge::{self, EntryContribution, MergeError, MergedEntry, Scalar};
use super::DevService::evaluate_dev_service;
use super::Environment::{
    files_from_value, lifecycle_from_field, languages_from_value, profiles_from_value,
    qualified_call_name, EnvironmentIntegration, EnvironmentLifecycle, IntegrationKind,
    LanguageSpec, ProfileSpec,
};
use super::Diagnostics::{
    not_a_namespace_literal, packages_not_a_list, prompt_bad_field, prompt_bad_value,
    wrong_namespace_type,
};
use super::Computed::evaluate_named_fields;
use super::System::{evaluate_fleet, evaluate_image, evaluate_system, evaluate_vmtest};
use super::Types::{AdapterPlan, AdapterRecipe, DevServicePlan, EvaluatedModule};

/// Parse `src` and evaluate every enabled module it declares. `base_dir`
/// resolves `embed_file` inside contribution expressions, same as ordinary
/// comptime evaluation.
pub fn evaluate_source(src: &str, base_dir: &Path) -> Result<Vec<EvaluatedModule>, Diagnostic> {
    let program = parse_program(src)?;
    evaluate_modules(&program.items, src, base_dir)
}

/// Lex + parse `src` to a `Program`, collapsing any lex/parse failure to a
/// single diagnostic. The module surface is evaluated, not type-checked, here,
/// so the first error is enough to stop.
pub(super) fn parse_program(src: &str) -> Result<crate::AST::Program, Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    crate::Parser::parse(&toks).map_err(|mut diags| {
        diags.pop().unwrap_or_else(|| {
            Diagnostic::error(
                "E0000",
                "parse failed".into(),
                String::new(),
                String::new(),
                None,
            )
        })
    })
}

/// Evaluate every enabled `Item::Module` in `items` (already-parsed).
pub fn evaluate_modules(
    items: &[Item],
    src: &str,
    base_dir: &Path,
) -> Result<Vec<EvaluatedModule>, Diagnostic> {
    // Module fields use the canonical comptime/TIR evaluator.  Install the
    // bridge here as well as in higher-level workspace entry points so direct
    // callers of this public evaluator get the same semantics.
    jet_codegen::Codegen::TIR::install_comptime_bridge();
    let funcs = collect_funcs(items);
    let globals = collect_comptime_globals(items, &funcs, base_dir)?;
    let mut out = Vec::new();
    for item in items {
        let Item::Module(m) = item else { continue };
        if !m.is_auto_discovered() {
            continue;
        }
        out.push(evaluate_module(m, src, base_dir, &funcs, &globals)?);
    }
    Ok(out)
}

fn collect_funcs(items: &[Item]) -> HashMap<String, &Func> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Func(f) => Some((f.name.clone(), f)),
            _ => None,
        })
        .collect()
}

/// Resolve module-level immutable comptime values before any namespace field
/// runs. Module fields are allowed to use these values, but they must enter
/// through the same dependency, purity, and deterministic-value path as
/// sibling fields. In particular, a self-reference is a real cycle, not an
/// unresolved-name fallback.
fn collect_comptime_globals<'a>(
    items: &'a [Item],
    funcs: &HashMap<String, &'a Func>,
    base_dir: &Path,
) -> Result<HashMap<String, crate::Comptime::CtValue>, Diagnostic> {
    let definitions = items
        .iter()
        .filter_map(|item| match item {
            Item::Const(def) if def.is_comptime => {
                Some((def.name.clone(), (def.name_span, &def.value)))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let computed = evaluate_named_fields(
        &definitions,
        &HashMap::new(),
        funcs,
        &HashSet::new(),
        base_dir,
        None,
        "module-level comptime values must have a deterministic dependency order",
        "break the cycle by making one value independent or by moving the shared computation into a pure function",
    )?;
    Ok(computed.values)
}

fn evaluate_module<'a>(
    m: &ModuleDecl,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &'a Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<EvaluatedModule, Diagnostic> {
    let mut entries = Vec::new();
    let mut systems = Vec::new();
    let mut images = Vec::new();
    let mut fleets = Vec::new();
    let mut vmtests = Vec::new();
    let mut dev_services = Vec::new();
    let mut secrets = Vec::new();
    let mut adapters = Vec::new();
    let mut lifecycle = EnvironmentLifecycle::default();
    let mut profiles = Vec::new();
    let mut languages = Vec::new();
    let mut files = Vec::new();
    let integrations = evaluate_integration_imports(&m.imports, base_dir, funcs, globals)?;
    let mut integration_secrets = Vec::new();
    for integration in &integrations {
        for secret in &integration.secrets {
            push_unique_string(&mut integration_secrets, secret.clone());
        }
    }
    for c in &m.contributions {
        match (&c.namespace, &c.value) {
            (Namespace::Env, ContribValue::Expr(_)) => {
                let capture = evaluate_env_contribution(c, src, base_dir, funcs, globals)?;
                let EnvCapture {
                    entry,
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    profiles: captured_profiles,
                    languages: captured_languages,
                    files: captured_files,
                } = capture;
                lifecycle_merge(&mut lifecycle, captured_lifecycle, &m.name)?;
                profiles.extend(captured_profiles);
                languages.extend(captured_languages);
                files.extend(captured_files);
                entries.push(((c.namespace, c.path.clone()), entry));
                secrets.extend(names);
                adapters.extend(found_adapters);
            }
            (Namespace::Env, ContribValue::Env(lit)) => {
                let (capture, services) =
                    evaluate_env_role(lit, src, base_dir, funcs, globals)?;
                let EnvCapture {
                    entry,
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    profiles: captured_profiles,
                    languages: captured_languages,
                    files: captured_files,
                } = capture;
                lifecycle_merge(&mut lifecycle, captured_lifecycle, &m.name)?;
                profiles.extend(captured_profiles);
                languages.extend(captured_languages);
                files.extend(captured_files);
                entries.push(((c.namespace, c.path.clone()), entry));
                dev_services.extend(services);
                secrets.extend(names);
                adapters.extend(found_adapters);
            }
            (Namespace::System, ContribValue::System(lit)) => {
                systems.push(evaluate_system(&c.path, lit, src, base_dir, funcs, globals)?);
            }
            (Namespace::Image, ContribValue::Image(lit)) => {
                images.push(evaluate_image(&c.path, lit, src, base_dir, funcs, globals)?);
            }
            (Namespace::Fleet, ContribValue::Fleet(lit)) => {
                fleets.push(evaluate_fleet(&c.path, lit, src, base_dir, funcs, globals)?);
            }
            (Namespace::VmTest, ContribValue::VmTest(lit)) => {
                vmtests.push(evaluate_vmtest(&c.path, lit, src, base_dir, funcs, globals)?);
            }
            // Namespace/value-shape mismatches can't occur: the parser pairs each
            // namespace with its dedicated value parser (see `contribution`).
            _ => unreachable!("contribution namespace/value shape mismatch"),
        }
    }
    for secret in integration_secrets {
        push_unique_string(&mut secrets, secret);
    }
    let environment_names = entries
        .iter()
        .filter_map(|((namespace, name), _)| {
            (*namespace == Namespace::Env).then_some(name.clone())
        })
        .collect();
    Ok(EvaluatedModule {
        name: m.name.clone(),
        entries,
        systems,
        images,
        fleets,
        vmtests,
        dev_services,
        secrets,
        adapters,
        lifecycle,
        profiles,
        languages,
        files,
        integrations,
        environment_names,
    })
}

fn push_unique_string(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

/// Lower first-party integration imports into the same typed facts consumed by
/// package realization, environment files, trust, and disclosure. Unknown
/// calls remain the existing E0969 import diagnostic; there is no stringly
/// escape hatch here.
fn evaluate_integration_imports(
    imports: &[Expr],
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<Vec<EnvironmentIntegration>, Diagnostic> {
    let mut leaves = Vec::new();
    for import in imports {
        collect_import_leaves(import, &mut leaves);
    }
    let mut integrations = Vec::new();
    for import in leaves {
        let Some(name) = qualified_call_name(import) else {
            continue;
        };
        if name == Syntax::BUILTIN_FIND {
            continue;
        }
        let Some(kind) = IntegrationKind::from_call(&name) else {
            return Err(super::Diagnostics::bad_import_directive(import.span()));
        };
        let (args, name_span) = match import {
            Expr::Call(call) => (call.args.as_slice(), call.name_span),
            Expr::MethodCall {
                args, method_span, ..
            } => (args.as_slice(), *method_span),
            _ => continue,
        };
        let integration = evaluate_integration_call(&name, args, kind, base_dir, funcs, globals)?;
        if let Some(existing) = integrations
            .iter()
            .find(|item: &&EnvironmentIntegration| item.name == integration.name)
        {
            if *existing != integration {
                return Err(Diagnostic::error(
                    "E1335",
                    format!("integration `{}` has conflicting declarations", integration.name),
                    "one environment graph cannot silently choose different SDK, host, credential, or grant facts".to_string(),
                    "merge the integration options so they agree, or keep one declaration".to_string(),
                    Some(name_span),
                ));
            }
        } else {
            integrations.push(integration);
        }
    }
    Ok(integrations)
}

fn collect_import_leaves<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    match expr {
        Expr::ListLit(items, _) => {
            for item in items {
                collect_import_leaves(item, out);
            }
        }
        _ => out.push(expr),
    }
}

fn evaluate_integration_call(
    name: &str,
    args: &[CallArg],
    kind: IntegrationKind,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<EnvironmentIntegration, Diagnostic> {
    let mut integration = EnvironmentIntegration {
        kind,
        name: name.to_string(),
        preset: kind.as_str().to_string(),
        ..Default::default()
    };
    match kind {
        IntegrationKind::Android => {
            integration.options.insert("api".into(), "34".into());
            integration.options.insert("build_tools".into(), "34.0.0".into());
            integration.options.insert("ndk".into(), "26.3".into());
            integration.options.insert("license".into(), "policy-required".into());
            integration.packages = vec!["android-tools@nixpkgs".into(), "android-sdk@nixpkgs".into()];
            integration.tasks.push("android-sdk-check".into());
            integration.providers.push("nixpkgs".into());
            integration.host_checks.push("target:linux-or-android".into());
        }
        IntegrationKind::Apple => {
            integration.options.insert("targets".into(), "IOS".into());
            integration.options.insert("license".into(), "policy-required".into());
            integration.packages.push("apple-sdk@nixpkgs".into());
            integration.tasks.push("apple-sdk-check".into());
            integration.providers.push("nixpkgs".into());
            integration.host_checks.push("target:darwin-or-macos".into());
        }
        IntegrationKind::Certificates => {
            integration.preset = "certificate-store".into();
            integration.tasks.push("certificate-store-check".into());
            integration.providers.push("vault".into());
            integration.grants.push("certificate.read".into());
        }
        IntegrationKind::Hosts => {
            integration.preset = "project-hosts".into();
            integration.tasks.push("host-binding-check".into());
            integration.providers.push("host-binding".into());
        }
        IntegrationKind::CodexAgent => {
            integration.preset = "codex".into();
            integration.tasks.push("mcp-agent-check".into());
            integration.providers.push("mcp".into());
            integration.grants.push("mcp.read".into());
        }
        IntegrationKind::Editor => {
            integration.preset = "vscode".into();
            integration.packages.push("vscode@nixpkgs".into());
            integration.providers.push("nixpkgs".into());
        }
        IntegrationKind::CloudCredentials => {
            integration.preset = "cloud-credentials".into();
            integration.tasks.push("credential-store-check".into());
            integration.providers.push("credential-store".into());
            integration.grants.push("credential.read".into());
        }
        IntegrationKind::Vault => {
            integration.preset = "vault".into();
            integration.tasks.push("vault-check".into());
            integration.providers.push("vault".into());
            integration.grants.push("vault.read".into());
        }
    }
    for (index, arg) in args.iter().enumerate() {
        let key = arg
            .label
            .as_ref()
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| format!("arg{index}"));
        let value = integration_arg_value(&arg.expr, base_dir, funcs, globals)?;
        lower_integration_arg(&mut integration, &key, &value);
    }
    Ok(integration)
}

fn integration_arg_value(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    match expr {
        Expr::Ident(name, _) => Ok(CtValue::Str(name.clone())),
        Expr::Field(base, member, _) => {
            Ok(CtValue::Str(format!("{}.{}", expression_name(base), member)))
        }
        Expr::ListLit(values, _) => values
            .iter()
            .map(|value| integration_arg_value(value, base_dir, funcs, globals))
            .collect::<Result<Vec<_>, _>>()
            .map(CtValue::List),
        Expr::MapLit(values, _) => {
            let mut lowered = BTreeMap::new();
            for (key, value) in values {
                let key = integration_arg_value(key, base_dir, funcs, globals)?;
                let value = integration_arg_value(value, base_dir, funcs, globals)?;
                let Some(key) = integration_ct_key(&key) else {
                    return Err(Diagnostic::error(
                        "E1335",
                        "integration map keys must be deterministic scalar values".to_string(),
                        "host mappings and integration selections become named graph facts; arbitrary values cannot be used as stable keys".to_string(),
                        "use a string, integer, boolean, or character key".to_string(),
                        Some(expr.span()),
                    ));
                };
                if lowered.insert(key, value).is_some() {
                    return Err(Diagnostic::error(
                        "E1335",
                        "integration map contains a duplicate key".to_string(),
                        "one integration fact graph cannot silently choose between two values for the same host or secret name".to_string(),
                        "remove the duplicate key or merge its values".to_string(),
                        Some(expr.span()),
                    ));
                }
            }
            Ok(CtValue::Map(lowered))
        }
        _ => {
            check_build_io(expr)?;
            Comptime::evaluate(expr, funcs, &HashSet::new(), base_dir, globals)
        }
    }
}

fn integration_ct_key(value: &CtValue) -> Option<CtKey> {
    match value {
        CtValue::Str(value) => Some(CtKey::Str(value.clone())),
        CtValue::Int(value) => Some(CtKey::Int(*value)),
        CtValue::Bool(value) => Some(CtKey::Bool(*value)),
        CtValue::Char(value) => Some(CtKey::Char(*value)),
        _ => None,
    }
}

fn expression_name(expr: &Expr) -> String {
    match expr {
        Expr::Ident(name, _) => name.clone(),
        Expr::Field(base, member, _) => format!("{}.{}", expression_name(base), member),
        _ => expr.span().start.to_string(),
    }
}

fn lower_integration_arg(integration: &mut EnvironmentIntegration, key: &str, value: &CtValue) {
    let display = integration_value_text(value);
    match integration.kind {
        IntegrationKind::Certificates
        | IntegrationKind::CloudCredentials
        | IntegrationKind::Vault => {
            let names = integration_names(value);
            if names.is_empty() {
                integration.losses.push(format!(
                    "{key}: secret input must be a named reference; value was redacted"
                ));
            } else {
                for name in names {
                    push_unique_string(&mut integration.secrets, name);
                }
            }
            integration
                .options
                .insert(key.to_string(), "<redacted-names>".into());
        }
        IntegrationKind::Hosts => {
            if let CtValue::Map(entries) = value {
                for (map_key, map_value) in entries {
                    let CtKey::Str(host) = map_key else { continue };
                    integration.options.insert(
                        format!("host.{host}"),
                        integration_value_text(map_value),
                    );
                }
            } else {
                integration.options.insert(key.to_string(), display);
            }
        }
        _ => {
            integration.options.insert(key.to_string(), display);
        }
    }
}

fn integration_names(value: &CtValue) -> Vec<String> {
    match value {
        CtValue::Str(value) => vec![value.clone()],
        CtValue::List(values) => values.iter().flat_map(integration_names).collect(),
        CtValue::Enum { variant, .. } => {
            vec![variant.rsplit('.').next().unwrap_or(variant).to_string()]
        }
        _ => Vec::new(),
    }
}

fn integration_value_text(value: &CtValue) -> String {
    match value {
        CtValue::Str(value) => value.clone(),
        CtValue::Int(value) => value.to_string(),
        CtValue::Bool(value) => value.to_string(),
        CtValue::Char(value) => value.to_string(),
        CtValue::Enum { variant, args, .. } if args.is_empty() => {
            variant.rsplit('.').next().unwrap_or(variant).to_string()
        }
        CtValue::List(values) => values
            .iter()
            .map(integration_value_text)
            .collect::<Vec<_>>()
            .join(","),
        CtValue::Map(values) => values
            .iter()
            .map(|(key, value)| format!("{}={}", integration_key_text(key), integration_value_text(value)))
            .collect::<Vec<_>>()
            .join(","),
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(name, value)| format!("{name}={}", integration_value_text(value)))
            .collect::<Vec<_>>()
            .join(","),
        _ => value.jet_show(),
    }
}

fn integration_key_text(key: &CtKey) -> String {
    match key {
        CtKey::Int(value) => value.to_string(),
        CtKey::Str(value) => value.clone(),
        CtKey::Bool(value) => value.to_string(),
        CtKey::Char(value) => value.to_string(),
    }
}

fn lifecycle_merge(
    target: &mut EnvironmentLifecycle,
    incoming: EnvironmentLifecycle,
    module_name: &str,
) -> Result<(), Diagnostic> {
    target.dotenv.extend(incoming.dotenv);
    target.unset.extend(incoming.unset);
    target.on_enter.extend(incoming.on_enter);
    target.checks.extend(incoming.checks);
    if incoming.reload_explicit {
        if target.reload_explicit && target.reload != incoming.reload {
            return Err(Diagnostic::error(
                "E1333",
                format!("reload policy is declared more than once in module `{module_name}`"),
                "one module cannot silently choose between different reload policies".to_string(),
                "merge the reload declarations so they agree, or keep one policy owner".to_string(),
                None,
            ));
        }
        target.reload = incoming.reload;
        target.reload_explicit = true;
    }
    Ok(())
}

struct EnvCapture {
    entry: EntryContribution,
    secrets: Vec<String>,
    adapters: Vec<AdapterPlan>,
    lifecycle: EnvironmentLifecycle,
    profiles: Vec<ProfileSpec>,
    languages: Vec<LanguageSpec>,
    files: Vec<super::Environment::ManagedFile>,
}

fn namespace_type(ns: Namespace) -> &'static str {
    match ns {
        Namespace::Env => Syntax::TYPE_ENV,
        Namespace::System => Syntax::TYPE_SYSTEM,
        Namespace::Image => Syntax::TYPE_IMAGE,
        Namespace::Fleet => Syntax::TYPE_FLEET,
        Namespace::VmTest => Syntax::TYPE_VMTEST,
        Namespace::Perf => Syntax::TYPE_BUDGET,
    }
}

/// E3402: builtins that perform ambient I/O or network access. A sandboxed
/// package build / module-field evaluation must reach none of them. The fs/net
/// names are future-proofing for the build-from-source path; print/eprint/input
/// are reachable today, so the diagnostic is live (I4).
fn is_ambient_io_builtin(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "write_file"
            | "fetch"
            | "http_get"
            | "env_var"
            | "print"
            | "eprint"
            | "input"
            | "read_all_input"
    )
}

/// Scan a field expression for an ambient-I/O call; the first one fires E3402.
pub(super) fn check_build_io(value: &Expr) -> Result<(), Diagnostic> {
    let mut found: Option<(String, crate::Diagnostics::Span)> = None;
    crate::Comptime::walk_calls(value, &mut |name, span| {
        if found.is_none() && is_ambient_io_builtin(name) {
            found = Some((name.to_string(), span));
        }
    });
    match found {
        Some((name, span)) => Err(crate::Sema::e3402(&name, Some(span))),
        None => Ok(()),
    }
}

fn evaluate_env_contribution(
    c: &Contribution,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<EnvCapture, Diagnostic> {
    let expected = namespace_type(c.namespace);
    let ContribValue::Expr(value) = &c.value else {
        unreachable!("evaluate_env_contribution called on a non-env contribution")
    };
    // U18: a bare `{ … }` elaborates to the namespace type; an explicit
    // `Env { … }` stays legal. A non-record value (or the wrong type name) is
    // E0966.
    let Expr::StructLit {
        type_name, fields, ..
    } = value
    else {
        return Err(not_a_namespace_literal(expected, value.span()));
    };
    if !type_name.is_empty() && type_name != expected {
        return Err(wrong_namespace_type(expected, type_name, value.span()));
    }

    Ok(evaluate_env_fields(fields, src, base_dir, funcs, globals)?)
}

/// Shared field-loop for both `env.<name>:` producer shapes (the legacy
/// `Expr::StructLit`'s fields and the canonical role-module's `EnvLit::fields`):
/// `packages:` reuses the Pkg-sugar text slice; everything else is a pure
/// comptime scalar setting.
fn evaluate_env_fields(
    fields: &[(String, crate::Diagnostics::Span, Expr)],
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<EnvCapture, Diagnostic> {
    let mut entry = EntryContribution::default();
    let mut secrets = Vec::new();
    let mut adapters = Vec::new();
    let mut lifecycle = EnvironmentLifecycle::default();
    let mut profiles = Vec::new();
    let mut languages = Vec::new();
    let mut files = Vec::new();
    let extern_names = HashSet::new();
    // Module fields are a small pure dependency graph.  The old source-order
    // loop made a valid `port: base + 1, base: 8000` fail or, worse, made the
    // result depend on declaration order.  Resolve every sibling dependency
    // first, then evaluate the field with the resolved values in its globals.
    // `packages` remains text-level sugar and is deliberately not exposed as a
    // comptime value.
    let field_map = fields
        .iter()
        .filter(|(name, _, _)| name != &Syntax::SYSTEM_FIELD_PACKAGES)
        .map(|(name, span, value)| (name.clone(), (*span, value)))
        .collect::<HashMap<_, _>>();
    for (name, _, value) in fields {
        if name == Syntax::SYSTEM_FIELD_PACKAGES {
            let extracted = extract_packages(value, src)?;
            entry.packages.extend(extracted.packages);
            adapters.extend(extracted.adapters);
        }
    }
    let computed = evaluate_named_fields(
        &field_map,
        globals,
        funcs,
        &extern_names,
        base_dir,
        Some(src),
        "module fields are pure computed values; a cycle has no deterministic evaluation order",
        "break the cycle by making one field a literal or by moving the shared computation into a pure function",
    )?;
    let resolved = computed.values;
    for (name, span, value) in fields {
        if name == Syntax::SYSTEM_FIELD_PACKAGES {
            continue;
        } else if name == Syntax::ENV_FIELD_PROMPT {
            capture_prompt_setting(
                &mut entry,
                value,
                base_dir,
                funcs,
                &resolved,
            )?;
        } else if name == Syntax::ENV_FIELD_SECRETS {
            // U13: `secrets: ["name", …]` — a plain list of strings, no Pkg
            // sugar. Evaluated as an ordinary comptime expression; anything
            // that isn't a `[String]` is captured as a scalar setting instead
            // (E1242-style "wrong shape" surfaces at env-entry validation,
            // not here — this stays a pure capture step, no field-check).
            let v = resolved
                .get(name)
                .cloned()
                .ok_or_else(|| field_missing_value(name, value.span()))?;
            match names_from(&v) {
                Some(names) => secrets.extend(names),
                None => {
                    entry
                        .settings
                        .entry(name.clone())
                        .or_default()
                        .push(Scalar::normal(v.jet_show()));
                }
            }
        } else if name == Syntax::ENV_FIELD_PROFILES {
            if let Some(value) = resolved.get(name) {
                profiles.extend(profiles_from_value(value).map_err(|error| {
                    Diagnostic::error(
                        "E1332",
                        format!("environment profile declaration is invalid: {error}"),
                        "profiles are named typed records with string package, variable, and inheritance facts".to_string(),
                        "use `profiles: { dev: .{ packages: [\"tool@nixpkgs\"] } }`".to_string(),
                        Some(*span),
                    )
                })?);
            }
        } else if name == Syntax::ENV_FIELD_LANGUAGES {
            if let Some(value) = resolved.get(name) {
                languages.extend(languages_from_value(value).map_err(|error| {
                    Diagnostic::error(
                        "E1333",
                        format!("language-pack declaration is invalid: {error}"),
                        "languages is a typed map of Lang records that expands through Jet's catalog".to_string(),
                        "use `languages: { rust: Lang.{ enable: true } }` or another name from `jet env info`".to_string(),
                        Some(*span),
                    )
                })?);
            }
        } else if name == Syntax::ENV_FIELD_FILES {
            if let Some(value) = resolved.get(name) {
                files.extend(files_from_value(value).map_err(|error| {
                    Diagnostic::error(
                        "E1326",
                        format!("environment file declaration is invalid: {error}"),
                        "managed files use project-relative destinations and typed Symlink, Seed, or Copy records".to_string(),
                        "fix the file destination and source/content shape before running `jet env sync`".to_string(),
                        Some(*span),
                    )
                })?);
            }
        } else if matches!(
            name.as_str(),
            Syntax::ENV_FIELD_DOTENV
                | Syntax::ENV_FIELD_UNSET
                | Syntax::ENV_FIELD_ON_ENTER
                | Syntax::ENV_FIELD_CHECKS
                | Syntax::ENV_FIELD_RELOAD
        ) {
            if let Some(value) = resolved.get(name) {
                lifecycle_from_field(&mut lifecycle, name, value).map_err(|error| {
                    Diagnostic::error(
                        "E1333",
                        format!("environment lifecycle declaration is invalid: {error}"),
                        "lifecycle fields use typed dotenv, unset, hook, and reload records".to_string(),
                        "fix the field shape, for example `dotenv: [\".env\"]` or `reload: .Prompt`".to_string(),
                        Some(*span),
                    )
                })?;
            }
        } else {
            let v = resolved
                .get(name)
                .cloned()
                .ok_or_else(|| field_missing_value(name, value.span()))?;
            entry
                .settings
                .entry(name.clone())
                .or_default()
                .push(Scalar::normal(v.jet_show()));
        }
    }
    Ok(EnvCapture {
        entry,
        secrets,
        adapters,
        lifecycle,
        profiles,
        languages,
        files,
    })
}

fn field_missing_value(name: &str, span: crate::Diagnostics::Span) -> Diagnostic {
    Diagnostic::error(
        "E3403",
        format!("computed module field `{name}` did not produce a value"),
        "a pure module field must evaluate to one deterministic value before the module is merged".to_string(),
        "return a value from the field expression".to_string(),
        Some(span),
    )
}

fn capture_prompt_setting(
    entry: &mut EntryContribution,
    value: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<(), Diagnostic> {
    if let Expr::StructLit {
        type_name, fields, ..
    } = value
    {
        if type_name == Syntax::TYPE_PROMPT {
            let extern_names = HashSet::new();
            for (field, span, expr) in fields {
                check_build_io(expr)?;
                let v = Comptime::evaluate(expr, funcs, &extern_names, base_dir, globals)?;
                match field.as_str() {
                    Syntax::PROMPT_FIELD_LABEL => {
                        let Some(label) = string_value(&v) else {
                            return Err(prompt_bad_value(field, "a quoted label", *span));
                        };
                        entry
                            .settings
                            .entry(Syntax::ENV_FIELD_PROMPT.to_string())
                            .or_default()
                            .push(Scalar::normal(label));
                    }
                    Syntax::PROMPT_FIELD_PATH => {
                        let Some(word) = prompt_word(&v) else {
                            return Err(prompt_bad_value(field, ".Short or .Full", *span));
                        };
                        if word != Syntax::PROMPT_PATH_SHORT && word != Syntax::PROMPT_PATH_FULL {
                            return Err(prompt_bad_value(field, ".Short or .Full", *span));
                        }
                        entry
                            .settings
                            .entry(Syntax::PROMPT_SETTING_PATH.to_string())
                            .or_default()
                            .push(Scalar::normal(word));
                    }
                    Syntax::PROMPT_FIELD_STRIP => {
                        let Some(word) = prompt_word(&v) else {
                            return Err(prompt_bad_value(field, ".On or .Off", *span));
                        };
                        if word != Syntax::PROMPT_STRIP_ON && word != Syntax::PROMPT_STRIP_OFF {
                            return Err(prompt_bad_value(field, ".On or .Off", *span));
                        }
                        entry
                            .settings
                            .entry(Syntax::PROMPT_SETTING_STRIP.to_string())
                            .or_default()
                            .push(Scalar::normal(word));
                    }
                    _ => return Err(prompt_bad_field(field, *span)),
                }
            }
            return Ok(());
        }
    }

    check_build_io(value)?;
    let extern_names = HashSet::new();
    let v = Comptime::evaluate(value, funcs, &extern_names, base_dir, globals)?;
    entry
        .settings
        .entry(Syntax::ENV_FIELD_PROMPT.to_string())
        .or_default()
        .push(Scalar::normal(v.jet_show()));
    Ok(())
}

fn string_value(v: &crate::Comptime::CtValue) -> Option<String> {
    match v {
        crate::Comptime::CtValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn prompt_word(v: &crate::Comptime::CtValue) -> Option<String> {
    match v {
        crate::Comptime::CtValue::Enum { variant, .. } => {
            variant.rsplit('.').next().map(str::to_string)
        }
        crate::Comptime::CtValue::Str(s) => Some(s.clone()),
        _ => None,
    }
}

/// U13: `[String]` → `Vec<String>`, or `None` if `v` isn't a list of strings
/// (caller falls back to capturing it as an opaque scalar setting).
fn names_from(v: &crate::Comptime::CtValue) -> Option<Vec<String>> {
    let crate::Comptime::CtValue::List(xs) = v else {
        return None;
    };
    xs.iter()
        .map(|x| match x {
            crate::Comptime::CtValue::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// U12/D-JPK-MODBODY1=A: evaluate a canonical `env.<name>: { … }` role-module
/// body (`EnvLit`) — the same scalar/package fields as the legacy form, plus a
/// dev-supervised `services:` map, field-checked and captured as
/// `DevServicePlan`s (distinct from the jetos `system.*.services` capture).
fn evaluate_env_role(
    lit: &EnvLit,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<
    (
        EnvCapture,
        Vec<DevServicePlan>,
    ),
    Diagnostic,
> {
    let capture = evaluate_env_fields(&lit.fields, src, base_dir, funcs, globals)?;
    let mut services = Vec::new();
    for s in &lit.services {
        services.push(evaluate_dev_service(s, base_dir, funcs, globals)?);
    }
    Ok((capture, services))
}

/// Pull the package list out of a `packages: [ … ]` field by slicing its
/// source text and reusing the tested sugar parser (see module docs).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ExtractedPackages {
    pub packages: Vec<Merge::Pkg>,
    pub adapters: Vec<AdapterPlan>,
}

pub(super) fn extract_packages(value: &Expr, src: &str) -> Result<ExtractedPackages, Diagnostic> {
    let Expr::ListLit(_, span) = value else {
        return Err(packages_not_a_list(value.span()));
    };
    let text = &src[span.start..span.end];
    let body = text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .unwrap_or(text);
    let mut out = ExtractedPackages::default();
    let mut plain = Vec::new();
    for item in split_top_level(body) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if item.starts_with("Pkg.adapt") {
            out.adapters.push(parse_adapter(item)?);
        } else {
            plain.push(item);
        }
    }
    out.packages = Merge::parse_package_list(&plain.join(", "));
    Ok(out)
}

fn split_top_level(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, c) in body.char_indices() {
        if in_string {
            escape = c == '\\' && !escape;
            if c == '"' && !escape {
                in_string = false;
            } else if c != '\\' {
                escape = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < body.len() {
        out.push(&body[start..]);
    }
    out
}

fn parse_adapter(item: &str) -> Result<AdapterPlan, Diagnostic> {
    let args = call_args(item, "Pkg.adapt").ok_or_else(|| adapter_shape(item))?;
    let name = named_string(args, "name").ok_or_else(|| adapter_shape(item))?;
    let source = named_raw(args, "source").ok_or_else(|| adapter_shape(item))?;
    let source = unquote(source.trim()).unwrap_or(source);
    super::super::RefSpec::classify_provider_ref(&source).map_err(|_| adapter_shape(item))?;
    let deps = named_raw(args, "deps")
        .map(|raw| {
            let body = raw
                .trim()
                .strip_prefix('[')
                .and_then(|t| t.strip_suffix(']'))
                .unwrap_or(raw.trim());
            Merge::parse_package_list(body)
        })
        .unwrap_or_default();
    let recipe = parse_recipe(&named_raw(args, "recipe").ok_or_else(|| adapter_shape(item))?)?;
    Ok(AdapterPlan {
        name,
        source,
        deps,
        recipe,
    })
}

fn parse_recipe(raw: &str) -> Result<AdapterRecipe, Diagnostic> {
    let raw = raw.trim();
    if raw.starts_with("Recipe.copy") {
        return Ok(AdapterRecipe::Copy);
    }
    if let Some(args) = call_args(raw, "Recipe.prebuilt") {
        let bin = named_string(args, "bin").ok_or_else(|| adapter_shape(raw))?;
        let as_name = named_string(args, "as")
            .or_else(|| named_string(args, "as_name"))
            .unwrap_or_else(|| {
                std::path::Path::new(&bin)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&bin)
                    .to_string()
            });
        return Ok(AdapterRecipe::Prebuilt { bin, as_name });
    }
    Err(adapter_shape(raw))
}

fn call_args<'a>(raw: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = raw.trim().strip_prefix(prefix)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    let end = rest.rfind(')')?;
    Some(&rest[..end])
}

fn named_raw(args: &str, key: &str) -> Option<String> {
    let needle = format!("{key}:");
    for item in split_top_level(args) {
        let item = item.trim();
        if let Some(rest) = item.strip_prefix(&needle) {
            return Some(rest.trim().trim_end_matches(',').to_string());
        }
    }
    None
}

fn named_string(args: &str, key: &str) -> Option<String> {
    let raw = named_raw(args, key)?;
    unquote(raw.trim())
}

fn unquote(raw: &str) -> Option<String> {
    let s = raw.strip_prefix('"')?.strip_suffix('"')?;
    Some(s.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn adapter_shape(_raw: &str) -> Diagnostic {
    Diagnostic::error(
        "E1270",
        "adapter package declaration is not complete".to_string(),
        "`Pkg.adapt` needs `name:`, `source:`, and a supported `recipe:`; this U20 slice supports `Recipe.copy()` and `Recipe.prebuilt(bin:, as:)`.".to_string(),
        "write `Pkg.adapt(name: \"tool\", source: \"./vendor/tool\", recipe: Recipe.copy())`.".to_string(),
        None,
    )
}

/// Merge every evaluated module's contributions through the §6 engine,
/// grouping by `(namespace, path)` first.
pub fn merge_all(
    modules: &[EvaluatedModule],
) -> Result<HashMap<(Namespace, String), MergedEntry>, MergeError> {
    let mut by_key: HashMap<(Namespace, String), Vec<EntryContribution>> = HashMap::new();
    for module in modules {
        for (key, entry) in &module.entries {
            by_key.entry(key.clone()).or_default().push(entry.clone());
        }
    }
    let mut out = HashMap::new();
    for (key, contribs) in by_key {
        out.insert(key, Merge::merge_entry(&contribs)?);
    }
    Ok(out)
}

/// Render one merged `Pkg` as a `<package>@<source>` ref. A bare package (the
/// sugar's empty source) resolves against the conventional `default` source.
pub fn pkg_ref(pkg: &Merge::Pkg) -> String {
    let source = if pkg.source.is_empty() {
        Syntax::DEFAULT_SOURCE
    } else {
        pkg.source.as_str()
    };
    format!("{}{}{}", pkg.name, Syntax::REF_PROVIDER_AT, source)
}
