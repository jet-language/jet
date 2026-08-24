//! The env-namespace evaluation core: parse a source to a `Program`, evaluate
//! every enabled `Item::Module`, run each `env.*` field through `comptime` (or
//! the text-level Pkg-sugar parser for `packages:`), and merge the resulting
//! contributions through the §6 engine.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use crate::Comptime;
use crate::Diagnostics::Diagnostic;
use crate::Lexer::{StrTokPart, TokKind, Token};
use crate::Syntax;
use crate::AST::{
    CallArg, ContribValue, Contribution, CtKey, CtValue, EnvLit, Expr, Func, Item, ModuleDecl,
    Namespace, StrPart,
};

use super::super::Merge::{
    self, ContributionLayer, EntryContribution, FactContribution, FactValue, MergeError,
    MergedEntry, SourceScope,
};
use super::DevService::evaluate_dev_service;
use super::Environment::{
    files_from_value, lifecycle_from_field, languages_from_value, presets_from_value,
    qualified_call_name, EnvironmentIntegration, EnvironmentLifecycle, IntegrationKind,
    valid_env_name, LanguageSpec, PackageProfileSpec, PresetSpec,
};
use super::Diagnostics::{
    not_a_namespace_literal, packages_not_a_list, prompt_bad_field, prompt_bad_value,
    wrong_namespace_type,
};
use super::Computed::evaluate_named_fields;
use super::System::{evaluate_fleet, evaluate_image, evaluate_system, evaluate_vmtest};
use super::Types::{
    AdapterPlan, AdapterRecipe, DevServicePlan, EnvironmentContribution, EvaluatedModule,
    SecretDeclaration, SecretDefault, SecretGenerator, SecretRotationPolicy, SecretSpec,
};
use super::Types::EnvironmentRead;

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
    let (toks, lex_diags) = crate::Lexer::lex_config(src);
    if let Some(d) = lex_diags.into_iter().next() {
        return Err(d);
    }
    crate::Parser::parse_config_with_source(&toks, src).map_err(|mut diags| {
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

/// Read the one config-surface spelling of environment access. The lexer has
/// already separated `$` from the following identifier, so quoted shell text
/// such as `"echo $HOME"` is not mistaken for a Jet environment read. String
/// interpolation token streams are walked recursively because they are parsed
/// as expressions by the config parser.
pub(super) fn environment_reads(src: &str) -> Result<Vec<EnvironmentRead>, Diagnostic> {
    let (tokens, lex_diags) = crate::Lexer::lex(src);
    if let Some(diagnostic) = lex_diags.into_iter().next() {
        return Err(diagnostic);
    }
    let mut reads = Vec::new();
    collect_environment_reads(&tokens, &mut reads);
    Ok(reads)
}

fn collect_environment_reads(tokens: &[Token], reads: &mut Vec<EnvironmentRead>) {
    let tokens = crate::Lexer::without_comments(tokens);
    for (index, token) in tokens.iter().enumerate() {
        if matches!(&token.kind, TokKind::Dollar) {
            if let Some(Token {
                kind: TokKind::Ident(name),
                ..
            }) = tokens.get(index + 1)
            {
                let name = format!("${name}");
                if !reads.iter().any(|read| read.name == name) {
                    reads.push(EnvironmentRead {
                        name,
                        ty: Syntax::TYPE_STRING.to_string(),
                    });
                }
            }
        }
        if let TokKind::Str(parts) = &token.kind {
            for part in parts {
                if let StrTokPart::Interp(interpolation) = part {
                    collect_environment_reads(interpolation, reads);
                }
            }
        }
    }
}

fn environment_globals(reads: &[EnvironmentRead]) -> HashMap<String, CtValue> {
    reads
        .iter()
        .map(|read| {
            let name = read.name.strip_prefix('$').unwrap_or(read.name.as_str());
            let value = std::env::var_os(name)
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            (read.name.clone(), CtValue::Str(value))
        })
        .collect()
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
    let reads = environment_reads(src)?;
    let globals = collect_comptime_globals(
        items,
        &funcs,
        base_dir,
        &environment_globals(&reads),
    )?;
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
    environment_globals: &HashMap<String, CtValue>,
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
        environment_globals,
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
    let mut environment_contributions = Vec::new();
    let mut package_profiles = Vec::new();
    let integrations = evaluate_integration_imports(&m.imports, base_dir, funcs, globals)?;
    for c in &m.contributions {
        match (&c.namespace, &c.value) {
            (Namespace::Env, ContribValue::Expr(_)) => {
                let source = format!("{}.{}", m.name, c.path);
                let capture = evaluate_env_contribution(
                    c,
                    src,
                    base_dir,
                    funcs,
                    globals,
                    &source,
                )?;
                let EnvCapture {
                    entry,
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    presets: captured_presets,
                    languages: captured_languages,
                    files: captured_files,
                } = capture;
                let environment_reads = environment_reads(&src[c.span.start..c.span.end])?;
                environment_contributions.push(EnvironmentContribution {
                    name: c.path.clone(),
                    environment_reads,
                    dev_services: Vec::new(),
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    presets: captured_presets,
                    languages: captured_languages,
                    files: captured_files,
                });
                entries.push(((c.namespace, c.path.clone()), entry));
            }
            (Namespace::Env, ContribValue::Env(lit)) => {
                let source = format!("{}.{}", m.name, c.path);
                let (capture, services) = evaluate_env_role(
                    lit,
                    src,
                    base_dir,
                    funcs,
                    globals,
                    &source,
                )?;
                let EnvCapture {
                    entry,
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    presets: captured_presets,
                    languages: captured_languages,
                    files: captured_files,
                } = capture;
                let environment_reads = environment_reads(&src[c.span.start..c.span.end])?;
                environment_contributions.push(EnvironmentContribution {
                    name: c.path.clone(),
                    environment_reads,
                    dev_services: services,
                    secrets: names,
                    adapters: found_adapters,
                    lifecycle: captured_lifecycle,
                    presets: captured_presets,
                    languages: captured_languages,
                    files: captured_files,
                });
                entries.push(((c.namespace, c.path.clone()), entry));
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
            (Namespace::Profile, ContribValue::Profile(lit)) => {
                package_profiles.push(evaluate_package_profile_fields(
                    &c.path,
                    &lit.fields,
                    src,
                    base_dir,
                    funcs,
                    globals,
                    &m.name,
                )?);
            }
            (Namespace::Profile, ContribValue::Expr(value)) => {
                package_profiles.push(evaluate_package_profile_expr(
                    &c.path,
                    value,
                    src,
                    base_dir,
                    funcs,
                    globals,
                    &m.name,
                )?);
            }
            // Namespace/value-shape mismatches can't occur: the parser pairs each
            // namespace with its dedicated value parser (see `contribution`).
            _ => unreachable!("contribution namespace/value shape mismatch"),
        }
    }
    Ok(EvaluatedModule {
        name: m.name.clone(),
        entries,
        systems,
        images,
        fleets,
        vmtests,
        environment_contributions,
        integrations,
        package_profiles,
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
        let secret_reference = matches!(
            kind,
            IntegrationKind::Certificates
                | IntegrationKind::CloudCredentials
                | IntegrationKind::Vault
        )
        .then(|| integration_reference_names(&arg.expr));
        // Do not evaluate an invalid secret expression. Comptime values can
        // contain the caller's plaintext, while the only accepted secret
        // shape is a named reference already visible in the AST.
        let value = match secret_reference {
            Some(Err(())) => CtValue::Int(0),
            _ => integration_arg_value(&arg.expr, base_dir, funcs, globals)?,
        };
        lower_integration_arg(&mut integration, &key, &arg.expr, &value);
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

fn lower_integration_arg(
    integration: &mut EnvironmentIntegration,
    key: &str,
    expr: &Expr,
    value: &CtValue,
) {
    let display = integration_value_text(value);
    match integration.kind {
        IntegrationKind::Certificates
        | IntegrationKind::CloudCredentials
        | IntegrationKind::Vault => {
            let names = integration_reference_names(expr).unwrap_or_default();
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

fn integration_reference_names(expr: &Expr) -> Result<Vec<String>, ()> {
    match expr.without_parens() {
        Expr::Ident(name, _) => Ok(vec![name.clone()]),
        Expr::Field(_, _, _) => integration_reference_path(expr)
            .map(|name| vec![name])
            .ok_or(()),
        Expr::ListLit(values, _) => {
            let mut names = Vec::new();
            for value in values {
                names.extend(integration_reference_names(value)?);
            }
            Ok(names)
        }
        _ => Err(()),
    }
}

fn integration_reference_path(expr: &Expr) -> Option<String> {
    match expr.without_parens() {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, member, _) => {
            Some(format!("{}.{}", integration_reference_path(base)?, member))
        }
        _ => None,
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
        // D-MAP-KEY1: a composite key is value-semantic, so its text is the
        // rendering of the value it holds. An integration key is a display
        // string, not an identity, so deferring to jet_show keeps one
        // rendering rule rather than a second one written here.
        CtKey::Tuple(_) | CtKey::Struct { .. } | CtKey::Enum { .. } => {
            key.to_value().jet_show()
        }
    }
}

pub(super) fn lifecycle_merge(
    target: &mut EnvironmentLifecycle,
    incoming: EnvironmentLifecycle,
    module_name: &str,
) -> Result<(), Diagnostic> {
    target.dotenv.extend(incoming.dotenv);
    target.unset.extend(incoming.unset);
    target.on_enter.extend(incoming.on_enter);
    target.checks.extend(incoming.checks);
    if let Some(path) = incoming.git_hooks_path {
        if let Some(existing) = &target.git_hooks_path {
            if existing != &path {
                return Err(Diagnostic::error(
                    "E1333",
                    format!(
                        "git hook path is declared more than once with conflicting values in module `{module_name}`"
                    ),
                    "one environment cannot silently choose between different Git hook directories".to_string(),
                    "merge the git_hooks_path declarations so they agree, or keep one path owner".to_string(),
                    None,
                ));
            }
        } else {
            target.git_hooks_path = Some(path);
        }
    }
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
    if let Some(formatter) = incoming.formatter {
        if let Some(existing) = &target.formatter {
            if existing != &formatter {
                return Err(Diagnostic::error(
                    "E1333",
                    format!("formatter is declared more than once with conflicting packages in module `{module_name}`"),
                    "one environment cannot silently choose between different external formatters".to_string(),
                    "merge the formatter declarations so they agree, or keep one formatter owner".to_string(),
                    None,
                ));
            }
        } else {
            target.formatter = Some(formatter);
        }
    }
    Ok(())
}

struct EnvCapture {
    entry: EntryContribution,
    secrets: Vec<SecretSpec>,
    adapters: Vec<AdapterPlan>,
    lifecycle: EnvironmentLifecycle,
    presets: Vec<PresetSpec>,
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
        Namespace::Profile => Syntax::TYPE_PROFILE,
        Namespace::Perf => Syntax::TYPE_BUDGET,
    }
}

fn evaluate_package_profile_expr(
    name: &str,
    value: &Expr,
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
    module_name: &str,
) -> Result<PackageProfileSpec, Diagnostic> {
    let Expr::StructLit {
        type_name, fields, ..
    } = value
    else {
        return Err(not_a_namespace_literal(Syntax::TYPE_PROFILE, value.span()));
    };
    if !type_name.is_empty() && type_name != Syntax::TYPE_PROFILE {
        return Err(wrong_namespace_type(Syntax::TYPE_PROFILE, type_name, value.span()));
    }
    evaluate_package_profile_fields(
        name,
        fields,
        src,
        base_dir,
        funcs,
        globals,
        module_name,
    )
}

fn evaluate_package_profile_fields(
    name: &str,
    fields: &[(String, crate::Diagnostics::Span, Expr)],
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
    module_name: &str,
) -> Result<PackageProfileSpec, Diagnostic> {
    let allowed = [
        Syntax::PROFILE_FIELD_EXTENDS,
        Syntax::PROFILE_FIELD_PACKAGES,
        Syntax::PROFILE_FIELD_COLLISIONS,
    ];
    let mut seen = HashSet::new();
    for (field, span, _) in fields {
        if !allowed.iter().any(|allowed| *allowed == field) {
            return Err(Diagnostic::error(
                "E1333",
                format!("package generation `{name}` has unknown field `{field}`"),
                "a source-backed package generation has only `extends`, `packages`, and `collisions` facts".to_string(),
                "remove the field or move user/environment settings into a declared `user.<name>` generation".to_string(),
                Some(*span),
            ));
        }
        if !seen.insert(field) {
            return Err(Diagnostic::error(
                "E1332",
                format!("package generation `{name}` repeats field `{field}`"),
                "one package generation must have one value for each typed fact".to_string(),
                "keep one declaration for the field, or split the generations and compose them with `extends`".to_string(),
                Some(*span),
            ));
        }
    }
    let field_map = fields
        .iter()
        .filter(|(field, _, _)| {
            field != Syntax::PROFILE_FIELD_PACKAGES
                && field != Syntax::PROFILE_FIELD_COLLISIONS
        })
        .map(|(field, span, value)| (field.clone(), (*span, value)))
        .collect::<HashMap<_, _>>();
    let computed = evaluate_named_fields(
        &field_map,
        globals,
        funcs,
        &HashSet::new(),
        base_dir,
        Some(src),
        "package generation fields are pure computed values; a cycle has no deterministic evaluation order",
        "break the cycle by making one generation field a literal or by moving the shared computation into a pure function",
    )?;
    let resolved = computed.values;
    let mut packages = Vec::new();
    for (field, _, value) in fields {
        if field != Syntax::PROFILE_FIELD_PACKAGES {
            continue;
        }
        let extracted = extract_packages(value, src)?;
        for package in extracted.packages {
            let raw = pkg_ref(&package);
            if !packages.iter().any(|existing| existing == &raw) {
                packages.push(raw);
            }
        }
        if !extracted.adapters.is_empty() {
            return Err(Diagnostic::error(
                "E1335",
                format!("package generation `{name}` contains an adapter package"),
                "generation packages must retain provider identity and cannot hide a build recipe inside the generation declaration".to_string(),
                "realize the adapter as a named package, then reference its exact package identity".to_string(),
                Some(value.span()),
            ));
        }
    }
    let extends = match resolved.get(Syntax::PROFILE_FIELD_EXTENDS) {
        None => Vec::new(),
        Some(value) => string_names_from(value).ok_or_else(|| {
            Diagnostic::error(
                "E1333",
                format!("package generation `{name}` has invalid `extends`"),
                "generation inheritance is a deterministic list of generation names".to_string(),
                "write `extends: [\"base\"]`".to_string(),
                Some(value_span(fields, Syntax::PROFILE_FIELD_EXTENDS)),
            )
        })?,
    };
    let collisions = fields
        .iter()
        .find(|(field, _, _)| field == Syntax::PROFILE_FIELD_COLLISIONS)
        .map(|(_, _, value)| package_profile_collisions_source(src, name, value.span()))
        .transpose()?
        .unwrap_or_default();
    Ok(PackageProfileSpec {
        name: name.to_string(),
        extends,
        packages,
        collisions,
        sources: vec![module_name.to_string()],
    })
}

fn value_span(
    fields: &[(String, crate::Diagnostics::Span, Expr)],
    name: &str,
) -> crate::Diagnostics::Span {
    fields
        .iter()
        .find(|(field, _, _)| field == name)
        .map(|(_, span, _)| *span)
        .unwrap_or_else(|| crate::Diagnostics::Span::new(0, 0))
}

fn package_profile_collisions_source(
    src: &str,
    name: &str,
    span: crate::Diagnostics::Span,
) -> Result<BTreeMap<String, String>, Diagnostic> {
    let Some(fragment) = src.get(span.start..span.end) else {
        return Err(Diagnostic::error(
            "E1333",
            format!("package generation `{name}` has invalid `collisions`"),
            "collision policy is an exact string path-to-provider map".to_string(),
            "write `collisions: { \"bin/editor\": \"editor@nixpkgs\" }`".to_string(),
            Some(span),
        ));
    };
    let values = quoted_collision_values(fragment);
    if values.is_empty() || values.len() % 2 != 0 {
        return Err(Diagnostic::error(
            "E1333",
            format!("package generation `{name}` has invalid `collisions`"),
            "collision policy is an exact string path-to-provider map".to_string(),
            "write `collisions: { \"bin/editor\": \"editor@nixpkgs\" }`".to_string(),
            Some(span),
        ));
    }
    let mut out = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        let path = &pair[0];
        let provider = &pair[1];
        if path.trim().is_empty() || provider.trim().is_empty() || path.contains('\0') {
            return Err(Diagnostic::error(
                "E1333",
                format!("package generation `{name}` has an invalid collision entry"),
                "exact collision paths and provider identities cannot be empty or contain NUL".to_string(),
                "name the path and the exact package provider".to_string(),
                Some(span),
            ));
        }
        if out.insert(path.clone(), provider.clone()).is_some() {
            return Err(Diagnostic::error(
                "E1332",
                format!("package generation `{name}` repeats collision path `{path}`"),
                "one generation cannot choose two providers for one exact path".to_string(),
                "keep one collision selection for the path".to_string(),
                Some(span),
            ));
        }
    }
    Ok(out)
}

fn quoted_collision_values(fragment: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = fragment.chars();
    while let Some(character) = chars.next() {
        if character != '"' {
            continue;
        }
        let mut value = String::new();
        let mut escaped = false;
        let mut closed = false;
        for character in chars.by_ref() {
            if escaped {
                value.push(match character {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '\\' => '\\',
                    '"' => '"',
                    other => other,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                closed = true;
                break;
            } else {
                value.push(character);
            }
        }
        if !closed {
            return Vec::new();
        }
        values.push(value);
    }
    values
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
    source: &str,
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

    Ok(evaluate_env_fields(
        fields, src, base_dir, funcs, globals, source,
    )?)
}

/// Shared field-loop for both `env.<name>:` producer shapes (the legacy
/// `Expr::StructLit`'s fields and the canonical role-module's `EnvLit::fields`):
/// `packages:` reuses the Pkg-sugar text slice; everything else is a pure
/// comptime fact setting.
fn evaluate_env_fields(
    fields: &[(String, crate::Diagnostics::Span, Expr)],
    src: &str,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
    source: &str,
) -> Result<EnvCapture, Diagnostic> {
    let mut entry = EntryContribution::default();
    let mut secrets = Vec::new();
    let mut adapters = Vec::new();
    let mut lifecycle = EnvironmentLifecycle::default();
    let mut presets = Vec::new();
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
        .filter(|(name, _, _)| {
            name != &Syntax::SYSTEM_FIELD_PACKAGES
                && name != &Syntax::ENV_FIELD_SECRETS
                && !is_lifecycle_field(name)
        })
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
                source,
            )?;
        } else if name == Syntax::ENV_FIELD_SECRETS {
            secrets.extend(parse_secret_specs(
                value,
                base_dir,
                funcs,
                globals,
            )?);
        } else if name == Syntax::ENV_FIELD_PRESETS {
            if let Some(value) = resolved.get(name) {
                presets.extend(presets_from_value(value).map_err(|error| {
                    Diagnostic::error(
                        "E1332",
                        format!("environment preset declaration is invalid: {error}"),
                        "presets are named typed records with string package, variable, and inheritance facts".to_string(),
                        "use `presets: { dev: .{ packages: [\"tool@nixpkgs\"] } }`".to_string(),
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
        } else if is_lifecycle_field(name) {
            let value = evaluate_lifecycle_value(value, base_dir, funcs, globals)?;
            lifecycle_from_field(&mut lifecycle, name, &value).map_err(|error| {
                Diagnostic::error(
                    "E1333",
                    format!("environment lifecycle declaration is invalid: {error}"),
                    "lifecycle fields use typed dotenv, unset, job names, hook records, and Git hook paths".to_string(),
                    "fix the field shape, for example `on_enter: [prepare]`, `dotenv: [\".env\"]`, `git_hooks_path: \"scripts/githooks\"`, or `reload: .Prompt`".to_string(),
                    Some(*span),
                )
            })?;
        } else {
            let v = resolved
                .get(name)
                .cloned()
                .ok_or_else(|| field_missing_value(name, value.span()))?;
            record_setting(&mut entry, name, v.jet_show(), *span, source);
        }
    }
    Ok(EnvCapture {
        entry,
        secrets,
        adapters,
        lifecycle,
        presets,
        languages,
        files,
    })
}

fn is_lifecycle_field(name: &str) -> bool {
    matches!(
        name,
        Syntax::ENV_FIELD_DOTENV
            | Syntax::ENV_FIELD_UNSET
            | Syntax::ENV_FIELD_ON_ENTER
            | Syntax::ENV_FIELD_CHECKS
            | Syntax::ENV_FIELD_GIT_HOOKS_PATH
            | Syntax::ENV_FIELD_FORMATTER
            | Syntax::ENV_FIELD_RELOAD
    )
}

/// Lifecycle names are references, not comptime variable reads. Keep bare
/// identifiers as strings while still evaluating ordinary literals and the
/// explicit expert record's scalar fields.
fn evaluate_lifecycle_value(
    expr: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Result<crate::Comptime::CtValue, Diagnostic> {
    match expr {
        Expr::Ident(name, _) => Ok(crate::Comptime::CtValue::Str(name.clone())),
        Expr::Field(..) => Ok(crate::Comptime::CtValue::Str(expression_name(expr))),
        Expr::ListLit(values, _) => values
            .iter()
            .map(|value| evaluate_lifecycle_value(value, base_dir, funcs, globals))
            .collect::<Result<Vec<_>, _>>()
            .map(crate::Comptime::CtValue::List),
        Expr::StructLit {
            type_name, fields, ..
        } => fields
            .iter()
            .map(|(name, _, value)| {
                Ok((
                    name.clone(),
                    evaluate_lifecycle_value(value, base_dir, funcs, globals)?,
                ))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|fields| crate::Comptime::CtValue::Struct {
                type_name: type_name.clone(),
                fields,
            }),
        Expr::MapLit(entries, _) => entries
            .iter()
            .map(|(key, value)| {
                let key = evaluate_lifecycle_value(key, base_dir, funcs, globals)?;
                let key = integration_ct_key(&key).ok_or_else(|| {
                    Diagnostic::error(
                        "E1333",
                        "lifecycle map keys must be deterministic scalar values".to_string(),
                        "lifecycle records are captured as stable plan facts".to_string(),
                        "use a string, integer, boolean, or character key".to_string(),
                        Some(expr.span()),
                    )
                })?;
                Ok((
                    key,
                    evaluate_lifecycle_value(value, base_dir, funcs, globals)?,
                ))
            })
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|entries| crate::Comptime::CtValue::Map(entries.into_iter().collect())),
        _ => {
            check_build_io(expr)?;
            Comptime::evaluate(expr, funcs, &HashSet::new(), base_dir, globals)
        }
    }
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
    source: &str,
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
                        record_setting(
                            entry,
                            Syntax::ENV_FIELD_PROMPT,
                            label,
                            *span,
                            source,
                        );
                    }
                    Syntax::PROMPT_FIELD_PATH => {
                        let Some(word) = prompt_word(&v) else {
                            return Err(prompt_bad_value(field, ".Short or .Full", *span));
                        };
                        if word != Syntax::PROMPT_PATH_SHORT && word != Syntax::PROMPT_PATH_FULL {
                            return Err(prompt_bad_value(field, ".Short or .Full", *span));
                        }
                        record_setting(
                            entry,
                            Syntax::PROMPT_SETTING_PATH,
                            word,
                            *span,
                            source,
                        );
                    }
                    Syntax::PROMPT_FIELD_STRIP => {
                        let Some(word) = prompt_word(&v) else {
                            return Err(prompt_bad_value(field, ".On or .Off", *span));
                        };
                        if word != Syntax::PROMPT_STRIP_ON && word != Syntax::PROMPT_STRIP_OFF {
                            return Err(prompt_bad_value(field, ".On or .Off", *span));
                        }
                        record_setting(
                            entry,
                            Syntax::PROMPT_SETTING_STRIP,
                            word,
                            *span,
                            source,
                        );
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
    record_setting(
        entry,
        Syntax::ENV_FIELD_PROMPT,
        v.jet_show(),
        value.span(),
        source,
    );
    Ok(())
}

fn record_setting(
    entry: &mut EntryContribution,
    key: &str,
    value: String,
    span: crate::Diagnostics::Span,
    source: &str,
) {
    entry
        .settings
        .entry(key.to_string())
        .or_default()
        .push(
            FactContribution::new(
                key,
                FactValue::Text(value),
                SourceScope::Item,
                ContributionLayer::Environment,
                format!("{source}.{key}"),
            )
            .at(span),
        );
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

/// Lower a general `[String]` field. This is used by package-generation
/// inheritance; secret declarations use the typed map parser below.
fn string_names_from(v: &crate::Comptime::CtValue) -> Option<Vec<String>> {
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

fn secret_decl_error(message: impl Into<String>, span: crate::Diagnostics::Span) -> Diagnostic {
    Diagnostic::error(
        "E1333",
        format!("secret declaration is invalid: {}", message.into()),
        "`secrets:` is one typed map from a secret name to either a metadata record or a `compose(template:, from:)` declaration".to_string(),
        "fix the named secret declaration without putting a secret value in the configuration".to_string(),
        Some(span),
    )
}

fn secret_name_error(name: &str, span: crate::Diagnostics::Span) -> Diagnostic {
    secret_decl_error(format!("secret name `{name}` is not a valid name"), span)
}

/// Lower one `secrets:` map. Both declaration shapes enter through this one
/// function; the dispatch is on the value after the map key, never on a second
/// grammar or a second field parser.
fn parse_secret_specs(
    value: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<Vec<SecretSpec>, Diagnostic> {
    let mut entries = Vec::<(String, crate::Diagnostics::Span, &Expr)>::new();
    match value.without_parens() {
        Expr::StructLit {
            type_name, fields, ..
        } => {
            if !type_name.is_empty() {
                return Err(secret_decl_error(
                    "the secret declaration map must be an inferred `{ … }` record",
                    value.span(),
                ));
            }
            entries.extend(fields.iter().map(|(name, span, value)| (name.clone(), *span, value)));
        }
        Expr::MapLit(fields, _) => {
            for (key, value) in fields {
                let key = secret_eval_value(key, base_dir, funcs, globals).map_err(|_| {
                    secret_decl_error("secret map keys must be strings or bare names", key.span())
                })?;
                let CtValue::Str(name) = key else {
                    return Err(secret_decl_error(
                        "secret map keys must be strings or bare names",
                        key_span(key, value.span()),
                    ));
                };
                entries.push((name, value.span(), value));
            }
        }
        _ => {
            return Err(secret_decl_error(
                "`secrets:` must be a map from names to declarations",
                value.span(),
            ));
        }
    }

    let mut seen = HashSet::new();
    let mut specs = Vec::with_capacity(entries.len());
    for (name, span, declaration) in entries {
        if !valid_env_name(&name) {
            return Err(secret_name_error(&name, span));
        }
        if !seen.insert(name.clone()) {
            return Err(secret_decl_error(
                format!("secret name `{name}` is declared more than once"),
                span,
            ));
        }
        let spec = match declaration.without_parens() {
            Expr::Call(call) if call.name == Syntax::SECRET_COMPOSE => {
                parse_secret_compose(&name, call, base_dir, funcs, globals)?
            }
            Expr::StructLit { .. } => {
                parse_secret_metadata(&name, declaration, base_dir, funcs, globals)?
            }
            _ => {
                return Err(secret_decl_error(
                    format!("secret `{name}` must use a metadata record or `compose(...)`"),
                    declaration.span(),
                ));
            }
        };
        specs.push(spec);
    }
    validate_secret_graph(&specs, value.span())?;
    Ok(specs)
}

fn key_span(_key: CtValue, fallback: crate::Diagnostics::Span) -> crate::Diagnostics::Span {
    // Keys have no independent span after comptime lowering. Keep the error
    // anchored to the value, and never render the rejected key's contents.
    fallback
}

fn parse_secret_metadata(
    name: &str,
    value: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<SecretSpec, Diagnostic> {
    let Expr::StructLit {
        type_name, fields, ..
    } = value.without_parens()
    else {
        unreachable!("secret metadata dispatch validates the record shape")
    };
    if !type_name.is_empty() {
        return Err(secret_decl_error(
            format!("secret `{name}` metadata must be an inferred record"),
            value.span(),
        ));
    }
    let allowed = [
        Syntax::SECRET_META_FIELD_DESCRIPTION,
        Syntax::SECRET_META_FIELD_REQUIRED,
        Syntax::SECRET_META_FIELD_ALLOWED_ENVIRONMENTS,
        Syntax::SECRET_META_FIELD_ROTATION,
        Syntax::SECRET_META_FIELD_DEFAULT,
        Syntax::SECRET_META_FIELD_GENERATE,
    ];
    let mut values = BTreeMap::<String, (crate::Diagnostics::Span, CtValue)>::new();
    for (field, span, expression) in fields {
        if !allowed.iter().any(|known| known == field) {
            return Err(secret_decl_error(
                format!("secret `{name}` has unknown field `{field}`"),
                *span,
            ));
        }
        if values.contains_key(field) {
            return Err(secret_decl_error(
                format!("secret `{name}` repeats field `{field}`"),
                *span,
            ));
        }
        let lowered = secret_eval_value(expression, base_dir, funcs, globals).map_err(|_| {
            secret_decl_error(
                format!("secret `{name}` field `{field}` is not a deterministic value"),
                *span,
            )
        })?;
        values.insert(field.clone(), (*span, lowered));
    }

    let description = values
        .get(Syntax::SECRET_META_FIELD_DESCRIPTION)
        .map(|(span, value)| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(secret_decl_error(
                format!(
                    "secret `{name}` field `{}` must be a string",
                    Syntax::SECRET_META_FIELD_DESCRIPTION
                ),
                *span,
            )),
        })
        .transpose()?;
    let required = match values.get(Syntax::SECRET_META_FIELD_REQUIRED) {
        None => true,
        Some((_span, CtValue::Bool(value))) => *value,
        Some((span, _)) => {
            return Err(secret_decl_error(
                format!(
                    "secret `{name}` field `{}` must be a boolean",
                    Syntax::SECRET_META_FIELD_REQUIRED
                ),
                *span,
            ));
        }
    };
    let allowed_environments = match values.get(Syntax::SECRET_META_FIELD_ALLOWED_ENVIRONMENTS) {
        None => Vec::new(),
        Some((span, value)) => secret_environment_list(name, value, *span)?,
    };
    let rotation = match values.get(Syntax::SECRET_META_FIELD_ROTATION) {
        None => SecretRotationPolicy::None,
        Some((span, value)) => secret_rotation(name, value, *span)?,
    };
    let default = match values.get(Syntax::SECRET_META_FIELD_DEFAULT) {
        None => SecretDefault::None,
        Some((span, value)) => secret_default(name, value, *span)?,
    };
    let generate = match values.get(Syntax::SECRET_META_FIELD_GENERATE) {
        None => SecretGenerator::None,
        Some((span, value)) => secret_generator(name, value, *span)?,
    };
    validate_secret_policies(
        name,
        required,
        &allowed_environments,
        &default,
        &generate,
        value.span(),
    )?;
    Ok(SecretSpec {
        name: name.to_string(),
        description,
        required,
        allowed_environments,
        rotation,
        default,
        generate,
        declaration: SecretDeclaration::Stored,
        implicit: false,
    })
}

fn validate_secret_policies(
    name: &str,
    required: bool,
    allowed_environments: &[String],
    default: &SecretDefault,
    generate: &SecretGenerator,
    span: crate::Diagnostics::Span,
) -> Result<(), Diagnostic> {
    let has_default = matches!(default, SecretDefault::PerProfile(values) if !values.is_empty());
    let has_generator = !matches!(generate, SecretGenerator::None);
    if has_default && has_generator {
        return Err(secret_decl_error(
            format!(
                "secret `{name}` cannot declare both `{}` and `{}`",
                Syntax::SECRET_META_FIELD_DEFAULT,
                Syntax::SECRET_META_FIELD_GENERATE
            ),
            span,
        ));
    }
    if required && (has_default || has_generator) {
        return Err(secret_decl_error(
            format!(
                "secret `{name}` is required and cannot declare `{}` or `{}`",
                Syntax::SECRET_META_FIELD_DEFAULT,
                Syntax::SECRET_META_FIELD_GENERATE
            ),
            span,
        ));
    }
    if !allowed_environments.is_empty() {
        if let SecretDefault::PerProfile(values) = default {
            for profile in values.keys() {
                if !allowed_environments.iter().any(|allowed| allowed == profile) {
                    return Err(secret_decl_error(
                        format!(
                            "secret `{name}` default profile `{profile}` is outside `{}`",
                            Syntax::SECRET_META_FIELD_ALLOWED_ENVIRONMENTS
                        ),
                        span,
                    ));
                }
            }
        }
    }
    Ok(())
}

fn parse_secret_compose(
    name: &str,
    call: &crate::AST::Call,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<SecretSpec, Diagnostic> {
    let mut template = None;
    let mut from = None;
    let mut seen = HashSet::new();
    for argument in &call.args {
        let Some((label, label_span)) = &argument.label else {
            return Err(secret_decl_error(
                format!("secret `{name}` compose arguments must be labeled"),
                argument.span,
            ));
        };
        if !seen.insert(label.clone()) {
            return Err(secret_decl_error(
                format!("secret `{name}` compose repeats field `{label}`"),
                *label_span,
            ));
        }
        match label.as_str() {
            Syntax::SECRET_COMPOSE_FIELD_TEMPLATE => {
                let value = secret_template_value(&argument.expr, base_dir, funcs, globals)
                    .map_err(|_| {
                        secret_decl_error(
                            format!("secret `{name}` compose `template` must be text"),
                            argument.expr.span(),
                        )
                    })?;
                template = Some((value, argument.expr.span()));
            }
            Syntax::SECRET_COMPOSE_FIELD_FROM => {
                let value = secret_eval_value(&argument.expr, base_dir, funcs, globals).map_err(|_| {
                    secret_decl_error(
                        format!("secret `{name}` compose `from` must be a list of names"),
                        argument.expr.span(),
                    )
                })?;
                from = Some((value, argument.expr.span()));
            }
            _ => {
                return Err(secret_decl_error(
                    format!("secret `{name}` compose has unknown field `{label}`"),
                    *label_span,
                ));
            }
        }
    }
    let (template, template_span) = template.ok_or_else(|| {
        secret_decl_error(
            format!("secret `{name}` compose needs `template`"),
            call.name_span,
        )
    })?;
    let (from, from_span) = from.ok_or_else(|| {
        secret_decl_error(
            format!("secret `{name}` compose needs `from`"),
            call.name_span,
        )
    })?;
    let mut from = secret_string_list(&from, &format!("secret `{name}` compose `from`"), from_span)?;
    if from.is_empty() {
        return Err(secret_decl_error(
            format!("secret `{name}` compose `from` must not be empty"),
            from_span,
        ));
    }
    let mut seen_inputs = HashSet::new();
    for input in &from {
        if !valid_env_name(input) {
            return Err(secret_name_error(input, from_span));
        }
        if !seen_inputs.insert(input.clone()) {
            return Err(secret_decl_error(
                format!("secret `{name}` compose repeats input `{input}`"),
                from_span,
            ));
        }
    }
    validate_secret_template(name, &template, &from, template_span)?;
    from.sort();
    Ok(SecretSpec {
        name: name.to_string(),
        description: None,
        required: true,
        allowed_environments: Vec::new(),
        rotation: SecretRotationPolicy::None,
        default: SecretDefault::None,
        generate: SecretGenerator::None,
        declaration: SecretDeclaration::Compose { template, from },
        implicit: false,
    })
}

fn secret_environment_list(
    name: &str,
    value: &CtValue,
    span: crate::Diagnostics::Span,
) -> Result<Vec<String>, Diagnostic> {
    let values = secret_string_list(
        value,
        &format!(
            "secret `{name}` field `{}`",
            Syntax::SECRET_META_FIELD_ALLOWED_ENVIRONMENTS
        ),
        span,
    )?;
    let mut seen = HashSet::new();
    for environment in &values {
        if !valid_env_name(environment) {
            return Err(secret_decl_error(
                format!("secret `{name}` names invalid environment label `{environment}`"),
                span,
            ));
        }
        if !seen.insert(environment.clone()) {
            return Err(secret_decl_error(
                format!("secret `{name}` repeats allowed environment `{environment}`"),
                span,
            ));
        }
    }
    Ok(values)
}

fn secret_rotation(
    name: &str,
    value: &CtValue,
    span: crate::Diagnostics::Span,
) -> Result<SecretRotationPolicy, Diagnostic> {
    match value {
        CtValue::Str(value) if value == Syntax::SECRET_NONE => Ok(SecretRotationPolicy::None),
        CtValue::Struct { type_name, fields } if type_name == "SecretMaxAge" => {
            let seconds = fields
                .iter()
                .find_map(|(field, value)| (field == "seconds").then_some(value))
                .and_then(|value| match value {
                    CtValue::Int(value) if *value > 0 => u64::try_from(*value).ok(),
                    _ => None,
                })
                .ok_or_else(|| {
                    secret_decl_error(
                        format!("secret `{name}` rotation max age must be positive"),
                        span,
                    )
                })?;
            Ok(SecretRotationPolicy::MaxAge { seconds })
        }
        _ => Err(secret_decl_error(
            format!(
                "secret `{name}` field `{}` must be `{}` or `{}`(…)",
                Syntax::SECRET_META_FIELD_ROTATION,
                Syntax::SECRET_NONE,
                Syntax::SECRET_ROTATION_MAX_AGE
            ),
            span,
        )),
    }
}

fn secret_default(
    name: &str,
    value: &CtValue,
    span: crate::Diagnostics::Span,
) -> Result<SecretDefault, Diagnostic> {
    if matches!(value, CtValue::Str(value) if value == Syntax::SECRET_NONE) {
        return Ok(SecretDefault::None);
    }
    let fields = match value {
        CtValue::Struct { fields, .. } => fields
            .iter()
            .map(|(profile, value)| (profile.clone(), value))
            .collect::<Vec<_>>(),
        CtValue::Map(entries) => {
            let mut fields = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                let CtKey::Str(profile) = key else {
                    return Err(secret_decl_error(
                        format!("secret `{name}` default profiles must be named strings"),
                        span,
                    ));
                };
                fields.push((profile.clone(), value));
            }
            fields
        }
        _ => {
            return Err(secret_decl_error(
                format!("secret `{name}` field `default` must be `none` or a profile map"),
                span,
            ));
        }
    };
    let mut result = BTreeMap::new();
    for (profile, value) in fields {
        if !valid_env_name(&profile) {
            return Err(secret_decl_error(
                format!("secret `{name}` default uses invalid profile `{profile}`"),
                span,
            ));
        }
        let CtValue::Str(value) = value else {
            return Err(secret_decl_error(
                format!("secret `{name}` default values must be strings"),
                span,
            ));
        };
        if result.insert(profile.clone(), value.clone()).is_some() {
            return Err(secret_decl_error(
                format!("secret `{name}` repeats default profile `{profile}`"),
                span,
            ));
        }
    }
    Ok(SecretDefault::PerProfile(result))
}

fn secret_generator(
    name: &str,
    value: &CtValue,
    span: crate::Diagnostics::Span,
) -> Result<SecretGenerator, Diagnostic> {
    if matches!(value, CtValue::Str(value) if value == Syntax::SECRET_NONE) {
        return Ok(SecretGenerator::None);
    }
    if let CtValue::Struct { type_name, fields } = value {
        if type_name == "SecretRandom" {
            let length = fields
                .iter()
                .find_map(|(field, value)| {
                    (field == Syntax::SECRET_GENERATOR_FIELD_LENGTH).then_some(value)
                })
                .and_then(|value| match value {
                    CtValue::Int(value) if *value > 0 => u64::try_from(*value).ok(),
                    _ => None,
                })
                .ok_or_else(|| {
                    secret_decl_error(
                        format!("secret `{name}` random generator length must be positive"),
                        span,
                    )
                })?;
            return Ok(SecretGenerator::Random { length });
        }
    }
    Err(secret_decl_error(
        format!(
            "secret `{name}` field `{}` must be `{}` or `{}`({}: …)",
            Syntax::SECRET_META_FIELD_GENERATE,
            Syntax::SECRET_NONE,
            Syntax::SECRET_GENERATOR_RANDOM,
            Syntax::SECRET_GENERATOR_FIELD_LENGTH
        ),
        span,
    ))
}

fn secret_string_list(
    value: &CtValue,
    scope: &str,
    span: crate::Diagnostics::Span,
) -> Result<Vec<String>, Diagnostic> {
    let CtValue::List(values) = value else {
        return Err(secret_decl_error(format!("{scope} must be a list of strings"), span));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(secret_decl_error(format!("{scope} must contain only strings"), span)),
        })
        .collect()
}

fn secret_template_value(
    value: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<String, Diagnostic> {
    match value.without_parens() {
        Expr::Str(parts, _) => {
            let mut template = String::new();
            for part in parts {
                match part {
                    StrPart::Lit(value) => template.push_str(value),
                    StrPart::Interp(expression, _) => {
                        let Some(name) = secret_reference_name(expression) else {
                            return Err(secret_decl_error(
                                "compose template interpolations must name secrets",
                                expression.span(),
                            ));
                        };
                        template.push('{');
                        template.push_str(&name);
                        template.push('}');
                    }
                }
            }
            Ok(template)
        }
        Expr::Ident(name, _) => Ok(name.clone()),
        Expr::Field(..) => Ok(expression_name(value)),
        _ => match secret_eval_value(value, base_dir, funcs, globals)? {
            CtValue::Str(value) => Ok(value),
            _ => Err(secret_decl_error(
                "compose `template` must be a string",
                value.span(),
            )),
        },
    }
}

fn secret_reference_name(value: &Expr) -> Option<String> {
    match value.without_parens() {
        Expr::Ident(_, _) | Expr::Field(_, _, _) => Some(expression_name(value)),
        _ => None,
    }
}

fn validate_secret_template(
    name: &str,
    template: &str,
    from: &[String],
    span: crate::Diagnostics::Span,
) -> Result<(), Diagnostic> {
    if template.is_empty() || template.chars().any(char::is_control) {
        return Err(secret_decl_error(
            format!("secret `{name}` compose template must be non-empty text"),
            span,
        ));
    }
    let mut placeholders = HashSet::new();
    let mut chars = template.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' => {
                let mut placeholder = String::new();
                let mut closed = false;
                for next in chars.by_ref() {
                    if next == '}' {
                        closed = true;
                        break;
                    }
                    if next == '{' || next.is_whitespace() {
                        return Err(secret_decl_error(
                            format!("secret `{name}` compose template has an invalid placeholder"),
                            span,
                        ));
                    }
                    placeholder.push(next);
                }
                if !closed || !valid_env_name(&placeholder) {
                    return Err(secret_decl_error(
                        format!("secret `{name}` compose template has an invalid placeholder"),
                        span,
                    ));
                }
                placeholders.insert(placeholder);
            }
            '}' => {
                return Err(secret_decl_error(
                    format!("secret `{name}` compose template has an unmatched brace"),
                    span,
                ));
            }
            _ => {}
        }
    }
    let inputs = from.iter().cloned().collect::<HashSet<_>>();
    if placeholders != inputs {
        return Err(secret_decl_error(
            format!("secret `{name}` compose template placeholders must match `from`"),
            span,
        ));
    }
    Ok(())
}

/// A graph check shared by each map parser and the selected-environment merge.
/// Unknown input names are allowed here; activation is the tier that checks
/// whether each source exists in the encrypted store.
pub(super) fn validate_secret_graph(
    specs: &[SecretSpec],
    span: crate::Diagnostics::Span,
) -> Result<(), Diagnostic> {
    let composed = specs
        .iter()
        .filter_map(|spec| match &spec.declaration {
            SecretDeclaration::Compose { from, .. } => Some((spec.name.clone(), from.clone())),
            SecretDeclaration::Stored => None,
        })
        .collect::<BTreeMap<_, _>>();
    let mut states = HashMap::<String, u8>::new();
    for name in composed.keys() {
        secret_graph_visit(name, &composed, &mut states, span)?;
    }
    Ok(())
}

fn secret_graph_visit(
    name: &str,
    composed: &BTreeMap<String, Vec<String>>,
    states: &mut HashMap<String, u8>,
    span: crate::Diagnostics::Span,
) -> Result<(), Diagnostic> {
    match states.get(name).copied() {
        Some(1) => {
            return Err(secret_decl_error(
                format!("secret composition cycle includes `{name}`"),
                span,
            ));
        }
        Some(2) => return Ok(()),
        _ => {}
    }
    states.insert(name.to_string(), 1);
    if let Some(inputs) = composed.get(name) {
        for input in inputs {
            if composed.contains_key(input) {
                secret_graph_visit(input, composed, states, span)?;
            }
        }
    }
    states.insert(name.to_string(), 2);
    Ok(())
}

fn secret_eval_value(
    value: &Expr,
    base_dir: &Path,
    funcs: &HashMap<String, &Func>,
    globals: &HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    match value.without_parens() {
        Expr::Ident(name, _) => Ok(globals
            .get(name)
            .cloned()
            .unwrap_or_else(|| CtValue::Str(name.clone()))),
        Expr::Field(..) => Ok(CtValue::Str(expression_name(value))),
        Expr::ListLit(values, _) => values
            .iter()
            .map(|value| secret_eval_value(value, base_dir, funcs, globals))
            .collect::<Result<Vec<_>, _>>()
            .map(CtValue::List),
        Expr::MapLit(values, _) => {
            let mut lowered = BTreeMap::new();
            for (key, value_expr) in values {
                let key = CtKey::from_value(secret_eval_value(key, base_dir, funcs, globals)?)
                    .ok_or_else(|| secret_decl_error("secret map keys must be scalar values", key.span()))?;
                let lowered_value = secret_eval_value(value_expr, base_dir, funcs, globals)?;
                if lowered.insert(key, lowered_value).is_some() {
                    return Err(secret_decl_error(
                        "secret map contains a duplicate key",
                        value_expr.span(),
                    ));
                }
            }
            Ok(CtValue::Map(lowered))
        }
        Expr::StructLit {
            type_name, fields, ..
        } => {
            let fields = fields
                .iter()
                .map(|(name, _, value)| {
                    secret_eval_value(value, base_dir, funcs, globals)
                        .map(|value| (name.clone(), value))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CtValue::Struct {
                type_name: type_name.clone(),
                fields,
            })
        }
        Expr::UnitLit { raw, int, suffix, .. } => {
            let Some(amount) = int else {
                return Err(secret_decl_error(
                    "secret duration policies need an integer unit literal",
                    value.span(),
                ));
            };
            let _ = raw;
            Ok(CtValue::Struct {
                type_name: suffix.clone(),
                fields: vec![("value".to_string(), CtValue::Int(*amount))],
            })
        }
        Expr::Call(call) if call.name == Syntax::SECRET_ROTATION_MAX_AGE => {
            if call.args.len() != 1 || call.args[0].label.is_some() {
                return Err(secret_decl_error(
                    "`max_age` needs one duration argument",
                    call.name_span,
                ));
            }
            let duration = secret_eval_value(&call.args[0].expr, base_dir, funcs, globals)?;
            let seconds = secret_duration_seconds(&duration).ok_or_else(|| {
                secret_decl_error("`max_age` needs a positive duration", call.args[0].expr.span())
            })?;
            Ok(CtValue::Struct {
                type_name: "SecretMaxAge".to_string(),
                fields: vec![("seconds".to_string(), CtValue::Int(seconds as i64))],
            })
        }
        Expr::Call(call) if call.name == Syntax::SECRET_GENERATOR_RANDOM => {
            let mut length = None;
            for argument in &call.args {
                if argument.label.as_ref().map(|(name, _)| name.as_str())
                    != Some(Syntax::SECRET_GENERATOR_FIELD_LENGTH)
                {
                    return Err(secret_decl_error(
                        "`random` needs a labeled `length` argument",
                        argument.span,
                    ));
                }
                if length.is_some() {
                    return Err(secret_decl_error(
                        "`random` repeats `length`",
                        argument.span,
                    ));
                }
                let value = secret_eval_value(&argument.expr, base_dir, funcs, globals)?;
                let CtValue::Int(value) = value else {
                    return Err(secret_decl_error(
                        "`random` length must be an integer",
                        argument.expr.span(),
                    ));
                };
                length = Some(value);
            }
            let Some(length) = length.filter(|value| *value > 0) else {
                return Err(secret_decl_error(
                    "`random` length must be positive",
                    call.name_span,
                ));
            };
            Ok(CtValue::Struct {
                type_name: "SecretRandom".to_string(),
                fields: vec![(
                    Syntax::SECRET_GENERATOR_FIELD_LENGTH.to_string(),
                    CtValue::Int(length),
                )],
            })
        }
        Expr::Paren(inner, _) => secret_eval_value(inner, base_dir, funcs, globals),
        _ => Comptime::evaluate(value, funcs, &HashSet::new(), base_dir, globals),
    }
}

fn secret_duration_seconds(value: &CtValue) -> Option<i64> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    let amount = fields.iter().find_map(|(field, value)| {
        (field == "value").then_some(value).and_then(|value| match value {
            CtValue::Int(value) if *value > 0 => Some(*value),
            _ => None,
        })
    })?;
    let unit = type_name.rsplit('.').next().unwrap_or(type_name);
    Some(match unit {
        "d" | "day" | "days" => amount.saturating_mul(86_400),
        "h" | "hour" | "hours" => amount.saturating_mul(3_600),
        "m" | "min" | "minute" | "minutes" => amount.saturating_mul(60),
        "s" | "sec" | "second" | "seconds" => amount,
        _ => return None,
    })
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
    source: &str,
) -> Result<
    (
        EnvCapture,
        Vec<DevServicePlan>,
    ),
    Diagnostic,
> {
    let capture = evaluate_env_fields(&lit.fields, src, base_dir, funcs, globals, source)?;
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
        if item.starts_with(Syntax::PKG_ADAPT) {
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
    let args = call_args(item, Syntax::PKG_ADAPT).ok_or_else(|| adapter_shape(item))?;
    validate_fields(
        args,
        &[
            Syntax::ADAPTER_FIELD_NAME,
            Syntax::ADAPTER_FIELD_SOURCE,
            Syntax::ADAPTER_FIELD_DEPS,
            Syntax::ADAPTER_FIELD_RECIPE,
        ],
    )?;
    let name = named_string(args, Syntax::ADAPTER_FIELD_NAME)
        .ok_or_else(|| adapter_shape(item))?;
    let source = named_raw(args, Syntax::ADAPTER_FIELD_SOURCE)
        .ok_or_else(|| adapter_shape(item))?;
    let source = unquote(source.trim()).unwrap_or(source);
    super::super::RefSpec::classify_provider_ref(&source).map_err(|_| adapter_shape(item))?;
    let deps = named_raw(args, Syntax::ADAPTER_FIELD_DEPS)
        .map(|raw| {
            let body = raw
                .trim()
                .strip_prefix('[')
                .and_then(|t| t.strip_suffix(']'))
                .unwrap_or(raw.trim());
            Merge::parse_package_list(body)
        })
        .unwrap_or_default();
    let recipe = parse_recipe(
        &named_raw(args, Syntax::ADAPTER_FIELD_RECIPE)
            .ok_or_else(|| adapter_shape(item))?,
    )?;
    Ok(AdapterPlan {
        name,
        source,
        deps,
        recipe,
    })
}

fn parse_recipe(raw: &str) -> Result<AdapterRecipe, Diagnostic> {
    let raw = raw.trim();
    if let Some(args) = call_args(raw, Syntax::RECIPE_BUILD) {
        validate_fields(args, &[Syntax::RECIPE_FIELD_STEPS])?;
        let steps = parse_build_steps(args)?;
        return Ok(AdapterRecipe::Build(super::super::Recipe::BuildRecipe { steps }));
    }
    if let Some(args) = call_args(raw, Syntax::RECIPE_COPY) {
        if args.trim().is_empty() {
            return Ok(AdapterRecipe::Copy);
        }
        return Err(adapter_shape(raw));
    }
    if let Some(args) = call_args(raw, Syntax::RECIPE_PREBUILT) {
        if args.trim().is_empty() {
            return Err(adapter_shape(raw));
        }
        validate_fields(
            args,
            &[
                Syntax::RECIPE_FIELD_BIN,
                Syntax::RECIPE_FIELD_AS,
                Syntax::RECIPE_FIELD_AS_NAME,
            ],
        )?;
        let bin = named_string(args, Syntax::RECIPE_FIELD_BIN)
            .ok_or_else(|| adapter_shape(raw))?;
        let as_name = named_string(args, Syntax::RECIPE_FIELD_AS)
            .or_else(|| named_string(args, Syntax::RECIPE_FIELD_AS_NAME))
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

fn parse_build_steps(args: &str) -> Result<Vec<super::super::Recipe::BuildStep>, Diagnostic> {
    let raw = named_field(args, Syntax::RECIPE_FIELD_STEPS)
        .ok_or_else(|| adapter_shape(args))?;
    let body = raw
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| adapter_shape(args))?;
    let mut steps = Vec::new();
    for item in split_top_level(body) {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        steps.push(parse_build_step(item)?);
    }
    if steps.is_empty() {
        return Err(adapter_shape(args));
    }
    Ok(steps)
}

fn parse_build_step(raw: &str) -> Result<super::super::Recipe::BuildStep, Diagnostic> {
    use super::super::Recipe::BuildStep;

    if let Some(args) = call_args(raw, Syntax::RECIPE_STEP_FETCH) {
        validate_fields(
            args,
            &[Syntax::RECIPE_STEP_FIELD_URL, Syntax::RECIPE_STEP_FIELD_SHA256],
        )?;
        return Ok(BuildStep::Fetch {
            url: required_field_string(args, Syntax::RECIPE_STEP_FIELD_URL)?,
            sha256: required_field_string(args, Syntax::RECIPE_STEP_FIELD_SHA256)?,
        });
    }
    if let Some(args) = call_args(raw, Syntax::RECIPE_STEP_EXEC) {
        validate_fields(
            args,
            &[Syntax::RECIPE_STEP_FIELD_TOOL, Syntax::RECIPE_STEP_FIELD_ARGS],
        )?;
        return Ok(BuildStep::Exec {
            tool: required_field_string(args, Syntax::RECIPE_STEP_FIELD_TOOL)?,
            args: required_field_strings(args, Syntax::RECIPE_STEP_FIELD_ARGS)?,
        });
    }
    if let Some(args) = call_args(raw, Syntax::RECIPE_STEP_INSTALL) {
        validate_fields(
            args,
            &[Syntax::RECIPE_STEP_FIELD_SRC, Syntax::RECIPE_STEP_FIELD_DEST],
        )?;
        return Ok(BuildStep::Install {
            src: required_field_string(args, Syntax::RECIPE_STEP_FIELD_SRC)?,
            dest: required_field_string(args, Syntax::RECIPE_STEP_FIELD_DEST)?,
        });
    }
    if let Some(args) = call_args(raw, Syntax::RECIPE_STEP_INSTALL_TREE) {
        validate_fields(
            args,
            &[Syntax::RECIPE_STEP_FIELD_SRC, Syntax::RECIPE_STEP_FIELD_DEST],
        )?;
        return Ok(BuildStep::InstallTree {
            src: required_field_string(args, Syntax::RECIPE_STEP_FIELD_SRC)?,
            dest: required_field_string(args, Syntax::RECIPE_STEP_FIELD_DEST)?,
        });
    }
    Err(adapter_shape(raw))
}

fn named_field<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    let fields = named_fields(args)?;
    let mut found = None;
    for (name, value) in fields {
        if name == key {
            if found.is_some() {
                return None;
            }
            found = Some(value);
        }
    }
    found
}

fn named_fields<'a>(args: &'a str) -> Option<Vec<(&'a str, &'a str)>> {
    split_top_level(args)
        .into_iter()
        .filter(|item| !item.trim().is_empty())
        .map(|item| {
            let (name, value) = item.trim().split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.is_empty()).then_some((name, value))
        })
        .collect()
}

fn validate_fields(args: &str, allowed: &[&str]) -> Result<(), Diagnostic> {
    let fields = named_fields(args).ok_or_else(|| adapter_shape(args))?;
    let mut seen = HashSet::new();
    if fields.iter().any(|(name, _)| {
        !allowed.contains(name) || !seen.insert(*name)
    }) {
        return Err(adapter_shape(args));
    }
    Ok(())
}

fn required_field_string(args: &str, key: &str) -> Result<String, Diagnostic> {
    let value = named_field(args, key).ok_or_else(|| adapter_shape(args))?;
    unquote(value.trim()).ok_or_else(|| adapter_shape(args))
}

fn required_field_strings(args: &str, key: &str) -> Result<Vec<String>, Diagnostic> {
    let value = named_field(args, key).ok_or_else(|| adapter_shape(args))?;
    let body = value
        .trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| adapter_shape(args))?;
    split_top_level(body)
        .into_iter()
        .map(|item| unquote(item.trim()).ok_or_else(|| adapter_shape(args)))
        .collect()
}

fn call_args<'a>(raw: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = raw.trim().strip_prefix(prefix)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    rest.strip_suffix(')')
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
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars.next()?;
                let (_, decoded) = Syntax::ESCAPES
                    .iter()
                    .find(|&&(marker, _)| marker == escaped)?;
                out.push(*decoded);
            }
            '{' if chars.clone().next() == Some('{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.clone().next() == Some('}') => {
                chars.next();
                out.push('}');
            }
            other => out.push(other),
        }
    }
    Some(out)
}

fn adapter_shape(_raw: &str) -> Diagnostic {
    Diagnostic::error(
        "E1270",
        "adapter package declaration is not complete".to_string(),
        "`Pkg.adapt` needs `name:`, `source:`, and a supported `recipe:`; recipes are `Recipe.copy()`, `Recipe.prebuilt(bin:, as:)`, or a finite `Recipe.build(steps: […])`.".to_string(),
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
    if pkg.source.is_empty() && pkg.name.contains(Syntax::REF_PROVIDER_AT) {
        return pkg.name.clone();
    }
    let source = if pkg.source.is_empty() {
        Syntax::DEFAULT_SOURCE
    } else {
        pkg.source.as_str()
    };
    format!("{}{}{}", pkg.name, Syntax::REF_PROVIDER_AT, source)
}
