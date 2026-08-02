use super::actions_policy::{
    ActionCache, ActionKind, ActionSpec, BuildCapability, BuildPolicy, BuildResourcePool, LegacyWrapperSpec,
};
use super::context::BuildContext;
use super::errors_keys::BuildError;
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
use crate::AST::{ComptimeInput, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct ProgramBuildSession {
    context: BuildContext,
    package: String,
    project_root: PathBuf,
    diagnostics: Vec<Diagnostic>,
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

pub fn finish_program_build(context: &CtValue, returned: &CtValue) -> Result<(BuildPlan, Vec<Diagnostic>), Diagnostic> {
    let id = build_session_id(context).ok_or_else(|| build_diag("lost build context", Span::new(0, 0)))?;
    let default = returned_handle(returned, crate::Syntax::TYPE_BUILD_PLAN)
        .and_then(|(_, target)| (target >= 0).then_some(target as usize));
    let session = PROGRAM_BUILD_SESSIONS.with(|sessions| sessions.borrow_mut().remove(&id));
    let session = session.ok_or_else(|| build_diag("build context expired", Span::new(0, 0)))?;
    let plan = match default {
        Some(target) => session
            .context
            .plan_with_default(TargetRef {
                id: TargetId(target),
                context: id,
            })
            .map_err(|e| build_error_diag(&e, Span::new(0, 0))),
        None => session
            .context
            .plan()
            .map_err(|e| build_error_diag(&e, Span::new(0, 0))),
    }?;
    Ok((plan, session.diagnostics))
}

#[doc(hidden)]
pub fn eval_program_build_method(
    receiver: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
    in_impure_gate: bool,
) -> Option<Result<CtValue, Diagnostic>> {
    let id = build_session_id(receiver)?;
    Some(PROGRAM_BUILD_SESSIONS.with(|sessions| {
        let mut sessions = sessions.borrow_mut();
        let session = sessions
            .get_mut(&id)
            .ok_or_else(|| build_diag("build context expired", span))?;
        eval_session_method(id, session, method, args, span, in_impure_gate)
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
        "find" => Some(
            (|| {
                if args.len() != 1 {
                    return Err(build_diag("`b.find` requires exactly one glob", span));
                }
                let builtin = crate::Syntax::BUILTIN_FIND;
                let glob = first_string_literal.ok_or_else(|| {
                    crate::Comptime::Methods::embed_path_err(builtin, "literal", span)
                })?;
                let glob =
                    crate::Comptime::Methods::check_literal_embed_path(builtin, glob, span)?;
                crate::Comptime::Methods::eval_locked_find(
                    base_dir,
                    &glob,
                    embed_inputs,
                    span,
                )
            })(),
        ),
        "embed" => Some(crate::Comptime::Methods::eval_build_embed(
            args,
            base_dir,
            embed_inputs,
            span,
        )),
        "fetch" => Some(
            crate::Comptime::Methods::eval_net_fetch(args, embed_inputs, span)
                .map(|value| CtValue::ResOk(Box::new(value))),
        ),
        _ => None,
    }
}

fn eval_session_method(
    id: u64,
    session: &mut ProgramBuildSession,
    method: &str,
    args: Vec<CtValue>,
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
            format!("`b.{method}` touches the ambient world and must be inside `#Impure(\"reason\")`"),
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
            let name = string_arg(&args, 0, span)?;
            let source = string_arg(&args, 1, span)?;
            let path = format!(".jet/generated/{}/{}.jet", session.package, safe_name(&name));
            session.context.generate(name, path, source).map(|_| CtValue::Unit)
        }
        "action" => {
            let name = string_arg(&args, 0, span)?;
            let inputs = string_list_arg(&args, 1, span)?;
            let outputs = string_list_arg(&args, 2, span)?;
            let argv = string_list_arg(&args, 3, span)?;
            let caps = string_list_arg(&args, 4, span)?;
            let mut spec = ActionSpec::cached(argv).with_inputs(inputs).with_outputs(outputs);
            for cap in caps {
                spec = spec.with_cap(parse_capability(&cap, span)?);
            }
            if args.len() >= 7 {
                let toolchain = handle_arg(
                    &args,
                    5,
                    crate::Syntax::TYPE_BUILD_TOOLCHAIN,
                    id,
                    span,
                )?;
                spec = spec.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
                for probe in handle_list_arg(
                    &args,
                    6,
                    crate::Syntax::TYPE_BUILD_PROBE,
                    id,
                    span,
                )? {
                    spec = spec.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            if args.len() >= 8 {
                let signing = handle_arg(
                    &args,
                    7,
                    "BuildSigningIdentity",
                    id,
                    span,
                )?;
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
                        "cpu" => BuildResourcePool::Cpu,
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
                _ => return Err(build_diag(&format!("unknown legacy wrapper `{kind}`"), span)),
            }
            .with_inputs(inputs)
            .with_outputs(outputs);
            for cap in caps {
                wrapper = wrapper.with_cap(parse_capability(&cap, span)?);
            }
            if args.len() >= 7 {
                let toolchain = handle_arg(
                    &args,
                    6,
                    crate::Syntax::TYPE_BUILD_TOOLCHAIN,
                    id,
                    span,
                )?;
                wrapper = wrapper.with_toolchain(ToolchainHandle {
                    id: ToolchainId(toolchain),
                    context: id,
                });
            }
            if args.len() >= 8 {
                for probe in handle_list_arg(
                    &args,
                    7,
                    crate::Syntax::TYPE_BUILD_PROBE,
                    id,
                    span,
                )? {
                    wrapper = wrapper.with_probe(ProbeHandle {
                        id: ProbeId(probe),
                        context: id,
                    });
                }
            }
            if args.len() >= 9 {
                let signing = handle_arg(
                    &args,
                    8,
                    "BuildSigningIdentity",
                    id,
                    span,
                )?;
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
                        "cpu" => BuildResourcePool::Cpu,
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
                        build_diag("legacy wrapper environment entries must use KEY=VALUE", span)
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
                    _ => return Err(build_diag("legacy wrapper cache must be cached or phony", span)),
                });
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
                let imported = LegacyWrapperSpec::from_project_file(
                    &session.project_root,
                    wrapper.kind,
                )
                .map_err(|error| build_error_diag(&error, span))?;
                validate_legacy_import_contract(&wrapper, &imported, span)?;
                for (label, value) in imported.labels {
                    if !matches!(label.as_str(), "legacy.import" | "legacy.project-file") {
                        wrapper = wrapper.with_label(label, value);
                    }
                }
                wrapper = wrapper.with_project_file(project_file);
            }
            let spec = wrapper
                .into_action_spec(session.context.policy())
                .map_err(|error| build_error_diag(&error, span))?;
            session
                .context
                .action(name, spec)
                .map(|handle| handle_value(crate::Syntax::TYPE_BUILD_ACTION, id, handle.id.0))
        }
        "add_executable" | "add_library" | "add_test" | "add_bench" | "add_asset_bundle"
        | "add_doc" | "add_install" | "add_package" | "add_publish" => {
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
                for target in handle_list_arg(&args, 3, crate::Syntax::TYPE_BUILD_TARGET, id, span)? {
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
                let toolchain = handle_arg(&args, 5, crate::Syntax::TYPE_BUILD_TOOLCHAIN, id, span)?;
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
                "add_bench" => session.context.add_bench(name, spec).map(|h| h.id.0),
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
                toolchain = toolchain.with_sdk(SdkIdentity::new(sdk, "declared", provenance.clone()));
            }
            if args.len() >= 5 {
                let linker = string_arg(&args, 4, span)?;
                toolchain = toolchain.with_linker(LinkerIdentity::new(linker, provenance.clone()));
            }
            if args.len() >= 6 {
                let sysroot = string_arg(&args, 5, span)?;
                toolchain = toolchain.with_sysroot(SysrootIdentity::new(sysroot, "declared", provenance.clone()));
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
                _ => return Err(build_diag(&format!("unknown typed probe kind `{kind}`"), span)),
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
                    _ => return Err(build_diag("probe reproducibility must be `ambient` or `reproducible`", span)),
                };
                spec = spec.with_reproducibility(match value.to_ascii_lowercase().as_str() {
                    "ambient" => ReproducibilityClass::Ambient,
                    "reproducible" => ReproducibilityClass::Reproducible,
                    _ => return Err(build_diag("probe reproducibility must be `ambient` or `reproducible`", span)),
                });
            }
            if let Some(index) = toolchain_index {
                let toolchain = handle_arg(
                    &args,
                    index,
                    crate::Syntax::TYPE_BUILD_TOOLCHAIN,
                    id,
                    span,
                )?;
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
                    "codes beginning with E or W followed only by digits belong to the compiler".to_string(),
                    "use a project prefix such as `ORG01`".to_string(),
                    Some(diagnostic_span),
                ));
            } else {
                session.diagnostics.push(Diagnostic::error(code, what, why, fix, Some(diagnostic_span)));
            }
            return Ok(CtValue::Unit);
        }
        "plan" => {
            let default = if args.is_empty() {
                usize::MAX
            } else {
                handle_arg(&args, 0, crate::Syntax::TYPE_BUILD_TARGET, id, span)?
            };
            return Ok(CtValue::ResOk(Box::new(handle_value(
                crate::Syntax::TYPE_BUILD_PLAN,
                id,
                default,
            ))));
        }
        // D-BUILDCTX-FLAGS1=A
        "default_profile" => {
            let profile = match args.first() {
                Some(CtValue::Str(value)) => value.clone(),
                Some(CtValue::Enum { variant, .. }) => variant.clone(),
                Some(CtValue::Struct { type_name, fields }) if type_name.ends_with("Profile") || type_name.contains("Build") => {
                    fields
                        .iter()
                        .find_map(|(n, v)| match (n.as_str(), v) {
                            ("name" | "tag" | "variant", CtValue::Str(s)) => Some(s.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "release".to_string())
                }
                _ => {
                    // Leading-dot enum often arrives as enum variant name via show.
                    return Err(build_diag(
                        "`b.default_profile` needs a profile name or `.Release` / `.Debug`",
                        span,
                    ));
                }
            };
            let profile = match profile.to_ascii_lowercase().as_str() {
                "release" => "release".to_string(),
                "debug" => "debug".to_string(),
                "ci" => "ci".to_string(),
                other => other.to_string(),
            };
            session.context.default_profile(profile);
            return Ok(CtValue::Unit);
        }
        "default_allow" => {
            let Some(CtValue::List(values)) = args.first() else {
                return Err(build_diag("`b.default_allow` needs a list of effects", span));
            };
            let mut effects = Vec::new();
            for value in values {
                let name = match value {
                    CtValue::Str(s) => s.clone(),
                    CtValue::Enum { variant, .. } => variant.clone(),
                    other => {
                        return Err(build_diag(
                            &format!("`b.default_allow` entry must be an effect, got {other:?}"),
                            span,
                        ));
                    }
                };
                if crate::Comptime::Build::BuildCapability::parse(&name).is_none() {
                    return Err(build_diag(
                        &format!("unknown build effect `{name}` in default_allow"),
                        span,
                    ));
                }
                effects.push(name);
            }
            session.context.default_allow(effects);
            return Ok(CtValue::Unit);
        }
        _ => return None.ok_or_else(|| build_diag(&format!("unknown build method `{method}`"), span)),
    };
    Ok(match result {
        Ok(value) => CtValue::ResOk(Box::new(value)),
        Err(error) => CtValue::ResErr(Box::new(CtValue::Str(build_error_text(&error)))),
    })
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
    let declared_inputs = declared.inputs.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    let imported_inputs = imported.inputs.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    if declared_inputs != imported_inputs {
        return Err(build_diag(
            "legacy wrapper inputs must exactly match its canonical project-file import",
            span,
        ));
    }
    let declared_outputs = declared.outputs.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    let imported_outputs = imported.outputs.iter().cloned().collect::<std::collections::BTreeSet<_>>();
    if declared_outputs != imported_outputs {
        return Err(build_diag(
            "legacy wrapper outputs must exactly match its canonical project-file import",
            span,
        ));
    }
    if declared.caps != imported.caps {
        return Err(build_diag(
            "legacy wrapper capabilities must exactly match its canonical project-file import",
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
    let CtValue::Struct { type_name, fields } = value else { return None };
    if type_name != crate::Syntax::TYPE_BUILD_CONTEXT { return None; }
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("__session", CtValue::Int(id)) if *id >= 0 => Some(*id as u64),
        _ => None,
    })
}

fn returned_handle(value: &CtValue, type_name: &str) -> Option<(u64, i64)> {
    let value = match value { CtValue::ResOk(value) => value.as_ref(), other => other };
    let CtValue::Struct { type_name: actual, fields } = value else { return None };
    if actual != type_name { return None; }
    let session = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("session", CtValue::Int(id)) => Some(*id as u64), _ => None
    })?;
    let id = fields.iter().find_map(|(n, v)| match (n.as_str(), v) {
        ("id", CtValue::Int(id)) => Some(*id), _ => None
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

fn handle_arg(args: &[CtValue], index: usize, ty: &str, session: u64, span: Span) -> Result<usize, Diagnostic> {
    match args.get(index).and_then(|v| returned_handle(v, ty)) {
        Some((actual, id)) if actual == session && id >= 0 => Ok(id as usize),
        _ => Err(build_diag(&format!("argument {} must be a `{ty}` from this build", index + 1), span)),
    }
}

fn handle_list_arg(args: &[CtValue], index: usize, ty: &str, session: u64, span: Span) -> Result<Vec<usize>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(&format!("argument {} must be a list", index + 1), span));
    };
    values.iter().map(|value| handle_arg(std::slice::from_ref(value), 0, ty, session, span)).collect()
}

fn string_arg(args: &[CtValue], index: usize, span: Span) -> Result<String, Diagnostic> {
    match args.get(index) {
        Some(CtValue::Str(value)) => Ok(value.clone()),
        _ => Err(build_diag(&format!("argument {} must be String", index + 1), span)),
    }
}

fn source_span_arg(args: &[CtValue], index: usize, span: Span) -> Result<Span, Diagnostic> {
    let Some(CtValue::Struct { type_name, fields }) = args.get(index) else {
        return Err(build_diag("custom build diagnostics require a source span", span));
    };
    if type_name != crate::Syntax::TYPE_SOURCE_SPAN {
        return Err(build_diag("custom build diagnostics require a source span", span));
    }
    let value = |name: &str| {
        fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
            (actual, CtValue::Int(value)) if actual == name && *value >= 0 => Some(*value as usize),
            _ => None,
        })
    };
    match (value("start"), value("end")) {
        (Some(start), Some(end)) if start <= end => Ok(Span::new(start, end)),
        _ => Err(build_diag("custom build diagnostics require a valid source span", span)),
    }
}

fn string_list_arg(args: &[CtValue], index: usize, span: Span) -> Result<Vec<String>, Diagnostic> {
    let Some(CtValue::List(values)) = args.get(index) else {
        return Err(build_diag(&format!("argument {} must be [String]", index + 1), span));
    };
    values.iter().map(|value| match value {
        CtValue::Str(value) => Ok(value.clone()),
        _ => Err(build_diag(&format!("argument {} must be [String]", index + 1), span)),
    }).collect()
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
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn build_error_diag(error: &BuildError, span: Span) -> Diagnostic {
    build_diag(&build_error_text(error), span)
}

fn build_error_text(error: &BuildError) -> String {
    match error {
        BuildError::DuplicateTargetName(name) => format!("target name `{name}` is registered twice"),
        BuildError::DuplicateActionName(name) => format!("action name `{name}` is registered twice"),
        BuildError::DuplicateToolchainName(name) => format!("toolchain name `{name}` is registered twice"),
        BuildError::DuplicateProbeName(name) => format!("probe name `{name}` is registered twice"),
        BuildError::InvalidPath(path) => {
            format!("build path `{path}` must be relative and stay inside the project")
        }
        BuildError::LegacyProjectFileMissing(kind) => format!(
            "legacy {} import requires `{}` in the project root",
            kind.as_str(),
            kind.project_file()
        ),
        BuildError::LegacyProjectFileInvalid(path) => format!(
            "legacy project file `{path}` must be a readable regular file"
        ),
        BuildError::UndeclaredEnvName { action, key } => {
            format!("action `{action}` allowlists undeclared environment variable `{key}`")
        }
        BuildError::PackagedPlugin(message) => format!("packaged build plugin rejected: {message}"),
        BuildError::InvalidPluginDigest(message) => {
            format!("build plugin component digest is invalid: {message}")
        }
        BuildError::CachedActionWithoutOutputs(name) => format!("cached action `{name}` declares no outputs"),
        BuildError::PhonyActionWithoutCaps(name) => format!("uncached action `{name}` declares no capabilities"),
        BuildError::DuplicateBuildOutput { output, first_action, second_action } => format!(
            "output `{output}` is owned by both `{first_action}` and `{second_action}`"
        ),
        BuildError::DuplicateGeneratedModuleName(name) => {
            format!("E3511: generation rounds form a cycle: module `{name}` is generated twice")
        }
        BuildError::DuplicateGeneratedModulePath(path) => {
            format!("E3511: generation rounds form a cycle: path `{path}` is generated twice")
        }
        BuildError::GeneratedModuleCycle { module, path } => format!(
            "E3511: generation rounds form a cycle: `{module}` and `{path}` have two producers"
        ),
        BuildError::TargetDependencyCycle => "target dependency graph contains a cycle".to_string(),
        BuildError::ActionDependencyCycle => "action dependency graph contains a cycle".to_string(),
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
