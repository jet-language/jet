use crate::Codegen::Cx;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::{LowerEnv, TExpr};
use crate::Syntax;
use crate::AST::{Expr, Item, ProgramBundle, Type, UnOp};
use std::collections::HashSet;

pub(crate) fn imported_type_name(owner: &str, leaf: &str) -> String {
    format!("{owner}::{leaf}")
}

pub(crate) fn imported_type_owners(bundle: &ProgramBundle, module_idx: usize) -> Vec<String> {
    bundle
        .name_ledger
        .module_identity(module_idx)
        .into_iter()
        .collect()
}

pub(crate) fn module_owned_type_names(items: &[Item]) -> HashSet<String> {
    let mut names = HashSet::new();
    // `Ordering` is NOT listed. It has no source Item because it is declared
    // once per generated crate and imported into every module (`MOD_USE`), so
    // an imported `compare` return keeps the bare nominal and both sides name
    // the same Rust type. Claiming module ownership forced a
    // `__jet_<module>::__jet_Ordering` qualification that the Prelude-owned
    // `__jet_Comparable` rejects (E0053, I2).
    for item in items {
        match item {
            Item::Struct(definition) => {
                names.insert(definition.name.clone());
            }
            Item::Enum(definition) => {
                names.insert(definition.name.clone());
            }
            Item::UnitFamily(family) => {
                names.extend(
                    family
                        .distinct_defs()
                        .iter()
                        .map(|member| member.name.clone()),
                );
            }
            Item::Distinct(definition) => {
                names.insert(definition.name.clone());
            }
            _ => {}
        }
    }
    names
}

fn module_has_nominal_type(items: &[Item], name: &str) -> bool {
    items.iter().any(|item| match item {
        Item::Struct(definition) => definition.name == name,
        Item::Enum(definition) => definition.name == name,
        Item::Distinct(definition) => definition.name == name,
        Item::UnitFamily(family) => family
            .distinct_defs()
            .iter()
            .any(|member| member.name == name),
        _ => false,
    })
}

fn canonical_nominal_name(
    bundle: &ProgramBundle,
    module_idx: usize,
    name: &str,
    owned: &HashSet<String>,
    seen: &mut HashSet<(usize, String)>,
) -> Option<String> {
    if name.contains("::") {
        return Some(name.to_string());
    }
    if !seen.insert((module_idx, name.to_string())) {
        return None;
    }
    if owned.contains(name) || module_has_nominal_type(&bundle.modules[module_idx].items, name) {
        return bundle.name_ledger.nominal_identity(module_idx, name);
    }
    if let Some((namespace, leaf)) = name.rsplit_once('.') {
        let target = bundle
            .name_ledger
            .effective_alias(module_idx, namespace)
            .and_then(|alias| alias.target_module)?;
        return canonical_nominal_name(bundle, target, leaf, &HashSet::new(), seen);
    }
    let alias = bundle.name_ledger.effective_alias(module_idx, name)?;
    let target = alias.target_module?;
    let leaf = alias
        .target
        .rsplit_once('.')
        .map_or(alias.target.as_str(), |(_, leaf)| leaf);
    canonical_nominal_name(bundle, target, leaf, &HashSet::new(), seen)
}

/// Resolve a nominal spelling as written *inside* `module_idx` to the canonical
/// module-qualified identity of the declaration it names. This is the one
/// resolution rule -- own declaration, member-list alias, or `alias.Leaf` path
/// -- shared by field-shape qualification and #2252's runtime projection, so no
/// engine re-derives an owner by matching table-key suffixes.
pub(crate) fn canonical_nominal_from(
    bundle: &ProgramBundle,
    module_idx: usize,
    name: &str,
) -> Option<String> {
    canonical_nominal_name(
        bundle,
        module_idx,
        name,
        &HashSet::new(),
        &mut HashSet::new(),
    )
}

fn qualify_imported_nominal_name(
    bundle: &ProgramBundle,
    target: usize,
    name: &str,
    owned: &HashSet<String>,
    binders: &[String],
) -> String {
    if binders.iter().any(|binder| binder == name) {
        return name.to_string();
    }
    canonical_nominal_name(bundle, target, name, owned, &mut HashSet::new())
        .unwrap_or_else(|| name.to_string())
}

fn rewrite_apply_heads(ty: &Type, qualify: &impl Fn(&str) -> String) -> Type {
    match ty {
        Type::Apply { name, args } => Type::Apply {
            name: qualify(name),
            args: args
                .iter()
                .map(|arg| rewrite_apply_heads(arg, qualify))
                .collect(),
        },
        Type::List(inner) => Type::List(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Map {
            key,
            key_span,
            value,
        } => Type::Map {
            key: Box::new(rewrite_apply_heads(key, qualify)),
            key_span: *key_span,
            value: Box::new(rewrite_apply_heads(value, qualify)),
        },
        Type::Shared(inner) => Type::Shared(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Option(inner) => Type::Option(Box::new(rewrite_apply_heads(inner, qualify))),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(rewrite_apply_heads(ok, qualify)),
            err: Box::new(rewrite_apply_heads(err, qualify)),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            return_view_provenance,
            call_metadata,
        } => Type::Fn {
            params: params
                .iter()
                .map(|param| rewrite_apply_heads(param, qualify))
                .collect(),
            ret: ret
                .as_ref()
                .map(|ret| Box::new(rewrite_apply_heads(ret, qualify))),
            effect_bound: effect_bound.clone(),
            param_contract: param_contract.clone(),
            return_view_provenance: return_view_provenance.clone(),
            call_metadata: call_metadata.clone(),
        },
        Type::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), Box::new(rewrite_apply_heads(ty, qualify))))
                .collect(),
        ),
        Type::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(rewrite_apply_heads(elem, qualify)),
            len: len.clone(),
        },
        Type::Tagged { marker, inner } => Type::Tagged {
            marker: marker.clone(),
            inner: Box::new(rewrite_apply_heads(inner, qualify)),
        },
        Type::Union(members) => crate::AST::canonicalize_union(
            members
                .iter()
                .map(|member| rewrite_apply_heads(member, qualify))
                .collect(),
        ),
        Type::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(rewrite_apply_heads(base, qualify)),
            dimension: dimension.clone(),
        },
        Type::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(rewrite_apply_heads(base, qualify)),
            lo: *lo,
            hi: *hi,
        },
        _ => ty.clone(),
    }
}

/// Keep every nominal reference owned by an imported module under its canonical
/// module identity. This is used by both field-shape registration and
/// cross-module call metadata, so nested/generic references share one key.
pub(crate) fn qualify_imported_type(
    bundle: &ProgramBundle,
    target: usize,
    _owner: &str,
    binders: &[String],
    ty: &Type,
) -> Type {
    let owned = module_owned_type_names(&bundle.modules[target].items);
    let mapped = ty.map_named_types(&|name| {
        let qualified = qualify_imported_nominal_name(bundle, target, name, &owned, binders);
        (qualified != name).then_some(qualified)
    });
    rewrite_apply_heads(&mapped, &|name| {
        qualify_imported_nominal_name(bundle, target, name, &owned, binders)
    })
}

/// Register imported struct shapes under canonical nominal identities. A
/// leaf-only table cannot distinguish two imported modules that export the same
/// nominal.
pub(crate) fn register_imported_struct_shapes(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    module_idx: usize,
) {
    let module = &bundle.modules[module_idx];
    let own_type_names = module_owned_type_names(&module.items);
    let mut imported = Vec::<(usize, String)>::new();
    for import in &module.imports {
        if import.is_c_import().unwrap_or_else(|error| {
            unreachable!("invalid foreign import reached codegen: {}", error.path)
        }) {
            continue;
        }
        let Some(target) = bundle.name_ledger.import_target(module_idx, import.span) else {
            continue;
        };
        for item in &bundle.modules[target].items {
            if let Item::Struct(definition) = item {
                if bundle
                    .name_ledger
                    .visible(module_idx, target, &definition.name)
                {
                    imported.push((target, definition.name.clone()));
                }
            }
        }
    }
    imported.extend(crate::Codegen::Imports::selective_nominal_targets(
        bundle, module_idx,
    ));
    for ((_, _), (rust_mod, _)) in crate::Codegen::Imports::reexport_call_map(bundle, module_idx) {
        let Some(target) = bundle
            .modules
            .iter()
            .position(|candidate| crate::Codegen::mangle(&candidate.alias) == rust_mod)
        else {
            continue;
        };
        for item in &bundle.modules[target].items {
            if let Item::Struct(definition) = item {
                if bundle
                    .name_ledger
                    .visible(module_idx, target, &definition.name)
                {
                    imported.push((target, definition.name.clone()));
                }
            }
        }
    }
    imported.sort();
    imported.dedup();
    for (target, definition_name) in imported {
        // A local declaration shadows an imported leaf for bare struct
        // literals. Do not let the imported shape overwrite the local
        // identity in `local_type_identities`; qualified `alias.Type`
        // literals still resolve through `foreign_types`.
        if own_type_names.contains(&definition_name) {
            continue;
        }
        let Some(definition) = bundle.modules[target]
            .items
            .iter()
            .find_map(|item| match item {
                Item::Struct(definition) if definition.name == definition_name => Some(definition),
                _ => None,
            })
        else {
            continue;
        };
        register_struct_shape(cx, bundle, target, definition);
    }
}

/// #2252: the module that DECLARES a nominal knows it only by its local leaf,
/// but its own items are lowered under the canonical owner whenever another
/// context consumes them (the imported-module pass in `TIR/mod.rs` lowers
/// `plan.jet`'s methods and generated codecs as `<dir>::plan.jet::ListReport::…`).
/// A leaf-only shape table cannot answer that owner key, so the structural gate
/// refused every such method and the default tier deopted on a method the
/// program plainly declares. Registering the declaring module's own nominals
/// through the same seam keeps ONE canonical qualified-shape fact per type,
/// identical to the row a consumer context already holds.
pub(crate) fn register_own_struct_shapes(cx: &mut Cx, bundle: &ProgramBundle, module_idx: usize) {
    for item in &bundle.modules[module_idx].items {
        match item {
            Item::Struct(definition) => {
                register_struct_shape(cx, bundle, module_idx, definition);
            }
            Item::Enum(definition) if definition.type_params.is_empty() => {
                register_enum_shape(cx, bundle, module_idx, definition);
            }
            _ => {}
        }
    }
}

/// A generated codec can belong to a private enum just as it can to a private
/// struct. Keep the variant payloads under the same canonical owner used by
/// imported calls, so lowering does not depend on export visibility.
fn register_enum_shape(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    target: usize,
    definition: &crate::AST::EnumDef,
) {
    let owner = bundle
        .name_ledger
        .module_identity(target)
        .expect("name ledger must contain every loaded module");
    let qualified = imported_type_name(&owner, &definition.name);
    let binders = definition
        .type_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    cx.local_type_identities
        .insert(definition.name.clone(), qualified.clone());
    let variants = definition
        .variants
        .iter()
        .map(|variant| {
            (
                variant.name.clone(),
                qualify_variant_payload(bundle, target, &owner, &binders, &variant.payload),
            )
        })
        .collect();
    let rust_mod = crate::Codegen::mangle(&bundle.modules[target].alias);
    cx.type_names.insert(qualified.clone());
    cx.foreign_types.insert(qualified.clone(), rust_mod);
    cx.enum_variants.insert(qualified.clone(), variants);
    for variant in &definition.variants {
        cx.variant_owner
            .entry(variant.name.clone())
            .or_insert_with(|| qualified.clone());
    }
}

fn qualify_variant_payload(
    bundle: &ProgramBundle,
    target: usize,
    owner: &str,
    binders: &[String],
    payload: &crate::AST::VariantPayload,
) -> crate::AST::VariantPayload {
    match payload {
        crate::AST::VariantPayload::Unit => crate::AST::VariantPayload::Unit,
        crate::AST::VariantPayload::Single(ty, span) => crate::AST::VariantPayload::Single(
            qualify_imported_type(bundle, target, owner, binders, ty),
            *span,
        ),
        crate::AST::VariantPayload::Named(fields) => crate::AST::VariantPayload::Named(
            fields
                .iter()
                .map(|field| {
                    let mut qualified = field.clone();
                    qualified.ty =
                        qualify_imported_type(bundle, target, owner, binders, &field.ty);
                    qualified
                })
                .collect(),
        ),
    }
}

/// One canonical nominal row: the shape facts a struct owned by `target` carries
/// under its module identity. Field and reflection types are qualified through
/// the same helper the call metadata uses, so nested references share one key.
fn register_struct_shape(
    cx: &mut Cx,
    bundle: &ProgramBundle,
    target: usize,
    definition: &crate::AST::StructDef,
) {
    let owner = bundle
        .name_ledger
        .module_identity(target)
        .expect("name ledger must contain every loaded module");
    let qualified = imported_type_name(&owner, &definition.name);
    let binders = definition
        .type_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    cx.local_type_identities
        .insert(definition.name.clone(), qualified.clone());
    let rust_mod = crate::Codegen::mangle(&bundle.modules[target].alias);
    let target_type_names: HashSet<String> = bundle.modules[target]
        .items
        .iter()
        .flat_map(|item| match item {
            Item::Struct(definition) => vec![definition.name.clone()],
            Item::Enum(definition) => vec![definition.name.clone()],
            Item::UnitFamily(family) => family
                .distinct_defs()
                .iter()
                .map(|member| member.name.clone())
                .collect(),
            Item::Distinct(definition) => vec![definition.name.clone()],
            _ => Vec::new(),
        })
        .collect();
    let fields = definition
        .reflection_fields()
        .map(|field| {
            (
                field.name.clone(),
                qualify_imported_type(bundle, target, &owner, &binders, &field.ty),
            )
        })
        .collect::<Vec<(String, Type)>>();
    let reflection_fields = jet_foundation::Reflection::fields(definition)
        .into_iter()
        .map(|mut field| {
            field.ty = qualify_imported_type(bundle, target, &owner, &binders, &field.ty);
            field
        })
        .collect::<Vec<_>>();
    cx.type_names.insert(qualified.clone());
    cx.foreign_types.insert(qualified.clone(), rust_mod);
    cx.struct_fields.insert(qualified.clone(), fields);
    let computed = definition
        .fields
        .iter()
        .filter(|field| field.computed.is_some())
        .map(|field| field.name.clone())
        .collect::<HashSet<_>>();
    if !computed.is_empty() {
        cx.computed_fields.insert(qualified.clone(), computed);
    }
    let (memo_fields, memo_dependencies) =
        crate::Codegen::Context::memo_facts_for_struct(definition);
    if !memo_fields.is_empty() {
        cx.memo_fields.insert(qualified.clone(), memo_fields);
    }
    if !memo_dependencies.is_empty() {
        cx.memo_dependencies
            .insert(qualified.clone(), memo_dependencies);
    }
    cx.reflection_fields
        .insert(qualified.clone(), reflection_fields);
    if crate::Codegen::type_is_cloneable_struct(definition, &target_type_names) {
        cx.cloneable.insert(qualified.clone());
    }
    if !definition.type_params.is_empty() {
        let params = definition
            .type_params
            .iter()
            .map(|param| param.name.clone())
            .collect::<Vec<_>>();
        cx.struct_type_params
            .insert(qualified.clone(), params.iter().cloned().collect());
        cx.struct_type_param_order.insert(qualified, params);
    }
}

/// D-INCR1: the structured place `++`/`--` reads and updates. A bare identifier
/// resolves to its slot; anything else is the already-structured place expression
/// the operand lowers to.
pub(super) fn lower_incdec_place(
    operand: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> crate::Codegen::TIR::TPlace {
    use crate::Codegen::TIR::TPlace;
    match operand {
        Expr::Ident(name, _) => TPlace::Local(
            cx.persistent_local(name)
                .unwrap_or_else(|| env.local_of(name)),
        ),
        other => TPlace::Expr(Box::new(lower_expr(other, cx, env))),
    }
}

/// Replay codegen's `operand_is_integer` (Codegen/Expression.rs) on an AST
/// operand, using the lowering env for identifier types. The result MUST match
/// that function bit-for-bit so the TIR's overflow-trap decision is identical to
/// the AST path's. Like the original: literals/negation/nested-arithmetic-left
/// resolve structurally; an `Ident` resolves via its slot type; everything else
/// (notably a struct-field read) is unresolved (`None`) and so never traps.
pub(crate) fn ast_operand_is_integer(e: &Expr, env: &LowerEnv) -> Option<bool> {
    match e {
        Expr::Int(..) => Some(true),
        Expr::Float(..) => Some(false),
        Expr::Unary(UnOp::Neg, inner, _) => ast_operand_is_integer(inner, env),
        Expr::Binary(_, l, _, _) => ast_operand_is_integer(l, env),
        // Mirror `expr_jet_ty`: only `Ident`/`Str`/`Char` resolve here. A `Field`
        // (and anything else) resolves to `None` — exactly as the AST path does,
        // so a field operand never enables the overflow trap.
        Expr::Ident(name, _) => env.ty_of(name).map(|t| t.is_integer()),
        Expr::Str(..) => Some(false),
        Expr::Char(..) => Some(false),
        _ => None,
    }
}

/// Look up a field's declared type on a resolved struct receiver type. Returns
/// `None` when the receiver is not a known struct or the field is absent — both
/// impossible for a covered function (sema validated the access).
pub(crate) fn struct_field_type(cx: &Cx, recv_ty: &Type, field: &str) -> Option<Type> {
    // D-PIN2=A / D-PIN3=A: a pin is a window onto a place, so reaching a field
    // through `Pin<T>` resolves against `T`. The field's own declared type is
    // the mark: a `Pin<U>` field comes back as `Pin<U>` and stays a window.
    if let Type::Apply { name, args } = recv_ty {
        if name == crate::Syntax::TYPE_PIN && args.len() == 1 {
            return struct_field_type(cx, &args[0], field);
        }
    }
    // D-PATCH1: sema synthesizes `T.Patch` outside the source shape table.
    // Reconstruct each patch field from its concrete base field; falling
    // through to the old integer sentinel would erase `?T` at every patch
    // projection and make the resident carrier read a result handle as `T`.
    if let Type::Named(name) = recv_ty {
        if let Some(base_name) = name.strip_suffix(".Patch") {
            return cx
                .struct_fields
                .get(base_name)
                .and_then(|fields| {
                    fields
                        .iter()
                        .find(|(candidate, _)| candidate == field)
                        .map(|(_, ty)| Type::Option(Box::new(ty.clone())))
                });
        }
    }
    // D-SHAREDGUARD2=A: `SharedGuard.value` is a compiler-known place rather
    // than a stored public field. Keep the TIR projection in sync with sema,
    // including the hidden read/edit tag carried by guard values.
    if field == "value" {
        if let Type::Apply { name, args } = recv_ty {
            if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                return Some(args[0].clone());
            }
        }
        if let Type::Tagged { marker, inner } = recv_ty {
            if matches!(
                marker,
                crate::AST::TagMarker::Internal(
                    crate::AST::InternalTag::SharedGuardRead
                        | crate::AST::InternalTag::SharedGuardEdit
                )
            ) {
                if let Type::Apply { name, args } = inner.as_ref() {
                    if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 {
                        return Some(args[0].clone());
                    }
                }
            }
        }
    }
    // c109 Phase 23: a named-tuple field read (`p.x`) — resolve the field's type off
    // the `Type::Tuple` directly (a tuple has no `cx.struct_fields` entry; its struct
    // is the generated `JetTup_<hash>`). Keeps the field read's result type total.
    if let Type::Tuple(fields) = recv_ty {
        return fields
            .iter()
            .find(|(f, _)| f == field)
            .map(|(_, t)| (**t).clone());
    }
    // Card 2021: a CORE record's field types are NOT restated here. This
    // function used to carry a hand-kept ladder of 41 core structs, while sema
    // declared 115 — so every field of the other 74 (`ProcessResult` among
    // them) resolved through the caller's `.unwrap_or(Type::Int)` and print
    // picked the INTEGER accessor for a `String`. Reading the declaring table
    // instead makes the answer total for every core struct at once, which is
    // what a site-local repair of one field could never do.
    //
    // Precedence mirrors `Checker::field_type` (CheckerInfer/expr.rs) exactly:
    // a user struct claiming the name wins (D-SHIFT1 user-type-wins), and only
    // then does the reserved core shape answer. Codegen must agree with the
    // types sema already committed to; disagreeing is how rustc gets handed
    // Jet's own ill-typed output, which is an internal compiler error (I2).
    // #2252: an imported nominal's shape row is registered under its DECLARING
    // module's canonical identity, while the consumer names the same type by an
    // alias path (`plan.ListReport`) or a bare leaf. Project the spelling
    // through `imported_type_metadata_name` -- the one name-ledger-derived
    // resolver every other backend fact (unit labels, quantities, distinct
    // bases) already reads -- so an imported field answers with its declared
    // type. Without it the miss fell through to the caller's
    // `unwrap_or(Type::Int)` and handed rustc `jet_int_to_string(<String>)` for
    // a `String` field, which is an internal compiler error (I2).
    let shape_key = |name: &str| -> Option<String> {
        if cx.struct_fields.contains_key(name) {
            return Some(name.to_string());
        }
        cx.imported_type_metadata_name(name)
            .filter(|identity| cx.struct_fields.contains_key(identity))
    };
    if let Type::Apply { name, args } = recv_ty {
        if let Some(key) = shape_key(name) {
            let fields = cx.struct_fields.get(&key)?;
            let field_ty = fields
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, t)| t.clone())?;
            let params = cx.struct_type_param_order.get(&key)?;
            let subst = params
                .iter()
                .zip(args)
                .map(|(param, arg)| (param.clone(), arg.clone()))
                .collect();
            return Some(crate::Generics::substitute_type(&field_ty, &subst));
        }
        // A reserved core GENERIC (`DataJoin<L, R>`, `VjpRun<T>`, `Rotation<T>`)
        // resolves its field
        // against its type arguments, so a chained access (`r.migration
        // .migrated`) types the intermediate instead of mis-mangling the next
        // field.
        return crate::Sema::core_struct_field_type(name, field, args);
    }
    let Type::Named(name) = recv_ty else {
        return None;
    };
    if let Some(field_ty) = shape_key(name)
        .and_then(|key| cx.struct_fields.get(&key))
        .and_then(|fields| fields.iter().find(|(f, _)| f == field))
        .map(|(_, t)| t.clone())
    {
        return Some(field_ty);
    }
    crate::Sema::core_struct_field_type(name, field, &[])
}

/// The type of an integer literal given its elaborated width.
pub(crate) fn int_lit_type(width: &Option<(bool, u8)>) -> Type {
    match width {
        Some((signed, bits)) => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        None => Type::Int,
    }
}

pub(crate) fn unit_type() -> Type {
    Type::Named("Unit".to_string())
}

pub(crate) fn let_ty_for_opt(
    ty: Option<&Type>,
    is_resource: bool,
    gc: bool,
) -> crate::Codegen::TIR::TLetTy {
    use crate::Codegen::TIR::TLetTy;
    let Some(ty) = ty else {
        return TLetTy::Inferred;
    };
    if is_resource {
        return TLetTy::resource(ty.clone());
    }
    if gc {
        return TLetTy::automatic_root(ty.clone());
    }
    TLetTy::plain(ty.clone())
}

pub(crate) fn let_ty_tuple(types: Vec<Type>) -> crate::Codegen::TIR::TLetTy {
    crate::Codegen::TIR::TLetTy::Tuple(types)
}

/// A comptime scalar as a structured literal node. Sema already folded the
/// value, so the scalar cases carry the number/text/flag/char itself instead of the
/// rendered Rust text — every engine reads the fact, and emit still renders the
/// same bytes `CtValue::serialize` would have produced.
pub(crate) fn lower_comptime_scalar(
    value: Option<&crate::AST::CtValue>,
    ty: Option<&Type>,
) -> Option<crate::Codegen::TIR::TExprKind> {
    use crate::Codegen::TIR::{TExprKind, TStrPart};
    match value? {
        crate::AST::CtValue::Int(int) => Some(TExprKind::IntLit(
            *int,
            match ty {
                Some(Type::IntN { signed, bits }) => Some((*signed, *bits)),
                _ => None,
            },
        )),
        crate::AST::CtValue::BigInt(value) => Some(TExprKind::CtLit(
            crate::AST::CtValue::BigInt(value.clone()),
        )),
        crate::AST::CtValue::Float(float) => Some(TExprKind::FloatLit(float.as_f64())),
        crate::AST::CtValue::Bool(flag) => Some(TExprKind::BoolLit(*flag)),
        crate::AST::CtValue::Char(ch) => Some(TExprKind::CharLit(*ch)),
        crate::AST::CtValue::Str(text) => {
            Some(TExprKind::StrLit(vec![TStrPart::Lit(text.clone())]))
        }
        // D-TYPE2-DEFAULT1 / #2774: a folded Fraction is still an opaque
        // Prelude carrier, not a generated user struct. Rebuild it through
        // the canonical constructor so AOT receives the same reduced ratio
        // as the comptime and resident engines.
        crate::AST::CtValue::Struct { type_name, .. }
            if type_name == Syntax::TYPE_FRACTION =>
        {
            let fraction = crate::Numeric::CtFraction::from_value(value?).ok()?;
            let numerator = fraction.numerator.try_i64()?;
            let denominator = fraction.denominator.try_i64()?;
            Some(TExprKind::PreciseBuiltin {
                type_name: Syntax::TYPE_FRACTION.to_string(),
                func: "from_parts".to_string(),
                args: vec![
                    TExpr {
                        ty: Type::Int,
                        kind: TExprKind::IntLit(numerator, None),
                    },
                    TExpr {
                        ty: Type::Int,
                        kind: TExprKind::IntLit(denominator, None),
                    },
                ],
            })
        }
        // D-TYPE2-IMAG1=A: a folded Complex still enters the canonical
        // precise constructor seam. This keeps AOT, resident JIT, and web
        // from treating its CtValue struct as an unrelated user record.
        crate::AST::CtValue::Struct { type_name, fields } if type_name == Syntax::TYPE_COMPLEX => {
            let part = |name: &str| {
                fields
                    .iter()
                    .find(|(field, _)| field == name)
                    .and_then(|(_, value)| match value {
                        crate::AST::CtValue::Int(value) => Some(*value as f64),
                        crate::AST::CtValue::Float(value) => Some(value.as_f64()),
                        _ => None,
                    })
            };
            let real = part("real")?;
            let imaginary = part("imaginary")?;
            Some(TExprKind::PreciseBuiltin {
                type_name: Syntax::TYPE_COMPLEX.to_string(),
                func: "from_parts".to_string(),
                args: vec![
                    TExpr {
                        ty: Type::Float,
                        kind: TExprKind::FloatLit(real),
                    },
                    TExpr {
                        ty: Type::Float,
                        kind: TExprKind::FloatLit(imaginary),
                    },
                ],
            })
        }
        _ => None,
    }
}

/// The resolved return carrier of a called plain function.
///
/// `Func::return_type` stores the source success type for a bare declaration,
/// while the emitted callable always returns its effective failure carrier.
/// Project that same carrier here so a call inside an inferred fallible
/// callback has the ABI type that its generated Rust value already has.
pub(crate) fn call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret, .. }) => {
            let declared = ret.as_deref().map(|ty| cx.expand_type_aliases(ty));
            jet_foundation::AST::FailureContract::from_return_type(declared.as_ref())
                .effective_type()
        }
        // c109 Phase 23: a distinct-type constructor `UserId(x)` yields the distinct
        // type itself (it has no `fn_types` entry). Keeps the call's result type total.
        _ if cx.distinct_types.contains_key(name) => Type::Named(name.to_string()),
        _ => unit_type(),
    }
}
/// Return the ABI return stored for a foreign call.
///
/// `Context` keeps `extern rust`/C-module declarations at their raw bridge ABI,
/// while `#Import(c)` remains an ordinary Jet function with its effective
/// carrier. Read `fn_types` directly; `fn_source_types` would erase that
/// distinction by restoring the source spelling for guest imports.
pub(crate) fn extern_call_return_type(cx: &Cx, name: &str) -> Type {
    match cx.fn_types.get(name) {
        Some(Type::Fn { ret, .. }) => ret
            .as_deref()
            .map(|ty| cx.expand_type_aliases(ty))
            .unwrap_or_else(unit_type),
        _ => unit_type(),
    }
}

/// Project a checked cross-module return fact onto the emitted callable's
/// hidden carrier.  The checked fact is authoritative: a plain source return
/// becomes the shared default `Result`, while explicit `Result`/`Option` and
/// `Never` retain their declared carrier.
pub(crate) fn module_call_target_return(
    cx: &Cx,
    resolved_ret: Option<&Type>,
) -> Result<Type, String> {
    let Some(resolved_ret) = resolved_ret else {
        return Err("checked module call has no resolved return type".to_string());
    };
    let resolved_ret = cx.expand_type_aliases(resolved_ret);
    // `lower_func_with_web_boundary` keeps a source-declared Stream raw rather
    // than lifting it into the ordinary failure carrier. Mirror that existing
    // TFunc rule before expanding aliases.
    if matches!(
        &resolved_ret,
        Type::Apply { name, args }
            if name == crate::Syntax::TYPE_STREAM && args.len() == 1
    ) {
        return Ok(resolved_ret);
    }
    Ok(
        jet_foundation::AST::FailureContract::from_return_type(Some(&resolved_ret))
            .effective_type(),
    )
}

/// Resolve the effective return contract recorded for a source-module import.
/// The outer `Option` distinguishes missing import metadata from a target whose
/// source declaration has no explicit return type. Jet module calls must still
/// use the checked call fact for the inner carrier projection.
pub(crate) fn imported_module_call_target_return(
    cx: &Cx,
    alias: &str,
    method: &str,
    resolved_ret: Option<&Type>,
) -> Result<Option<Type>, String> {
    let Some(declared) = cx
        .import_rets
        .get(&(alias.to_string(), method.to_string()))
    else {
        return Err(format!(
            "checked module call `{alias}.{method}` has no import return fact"
        ));
    };
    let direct_c_function = cx.import_mods.get(alias).is_some_and(|module| {
        cx.direct_c_functions
            .contains(&format!("{module}::{method}"))
    });
    if direct_c_function {
        // CModule wrappers are emitted with the binding's declared C ABI return
        // type. They are not Jet callables and must not acquire a hidden `?`.
        return Ok(Some(declared.clone().unwrap_or_else(unit_type)));
    }
    module_call_target_return(cx, resolved_ret).map(Some)
}

/// Resolve the source-visible return type of a cross-module target from sema's
/// checked call fact.  The generated callable has the effective carrier, but
/// a module call's `TExpr.ty` stays on the source side until MIR applies the
/// carrier seam.
pub(crate) fn module_call_source_return_type(
    cx: &Cx,
    name: &str,
    resolved_ret: Option<&Type>,
) -> Result<Type, String> {
    let Some(resolved_ret) = resolved_ret else {
        return Err(format!(
            "checked module call `{name}` has no resolved return type"
        ));
    };
    Ok(cx.expand_type_aliases(resolved_ret))
}

/// Resolve a generic call's result using the explicit arguments first, then
/// the concrete lowered argument types. This is the codegen-side mirror of
/// sema's substitution; engines receive a concrete TIR type, never a binder.
pub(crate) fn call_return_type_with_args(
    cx: &Cx,
    name: &str,
    type_args: &[Type],
    args: &[crate::Codegen::TIR::TCallArg],
) -> Type {
    let declared = call_return_type(cx, name);
    substitute_call_type(cx, name, declared, type_args, args)
}

fn substitute_call_type(
    cx: &Cx,
    name: &str,
    declared: Type,
    type_args: &[Type],
    args: &[crate::Codegen::TIR::TCallArg],
) -> Type {
    let Some(params) = cx.fn_type_params.get(name) else {
        return declared;
    };
    if params.is_empty() {
        return declared;
    }
    let Some(order) = cx.fn_type_param_order.get(name) else {
        return declared;
    };
    let mut subst = std::collections::HashMap::new();
    for (param, actual) in order.iter().zip(type_args) {
        subst.insert(param.clone(), cx.expand_type_aliases(actual));
    }
    if let Some(sig) = cx.sigs.get(name) {
        for ((_, template), actual) in sig.iter().zip(args) {
            let actual_ty = if actual.widen_to_vec {
                match &actual.value.ty {
                    Type::FixedList { elem, .. } => Type::List(elem.clone()),
                    other => other.clone(),
                }
            } else {
                actual.value.ty.clone()
            };
            if !crate::Codegen::TIR::bind_generic_type(template, &actual_ty, params, &mut subst) {
                return declared;
            }
        }
    }
    crate::Generics::substitute_type(&declared, &subst)
}
