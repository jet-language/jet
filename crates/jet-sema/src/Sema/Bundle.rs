use super::*;
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use crate::AST::{
    CodeModule, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr, ForKind, Func, GenericModuleDef,
    GenericModuleParam, IfStmt, ImportKind, Item, LValue, LambdaBody, ModuleAliasDef, ModuleArg,
    OrFallback, ProgramBundle, RustConstKind, Stmt, StrPart, Type, VariantPayload,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod GenericModules;
mod Validation;

pub(crate) use GenericModules::expand_generic_module_aliases;
use GenericModules::{clone_enum, clone_struct};
#[allow(unused_imports)]
pub(crate) use Validation::{
    check_func_body_bundle, check_module_bodies, collect_core_expr, collect_core_if,
    collect_core_lvalue, collect_core_stmts, collect_used_core, fn_types_compatible,
    func_sig_to_fn_type, register_func_item,
};
use Validation::{
    apply_helper_layer_inference, qualified_effect_facts, taint_check_item,
};

fn is_fallible_void_entry_return(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Result { ok, err }
            if matches!(ok.as_ref(), Type::Named(n) if n == Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n) if n == Syntax::TYPE_ERROR)
    )
}

/// D-CLIFLAG1: what `fn run`'s single parameter type turned out to be.
enum CliEntryShape {
    /// A `@[Cli]`-derived struct — flags come straight from its fields.
    Struct,
    /// An `enum` whose every variant carries a `@[Cli]` struct payload.
    Enum,
    /// An `enum` parameter with at least one non-`@[Cli]` variant (E1307).
    EnumBadVariants(Vec<Diagnostic>),
    /// Neither of the above (E1308).
    Invalid,
}

/// D-CLIFLAG1: classify `fn run`'s parameter type against its defining module.
/// The entry signature stays in the entry file; its public `@[Cli]` type may
/// live in one directly imported module.
fn cli_entry_param_shape(items: &[Item], ty: &Type, reg: &TraitRegistry) -> CliEntryShape {
    let Type::Named(name) = ty else {
        return CliEntryShape::Invalid;
    };
    let name = name.rsplit('.').next().unwrap_or(name);
    if reg.implements_trait(name, "Cli") {
        return CliEntryShape::Struct;
    }
    let enum_def: Option<&EnumDef> = items.iter().find_map(|i| match i {
        Item::Enum(e) if &e.name == name => Some(e),
        _ => None,
    });
    let Some(e) = enum_def else {
        return CliEntryShape::Invalid;
    };
    let mut bad = Vec::new();
    for v in &e.variants {
        let ok = matches!(
            &v.payload,
            VariantPayload::Single(Type::Named(p), _) if reg.implements_trait(p, "Cli")
        );
        if !ok {
            bad.push(e1307(&v.name, v.name_span));
        }
    }
    if bad.is_empty() {
        CliEntryShape::Enum
    } else {
        CliEntryShape::EnumBadVariants(bad)
    }
}

/// E0101: the entry file has no canonical `fn run`.
fn no_run_error() -> Diagnostic {
    Diagnostic::error(
        "E0101",
        "this program has no `run` function".to_string(),
        "running a program starts at `fn run`, and the entry file doesn't define one".to_string(),
        "add `fn run() { ... }` to the entry file".to_string(),
        None,
    )
}

fn package_scope_for(path: &Path, project_root: &Path) -> PathBuf {
    let norm_path = normalize_sem_path(path);
    let norm_root = normalize_sem_path(project_root);
    if norm_path.starts_with(&norm_root) {
        return norm_root;
    }
    norm_path
        .parent()
        .map(normalize_sem_path)
        .unwrap_or(norm_path)
}

fn normalize_sem_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// D-MOD2: inside an inline `module math { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `math__helper`. This pre-pass rewrites
/// such call names so registration, body-checking, and codegen all agree.
/// Only callee names are rewritten (the unambiguous case); a sibling referenced
/// as a value resolves through normal name lookup and yields a clean Jet error
/// rather than leaking to rustc.
pub(crate) fn mangle_inline_sibling_calls(bundle: &mut ProgramBundle) {
    for module in bundle.modules.iter_mut() {
        for item in module.items.iter_mut() {
            let Item::CodeModule(cm) = item else { continue };
            let Some(body) = &mut cm.body else { continue };
            let siblings: HashSet<String> = body
                .iter()
                .filter_map(|i| match i {
                    Item::Func(f) => Some(f.name.clone()),
                    _ => None,
                })
                .collect();
            if siblings.is_empty() {
                continue;
            }
            for inner in body.iter_mut() {
                if let Item::Func(f) = inner {
                    rewrite_inline_calls_stmts(&mut f.body, &siblings, &cm.name);
                }
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_stmts(
    stmts: &mut [Stmt],
    siblings: &HashSet<String>,
    modname: &str,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Val(b) => rewrite_inline_calls_expr(&mut b.init, siblings, modname),
            Stmt::Assign { value, .. } => rewrite_inline_calls_expr(value, siblings, modname),
            Stmt::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Return(None, _)
            | Stmt::Break(_)
            | Stmt::Continue(_)
            | Stmt::BreakLabel(..)
            | Stmt::ContinueLabel(..) => {}
            Stmt::If(ifs) => rewrite_inline_calls_if(ifs, siblings, modname),
            Stmt::While { cond, body, .. } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        rewrite_inline_calls_expr(start, siblings, modname);
                        rewrite_inline_calls_expr(end, siblings, modname);
                        if let Some(step) = step {
                            rewrite_inline_calls_expr(step, siblings, modname);
                        }
                    }
                    ForKind::In { collection } => {
                        rewrite_inline_calls_expr(collection, siblings, modname);
                    }
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                rewrite_inline_calls_expr(subject, siblings, modname);
                for a in arms.iter_mut() {
                    rewrite_inline_calls_expr(&mut a.cond, siblings, modname);
                    rewrite_inline_calls_stmts(&mut a.body, siblings, modname);
                }
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                body: inner,
                ..
            } => {
                rewrite_inline_calls_expr(&mut init.init, siblings, modname);
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(inner, siblings, modname);
            }
            Stmt::Loop { body: inner, .. }
            | Stmt::Unsafe { body: inner, .. }
            | Stmt::Impure { body: inner, .. }
            | Stmt::Reactive { body: inner, .. }
            | Stmt::Shield { body: inner, .. }
            | Stmt::Off { body: inner, .. }
            | Stmt::DebugOnly { body: inner, .. }
            | Stmt::Region { body: inner, .. }
            | Stmt::Policy { body: inner, .. }
            | Stmt::TaskGroup { body: inner, .. }
            | Stmt::Layout { body: inner, .. }
            | Stmt::Caps { body: inner, .. }
            | Stmt::Grant { body: inner, .. }
            | Stmt::Transact { body: inner, .. }
            | Stmt::AssumeDet { body: inner, .. } => {
                rewrite_inline_calls_stmts(inner, siblings, modname);
            }
            // D-CTMARKER1: rewrite inline calls in comptime block body.
            Stmt::ComptimeBlock { body, .. } => rewrite_inline_calls_stmts(body, siblings, modname),
            // D-WHEN1: rewrite calls in both arms so sibling resolution works
            // regardless of which arm is selected at comptime.
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(then_body, siblings, modname);
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            // D-CTX1: rewrite inline calls in field values and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields.iter_mut() {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            // D-TERM1 (ratified 2026-06-22): rewrite inline calls in live block body.
            Stmt::Live { body, .. } => {
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            // D-DOTSCOPE1: rewrite inline calls in a scope-member region body.
            Stmt::ScopeMember { body, .. } => {
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_if(ifs: &mut IfStmt, siblings: &HashSet<String>, modname: &str) {
    rewrite_inline_calls_expr(&mut ifs.cond, siblings, modname);
    rewrite_inline_calls_stmts(&mut ifs.then_body, siblings, modname);
    match &mut ifs.else_branch {
        Some(ElseBranch::Else(b)) => rewrite_inline_calls_stmts(b, siblings, modname),
        Some(ElseBranch::ElseIf(next)) => rewrite_inline_calls_if(next, siblings, modname),
        None => {}
    }
}

pub(crate) fn rewrite_inline_calls_expr(
    expr: &mut Expr,
    siblings: &HashSet<String>,
    modname: &str,
) {
    match expr {
        Expr::Call(c) => {
            if siblings.contains(&c.name) {
                c.name = format!("{}__{}", modname, c.name);
            }
            for a in c.args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::PtrFromAddr { addr, .. } => rewrite_inline_calls_expr(addr, siblings, modname),
        Expr::Ident(name, _) => {
            if siblings.contains(name) {
                *name = modname.to_string();
            }
        }
        Expr::Char(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Str(parts, _) => {
            for p in parts.iter_mut() {
                if let StrPart::Interp(e, _) = p {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => rewrite_inline_calls_expr(inner, siblings, modname),
        Expr::OptField { base, .. } => rewrite_inline_calls_expr(base, siblings, modname),
        Expr::MethodCall { receiver, args, .. } => {
            rewrite_inline_calls_expr(receiver, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        rewrite_inline_calls_expr(e, siblings, modname);
                    }
                }
            }
        }
        Expr::OrFallback { value, fallback, .. } => {
            rewrite_inline_calls_expr(value, siblings, modname);
            match fallback {
                OrFallback::Value(e) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(None, _)
                | OrFallback::Panic { .. }
                | OrFallback::Break(_)
                | OrFallback::Continue(_) => {}
            }
        }
        Expr::PatternTest { subject, .. } => {
            rewrite_inline_calls_expr(subject, siblings, modname)
        }
        Expr::Binary(_, l, r, _) => {
            rewrite_inline_calls_expr(l, siblings, modname);
            rewrite_inline_calls_expr(r, siblings, modname);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::ListLit(elems, _) => {
            for e in elems.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries.iter_mut() {
                rewrite_inline_calls_expr(k, siblings, modname);
                rewrite_inline_calls_expr(v, siblings, modname);
            }
        }
        Expr::Index { base, index, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(index, siblings, modname);
        }
        Expr::Slice { base, start, end, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(start, siblings, modname);
            rewrite_inline_calls_expr(end, siblings, modname);
        }
        Expr::CallValue { callee, args, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::Lambda(lam) => match &mut lam.body {
            LambdaBody::Expr(e) => rewrite_inline_calls_expr(e, siblings, modname),
            LambdaBody::Block(stmts) => rewrite_inline_calls_stmts(stmts, siblings, modname),
        },
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            rewrite_inline_calls_expr(cond, siblings, modname);
            rewrite_inline_calls_stmts(then_body, siblings, modname);
            rewrite_inline_calls_expr(then_value, siblings, modname);
            rewrite_inline_calls_stmts(else_body, siblings, modname);
            rewrite_inline_calls_expr(else_value, siblings, modname);
        }
        Expr::FanOut { callee, items, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for item in items.iter_mut() {
                rewrite_inline_calls_expr(item, siblings, modname);
            }
        }
        Expr::Paren(inner, _) => rewrite_inline_calls_expr(inner, siblings, modname),
        Expr::Spread(inner, _) => rewrite_inline_calls_expr(inner, siblings, modname),
    }
}

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, false, false).0
}

/// Like `check_bundle` but also returns effect facts for D-SEMINDEX1.
pub fn check_bundle_with_effect_facts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    check_bundle_opts(bundle, mode, false, false)
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, true, false).0
}

/// Like `check_bundle` but with D-CTEFFECT1 `--allow-impure` flag.
pub fn check_bundle_allow_impure(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, false, true).0
}

pub(crate) fn check_bundle_opts(
    bundle: &mut ProgramBundle,
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
) -> (Vec<Diagnostic>, super::Effects::SemIndexEffectFacts) {
    let mut diags = super::BudgetSpecs::validate_bundle(bundle);
    diags.extend(super::Casing::validate_bundle(bundle));
    // D-OSTARGET2=B (ratified 2026-07-03): fold every `comptime if build.os == {
    // … }` switch to the arm matching this build's active OS *before* any other
    // pass sees a body — so OS-gating checks, the type-checker, and codegen only
    // meet the taken arm. Rewrites into a `comptime if` chain (reuses D-WHEN1).
    diags.extend(super::desugar_os_switches(bundle));
    // D-MIGRATE4: desugar each `change … via { (old) => … }` converter on a
    // decodable `@PublishedSchema` type into a synthetic top-level converter
    // function, so the runtime migration step (codegen) can call it. Runs before
    // registration/checking so those synthetic functions are type-checked and
    // lowered through the normal pipeline. Sets `conv_fn` on the `change` op.
    super::desugar_migrations(bundle);
    // D-GENMOD2=A: expand module aliases into concrete CodeModules before any
    // sibling-call mangling or registration sees the items.
    expand_generic_module_aliases(bundle, &mut diags);
    // D-MOD2: rewrite inline-module sibling calls to their mangled names before any
    // registration/checking/codegen sees the bodies.
    mangle_inline_sibling_calls(bundle);
    // D-UNSAFE-OBLIG1=A: run after compile-time branch selection and generic
    // module expansion, but before registration/TIR. Assertions are checked and
    // erased here so no generated or untaken body bypasses the policy.
    diags.extend(super::UnsafeObligations::check_and_strip(bundle));
    let mut states: Vec<ModuleState> = bundle
        .modules
        .iter()
        .map(|m| ModuleState {
            module_path: m.display.clone(),
            module_alias: m.alias.clone(),
            func_spans: HashMap::new(),
            const_spans: HashMap::new(),
            import_spans: HashMap::new(),
            package_scope: package_scope_for(&m.path, &bundle.project_root),
            funcs: HashMap::new(),
            func_pub: HashMap::new(),
            func_pkg_pub: HashMap::new(),
            type_pub: HashMap::new(),
            type_pkg_pub: HashMap::new(),
            method_pub: HashMap::new(),
            method_pkg_pub: HashMap::new(),
            field_pub: HashMap::new(),
            field_pkg_pub: HashMap::new(),
            registry: TypeRegistry {
                types: HashMap::new(),
                unit_dimensions: HashMap::new(),
                computed_fields: HashMap::new(),
            },
            consts: HashMap::new(),
            imports: HashMap::new(),
            core_imports: HashMap::new(),
            tests: HashMap::new(),
            trait_reg: TraitRegistry::default(),
            code_modules: HashMap::new(),
            code_module_identities: HashMap::new(),
            unqualified: HashMap::new(),
            unqualified_file: HashMap::new(),
            reexports: HashMap::new(),
        })
        .collect();

    // Generic-instance declarations have one AST/codegen owner, while every
    // consumer registry receives the same nominal metadata. This is not a
    // declaration clone: generated Rust/TIR still sees the owner item once.
    let shared_instance_nominals: Vec<(usize, Item)> = bundle.modules.iter().enumerate().flat_map(|(owner, module)| {
        let prefixes: Vec<String> = module.items.iter().filter_map(|item| match item {
            Item::CodeModule(cm) if cm.instance_identity.is_some() =>
                Some(GenericModules::module_type_prefix(&cm.name)),
            _ => None,
        }).collect();
        module.items.iter().filter_map(move |item| match item {
            Item::Struct(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Struct(clone_struct(def)))),
            Item::Enum(def) if prefixes.iter().any(|prefix| def.name.starts_with(prefix)) => Some((owner, Item::Enum(clone_enum(def)))),
            _ => None,
        })
    }).collect();
    for (owner, item) in &shared_instance_nominals {
        for (consumer, st) in states.iter_mut().enumerate() {
            if consumer == *owner { continue; }
            match item {
                Item::Struct(def) => {
                    register_struct(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                Item::Enum(def) => {
                    register_enum(def, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(def.name.clone(), def.is_pub && !def.is_package_pub);
                    st.type_pkg_pub.insert(def.name.clone(), def.is_package_pub);
                }
                _ => unreachable!(),
            }
        }
    }

    // D-METADERIVE1=A orphan law needs a bundle-wide provider view: a derive
    // may be supplied by the entry module for an imported type, or imported
    // for an entry-local type.  Clone provider bodies/helpers before mutating
    // modules so expansion can attach generated items beside the target type.
    let derive_providers: Vec<(
        usize,
        String,
        String,
        Vec<crate::AST::Stmt>,
        HashMap<String, Func>,
    )> = bundle
        .modules
        .iter()
        .enumerate()
        .flat_map(|(origin, module)| {
            let helpers: HashMap<String, Func> = module
                .items
                .iter()
                .filter_map(|item| match item {
                    Item::Func(f) => Some((f.name.clone(), f.clone())),
                    _ => None,
                })
                .collect();
            module.items.iter().filter_map(move |item| match item {
                Item::UserDerive(d) => Some((
                    origin,
                    d.trait_name.clone(),
                    d.type_param.clone(),
                    d.body.clone(),
                    helpers.clone(),
                )),
                _ => None,
            })
        })
        .collect();

    // D-MARK-VOCAB1 (card #518): the dynamic half of the `@Rule` vocabulary
    // vocabulary — every `derive T.Name { … }` provider in the bundle, not
    // just this module's own, per the same bundle-wide orphan-rule view as
    // `derive_providers` above.
    let known_derive_names: HashSet<String> =
        derive_providers.iter().map(|(_, name, _, _, _)| name.clone()).collect();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        super::Protocol::expand_module_protocols(&mut module.items, &mut diags);
        // D-DOTSCOPE1: validate contextual `.member { … }` scope statements
        // against each marker's declared vocabulary (E0614/E0615/E0616/E0617/E0618).
        diags.extend(super::ScopeMembers::check(&module.items));
        // D-FIELDPOL1: computed-field cycle check (E0338) + `self.field`
        // rewrite + synthesized getter methods, before anything else.
        process_computed_fields(&mut module.items, &mut diags);
        // D-VALIDATE1 (card #506): `validate { … }` block shape check +
        // synthesized `Type.validate(value)`, same pre-registration timing.
        process_validate_blocks(&mut module.items, &mut diags);
        // D-PATCH1: synthetic `T.Patch` before struct registration.
        inject_patchable_types(&mut module.items, &mut diags);
        // Card #436: `CFFI::assemble` (jetpack crate) drains every
        // `@Extern`/`@Bindgen module` out of its declaring file and re-homes
        // it in a synthetic per-lib module (`<c.lib>`) with an empty
        // registry of its own — so a struct/enum/distinct declared in an
        // ordinary file was NEVER visible to `is_c_abi_type`'s `Type::Named`
        // lookup (`c_named_type_ok`, Sema/FFI.rs), and every named type was
        // silently rejected at the C boundary regardless of its shape. Real
        // modules are always processed before any synthetic one (assemble
        // only appends), so by this iteration every preceding module's
        // registry is already fully populated; merge them once here so a
        // same-project named type resolves. Type names are unique
        // program-wide (a duplicate definition is its own error elsewhere),
        // so this union is sound.
        let ffi_named_types: Option<HashMap<String, TypeDef>> = if module
            .items
            .iter()
            .any(|i| matches!(i, Item::CModule(_)))
        {
            Some(
                states[..idx]
                    .iter()
                    .flat_map(|s| s.registry.types.iter())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        } else {
            None
        };
        let st = &mut states[idx];
        for import in &module.imports {
            if !matches!(import.kind, crate::AST::ImportKind::Unqualified { .. }) {
                st.import_spans.insert(import.import_alias(), import.alias_span);
            }
        }
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    st.func_spans.insert(f.name.clone(), f.name_span);
                }
                Item::Const(c) => {
                    st.const_spans.insert(c.name.clone(), c.name_span);
                }
                _ => {}
            }
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags),
                Item::Struct(s) => {
                    register_struct(
                        s,
                        &mut st.registry,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    );
                    st.type_pub
                        .insert(s.name.clone(), s.is_pub && !s.is_package_pub);
                    st.type_pkg_pub.insert(s.name.clone(), s.is_package_pub);
                    for fld in &s.fields {
                        st.field_pub.insert(
                            (s.name.clone(), fld.name.clone()),
                            fld.is_pub && !fld.is_package_pub,
                        );
                        st.field_pkg_pub
                            .insert((s.name.clone(), fld.name.clone()), fld.is_package_pub);
                    }
                    for m in &s.methods {
                        st.method_pub.insert(
                            (s.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((s.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(e.name.clone(), e.is_pub && !e.is_package_pub);
                    st.type_pkg_pub.insert(e.name.clone(), e.is_package_pub);
                    for m in &e.methods {
                        st.method_pub.insert(
                            (e.name.clone(), m.name.clone()),
                            m.is_pub && !m.is_package_pub,
                        );
                        st.method_pkg_pub
                            .insert((e.name.clone(), m.name.clone()), m.is_package_pub);
                    }
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
                        diags.push(Diagnostic::error(
                            "E0301",
                            format!("`impl {}` names a type that doesn't exist", i.type_name),
                            format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                            format!(
                                "define `struct {}` or `enum {}` first",
                                i.type_name, i.type_name
                            ),
                            Some(i.type_span),
                        ));
                    } else if !i.type_name.contains('.') {
                        for m in &i.methods {
                            st.method_pub.insert(
                                (i.type_name.clone(), m.name.clone()),
                                m.is_pub && !m.is_package_pub,
                            );
                            st.method_pkg_pub
                                .insert((i.type_name.clone(), m.name.clone()), m.is_package_pub);
                        }
                    }
                }
                Item::Const(c) => {
                    if let Some(meta) = &c.meta {
                        diags.extend(CheckerCore::check_meta_attr_fields(meta));
                    }
                    register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry)
                }
                Item::Distinct(d) => {
                    register_distinct(d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                    st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                }
                Item::TypeAlias(a) => {
                    register_type_alias(a, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub
                        .insert(a.name.clone(), a.is_pub && !a.is_package_pub);
                    st.type_pkg_pub.insert(a.name.clone(), a.is_package_pub);
                }
                // D-QUAL3: a unit family lowers to one `@Numeric` distinct type
                // per member, each erasing to `Float`.
                Item::UnitFamily(uf) => {
                    let dimension = crate::AST::Dimension::for_family(&uf.family);
                    for d in uf.distinct_defs() {
                        register_distinct(&d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                        if let Some(dimension) = dimension {
                            st.registry.unit_dimensions.insert(d.name.clone(), dimension);
                        }
                        st.type_pub
                            .insert(d.name.clone(), d.is_pub && !d.is_package_pub);
                        st.type_pkg_pub.insert(d.name.clone(), d.is_package_pub);
                    }
                }
                Item::Test(t) => {
                    if name_defined(&t.name, &st.funcs, &st.registry, &st.consts)
                        || st.tests.contains_key(&t.name)
                    {
                        diags.push(defined_twice(
                            &t.name,
                            "every test needs a unique name so failures are easy to find",
                            t.name_span,
                        ));
                    } else {
                        st.tests.insert(t.name.clone(), t.name_span);
                    }
                }
                // D-BENCH1: `@Bench` blocks define no referenceable name; codegen
                // discovers them straight from the AST, so registration is a no-op.
                Item::Bench(_) => {}
                Item::ExternRust(block) => {
                    if check_extern_block(block, &st.registry, &mut diags) {
                        for ef in &block.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                                false,
                            );
                        }
                    }
                }
                Item::CModule(cm) => {
                    // Card #436: check named C-ABI types (struct/enum/distinct)
                    // against the merged cross-file view built above, not the
                    // synthetic module's own (empty) registry. See the comment
                    // at `ffi_named_types`'s construction.
                    let merged_registry = ffi_named_types.as_ref().map(|extra| {
                        let mut types = st.registry.types.clone();
                        for (k, v) in extra {
                            types.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        TypeRegistry {
                            types,
                            unit_dimensions: st.registry.unit_dimensions.clone(),
                            computed_fields: st.registry.computed_fields.clone(),
                        }
                    });
                    let check_registry = merged_registry.as_ref().unwrap_or(&st.registry);
                    if check_c_module(cm, check_registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                check_registry,
                                &st.consts,
                                &mut diags,
                                true,
                            );
                            // C FFI functions are callable across the `use c.<lib>`
                            // alias — expose them like any pub item.
                            st.func_pub.insert(ef.name.clone(), true);
                        }
                    }
                }
                Item::Trait(t) => {
                    st.type_pub
                        .insert(t.name.clone(), t.is_pub && !t.is_package_pub);
                    st.type_pkg_pub.insert(t.name.clone(), t.is_package_pub);
                }
                // D-QUAL2: a tag is a marker; it registers no callable items.
                Item::Tag(_) => {}
                Item::Module(_) => {}
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        // D-MOD2: register inline module functions under mangled names
                        // (`math__double`) so call-site sema can check them.
                        st.code_modules.insert(cm.name.clone(), cm.name.clone());
                        st.code_module_identities.insert(
                            cm.name.clone(),
                            cm.instance_identity.as_ref()
                                .map(|identity| format!("instance:{}", identity.fingerprint))
                                .unwrap_or_else(|| format!("module:{}::{}", st.module_path, cm.name)),
                        );
                        for inner in body {
                            if let Item::Func(f) = inner {
                                let mangled = format!("{}__{}", cm.name, f.name);
                                st.func_spans.insert(mangled.clone(), f.name_span);
                                st.funcs.insert(mangled.clone(), func_to_sig(f));
                                st.func_pub.insert(mangled, f.is_pub && !f.is_package_pub);
                                st.func_pkg_pub
                                    .insert(format!("{}__{}", cm.name, f.name), f.is_package_pub);
                            }
                        }
                    }
                }
                Item::ErrorConv(_) => {}
                // D-MIGRATE1: migration decls are handled by the schema diff pass; no registration needed.
                Item::Migration(_) => {}
                // D-STATE-DECL: state-set decls are sema-only (I3); no type to register.
                Item::StateDecl(_) => {}
                // D-PROTO1/D-PROTO2: expanded before registration; declaration erases.
                Item::ProtocolDecl(_) => {}
                // D-METADERIVE1=A: user-authored derive blocks are expanded below; skip here.
                Item::UserDerive(_) => {}
                Item::GenericModule(_) | Item::ModuleAlias(_) => {}
            }
        }
        // D-METADERIVE1=A: user-derive expansion — run after struct/func registration so
        // derive bodies can call helper functions and access TypeInfo. Re-entry (D-CTCODEGEN1=A):
        // emitted fragments go through the full lexer→parser pipeline and are appended as items.
        {
            if !derive_providers.is_empty() {
                let struct_infos: Vec<&crate::AST::StructDef> = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Struct(s) = i {
                            Some(s)
                        } else {
                            None
                        }
                    })
                    .collect();

                let mut new_items: Vec<Item> = Vec::new();

                for s in &struct_infos {
                    for (derive_name, derive_span) in &s.derives {
                        // Prefer an entry-local provider, then one beside the target.
                        // Remaining imported/imported pairs violate the orphan law:
                        // either provider or target must be entry-local.
                        let provider = derive_providers
                            .iter()
                            .filter(|(_, name, _, _, _)| name == derive_name)
                            .min_by_key(|(origin, _, _, _, _)| {
                                if *origin == 0 {
                                    0
                                } else if *origin == idx {
                                    1
                                } else {
                                    2
                                }
                            });
                        let Some((provider_idx, _, type_param, body, helper_funcs)) = provider else {
                            continue;
                        };
                        if idx > 0 && *provider_idx > 0 {
                            diags.push(Diagnostic::error(
                                "E2711",
                                format!(
                                    "derive orphan rule: neither `derive T.{}` nor `{}` is local",
                                    derive_name, s.name
                                ),
                                "a generated implementation is owned locally only when the derive provider or target type lives in the entry module".to_string(),
                                format!(
                                    "define `derive T.{}` or `{}` in the entry module",
                                    derive_name, s.name
                                ),
                                // The violating marker belongs to an imported source file;
                                // the bundled diagnostic currently renders against the entry
                                // file, so omit a misleading entry-file caret.
                                None,
                            ));
                            continue;
                        }
                        let actual_funcs: HashMap<String, &Func> = helper_funcs
                            .iter()
                            .map(|(name, func)| (name.clone(), func))
                            .collect();
                        let type_info = crate::Comptime::build_struct_type_info(s);

                        match crate::Comptime::evaluate_derive_body(
                            body,
                            type_param,
                            type_info,
                            &actual_funcs,
                            &bundle.project_root,
                        ) {
                                Ok(fragments) => {
                                    for fragment in fragments {
                                        let (toks, lex_diags) = crate::Lexer::lex(&fragment);
                                        if !lex_diags.is_empty() {
                                            let detail = lex_diags
                                                .first()
                                                .map(|d| d.what.as_str())
                                                .unwrap_or("the generated text could not be read");
                                            diags.push(Diagnostic::error(
                                                "E2710",
                                                format!(
                                                    "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                    derive_name, derive_name, s.name
                                                ),
                                                format!(
                                                    "generated source did not pass the ordinary lexer and parser: {detail}"
                                                ),
                                                "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                Some(*derive_span),
                                            ));
                                            continue;
                                        }
                                        match crate::Parser::parse(&toks) {
                                            Ok(mut prog) => new_items.extend(prog.items.drain(..)),
                                            Err(parse_diags) => {
                                                let detail = parse_diags
                                                    .first()
                                                    .map(|d| d.what.as_str())
                                                    .unwrap_or("the generated text was not valid Jet");
                                                diags.push(Diagnostic::error(
                                                    "E2710",
                                                    format!(
                                                        "`derive T.{}` generated invalid Jet while expanding `#[{}]` on `{}`",
                                                        derive_name, derive_name, s.name
                                                    ),
                                                    format!(
                                                        "generated source did not pass the ordinary lexer and parser: {detail}"
                                                    ),
                                                    "fix the `derive` body so every emitted fragment is valid Jet source".to_string(),
                                                    Some(*derive_span),
                                                ));
                                            }
                                        }
                                    }
                                }
                                // E2710: derive body failed at comptime. Wrap with context
                                // pointing at the #[TraitName] trigger on the struct.
                            Err(inner) => diags.push(Diagnostic::error(
                                    "E2710",
                                    format!(
                                        "`derive T.{}` body failed while expanding `#[{}]` on `{}`",
                                        derive_name, derive_name, s.name
                                    ),
                                    inner.what.clone(),
                                    "fix the `derive` body so it generates valid Jet at compile time".to_string(),
                                    Some(*derive_span),
                            )),
                        }
                    }
                }

                // Register new items before synthesis so they go through
                // the normal sema pipeline.
                for item in &new_items {
                    match item {
                        Item::Func(f) => register_func_item(f, st, &mut diags),
                        Item::Struct(s) => {
                            register_struct(
                                s,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                            st.type_pub
                                .insert(s.name.clone(), s.is_pub && !s.is_package_pub);
                            st.type_pkg_pub.insert(s.name.clone(), s.is_package_pub);
                            for field in &s.fields {
                                st.field_pub.insert(
                                    (s.name.clone(), field.name.clone()),
                                    field.is_pub && !field.is_package_pub,
                                );
                                st.field_pkg_pub.insert(
                                    (s.name.clone(), field.name.clone()),
                                    field.is_package_pub,
                                );
                            }
                        }
                        Item::Enum(e) => {
                            register_enum(
                                e,
                                &mut st.registry,
                                &mut diags,
                                &st.funcs,
                                &st.consts,
                            );
                            st.type_pub
                                .insert(e.name.clone(), e.is_pub && !e.is_package_pub);
                            st.type_pkg_pub.insert(e.name.clone(), e.is_package_pub);
                        }
                        Item::Impl(i) => {
                            for m in &i.methods {
                                st.method_pub
                                    .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                            }
                        }
                        _ => {}
                    }
                }
                module.items.extend(new_items);
            }
        }

        // Defaults must exist before serde source expansion so Decode bodies
        // embed the evaluated value rather than re-evaluating at runtime.
        let serde_core_imports: HashMap<String, String> = module
            .imports
            .iter()
            .filter_map(|imp| Some((imp.import_alias(), imp.core_module_path()?)))
            .collect();
        let serde_base = module
            .path
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_default_markers(
            &mut module.items,
            &serde_base,
            &mut diags,
            &serde_core_imports,
        );
        // D-SERDE2=A/R11: built-in codecs re-enter as ordinary Jet source in
        // bundle builds too; this is the production multi-file path.
        super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);

        // S62 + D-LIB2: synthesis must happen before register_impl_methods
        // so the synthesised Func nodes appear in the type registry.
        synthesize_impls(&mut module.items);
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_patchable_methods(&module.items, &mut st.registry);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        // D-TXN-ROLLBACK layer 2: ensure Rollback is known before user impl blocks.
        st.trait_reg.register_synthetic_rollback();
        st.trait_reg.register_synthetic_display_debug();
        st.trait_reg.register_synthetic_close();
        st.trait_reg.register_synthetic_iter_index();
        st.trait_reg.register_synthetic_io();
        st.trait_reg.register_items(&module.items, &mut diags);
        // D-SERDE: validate `@[Codable]`/`@[Encode]`/`@[Decode]` markers (E2407–E2412)
        // now that the trait registry resolves field/variant types — keeps the emitted
        // `impl`s rustc-clean (I2).
        diags.extend(validate_serde_items(&module.items, &st.trait_reg));
        // D-MARK-VOCAB1 (card #518): a marker name outside the registered
        // `@`/`#` plane vocabulary is E0927, instead of silently doing
        // nothing (the parser accepts any PascalCase name structurally).
        diags.extend(check_marker_vocabulary(&module.items, &known_derive_names));
        // D-CLIFLAG1: validate `@[Cli]`-derived structs (E1305/E1306), same
        // timing as the serde pass above (trait registry must be built so
        // `Cli` is visible on `s.derives`).
        diags.extend(validate_cli_items(&module.items, &st.trait_reg));
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(
            &module.items,
            &bundle.project_root,
            &st.trait_reg,
        ));
    }

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty, _)) =
                            fields.iter().find(|(n, _, _, _)| n == field_name)
                        {
                            let field_type_name = field_ty.name();
                            if !st.trait_reg.implements_trait(&field_type_name, trait_name) {
                                diags.push(Diagnostic::error(
                                    "E2401",
                                    format!(
                                        "`{}` doesn't implement `{}`, so it can't delegate",
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "`impl {}.{} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                        i.type_name, trait_name, field_name,
                                        trait_name, field_name,
                                        field_type_name, trait_name
                                    ),
                                    format!(
                                        "implement `impl {}: {}` on the field's type, or choose a different field",
                                        field_type_name, trait_name
                                    ),
                                    Some(i.type_span),
                                ));
                            }
                        } else {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!("`{}` has no field `{}`", i.type_name, field_name),
                                format!(
                                    "`impl {}.{} using {}` needs `{}` to have a field named `{}`",
                                    i.type_name, trait_name, field_name, i.type_name, field_name
                                ),
                                format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                                Some(i.type_span),
                            ));
                        }
                    }
                }
            }
        }
    }

    // S57 (M9.5): evaluate comptime bindings per module. `embed_file` paths
    // resolve against each module file's own directory (S16 convention).
    // D-CTCORE1: pre-collect core_imports (alias→module) per module so the
    // comptime interpreter can evaluate whitelisted pure Core calls. Build a
    // SEPARATE local map — not `states[idx].core_imports` — so the duplicate
    // import check in the full import-resolution loop (below) is unaffected.
    let ct_core_imports: Vec<HashMap<String, String>> = bundle
        .modules
        .iter()
        .map(|module| {
            module
                .imports
                .iter()
                .filter_map(|imp| {
                    let path = imp.core_module_path()?;
                    let alias = imp.import_alias();
                    Some((alias, path))
                })
                .collect()
        })
        .collect();
    let mut top_level_embed_inputs = Vec::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let base = module
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_comptime_items(
            &mut module.items,
            &mut states[idx].consts,
            &base,
            &mut diags,
            &ct_core_imports[idx],
            Some(&mut top_level_embed_inputs),
        );
    }
    bundle.comptime_inputs.extend(top_level_embed_inputs);

    // D-MOD3/4: Unqualified imports (`use alias.Item`) are processed in a
    // dedicated pass *after* file-module aliases land in `st.imports` below.

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            // Unqualified imports are handled in the dedicated pass below.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            let alias = imp.import_alias();
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.core_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let ImportKind::Module(name, _) = &imp.kind {
                if crate::Syntax::is_legacy_std_import(name) {
                    diags.push(Diagnostic::error(
                        "E0019",
                        format!("`{name}` is the old standard-library import spelling"),
                        "the standard library module was renamed to `core`".to_string(),
                        format!(
                            "use `import {}` or `import {}.fs as fs`",
                            Syntax::CORE_SHORT,
                            Syntax::CORE_SHORT
                        ),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-CORENS1 / E0341: old `jet.<ring>` spelling → teach the new `core.<ring>`.
                if let Some(ring) = name.strip_prefix("jet.") {
                    if crate::Syntax::is_ring_module(ring) {
                        diags.push(Diagnostic::error(
                            "E0341",
                            format!("`use jet.{ring}` is the old first-party library spelling"),
                            "first-party libraries moved to the `core.*` namespace (D-CORENS1)"
                                .to_string(),
                            format!("write `use core.{ring}` instead"),
                            Some(imp.span),
                        ));
                        continue;
                    }
                }
            }
            if let Some(module) = imp.core_module_path() {
                if !crate::Syntax::is_known_core_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no core module `{}`", module),
                        "`core` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", crate::Syntax::core_modules_list()),
                        Some(imp.span),
                    ));
                    continue;
                }
                // D-RINGLAYER1=A: infer minimum layer and enforce optional ceiling.
                if let Some(mod_layer) = crate::Syntax::core_module_layer(&module) {
                    if let Some(ceiling) = bundle.layer_ceiling {
                        if mod_layer > ceiling {
                            diags.push(crate::Syntax::layer_ceiling_exceeded(
                                &module,
                                mod_layer,
                                ceiling,
                                Some(imp.span),
                                Some(&format!("`use {module}`")),
                            ));
                            continue;
                        }
                    }
                    if mod_layer > bundle.inferred_layer {
                        bundle.inferred_layer = mod_layer;
                    }
                }
                st.core_imports.insert(alias, module);
                continue;
            }
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if imp.is_c_import() {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias, target);
                }
                continue;
            }
            if let Some(target) = bundle.import_targets.get(&(idx, imp.span)).copied() {
                st.imports.insert(alias, target);
            }
        }
    }

    // D-MOD3/4: process `use alias.Item` unqualified imports now that file-module
    // aliases are registered in `st.imports`. `pub use` additionally re-exports the
    // item onto this module's public surface (`reexports`).
    for (idx, module) in bundle.modules.iter().enumerate() {
        for imp in &module.imports {
            let ImportKind::Unqualified {
                module_alias,
                module_alias_span,
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            let st = &mut states[idx];
            if let Some(canonical) = st.code_modules.get(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let mangled = format!("{}__{}", canonical, orig);
                    if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !st.func_pub.get(&mangled).copied().unwrap_or(false)
                        && !st.func_pkg_pub.get(&mangled).copied().unwrap_or(false)
                    {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!(
                                "add `pub` before `fn {}` in module `{}`",
                                orig, module_alias
                            ),
                            Some(*module_alias_span),
                        ));
                    } else {
                        st.unqualified.insert(local.to_string(), mangled.clone());
                        if imp.is_pub {
                            st.reexports.insert(local.to_string(), (mangled, idx));
                        }
                    }
                }
            } else if module_alias == "core" || module_alias == "jet" {
                // Std namespace prefix: `use core.mem` → bind each item as a Core import.
                // Each item `x` becomes `core.x` in the known-modules table.
                let st = &mut states[idx];
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let full = format!("core.{}", orig);
                    if !crate::Syntax::is_known_core_module(&full) {
                        diags.push(Diagnostic::error(
                            "E1001",
                            format!("there is no core module `{}`", full),
                            "`core` is compiler-known in M10, and only the frozen core modules exist".to_string(),
                            format!("import one of: {}", crate::Syntax::core_modules_list()),
                            Some(*module_alias_span),
                        ));
                    } else if st.core_imports.contains_key(local) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{}` is used twice", local),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else {
                        // D-RINGLAYER1=A M2: unqualified `use core.X` obeys the same layer rules.
                        if let Some(mod_layer) = crate::Syntax::core_module_layer(&full) {
                            if let Some(ceiling) = bundle.layer_ceiling {
                                if mod_layer > ceiling {
                                    diags.push(crate::Syntax::layer_ceiling_exceeded(
                                        &full,
                                        mod_layer,
                                        ceiling,
                                        Some(*module_alias_span),
                                        Some(&format!("`use core.{orig}`")),
                                    ));
                                    continue;
                                }
                            }
                            if mod_layer > bundle.inferred_layer {
                                bundle.inferred_layer = mod_layer;
                            }
                        }
                        st.core_imports.insert(local.to_string(), full);
                    }
                }
            } else if st.imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = st.imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for (orig, alias_opt) in items {
                    let local = alias_opt.as_deref().unwrap_or(orig.as_str());
                    let same_pkg = states[target_idx].package_scope == states[idx].package_scope;
                    let is_pub = states[target_idx]
                        .func_pub
                        .get(orig.as_str())
                        .copied()
                        .unwrap_or(false)
                        || (same_pkg
                            && states[target_idx]
                                .func_pkg_pub
                                .get(orig.as_str())
                                .copied()
                                .unwrap_or(false));
                    let exists = states[target_idx].funcs.contains_key(orig.as_str());
                    if !exists {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", orig, module_alias),
                            "check the module for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !is_pub {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", orig, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in the imported file", orig),
                            Some(*module_alias_span),
                        ));
                    } else {
                        states[idx]
                            .unqualified_file
                            .insert(local.to_string(), (orig.clone(), target_idx));
                        if is_reexport {
                            states[idx]
                                .reexports
                                .insert(local.to_string(), (orig.clone(), target_idx));
                        }
                    }
                }
            } else {
                // Module alias not found — E0610.
                diags.push(Diagnostic::error(
                    "E0610",
                    format!("no module named `{}` in scope", module_alias),
                    "the alias must refer to a module imported earlier in this file".to_string(),
                    format!("add `import … as {}`  before this `use`", module_alias),
                    Some(*module_alias_span),
                ));
            }
        }
    }

    for idx in 0..bundle.modules.len() {
        for item in &bundle.modules[idx].items {
            let Item::Impl(i) = item else { continue };
            if !i.type_name.contains('.') {
                continue;
            }
            if !impl_type_exists(
                &i.type_name,
                &states[idx].registry,
                &states[idx].imports,
                Some(&states),
            ) {
                diags.push(Diagnostic::error(
                    "E0301",
                    format!("`impl {}` names a type that doesn't exist", i.type_name),
                    format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                    format!(
                        "define `struct {}` or `enum {}` first",
                        i.type_name, i.type_name
                    ),
                    Some(i.type_span),
                ));
            } else {
                for m in &i.methods {
                    states[idx]
                        .method_pub
                        .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                }
            }
        }
    }

    // Parity with the single-file path: `@static` and address-taken consts
    // must lower to Rust `static` in bundle mode too.
    for module in bundle.modules.iter_mut() {
        let const_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Const(c) => Some(c.name.clone()),
                _ => None,
            })
            .collect();
        let mut address_taken: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken)
                }
                Item::Struct(s) => {
                    for m in &s.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Test(t) => {
                    walk_stmts_for_const_refs(&t.body, &const_names, &mut address_taken)
                }
                Item::Bench(b) => {
                    walk_stmts_for_const_refs(&b.body, &const_names, &mut address_taken)
                }
                Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags erase
            | Item::Module(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases
            | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::ErrorConv(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL: erases
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: already expanded above
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
                c.rust_kind = if force_static || address_taken.contains(&c.name) {
                    RustConstKind::Static
                } else {
                    RustConstKind::Const
                };
            }
        }
    }

    // Each non-entry module becomes a Rust `mod user_<alias>`; a type in the
    // entry file with the same name would collide in the type namespace.
    for (idx, m) in bundle.modules.iter().enumerate() {
        if idx == bundle.entry {
            continue;
        }
        if states[bundle.entry].registry.contains(&m.alias) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "the type `{}` clashes with the imported file `{}`",
                    m.alias, m.display
                ),
                "a type and an imported module can't share a name".to_string(),
                format!(
                    "rename the type, or import with `{} other_name`",
                    Syntax::KW_AS
                ),
                None,
            ));
        }
    }

    let entry = &states[bundle.entry];
    if mode == CompileMode::Run || mode == CompileMode::Eval {
        let entry_items = &bundle.modules[bundle.entry].items;
        if let Some(run_fn) = entry_items.iter().find_map(|i| match i {
            Item::Func(f) if f.name == "run" => Some(f),
            _ => None,
        }) {
            // S12/D-S80-RUN1/D-CLIFLAG1: `run` is the only program entry name.
            // It is zero-arg (optionally `-> Void ?`), or one typed CLI-spec
            // parameter (`@[Cli]` struct / enum).
            if run_fn.params.is_empty() {
                if mode == CompileMode::Run
                    && run_fn
                        .return_type
                        .as_ref()
                        .is_some_and(|ret| !is_fallible_void_entry_return(ret))
                {
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`run` returns the wrong kind of value".to_string(),
                        "`run` is where running starts; it either returns nothing or reports top-level errors with `Void ?`"
                            .to_string(),
                        "write `fn run() { ... }`, or `fn run() -> Void ? { ... }` if the entry uses `?`"
                            .to_string(),
                        Some(run_fn.name_span),
                    ));
                }
            } else if run_fn.params.len() == 1 {
                let param = &run_fn.params[0];
                let cli_module = jet_foundation::CliSchema::entry_type_module(bundle)
                    .unwrap_or(bundle.entry);
                match cli_entry_param_shape(
                    &bundle.modules[cli_module].items,
                    &param.ty,
                    &states[cli_module].trait_reg,
                ) {
                    CliEntryShape::Struct | CliEntryShape::Enum => {}
                    CliEntryShape::EnumBadVariants(bad) => diags.extend(bad),
                    CliEntryShape::Invalid => diags.push(e1308(Some(param.ty_span))),
                }
            } else {
                diags.push(e1308(Some(run_fn.name_span)));
            }
        } else {
            diags.push(no_run_error());
        }
    }
    match mode {
        CompileMode::Test if entry.tests.is_empty() => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `@{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: @{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        // `jet bench` checks the AST for `@Bench` blocks before entering Bench
        // mode and falls back to whole-program timing otherwise, so an empty
        // bench set is never an error here.
        CompileMode::Bench
        | CompileMode::Test
        | CompileMode::Run
        | CompileMode::Check
        | CompileMode::Eval => {}
    }

    // D-EFF1: collect effect summaries across every module, then run the
    // whole-program fixpoint and enforce each `#(…)` bound once.
    // D-CTEFFECT1 Tier-1: accumulate embed inputs from all module checks.
    // Use a temporary to avoid simultaneous &mut borrows of `bundle`.
    let mut embed_inputs = std::mem::take(&mut bundle.comptime_inputs);
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    let mut reference_anchors = HashMap::new();
    let mut module_effect_summaries: Vec<(String, HashMap<String, EffectSummary>)> = Vec::new();
    // D-METHODMACRO1=A: top-level function names whose address was taken
    // anywhere in the bundle, accumulated across every module below; the
    // `@InlineAlways` address-taken pass (E0918) runs after the loop, once
    // this set is complete across the whole bundle.
    let mut global_addr_taken: HashSet<String> = HashSet::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let mut local_summaries = HashMap::new();
        diags.extend(check_module_bodies(
            module,
            idx,
            &states,
            mode,
            freestanding,
            allow_impure,
            &mut local_summaries,
            &mut embed_inputs,
            &mut global_addr_taken,
            &mut reference_anchors,
        ));
        seed_trait_dispatch_effects(&module.items, &mut local_summaries);
        apply_effect_via(&module.items, &mut local_summaries, &mut Vec::new());
        effect_summaries.extend(local_summaries.clone());
        module_effect_summaries.push((module.alias.clone(), local_summaries));
    }
    bundle.comptime_inputs = embed_inputs;
    // D-METHODMACRO1=A: E0918 (address-taken) needs every module's function
    // bodies checked first. Methods can't appear in `global_addr_taken`
    // (Jet's grammar has no way to read a method's bare name as a value), so
    // this only ever fires for top-level functions.
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Func(f) = item {
                if f.is_inline_always && global_addr_taken.contains(&f.name) {
                    diags.push(e0918_address_taken(
                        &f.name,
                        f.inline_span.unwrap_or(f.name_span),
                    ));
                }
            }
        }
    }
    // D-EFF2 (`#(via f)`): seed each via-fn's summary with its callback's bound
    // before the fixpoint, so its published effect set is a tight pass-through.
    for module in &bundle.modules {
        apply_effect_via(&module.items, &mut effect_summaries, &mut diags);
    }
    // File modules need qualified facts: bare top-level names overwrite one
    // another, while D-EFFECT-OMIT1 requires one cross-package solver answer.
    let (public_summaries, public_solved) = qualified_effect_facts(&module_effect_summaries);
    // `public_summaries` also carries unique short aliases for tooling. Run
    // diagnostics only over canonical module-qualified nodes so each source
    // obligation is reported once.
    let module_aliases = bundle
        .modules
        .iter()
        .map(|module| format!("{}::", module.alias))
        .collect::<Vec<_>>();
    let validation_summaries = public_summaries
        .iter()
        .filter(|(key, _)| module_aliases.iter().any(|prefix| key.starts_with(prefix)))
        .map(|(key, summary)| (key.clone(), summary.clone()))
        .collect::<HashMap<_, _>>();
    for module in &bundle.modules {
        let prefix = format!("{}::", module.alias);
        let local_solved = public_solved
            .iter()
            .filter_map(|(key, row)| key.strip_prefix(&prefix).map(|key| (key.to_string(), row.clone())))
            .collect::<HashMap<_, _>>();
        let local_summaries = validation_summaries
            .iter()
            .map(|(key, summary)| {
                if let Some(key) = key.strip_prefix(&prefix) {
                    let mut summary = summary.clone();
                    summary.edges = summary
                        .edges
                        .iter()
                        .map(|edge| edge.strip_prefix(&prefix).unwrap_or(edge).to_string())
                        .collect();
                    for call in &mut summary.memory.calls {
                        call.callee = call
                            .callee
                            .strip_prefix(&prefix)
                            .unwrap_or(&call.callee)
                            .to_string();
                    }
                    (key.to_string(), summary)
                } else {
                    (key.clone(), summary.clone())
                }
            })
            .collect::<HashMap<_, _>>();
        check_effect_boundaries(
            &module.items,
            &local_solved,
            &local_summaries,
            &mut diags,
        );
        super::Effects::check_inferred_purity(
            &module.items,
            &module.alias,
            &validation_summaries,
            &public_solved,
            &mut diags,
        );
        check_replayable_effects(&module.items, &local_solved, &mut diags);
        check_secret_grants(
            &module.items,
            &module.alias,
            &validation_summaries,
            &mut diags,
        );
    }
    check_region_caps(&validation_summaries, &public_solved, &mut diags);
    // D-EFF2: callback param effect bounds (E0747).
    check_callback_bounds(&validation_summaries, &public_solved, &mut diags);

    // D-WASM1=A (c123 M1): JS/WASM partition inference and boundary checks.
    // D-MEM-FACTS1: module `@Policy(no_alloc)` declarations are checked only
    // after the same qualified, dependency-complete graph reaches its fixpoint.
    // #657 feeds the other scope levels and the two remaining fact values into
    // this declaration surface; reachability itself stays single-mechanism.
    let (memory_summaries, memory_declarations) =
        super::MemoryFacts::bundle_memory_inputs(bundle, &public_summaries);
    let memory_projections = memory_declarations
        .iter()
        .flat_map(|declaration| {
            declaration.roots.iter().map(|root| {
                (
                    (root.clone(), declaration.fact),
                    super::MemoryFacts::project_memory_fact(
                        declaration.fact,
                        root,
                        &memory_summaries,
                    ),
                )
            })
        })
        .collect::<HashMap<_, _>>();
    diags.extend(super::MemoryFacts::check_memory_facts(
        &memory_declarations,
        &memory_summaries,
    ));
    diags.extend(check_web_partition(
        bundle,
        &public_summaries,
        &public_solved,
    ));

    // D-OSTARGET1=A (ratified 2026-07-01, c134): native OS platform gating —
    // mixed-axis conflicts and unmatched cross-gate calls.
    diags.extend(check_os_target(bundle));

    // D-TAINT1: taint tracking across every module. `@Sanitizer fn`s are
    // collected program-wide (a sanitizer in one module clears taint at a call in
    // another); each module's bodies are checked against its own Core aliases so
    // a sink call (Db/Exec/Net effect) resolves correctly. Erased in codegen (I3).
    let mut sanitizers: std::collections::HashSet<String> = std::collections::HashSet::new();
    for module in &bundle.modules {
        collect_sanitizers(&module.items, &mut sanitizers);
    }
    for (idx, module) in bundle.modules.iter().enumerate() {
        let core_imports = &states[idx].core_imports;
        for item in &module.items {
            taint_check_item(item, &sanitizers, core_imports, &mut diags);
        }
    }

    // D-STATE1 / D-STATE-DECL: typestate across the whole bundle. State-set
    // declarations are collected program-wide, then declarations validated (E0151,
    // L0151) and per-body forward dataflow checked (E0150). Erased in codegen (I3).
    let mut state_tbl = crate::Sema::StateTable::default();
    for module in &bundle.modules {
        state_tbl.add_items(&module.items);
    }
    if !state_tbl.is_empty() {
        for module in &bundle.modules {
            state_tbl.validate_declarations(&module.items, &mut diags);
            crate::Sema::check_items_state(&module.items, &state_tbl, &mut diags);
        }
    }

    let (mut used_core, usage_spans, ffi_callback_fns) = collect_used_core(bundle, &states);
    // D-CLIFLAG1: a `@[Cli]`-derived struct's generated `__jet_cli_spec_*`/
    // `__jet_cli_decode_*` functions (and the synthesized `fn main` for a
    // typed `fn run`) call straight into `core.args`'s `JetArgsSpec`/
    // `JetParsedArgs` prelude — but they're pure codegen text, not a Jet
    // method call `collect_used_core` can see by walking function bodies.
    // Force the same `CORELIB_PRELUDE` inclusion a hand-written
    // `use core.args` would trigger (any key works; the caller only checks
    // "is this set empty").
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if s.derives.iter().any(|(t, _)| t == "Cli")))
    }) {
        used_core.insert("core.args::spec".to_string());
    }
    // D-MEM1 S6: `Shared<T>`/`Pool<T>`/`Id<T>` need `CORELIB_PRELUDE`'s `jet_std`
    // module (`JetShared`/`JetPool`/`JetId`), but need no `use core.X` import to
    // reach them (unlike `tasks.spawn` etc.) — `collect_used_core` only walks
    // import aliases, so it never sees them. Same forced-insert shape as
    // D-CLIFLAG1 above; a cheap source-text scan is deliberately over-eager (a
    // false positive just includes the prelude when it wasn't strictly needed —
    // harmless, `#![allow(warnings)]` covers the unused code).
    if bundle.modules.iter().any(|m| {
        m.source.contains("Pool<")
            || m.source.contains("Shared<")
            || m.source.contains("Shared.new(")
            || m.source.contains("Id<")
    }) {
        used_core.insert("core.mem::pool_shared".to_string());
    }
    // D-VALIDATE1 (card #506): a `validate { … }` block synthesizes
    // `Type.validate(value)`, which returns `jet_std::FieldError` — same
    // forced-insert shape as D-CLIFLAG1/D-MEM1 S6 above, since declaring the
    // block needs no `use core.X` import to reach `CORELIB_PRELUDE`.
    if bundle.modules.iter().any(|m| {
        m.items
            .iter()
            .any(|i| matches!(i, Item::Struct(s) if !s.validate_block.is_empty()))
    }) {
        used_core.insert("core.validate::field_error".to_string());
    }
    // D-EMAIL-SMTP-CONFIG1=A: sema canonicalizes `email.Limits.safe()` to a
    // static `Limits.safe()` call before this late usage walk. Preserve CoreLib
    // reachability for type-only SMTP policy programs.
    if bundle.modules.iter().zip(states.iter()).any(|(module, state)| {
        module.source.contains(".Limits")
            && state.core_imports.values().any(|path| path == "core.email")
    }) {
        used_core.insert("core.email::Limits.safe".to_string());
    }
    bundle.used_core = used_core;
    bundle.ffi_callback_fns = ffi_callback_fns;
    diags.extend(super::MemoryFacts::annotate_scoped_gc_promotions(bundle));
    apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);
    (
        diags,
        super::Effects::SemIndexEffectFacts {
            summaries: public_summaries,
            solved: public_solved,
            memory_declarations,
            memory_projections,
            reference_anchors,
        },
    )
}

#[cfg(test)]
mod structure_tests {
    #[test]
    fn bundle_stays_split_without_reordering_passes() {
        const MAX_MODULE_LINES: usize = 2500;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |relative: &str| std::fs::read_to_string(root.join(relative)).unwrap();
        let bundle = read("src/Sema/Bundle.rs");
        let generic = read("src/Sema/Bundle/GenericModules.rs");
        let validation = read("src/Sema/Bundle/Validation.rs");
        let production = bundle
            .split("#[cfg(test)]\nmod structure_tests")
            .next()
            .unwrap();
        for (relative, source) in [
            ("src/Sema/Bundle.rs", production),
            ("src/Sema/Bundle/GenericModules.rs", generic.as_str()),
            ("src/Sema/Bundle/Validation.rs", validation.as_str()),
        ] {
            assert!(
                source.lines().count() < MAX_MODULE_LINES,
                "{relative} must stay below the card #510 module boundary"
            );
            assert!(!source.contains("include!("));
            assert!(!source.contains("#[path"));
        }
        assert!(production.contains("\nmod GenericModules;\nmod Validation;\n"));

        let ordered = [
            "expand_generic_module_aliases(bundle, &mut diags);",
            "mangle_inline_sibling_calls(bundle);",
            "super::Registration::expand_builtin_serde_items(&mut module.items, &mut diags);",
            "register_type_methods(&module.items, &mut st.registry, &mut diags);",
            "register_impl_methods(&module.items, &mut st.registry, &mut diags);",
            "diags.extend(check_module_bodies(",
            "collect_used_core(bundle, &states)",
            "apply_helper_layer_inference(bundle, &states, &usage_spans, &mut diags);",
        ];
        let positions: Vec<usize> = ordered
            .iter()
            .map(|needle| production.find(needle).unwrap())
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
