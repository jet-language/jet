//! Pipeline composition — the compiler's execution stages assembled in one place.
//!
//! `lib.rs` public functions are thin facades over these. `LSP/Check.rs` calls
//! `check_file` directly for document checking.

use crate::Diagnostics::{Diagnostic, Severity};
use std::path::Path;

/// Main pipeline: load from file path → sema → ffi → codegen.
///
/// D-OSTARGET1=A (ratified 2026-07-01, c134): `cross_target` is the raw
/// `--target=<triple>` string (or `None`) — reused as-is from the existing
/// E2-M15 cross-compile flag, resolved to a native OS bucket in
/// `compile_bundle_path_opts_dbg` (host OS when `None` or unrecognized).
pub fn compile_bundle_path_opts(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        allow_impure,
        web_target,
        false,
        false,
        cross_target,
        None,
    )
}

#[derive(Debug)]
pub enum TargetProfileCompileError {
    Diagnostics(Vec<Diagnostic>),
    Profile(Vec<crate::TargetProfile::TargetProfileError>),
}

/// D-TARGET-* production hook: validate a selected typed target profile from
/// sema facts before codegen. CLI/UI wording remains future work; this returns
/// profile errors as data.
pub fn compile_bundle_path_with_target_profile(
    file: &str,
    mode: crate::Sema::CompileMode,
    profile: &crate::TargetProfile::TargetProfile,
) -> Result<crate::CompileOutput, TargetProfileCompileError> {
    let usage = target_profile_usage_for_file(file, mode)
        .map_err(TargetProfileCompileError::Diagnostics)?;
    let profile_errors = profile.validate(&usage);
    if !profile_errors.is_empty() {
        return Err(TargetProfileCompileError::Profile(profile_errors));
    }
    compile_bundle_path_opts_full(
        file,
        mode,
        profile.no_os,
        false,
        false,
        false,
        false,
        Some(profile.triple.as_str()),
        None,
    )
    .map_err(TargetProfileCompileError::Diagnostics)
}

/// Like `compile_bundle_path_opts`, but for `jet build --target=plugin`
/// (D-PLUGIN1=B / D-DEP-WASM1=A, c81): also emits the guest `.wit` + wasm32
/// Rust artifacts (`Codegen::emit_plugin`).
pub fn compile_bundle_path_opts_plugin(
    file: &str,
    mode: crate::Sema::CompileMode,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(file, mode, false, false, false, true, false, cross_target, None)
}

/// Like `compile_bundle_path_opts`, but `debug_linemap = true` routes codegen
/// through `emit_bundle_dbg` (D-DBG3 step 2 / dap-debugger): every generated
/// statement gets a `// jet:line N` marker the native `jet debug` backend reads
/// back into a rust-line -> jet-line table. Used ONLY by the native debug build
/// path — every other caller keeps `debug_linemap = false` (byte-identical output).
pub fn compile_bundle_path_opts_dbg(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        mode,
        freestanding,
        allow_impure,
        web_target,
        false,
        debug_linemap,
        cross_target,
        None,
    )
}

/// Compile one explicitly addressed runnable Output. Selection is resolved in
/// sema and carried into every lower tier as one checked callable fact.
pub fn compile_bundle_path_output(
    file: &str,
    output: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_output_opts(file, output, false, false, false, false, None)
}

pub fn compile_bundle_path_output_opts(
    file: &str,
    output: &str,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    plugin_target: bool,
    cross_target: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_opts_full(
        file,
        crate::Sema::CompileMode::Run,
        freestanding,
        allow_impure,
        web_target,
        plugin_target,
        false,
        cross_target,
        Some(output),
    )
}

fn target_profile_usage_for_file(
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::TargetProfile::TargetProfileUse, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    let mut errors = Vec::new();
    for d in std::mem::take(&mut bundle.parse_teaching)
        .into_iter()
        .chain(diags)
    {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mmio = collect_mmio_usage(&bundle);
    let mut core_apis: Vec<String> = bundle.used_core.into_iter().collect();
    core_apis.sort();
    Ok(crate::TargetProfile::TargetProfileUse {
        core_apis,
        mmio,
        ..crate::TargetProfile::TargetProfileUse::default()
    })
}

#[derive(Clone, Copy)]
struct PtrFact {
    address: u64,
    size: crate::TargetProfile::ByteSize,
}

fn collect_mmio_usage(bundle: &crate::AST::ProgramBundle) -> Vec<crate::TargetProfile::MmioAccess> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        let core_aliases = core_aliases(module);
        for item in &module.items {
            match item {
                crate::AST::Item::Func(f) => collect_mmio_func(f, &core_aliases, &mut out),
                crate::AST::Item::Struct(s) => {
                    for m in &s.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                crate::AST::Item::Enum(e) => {
                    for m in &e.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                crate::AST::Item::Impl(i) => {
                    for m in &i.methods {
                        collect_mmio_func(m, &core_aliases, &mut out);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn core_aliases(module: &crate::AST::LoadedModule) -> std::collections::HashMap<String, String> {
    let mut aliases = std::collections::HashMap::new();
    for import in &module.imports {
        if let crate::AST::ImportKind::Module(name, _) = &import.kind {
            if crate::Syntax::is_known_core_module(name) {
                let alias = if import.alias.is_empty() {
                    name.rsplit('.').next().unwrap_or(name).to_string()
                } else {
                    import.alias.clone()
                };
                aliases.insert(alias, name.clone());
            }
        }
    }
    aliases
}

fn collect_mmio_func(
    f: &crate::AST::Func,
    core_aliases: &std::collections::HashMap<String, String>,
    out: &mut Vec<crate::TargetProfile::MmioAccess>,
) {
    let mut ptrs = std::collections::HashMap::new();
    let reason = if f.is_unsafe {
        f.unsafe_reason.as_deref()
    } else {
        None
    };
    collect_mmio_stmts(&f.body, core_aliases, &mut ptrs, reason, out);
}

fn collect_mmio_stmts(
    stmts: &[crate::AST::Stmt],
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &mut std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetProfile::MmioAccess>,
) {
    for stmt in stmts {
        match stmt {
            crate::AST::Stmt::Val(b) => {
                collect_mmio_expr(&b.init, core_aliases, ptrs, unsafe_reason, out);
                if let Some(fact) = ptr_fact_from_expr(&b.init) {
                    ptrs.insert(b.name.clone(), fact);
                }
            }
            crate::AST::Stmt::Expr(e) | crate::AST::Stmt::Return(Some(e), _) => {
                collect_mmio_expr(e, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Assign { value, .. } => {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::If(i) => {
                collect_mmio_expr(&i.cond, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_stmts(&i.then_body, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_else(&i.else_branch, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::While { cond, body, .. } => {
                collect_mmio_expr(cond, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::For { kind, body, .. } => {
                collect_mmio_for_kind(kind, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                collect_mmio_expr(subject, core_aliases, ptrs, unsafe_reason, out);
                for arm in arms {
                    collect_mmio_stmts(&arm.body, core_aliases, ptrs, unsafe_reason, out);
                }
                if let Some(body) = else_body {
                    collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
                }
            }
            crate::AST::Stmt::Loop { body, .. }
            | crate::AST::Stmt::Impure { body, .. }
            | crate::AST::Stmt::Reactive { body, .. }
            | crate::AST::Stmt::Region { body, .. }
            | crate::AST::Stmt::Policy { body, .. }
            | crate::AST::Stmt::TaskGroup { body, .. } => {
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Unsafe { audit, body, .. } => {
                collect_mmio_stmts(body, core_aliases, ptrs, audit.as_deref(), out);
            }
            crate::AST::Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                collect_mmio_expr(&init.init, core_aliases, ptrs, unsafe_reason, out);
                if let Some(fact) = ptr_fact_from_expr(&init.init) {
                    ptrs.insert(init.name.clone(), fact);
                }
                collect_mmio_expr(cond, core_aliases, ptrs, unsafe_reason, out);
                if let Some(step) = step {
                    collect_mmio_stmts(
                        std::slice::from_ref(step),
                        core_aliases,
                        ptrs,
                        unsafe_reason,
                        out,
                    );
                }
                collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
            }
            crate::AST::Stmt::Return(None, _)
            | crate::AST::Stmt::Break(_)
            | crate::AST::Stmt::Continue(_)
            | crate::AST::Stmt::BreakLabel(_, _)
            | crate::AST::Stmt::ContinueLabel(_, _) => {}
            _ => {}
        }
    }
}

fn collect_mmio_else(
    branch: &Option<crate::AST::ElseBranch>,
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &mut std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetProfile::MmioAccess>,
) {
    match branch {
        Some(crate::AST::ElseBranch::Else(body)) => {
            collect_mmio_stmts(body, core_aliases, ptrs, unsafe_reason, out);
        }
        Some(crate::AST::ElseBranch::ElseIf(i)) => {
            collect_mmio_expr(&i.cond, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_stmts(&i.then_body, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_else(&i.else_branch, core_aliases, ptrs, unsafe_reason, out);
        }
        None => {}
    }
}

fn collect_mmio_for_kind(
    kind: &crate::AST::ForKind,
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &mut std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetProfile::MmioAccess>,
) {
    match kind {
        crate::AST::ForKind::Range { start, end, step } => {
            collect_mmio_expr(start, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(end, core_aliases, ptrs, unsafe_reason, out);
            if let Some(step) = step {
                collect_mmio_expr(step, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::ForKind::In { collection, step } => {
            collect_mmio_expr(collection, core_aliases, ptrs, unsafe_reason, out);
            if let Some(step) = step {
                collect_mmio_expr(step, core_aliases, ptrs, unsafe_reason, out);
            }
        }
    }
}

fn collect_mmio_expr(
    expr: &crate::AST::Expr,
    core_aliases: &std::collections::HashMap<String, String>,
    ptrs: &std::collections::HashMap<String, PtrFact>,
    unsafe_reason: Option<&str>,
    out: &mut Vec<crate::TargetProfile::MmioAccess>,
) {
    match expr {
        crate::AST::Expr::PtrFromAddr { addr, .. } => {
            collect_mmio_expr(addr, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if is_core_mem_receiver(receiver, core_aliases)
                && matches!(method.as_str(), "volatile_read" | "volatile_write")
            {
                if let Some(first) = args.first() {
                    if let crate::AST::Expr::Ident(name, _) = &first.expr {
                        if let Some(fact) = ptrs.get(name) {
                            out.push(crate::TargetProfile::MmioAccess {
                                address: fact.address,
                                size: fact.size,
                                unsafe_gate: unsafe_reason.map(|reason| {
                                    crate::TargetProfile::UnsafeGate {
                                        reason: reason.to_string(),
                                    }
                                }),
                            });
                        }
                    }
                }
            }
            collect_mmio_expr(receiver, core_aliases, ptrs, unsafe_reason, out);
            for arg in args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Call(c) => {
            for arg in &c.args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Binary(_, a, b, _) => {
            collect_mmio_expr(a, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(b, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::Index { base, index, .. } => {
            collect_mmio_expr(base, core_aliases, ptrs, unsafe_reason, out);
            collect_mmio_expr(index, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            if let crate::AST::OrFallback::Value(v) = fallback {
                collect_mmio_expr(v, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::Unary(_, e, _)
        | crate::AST::Expr::Copy(e, _)
        | crate::AST::Expr::Place(e, _, _)
        | crate::AST::Expr::Deref(e, _)
        | crate::AST::Expr::RawOf(e, _)
        | crate::AST::Expr::Field(e, _, _)
        | crate::AST::Expr::OptField { base: e, .. } => {
            collect_mmio_expr(e, core_aliases, ptrs, unsafe_reason, out);
        }
        crate::AST::Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::ListLit(elems, _) => {
            for elem in elems {
                collect_mmio_expr(elem, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                collect_mmio_expr(k, core_aliases, ptrs, unsafe_reason, out);
                collect_mmio_expr(v, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                collect_mmio_expr(value, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        crate::AST::Expr::CallValue { callee, args, .. } => {
            collect_mmio_expr(callee, core_aliases, ptrs, unsafe_reason, out);
            for arg in args {
                collect_mmio_expr(&arg.expr, core_aliases, ptrs, unsafe_reason, out);
            }
        }
        _ => {}
    }
}

fn is_core_mem_receiver(
    receiver: &crate::AST::Expr,
    core_aliases: &std::collections::HashMap<String, String>,
) -> bool {
    matches!(receiver, crate::AST::Expr::Ident(alias, _) if core_aliases.get(alias).is_some_and(|m| m == "core.mem"))
}

fn ptr_fact_from_expr(expr: &crate::AST::Expr) -> Option<PtrFact> {
    let crate::AST::Expr::PtrFromAddr { elem, addr, .. } = expr else {
        return None;
    };
    let crate::AST::Expr::Int(address, _, _, _) = addr.as_ref() else {
        return None;
    };
    if *address < 0 {
        return None;
    }
    Some(PtrFact {
        address: *address as u64,
        size: byte_size_for_type(elem)?,
    })
}

fn byte_size_for_type(ty: &crate::AST::Type) -> Option<crate::TargetProfile::ByteSize> {
    match ty {
        crate::AST::Type::Bool => Some(crate::TargetProfile::ByteSize::bytes(1)),
        crate::AST::Type::Char | crate::AST::Type::Float32 => {
            Some(crate::TargetProfile::ByteSize::bytes(4))
        }
        crate::AST::Type::Int | crate::AST::Type::Float | crate::AST::Type::String => {
            Some(crate::TargetProfile::ByteSize::bytes(8))
        }
        crate::AST::Type::IntN { bits, .. } => {
            Some(crate::TargetProfile::ByteSize::bytes((*bits as u64) / 8))
        }
        crate::AST::Type::Tagged { inner, .. } => byte_size_for_type(inner),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct BuildRunOptions {
    pub grants: std::collections::BTreeSet<crate::Comptime::Build::BuildCapability>,
    pub execute: bool,
    pub allow_impure: bool,
    /// Validate and expose declared graph authority without granting ambient
    /// comptime authority. Used only by read-only CLI/LSP inspection.
    pub inspect_only: bool,
    pub locked: bool,
    pub freestanding: bool,
    pub web_target: bool,
    pub plugin_target: bool,
    pub cross_target: Option<String>,
}

impl Default for BuildRunOptions {
    fn default() -> Self {
        BuildRunOptions {
            grants: std::collections::BTreeSet::new(),
            execute: true,
            allow_impure: false,
            inspect_only: false,
            locked: false,
            freestanding: false,
            web_target: false,
            plugin_target: false,
            cross_target: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedSourceProvenance {
    pub name: String,
    pub path: std::path::PathBuf,
    pub digest: crate::Comptime::Build::ContentDigest,
}

#[derive(Debug, Clone)]
pub struct BuildRun {
    pub plan: crate::Comptime::Build::BuildPlan,
    pub execution: crate::Comptime::Build::BuildExecutionReport,
    pub probes: Vec<crate::Comptime::Build::BuildProbeFact>,
    pub generated: Vec<GeneratedSourceProvenance>,
}

#[derive(Debug)]
pub struct BuildCompileOutput {
    pub compile: crate::CompileOutput,
    pub build: Option<BuildRun>,
}

struct BuildFilesystemTransaction {
    files: Vec<(std::path::PathBuf, Option<Vec<u8>>)>,
    committed: bool,
}

impl BuildFilesystemTransaction {
    fn new(paths: impl IntoIterator<Item = std::path::PathBuf>) -> std::io::Result<Self> {
        let mut seen = std::collections::BTreeSet::new();
        let mut files = Vec::new();
        for path in paths.into_iter().filter(|path| seen.insert(path.clone())) {
            let before = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            files.push((path, before));
        }
        Ok(Self {
            files,
            committed: false,
        })
    }
    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for BuildFilesystemTransaction {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, before) in self.files.iter().rev() {
            match before {
                Some(bytes) => {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(path, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

/// Static graph facts for CLI and LSP consumers. Build evaluation is pure and
/// generated files/actions are not materialized or executed.
pub fn query_build_plan(
    file: &str,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    compile_bundle_path_build(file, build_query_options())
    .map(|output| output.build.map(|build| build.plan))
}

fn build_query_options() -> BuildRunOptions {
    BuildRunOptions {
        // Inspection verifies source declarations and @Impure gates, but it
        // must not require execution grants merely to display the graph.
        grants: crate::Comptime::Build::BuildCapability::ALL.into_iter().collect(),
        execute: false,
        // Graph inspection may describe effectful actions, but it has no
        // authority to perform ambient comptime I/O. A user-written @Impure
        // gate therefore still reaches E3411 instead of touching the host.
        allow_impure: false,
        inspect_only: true,
        locked: false,
        freestanding: false,
        web_target: false,
        plugin_target: false,
        cross_target: None,
    }
}

/// Ratified D-BUILDQUERY1 query expressions. `build` is deliberately the only
/// expression until another query spelling is owner-ratified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildQueryExpression {
    Build,
}

pub fn evaluate_build_query(
    file: &str,
    expression: BuildQueryExpression,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    match expression {
        BuildQueryExpression::Build => query_build_plan(file),
    }
}

/// LSP variant: the open document is authoritative even before save.
pub fn query_build_plan_with_overlay(
    file: &str,
    source: &str,
) -> Result<Option<crate::Comptime::Build::BuildPlan>, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(
        file,
        build_query_options(),
        Some((std::path::Path::new(file), source)),
    )
    .map(|output| output.build.map(|build| build.plan))
}

/// One canonical graph representation shared by CLI and LSP.
pub fn build_plan_json(plan: &crate::Comptime::Build::BuildPlan) -> String {
    fn escape(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }
    fn strings(values: &[String]) -> String {
        format!(
            "[{}]",
            values
                .iter()
                .map(|value| format!("\"{}\"", escape(value)))
                .collect::<Vec<_>>()
                .join(",")
        )
    }
    let graph = plan.graph();
    format!("{{\"schema_version\":1,\"default\":{},\"targets\":[{}],\"actions\":[{}],\"files\":[{}],\"toolchains\":[{}],\"probes\":[{}],\"generated\":[{}]}}",
        plan.default_target().map(|target| target.id().0.to_string()).unwrap_or_else(|| "null".to_string()),
        graph.targets.iter().map(|target| format!("{{\"id\":{},\"name\":\"{}\",\"kind\":\"{:?}\",\"deps\":[{}],\"actions\":[{}],\"files\":{}}}", target.id.0, escape(&target.name), target.kind, target.deps.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), target.actions.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), strings(&target.files))).collect::<Vec<_>>().join(","),
        graph.actions.iter().map(|action| { let real = &plan.actions()[action.id.0]; format!("{{\"id\":{},\"name\":\"{}\",\"inputs\":{},\"outputs\":{},\"caps\":{},\"pools\":{},\"toolchain\":\"{}\",\"probes\":{},\"cache\":\"{:?}\",\"provenance\":{}}}", action.id.0, escape(&action.name), strings(&action.inputs), strings(&action.outputs), strings(&action.caps.iter().map(|cap| cap.name().to_string()).collect::<Vec<_>>()), strings(&action.pools.iter().map(|pool| pool.as_str().to_string()).collect::<Vec<_>>()), escape(&plan.toolchains()[real.toolchain.id().0].name), strings(&real.probes.iter().map(|probe| plan.probes()[probe.id().0].name.clone()).collect::<Vec<_>>()), real.cache, strings(&plan.explain_action_named(&action.name).map(|fact| fact.provenance).unwrap_or_default())) }).collect::<Vec<_>>().join(","),
        graph.files.iter().map(|file| format!("{{\"path\":\"{}\",\"owner\":{},\"consumers\":[{}],\"targets\":[{}]}}", escape(&file.path), file.owner.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string()), file.consumers.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","), file.targets.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(","),
        plan.toolchains().iter().map(|tool| format!("{{\"name\":\"{}\",\"target\":\"{}\"}}", escape(&tool.name), escape(&tool.target_triple))).collect::<Vec<_>>().join(","),
        plan.probes().iter().map(|probe| format!("{{\"name\":\"{}\",\"kind\":\"{:?}\",\"reproducibility\":\"{:?}\"}}", escape(&probe.name), probe.kind, probe.reproducibility)).collect::<Vec<_>>().join(","),
        plan.generated_modules().iter().map(|module| format!("{{\"name\":\"{}\",\"path\":\"{}\",\"digest\":\"{}\"}}", escape(&module.name), escape(module.path.as_str()), module.source_digest.as_str())).collect::<Vec<_>>().join(",")
    )
}

/// D-BUILDENTRY1 complete driver staging: check root bundle, evaluate selected
/// root `fn build`, materialize/re-check generated Jet, execute canonical graph,
/// remove build-only entry, then codegen runtime program.
pub fn compile_bundle_path_build(
    file: &str,
    options: BuildRunOptions,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    compile_bundle_path_build_inner(file, options, None)
}

fn compile_bundle_path_build_inner(
    file: &str,
    options: BuildRunOptions,
    overlay: Option<(&std::path::Path, &str)>,
) -> Result<BuildCompileOutput, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, overlay, false)?;
    let active_os = crate::Syntax::OsTarget::active(options.cross_target.as_deref());
    let compile_mode = if options.plugin_target {
        crate::Sema::CompileMode::Check
    } else {
        crate::Sema::CompileMode::Run
    };
    bundle.active_os = active_os;
    bundle.web_partition_enforced = options.web_target;
    let build_index = bundle.modules[bundle.entry]
        .items
        .iter()
        .position(|item| matches!(item, crate::AST::Item::Func(func) if func.name == "build"));
    if let Some(index) = build_index {
        let crate::AST::Item::Func(build) = &bundle.modules[bundle.entry].items[index] else {
            unreachable!()
        };
        if !valid_build_signature(build) {
            return Err(vec![bad_build_signature(build.name_span)]);
        }
    }

    // Build code is compiler-host code. Target restrictions apply only after
    // the selected runtime program replaces it.
    let (diags, effect_facts) =
        crate::Sema::check_bundle_with_effect_facts(&mut bundle, compile_mode);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for diag in std::mem::take(&mut bundle.parse_teaching)
        .into_iter()
        .chain(diags)
    {
        match diag.severity {
            // Generated declarations do not exist during the pre-build
            // reflection pass. Defer only unknown-name errors to the fresh
            // selected-program sema pass after generation; every other error
            // still blocks build evaluation.
            Severity::Error if build_index.is_some() && diag.code == "E0102" => {}
            Severity::Error => errors.push(diag),
            Severity::Lint => lints.push(diag),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut build_run = None;
    let mut filesystem_transaction = None;
    if let Some(index) = build_index {
        let build = match &bundle.modules[bundle.entry].items[index] {
            crate::AST::Item::Func(func) => func,
            _ => unreachable!(),
        };
        let declared_build_effects = build
            .declared_effects
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|(name, _)| crate::Comptime::Build::BuildCapability::parse(name))
            .collect::<std::collections::BTreeSet<_>>();
        let has_impure_gate = build
            .body
            .iter()
            .any(|stmt| matches!(stmt, crate::AST::Stmt::Impure { .. }));
        let mut funcs = std::collections::HashMap::new();
        let mut methods = std::collections::HashMap::new();
        let mut structs = std::collections::HashMap::new();
        let mut enums = std::collections::HashMap::new();
        let mut migrations: std::collections::HashMap<String, Vec<&crate::AST::MigrationDecl>> =
            std::collections::HashMap::new();
        let computed_fields = std::collections::HashMap::new();
        let mut distinct_ranges = std::collections::HashMap::new();
        let mut distinct_bases = std::collections::HashMap::new();
        let mut function_name_counts = std::collections::HashMap::<String, usize>::new();
        let mut type_name_counts = std::collections::HashMap::<String, usize>::new();
        for module in &bundle.modules {
            for item in &module.items {
                match item {
                    crate::AST::Item::Func(func) => {
                        *function_name_counts.entry(func.name.clone()).or_default() += 1
                    }
                    crate::AST::Item::Struct(def) => {
                        *type_name_counts.entry(def.name.clone()).or_default() += 1
                    }
                    crate::AST::Item::Enum(def) => {
                        *type_name_counts.entry(def.name.clone()).or_default() += 1
                    }
                    _ => {}
                }
            }
        }
        for module in &bundle.modules {
            for item in &module.items {
                match item {
                    crate::AST::Item::Func(func) => {
                        funcs.insert(format!("{}::{}", module.alias, func.name), func);
                        if function_name_counts.get(&func.name) == Some(&1) {
                            funcs.insert(func.name.clone(), func);
                        }
                    }
                    crate::AST::Item::Struct(def) => {
                        let owner = format!("{}::{}", module.alias, def.name);
                        structs.insert(owner.clone(), def);
                        if type_name_counts.get(&def.name) == Some(&1) {
                            structs.insert(def.name.clone(), def);
                        }
                        for method in &def.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&def.name) == Some(&1) {
                                methods.insert((def.name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Enum(def) => {
                        let owner = format!("{}::{}", module.alias, def.name);
                        enums.insert(owner.clone(), def);
                        if type_name_counts.get(&def.name) == Some(&1) {
                            enums.insert(def.name.clone(), def);
                        }
                        for method in &def.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&def.name) == Some(&1) {
                                methods.insert((def.name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Impl(imp) => {
                        let owner = format!("{}::{}", module.alias, imp.type_name);
                        for method in &imp.methods {
                            methods.insert((owner.clone(), method.name.clone()), method);
                            if type_name_counts.get(&imp.type_name) == Some(&1) {
                                methods
                                    .insert((imp.type_name.clone(), method.name.clone()), method);
                            }
                        }
                    }
                    crate::AST::Item::Migration(m) => {
                        migrations.entry(m.type_name.clone()).or_default().push(m);
                    }
                    crate::AST::Item::Distinct(def) => {
                        distinct_ranges.insert(
                            def.name.clone(),
                            def.range.map(|(lo, hi, _)| (lo, hi)),
                        );
                        distinct_bases.insert(def.name.clone(), def.base.clone());
                    }
                    crate::AST::Item::UnitFamily(family) => {
                        for def in family.distinct_defs() {
                            distinct_ranges.insert(
                                def.name.clone(),
                                def.range.map(|(lo, hi, _)| (lo, hi)),
                            );
                            distinct_bases.insert(def.name, def.base);
                        }
                    }
                    _ => {}
                }
            }
        }
        let globals = std::collections::HashMap::new();
        let core_imports = core_aliases(&bundle.modules[bundle.entry]);
        let info = crate::Comptime::ProgramInfo {
            globals,
            methods,
            structs,
            enums,
            computed_fields,
            distinct_ranges,
            distinct_bases,
            core_imports,
            migrations,
        };
        let semantic_facts = program_semantic_facts(&bundle, &effect_facts);
        let program_value = crate::Comptime::build_program_info(&bundle, &semantic_facts);
        let base_dir = std::path::Path::new(file)
            .parent()
            .unwrap_or(std::path::Path::new("."));
        let package = std::path::Path::new(file)
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("app");
        let evaluated = crate::Comptime::run_build_entry(
            build,
            &funcs,
            base_dir,
            &info,
            program_value,
            package,
            options.allow_impure,
        )
        .map_err(|diag| vec![diag])?;

        if !evaluated.diagnostics.is_empty() {
            return Err(evaluated.diagnostics);
        }

        validate_build_authority(
            &evaluated.plan,
            &declared_build_effects,
            has_impure_gate,
            &options,
            build.name_span,
        )?;

        let selected_actions = evaluated
            .plan
            .selected_action_ids()
            .map_err(|error| vec![build_plan_diagnostic(&error)])?;
        let selected_generated = evaluated
            .plan
            .selected_generated_modules()
            .map_err(|error| vec![build_plan_diagnostic(&error)])?;
        let transaction_paths = selected_generated
            .iter()
            .map(|module| bundle.project_root.join(module.path.as_str()))
            .chain(
                evaluated
                    .plan
                    .actions()
                    .iter()
                    .filter(|action| selected_actions.contains(&action.id))
                    .flat_map(|action| {
                        action
                            .outputs
                            .iter()
                            .map(|output| bundle.project_root.join(output.as_str()))
                    }),
            )
            .chain(std::iter::once(bundle.project_root.join(".jet/lock")))
            .collect::<Vec<_>>();
        filesystem_transaction = Some(
            BuildFilesystemTransaction::new(transaction_paths)
                .map_err(|error| vec![generated_io_diag("build filesystem transaction", &error)])?,
        );
        if options.locked {
            let planned_generated = selected_generated
                .iter()
                .map(|module| crate::AST::ComptimeInput {
                    path: module.path.as_str().to_string(),
                    hash: module.source_digest.as_str().to_string(),
                })
                .collect::<Vec<_>>();
            crate::Lock::record_generated_inputs(&bundle.project_root, &planned_generated, true)
                .map_err(|diagnostic| vec![diagnostic])?;
        }
        let mut generated = if options.execute {
            materialize_and_check_generated(&selected_generated, &bundle.project_root)?
        } else {
            selected_generated
                .iter()
                .map(|module| GeneratedSourceProvenance {
                    name: module.name.clone(),
                    path: bundle.project_root.join(module.path.as_str()),
                    digest: module.source_digest.clone(),
                })
                .collect()
        };
        let executed = if options.execute {
            crate::Comptime::Build::execute_build_plan(
                &evaluated.plan,
                &bundle.project_root,
                &options.grants,
            )
            .map_err(|error| vec![build_execution_diagnostic(error)])?
        } else {
            crate::Comptime::Build::BuildExecutionResult {
                report: evaluated
                    .plan
                    .execution_report(&[])
                    .map_err(|error| vec![build_plan_diagnostic(&error)])?,
                probes: Vec::new(),
            }
        };
        if options.execute {
            generated.extend(check_action_generated_sources(
                &evaluated.plan,
                &bundle.project_root,
            )?);
            let mut locked_provenance = generated
                .iter()
                .map(|item| crate::AST::ComptimeInput {
                    path: item
                        .path
                        .strip_prefix(&bundle.project_root)
                        .unwrap_or(&item.path)
                        .display()
                        .to_string(),
                    hash: item.digest.as_str().to_string(),
                })
                .collect::<Vec<_>>();
            locked_provenance.extend(evaluated.comptime_inputs.iter().cloned());
            for action in evaluated
                .plan
                .actions()
                .iter()
                .filter(|action| selected_actions.contains(&action.id))
            {
                for input in &action.inputs {
                    let path = bundle.project_root.join(input.as_str());
                    let bytes = std::fs::read(&path)
                        .map_err(|error| vec![generated_io_diag(&action.name, &error)])?;
                    locked_provenance.push(crate::AST::ComptimeInput {
                        path: input.as_str().to_string(),
                        hash: crate::Comptime::Build::ContentDigest::from_bytes(&bytes)
                            .as_str()
                            .to_string(),
                    });
                }
                for output in &action.outputs {
                    let path = bundle.project_root.join(output.as_str());
                    let bytes = std::fs::read(&path)
                        .map_err(|error| vec![generated_io_diag(&action.name, &error)])?;
                    locked_provenance.push(crate::AST::ComptimeInput {
                        path: output.as_str().to_string(),
                        hash: crate::Comptime::Build::ContentDigest::from_bytes(&bytes)
                            .as_str()
                            .to_string(),
                    });
                }
            }
            locked_provenance.sort_by(|a, b| a.path.cmp(&b.path));
            locked_provenance.dedup_by(|a, b| a.path == b.path && a.hash == b.hash);
            if let Err(diagnostic) = crate::Lock::record_generated_inputs(
                &bundle.project_root,
                &locked_provenance,
                options.locked,
            ) {
                return Err(vec![diagnostic]);
            }
        }
        let mut planned_bundle = if options.execute {
            load_planned_runtime_bundle(file, &evaluated.plan, &generated, &bundle.project_root)?
        } else {
            bundle
        };
        planned_bundle
            .comptime_inputs
            .extend(evaluated.comptime_inputs);
        build_run = Some(BuildRun {
            plan: evaluated.plan,
            execution: executed.report,
            probes: executed.probes,
            generated,
        });
        bundle = planned_bundle;
        bundle.active_os = active_os;
        bundle.web_partition_enforced = options.web_target;
    }

    // Imported build entries are checked but never run. They are build-only
    // values and must not leak into runtime codegen (root was removed above).
    for module in &mut bundle.modules {
        module
            .items
            .retain(|item| !matches!(item, crate::AST::Item::Func(func) if func.name == "build"));
    }

    // The selected target source closure and generated modules are a fresh
    // program, not syntax checked in isolation. Re-run the complete front end
    // before any runtime codegen.
    if build_run.is_some() && options.execute {
        let planned_diags = if options.freestanding {
            crate::Sema::check_bundle_freestanding(&mut bundle, compile_mode)
        } else if options.allow_impure {
            crate::Sema::check_bundle_allow_impure(&mut bundle, compile_mode)
        } else {
            crate::Sema::check_bundle(&mut bundle, compile_mode)
        };
        let mut planned_errors = Vec::new();
        for diag in std::mem::take(&mut bundle.parse_teaching)
            .into_iter()
            .chain(planned_diags)
        {
            match diag.severity {
                Severity::Error => planned_errors.push(diag),
                Severity::Lint => lints.push(diag),
            }
        }
        if !planned_errors.is_empty() {
            return Err(planned_errors);
        }
    }

    // Static graph/query/explain (`execute: false`) must not codegen the
    // pre-build entry: `fn run` may call generated symbols that only exist
    // after materialization. CLI/LSP consumers only need `build.plan`.
    if !options.execute {
        if let Some(transaction) = filesystem_transaction.as_mut() {
            transaction.commit();
        }
        return Ok(BuildCompileOutput {
            compile: crate::CompileOutput {
                rust: String::new(),
                lints,
                ffi: None,
                clinks: Vec::new(),
                capabilities: crate::Capabilities::default(),
                comptime_inputs: std::mem::take(&mut bundle.comptime_inputs),
                web: None,
                web_partition_report: None,
                plugin: None,
                inferred_layer: bundle.inferred_layer,
                layer_ceiling: bundle.layer_ceiling,
            },
            build: build_run,
        });
    }

    let ffi = match options.cross_target.as_deref() {
        Some(target) => crate::FFI::prepare_for_target(&bundle, target),
        None => crate::FFI::prepare(&bundle),
    }
    .map_err(|diags| diags)?;
    if options.web_target {
        let misses = crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref());
        if !misses.is_empty() {
            return Err(misses.into_iter().map(|miss| Diagnostic::error(
                "E-WEB-TIR-UNSUPPORTED",
                format!("web output cannot compile `{}` yet", miss.func_name),
                "the selected BuildPlan program uses a construct unavailable to web lowering".to_string(),
                "select web-covered sources or simplify the named function".to_string(),
                Some(miss.span),
            )).collect());
        }
    }
    let rust = crate::Codegen::emit_bundle_dbg(&bundle, ffi.as_ref(), false, active_os);
    let web = if options.web_target {
        Some(
            crate::Codegen::emit_web(&bundle, compile_mode, ffi.as_ref()).map_err(|miss| {
                vec![Diagnostic::error(
                    "E-WEB-TIR-UNSUPPORTED",
                    format!("web output cannot compile `{}` yet", miss.func_name),
                    "web emitter capability facts drifted after validation".to_string(),
                    "report this compiler bug with the named function".to_string(),
                    Some(miss.span),
                )]
            })?,
        )
    } else {
        None
    };
    let plugin = if options.plugin_target {
        let errors = crate::PluginExport::validate_export_surface(&bundle);
        if !errors.is_empty() {
            return Err(errors);
        }
        let name = crate::PluginExport::resolve_export_name(&bundle);
        crate::PluginExport::check_and_freeze_version(&bundle, &name)?;
        Some(crate::Codegen::emit_plugin(&bundle, &rust, &name))
    } else {
        None
    };
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let compile = crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs: std::mem::take(&mut bundle.comptime_inputs),
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    };
    if let Some(transaction) = filesystem_transaction.as_mut() {
        transaction.commit();
    }
    Ok(BuildCompileOutput {
        compile,
        build: build_run,
    })
}

fn load_planned_runtime_bundle(
    build_file: &str,
    plan: &crate::Comptime::Build::BuildPlan,
    generated: &[GeneratedSourceProvenance],
    project_root: &std::path::Path,
) -> Result<crate::AST::ProgramBundle, Vec<Diagnostic>> {
    let sources = plan
        .selected_sources()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    let Some(entry_source) = sources.first() else {
        return Err(vec![Diagnostic::error(
            "E3502",
            "the selected build target has no Jet sources".to_string(),
            "BuildPlan selects the exact program passed back through lexer, parser, sema, and codegen".to_string(),
            "add the runtime entry file to the selected target's sources".to_string(),
            None,
        )]);
    };
    let resolve = |path: &str| {
        let path = std::path::Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        }
    };
    let entry_path = resolve(entry_source.as_str());
    let mut bundle = crate::Loader::load_entry_with_overlay(
        entry_path.to_str().unwrap_or(build_file),
        None,
        false,
    )?;

    // Additional selected roots and generated modules merge before the one
    // complete sema pass, so runtime code can call generated declarations.
    let mut additions = sources
        .iter()
        .skip(1)
        .map(|path| ("selected source", resolve(path.as_str())))
        .chain(
            generated
                .iter()
                .map(|item| (item.name.as_str(), item.path.clone())),
        )
        .collect::<Vec<_>>();
    additions.sort_by(|a, b| a.1.cmp(&b.1));
    additions.dedup_by(|a, b| a.1 == b.1);
    for (generator, path) in additions {
        let loaded = crate::Loader::load_entry_with_overlay(
            path.to_str().unwrap_or(build_file),
            None,
            false,
        )
        .map_err(|mut diagnostics| {
            for diagnostic in &mut diagnostics {
                diagnostic.what = format!("generated by `{generator}`: {}", diagnostic.what);
            }
            diagnostics
        })?;
        let entry = loaded.entry;
        bundle.modules[bundle.entry]
            .items
            .extend(loaded.modules.into_iter().nth(entry).unwrap().items);
    }
    Ok(bundle)
}

fn check_action_generated_sources(
    plan: &crate::Comptime::Build::BuildPlan,
    root: &std::path::Path,
) -> Result<Vec<GeneratedSourceProvenance>, Vec<Diagnostic>> {
    let registered = plan
        .generated_modules()
        .iter()
        .map(|module| module.path.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut out = Vec::new();
    let selected = plan
        .selected_action_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    for action in plan
        .actions()
        .iter()
        .filter(|action| selected.contains(&action.id))
    {
        for output in &action.outputs {
            if !output.as_str().ends_with(".jet") || registered.contains(output.as_str()) {
                continue;
            }
            let path = root.join(output.as_str());
            let source = std::fs::read_to_string(&path)
                .map_err(|error| vec![generated_io_diag(&action.name, &error)])?;
            let mut generated_bundle = crate::Loader::load_entry_with_overlay(
                path.to_str().unwrap_or(output.as_str()),
                None,
                false,
            )
            .map_err(|mut diags| {
                for diag in &mut diags {
                    diag.what = format!("generated action `{}`: {}", action.name, diag.what);
                }
                diags
            })?;
            let mut diags =
                crate::Sema::check_bundle(&mut generated_bundle, crate::Sema::CompileMode::Check);
            diags.retain(|diag| diag.severity == Severity::Error);
            if !diags.is_empty() {
                for diag in &mut diags {
                    diag.what = format!("generated action `{}`: {}", action.name, diag.what);
                }
                return Err(diags);
            }
            out.push(GeneratedSourceProvenance {
                name: action.name.clone(),
                path,
                digest: crate::Comptime::Build::ContentDigest::from_bytes(source.as_bytes()),
            });
        }
    }
    Ok(out)
}

fn valid_build_signature(func: &crate::AST::Func) -> bool {
    if func.params.len() != 1
        || func.params[0].ty
            != crate::AST::Type::Named(crate::Syntax::TYPE_BUILD_CONTEXT.to_string())
    {
        return false;
    }
    matches!(
        func.return_type.as_ref(),
        Some(crate::AST::Type::Result { ok, .. })
            if **ok == crate::AST::Type::Named(crate::Syntax::TYPE_BUILD_PLAN.to_string())
    )
}

fn validate_build_authority(
    plan: &crate::Comptime::Build::BuildPlan,
    declared: &std::collections::BTreeSet<crate::Comptime::Build::BuildCapability>,
    has_impure_gate: bool,
    options: &BuildRunOptions,
    span: crate::Diagnostics::Span,
) -> Result<(), Vec<Diagnostic>> {
    let selected = plan
        .selected_action_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?;
    let mut required = plan
        .actions()
        .iter()
        .filter(|action| selected.contains(&action.id))
        .flat_map(|action| action.caps.iter().copied())
        .collect::<std::collections::BTreeSet<_>>();
    if !plan
        .selected_probe_ids()
        .map_err(|error| vec![build_plan_diagnostic(&error)])?
        .is_empty()
    {
        required.insert(crate::Comptime::Build::BuildCapability::Exec);
    }
    for effect in required {
        if !declared.contains(&effect) {
            return Err(vec![Diagnostic::error(
                "E3503",
                format!("this build uses `{}` without declaring it on `fn build`", effect.name()),
                "the build signature is the static authority manifest audited before build code executes".to_string(),
                format!("add `{}` to the build function's `--[...]->` effect row", effect.name()),
                Some(span),
            )]);
        }
        if !has_impure_gate {
            return Err(vec![Diagnostic::error(
                "E3502",
                format!("build authority `{}` must be used inside `@Impure(\"reason\")`", effect.name()),
                "ambient build effects need an audited source gate as well as a policy grant".to_string(),
                "wrap the action or probe declarations in `@Impure(\"why this build needs ambient authority\")`".to_string(),
                Some(span),
            )]);
        }
        if !options.inspect_only && (!options.allow_impure || !options.grants.contains(&effect)) {
            return Err(vec![Diagnostic::error(
                "E3503",
                format!("this build asks for `{}`, which effective policy has not granted", effect.name()),
                "a source declaration and `@Impure` gate do not widen CLI, package, or workspace policy".to_string(),
                format!("pass `--allow-{}` or grant it in package/workspace build policy", effect.flag()),
                Some(span),
            )]);
        }
    }
    Ok(())
}

fn program_semantic_facts(
    bundle: &crate::AST::ProgramBundle,
    checked: &crate::Sema::SemIndexEffectFacts,
) -> crate::Comptime::ProgramSemanticFacts {
    fn reaches_panic(
        name: &str,
        summaries: &std::collections::HashMap<String, crate::Sema::EffectSummary>,
        visiting: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        if !visiting.insert(name.to_string()) {
            return false;
        }
        let reached = summaries.get(name).is_some_and(|summary| {
            summary.edges.contains("__jet_panic__")
                || summary
                    .edges
                    .iter()
                    .any(|callee| reaches_panic(callee, summaries, visiting))
        });
        visiting.remove(name);
        reached
    }
    let mut effects = std::collections::HashMap::new();
    let mut panic_facts = std::collections::BTreeSet::new();
    for module in &bundle.modules {
        for item in &module.items {
            let crate::AST::Item::Func(func) = item else {
                continue;
            };
            let qualified = format!("{}::{}", module.alias, func.name);
            let values = checked
                .solved
                .get(&qualified)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default();
            effects.insert(qualified.clone(), values);
            if reaches_panic(
                &qualified,
                &checked.summaries,
                &mut std::collections::BTreeSet::new(),
            ) {
                panic_facts.insert(qualified);
            }
        }
    }
    let reaches_panic = panic_facts;
    crate::Comptime::ProgramSemanticFacts {
        effects,
        reaches_panic,
    }
}

fn bad_build_signature(span: crate::Diagnostics::Span) -> Diagnostic {
    Diagnostic::error(
        "E3501",
        "`fn build` must take one `BuildContext` and return `BuildPlan ?`".to_string(),
        "the build entry is a typed contract: its parameter is its authority and its result is the graph Jet executes".to_string(),
        "write `fn build(b: BuildContext) -> BuildPlan ?`".to_string(),
        Some(span),
    )
}

fn materialize_and_check_generated(
    modules: &[&crate::Comptime::Build::BuildGeneratedModule],
    root: &std::path::Path,
) -> Result<Vec<GeneratedSourceProvenance>, Vec<Diagnostic>> {
    let mut provenance = Vec::new();
    for module in modules {
        let path = root.join(module.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| vec![generated_io_diag(&module.name, &error)])?;
        }
        std::fs::write(&path, &module.source)
            .map_err(|error| vec![generated_io_diag(&module.name, &error)])?;
        let mut generated_bundle = crate::Loader::load_entry_with_overlay(
            path.to_str().unwrap_or(module.path.as_str()),
            None,
            false,
        )
        .map_err(|mut diags| {
            for diag in &mut diags {
                diag.what = format!("generated module `{}`: {}", module.name, diag.what);
            }
            diags
        })?;
        let mut diags =
            crate::Sema::check_bundle(&mut generated_bundle, crate::Sema::CompileMode::Check);
        diags.retain(|diag| diag.severity == Severity::Error);
        if !diags.is_empty() {
            for diag in &mut diags {
                diag.what = format!("generated module `{}`: {}", module.name, diag.what);
            }
            return Err(diags);
        }
        provenance.push(GeneratedSourceProvenance {
            name: module.name.clone(),
            path,
            digest: module.source_digest.clone(),
        });
    }
    Ok(provenance)
}

fn generated_io_diag(name: &str, error: &std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("generated module `{name}` could not be materialized"),
        format!("generated source must be a real file before it re-enters the front end: {error}"),
        "make sure `.jet/generated` is writable and the generated module name is unique"
            .to_string(),
        None,
    )
}

fn build_plan_diagnostic(error: &crate::Comptime::Build::BuildError) -> Diagnostic {
    Diagnostic::error(
        "E3502",
        format!("build plan is invalid: {error:?}"),
        "all graph handles must belong to one selected root build and every action output must have one owner".to_string(),
        "fix the named graph node and use `jet inspect explain-build` to inspect its inputs".to_string(),
        None,
    )
}

fn build_execution_diagnostic(error: crate::Comptime::Build::BuildExecutionError) -> Diagnostic {
    use crate::Comptime::Build::BuildExecutionError;
    match error {
        BuildExecutionError::MissingGrant { action, capability } => Diagnostic::error(
            "E3504",
            format!("build action `{action}` asks for ungranted `{capability:?}` authority"),
            "declaring a capability in `fn build` does not grant it; root policy must approve each ambient effect".to_string(),
            format!("pass `--allow-{}` for this run, or grant it in package/workspace policy", format!("{capability:?}").to_ascii_lowercase()),
            None,
        ),
        BuildExecutionError::ActionFailed { action, exit_code, stderr } => Diagnostic::error(
            "E3505",
            format!("build action `{action}` exited with status {exit_code}"),
            if stderr.is_empty() {
                "the declared command failed inside the build sandbox without writing stderr".to_string()
            } else {
                format!("the sandboxed command reported: {stderr}")
            },
            "fix the action command, declared inputs/outputs, toolchain, or probe, then rerun `jet build`".to_string(),
            None,
        ),
        BuildExecutionError::ProbeFailed { probe, detail } => Diagnostic::error(
            "E3505",
            format!("build probe `{probe}` failed"),
            detail,
            "fix the typed probe or select a toolchain that provides it".to_string(),
            None,
        ),
        BuildExecutionError::SandboxUnavailable => Diagnostic::error(
            "E3505",
            "build sandbox is unavailable".to_string(),
            "Jet refuses to run typed build actions without the required bubblewrap isolation".to_string(),
            "install bubblewrap or run on a supported build worker; there is no ambient fallback".to_string(),
            None,
        ),
        BuildExecutionError::Io { action, detail } => Diagnostic::error(
            "E3505",
            format!("build action `{action}` could not access a declared build path"),
            detail,
            "fix the declared input/output path or make the project build directory writable".to_string(),
            None,
        ),
        BuildExecutionError::InvalidGraph(error) => build_plan_diagnostic(&error),
    }
}

/// The real implementation behind every `compile_bundle_path_opts*` facade —
/// see `compile_bundle_path_opts` (native) / `compile_bundle_path_opts_dbg`
/// (native debug) / `compile_bundle_path_opts_plugin` (c81 plugin guest) for
/// the public entry points.
fn compile_bundle_path_opts_full(
    file: &str,
    mode: crate::Sema::CompileMode,
    freestanding: bool,
    allow_impure: bool,
    web_target: bool,
    plugin_target: bool,
    debug_linemap: bool,
    cross_target: Option<&str>,
    explicit_output: Option<&str>,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    // D-OSTARGET1=A: resolve the active native OS bucket once, from the same
    // `--target=<triple>` flag E2-M15 already threads through (host OS when
    // absent or unrecognized, e.g. a wasm/web pseudo-target).
    let active_os = crate::Syntax::OsTarget::active(cross_target);
    let timing = crate::PhaseTiming::enabled();
    let mut timer = crate::PhaseTiming::PhaseTimer::new();
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    // D-OSTARGET2=B: the `comptime if build.os == { … }` desugar (run in sema)
    // must fold to the same OS bucket codegen filters `impl`s by, so seed the
    // bundle from the same resolved `active_os` as `emit_bundle`.
    bundle.active_os = active_os;
    if web_target {
        bundle.web_partition_enforced = true;
    }
    if timing {
        timer.lap("load"); // lex + parse + module resolution
    }
    let diags = if let Some(output) = explicit_output {
        crate::Sema::check_bundle_for_output_opts(
            &mut bundle,
            mode,
            output,
            freestanding,
            allow_impure,
        )
    } else if freestanding {
        crate::Sema::check_bundle_freestanding(&mut bundle, mode)
    } else if allow_impure {
        crate::Sema::check_bundle_allow_impure(&mut bundle, mode)
    } else {
        crate::Sema::check_bundle(&mut bundle, mode)
    };
    if timing {
        timer.lap("sema");
    }
    // U11 (D-JPK-SCRIPTDEP1=A) and any other loader-time teaching diagnostic
    // (`bundle.parse_teaching`) ride the same errors/lints split as sema's —
    // `check_file` already does this for `jet check`/LSP; `jet run`/`build`
    // was dropping them on the floor (parse_teaching had no active producer
    // before U11's L0203, so the gap went unnoticed).
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in std::mem::take(&mut bundle.parse_teaching)
        .into_iter()
        .chain(diags)
    {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi_result = match cross_target {
        Some(target) => crate::FFI::prepare_for_target(&bundle, target),
        None => crate::FFI::prepare(&bundle),
    };
    let ffi = match ffi_result {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    if timing {
        timer.lap("ffi");
    }
    if web_target {
        let web_tir_errors: Vec<_> =
            crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref())
                .into_iter()
                .map(|miss| {
                    Diagnostic::error(
                        "E-WEB-TIR-UNSUPPORTED",
                        format!("web output cannot compile `{}` yet", miss.func_name),
                        "web builds use the same checked executable body path as native builds; this function uses a construct the web output cannot lower today".to_string(),
                        "move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target".to_string(),
                        Some(miss.span),
                    )
                })
                .collect();
        if !web_tir_errors.is_empty() {
            return Err(web_tir_errors);
        }
    }
    let rust = crate::Codegen::emit_bundle_dbg(&bundle, ffi.as_ref(), debug_linemap, active_os);
    let web = if web_target {
        Some(
            crate::Codegen::emit_web(&bundle, mode, ffi.as_ref()).map_err(|miss| {
                vec![Diagnostic::error(
                    "E-WEB-TIR-UNSUPPORTED",
                    format!("web output cannot compile `{}` yet", miss.func_name),
                    "web emitter capability facts drifted after validation".to_string(),
                    "report this compiler bug with the named function".to_string(),
                    Some(miss.span),
                )]
            })?,
        )
    } else {
        None
    };
    // D-PLUGIN1=B / D-DEP-WASM1=A / D-PLUGIN-EXPORT1=A (c81): the guest side of
    // a `target: plugin` build — a `.wit` world + wasm32 guest Rust, generated
    // from the entry module's exportable (`Int`/`Float`-only) `pub fn`s.
    let plugin = if plugin_target {
        // E1260: every `pub fn` in the entry module must be exportable —
        // never a silent skip (I3/I4).
        let surface_errors = crate::PluginExport::validate_export_surface(&bundle);
        if !surface_errors.is_empty() {
            return Err(surface_errors);
        }
        let export_name = crate::PluginExport::resolve_export_name(&bundle);
        // D-PLUGIN-VERSION1=A: freeze/diff the exported interface (E1257 on an
        // incompatible change) before handing artifacts to the wasm build step.
        crate::PluginExport::check_and_freeze_version(&bundle, &export_name)?;
        Some(crate::Codegen::emit_plugin(&bundle, &rust, &export_name))
    } else {
        None
    };
    if timing {
        timer.lap("codegen");
        timer.metric("rust_bytes", rust.len() as u128);
        timer.write_to(&bundle.project_root);
    }
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `@Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    if let Some(mf) = crate::Manifest::load(&bundle.project_root).and_then(|r| r.ok()) {
        crate::Lock::record_inferred_layer(
            &bundle.project_root,
            &mf.package.name,
            bundle.inferred_layer,
        );
    }
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        // Native C link flags are resolved separately at build time (so that
        // codegen / front-end checks never depend on system link discovery);
        // see `resolve_c_links`.
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompileSrcOptions {
    pub web_target: bool,
}

/// In-memory pipeline: lex → parse → bundle → sema → ffi → codegen.
pub fn compile_src(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options(src, file, mode, CompileSrcOptions::default())
}

/// Compile source wholly synthesized by the compiler or one of its tools.
/// Callers embedding user text must validate that text with `Lexer::lex` first.
pub fn compile_generated_src(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options_and_policy(src, file, mode, CompileSrcOptions::default(), true)
}

pub fn compile_src_with_options(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
    options: CompileSrcOptions,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    compile_src_with_options_and_policy(src, file, mode, options, false)
}

fn compile_src_with_options_and_policy(
    src: &str,
    file: &str,
    mode: crate::Sema::CompileMode,
    options: CompileSrcOptions,
    generated: bool,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let (toks, lex_diags) = if generated {
        crate::Lexer::lex_generated(src)
    } else {
        crate::Lexer::lex(src)
    };
    if !lex_diags.is_empty() {
        return Err(lex_diags);
    }
    let mut prog = crate::Parser::parse(&toks)?;
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from("."),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: prog.policy_declarations.clone(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        ffi_callback_fns: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: options.web_target,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OsTarget::host(),
    };
    // Active foreign caches may contribute generated C-ABI bridge modules.
    if let Err(diags) = crate::Foreign::assemble_active_namespaces(&mut bundle) {
        return Err(diags);
    }
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return Err(diags),
    };
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in diags {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    if options.web_target {
        let web_tir_errors: Vec<_> =
            crate::Codegen::validate_web_tir_support(&bundle, ffi.as_ref())
                .into_iter()
                .map(|miss| {
                    Diagnostic::error(
                        "E-WEB-TIR-UNSUPPORTED",
                        format!("web output cannot compile `{}` yet", miss.func_name),
                        "web builds use the same checked executable body path as native builds; this function uses a construct the web output cannot lower today".to_string(),
                        "move the unsupported work behind a Wasm export that uses covered Jet constructs, or simplify this function for the web target".to_string(),
                        Some(miss.span),
                    )
                })
                .collect();
        if !web_tir_errors.is_empty() {
            return Err(web_tir_errors);
        }
    }
    let rust = crate::Codegen::emit_bundle(&bundle, mode, ffi.as_ref());
    let web = if options.web_target {
        Some(crate::Codegen::emit_web(&bundle, mode, ffi.as_ref()).map_err(|miss| {
            vec![Diagnostic::error(
                "E-WEB-TIR-UNSUPPORTED",
                format!("web output cannot compile `{}` yet", miss.func_name),
                "web emitter capability facts drifted after validation".to_string(),
                "report this compiler bug with the named function".to_string(),
                Some(miss.span),
            )]
        })?)
    } else {
        None
    };
    // c110: capabilities are derived from semantic facts (resolved Core calls,
    // `@Unsafe` gates, FFI declarations), not from scanning the lowered Rust.
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

/// Check-only from file (+ optional in-memory overlay).
///
/// The `overlay` pair is `(canonical_path, text)` — the same shape
/// `Loader::load_entry_with_overlay` expects. Pass `None` for a plain
/// on-disk check; pass `Some((&abs, text))` for an LSP unsaved-buffer check.
/// `is_lsp` is forwarded as the `for_check` flag to the loader.
pub fn check_file(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (Vec<Diagnostic>, Option<crate::AST::ProgramBundle>) {
    let (diags, bundle, _facts) = check_file_with_effect_facts(file, overlay, is_lsp);
    (diags, bundle)
}

/// Like `check_file` but also returns effect facts for D-SEMINDEX1.
pub fn check_file_with_effect_facts(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    check_file_with_effect_facts_impl(file, overlay, is_lsp, None)
}

pub fn check_file_with_effect_facts_incremental(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
    cache: &mut crate::Sema::IncrementalSemaCache,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    check_file_with_effect_facts_impl(file, overlay, is_lsp, Some(cache))
}

fn check_file_with_effect_facts_impl(
    file: &str,
    overlay: Option<(&Path, &str)>,
    is_lsp: bool,
    incremental: Option<&mut crate::Sema::IncrementalSemaCache>,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    match crate::Loader::load_entry_with_overlay(file, overlay, is_lsp) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            let (check_diags, facts) = match incremental {
                Some(cache) => crate::Sema::check_bundle_with_effect_facts_incremental(
                    &mut bundle,
                    crate::Sema::CompileMode::Check,
                    cache,
                ),
                None => crate::Sema::check_bundle_with_effect_facts(
                    &mut bundle,
                    crate::Sema::CompileMode::Check,
                ),
            };
            diags.extend(check_diags);
            if let Some(crate::AST::Item::Func(build)) = bundle.modules[bundle.entry]
                .items
                .iter()
                .find(|item| matches!(item, crate::AST::Item::Func(func) if func.name == "build"))
            {
                if !valid_build_signature(build) {
                    diags.push(bad_build_signature(build.name_span));
                }
            }
            (diags, Some(bundle), facts)
        }
        Err(diags) => (diags, None, crate::Sema::SemIndexEffectFacts::default()),
    }
}

/// Check a staged multi-file tree. This is the authoritative compiler path;
/// callers do not need to mirror parser, loader, or sema behavior.
pub fn check_file_with_overlays(
    file: &str,
    overlays: &[(&Path, &str)],
    is_lsp: bool,
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    match crate::Loader::load_entry_with_overlays(file, overlays, is_lsp) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            let (check_diags, facts) = crate::Sema::check_bundle_with_effect_facts(
                &mut bundle,
                crate::Sema::CompileMode::Check,
            );
            diags.extend(check_diags);
            if let Some(crate::AST::Item::Func(build)) = bundle.modules[bundle.entry]
                .items
                .iter()
                .find(|item| matches!(item, crate::AST::Item::Func(func) if func.name == "build"))
            {
                if !valid_build_signature(build) {
                    diags.push(bad_build_signature(build.name_span));
                }
            }
            (diags, Some(bundle), facts)
        }
        Err(diags) => (diags, None, crate::Sema::SemIndexEffectFacts::default()),
    }
}

/// Structural tools check a staged file in its actual output directory and
/// load adjacent modules referenced by unqualified imports. This retains the
/// same parser/sema authority as ordinary checking.
pub fn check_file_with_overlays_and_import_root(
    file: &str,
    overlays: &[(&Path, &str)],
) -> (
    Vec<Diagnostic>,
    Option<crate::AST::ProgramBundle>,
    crate::Sema::SemIndexEffectFacts,
) {
    match crate::Loader::load_entry_with_overlays_and_import_root(file, overlays, false) {
        Ok(mut bundle) => {
            let mut diags = std::mem::take(&mut bundle.parse_teaching);
            let (check_diags, facts) = crate::Sema::check_bundle_with_effect_facts(
                &mut bundle,
                crate::Sema::CompileMode::Check,
            );
            diags.extend(check_diags);
            (diags, Some(bundle), facts)
        }
        Err(diags) => (diags, None, crate::Sema::SemIndexEffectFacts::default()),
    }
}

/// Check-only from source text (eval mode). Returns only error-severity diagnostics.
pub fn check_eval(src: &str, file: &str) -> Vec<Diagnostic> {
    let (toks, lex_diags) = crate::Lexer::lex(src);
    if !lex_diags.is_empty() {
        return lex_diags;
    }
    let mut prog = match crate::Parser::parse(&toks) {
        Ok(p) => p,
        Err(ds) => return ds,
    };
    let mut bundle = crate::AST::ProgramBundle {
        entry: 0,
        project_root: std::path::PathBuf::from(
            std::path::Path::new(file)
                .parent()
                .unwrap_or(std::path::Path::new(".")),
        ),
        modules: vec![crate::AST::LoadedModule {
            path: std::path::PathBuf::from(file),
            display: file.to_string(),
            alias: "main".to_string(),
            imports: std::mem::take(&mut prog.imports),
            items: std::mem::take(&mut prog.items),
            block_spans: std::mem::take(&mut prog.block_spans),
            source: src.to_string(),
            web_target_ceiling: prog.web_target_ceiling,
            pub_file: prog.pub_file,
            no_prelude: prog.no_prelude,
            html_path: prog.html_path.clone(),
            no_alloc_policy: prog.no_alloc_policy,
            policy_declarations: prog.policy_declarations.clone(),
        }],
        parse_teaching: Vec::new(),
        used_core: std::collections::HashSet::new(),
        ffi_callback_fns: std::collections::HashSet::new(),
        cffi: crate::CFFI::CFfi::default(),
        comptime_inputs: Vec::new(),
        import_targets: std::collections::HashMap::new(),
        layer_ceiling: None,
        inferred_layer: crate::Syntax::RuntimeLayer::Core,
        web_partitions: std::collections::HashMap::new(),
        web_partition_enforced: false,
        web_partition_report: None,
        dep_roots: std::collections::HashMap::new(),
        active_os: crate::Syntax::OsTarget::host(),
    };
    if let Err(diags) = crate::Foreign::assemble_active_namespaces(&mut bundle) {
        return diags;
    }
    bundle.cffi = match crate::CFFI::assemble(&mut bundle) {
        Ok(c) => c,
        Err(diags) => return diags,
    };
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Eval);
    diags
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

/// Test harness pipeline.
pub fn compile_tests(
    file: &str,
    coverage: bool,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((
        crate::Codegen::emit_bundle_tests_cov(&bundle, ffi.as_ref(), coverage),
        ffi,
    ))
}

/// D-TESTKIT1=A (c308 pass 2, gap #1): a CLI-level error selecting the `jet
/// fuzz` target — no property test, an ambiguous set, or a named test that
/// doesn't exist / isn't a property test. Same tier as `run_bench`'s "can't
/// find the file" message: argument validation, not a compiler diagnostic.
pub enum FuzzCompileError {
    Diagnostics(Vec<Diagnostic>),
    Target(String),
}

/// `jet fuzz <file> [<name>]` pipeline: same front end as `compile_tests`
/// (sema runs in `Test` mode — a property test's body is checked exactly as
/// `jet test` checks it), but codegen emits the fuzz driver harness instead.
pub fn compile_fuzz(
    file: &str,
    test_name: Option<&str>,
) -> Result<(String, Option<crate::FFI::FfiLink>), FuzzCompileError> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)
        .map_err(FuzzCompileError::Diagnostics)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Test);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(FuzzCompileError::Diagnostics(errors));
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(FuzzCompileError::Diagnostics(ffi_diags)),
    };
    match crate::Codegen::emit_bundle_fuzz(&bundle, ffi.as_ref(), file, test_name) {
        Ok(code) => Ok((code, ffi)),
        Err(msg) => Err(FuzzCompileError::Target(msg)),
    }
}

/// c-devserver (owner-directed 2026-07-01): `jet dev <file>` when the file
/// defines a top-level `fn dev()` — compiles NATIVELY with `dev()` as the
/// program's real entry instead of `run()`. Mechanically: before sema runs,
/// park any existing `fn run` and inject a synthetic `fn run() { entry_fn() }`
/// (I3: codegen stays dumb; sema never special-cases any entry name other
/// than `"run"` — see `Registration.rs`/`Bundle.rs`'s `funcs.get("run")`).
/// The selected function keeps its source name so callers (D-JPK-TASKRUN1
/// plain-call task deps) still resolve. Same path serves `jet run --task`.
/// Native only — never freestanding/impure/web (those toggles don't apply to
/// the `fn dev()` entry path; a `dev()` function's job is to configure and run
/// an ordinary value like `core.web.devserver`, nothing more).
pub fn compile_bundle_path_with_entry(
    file: &str,
    entry_fn: &str,
) -> Result<crate::CompileOutput, Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    swap_entry_point(&mut bundle, entry_fn);
    let mode = crate::Sema::CompileMode::Run;
    let diags = crate::Sema::check_bundle(&mut bundle, mode);
    // U11 (D-JPK-SCRIPTDEP1=A): see the matching comment in
    // `compile_bundle_path_opts_dbg` — `parse_teaching` rides along here too.
    let mut errors = Vec::new();
    let mut lints = Vec::new();
    for d in std::mem::take(&mut bundle.parse_teaching)
        .into_iter()
        .chain(diags)
    {
        match d.severity {
            Severity::Error => errors.push(d),
            Severity::Lint => lints.push(d),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    // D-OSTARGET1=A: `jet dev`'s entry-swap path never cross-compiles — host OS.
    let rust = crate::Codegen::emit_bundle_dbg(
        &bundle,
        ffi.as_ref(),
        false,
        crate::Syntax::OsTarget::host(),
    );
    let capabilities = crate::Capabilities::from_sema(
        &bundle.used_core,
        crate::bundle_uses_unsafe(&bundle),
        ffi.is_some() || bundle.cffi.links_c(),
    );
    let comptime_inputs = std::mem::take(&mut bundle.comptime_inputs);
    Ok(crate::CompileOutput {
        rust,
        lints,
        ffi,
        clinks: Vec::new(),
        capabilities,
        comptime_inputs,
        web: None,
        web_partition_report: bundle.web_partition_report.clone(),
        plugin: None,
        inferred_layer: bundle.inferred_layer,
        layer_ceiling: bundle.layer_ceiling,
    })
}

/// Make `entry_fn` the program entry without renaming it for name resolution.
///
/// Sema/codegen still require a literal `fn run` (Registration/Bundle
/// `funcs.get("run")`). D-JPK-TASKRUN1 also says a cross-task dependency is a
/// plain call — so renaming `@Task fn greet` → `run` would break
/// `seed()`'s `greet()` with E0102. Fix: park any existing `fn run` as
/// `__jet_unused_run`, then inject a synthetic `fn run(…) { entry_fn(…) }`
/// that forwards params (and return) while leaving `entry_fn` callable.
///
/// The wrapper is never `@Task` (avoids E0928 on reserved lifecycle name
/// `run`). A no-op when `entry_fn` is already `"run"`, or when no function
/// named `entry_fn` exists (caller surfaces E0101 / E1294 separately).
fn swap_entry_point(bundle: &mut crate::AST::ProgramBundle, entry_fn: &str) {
    if entry_fn == "run" {
        return;
    }
    use crate::Diagnostics::Span;
    use crate::AST::{Call, CallArg, CallArgFlags, Expr, Func, Item, Stmt};

    let items = &mut bundle.modules[bundle.entry].items;
    let Some(target) = items.iter().find_map(|item| match item {
        Item::Func(f) if f.name == entry_fn => Some(f.clone()),
        _ => None,
    }) else {
        return;
    };

    for item in items.iter_mut() {
        if let Item::Func(f) = item {
            if f.name == "run" {
                f.name = "__jet_unused_run".to_string();
            }
        }
    }

    let zero = Span::new(0, 0);
    let args: Vec<CallArg> = target
        .params
        .iter()
        .map(|p| CallArg {
            convention: p.convention,
            expr: Expr::Ident(p.name.clone(), p.name_span),
            span: p.name_span,
            flags: CallArgFlags::default(),
            label: None,
            spread: p.variadic,
        })
        .collect();
    let call = Expr::Call(Call {
        name: entry_fn.to_string(),
        name_span: target.name_span,
        args,
        range_checked: false,
    });
    let body = if target.return_type.is_some() {
        vec![Stmt::Return(Some(call), zero)]
    } else {
        vec![Stmt::Expr(call)]
    };

    items.push(Item::Func(Func {
        span: target.span,
        is_pub: false,
        is_package_pub: false,
        external_type: None,
        name: "run".to_string(),
        name_span: target.name_span,
        meta: None,
        type_params: target.type_params.clone(),
        params: target.params.clone(),
        return_type: target.return_type.clone(),
        return_type_span: target.return_type_span,
        return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
        is_unsafe: false,
        unsafe_reason: None,
        unsafe_span: None,
        is_pure: false,
        is_sanitizer: false,
        declared_effects: None,
        effect_via: None,
        state_requires: None,
        state_transition: None,
        is_reactive: false,
        is_replayable: false,
        replayable_span: None,
        is_task: false,
        task_span: None,
        every: None,
        is_must_use: false,
        must_use_span: None,
        maturity: None,
        maturity_span: None,
        is_inline: false,
        is_inline_always: false,
        inline_span: None,
        web_marker: None,
        pre: Vec::new(),
        post: Vec::new(),
        inline_foreign: None,
        body,
    }));
}

/// Bench pipeline.
pub fn compile_benches(
    file: &str,
) -> Result<(String, Option<crate::FFI::FfiLink>), Vec<Diagnostic>> {
    let mut bundle = crate::Loader::load_entry_with_overlay(file, None, false)?;
    let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Bench);
    let mut errors = Vec::new();
    for d in diags {
        if d.severity == Severity::Error {
            errors.push(d);
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let ffi = match crate::FFI::prepare(&bundle) {
        Ok(link) => link,
        Err(ffi_diags) => return Err(ffi_diags),
    };
    Ok((
        crate::Codegen::emit_bundle_benches(&bundle, ffi.as_ref()),
        ffi,
    ))
}
