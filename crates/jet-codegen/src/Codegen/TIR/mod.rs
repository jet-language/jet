//! TIR — a small, *typed* intermediate representation for codegen (c109 Phase 1).
//!
//! ## Why this exists
//!
//! Today codegen (`emit_func` and friends) re-derives semantic facts while it
//! emits Rust: it calls `expr_jet_ty` to re-infer expression types and
//! `operand_is_integer` to re-decide which operator traps on overflow. That is
//! exactly the "codegen re-derives / falls back" smell that invariant I3
//! ("codegen is dumb") forbids, and it is the bug class that produced the I2
//! holes the checked-IR effort was built to kill.
//!
//! The TIR is the fix. It is a distinct, post-sema representation whose defining
//! property is **TOTALITY**: every fact codegen needs is carried concretely on
//! the node — never re-inferred, never an `Option` codegen has to fall back from.
//! Every `TExpr` carries its resolved `Type`; every `Binary` carries its
//! overflow decision as a plain `bool`; every `Let` carries the resolved binding
//! type. The borrowed `TFactChannel` below exposes the optimizer-relevant
//! sema facts without allocating a side table or introducing another IR.
//! Consumers treat an absent channel fact as "not proven" and keep the checked
//! Prelude operation.
//!
//! The emitter (`emit_tir_func`) makes ZERO semantic decisions: it
//! pattern-matches TIR fields and formats Rust. It never calls `expr_jet_ty` or
//! `operand_is_integer`.

//!
//! ## Coverage contract
//!
//! `tir_covers` proves that every reachable function is fully represented by TIR.
//! TIR is the only codegen seam: a coverage miss is an internal compiler error,
//! never a fallback. Add a node only when its construct is covered, and make every
//! field total.

// Re-export the parent `Codegen` glob so the split-out submodules
// (`subset`/`lower`/`emit`) reach `Cx`, `mangle`, `rust_*`, etc. via `use super::*`.
pub(crate) use super::*;

mod emit;
mod eval;
pub use eval::{
    install_comptime_bridge, lower_interp_program, new_memo_state, run_named_func,
    run_named_func_with_memos, run_program, run_program_with_structs, set_native_call_hook,
    stable_memo_field_slot, stable_place_address, tir_place_address_key, MemoState, NativeCallHook,
};

/// Resolve a reflected row through its enclosing generic owner. AOT, Web, and
/// the interpreter call this one substitution seam before building carriers.
pub(crate) fn substitute_reflect_field_type(
    struct_type_params: &std::collections::HashMap<String, Vec<String>>,
    owner_ty: &Type,
    declared: &Type,
) -> Type {
    let Type::Apply { name, args } = owner_ty else {
        return declared.clone();
    };
    let Some(params) = struct_type_params.get(name) else {
        return declared.clone();
    };
    let substitutions = params
        .iter()
        .zip(args)
        .map(|(param, arg)| (param.clone(), arg.clone()))
        .collect();
    crate::Generics::substitute_type(declared, &substitutions)
}
mod lower;
mod subset;

// Re-export every submodule item so existing `TIR::<name>` call sites and the
// `#[cfg(test)] mod tests` block (which uses `super::*`) keep resolving unchanged.
pub(crate) use emit::*;
pub(crate) use lower::*;
pub use subset::is_civil_time_method_name;
pub(crate) use subset::*;

#[cfg(test)]
pub(crate) fn unmatched_enum_match_guard(
    fallthrough: bool,
    span: crate::Diagnostics::Span,
) -> Result<(), crate::Diagnostics::Diagnostic> {
    eval::unmatched_enum_match_guard(fallthrough, span)
}

use crate::Codegen::{mangle, mangle_path};
use crate::AST::{
    AccessConvention, BinOp, CtValue, Expr, Item, Pattern, ProgramBundle, Type, UnOp,
    VariantPayload,
};

/// D-FACT-ENUM-TIR: derive expansion replaces a typed fact read with a
/// `ComptimeName` carrying its enum value before sema sees the generated body.
/// Canonical fact metadata proves the enum kind; the AST node proves this is a
/// compiler-substituted fact rather than an ordinary user enum.
fn compiler_owned_unit_enum(
    type_name: &str,
) -> Option<std::collections::HashMap<String, (crate::Diagnostics::Span, crate::AST::VariantPayload)>>
{
    if is_eval_fragment() {
        return None;
    }
    crate::Sema::core_fact_kind_variants(type_name)
}

fn typed_unit_enum_value(expr: &Expr) -> Option<(&str, &str)> {
    match expr {
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => typed_unit_enum_value(inner),
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if args.is_empty() => Some((type_name, variant)),
        Expr::ComptimeName {
            value:
                Some(CtValue::Enum {
                    type_name,
                    variant,
                    args,
                }),
            ..
        } if args.is_empty() => Some((type_name, variant)),
        _ => None,
    }
}

fn compiler_fact_enum_value(expr: &Expr) -> Option<(&str, &str)> {
    match expr {
        Expr::Paren(inner, _) | Expr::Copy(inner, _) => compiler_fact_enum_value(inner),
        Expr::ComptimeName {
            value:
                Some(CtValue::Enum {
                    type_name,
                    variant,
                    args,
                }),
            ..
        } if args.is_empty() => Some((type_name, variant)),
        _ => None,
    }
}

/// Fold equality between compiler-owned fact enum values before any engine or
/// Rust emission sees them. The caller must use this predicate as its coverage
/// proof and its lowering decision so a fact cannot re-enter runtime dispatch.
pub(crate) fn fold_typed_fact_enum_equality(op: BinOp, lhs: &Expr, rhs: &Expr) -> Option<bool> {
    if !matches!(op, BinOp::Eq | BinOp::Ne) {
        return None;
    }
    if compiler_fact_enum_value(lhs).is_none() && compiler_fact_enum_value(rhs).is_none() {
        return None;
    }
    let (left_type, left_variant) = typed_unit_enum_value(lhs)?;
    let (right_type, right_variant) = typed_unit_enum_value(rhs)?;
    if left_type != right_type {
        return None;
    }
    let variants = compiler_owned_unit_enum(left_type)?;
    let is_unit = |variant: &str| matches!(variants.get(variant), Some((_, VariantPayload::Unit)));
    if !is_unit(left_variant) || !is_unit(right_variant) {
        return None;
    }
    Some(if op == BinOp::Eq {
        left_variant == right_variant
    } else {
        left_variant != right_variant
    })
}

/// The parser represents `value == .Variant` as a pattern test rather than a
/// binary expression. Fold that generated fact shape at the same typed boundary.
pub(crate) fn fold_typed_fact_enum_pattern(subject: &Expr, pattern: &Pattern) -> Option<bool> {
    let (type_name, value_variant) = compiler_fact_enum_value(subject)?;
    let Pattern::Variant {
        variant, bindings, ..
    } = pattern
    else {
        return None;
    };
    if !bindings.is_empty() {
        return None;
    }
    let variants = compiler_owned_unit_enum(type_name)?;
    let is_unit = |variant: &str| matches!(variants.get(variant), Some((_, VariantPayload::Unit)));
    if !is_unit(value_variant) || !is_unit(variant) {
        return None;
    }
    Some(value_variant == variant)
}

thread_local! {
    static LAST_JIT_LOWER_FAILURE: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
}

/// c139 M4: lowered spawn-lambda body for Cranelift JIT (captures as explicit params).
pub struct TJitSpawnLambda {
    pub params: Vec<(String, Type)>,
    pub captures: Vec<JitSpawnCapture>,
    /// D-CONC-FREEZE1=A: frozen source names carried from sema's one flow
    /// fact; the JIT only marshals the already-owned capture slots.
    pub frozen_captures: Vec<String>,
    pub body: TJitSpawnBody,
    pub ret: Type,
    /// D-HARDENED1 / D-MEM-SENTRY1: the task body mints an address from its
    /// own frame storage and must execute through the shared lifetime token.
    pub uses_stack_sentry: bool,
}

pub struct JitSpawnCapture {
    /// Local name used by the lowered spawn body.
    pub name: String,
    /// Source local read at the spawn site.
    pub source: String,
    pub ty: Type,
    pub clone_at_spawn: bool,
    /// D-CONC-FREEZE1=A: sema proved this capture is an immutable snapshot.
    /// The JIT carries the fact; it does not re-run crossing policy.
    pub frozen_at_spawn: bool,
    /// D-MEM-COPYSEM1=A: a read-only view capture is copied into its owning
    /// destination at the spawn/callback boundary. The JIT only marshals the
    /// already-resolved owned type; copy semantics stay on the shared Prelude
    /// path used by AOT and the interpreter.
    pub materialize_at_spawn: bool,
}

/// One bounded traversal primitive for TIR lowering consumers.
///
/// The stack is heap-owned and stores pending nodes, so deep source/TIR
/// structure does not consume the host call stack. Consumers use the same
/// push/pop order for AST scans, TIR statement lowering, and nested-lambda
/// shape inspection.
pub struct TirWorklist<T> {
    pending: Vec<T>,
}

impl<T> TirWorklist<T> {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub fn from_reversed<I>(items: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let mut pending = items.into_iter().collect::<Vec<_>>();
        pending.reverse();
        Self { pending }
    }

    pub fn push(&mut self, item: T) {
        self.pending.push(item);
    }

    pub fn extend<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.pending.extend(items);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.pending.pop()
    }
}

impl<T> Default for TirWorklist<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub enum TJitSpawnBody {
    Expr(Box<TExpr>),
    Block {
        prefix: Vec<TStmt>,
        tail: Option<Box<TExpr>>,
    },
    /// A reactive body lowered once and shared with the normal lambda path.
    SharedBlock {
        body: std::sync::Arc<[TStmt]>,
        tail: bool,
    },
}

/// c139 M3: every lowered function the JIT may compile from the entry module.
pub struct JitProgram {
    /// Display path of the entry module (for overflow trap messages).
    pub source_file: String,
    /// Source text for the one runtime stop renderer's context box.
    pub source_text: String,
    /// D-MEM-GUARANTEE1: package hardening is a checked bundle fact carried
    /// into named deopt; the evaluator never reparses package.jet.
    pub package_hardened: bool,
    /// D-EFFECT-AUTHORITY1: the sema-projected application decision travels
    /// with the lowered program. TIR, JIT, deopt, and the interpreter consume
    /// this value; none of them rediscover package.jet or re-solve effects.
    pub application_authority: jet_foundation::Authority::ApplicationAuthority,
    /// D-REL3: the package edition is a checked bundle fact carried the same
    /// way, because every tier that runs this program answers edition-gated
    /// questions (`core.data`'s checked surface, `fixed_sigs.rs`) from the
    /// `PACKAGE_EDITION` thread-local. The evaluator runs the body on its own
    /// sized worker and named deopt runs on the JIT's thread, so a caller's
    /// ambient scope never reaches either; the fact travels with the program
    /// and is established under the boundary instead.
    pub edition: String,
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
    /// D-FIELDMEMO1=A: stored-field dependency edges consumed by the JIT
    /// invalidation adapter. Values are memo getter names, not policy.
    pub memo_dependencies:
        std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>>,
    /// D-METAREFLECT1: registered field rows shared with comptime reflection.
    pub reflection_fields:
        std::collections::HashMap<String, Vec<jet_foundation::Reflection::ReflectionField>>,
    /// Canonical typeable paths used by interpreter reflection.
    pub reflect_paths: std::collections::HashMap<String, String>,
    /// #2252: the ONE projection from a TIR nominal spelling to the canonical
    /// module-qualified identity every shape, codec, and method table is keyed
    /// by. TIR carries three spellings of the same imported type - the bare
    /// local leaf inside the module that declares it, the alias-qualified
    /// source name in a consumer (`plan.ListReport`), and the canonical
    /// identity a qualified call return already resolves to - while the tables
    /// hold exactly one. Both engines resolve through this map instead of
    /// searching table keys by suffix; a spelling two modules could claim is
    /// absent, so an ambiguous name still reports the ordinary missing-shape
    /// diagnostic rather than silently picking a module.
    pub nominal_identities: std::collections::HashMap<String, String>,
    /// Declared generic parameter names per struct, in source order.
    pub struct_type_params: std::collections::HashMap<String, Vec<String>>,
    /// M5: mangled variant names per enum type (discriminant order).
    pub enum_variants: std::collections::HashMap<String, Vec<String>>,
    /// M5: payload field types per `__jet_Type::__jet_Variant` pattern prefix.
    pub enum_variant_payload_types: std::collections::HashMap<String, Vec<Type>>,
    /// Functions whose typed decode must use the canonical TIR migration plan.
    pub canonical_deopt: std::collections::HashSet<String>,
    /// Functions whose codec calls must remain canonical during named deopt.
    pub canonical_calls: std::collections::HashSet<String>,
    pub int_constants: std::collections::HashMap<String, i64>,
    pub constants: std::collections::HashMap<String, crate::AST::CtValue>,
    pub distinct_bases: std::collections::HashMap<String, Type>,
    pub distinct_ranges: std::collections::HashMap<String, (i64, i64)>,
    /// Published-schema migration plans compiled from sema facts. The
    /// evaluator applies these wire-key operations before re-entering the
    /// ordinary lowered `Decode` body.
    pub codec_migrations: std::collections::HashMap<String, TCodecMigrationPlan>,
    /// Sema-resolved `(trait, method) -> concrete owner` dispatch facts.
    pub trait_method_owners: std::collections::HashMap<(String, String), Vec<String>>,
    /// Sema-resolved `(collection, iterator) -> Iterable.Item` facts.
    pub iterable_item_types: std::collections::HashMap<(String, String), Type>,
}

fn published_schema_unknown_field(s: &crate::AST::StructDef) -> bool {
    s.is_published_schema
}

fn add_published_schema_field(s: &crate::AST::StructDef, fields: &mut Vec<String>) {
    if published_schema_unknown_field(s) {
        fields.push(crate::Syntax::PUBLISHED_UNKNOWN_FIELDS.to_string());
    }
}

fn add_published_schema_field_type(s: &crate::AST::StructDef, types: &mut Vec<Type>) {
    if published_schema_unknown_field(s) {
        types.push(Type::Named(crate::Syntax::TYPE_DATA.to_string()));
    }
}

#[derive(Debug, Clone)]
pub struct TCodecMigrationPlan {
    pub historical_shapes: Vec<Vec<String>>,
    pub steps: Vec<Vec<TCodecMigrationOp>>,
}

/// D-MIGRATE4 version vocabulary. Shapes count from one, and a step names the
/// shapes it moves between. Codegen bakes these names into the generated
/// chain-walker and the evaluator keeps them internal to the decode adapter,
/// so both read them from here. `index` is zero-based.
pub fn migration_shape_name(index: usize) -> String {
    format!("v{}", index + 1)
}

pub fn migration_step_name(index: usize) -> String {
    format!(
        "{}->{}",
        migration_shape_name(index),
        migration_shape_name(index + 1)
    )
}

#[derive(Debug, Clone)]
pub enum TCodecMigrationOp {
    Rename {
        from_key: String,
        to_key: String,
    },
    Remove {
        key: String,
    },
    Add {
        key: String,
        ty: Type,
        default_fn: String,
    },
    Change {
        key: String,
        from_ty: Type,
        to_ty: Type,
        converter_fn: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceProvenance {
    pub canonical_module: String,
    pub fingerprint: String,
    pub full_key_hex: String,
}

pub fn instance_provenance(bundle: &ProgramBundle) -> Vec<InstanceProvenance> {
    bundle
        .modules
        .iter()
        .flat_map(|module| {
            module.items.iter().filter_map(|item| {
                let Item::CodeModule(instance) = item else {
                    return None;
                };
                let identity = instance.instance_identity.as_ref()?;
                Some(InstanceProvenance {
                    canonical_module: instance.name.clone(),
                    fingerprint: identity.fingerprint.clone(),
                    full_key_hex: identity
                        .full_key
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect(),
                })
            })
        })
        .collect()
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
            .map(|variant| mangle_path(&variant.name))
            .collect(),
    );
    for variant in variants {
        let pattern = format!("{}::{}", mangle_path(enum_name), mangle_path(&variant.name));
        enum_variant_payload_types.insert(pattern, payload_types_for_variant(&variant.payload));
    }
}

fn register_imported_enum_variants(
    bundle: &ProgramBundle,
    module_idx: usize,
    owner: &str,
    enum_name: &str,
    variants: &[crate::AST::Variant],
    enum_variants: &mut std::collections::HashMap<String, Vec<String>>,
    enum_variant_payload_types: &mut std::collections::HashMap<String, Vec<Type>>,
) {
    let identity = crate::Codegen::TIR::imported_type_name(owner, enum_name);
    enum_variants.insert(
        identity.clone(),
        variants
            .iter()
            .map(|variant| mangle_path(&variant.name))
            .collect(),
    );
    for variant in variants {
        let pattern = format!("{}::{}", mangle_path(&identity), mangle_path(&variant.name));
        let payload = payload_types_for_variant(&variant.payload)
            .into_iter()
            .map(|ty| crate::Codegen::TIR::qualify_imported_type(bundle, module_idx, owner, &ty))
            .collect();
        enum_variant_payload_types.insert(pattern, payload);
    }
}

fn compile_codec_migrations(
    cx: &Cx,
    items: &[Item],
) -> Option<std::collections::HashMap<String, TCodecMigrationPlan>> {
    let mut plans = std::collections::HashMap::new();
    for item in items {
        let Item::Struct(def) = item else { continue };
        let Some(blocks) = super::Items::migration_blocks(cx, def) else {
            continue;
        };
        let style = super::Items::container_rename_all(&def.serde_markers);
        let historical_shapes = super::Items::migration_shapes(style.as_deref(), def, blocks);
        let mut steps = Vec::with_capacity(blocks.len());
        for block in blocks {
            let mut lowered = Vec::with_capacity(block.ops.len());
            for op in &block.ops {
                lowered.push(match op {
                    crate::AST::MigrationOp::Rename { from, to, .. } => TCodecMigrationOp::Rename {
                        from_key: super::Items::migration_wire_key(style.as_deref(), def, from),
                        to_key: super::Items::migration_wire_key(style.as_deref(), def, to),
                    },
                    crate::AST::MigrationOp::Remove { field, .. } => TCodecMigrationOp::Remove {
                        key: super::Items::migration_wire_key(style.as_deref(), def, field),
                    },
                    crate::AST::MigrationOp::Add {
                        field,
                        ty,
                        default_fn,
                        ..
                    } => TCodecMigrationOp::Add {
                        key: super::Items::migration_wire_key(style.as_deref(), def, field),
                        ty: ty.clone(),
                        default_fn: default_fn.clone()?,
                    },
                    crate::AST::MigrationOp::Change {
                        field,
                        from_ty,
                        to_ty,
                        conv_fn,
                        ..
                    } => TCodecMigrationOp::Change {
                        key: super::Items::migration_wire_key(style.as_deref(), def, field),
                        from_ty: from_ty.clone(),
                        to_ty: to_ty.clone(),
                        converter_fn: conv_fn.clone()?,
                    },
                });
            }
            steps.push(lowered);
        }
        plans.insert(
            def.name.clone(),
            TCodecMigrationPlan {
                historical_shapes,
                steps,
            },
        );
    }
    Some(plans)
}

fn register_union_type(
    ty: &Type,
    enum_variants: &mut std::collections::HashMap<String, Vec<String>>,
    enum_variant_payload_types: &mut std::collections::HashMap<String, Vec<Type>>,
) {
    match ty {
        Type::Union(members) => {
            let name = crate::AST::union_enum_name(members);
            enum_variants
                .entry(name.clone())
                .or_insert_with(|| members.iter().map(crate::AST::union_member_tag).collect());
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
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => {
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

/// c139 M3: the single lowered function selected without argv dispatch.
///
/// Program-struct CLIs return `None`: their entry is one of several commands
/// selected later from the checked schema, not a synthetic top-level `run`.
pub fn lower_entry_main_for_jit(bundle: &ProgramBundle) -> Option<TFunc> {
    lower_jit_program(bundle).and_then(|p| {
        let entry = p.entry;
        let entry = if entry == super::mangle_generated("cli_main") {
            "run".to_string()
        } else {
            entry
        };
        p.funcs.into_iter().find(|f| f.name == entry)
    })
}

/// Rust local place for JIT variable lookup (`__jet_x`).
pub fn local_place(name: &str) -> String {
    super::mangle(name)
}

/// One local or parameter slot, carried as structure instead of a Rust place
/// string. Every engine resolves a slot from these facts alone.
///
/// `name` is the slot's identity: a user binding carries its Jet name, which Rust
/// spells `__jet_<name>`; a compiler-generated temp (`generated`) is allocated
/// through the reserved machine-name lane, which cannot collide with a mangled
/// user name.
/// `deref` records a by-reference slot, which Rust reads through `(*…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TLocal {
    pub name: String,
    pub generated: bool,
    pub deref: bool,
    /// Storage provenance for a place whose Rust shape does not carry it.
    /// `&self` is represented as the bare Rust receiver so ordinary field and
    /// method emission stays unchanged, but its address still belongs to the
    /// caller's storage rather than this method's frame.
    pub address_lifetime: Option<TAddressLifetime>,
    /// D-PERSIST-DEVSTATE1=A: canonical module-store identity for a pinned
    /// binding. Persistent slots do not enter the ordinary local variable map.
    pub persist_key: Option<String>,
    pub persist_ty: Option<Type>,
    /// The Rust binding is mutable. This is a TIR ownership fact, not an
    /// emitter-side repair for a generated spelling.
    pub mutable: bool,
    /// A dead `String.from_bytes(local)` binding may be fused into the following
    /// map update. This carries the source place only when lowering proved the
    /// binding is not read after that update; other engines ignore the AOT
    /// representation hint and retain the ordinary binding semantics.
    pub string_bytes_source: Option<Box<TLocal>>,
    /// The Rust binding is a vetted Prelude storage wrapper until sema-proved
    /// initialization; ordinary TIR reads still have the declared Jet type.
    pub uninit_scalar: bool,
    pub uninit_fixed: bool,
}

impl TLocal {
    /// A user binding, read by value.
    pub fn user(name: impl Into<String>) -> TLocal {
        TLocal {
            name: name.into(),
            generated: false,
            deref: false,
            address_lifetime: None,
            persist_key: None,
            persist_ty: None,
            mutable: false,
            string_bytes_source: None,
            uninit_scalar: false,
            uninit_fixed: false,
        }
    }

    /// A compiler-generated temp slot, read by value.
    pub fn generated(name: impl Into<String>) -> TLocal {
        let name = name.into();
        let name = if name == "self" {
            name
        } else {
            super::mangle_generated(&name)
        };
        TLocal {
            name,
            generated: true,
            deref: false,
            address_lifetime: None,
            persist_key: None,
            persist_ty: None,
            mutable: false,
            string_bytes_source: None,
            uninit_scalar: false,
            uninit_fixed: false,
        }
    }

    /// The compiler-owned STM handle used by `#Transact` Shared edits.
    pub fn stm() -> TLocal {
        TLocal::generated("stm").as_mutable()
    }

    /// A module binding whose value belongs to the shared development store.
    pub fn persistent(name: impl Into<String>, module: &str, ty: Type) -> TLocal {
        let name = name.into();
        TLocal {
            name: name.clone(),
            generated: false,
            deref: false,
            address_lifetime: None,
            persist_key: Some(format!("{module}::{name}")),
            persist_ty: Some(ty),
            mutable: true,
            string_bytes_source: None,
            uninit_scalar: false,
            uninit_fixed: false,
        }
    }

    pub fn is_persistent(&self) -> bool {
        self.persist_key.is_some()
    }

    pub fn as_mutable(mut self) -> TLocal {
        self.mutable = true;
        self
    }

    pub fn with_string_bytes_source(mut self, source: TLocal) -> TLocal {
        self.string_bytes_source = Some(Box::new(source));
        self
    }

    pub fn as_uninit_scalar(mut self) -> TLocal {
        self.uninit_scalar = true;
        self
    }

    pub fn as_uninit_fixed(mut self) -> TLocal {
        self.uninit_fixed = true;
        self
    }

    /// The same slot read through a by-reference deref.
    pub fn through_ref(mut self) -> TLocal {
        self.deref = true;
        self
    }

    pub fn with_address_lifetime(mut self, lifetime: TAddressLifetime) -> TLocal {
        self.address_lifetime = Some(lifetime);
        self
    }

    /// The Rust binding identifier for this slot, without the deref wrapper.
    pub fn rust_name(&self) -> String {
        if self.generated {
            self.name.clone()
        } else if self.is_persistent() {
            local_place(&self.name).to_uppercase()
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

/// Storage provenance for an address-producing TIR expression. This is a
/// lowering fact, not a second address-admission policy: sema still decides
/// which source forms are legal, and each engine only marshals this fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TAddressLifetime {
    /// A value in the current Jet function/lambda/task frame.
    Stack,
    /// Storage owned by a Jet allocator or collection buffer.
    Heap,
    /// A persistent/static slot whose lifetime is not the current frame.
    Static,
    /// A by-reference slot owned by an enclosing caller or boundary.
    Borrowed,
    /// An address whose owner is outside Jet's allocation ledger.
    Foreign,
    /// Lowering cannot prove a more precise owner.
    Unknown,
}

/// Classify the storage reached by an address-producing TIR expression.
/// Unknown and borrowed forms retain the established unowned registration
/// path; only a proven current-frame place receives an expiring sentry token.
pub fn tir_address_lifetime(expr: &TExpr) -> TAddressLifetime {
    match &expr.kind {
        TExprKind::Local(local) => local.address_lifetime.unwrap_or_else(|| {
            if local.is_persistent() {
                TAddressLifetime::Static
            } else if local.deref {
                TAddressLifetime::Borrowed
            } else {
                TAddressLifetime::Stack
            }
        }),
        TExprKind::ConstRef(_) => TAddressLifetime::Static,
        TExprKind::Field { recv, .. } => tir_address_lifetime(recv),
        TExprKind::Index { base, .. } => match &base.ty {
            Type::FixedList { .. } => tir_address_lifetime(base),
            Type::List(_) => TAddressLifetime::Heap,
            _ => TAddressLifetime::Unknown,
        },
        // Distinct values are transparent to the resident raw-place lowering:
        // it takes the address of the same local slot after the nominal wrapper
        // is erased. Preserve that storage fact here so the JIT admission gate
        // and every emitter choose the same frame token.
        TExprKind::DistinctCtor { arg, .. } => tir_address_lifetime(arg),
        TExprKind::PoolSlot { .. } => TAddressLifetime::Heap,
        TExprKind::HandleMethod {
            op: THandleOp::AllocAlloc,
            ..
        } => TAddressLifetime::Heap,
        TExprKind::Borrow { place, .. } => tir_address_lifetime(place),
        TExprKind::RawOf(inner) => tir_address_lifetime(inner),
        TExprKind::Deref(_) | TExprKind::PtrFromAddr { .. } => TAddressLifetime::Foreign,
        _ => TAddressLifetime::Unknown,
    }
}

/// D-FAIL-BIND1=A: the one compiler-generated slot used to carry a failed
/// report into a `??` fallback. Its reserved spelling is shared by AOT, JIT,
/// and TIR-eval; engines only marshal the value already placed here.
pub fn ambient_err_local() -> TLocal {
    TLocal::generated("ambient_err")
}

/// A resolved user method identity. `name` is the Jet method name — the key the
/// JIT and interpreter dispatch on. `mangled` records the one Rust spelling fact:
/// an inherent method becomes `__jet_<name>`, while a trait-impl or dynamic-dispatch
/// method keeps the bare name the trait owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TMethodRef {
    pub name: String,
    pub mangled: bool,
    /// Sema-resolved trait that owns a dynamic call. `None` for inherent and
    /// prelude calls; JIT dispatch never reconstructs this fact from names.
    pub trait_owner: Option<String>,
}

impl TMethodRef {
    /// An inherent user method — Rust spells it `__jet_<name>`.
    pub fn inherent(name: impl Into<String>) -> TMethodRef {
        TMethodRef {
            name: name.into(),
            mangled: true,
            trait_owner: None,
        }
    }

    /// A trait-owned method — Rust spells it bare (the trait declared the name).
    pub fn bare(name: impl Into<String>) -> TMethodRef {
        TMethodRef {
            name: name.into(),
            mangled: false,
            trait_owner: None,
        }
    }

    pub fn trait_method(trait_owner: impl Into<String>, name: impl Into<String>) -> TMethodRef {
        TMethodRef {
            name: name.into(),
            mangled: false,
            trait_owner: Some(trait_owner.into()),
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
    demands: &mut std::collections::BTreeMap<String, (Type, String, Vec<Type>)>,
    ty: &Type,
    method: &str,
) {
    if matches!(ty, Type::Apply { .. }) {
        demands.insert(
            generic_method_instance_key(ty, method, &[]),
            (ty.clone(), method.to_string(), Vec::new()),
        );
    }
}

/// Stable symbol key for one concrete method instance. Empty method arguments
/// retain the historical `Owner<Args>::method` key used by serde demands.
pub fn generic_method_instance_key(owner: &Type, method: &str, type_args: &[Type]) -> String {
    let base = format!("{}::{method}", owner.name());
    if type_args.is_empty() {
        return base;
    }
    let suffix = type_args
        .iter()
        .map(Type::name)
        .map(|name| {
            name.chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("__");
    format!("{base}__generic__{suffix}")
}

/// Seed monomorphize demand for generic Codable from encoding core calls and
/// SerdeEncode/DataTreeDecode ops already present in lowered bodies.
fn collect_serde_codec_demands(
    funcs: &[TFunc],
    demands: &mut std::collections::BTreeMap<String, (Type, String, Vec<Type>)>,
) {
    fn walk_expr(
        expr: &TExpr,
        demands: &mut std::collections::BTreeMap<String, (Type, String, Vec<Type>)>,
    ) {
        // Keep the concrete owner type alive at every TIR node. A generic value
        // can be constructed in one statement and consumed by a later CoreCall;
        // collecting only the operation node loses the `Owner<Args>` identity
        // before the demand worklist runs.
        demand_serde_codec(demands, &expr.ty, "encode");
        demand_serde_codec(demands, &expr.ty, "decode");
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
            TExprKind::DecodeUnder { segment, inner } => {
                walk_expr(segment, demands);
                walk_expr(inner, demands);
            }
            TExprKind::Try { inner, note, .. } => {
                walk_expr(inner, demands);
                if let Some(note) = note {
                    walk_expr(note, demands);
                }
            }
            TExprKind::OrFallback { value, fallback } => {
                walk_expr(value, demands);
                match fallback {
                    TOrFallback::Value(inner) | TOrFallback::Return(Some(inner)) => {
                        walk_expr(inner, demands)
                    }
                    TOrFallback::Panic { msg, .. } => walk_expr(msg, demands),
                    TOrFallback::Return(None)
                    | TOrFallback::Break
                    | TOrFallback::Continue
                    | TOrFallback::BreakLabel(_)
                    | TOrFallback::ContinueLabel(_) => {}
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
                        | "core.encoding.cbor"
                );
                if encoding
                    && matches!(
                        method.as_str(),
                        "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical"
                    )
                {
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
    fn walk_stmt(
        stmt: &TStmt,
        demands: &mut std::collections::BTreeMap<String, (Type, String, Vec<Type>)>,
    ) {
        match stmt {
            TStmt::ExprStmt(e) | TStmt::Return(Some(e)) => walk_expr(e, demands),
            TStmt::Let { init, .. } | TStmt::Assign { value: init, .. } => walk_expr(init, demands),
            TStmt::RefutableBind { init, fallback, .. } => {
                walk_expr(init, demands);
                for stmt in fallback {
                    walk_stmt(stmt, demands);
                }
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
    while let Some((key, (owner_ty, method_name, method_type_args))) = pending.pop_first() {
        if !processed.insert(key.clone()) {
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
        let (name, owner_args): (&str, &[Type]) = match &owner_ty {
            Type::Apply { name, args } => (name.as_str(), args.as_slice()),
            Type::Named(name) => (name.as_str(), &[]),
            _ => continue,
        };
        let Some(params) = items.iter().find_map(|item| match item {
            Item::Struct(def) if def.name == name => Some(def.type_params.as_slice()),
            Item::Enum(def) if def.name == name => Some(def.type_params.as_slice()),
            Item::Distinct(def) if def.name == name => Some(&[]),
            _ => None,
        }) else {
            continue;
        };
        let owner_subst: std::collections::HashMap<String, Type> = params
            .iter()
            .zip(owner_args)
            .map(|(param, arg)| (param.name.clone(), arg.clone()))
            .collect();
        for item in items {
            let (method, trait_name, generated_serde) = match item {
                Item::Struct(def) if def.name == name => {
                    match def.methods.iter().find(|method| method.name == method_name) {
                        Some(method) => (method, None, false),
                        None => continue,
                    }
                }
                Item::Enum(def) if def.name == name => {
                    match def.methods.iter().find(|method| method.name == method_name) {
                        Some(method) => (method, None, false),
                        None => continue,
                    }
                }
                Item::Impl(imp) if imp.type_name == name => {
                    let Some(method) = imp.methods.iter().find(|method| method.name == method_name)
                    else {
                        continue;
                    };
                    match &imp.trait_name {
                        None => (method, None, false),
                        Some(t)
                            if matches!(
                                t.as_str(),
                                crate::Generics::ENCODE
                                    | crate::Generics::DECODE
                                    | crate::Generics::CHECKED_TEXT
                            ) =>
                        {
                            (method, Some(t.as_str()), imp.is_generated_serde)
                        }
                        Some(_) => continue,
                    }
                }
                _ => continue,
            };
            let mut subst = owner_subst.clone();
            // A codec impl keeps the owner's type parameters on its ordinary
            // method. A concrete owner such as `Wrap<Int>` binds those
            // parameters; they are not independent method arguments to discard.
            let owner_binds_method_params = trait_name.is_some_and(|trait_name| {
                matches!(
                    trait_name,
                    crate::Generics::ENCODE | crate::Generics::DECODE
                )
            }) && !owner_subst.is_empty()
                && method
                    .type_params
                    .iter()
                    .all(|param| owner_subst.contains_key(&param.name));
            if owner_binds_method_params {
                if !method_type_args.is_empty() {
                    continue;
                }
            } else if !method_type_args.is_empty() {
                if method_type_args.len() != method.type_params.len() {
                    continue;
                }
                for (param, actual) in method.type_params.iter().zip(&method_type_args) {
                    subst.insert(param.name.clone(), actual.clone());
                }
            } else if !method.type_params.is_empty() {
                continue;
            }
            let mut specialized = crate::Sema::specialize_function_types(method.clone(), &subst);
            let residual_type_params: std::collections::HashSet<String> = specialized
                .type_params
                .iter()
                .map(|param| param.name.clone())
                .collect();
            // Subst already rewrote the binder; drop residual type params so the
            // mono body is admitted as a concrete JIT function.
            specialized.type_params.clear();
            // Keep the source owner's generic identity visible while the structural
            // gate and lowerer inspect the specialized body. Field types are concrete
            // after substitution, but sema's resolved codec calls still name the
            // owner's type parameter (for example `T.encode()`).
            let previous_type_params = cx.current_type_params.borrow().clone();
            let mut method_type_params = previous_type_params.clone();
            method_type_params.extend(residual_type_params);
            cx.current_type_params.replace(method_type_params);
            // D-SERDE2=A / I9 + I8: a generated codec's provenance is the coverage
            // authority in EVERY tier, exactly as the entry-module `Item::Impl` arm
            // below and AOT's `Codegen/Items.rs::emit_trait_method` already treat it.
            // That arm defers every method that still carries type params, so this is
            // the only path that can lower a GENERIC generated codec. Re-deciding
            // coverage here was a second copy of one admission rule (I8), and the two
            // disagreed: a generic derived codec whose specialized body fell outside
            // the structural subset was dropped from the JIT program, so the resident
            // JIT refused and the interpreter reported `Encode/Decode body for
            // `Wrap<Int>`` unsupported for a method AOT emits from the AST path — one
            // program, two meanings by tier. Bodies come from the fixed
            // `Registration/Serde.rs::serde_method` template and `is_generated_serde`
            // is parser-unforgeable, so this admits no user code.
            let covered = generated_serde
                || match trait_name {
                    Some(trait_name) => tir_covers_trait_method(&specialized, name, cx, trait_name),
                    None => tir_covers_method(&specialized, name, cx),
                };
            if !covered {
                cx.current_type_params.replace(previous_type_params);
                continue;
            }
            let mut lowered = if let Some(trait_name) = trait_name {
                // Bind `self` as `Wrap<Int>` so field reads substitute `T` → arg.
                // Encode is an ordinary instance method; Decode stays on the static
                // trait-method ABI (`tree` only, no receiver).
                if trait_name == crate::Generics::ENCODE && matches!(&owner_ty, Type::Apply { .. })
                {
                    lower_method_for_owner(
                        &specialized,
                        name,
                        owner_ty.clone(),
                        cx,
                        generated_serde && specialized.compiler_generated,
                    )
                } else {
                    lower_trait_method(
                        &specialized,
                        name,
                        cx,
                        trait_name,
                        generated_serde && specialized.compiler_generated,
                    )
                }
            } else {
                lower_method_for_owner(&specialized, name, owner_ty.clone(), cx, false)
            };
            cx.current_type_params.replace(previous_type_params);
            lowered.name = key.clone();
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
        Type::Result { ok, err } => {
            matches!(actual, Type::Result { ok: actual_ok, err: actual_err }
            if bind_generic_type(ok, actual_ok, params, subst)
                && bind_generic_type(err, actual_err, params, subst))
        }
        Type::Fn {
            params: template_params,
            ret: template_ret,
            ..
        } => {
            let Type::Fn {
                params: actual_params,
                ret: actual_ret,
                ..
            } = actual
            else {
                return false;
            };
            template_params.len() == actual_params.len()
                && template_params
                    .iter()
                    .zip(actual_params)
                    .all(|(template, actual)| bind_generic_type(template, actual, params, subst))
                && match (template_ret, actual_ret) {
                    (Some(template), Some(actual)) => {
                        bind_generic_type(template, actual, params, subst)
                    }
                    (None, None) => true,
                    _ => false,
                }
        }
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
                crate::Codegen::VariadicBound::build_variadic_bound_func(source, bounds, arity),
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
        if template.type_params.is_empty() {
            continue;
        }
        let explicit_count = template.type_params.len();
        let (param_actuals, explicit_actuals) =
            if actuals.len() == template.params.len() + explicit_count {
                let split = template.params.len();
                (&actuals[..split], &actuals[split..])
            } else if actuals.len() == template.params.len() {
                (&actuals[..], &[][..])
            } else {
                continue;
            };
        let names: std::collections::HashSet<String> = template
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect();
        let mut subst = std::collections::HashMap::new();
        for (param, actual) in template.type_params.iter().zip(explicit_actuals) {
            subst.insert(param.name.clone(), actual.clone());
        }
        if !template
            .params
            .iter()
            .zip(param_actuals)
            .all(|(param, actual)| bind_generic_type(&param.ty, actual, &names, &mut subst))
            || subst.len() != names.len()
        {
            continue;
        }
        let mut specialized = crate::Sema::specialize_function_types(template, &subst);
        let residual_type_params: std::collections::HashSet<String> = specialized
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect();
        specialized.type_params.clear();
        // Sema has replaced the value types, but bounded operator calls retain
        // their resolved source type-parameter owner (`T::compare`). Preserve
        // that exact identity while coverage and lowering consume the body.
        let previous_type_params = cx.current_type_params.replace(residual_type_params);
        let covered = tir_covers(&specialized, cx);
        if !covered {
            cx.current_type_params.replace(previous_type_params);
            continue;
        }
        let mut lowered = lower_func(&specialized, cx);
        cx.current_type_params.replace(previous_type_params);
        lowered.name = emitted_name;
        funcs.push(lowered);
    }
}

/// c139 M3: lower every `tir_covers` top-level function in the entry module so the
/// JIT can compile multi-function programs (calls between covered helpers).
fn memo_dependency_facts(
    cx: &Cx,
) -> std::collections::HashMap<String, std::collections::HashMap<String, Vec<String>>> {
    cx.memo_dependencies
        .iter()
        .map(|(owner, sources)| {
            let sources = sources
                .iter()
                .map(|(source, memo_fields)| {
                    let mut memo_fields = memo_fields.iter().cloned().collect::<Vec<_>>();
                    memo_fields.sort();
                    (source.clone(), memo_fields)
                })
                .collect();
            (owner.clone(), sources)
        })
        .collect()
}

/// D-SERDE2=A / I9: lower the compiler-written codecs an IMPORTED module declares.
///
/// A generated codec is a top-level `Item::Impl` carrying the parser-unforgeable
/// `is_generated_serde` provenance — the same authority the entry-module
/// `Item::Impl` arm below and `Codegen/Items.rs::emit_trait_method` already treat
/// as the coverage decision for compiler-written code. The imported-module loop
/// matches only `Item::Func`, `Item::CodeModule`, `Item::Struct`/`Item::Enum` and
/// the unit-label `Display` impl, so without this pass AOT emits an imported
/// type's `Encode`/`Decode` while the JIT program and the interpreter have no
/// method at all: encoding a struct with an imported field succeeds under
/// `jet build` and stops at E0956 under the default `jet run`. One program, two
/// meanings by tier — the I9 split this closes.
///
/// The codec is lowered against its DECLARING context. That context owns
/// private nominals that an importing context must not expose, while
/// `register_own_struct_shapes` also registers each local leaf under its
/// canonical bundle identity. The generated body, its return type, and every
/// imported call therefore use one nominal identity without making private
/// declarations visible to consumers.
fn lower_imported_generated_codecs(
    bundle: &ProgramBundle,
    module_idx: usize,
    cx: &Cx,
    funcs: &mut Vec<TFunc>,
) {
    if module_idx == bundle.entry {
        return;
    }
    let imported = &bundle.modules[module_idx];
    for item in &imported.items {
        let Item::Impl(implementation) = item else {
            continue;
        };
        if !implementation.is_generated_serde {
            continue;
        }
        let Some(trait_name) = &implementation.trait_name else {
            continue;
        };
        // D-PUBSCHEMA: a published schema's unknown-field holder is registered
        // only in the declaring module's context, so its codec cannot be
        // reproduced faithfully from here. Leave it refused rather than lower a
        // codec that silently drops the merge AOT performs.
        if imported.items.iter().any(|candidate| {
            matches!(candidate, Item::Struct(definition)
                if definition.name == implementation.type_name
                    && definition.is_published_schema)
        }) {
            continue;
        }
        for owner in imported_type_owners(bundle, module_idx) {
            let qualified = imported_type_name(&owner, &implementation.type_name);
            if !cx.struct_fields.contains_key(&qualified)
                && !cx.enum_variants.contains_key(&qualified)
            {
                continue;
            }
            for method in &implementation.methods {
                if !method.type_params.is_empty() {
                    continue;
                }
                let name = format!("{qualified}::{}", method.name);
                if funcs.iter().any(|function| function.name == name) {
                    continue;
                }
                let mut lowered = lower_trait_method(
                    method,
                    &qualified,
                    cx,
                    trait_name,
                    implementation.is_generated_serde && method.compiler_generated,
                );
                // `decode` declares `Result<Badge, [FieldError]>` with the
                // declaring module's leaf; carry it to the same canonical
                // identity the owner and the body already use.
                lowered.ret = lowered
                    .ret
                    .as_ref()
                    .map(|ty| qualify_imported_type(bundle, module_idx, &owner, ty));
                lowered.name = name;
                funcs.push(lowered);
            }
        }
    }
}

/// Lowering is an unbounded-depth recursive descent over user syntax, so the
/// frame requirement is per source-nesting level. This is the narrowest point
/// every caller shares -- the driver's seams, the JIT's public entries, a test
/// helper, and any embedder -- so the sized stack is installed here rather than
/// chased caller by caller. The boundary is re-entrant, so an outer one already
/// on the worker makes this run inline.
///
/// `LAST_JIT_LOWER_FAILURE` is thread-local and callers read it back through
/// `lower_jit_program_fail_reason`, so the reason recorded inside the worker is
/// carried out and restored on the caller's thread.
pub fn lower_jit_program(bundle: &ProgramBundle) -> Option<JitProgram> {
    if jet_foundation::CompilerStack::on_compiler_worker() {
        return lower_jit_program_on_stack(bundle);
    }
    let (program, failure) = jet_foundation::CompilerStack::run_on_compiler_stack(|| {
        let program = lower_jit_program_on_stack(bundle);
        let failure = LAST_JIT_LOWER_FAILURE.with(|failure| failure.borrow_mut().take());
        (program, failure)
    });
    LAST_JIT_LOWER_FAILURE.with(|slot| *slot.borrow_mut() = failure);
    program
}

/// #2252: build [`JitProgram::nominal_identities`].
///
/// The name ledger already owns both halves of this fact: `nominal_identity`
/// gives a declaration its canonical module-qualified name, and the recorded
/// import aliases give every consumer spelling that reaches it. This is the one
/// place the two are joined, so no engine reconstructs an owner by searching
/// table keys for a matching suffix.
///
/// Two rules keep the projection honest:
/// - The entry module's own nominals keep their source spelling: the shape
///   tables register them that way, so remapping them would break the rows the
///   entry itself lowers against.
/// - A spelling that two declarations could claim (the same leaf exported by
///   two modules, or one alias bound to different modules in two consumers) is
///   dropped. The lookup then misses exactly as it does today and the ordinary
///   missing-shape diagnostic stands, instead of one module answering for
///   another.
fn nominal_identity_projection(
    bundle: &ProgramBundle,
) -> std::collections::HashMap<String, String> {
    let mut claims: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    // Loaded dependency items may be copied into the entry module's merged
    // item list. Only ledger-owned declarations are entry-local; imported
    // leaves must remain eligible for the declaring module's bare spelling.
    let entry_names = bundle
        .modules
        .get(bundle.entry)
        .map(|module| {
            module_owned_type_names(&module.items)
                .into_iter()
                .filter(|name| bundle.name_ledger.declaration(bundle.entry, name).is_some())
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();
    for (module_idx, module) in bundle.modules.iter().enumerate() {
        // Every spelling a program can write for an imported nominal: the bare
        // leaf inside its declaring module, and `alias.Leaf` in each consumer.
        let mut spellings: Vec<String> = Vec::new();
        if module_idx != bundle.entry {
            spellings.extend(module_owned_type_names(&module.items));
        }
        for import in &module.imports {
            let alias = import.import_alias();
            let Some(target) = bundle
                .name_ledger
                .effective_alias(module_idx, &alias)
                .and_then(|alias| alias.target_module)
            else {
                continue;
            };
            spellings.extend(
                module_owned_type_names(&bundle.modules[target].items)
                    .into_iter()
                    .map(|leaf| format!("{alias}.{leaf}")),
            );
        }
        for spelling in spellings {
            if !spelling.contains('.') && entry_names.contains(&spelling) {
                continue;
            }
            let Some(identity) = canonical_nominal_from(bundle, module_idx, &spelling) else {
                continue;
            };
            claims.entry(spelling).or_default().insert(identity);
        }
    }
    claims
        .into_iter()
        .filter_map(|(spelling, identities)| {
            let mut identities = identities.into_iter();
            let identity = identities.next()?;
            identities.next().is_none().then_some((spelling, identity))
        })
        .collect()
}
/// Name a selected zero-argument imported Executable or Service the way the
/// imported function table does. Local Outputs deliberately stay on the
/// original entry selection path below; Check Outputs are plural test-harness
/// entries, not a singular JIT entry.
fn selected_imported_zero_arg_tir_entry(bundle: &ProgramBundle) -> Option<String> {
    let module = bundle.modules.get(bundle.entry)?;
    module.items.iter().find_map(|item| {
        let Item::Const(value) = item else {
            return None;
        };
        let output = value.resolved_output.as_ref()?;
        if !output.selected
            || output.module == bundle.entry
            || !output.params.is_empty()
            || !matches!(
                output.kind,
                crate::AST::OutputKind::Executable | crate::AST::OutputKind::Service
            )
        {
            return None;
        }
        Some(output.lowered_name.clone())
    })
}
fn selected_zero_arg_tir_entry(bundle: &ProgramBundle) -> Option<String> {
    let module = bundle.modules.get(bundle.entry)?;
    selected_imported_zero_arg_tir_entry(bundle)
        .or_else(|| {
            module.items.iter().find_map(|item| match item {
                Item::Const(value) => value.resolved_output.as_ref().and_then(|output| {
                    (output.selected && output.module == bundle.entry && output.params.is_empty())
                        .then(|| output.semantic_name.clone())
                }),
                _ => None,
            })
        })
        .or_else(|| {
            module.items.iter().find_map(|item| match item {
                Item::Func(function) if function.name == "run" && function.params.is_empty() => {
                    Some("run".to_string())
                }
                _ => None,
            })
        })
}

fn lower_jit_program_on_stack(bundle: &ProgramBundle) -> Option<JitProgram> {
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
        let zero_arg_entry = selected_zero_arg_tir_entry(bundle);
        let cli_schema = zero_arg_entry
            .is_none()
            .then(|| jet_foundation::CLISchema::entry_schema_for_bundle(bundle))
            .flatten();
        let cli_run = cli_schema.as_ref().and_then(|_| {
            module.items.iter().find_map(|item| match item {
                Item::Func(function) if function.name == "run" => Some(function.name.clone()),
                Item::Const(value) => value.resolved_output.as_ref().and_then(|output| {
                    (output.selected && output.module == bundle.entry && output.params.len() == 1)
                        .then(|| output.semantic_name.clone())
                }),
                _ => None,
            })
        });
        let entry_name = match (zero_arg_entry, cli_run, cli_schema.is_some()) {
            (Some(name), _, _) => name,
            (None, Some(_), _) | (None, None, true) => super::mangle_generated("cli_main"),
            (None, None, false) => return None,
        };
        cx.jit_spawn_lambdas.borrow_mut().clear();
        cx.jit_spawn_sites.borrow_mut().clear();
        cx.jit_method_calls.borrow_mut().clear();
        cx.jit_generic_calls.borrow_mut().clear();
        cx.jit_canonical_deopt.borrow_mut().clear();
        cx.jit_canonical_calls.borrow_mut().clear();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    // D-FFI-INLINE1: body lives in the hidden bridge; calls are ExternCall.
                    if f.inline_foreign.is_some() {
                        continue;
                    }
                    let covered = tir_covers(f, &cx);
                    if !f.type_params.is_empty() || !covered {
                        continue;
                    }
                    let lowered = lower_func(f, &cx);
                    funcs.push(lowered);
                }
                Item::ErrorConv(ec) => {
                    if !tir_covers_error_conv_body(&ec.body, &cx) {
                        continue;
                    }
                    funcs.push(lower_error_conv(ec, &cx));
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
                                ) && !(implementation.compiler_generated
                                    && tir_covers_compiler_derive_method(method, &cx))
                                {
                                    continue;
                                }
                                let mut lowered = lower_trait_method(
                                    method,
                                    &s.name,
                                    &cx,
                                    &implementation.trait_name,
                                    implementation.compiler_generated && method.compiler_generated,
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
                                ) && !(implementation.compiler_generated
                                    && tir_covers_compiler_derive_method(method, &cx))
                                {
                                    continue;
                                }
                                let mut lowered = lower_trait_method(
                                    method,
                                    &e.name,
                                    &cx,
                                    &implementation.trait_name,
                                    implementation.compiler_generated && method.compiler_generated,
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
                            Item::Enum(e) if e.name == imp.type_name => {
                                Some(e.type_params.as_slice())
                            }
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
                                // D-SERDE2=A / I9: a generated codec's provenance is the
                                // coverage authority in every execution tier, exactly as it
                                // is in the AOT emitter (Codegen/Items.rs::emit_trait_method).
                                // Dropping it here would leave the Cranelift JIT and the
                                // interpreter without a method AOT emits.
                                if !imp.is_generated_serde
                                    && !tir_covers_trait_method(
                                        &specialized,
                                        &imp.type_name,
                                        &cx,
                                        trait_name,
                                    )
                                {
                                    continue;
                                }
                                lower_trait_method(
                                    &specialized,
                                    &imp.type_name,
                                    &cx,
                                    trait_name,
                                    imp.is_generated_serde && specialized.compiler_generated,
                                )
                            } else {
                                if !tir_covers_method(&specialized, &imp.type_name, &cx) {
                                    continue;
                                }
                                lower_method_for_owner(
                                    &specialized,
                                    &imp.type_name,
                                    owner_ty.clone(),
                                    &cx,
                                    false,
                                )
                            };
                            lowered.name = format!("{}::{}", owner_ty.name(), method.name);
                            funcs.push(lowered);
                        }
                    }
                }
                Item::CodeModule(cm) => {
                    let Some(body) = &cm.body else { continue };
                    let member_prefix = jet_foundation::Names::member_name(&cm.name, "");
                    for inner in body {
                        match inner {
                            Item::Func(f) => {
                                if !f.type_params.is_empty() || !tir_covers(f, &cx) {
                                    continue;
                                }
                                // Match the AOT inline-module path: lower against the
                                // emitted module-qualified function name so body-local
                                // import scopes select `cm__function` while preserving
                                // the same canonical TIR call form for tier 0.
                                let member = jet_foundation::Names::member_name(&cm.name, &f.name);
                                let mut mangled_f = f.clone();
                                mangled_f.name = member.clone();
                                let mut lowered = lower_func(&mangled_f, &cx);
                                lowered.name = member;
                                funcs.push(lowered);
                            }
                            Item::Struct(s) => {
                                let type_name = if s.name.starts_with(&member_prefix) {
                                    s.name.clone()
                                } else {
                                    jet_foundation::Names::member_name(&cm.name, &s.name)
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
                                let type_name = if imp.type_name.starts_with(&member_prefix) {
                                    imp.type_name.clone()
                                } else {
                                    jet_foundation::Names::member_name(&cm.name, &imp.type_name)
                                };
                                for method in &imp.methods {
                                    let mut lowered = if let Some(trait_name) = &imp.trait_name {
                                        // D-SERDE2=A / I9: same generated-codec provenance
                                        // authority as the top-level impl arm above.
                                        if !imp.is_generated_serde
                                            && !tir_covers_trait_method(
                                                method, &type_name, &cx, trait_name,
                                            )
                                        {
                                            continue;
                                        }
                                        lower_trait_method(
                                            method,
                                            &type_name,
                                            &cx,
                                            trait_name,
                                            imp.is_generated_serde && method.compiler_generated,
                                        )
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
        let mut spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
        let mut reflect_paths = cx.reflect_paths.clone();
        let mut memo_dependencies = memo_dependency_facts(&cx);
        // File-module calls carry their already-resolved Rust path in TIR. Give the
        // resident JIT the same qualified target instead of forcing the whole
        // program through the interpreter, which cannot execute foreign binders.
        for (module_idx, imported) in bundle.modules.iter().enumerate() {
            if module_idx == bundle.entry {
                continue;
            }
            let imported_owner = bundle
                .name_ledger
                .module_identity(module_idx)
                .expect("name ledger must contain every loaded module");
            let mut imported_cx = build_cx_items(
                &imported.items,
                &imported.source,
                &imported.display,
                None,
                &extern_funcs,
            );
            populate_cx_from_bundle(&mut imported_cx, bundle, module_idx);
            // #2252: this pass lowers the module's OWN items under their
            // canonical owner (`{owner}::{Leaf}::{method}` below), so the
            // context needs the same canonical shape rows a consumer context
            // already holds. `populate_cx_from_bundle` registers only what this
            // module imports, leaving its own nominals keyed by bare leaf: the
            // structural gate then refused every method whose owner is the
            // qualified name, and the default tier deopted (E0956) on methods
            // and generated codecs the program plainly declares.
            register_own_struct_shapes(&mut imported_cx, bundle, module_idx);
            imported_cx.jit_local_call_prefix = Some(format!("{}::", mangle(&imported.alias)));
            lower_imported_generated_codecs(bundle, module_idx, &imported_cx, &mut funcs);
            for (owner, sources) in memo_dependency_facts(&imported_cx) {
                let target = memo_dependencies.entry(owner).or_default();
                for (source, fields) in sources {
                    target.entry(source).or_insert(fields);
                }
            }
            for (name, path) in imported_cx.reflect_paths.iter() {
                reflect_paths
                    .entry(name.clone())
                    .or_insert_with(|| path.clone());
            }
            imported_cx.jit_spawn_site_base = spawn_lambdas.len();
            for item in &imported.items {
                match item {
                    Item::Func(function)
                        if function.type_params.is_empty()
                            && tir_covers(function, &imported_cx) =>
                    {
                        imported_cx.jit_local_call_prefix =
                            Some(format!("{}::", mangle(&imported.alias)));
                        let mut lowered = lower_func(function, &imported_cx);
                        lowered.name =
                            format!("{}::{}", mangle(&imported.alias), mangle(&function.name));
                        funcs.push(lowered);
                    }
                    Item::CodeModule(code_module) => {
                        let Some(body) = &code_module.body else {
                            continue;
                        };
                        for inner in body {
                            let Item::Func(function) = inner else {
                                continue;
                            };
                            if !function.type_params.is_empty()
                                || !tir_covers(function, &imported_cx)
                            {
                                continue;
                            }
                            imported_cx.jit_local_call_prefix =
                                Some(format!("{}::", mangle(&code_module.name)));
                            // Keep inline body-local import lookup aligned with the
                            // emitted `code_module__function` name. The final TIR
                            // symbol remains the imported module's Rust-qualified ABI.
                            let mut mangled_function = function.clone();
                            mangled_function.name = jet_foundation::Names::member_name(
                                &code_module.name,
                                &function.name,
                            );
                            let mut lowered = lower_func(&mangled_function, &imported_cx);
                            lowered.name = format!(
                                "{}::{}",
                                mangle(&code_module.name),
                                mangle(&function.name)
                            );
                            funcs.push(lowered);
                        }
                    }
                    item @ (Item::Struct(_) | Item::Enum(_)) => {
                        let (name, type_params, methods, trait_impls) = match item {
                            Item::Struct(definition) => (
                                &definition.name,
                                &definition.type_params,
                                &definition.methods,
                                &definition.trait_impls,
                            ),
                            Item::Enum(definition) => (
                                &definition.name,
                                &definition.type_params,
                                &definition.methods,
                                &definition.trait_impls,
                            ),
                            _ => unreachable!("nominal item gate"),
                        };
                        if !type_params.is_empty() {
                            continue;
                        }
                        imported_cx.jit_local_call_prefix =
                            Some(format!("{}::", mangle(&imported.alias)));
                        for owner in imported_type_owners(bundle, module_idx) {
                            let qualified = imported_type_name(&owner, name);
                            for method in methods {
                                if !tir_covers_method(method, &qualified, &imported_cx) {
                                    continue;
                                }
                                let mut lowered = lower_method_for_owner(
                                    method,
                                    &qualified,
                                    Type::Named(qualified.clone()),
                                    &imported_cx,
                                    false,
                                );
                                lowered.ret = lowered.ret.as_ref().map(|ty| {
                                    qualify_imported_type(bundle, module_idx, &owner, ty)
                                });
                                lowered.name = format!("{}::{}", qualified, method.name);
                                funcs.push(lowered);
                            }
                            for implementation in trait_impls {
                                if matches!(
                                    implementation.trait_name.as_str(),
                                    crate::Generics::ENCODE | crate::Generics::DECODE
                                ) {
                                    continue;
                                }
                                for method in &implementation.methods {
                                    if !tir_covers_trait_method(
                                        method,
                                        &qualified,
                                        &imported_cx,
                                        &implementation.trait_name,
                                    ) && !(implementation.compiler_generated
                                        && tir_covers_compiler_derive_method(method, &imported_cx))
                                    {
                                        continue;
                                    }
                                    let mut lowered = lower_trait_method(
                                        method,
                                        &qualified,
                                        &imported_cx,
                                        &implementation.trait_name,
                                        implementation.compiler_generated
                                            && method.compiler_generated,
                                    );
                                    lowered.ret = lowered.ret.as_ref().map(|ty| {
                                        qualify_imported_type(bundle, module_idx, &owner, ty)
                                    });
                                    lowered.name = format!("{}::{}", qualified, method.name);
                                    funcs.push(lowered);
                                }
                            }
                        }
                    }
                    Item::Impl(implementation) if implementation.trait_name.is_none() => {
                        let owner_is_generic = imported.items.iter().any(|item| match item {
                            Item::Struct(definition) => {
                                definition.name == implementation.type_name
                                    && !definition.type_params.is_empty()
                            }
                            Item::Enum(definition) => {
                                definition.name == implementation.type_name
                                    && !definition.type_params.is_empty()
                            }
                            _ => false,
                        });
                        if owner_is_generic || implementation.is_generated_serde {
                            continue;
                        }
                        imported_cx.jit_local_call_prefix =
                            Some(format!("{}::", mangle(&imported.alias)));
                        for owner in imported_type_owners(bundle, module_idx) {
                            let qualified = imported_type_name(&owner, &implementation.type_name);
                            for method in &implementation.methods {
                                if !method.type_params.is_empty()
                                    || !tir_covers_method(
                                        method,
                                        &implementation.type_name,
                                        &imported_cx,
                                    )
                                {
                                    continue;
                                }
                                let name = format!("{qualified}::{}", method.name);
                                if funcs.iter().any(|function| function.name == name) {
                                    continue;
                                }
                                let mut lowered = lower_method_for_owner(
                                    method,
                                    &qualified,
                                    Type::Named(qualified.clone()),
                                    &imported_cx,
                                    false,
                                );
                                lowered.ret = lowered.ret.as_ref().map(|ty| {
                                    qualify_imported_type(bundle, module_idx, &owner, ty)
                                });
                                lowered.name = name;
                                funcs.push(lowered);
                            }
                        }
                    }
                    Item::Impl(implementation)
                        if implementation.trait_name.as_deref()
                            == Some(crate::Syntax::TRAIT_DISPLAY)
                            && imported_cx
                                .unit_labels
                                .contains_key(&implementation.type_name) =>
                    {
                        for method in &implementation.methods {
                            if !tir_covers_trait_method(
                                method,
                                &implementation.type_name,
                                &imported_cx,
                                crate::Syntax::TRAIT_DISPLAY,
                            ) {
                                continue;
                            }
                            let mut lowered = lower_trait_method(
                                method,
                                &implementation.type_name,
                                &imported_cx,
                                crate::Syntax::TRAIT_DISPLAY,
                                false,
                            );
                            lowered.name = format!(
                                "{}::{}::{}",
                                imported_owner, implementation.type_name, method.name
                            );
                            funcs.push(lowered);
                        }
                    }
                    Item::Impl(implementation) => {
                        // #2252: an imported module's `impl` blocks are ordinary
                        // top-level items, exactly as in the entry module, but
                        // this loop only lowered the methods written INSIDE the
                        // struct or enum. An `impl Type { ... }` method was
                        // therefore absent from the program while the consumer's
                        // projected key named it correctly, so the resident tier
                        // reported a missing method and the whole program
                        // deopted to the interpreter. Generated codecs stay with
                        // `lower_imported_generated_codecs`, the one pass that
                        // owns their provenance.
                        if implementation.is_generated_serde {
                            continue;
                        }
                        let owner_params = imported
                            .items
                            .iter()
                            .find_map(|item| match item {
                                Item::Struct(definition)
                                    if definition.name == implementation.type_name =>
                                {
                                    Some(definition.type_params.as_slice())
                                }
                                Item::Enum(definition)
                                    if definition.name == implementation.type_name =>
                                {
                                    Some(definition.type_params.as_slice())
                                }
                                _ => None,
                            })
                            .unwrap_or(&[]);
                        if !owner_params.is_empty() {
                            continue;
                        }
                        imported_cx.jit_local_call_prefix =
                            Some(format!("{}::", mangle(&imported.alias)));
                        for owner in imported_type_owners(bundle, module_idx) {
                            let qualified = imported_type_name(&owner, &implementation.type_name);
                            for method in &implementation.methods {
                                if !method.type_params.is_empty() {
                                    continue;
                                }
                                let mut lowered =
                                    if let Some(trait_name) = &implementation.trait_name {
                                        if !tir_covers_trait_method(
                                            method,
                                            &qualified,
                                            &imported_cx,
                                            trait_name,
                                        ) {
                                            continue;
                                        }
                                        lower_trait_method(
                                            method,
                                            &qualified,
                                            &imported_cx,
                                            trait_name,
                                            false,
                                        )
                                    } else {
                                        if !tir_covers_method(method, &qualified, &imported_cx) {
                                            continue;
                                        }
                                        lower_method_for_owner(
                                            method,
                                            &qualified,
                                            Type::Named(qualified.clone()),
                                            &imported_cx,
                                            false,
                                        )
                                    };
                                lowered.ret = lowered.ret.as_ref().map(|ty| {
                                    qualify_imported_type(bundle, module_idx, &owner, ty)
                                });
                                lowered.name = format!("{}::{}", qualified, method.name);
                                funcs.push(lowered);
                            }
                        }
                    }
                    _ => {}
                }
            }
            spawn_lambdas.extend(std::mem::take(
                &mut *imported_cx.jit_spawn_lambdas.borrow_mut(),
            ));
        }
        let entry_ok = if entry_name == super::mangle_generated("cli_main") {
            // A program-struct CLI has no literal `run`: argv selects one of its
            // lowered methods or bound functions at execution time. `cli::prepare`
            // resolves that target from the checked schema, and the evaluator
            // reports a missing selected TIR entry instead of rejecting the whole
            // program before dispatch.
            cli_schema.is_some() || funcs.iter().any(|function| function.name == "run")
        } else {
            funcs.iter().any(|function| function.name == entry_name)
        };
        if !entry_ok {
            return None;
        }
        let mut struct_fields = std::collections::HashMap::new();
        let mut struct_field_types = std::collections::HashMap::new();
        let mut reflection_fields = std::collections::HashMap::new();
        let mut struct_type_params = std::collections::HashMap::new();
        let mut enum_variants = std::collections::HashMap::new();
        let mut enum_variant_payload_types = std::collections::HashMap::new();
        enum_variants.insert(
            crate::Syntax::TYPE_ORDERING.to_string(),
            ["Less", "Equal", "Greater"]
                .into_iter()
                .map(mangle)
                .collect(),
        );
        // D-CONC-FAIL1=A: `TaskFailure` is a Prelude enum, so register its
        // packed JIT/AOT shape even when the source only reaches it through
        // `Task<T>.join()` and never constructs a variant explicitly.
        enum_variants.insert(
            crate::Syntax::TYPE_TASK_FAILURE.to_string(),
            ["Cancelled", "DeadlineBlown", "Panicked"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        enum_variant_payload_types.insert(
            format!("{}::Panicked", crate::Syntax::TYPE_TASK_FAILURE),
            vec![Type::String],
        );
        // D-SERVICE-WORKFLOW1=D / D-CONC-OUTCOME1: workflow activity calls carry
        // the service result/status enums even though their Prelude definitions
        // are emitted only for programs that use the service surface.
        enum_variants.insert(
            "TaskOutcome".to_string(),
            ["Finished", "Panicked", "Cancelled", "DeadlineBlown"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        enum_variant_payload_types.insert("TaskOutcome::Panicked".to_string(), vec![Type::String]);
        enum_variants.insert(
            "TaskStatus".to_string(),
            ["Running", "Paused", "CancelRequested"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        // stdlib-api-laws D4 (#2055): `WatchEvent.domain`/`.kind` are Prelude enums
        // reached only through `core.watcher` polling, never constructed in source —
        // register their packed JIT/AOT shape for the same reason as `TaskFailure`.
        // Declaration order must match `Prelude/CoreLib/JetStd/CommonTypes.rs`.
        enum_variants.insert(
            "WatchDomain".to_string(),
            ["File", "Process", "Port"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        enum_variants.insert(
            "WatchKind".to_string(),
            ["Created", "Modified", "Removed", "Error", "Exited", "Ready"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        );
        let mut int_constants = std::collections::HashMap::new();
        let mut constants = std::collections::HashMap::new();
        // D-PERSIST1: shared-heap overrides for `#Persist` bindings (tier-0 + tier-1).
        let persist_prep = jet_foundation::Persist::prepare_bundle(bundle);
        let persist_overrides = persist_prep.by_name;
        for item in &module.items {
            match item {
                Item::Struct(s) => {
                    struct_type_params.insert(
                        s.name.clone(),
                        s.type_params
                            .iter()
                            .map(|param| param.name.clone())
                            .collect(),
                    );
                    struct_fields.insert(s.name.clone(), {
                        let mut fields = s
                            .reflection_fields()
                            .map(|f| mangle(&f.name))
                            .collect::<Vec<_>>();
                        add_published_schema_field(s, &mut fields);
                        fields
                    });
                    struct_field_types.insert(s.name.clone(), {
                        let mut types = s
                            .reflection_fields()
                            .map(|f| f.ty.clone())
                            .collect::<Vec<_>>();
                        add_published_schema_field_type(s, &mut types);
                        types
                    });
                    reflection_fields.insert(s.name.clone(), jet_foundation::Reflection::fields(s));
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
                    let persisted = c
                        .is_persist
                        .then(|| persist_overrides.get(&c.name).cloned())
                        .flatten();
                    if let Some(value) = persisted.clone().or_else(|| c.ct.clone()) {
                        constants.insert(c.name.clone(), value);
                    }
                    let value = match persisted.or_else(|| c.ct.clone()) {
                        Some(crate::AST::CtValue::Int(value)) => Some(value),
                        Some(_) => None,
                        None => match &c.value {
                            crate::AST::Expr::Int(value, _, _, _) => Some(*value),
                            _ => None,
                        },
                    };
                    if let Some(value) = value {
                        int_constants.insert(c.name.clone(), value);
                    }
                }
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        let member_prefix = jet_foundation::Names::member_name(&cm.name, "");
                        for inner in body {
                            match inner {
                                Item::Struct(s) if s.type_params.is_empty() => {
                                    let name = if s.name.starts_with(&member_prefix) {
                                        s.name.clone()
                                    } else {
                                        jet_foundation::Names::member_name(&cm.name, &s.name)
                                    };
                                    struct_fields.insert(name.clone(), {
                                        let mut fields = s
                                            .reflection_fields()
                                            .map(|f| mangle(&f.name))
                                            .collect::<Vec<_>>();
                                        add_published_schema_field(s, &mut fields);
                                        fields
                                    });
                                    struct_field_types.insert(name.clone(), {
                                        let mut types = s
                                            .reflection_fields()
                                            .map(|f| f.ty.clone())
                                            .collect::<Vec<_>>();
                                        add_published_schema_field_type(s, &mut types);
                                        types
                                    });
                                    reflection_fields
                                        .insert(name, jet_foundation::Reflection::fields(s));
                                }
                                Item::Enum(e) if e.type_params.is_empty() => {
                                    let name = if e.name.starts_with(&member_prefix) {
                                        e.name.clone()
                                    } else {
                                        jet_foundation::Names::member_name(&cm.name, &e.name)
                                    };
                                    register_enum_variants(
                                        &name,
                                        &e.variants,
                                        &mut enum_variants,
                                        &mut enum_variant_payload_types,
                                    );
                                }
                                Item::Const(c) => {
                                    if let Some(value) = &c.ct {
                                        constants.insert(
                                            jet_foundation::Names::member_name(&cm.name, &c.name),
                                            value.clone(),
                                        );
                                    }
                                    let value = match &c.ct {
                                        Some(crate::AST::CtValue::Int(value)) => Some(*value),
                                        _ => match &c.value {
                                            crate::AST::Expr::Int(value, _, _, _) => Some(*value),
                                            _ => None,
                                        },
                                    };
                                    if let Some(value) = value {
                                        int_constants.insert(
                                            jet_foundation::Names::member_name(&cm.name, &c.name),
                                            value,
                                        );
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
        for (module_idx, imported) in bundle.modules.iter().enumerate() {
            if module_idx == bundle.entry {
                continue;
            }
            for item in &imported.items {
                match item {
                    Item::Struct(s) => {
                        for owner in crate::Codegen::TIR::imported_type_owners(bundle, module_idx) {
                            let name = crate::Codegen::TIR::imported_type_name(&owner, &s.name);
                            struct_type_params.insert(
                                name.clone(),
                                s.type_params
                                    .iter()
                                    .map(|param| param.name.clone())
                                    .collect(),
                            );
                            struct_fields.insert(name.clone(), {
                                let mut fields = s
                                    .reflection_fields()
                                    .map(|field| mangle(&field.name))
                                    .collect::<Vec<_>>();
                                add_published_schema_field(s, &mut fields);
                                fields
                            });
                            struct_field_types.insert(name.clone(), {
                                let mut types = s
                                    .reflection_fields()
                                    .map(|field| {
                                        crate::Codegen::TIR::qualify_imported_type(
                                            bundle, module_idx, &owner, &field.ty,
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                add_published_schema_field_type(s, &mut types);
                                types
                            });
                            reflection_fields.insert(
                                name,
                                jet_foundation::Reflection::fields(s)
                                    .into_iter()
                                    .map(|mut field| {
                                        field.ty = crate::Codegen::TIR::qualify_imported_type(
                                            bundle, module_idx, &owner, &field.ty,
                                        );
                                        field
                                    })
                                    .collect(),
                            );
                        }
                        for field in &s.fields {
                            register_union_type(
                                &field.ty,
                                &mut enum_variants,
                                &mut enum_variant_payload_types,
                            );
                        }
                    }
                    Item::Enum(e) if e.type_params.is_empty() => {
                        for owner in crate::Codegen::TIR::imported_type_owners(bundle, module_idx) {
                            register_imported_enum_variants(
                                bundle,
                                module_idx,
                                &owner,
                                &e.name,
                                &e.variants,
                                &mut enum_variants,
                                &mut enum_variant_payload_types,
                            );
                        }
                    }
                    Item::CodeModule(code_module) => {
                        let Some(body) = &code_module.body else {
                            continue;
                        };
                        for inner in body {
                            match inner {
                                Item::Struct(s) if s.type_params.is_empty() => {
                                    struct_fields.insert(
                                        s.name.clone(),
                                        s.reflection_fields()
                                            .map(|field| mangle(&field.name))
                                            .collect(),
                                    );
                                    struct_field_types.insert(
                                        s.name.clone(),
                                        s.reflection_fields()
                                            .map(|field| field.ty.clone())
                                            .collect(),
                                    );
                                    reflection_fields.insert(
                                        s.name.clone(),
                                        jet_foundation::Reflection::fields(s),
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
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
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
                fields.iter().map(|(name, _)| mangle(name)).collect(),
            );
            struct_field_types.insert(
                tuple_ty.name(),
                fields.iter().map(|(_, ty)| ty.clone()).collect(),
            );
        }
        let distinct_bases = cx
            .distinct_types
            .iter()
            .map(|(name, (base, _))| (name.clone(), base.clone()))
            .collect();
        let mut trait_method_owners =
            std::collections::HashMap::<(String, String), Vec<String>>::new();
        for item in &module.items {
            let mut record = |trait_name: &str, owner: &str, methods: &[crate::AST::Func]| {
                for method in methods {
                    trait_method_owners
                        .entry((trait_name.to_string(), method.name.clone()))
                        .or_default()
                        .push(owner.to_string());
                }
            };
            match item {
                Item::Struct(def) => {
                    for implementation in &def.trait_impls {
                        record(
                            &implementation.trait_name,
                            &def.name,
                            &implementation.methods,
                        );
                    }
                }
                Item::Enum(def) => {
                    for implementation in &def.trait_impls {
                        record(
                            &implementation.trait_name,
                            &def.name,
                            &implementation.methods,
                        );
                    }
                }
                Item::Impl(implementation) => {
                    if let Some(trait_name) = &implementation.trait_name {
                        record(
                            trait_name,
                            &implementation.type_name,
                            &implementation.methods,
                        );
                    }
                }
                _ => {}
            }
        }
        let iterable_item_types = cx
            .iterable_hooks
            .iter()
            .map(|(collection, hook)| {
                (
                    (collection.clone(), hook.iter_type.clone()),
                    hook.item_type.clone(),
                )
            })
            .collect();
        let codec_migrations = compile_codec_migrations(&cx, &module.items)?;
        let canonical_deopt = cx.jit_canonical_deopt.borrow().clone();
        let canonical_calls = cx.jit_canonical_calls.borrow().clone();
        Some(JitProgram {
            instance_provenance: instance_provenance(bundle),
            source_file: module.display.clone(),
            source_text: module.source.clone(),
            package_hardened: bundle.package_guarantees.harden,
            application_authority: bundle.package_guarantees.application_authority.clone(),
            edition: bundle.edition.clone(),
            entry: entry_name,
            funcs,
            spawn_lambdas,
            struct_fields,
            struct_field_types,
            memo_dependencies,
            reflection_fields,
            reflect_paths,
            nominal_identities: nominal_identity_projection(bundle),
            struct_type_params,
            enum_variants,
            enum_variant_payload_types,
            canonical_deopt,
            canonical_calls,
            int_constants,
            constants,
            distinct_bases,
            distinct_ranges: cx.distinct_ranges.clone(),
            codec_migrations,
            trait_method_owners,
            iterable_item_types,
        })
    })
}

/// The two `lower_jit_program_fail_reason` answers that describe a real
/// user-side missing entry point rather than a compiler defect: the program
/// simply has nothing to run. Every OTHER reason means lowering itself failed
/// on a program that does have an entry, which is an I2 internal compiler
/// error, not a user diagnostic (card #2001). The dev interpreter boundary in
/// `eval::run_bundle_at_stage` keys E2201 off exactly these two, so they are
/// named here beside the strings they must stay equal to.
pub const NO_RUNNABLE_ENTRY: &str = "no runnable entry";
pub const CLI_ENTRY_MISSING_RUN: &str = "cli entry missing `run`";

/// Why `lower_jit_program` returned `None`.
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
    let selected = selected_zero_arg_tir_entry(bundle).or_else(|| {
        jet_foundation::CLISchema::entry_schema_for_bundle(bundle)
            .map(|_| super::mangle_generated("cli_main"))
    });
    let Some(selected) = selected else {
        return NO_RUNNABLE_ENTRY.to_string();
    };
    let entry_check = if selected == super::mangle_generated("cli_main") {
        "run".to_string()
    } else {
        selected.clone()
    };
    let mut saw_entry = false;
    let mut entry_tir = false;
    for item in &module.items {
        let Item::Func(f) = item else {
            continue;
        };
        if f.name == entry_check {
            saw_entry = true;
            entry_tir = tir_covers(f, &cx);
        }
    }
    if !saw_entry {
        return if selected == super::mangle_generated("cli_main") {
            CLI_ENTRY_MISSING_RUN.to_string()
        } else {
            "selected entry is not a top-level function".to_string()
        };
    }
    if !entry_tir {
        let mut locals: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in &module.items {
            let Item::Func(f) = item else {
                continue;
            };
            if f.name != entry_check {
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

/// Runtime sentry metadata for one source-level `#Unsafe` gate. The metadata
/// is data in TIR; each engine only marshals it into the shared Prelude sentry.
#[derive(Clone, Debug)]
pub struct TUnsafeGate {
    pub file: String,
    pub line: u32,
    pub reason: String,
    pub enabled: bool,
    /// D-MEM-GUARANTEE1: this gate belongs to a contained dependency and
    /// must activate the same Prelude witness even in release.
    pub fenced: bool,
}

/// A lowered top-level function. `params` are already mangled to their Rust
/// names and carry their resolved Jet `Type`; `ret` is the resolved return type.
pub struct TFunc {
    /// Jet function name (unmangled) — the emitter mangles via `cx.mangle_name`.
    pub name: String,
    /// Source range for function-level evaluator diagnostics.
    ///
    /// Expression-precise spans remain owned by Tower #1329.
    pub source_span: crate::Diagnostics::Span,
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
    /// Compiler-synthesized function (auto-derive/serde builders). The stack
    /// frame keeps its file/line/name attribution, but never embeds the raw
    /// source line — that line is a declaration the user wrote for something
    /// else (e.g. a `#UnitFamily(...)` marker), not this function.
    pub synthetic: bool,
    /// c109 Phase 18: an `#Unsafe fn` (S58, E2-M13/D-LL1) lowers to a Rust `unsafe fn`
    /// (the `unsafe ` keyword prefixes the signature), so the body may use gated pointer
    /// ops directly — calling it is already gated to an `#Unsafe` block in sema (E3103).
    /// I1: this is true ONLY when the source function was `#Unsafe fn`; no `unsafe` is
    /// ever emitted without that source gate. Applies to `TopLevel`/`Method`; a trait
    /// method carries its own `is_unsafe` on `TFuncKind::TraitMethod`.
    pub is_unsafe: bool,
    /// Function-level `#Unsafe("reason") fn` sentry scope, if present.
    pub unsafe_gate: Option<TUnsafeGate>,
    /// D-CABI-CALLBACK1: named pure, monomorphic top-level functions expose a
    /// stable C-convention symbol; sema alone decides whether it may cross C.
    pub is_pure: bool,
    /// D-MEMO1=A: sema-proved result-cache configuration. TIR carries the
    /// bound; emitters and evaluators only marshal it into the shared Prelude.
    pub memo_bound: Option<Option<usize>>,
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
    /// D-SIMD3=B: `#Scalar` is the explicit native auto-vectorization opt-out.
    /// It is a codegen boundary hint; semantics remain in the shared Prelude.
    pub is_scalar: bool,
    /// D-COMPUTE-KERNEL-SURFACE1=B: sema's complete safe-kernel proof. The
    /// emitter and interpreter carry this fact without re-deriving it.
    pub kernel_proof: Option<crate::AST::KernelProof>,
    /// D-FIELDMEMO1=A: the synthetic getter stores its result in this hidden
    /// owner field. `None` keeps ordinary functions and methods unchanged.
    pub memo_field: Option<String>,
    /// D-HARDENED1 / D-MEM-SENTRY1: lowering proved that this body mints an
    /// address from current-frame storage. Engines install one shared Prelude
    /// frame token before executing such a body.
    pub uses_stack_sentry: bool,
    pub body: Vec<TStmt>,
    /// c109 Phase 7: how this function is emitted. A top-level function gets
    /// `pub fn name(…)` at module scope; a method gets `pub fn __jet_name(<self>, …)`
    /// inside an `impl` block (indented), with the `self` receiver form per the
    /// resolved convention (or no receiver for a static method).
    pub kind: TFuncKind,
}

impl TFunc {
    /// Return function-level facts without copying a side bundle.
    ///
    /// `is_pure` is a sema proof, not an inference from the body. A false bit
    /// therefore remains `Unknown`, so an optimizer cannot treat an ordinary
    /// function as impure merely because no purity proof was attached.
    pub fn fact_channel(&self) -> TFactChannel<'_> {
        TFactChannel {
            ty: self.ret.as_ref(),
            integer_bounds: self.ret.as_ref().and_then(integer_bounds_for_type),
            exclusivity: TExclusivity::Unknown,
            purity: if self.is_pure {
                TPurity::Pure
            } else {
                TPurity::Unknown
            },
            no_cross_iteration_deps: false,
            comptime_value: None,
            cost: None,
        }
    }
}

/// One lowered `#Pre`/`#Post` clause. `condition` and `message` share the
/// function's parameter bindings; postconditions additionally bind
/// `__jet_result` to the returned value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TContractKind {
    Pre,
    Post,
}

/// Why a contract node remains or disappears from executable TIR.
///
/// `Stripped` is reserved for the ratified per-module build policy.  It is
/// deliberately distinct from `Proven`: an explicit policy choice is not a
/// proof fact and must remain auditable in TIR.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TContractDisposition {
    Check,
    Proven,
    Stripped,
}

pub struct TContract {
    pub kind: TContractKind,
    pub condition: TExpr,
    pub message: TExpr,
    pub file: String,
    pub line: u32,
    pub span: crate::Diagnostics::Span,
    pub disposition: TContractDisposition,
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
/// the static `jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<FieldError>>`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SerdeCodec {
    Encode,
    Decode,
}

/// c109 Phase 7: the emission shape of a lowered function.
pub enum TFuncKind {
    /// A module-level free function — `pub fn name(params) { … }`.
    TopLevel,
    /// An inherent method inside `impl __jet_<T> { … }`. `self_conv` is the receiver
    /// convention for an instance method (`Read`→`&self`, `Mutate`→`&mut self`,
    /// `Move`→`self`), or `None` for a STATIC (associated) method (no `self`
    /// parameter). The method name is mangled (`__jet_<name>`) and emitted with `pub`.
    Method {
        self_conv: Option<AccessConvention>,
        owner_type: Type,
    },
    /// c109 Phase 12: a trait-impl method inside `impl Trait for __jet_<T> { … }` (the
    /// caller `emit_trait_impl`/`emit_external_trait_impl` opened the block). Distinct
    /// from an inherent `Method`: the method name is BARE (the trait owns it — no
    /// `__jet_` mangle) and there is NO `pub`. `self_conv` is the receiver convention
    /// (`Read`→`&self`, `Mutate`→`&mut self`, `Move`→`self`), or `None` for a static
    /// trait method. D-MUTSELF1: a `mut self` trait method gets `&mut self` and may
    /// mutate the receiver in place. `is_unsafe` reproduces the `unsafe fn` prefix
    /// for an `#Unsafe fn` trait method (S58/D-LL1 — the body may use gated ops;
    /// calling it is already gated to an `#Unsafe` block).
    TraitMethod {
        is_unsafe: bool,
        self_conv: Option<AccessConvention>,
        /// D-SERDE2 (card #131 S1-bridge): a hand-written `impl T.Encode` /
        /// `impl T.Decode` method. The user writes the verbs `encode`/`decode`
        /// with Jet-facing signatures, but the Rust `__jet_Encode`/`__jet_Decode`
        /// traits declare `jet_encode(&self) -> DataTree` /
        /// `jet_decode(tree: &DataTree) -> Result<Self, [FieldError]>`. This bridges
        /// the name + signature internally (I2: a sema-accepted hand impl must
        /// produce Rust rustc accepts). `None` for every ordinary trait method.
        serde: Option<SerdeCodec>,
    },
    /// c109 Phase 15: a DELEGATION trait method (`using field`) — `emit_delegation_method`
    /// (Source/Codegen/Items.rs). The whole method is structural: a forwarding call
    /// `(self).<field>.<method>(<args>)` to the delegated field, with the BARE trait
    /// method name (no `__jet_` mangle). There is NO body to lower — the forward string is
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

/// c109 Phase 22: a special source iteration form on a `loop x in <coll>`,
/// resolved at lowering from the collection's type or method-call shape.
/// Each carries the receiver's emitted Rust string;
/// `file`/the panic line are program/source facts. The plain `.iter().cloned()` form
/// (incl. a non-special method-call collection like `.split(…)`, which `emit_for_in`
/// routes to its `else` default) is represented by `ForIn.method_kind == None`.
pub enum TForInMethod {
    /// `loop c in s.chars()` — char iteration: `for __jet_c in ({recv}).chars()`,
    /// binding `let <var> = __jet_c;`.
    Chars,
    /// `loop line in reader.lines()` on a `FileReader` — streaming `BufRead::lines`
    /// over the reader's `inner`, with a mid-stream-error panic (line `0`, `cx.file`).
    LinesFile,
    /// `loop line in io.stdin().lines()` / a `StdinHandle` — the same streaming read,
    /// but the receiver is materialised into a `__jet_stdin_h` local inside an extra
    /// block (so the `io.stdin()` temporary outlives the loop body), with a matching
    /// extra closing brace.
    LinesStdin,
    /// D-PROCESS1=A: `loop line in child.stdout.lines()` / `child.stderr.lines()` —
    /// a `ProcessChild`'s streaming reader. The receiver string is the plain field
    /// access (`(child).stdout`); each iteration polls
    /// `jet_process_stream_next_line(&recv)` via a `let Some(x) = … else { break }`,
    /// so (unlike `LinesFile`/`LinesStdin`) no extra wrapper block is needed.
    LinesProcessStream,
    /// D-CONC-CHAN1=A: `loop value in receiver` pulls until the receiver closes.
    ChannelReceiver,
    /// D-ENCSTREAM-SURFACE1=A: bounded synchronous codec-reader pull source.
    EncodingReader { reader_type: String },
    /// D-ITER-HOOK: `loop x in mytype` when `mytype` implements `Iterable`.
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
///  - `WithPrelude` — compiler-owned statements that must run immediately before
///    the condition and remain in scope for both the condition and its selected
///    branch. This is used when a structural pattern reads one subject in both
///    places; the subject is evaluated once, outside the condition's expression
///    scope.
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
    IsNone {
        subj: TExpr,
    },
    Matches {
        pattern: TPattern,
        subj: TExpr,
    },
    WithPrelude {
        prelude: Vec<TStmt>,
        cond: Box<TIfCond>,
    },
}

/// D-DOTSCOPE1: which `#Test` scope member a `TStmt::ScopeMember` is.
pub enum ScopeMemberKind {
    /// `.setup { … }` — the body's statements are spliced inline (bindings leak
    /// to the rest of the test), running first.
    Setup,
    /// `.expect_fail { … }` / `.expect_fail(E3010) { … }` — the region must
    /// fail, optionally with the named runtime stop code.
    ExpectFail(Option<String>),
    /// `.timeout(dur) { … }` — post-hoc budget. The region runs to completion,
    /// then its elapsed time is compared against the canonical Duration value;
    /// over budget fails the test. (v1: post-hoc — does not interrupt a hang.)
    Timeout(TExpr),
    /// `.measure { … }` — a claim selected by `jet test --measure`. Plain
    /// `jet test` still executes the body once as an ordinary correctness claim.
    Measure,
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

/// Injected prelude struct fields (HTTPRequest route metadata). Emit spells lines.
#[derive(Clone)]
pub enum TStructExtra {
    /// HTTPRequest: `params: BTreeMap::new(), route_template: None`
    HTTPRequestParams,
}

/// Host/prelude call assembled only in emit — structured pieces, no Rust source text.
pub enum THostCall {
    /// `{root}{helper}({args…})` with per-arg wrap style.
    Helper { helper: String, args: Vec<THostArg> },
    /// `(recv).{method}({args})`
    Method {
        recv: Box<TExpr>,
        method: String,
        args: Vec<TExpr>,
    },
    /// D-FAIL-CARRIER1=A: read a middle state off the outcome. `field` is the
    /// Jet field the error type carries it on, and `notes` picks which prelude
    /// reader decides what a success answers — `jet_notes` for the words a
    /// failure had, `jet_partial` for the payload it kept. Every engine reads
    /// this one node, so the rule is stated once and no Rust text is written
    /// here.
    CarrierFact {
        recv: Box<TExpr>,
        field: String,
        notes: bool,
    },
    /// `Cell(Read|Edit)Guard.map/split`: sema-proved paths, shared by all tiers.
    CellGuardProject {
        recv: Box<TExpr>,
        paths: Vec<Vec<String>>,
        result_ty: Type,
        editable: bool,
        edit_paths_disjoint: bool,
    },
    /// D-OOBPROOF1: sema-proved fixed-list index. Each engine performs a
    /// direct read; the checked helper remains for unproven indexing.
    FixedListIndex {
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: u32,
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
    GcRead { root: String },
    /// Option/pattern projection helpers: `(inner).is_some()` / `.unwrap()` / field project.
    OptionProbe {
        inner: Box<TExpr>,
        kind: TOptionProbe,
    },
    /// D-PARSESTR1: str-match scan against the lowered subject; emit builds the IIFE.
    /// Statement switches pass `SwitchSubjectValue`, while value-form dispatch
    /// passes its own subject local. Keeping the subject in TIR makes the scan
    /// usable in every expression position without relying on switch ambient.
    StrMatchScan {
        subject: Box<TExpr>,
        parts: Vec<crate::AST::StrMatchPart>,
        probe: TMatchProbe,
    },
    /// D-BINPAT1: binary-pattern scan against the lowered subject.
    BinMatchScan {
        subject: Box<TExpr>,
        parts: Vec<crate::AST::BinMatchPart>,
        probe: TMatchProbe,
    },
    /// Tuple element project: `(base).{index}` (after str/bin-match unwrap).
    TupleIndex { base: Box<TExpr>, index: usize },
    /// Struct-pattern subject field: `((*__jet_switch_subject).{field})`
    SwitchSubjectField { field: String },
    /// The already-evaluated subject of the enclosing `MixedSwitch`.
    SwitchSubjectValue,
    /// Generator `yield e` → `__jet_yield_tx.send(e)`.
    YieldSend { value: Box<TExpr> },
    /// SQL/HTML/Sh and URL/Path/DateTime typed constructors from literals + hole exprs.
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
    /// `core.sys.set` with rich panic on invalid runtime strings.
    EnvSet {
        name: Box<TExpr>,
        value: Box<TExpr>,
        loc: TPanicLoc,
    },
    /// Numeric bounds constant: `{rust_type(ty)}::{member}`.
    NumericBounds { ty: Type, member: String },
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
    SQLRaw,
    HTMLRaw,
    ShRaw,
    SQLTemplate,
    SQLParams,
    HTMLText,
}

/// D-TYPEDTEXT1=D / D-BOUND-HEAD1=A: TIR carries the same descriptor as the
/// source surface. There is no second kind table in an execution tier.
pub type TTypedTextInterpKind = crate::Syntax::TypedHeadKind;

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
    /// A function local whose value crosses the native interrupt boundary.
    /// Emit uses Arc-backed `Send + Sync` storage instead of ordinary `Rc`.
    SendFn(Type),
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
    /// `assert(cond[, msg])`
    Require {
        cond: Box<TExpr>,
        msg: Option<Box<TExpr>>,
    },
    /// `assert_eq(left, right)`
    RequireEq { left: Box<TExpr>, right: Box<TExpr> },
    /// `panic(msg)` / `?? panic(msg)`
    Panic { msg: Box<TExpr> },
}

/// A lowered statement. Only the constructs the Phase-1 subset allows.
pub enum TStmt {
    /// D-FAIL-TIER1: one executable contract check.  A precondition node is
    /// placed immediately before the call that supplies its arguments;
    /// postconditions live in `ContractScope` around the callee body.
    Contract {
        contract: TContract,
    },
    /// D-FAIL-TIER1: function-body contract scope.  The post list is checked
    /// against the returned value on every return path by each backend.
    ContractScope {
        pre: Vec<TContract>,
        body: Vec<TStmt>,
        post: Vec<TContract>,
        ret: Option<Type>,
    },
    /// `let [mut] name[: ty] = init;`. All presentation facts are resolved at
    /// lowering, reproducing `emit_let` (Source/Codegen/Statement.rs) byte-for-byte:
    /// `kw` is `"let"` or `"let mut"` (the `mut` accounts for the source `mutable`
    /// flag AND the forced-mut cases — a handle binding FileReader/FileWriter/
    /// TcpStream/HTTPRouter/Arena/… needs `let mut` even when bound immutably, an
    /// escaping FnMut lambda binding, or an owned `ResourceTake`); `let_ty` is the
    /// structured annotation (emit spells the `: …` clause). The binding's resolved
    /// type is carried on the `LowerEnv` slot (for downstream facts), so it is not
    /// duplicated on the node.
    Let {
        name: String,
        kw: &'static str,
        let_ty: TLetTy,
        init: TExpr,
        /// D-OPTGC1=A: sema's complete automatic-promotion decision.
        gc_promotion: Option<crate::AST::GcPromotion>,
        gc_transferred: bool,
    },
    /// D-CHOOSE-TEST1=A: a subject-first pattern test whose miss route
    /// diverges. Unlike an `if let`, Rust's `let ... else` keeps every capture
    /// in the surrounding scope. The same node lets the interpreter bind into
    /// its current scope before continuing, preserving tier parity.
    RefutableBind {
        pattern: TPattern,
        init: TExpr,
        fallback: Vec<TStmt>,
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
        /// Non-copyable guard fields must move out of the owned tuple instead
        /// of borrowing it and accidentally cloning the guarded value.
        move_fields: bool,
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
        /// Structs containing a SharedGuard are consumed so the owned guard
        /// moves into the binding instead of cloning its dereferenced payload.
        move_fields: bool,
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
        /// non-Copy parameter in deref position). Assigning `(*__jet_s)` directly moves
        /// out of a shared reference (E0507); emitting `((*__jet_s)).clone()` is correct.
        /// Mirrors the `lower_enum_arg` clone predicate. False for scalars and owned values.
        clone_value: bool,
        /// Source line of the assignment, so a compound operator that traps
        /// (D-EXPSEM1: `^=`) can name the line the author wrote.
        line: u32,
    },
    Return(Option<TExpr>),
    /// A call used for effect: `print(x);`, `helper(a);`.
    ExprStmt(TExpr),
    /// D-CONC-SPAWN1=D: a lexical structured-concurrency scope. Engines create
    /// one group, run `body`, and close it on every exit path.
    TaskGroup {
        group: TLocal,
        limit: Option<TExpr>,
        body: Vec<TStmt>,
    },
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
    /// `else_is_elseif` distinguishes a nested residual guard arm from the
    /// explicit `else` body of the canonical subjectless `Switch`: nested arms
    /// emit as `} else if …`, while the final body stays `} else { … }`.
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
    /// `loop i in start..end [, stride]` — a numeric range loop (`ForKind::Range`).
    /// Inclusive `..` (S22) lowers to `start..=end`; exclusive `..<`
    /// (D-RANGE-EXCL1=C) lowers to `start..end`. Optional stride uses `.step_by`.
    Range {
        label: Option<String>,
        var: String,
        /// D-RANGE-VALUE1=A: `Some` evaluates one Range value once. Literal
        /// ranges keep `None` and the direct start/end jump lowering.
        source: Option<TExpr>,
        start: TExpr,
        end: TExpr,
        step: Option<TExpr>,
        exclusive: bool,
        /// D-SIMD3=B: sema's complete proof for the loop's vectorizable shape.
        auto_vectorization: Option<crate::AST::AutoVectorizationFacts>,
        body: Vec<TStmt>,
    },
    /// `break` / `break(name)` (label resolved at lowering).
    Break(Option<String>),
    /// `break value` / `break(name, value)`.
    BreakValue {
        label: Option<String>,
        value: TExpr,
    },
    /// Source `next` / `next(name)`; internally retained as Continue.
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
    /// a block that binds `__jet_switch_subject` to a borrow of the subject (the
    /// binding is unused in this form but emitted for parity). Each arm's `(lo, hi)`
    /// becomes `(subj >= lo && subj <= hi)`, reading the subject's resolved place.
    RangeSwitch {
        /// The matched subject expression. Emit borrows it for the
        /// `__jet_switch_subject` binding and re-emits it in each range test.
        subject: TExpr,
        arms: Vec<(i64, i64, Vec<TStmt>)>,
        else_body: Vec<TStmt>,
    },
    /// c109 Phase 5: indexed assignment `coll[i] = value` (`Stmt::Assign` with an
    /// `LValue::Index`). `is_map` is the resolved `IndexKind` (TOTAL, from sema):
    /// `true` → `jet_map_insert(&mut (base), (i).clone(), v)`; `false` →
    /// `(base)[i as usize] = v`. `base` may itself be an index projection: each
    /// engine must lower that chain as a mutable place, not as a cloned value.
    /// Both wrap the value in a `{ let __jet_v = …; … }` block, byte-for-byte the
    /// AST `LValue::Index` form. Compound ops (`+=`) on an index are not a Jet
    /// construct here (the parser/sema only admit a plain `=` to an index lvalue),
    /// so no `op` is carried.
    IndexAssign {
        /// The base is fixed-list storage created with `Type.{ uninit }`.
        /// Engines keep the same TIR operation but choose storage-safe writes.
        uninit: bool,
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
    /// c109 Phase 5/22: collection iteration `loop x in coll` / `loop (k, v) in map`
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
        /// The expression iterated over: the method receiver for a method-call
        /// form, otherwise the whole collection (including codec readers).
        source: TExpr,
        /// The whole collection expression, whose type carries the element type.
        collection: TExpr,
        /// D-LOOP-ADVANCE2=A source stride, evaluated once before the first pull.
        step: Option<TExpr>,
        method_kind: Option<TForInMethod>,
        /// D-SOA1 / D-SOA-TIER1=A: the collection is a `#layout(columnar)` list —
        /// iterate the gathered record view (`iter_aos()`, owned `S` pulled out of
        /// the shared column store) instead of `iter().cloned()`. Always `false`
        /// for the map/method forms.
        columnar: bool,
        /// D-ONCE-WORD1 / D-CONC-STREAM1: the collection is a `Stream<T>` —
        /// iterate it directly BY VALUE; the shared Stream Prelude owns the
        /// producer task and cancellation at the consumer's wait boundary.
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
    /// `__jet_switch_subject = &(subject)` (emitted for parity even when unused), then an
    /// `if/else if …` chain over each arm's condition, with the `else`/fallthrough form
    /// reproduced exactly. Each arm's condition is resolved to a Rust string at lowering
    /// (emit makes no decision). `else_body` is the optional `else` arm.
    MixedSwitch {
        /// The matched subject. Emit borrows it for the parity binding; arm
        /// conditions are already structured `TExpr` values.
        subject: TExpr,
        class: BranchClass,
        arms: Vec<(TExpr, Vec<TStmt>)>,
        else_body: Option<Vec<TStmt>>,
    },
    /// c109 Phase 18: an audited `#Unsafe { … }` gate region (`Stmt::Unsafe`, S58,
    /// E2-M13/D-LL1). The AST `emit_stmts` lowers it straight to a Rust `unsafe { … }`
    /// block; the `#Audit("…")` annotation (the `audit` field) emits NOTHING (codegen is
    /// dumb — sema validated the audit). I1: this TIR node exists ONLY for a source
    /// `#Unsafe` region, so the emitted `unsafe { … }` is always 1:1 with a source gate.
    /// Body bindings use the `unsafe` block's child lexical env.
    Unsafe {
        gate: TUnsafeGate,
        body: Vec<TStmt>,
    },
    /// D-MEM-SENTRY1: a source/package sentry policy scope. It changes only
    /// the Prelude gate bit and preserves the enclosing gate provenance.
    SentryPolicy {
        enabled: bool,
        body: Vec<TStmt>,
    },
    /// D-CTEFFECT1: an explicit `#Impure("reason") { … }` policy gate.
    /// AOT/JIT execute a plain lexical block; comptime evaluation raises its
    /// impurity depth only while evaluating this body.
    Impure(Vec<TStmt>),
    /// D-REACTCORE1: `#Reactive { … }` — register a reactive effect at this point.
    /// `closure` is the AOT Rust string; `executable` is the JIT-compilable body.
    Reactive {
        closure: String,
        executable: Box<TLambda>,
    },
    /// c109 Phase 19: an explicit `region r { … }` (D-REGION1 opt B). Lowers to a plain
    /// Rust block `{ … }` — a lexical scope. The region's escape bound (E0631) and arena
    /// drop ordering (S63 RAII) are enforced entirely in sema; codegen is dumb (I3).
    /// Body bindings live only in the child `LowerEnv` matching that Rust scope.
    Region(Vec<TStmt>),
    /// D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }` — a Cassowary-style
    /// constraint block. Unlike `Region`/the `task.group` path, this DOES need a
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
        /// D-STM1=A (card #506): the compiler-owned mutable STM handle when the
        /// block touches the `Shared<T>` plane. Its presence means emission wraps
        /// the body in `jet_stm::begin()` … `.commit()` — the atomic multi-handle
        /// commit. `None` preserves the plain local-only transaction shape.
        stm: Option<TLocal>,
        body: Vec<TStmt>,
    },
    /// D-DBG3 step 2 (dap-debugger): a source line marker, one per lowered `Stmt`,
    /// inserted ONLY when `cx.debug_linemap` is set (native `jet debug` builds —
    /// never a normal build or the JIT tier, so this is invisible to the JIT
    /// lowering gate and every other TStmt consumer). Emits a `// jet:line N`
    /// comment immediately before the statement's generated Rust, giving the native
    /// backend a rust-line -> jet-line table without touching any other TStmt shape.
    LineMarker(usize),
    /// Source location for the following statement. Evaluators consume it;
    /// code generators discard it.
    SourceSpan(crate::Diagnostics::Span),
}

impl TStmt {
    /// Return the loop proof, if this statement carries one.
    pub fn fact_channel(&self) -> TFactChannel<'_> {
        match self {
            TStmt::Range {
                auto_vectorization: Some(facts),
                ..
            } => TFactChannel::from_auto_vectorization(facts),
            _ => TFactChannel::unknown(),
        }
    }
}

/// D-BRANCH-CODEGEN1=B: total lowering classification for one arm table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchClass {
    Bool2,
    Enum,
    DenseInt,
    SparseInt,
    Ordered,
    Mixed,
}

/// c109 Phase 4: one lowered arm of an exhaustive enum match. `pattern` is the
/// fully-resolved Rust match pattern (`__jet_Light::__jet_Red`,
/// `__jet_Conn::__jet_Active(user_id) | __jet_Conn::__jet_Reconnecting(user_id)`,
/// `__jet_Http::__jet_Good(__jet_range_0)`); `guard` is the optional `if …` range
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
    /// An Option binding test (`if x == Some(v)`).
    OptionBinding,
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

    /// An Option payload-binding test (`if x == Some(v)`).
    pub fn option_binding(pattern: crate::AST::Pattern) -> TPattern {
        TPattern {
            pattern,
            enum_type: None,
            position: TPatternPosition::OptionBinding,
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

/// Compact, typed bounds carried by a sema proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TIntegerBounds {
    pub lo: i128,
    pub hi: i128,
}

impl TIntegerBounds {
    pub const fn exact(value: i128) -> Self {
        Self {
            lo: value,
            hi: value,
        }
    }
}

/// A fully typed expression. The resolved type is carried on every node,
/// so codegen never recomputes it.
pub struct TExpr {
    pub ty: Type,
    pub kind: TExprKind,
}

/// The semantic operation whose cost can remain after optimization.  These
/// categories are deliberately small and typed: reporting must consume the
/// same fact that lowering gave to every backend, not infer a cost from Rust
/// text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TCostKind {
    /// A read-only view crossed an owning boundary and must be copied.
    ViewMaterialization,
    /// A shared map backing store needs `Arc::make_mut` before an edit.
    CollectionCopyOnWrite,
    /// An exact `Int` operation may leave the packed signed-63-bit rail.
    ExactIntSpill,
    /// A `Result`/`Option` carrier is constructed at a call or explicit
    /// constructor site.
    OutcomeConstruction,
    /// A generic collection representation remains because no shape proof
    /// selected the direct representation.
    RepresentationFallback,
}

impl TCostKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ViewMaterialization => "view materialization",
            Self::CollectionCopyOnWrite => "map copy-on-write",
            Self::ExactIntSpill => "exact Int spill",
            Self::OutcomeConstruction => "outcome construction",
            Self::RepresentationFallback => "generic representation fallback",
        }
    }

    pub const fn operation(self) -> &'static str {
        match self {
            Self::ViewMaterialization => "materializing a read-only view",
            Self::CollectionCopyOnWrite => "editing a shared map backing store",
            Self::ExactIntSpill => "an exact Int operation may spill to bigint",
            Self::OutcomeConstruction => "constructing an Outcome carrier",
            Self::RepresentationFallback => "using a generic collection representation",
        }
    }

    pub const fn fix(self) -> &'static str {
        match self {
            Self::ViewMaterialization => {
                "keep the value in its view form, or make the copy explicit outside the loop"
            }
            Self::CollectionCopyOnWrite => {
                "mutate an exclusive map place, or move the edit outside the loop"
            }
            Self::ExactIntSpill => {
                "bound the operands so the result stays on the packed Int rail, or accept exact bigint cost"
            }
            Self::OutcomeConstruction => {
                "consume the outcome through an immediate fast path when one exists"
            }
            Self::RepresentationFallback => {
                "provide the collection shape proof that selects the direct representation"
            }
        }
    }
}

/// Whether a typed cost is still present in emitted semantics.  `Removed` is
/// retained for `jet explain --cost`, while lint consumers only surface
/// `SemanticRemainder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TCostState {
    SemanticRemainder,
    OptimizerRemoved,
}

/// One cost fact projected through the same borrowed channel as type, bounds,
/// access, and purity facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TCostFact {
    pub kind: TCostKind,
    pub state: TCostState,
}

impl TCostFact {
    pub const fn semantic(kind: TCostKind) -> Self {
        Self {
            kind,
            state: TCostState::SemanticRemainder,
        }
    }
}

/// A lowered cost site.  The span is the nearest source marker available in
/// TIR; function-level attribution remains total when debug line markers are
/// not present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TCostSite {
    pub function: String,
    pub span: crate::Diagnostics::Span,
    pub kind: TCostKind,
    pub state: TCostState,
    pub loop_depth: usize,
}

/// A cost projection must never turn a filtered TIR program into a green
/// report.  A lowerer refusal is therefore part of the public projection
/// result, not an empty report that an adapter can accidentally ignore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TCostReportError {
    Lowering { reason: String },
    Incomplete { surfaces: Vec<String> },
}

impl TCostReportError {
    pub fn message(&self) -> String {
        match self {
            Self::Lowering { reason } => {
                format!("typed TIR lowering failed for cost projection: {reason}")
            }
            Self::Incomplete { surfaces } => format!(
                "cost projection is incomplete: reachable sema surface(s) omitted by typed TIR: {}",
                surfaces.join(", ")
            ),
        }
    }
}

/// Complete cost projection for one checked program.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TCostReport {
    pub sites: Vec<TCostSite>,
}

/// The access fact available to an optimizer consumer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TExclusivity {
    /// Sema did not prove an access mode at this subject.
    Unknown,
    /// A shared/read access. It does not authorize mutation or reordering.
    Shared,
    /// A sema-proven exclusive/write access.
    Exclusive,
    /// A sema-proven consuming/move access.
    Move,
}

/// Three-state purity fact. `Unknown` is the conservative default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TPurity {
    Unknown,
    Pure,
    Impure,
}

/// One read-only fact channel shared by TIR optimization consumers.
///
/// This is deliberately a borrowed view over facts already carried by the
/// frozen TIR (`TExpr.ty`/`CtLit`, `TCallArg` access flags, `TStmt` loop proofs,
/// and `TFunc.is_pure`). It has no per-node allocation and no parallel schema.
/// A missing field means sema did not prove that fact; consumers must retain
/// the checked Prelude operation or other conservative dependency.
#[derive(Clone, Copy, Debug)]
pub struct TFactChannel<'a> {
    pub ty: Option<&'a Type>,
    pub integer_bounds: Option<TIntegerBounds>,
    pub exclusivity: TExclusivity,
    pub purity: TPurity,
    /// Sema's loop-level proof that one iteration cannot observe another's
    /// writes. Unknown stays false so vectorizers never infer independence.
    pub no_cross_iteration_deps: bool,
    pub comptime_value: Option<&'a crate::AST::CtValue>,
    /// Typed cost evidence. `SemanticRemainder` is a real cost; `OptimizerRemoved`
    /// is a proof that the same operation remains semantically available but
    /// its dynamic fallback is unreachable for the carried bounds.
    pub cost: Option<TCostFact>,
}

impl<'a> TFactChannel<'a> {
    pub const fn unknown() -> Self {
        Self {
            ty: None,
            integer_bounds: None,
            exclusivity: TExclusivity::Unknown,
            purity: TPurity::Unknown,
            no_cross_iteration_deps: false,
            comptime_value: None,
            cost: None,
        }
    }

    /// Project the complete sema loop proof into the common channel.
    pub fn from_auto_vectorization(facts: &'a crate::AST::AutoVectorizationFacts) -> Self {
        Self {
            ty: Some(&facts.element_type),
            integer_bounds: None,
            exclusivity: if facts.no_aliasing {
                TExclusivity::Exclusive
            } else {
                TExclusivity::Unknown
            },
            purity: if facts.effect_free_body {
                TPurity::Pure
            } else {
                TPurity::Unknown
            },
            no_cross_iteration_deps: facts.no_cross_iteration_deps,
            comptime_value: None,
            cost: None,
        }
    }
}

fn integer_bounds_for_type(ty: &Type) -> Option<TIntegerBounds> {
    ty.integer_range().map(|(lo, hi)| TIntegerBounds { lo, hi })
}

fn integer_bounds_for_expr(expr: &TExpr) -> Option<TIntegerBounds> {
    match &expr.kind {
        TExprKind::IntLit(value, _) => Some(TIntegerBounds::exact(*value as i128)),
        TExprKind::CtLit(crate::AST::CtValue::Int(value)) => {
            Some(TIntegerBounds::exact(*value as i128))
        }
        TExprKind::CtLit(crate::AST::CtValue::BigInt(value)) => {
            value.try_i128().map(TIntegerBounds::exact)
        }
        TExprKind::NumericMethod {
            op: TNumericOp::InlineRange { lo, hi, .. },
            ..
        } => Some(TIntegerBounds {
            lo: *lo as i128,
            hi: *hi as i128,
        }),
        _ => integer_bounds_for_type(&expr.ty),
    }
}
/// D-INTBIG1: the inline rail is a signed 63-bit payload, represented by the
/// same half-i64 bounds as `Prelude/Core/JetInt`. Keep this projection in TIR
/// so cost consumers share one proof rather than reading backend constants.
const INT_SMALL_MIN: i128 = -(1i128 << 62);
const INT_SMALL_MAX: i128 = (1i128 << 62) - 1;

fn bounds_fit_inline(bounds: TIntegerBounds) -> bool {
    bounds.lo >= INT_SMALL_MIN && bounds.hi <= INT_SMALL_MAX
}

fn combine_integer_bounds(
    op: BinOp,
    lhs: TIntegerBounds,
    rhs: TIntegerBounds,
) -> Option<TIntegerBounds> {
    let candidates = match op {
        BinOp::Add => [
            lhs.lo.checked_add(rhs.lo)?,
            lhs.hi.checked_add(rhs.hi)?,
            0,
            0,
        ],
        BinOp::Sub => [
            lhs.lo.checked_sub(rhs.hi)?,
            lhs.hi.checked_sub(rhs.lo)?,
            0,
            0,
        ],
        BinOp::Mul => [
            lhs.lo.checked_mul(rhs.lo)?,
            lhs.lo.checked_mul(rhs.hi)?,
            lhs.hi.checked_mul(rhs.lo)?,
            lhs.hi.checked_mul(rhs.hi)?,
        ],
        _ => return None,
    };
    let mut lo = candidates[0];
    let mut hi = candidates[0];
    for value in candidates.into_iter().skip(1) {
        lo = lo.min(value);
        hi = hi.max(value);
    }
    Some(TIntegerBounds { lo, hi })
}

fn integer_cost_fact(op: BinOp, lhs: &TExpr, rhs: &TExpr) -> TCostFact {
    let state = match (integer_bounds_for_expr(lhs), integer_bounds_for_expr(rhs)) {
        (Some(lhs), Some(rhs)) => match combine_integer_bounds(op, lhs, rhs) {
            Some(result)
                if bounds_fit_inline(lhs)
                    && bounds_fit_inline(rhs)
                    && bounds_fit_inline(result) =>
            {
                TCostState::OptimizerRemoved
            }
            _ => TCostState::SemanticRemainder,
        },
        _ => TCostState::SemanticRemainder,
    };
    TCostFact {
        kind: TCostKind::ExactIntSpill,
        state,
    }
}

fn integer_unary_cost_fact(operand: &TExpr) -> TCostFact {
    let state = match integer_bounds_for_expr(operand).and_then(|bounds| {
        bounds.lo.checked_neg().map(|_| TIntegerBounds {
            lo: -bounds.hi,
            hi: -bounds.lo,
        })
    }) {
        Some(result)
            if integer_bounds_for_expr(operand).is_some_and(bounds_fit_inline)
                && bounds_fit_inline(result) =>
        {
            TCostState::OptimizerRemoved
        }
        _ => TCostState::SemanticRemainder,
    };
    TCostFact {
        kind: TCostKind::ExactIntSpill,
        state,
    }
}

fn is_outcome_type(ty: &Type) -> bool {
    matches!(ty, Type::Result { .. } | Type::Option(_))
}

fn is_outcome_constructor(kind: &TExprKind) -> bool {
    matches!(
        kind,
        TExprKind::Absent
            | TExprKind::Present(_)
            | TExprKind::Ok(_)
            | TExprKind::Err(_)
            | TExprKind::Call { .. }
            | TExprKind::MethodCall { .. }
            | TExprKind::FnFieldCall { .. }
            | TExprKind::StaticCall { .. }
            | TExprKind::BuiltinMethod { .. }
            | TExprKind::ClosureMethod { .. }
            | TExprKind::HandleMethod { .. }
            | TExprKind::CoreCall { .. }
            | TExprKind::ModuleCall { .. }
            | TExprKind::ExternCall { .. }
            | TExprKind::HostCall(_)
            | TExprKind::RangeCheckedCtor { .. }
            | TExprKind::DistinctConvert { fallible: true, .. }
            | TExprKind::UnitConvert { fallible: true, .. }
            | TExprKind::DecodeUnder { .. }
            | TExprKind::OptionLift2 { .. }
            | TExprKind::OptField { .. }
            | TExprKind::TaskGroupAll { .. }
            | TExprKind::TaskGroupRace { .. }
            | TExprKind::TaskGroupAny { .. }
    )
}

fn outcome_fast_path_available(expr: &TExpr) -> bool {
    if !is_outcome_type(&expr.ty) {
        return false;
    }
    match &expr.kind {
        TExprKind::BuiltinMethod { op, args, .. } if args.is_empty() => {
            op.outcome_fast_path().is_some()
        }
        TExprKind::HandleMethod { op, args, .. } if args.is_empty() => {
            op.outcome_fast_path().is_some()
        }
        _ => false,
    }
}

fn outcome_cost_state(expr: &TExpr) -> Option<TCostState> {
    // The collector below only records a site when the expression carries an
    // outcome-construction fact. The operation capability is the complete
    // removal proof, so do not duplicate that fact match here: it can turn a
    // successfully lowered immediate handler back into a semantic warning.
    outcome_fast_path_available(expr).then_some(TCostState::OptimizerRemoved)
}

fn cost_fact_for_expr(expr: &TExpr) -> Option<TCostFact> {
    match &expr.kind {
        TExprKind::MaterializeView(_) => Some(TCostFact::semantic(TCostKind::ViewMaterialization)),
        TExprKind::BuiltinMethod { recv, op, .. }
            if matches!(&recv.ty, Type::Map { .. }) && op.needs_mut_receiver_place() =>
        {
            Some(TCostFact::semantic(TCostKind::CollectionCopyOnWrite))
        }
        TExprKind::Binary { op, lhs, rhs, .. }
            if matches!(&expr.ty, Type::Int)
                && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) =>
        {
            Some(integer_cost_fact(*op, lhs, rhs))
        }
        TExprKind::Unary {
            op: UnOp::Neg,
            operand,
        } if matches!(&expr.ty, Type::Int) => Some(integer_unary_cost_fact(operand)),
        TExprKind::Index {
            base,
            is_map: false,
            uninit_fixed: false,
            ..
        } if matches!(&base.ty, Type::List(_) | Type::FixedList { .. }) => {
            Some(TCostFact::semantic(TCostKind::RepresentationFallback))
        }
        _ if is_outcome_type(&expr.ty) && is_outcome_constructor(&expr.kind) => {
            Some(TCostFact::semantic(TCostKind::OutcomeConstruction))
        }
        _ => None,
    }
}

/// One piece of a D-VARIADIC1 list spread literal — either a single element or `...list`.
pub enum ListSpreadPart {
    Elem(TExpr),
    Spread(TExpr),
}

impl TExpr {
    /// Return the optimizer facts already established for this expression.
    ///
    /// `ty` and `CtLit` are sema's resolved type/value carriers. The
    /// `InlineRange` operation is retained in the structured kind after the
    /// runtime range carrier is canonicalized, so its interval remains a fact
    /// rather than a codegen guess. Access facts are exposed only for explicit
    /// borrow/resource nodes; all other expressions stay conservative.
    pub fn fact_channel(&self) -> TFactChannel<'_> {
        let exclusivity = match &self.kind {
            TExprKind::Borrow { mutable: true, .. } => TExclusivity::Exclusive,
            TExprKind::Borrow { mutable: false, .. } => TExclusivity::Shared,
            TExprKind::ResourceTake(_) => TExclusivity::Move,
            _ => TExclusivity::Unknown,
        };
        let comptime_value = match &self.kind {
            TExprKind::CtLit(value) => Some(value),
            _ => None,
        };
        TFactChannel {
            ty: Some(&self.ty),
            integer_bounds: integer_bounds_for_expr(self),
            exclusivity,
            purity: TPurity::Unknown,
            no_cross_iteration_deps: false,
            comptime_value,
            cost: cost_fact_for_expr(self),
        }
    }
}
fn collect_cost_lambda(
    lambda: &TLambda,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match &lambda.executable {
        TLambdaBody::Expr(expr) => {
            collect_cost_expr(expr, function, span, loop_depth, sites);
        }
        TLambdaBody::Block(body) => {
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TLambdaBody::SharedBlock(body) => {
            collect_cost_stmts(body.as_ref(), function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_place(
    place: &TPlace,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    if let TPlace::Expr(expr) = place {
        collect_cost_expr(expr, function, span, loop_depth, sites);
    }
}

fn collect_cost_enum_payload(
    payload: &TEnumPayload,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match payload {
        TEnumPayload::Unit => {}
        TEnumPayload::Positional(args) => {
            for arg in args {
                collect_cost_expr(&arg.value, function, span, loop_depth, sites);
            }
        }
        TEnumPayload::Named(args) => {
            for (_, arg) in args {
                collect_cost_expr(&arg.value, function, span, loop_depth, sites);
            }
        }
    }
}

fn collect_cost_require(
    require: &TRequireKind,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match require {
        TRequireKind::Require { cond, msg } => {
            collect_cost_expr(cond, function, span, loop_depth, sites);
            if let Some(msg) = msg {
                collect_cost_expr(msg, function, span, loop_depth, sites);
            }
        }
        TRequireKind::RequireEq { left, right } => {
            collect_cost_expr(left, function, span, loop_depth, sites);
            collect_cost_expr(right, function, span, loop_depth, sites);
        }
        TRequireKind::Panic { msg } => {
            collect_cost_expr(msg, function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_core_closure(
    kind: &TCoreClosureKind,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match kind {
        TCoreClosureKind::Spawn {
            group, executable, ..
        } => {
            if let Some(group) = group {
                collect_cost_expr(group, function, span, loop_depth, sites);
            }
            collect_cost_lambda(executable, function, span, loop_depth, sites);
        }
        TCoreClosureKind::Serve { addr, .. } => {
            collect_cost_expr(addr, function, span, loop_depth, sites);
        }
        TCoreClosureKind::OnInterrupt { callback } => {
            collect_cost_expr(callback, function, span, loop_depth, sites);
        }
        TCoreClosureKind::Guard { executable, .. }
        | TCoreClosureKind::OnCommit { executable, .. }
        | TCoreClosureKind::OnRollback { executable, .. }
        | TCoreClosureKind::ReactiveDerived { executable, .. }
        | TCoreClosureKind::ReactiveEffect { executable, .. }
        | TCoreClosureKind::UiReactiveRender { executable, .. } => {
            collect_cost_lambda(executable, function, span, loop_depth, sites);
        }
        TCoreClosureKind::UiButtonOnClick {
            label, executable, ..
        } => {
            collect_cost_expr(label, function, span, loop_depth, sites);
            collect_cost_lambda(executable, function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_fn_value(
    kind: &TFnValueKind,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match kind {
        TFnValueKind::NamedFn { lambda, .. } => {
            if let Some(lambda) = lambda {
                collect_cost_lambda(lambda, function, span, loop_depth, sites);
            }
        }
        TFnValueKind::Policy {
            callee,
            policy_args,
            ..
        } => {
            collect_cost_expr(callee, function, span, loop_depth, sites);
            collect_cost_call_args(policy_args, function, span, loop_depth, sites);
        }
        TFnValueKind::Call { callee, args } => {
            collect_cost_expr(callee, function, span, loop_depth, sites);
            collect_cost_call_args(args, function, span, loop_depth, sites);
        }
        TFnValueKind::Interrupt { value } => {
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_host_arg(
    arg: &THostArg,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match arg {
        THostArg::Expr(value) | THostArg::Borrow(value) => {
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        THostArg::Lambda(lambda) => {
            collect_cost_lambda(lambda, function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_call_args(
    args: &[TCallArg],
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    for arg in args {
        collect_cost_expr(&arg.value, function, span, loop_depth, sites);
    }
}

fn collect_cost_expr(
    expr: &TExpr,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    collect_cost_expr_with_state(expr, function, span, loop_depth, sites, None);
}

fn collect_cost_expr_with_state(
    expr: &TExpr,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
    state_override: Option<TCostState>,
) {
    let expr_span = match &expr.kind {
        TExprKind::CoreCall { source_span, .. } => *source_span,
        _ => span,
    };
    if let Some(fact) = expr.fact_channel().cost {
        sites.push(TCostSite {
            function: function.to_string(),
            span: expr_span,
            kind: fact.kind,
            state: state_override.unwrap_or(fact.state),
            loop_depth,
        });
    }

    match &expr.kind {
        TExprKind::InlineBlock(body) => {
            collect_cost_stmts(body, function, expr_span, loop_depth, sites);
        }
        TExprKind::StrLit(parts) => {
            for part in parts {
                if let TStrPart::Interp(value, _) = part {
                    collect_cost_expr(value, function, expr_span, loop_depth, sites);
                }
            }
        }
        TExprKind::HostCall(host) => match host.as_ref() {
            THostCall::Helper { args, .. } => {
                for arg in args {
                    collect_cost_host_arg(arg, function, expr_span, loop_depth, sites);
                }
            }
            THostCall::Method { recv, args, .. } => {
                collect_cost_expr(recv, function, expr_span, loop_depth, sites);
                for arg in args {
                    collect_cost_expr(arg, function, expr_span, loop_depth, sites);
                }
            }
            THostCall::CarrierFact { recv, .. }
            | THostCall::CellGuardProject { recv, .. }
            | THostCall::OptionProbe { inner: recv, .. }
            | THostCall::TupleIndex { base: recv, .. } => {
                collect_cost_expr(recv, function, expr_span, loop_depth, sites);
            }
            THostCall::FixedListIndex { base, index, .. } => {
                collect_cost_expr(base, function, expr_span, loop_depth, sites);
                collect_cost_expr(index, function, expr_span, loop_depth, sites);
            }
            THostCall::TypedText { arg, .. } | THostCall::YieldSend { value: arg } => {
                collect_cost_expr(arg, function, expr_span, loop_depth, sites);
            }
            THostCall::GcEdit {
                edit, index_temp, ..
            } => {
                collect_cost_expr(edit, function, expr_span, loop_depth, sites);
                if let Some((_, index)) = index_temp {
                    collect_cost_expr(index, function, expr_span, loop_depth, sites);
                }
            }
            THostCall::StrMatchScan { subject, .. } => {
                collect_cost_expr(subject, function, expr_span, loop_depth, sites);
            }
            THostCall::BinMatchScan { subject, .. } => {
                collect_cost_expr(subject, function, expr_span, loop_depth, sites);
            }
            THostCall::TypedTextInterp { holes, .. } => {
                for hole in holes {
                    collect_cost_expr(hole, function, expr_span, loop_depth, sites);
                }
            }
            THostCall::ExpectSnapshot { value, .. } => {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
            THostCall::EnvSet { name, value, .. } => {
                collect_cost_expr(name, function, expr_span, loop_depth, sites);
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
            THostCall::ExpiringSecretNew {
                value,
                duration,
                clock,
                ..
            }
            | THostCall::ExpiringValueNew {
                value,
                duration,
                clock,
            } => {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
                collect_cost_expr(duration, function, expr_span, loop_depth, sites);
                collect_cost_expr(clock, function, expr_span, loop_depth, sites);
            }
            THostCall::CCallback { lambda, .. } => {
                collect_cost_lambda(lambda, function, expr_span, loop_depth, sites);
            }
            THostCall::FnName(_)
            | THostCall::GcRead { .. }
            | THostCall::SwitchSubjectField { .. }
            | THostCall::SwitchSubjectValue
            | THostCall::NumericBounds { .. } => {}
        },
        TExprKind::Call { args, .. }
        | TExprKind::StaticCall { args, .. }
        | TExprKind::ModuleCall { args, .. } => {
            collect_cost_call_args(args, function, expr_span, loop_depth, sites);
        }
        TExprKind::DistinctCtor { arg, .. }
        | TExprKind::RangeCheckedCtor { arg, .. }
        | TExprKind::DistinctConvert { arg, .. }
        | TExprKind::Print(arg)
        | TExprKind::Drop(arg)
        | TExprKind::Close(arg)
        | TExprKind::ResourceNew(arg)
        | TExprKind::DistinctRaw(arg)
        | TExprKind::Present(arg)
        | TExprKind::Ok(arg)
        | TExprKind::Err(arg)
        | TExprKind::Deref(arg)
        | TExprKind::RawOf(arg)
        | TExprKind::Clone(arg)
        | TExprKind::ExplicitCopy(arg)
        | TExprKind::MaterializeView(arg) => {
            collect_cost_expr(arg, function, expr_span, loop_depth, sites);
        }
        TExprKind::UnitConvert { arg, rounding, .. } => {
            collect_cost_expr(arg, function, expr_span, loop_depth, sites);
            if let Some((_, rounding)) = rounding {
                collect_cost_expr(rounding, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::MathBuiltin { args, .. } | TExprKind::PreciseBuiltin { args, .. } => {
            for arg in args {
                collect_cost_expr(arg, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::RequireStop { kind, .. } => {
            collect_cost_require(kind, function, expr_span, loop_depth, sites);
        }
        TExprKind::AmbientInput { prompt } => {
            if let Some(prompt) = prompt {
                collect_cost_expr(prompt, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::Binary { lhs, rhs, .. }
        | TExprKind::LayoutCompare { lhs, rhs, .. }
        | TExprKind::NumericBinaryMethod {
            recv: lhs,
            arg: rhs,
            ..
        }
        | TExprKind::OverflowOpt { lhs, rhs, .. } => {
            collect_cost_expr(lhs, function, expr_span, loop_depth, sites);
            collect_cost_expr(rhs, function, expr_span, loop_depth, sites);
        }
        TExprKind::CompareChain { operands, .. } => {
            for operand in operands {
                collect_cost_expr(operand, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::LayoutLit { inner }
        | TExprKind::Unary { operand: inner, .. }
        | TExprKind::Borrow { place: inner, .. } => {
            collect_cost_expr(inner, function, expr_span, loop_depth, sites);
        }
        TExprKind::IncDec { place, .. } => {
            collect_cost_place(place, function, expr_span, loop_depth, sites);
        }
        TExprKind::StructLit { fields, .. } => {
            for (_, value, _) in fields {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::Field { recv, .. }
        | TExprKind::SharedGuardValue { guard: recv, .. }
        | TExprKind::SharedGuardMap { guard: recv, .. }
        | TExprKind::SharedGuardSplit { guard: recv, .. }
        | TExprKind::PtrFromAddr { addr: recv, .. }
        | TExprKind::MathSwizzleRead { recv, .. }
        | TExprKind::PatternMatches { subj: recv, .. }
        | TExprKind::TaskGroupAll { tasks: recv }
        | TExprKind::TaskGroupRace { tasks: recv }
        | TExprKind::TaskGroupAny { tasks: recv } => {
            collect_cost_expr(recv, function, expr_span, loop_depth, sites);
        }
        TExprKind::SharedGuardWait {
            guard,
            condition,
            predicate,
        } => {
            collect_cost_expr(guard, function, expr_span, loop_depth, sites);
            collect_cost_expr(condition, function, expr_span, loop_depth, sites);
            collect_cost_lambda(predicate, function, expr_span, loop_depth, sites);
        }
        TExprKind::ConditionNotify { condition, .. } => {
            collect_cost_expr(condition, function, expr_span, loop_depth, sites);
        }
        TExprKind::EnumLit { payload, .. } => {
            collect_cost_enum_payload(payload, function, expr_span, loop_depth, sites);
        }
        TExprKind::JSONLit { arg, .. } | TExprKind::DBValueLit { arg, .. } => {
            if let Some(arg) = arg {
                collect_cost_expr(&arg.0, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::ListLit(values) | TExprKind::ColumnarListLit { elems: values, .. } => {
            for value in values {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::ListSpread { parts } => {
            for part in parts {
                match part {
                    ListSpreadPart::Elem(value) | ListSpreadPart::Spread(value) => {
                        collect_cost_expr(value, function, expr_span, loop_depth, sites);
                    }
                }
            }
        }
        TExprKind::ColumnarGather { base, index, .. }
        | TExprKind::ColumnarColumnRead { base, index, .. }
        | TExprKind::Index { base, index, .. }
        | TExprKind::IndexHook { base, index, .. }
        | TExprKind::MathLaneIndex { base, index, .. } => {
            collect_cost_expr(base, function, expr_span, loop_depth, sites);
            collect_cost_expr(index, function, expr_span, loop_depth, sites);
        }
        TExprKind::PoolSlot { pool, id, .. } => {
            collect_cost_expr(pool, function, expr_span, loop_depth, sites);
            collect_cost_expr(id, function, expr_span, loop_depth, sites);
        }
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            collect_cost_expr(base, function, expr_span, loop_depth, sites);
            collect_cost_expr(start, function, expr_span, loop_depth, sites);
            collect_cost_expr(end, function, expr_span, loop_depth, sites);
            if let Some(range) = range {
                collect_cost_expr(range, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::MethodCall { recv, args, .. } | TExprKind::FnFieldCall { recv, args, .. } => {
            collect_cost_expr(recv, function, expr_span, loop_depth, sites);
            collect_cost_call_args(args, function, expr_span, loop_depth, sites);
        }
        TExprKind::DecodeUnder { segment, inner } => {
            collect_cost_expr(segment, function, expr_span, loop_depth, sites);
            collect_cost_expr(inner, function, expr_span, loop_depth, sites);
        }
        TExprKind::BuiltinMethod { recv, args, .. }
        | TExprKind::ClosureMethod { recv, args, .. }
        | TExprKind::HandleMethod { recv, args, .. } => {
            collect_cost_expr(recv, function, expr_span, loop_depth, sites);
            for arg in args {
                collect_cost_expr(arg, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::CoreCall { args, .. } => {
            for arg in args {
                collect_cost_expr(arg, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            collect_cost_cond(cond, function, expr_span, loop_depth, sites);
            collect_cost_stmts(then_body, function, expr_span, loop_depth, sites);
            collect_cost_expr(then_value, function, expr_span, loop_depth, sites);
            collect_cost_stmts(else_body, function, expr_span, loop_depth, sites);
            collect_cost_expr(else_value, function, expr_span, loop_depth, sites);
        }
        TExprKind::Try { inner, note, .. } => {
            collect_cost_expr(inner, function, expr_span, loop_depth, sites);
            if let Some(note) = note {
                collect_cost_expr(note, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::OrFallback { value, fallback } => {
            collect_cost_expr_with_state(
                value,
                function,
                expr_span,
                loop_depth,
                sites,
                outcome_cost_state(value),
            );
            match fallback {
                TOrFallback::Value(value) | TOrFallback::Return(Some(value)) => {
                    collect_cost_expr(value, function, expr_span, loop_depth, sites);
                }
                TOrFallback::Panic { msg, .. } => {
                    collect_cost_expr(msg, function, expr_span, loop_depth, sites);
                }
                TOrFallback::Return(None)
                | TOrFallback::Break
                | TOrFallback::Continue
                | TOrFallback::BreakLabel(_)
                | TOrFallback::ContinueLabel(_) => {}
            }
        }
        TExprKind::OptField { base, .. } => {
            collect_cost_expr(base, function, expr_span, loop_depth, sites);
        }
        TExprKind::OptionLift2 { f, a, b } => {
            collect_cost_expr(f, function, expr_span, loop_depth, sites);
            collect_cost_expr(a, function, expr_span, loop_depth, sites);
            collect_cost_expr(b, function, expr_span, loop_depth, sites);
        }
        TExprKind::HostBorrowCallback { callable, .. } => {
            collect_cost_expr(callable, function, expr_span, loop_depth, sites);
        }
        TExprKind::Lambda(lambda) => {
            collect_cost_lambda(lambda, function, expr_span, loop_depth, sites);
        }
        TExprKind::NumericMethod { recv, .. } => {
            collect_cost_expr(recv, function, expr_span, loop_depth, sites);
        }
        TExprKind::SelectRecv { builder, channel } => {
            collect_cost_expr(builder, function, expr_span, loop_depth, sites);
            collect_cost_expr(channel, function, expr_span, loop_depth, sites);
        }
        TExprKind::SelectAfter {
            builder,
            duration,
            value,
        } => {
            collect_cost_expr(builder, function, expr_span, loop_depth, sites);
            collect_cost_expr(duration, function, expr_span, loop_depth, sites);
            if let Some(value) = value {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::SelectWait { builder, .. } => {
            collect_cost_expr(builder, function, expr_span, loop_depth, sites);
        }
        TExprKind::CoreClosureCall { kind } => {
            collect_cost_core_closure(kind, function, expr_span, loop_depth, sites);
        }
        TExprKind::FnValue { kind } => {
            collect_cost_fn_value(kind, function, expr_span, loop_depth, sites);
        }
        TExprKind::ExternCall { args, .. } => {
            for arg in args {
                collect_cost_expr(&arg.value, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::TupleLit { fields, .. } => {
            for (_, value) in fields {
                collect_cost_expr(value, function, expr_span, loop_depth, sites);
            }
        }
        TExprKind::MapLit(fields) => {
            for (left, right) in fields {
                collect_cost_expr(left, function, expr_span, loop_depth, sites);
                collect_cost_expr(right, function, expr_span, loop_depth, sites);
            }
        }
        _ => {}
    }
}

fn collect_cost_cond(
    cond: &TIfCond,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match cond {
        TIfCond::Plain(expr)
        | TIfCond::IfLet { subj: expr, .. }
        | TIfCond::IsNone { subj: expr }
        | TIfCond::Matches { subj: expr, .. } => {
            collect_cost_expr(expr, function, span, loop_depth, sites);
        }
        TIfCond::And { left, right } => {
            collect_cost_cond(left, function, span, loop_depth, sites);
            collect_cost_cond(right, function, span, loop_depth, sites);
        }
        TIfCond::WithPrelude { prelude, cond } => {
            collect_cost_stmts(prelude, function, span, loop_depth, sites);
            collect_cost_cond(cond, function, span, loop_depth, sites);
        }
    }
}

fn collect_cost_stmts(
    stmts: &[TStmt],
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    let mut current_span = span;
    for stmt in stmts {
        if let TStmt::SourceSpan(source_span) = stmt {
            current_span = *source_span;
            continue;
        }
        collect_cost_stmt(stmt, function, current_span, loop_depth, sites);
    }
}

fn collect_cost_stmt(
    stmt: &TStmt,
    function: &str,
    span: crate::Diagnostics::Span,
    loop_depth: usize,
    sites: &mut Vec<TCostSite>,
) {
    match stmt {
        TStmt::Contract { contract } => {
            collect_cost_expr(&contract.condition, function, span, loop_depth, sites);
            collect_cost_expr(&contract.message, function, span, loop_depth, sites);
        }
        TStmt::ContractScope {
            pre, body, post, ..
        } => {
            for contract in pre.iter().chain(post) {
                collect_cost_expr(&contract.condition, function, span, loop_depth, sites);
                collect_cost_expr(&contract.message, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::Let { init, .. }
        | TStmt::TupleDestructure { init, .. }
        | TStmt::StructDestructure { init, .. }
        | TStmt::ListDestructure { init, .. } => {
            collect_cost_expr(init, function, span, loop_depth, sites);
        }
        TStmt::RefutableBind { init, fallback, .. } => {
            collect_cost_expr(init, function, span, loop_depth, sites);
            collect_cost_stmts(fallback, function, span, loop_depth, sites);
        }
        TStmt::GcEdit {
            index_temp, stmt, ..
        } => {
            if let Some((_, index)) = index_temp {
                collect_cost_expr(index, function, span, loop_depth, sites);
            }
            collect_cost_stmt(stmt, function, span, loop_depth, sites);
        }
        TStmt::SplitViews { owner, .. } => {
            if let Some(owner) = owner {
                collect_cost_expr(owner, function, span, loop_depth, sites);
            }
        }
        TStmt::Assign { place, value, .. } => {
            collect_cost_place(place, function, span, loop_depth, sites);
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        TStmt::Return(value) => {
            if let Some(value) = value {
                collect_cost_expr(value, function, span, loop_depth, sites);
            }
        }
        TStmt::ExprStmt(expr) => collect_cost_expr(expr, function, span, loop_depth, sites),
        TStmt::TaskGroup { limit, body, .. } => {
            if let Some(limit) = limit {
                collect_cost_expr(limit, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::DeferClose { close, .. } => {
            collect_cost_expr(close, function, span, loop_depth, sites);
        }
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            collect_cost_cond(cond, function, span, loop_depth, sites);
            collect_cost_stmts(then_body, function, span, loop_depth, sites);
            if let Some(else_body) = else_body {
                collect_cost_stmts(else_body, function, span, loop_depth, sites);
            }
        }
        TStmt::Loop { body, .. } => {
            collect_cost_stmts(body, function, span, loop_depth + 1, sites);
        }
        TStmt::While { cond, body, .. } => {
            collect_cost_expr(cond, function, span, loop_depth + 1, sites);
            collect_cost_stmts(body, function, span, loop_depth + 1, sites);
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            collect_cost_stmt(init, function, span, loop_depth, sites);
            collect_cost_expr(cond, function, span, loop_depth + 1, sites);
            if let Some(step) = step {
                collect_cost_stmt(step, function, span, loop_depth + 1, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth + 1, sites);
        }
        TStmt::Range {
            source,
            start,
            end,
            step,
            body,
            ..
        } => {
            if let Some(source) = source {
                collect_cost_expr(source, function, span, loop_depth, sites);
            }
            collect_cost_expr(start, function, span, loop_depth, sites);
            collect_cost_expr(end, function, span, loop_depth, sites);
            if let Some(step) = step {
                collect_cost_expr(step, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth + 1, sites);
        }
        TStmt::BreakValue { value, .. } => {
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        TStmt::EnumMatch {
            scrutinee,
            arms,
            else_body,
            ..
        } => {
            collect_cost_expr(scrutinee, function, span, loop_depth, sites);
            for arm in arms {
                collect_cost_stmts(&arm.body, function, span, loop_depth, sites);
            }
            if let Some(else_body) = else_body {
                collect_cost_stmts(else_body, function, span, loop_depth, sites);
            }
        }
        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            collect_cost_expr(subject, function, span, loop_depth, sites);
            for (_, _, body) in arms {
                collect_cost_stmts(body, function, span, loop_depth, sites);
            }
            collect_cost_stmts(else_body, function, span, loop_depth, sites);
        }
        TStmt::IndexAssign {
            base,
            index,
            value,
            is_map,
            ..
        } => {
            if *is_map {
                sites.push(TCostSite {
                    function: function.to_string(),
                    span,
                    kind: TCostKind::CollectionCopyOnWrite,
                    state: TCostState::SemanticRemainder,
                    loop_depth,
                });
            }
            collect_cost_expr(base, function, span, loop_depth, sites);
            collect_cost_expr(index, function, span, loop_depth, sites);
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        TStmt::IndexFieldAssign(assign) => {
            if assign.is_map {
                sites.push(TCostSite {
                    function: function.to_string(),
                    span,
                    kind: TCostKind::CollectionCopyOnWrite,
                    state: TCostState::SemanticRemainder,
                    loop_depth,
                });
            }
            collect_cost_expr(&assign.base, function, span, loop_depth, sites);
            collect_cost_expr(&assign.index, function, span, loop_depth, sites);
            collect_cost_expr(&assign.value, function, span, loop_depth, sites);
        }
        TStmt::IndexHookAssign {
            base, index, value, ..
        } => {
            collect_cost_expr(base, function, span, loop_depth, sites);
            collect_cost_expr(index, function, span, loop_depth, sites);
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        TStmt::MathSwizzleAssign { base, value, .. } => {
            collect_cost_expr(base, function, span, loop_depth, sites);
            collect_cost_expr(value, function, span, loop_depth, sites);
        }
        TStmt::ForIn {
            source,
            collection,
            step,
            body,
            ..
        } => {
            collect_cost_expr(source, function, span, loop_depth, sites);
            collect_cost_expr(collection, function, span, loop_depth, sites);
            if let Some(step) = step {
                collect_cost_expr(step, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth + 1, sites);
        }
        TStmt::Inline(body)
        | TStmt::DebugOnly(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Live { body }
        | TStmt::Shield { body } => {
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::Reactive { executable, .. } => {
            collect_cost_lambda(executable, function, span, loop_depth, sites);
        }
        TStmt::ScopeMember { kind, body } => {
            if let ScopeMemberKind::Timeout(timeout) = kind {
                collect_cost_expr(timeout, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            collect_cost_expr(subject, function, span, loop_depth, sites);
            for (condition, body) in arms {
                collect_cost_expr(condition, function, span, loop_depth, sites);
                collect_cost_stmts(body, function, span, loop_depth, sites);
            }
            if let Some(else_body) = else_body {
                collect_cost_stmts(else_body, function, span, loop_depth, sites);
            }
        }
        TStmt::Layout { body, .. } => {
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::ContextBlock { guards, body } => {
            for (_, guard) in guards {
                collect_cost_expr(guard, function, span, loop_depth, sites);
            }
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::Transact { body, .. } => {
            collect_cost_stmts(body, function, span, loop_depth, sites);
        }
        TStmt::SourceSpan(_) | TStmt::LineMarker(_) | TStmt::Break(_) | TStmt::Continue(_) => {}
    }
}

struct TCostCallable {
    module: usize,
    declaration_span: crate::Diagnostics::Span,
    source_span: Option<crate::Diagnostics::Span>,
    body_span: Option<crate::Diagnostics::Span>,
    source_name: Option<String>,
    declaration_name: String,
    method: bool,
    declaration_path: String,
    label: String,
    foreign: bool,
    type_parameterized: bool,
    root: bool,
}

fn find_cost_method(
    methods: &[crate::AST::Func],
    span: crate::Diagnostics::Span,
) -> Option<&crate::AST::Func> {
    methods.iter().find(|method| method.name_span == span)
}

fn find_cost_function(
    items: &[Item],
    span: crate::Diagnostics::Span,
) -> Option<(&crate::AST::Func, bool)> {
    for item in items {
        match item {
            Item::Func(function) if function.name_span == span => return Some((function, false)),
            Item::Struct(definition) => {
                if let Some(function) = find_cost_method(&definition.methods, span) {
                    return Some((function, !definition.type_params.is_empty()));
                }
                for implementation in &definition.trait_impls {
                    if let Some(function) = find_cost_method(&implementation.methods, span) {
                        return Some((function, !definition.type_params.is_empty()));
                    }
                }
            }
            Item::Enum(definition) => {
                if let Some(function) = find_cost_method(&definition.methods, span) {
                    return Some((function, !definition.type_params.is_empty()));
                }
                for implementation in &definition.trait_impls {
                    if let Some(function) = find_cost_method(&implementation.methods, span) {
                        return Some((function, !definition.type_params.is_empty()));
                    }
                }
            }
            Item::Impl(implementation) => {
                let owner_is_generic = items.iter().any(|candidate| match candidate {
                    Item::Struct(definition) => {
                        definition.name == implementation.type_name
                            && !definition.type_params.is_empty()
                    }
                    Item::Enum(definition) => {
                        definition.name == implementation.type_name
                            && !definition.type_params.is_empty()
                    }
                    _ => false,
                });
                if let Some(function) = find_cost_method(&implementation.methods, span) {
                    return Some((function, owner_is_generic));
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(function) = find_cost_function(body, span) {
                        return Some(function);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn selected_cost_output(
    items: &[Item],
    target_module: usize,
    target_span: crate::Diagnostics::Span,
) -> bool {
    items.iter().any(|item| match item {
        Item::Const(value) => value.resolved_output.as_ref().is_some_and(|output| {
            output.selected && output.module == target_module && output.definition == target_span
        }),
        Item::CodeModule(module) => module
            .body
            .as_deref()
            .is_some_and(|body| selected_cost_output(body, target_module, target_span)),
        _ => false,
    })
}

fn cost_callable_is_root(
    bundle: &ProgramBundle,
    declaration: &jet_foundation::Names::NameDeclaration,
    function: Option<&crate::AST::Func>,
    app_graph: Option<&jet_foundation::App::AppGraph>,
) -> bool {
    let Some(function) = function else {
        return false;
    };
    if function.is_job
        || matches!(
            function.web_marker,
            Some(jet_foundation::WebPartition::WebPartitionMarker::WasmExport)
        )
        || (declaration.module == bundle.entry && function.name == "run")
        || bundle
            .modules
            .iter()
            .any(|module| selected_cost_output(&module.items, declaration.module, declaration.span))
    {
        return true;
    }
    if declaration.module != bundle.entry {
        return false;
    }
    let Some(graph) = app_graph else { return false };
    graph
        .routes
        .iter()
        .any(|route| route.handler == function.name)
        || graph
            .actions
            .iter()
            .any(|action| action.handler == function.name)
        || graph
            .mounts
            .iter()
            .any(|mount| mount.handler == function.name)
}

fn cost_callables(
    bundle: &ProgramBundle,
    app_graph: Option<&jet_foundation::App::AppGraph>,
) -> Vec<TCostCallable> {
    let mut callables = Vec::new();
    for declaration in bundle.name_ledger.declarations() {
        if !matches!(declaration.kind.as_str(), "function" | "method" | "extern") {
            continue;
        }
        let function = if declaration.kind == "extern" {
            None
        } else {
            bundle
                .modules
                .get(declaration.module)
                .and_then(|module| find_cost_function(&module.items, declaration.span))
        };
        if declaration.kind != "extern" && function.is_none() {
            continue;
        }
        let (source_span, body_span, source_name, inline_foreign) = function.as_ref().map_or(
            (None, None, None, false),
            |(function, _owner_is_generic)| {
                let inline_foreign = function.inline_foreign.is_some();
                (
                    Some(function.span),
                    (!inline_foreign).then_some(function.span),
                    Some(function.name.clone()),
                    inline_foreign,
                )
            },
        );
        let type_parameterized = function
            .as_ref()
            .is_some_and(|(function, owner_is_generic)| {
                *owner_is_generic || !function.type_params.is_empty()
            });
        let root = cost_callable_is_root(
            bundle,
            declaration,
            function.map(|(function, _)| function),
            app_graph,
        );
        let module_label = bundle
            .modules
            .get(declaration.module)
            .map(|module| module.display.as_str())
            .unwrap_or("<unknown module>");
        callables.push(TCostCallable {
            module: declaration.module,
            declaration_span: declaration.span,
            source_span,
            body_span,
            source_name,
            declaration_name: declaration.name.clone(),
            method: declaration.kind == "method",
            declaration_path: declaration.path.clone(),
            label: format!("{module_label}:{}", declaration.path),
            foreign: declaration.kind == "extern" || inline_foreign,
            type_parameterized,
            root,
        });
    }
    callables
}

fn cost_module_path_matches(bundle: &ProgramBundle, module: usize, path: &str) -> bool {
    bundle
        .name_ledger
        .module_path(module)
        .is_some_and(|candidate| candidate == path)
        || bundle
            .modules
            .get(module)
            .is_some_and(|candidate| candidate.display.as_str() == path)
}

fn reachable_cost_callables(bundle: &ProgramBundle, callables: &[TCostCallable]) -> Vec<bool> {
    let mut reachable = callables
        .iter()
        .map(|callable| callable.root)
        .collect::<Vec<_>>();
    loop {
        let mut changed = false;
        for ((source, start, end), reference) in bundle.name_ledger.references() {
            if reference.kind != "function" {
                continue;
            }
            let caller = callables
                .iter()
                .enumerate()
                .filter(|(_, callable)| {
                    cost_module_path_matches(bundle, callable.module, source)
                        && callable
                            .body_span
                            .is_some_and(|span| *start >= span.start && *end <= span.end)
                })
                .min_by_key(|(_, callable)| {
                    callable
                        .body_span
                        .map_or(usize::MAX, |span| span.end.saturating_sub(span.start))
                })
                .map(|(index, _)| index);
            let Some(caller) = caller else { continue };
            if !reachable[caller] {
                continue;
            }
            let target = callables
                .iter()
                .enumerate()
                .find(|(_, callable)| {
                    callable.declaration_span == reference.def_span
                        && cost_module_path_matches(bundle, callable.module, &reference.module_path)
                })
                .map(|(index, _)| index);
            if let Some(target) = target {
                if !reachable[target] {
                    reachable[target] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            return reachable;
        }
    }
}

fn lowered_cost_method_matches(lowered: &str, expected: &str) -> bool {
    if lowered == expected || lowered.starts_with(&format!("{expected}__generic__")) {
        return true;
    }
    let Some((owner, method)) = expected.rsplit_once("::") else {
        return false;
    };
    lowered.starts_with(&format!("{owner}<")) && lowered.ends_with(&format!("::{method}"))
}

fn lowered_cost_function_matches(lowered: &str, expected: &str) -> bool {
    lowered == expected || lowered.starts_with(&format!("{expected}__va"))
}

fn cost_callable_lowered_name(bundle: &ProgramBundle, callable: &TCostCallable) -> Option<String> {
    if callable.module == bundle.entry {
        return Some(if callable.method {
            callable.declaration_name.replace('.', "::")
        } else {
            callable.declaration_name.clone()
        });
    }
    if callable.method {
        let owner = bundle.name_ledger.module_identity(callable.module)?;
        return Some(format!(
            "{owner}::{}",
            callable.declaration_name.replace('.', "::")
        ));
    }
    let module = bundle.modules.get(callable.module)?;
    let module_alias = bundle.name_ledger.module_alias(callable.module)?;
    let relative_path = callable
        .declaration_path
        .strip_prefix(&format!("{module_alias}."))
        .unwrap_or(callable.declaration_path.as_str());
    let namespace = relative_path
        .rsplit_once('.')
        .map(|(parent, _)| parent.rsplit('.').next().unwrap_or(parent))
        .map_or_else(|| mangle(module.alias.as_str()), mangle);
    let source_name = callable.source_name.as_deref()?;
    Some(format!("{namespace}::{}", mangle(source_name)))
}

fn cost_callable_is_covered(
    bundle: &ProgramBundle,
    callable: &TCostCallable,
    program: &JitProgram,
) -> bool {
    let Some(source_span) = callable.source_span else {
        return false;
    };
    let Some(expected) = cost_callable_lowered_name(bundle, callable) else {
        return false;
    };
    program.funcs.iter().any(|function| {
        if function.source_span != source_span {
            return false;
        }
        if callable.method {
            lowered_cost_method_matches(&function.name, &expected)
        } else {
            lowered_cost_function_matches(&function.name, &expected)
        }
    })
}

fn collect_cost_typed_conversions(
    body: &[crate::AST::Stmt],
    names: &mut std::collections::BTreeSet<String>,
) {
    for statement in body {
        statement.for_each_expr(|expression| {
            if let Expr::Try(
                _,
                _,
                crate::AST::TryConvert::Typed { fn_name, .. },
                _,
            ) = expression
            {
                names.insert(fn_name.clone());
            }
        });
    }
}

fn find_cost_error_conversion<'a>(
    items: &'a [Item],
    name: &str,
) -> Option<&'a crate::AST::ErrorConvDef> {
    for item in items {
        match item {
            Item::ErrorConv(conversion)
                if crate::Sema::error_conv_fn_name(&conversion.from_ty, &conversion.to_ty)
                    == name =>
            {
                return Some(conversion);
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(conversion) = find_cost_error_conversion(body, name) {
                        return Some(conversion);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn cost_error_conversion_gaps(
    bundle: &ProgramBundle,
    program: &JitProgram,
    callables: &[TCostCallable],
    reachable: &[bool],
) -> Vec<String> {
    let mut required = std::collections::BTreeSet::new();
    for (callable, is_reachable) in callables.iter().zip(reachable.iter().copied()) {
        if !is_reachable || callable.foreign {
            continue;
        }
        let Some(module) = bundle.modules.get(callable.module) else {
            continue;
        };
        let Some((function, _)) = find_cost_function(&module.items, callable.declaration_span)
        else {
            continue;
        };
        collect_cost_typed_conversions(&function.body, &mut required);
    }

    // A conversion body is itself checked code. Keep following its typed
    // conversions so a filtered conversion cannot hide a second omitted body.
    loop {
        let mut changed = false;
        for name in required.iter().cloned().collect::<Vec<_>>() {
            let Some(conversion) = bundle
                .modules
                .iter()
                .find_map(|module| find_cost_error_conversion(&module.items, &name))
            else {
                continue;
            };
            let before = required.len();
            collect_cost_typed_conversions(&conversion.body, &mut required);
            changed |= required.len() != before;
        }
        if !changed {
            break;
        }
    }

    required
        .into_iter()
        .filter_map(|name| {
            let conversion = bundle
                .modules
                .iter()
                .enumerate()
                .find_map(|(module, data)| {
                    find_cost_error_conversion(&data.items, &name)
                        .map(|conversion| (module, conversion))
                });
            let Some((module, conversion)) = conversion else {
                return Some(format!(
                    "typed error conversion `{name}` has no checked declaration"
                ));
            };
            let covered = program.funcs.iter().any(|function| {
                function.name == name && function.source_span == conversion.from_span
            });
            if covered {
                return None;
            }
            let module_label = bundle
                .modules
                .get(module)
                .map(|module| module.display.as_str())
                .unwrap_or("<unknown module>");
            Some(format!(
                "{module_label}:{name} (reachable typed error conversion has no complete TIR body)"
            ))
        })
        .collect()
}

fn cost_coverage_gaps(bundle: &ProgramBundle, program: &JitProgram) -> Vec<String> {
    let app_graph = crate::Sema::extract_app_graph(bundle).0;
    let callables = cost_callables(bundle, app_graph.as_ref());
    let reachable = reachable_cost_callables(bundle, &callables);
    let mut gaps = callables
        .iter()
        .zip(reachable.iter().copied())
        .filter_map(|(callable, reachable)| {
            (reachable && !cost_callable_is_covered(bundle, callable, program)).then(|| {
                let reason = if callable.foreign {
                    "foreign callable has no typed TIR body"
                } else if callable.type_parameterized {
                    "type-parameterized callable has no complete TIR specialization"
                } else {
                    "sema-checked callable is outside typed TIR coverage"
                };
                format!("{} ({reason})", callable.label)
            })
        })
        .collect::<Vec<_>>();
    gaps.extend(cost_error_conversion_gaps(
        bundle, program, &callables, &reachable,
    ));
    gaps.sort();
    gaps.dedup();
    gaps
}

/// Project typed cost facts from the frozen TIR.  Lowering remains the sole
/// producer of the fact channel; CLI tooling and linting consume this one
/// report instead of reconstructing cost from emitted Rust.
pub fn cost_report(bundle: &ProgramBundle) -> Result<TCostReport, TCostReportError> {
    let Some(program) = lower_jit_program(bundle) else {
        let reason = lower_jit_program_fail_reason(bundle);
        if matches!(reason.as_str(), NO_RUNNABLE_ENTRY | CLI_ENTRY_MISSING_RUN) {
            return Ok(TCostReport::default());
        }
        return Err(TCostReportError::Lowering { reason });
    };
    let gaps = cost_coverage_gaps(bundle, &program);
    if !gaps.is_empty() {
        return Err(TCostReportError::Incomplete { surfaces: gaps });
    }
    let mut report = TCostReport::default();
    for function in &program.funcs {
        collect_cost_stmts(
            &function.body,
            &function.name,
            function.source_span,
            0,
            &mut report.sites,
        );
    }
    Ok(report)
}

/// The result of the read-only lowering coverage pass used by project checks.
///
/// This is deliberately separate from [`cost_report`].  The cost projection
/// consumes a completed JIT program; a check must not use that lowerer's
/// omission behavior as a coverage oracle.  This pass walks the sema-owned
/// callable denominator and asks the same TIR coverage predicates that the
/// emitters use, without emitting Rust, invoking rustc, or starting a runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TirCoverageIssue {
    /// Adapter whose checked route could not cover the callable.
    pub tier: &'static str,
    /// Stable sema declaration label, including its module path.
    pub callable: String,
    /// The structural refusal or missing route that needs attention.
    pub reason: String,
    /// Source declaration span for the diagnostic row.
    pub span: crate::Diagnostics::Span,
}

/// Validate the native AOT and default JIT TIR routes without probing either
/// emitter.  `AOT` validates every callable the native item emitter would
/// visit; `JIT` validates only sema-reachable callables, matching the existing
/// call-graph denominator used by cost coverage.
pub fn validate_tir_support(bundle: &ProgramBundle) -> Vec<TirCoverageIssue> {
    let app_graph = crate::Sema::extract_app_graph(bundle).0;
    let callables = cost_callables(bundle, app_graph.as_ref());
    let reachable = reachable_cost_callables(bundle, &callables);
    let mut contexts = std::collections::HashMap::new();
    let mut issues = Vec::new();

    for (index, callable) in callables.iter().enumerate() {
        if callable.foreign {
            // Foreign and inline-foreign bodies are checked by their bridge,
            // not by Jet TIR. They remain in the denominator for graph
            // reachability but are not a TIR coverage miss.
            continue;
        }
        let Some(module) = bundle.modules.get(callable.module) else {
            issues.push(TirCoverageIssue {
                tier: "AOT",
                callable: callable.label.clone(),
                reason: "callable points at a missing module".to_string(),
                span: callable.declaration_span,
            });
            if reachable[index] {
                issues.push(TirCoverageIssue {
                    tier: "JIT",
                    callable: callable.label.clone(),
                    reason: "callable points at a missing module".to_string(),
                    span: callable.declaration_span,
                });
            }
            continue;
        };
        let cx = contexts.entry(callable.module).or_insert_with(|| {
            let extern_funcs = bundle_extern_funcs(bundle);
            let mut cx = build_cx_items(
                &module.items,
                &module.source,
                &module.display,
                None,
                &extern_funcs,
            );
            populate_cx_from_bundle(&mut cx, bundle, callable.module);
            register_foreign_enum_variants(&mut cx, bundle, callable.module);
            update_cloneability_with_foreign_types(&mut cx, &module.items);
            cx
        });
        let outcome = validate_tir_callable(&module.items, callable.declaration_span, cx);
        match outcome {
            Some(Ok(())) => {}
            Some(Err(reason)) => {
                issues.push(TirCoverageIssue {
                    tier: "AOT",
                    callable: callable.label.clone(),
                    reason: reason.clone(),
                    span: callable.declaration_span,
                });
                if reachable[index] {
                    issues.push(TirCoverageIssue {
                        tier: "JIT",
                        callable: callable.label.clone(),
                        reason,
                        span: callable.declaration_span,
                    });
                }
            }
            None => {
                let reason = "checked callable has no TIR validation route".to_string();
                issues.push(TirCoverageIssue {
                    tier: "AOT",
                    callable: callable.label.clone(),
                    reason: reason.clone(),
                    span: callable.declaration_span,
                });
                if reachable[index] {
                    issues.push(TirCoverageIssue {
                        tier: "JIT",
                        callable: callable.label.clone(),
                        reason,
                        span: callable.declaration_span,
                    });
                }
            }
        }
    }

    issues.sort_by(|left, right| {
        left.tier
            .cmp(right.tier)
            .then(left.callable.cmp(&right.callable))
            .then(left.span.start.cmp(&right.span.start))
    });
    issues.dedup();
    issues
}

/// Return `Some(Ok)` when the declaration is on the checked TIR route,
/// `Some(Err)` when the route is known but unsupported, and `None` when the
/// sema declaration cannot be matched to an AST item.
fn validate_tir_callable(
    items: &[Item],
    declaration_span: crate::Diagnostics::Span,
    cx: &mut Cx,
) -> Option<Result<(), String>> {
    for item in items {
        match item {
            Item::Func(function) if function.name_span == declaration_span => {
                if function.inline_foreign.is_some() {
                    return Some(Ok(()));
                }
                if tir_covers(function, cx) {
                    return Some(Ok(()));
                }
                return Some(Err(format!(
                    "function is outside tir_covers ({})",
                    refusal::describe(cx)
                )));
            }
            Item::Struct(definition) => {
                if let Some(method) = definition
                    .methods
                    .iter()
                    .find(|method| method.name_span == declaration_span)
                {
                    return Some(validate_tir_method(
                        method,
                        &definition.name,
                        None,
                        false,
                        cx,
                    ));
                }
                for implementation in &definition.trait_impls {
                    if let Some(method) = implementation
                        .methods
                        .iter()
                        .find(|method| method.name_span == declaration_span)
                    {
                        return Some(validate_tir_method(
                            method,
                            &definition.name,
                            Some(implementation.trait_name.as_str()),
                            implementation.compiler_generated,
                            cx,
                        ));
                    }
                }
            }
            Item::Enum(definition) => {
                if let Some(method) = definition
                    .methods
                    .iter()
                    .find(|method| method.name_span == declaration_span)
                {
                    return Some(validate_tir_method(
                        method,
                        &definition.name,
                        None,
                        false,
                        cx,
                    ));
                }
                for implementation in &definition.trait_impls {
                    if let Some(method) = implementation
                        .methods
                        .iter()
                        .find(|method| method.name_span == declaration_span)
                    {
                        return Some(validate_tir_method(
                            method,
                            &definition.name,
                            Some(implementation.trait_name.as_str()),
                            implementation.compiler_generated,
                            cx,
                        ));
                    }
                }
            }
            Item::Impl(implementation) => {
                if implementation
                    .os_target
                    .is_some_and(|target| target != cx.active_os)
                {
                    if implementation
                        .methods
                        .iter()
                        .any(|method| method.name_span == declaration_span)
                    {
                        return Some(Ok(()));
                    }
                    continue;
                }
                if let Some(method) = implementation
                    .methods
                    .iter()
                    .find(|method| method.name_span == declaration_span)
                {
                    return Some(validate_tir_method(
                        method,
                        &implementation.type_name,
                        implementation.trait_name.as_deref(),
                        implementation.is_generated_serde,
                        cx,
                    ));
                }
            }
            Item::CodeModule(module) => {
                if let Some(body) = &module.body {
                    if let Some(result) = validate_tir_callable(body, declaration_span, cx) {
                        return Some(result);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn validate_tir_method(
    method: &crate::AST::Func,
    owner: &str,
    trait_name: Option<&str>,
    compiler_generated: bool,
    cx: &mut Cx,
) -> Result<(), String> {
    let covered = match trait_name {
        Some(trait_name) => {
            tir_covers_trait_method(method, owner, cx, trait_name)
                || (compiler_generated && tir_covers_compiler_derive_method(method, cx))
        }
        None => tir_covers_method(method, owner, cx),
    };
    if covered {
        Ok(())
    } else {
        Err(format!(
            "method is outside tir_covers ({})",
            refusal::describe(cx)
        ))
    }
}

/// D-MEM-COPYSEM1=A: the ONE mapping from a `MaterializeView` source type to
/// the shared Prelude materialization symbol (`Prelude/Core/ViewCopy.rs` for
/// native/wasm, `Prelude/Core/ViewCopy.js` for the web tier). Per I9 no engine
/// re-derives it: AOT emit, the wasm and JS web emitters, and lambda-capture
/// lowering all read this table, so a `String` window and a `[T]` window can
/// never disagree about which kernel copies them. Mirrors sema's
/// `owned_type_for_read_view`, which decides the *type* of the same store.
///
/// A string window is the fallback because sema only builds `MaterializeView`
/// for a proven read window: a `View<str>`, a `Type::String` local flagged
/// `string_view`, or a range place. The first two are strings, so anything
/// that is not list-shaped here is the string case.
pub fn view_copy_symbol(source: &Type) -> &'static str {
    match source {
        Type::Apply { name, args } if name == "View" => {
            if matches!(args.as_slice(), [Type::Named(element)] if element == "str") {
                "jet_string_view_copy"
            } else {
                "jet_view_copy"
            }
        }
        Type::List(_) | Type::FixedList { .. } => "jet_view_copy",
        _ => "jet_string_view_copy",
    }
}

/// D-MEM-COPYSEM1=A: the owned destination type a `View<T>` window
/// materializes into, paired with `view_copy_symbol` so a capture store cannot
/// pick one without the other. `None` means the source is not a declared view
/// window and the caller keeps its own type.
pub fn view_copy_owned_type(source: &Type) -> Option<Type> {
    let Type::Apply { name, args } = source else {
        return None;
    };
    if name != "View" || args.len() != 1 {
        return None;
    }
    if matches!(&args[0], Type::Named(element) if element == "str") {
        Some(Type::String)
    } else {
        Some(Type::List(Box::new(args[0].clone())))
    }
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
    /// Compiler-private result block used by finite yielding loops. Unlike a
    /// lambda, it executes in the current function, so `return` and cleanup
    /// retain ordinary loop semantics.
    InlineBlock(Vec<TStmt>),
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
        /// D-GENERIC-CALL1=A: explicit source-level type arguments. This is
        /// separate from value arguments so result-only generics retain their
        /// specialization in every execution tier.
        type_args: Vec<Type>,
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
    /// postfix `?`. Emits `__jet_T::try_new(arg)` returning `Result<__jet_T,
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
        /// First-order relative standard uncertainty contributed by measured
        /// unit scales. `None` keeps the ordinary exact quantity result.
        relative_uncertainty: Option<f64>,
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
    /// D-DECIMAL1 / D-NUMTYPE1: precise numeric ctor/method/binop.
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
    /// byte-for-byte the ambient-input call branch. `prompt` is `Some` when a String
    /// prompt arg is given, else `None`.
    /// parity: guard tests/tir_language_features.rs::ambient_input
    AmbientInput {
        prompt: Option<Box<TExpr>>,
    },
    /// c109 Phase 26: an `assert(cond[, msg])` / `assert_eq(a, b)` / `panic(msg)`
    /// rich-runtime-report builtin (S36). Structured facts only — emit formats
    /// `jet_panic_rich` / test-mode `return Err` (I3: no `cx.src` re-read for
    /// location; `loc` was captured at lowering).
    RequireStop {
        kind: TRequireKind,
        loc: TPanicLoc,
        /// True only for the unconditional builtin `panic(...)`; `assert`
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
        /// c109 Phase 17: injected prelude fields (HTTPRequest route metadata).
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
    /// D-SHAREDGUARD2=A: compiler-known `guard.value` projection. The public
    /// guard has no Rust field; it dereferences its owned lock lease.
    SharedGuardValue {
        guard: Box<TExpr>,
        editable: bool,
    },
    /// D-SHAREDGUARD1=A: consume one guard and retain the same lock lease while
    /// narrowing its visible value to a sema-validated field path.
    SharedGuardMap {
        guard: Box<TExpr>,
        path: Vec<String>,
        editable: bool,
    },
    /// D-SHAREDGUARD1=A: consume one guard and create two disjoint views backed
    /// by the same lock lease. Sema proved the paths do not overlap.
    SharedGuardSplit {
        guard: Box<TExpr>,
        first: Vec<String>,
        second: Vec<String>,
        editable: bool,
    },
    /// D-SHAREDGUARD1=A: wait on a condition while retaining ownership of the
    /// edit guard. The runtime performs register/release/park/reacquire/recheck.
    SharedGuardWait {
        guard: Box<TExpr>,
        condition: Box<TExpr>,
        predicate: Box<TLambda>,
    },
    /// D-SHAREDGUARD1=A: wake one or every waiter registered on a Condition.
    ConditionNotify {
        condition: Box<TExpr>,
        all: bool,
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
    /// named-payload `Variant { f: v, … }`. The Rust head (`__jet_Enum::__jet_Variant`)
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
    /// (`{root}jet_std::JSON::Object`, NOT `__jet_…`), distinct from a user enum's
    /// `EnumLit`. `variant` is the bare variant name (`Object`/`Text`/…). `arg` is the
    /// payload `TExpr` plus the resolved `implicit_clone` flag (sema's `CallArg.flags`,
    /// total) — `true` → `(…).clone()`, reproducing `emit_core_json_lit` (Expression.rs)
    /// byte-for-byte. `JSON.Null` has no arg (`None`). The `{root}jet_std::JSON` prefix
    /// is rendered at emit (`cx.root_prefix` is program-level, read there).
    /// parity: guard tests/tir_language_features.rs::json_value_construct_match_render
    JSONLit {
        variant: String,
        arg: Option<Box<(TExpr, bool)>>,
    },
    /// D-DBDRIVER1: a `DBValue` construction (`DBValue.Int(n)` / `.Float(f)` /
    /// `.Text(s)` / `.Bool(b)` / `.Null`) — the tagged SQL parameter/column value.
    /// Same shape as `JSONLit` (a FOREIGN prelude enum, not a user `EnumLit`), kept
    /// as its own node rather than reusing `JSONLit` because `DBValue` renders to
    /// a DIFFERENT prelude type (`jet_std::DBValue`, not `jet_std::DataTree`) and
    /// has no recursive `Array`/`Object`-style payload to special-case.
    DBValueLit {
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
    /// D-SOA1 / D-SOA-TIER1=A: a list literal whose element is a
    /// `#layout(columnar)` struct `S`. The elements build the array-of-structs,
    /// then the shared column store scatters them across the columns.
    /// `columns_ty` is the resolved `[S]` Rust type — the Prelude-owned
    /// `JetColumnList<S>`, never a per-struct storage type.
    ColumnarListLit {
        columns_ty: String,
        elems: Vec<TExpr>,
    },
    /// D-SOA1 / D-SOA-TIER1=A: index-read `xs[i]` on a columnar list — pulls the
    /// logical `S` out of the columns at `i` through THE shared gather read,
    /// bounds-checked with the same list stop an array-of-structs `xs[i]` uses.
    ColumnarGather {
        base: Box<TExpr>,
        index: Box<TExpr>,
        line: usize,
    },
    /// D-SOA1 / D-SOA-TIER1=A: a fused `xs[i].field` field-read on a columnar
    /// list — one cell straight out of that field's column, with no whole-`S`
    /// gather. This is the cache-friendly fast path.
    ///
    /// `column` is the field's position in declaration order among STORED
    /// fields, which is exactly the column order the store was built with
    /// (a computed field is never a column, D-FIELDPOL1). `accessor` unwraps
    /// that column's cell back to the field's own type; every tier resolves the
    /// same column index, and only the accessor is engine-specific.
    ColumnarColumnRead {
        base: Box<TExpr>,
        index: Box<TExpr>,
        column: usize,
        accessor: String,
        line: usize,
    },
    /// c109 Phase 23: a named-tuple literal `(x: 1, y: 2)` (S73/D-SG7). The generated
    /// struct name (`JetTup_<hash>`) and the CANONICAL field order are resolved at
    /// lowering from the literal's sema-attached `Type::Tuple`; each field's value is
    /// reordered to that canonical order (a `(y: 3, x: 4)` literal becomes
    /// `JetTup_…{ __jet_x: 4, __jet_y: 3 }`). Reproduces `emit_expr`'s `TupleLit` arm
    /// byte-for-byte — `struct_name { __jet_<f>: <v>, … }`. `fields` are the already
    /// mangled-name + lowered-value pairs in canonical order.
    TupleLit {
        struct_name: String,
        fields: Vec<(String, TExpr)>,
    },
    /// c109 Phase 5: a map literal `[k: v, …]` or empty `[:]`. A map remains
    /// one typed value through TIR; each engine constructs it from these
    /// ordered pairs, so nested maps are ordinary value expressions.
    MapLit(Vec<(TExpr, TExpr)>),
    /// c109 Phase 5: indexing `coll[i]` (`Expr::Index`). `is_map` is the resolved
    /// `IndexKind` carried TOTALLY from sema (never re-inferred): `true` → the
    /// `jet_index_map` helper, `false` → `jet_index_vec`. `line` is the source line
    /// for the bounds/missing-key panic message, resolved at lowering.
    Index {
        base: Box<TExpr>,
        index: Box<TExpr>,
        is_map: bool,
        /// The base uses the vetted `JetUninitFixed` prelude wrapper in AOT.
        uninit_fixed: bool,
        line: usize,
    },
    /// D-MEM1 S6: `pool[id]` / `pool[id].field` — a generation-checked slot in a
    /// `Pool<T>`. `mutable` selects the in-place `jet_pool_get_mut` place over the
    /// `jet_pool_get` value clone, so a write or a mutating receiver edits the
    /// stored element instead of a throwaway copy. `field_rust` narrows the place
    /// to one mangled field. `src_line` is captured at lowering so every engine
    /// receives the stop's own source line rather than a function-frame fallback.
    PoolSlot {
        pool: Box<TExpr>,
        id: Box<TExpr>,
        mutable: bool,
        /// Jet field name when narrowing `pool[id].field`; emit mangles.
        field: Option<String>,
        line: usize,
        src_line: String,
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
    /// An owned slice expression. Place contexts lower the same source shape to
    /// `ViewNew` or `ViewMutNew`; explicit copy stays here. `line` identifies
    /// bounds failures.
    Slice {
        base: Box<TExpr>,
        start: Box<TExpr>,
        end: Box<TExpr>,
        /// D-RANGE-VALUE1=A: a stored Range source, evaluated once.
        range: Option<Box<TExpr>>,
        line: usize,
    },
    /// c109 Phase 6: the sema-inserted `.clone()` on an owning non-Copy field read
    /// or borrowed value. This is ordinary sharing/cloning semantics. The
    /// user-written `~` copy has its own `ExplicitCopy` node so a Tensor does not
    /// silently turn compiler-inserted clones into deep storage copies.
    Clone(Box<TExpr>),
    /// D-MEM1/D-CAP2: the explicit Jet `~` copy signal. Backends route Tensor
    /// values through the shared Prelude copy operation; non-Tensor values keep
    /// their ordinary clone/materialization semantics.
    ExplicitCopy(Box<TExpr>),
    /// D-SHAPE-PLACE1=A: a checked local whole/field/index place borrow.
    /// Range places use `ViewNew`/`ViewMutNew` so bounds are checked once.
    Borrow {
        place: Box<TExpr>,
        mutable: bool,
    },
    /// D-MEM-COPYSEM1: an explicit `~` or an implicit read-only view crossing
    /// an owning destination materializes the owned target through the shared
    /// Prelude. A plain `.clone()` is not sufficient for string views: cloning
    /// a `&str` hands back another `&str`, not the owned `String` needed to leave
    /// the view's scope. Generic `View<T>` uses the same path to produce `[T]`.
    MaterializeView(Box<TExpr>),
    /// c109 Phase 6: a user-defined instance method call `recv.method(args)` on a
    /// covered struct/enum. All dispatch facts are resolved at lowering (totality):
    /// `recv` is the lowered receiver (emitted as the AST path emits it — autoref
    /// handles `&self`/`&mut self`/`self`); `method_rust` is the already-resolved
    /// Rust method name (mangled `__jet_<m>`, or the bare name for a trait-impl
    /// method, decided here from `cx.trait_methods`); each arg carries its
    /// borrow/clone decisions, mirroring `emit_call_args`.
    MethodCall {
        recv: Box<TExpr>,
        method: TMethodRef,
        /// D-GENERIC-CALL1=A: resolved method-owned call arguments, explicit or
        /// inferred. Empty for a non-generic method.
        type_args: Vec<Type>,
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
    /// `field_rust` is the mangled `__jet_<field>`; args emit PLAINLY (the AST passes
    /// `None` to `emit_call_args` — no param convention, only each arg's own clone flags).
    FnFieldCall {
        recv: Box<TExpr>,
        /// Jet field name; emit mangles.
        field: String,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 7: a STATIC (associated) method call `Type.make(args)`. Resolved
    /// at lowering to `__jet_<Type>::__jet_<method>(args)` — `type_prefix` is the
    /// already-resolved Rust type head (`__jet_<Type>`), `method_rust` the mangled
    /// method name. Mirrors the AST type-name dispatch (Expression.rs ~L1644).
    StaticCall {
        owner: TStaticOwner,
        owner_type: Option<Type>,
        method: TMethodRef,
        /// D-GENERIC-CALL1=A: resolved method-owned call arguments on a static
        /// method, explicit or inferred. Receiver/owner arguments live in
        /// `owner_type`.
        type_args: Vec<Type>,
        args: Vec<TCallArg>,
    },
    /// D-VALIDATE-DECODE1=B: prefix every error in a typed child result while
    /// preserving its success value. The segment and result are already
    /// lowered; every execution tier applies the same list transform.
    DecodeUnder {
        segment: Box<TExpr>,
        inner: Box<TExpr>,
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
    /// is a core import (`cx.core_imports`). The `(module, method)` dispatch
    /// is a pure syntactic match on two already-resolved strings — NO type inference
    /// (I3) — so the TIR carries
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
    /// D-FAIL-BREACH1=A: a `#Todo` typed goal (`Expr::Todo`, D-TOOL2, E2-M11)
    /// emits the registered E3011 Prelude stop. The `expected_type` is the total
    /// sema fact; `line` is the source line resolved at lowering.
    Todo {
        line: usize,
        expected_type: String,
    },
    /// Card #1440: the synthesized dead end of an else-less exhaustive value
    /// dispatch (`Expr::NoElse`). Sema proved the pattern arms cover the
    /// subject's whole type (E0307), so no execution reaches it on any tier —
    /// AOT emits a diverging `unreachable!(…)`, JIT traps, TIR-eval errors.
    Unreachable {
        line: usize,
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
    /// c109 Phase 8: `Ok(x)` — a success value of `T !E` (`Ok(x)`).
    Ok(Box<TExpr>),
    /// c109 Phase 8: `Err(e)` — a failure value of `T !E` (`Err(e)`).
    Err(Box<TExpr>),
    /// c109 Phase 8: the `?` propagation operator (`Expr::Try`). The error
    /// conversion (`convert`) is the TOTAL sema fact (`TryConvert`): a `None` is a
    /// bare propagate or a declared typed conversion calls the declared
    /// conversion. The frame-trace location (`file`, `line`, `fn_name`) is
    /// resolved at lowering so the emitted `jet_trace_err(…)?` matches the AST path
    /// byte-for-byte (the emitter never reads `cx.current_fn`/`cx.src`).
    Try {
        inner: Box<TExpr>,
        /// Optional D-FAIL-CTX1 note. Lowered as a closure/cold branch so its
        /// interpolation is never evaluated when the operand succeeds.
        note: Option<Box<TExpr>>,
        convert: TTryConvert,
        /// Pre-escaped Rust string literal for the source file (`escape_rust_str`).
        file: String,
        line: usize,
        /// Pre-escaped Rust string literal for the enclosing function name.
        fn_name: String,
    },
    /// c109 Phase 8: the `??` fallback operator (`Expr::OrFallback`).
    /// D-FAIL-CARRIER1=A: one carrier, so one lowering —
    /// `match … { Ok(v) => v, Err(_) => fb }` reads `?T` and `T !E` alike.
    /// The fallback is a value or an early `return` (the panic form is deferred —
    /// its `safe_locals_expr` reproduction is out of subset).
    OrFallback {
        value: Box<TExpr>,
        fallback: TOrFallback,
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
    /// `prep` holds the per-`cloned_capture` `let __jet_cap_<n> = (place).clone();`
    /// prelude (resolved from the *outer* env at lowering, since the cap's source
    /// place is an outer local); `params` is the already-rendered `name[: ty]` list;
    /// `body` is the lowered closure body; `is_move`/`boxed` reproduce the AST path's
    /// `move ` keyword (off `needs_fn_mut`/`escapes`) and `Box::new(…)` (off `escapes`)
    /// wrappers. The whole thing is wrapped in `{ <prep> <closure> }` when `prep` is
    /// non-empty — byte-for-byte the TIR lambda encoding.
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
    /// Adapt a Jet callback to a collection helper's borrowed host inputs.
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
    /// D-GO127-STDLIB1=A: a binary exact-Int method whose operation is
    /// resolved at lowering and whose right-hand operand remains a TIR child.
    /// Keeping the operand in its own node preserves the nullary NumericMethod
    /// shape used by predicates and conversions.
    NumericBinaryMethod {
        recv: Box<TExpr>,
        op: TNumericOp,
        arg: Box<TExpr>,
    },
    /// c109 Phase 28: an overflow opt-out builtin `wrapping(e)`/`saturating(e)`/
    /// `checked(e)` (D-NUMOPS1). The single integer `Expr::Binary` argument lowers to
    /// Rust's matching
    /// method: `(lhs).{prefix}_{op}(rhs)` where `prefix ∈ {wrapping, saturating,
    /// checked}` and `op ∈ {add, sub, mul, div, rem}`, or a standard fixed-width
    /// rotation with `prefix ∈ {rotate_left, rotate_right}`. PLAIN operands (no
    /// overflow trap).
    /// `prefix` + `op` are resolved at lowering (total facts), emit only assembles.
    OverflowOpt {
        prefix: String,
        op: &'static str,
        line: u32,
        /// D-WRAP-SCOPE1=A: the lexical policy fact that caused this node.
        /// Explicit receiver methods have no policy fact.
        policy: Option<crate::AST::ArithmeticPolicyFact>,
        lhs: Box<TExpr>,
        rhs: Box<TExpr>,
    },
    /// c109 Phase 13: a method ON a handle (FileReader/FileWriter/StdinHandle/
    /// Stopwatch/TcpListener/TcpStream/HTTPRequest/HTTPResponse) — the handle arms of
    /// built-in method lowering. The handle-receiver
    /// dispatch (`rty == Some(Named(<handle>))`) is resolved at lowering into a total
    /// `THandleOp`, so emit makes no type decision (I3). Args are emitted PLAINLY
    /// (`emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    HandleMethod {
        recv: Box<TExpr>,
        op: THandleOp,
        args: Vec<TExpr>,
    },
    /// c109 Phase 13: a closure-taking core/stdlib call.
    /// `task`, `http.serve`, and `scope.guard` are NOT in `core_fixed_sig` and each
    /// has a bespoke emit shape the plain `CoreCall` cannot reproduce: `task` wraps a
    /// `emit_spawn_lambda` (`move |…|`,
    /// NEVER `Box::new`) in `JetTask::spawn(…)`; `serve` (lambda handler) emits
    /// `jet_http_serve(&(addr), <lambda>)`; `guard` emits `jet_scope_guard(<lambda>)`.
    /// The closure body is lowered + rendered at lowering (the lambda is in subset —
    /// Phase 11), so emit only assembles. `kind` selects the bespoke shape.
    CoreClosureCall {
        kind: TCoreClosureKind,
    },
    /// D-CONC-SPAWN1=D: `task.all { … }` — join every child, collect results.
    TaskGroupAll {
        tasks: Box<TExpr>,
    },
    /// D-CONC-SPAWN1=D: `task.race { … }` — first successful child wins.
    TaskGroupRace {
        tasks: Box<TExpr>,
    },
    /// D-CONC-SPAWN1=D: `task.any { … }` — first completed child wins.
    TaskGroupAny {
        tasks: Box<TExpr>,
    },
    /// Compiler-private readiness-table carrier. Source `if { ... }` lowers
    /// to this chain without exposing a public builder spelling.
    SelectStart,
    /// Compiler-private receiver arm in the readiness-table carrier.
    SelectRecv {
        builder: Box<TExpr>,
        channel: Box<TExpr>,
    },
    /// Compiler-private timer arm in the readiness-table carrier.
    SelectAfter {
        builder: Box<TExpr>,
        duration: Box<TExpr>,
        value: Option<Box<TExpr>>,
    },
    /// D-CONC-CHAN2=D: a readiness table with `else` uses the same tagged door
    /// without parking; `nonblocking` is compiler-owned, not user syntax.
    SelectWait {
        builder: Box<TExpr>,
        nonblocking: bool,
    },
    /// c109 Phase 13: a fn-typed-VALUE form. Either a bare function name used as a
    /// value (`Expr::Ident` resolving to a top-level fn) or a call THROUGH a fn-value
    /// (`Expr::CallValue` / sema's `.call(args)` projection). A bare fn-name value emits the
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
        /// Effective return type of the lowered target function. `ty` remains the
        /// source-visible call type; the AOT emitter uses this separate fact only
        /// to adapt a hidden `Result` carrier to its raw success payload.
        target_return: Option<Type>,
        /// D-GENERIC-CALL1=A: explicit generic arguments for the target.
        type_args: Vec<Type>,
        args: Vec<TCallArg>,
    },
    /// c109 Phase 14: an FFI extern call (`extern rust`/`extern C`). `emit_call`'s
    /// `extern_funcs` arm emits `{ffi_crate}::{wrapper}(args)` with args lowered via
    /// `emit_extern_call_args` (a DISTINCT arg form — a non-scalar `Read` param is
    /// `(…).clone()`, NOT `&(…)`). `wrapper` is the resolved FFI symbol; `args` carry
    /// the resolved per-arg clone decision. `c_abi` marks the hidden C bridge,
    /// whose scalar boundary needs the Prelude's Jet/C conversion at the call
    /// site. `cx.ffi_crate` is program-level (read at emit, like Phase 10's regex
    /// form). I1: an extern call introduces no Rust
    /// `unsafe` by itself — this reproduces the AST emit byte-for-byte, which emits no
    /// `unsafe`.
    ExternCall {
        wrapper: String,
        c_abi: bool,
        args: Vec<TExternArg>,
    },
}

/// c109 Phase 14: a resolved cross-module call form. Each variant pre-resolves the
/// path pieces of one `emit_call`/`emit_method_call` module-call arm; emit prepends
/// `cx.root_prefix` exactly where the AST path does (or omits it where the AST does).
pub enum TModuleCallForm {
    /// `import_mods` qualified call (`mod.fn()`) and `reexport_calls` (`pub use`) —
    /// both emit `{root}{rust_mod}::{rust_fn}(args)`. `rust_mod` is the resolved Rust
    /// module name (`__jet_<stem>`); `rust_fn` is the mangled function name.
    Qualified { rust_mod: String, rust_fn: String },
    /// `code_modules` qualified call (`alias.method()`) and unqualified inline import —
    /// both emit `{root}__jet_{mangled}(args)` where `mangled` is `alias__method`.
    InlineMangled { mangled: String },
}

/// c109 Phase 14: a resolved FFI extern call argument (see `TExprKind::ExternCall`).
/// `emit_extern_call_args` wraps the value in `(…).clone()` when the arg has an
/// `implicit_clone` flag OR its param is a non-scalar `Read` (resolved here into one
/// total `clone` bool; the `shared_auto_clone`/Arc form is excluded from the subset).
pub struct TExternArg {
    pub value: TExpr,
    pub clone: bool,
    /// D-FFI-CAP1: an explicit `&` foreign parameter keeps exclusive access
    /// through the call instead of silently becoming a cloned value.
    pub mut_borrow: bool,
}

/// c109 Phase 13: the closure-taking core-call shapes (see
/// `TExprKind::CoreClosureCall`). Each holds the already-rendered closure string
/// (`spawn_closure` is the distinct `emit_spawn_lambda` form; `serve`/`guard` use the
/// plain `emit_lambda` form) plus, for `serve`, the lowered address arg.
pub enum TCoreClosureKind {
    /// `task <body>` uses no group. A `task.group` child carries the same
    /// internal group collector through every named helper call.
    Spawn {
        group: Option<Box<TExpr>>,
        site: usize,
        /// Optional direct named-call/function-reference identity. `None` keeps
        /// the runtime's bounded `task@<site>` fallback for arbitrary bodies.
        label: Option<String>,
        spawn_closure: String,
        executable: Box<TLambda>,
    },
    /// `http.serve(addr, <lambda>)` → `{root}jet_http_serve(&(<addr>), <closure>)`.
    Serve { addr: Box<TExpr>, closure: String },
    /// `core.sys.on_interrupt(<callback>)` crosses the shared Send-safe runtime
    /// boundary. Engines only marshal this lowered callback to their adapter.
    OnInterrupt { callback: Box<TExpr> },
    /// `scope.guard(<lambda>)` → `{root}jet_scope_guard(<closure>)`.
    Guard {
        closure: String,
        executable: Box<TLambda>,
    },
    /// D-TXN3: `<handle>.on_commit(<lambda>)` → `<handle>.on_commit(Box::new(<closure>))`.
    OnCommit {
        handle: String,
        closure: String,
        executable: Box<TLambda>,
    },
    /// D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(<lambda>)` →
    /// `<handle>.on_rollback(Box::new(<closure>))`. Mirror of `OnCommit`.
    OnRollback {
        handle: String,
        closure: String,
        executable: Box<TLambda>,
    },
    /// D-REACT1=B: `reactive.derived(<lambda>)` → `{root}jet_std::JetDerived::new(<closure>)`.
    /// `executable` is AOT-ignored; Cranelift JIT compiles it (captures via spawn-lambda table).
    ///
    /// `site` is this callback's index in the JIT spawn-lambda table, exactly the
    /// fact `Spawn` above carries. Lowering owns it: a lambda body is lowered
    /// once per pass (the AOT closure text, `executable`, and the JIT spawn
    /// body), so the index cannot be re-derived from traversal order without
    /// drifting. Every engine reads this number; none recomputes it.
    ReactiveDerived {
        closure: String,
        executable: Box<TLambda>,
        site: usize,
    },
    /// D-EFFECT-LIFECYCLE1=A: `reactive.effect(<lambda>)` returns a lifecycle handle.
    ReactiveEffect {
        closure: String,
        executable: Box<TLambda>,
        site: usize,
    },
    /// D-RENDERTGT2=A (c133 M2): reactive UI render loop through the backend seam.
    UiReactiveRender {
        closure: String,
        executable: Box<TLambda>,
        site: usize,
    },
    /// D-WEB-CLICK-PORT1=D: `ui.button(label, on_click: <lambda>)`.
    UiButtonOnClick {
        label: Box<TExpr>,
        closure: String,
        executable: Box<TLambda>,
        site: usize,
    },
}

/// c109 Phase 13: fn-typed values plus the canonical interrupt callback form
/// (see `TExprKind::FnValue`).
pub enum TFnValueKind {
    /// A bare function name used as a value. `wrapper` is the already-rendered
    /// `Box::new(move |…| __jet_<name>(…)) as <fn-type>` string (`emit_named_fn_value`),
    /// produced at lowering so emit only echoes it.
    NamedFn {
        wrapper: String,
        /// Jet function key for native backends. `None` is a rendered closure
        /// coercion. `lambda` carries that closure's target-neutral executable
        /// body so Web and the TIR evaluator do not depend on the Rust wrapper.
        name: Option<String>,
        lambda: Option<Box<TLambda>>,
    },
    /// D-STRUCT-POLICY1=A: a checked package policy closes over its typed
    /// setting values and the supplied callable, then forwards the target's
    /// full argument contract to the generated checked wrapper function.
    Policy {
        wrapper: String,
        fn_type: Type,
        policy_args: Vec<TCallArg>,
        policy_conventions: Vec<crate::AST::AccessConvention>,
        callee: Box<TExpr>,
    },
    /// A call through a fn-value. `callee` lowers to its place (a local
    /// of `Type::Fn`, or another fn-value form); args are lowered plainly.
    Call {
        callee: Box<TExpr>,
        args: Vec<TCallArg>,
    },
    /// D-OSINTERRUPT1: one Send-safe callback representation. `value` is the
    /// already-lowered inline, named, or indirect callable. AOT emits its
    /// `Arc<dyn Fn() + Send + Sync + 'static>` value; resident JIT marshals it
    /// to one `(function, environment)` record; the interpreter keeps its
    /// callable index. The engines do not infer callback policy here.
    Interrupt { value: Box<TExpr> },
}

/// c109 Phase 12: a resolved numeric method form, one per numeric arm. The width
/// source/target and the
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
    BitCount {
        method: String,
        width: u32,
    },
    /// A widening / float-targeted / float-sourced conversion → `(({recv}) as {dst})`.
    CastAs {
        dst_rust: String,
    },
    /// D-NUMWIDEN-CROSS1=E: an implicit integer-to-float crossing whose source
    /// type is not wholly exact. Every engine calls Prelude/NumericWiden.rs.
    CheckedIntToFloat {
        source_signed: bool,
        target_f32: bool,
        line: u32,
    },
    /// A non-fallible fixed-width construction from exact `Int`. The shared
    /// conversion range is checked, then the arithmetic stop boundary is used
    /// instead of exposing a `Result` to the source expression.
    CheckedIntToFixed {
        host_kind: i64,
        dst_rust: String,
        dst_spelling: String,
        line: u32,
    },
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
    /// D-TYPE2-SPELL1: check an `Int` against an inline structural range. The
    /// range is carried by the resolved op; TIR and every engine erase the
    /// destination to the ordinary `Int` carrier.
    InlineRange {
        lo: i64,
        hi: i64,
        fallible: bool,
    },
    /// `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string`
    /// arm of `emit_builtin_method`, which fires for any receiver type).
    ToShow,
    /// D-GO127-STDLIB1=A: exact-Int Euclidean quotient/remainder. The source
    /// line is carried so divide-by-zero uses the normal arithmetic boundary.
    EuclideanDiv {
        line: u32,
    },
    EuclideanRem {
        line: u32,
    },
}

/// c109 Phase 11: a resolved closure-taking collection-method op, one per
/// built-in collection-method closure arm. The
/// receiver-type branch (Map vs list vs trait-object list) and the Fn-vs-FnMut
/// branch (off the lambda arg's `needs_fn_mut` meta) are decided ONCE at lowering;
/// the variant encodes the chosen form so emit only formats.
// Debug names the variant in engine rejection text: a JIT refusal is a silent
// interpreter deopt, so the message must say WHICH closure method was refused.
#[derive(Debug)]
pub enum TClosureOp {
    /// Prove two indexes before lending their mutable views to one callback.
    EditDisjoint,
    /// `map` on a list — `jet_list_map((recv).clone(), f)`.
    Map,
    /// `map` on a list whose lambda is FnMut — `jet_list_map_mut((recv).clone(), f)`.
    MapMut,
    /// Fallible `map` — the callback owns the failure row and the collection
    /// helper returns `Result<Collection<U>, E>`.
    TryMap,
    /// `filter` — `jet_list_filter((recv).clone(), f)`.
    Filter,
    /// Fallible `filter` — the callback returns `Result<Bool, E>` and the
    /// collection helper stops at the first failure.
    TryFilter,
    /// `each` on a list — `jet_list_each((recv).clone(), f)`.
    Each,
    /// `each` on a list whose lambda is FnMut — `jet_list_each_mut((recv).clone(), f)`.
    EachMut,
    /// `each` on a list of trait objects — `jet_list_each_ref(&(recv), f)`.
    EachRef,
    /// `each` on a map — `jet_map_each((recv).clone(), f)`.
    EachMap,
    MapAny,
    MapAll,
    MapFilter,
    MapMap,
    MapFold,
    MapFlatMap,
    ListBinarySearchBy,
    ListMinMaxBy {
        tuple_struct: String,
    },
    /// `find` — `jet_list_find((recv).clone(), f)`.
    Find,
    /// `any` — `jet_list_any((recv).clone(), f)`.
    Any,
    /// `any` on `Tally<T>` — `(recv).keys().any(f)`.
    BagAny,
    /// `all` — `jet_list_all((recv).clone(), f)`.
    All,
    /// `sort_by` — `{ jet_list_sort_by(&mut recv, f); }`.
    SortBy,
    /// `sort_by_desc` — `{ jet_list_sort_by_desc(&mut recv, f); }`.
    SortByDesc,
    /// Fallible `sort_by` — the callback keys are collected before the list moves.
    TrySortBy,
    /// Fallible `sort_by_desc` — the callback keys are collected before the list moves.
    TrySortByDesc,
    /// `sort_by` with a binary `T -> T -> Ordering` comparator.
    SortByCompare,
    /// `reduce` — `jet_list_reduce((recv).clone(), seed, f)`.
    Reduce,
    // D-ITER1 / D-CORE-EAGER2=A: receiver-sensitive adapter closure methods.
    /// `take_while(f)` — `jet_list_take_while((recv).clone(), f)`.
    TakeWhile,
    /// `skip_while(f)` — `jet_list_skip_while((recv).clone(), f)`.
    SkipWhile,
    /// `flat_map(f)` — eager for List, lazy for Iter; emitter selects the kernel.
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
    Partition {
        tuple_struct: String,
    },
    // #1479
    /// `dedup_by(f)` — `jet_iter_dedup_by({as_iter}, f)`.
    DedupBy,
    /// `is_sorted_by(f)` — `jet_iter_is_sorted_by({as_iter}, f)`.
    IsSortedBy,
    /// `chunk_while(f)` — `jet_iter_chunk_while({as_iter}, f)`.
    ChunkWhile,
    // D-FAILCOMP1: failure-aware adapters.
    /// `filter_map(f)` — `jet_list_filter_map((recv).clone(), f)`.
    FilterMap,
    // D-PARCAPTURE1=D: explicit `para_` adapters.
    ParaMap,
    ParaFilter,
    ParaPartition {
        tuple_struct: String,
    },
    ParaFold,
    // D-HOLE1: Option combinators.
    /// `map` on `?T` — `(recv).as_ref().map(f)` (Rust's native `Option::map`, no
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
/// `Lambda.meta`. `prep` is the rendered clone/materialization capture prelude
/// (`let __jet_cap_<n> = (place).clone();\n    ` per capture); `params` the rendered `name[: ty]`
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
    /// Escaping non-mut Fn value: wrap with `std::rc::Rc::new` so collections can `.cloned()`.
    pub rc: bool,
    pub arc: bool,
    /// JIT capture pack: (enclosing Jet name, body place, type). Empty = non-capturing.
    pub captures: Vec<(String, String, Type)>,
    /// D-MEM-COPYSEM1=A: source names whose capture slot is an owned
    /// materialization of a read-only view rather than a reference clone.
    pub materialized_captures: Vec<String>,
    /// D-CONC-FREEZE1=A: frozen source names carried from sema's one flow-fact
    /// proof. This is metadata only; the capture slot is already owned.
    pub frozen_captures: Vec<String>,
    /// D-HARDENED1 / D-MEM-SENTRY1: the closure body mints an address from its
    /// current frame storage and needs a token around each invocation.
    pub uses_stack_sentry: bool,
}

pub enum TLambdaBody {
    Expr(Box<TExpr>),
    Block(Vec<TStmt>),
    /// A deferred body shared by the AOT closure representation and JIT lambda.
    SharedBlock(std::sync::Arc<[TStmt]>),
}

/// c109 Phase 8: the resolved error-conversion of a `?`, mirroring `AST::TryConvert`
/// (the total sema fact). Carried onto the TIR so the emitter never re-derives it.
pub enum TTryConvert {
    /// Error types match — bare `jet_trace_err(x, …)?`.
    None,
    /// The source error is `Never`; sema proved the failure route is
    /// unreachable, so lowering unwraps the shared carrier without a
    /// conversion or propagation branch.
    Never,
    /// D-FAIL-ERROR1=A: construct the default `Err` value from a message.
    DefaultErr,
    /// Declared `impl Source -> Target` conversion — `.map_err(<fn>)` (D-ERR-CONV);
    /// holds the mangled Rust conversion-function name and resolved error types.
    Typed {
        fn_name: String,
        source: Type,
        target: Type,
    },
    /// D-UNIONTYPE1=A: wrap the error into a compiler-generated union enum.
    WidenUnion { enum_name: String, tag: String },
}

/// Whether a `?` produces the default `Err` carrier after its sema-selected
/// conversion. Only that carrier can own structured context frames.
pub fn try_target_is_default_error(inner: &TExpr, convert: &TTryConvert) -> bool {
    match convert {
        TTryConvert::DefaultErr => true,
        TTryConvert::None => matches!(
            inner.ty.unwrap_result().map(|(_, error)| error),
            Some(Type::Named(name)) if name == crate::Syntax::TYPE_ERR
        ),
        TTryConvert::Typed { target, .. } => matches!(
            target,
            Type::Named(name) if name == crate::Syntax::TYPE_ERR
        ),
        TTryConvert::Never | TTryConvert::WidenUnion { .. } => false,
    }
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
    /// D-LOOPLABEL3=A as amended by D-ARROW-CONTROL1=A: `?? break(label)`.
    BreakLabel(String),
    /// D-LOOPLABEL3=A as amended by D-ARROW-CONTROL1=A: `?? next(label)`.
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

/// D-LISTREMOVE1/F: the selector is resolved before emission. `Dynamic` is only
/// used for an Int list when the selector is stored in a variable; other element
/// types cannot accept both a value and a positional Int in one statically typed call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRemoveMode {
    Value,
    Slot,
    Dynamic,
}

/// c109 Phase 9: a resolved built-in collection/string method op. Each variant is
/// one emit form from built-in collection-method lowering. The
/// receiver-type dispatch (`rty = expr_jet_ty(receiver)` → Map vs List vs String)
/// is decided ONCE at lowering — the variant encodes the chosen branch, so emit
/// only formats. Line numbers (for the bounds/remove panic frames) are resolved at
/// lowering; `cx.file`/`cx.root_prefix` are read at emit (program-level, not a
/// per-node decision). Args are emitted plainly (no clone/borrow wrappers), exactly
/// as `emit_builtin_method`'s `arg(i)` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TZipMode {
    Strict,
    Short,
    Pad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TZipFillMode {
    DefaultNone,
    Common,
    Columns,
}

/// A resolved fast payload access for an outcome consumed immediately by `??`.
/// The carrier is still authoritative on the ordinary path; this descriptor only
/// lets emit elide its construction on the success edge and reconstruct failure
/// at the cold edge. Operation tables publish capabilities, while the use site
/// decides whether the immediate-outcome shape permits the optimization.
#[derive(Clone, Copy)]
pub(crate) enum TOutcomeFastPath {
    FixedRead {
        buffer: TOutcomeFastBuffer,
        helper: &'static str,
        error_method: Option<&'static str>,
        width: usize,
    },
}

#[derive(Clone, Copy)]
pub(crate) enum TOutcomeFastBuffer {
    Reader,
    Bytes,
}

/// The fixed-width Reader facts consumed by both immediate-outcome lowering
/// and the AOT region rewrite. Keeping the payload type and byte order here
/// prevents an emitter from recognizing a helper name and guessing how to
/// decode its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TReaderFixedWidth {
    U8,
    I8,
    U16Le,
    U16Be,
    I16Le,
    I16Be,
    U32Le,
    U32Be,
    I32Le,
    I32Be,
    U64Le,
    U64Be,
    I64Le,
    I64Be,
    F32Le,
    F32Be,
    F64Le,
    F64Be,
}

impl TReaderFixedWidth {
    fn fast_path_facts(self) -> (&'static str, &'static str, usize) {
        match self {
            Self::U8 => ("jet_reader_read_u8_fast", "read_u8", 1),
            Self::I8 => ("jet_reader_read_i8_fast", "read_i8", 1),
            Self::U16Le => ("jet_reader_read_u16_le_fast", "read_u16_le", 2),
            Self::U16Be => ("jet_reader_read_u16_be_fast", "read_u16_be", 2),
            Self::I16Le => ("jet_reader_read_i16_le_fast", "read_i16_le", 2),
            Self::I16Be => ("jet_reader_read_i16_be_fast", "read_i16_be", 2),
            Self::U32Le => ("jet_reader_read_u32_le_fast", "read_u32_le", 4),
            Self::U32Be => ("jet_reader_read_u32_be_fast", "read_u32_be", 4),
            Self::I32Le => ("jet_reader_read_i32_le_fast", "read_i32_le", 4),
            Self::I32Be => ("jet_reader_read_i32_be_fast", "read_i32_be", 4),
            Self::U64Le => ("jet_reader_read_u64_le_fast", "read_u64_le", 8),
            Self::U64Be => ("jet_reader_read_u64_be_fast", "read_u64_be", 8),
            Self::I64Le => ("jet_reader_read_i64_le_fast", "read_i64_le", 8),
            Self::I64Be => ("jet_reader_read_i64_be_fast", "read_i64_be", 8),
            Self::F32Le => ("jet_reader_read_f32_le_fast", "read_f32_le", 4),
            Self::F32Be => ("jet_reader_read_f32_be_fast", "read_f32_be", 4),
            Self::F64Le => ("jet_reader_read_f64_le_fast", "read_f64_le", 8),
            Self::F64Be => ("jet_reader_read_f64_be_fast", "read_f64_be", 8),
        }
    }

    pub(crate) const fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16Le | Self::U16Be | Self::I16Le | Self::I16Be => 2,
            Self::U32Le | Self::U32Be | Self::I32Le | Self::I32Be => 4,
            Self::U64Le | Self::U64Be | Self::I64Le | Self::I64Be => 8,
            Self::F32Le | Self::F32Be => 4,
            Self::F64Le | Self::F64Be => 8,
        }
    }

    /// Render one fixed-width payload load from already-proven byte access.
    /// Both immediate `??` lowering and the bounded Reader region use this
    /// operation table fact; neither emitter guesses byte order from a helper
    /// name or duplicates the typed decode table.
    pub(crate) fn emit_load(self, byte_at: impl Fn(usize) -> String) -> String {
        let byte = |offset| byte_at(offset);
        let (ty, endian) = match self {
            Self::U8 => return byte(0),
            Self::I8 => return format!("{} as i8", byte(0)),
            Self::U16Le => ("u16", "from_le_bytes"),
            Self::U16Be => ("u16", "from_be_bytes"),
            Self::I16Le => ("i16", "from_le_bytes"),
            Self::I16Be => ("i16", "from_be_bytes"),
            Self::U32Le => ("u32", "from_le_bytes"),
            Self::U32Be => ("u32", "from_be_bytes"),
            Self::I32Le => ("i32", "from_le_bytes"),
            Self::I32Be => ("i32", "from_be_bytes"),
            Self::U64Le => ("u64", "from_le_bytes"),
            Self::U64Be => ("u64", "from_be_bytes"),
            Self::I64Le => ("i64", "from_le_bytes"),
            Self::I64Be => ("i64", "from_be_bytes"),
            Self::F32Le => ("f32", "from_le_bytes"),
            Self::F32Be => ("f32", "from_be_bytes"),
            Self::F64Le => ("f64", "from_le_bytes"),
            Self::F64Be => ("f64", "from_be_bytes"),
        };
        let bytes = (0..self.width()).map(byte).collect::<Vec<_>>().join(", ");
        format!("{ty}::{endian}([{bytes}])")
    }
}

// Debug names the variant in engine rejection text: a JIT refusal is a silent
// interpreter deopt, so the message must say WHICH builtin method was refused.
#[derive(Debug)]
pub enum TBuiltinOp {
    /// `len` on a `String` → `jet_char_len(&(recv))` (char count, not byte len).
    LenString,
    /// `len` on a list/map → `(recv).len() as i64`.
    LenList,
    /// `is_empty()` on a list/map/string → `(recv).is_empty()` (Bool).
    IsEmpty,
    /// `push(x)` → `(recv).push(a0)`.
    Push,
    /// D-ALLOCFAIL1=A: fallible list/string mutation and reservation.
    ListTryNew,
    ListTryWithCapacity,
    TryPush,
    TryReserve,
    TryInsertMap,
    TryStringPush,
    /// `pop()` → `(recv).pop()`.
    Pop,
    /// `PriorityQueue.pop()` → the shared Prelude heap kernel.
    PriorityQueuePop,
    /// `add(k, v)` on a map → displaced value, if any.
    InsertMap,
    /// `add_new(k, v)` on a map → false without overwriting an existing key.
    AddNewMap,
    /// D-MAP-MERGE1=E: `merge(other)` → right wins on shared keys.
    MapMerge,
    /// D-MAP-MERGE1=E: `merge(other, conflict)` → callback resolves shared keys.
    MapMergeWith,
    /// `insert(i, v)` on a list → `(recv).insert(a0 as usize, a1)`.
    InsertList,
    /// `remove(k)` on a map → `(recv).remove(&(a0).clone())`.
    RemoveMap,
    /// `remove(x[, by])` on a list; the selector is fixed at lowering.
    RemoveList {
        line: usize,
        mode: ListRemoveMode,
    },
    /// `count(value)` on a list.
    CountList,
    /// `counts()` on a list or iterator → a frequency map.
    Counts,
    /// `extend(other)` on a list.
    ExtendList,
    /// `concat(other)` on a list.
    ConcatList,
    /// `get(k)` on a map → `(recv).get(&(a0).clone()).cloned()`.
    GetMap,
    /// `get(i)` on a list → `(recv).get(a0 as usize).cloned()`.
    GetList,
    /// `first()` → the consuming `jet_iter_first` terminal or a cloned collection item.
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
    /// `sort_desc()` → the shared descending Prelude kernel.
    SortDesc,
    /// `Ordering.then(other)` keeps the first non-equal result.
    OrderingThen,
    /// `Ordering.reverse()` swaps Less and Greater.
    OrderingReverse,
    /// `join(sep)` → `(recv).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join((a0).as_str())`.
    JoinSep,
    Sum {
        float: bool,
    },
    Product {
        float: bool,
    },
    Min {
        float: bool,
    },
    Max {
        float: bool,
    },
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
    /// `String.from_bytes(bytes)` → `{root}jet_string_from_bytes(&(recv))`.
    StringFromBytes,
    /// `String.from_bytes_lossy(bytes)` → `{root}jet_string_from_bytes_lossy(&(recv))`.
    StringFromBytesLossy,
    /// `trim()` → pinned `jet_unicode_trim(&(recv))`.
    Trim,
    TrimStart,
    TrimEnd,
    PadStart,
    PadEnd,
    StringIndexOf,
    StringCount,
    StringIsAlphabetic,
    StringIsNumeric,
    StringIsWhitespace,
    StringIsAscii,
    StringToTitle,
    /// #1476: remaining ambient String methods, dispatched by name.
    StringMethod {
        method: String,
    },
    StringSplitOnce {
        tuple_struct: String,
    },
    /// `cut_last(sep)` → the shared Unicode terminal-boundary helper.
    StringCutLast {
        tuple_struct: String,
    },
    /// `split(sep)` → `jet_iter_string_split(&(recv), &a0)` (lazy `JetIter<String>`).
    Split,
    /// c97/D-STRPARSE1: `lines()` → `{root}jet_string_lines(&(recv))`.
    Lines,
    /// c97/D-STRPARSE1: `Int.parse(text)` → checked integer parse.
    ParseInt,
    /// c97/D-STRPARSE1: `Float.parse(text)` → checked floating-point parse.
    ParseFloat,
    /// `Int.to_radix(base)` → exact lowercase text through the shared Int kernel.
    IntToRadix {
        line: u32,
    },
    /// `Int.from_radix(text, base)` → exact Int through the shared Int kernel.
    IntFromRadix {
        line: u32,
    },
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
    /// ASCII-only lowercasing; all non-ASCII code points remain unchanged.
    ToAsciiLower,
    /// ASCII-only uppercasing; all non-ASCII code points remain unchanged.
    ToAsciiUpper,
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
    /// `keys()` → a lazy `JetIter` over map keys.
    Keys,
    /// `values()` → a lazy `JetIter` over map values.
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
    /// D-LOOPMAP1=B: `list.lazy()` → `Iter<T>` pipeline plane.
    ListLazy,
    /// `step_by(n)` → `jet_list_step_by((recv).clone(), a0)`.
    StepBy,
    /// `dedup()` → `jet_iter_dedup(...)`.
    Dedup,
    /// `chunks(n)` → `jet_iter_chunks(...)`.
    Chunks,
    /// `windows(n)` → `jet_iter_windows(...)`.
    Windows,
    // #1479: remaining Iter ledger surface (non-closure).
    /// `repeat(n)` → `jet_iter_repeat({as_iter}, n)`.
    IterRepeat,
    /// `cycle(n)` → `jet_iter_cycle({as_iter}, n)` — exactly `n` items.
    IterCycle,
    /// `drop_last(n)` → `jet_iter_drop_last({as_iter}, n)`.
    IterDropLast,
    /// `shuffle()` → `jet_iter_shuffle({as_iter})`.
    IterShuffle,
    /// `is_sorted()` → `jet_iter_is_sorted({as_iter})`.
    IterIsSorted,
    /// `last_index_of(v)` → `jet_iter_last_index_of({as_iter}, v)`.
    IterLastIndexOf,
    /// `average()` → `jet_iter_average_{int,float}({as_iter})`.
    IterAverage {
        float: bool,
    },
    /// `compare(other)` → `jet_iter_compare({as_iter}, other)`.
    IterCompare,
    /// `split(n)` → `jet_iter_split_at({as_iter}, n, |l,r| Struct{…})`.
    IterSplit {
        tuple_struct: String,
    },
    // #1477 List/Map non-closure surface
    ListSlice,
    ListCopy,
    ListEqual,
    ListBinarySearch,
    ListUnion,
    ListIntersection,
    ListDifference,
    ListRandom,
    ListMinMax {
        tuple_struct: String,
    },
    MapCopy,
    MapEqual,
    MapFirst,
    MapToList {
        tuple_struct: String,
    },
    /// `top_n(n)` on a map → key/value rows ordered by descending value.
    MapTopN {
        tuple_struct: String,
    },
    MapMin,
    MapMax,
    MapIntersection,
    MapSliceKeys,
    MapNew,
    MapFromKeys,
    MapContainsValue,
    MapPopFirst,
    ListReplace,
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
        mode: TZipMode,
        /// Output field names in source input order.
        fields: Vec<String>,
        /// The left input is a nested pair from an earlier n-ary stage.
        flatten: bool,
        /// Number of source inputs in the complete zip family call.
        input_count: usize,
        fill_mode: TZipFillMode,
        /// Resolved row-field types, used to marshal typed fills per column.
        field_types: Vec<Type>,
    },
    // D-HOLE1: Option combinators.
    /// `zip(?U)` on `?T` → `(recv).clone().zip((a0).clone()).map(|(x,y)| Struct{…})`
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
    SetIntersection,
    SetDifference,
    SetSymmetricDifference,
    SetIsSubset,
    SetIsSuperset,
    SetIsDisjoint,
    /// #1478: `set.copy()` / `set.to_set()` → clone.
    SetCopy,
    /// #1478: `set.equal(other)` → `recv == other`.
    SetEqual,
    /// #1478: `set.capacity()` → `recv.capacity() as i64`.
    SetCapacity,
    /// #1478: `set.first()` → arbitrary element (unordered).
    SetFirst,
    /// #1478: `set.values()` → lazy view over the same arbitrary order as `to_list`.
    SetValues,
    /// D-ONCE-VERB1=A: `set.pop(v)` → shared remove-and-return kernel.
    SetPop,
    /// D-SET-DECLINE1=C: `set.sort()` → a fresh sorted `List`, same
    /// to-list-then-sort machinery `to_list()` already runs (Set never mutates).
    SetSort,
    /// D-SET-DECLINE1=C: `set.shuffle()` → a fresh shuffled `List`, the same
    /// `jet_iter_shuffle` engine `List.shuffle()` already runs.
    SetShuffle,
    SortedSetFrom,
    SortedSetInsert,
    SortedSetRemove,
    SortedSetToList,
    SortedSetUnion,
    SortedSetIntersection,
    SortedSetDifference,
    SortedSetSymmetricDifference,
    SortedSetIsSubset,
    SortedSetIsSuperset,
    SortedSetIsDisjoint,
    PriorityQueueFrom,
    PriorityQueuePeek,
    PriorityQueueToSortedList,
    /// D-LISTREMOVE1/F: `remove(x[, by])` on a `PriorityQueue`; same selector
    /// shape as `RemoveList`, no positional ordering guarantee across pushes.
    PriorityQueueRemove {
        line: usize,
        mode: ListRemoveMode,
    },
    LruPut,
    LruAddNew,
    LruGet,
    LruCapacity,
    LruKeys,
    BitSetAdd,
    BitSetRemove,
    BitSetCount,
    BitSetToList,
    BitSetCopy,
    BitSetNew,
    ByteBufferNew,
    ByteBufferFrom,
    ByteBufferWrite {
        method: String,
    },
    ByteBufferToBytes,
    /// Generic `Bytes` instance call → `(recv).method(args…)` (cursor + string-like).
    ByteBufferMethod {
        method: String,
    },
    ByteBufferWithCapacity,
    // D-TAG1: Tally<T> counted multiset (HashMap-backed).
    BagAdd,
    BagRemove,
    BagHas,
    BagCount,
    BagLen,
    // D-COLLBREADTH1=A: Queue<T> operations.
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
    /// `deque.capacity()` → `(recv).capacity() as i64`.
    DequeCapacity,
    /// `deque.contains(v)` → `(recv).iter().any(|x| x == &v)`.
    DequeContains,
    /// `deque.get(i)` → `(recv).get(i as usize).cloned()`.
    DequeGet,
    /// `deque.delete(v)` — remove first equal element (unit).
    DequeDelete,
    /// `deque.to_list()` → `(recv).iter().cloned().collect::<Vec<_>>()`.
    DequeToList,
    /// `deque.join(sep)` — string-join of elements via jet_show.
    DequeJoin,
    /// `deque.reverse()` — reverse in place via make_contiguous.
    DequeReverse,
    /// `deque.split(i)` → `(recv).split_off(i as usize)` (returns other half).
    DequeSplit,
    /// `Queue.from(xs)` / `Queue.init(xs)` — build from a list.
    DequeFrom,
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
    /// D-SHAPE-PLACE1=A: a checked rank-preserving Tensor window. The ambient
    /// evaluator materializes the read view and tracks the mutable view handle;
    /// AOT calls the same Prelude bounds law directly.
    ComputeViewNew {
        line: usize,
    },
    ComputeViewMutNew {
        line: usize,
    },
    /// D-MEMDISJOINT1=A: checked runtime split into two tracked mutable views.
    SplitWrite {
        tuple_struct: String,
    },
    /// D-MEMDISJOINT1=A: checked runtime selection of distinct one-item views.
    GetDisjointWrite,
}

impl TBuiltinOp {
    /// A resolved builtin with a write receiver must receive a live place.
    /// Keep this fact on the TIR op so lowering and every emitter agree; lazy
    /// iterator adapters are value operations and are intentionally absent.
    pub(crate) fn needs_mut_receiver_place(&self) -> bool {
        match self {
            Self::Push
            | Self::TryPush
            | Self::TryReserve
            | Self::TryInsertMap
            | Self::TryStringPush
            | Self::Pop
            | Self::PriorityQueuePop
            | Self::InsertMap
            | Self::AddNewMap
            | Self::InsertList
            | Self::RemoveMap
            | Self::RemoveList { .. }
            | Self::PriorityQueueRemove { .. }
            | Self::MapPopFirst
            | Self::ExtendList
            | Self::Reverse
            | Self::Sort
            | Self::SortDesc
            | Self::Clear
            | Self::SetInsert
            | Self::SetRemove
            | Self::SetPop
            | Self::SortedSetInsert
            | Self::SortedSetRemove
            | Self::BitSetAdd
            | Self::BitSetRemove
            | Self::BagAdd
            | Self::BagRemove
            | Self::LruPut
            | Self::LruAddNew
            | Self::LruGet
            | Self::ByteBufferWrite { .. }
            | Self::DequePushFront
            | Self::DequePushBack
            | Self::DequePopFront
            | Self::DequePopBack
            | Self::DequeDelete
            | Self::DequeReverse
            | Self::DequeSplit
            | Self::SplitWrite { .. }
            | Self::GetDisjointWrite => true,
            Self::ByteBufferMethod { method } => matches!(
                method.as_str(),
                "clear"
                    | "seek"
                    | "rewind"
                    | "next"
                    | "read"
                    | "read_byte"
                    | "read_bytes"
                    | "read_string"
                    | "flush"
                    | "close"
                    | "shutdown"
                    | "copy_to"
                    | "write_to"
            ),
            _ => false,
        }
    }

    /// Publish only byte-wise outcome operations whose success payload can be
    /// read without building the `JetOutcome` carrier first. This is a
    /// capability table, not the decision to optimize a particular expression.
    pub(crate) fn outcome_fast_path(&self) -> Option<TOutcomeFastPath> {
        match self {
            Self::ByteBufferMethod { method }
                if matches!(method.as_str(), "next" | "read_byte") =>
            {
                Some(TOutcomeFastPath::FixedRead {
                    buffer: TOutcomeFastBuffer::Bytes,
                    helper: "read_byte_fast",
                    error_method: None,
                    width: 1,
                })
            }
            _ => None,
        }
    }
}

/// c109 Phase 13: a resolved handle-method op, one per handle arm of
/// built-in method lowering. The handle-receiver branch
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
    /// D-DATAFLOW1=A: typed pull `DataStream<T>.next()` → `?T !DataError`.
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
    /// D-CMD-OVERRIDE1=C: `TestSuite.run()` calls the installed command runner.
    TestSuiteRun,
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
    /// D-DET-CAPAPI Rng: `pick(list)` → `{root}jet_rng_pick(&mut (recv), &(a0))` (uniform `?T`).
    RngPick,
    /// D-RANDOMDIST1 Rng: `weighted_pick(list, weights)` → `{root}jet_rng_weighted_pick(&mut (recv), &(a0), &(a1))`.
    RngWeightedPick,
    /// D-RANDOMDIST1 Rng: `sample(list, k)` → `{root}jet_rng_sample(&mut (recv), &(a0), a1)`.
    RngSample,
    /// D-DET-CAPAPI Rng: `shuffle(&list)` → `{root}jet_rng_shuffle(&mut (recv), &mut (a0))` (in-place).
    RngShuffle,
    /// D-TESTDATA1 Fake: locale and deterministic fake-data domain draws.
    FakeLocale,
    FakeName,
    FakeEmail,
    FakeHost,
    FakeAddress,
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
    /// D-GAME-LOOP1=A: `backend.should_continue()` → Bool.
    GameBackendShouldContinue,
    /// D-GAME-LOOP1=A: `backend.present()` → Unit.
    GameBackendPresent,
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
    /// D-TIMERES1=A: duration facts.
    DurationIsZero,
    DurationTotalSeconds,
    DurationDifference,
    /// D-TIMEDEPTH1=A: Temporal duration projections and exact rounding.
    DurationAbs,
    DurationNegated,
    DurationSign,
    DurationTotalIn,
    DurationRound,
    /// D-TYPE2-TIME1=A: dimensional algebra reads canonical Time in seconds;
    /// the stored carrier remains i64 nanoseconds.
    DurationSecondsValue,
    /// D-TIMERES1=A: checked scalar arithmetic on the canonical nanosecond
    /// carrier. The factor is the sole plain argument.
    DurationScale,
    DurationDivide,
    /// D-INTBIG1 / D-DECIMAL1: instance methods on precise numeric types.
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
    TLSStreamReadDeadline,
    TLSStreamWriteAllDeadline,
    TLSStreamReady,
    TLSStreamClose,
    TLSStreamCloseWrite,
    TLSStreamPeerIdentity,
    TLSClientConfigDefault,
    TLSClientConfigWithAlpn,
    TLSRootCertificatesFromPem,
    TLSClientIdentityFromPem,
    TLSClientConfigWithTrust,
    TLSClientConfigWithIdentity,
    TLSClientConfigWithVersionBounds,
    HTTPClientNew,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `alloc(v)` → `(recv).alloc(a0)` (hands back a
    /// `&mut T` view into the allocator's storage). The arg is emitted plainly.
    AllocAlloc,
    /// D-ALLOCFAIL1=A: allocator `try_alloc(v)` returns `T !AllocError`.
    AllocTryAlloc,
    /// c109 Phase 19: Arena/Bump/Pool/Fixed `reset()` → `(recv).reset()`.
    AllocReset,
    /// c109 Phase 20: HTTPRequest `method()`/`path()`/`body()` → `(recv).<field>.clone()`.
    HTTPReqField(&'static str),
    /// c109 Phase 20: HTTPRequest `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HTTPReqHeader,
    /// c109 Phase 20: HTTPRequest `param(name)` → `{root}jet_http_request_param(&(recv), &(a0))`.
    HTTPReqParam,
    HTTPReqTrailers,
    /// c109 Phase 20: HTTPResponse `status()`/`body()` → `(recv).<field>.clone()`.
    HTTPRespField(&'static str),
    /// c109 Phase 20: HTTPResponse `header(name)` → `(recv).headers.get(&a0).cloned()`.
    HTTPRespHeader,
    HTTPRespTrailers,
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
    /// D-CLI-DOCS1: ArgsSpec `.description(text)` → `(recv).description(&a0)` → `JetArgsSpec`.
    ArgsSpecDescription,
    ArgsSpecSubcommand,
    ArgsSpecVersion,
    ArgsSpecCompletion,
    /// D-ARGS1: ArgsSpec `.help()` → `(recv).help()` → `String`.
    ArgsSpecHelp,
    /// D-ARGS1: ArgsSpec `.parse(argv)` → `{root}jet_args_parse(&(recv), &(a0))` → `Result<JetParsedArgs, String>`.
    ArgsSpecParse,
    /// D-ARGS-EXIT1: ArgsSpec `.parse_or_exit(argv)` → parsed args or process exit.
    ArgsSpecParseOrExit,
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
    /// D-PROCESS-SESSION2=D: resize the typed terminal handle.
    TerminalSessionResize,
    /// D-PROCESS1=A: `child.stdin.write(text)` →
    /// `{root}jet_process_stdin_write(&(recv), &(a0))` → `Result<(), IOError>`.
    ProcessStdinWrite,
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s `Value` handle — plain
    /// inherent-method passthrough, same shape as `ArgsSpecHelp`.
    ReflectValueTypeName,
    ReflectValuePath,
    ReflectValueDisplay,
    ReflectValueFields,
    /// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x).fields()`'s `Field` handle.
    ReflectFieldName,
    ReflectFieldValue,
    /// D-CONC-FAIL1=A: Task `join()` → `(recv).join()`; sema types it as
    /// `T !TaskFailure`.
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
    /// c109 Phase 21 / D-TUPLE-DESTRUCT1: Receiver `receive()` → `(recv).receive()` →
    /// `Result<T, Closed>`.
    ChannelReceive,
    /// Explicit channel close on Receiver or Sender.
    ChannelClose,
    /// c109 Phase 21: Sender `send(v)` → `(recv).send(a0)`. Returns unit.
    SenderSend,
    /// c109 Phase 25: HTTPRouter `get`/`post`/`put`/`delete` route registration
    /// (D-ROUTE1=A). Emits `{root}jet_http_router_register(&mut (recv), "<VERB>".to_string(),
    /// <path>, <handler>)` where `<path>` is the lowered first arg (args[0]) and `<handler>`
    /// is a pre-rendered boxed-closure string (`emit_router_handler` reproduction, resolved
    /// at lowering). `verb` is the uppercase HTTP method literal.
    HTTPRouterRegister {
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
    /// `callback_index` is an index into `JitProgram.spawn_lambdas` for `on`/`once`.
    WatchMethod {
        method: String,
        callback_index: Option<usize>,
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
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP client type (HTTPRequest/HTTPResponse).
    HTTPClientMethod {
        kind: String,
        method: String,
    },
    /// D-NETDEP1=A / D-HTTPLIB1=A: method call on an HTTP server type (HTTPMux/HTTPRequest/HTTPResponse).
    HTTPServerMethod {
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
    /// D-DATATREE-ERGO1=A: `DataTree.to_text()` → `(recv).to_text()`.
    DataTreeToText,
    /// D-DATATREE-ERGO1=A: `DataTree.equal_unordered(other)` →
    /// `(recv).equal_unordered(&a0)`.
    DataTreeEqualUnordered,
    /// D-SERDE16=A: `tree.decode<T>()` dispatches the public `T.Decode` protocol.
    DataTreeDecode(Type),
    /// D-SERDE2=A: `value.encode()` dispatches the public Encode protocol.
    SerdeEncode,
    /// D-SERDE-ACCESS=B: same accessors on `JSON`/`Data`.
    JSONField,
    JSONAt,
    JSONInt,
    JSONText,
    JSONBool,
    JSONFloat,
    JSONToText,
    JSONEqualUnordered,
    /// D-PATHFS1: `Path.from(str)` constructor → `{root}jet_path_from(&(recv))`.
    PathFrom,
    /// D-CORE-PATH1: `Path.home()` reads the host home path through the shared
    /// Prelude path kernel and returns a typed `Path`.
    PathHome,
    /// D-PATHFS1: `path.join(other)` → `{root}jet_path_join(&(recv), &(a0))` → `JetPath`.
    PathJoin,
    /// D-PATHFS1: `path.parent()` → `{root}jet_path_parent(&(recv))` → `Option<JetPath>`.
    PathParent,
    /// D-PATHFS1: `path.extension()` → `{root}jet_path_extension(&(recv))` → `Option<String>`.
    PathExtension,
    /// D-PATHFS1: `path.stem()` → `{root}jet_path_stem(&(recv))` → `Option<String>`.
    PathStem,
    /// D-PATHFS1: `path.normalize()` → `{root}jet_path_normalize(&(recv))` → `Path`.
    PathNormalize,
    /// D-STDLIB-SMALL1: lexical candidate-within-base comparison.
    PathIsWithin,
    /// D-PATHFS1: `path.to_string()` → `(recv).jet_show()` → `String`.
    PathToString,
    /// D-PATHFS1: `path.write_atomic(bytes)` → `{root}jet_path_write_atomic(&(recv), &(a0))` → `Result<(), IOError>`.
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
    /// D-WEBAPP1=D: `App` builder methods (`.route`/`.action`/`.mount`/…).
    AppMethod {
        method: String,
    },
    /// D-DBPOLICY-BIND1: bind a validated RowPolicy + user to a DBConnection.
    DBWithPolicy,
    /// D-SERVICE-AUTHORITY1: durable authority methods share the Prelude log.
    ServiceRuntimeSend,
    ServiceRuntimeRetry,
    ServiceRuntimeDeadLetter,
    ServiceRuntimeRetain,
    ServiceRuntimeCommit,
    /// D-DBDRIVER1: `conn.query(sql, params)` → `Result<Vec<Row>, DBError>`. Encodes
    /// `params` via `jet_std::jet_db_encode_params`, calls the FFI bridge's
    /// `jet_db_query`, decodes the wire result via `jet_std::jet_db_decode_query_result`.
    DBQuery,
    /// D-DBDRIVER1: `conn.query_one(sql, params)` → `Result<Option<Row>, DBError>`.
    /// Same as `DBQuery` but takes only the first row (if any).
    DBQueryOne,
    /// D-DBDRIVER1: `conn.execute(sql, params)` → `Result<Int, DBError>` (affected rows).
    DBExecute,
    /// D-DBPOLICY-BIND1: scoped query registered with the same live registry as
    /// `app.live`, after policy transformation.
    DBLive,
    /// D-DBDRIVER1: `conn.begin()` → `{ffi}::jet_db_begin((recv).handle)` → `Bool`.
    DBBegin,
    /// D-DBDRIVER1: `conn.commit()` → `{ffi}::jet_db_commit((recv).handle)` → `Bool`.
    DBCommit,
    /// D-DBDRIVER1: `conn.rollback()` → `{ffi}::jet_db_rollback((recv).handle)` → `Bool`.
    DBRollback,
    /// D-DBDRIVER1: `conn.close()` → `{ffi}::jet_db_close((recv).handle)` → `Bool`.
    DBClose,
    /// D-DBDRIVER1: `DBValue` accessor methods (`.int()`/`.float()`/`.text()`/
    /// `.bool()`/`.is_null()`) → `(recv).<method>()`, same shape as `JSONInt`/….
    DBValueInt,
    DBValueFloat,
    DBValueText,
    DBValueBool,
    DBValueIsNull,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call(name, args)` →
    /// `Result<Float, String>`, a homogeneous `[Float]` call across the
    /// sandboxed Component Model boundary (wire-encoded, see `Prelude/Plugin.rs`).
    PluginCall,
    /// D-DEP-WASM1=A / D-PLUGIN1=B (c81): `plugin.call_int(name, args)` →
    /// `Result<Int, String>`, the `[Int]` sibling of `PluginCall`.
    PluginCallInt,
    /// `Result<Bool, String>`, the `[Bool]` sibling of `PluginCall`.
    PluginCallBool,
    /// `Result<String, String>`, the `[String]` sibling of `PluginCall`.
    PluginCallText,
    /// D-LIB-CALLGRANT1=A: `mod.on_tick(dt)` → the checked native entry point.
    ModOnTick,
    /// D-SHIFT1 (c7shift): `Reader.over(bytes)` constructor →
    /// `{root}jet_reader_over(&(recv))` → `JetReader`. `recv` is the `[U8]`
    /// argument (same "arg becomes the recv slot" shape as `PathFrom`).
    ReaderOver,
    /// D-SHIFT1: `reader.read_u8()` → `{root}jet_reader_read_u8(&mut (recv))`
    /// → `Result<U8, String>`. Bounds miss is an ordinary `Err`, never a panic.
    ReaderReadU8,
    ReaderReadI8,
    ReaderReadU16Le,
    ReaderReadU16Be,
    ReaderReadI16Le,
    ReaderReadI16Be,
    ReaderReadU32Le,
    ReaderReadU32Be,
    ReaderReadI32Le,
    ReaderReadI32Be,
    ReaderReadU64Le,
    ReaderReadU64Be,
    ReaderReadI64Le,
    ReaderReadI64Be,
    ReaderReadF32Le,
    ReaderReadF32Be,
    ReaderReadF64Le,
    ReaderReadF64Be,
    ReaderPeek,
    ReaderSeek,
    ReaderSkip,
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
    /// D-BINPAT1 (card #506 follow-up): `reader.take_pattern([U8]{"…"})` —
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

impl THandleOp {
    /// Publish the fixed-width Reader payload fact once for every consumer.
    /// The AOT region path uses the typed variant; the ordinary immediate
    /// outcome path below derives its helper and error facts from the same
    /// table.
    pub(crate) fn reader_fixed_width(&self) -> Option<TReaderFixedWidth> {
        Some(match self {
            Self::ReaderReadU8 => TReaderFixedWidth::U8,
            Self::ReaderReadI8 => TReaderFixedWidth::I8,
            Self::ReaderReadU16Le => TReaderFixedWidth::U16Le,
            Self::ReaderReadU16Be => TReaderFixedWidth::U16Be,
            Self::ReaderReadI16Le => TReaderFixedWidth::I16Le,
            Self::ReaderReadI16Be => TReaderFixedWidth::I16Be,
            Self::ReaderReadU32Le => TReaderFixedWidth::U32Le,
            Self::ReaderReadU32Be => TReaderFixedWidth::U32Be,
            Self::ReaderReadI32Le => TReaderFixedWidth::I32Le,
            Self::ReaderReadI32Be => TReaderFixedWidth::I32Be,
            Self::ReaderReadU64Le => TReaderFixedWidth::U64Le,
            Self::ReaderReadU64Be => TReaderFixedWidth::U64Be,
            Self::ReaderReadI64Le => TReaderFixedWidth::I64Le,
            Self::ReaderReadI64Be => TReaderFixedWidth::I64Be,
            Self::ReaderReadF32Le => TReaderFixedWidth::F32Le,
            Self::ReaderReadF32Be => TReaderFixedWidth::F32Be,
            Self::ReaderReadF64Le => TReaderFixedWidth::F64Le,
            Self::ReaderReadF64Be => TReaderFixedWidth::F64Be,
            _ => return None,
        })
    }

    /// Publish fixed-width Reader payload access for the same generic
    /// immediate-outcome optimization used by byte buffers.
    pub(crate) fn outcome_fast_path(&self) -> Option<TOutcomeFastPath> {
        let reader = self.reader_fixed_width()?;
        let (helper, method, width) = reader.fast_path_facts();
        Some(TOutcomeFastPath::FixedRead {
            buffer: TOutcomeFastBuffer::Reader,
            helper,
            error_method: Some(method),
            width,
        })
    }
}

/// One lowered call argument, with the borrow/clone decisions already made (so
/// the emitter reproduces `emit_call_args` without consulting `cx.sigs`).
///
/// Emission order mirrors `emit_call_args` exactly: the clone wrapper (`.clone()`
/// or `Arc::clone(&…)`) is applied to the raw value first, then the borrow wrapper
/// (`&(…)` for a `Read` non-scalar, `&mut (…)` for a `Mutate`).
pub struct TCallArg {
    pub value: TExpr,
    /// D-META-BODY1=A: `b.generate(name) { … }` carries its typed item
    /// template beside the lowered placeholder value.
    pub template_items: Option<Vec<crate::AST::DeriveBodyItem>>,
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
    /// c109 Phase 13: Fn-typed-parameter coercion. When set, emit wraps with
    /// `Rc`/`Arc`/`Box::new` to match `cx.rust_type(&ty)` (unless `already_boxed`),
    /// then ` as <fn-type>`. Named-fn / escaping-lambda values already wrap.
    pub fn_coerce: Option<TFnCoerce>,
    /// D-FIXARR1: a `[T#N]` argument passed to a `[T]` (Vec) slot is widened by
    /// copying into a growable list. When true, emit wraps with `.to_vec()`.
    pub widen_to_vec: bool,
    /// D-UNIONTYPE1=A: a member value passed where a union is expected. When
    /// `Some(union)`, emit wraps as `__jet_<UnionEnum>::<MemberTag>(value)`.
    pub widen_to_union: Option<Type>,
    /// S48: the parameter is a single-trait value slot (`fn show(s: Shape)`) and
    /// this argument is a concrete implementor, so it boxes invisibly. When
    /// `Some(trait)`, emit wraps with `Box::new(value) as Box<dyn <trait>>` —
    /// the same slot-driven boxing a `[Shape]` list element already gets in
    /// `emit_tir_expr`'s `ListLit` arm, decided here so emit stays dumb.
    pub box_as_trait: Option<String>,
}

impl TCallArg {
    /// Return the call-site access and value facts selected during lowering.
    /// Ordinary by-value arguments stay unknown because a missing convention
    /// is not proof of a move or an exclusive access.
    pub fn fact_channel(&self) -> TFactChannel<'_> {
        let mut facts = self.value.fact_channel();
        facts.exclusivity = if self.mut_borrow {
            TExclusivity::Exclusive
        } else if self.borrow {
            TExclusivity::Shared
        } else if matches!(&self.value.kind, TExprKind::ResourceTake(_)) {
            TExclusivity::Move
        } else {
            TExclusivity::Unknown
        };
        facts
    }
}

/// c109 Phase 13: the resolved Fn-typed-argument coercion (`emit_call_args`).
pub struct TFnCoerce {
    /// Target fn type; emit spells via `cx.rust_type`.
    pub ty: Type,
    /// Value already emits `Rc`/`Arc`/`Box::new` — apply only ` as <fn-type>`.
    pub already_boxed: bool,
}

// ---------------------------------------------------------------------------
// The gate: is this function fully inside the Phase-1 subset?
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
