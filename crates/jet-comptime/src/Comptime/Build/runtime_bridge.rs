use super::actions_policy::{
    ActionCache, ActionKind, ActionSpec, BuildCapability, BuildPolicy, BuildResourcePool,
    LegacyWrapperSpec,
};
use super::context::BuildContext;
use super::errors_keys::{dependency_cycle_text, BuildError};
use super::handles::{
    ActionHandle, ActionId, ProbeHandle, ProbeId, SigningIdentityHandle, SigningIdentityId,
    TargetId, TargetRef, ToolchainHandle, ToolchainId,
};
use super::plan_graph::BuildPlan;
use super::provenance_toolchains::{
    BuildProvenance, LinkerIdentity, ProbeSpec, ReproducibilityClass, SdkIdentity,
    SigningIdentitySpec, SysrootIdentity, ToolchainSpec,
};
use super::targets::TargetSpec;
use crate::Diagnostics::{Diagnostic, Span, StructuredDiagnostic};
use crate::AST::{ComptimeInput, CtValue};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct ProgramBuildSession {
    context: BuildContext,
    package: String,
    project_root: PathBuf,
    diagnostics: Vec<Diagnostic>,
    last_span: Span,
}

thread_local! {
    static PROGRAM_BUILD_SESSIONS: RefCell<BTreeMap<u64, ProgramBuildSession>> =
        const { RefCell::new(BTreeMap::new()) };
}

/// Start one selected-root build evaluation. Session is thread-local so
/// parallel compiler invocations cannot share graph mutation.
pub fn begin_program_build(package: impl Into<String>, program: CtValue) -> CtValue {
    begin_program_build_with_policy(package, program, BuildPolicy::local_default())
}

/// Start a build session with the caller's already-resolved policy. The
/// default entry point remains permissive for Rust-only graph consumers; the
/// compiler driver passes the real package/workspace policy here.
pub fn begin_program_build_with_policy(
    package: impl Into<String>,
    program: CtValue,
    policy: BuildPolicy,
) -> CtValue {
    begin_program_build_with_policy_at(package, program, policy, Path::new("."))
}

/// Start a build session with the owning project root. The root is retained
/// for production-only imports whose semantics come from a canonical project
/// file (legacy wrappers); source text cannot substitute another root.
pub fn begin_program_build_with_policy_at(
    package: impl Into<String>,
    program: CtValue,
    policy: BuildPolicy,
    project_root: &Path,
) -> CtValue {
    let context = BuildContext::new_with_policy(policy);
    let id = context.context;
    PROGRAM_BUILD_SESSIONS.with(|sessions| {
        sessions.borrow_mut().insert(
            id,
            ProgramBuildSession {
                context,
                package: package.into(),
                project_root: project_root.to_path_buf(),
                diagnostics: Vec::new(),
                last_span: Span::new(0, 0),
            },
        );
    });
    CtValue::Struct {
        type_name: crate::Syntax::TYPE_BUILD_CONTEXT.to_string(),
        fields: vec![
            ("__session".to_string(), CtValue::Int(id as i64)),
            ("program".to_string(), program),
        ],
    }
}

pub fn abort_program_build(context: &CtValue) {
    if let Some(id) = build_session_id(context) {
        PROGRAM_BUILD_SESSIONS.with(|sessions| {
            sessions.borrow_mut().remove(&id);
        });
    }
}

pub fn finish_program_build(
    context: &CtValue,
    returned: &CtValue,
) -> Result<(BuildPlan, Vec<Diagnostic>), Diagnostic> {
    let id = build_session_id(context)
        .ok_or_else(|| build_diag("lost build context", Span::new(0, 0)))?;
    let default = returned_handle(returned, crate::Syntax::TYPE_BUILD_PLAN)
        .and_then(|(_, target)| (target >= 0).then_some(target as usize));
    let session = PROGRAM_BUILD_SESSIONS.with(|sessions| sessions.borrow_mut().remove(&id));
    let session = session.ok_or_else(|| build_diag("build context expired", Span::new(0, 0)))?;
    let error_span = session.last_span;
    let plan = match default {
        Some(target) => session
            .context
            .plan_with_default(TargetRef {
                id: TargetId(target),
                context: id,
            })
            .map_err(|e| build_error_diag(&e, error_span)),
        None => session
            .context
            .plan()
            .map_err(|e| build_error_diag(&e, error_span)),
    }?;
    Ok((plan, session.diagnostics))
}

#[doc(hidden)]
pub fn eval_program_build_method(
    receiver: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    generated_source: Option<&str>,
    span: Span,
    in_impure_gate: bool,
) -> Option<Result<CtValue, Diagnostic>> {
    let id = build_session_id(receiver)?;
    Some(PROGRAM_BUILD_SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        let session = sessions
            .get_mut(&id)
            .ok_or_else(|| build_diag("build context expired", span))?;
        session.last_span = span;
        eval_session_method(
            id,
            session,
            method,
            args,
            generated_source,
            span,
            in_impure_gate,
        )
    }))
}

#[doc(hidden)]
pub fn eval_program_build_input_method(
    receiver: &CtValue,
    method: &str,
    args: &[CtValue],
    first_string_literal: Option<&str>,
    base_dir: &Path,
    embed_inputs: Option<&mut Vec<ComptimeInput>>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    build_session_id(receiver)?;
    match method {
        "find" => Some((|| {
            if args.len() != 1 {
                return Err(build_diag("`b.find` requires exactly one glob", span));
            }
            let builtin = crate::Syntax::BUILTIN_FIND;
            let glob = first_string_literal.ok_or_else(|| {
                crate::Comptime::Methods::embed_path_err(builtin, "literal", span)
            })?;
            let glob = crate::Comptime::Methods::check_literal_embed_path(builtin, glob, span)?;
            crate::Comptime::Methods::eval_locked_find(base_dir, &glob, embed_inputs, span)
        })()),
        "embed" => Some(crate::Comptime::Methods::eval_build_embed(
            args,
            base_dir,
            embed_inputs,
            span,
        )),
        "fetch" => Some(
            crate::Comptime::Methods::eval_net_fetch(args, embed_inputs, span)
                .map(|value| CtValue::Present(Box::new(value))),
        ),
        _ => None,
    }
}

fn eval_session_method(
    id: u64,
    session: &mut ProgramBuildSession,
    method: &str,
    args: Vec<CtValue>,
    generated_source: Option<&str>,
    span: Span,
    in_impure_gate: bool,
) -> Result<CtValue, Diagnostic> {
    let action_caps_index = match method {
        "action" => Some(4),
        "legacy" => Some(5),
        _ => None,
    };
    let action_has_effects = action_caps_index
        .and_then(|index| args.get(index))
        .is_some_and(|value| matches!(value, CtValue::List(values) if !values.is_empty()));
    let plugin_requested = method == "plugin";
    let ambient_probe = method == "probe"
        && !args.iter().rev().take(2).any(|value| {
            matches!(value, CtValue::Str(name) if name.eq_ignore_ascii_case("reproducible"))
        });
    if (ambient_probe || action_has_effects || plugin_requested) && !in_impure_gate {
        return Err(Diagnostic::error(
            "E3502",
            format!(
                "`b.{method}` touches the ambient world and must be inside `#Impure(\"reason\")`"
            ),
            "build effects must be declared exactly where an auditor can see them".to_string(),
            format!("wrap the `b.{method}` call in `#Impure(\"why it is needed\")`"),
            Some(span),
        ));
    }
    let result = match method {
        "plugin" => {
            let manifest = string_arg(&args, 0, span)?;
            let component = string_arg(&args, 1, span)?;
            let policy = session.context.policy().clone();
            session
                .context
                .apply_packaged_wasm_component_plugin_from_host(
                    Path::new(&manifest),
                    Path::new(&component),
                    &policy,
                )
                .map(|_| CtValue::Unit)
        }
        "generate" => {
            if args.len() != 1 {
                return Err(build_diag(
                    "`b.generate` requires a name and typed item block",
                    span,
                ));
            }
            let name = string_arg(&args, 0, span)?;
            let source = generated_source.ok_or_else(|| {
                build_diag(
                    "`b.generate` requires a typed item block; source strings are retired",
                    span,
                )
            })?;
            let path = format!(
                ".jet/generated/{}/{}.jet",
                session.package,
                safe_name(&name)
            );
            session
                .context
                .generate(name, path, source.to_string())
                .map(|_| CtValue::Unit)
        }
        "action" => {
            let name = string_arg(&args, 0, span)?;
            let inputs = string_list_arg(&args, 1, span)?;
            let outputs = string_list_arg(&args, 2, span)?;
            let argv = string_list_arg(&args, 3, span)?;
            let caps = string_list_arg(&args, 4, span)?;
            let mut spec = ActionSpec::cached(argv)
                .with_inputs(inputs)
                .with_outputs(outputs);
            for cap in caps {
                spec = spec.with_cap(parse_capability(&cap, span)?);
            }
            if args.len() >= 7 {
                let toolchain =
                    handle_arg(&args, 5, crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, span)?;
                spec = spec.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
                for probe in handle_list_arg(&args, 6, crate::Syntax::TYPE_BUILD_PROBE, id, span)? {
                    spec = spec.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            if args.len() >= 8 {
                let signing = handle_arg(&args, 7, "BuildSigningIdentity", id, span)?;
                spec = spec.with_signing_identity(super::handles::SigningIdentityHandle {
                    id: super::handles::SigningIdentityId(signing),
                    context: id,
                });
            }
            if args.len() >= 9 {
                let kind = string_arg(&args, 8, span)?;
                let kind = match kind.to_ascii_lowercase().as_str() {
                    "compile" => ActionKind::Compile,
                    "docs" => ActionKind::Docs,
                    "debug" => ActionKind::Debug,
                    "source_archive" | "source-archive" => ActionKind::SourceArchive,
                    "generic" => ActionKind::Generic,
                    _ => return Err(build_diag(&format!("unknown action kind `{kind}`"), span)),
                };
                spec = spec.with_kind(kind);
            }
            if args.len() >= 10 {
                for pool in string_list_arg(&args, 9, span)? {
                    let pool = match pool.to_ascii_lowercase().as_str() {
                        "cpu" => BuildResourcePool::CPU,
                        "memory" => BuildResourcePool::Memory,
                        "linker" => BuildResourcePool::Linker,
                        "console" => BuildResourcePool::Console,
                        "gpu" => BuildResourcePool::GPU,
                        _ => BuildResourcePool::Custom(pool),
                    };
                    spec = spec.with_pool(pool);
                }
            }
            if args.len() >= 11 {
                for entry in string_list_arg(&args, 10, span)? {
                    let (key, value) = entry.split_once('=').ok_or_else(|| {
                        build_diag("action environment entries must use KEY=VALUE", span)
                    })?;
                    spec = spec.with_env(key, value);
                }
            }
            if args.len() >= 12 {
                spec = spec.with_env_allowlist(string_list_arg(&args, 11, span)?);
            }
            if args.len() >= 13 {
                for entry in string_list_arg(&args, 12, span)? {
                    let (helper, version) = entry.split_once('=').ok_or_else(|| {
                        build_diag("action helper entries must use NAME=VERSION", span)
                    })?;
                    spec = spec.with_helper_version(helper, version);
                }
            }
            if args.len() >= 14 {
                for entry in string_list_arg(&args, 13, span)? {
                    let (label, value) = entry.split_once('=').ok_or_else(|| {
                        build_diag("action label entries must use NAME=VALUE", span)
                    })?;
                    spec = spec.with_label(label, value);
                }
            }
            if args.len() >= 15 {
                let cache = string_arg(&args, 14, span)?;
                spec.cache = match cache.to_ascii_lowercase().as_str() {
                    "cached" => ActionCache::Cached,
                    "phony" | "uncached" => ActionCache::UncachedPhony,
                    _ => return Err(build_diag("action cache must be cached or phony", span)),
                };
            }
            session
                .context
                .action(name, spec)
                .map(|handle| handle_value(crate::Syntax::TYPE_BUILD_ACTION, id, handle.id.0))
        }
        "legacy" => {
            let kind = string_arg(&args, 0, span)?;
            let name = string_arg(&args, 1, span)?;
            let inputs = string_list_arg(&args, 2, span)?;
            let outputs = string_list_arg(&args, 3, span)?;
            let argv = string_list_arg(&args, 4, span)?;
            let caps = string_list_arg(&args, 5, span)?;
            let mut wrapper = match kind.to_ascii_lowercase().as_str() {
                "cmake" => LegacyWrapperSpec::cmake(argv),
                "make" => LegacyWrapperSpec::make(argv),
                "gradle" => LegacyWrapperSpec::gradle(argv),
                "npm" => LegacyWrapperSpec::npm(argv),
                "cargo" => LegacyWrapperSpec::cargo(argv),
                _ => {
                    return Err(build_diag(
                        &format!("unknown legacy wrapper `{kind}`"),
                        span,
                    ))
                }
            }
            .with_inputs(inputs)
            .with_outputs(outputs);
            for cap in caps {
                wrapper = wrapper.with_cap(parse_capability(&cap, span)?);
            }
            if args.len() >= 7 {
                let toolchain =
                    handle_arg(&args, 6, crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, span)?;
                wrapper = wrapper.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
            }
            if args.len() >= 8 {
                for probe in handle_list_arg(&args, 7, crate::Syntax::TYPE_BUILD_PROBE, id, span)? {
                    wrapper = wrapper.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            if args.len() >= 9 {
                let signing = handle_arg(&args, 8, "BuildSigningIdentity", id, span)?;
                wrapper = wrapper.with_signing_identity(SigningIdentityHandle {
                    id: SigningIdentityId(signing),
                    context: id,
                });
            }
            if args.len() >= 10 {
                let kind = string_arg(&args, 9, span)?;
                wrapper = wrapper.with_kind(match kind.to_ascii_lowercase().as_str() {
                    "compile" => ActionKind::Compile,
                    "docs" => ActionKind::Docs,
                    "debug" => ActionKind::Debug,
                    "source_archive" | "source-archive" => ActionKind::SourceArchive,
                    "generic" => ActionKind::Generic,
                    _ => return Err(build_diag(&format!("unknown action kind `{kind}`"), span)),
                });
            }
            if args.len() >= 11 {
                for pool in string_list_arg(&args, 10, span)? {
                    let pool = match pool.to_ascii_lowercase().as_str() {
                        "cpu" => BuildResourcePool::CPU,
                        "memory" => BuildResourcePool::Memory,
                        "linker" => BuildResourcePool::Linker,
                        "console" => BuildResourcePool::Console,
                        "gpu" => BuildResourcePool::GPU,
                        _ => BuildResourcePool::Custom(pool),
                    };
                    wrapper = wrapper.with_pool(pool);
                }
            }
            if args.len() >= 12 {
                for entry in string_list_arg(&args, 11, span)? {
                    let (key, value) = entry.split_once('=').ok_or_else(|| {
                        build_diag(
                            "legacy wrapper environment entries must use KEY=VALUE",
                            span,
                        )
                    })?;
                    wrapper = wrapper.with_env(key, value);
                }
            }
            if args.len() >= 13 {
                wrapper = wrapper.with_env_allowlist(string_list_arg(&args, 12, span)?);
            }
            if args.len() >= 14 {
                for entry in string_list_arg(&args, 13, span)? {
                    let (helper, version) = entry.split_once('=').ok_or_else(|| {
                        build_diag("legacy wrapper helper entries must use NAME=VERSION", span)
                    })?;
                    wrapper = wrapper.with_helper_version(helper, version);
                }
            }
            if args.len() >= 15 {
                for entry in string_list_arg(&args, 14, span)? {
                    let (label, value) = entry.split_once('=').ok_or_else(|| {
                        build_diag("legacy wrapper label entries must use NAME=VALUE", span)
                    })?;
                    if matches!(label, "legacy.import" | "legacy.project-file") {
                        return Err(build_diag(
                            "legacy import labels are reserved; pass the canonical project file as the final argument",
                            span,
                        ));
                    }
                    wrapper = wrapper.with_label(label, value);
                }
            }
            if args.len() >= 16 {
                let cache = string_arg(&args, 15, span)?;
                wrapper = wrapper.with_cache(match cache.to_ascii_lowercase().as_str() {
                    "cached" => ActionCache::Cached,
                    "phony" | "uncached" => ActionCache::UncachedPhony,
                    _ => {
                        return Err(build_diag(
                            "legacy wrapper cache must be cached or phony",
                            span,
                        ))
                    }
                });
            }
            // A denied wrapper cannot contribute a graph. Preserve the
            // policy diagnostic before reading a canonical file that will
            // never be admitted, while allowed builds still take the exact
            // importer contract path below.
            if matches!(
                &session.context.policy().legacy_wrappers,
                super::actions_policy::PolicySetting::Deny(_)
            ) {
                let error = BuildError::PolicyDenied(wrapper.explain(session.context.policy()));
                return Ok(CtValue::failed(Box::new(build_error_value(&error, span))));
            }
            if args.len() >= 17 {
                let project_file = string_arg(&args, 16, span)?;
                if project_file != wrapper.kind.project_file() {
                    return Err(build_diag(
                        &format!(
                            "legacy {} import must use `{}`",
                            wrapper.kind.as_str(),
                            wrapper.kind.project_file()
                        ),
                        span,
                    ));
                }
                let imported = match LegacyWrapperSpec::from_project_file(
                    &session.project_root,
                    wrapper.kind,
                ) {
                    Ok(imported) => imported,
                    Err(error) => {
                        return Ok(CtValue::failed(Box::new(build_error_value(&error, span))))
                    }
                };
                validate_legacy_import_contract(&wrapper, &imported, span)?;
                for (label, value) in imported.labels {
                    if !matches!(label.as_str(), "legacy.import" | "legacy.project-file") {
                        wrapper = wrapper.with_label(label, value);
                    }
                }
                wrapper = wrapper.with_project_file(project_file);
            }
            let spec = match wrapper.into_action_spec(session.context.policy()) {
                Ok(spec) => spec,
                Err(error) => {
                    return Ok(CtValue::failed(Box::new(build_error_value(&error, span))))
                }
            };
            session
                .context
                .action(name, spec)
                .map(|handle| handle_value(crate::Syntax::TYPE_BUILD_ACTION, id, handle.id.0))
        }
        "add_executable" | "add_library" | "add_test" | "add_asset_bundle" | "add_doc"
        | "add_install" | "add_package" | "add_publish" => {
            let name = string_arg(&args, 0, span)?;
            let sources = string_list_arg(&args, 1, span)?;
            let actions = handle_list_arg(&args, 2, crate::Syntax::TYPE_BUILD_ACTION, id, span)?;
            let mut spec = TargetSpec::new();
            for source in sources {
                spec = spec.with_source(source);
            }
            for action in actions {
                spec = spec.with_action(ActionHandle {
                    id: ActionId(action),
                    context: id,
                });
            }
            if args.len() >= 4 {
                for target in handle_list_arg(&args, 3, crate::Syntax::TYPE_BUILD_TARGET, id, span)?
                {
                    spec = spec.with_dep(TargetRef {
                        id: TargetId(target),
                        context: id,
                    });
                }
            }
            if args.len() >= 5 {
                for probe in handle_list_arg(&args, 4, crate::Syntax::TYPE_BUILD_PROBE, id, span)? {
                    spec = spec.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            if args.len() >= 6 {
                let toolchain =
                    handle_arg(&args, 5, crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, span)?;
                spec = spec.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
            }
            if args.len() >= 7 {
                let signing = handle_arg(&args, 6, "BuildSigningIdentity", id, span)?;
                spec = spec.with_signing_identity(super::handles::SigningIdentityHandle {
                    id: super::handles::SigningIdentityId(signing),
                    context: id,
                });
            }
            let target = match method {
                "add_executable" => session.context.add_executable(name, spec).map(|h| h.id.0),
                "add_library" => session.context.add_library(name, spec).map(|h| h.id.0),
                "add_test" => session.context.add_test(name, spec).map(|h| h.id.0),
                "add_asset_bundle" => session.context.add_asset_bundle(name, spec).map(|h| h.id.0),
                "add_doc" => session.context.add_doc(name, spec).map(|h| h.id.0),
                "add_install" => session.context.add_install(name, spec).map(|h| h.id.0),
                "add_package" => session.context.add_package(name, spec).map(|h| h.id.0),
                "add_publish" => session.context.add_publish(name, spec).map(|h| h.id.0),
                _ => unreachable!(),
            };
            target.map(|target| handle_value(crate::Syntax::TYPE_BUILD_TARGET, id, target))
        }
        "toolchain" => {
            let name = string_arg(&args, 0, span)?;
            let triple = string_arg(&args, 1, span)?;
            let provenance = BuildProvenance::user_declared("fn build", None);
            let mut toolchain = ToolchainSpec::target(triple, provenance.clone());
            if args.len() >= 3 {
                toolchain = toolchain.with_host_triple(string_arg(&args, 2, span)?);
            }
            if args.len() >= 4 {
                let sdk = string_arg(&args, 3, span)?;
                toolchain =
                    toolchain.with_sdk(SdkIdentity::new(sdk, "declared", provenance.clone()));
            }
            if args.len() >= 5 {
                let linker = string_arg(&args, 4, span)?;
                toolchain = toolchain.with_linker(LinkerIdentity::new(linker, provenance.clone()));
            }
            if args.len() >= 6 {
                let sysroot = string_arg(&args, 5, span)?;
                toolchain = toolchain.with_sysroot(SysrootIdentity::new(
                    sysroot,
                    "declared",
                    provenance.clone(),
                ));
            }
            session
                .context
                .toolchain(name, toolchain)
                .map(|h| handle_value(crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, h.id.0))
        }
        "signing" => {
            let name = string_arg(&args, 0, span)?;
            let label = string_arg(&args, 1, span)?;
            session
                .context
                .signing_identity(
                    name,
                    SigningIdentitySpec::new(
                        label,
                        BuildProvenance::user_declared("fn build", None),
                    ),
                )
                .map(|handle| handle_value("BuildSigningIdentity", id, handle.id.0))
        }
        "probe" => {
            let name = string_arg(&args, 0, span)?;
            let kind = string_arg(&args, 1, span)?;
            let value = string_arg(&args, 2, span)?;
            let mut spec = match kind.as_str() {
                "find_program" => ProbeSpec::find_program(value),
                "pkg_config" => ProbeSpec::pkg_config(value),
                "header" => ProbeSpec::header_check(value),
                "compile_check" if (4..=6).contains(&args.len()) => ProbeSpec::compile_check(
                    value,
                    std::iter::empty::<String>(),
                    string_arg(&args, 3, span)?,
                ),
                _ => {
                    return Err(build_diag(
                        &format!("unknown typed probe kind `{kind}`"),
                        span,
                    ))
                }
            };
            let (reproducibility, toolchain_index) = if kind == "compile_check" {
                match args.get(4) {
                    Some(CtValue::Struct { .. }) => (None, Some(4)),
                    Some(_) => (args.get(4), (args.len() >= 6).then_some(5)),
                    None => (None, None),
                }
            } else {
                if args.len() > 5 {
                    return Err(build_diag(
                        "non-compile probes accept at most one reproducibility value and one toolchain",
                        span,
                    ));
                }
                match args.get(3) {
                    Some(CtValue::Struct { .. }) => (None, Some(3)),
                    Some(_) => (args.get(3), (args.len() >= 5).then_some(4)),
                    None => (None, None),
                }
            };
            if let Some(value) = reproducibility {
                let value = match value {
                    CtValue::Str(value) => value,
                    _ => {
                        return Err(build_diag(
                            "probe reproducibility must be `ambient` or `reproducible`",
                            span,
                        ))
                    }
                };
                spec = spec.with_reproducibility(match value.to_ascii_lowercase().as_str() {
                    "ambient" => ReproducibilityClass::Ambient,
                    "reproducible" => ReproducibilityClass::Reproducible,
                    _ => {
                        return Err(build_diag(
                            "probe reproducibility must be `ambient` or `reproducible`",
                            span,
                        ))
                    }
                });
            }
            if let Some(index) = toolchain_index {
                let toolchain =
                    handle_arg(&args, index, crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, span)?;
                spec = spec.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
            }
            session
                .context
                .probe(name, spec)
                .map(|h| handle_value(crate::Syntax::TYPE_BUILD_PROBE, id, h.id.0))
        }
        "error" => {
            let diagnostic_span = source_span_arg(&args, 0, span)?;
            let code = string_arg(&args, 1, span)?;
            let what = string_arg(&args, 2, span)?;
            let why = string_arg(&args, 3, span)?;
            let fix = string_arg(&args, 4, span)?;
            let reserved = code.len() > 1
                && matches!(code.as_bytes()[0], b'E' | b'W')
                && code.as_bytes()[1..].iter().all(u8::is_ascii_digit);
            if reserved {
                session.diagnostics.push(Diagnostic::error(
                    "E3530",
                    format!("build rule code `{code}` is reserved"),
                    "codes beginning with E or W followed only by digits belong to the compiler"
                        .to_string(),
                    "use a project prefix such as `ORG01`".to_string(),
                    Some(diagnostic_span),
                ));
            } else {
                match build_project_error(code, what, why, fix, Some(diagnostic_span)) {
                    Ok(diagnostic) => session.diagnostics.push(diagnostic),
                    Err(detail) => session
                        .diagnostics
                        .push(build_diag(detail, diagnostic_span)),
                }
            }
            return Ok(CtValue::Unit);
        }
        "plan" => {
            let default = if args.is_empty() {
                usize::MAX
            } else {
                handle_arg(&args, 0, crate::Syntax::TYPE_BUILD_TARGET, id, span)?
            };
            return Ok(CtValue::Present(Box::new(handle_value(
                crate::Syntax::TYPE_BUILD_PLAN,
                id,
                default,
            ))));
        }
        "contribute" => {
            if args.len() != 2 {
                return Err(build_diag(
                    "`b.contribute` requires a fact name and one value",
                    span,
                ));
            }
            let name = string_arg(&args, 0, span)?;
            if name.trim().is_empty() || name.contains('.') {
                return Err(build_diag(
                    "`b.contribute` needs one declared setting name, such as `cache_slots`",
                    span,
                ));
            }
            let value = fact_value_arg(&args[1], span)?;
            let contribution = jet_foundation::Policy::FactContribution::new(
                format!("Build.Settings.{name}"),
                value,
                jet_foundation::Policy::SourceScope::Function,
                jet_foundation::Policy::ContributionLayer::Environment,
                format!("{}::build", session.package),
            )
            .at(span)
            .with_reason("computed by fn build");
            session.context.contribute(contribution);
            return Ok(CtValue::Unit);
        }
        _ => {
            return None
                .ok_or_else(|| build_diag(&format!("unknown build method `{method}`"), span))
        }
    };
    Ok(match result {
        Ok(value) => CtValue::Present(Box::new(value)),
        Err(error) => CtValue::failed(Box::new(build_error_value(&error, span))),
    })
}

fn fact_value_arg(
    value: &CtValue,
    span: Span,
) -> Result<jet_foundation::Policy::FactValue, Diagnostic> {
    match value {
        CtValue::Bool(value) => Ok(jet_foundation::Policy::FactValue::Bool(*value)),
        CtValue::Int(value) => Ok(jet_foundation::Policy::FactValue::Int(*value)),
        CtValue::Char(value) => Ok(jet_foundation::Policy::FactValue::Char(*value)),
        CtValue::Str(value) => Ok(jet_foundation::Policy::FactValue::Text(value.clone())),
        CtValue::Enum { variant, args, .. } if args.is_empty() => {
            Ok(jet_foundation::Policy::FactValue::Enum(variant.clone()))
        }
        _ => Err(build_diag(
            "`b.contribute` accepts only Bool, Int, Char, String, or a fieldless enum value",
            span,
        )),
    }
}

/// A source-level `b.legacy` call declares the graph handles, while the
/// canonical project file remains the authority for the wrapper command and
/// discovered paths. Require the caller to repeat every imported fact in the
/// typed call. This makes the production graph inspectable and prevents a
/// stale source-side facade from silently discarding project-file semantics.
fn validate_legacy_import_contract(
    declared: &LegacyWrapperSpec,
    imported: &LegacyWrapperSpec,
    span: Span,
) -> Result<(), Diagnostic> {
    if declared.argv != imported.argv {
        return Err(build_diag(
            "legacy wrapper argv does not match its canonical project-file import",
            span,
        ));
    }
    let declared_inputs = declared
        .inputs
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let imported_inputs = imported
        .inputs
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if declared_inputs != imported_inputs {
        return Err(build_diag(
            "legacy wrapper inputs must exactly match its canonical project-file import",
            span,
        ));
    }
    let declared_outputs = declared
        .outputs
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let imported_outputs = imported
        .outputs
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if declared_outputs != imported_outputs {
        return Err(build_diag(
            "legacy wrapper outputs must exactly match its canonical project-file import",
            span,
        ));
    }
    if declared.caps != imported.caps {
        return Err(build_diag(
            "legacy wrapper abilities must exactly match its canonical project-file import",
            span,
        ));
    }
    if declared.env != imported.env {
        return Err(build_diag(
            "legacy wrapper environment does not match its canonical project-file import",
            span,
        ));
    }
    if declared.env_allowlist != imported.env_allowlist {
        return Err(build_diag(
            "legacy wrapper environment allowlist must exactly match its canonical project-file import",
            span,
        ));
    }
    if declared.resource_pools != imported.resource_pools {
        return Err(build_diag(
            "legacy wrapper resource pools must exactly match its canonical project-file import",
            span,
        ));
    }
    if imported.cache != ActionCache::Cached && declared.cache != imported.cache {
        return Err(build_diag(
            "legacy wrapper cache policy does not match its canonical project-file import",
            span,
        ));
    }
    if imported.action_kind != ActionKind::Generic && declared.action_kind != imported.action_kind {
        return Err(build_diag(
            "legacy wrapper action kind does not match its canonical project-file import",
            span,
        ));
    }
    Ok(())
}

fn build_session_id(value: &CtValue) -> Option<u64> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != crate::Syntax::TYPE_BUILD_CONTEXT {
        return None;
    }
    fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("__session", CtValue::Int(id)) if *id >= 0 => Some(*id as u64),
            _ => None,
        })
}

fn returned_handle(value: &CtValue, type_name: &str) -> Option<(u64, i64)> {
    let value = match value {
        CtValue::Present(value) => value.as_ref(),
        other => other,
    };
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return None;
    };
    if actual != type_name {
        return None;
    }
    let session = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("session", CtValue::Int(id)) => Some(*id as u64),
        _ => None,
    })?;
    let id = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("id", CtValue::Int(id)) => Some(*id),
        _ => None,
    })?;
    Some((session, id))
}

fn handle_value(type_name: &str, session: u64, id: usize) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![
            ("session".to_string(), CtValue::Int(session as i64)),
            ("id".to_string(), CtValue::Int(id as i64)),
        ],
    }
}

fn handle_arg(
    args: &[CtValue],
    index: usize,
    ty: &str,
    session: u64,
    span: Span,
) -> Result<usize, Diagnostic> {
    match args.get(index).and_then(|v| returned_handle(v, ty)) {
        Some((actual, id)) if actual == session && id >= 0 => Ok(id as usize),
        _ => Err(build_diag(
            &format!("argument {} must be a `{ty}` from this build", index + 1),
            span,
        )),
    }
}

fn handle_list_arg(
    args: &[CtValue],
    index: usize,
    ty: &str,
    session: u64,
    span: Span,
) -> Result<Vec<usize>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(
            &format!("argument {} must be a list", index + 1),
            span,
        ));
    };
    values
        .iter()
        .map(|value| handle_arg(std::slice::from_ref(value), 0, ty, session, span))
        .collect()
}

fn string_arg(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(build_diag(
            &format!("argument {} must be String", index + 1),
            span,
        )),
    }
}

fn source_span_arg(args: &[CtValue], index: usize, span: Span) -> Result<Span, Diagnostic> {
    let Some(CtValue::Struct { type_name, fields }) = args.get(index) else {
        return Err(build_diag(
            "custom build diagnostics require a source span",
            span,
        ));
    };
    if type_name != crate::Syntax::TYPE_SOURCE_SPAN {
        return Err(build_diag(
            "custom build diagnostics require a source span",
            span,
        ));
    }
    let value = |name: &str| {
        fields
            .iter()
            .find_map(|(field, value)| match (field.as_str(), value) {
                (actual, CtValue::Int(value)) if actual == name && *value >= 0 => {
                    Some(*value as usize)
                }
                _ => None,
            })
    };
    match (value("start"), value("end")) {
        (Some(start), Some(end)) if start <= end => Ok(Span::new(start, end)),
        _ => Err(build_diag(
            "custom build diagnostics require a valid source span",
            span,
        )),
    }
}

fn string_list_arg(args: &[CtValue], index: usize, span: Span) -> Result<Vec<String>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(
            &format!("argument {} must be [String]", index + 1),
            span,
        ));
    };
    values
        .iter()
        .map(|value| match value {
            CtValue::Str(value) => Ok(value.clone()),
            _ => Err(build_diag(
                &format!("argument {} must be [String]", index + 1),
                span,
            )),
        })
        .collect()
}

fn parse_capability(name: &str, span: Span) -> Result<BuildCapability, Diagnostic> {
    BuildCapability::parse(name).ok_or_else(|| {
        build_diag(
            &format!("unknown build effect `{name}`; use one of the ten declared effects"),
            span,
        )
    })
}

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn build_error_jet_error(error: &BuildError, span: Span) -> jet_foundation::Outcome::JetErr {
    let mut jet_error = jet_foundation::Outcome::jet_err_with_identity(
        build_error_text(error),
        Ok("E3502".to_string()),
        Err(jet_foundation::Outcome::JetAbsent),
        format!("BuildError::{}", build_error_variant(error)),
    );
    jet_foundation::Outcome::jet_err_set_details(
        &mut jet_error,
        build_error_details(error, span),
    );
    jet_error
}

fn build_error_value(error: &BuildError, span: Span) -> CtValue {
    CtValue::from_jet_err(&build_error_jet_error(error, span))
}

pub fn build_error_diagnostic_from_value(
    error: &CtValue,
    fallback_span: Span,
) -> Option<Diagnostic> {
    let jet_error = error.to_jet_err()?;
    jet_foundation::Outcome::jet_err_details(&jet_error)?;
    let report = jet_foundation::Outcome::jet_error_report(&jet_error);
    let span = report
        .details
        .as_ref()
        .and_then(|details| details.source_span.as_ref())
        .map_or(fallback_span, |span| Span::new(span.start, span.end));
    let mut diagnostic = build_diag(&report.message, span);
    diagnostic.structured = Some(StructuredDiagnostic::BuildError { report });
    Some(diagnostic)
}

fn build_error_diag(error: &BuildError, span: Span) -> Diagnostic {
    let report = jet_foundation::Outcome::jet_error_report(&build_error_jet_error(error, span));
    let mut diagnostic = build_diag(&report.message, span);
    diagnostic.structured = Some(StructuredDiagnostic::BuildError { report });
    diagnostic
}

fn build_error_variant(error: &BuildError) -> &'static str {
    match error {
        BuildError::EmptyTargetName => "EmptyTargetName",
        BuildError::EmptyActionName => "EmptyActionName",
        BuildError::EmptyToolchainName => "EmptyToolchainName",
        BuildError::EmptySigningIdentityName => "EmptySigningIdentityName",
        BuildError::EmptyProbeName => "EmptyProbeName",
        BuildError::DuplicateTargetName(_) => "DuplicateTargetName",
        BuildError::DuplicateActionName(_) => "DuplicateActionName",
        BuildError::CompilerPackageDependencyMissing { .. } => {
            "CompilerPackageDependencyMissing"
        }
        BuildError::DuplicateToolchainName(_) => "DuplicateToolchainName",
        BuildError::DuplicateSigningIdentityName(_) => "DuplicateSigningIdentityName",
        BuildError::DuplicateProbeName(_) => "DuplicateProbeName",
        BuildError::EmptyPath => "EmptyPath",
        BuildError::InvalidPath(_) => "InvalidPath",
        BuildError::EmptyToolchainTriple(_) => "EmptyToolchainTriple",
        BuildError::EmptyIdentityField(_) => "EmptyIdentityField",
        BuildError::MissingLockedProvenance(_) => "MissingLockedProvenance",
        BuildError::EmptyProbeField(_) => "EmptyProbeField",
        BuildError::EmptyActionArgv(_) => "EmptyActionArgv",
        BuildError::EmptyEnvName(_) => "EmptyEnvName",
        BuildError::UndeclaredEnvName { .. } => "UndeclaredEnvName",
        BuildError::CachedActionWithoutOutputs(_) => "CachedActionWithoutOutputs",
        BuildError::PhonyActionWithoutCaps(_) => "PhonyActionWithoutCaps",
        BuildError::PhonyActionWithOutputs(_) => "PhonyActionWithOutputs",
        BuildError::DuplicateActionOutput { .. } => "DuplicateActionOutput",
        BuildError::DuplicateBuildOutput { .. } => "DuplicateBuildOutput",
        BuildError::UnknownTarget(_) => "UnknownTarget",
        BuildError::UnknownAction(_) => "UnknownAction",
        BuildError::UnknownToolchain(_) => "UnknownToolchain",
        BuildError::UnknownSigningIdentity(_) => "UnknownSigningIdentity",
        BuildError::UnknownProbe(_) => "UnknownProbe",
        BuildError::LegacyWrapperWithoutInputs(_) => "LegacyWrapperWithoutInputs",
        BuildError::LegacyWrapperWithoutOutputs(_) => "LegacyWrapperWithoutOutputs",
        BuildError::LegacyWrapperWithoutCaps(_) => "LegacyWrapperWithoutCaps",
        BuildError::LegacyWrapperCommandMismatch { .. } => "LegacyWrapperCommandMismatch",
        BuildError::LegacyProjectFileMissing(_) => "LegacyProjectFileMissing",
        BuildError::LegacyProjectFileInvalid(_) => "LegacyProjectFileInvalid",
        BuildError::PolicyDenied(_) => "PolicyDenied",
        BuildError::EmptyPluginField(_) => "EmptyPluginField",
        BuildError::InvalidPluginDigest(_) => "InvalidPluginDigest",
        BuildError::PackagedPlugin(_) => "PackagedPlugin",
        BuildError::PluginVersionMismatch { .. } => "PluginVersionMismatch",
        BuildError::EmptyGeneratedModuleField(_) => "EmptyGeneratedModuleField",
        BuildError::InvalidGeneratedModulePath(_) => "InvalidGeneratedModulePath",
        BuildError::DuplicateGeneratedModuleName(_) => "DuplicateGeneratedModuleName",
        BuildError::DuplicateGeneratedModulePath(_) => "DuplicateGeneratedModulePath",
        BuildError::GeneratedModuleCycle { .. } => "GeneratedModuleCycle",
        BuildError::TargetDependencyCycle(_) => "TargetDependencyCycle",
        BuildError::ActionDependencyCycle(_) => "ActionDependencyCycle",
    }
}

fn build_error_details(
    error: &BuildError,
    span: Span,
) -> jet_foundation::Outcome::JetErrorDetails {
    let fields = match error {
        BuildError::EmptyTargetName
        | BuildError::EmptyActionName
        | BuildError::EmptyToolchainName
        | BuildError::EmptySigningIdentityName
        | BuildError::EmptyProbeName
        | BuildError::EmptyPath => Vec::new(),
        BuildError::DuplicateTargetName(value)
        | BuildError::DuplicateActionName(value)
        | BuildError::DuplicateToolchainName(value)
        | BuildError::DuplicateSigningIdentityName(value)
        | BuildError::DuplicateProbeName(value)
        | BuildError::InvalidPath(value)
        | BuildError::EmptyToolchainTriple(value)
        | BuildError::EmptyIdentityField(value)
        | BuildError::MissingLockedProvenance(value)
        | BuildError::EmptyProbeField(value)
        | BuildError::EmptyActionArgv(value)
        | BuildError::EmptyEnvName(value)
        | BuildError::CachedActionWithoutOutputs(value)
        | BuildError::PhonyActionWithoutCaps(value)
        | BuildError::PhonyActionWithOutputs(value)
        | BuildError::LegacyProjectFileInvalid(value)
        | BuildError::EmptyPluginField(value)
        | BuildError::InvalidPluginDigest(value)
        | BuildError::PackagedPlugin(value)
        | BuildError::EmptyGeneratedModuleField(value)
        | BuildError::InvalidGeneratedModulePath(value)
        | BuildError::DuplicateGeneratedModuleName(value)
        | BuildError::DuplicateGeneratedModulePath(value) => {
            vec![build_error_text_field("value", value)]
        }
        BuildError::CompilerPackageDependencyMissing {
            package,
            dependency,
        } => vec![
            build_error_text_field("package", package),
            build_error_text_field("dependency", dependency),
        ],
        BuildError::UndeclaredEnvName { action, key } => vec![
            build_error_text_field("action", action),
            build_error_text_field("key", key),
        ],
        BuildError::DuplicateActionOutput { action, output } => vec![
            build_error_text_field("action", action),
            build_error_text_field("output", output),
        ],
        BuildError::DuplicateBuildOutput {
            output,
            first_action,
            second_action,
        } => vec![
            build_error_text_field("output", output),
            build_error_text_field("first_action", first_action),
            build_error_text_field("second_action", second_action),
        ],
        BuildError::UnknownTarget(id) => vec![build_error_number_field("id", id.0)],
        BuildError::UnknownAction(id) => vec![build_error_number_field("id", id.0)],
        BuildError::UnknownToolchain(id) => vec![build_error_number_field("id", id.0)],
        BuildError::UnknownSigningIdentity(id) => vec![build_error_number_field("id", id.0)],
        BuildError::UnknownProbe(id) => vec![build_error_number_field("id", id.0)],
        BuildError::LegacyWrapperWithoutInputs(kind)
        | BuildError::LegacyWrapperWithoutOutputs(kind)
        | BuildError::LegacyWrapperWithoutCaps(kind)
        | BuildError::LegacyProjectFileMissing(kind) => {
            vec![build_error_text_field("kind", kind.as_str())]
        }
        BuildError::LegacyWrapperCommandMismatch { wrapper, actual } => vec![
            build_error_text_field("wrapper", wrapper.as_str()),
            build_error_text_field("actual", actual),
        ],
        BuildError::PolicyDenied(explanation) => vec![
            build_error_text_field("subject", &explanation.subject),
            build_error_bool_field("allowed", explanation.allowed),
            build_error_text_field("reason", &explanation.reason),
            build_error_json_field(
                "required_caps",
                format!(
                    "[{}]",
                    explanation
                        .required_caps
                        .iter()
                        .map(|cap| build_error_json_string(cap.name()))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            ),
        ],
        BuildError::PluginVersionMismatch {
            plugin,
            expected,
            actual,
        } => vec![
            build_error_text_field("plugin", plugin),
            build_error_text_field("expected", expected),
            build_error_text_field("actual", actual),
        ],
        BuildError::GeneratedModuleCycle { module, path } => vec![
            build_error_text_field("module", module),
            build_error_text_field("path", path),
        ],
        BuildError::TargetDependencyCycle(cycle)
        | BuildError::ActionDependencyCycle(cycle) => vec![build_error_json_field(
            "nodes",
            format!(
                "[{}]",
                cycle
                    .nodes()
                    .iter()
                    .map(|node| build_error_json_string(node))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        )],
    };
    jet_foundation::Outcome::JetErrorDetails {
        variant: build_error_variant(error).to_string(),
        fields,
        source_span: Some(jet_foundation::Outcome::JetErrorSpan {
            start: span.start,
            end: span.end,
        }),
    }
}

fn build_error_text_field(name: &str, value: &str) -> jet_foundation::Outcome::JetErrorField {
    build_error_json_field(name, build_error_json_string(value))
}

fn build_error_number_field(name: &str, value: usize) -> jet_foundation::Outcome::JetErrorField {
    build_error_json_field(name, value.to_string())
}

fn build_error_bool_field(
    name: &str,
    value: bool,
) -> jet_foundation::Outcome::JetErrorField {
    build_error_json_field(name, value.to_string())
}

fn build_error_json_field(name: &str, value: String) -> jet_foundation::Outcome::JetErrorField {
    jet_foundation::Outcome::JetErrorField {
        name: name.to_string(),
        value,
    }
}

fn build_error_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn build_error_text(error: &BuildError) -> String {
    match error {
        BuildError::DuplicateTargetName(name) => {
            format!("target name `{name}` is registered twice")
        }
        BuildError::DuplicateActionName(name) => {
            format!("action name `{name}` is registered twice")
        }
        BuildError::CompilerPackageDependencyMissing {
            package,
            dependency,
        } => format!(
            "compiler package `{package}` depends on missing package artifact `{dependency}`"
        ),
        BuildError::DuplicateToolchainName(name) => {
            format!("toolchain name `{name}` is registered twice")
        }
        BuildError::DuplicateProbeName(name) => format!("probe name `{name}` is registered twice"),
        BuildError::InvalidPath(path) => {
            format!("build path `{path}` must be relative and stay inside the project")
        }
        BuildError::LegacyProjectFileMissing(kind) => format!(
            "legacy {} import requires `{}` in the project root",
            kind.as_str(),
            kind.project_file()
        ),
        BuildError::LegacyProjectFileInvalid(path) => {
            format!("legacy project file `{path}` must be a readable regular file")
        }
        BuildError::UndeclaredEnvName { action, key } => {
            format!("action `{action}` allowlists undeclared environment variable `{key}`")
        }
        BuildError::PackagedPlugin(message) => format!("packaged build plugin rejected: {message}"),
        BuildError::InvalidPluginDigest(message) => {
            format!("build plugin component digest is invalid: {message}")
        }
        BuildError::CachedActionWithoutOutputs(name) => {
            format!("cached action `{name}` declares no outputs")
        }
        BuildError::PhonyActionWithoutCaps(name) => {
            format!("uncached action `{name}` declares no abilities")
        }
        BuildError::DuplicateBuildOutput {
            output,
            first_action,
            second_action,
        } => format!("output `{output}` is owned by both `{first_action}` and `{second_action}`"),
        BuildError::DuplicateGeneratedModuleName(name) => {
            format!("E3511: generation rounds form a cycle: module `{name}` is generated twice")
        }
        BuildError::DuplicateGeneratedModulePath(path) => {
            format!("E3511: generation rounds form a cycle: path `{path}` is generated twice")
        }
        BuildError::GeneratedModuleCycle { module, path } => format!(
            "E3511: generation rounds form a cycle: `{module}` and `{path}` have two producers"
        ),
        BuildError::TargetDependencyCycle(cycle) => dependency_cycle_text("target", cycle),
        BuildError::ActionDependencyCycle(cycle) => dependency_cycle_text("action", cycle),
        other => format!("{other:?}"),
    }
}

fn build_diag(detail: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("build plan is invalid: {detail}"),
        "`fn build` must return one deterministic typed graph whose handles all belong to that build".to_string(),
        "fix the named target/action/toolchain/probe and run `jet inspect explain-build` to inspect the graph".to_string(),
        Some(span),
    )
}

fn build_project_error(
    code: String,
    what: String,
    why: String,
    fix: String,
    span: Option<Span>,
) -> Result<Diagnostic, &'static str> {
    Diagnostic::project_error(code, what, why, fix, span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::CtReport;

    #[test]
    fn build_context_failure_keeps_typed_report_facts() {
        let context = begin_program_build("report-test", CtValue::Unit);
        let span = Span::new(11, 17);
        let args = || {
            vec![
                CtValue::Str("app".to_string()),
                CtValue::List(Vec::new()),
                CtValue::List(Vec::new()),
            ]
        };

        let first = eval_program_build_method(
            &context,
            "add_executable",
            args(),
            None,
            span,
            false,
        )
        .expect("build context")
        .expect("first target declaration");
        assert!(first.is_present());

        let mut failed = eval_program_build_method(
            &context,
            "add_executable",
            args(),
            None,
            span,
            false,
        )
        .expect("build context")
        .expect("duplicate target result");
        assert!(matches!(failed, CtValue::Failed(CtReport::Told(_))));
        assert!(failed.add_error_context(
            "while declaring app".to_string(),
            "build.jet".to_string(),
            23,
        ));

        jet_foundation::Outcome::jet_journey_reset();
        jet_foundation::Outcome::jet_journey_frame("build.jet", 23, "build", || {
            "while declaring app".to_string()
        });
        let jet_error = failed.to_jet_err().expect("default error carrier");
        let report = jet_foundation::Outcome::jet_error_report(&jet_error);

        assert_eq!(report.typed_identity.as_deref(), Some("BuildError::DuplicateTargetName"));
        let details = report.details.as_ref().expect("typed build details");
        assert_eq!(details.variant, "DuplicateTargetName");
        assert_eq!(details.fields.len(), 1);
        assert_eq!(details.fields[0].name, "value");
        assert_eq!(details.fields[0].value, "\"app\"");
        assert_eq!(
            details.source_span.as_ref(),
            Some(&jet_foundation::Outcome::JetErrorSpan { start: 11, end: 17 })
        );
        assert!(report.causes.is_empty());
        assert_eq!(report.context_frames.len(), 1);
        assert_eq!(report.context_frames[0].text, "while declaring app");
        assert_eq!(report.context_frames[0].file, "build.jet");
        assert_eq!(report.context_frames[0].line, 23);
        assert_eq!(report.source_journey.len(), 1);
        assert_eq!(report.source_journey[0].fn_name, "build");
        assert_eq!(report.source_journey[0].file, "build.jet");
        assert_eq!(report.source_journey[0].line, 23);
        assert!(report.to_json().contains(
            "\"details\":{\"variant\":\"DuplicateTargetName\",\"fields\":{\"value\":\"app\"}"
        ));

        let diagnostic = build_error_diagnostic_from_value(&failed, Span::new(0, 0))
            .expect("structured build diagnostic");
        assert!(matches!(
            diagnostic.structured,
            Some(StructuredDiagnostic::BuildError { .. })
        ));

        abort_program_build(&context);
    }
}
