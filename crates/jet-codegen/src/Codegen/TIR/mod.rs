//! TIR — a small, *typed* intermediate representation for codegen (c109 Phase 1).
//!
//! ## Why this exists
//!
//! Today codegen (`emit_func` and friends) re-derives semantic facts while it
//! emits Rust: it calls `expr_jet_ty` to re-infer expression types and
//! `operand_is_integer` to re-decide which operator traps on overflow. That is
//! exactly the "codegen re-derives / falls back" smell that invariant I3 ("codegen
//! is dumb") forbids, and it is the bug class that produced the I2 holes the
//! checked-IR effort was built to kill.
//!
//! The TIR is the fix. It is a distinct, post-sema representation whose defining
//! property is **TOTALITY**: every fact codegen needs is carried *concretely* on
//! the node — never re-inferred, never an `Option` codegen has to fall back from.
//! Every `TExpr` carries its resolved `Type`; every `Binary` carries its overflow
//! decision as a plain `bool`; every `Let` carries the resolved binding type. The
//! emitter (`emit_tir_func`) makes ZERO decisions: it pattern-matches TIR fields
//! and formats Rust. It never calls `expr_jet_ty` or `operand_is_integer`.
//!
//! ## Phase 1 scope (deliberately tiny)
//!
//! This is the foundational slice. It covers only the *simplest* top-level
//! functions — scalar/String params, arithmetic/logic/comparison, bindings,
//! assignments, returns, `if`, calls to plain functions and `print`. The gate
//! `tir_covers` decides, conservatively, whether a function is fully inside that
//! subset; anything outside stays on the existing AST `emit_func` path, untouched.
//! The two paths must produce byte-identical Rust (golden parity, `tests/golden.rs`),
//! which is how we prove the rest of the compiler is undisturbed.
//!
//! Later phases widen `tir_covers` and add TIR nodes until the AST codegen path
//! is deleted. So the rule for this module is: **add a node only when its
//! construct is in the covered subset, and make every field total.**

// Re-export the parent `Codegen` glob so the split-out submodules
// (`subset`/`lower`/`emit`) reach `Cx`, `mangle`, `rust_*`, etc. via `use super::*`.
pub(crate) use super::*;

mod emit;
mod eval;
pub use eval::{
    install_comptime_bridge, lower_interp_program, run_named_func, run_program,
    run_program_with_structs, set_native_call_hook, NativeCallHook,
};
mod lower;
mod subset;

// Re-export every submodule item so existing `TIR::<name>` call sites and the
// `#[cfg(test)] mod tests` block (which uses `super::*`) keep resolving unchanged.
pub(crate) use emit::*;
pub(crate) use lower::*;
pub(crate) use subset::*;

use crate::AST::{AccessConvention, BinOp, Item, ProgramBundle, Type, UnOp, VariantPayload};

thread_local! {
    static LAST_JIT_LOWER_FAILURE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// c139 M4: lowered spawn-lambda body for Cranelift JIT (captures as explicit params).
pub struct TJitSpawnLambda {
    pub params: Vec<(String, Type)>,
    pub captures: Vec<JitSpawnCapture>,
    pub body: TJitSpawnBody,
    pub ret: Type,
}

pub struct JitSpawnCapture {
    pub name: String,
    pub ty: Type,
    pub clone_at_spawn: bool,
}

pub enum TJitSpawnBody {
    Expr(Box<TExpr>),
    Block {
        prefix: Vec<TStmt>,
        tail: Option<Box<TExpr>>,
    },
}

/// c139 M3: every lowered function the JIT may compile from the entry module.
pub struct JitProgram {
    /// Display path of the entry module (for overflow trap messages).
    pub source_file: String,
    /// Sema-selected callable name. The JIT compiles this exact function and
    /// never assumes the source spelling `run`.
    pub entry: String,
    /// #91: canonical generic-instance fingerprints consumed by JIT caches,
    /// diagnostics, and parity tooling.
    pub instance_provenance: Vec<InstanceProvenance>,
    /// All top-level `tir_covers` functions in the entry module, including `run`.
    pub funcs: Vec<TFunc>,
    /// c139 M4: spawn lambda bodies in program traversal order (parallel to spawn sites in TIR).
    pub spawn_lambdas: Vec<TJitSpawnLambda>,
    /// M5: mangled field names per struct type (field order).
    pub struct_fields: std::collections::HashMap<String, Vec<String>>,
    /// M5: field types parallel to `struct_fields` order.
    pub struct_field_types: std::collections::HashMap<String, Vec<Type>>,
    /// M5: mangled variant names per enum type (discriminant order).
    pub enum_variants: std::collections::HashMap<String, Vec<String>>,
    /// M5: payload field types per `user_Type::user_Variant` pattern prefix.
    pub enum_variant_payload_types: std::collections::HashMap<String, Vec<Type>>,
    pub int_constants: std::collections::HashMap<String, i64>,
    pub distinct_bases: std::collections::HashMap<String, Type>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceProvenance {
    pub canonical_module: String,
    pub fingerprint: String,
    pub full_key_hex: String,
}

pub fn instance_provenance(bundle: &ProgramBundle) -> Vec<InstanceProvenance> {
    bundle.modules.iter().flat_map(|module| module.items.iter().filter_map(|item| {
        let Item::CodeModule(instance) = item else { return None };
        let identity = instance.instance_identity.as_ref()?;
        Some(InstanceProvenance {
            canonical_module: instance.name.clone(),
            fingerprint: identity.fingerprint.clone(),
            full_key_hex: identity.full_key.iter().map(|byte| format!("{byte:02x}")).collect(),
        })
    })).collect()
}

fn payload_types_for_variant(payload: &VariantPayload) -> Vec<Type> {
    match payload {
        VariantPayload::Unit => Vec::new(),
        VariantPayload::Single(ty, _) => vec![ty.clone()],
        VariantPayload::Named(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
    }
}

fn register_enum_variants(
    enum_name: &str,
    variants: &[crate::AST::Variant],
    enum_variants: &mut std::collections::HashMap<String, Vec<String>>,
    enum_variant_payload_types: &mut std::collections::HashMap<String, Vec<Type>>,
) {
    enum_variants.insert(
        enum_name.to_string(),
        variants
            .iter()
            .map(|variant| format!("user_{}", variant.name))
            .collect(),
    );
    for variant in variants {
        let pattern = format!("user_{enum_name}::user_{}", variant.name);
        enum_variant_payload_types.insert(pattern, payload_types_for_variant(&variant.payload));
    }
}

fn register_union_type(
    ty: &Type,
    enum_variants: &mut std::collections::HashMap<String, Vec<String>>,
    enum_variant_payload_types: &mut std::collections::HashMap<String, Vec<Type>>,
) {
    match ty {
        Type::Union(members) => {
            let name = crate::AST::union_enum_name(members);
            enum_variants.entry(name.clone()).or_insert_with(|| {
                members
                    .iter()
                    .map(crate::AST::union_member_tag)
                    .collect()
            });
            for member in members {
                let tag = crate::AST::union_member_tag(member);
                enum_variant_payload_types
                    .entry(format!("{name}::{tag}"))
                    .or_insert_with(|| vec![member.clone()]);
                register_union_type(member, enum_variants, enum_variant_payload_types);
            }
        }
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => {
            register_union_type(inner, enum_variants, enum_variant_payload_types)
        }
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            register_union_type(key, enum_variants, enum_variant_payload_types);
            register_union_type(value, enum_variants, enum_variant_payload_types);
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                register_union_type(param, enum_variants, enum_variant_payload_types);
            }
            if let Some(ret) = ret {
                register_union_type(ret, enum_variants, enum_variant_payload_types);
            }
        }
        Type::Apply { args, .. } => {
            for arg in args {
                register_union_type(arg, enum_variants, enum_variant_payload_types);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                register_union_type(field, enum_variants, enum_variant_payload_types);
            }
        }
        _ => {}
    }
}

/// c139 M3: every lowered function the JIT may compile from the entry module.
///
/// Returns `None` when there is no plain top-level `run`, or when `run` is
/// outside the existing `tir_covers` gate.
pub fn lower_entry_main_for_jit(bundle: &ProgramBundle) -> Option<TFunc> {
    lower_jit_program(bundle).map(|p| {
        let entry = p.entry;
        p.funcs
            .into_iter()
            .find(|f| f.name == entry)
            .expect("lower_jit_program always includes its selected entry")
    })
}

/// Rust local place for JIT variable lookup (`user_x`).
pub fn local_place(name: &str) -> String {
    super::mangle(name)
}

/// One local or parameter slot, carried as structure instead of a Rust place
/// string. Every engine resolves a slot from these three facts alone.
///
/// `name` is the slot's identity: a user binding carries its Jet name, which Rust
/// spells `user_<name>`; a compiler-generated temp (`generated`) carries its own
/// reserved identifier, which can never collide with a mangled user name.
/// `deref` records a by-reference slot, which Rust reads through `(*…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TLocal {
    pub name: String,
    pub generated: bool,
    pub deref: bool,
}

impl TLocal {
    /// A user binding, read by value.
    pub fn user(name: impl Into<String>) -> TLocal {
        TLocal {
            name: name.into(),
            generated: false,
            deref: false,
        }
    }

    /// A compiler-generated temp slot, read by value.
    pub fn generated(name: impl Into<String>) -> TLocal {
        TLocal {
            name: name.into(),
            generated: true,
            deref: false,
        }
    }

    /// The same slot read through a by-reference deref.
    pub fn through_ref(mut self) -> TLocal {
        self.deref = true;
        self
    }

    /// The Rust binding identifier for this slot, without the deref wrapper.
    pub fn rust_name(&self) -> String {
        if self.generated {
            self.name.clone()
        } else {
            local_place(&self.name)
        }
    }

    /// The Rust place this slot reads and writes.
    pub fn rust_place(&self) -> String {
        let rust = self.rust_name();
        if self.deref {
            format!("(*{rust})")
        } else {
            rust
        }
    }
}

/// A resolved user method identity. `name` is the Jet method name — the key the
/// JIT and interpreter dispatch on. `mangled` records the one Rust spelling fact:
/// an inherent method becomes `user_<name>`, while a trait-impl or dynamic-dispatch
/// method keeps the bare name the trait owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TMethodRef {
    pub name: String,
    pub mangled: bool,
}

impl TMethodRef {
    /// An inherent user method — Rust spells it `user_<name>`.
    pub fn inherent(name: impl Into<String>) -> TMethodRef {
        TMethodRef {
            name: name.into(),
            mangled: true,
        }
    }

    /// A trait-owned method — Rust spells it bare (the trait declared the name).
    pub fn bare(name: impl Into<String>) -> TMethodRef {
        TMethodRef {
            name: name.into(),
            mangled: false,
        }
    }

    /// The Rust method name.
    pub fn rust(&self) -> String {
        if self.mangled {
            super::mangle(&self.name)
        } else {
            self.name.clone()
        }
    }
}

/// One generic argument of a prelude container type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TPreludeArg {
    /// A resolved Jet type; the emitter spells it via `cx.rust_type`.
    Jet(Type),
    /// A host counter with no Jet spelling (the multiset's `usize` tally).
    HostUsize,
}

/// The owner of a static (associated) call.
pub enum TStaticOwner {
    /// A user type the front end compiles. Both the Rust spelling and the JIT's
    /// compiled-function key derive from this Jet type name.
    User(String),
    /// A prelude/host type the front end never compiles. `path` is a resolved
    /// symbol path, not composed source: `rooted` prefixes the generated crate
    /// root, and `generics` are resolved arguments the emitter spells.
    Prelude {
        rooted: bool,
        path: String,
        generics: Vec<TPreludeArg>,
    },
}

/// An assignable place. Every engine reads the structure directly: a local slot
/// by name, or the already-structured place expression a field/index/pool write
/// targets. Rust spelling happens only in the emit layer.
pub enum TPlace {
    Local(TLocal),
    /// A structured place expression — a field-read chain, a swizzle lane, a
    /// `Pool` slot. Its own node carries the facts; nothing is pre-rendered.
    Expr(Box<TExpr>),
}

impl TPlace {
    /// The local slot this place is rooted in, when it is a plain local.
    pub fn as_local(&self) -> Option<&TLocal> {
        match self {
            TPlace::Local(local) => Some(local),
            TPlace::Expr(_) => None,
        }
    }
}

fn demand_serde_codec(
    demands: &mut std::collections::BTreeMap<String, (Type, String)>,
    ty: &Type,
    method: &str,
) {
    if matches!(ty, Type::Apply { .. }) {
        demands.insert(
            format!("{}::{method}", ty.name()),
            (ty.clone(), method.to_string()),
        );
    }
}

/// Seed monomorphize demand for generic Codable from encoding core calls and
/// SerdeEncode/DataTreeDecode ops already present in lowered bodies.
fn collect_serde_codec_demands(
    funcs: &[TFunc],
    demands: &mut std::collections::BTreeMap<String, (Type, String)>,
) {
    fn walk_expr(expr: &TExpr, demands: &mut std::collections::BTreeMap<String, (Type, String)>) {
        match &expr.kind {
            TExprKind::Print(inner) | TExprKind::DistinctCtor { arg: inner, .. } => {
                walk_expr(inner, demands);
            }
            TExprKind::Call { args, .. } => {
                for a in args {
                    walk_expr(&a.value, demands);
                }
            }
            TExprKind::HandleMethod { recv, op, args } => {
                walk_expr(recv, demands);
                for a in args {
                    walk_expr(a, demands);
                }
                match op {
                    THandleOp::SerdeEncode => demand_serde_codec(demands, &recv.ty, "encode"),
                    THandleOp::DataTreeDecode(target) => {
                        demand_serde_codec(demands, target, "decode")
                    }
                    _ => {}
                }
            }
            TExprKind::CoreCall {
                module,
                method,
                args,
                ..
            } => {
                for a in args {
                    walk_expr(a, demands);
                }
                let encoding = matches!(
                    module.as_str(),
                    "core.encoding.json"
                        | "core.encoding.toml"
                        | "core.encoding.yaml"
                        | "core.encoding.csv"
                );
                if encoding && matches!(method.as_str(), "to_string" | "to_string_pretty") {
                    if let Some(arg) = args.first() {
                        demand_serde_codec(demands, &arg.ty, "encode");
                    }
                }
                if encoding && method == "decode" {
                    if let Type::Result { ok, .. } = &expr.ty {
                        demand_serde_codec(demands, ok, "decode");
                    }
                }
            }
            _ => {}
        }
    }
    fn walk_stmt(stmt: &TStmt, demands: &mut std::collections::BTreeMap<String, (Type, String)>) {
        match stmt {
            TStmt::ExprStmt(e) | TStmt::Return(Some(e)) => walk_expr(e, demands),
            TStmt::Let { init, .. } | TStmt::Assign { value: init, .. } => {
                walk_expr(init, demands)
            }
            _ => {}
        }
    }
    for func in funcs {
        for stmt in &func.body {
            walk_stmt(stmt, demands);
        }
    }
}

fn lower_demanded_generic_methods(items: &[Item], cx: &Cx, funcs: &mut Vec<TFunc>) -> Option<()> {
    let mut pending = std::mem::take(&mut *cx.jit_method_calls.borrow_mut());
    collect_serde_codec_demands(funcs, &mut pending);
    let mut processed = std::collections::BTreeSet::new();
    while let Some((key, (owner_ty, method_name))) = pending.pop_first() {
        if !processed.insert(key) {
            continue;
        }
        if let Some(chain) = crate::Generics::generic_depth_exceeded(&owner_ty) {
            LAST_JIT_LOWER_FAILURE.with(|failure| {
                *failure.borrow_mut() = Some(format!(
                    "E0909: generic instantiation goes too deep; simplify the types involved: {chain}"
                ));
            });
            return None;
        }
        let Type::Apply { name, args } = &owner_ty else {
            continue;
        };
        let Some(params) = items.iter().find_map(|item| match item {
            Item::Struct(def) if def.name == *name => Some(def.type_params.as_slice()),
            Item::Enum(def) if def.name == *name => Some(def.type_params.as_slice()),
            _ => None,
        }) else {
            continue;
        };
        let subst = params
            .iter()
            .zip(args)
            .map(|(param, arg)| (param.name.clone(), arg.clone()))
            .collect();
        for item in items {
            let (method, trait_name) = match item {
                Item::Struct(def) if def.name == *name => {
                    match def.methods.iter().find(|method| method.name == method_name) {
                        Some(method) => (method, None),
                        None => continue,
                    }
                }
                Item::Impl(imp) if imp.type_name == *name => {
                    let Some(method) = imp.methods.iter().find(|method| method.name == method_name)
                    else {
                        continue;
                    };
                    match &imp.trait_name {
                        None => (method, None),
                        Some(t)
                            if t == crate::Generics::ENCODE || t == crate::Generics::DECODE =>
                        {
                            (method, Some(t.as_str()))
                        }
                        Some(_) => continue,
                    }
                }
                _ => continue,
            };
            let mut specialized = crate::Sema::specialize_function_types(method.clone(), &subst);
            // Subst already rewrote the binder; drop residual type params so the
            // mono body is admitted as a concrete JIT function.
            specialized.type_params.clear();
            let mut lowered = if let Some(trait_name) = trait_name {
                if !tir_covers_trait_method(&specialized, name, cx, trait_name) {
                    continue;
                }
                // Bind `self` as `Wrap<Int>` so field reads substitute `T` → arg.
                // Encode is an ordinary instance method; Decode stays on the static
                // trait-method ABI (`tree` only, no receiver).
                if trait_name == crate::Generics::ENCODE
                    && matches!(&owner_ty, Type::Apply { .. })
                {
                    lower_method_for_owner(&specialized, name, owner_ty.clone(), cx)
                } else {
                    lower_trait_method(&specialized, name, cx, trait_name)
                }
            } else {
                if !tir_covers_method(&specialized, name, cx) {
                    continue;
                }
                lower_method_for_owner(&specialized, name, owner_ty.clone(), cx)
            };
            lowered.name = format!("{}::{}", owner_ty.name(), method.name);
            // Nested SerdeEncode/DataTreeDecode inside this body may demand more.
            collect_serde_codec_demands(std::slice::from_ref(&lowered), &mut pending);
            funcs.push(lowered);
        }
        for (key, call) in std::mem::take(&mut *cx.jit_method_calls.borrow_mut()) {
            if !processed.contains(&key) {
                pending.entry(key).or_insert(call);
            }
        }
    }
    Some(())
}

pub(crate) fn bind_generic_type(
    template: &Type,
    actual: &Type,
    params: &std::collections::HashSet<String>,
    subst: &mut std::collections::HashMap<String, Type>,
) -> bool {
    match template {
        Type::Named(name) if params.contains(name) => match subst.get(name) {
            Some(bound) => bound == actual,
            None => {
                subst.insert(name.clone(), actual.clone());
                true
            }
        },
        Type::Apply { name, args } => {
            let Type::Apply {
                name: actual_name,
                args: actual_args,
            } = actual
            else {
                return false;
            };
            name == actual_name
                && args.len() == actual_args.len()
                && args
                    .iter()
                    .zip(actual_args)
                    .all(|(left, right)| bind_generic_type(left, right, params, subst))
        }
        Type::List(inner) => matches!(actual, Type::List(actual_inner)
            if bind_generic_type(inner, actual_inner, params, subst)),
        Type::Option(inner) => matches!(actual, Type::Option(actual_inner)
            if bind_generic_type(inner, actual_inner, params, subst)),
        Type::Result { ok, err } => matches!(actual, Type::Result { ok: actual_ok, err: actual_err }
            if bind_generic_type(ok, actual_ok, params, subst)
                && bind_generic_type(err, actual_err, params, subst)),
        Type::Tagged { inner, .. } => bind_generic_type(inner, actual, params, subst),
        _ => template == actual,
    }
}

fn specialize_generic_free_functions(items: &[Item], cx: &Cx, funcs: &mut Vec<TFunc>) {
    let calls = std::mem::take(&mut *cx.jit_generic_calls.borrow_mut());
    for (called_name, shapes) in calls {
        if funcs.iter().any(|func| func.name == called_name) {
            continue;
        }
        let mut unique = shapes;
        unique.sort_by_key(|shape| format!("{shape:?}"));
        unique.dedup();
        // One native symbol has one ABI. Multiple concrete shapes keep the
        // program outside resident JIT until call-site symbol mangling lands.
        let [actuals] = unique.as_slice() else {
            continue;
        };
        let (template, emitted_name) = if let Some((base, arity)) = called_name
            .rsplit_once("__va")
            .and_then(|(base, arity)| arity.parse::<usize>().ok().map(|arity| (base, arity)))
        {
            let Some((_, bounds)) = cx.variadic_bound_fns.get(base) else {
                continue;
            };
            let Some(source) = items.iter().find_map(|item| match item {
                Item::Func(func) if func.name == base => Some(func),
                _ => None,
            }) else {
                continue;
            };
            (
                crate::Codegen::VariadicBound::build_variadic_bound_func(
                    source, bounds, arity,
                ),
                called_name.clone(),
            )
        } else {
            let Some(source) = items.iter().find_map(|item| match item {
                Item::Func(func) if func.name == called_name => Some(func.clone()),
                _ => None,
            }) else {
                continue;
            };
            (source, called_name.clone())
        };
        if template.params.len() != actuals.len() || template.type_params.is_empty() {
            continue;
        }
        let names: std::collections::HashSet<String> =
            template.type_params.iter().map(|param| param.name.clone()).collect();
        let mut subst = std::collections::HashMap::new();
        if !template
            .params
            .iter()
            .zip(actuals)
            .all(|(param, actual)| bind_generic_type(&param.ty, actual, &names, &mut subst))
            || subst.len() != names.len()
        {
            continue;
        }
        let mut specialized = crate::Sema::specialize_function_types(template, &subst);
        specialized.type_params.clear();
        if !tir_covers(&specialized, cx) {
            continue;
        }
        let mut lowered = lower_func(&specialized, cx);
        lowered.name = emitted_name;
        funcs.push(lowered);
    }
}

/// c139 M3: lower every `tir_covers` top-level function in the entry module so the
/// JIT can compile multi-function programs (calls between covered helpers).
pub fn lower_jit_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    jet_foundation::PackageEdition::with_package_edition(&bundle.edition, || {
    LAST_JIT_LOWER_FAILURE.with(|failure| *failure.borrow_mut() = None);
    let module = bundle.modules.get(bundle.entry)?;
    let extern_funcs = bundle_extern_funcs(bundle);
    let mut cx = build_cx_items(
        &module.items,
        &module.source,
        &module.display,
        None,
        &extern_funcs,
    );
    populate_cx_from_bundle(&mut cx, bundle, bundle.entry);
    let type_shapes = collect_type_shapes(&module.items);
    let mut funcs = Vec::new();
    let entry_name = module
        .items
        .iter()
        .find_map(|item| {
                let Item::Const(value) = item else { return None };
                value.resolved_output.as_ref().and_then(|output| {
                    (output.selected
                        && output.module == bundle.entry
                        && output.params.is_empty())
                    .then(|| output.semantic_name.clone())
                })
            })
        .or_else(|| module.items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == "run" && function.params.is_empty() => {
                Some("run".to_string())
            }
            _ => None,
        }))?;
    cx.jit_spawn_lambdas.borrow_mut().clear();
    cx.jit_method_calls.borrow_mut().clear();
    cx.jit_generic_calls.borrow_mut().clear();
    for item in &module.items {
        match item {
            Item::Func(f) => {
                if !f.type_params.is_empty() || !tir_covers(f, &cx) {
                    continue;
                }
                let lowered = lower_func(f, &cx);
                funcs.push(lowered);
            }
            Item::Struct(s) => {
                if s.type_params.is_empty() {
                    for m in &s.methods {
                        if !tir_covers_method(m, &s.name, &cx) {
                            continue;
                        }
                        let mut lowered = lower_method(m, &s.name, &cx);
                        lowered.name = format!("{}::{}", s.name, m.name);
                        funcs.push(lowered);
                    }
                    for implementation in &s.trait_impls {
                        if matches!(
                            implementation.trait_name.as_str(),
                            crate::Generics::ENCODE | crate::Generics::DECODE
                        ) {
                            continue;
                        }
                        for method in &implementation.methods {
                            if !tir_covers_trait_method(
                                method,
                                &s.name,
                                &cx,
                                &implementation.trait_name,
                            ) {
                                continue;
                            }
                            let mut lowered = lower_trait_method(
                                method,
                                &s.name,
                                &cx,
                                &implementation.trait_name,
                            );
                            lowered.name = format!("{}::{}", s.name, method.name);
                            funcs.push(lowered);
                        }
                    }
                }
            }
            Item::Enum(e) => {
                if e.type_params.is_empty() {
                    for method in &e.methods {
                        if !tir_covers_method(method, &e.name, &cx) {
                            continue;
                        }
                        let mut lowered = lower_method(method, &e.name, &cx);
                        lowered.name = format!("{}::{}", e.name, method.name);
                        funcs.push(lowered);
                    }
                    for implementation in &e.trait_impls {
                        if matches!(
                            implementation.trait_name.as_str(),
                            crate::Generics::ENCODE | crate::Generics::DECODE
                        ) {
                            continue;
                        }
                        for method in &implementation.methods {
                            if !tir_covers_trait_method(
                                method,
                                &e.name,
                                &cx,
                                &implementation.trait_name,
                            ) {
                                continue;
                            }
                            let mut lowered = lower_trait_method(
                                method,
                                &e.name,
                                &cx,
                                &implementation.trait_name,
                            );
                            lowered.name = format!("{}::{}", e.name, method.name);
                            funcs.push(lowered);
                        }
                    }
                }
            }
            Item::Impl(imp) => {
                let owner_params = module
                    .items
                    .iter()
                    .find_map(|item| match item {
                        Item::Struct(s) if s.name == imp.type_name => {
                            Some(s.type_params.as_slice())
                        }
                        Item::Enum(e) if e.name == imp.type_name => Some(e.type_params.as_slice()),
                        _ => None,
                    })
                    .unwrap_or(&[]);
                let owners = if imp.trait_name.is_none() && !owner_params.is_empty() {
                    Vec::new()
                } else {
                    vec![Type::Named(imp.type_name.clone())]
                };
                for owner_ty in owners {
                    let subst = match &owner_ty {
                        Type::Apply { args, .. } => owner_params
                            .iter()
                            .zip(args)
                            .map(|(param, arg)| (param.name.clone(), arg.clone()))
                            .collect(),
                        _ => std::collections::HashMap::new(),
                    };
                    for method in &imp.methods {
                        let specialized = if subst.is_empty() {
                            method.clone()
                        } else {
                            crate::Sema::specialize_function_types(method.clone(), &subst)
                        };
                        if !specialized.type_params.is_empty() {
                            continue;
                        }
                        let mut lowered = if let Some(trait_name) = &imp.trait_name {
                            if !tir_covers_trait_method(
                                &specialized,
                                &imp.type_name,
                                &cx,
                                trait_name,
                            ) {
                                continue;
                            }
                            lower_trait_method(&specialized, &imp.type_name, &cx, trait_name)
                        } else {
                            if !tir_covers_method(&specialized, &imp.type_name, &cx) {
                                continue;
                            }
                            lower_method_for_owner(
                                &specialized,
                                &imp.type_name,
                                owner_ty.clone(),
                                &cx,
                            )
                        };
                        lowered.name = format!("{}::{}", owner_ty.name(), method.name);
                        funcs.push(lowered);
                    }
                }
            }
            Item::CodeModule(cm) => {
                let Some(body) = &cm.body else { continue };
                for inner in body {
                    match inner {
                        Item::Func(f) => {
                            if !f.type_params.is_empty() || !tir_covers(f, &cx) {
                                continue;
                            }
                            let mut lowered = lower_func(f, &cx);
                            lowered.name = format!("{}__{}", cm.name, f.name);
                            funcs.push(lowered);
                        }
                        Item::Struct(s) => {
                            let type_name = if s.name.starts_with(&format!("{}__", cm.name)) {
                                s.name.clone()
                            } else {
                                format!("{}__{}", cm.name, s.name)
                            };
                            for method in &s.methods {
                                if !tir_covers_method(method, &type_name, &cx) {
                                    continue;
                                }
                                let mut lowered = lower_method(method, &type_name, &cx);
                                lowered.name = format!("{}::{}", type_name, method.name);
                                funcs.push(lowered);
                            }
                        }
                        Item::Impl(imp) => {
                            let type_name = if imp
                                .type_name
                                .starts_with(&format!("{}__", cm.name))
                            {
                                imp.type_name.clone()
                            } else {
                                format!("{}__{}", cm.name, imp.type_name)
                            };
                            for method in &imp.methods {
                                let mut lowered = if let Some(trait_name) = &imp.trait_name {
                                    if !tir_covers_trait_method(method, &type_name, &cx, trait_name)
                                    {
                                        continue;
                                    }
                                    lower_trait_method(method, &type_name, &cx, trait_name)
                                } else {
                                    if !tir_covers_method(method, &type_name, &cx) {
                                        continue;
                                    }
                                    lower_method(method, &type_name, &cx)
                                };
                                lowered.name = format!("{}::{}", type_name, method.name);
                                funcs.push(lowered);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    lower_demanded_generic_methods(&module.items, &cx, &mut funcs)?;
    specialize_generic_free_functions(&module.items, &cx, &mut funcs);
    if !funcs.iter().any(|function| function.name == entry_name) {
        return None;
    }
    let spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
    let mut struct_fields = std::collections::HashMap::new();
    let mut struct_field_types = std::collections::HashMap::new();
    let mut enum_variants = std::collections::HashMap::new();
    let mut enum_variant_payload_types = std::collections::HashMap::new();
    enum_variants.insert(
        crate::Syntax::TYPE_ORDERING.to_string(),
        ["user_Less", "user_Equal", "user_Greater"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    );
    let mut int_constants = std::collections::HashMap::new();
    for item in &module.items {
        match item {
            Item::Struct(s) => {
                struct_fields.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .map(|f| format!("user_{}", f.name))
                        .collect(),
                );
                struct_field_types.insert(
                    s.name.clone(),
                    s.fields.iter().map(|f| f.ty.clone()).collect(),
                );
                for field in &s.fields {
                    register_union_type(
                        &field.ty,
                        &mut enum_variants,
                        &mut enum_variant_payload_types,
                    );
                }
            }
            Item::Enum(e) if e.type_params.is_empty() => {
                register_enum_variants(
                    &e.name,
                    &e.variants,
                    &mut enum_variants,
                    &mut enum_variant_payload_types,
                );
            }
            Item::Func(function) => {
                for param in &function.params {
                    register_union_type(
                        &param.ty,
                        &mut enum_variants,
                        &mut enum_variant_payload_types,
                    );
                }
                if let Some(ret) = &function.return_type {
                    register_union_type(
                        ret,
                        &mut enum_variants,
                        &mut enum_variant_payload_types,
                    );
                }
            }
            Item::Const(c) => {
                let value = match &c.ct {
                    Some(crate::AST::CtValue::Int(value)) => Some(*value),
                    _ => match &c.value { crate::AST::Expr::Int(value, _, _, _) => Some(*value), _ => None },
                };
                if let Some(value) = value {
                    int_constants.insert(c.name.clone(), value);
                }
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    for inner in body {
                        match inner {
                            Item::Struct(s) if s.type_params.is_empty() => {
                                let name = if s.name.starts_with(&format!("{}__", cm.name)) {
                                    s.name.clone()
                                } else {
                                    format!("{}__{}", cm.name, s.name)
                                };
                                struct_fields.insert(
                                    name.clone(),
                                    s.fields
                                        .iter()
                                        .map(|f| format!("user_{}", f.name))
                                        .collect(),
                                );
                                struct_field_types.insert(
                                    name,
                                    s.fields.iter().map(|f| f.ty.clone()).collect(),
                                );
                            }
                            Item::Enum(e) if e.type_params.is_empty() => {
                                let name = if e.name.starts_with(&format!("{}__", cm.name)) {
                                    e.name.clone()
                                } else {
                                    format!("{}__{}", cm.name, e.name)
                                };
                                register_enum_variants(
                                    &name,
                                    &e.variants,
                                    &mut enum_variants,
                                    &mut enum_variant_payload_types,
                                );
                            }
                            Item::Const(c) => {
                                let value = match &c.ct {
                                    Some(crate::AST::CtValue::Int(value)) => Some(*value),
                                    _ => match &c.value {
                                        crate::AST::Expr::Int(value, _, _, _) => Some(*value),
                                        _ => None,
                                    },
                                };
                                if let Some(value) = value {
                                    int_constants.insert(format!("{}__{}", cm.name, c.name), value);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for (_, fields) in type_shapes.tuples {
        let tuple_ty = Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), Box::new(ty.clone())))
                .collect(),
        );
        struct_fields.insert(
            tuple_ty.name(),
            fields
                .iter()
                .map(|(name, _)| format!("user_{}", name))
                .collect(),
        );
        struct_field_types.insert(
            tuple_ty.name(),
            fields.iter().map(|(_, ty)| ty.clone()).collect(),
        );
    }
    let mut distinct_bases = std::collections::HashMap::new();
    for item in &module.items {
        match item {
            Item::Distinct(def) => {
                distinct_bases.insert(def.name.clone(), def.base.clone());
            }
            Item::UnitFamily(family) => {
                for def in family.distinct_defs() {
                    distinct_bases.insert(def.name, def.base);
                }
            }
            _ => {}
        }
    }
    Some(JitProgram {
        instance_provenance: instance_provenance(bundle),
        source_file: module.display.clone(),
        entry: entry_name,
        funcs,
        spawn_lambdas,
        struct_fields,
        struct_field_types,
        enum_variants,
        enum_variant_payload_types,
        int_constants,
        distinct_bases,
    })
    })
}

/// Test hook: why `lower_jit_program` returned `None`.
#[doc(hidden)]
pub fn lower_jit_program_fail_reason(bundle: &ProgramBundle) -> String {
    if let Some(reason) = LAST_JIT_LOWER_FAILURE.with(|failure| failure.borrow_mut().take()) {
        return reason;
    }
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return "missing entry module".to_string();
    };
    let extern_funcs = bundle_extern_funcs(bundle);
    let mut cx = build_cx_items(
        &module.items,
        &module.source,
        &module.display,
        None,
        &extern_funcs,
    );
    populate_cx_from_bundle(&mut cx, bundle, bundle.entry);
    let selected = module.items.iter().find_map(|item| match item {
            Item::Const(value) => value.resolved_output.as_ref().and_then(|output| {
                (output.selected
                    && output.module == bundle.entry
                    && output.params.is_empty())
                .then(|| output.semantic_name.clone())
            }),
            _ => None,
        }).or_else(|| module.items.iter().find_map(|item| match item {
            Item::Func(function) if function.name == "run" && function.params.is_empty() => {
                Some("run".to_string())
            }
            _ => None,
        }));
    let Some(selected) = selected else {
        return "no zero-parameter runnable entry".to_string();
    };
    let mut saw_entry = false;
    let mut entry_tir = false;
    for item in &module.items {
        let Item::Func(f) = item else {
            continue;
        };
        if f.name == selected {
            saw_entry = true;
            entry_tir = tir_covers(f, &cx);
        }
    }
    if !saw_entry {
        return "selected entry is not a top-level function".to_string();
    }
    if !entry_tir {
        let mut locals: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in &module.items {
            let Item::Func(f) = item else {
                continue;
            };
            if f.name != selected {
                continue;
            }
            for (i, stmt) in f.body.iter().enumerate() {
                let mut probe = locals.clone();
                if !subset::stmt_in_subset(stmt, &cx, &mut probe) {
                    if let crate::AST::Stmt::Val(b) = stmt {
                        if !subset::expr_in_subset(&b.init, &cx, &locals) {
                            return format!("entry stmt {i} init outside tir_covers");
                        }
                    }
                    return format!("entry stmt {i} outside tir_covers");
                }
                let _ = subset::stmt_in_subset(stmt, &cx, &mut locals);
            }
        }
        return "entry outside tir_covers".to_string();
    }
    "unknown".to_string()
}

// ---------------------------------------------------------------------------
// TIR types. Every node carries the facts codegen needs, pre-resolved (totality).
// ---------------------------------------------------------------------------

/// A lowered top-level function. `params` are already mangled to their Rust
/// names and carry their resolved Jet `Type`; `ret` is the resolved return type.
pub struct TFunc {
    /// Jet function name (unmangled) — the emitter mangles via `cx.mangle_name`.
    pub name: String,
    /// `(mangled rust name, resolved jet type, convention)` per parameter. The
    /// convention is kept so the emitter reproduces the `&`/by-value Rust form
    /// without re-deciding (it mirrors `rust_param_type`).
    pub params: Vec<(String, Type, AccessConvention)>,
    /// Web-export boundary facts. A Codable struct parameter stays a typed value
    /// in the executable TIR body, while the external Wasm wrapper receives its
    /// scalar fields. Lowering resolves every Rust name/type here; Web emission
    /// only formats the wrapper and never re-discovers struct semantics.
    pub web_param_reconstructions: Vec<TWebParamReconstruction>,
    /// Resolved return type, or `None` for a unit-returning function.
    pub ret: Option<Type>,
    /// Sema-proven hidden automatic-root return representation.
    pub gc_return: bool,
    /// Sema-proved owner source for a returned `View`/`ViewMut`. Codegen reads
    /// this fact mechanically when spelling hidden Rust lifetimes.
    pub return_view_provenance: Option<crate::AST::ViewProvenanceMap>,
    /// c109 Phase 17: the rendered Rust generic clause (`<T: Clone>` / `<T, U>` / empty),
    /// resolved at lowering via `Generics::rust_type_param_list(&f.type_params, …)` exactly
    /// as `emit_func` does, including only bounds required by lowered operations.
    /// Emitted verbatim after the function name; empty for a non-generic function.
    pub generics: String,
    /// Types of operands materialized with `.clone()` while lowering this body.
    /// Generic inherent impl emission unions these facts to derive minimal bounds.
    pub clone_types: Vec<Type>,
    pub is_main: bool,
    /// D-COV1: the 1-based Jet source line of this function's name, for the
    /// `jet_cov(line)` coverage probe. Only read in coverage mode.
    pub line: usize,
    /// c109 Phase 18: an `#Unsafe fn` (S58, E2-M13/D-LL1) lowers to a Rust `unsafe fn`
    /// (the `unsafe ` keyword prefixes the signature), so the body may use gated pointer
    /// ops directly — calling it is already gated to an `#Unsafe` block in sema (E3103).
    /// I1: this is true ONLY when the source function was `#Unsafe fn`; no `unsafe` is
    /// ever emitted without that source gate. Applies to `TopLevel`/`Method`; a trait
    /// method carries its own `is_unsafe` on `TFuncKind::TraitMethod`.
    pub is_unsafe: bool,
    /// D-CABI-CALLBACK1: named pure, monomorphic top-level functions expose a
    /// stable C-convention symbol; sema alone decides whether it may cross C.
    pub is_pure: bool,
    /// D-REACTCORE1: `#Reactive fn` — the body is emitted inside `jet_reactive_effect`.
    pub is_reactive: bool,
    /// D-DATARACE1=C: upgrade-report lines for reactive boxes that crossed a boundary.
    pub reactive_upgrades: Vec<String>,
    /// D-METHODMACRO1=A: `#Inline fn` — emits `#[inline]`. Soft hint; sema never
    /// rejects it.
    pub is_inline: bool,
    /// D-METHODMACRO1=A: `#Inline(Always) fn` — emits `#[inline(always)]`. Only ever
    /// `true` here once sema has confirmed the function can actually inline
    /// (E0917/E0918/E0919 would have failed the build otherwise) — I3: sema
    /// decides, codegen just emits.
    pub is_inline_always: bool,
    pub body: Vec<TStmt>,
    /// c109 Phase 7: how this function is emitted. A top-level function gets
    /// `pub fn name(…)` at module scope; a method gets `pub fn user_name(<self>, …)`
    /// inside an `impl` block (indented), with the `self` receiver form per the
    /// resolved convention (or no receiver for a static method).
    pub kind: TFuncKind,
}

/// One typed parameter reconstructed by a flattened WebAssembly export wrapper.
pub struct TWebParamReconstruction {
    /// Param slot the reconstructed value binds into (matches `TFunc.params`).
    pub local: TLocal,
    /// Struct type being rebuilt from flattened ABI scalars; emit spells Rust.
    pub ty: Type,
    /// `(mangled field, flattened ABI parameter, resolved scalar type)`.
    pub fields: Vec<(String, String, Type)>,
}

/// D-SERDE2 (card #131 S1-bridge): which built-in codec trait a hand impl method
/// bridges to. `Encode` → `jet_encode(&self) -> jet_std::DataTree`; `Decode` →
/// the static `jet_decode(tree: &jet_std::DataTree) -> Result<Self, DecodeError>`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SerdeCodec {
    Encode,
    Decode,
}

/// c109 Phase 7: the emission shape of a lowered function.
pub enum TFuncKind {
    /// A module-level free function — `pub fn name(params) { … }`.
    TopLevel,
    /// An inherent method inside `impl user_<T> { … }`. `self_conv` is the receiver
    /// convention for an instance method (`Read`→`&self`, `Mutate`→`&mut self`,
    /// `Move`→`self`), or `None` for a STATIC (associated) method (no `self`
    /// parameter). The method name is mangled (`user_<name>`) and emitted with `pub`.
    Method {
        self_conv: Option<AccessConvention>,
        owner_type: Type,
    },
    /// c109 Phase 12: a trait-impl method inside `impl Trait for user_<T> { … }` (the
    /// caller `emit_trait_impl`/`emit_external_trait_impl` opened the block). Distinct
    /// from an inherent `Method`: the method name is BARE (the trait owns it — no
    /// `user_` mangle) and there is NO `pub`. `self_conv` is the receiver convention
    /// (`Read`→`&self`, `Mutate`→`&mut self`, `Move`→`self`) — D-MUTSELF1: a `mut self`
    /// trait method gets `&mut self` and may mutate the receiver in place. `is_unsafe`
    /// reproduces the `unsafe fn` prefix for an `#Unsafe fn` trait method (S58/D-LL1 —
    /// the body may use gated ops; calling it is already gated to an `#Unsafe` block).
    TraitMethod {
        is_unsafe: bool,
        self_conv: AccessConvention,
        /// D-SERDE2 (card #131 S1-bridge): a hand-written `impl T.Encode` /
        /// `impl T.Decode` method. The user writes the verbs `encode`/`decode`
        /// with Jet-facing signatures, but the Rust `user_Encode`/`user_Decode`
        /// traits declare `jet_encode(&self) -> DataTree` /
        /// `jet_decode(tree: &DataTree) -> Result<Self, DecodeError>`. This bridges
        /// the name + signature internally (I2: a sema-accepted hand impl must
        /// produce Rust rustc accepts). `None` for every ordinary trait method.
        serde: Option<SerdeCodec>,
    },
    /// c109 Phase 15: a DELEGATION trait method (`using field`) — `emit_delegation_method`
    /// (Source/Codegen/Items.rs). The whole method is structural: a forwarding call
    /// `(self).<field>.<method>(<args>)` to the delegated field, with the BARE trait
    /// method name (no `user_` mangle). There is NO body to lower — the forward string is
    /// resolved at lowering. The signature reproduces `emit_delegation_method`'s exact
    /// shape (a quirky two-space `  {` before the brace, `&self` receiver, no `pub`).
    /// `has_return` decides whether the forward line ends in `;` (unit) or not (returns).
    /// `sig` is the fully-rendered signature line (`    fn name(params)  {\n` with its
    /// quirky double space) and `fwd` the forwarding call — both resolved at lowering.
    Delegation {
        sig: String,
        fwd: String,
        has_return: bool,
    },
}

/// c109 Phase 22: the method-call-collection iteration form on a `loop x; <coll>`,
/// resolved at lowering from `emit_for_in`'s `Expr::MethodCall` branches
/// (Source/Codegen/Statement.rs). Each carries the receiver's emitted Rust string;
/// `file`/the panic line are program/source facts. The plain `.iter().cloned()` form
/// (incl. a non-special method-call collection like `.split(…)`, which `emit_for_in`
/// routes to its `else` default) is represented by `ForIn.method_kind == None`.
pub enum TForInMethod {
    /// `loop c; s.chars()` — char iteration: `for _jet_c in ({recv}).chars()`,
    /// binding `let <var> = _jet_c;`.
    Chars,
    /// `loop line; reader.lines()` on a `FileReader` — streaming `BufRead::lines`
    /// over the reader's `inner`, with a mid-stream-error panic (line `0`, `cx.file`).
    LinesFile,
    /// `loop line; io.stdin().lines()` / a `StdinHandle` — the same streaming read,
    /// but the receiver is materialised into a `_jet_stdin_h` local inside an extra
    /// block (so the `io.stdin()` temporary outlives the loop body), with a matching
    /// extra closing brace.
    LinesStdin,
    /// D-PROCESS1=A: `loop line; child.stdout.lines()` / `child.stderr.lines()` —
    /// a `ProcessChild`'s streaming reader. The receiver string is the plain field
    /// access (`(child).stdout`); each iteration polls
    /// `jet_process_stream_next_line(&recv)` via a `let Some(x) = … else { break }`,
    /// so (unlike `LinesFile`/`LinesStdin`) no extra wrapper block is needed.
    LinesProcessStream,
    /// D-ITER-HOOK: `loop x; mytype` when `mytype` implements `Iterable`.
    Iterable {
        coll_type: String,
        iter_type: String,
    },
}

/// c109 Phase 22: an `if` condition, resolved at lowering from the AST node shape
/// (`emit_if`/`if_pattern_test`, Source/Codegen/Statement.rs):
///  - `Plain` — a boolean expression: `if {cond} {`.
///  - `And` — a short-circuiting conjunction whose earlier pattern bindings
///    dominate every later condition and the selected body.
///  - `IfLet` — an optional-binding test (`x == value(b)` → `Some(b)`, `Ok(b)`/`Err(b)`,
///    a variant `c == Active(id)`): `if let {pat_str} = {subj} {`. The bound name(s)
///    are in scope in the then-branch (the binding's resolved type is bound at lowering,
///    mirroring `add_pattern_bindings`).
///  - `IsNone` — an `x == null` test (`Pattern::Absent`): `if {subj}.is_none() {`.
///  - `Matches` — a binding-free enum variant/group test (`d == .Fire`): `if matches!(&{subj}, {pat}) {`.
pub enum TIfCond {
    Plain(TExpr),
    /// A right-associated, short-circuiting conjunction. `left` is atomic;
    /// bindings it introduces dominate `right` and the selected body.
    And {
        left: Box<TIfCond>,
        right: Box<TIfCond>,
    },
    IfLet {
        pattern: TPattern,
        subj: TExpr,
    },
    IsNone { subj: TExpr },
    Matches { pattern: TPattern, subj: TExpr },
}

/// D-DOTSCOPE1: which `#Test` scope member a `TStmt::ScopeMember` is.
pub enum ScopeMemberKind {
    /// `.setup { … }` — the body's statements are spliced inline (bindings leak
    /// to the rest of the test), running first.
    Setup,
    /// `.expect_fail { … }` — the region must fail (a `require` failure or a
    /// panic). Runs under a panic-catching boundary; if it completes cleanly the
    /// test fails with "expected this region to fail, but it passed".
    ExpectFail,
    /// `.timeout(dur) { … }` — post-hoc budget in nanoseconds. The region runs to
    /// completion, then its elapsed time is compared against the budget; over
    /// budget fails the test. (v1: post-hoc — does not interrupt a hang.)
    Timeout(u64),
    /// `.skip { … }` — a region that is not executed. Emitted as `if false { … }`
    /// so the body still type-checks but never runs.
    Skip,
}

/// D-SHAPE-PLACE1: a field write through an indexed collection element.
///
/// This remains structured through lowering so every backend mutates the
/// collection element itself instead of reconstructing the field-read
/// expression, whose list-index path returns a clone.
pub struct TIndexFieldAssign {
    pub base: TExpr,
    pub index: TExpr,
    pub is_map: bool,
    pub index_proven: bool,
    /// Jet field name; emit mangles.
    pub field: String,
    pub field_ty: Type,
    pub op: Option<BinOp>,
    pub value: TExpr,
    pub clone_value: bool,
    pub line: usize,
}


/// Injected prelude struct fields (HttpRequest route metadata). Emit spells lines.
#[derive(Clone)]
pub enum TStructExtra {
    /// HttpRequest: `params: BTreeMap::new(), route_template: None`
    HttpRequestParams,
}

/// Host/prelude call assembled only in emit — structured pieces, no Rust source text.
pub enum THostCall {
    /// `{root}{helper}({args…})` with per-arg wrap style.
    Helper {
        helper: String,
        args: Vec<THostArg>,
    },
    /// `(recv).{method}({args})`
    Method {
        recv: Box<TExpr>,
        method: String,
        args: Vec<TExpr>,
    },
    /// `(({base})[({index}).0 as usize].clone())` FixedList index.
    FixedListIndex {
        base: Box<TExpr>,
        index: Box<TExpr>,
    },
    /// Typed-text audited escapes / projections.
    TypedText {
        kind: TTypedTextForm,
        arg: Box<TExpr>,
    },
    /// Bare fn name used as a value before FnValue wrapping (Jet name).
    FnName(String),
    /// GC edit expression — structured slots; emit formats jet_gc edit wrappers.
    GcEdit {
        root: String,
        method_span_start: usize,
        edges: Vec<String>,
        edit: Box<TExpr>,
        index_temp: Option<(String, TExpr)>,
        kind: TGcEditKind,
    },
    /// GC local read: `jet_gc::runtime_or_exit(root.read(|__jet_value| __jet_value.clone()))`.
    GcRead {
        root: String,
    },
    /// Option/pattern projection helpers: `(inner).is_some()` / `.unwrap()` / field project.
    OptionProbe {
        inner: Box<TExpr>,
        kind: TOptionProbe,
    },
    /// D-PARSESTR1: str-match scan against `_jet_switch_subject`; emit builds the IIFE.
    StrMatchScan {
        parts: Vec<crate::AST::StrMatchPart>,
        probe: TMatchProbe,
    },
    /// D-BINPAT1: binary-pattern scan against `_jet_switch_subject`.
    BinMatchScan {
        parts: Vec<crate::AST::BinMatchPart>,
        probe: TMatchProbe,
    },
    /// Tuple element project: `(base).{index}` (after str/bin-match unwrap).
    TupleIndex {
        base: Box<TExpr>,
        index: usize,
    },
    /// Struct-pattern subject field: `((*_jet_switch_subject).{field})`
    SwitchSubjectField {
        field: String,
    },
    /// Generator `yield e` → `__jet_yield_tx.send(e)`.
    YieldSend {
        value: Box<TExpr>,
    },
    /// Sql/Html/Sh typed-text constructor from literals + hole exprs.
    TypedTextInterp {
        kind: TTypedTextInterpKind,
        literals: Vec<String>,
        holes: Vec<TExpr>,
    },
    /// `expect(x).snapshot()` harness call.
    ExpectSnapshot {
        value: Box<TExpr>,
        snap_path: String,
    },
    /// `core.env.set` with rich panic on invalid runtime strings.
    EnvSet {
        name: Box<TExpr>,
        value: Box<TExpr>,
        loc: TPanicLoc,
    },
    /// Numeric bounds constant: `{rust_type(ty)}::{member}`.
    NumericBounds {
        ty: Type,
        member: String,
    },
    /// `ExpiringSecret::<T>::new(value, ttl.ms, clock observer)`.
    ExpiringSecretNew {
        value: Box<TExpr>,
        duration: Box<TExpr>,
        clock: Box<TExpr>,
        elem: Type,
    },
    /// `jet_expiring_new(value, duration_ms, clock_now)`.
    ExpiringValueNew {
        value: Box<TExpr>,
        duration: Box<TExpr>,
        clock: Box<TExpr>,
    },
    /// D-CABI-CALLBACK1: `extern "C" fn` wrapper around a lowered lambda.
    CCallback {
        symbol: String,
        lambda: TLambda,
        ret: Option<Type>,
    },
}

/// Which jet_gc edit wrapper to emit for a collector-owned method call.
#[derive(Clone, Copy)]
pub enum TGcEditKind {
    Clear,
    Pop,
    RemoveIndex,
    InsertIndex,
    Prepend,
    Additive,
    Plain,
    EdgeSlot,
}

/// Str/bin-match scan result shape: bool test vs unwrap the hole tuple.
#[derive(Clone, Copy)]
pub enum TMatchProbe {
    IsSome,
    Unwrap,
}

pub enum THostArg {
    Expr(TExpr),
    /// Wrap as `&(expr)`
    Borrow(TExpr),
    /// Pre-lowered lambda (structured); emit uses TLambda spelling.
    Lambda(TLambda),
}

#[derive(Clone)]
pub enum TOptionProbe {
    IsSome,
    Unwrap,
    /// Project a Jet field after unwrap: `.{field}.clone()`
    Field(String),
}

#[derive(Clone, Copy)]
pub enum TTypedTextForm {
    SqlRaw,
    HtmlRaw,
    ShRaw,
    SqlTemplate,
    SqlParams,
    HtmlText,
}

#[derive(Clone, Copy)]
pub enum TTypedTextInterpKind {
    Sql,
    Html,
    Sh,
}

/// Let binding type annotation. Emit spells the `: …` clause (I3: no Rust text here).
#[derive(Clone)]
pub enum TLetTy {
    Inferred,
    /// `: &str` for a string-view binding.
    StrView,
    /// Explicit Jet type, optionally wrapped for resources / GC roots.
    Annotated {
        ty: Type,
        mut_fn: bool,
        wrapper: TLetWrapper,
    },
    /// Pattern-binding tuple annotation spelled `(T0, T1, …)`.
    Tuple(Vec<Type>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TLetWrapper {
    None,
    Resource,
    AutomaticRoot,
}

impl TLetTy {
    pub fn inferred() -> Self {
        Self::Inferred
    }

    pub fn plain(ty: Type) -> Self {
        Self::Annotated {
            ty,
            mut_fn: false,
            wrapper: TLetWrapper::None,
        }
    }

    pub fn of(ty: Type, mut_fn: bool, wrapper: TLetWrapper) -> Self {
        Self::Annotated {
            ty,
            mut_fn,
            wrapper,
        }
    }

    pub fn resource(ty: Type) -> Self {
        Self::of(ty, false, TLetWrapper::Resource)
    }

    pub fn automatic_root(ty: Type) -> Self {
        Self::of(ty, false, TLetWrapper::AutomaticRoot)
    }
}

/// Source location + locals snapshot for rich require/panic reports.
/// Emit alone formats `jet_panic_rich` / test-mode `return Err`.
#[derive(Clone)]
pub struct TPanicLoc {
    pub file: String,
    pub src_line: String,
    pub line: u32,
    pub col: u32,
    pub caret: u32,
    pub fn_name: String,
    /// `(display_name, place)` for scalar locals shown in debug panics.
    pub locals: Vec<(String, TLocal)>,
}

pub enum TRequireKind {
    /// `require(cond[, msg])`
    Require {
        cond: Box<TExpr>,
        msg: Option<Box<TExpr>>,
    },
    /// `require_eq(left, right)`
    RequireEq {
        left: Box<TExpr>,
        right: Box<TExpr>,
    },
    /// `panic(msg)` / `?? panic(msg)`
    Panic {
        msg: Box<TExpr>,
    },
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub enum TStmt {
    /// `let [mut] name[: ty] = init;`. All presentation facts are resolved at
    /// lowering, reproducing `emit_let` (Source/Codegen/Statement.rs) byte-for-byte:
    /// `kw` is `"let"` or `"let mut"` (the `mut` accounts for the source `mutable`
    /// flag AND the forced-mut cases — a handle binding FileReader/FileWriter/
    /// TcpStream/HttpRouter/Arena/… needs `let mut` even when bound immutably, and an
    /// escaping FnMut lambda binding); `let_ty` is the structured annotation (emit
    /// spells the `: …` clause). The binding's resolved type is carried on the
    /// `LowerEnv` slot (for downstream facts), so it is not duplicated on the node.
    Let {
        name: String,
        kw: &'static str,
        let_ty: TLetTy,
        init: TExpr,
        /// D-OPTGC1=A: sema's complete automatic-promotion decision.
        gc_promotion: Option<crate::AST::GcPromotion>,
        gc_transferred: bool,
    },
    /// D-OPTGC1=A: assignment through a collector-owned bare value.
    GcEdit {
        root: String,
        slot: String,
        edges: Vec<String>,
        replace_all: bool,
        index_temp: Option<(String, TExpr)>,
        stmt: Box<TStmt>,
    },
    /// D-SHAPE-PLACE1=A: one acquisition in a sema-proven disjoint constant
    /// index/range partition. The first acquisition initializes `root`; later
    /// acquisitions split a retained region at their original statement.
    SplitViews {
        owner: Option<TExpr>,
        root: String,
        len: String,
        source: String,
        source_start: i64,
        before: String,
        split_tail: String,
        segment: String,
        after: String,
        name: String,
        start: i64,
        end: i64,
        single: bool,
        write: bool,
        elem_ty: Option<Type>,
        line: usize,
    },
    /// c109 Phase 23: a TUPLE-destructuring binding `(a, b) :: <init>` (S74,
    /// `BindPattern::Tuple`). Reproduces `emit_stmt`'s destructure form byte-for-byte:
    /// a `let {tmp} = &({init});` temp (borrowed — never moves out of a shared ref, I2),
    /// then one `let[ mut] {elem_rust} = ({tmp}).{field_rust}.clone();` per element,
    /// pairing the pattern's elements to the tuple type's CANONICAL fields by position
    /// (resolved at lowering off the init's total `Type::Tuple`). `tmp` is the
    /// `__jet_d{span}` name the AST uses (resolved at lowering); `kw` is `"let"`/`"let mut"`.
    TupleDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        /// `(elem_rust_name, field_rust_name)` per bound element, canonical order.
        binds: Vec<(String, String)>,
    },
    /// c109: a STRUCT-destructuring binding `Type.{ x, y } :: <init>` (S74,
    /// `BindPattern::Struct`). Reproduces `emit_stmt`'s `BindPattern::Struct` arm
    /// byte-for-byte: a `let {tmp} = &({init});` borrow temp, then one
    /// `let[ mut] {local_rust} = ({tmp}).{field_rust}.clone();` per bound field.
    /// D-DESTRUCT1: `local_rust`/`field_rust` diverge for a renamed field
    /// (`severity: sev` binds local `sev` from field `severity`); they're equal
    /// when unrenamed (pre-D-DESTRUCT1 shape). The field's resolved type comes
    /// from `cx.struct_fields` (the init's `Type::Named`/`Apply` name), resolved
    /// at lowering for the slot.
    StructDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        /// `(local_rust_name, field_rust_name)` per bound field, source order.
        binds: Vec<(String, String)>,
    },
    /// c109 Phase 26: a LIST-destructuring binding `[a, b, c] :: <init>` (S74,
    /// `BindPattern::List`). Reproduces `emit_stmt`'s `BindPattern::List` arm
    /// byte-for-byte: a `let {tmp} = &({init});` borrow temp, then one
    /// `let[ mut] {elem_rust} = jet_unpack_vec({tmp}, {want}, {i}, {file:?}, {line});`
    /// per element. `want` is the element count, `i` the position, and `file`/`line`
    /// the destructure span's source location (resolved at lowering for the
    /// bounds-mismatch panic). Each element binds a non-deref slot whose type
    /// reproduces `expr_jet_ty(init)`'s `Some(List(inner))` partiality (a non-`List`
    /// init — e.g. a `[T#N]` fan-out result — yields `None`, matching the AST).
    ListDestructure {
        tmp: String,
        init: TExpr,
        kw: &'static str,
        want: usize,
        file: String,
        line: usize,
        /// `elem_rust_name` per bound element, source order.
        elems: Vec<String>,
    },
    /// `place [op]= value;` to a plain local (subset excludes indexed assigns).
    /// `op` is the compound-assignment operator (`+=` etc.) or `None` for `=`.
    Assign {
        /// The structured target: a local slot, or a place expression (a field
        /// chain, a `Pool` slot). Every engine reads the structure; only emit
        /// spells Rust.
        place: TPlace,
        op: Option<BinOp>,
        value: TExpr,
        /// c150: true when the value is a borrowed non-scalar ident (a `Read`-convention
        /// non-Copy parameter in deref position). Assigning `(*user_s)` directly moves
        /// out of a shared reference (E0507); emitting `((*user_s)).clone()` is correct.
        /// Mirrors the `lower_enum_arg` clone predicate. False for scalars and owned values.
        clone_value: bool,
    },
    Return(Option<TExpr>),
    /// A call used for effect: `print(x);`, `helper(a);`.
    ExprStmt(TExpr),
    /// D-SHAPE-RESOURCE2=A: one sema-checked `defer close(^resource)` action.
    /// AOT emits a Drop guard; non-resident dev tiers use their named fallback.
    DeferClose {
        close: TExpr,
        resource: String,
        id: usize,
    },
    /// Statement-form `if`/`else`. `else_body` is `None` for a bare `if`.
    /// `cond` (c109 Phase 22) is a `TIfCond`: a plain boolean expr, an optional-binding
    /// `if let <pat> = <subj>` (an `x == value(b)`/`Ok(b)`/`Err(b)`/variant condition),
    /// or an `<subj>.is_none()` test (`x == null`) — reproducing `emit_if`'s three
    /// condition shapes (Source/Codegen/Statement.rs).
    /// `else_is_elseif` distinguishes the source `ElseBranch`: `true` for a real
    /// `else if` chain (`ElseBranch::ElseIf` — the else-body is the synthesised nested
    /// `If`, emitted as `} else if …`), `false` for an explicit `else { … }` block
    /// (`ElseBranch::Else`, emitted as `} else { … }` even when the block holds a
    /// single `if`). The AST path keys solely on the `ElseBranch` variant; the TIR
    /// must NOT flatten an explicit `else { if … }` into `else if` (a parity drift).
    If {
        cond: TIfCond,
        then_body: Vec<TStmt>,
        else_body: Option<Vec<TStmt>>,
        else_is_elseif: bool,
    },
    /// `loop { … }` — an infinite loop (`Stmt::Loop`). `label` is the optional
    /// `name :: loop` rendered as `'jet_<name>:` (resolved at lowering, never re-derived).
    Loop {
        label: Option<String>,
        body: Vec<TStmt>,
    },
    /// `loop cond { … }` — the while form (`Stmt::While`). Lowers to Rust `while`.
    While {
        label: Option<String>,
        cond: TExpr,
        body: Vec<TStmt>,
    },
    /// D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` — the three-part counted loop.
    /// Emitted as `{ let mut init_name = init_val; loop { if !(cond) { break; } body; step; } }`.
    CountedLoop {
        label: Option<String>,
        init: Box<TStmt>,
        cond: TExpr,
        step: Option<Box<TStmt>>,
        body: Vec<TStmt>,
    },
    /// `loop i; start..end [; stride]` — a numeric range loop (`ForKind::Range`).
    /// Inclusive `..` (S22) lowers to `start..=end`; exclusive `..<`
    /// (D-RANGE-EXCL1=C) lowers to `start..end`. Optional stride uses `.step_by`.
    Range {
        label: Option<String>,
        var: String,
        start: TExpr,
        end: TExpr,
        step: Option<TExpr>,
        exclusive: bool,
        body: Vec<TStmt>,
    },
    /// `break` / `name.break()` (label resolved at lowering).
    Break(Option<String>),
    /// Source `next` / `name.next()`; internally retained as Continue.
    Continue(Option<String>),
    /// c109 Phase 4: an exhaustive `when`/match on an enum subject (`Stmt::Switch`
    /// whose arms are all variant patterns). Lowers to a Rust `match`, mirroring
    /// `emit_pattern_match_switch` byte-for-byte. `subject` is the already-lowered
    /// subject expression; `clone_subject` reproduces the AST path's `(subj).clone()`
    /// when the subject reads as a borrow (a by-reference enum param), so the match
    /// owns the value. Each arm carries its resolved Rust pattern string and an
    /// optional range-guard string (both fully resolved at lowering — emit makes no
    /// pattern decision). `fallthrough` records whether the AST path appends the
    /// `_ => unreachable!("jet: exhaustiveness bug")` arm (true when there is no
    /// explicit `else`); sema already proved exhaustiveness (E0307), so the dead arm
    /// exists only because rustc cannot see that proof.
    EnumMatch {
        /// The matched subject. A by-reference subject sets `clone_subject` so the
        /// match owns the value; the slot itself is read without its deref.
        scrutinee: TExpr,
        clone_subject: bool,
        arms: Vec<TMatchArm>,
        else_body: Option<Vec<TStmt>>,
        fallthrough: bool,
    },
    /// c109 Phase 4: a `when`/match whose arms are all arm-head *range* patterns
    /// (`0..59 -> …`) over a scalar subject, plus a required `else`. The AST path
    /// (`emit_mixed_switch`) lowers this to an `if/else if … else` chain wrapped in
    /// a block that binds `_jet_switch_subject` to a borrow of the subject (the
    /// binding is unused in this form but emitted for parity). Each arm's `(lo, hi)`
    /// becomes `(subj >= lo && subj <= hi)`, reading the subject's resolved place.
    RangeSwitch {
        /// The matched subject expression. Emit borrows it for the
        /// `_jet_switch_subject` binding and re-emits it in each range test.
        subject: TExpr,
        arms: Vec<(i64, i64, Vec<TStmt>)>,
        else_body: Vec<TStmt>,
    },
    /// c109 Phase 5: indexed assignment `coll[i] = value` (`Stmt::Assign` with an
    /// `LValue::Index`). `is_map` is the resolved `IndexKind` (TOTAL, from sema):
    /// `true` → `jet_map_insert(&mut (base), (i).clone(), v)`; `false` →
    /// `(base)[i as usize] = v`. Both wrap the value in a `{ let __jet_v = …; … }`
    /// block, byte-for-byte the AST `LValue::Index` form. Compound ops (`+=`) on an
    /// index are not a Jet construct here (the parser/sema only admit a plain `=` to
    /// an index lvalue), so no `op` is carried.
    IndexAssign {
        base: TExpr,
        index: TExpr,
        is_map: bool,
        value: TExpr,
    },
    /// D-SHAPE-PLACE1: `coll[i].field [op]= value`.
    IndexFieldAssign(Box<TIndexFieldAssign>),
    /// D-INDEX-HOOK: `mytype[k] = v` via `IndexMut::set`.
    IndexHookAssign {
        type_name: String,
        base: TExpr,
        index: TExpr,
        value: TExpr,
    },
    /// D-SWIZZLE1: write swizzle `v.xy = value` — ordered lane assignments into the
    /// receiver's backing array. Sema rejects overlapping patterns (E3111).
    MathSwizzleAssign {
        base: TExpr,
        type_name: String,
        lanes: Vec<u8>,
        value: TExpr,
        clone_value: bool,
    },
    /// c109 Phase 5/22: collection iteration `loop x; coll` / `loop k, v; map`
    /// (`Stmt::For` with `ForKind::In`). `var2` distinguishes the two-binding map
    /// form (which iterates `(coll).iter()` and clones each key/value) from the
    /// single-binding form (`(coll).iter().cloned()`), reproducing `emit_for_in`
    /// exactly. `method_kind` (c109 Phase 22) carries the method-call-collection
    /// iteration form (`.chars()` char iteration, `.lines()` streaming reads)
    /// resolved at lowering off the same `emit_for_in` branch; `None` is the plain
    /// `.iter()` form (incl. a non-special method-call collection like `.split(…)`,
    /// which the AST routes to the `.iter().cloned()` default). When `method_kind`
    /// is set `source` holds the method *receiver* (not the whole method call), and
    /// `var2` is always `None` (a method-call collection is single-binding only).
    ForIn {
        label: Option<String>,
        var: String,
        var2: Option<String>,
        /// The expression iterated over: the method receiver for a `method_kind`
        /// form, otherwise the whole collection.
        source: TExpr,
        /// The whole collection expression, whose type carries the element type.
        collection: TExpr,
        /// D-LOOP-ADVANCE2=A source stride, evaluated once before the first pull.
        step: Option<TExpr>,
        method_kind: Option<TForInMethod>,
        /// D-SOA1: the collection is a `#layout(columnar)` list — iterate via
        /// `({coll}).iter_aos()` (yields owned gathered `S`) instead of
        /// `({coll}).iter().cloned()`. Always `false` for the map/method forms.
        columnar: bool,
        /// D-STREAMYIELD1: the collection is a `Stream<T>` (`Receiver<T>`) —
        /// iterate it directly BY VALUE (`for x in (coll) { }`; `Receiver<T>`
        /// already implements `IntoIterator<Item = T>`), not `.iter().cloned()`.
        by_value: bool,
        body: Vec<TStmt>,
    },
    /// c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema picked the
    /// branch (`selected_then`); the AST `emit_stmts` emits ONLY that branch's
    /// statements INLINE at the same indent (no `if`, no block — and its `let`s leak
    /// into the outer scope, exactly like a plain block). The TIR carries the lowered
    /// statements of the selected branch and emits them with no wrapper. When the
    /// selected branch is `else` but there is no else-body (or sema didn't resolve),
    /// this holds an empty vec (emits nothing).
    Inline(Vec<TStmt>),
    /// D-CANVASSTATE1=D: `#DebugOnly <stmt>` / `#DebugOnly { … }`.
    /// AOT emission gates this behind `#[cfg(not(jet_release))]`; dev/JIT tiers
    /// lower it as ordinary debug code. `#Off` has no TIR node: it lowers to an
    /// empty `Inline`.
    DebugOnly(Vec<TStmt>),
    /// c109 Phase 15: a MIXED comparison/Bool `when` switch (`emit_mixed_switch`,
    /// Source/Codegen/Statement.rs) — the general `if/else if … else` form used when the
    /// arms are NOT all-variant (that is shape A, a Rust `match`), NOT all-range (shape
    /// B, `RangeSwitch`), and NOT all-fallible (shape C). Each arm head is a plain
    /// comparison/Bool expression. The AST path wraps the chain in a block that binds
    /// `_jet_switch_subject = &(subject)` (emitted for parity even when unused), then an
    /// `if/else if …` chain over each arm's condition, with the `else`/fallthrough form
    /// reproduced exactly. Each arm's condition is resolved to a Rust string at lowering
    /// (emit makes no decision). `else_body` is the optional `else` arm.
    MixedSwitch {
        /// The matched subject. Emit borrows it for the parity binding; arm
        /// conditions are already structured `TExpr` values.
        subject: TExpr,
        arms: Vec<(TExpr, Vec<TStmt>)>,
        else_body: Option<Vec<TStmt>>,
    },
    /// c109 Phase 18: an audited `#Unsafe { … }` gate region (`Stmt::Unsafe`, S58,
    /// E2-M13/D-LL1). The AST `emit_stmts` lowers it straight to a Rust `unsafe { … }`
    /// block; the `#Audit("…")` annotation (the `audit` field) emits NOTHING (codegen is
    /// dumb — sema validated the audit). I1: this TIR node exists ONLY for a source
    /// `#Unsafe` region, so the emitted `unsafe { … }` is always 1:1 with a source gate.
    /// Body bindings use the `unsafe` block's child lexical env.
    Unsafe(Vec<TStmt>),
    /// D-CTEFFECT1: an explicit `#Impure("reason") { … }` policy gate.
    /// AOT/JIT execute a plain lexical block; comptime evaluation raises its
    /// impurity depth only while evaluating this body.
    Impure(Vec<TStmt>),
    /// D-REACTCORE1: `#Reactive { … }` — register a reactive effect at this point.
    Reactive {
        closure: String,
    },
    /// c109 Phase 19: an explicit `region r { … }` (D-REGION1 opt B). Lowers to a plain
    /// Rust block `{ … }` — a lexical scope. The region's escape bound (E0631) and arena
    /// drop ordering (S63 RAII) are enforced entirely in sema; codegen is dumb (I3).
    /// Body bindings live only in the child `LowerEnv` matching that Rust scope.
    Region(Vec<TStmt>),
    /// D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }` — a Cassowary-style
    /// constraint block. Unlike `Region`/the taskgroup path, this DOES need a
    /// real runtime object: `handle` is the slot the fresh `jet_layout::Handle`
    /// binds into, `label` is the source name (for the
    /// handle's debug/conflict-report label), and `body` is the block's
    /// statements lowered on the SAME env the handle was just bound into (the
    /// parser already desugared every `box.anchor` read to an ordinary
    /// `NAME.h(box, anchor)`/`NAME.v(box, anchor)` method call, so `body` is
    /// nothing but plain statements — no layout-specific TIR shape needed
    /// beyond the handle construction itself).
    Layout {
        handle: TLocal,
        label: String,
        body: Vec<TStmt>,
    },
    /// c109 Phase 19: a `#Context(field: value) { … }` smart-context block (D-CTX1). Lowers
    /// to a plain block with one RAII/no-op guard per field (in declaration order)
    /// BEFORE the body: `allocator`/`deadline` push a dynamic context guard in
    /// `jet_mem`; `logger` stays a v1 no-op value bind. Each `(field_name, value)`
    /// pair is resolved at lowering. The body uses the block's child lexical env.
    ContextBlock {
        guards: Vec<(String, TExpr)>,
        body: Vec<TStmt>,
    },
    /// D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input block.
    /// Lowers to:
    ///   `{ jet_term_enter(); let _live_guard = jet_scope_guard(|| jet_term_leave()); <body> }`
    /// The scope guard guarantees `jet_term_leave()` runs on every exit path — normal
    /// fall-through, early `return`, `?` propagation, and panic unwind. Codegen is dumb
    /// (I3): no decisions here, only emitting the already-checked RAII form.
    Live {
        body: Vec<TStmt>,
    },
    /// D-SHIELDNAME1=A (ratified 2026-07-11): `#Shield { … }` — a cancellation-shield
    /// region. Lowers to:
    ///   `{ jet_scheduler_shield_enter(); let _shield_guard = jet_scope_guard(|| jet_scheduler_shield_leave()); <body> }`
    /// The scope guard guarantees `jet_scheduler_shield_leave()` runs on every exit
    /// path (normal, `return`, `?`, panic unwind) so a deferred cancel/deadline lands
    /// at region exit — deadline first, then cancel (the runtime `_leave` decides the
    /// order). A no-op outside a task (SHIELD_DEPTH is thread-local). Codegen is dumb
    /// (I3): only emits the already-checked RAII form.
    Shield {
        body: Vec<TStmt>,
    },
    /// D-DOTSCOPE1: a `#Test` scope-member region — `.setup` / `.expect_fail` /
    /// `.timeout(dur)` / `.skip`. Emitted only inside a `jet test` harness fn
    /// (`fn jet_test_N() -> Result<(), String>`); see `emit_tir_stmt` for the
    /// per-kind lowering. Whole-test `.skip` (a `.skip` first statement) is
    /// handled by the harness `main`, not here.
    ScopeMember {
        kind: ScopeMemberKind,
        body: Vec<TStmt>,
    },
    /// D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` — a transaction
    /// block. Lowers to:
    ///   `{ let mut <handle> = jet_transaction(); <body>; <handle>.commit(); }`
    /// `<handle>.on_commit(() => { … })` inside the body lowers to
    /// `<handle>.on_commit(Box::new(move || { … }))`. The registered hooks run LIFO
    /// in `JetTransaction::drop` — but only if `commit()` ran. A `?`-failure or any
    /// early return skips `commit()`, so the hooks drop un-run (D-TXN3). The
    /// irreversible-effect rejection (E0746, D-TXN2) and rollback are sema's job;
    /// codegen is dumb (I3): effects/transaction state are a compile-time fact.
    Transact {
        /// The transaction handle's slot, or `None` for a bare `#Transact { … }`
        /// with no handle (no `on_commit`/`on_rollback` hooks). When `snapshots` is
        /// non-empty a handle is synthesized even for a bare block, so the
        /// auto-snapshot has a transaction to register on.
        handle: Option<TLocal>,
        /// D-TXN-ROLLBACK layer 1+2: each snapshotted local plus, when the local's
        /// type implements `Rollback`, that type. Without a type the snapshot is
        /// clone-based (`jet_txn::snapshot`); with one it uses
        /// `jet_txn::snapshot_custom` and `<ty>::restore`, so the custom cheap diff
        /// runs instead of a full clone.
        snapshots: Vec<(TLocal, Option<Type>)>,
        /// D-STM1=A (card #506): true when the block touches the `Shared<T>` plane
        /// (some `.edit` inside routed to `edit_txn`), so emission wraps the body in
        /// `jet_stm::begin()` … `.commit()` — the atomic multi-handle commit. False
        /// for a plain local-only `#Transact` (byte-identical to the pre-STM output).
        uses_stm: bool,
        body: Vec<TStmt>,
    },
    /// D-DBG3 step 2 (dap-debugger): a source line marker, one per lowered `Stmt`,
    /// inserted ONLY when `cx.debug_linemap` is set (native `jet debug` builds —
    /// never a normal build or the JIT tier, so this is invisible to the JIT
    /// lowering gate and every other TStmt consumer). Emits a `// jet:line N`
    /// comment immediately before the statement's generated Rust, giving the native
    /// backend a rust-line -> jet-line table without touching any other TStmt shape.
    LineMarker(usize),
}

/// c109 Phase 4: one lowered arm of an exhaustive enum match. `pattern` is the
/// fully-resolved Rust match pattern (`user_Light::user_Red`,
/// `user_Conn::user_Active(user_id) | user_Conn::user_Reconnecting(user_id)`,
/// `user_Http::user_Good(__jet_range_0)`); `guard` is the optional `if …` range
/// guard. Both are computed once at lowering — emit only formats them.
pub struct TMatchArm {
    pub pattern: TPattern,
    pub body: Vec<TStmt>,
}

/// A pattern carried as structure instead of a rendered Rust pattern: the source
/// pattern sema checked, the resolved owning enum, and the syntactic position it
/// tests in. Every engine reads the pattern itself; only emit spells Rust.
#[derive(Debug, Clone)]
pub struct TPattern {
    pub pattern: crate::AST::Pattern,
    /// The owning enum, when the subject is a user/foreign/core enum.
    pub enum_type: Option<String>,
    pub position: TPatternPosition,
}

/// Where a `TPattern` is tested. The position decides how much a match binds,
/// which is a semantic fact each engine needs, not a spelling detail.
#[derive(Debug, Clone)]
pub enum TPatternPosition {
    /// A binding test that destructures payload slots into locals (`if x == Ok(v)`).
    Binding,
    /// A match-arm head, which also binds payload slots.
    Arm,
    /// A payload-free variant path, compared by value.
    VariantPath,
    /// D-ENC-DYN1: a `Data` object test that captures the raw entry pairs into
    /// `temp`; a body prefix collects them into the map the body sees.
    DataEntries { temp: String },
}

impl TPattern {
    /// A match-arm head over `enum_type`.
    pub fn arm(pattern: crate::AST::Pattern, enum_type: Option<String>) -> TPattern {
        TPattern {
            pattern,
            enum_type,
            position: TPatternPosition::Arm,
        }
    }

    /// A payload-binding test (`if let` position).
    pub fn binding(pattern: crate::AST::Pattern) -> TPattern {
        TPattern {
            pattern,
            enum_type: None,
            position: TPatternPosition::Binding,
        }
    }

    /// The variant this pattern tests, when it tests one.
    pub fn variant(&self) -> Option<&str> {
        match &self.pattern {
            crate::AST::Pattern::Variant { variant, .. } => Some(variant),
            crate::AST::Pattern::Or(alts, _) => match alts.first() {
                Some(crate::AST::Pattern::Variant { variant, .. }) => Some(variant),
                _ => None,
            },
            _ => None,
        }
    }
}

/// One piece of a D-VARIADIC1 list spread literal — either a single element or `...list`.
pub enum ListSpreadPart {
    Elem(TExpr),
    Spread(TExpr),
}

/// A lowered expression: a resolved `Type` plus its kind. `ty` is **total** — it
/// is never absent, and codegen never recomputes it.
pub struct TExpr {
    pub ty: Type,
    pub kind: TExprKind,
}

pub enum TExprKind {
    /// Integer literal with its D-SG9 width (`None` = default `Int`/i64). The
    /// width is the elaborated `(signed, bits)` sema attached to the AST node.
    IntLit(i64, Option<(bool, u8)>),
    FloatLit(f64),
    BoolLit(bool),
    CharLit(char),
    /// String literal / interpolation. Each part is literal text or an
    /// interpolated TExpr (totally typed, like every other node).
    StrLit(Vec<TStrPart>),
    /// A local or parameter slot, resolved to its Jet binding name plus the
    /// by-reference deref fact. Emit spells the Rust place; no engine parses it.
    Local(TLocal),
    /// Unit / default / uninit / comptime / host forms — structured facts only.
    /// Scalar comptime values use IntLit/BoolLit/CharLit via `lower_comptime_scalar`.
    Unit,
    DefaultLit,
    Uninit,
    CtLit(crate::AST::CtValue),
    HostCall(Box<THostCall>),
    /// A reference to a declared (non-comptime) const, by its Jet name. Emit
    /// resolves the Rust static name; other engines look the value up by the
    /// same Jet name in their own const table.
    ConstRef(String),
    /// D-ENC-DYN1: collect the ordered `Data` object entries a
    /// `TPatternPosition::DataEntries` test captured into the user-visible map.
    DataEntriesToMap(TLocal),
    /// Call to a plain top-level function. Each arg carries its emit decisions.
    Call {
        name: String,
        args: Vec<TCallArg>,
    },
    /// Transparent constructor for an unchecked distinct value. Resident JIT
    /// lowers this to the base scalar; AOT emits the nominal tuple constructor.
    DistinctCtor {
        name: String,
        arg: Box<TExpr>,
        base: Type,
    },
    /// D-RANGETYPE1: checked constructor for `distinct Int(lo..hi)` under
    /// postfix `?`. Emits `user_T::try_new(arg)` returning `Result<user_T,
    /// String>`; the enclosing `Try` node handles propagation.
    RangeCheckedCtor {
        name: String,
        arg: Box<TExpr>,
    },
    /// D-SHAPE-CONVERT1=A: numeric-backed distinct/unit conversion. `op`
    /// converts the named source into the distinct base; emit then wraps the
    /// value, composing a fallible numeric conversion and/or range check.
    DistinctConvert {
        name: String,
        arg: Box<TExpr>,
        op: TNumericOp,
        range: Option<(i64, i64)>,
        /// Sema's authoritative return contract. A literal may discharge a
        /// distinct range check, but never a conversion declared fallible.
        fallible: bool,
    },
    /// D-QUANTITY-CONVERT1=B: a checked or explicitly rounded conversion
    /// between two members of one package-owned unit family. The backend sees
    /// only erased Float values and resolved conversion coefficients.
    UnitConvert {
        destination: String,
        arg: Box<TExpr>,
        scale: crate::AST::UnitRatio,
        offset: crate::AST::UnitRatio,
        rounding: Option<(jet_foundation::UnitRoundingMode, Box<TExpr>)>,
        fallible: bool,
        file: String,
        line: u32,
    },
    /// D-SIMD2 / D-LINALG1: a built-in math-type constructor `F32x4(a,b,c,d)` /
    /// `Vec3(x,y,z)` / `Mat3(…)`, or a static method `F32x4.splat(x)` /
    /// `Vec3.from_array(a)`. Emits the prelude free function `{root}jet_math_<T>_<fn>(…)`
    /// (`_new` for the constructor) with plainly-lowered float/array args.
    MathBuiltin {
        type_name: String,
        func: String,
        args: Vec<TExpr>,
    },
    /// D-BIGINT1 / D-DECIMAL1: precise numeric ctor/method/binop → `jet_bigint_*` / `jet_decimal_*`.
    PreciseBuiltin {
        type_name: String,
        func: String,
        args: Vec<TExpr>,
    },
    /// `print(x)` — the one builtin the subset covers.
    Print(Box<TExpr>),
    /// D-LIN1-DROP (ratified 2026-06-25): `drop(x)` — deliberately discard a
    /// value (a `#SingleUse` value's audited terminal consumption). Lowers to a
    /// plain `drop(arg)` in Rust: a move-to-nowhere whose `Drop` runs. No
    /// `unsafe` is emitted (I3) — the `#Unsafe` gate is a sema-only audit.
    Drop(Box<TExpr>),
    /// D-SHAPE-RESOURCE2=A: ambient `close(^value)` after sema has proved the
    /// concrete value implements the nominal `Close` trait.
    Close(Box<TExpr>),
    ResourceNew(Box<TExpr>),
    ResourceTake(String),
    /// c109 Phase 25: the ambient prelude `input(...)` (D-PRELUDE1 = B). A bare call
    /// (no module alias) lowering to `{root}jet_std_io_input(None|Some(&(prompt)))`,
    /// byte-for-byte the `emit_call` ambient-input branch (Source/Codegen/Expression.rs
    /// ~L1778). `prompt` is `Some` when a String prompt arg is given, else `None`.
    AmbientInput {
        prompt: Option<Box<TExpr>>,
    },
    /// c109 Phase 26: a `require(cond[, msg])` / `require_eq(a, b)` / `panic(msg)`
    /// rich-runtime-report builtin (S36). Structured facts only — emit formats
    /// `jet_panic_rich` / test-mode `return Err` (I3: no `cx.src` re-read for
    /// location; `loc` was captured at lowering).
    RequireStop {
        kind: TRequireKind,
        loc: TPanicLoc,
        /// True only for the unconditional builtin `panic(...)`; `require`
        /// may fall through when the condition holds.
        always_stops: bool,
    },
    /// Binary op. `overflow` is the *computed* decision (true → emit the
    /// trapping `jet_add`/… helper). It mirrors today's `operand_is_integer`
    /// logic but is decided here, at lowering, from the total operand types.
    /// `line` is the source line of the operator, resolved at lowering, so the
    /// trapping helper's panic location matches the AST path byte-for-byte (the
    /// emitter never touches `cx.src`).
    Binary {
        op: BinOp,
        overflow: bool,
        line: u32,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// D-CHAINCMP1: `0 <= sev < 10` — a same-direction relational chain,
    /// `operands.len() == ops.len() + 1`. Dumb lowering (R1): emit binds each
    /// shared middle operand to a temp exactly once (a Rust block expression),
    /// then ANDs the adjacent-pair comparisons over those temps. Relational
    /// ops never trap on overflow (only `+ - * / << >>` do), so no `overflow`
    /// flag is needed here.
    CompareChain {
        operands: Vec<TExpr>,
        ops: Vec<BinOp>,
        hooks: Vec<bool>,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1 (GATE 1): `>=`/`<=`/`==` between layout
    /// values (`HVar`/`VVar`/`LengthVar`) produce a `Constraint`, which Rust's
    /// comparison operators can't do via operator syntax (`PartialOrd`/
    /// `PartialEq` are hard-locked to `bool`) — so this is a DEDICATED node,
    /// not `Binary`. Emits the matching `jet_layout::{ge,le,eq_}(lhs, rhs)`
    /// free function (registers the constraint on whichever side's `LinExpr`
    /// carries the owning handle). `Add`/`Sub` between layout values stay
    /// plain `Binary` — `jet_layout::LinExpr` implements `std::ops::{Add,Sub}`.
    LayoutCompare {
        op: BinOp,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// D-LAYOUT1: a plain `Int`/`Float` operand used on the other side of a
    /// layout `+`/`-`/`>=`/`<=`/`==` (axis-neutral, elaborates to `LengthVar`
    /// — see `layout_axis_of`). Wraps the numeric Rust value into a
    /// `jet_layout::LinExpr` constant so `Add`/`Sub`/`ge`/`le`/`eq_` only
    /// ever operate on `LinExpr` (no foreign-type operator-overload games
    /// with bare `f64`/`i64`).
    LayoutLit {
        inner: Box<TExpr>,
    },
    Unary {
        op: UnOp,
        operand: Box<TExpr>,
    },
    /// D-INCR1: `++`/`--` on a mutable integer lvalue. `place` is the structured
    /// assign/read target. `postfix`: return old value before update.
    IncDec {
        op: crate::AST::IncDecOp,
        place: TPlace,
        postfix: bool,
        ty: Type,
    },
    /// c109 Phase 3: a struct literal `S { f: v, … }`. The head type is `TExpr.ty`;
    /// each field carries its Jet name (emit mangles) and value. No clone/coercion
    /// at the literal site (mirrors the AST path).
    StructLit {
        /// Each field: Jet name, value, and `boxed` for self-referential `Box<…>` edges.
        fields: Vec<(String, TExpr, bool)>,
        /// c109 Phase 17: injected prelude fields (HttpRequest route metadata).
        /// Structured — emit spells the Rust field lines.
        extra: Option<TStructExtra>,
        /// c109 Phase 30: TRAIT-OBJECT coercion — `(trait, concrete owner)`.
        as_trait: Option<(String, String)>,
    },
    /// c109 Phase 3: a struct field *read* `recv.field` in borrow position.
    /// `field` is the Jet field name (emit mangles / core-renames).
    Field {
        recv: Box<TExpr>,
        field: String,
        boxed: bool,
    },
    /// c109 Phase 18: `mem.Ptr<T>.from_addr(addr)`. `elem` is the Jet element type;
    /// emit spells `(({addr}) as usize as *mut {elem})`.
    PtrFromAddr {
        elem: Type,
        addr: Box<TExpr>,
    },
    /// D-CAP9: postfix `p.*` — dereference a raw pointer. Emits Rust `(*(p))`. The
    /// `unsafe` needed to read through a raw pointer is supplied by the enclosing
    /// `#Unsafe` region/fn (sema-gated by E0208), so this node adds no `unsafe`.
    Deref(Box<TExpr>),
    /// D-CAP9: prefix `*x` — take a raw pointer to `x`. Emits `(&(x) as *const _)`.
    /// Forming a pointer is safe Rust; *using* it needs the surrounding `#Unsafe`
    /// region. Gated by E0208 in sema (raw-of only legal inside `#Unsafe`).
    RawOf(Box<TExpr>),
    /// Allocator constructor. Ordinary families carry the rendered runtime call;
    /// Fixed.new carries its comptime byte count to statement emission so the
    /// backing array can be declared immediately before the handle.
    AllocNew {
        ctor: String,
    },
    /// c109 Phase 4: an enum literal `Enum.Variant`, `Variant(args)`, or a
    /// named-payload `Variant { f: v, … }`. The Rust head (`user_Enum::user_Variant`)
    /// is resolved at lowering. `payload` carries the resolved arg form. The subset
    /// admits only scalar/Char payload values, so no clone/box decision is ever
    /// needed (a scalar arg is never borrowed-in-env, never a boxed edge — the AST
    /// path's `emit_boxed_enum_arg` is a no-op for these), keeping emit decision-free.
    EnumLit {
        /// Jet enum type name. Emit spells the Rust path via `tir_enum_lit_prefix`.
        enum_type: String,
        /// Jet variant name.
        variant: String,
        payload: TEnumPayload,
    },
    /// c109 Phase 24: a prelude `JSON` enum construction (`JSON.Null` /
    /// `JSON.Boolean(b)` / `JSON.Number(n)` / `JSON.Text(s)` / `JSON.Array(xs)` /
    /// `JSON.Object(map)`). The JSON enum is FOREIGN: its variants render non-mangled
    /// (`{root}jet_std::Json::Object`, NOT `user_…`), distinct from a user enum's
    /// `EnumLit`. `variant` is the bare variant name (`Object`/`Text`/…). `arg` is the
    /// payload `TExpr` plus the resolved `implicit_clone` flag (sema's `CallArg.flags`,
    /// total) — `true` → `(…).clone()`, reproducing `emit_core_json_lit` (Expression.rs)
    /// byte-for-byte. `JSON.Null` has no arg (`None`). The `{root}jet_std::Json` prefix
    /// is rendered at emit (`cx.root_prefix` is program-level, read there).
    JsonLit {
        variant: String,
        arg: Option<Box<(TExpr, bool)>>,
    },
    /// D-DBDRIVER1: a `DbValue` construction (`DbValue.Int(n)` / `.Float(f)` /
    /// `.Text(s)` / `.Bool(b)` / `.Null`) — the tagged SQL parameter/column value.
    /// Same shape as `JsonLit` (a FOREIGN prelude enum, not a user `EnumLit`), kept
    /// as its own node rather than reusing `JsonLit` because `DbValue` renders to
    /// a DIFFERENT prelude type (`jet_std::DbValue`, not `jet_std::DataTree`) and
    /// has no recursive `Array`/`Object`-style payload to special-case.
    DbValueLit {
        variant: String,
        arg: Option<Box<(TExpr, bool)>>,
    },
    /// c109 Phase 5: a list literal `[a, b, c]`. Lowers to Rust `vec![…]`. Each
    /// element is lowered as-is (the AST path applies no clone/coercion at the
    /// literal site — `emit_expr` per element).
    ListLit(Vec<TExpr>),
    /// D-VARIADIC1: `[a, ...xs, b]` — one growable list built via `extend`.
    ListSpread {
        parts: Vec<ListSpreadPart>,
    },
    /// D-SOA1: a list literal whose element is a `#layout(columnar)` struct `S`.
    /// Lowers to `user_<S>_columns::from_aos(vec![…])` — the elements build the
    /// array-of-structs, then `from_aos` distributes them across the columns.
    /// `columns_ty` is the resolved `user_<S>_columns` Rust path.
    ColumnarListLit {
        columns_ty: String,
        elems: Vec<TExpr>,
    },
    /// D-SOA1: index-read `xs[i]` on a columnar list — gathers the logical `S`
    /// from the columns at `i` (bounds-checked, same panic as `jet_index_vec`).
    /// Lowers to `(base).gather_at(i, file, line)`.
    ColumnarGather {
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: usize,
    },
    /// D-SOA1: a fused `xs[i].field` field-read on a columnar list — reads
    /// directly from the field's column (`jet_index_vec(&(base).user_<field>, i,
    /// …)`), the cache-friendly fast path (no whole-`S` gather).
    ColumnarColumnRead {
        base: Box<TExpr>,
        index: Box<TExpr>,
        column_rust: String,
        line: usize,
    },
    /// c109 Phase 23: a named-tuple literal `(x: 1, y: 2)` (S73/D-SG7). The generated
    /// struct name (`JetTup_<hash>`) and the CANONICAL field order are resolved at
    /// lowering from the literal's sema-attached `Type::Tuple`; each field's value is
    /// reordered to that canonical order (a `(y: 3, x: 4)` literal becomes
    /// `JetTup_…{ user_x: 4, user_y: 3 }`). Reproduces `emit_expr`'s `TupleLit` arm
    /// byte-for-byte — `struct_name { user_<f>: <v>, … }`. `fields` are the already
    /// mangled-name + lowered-value pairs in canonical order.
    TupleLit {
        struct_name: String,
        fields: Vec<(String, TExpr)>,
    },
    /// c109 Phase 5: a map literal `[k: v, …]` or empty `[:]`. The empty form
    /// lowers to `std::collections::BTreeMap::new()` (Rust infers the element
    /// types from the binding context); a non-empty form lowers to the
    /// `{ let mut _m = …; _m.insert((k).clone(), v); … _m }` builder, byte-for-byte
    /// the AST `Expr::MapLit` form.
    MapLit(Vec<(TExpr, TExpr)>),
    /// c109 Phase 5: indexing `coll[i]` (`Expr::Index`). `is_map` is the resolved
    /// `IndexKind` carried TOTALLY from sema (never re-inferred): `true` → the
    /// `jet_index_map` helper, `false` → `jet_index_vec`. `line` is the source line
    /// for the bounds/missing-key panic message, resolved at lowering.
    Index {
        base: Box<TExpr>,
        index: Box<TExpr>,
        is_map: bool,
        line: usize,
    },
    /// D-MEM1 S6: `pool[id]` / `pool[id].field` — a generation-checked slot in a
    /// `Pool<T>`. `mutable` selects the in-place `jet_pool_get_mut` place over the
    /// `jet_pool_get` value clone, so a write or a mutating receiver edits the
    /// stored element instead of a throwaway copy. `field_rust` narrows the place
    /// to one mangled field.
    PoolSlot {
        pool: Box<TExpr>,
        id: Box<TExpr>,
        mutable: bool,
        /// Jet field name when narrowing `pool[id].field`; emit mangles.
        field: Option<String>,
        line: usize,
    },
    /// D-INDEX-HOOK: `mytype[k]` when the type implements `Index`.
    IndexHook {
        type_name: String,
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: usize,
    },
    /// D-SIMD2: `v[i]` lane access on a SIMD lane type. Lowers to the bounds-checked
    /// prelude helper `{root}jet_math_<T>_lane(&v, i, file, line)`.
    MathLaneIndex {
        lane_ty: String,
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: u32,
    },
    /// D-SWIZZLE1: a read swizzle `v.xyz` on a vector/SIMD lane type. `lanes` holds
    /// source indices (x=0…w=3); length 1 → scalar, 2..4 → `VecN` constructor.
    MathSwizzleRead {
        type_name: String,
        recv: Box<TExpr>,
        lanes: Vec<u8>,
    },
    /// c109 Phase 5: an inclusive copy slice `coll[a..b]` (`Expr::Slice`). Lowers
    /// to the `jet_slice_vec` helper. `line` is the source line for the bounds
    /// panic, resolved at lowering.
    Slice {
        base: Box<TExpr>,
        start: Box<TExpr>,
        end: Box<TExpr>,
        line: usize,
    },
    /// c109 Phase 6: the sema-inserted `.clone()` on an owning non-Copy field read
    /// or borrowed value. Also the lowering target for `Expr::Copy` — D-CAP2
    /// (D-MEM1/S4) `copy x`, the one user-typable copy verb — so the compiler's
    /// own internal duplication rewrites and the explicit `copy x` a user writes
    /// share one TIR node (I8). The AST path emits `(recv).clone()`
    /// unconditionally; the TIR carries the lowered receiver and the result type
    /// (the receiver's type).
    Clone(Box<TExpr>),
    /// D-SHAPE-PLACE1=A: a checked local whole/field/index place borrow.
    /// Range places use `ViewNew`/`ViewMutNew` so bounds are checked once.
    Borrow {
        place: Box<TExpr>,
        mutable: bool,
    },
    /// D-MEM1 stage S5 (2026-07-04): `copy d` where `d` is a string-view local
    /// (`Binding.string_view`, a bare `&str` Rust place) — materializes it into
    /// an owned `String` via `.to_string()`. A plain `.clone()` (the `Clone`
    /// node above) would be wrong here: cloning a `&str` hands back another
    /// `&str`, not the owned `String` the copy needs to escape the view's scope.
    MaterializeView(Box<TExpr>),
    /// c109 Phase 6: a user-defined instance method call `recv.method(args)` on a
    /// covered struct/enum. All dispatch facts are resolved at lowering (totality):
    /// `recv` is the lowered receiver (emitted as the AST path emits it — autoref
    /// handles `&self`/`&mut self`/`self`); `method_rust` is the already-resolved
    /// Rust method name (mangled `user_<m>`, or the bare name for a trait-impl
    /// method, decided here from `cx.trait_methods`); each arg carries its
    /// borrow/clone decisions, mirroring `emit_call_args`.
    MethodCall {
        recv: Box<TExpr>,
        method: TMethodRef,
        args: Vec<TCallArg>,
        /// First source argument when it is one plain string literal. The
        /// comptime BuildContext host surface uses this to preserve the
        /// auditability rule for `b.find("glob")`.
        source_first_string_literal: Option<String>,
        /// Hidden source bridge for generic arithmetic trait dispatch. User
        /// methods keep their two-argument surface; primitive impls receive
        /// the Jet operator line through the synthetic trait's default helper.
        operator_line: Option<u32>,
    },
    /// c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)` where `step`
    /// is a `fn(...)` FIELD (not a user method). Emits `(({recv}).{field_rust})({args})`,
    /// byte-for-byte the AST `emit_method_call` fn-field branch (Expression.rs ~L1573).
    /// `field_rust` is the mangled `user_<field>`; args emit PLAINLY (the AST passes
    /// `None` to `emit_call_args` — no param convention, only each arg's own clone flags).
    FnFieldCall {
        recv: Box<TExpr>,
        /// Jet field name; emit mangles.
        field: String,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 7: a STATIC (associated) method call `Type.make(args)`. Resolved
    /// at lowering to `user_<Type>::user_<method>(args)` — `type_prefix` is the
    /// already-resolved Rust type head (`user_<Type>`), `method_rust` the mangled
    /// method name. Mirrors the AST type-name dispatch (Expression.rs ~L1644).
    StaticCall {
        owner: TStaticOwner,
        owner_type: Option<Type>,
        method: TMethodRef,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 9: a built-in collection/string method (`emit_builtin_method`).
    /// The receiver-type dispatch (`expr_jet_ty(receiver)` → Map/List/String) is
    /// resolved at lowering into a concrete `op`, so emit makes no type decision
    /// (I3). The args are lowered as PLAIN expressions — `emit_builtin_method`
    /// emits each arg via a raw `emit_expr`, with NO clone/borrow convention
    /// wrappers (unlike `emit_call_args`), so the TIR carries no `TCallArg` here.
    BuiltinMethod {
        recv: Box<TExpr>,
        op: TBuiltinOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 10: a core/stdlib module call `alias.method(args)` where `alias`
    /// is a core import (`cx.core_imports`). The `(module, method)` dispatch in
    /// `emit_core_call` (Source/Codegen/Expression.rs) is a pure syntactic match on
    /// two already-resolved strings — NO type inference (I3) — so the TIR carries
    /// `module`/`method` as resolved strings and the emitter reproduces the match
    /// byte-for-byte. The args are lowered as plain expressions; the sole generic
    /// conversion fact is D-FIXARR1 widening. Per-arm `&(…)`/`&mut (…)`/move wrappers
    /// stay baked into each emit arm. `cx.root_prefix`/`cx.ffi_crate` are program-level
    /// (read at emit, like Phase 9's `cx.file`), never a per-node decision.
    CoreCall {
        module: String,
        method: String,
        args: Vec<TExpr>,
        source_span: crate::Diagnostics::Span,
        /// D-FIXARR1: per-argument `[T#N]` to `[T]` widening, resolved from the
        /// authoritative Core signature during lowering.
        widen_to_vec: Vec<bool>,
    },
    /// `if`-expression form (S68 / D-SG2). Both arms are value blocks.
    IfExpr {
        cond: Box<TIfCond>,
        then_body: Vec<TStmt>,
        then_value: Box<TExpr>,
        else_body: Vec<TStmt>,
        else_value: Box<TExpr>,
    },
    /// c109 Phase 23: a `#Todo` typed hole (`Expr::Todo`, D-TOOL2, E2-M11). Emits a
    /// diverging `todo!("#Todo at {file}:{line} — expected {ty}")` (Expression.rs
    /// `Expr::Todo`). The `expected_type` is the TOTAL sema fact (sema fills it onto
    /// the AST node); `line` is the source line resolved at lowering. `cx.file` is
    /// program-level (read at emit, like every other `cx.file` use). `todo!()` is
    /// diverging in Rust so it type-checks in any expression position (I1: no unsafe).
    Todo {
        line: usize,
        expected_type: String,
    },
    /// c109 Phase 23: `.raw()` on a distinct type (`Expr::MethodCall { method: "raw" }`,
    /// D-DIST3). The AST `emit_method_call` special-cases this BEFORE any user dispatch:
    /// `({recv}).0` (the newtype's inner field). The receiver is lowered as-is; the
    /// result `ty` is the distinct base type (total, read from `cx.distinct_types`).
    DistinctRaw(Box<TExpr>),
    /// c109 Phase 8: `value(x)` — a present optional (`Some(x)`).
    Present(Box<TExpr>),
    /// c109 Phase 8: bare `null` — an absent optional (`None`).
    Absent,
    /// c109 Phase 8: `Ok(x)` — a success value of `T ? E` (`Ok(x)`).
    Ok(Box<TExpr>),
    /// c109 Phase 8: `Err(e)` — a failure value of `T ? E` (`Err(e)`).
    Err(Box<TExpr>),
    /// c109 Phase 8: the `?` propagation operator (`Expr::Try`). The error
    /// conversion (`convert`) is the TOTAL sema fact (`TryConvert`): a `None` is a
    /// bare propagate, a `Fallible` calls `.to_error()`, a `Typed(fn)` calls the
    /// declared conversion. The frame-trace location (`file`, `line`, `fn_name`) is
    /// resolved at lowering so the emitted `jet_trace_err(…)?` matches the AST path
    /// byte-for-byte (the emitter never reads `cx.current_fn`/`cx.src`).
    Try {
        inner: Box<TExpr>,
        convert: TTryConvert,
        /// Pre-escaped Rust string literal for the source file (`escape_rust_str`).
        file: String,
        line: usize,
        /// Pre-escaped Rust string literal for the enclosing function name.
        fn_name: String,
    },
    /// c109 Phase 8: the `??` fallback operator (`Expr::OrFallback`). `is_option`
    /// is the TOTAL sema fact: `true` → the value is `T?` and lowers to a
    /// `match … { Some(v) => v, None => fb }`; `false` → the value is `T ? E` and
    /// lowers to `match … { Ok(v) => v, Err(_) => fb }`. The fallback is a value or
    /// an early `return` (the panic form is deferred — its `safe_locals_expr`
    /// reproduction is out of subset).
    OrFallback {
        value: Box<TExpr>,
        fallback: TOrFallback,
        is_option: bool,
    },
    /// c109 Phase 8: optional field/chain `base?.member` (`Expr::OptField`). The
    /// `flatten` fact (TOTAL, from sema) picks the combinator: `true` → `.and_then`
    /// (the field is itself optional), `false` → `.map`. Mirrors the AST path's
    /// `(base).clone().{and_then|map}(|__optv| __optv.{member})` exactly.
    OptField {
        base: Box<TExpr>,
        /// Jet member name; emit mangles.
        member: String,
        flatten: bool,
    },
    /// c109 Phase 11: a lambda/closure literal (`Expr::Lambda`). Every capture/
    /// escape/Fn-vs-FnMut decision is the TOTAL sema fact (`Lambda.meta`), resolved
    /// at lowering — emit reads them, never recomputes capture analysis (I3). The
    /// `prep` holds the per-`cloned_capture` `let _jet_cap_<n> = (place).clone();`
    /// prelude (resolved from the *outer* env at lowering, since the cap's source
    /// place is an outer local); `params` is the already-rendered `name[: ty]` list;
    /// `body` is the lowered closure body; `is_move`/`boxed` reproduce the AST path's
    /// `move ` keyword (off `needs_fn_mut`/`escapes`) and `Box::new(…)` (off `escapes`)
    /// wrappers. The whole thing is wrapped in `{ <prep> <closure> }` when `prep` is
    /// non-empty — byte-for-byte `emit_lambda` (Source/Codegen/Expression.rs).
    Lambda(Box<TLambda>),
    /// D-TAG1: a binding-free enum variant/group pattern test (`d == .Fire`,
    /// `d == .Fire.Burn` in expression position). Lowers to `matches!(&subj, pat)`
    /// where `pat` is the same Rust pattern string `emit_match_pattern` uses for
    /// switch arms (group names expand to or-patterns over their leaves).
    PatternMatches {
        subj: Box<TExpr>,
        pattern: TPattern,
    },
    /// D-HOLE1: `Option.lift2(f, a, b)` — apply `f` to both payloads only when both
    /// `a`/`b` are present; `null` otherwise. `f`/`a`/`b` are lowered plainly as
    /// values (`f` via the generic `Expr::Lambda`/fn-value lowering, same as any
    /// other function-typed argument); emit destructures the zipped pair inside a
    /// closure. No user-visible tuple struct — the pair never surfaces as a Jet
    /// value.
    OptionLift2 {
        f: Box<TExpr>,
        a: Box<TExpr>,
        b: Box<TExpr>,
    },
    /// c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/
    /// `find`/`any`/`all`/`sort_by`/`reduce`). The receiver-type + Fn-vs-FnMut
    /// dispatch (`emit_builtin_method`'s closure arms) is resolved at lowering into a
    /// concrete `op`; emit only formats. `recv` is the lowered receiver, `args` the
    /// lowered closure arg(s) (a `reduce` carries the seed first, then the lambda) —
    /// emitted PLAINLY, exactly as `emit_builtin_method`'s `arg(i)`.
    ClosureMethod {
        recv: Box<TExpr>,
        op: TClosureOp,
        args: Vec<TExpr>,
    },
    /// Adapt a named Jet callback to a parallel helper's borrowed host inputs.
    /// Scalar reads dereference the host borrow; owned/non-scalar reads keep it.
    HostBorrowCallback {
        callable: Box<TExpr>,
        params: Vec<Type>,
    },
    /// c109 Phase 12: a numeric predicate / bit-population query
    /// (D-NUMOPS1: `is_nan`/`count_ones`/…) on a numeric receiver. These
    /// carry `recv_type == Some(<numeric name>)` (sema sets it for numeric receivers
    /// — CheckerInfer ~L2248). The receiver width source/target and the
    /// operation is resolved at lowering into a total `TNumericOp`, so emit makes no
    /// type decision (I3). No args (all numeric queries are nullary).
    NumericMethod {
        recv: Box<TExpr>,
        op: TNumericOp,
    },
    /// c109 Phase 28: an overflow opt-out builtin `wrapping(e)`/`saturating(e)`/
    /// `checked(e)` (D-NUMOPS1). The AST `emit_call` (Source/Codegen/Expression.rs
    /// ~L1756) lowers the single integer `Expr::Binary` argument to Rust's matching
    /// method: `(lhs).{prefix}_{op}(rhs)` where `prefix ∈ {wrapping, saturating,
    /// checked}` and `op ∈ {add, sub, mul, div}`. PLAIN operands (no overflow trap).
    /// `prefix` + `op` are resolved at lowering (total facts), emit only assembles.
    OverflowOpt {
        prefix: String,
        op: &'static str,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// c109 Phase 13: a method ON a handle (FileReader/FileWriter/StdinHandle/
    /// Stopwatch/TcpListener/TcpStream/HttpRequest/HttpResponse) — the handle arms of
    /// `emit_builtin_method` (Source/Codegen/Expression.rs). The handle-receiver
    /// dispatch (`rty == Some(Named(<handle>))`) is resolved at lowering into a total
    /// `THandleOp`, so emit makes no type decision (I3). Args are emitted PLAINLY
    /// (`emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    HandleMethod {
        recv: Box<TExpr>,
        op: THandleOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 13: a closure-taking core/stdlib call — `tasks.spawn`,
    /// `http.serve`, `scope.guard`. These are NOT in `core_fixed_sig` and each has a
    /// bespoke emit shape (`emit_core_call`, Source/Codegen/Expression.rs) the plain
    /// `CoreCall` cannot reproduce: `spawn` wraps a `emit_spawn_lambda` (`move |…|`,
    /// NEVER `Box::new`) in `JetTask::spawn(…)`; `serve` (lambda handler) emits
    /// `jet_http_serve(&(addr), <lambda>)`; `guard` emits `jet_scope_guard(<lambda>)`.
    /// The closure body is lowered + rendered at lowering (the lambda is in subset —
    /// Phase 11), so emit only assembles. `kind` selects the bespoke shape.
    CoreClosureCall {
        kind: TCoreClosureKind,
    },
    /// D-TASKSCOPE1=A: `g.all([h1, h2, …])` — join every handle, collect results.
    TaskGroupAll {
        tasks: Box<TExpr>,
    },
    /// D-CONCCOMB1=A: `g.race([h1, h2, …])` — first completed result wins.
    TaskGroupRace {
        tasks: Box<TExpr>,
    },
    /// D-CONCCOMB1=A: `g.any([h1, h2, …])` — first completed result (v1 alias).
    TaskGroupAny {
        tasks: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `g.select()` — start a scoped fluent select builder.
    SelectStart,
    /// D-CONCSELECT1=A: `.recv(ch)` on a select builder.
    SelectRecv {
        builder: Box<TExpr>,
        channel: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `.after(ms: …)` on a select builder.
    SelectAfter {
        builder: Box<TExpr>,
        millis: Box<TExpr>,
        value: Option<Box<TExpr>>,
    },
    /// D-CONCSELECT1=A: `.read(stream)` on a select builder.
    SelectRead {
        builder: Box<TExpr>,
        stream: Box<TExpr>,
    },
    /// D-CONCSELECT1=A: `.wait()` — multiplex until one arm wins.
    SelectWait {
        builder: Box<TExpr>,
    },
    /// c109 Phase 13: a fn-typed-VALUE form. Either a bare function name used as a
    /// value (`Expr::Ident` resolving to a top-level fn) or a call THROUGH a fn-value
    /// (`Expr::CallValue` — `(f)(args)`). A bare fn-name value emits the
    /// `Box::new(move |…| name(…)) as <fn-type>` wrapper (`emit_named_fn_value`,
    /// Source/Codegen/Statement.rs), resolved at lowering into `wrapper`. A
    /// `CallValue` emits `({callee})({args})` with the args lowered PLAINLY (the AST
    /// `Expr::CallValue` passes `None` to `emit_call_args` → no clone/borrow/convention
    /// wrappers). `kind` selects the form.
    FnValue {
        kind: TFnValueKind,
    },
    /// c109 Phase 14: a cross-module function call. The various module-call forms
    /// (qualified `mod.fn()` via `import_mods`, `pub use` re-exports via
    /// `reexport_calls`, inline code modules via `code_modules`, and the unqualified
    /// inline/file imports in `emit_call`) all resolve at LOWERING to a fully-decided
    /// `TModuleCallForm` — emit makes no table lookup or decision (I3). `args` carry
    /// their borrow/clone wrappers, resolved exactly as `emit_call_args` does from the
    /// callee's import signature. `cx.root_prefix` is the only program-level value the
    /// emitter reads (like Phase 9/10's `cx.file`/`cx.root_prefix`), placed exactly
    /// where the AST path prepends it.
    ModuleCall {
        form: TModuleCallForm,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 14: an FFI extern call (`extern rust`/`extern C`). `emit_call`'s
    /// `extern_funcs` arm emits `{ffi_crate}::{wrapper}(args)` with args lowered via
    /// `emit_extern_call_args` (a DISTINCT arg form — a non-scalar `Read` param is
    /// `(…).clone()`, NOT `&(…)`). `wrapper` is the resolved FFI symbol; `args` carry
    /// the resolved per-arg clone decision. `cx.ffi_crate` is program-level (read at
    /// emit, like Phase 10's regex form). I1: an extern call introduces no Rust
    /// `unsafe` by itself — this reproduces the AST emit byte-for-byte, which emits no
    /// `unsafe`.
    ExternCall {
        wrapper: String,
        args: Vec<TExternArg>,
    },
}

/// c109 Phase 14: a resolved cross-module call form. Each variant pre-resolves the
/// path pieces of one `emit_call`/`emit_method_call` module-call arm; emit prepends
/// `cx.root_prefix` exactly where the AST path does (or omits it where the AST does).
pub enum TModuleCallForm {
    /// `import_mods` qualified call (`mod.fn()`) and `reexport_calls` (`pub use`) —
    /// both emit `{root}{rust_mod}::{rust_fn}(args)`. `rust_mod` is the resolved Rust
    /// module name (`user_<stem>`); `rust_fn` is the mangled function name.
    Qualified { rust_mod: String, rust_fn: String },
    /// `code_modules` qualified call (`alias.method()`) and unqualified inline import —
    /// both emit `{root}user_{mangled}(args)` where `mangled` is `alias__method`.
    InlineMangled { mangled: String },
}

/// c109 Phase 14: a resolved FFI extern call argument (see `TExprKind::ExternCall`).
/// `emit_extern_call_args` wraps the value in `(…).clone()` when the arg has an
/// `implicit_clone` flag OR its param is a non-scalar `Read` (resolved here into one
/// total `clone` bool; the `shared_auto_clone`/Arc form is excluded from the subset).
pub struct TExternArg {
    pub value: TExpr,
    pub clone: bool,
}

/// c109 Phase 13: the three closure-taking core-call shapes (see
/// `TExprKind::CoreClosureCall`). Each holds the already-rendered closure string
/// (`spawn_closure` is the distinct `emit_spawn_lambda` form; `serve`/`guard` use the
/// plain `emit_lambda` form) plus, for `serve`, the lowered address arg.
pub enum TCoreClosureKind {
    /// `tasks.spawn(<lambda>)` → `{root}jet_std::JetTask::spawn(<spawn_closure>)`.
    Spawn { spawn_closure: String },
    /// `http.serve(addr, <lambda>)` → `{root}jet_http_serve(&(<addr>), <closure>)`.
    Serve { addr: Box<TExpr>, closure: String },
    /// `scope.guard(<lambda>)` → `{root}jet_scope_guard(<closure>)`.
    Guard { closure: String },
    /// D-TXN3: `<handle>.on_commit(<lambda>)` → `<handle>.on_commit(Box::new(<closure>))`.
    OnCommit { handle: String, closure: String },
    /// D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(<lambda>)` →
    /// `<handle>.on_rollback(Box::new(<closure>))`. Mirror of `OnCommit`.
    OnRollback { handle: String, closure: String },
    /// D-REACT1=B: `reactive.derived(<lambda>)` → `{root}jet_std::JetDerived::new(<closure>)`.
    ReactiveDerived { closure: String },
    /// D-EFFECT-LIFECYCLE1=A: `reactive.effect(<lambda>)` returns a lifecycle handle.
    ReactiveEffect { closure: String, executable: Box<TLambda> },
    /// D-RENDERTGT2=A (c133 M2): reactive UI render loop through the backend seam.
    UiReactiveRender { closure: String, executable: Box<TLambda> },
}

/// c109 Phase 13: the two fn-typed-value forms (see `TExprKind::FnValue`).
pub enum TFnValueKind {
    /// A bare function name used as a value. `wrapper` is the already-rendered
    /// `Box::new(move |…| user_<name>(…)) as <fn-type>` string (`emit_named_fn_value`),
    /// produced at lowering so emit only echoes it.
    NamedFn {
        wrapper: String,
        /// Jet function key for native backends. `None` is a rendered closure
        /// coercion used only by the Rust emitter.
        name: Option<String>,
    },
    /// A call through a fn-value `(f)(args)`. `callee` lowers to its place (a local
    /// of `Type::Fn`, or another fn-value form); args are lowered plainly.
    Call {
        callee: Box<TExpr>,
        args: Vec<TCallArg>,
    },
}

/// c109 Phase 12: a resolved numeric method form, one per `emit_builtin_method`
/// numeric arm (Source/Codegen/Expression.rs). The width source/target and the
/// widening-vs-narrowing branch (which `numeric_conversion` decides from the source
/// width name) are decided ONCE at lowering — the variant encodes the chosen form so
/// emit only formats.
pub enum TNumericOp {
    /// `is_nan`/`is_infinite`/`is_finite` → `({recv}).{method}()` (bool).
    Predicate(String),
    /// `count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros` →
    /// `(({recv}).{method}() as i64)` (Rust returns u32 → widen to Int).
    /// `width` is the receiver's bit width (baked at lowering — TirBridge may
    /// evaluate before locals carry `IntN` types).
    BitCount { method: String, width: u32 },
    /// `origin` on a Float receiver → resolved binding note or `"untracked"`.
    Origin(Option<String>),
    /// A widening / float-targeted / float-sourced conversion → `(({recv}) as {dst})`.
    CastAs { dst_rust: String },
    /// An integer-narrowing conversion → the checked `<{dst}>::try_from(...)` form
    /// returning `Result<T, String>`. `host_kind` is the Cranelift host integer
    /// width tag; `dst_rust`/`dst_spelling` are emit-only Rust spellings.
    TryFrom {
        host_kind: i64,
        dst_rust: String,
        dst_spelling: String,
    },
    /// A float-to-integer conversion. Finite in-range values truncate toward zero;
    /// non-finite and out-of-range values return `Err`.
    FloatToInt {
        host_kind: i64,
        dst_rust: String,
        dst_spelling: String,
        lower: String,
        upper_exclusive: String,
    },
    /// Checked f64/Float to f32/F32 narrowing. Values outside F32's finite
    /// range fail instead of becoming infinity.
    FloatNarrow {
        dst_spelling: String,
    },
    /// `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string`
    /// arm of `emit_builtin_method`, which fires for any receiver type).
    ToShow,
}

/// c109 Phase 11: a resolved closure-taking collection-method op, one per
/// `emit_builtin_method` closure arm (Source/Codegen/Expression.rs). The
/// receiver-type branch (Map vs list vs trait-object list) and the Fn-vs-FnMut
/// branch (off the lambda arg's `needs_fn_mut` meta) are decided ONCE at lowering;
/// the variant encodes the chosen form so emit only formats.
pub enum TClosureOp {
    /// `map` on a list — `jet_list_map((recv).clone(), f)`.
    Map,
    /// `map` on a list whose lambda is FnMut — `jet_list_map_mut((recv).clone(), f)`.
    MapMut,
    /// `filter` — `jet_list_filter((recv).clone(), f)`.
    Filter,
    /// `each` on a list — `jet_list_each((recv).clone(), f)`.
    Each,
    /// `each` on a list whose lambda is FnMut — `jet_list_each_mut((recv).clone(), f)`.
    EachMut,
    /// `each` on a list of trait objects — `jet_list_each_ref(&(recv), f)`.
    EachRef,
    /// `each` on a map — `jet_map_each((recv).clone(), f)`.
    EachMap,
    /// `find` — `jet_list_find((recv).clone(), f)`.
    Find,
    /// `any` — `jet_list_any((recv).clone(), f)`.
    Any,
    /// `any` on `Bag<T>` — `(recv).keys().any(f)`.
    BagAny,
    /// `all` — `jet_list_all((recv).clone(), f)`.
    All,
    /// `sort_by` — `{ jet_list_sort_by(&mut recv, f); }`.
    SortBy,
    /// `reduce` — `jet_list_reduce((recv).clone(), seed, f)`.
    Reduce,
    // D-ITER1: lazy adapter closure methods.
    /// `take_while(f)` — `jet_list_take_while((recv).clone(), f)`.
    TakeWhile,
    /// `skip_while(f)` — `jet_list_skip_while((recv).clone(), f)`.
    SkipWhile,
    /// `flat_map(f)` — `jet_list_flat_map((recv).clone(), f)`.
    FlatMap,
    /// `scan(seed, f)` — `jet_list_scan((recv).clone(), seed, f)`.
    Scan,
    /// `position(f)` — `jet_list_position((recv).clone(), f)`.
    Position,
    /// `min_by(f)` — `jet_list_min_by((recv).clone(), f)`.
    MinBy,
    /// `max_by(f)` — `jet_list_max_by((recv).clone(), f)`.
    MaxBy,
    /// `fold(init, f)` — `jet_list_fold((recv).clone(), init, f)`.
    Fold,
    /// `group_by(f)` — `jet_list_group_by((recv).clone(), f)`.
    GroupBy,
    /// `count_by(f)` — `jet_list_count_by((recv).clone(), f)`.
    CountBy,
    /// `partition(f)` — inline emit; struct name embedded. `TupleStruct` is `JetTup_<hash>`.
    Partition { tuple_struct: String },
    // D-FAILCOMP1: failure-aware adapters.
    /// `filter_map(f)` — `jet_list_filter_map((recv).clone(), f)`.
    FilterMap,
    // D-PARCAPTURE1=D: explicit `para_` adapters.
    ParaMap,
    ParaFilter,
    ParaPartition { tuple_struct: String },
    ParaFold,
    // D-HOLE1: Option combinators.
    /// `map` on `T?` — `(recv).as_ref().map(f)` (Rust's native `Option::map`, no
    /// prelude helper needed; `.as_ref()` supplies plain callback read access).
    OptionMap,
    // D-DYNARRAY1: `View<T>` read-only closure methods. `recv` is already a
    // `&[T]` borrow (see `Context::rust_type`'s `View` arm) — NOT `.clone()`d
    // into an owned `Vec` first, unlike every list closure op above; that
    // clone would silently defeat the zero-copy point of `.view(...)`.
    /// `view.fold(init, f)` — `jet_view_fold((recv), init, f)`.
    ViewFold,
    /// `view.map(f)` — `jet_view_map((recv), f)` (map-to-owned: returns `[R]`).
    ViewMap,
}

/// c109 Phase 11: a fully-resolved lambda/closure, every fact carried total from
/// `Lambda.meta`. `prep` is the rendered clone-capture prelude (`let _jet_cap_<n> =
/// (place).clone();\n    ` per cloned capture); `params` the rendered `name[: ty]`
/// param list; `body` the rendered closure body string (an expression body, or a
/// `{ … }` block) — rendered at lowering from the lowered body so emit stays a pure
/// wrapper; `is_move`/`boxed` reproduce the AST wrappers.
pub struct TLambda {
    pub prep: String,
    pub params: Vec<String>,
    pub body: String,
    /// Target-neutral executable body. Backends must consume this, never the
    /// Rust-rendered `body` compatibility field.
    pub executable: TLambdaBody,
    /// Unmangled source parameter names for non-Rust targets.
    pub source_params: Vec<String>,
    /// Stable native symbol and resolved signature for noncapturing JIT calls.
    pub jit_name: String,
    pub param_types: Vec<Type>,
    pub ret: Option<Type>,
    pub is_move: bool,
    pub boxed: bool,
    pub arc: bool,
}

pub enum TLambdaBody {
    Expr(Box<TExpr>),
    Block(Vec<TStmt>),
}

/// c109 Phase 8: the resolved error-conversion of a `?`, mirroring `AST::TryConvert`
/// (the total sema fact). Carried onto the TIR so the emitter never re-derives it.
pub enum TTryConvert {
    /// Error types match — bare `jet_trace_err(x, …)?`.
    None,
    /// Source error implements `Fallible` — `.map_err(|e| e.to_error())` (D-ERR2).
    Fallible,
    /// Declared `impl Source -> Target` conversion — `.map_err(<fn>)` (D-ERR-CONV);
    /// holds the mangled Rust conversion-function name.
    Typed(String),
    /// D-UNIONTYPE1=A: wrap the error into a compiler-generated union enum.
    WidenUnion { enum_name: String, tag: String },
}

/// c109 Phase 8: the resolved right-hand side of a `??` fallback (`AST::OrFallback`).
/// `Value` is an expression; `Return` is an early `return [expr]` from the enclosing
/// function. c109 Phase 15 / #776: `Panic` carries structured message + `TPanicLoc`;
/// emit alone formats `jet_panic_rich` (I3: no pre-rendered Rust blob on the node).
pub enum TOrFallback {
    Value(Box<TExpr>),
    Return(Option<Box<TExpr>>),
    /// Structured panic stop — emit formats `jet_panic_rich`.
    Panic {
        msg: Box<TExpr>,
        loc: TPanicLoc,
    },
    /// D-ORRETURN-ERG1=B: `?? break` — loop exit.
    Break,
    /// D-ORRETURN-ERG1=B: `?? next` — loop skip.
    Continue,
    /// D-LOOPLABEL3=A: `?? label.break()`.
    BreakLabel(String),
    /// D-LOOPLABEL3=A: `?? label.next()`.
    ContinueLabel(String),
}

pub enum TStrPart {
    Lit(String),
    Interp(TExpr, crate::AST::StrFormat),
}

/// c109 Phase 4/16: the resolved payload shape of an enum literal.
pub enum TEnumPayload {
    /// `Enum.Variant` — no payload, emits just the prefix.
    Unit,
    /// `Variant(a, b, …)` — positional payload values, emitted as `prefix(a, b)`.
    Positional(Vec<TEnumArg>),
    /// `Variant { f: v, … }` — named payload, emitted as `prefix { f: v, … }`.
    /// Each field's Rust name is already mangled at lowering.
    Named(Vec<(String, TEnumArg)>),
}

/// c109 Phase 16: one enum-literal payload argument with its resolved
/// borrow/box decisions. Reproduces `emit_boxed_enum_arg` (Expression.rs) as a
/// TOTAL fact decided at lowering: a non-scalar payload field whose value is a
/// borrowed-in-env ident gets `(…).clone()`; a recursive (`boxed_edge`) payload
/// gets `Box::new(…)`. For a scalar payload from a non-borrowed value both are
/// false (the Phase-4 no-op case), so emit is byte-identical.
pub struct TEnumArg {
    pub value: TExpr,
    /// Wrap the value in `(…).clone()` (non-scalar payload, borrowed-in-env arg).
    pub clone: bool,
    /// Wrap (after the clone) in `Box::new(…)` — a recursive boxed edge.
    pub boxed: bool,
}

/// c109 Phase 9: a resolved built-in collection/string method op. Each variant is
/// one emit form from `emit_builtin_method` (Source/Codegen/Expression.rs). The
/// receiver-type dispatch (`rty = expr_jet_ty(receiver)` → Map vs List vs String)
/// is decided ONCE at lowering — the variant encodes the chosen branch, so emit
/// only formats. Line numbers (for the bounds/remove panic frames) are resolved at
/// lowering; `cx.file`/`cx.root_prefix` are read at emit (program-level, not a
/// per-node decision). Args are emitted plainly (no clone/borrow wrappers), exactly
/// as `emit_builtin_method`'s `arg(i)` does.
pub enum TBuiltinOp {
    /// `len` on a `String` → `jet_char_len(&(recv))` (char count, not byte len).
    LenString,
    /// `len` on a list/map → `(recv).len() as i64`.
    LenList,
    /// `is_empty()` on a list/map/string → `(recv).is_empty()` (Bool).
    IsEmpty,
    /// `push(x)` → `(recv).push(a0)`.
    Push,
    /// `pop()` → `(recv).pop()`.
    Pop,
    /// `add(k, v)` on a map → displaced value, if any.
    InsertMap,
    /// `add_new(k, v)` on a map → false without overwriting an existing key.
    AddNewMap,
    /// `insert(i, v)` on a list → `(recv).insert(a0 as usize, a1)`.
    InsertList,
    /// `remove(k)` on a map → `(recv).remove(&(a0).clone())`.
    RemoveMap,
    /// `remove(i)` on a list → `jet_list_remove(&mut (recv), a0, file, line)`.
    RemoveList {
        line: usize,
    },
    /// `get(k)` on a map → `(recv).get(&(a0).clone()).cloned()`.
    GetMap,
    /// `get(i)` on a list → `(recv).get(a0 as usize).cloned()`.
    GetList,
    /// `first()` → `(recv).first().cloned()`.
    First,
    /// `last()` → `(recv).last().cloned()`.
    Last,
    /// `contains(x)` → `(recv).contains(&a0)` (list element / String substring).
    Contains,
    /// `index_of(x)` → `(recv).iter().position(|x| *x == a0).map(|i| i as i64)`.
    IndexOf,
    /// `reverse()` → `(recv).reverse()`.
    Reverse,
    /// `sort()` (no comparator) → `(recv).sort()`.
    Sort,
    /// `join(sep)` → `(recv).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join((a0).as_str())`.
    JoinSep,
    Sum { float: bool },
    Product { float: bool },
    Min { float: bool },
    Max { float: bool },
    Flatten,
    Intersperse,
    Unzip {
        tuple_struct: String,
    },
    /// `clear()` → `(recv).clear()`.
    Clear,
    /// `chars()` → `(recv).chars().collect::<Vec<char>>()`.
    Chars,
    /// `bytes()` → `{root}jet_string_bytes(&(recv))`.
    Bytes,
    /// `trim()` → pinned `jet_unicode_trim(&(recv))`.
    Trim,
    /// `split(sep)` → `jet_iter_string_split(&(recv), &a0)` (lazy `JetIter<String>`).
    Split,
    /// c97/D-STRPARSE1: `lines()` → `{root}jet_string_lines(&(recv))`.
    Lines,
    /// c97/D-STRPARSE1: `Int.parse(text)` → checked integer parse.
    ParseInt,
    /// c97/D-STRPARSE1: `Float.parse(text)` → checked floating-point parse.
    ParseFloat,
    /// `starts_with(s)` → `(recv).starts_with(&a0)`.
    StartsWith,
    /// `ends_with(s)` → `(recv).ends_with(&a0)`.
    EndsWith,
    /// `replace(from, to)` → `(recv).replace(&a0, &a1)`.
    Replace,
    /// `to_upper()` → pinned `jet_unicode_upper(&(recv))`.
    ToUpper,
    /// `to_lower()` → pinned `jet_unicode_lower(&(recv))`.
    ToLower,
    /// `repeat(n)` → `(recv).repeat(a0 as usize)`.
    Repeat,
    /// `slice(a, b)` → `jet_string_slice(&(recv), a0, a1, file, line)`.
    Slice {
        line: usize,
    },
    /// D-STR-AFTER1: `after(sep)` → `jet_string_after(&(recv), &a0)`. Substring
    /// strictly after the first `sep`; `sep` absent → the whole original string
    /// (mirrors `.replace`'s no-match-is-identity convention).
    After,
    /// D-STR-AFTER1: `before(sep)` → `jet_string_before(&(recv), &a0)`. Substring
    /// strictly before the first `sep`; `sep` absent → the whole original string.
    Before,
    /// D-MEM1 stage S5: the zero-copy sibling of `Trim`, used ONLY as the init
    /// of a `Binding` sema marked `string_view` (E2307 proves it can't outlive
    /// its owner) → `jet_string_trim_view(&(recv))`, a borrowed `&str`, no
    /// `.to_string()`.
    TrimView,
    /// D-MEM1 stage S5: the zero-copy sibling of `After`, same `string_view`
    /// gate → `jet_string_after_view(&(recv), &a0)`.
    AfterView,
    /// D-MEM1 stage S5: the zero-copy sibling of `Before`, same `string_view`
    /// gate → `jet_string_before_view(&(recv), &a0)`.
    BeforeView,
    /// `keys()` → `(recv).keys().cloned().collect::<Vec<_>>()`.
    Keys,
    /// `values()` → `(recv).values().cloned().collect::<Vec<_>>()`.
    Values,
    /// `contains_key(k)` → `(recv).contains_key(&a0)`.
    ContainsKey,
    /// `to_string()` (on a String receiver) → `(recv).jet_show()`.
    ToString,
    /// D-REGEXENGINE1=A: `Match.group(n)` → `(recv).group(a0)`.
    MatchGroup,
    // D-ITER1: non-closure lazy adapters.
    /// `take(n)` → `jet_list_take((recv).clone(), a0)`.
    Take,
    /// `skip(n)` → `jet_list_skip((recv).clone(), a0)`.
    Skip,
    /// D-ITERTOOLS1=A: `Iter.to_list()` / `.collect()` → owned `[T]`.
    IterToList,
    IterCollect,
    /// `step_by(n)` → `jet_list_step_by((recv).clone(), a0)`.
    StepBy,
    /// `dedup()` → `jet_list_dedup((recv).clone())`.
    Dedup,
    /// `chunks(n)` → `jet_list_chunks((recv).clone(), a0)`.
    Chunks,
    /// `windows(n)` → `jet_list_windows((recv).clone(), a0)`.
    Windows,
    /// `indexed()` → inline emit building `JetTup_<hash>` struct. The struct name
    /// is embedded here at lowering so emit is a pure formatter.
    Indexed {
        tuple_struct: String,
    },
    /// D-RANGE-EXCL1=C: `indexes()` → `Iter<Int>` of every valid index.
    Indexes,
    /// `zip([U])` → inline emit building `JetTup_<hash>` struct.
    Zip {
        tuple_struct: String,
    },
    // D-HOLE1: Option combinators.
    /// `zip(U?)` on `T?` → `(recv).clone().zip((a0).clone()).map(|(x,y)| Struct{…})`
    /// (Rust's native `Option::zip`, wrapped into the named-tuple struct). `elem_ty`
    /// (`(a: T, b: U)`) is the resolved pair type — carried so the call's own `TExpr`
    /// type is total (not the generic table's placeholder), even though it's rarely
    /// load-bearing in emit (a binding carries sema's `b.ty`).
    OptionZip {
        tuple_struct: String,
        elem_ty: Type,
    },
    // D-COLLBREADTH1=A: Set<T> operations.
    /// `Set.from([...])` — recv is the list: `(recv).into_iter().collect::<std::collections::HashSet<_>>()`.
    SetFrom,
    /// `set.add(v)` → `(recv).insert(a0)` (HashSet::insert; bool result discarded).
    SetInsert,
    /// `set.remove(v)` → `(recv).remove(&a0)` (bool result discarded).
    SetRemove,
    /// `set.to_list()` → `(recv).iter().cloned().collect::<Vec<_>>()`.
    SetToList,
    /// `set.union(other)` → `(recv).union(&(a0)).cloned().collect::<std::collections::HashSet<_>>()`.
    SetUnion,
    SortedSetFrom,
    SortedSetInsert,
    SortedSetRemove,
    SortedSetToList,
    SortedSetUnion,
    PriorityQueueFrom,
    PriorityQueuePeek,
    PriorityQueueToSortedList,
    LruPut,
    LruAddNew,
    LruGet,
    LruCapacity,
    LruKeys,
    BitSetAdd,
    BitSetRemove,
    BitSetCount,
    BitSetToList,
    BitSetNew,
    ByteBufferNew,
    ByteBufferFrom,
    ByteBufferWrite {
        method: String,
    },
    ByteBufferToBytes,
    // D-TAG1: Bag<T> counted multiset (HashMap-backed).
    BagAdd,
    BagRemove,
    BagHas,
    BagCount,
    BagLen,
    // D-COLLBREADTH1=A: Deque<T> operations.
    /// `deque.push_front(v)` → `(recv).push_front(a0)`.
    DequePushFront,
    /// `deque.push_back(v)` → `(recv).push_back(a0)`.
    DequePushBack,
    /// `deque.pop_front()` → `(recv).pop_front()` (returns `Option<T>`).
    DequePopFront,
    /// `deque.pop_back()` → `(recv).pop_back()` (returns `Option<T>`).
    DequePopBack,
    /// `deque.peek_front()` → `(recv).front().cloned()` (returns `Option<T>`).
    DequePeekFront,
    /// `deque.peek_back()` → `(recv).back().cloned()` (returns `Option<T>`).
    DequePeekBack,
    // D-FAILCOMP1: failure-aware list adapters.
    /// `try_collect()` on `[Result<T,E>]` → `jet_list_try_collect((recv).clone())`.
    TryCollect,
    // D-DYNARRAY1: `View<T>` — a zero-copy window (`&[T]`) over a list's
    // backing storage. The read-only accessor methods (`len`/`is_empty`/
    // `get`/`first`/`last`/`contains`/`index_of`) reuse `LenList`/`IsEmpty`/
    // `GetList`/`First`/`Last`/`Contains`/`IndexOf` above unchanged — every
    // one of those emits a plain Rust slice/`.get`/`.first`/… call that a
    // `&[T]` receiver satisfies exactly as a `Vec<T>` does.
    /// `list.view(a..b)` → `jet_view_new(&(recv), a0, a1, file, line)`.
    ViewNew {
        line: usize,
    },
    /// `&list[a..b]` → `jet_view_mut_new(&mut recv, a, b, file, line)`.
    ViewMutNew {
        line: usize,
    },
}

/// c109 Phase 13: a resolved handle-method op, one per handle arm of
/// `emit_builtin_method` (Source/Codegen/Expression.rs). The handle-receiver branch
/// (keyed on `rty == Some(Named(<handle>))`) is decided ONCE at lowering from the
/// total `recv_type` — emit only formats. Args are emitted plainly (raw `arg(i)`).
/// `{root}` denotes `cx.root_prefix` (program-level, read at emit).
pub enum THandleOp {
    /// D-SHAPE-DURATION1=A: checked type-owned runtime constructor.
    DurationNew {
        unit: &'static str,
        float: bool,
    },
    /// FileReader: `read_line()` → `{root}jet_std_file_reader_read_line(&mut (recv))`.
    FileReaderReadLine,
    /// FileWriter: `write_line(s)` → `{root}jet_std_file_writer_write_line(&mut (recv), &(a0))`.
    FileWriterWriteLine,
    /// FileWriter: `flush()` → `{root}jet_std_file_writer_flush(&mut (recv))`.
    FileWriterFlush,
    /// D-ENCSTREAM-SURFACE1=A: JSON pull reader/writer lifecycle.
    JSONReaderNext,
    JSONWriterWrite,
    JSONWriterFlush,
    JSONWriterFinish,
    JSONLReaderNext,
    JSONLWriterWrite,
    JSONLWriterFlush,
    JSONLWriterFinish,
    CSVReaderNext,
    /// D-DATAFLOW1=A: typed pull `DataStream<T>.next()` → `T? ? DataError`.
    DataStreamNext,
    XMLReaderNext,
    XMLWriterWrite,
    XMLWriterFlush,
    XMLWriterFinish,
    CSVWriterWrite,
    CSVWriterFlush,
    CSVWriterFinish,
    CBORReaderNext,
    CBORWriterWrite,
    CBORWriterFlush,
    CBORWriterFinish,
    /// StdinHandle: `read_line()` → `{root}jet_std_io_stdin_read_line(&mut (recv))`.
    StdinReadLine,
    /// Stdout/Stderr: stream writes and facts (D-COREIO1=A).
    StdoutWrite,
    StdoutWriteLine,
    StdoutWriteBytes,
    StdoutFlush,
    StdoutIsTty,
    StderrWrite,
    StderrWriteLine,
    StderrWriteBytes,
    StderrFlush,
    StderrIsTty,
    /// Stopwatch: `elapsed_millis()` → `{root}jet_stopwatch_elapsed_millis(&(recv))`.
    StopwatchElapsedMillis,
    /// D-DET1 Clock: `now()` → `{root}jet_clock_now(&(recv))` (current ms, no advance).
    ClockNow,
    /// D-DET1 Clock: `tick(ms)` → `{root}jet_clock_tick(&mut (recv), a0)` (advance + read).
    ClockTick,
    /// D-DET-CAPAPI Clock: `advance(to_ms)` → `{root}jet_clock_advance(&mut (recv), a0)` (absolute set + read).
    ClockAdvance,
    /// D-DET-CAPAPI Clock: `wait(d)` → `{root}jet_clock_wait(&mut (recv), &(a0))` (advance by a Duration + read).
    ClockWait,
    /// D-DET1 Rng: `int(lo, hi)` → `{root}jet_rng_int(&mut (recv), a0, a1)` (draw in [lo,hi]).
    RngInt,
    /// D-DET1 Rng: `float()` → `{root}jet_rng_float(&mut (recv))` (draw in [0,1)).
    RngFloat,
    /// D-RANDOMDIST1 Rng: `float_range(lo, hi)` → `{root}jet_rng_float_range(&mut (recv), a0, a1)`.
    RngFloatRange,
    /// D-DET-CAPAPI Rng: `bool()` → `{root}jet_rng_bool(&mut (recv))` (coin draw).
    RngBool,
    /// D-RANDOMDIST1 Rng: `bool(p)` → `{root}jet_rng_bool_p(&mut (recv), a0)`.
    RngBoolP,
    /// D-RANDOMDIST1 Rng: `normal(mean, stddev)` → `{root}jet_rng_normal(&mut (recv), a0, a1)`.
    RngNormal,
    /// D-RANDOMDIST1 Rng: `exponential(lambda)` → `{root}jet_rng_exponential(&mut (recv), a0)`.
    RngExponential,
    /// D-RANDOMDIST1 Rng: `bytes(n)` → `{root}jet_rng_bytes(&mut (recv), a0)`.
    RngBytes,
    /// D-RANDOMDIST1 Rng: `split()` → `{root}jet_rng_split(&mut (recv))`.
    RngSplit,
    /// D-DET-CAPAPI Rng: `pick(list)` → `{root}jet_rng_pick(&mut (recv), &(a0))` (uniform `T?`).
    RngPick,
    /// D-RANDOMDIST1 Rng: `weighted_pick(list, weights)` → `{root}jet_rng_weighted_pick(&mut (recv), &(a0), &(a1))`.
    RngWeightedPick,
    /// D-RANDOMDIST1 Rng: `sample(list, k)` → `{root}jet_rng_sample(&mut (recv), &(a0), a1)`.
    RngSample,
    /// D-DET-CAPAPI Rng: `shuffle(&list)` → `{root}jet_rng_shuffle(&mut (recv), &mut (a0))` (in-place).
    RngShuffle,
    /// D-SOLVER-LIB1=A: `Solver.new(seed)` → `{root}jet_solver_new(seed)`.
    SolverNew,
    /// D-SOLVER-LIB1=A: `solver.require(ok)` → `{root}jet_solver_require(&mut solver, ok)`.
    SolverRequire,
    /// D-SOLVER-LIB1=A: `solver.failure_count()` → `{root}jet_solver_failure_count(&solver)`.
    SolverFailureCount,
    /// D-SOLVER-LIB1=A: `solver.status()` → `{root}jet_solver_status(&solver)`.
    SolverStatus,
    GameSceneNew,
    GameReplayRecord,
    GameBackendHeadless,
    GameSceneOnFrame,
    GameSceneComponent,
    GameSceneQuery,
    GameAssetsImage,
    GameAssetsSound,
    GameInputBind,
    GameInputPressed,
    /// D-SHAPE-DURATIONCONVERT1=A: checked whole-unit read.
    DurationIn {
        unit: Option<&'static str>,
    },
    /// D-BIGINT1 / D-DECIMAL1: instance methods on precise numeric types.
    PreciseMethod {
        type_name: String,
        method: String,
    },
    /// TcpListener: `accept()` → `{root}jet_net_tcp_accept(&(recv))`.
    TcpListenerAccept,
    /// TcpListener: `local_addr()` → `{root}jet_net_listener_local_addr(&(recv))`.
    TcpListenerLocalAddr,
    /// TcpStream: `read()` → `{root}jet_net_tcp_read(&mut (recv))`.
    TcpStreamRead,
    /// TcpStream: `write(s)` → `{root}jet_net_tcp_write(&mut (recv), &(a0))`.
    TcpStreamWrite,
    /// TcpStream: `peer_addr()` → `{root}jet_net_tcp_peer_addr(&(recv))`.
    TcpStreamPeerAddr,
    /// TcpStream: `local_addr()` → `{root}jet_net_tcp_local_addr(&(recv))`.
    TcpStreamLocalAddr,
    /// TcpStream: `close()` → `{ drop(recv); }`.
    TcpStreamClose,
    TcpStreamReadBytes,
    TcpStreamReadText,
    TcpStreamWriteBytes,
    TcpStreamWriteAllBytes,
    TcpStreamWriteText,
    TcpStreamShutdown,
    TcpStreamReady,
    UdpSocketReady,
    UdpSocketClose,
    UdpSocketReceiveDeadline,
    UdpSocketSendToDeadline,
    UnixListenerAcceptDeadline,
    UnixStreamReadDeadline,
    UnixStreamWriteAllDeadline,
    UnixStreamReady,
    UnixStreamClose,
    UnixStreamSetTimeout,
    TlsStreamReadDeadline,
    TlsStreamWriteAllDeadline,
    TlsStreamReady,
    TlsStreamClose,
    TlsStreamCloseWrite,
    TlsStreamPeerIdentity,
    TlsClientConfigDefault,
    TlsClientConfigWithAlpn,
    TlsRootCertificatesFromPem,
    TlsClientIdentityFromPem,
    TlsClientConfigWithTrust,
    TlsClientConfigWithIdentity,
    TlsClientConfigWithVersionBounds,
    HttpClientNew,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `alloc(v)` → `(recv).alloc(a0)` (hands back a
    /// `&mut T` view into the allocator's storage). The arg is emitted plainly.
    AllocAlloc,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `reset()` → `(recv).reset()`.
    AllocReset,
    /// c109 Phase 20: HttpRequest `method()`/`path()`/`body()` → `(recv).<field>.clone()`.
    HttpReqField(&'static str),
    /// c109 Phase 20: HttpRequest `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HttpReqHeader,
    /// c109 Phase 20: HttpRequest `param(name)` → `{root}jet_http_request_param(&(recv), &(a0))`.
    HttpReqParam,
    HttpReqTrailers,
    /// c109 Phase 20: HttpResponse `status()`/`body()` → `(recv).<field>.clone()`.
    HttpRespField(&'static str),
    /// c109 Phase 20: HttpResponse `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HttpRespHeader,
    HttpRespTrailers,
    /// D-ARGS1: ArgsSpec `.flag(name, help)` → `(recv).flag(&a0, &a1)` → `JetArgsSpec`.
    ArgsSpecFlag,
    ArgsSpecFlagShort,
    /// D-ARGS1: ArgsSpec `.option(name, help, meta)` → `(recv).option(&a0, &a1, &a2)` → `JetArgsSpec`.
    ArgsSpecOption,
    ArgsSpecOptionShort,
    ArgsSpecOptionDefault,
    ArgsSpecOptionEnv,
    ArgsSpecOptionInt,
    ArgsSpecOptionFloat,
    ArgsSpecOptionChoice,
    ArgsSpecRepeat,
    ArgsSpecRequiredOption,
    /// D-ARGS1: ArgsSpec `.positional(name, help)` → `(recv).positional(&a0, &a1)` → `JetArgsSpec`.
    ArgsSpecPositional,
    ArgsSpecSubcommand,
    ArgsSpecVersion,
    ArgsSpecCompletion,
    /// D-ARGS1: ArgsSpec `.help()` → `(recv).help()` → `String`.
    ArgsSpecHelp,
    /// D-ARGS1: ArgsSpec `.parse(argv)` → `{root}jet_args_parse(&(recv), &(a0))` → `Result<JetParsedArgs, String>`.
    ArgsSpecParse,
    /// D-ARGS1: ParsedArgs `.flag(name)` → `{root}jet_args_flag(&(recv), &(a0))` → `bool`.
    ParsedArgsFlag,
    /// D-ARGS1: ParsedArgs `.option(name)` → `{root}jet_args_option(&(recv), &(a0))` → `Option<String>`.
    ParsedArgsOption,
    ParsedArgsOptionInt,
    ParsedArgsOptionFloat,
    ParsedArgsOptions,
    ParsedArgsSubcommand,
    /// D-ARGS1: ParsedArgs `.positional(n)` → `{root}jet_args_positional(&(recv), a0)` → `Option<String>`.
    ParsedArgsPositional,
    /// D-PROCESS1: ProcessSpec builder/run/spawn methods.
    ProcessSpecMethod {
        method: String,
    },
    /// D-PROCESS1: ProcessChild control/streaming methods.
    ProcessChildMethod {
        method: String,
    },
    /// D-PROCESS1=A: `child.stdin.write(text)` →
    /// `{root}jet_process_stdin_write(&(recv), &(a0))` → `Result<(), IOError>`.
    ProcessStdinWrite,
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s `Value` handle — plain
    /// inherent-method passthrough, same shape as `ArgsSpecHelp`.
    ReflectValueTypeName,
    ReflectValueDisplay,
    ReflectValueFields,
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x).fields()`'s `Field` handle.
    ReflectFieldName,
    ReflectFieldValue,
    /// c109 Phase 21: Task `join()` → `(recv).join()` (the no-arg `join` arm of
    /// `emit_builtin_method`, Source/Codegen/Expression.rs ~L967 — shared with the dead
    /// list no-arg join, but here it's the JetTask method). Returns the task's value `T`.
    TaskJoin,
    /// c109 Phase 21: Task `detach()` → `{ let _detach = (recv); }` (D-DETACH1 —
    /// fire-and-forget; drops the JoinHandle). Returns unit.
    TaskDetach,
    /// D-COROUTINE1=A: Task control-plane pause request (thread-runtime v1: metadata only).
    TaskPause,
    /// D-COROUTINE1=A: Task control-plane resume request (thread-runtime v1: metadata only).
    TaskResume,
    /// D-COROUTINE1=A: Task control-plane cancel request (thread-runtime v1: metadata only).
    TaskCancel,
    /// D-COROUTINE1=A: Task control-plane trace string.
    TaskTrace,
    /// c109 Phase 21 / D-TUPLE-DESTRUCT1: Receiver `receive()` → `(recv).receive()` →
    /// `Result<T, Closed>`.
    ChannelReceive,
    /// c109 Phase 21: Sender `send(v)` → `(recv).send(a0)`. Returns unit.
    SenderSend,
    /// c109 Phase 25: HttpRouter `get`/`post`/`put`/`delete` route registration
    /// (D-ROUTE1=A). Emits `{root}jet_http_router_register(&mut (recv), "<VERB>".to_string(),
    /// <path>, <handler>)` where `<path>` is the lowered first arg (args[0]) and `<handler>`
    /// is a pre-rendered boxed-closure string (`emit_router_handler` reproduction, resolved
    /// at lowering). `verb` is the uppercase HTTP method literal.
    HttpRouterRegister {
        verb: &'static str,
        handler: String,
        file: String,
        line: usize,
    },
    /// D-SIMD2 / D-LINALG1: an INSTANCE method on a built-in math value type. Emits
    /// the prelude free function `{root}jet_math_<type>_<method>(&(recv), <args>)`
    /// (e.g. `jet_math_Vec3_dot(&(v), w)`, `jet_math_F32x4_sum(&(v))`). `reduce`
    /// carries the validated marker op so the right fold function is named.
    MathMethod {
        type_name: String,
        method: String,
        reduce_op: Option<String>,
    },
    /// D-REACT1=B: `Signal.get()`/`Derived.get()` → `(recv).get()` (reads + tracks).
    ReactiveGet,
    /// D-REACT1=B: `Signal.set(v)` → `(recv).set(<arg0>)` (writes + notifies).
    ReactiveSet,
    /// D-EFFECT-LIFECYCLE1=A: Effect.unsubscribe()/is_active().
    ReactiveEffectMethod {
        method: String,
    },
    /// D-EVENT1=D: Event/Hook/Subscription/EventScope/EventTrace runtime methods.
    EventMethod {
        method: String,
    },
    /// D-WATCH-SCOPE1: WatchHandle/WatchSet polling and callback methods.
    WatchMethod {
        method: String,
    },
    /// D-HONESTNUM1=A: `Measurement<Float>` arithmetic / accessors.
    /// `.add(m)/.sub(m)/.mul(m)/.div(m)` → `(recv).<method>(a0)` → `JetMeasurement<f64>`.
    /// `.value()/.uncertainty()` → `(recv).<method>()` → `f64`.
    MeasurementMethod {
        method: String,
    },
    /// D-LAYOUT1 / D-LAYOUT-GATES1: an instance method on `LayoutHandle`
    /// (`.h`/`.v`/`.value`/`.suggest`/`.is_feasible`/`.conflict`) or
    /// `Constraint` (`.required`/`.strong`/`.medium`/`.weak`). Every Jet
    /// method name IS the `jet_layout` Rust method name (no renaming table
    /// needed, unlike `MathMethod`) — pure passthrough: `(recv).method(args)`.
    LayoutMethod {
        method: String,
    },
    /// D-PENDING1=B: `Loadable<T,E>` predicate / accessor methods.
    /// `.is_loading()/.is_loaded()/.is_failed()/.is_idle()` → `(recv).<method>()`.
    /// `.loaded()` → `(recv).loaded()` → `Option<T>`.
    /// `.or_else(default)` → `(recv).or_else(a0)` → `T`.
    LoadableMethod {
        method: String,
    },
    /// D-SHAPE-CTORVERB1=C: generic `ExpiringValue<T>` fallible accessors.
    ExpiringMethod {
        method: String,
    },
    /// D-APPROX1=A: method call on a sketch data structure (HyperLogLog/TDigest/CMS/ReservoirSampler).
    SketchMethod {
        sketch: String,
        method: String,
    },
    /// D-TIMEDEPTH1=A: method call on a civil-time type (Date/DateTime).
    CivilTimeMethod {
        kind: String,
        method: String,
    },
    /// D-URL1=A: method call on Url/Mime value types.
    UrlMimeMethod {
        kind: String,
        method: String,
    },
    /// D-EMAIL-SMTP-SURFACE1=A: Message envelope replacement.
    EmailMethod {
        method: String,
    },
    /// D-REGEXENGINE1=A: method call on Regex/Match value types.
    RegexMethod {
        kind: String,
        method: String,
    },
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP client type (HttpRequest/HttpResponse).
    HttpClientMethod {
        kind: String,
        method: String,
    },
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP server type (HttpMux/HttpRequest/HttpResponse).
    HttpServerMethod {
        kind: String,
        method: String,
    },
    /// D-SERDE-ACCESS=B: `DataTree.field(name)` → `(recv).field(&a0)`.
    DataTreeField,
    /// D-SERDE-ACCESS=B: `DataTree.at(i)` → `(recv).at(a0)`.
    DataTreeAt,
    /// D-SERDE-ACCESS=B: `DataTree.int()` → `(recv).int()`.
    DataTreeInt,
    /// D-SERDE-ACCESS=B: `DataTree.text()` → `(recv).text()`.
    DataTreeText,
    /// D-SERDE-ACCESS=B: `DataTree.bool()` → `(recv).bool()`.
    DataTreeBool,
    /// D-SERDE-ACCESS=B: `DataTree.float()` → `(recv).float()`.
    DataTreeFloat,
    /// D-SERDE16=A: `tree.decode<T>()` dispatches the public `T.Decode` protocol.
    DataTreeDecode(Type),
    /// D-SERDE2=A: `value.encode()` dispatches the public Encode protocol.
    SerdeEncode,
    /// D-SERDE-ACCESS=B: same accessors on `Json`/`Data`.
    JsonField,
    JsonAt,
    JsonInt,
    JsonText,
    JsonBool,
    JsonFloat,
    /// D-PATHFS1: `Path.from(str)` constructor → `{root}jet_path_from(&(recv))`.
    PathFrom,
    /// D-PATHFS1: `path.join(other)` → `{root}jet_path_join(&(recv), &(a0))` → `JetPath`.
    PathJoin,
    /// D-PATHFS1: `path.parent()` → `{root}jet_path_parent(&(recv))` → `Option<JetPath>`.
    PathParent,
    /// D-PATHFS1: `path.extension()` → `{root}jet_path_extension(&(recv))` → `Option<String>`.
    PathExtension,
    /// D-PATHFS1: `path.stem()` → `{root}jet_path_stem(&(recv))` → `Option<String>`.
    PathStem,
    /// D-PATHFS1: `path.to_string()` → `(recv).jet_show()` → `String`.
    PathToString,
    /// D-PATHFS1: `path.write_atomic(bytes)` → `{root}jet_path_write_atomic(&(recv), &(a0))` → `Result<(), IoError>`.
    PathWriteAtomic,
    /// D-PATHFS1: `path.walk()` → `{root}jet_path_walk(&(recv))` → `Vec<JetPath>`.
    PathWalk,
    /// D-RENDERTGT2=A (c133 M1): NullBackend measure/layout/paint/on_event/commands.
    UiBackendMethod {
        method: String,
    },
    /// c-devserver (owner-directed 2026-07-01): `DevServer` builder methods
    /// (`.html`/`.port`/`.serve`).
    DevServerMethod {
        method: String,
    },
    /// D-WEBAPP1=D: `WebApp` builder methods (`.route`/`.action`/`.mount`/…).
    WebAppMethod {
        method: String,
    },
    /// D-DBDRIVER1: `conn.query(sql, params)` → `Result<Vec<Row>, DbError>`. Encodes
    /// `params` via `jet_std::jet_db_encode_params`, calls the FFI bridge's
    /// `jet_db_query`, decodes the wire result via `jet_std::jet_db_decode_query_result`.
    DbQuery,
    /// D-DBDRIVER1: `conn.query_one(sql, params)` → `Result<Option<Row>, DbError>`.
    /// Same as `DbQuery` but takes only the first row (if any).
    DbQueryOne,
    /// D-DBDRIVER1: `conn.execute(sql, params)` → `Result<Int, DbError>` (affected rows).
    DbExecute,
    /// D-DBDRIVER1: `conn.begin()` → `{ffi}::jet_db_begin((recv).handle)` → `Bool`.
    DbBegin,
    /// D-DBDRIVER1: `conn.commit()` → `{ffi}::jet_db_commit((recv).handle)` → `Bool`.
    DbCommit,
    /// D-DBDRIVER1: `conn.rollback()` → `{ffi}::jet_db_rollback((recv).handle)` → `Bool`.
    DbRollback,
    /// D-DBDRIVER1: `conn.close()` → `{ffi}::jet_db_close((recv).handle)` → `Bool`.
    DbClose,
    /// D-DBDRIVER1: `DbValue` accessor methods (`.int()`/`.float()`/`.text()`/
    /// `.bool()`/`.is_null()`) → `(recv).<method>()`, same shape as `JsonInt`/….
    DbValueInt,
    DbValueFloat,
    DbValueText,
    DbValueBool,
    DbValueIsNull,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call(name, args)` →
    /// `Result<Float, String>`, a homogeneous `[Float]` call across the
    /// sandboxed Component Model boundary (wire-encoded, see `Prelude/Plugin.rs`).
    PluginCall,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call_int(name, args)` →
    /// `Result<Int, String>`, the `[Int]` sibling of `PluginCall`.
    PluginCallInt,
    /// D-SHIFT1 (c7shift): `Reader.over(bytes)` constructor →
    /// `{root}jet_reader_over(&(recv))` → `JetReader`. `recv` is the `[U8]`
    /// argument (same "arg becomes the recv slot" shape as `PathFrom`).
    ReaderOver,
    /// D-SHIFT1: `reader.read_u8()` → `{root}jet_reader_read_u8(&mut (recv))`
    /// → `Result<U8, String>`. Bounds miss is an ordinary `Err`, never a panic.
    ReaderReadU8,
    ReaderReadU16Le,
    ReaderReadU16Be,
    ReaderReadU32Le,
    ReaderReadU32Be,
    ReaderReadU64Le,
    ReaderReadU64Be,
    /// D-SHIFT1: `reader.take(n)` → `{root}jet_reader_take(&mut (recv), (a0))`
    /// → `Result<Vec<u8>, String>` (owned copy — see CoreLib.rs comment on
    /// why `take` copies rather than borrowing a `View<T>`).
    ReaderTake,
    /// D-SHIFT1: `reader.remaining()` → `{root}jet_reader_remaining(&(recv))` → `Int`.
    ReaderRemaining,
    /// D-SHIFT1: `reader.at_end()` → `{root}jet_reader_at_end(&(recv))` → `Bool`.
    ReaderAtEnd,
    /// D-SHIFT1: `Cursor.over(s)` constructor →
    /// `{root}jet_cursor_over(&(recv))` → `JetCursor`.
    CursorOver,
    /// D-SHIFT1: `cursor.take_until(delim)` →
    /// `{root}jet_cursor_take_until(&mut (recv), &(a0))` → `Result<String, String>`.
    CursorTakeUntil,
    /// D-SHIFT1: `cursor.skip_ws()` → `{root}jet_cursor_skip_ws(&mut (recv))` → `()`.
    CursorSkipWs,
    /// D-SHIFT1: `cursor.take_pattern("…")` — consume-mode reuse of the
    /// D-PARSESTR1 scan engine (`str_match_scan_closure_ex`, I8: one matcher,
    /// not two). `parts` is the pattern literal's already-parsed holes;
    /// `canonical` is the same `(name, type)` list sema put in the call's
    /// `resolved_ret` `Type::Tuple` (so `collect_tuple_shapes_from_expr`
    /// already registered the `JetTup_<hash>` struct this op constructs).
    CursorTakePattern {
        parts: Vec<crate::AST::StrMatchPart>,
        canonical: Vec<(String, Type)>,
    },
    /// D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` —
    /// consume-mode reuse of the D-BINPAT1 bit-scan engine
    /// (`bin_match_scan_closure_ex`, I8: one matcher, not two). `parts` is
    /// the pattern literal's already-parsed holes; `canonical` is the same
    /// `(name, type)` list sema put in the call's `resolved_ret` `Type::Tuple`
    /// — mirrors `CursorTakePattern` exactly, byte-mode sibling.
    ReaderTakePattern {
        parts: Vec<crate::AST::BinMatchPart>,
        canonical: Vec<(String, Type)>,
    },
}

/// One lowered call argument, with the borrow/clone decisions already made (so
/// the emitter reproduces `emit_call_args` without consulting `cx.sigs`).
///
/// Emission order mirrors `emit_call_args` exactly: the clone wrapper (`.clone()`
/// or `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`).
pub struct TCallArg {
    pub value: TExpr,
    /// Emit `&(...)` around the value (a non-scalar passed by `Read` convention).
    pub borrow: bool,
    /// Emit `&mut (...)` around the value (a `Mutate`-convention argument). c109
    /// Phase 6: method args may be `Mutate`; the plain-call path never sets this.
    pub mut_borrow: bool,
    /// Emit `(...).clone()` (an implicit clone — a value passed by `Move`).
    pub clone: bool,
    /// Emit `(...).clone()` (a `Shared` value auto-cloned at the call site — its
    /// own cheap-handle `Clone` impl; D-MEM1 S6 changed this from a hardcoded
    /// `Arc::clone(&...)` once `Shared<T>` stopped being a bare `Arc<T>`).
    /// c109 Phase 6: method/Arc args may set this; the plain-call path does not.
    pub arc_clone: bool,
    /// c109 Phase 13: the Fn-typed-parameter coercion (`emit_call_args`' fn-arg
    /// path). When `Some(<fn-type rust string>)`, the value is wrapped
    /// `Box::new(value) as <fn-type>` — unless it is ALREADY boxed (a bare fn-name
    /// value emits its own `Box::new(…)`, or the value is a fn-typed local ident), in
    /// which case only the ` as <fn-type>` suffix is applied. `already_boxed` carries
    /// that resolved decision so emit makes none. A read callback parameter borrows the
    /// resulting box like every other non-scalar parameter.
    pub fn_coerce: Option<TFnCoerce>,
    /// D-FIXARR1: a `[T#N]` argument passed to a `[T]` (Vec) slot is widened by
    /// copying into a growable list. When true, emit wraps with `.to_vec()`.
    pub widen_to_vec: bool,
    /// D-UNIONTYPE1=A: a member value passed where a union is expected. When
    /// `Some(union)`, emit wraps as `user_<UnionEnum>::<MemberTag>(value)`.
    pub widen_to_union: Option<Type>,
}

/// c109 Phase 13: the resolved Fn-typed-argument coercion (`emit_call_args`).
pub struct TFnCoerce {
    /// Target fn type; emit spells via `cx.rust_type`.
    pub ty: Type,
    /// Whether the value already produces a `Box::new(…)` — emit applies only ` as <fn-type>`.
    pub already_boxed: bool,
}

// ---------------------------------------------------------------------------
// The gate: is this function fully inside the Phase-1 subset?
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
