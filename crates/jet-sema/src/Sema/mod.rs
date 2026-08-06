//! Semantic checks. Everything here exists so that codegen can stay "dumb"
//! (invariant I3): by the time a Program reaches codegen, it must be
//! impossible for the generated Rust to fail to compile (invariant I2).
//!
//! M1: type inference, mutability, definite-return analysis.
//! M2: ownership — moves, call-site `&`/`^`,
//! view returns, use-after-move, and borrow rules that keep generated Rust
//! sound without surfacing Rust concepts to users.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Traits::TraitRegistry;
use crate::AST::{
    AccessConvention, Expr, ExternFn, Func, QuantityKind, Stmt, Type, VariantPayload,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

mod Casing;

/// Re-export so existing callers (`jet::Sema::FuncSig`) keep working.
pub use crate::AST::FuncSig;

#[derive(Debug, Clone)]
struct UninitState {
    fixed_len: Option<u64>,
    initialized_indexes: BTreeSet<u64>,
}

impl UninitState {
    fn scalar() -> Self {
        Self {
            fixed_len: None,
            initialized_indexes: BTreeSet::new(),
        }
    }

    fn fixed(len: u64) -> Self {
        Self {
            fixed_len: Some(len),
            initialized_indexes: BTreeSet::new(),
        }
    }

    #[allow(dead_code)]
    fn merge_paths(&mut self, other: &Self) {
        if self.fixed_len == other.fixed_len {
            self.initialized_indexes = self
                .initialized_indexes
                .intersection(&other.initialized_indexes)
                .copied()
                .collect();
        } else {
            self.fixed_len = None;
            self.initialized_indexes.clear();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MethodSig {
    name_span: Span,
    is_pub: bool,
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    /// D-GENERIC-CALL1=A: method-owned type parameters, distinct from the
    /// receiver's generic arguments.
    type_params: Vec<crate::AST::TypeParam>,
    is_static: bool,
    self_conv: Option<AccessConvention>,
    /// D-NARG1 (S61): parameter names and default-value presence, parallel to
    /// `params`. Excludes `self` (index 0 of params is self when self_conv is
    /// Some; param_info starts from the first non-self param).
    pub(crate) param_info: Vec<(String, bool)>,
    /// D-APILABEL1=A: public call label and zone, parallel to `param_info`
    /// (so it also excludes `self`).
    pub(crate) param_call: Vec<(String, crate::AST::ParamZone)>,
    /// D-VARIADIC1: parallel to `param_info` — true when that parameter is a
    /// rest parameter. The binder needs it to know the parameter may be left
    /// unbound; without it a labelled call to a variadic method reports a
    /// missing argument that is not missing.
    pub(crate) param_variadic: Vec<bool>,
    /// D-NARG1 (S61): default expressions for parameters, parallel to param_info.
    /// `None` when no default; only trailing params may have defaults.
    pub(crate) defaults: Vec<Option<crate::AST::Expr>>,
    /// D-MUSTUSE1 (c18iwxqx): `#MustUse` method — return cannot be silently ignored (E0419).
    pub(crate) must_use: bool,
    pub(crate) return_view_provenance: crate::AST::ViewProvenanceCell,
}

#[derive(Debug, Clone)]
pub(crate) enum TypeDef {
    Struct {
        fields: Vec<(String, Span, Type, bool)>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `struct`.
        /// Values of this type must be consumed exactly once (E0140/E0141) and
        /// may not be aliased (E0142).
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `#MustUse` was present before `struct`.
        must_use: bool,
        /// D-SOA1 / D-SOA2A=C: `#layout(columnar)` was present. A `[S]` of this
        /// struct is stored struct-of-arrays; sema gates the list-op surface to
        /// the v1-supported subset (E1108) and codegen lowers it columnar.
        columnar: bool,
        /// D-REPRC1: `#Layout(c)` was present — codegen stamps `#[repr(C)]`
        /// on the generated Rust struct, so field order/size/padding match C.
        /// A plain struct (no `#Layout(c)`) has an UNSPECIFIED Rust layout and
        /// must never be accepted at the C FFI boundary (card #436 / E3203) —
        /// only this flag makes `c_named_type_ok` (Sema/FFI.rs) say yes.
        is_c_layout: bool,
    },
    Enum {
        variants: HashMap<String, (Span, VariantPayload)>,
        variant_order: Vec<String>,
        /// D-TAG1: variant groups — group path → (span, ordered leaf paths in
        /// its subtree). A group name matches its whole subtree in patterns.
        groups: HashMap<String, (Span, Vec<String>)>,
        methods: HashMap<String, MethodSig>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `enum`.
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `#MustUse` was present before `enum`.
        must_use: bool,
        /// D-REPRC2: present only for `#Layout(c[, tag: Width])`.
        c_layout_tag: Option<crate::AST::CEnumTag>,
    },
    /// D-DIST1 (ratified 2026-06-19): a distinct type — a nominal wrapper over
    /// a base type. No implicit coercion either direction (E0128). Arithmetic
    /// only when `is_numeric` (D-DIST3, E0127).
    Distinct {
        base: Type,
        is_numeric: bool,
        /// D-CAPBUNDLE1: `#Comparable`/`#Printable`/`#CodableAsBase` grants.
        is_comparable: bool,
        is_printable: bool,
        is_codable_as_base: bool,
        /// D-RANGETYPE1: `distinct Int(0..10)` — the inclusive `(lo, hi)`
        /// bounds this type provably holds. `None` for a plain distinct type.
        range: Option<(i64, i64)>,
    },
    /// D-TYPEALIAS1 (ratified 2026-06-28): `alias Name<T> = …` — transparent
    /// generic shortcut; expands in sema, erases at codegen.
    Alias {
        params: Vec<crate::AST::TypeParam>,
        target: Type,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct UnitFact {
    package: std::path::PathBuf,
    family: String,
    member: String,
    /// D-DIMENSION-OPEN1=D: `None` for a family that never opted into a
    /// dimension. Its members still convert inside the family.
    dimension: Option<crate::AST::Dimension>,
    scale: crate::AST::UnitRatio,
    scale_provenance: crate::AST::UnitScaleProvenance,
    offset: crate::AST::UnitRatio,
    kind: QuantityKind,
}

impl UnitFact {
    fn is_measured(&self) -> bool {
        matches!(
            self.scale_provenance,
            crate::AST::UnitScaleProvenance::Measured { .. }
        )
    }

    fn is_symbolic(&self) -> bool {
        matches!(
            self.scale_provenance,
            crate::AST::UnitScaleProvenance::SymbolicPi { .. }
        )
    }

    fn conversion_parts_to(
        &self,
        destination: &Self,
    ) -> Option<(crate::AST::UnitRatio, crate::AST::UnitRatio)> {
        let scale = self.scale.div(&destination.scale).ok()?;
        let offset = self
            .offset
            .sub(&destination.offset)
            .ok()?
            .div(&destination.scale)
            .ok()?;
        Some((scale, offset))
    }

    fn conversion_is_total_to(&self, destination: &Self) -> bool {
        self.conversion_parts_to(destination).is_some_and(|(scale, offset)| {
            scale == crate::AST::UnitRatio::integer(1)
                && offset == crate::AST::UnitRatio::zero()
        })
    }

    fn conversion_is_finite_to(&self, destination: &Self) -> bool {
        self.conversion_parts_to(destination).is_some_and(|(scale, offset)| {
            [scale, offset].into_iter().all(|value| {
                value
                    .num
                    .to_string()
                    .parse::<f64>()
                    .is_ok_and(f64::is_finite)
                    && value
                        .den
                        .to_string()
                        .parse::<f64>()
                        .is_ok_and(f64::is_finite)
            })
        })
    }

    fn converted_value_is_exact_to(&self, destination: &Self, value: f64) -> bool {
        let Some((scale, offset)) = self.conversion_parts_to(destination) else {
            return false;
        };
        jet_foundation::jet_unit_conversion_exact(
            value,
            &scale.num.to_string(),
            &scale.den.to_string(),
            &offset.num.to_string(),
            &offset.den.to_string(),
        )
        .is_some()
    }
}

pub(crate) struct TypeRegistry {
    types: HashMap<String, TypeDef>,
    /// D-QUANTITY-PRINT1: all concrete types minted by `#UnitFamily`.
    unit_types: HashSet<String>,
    /// #603: the one normalized source of truth for concrete unit conversion,
    /// package identity, dimension, affine kind, and algebra facts.
    unit_facts: HashMap<String, UnitFact>,
    /// D-FIELDPOL1: struct name → computed field name → (span, declared
    /// type). A computed field never appears in `TypeDef::Struct::fields`
    /// (it's not a stored field, and is never required/allowed in a struct
    /// literal — E0339); this side table is the only place sema resolves its
    /// type for a *read* (`field_type`).
    computed_fields: HashMap<String, HashMap<String, (Span, Type)>>,
    /// D-FIELDDEF1=C: struct name → field name → default expression for omitted
    /// `Type.{ … }` construction and wire/CLI absence.
    field_defaults: HashMap<String, HashMap<String, crate::AST::Expr>>,
}

impl TypeRegistry {
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    /// A struct the user declared, so codegen emits a `user_<Name>` Rust type
    /// for it. False for a builtin the comptime evaluator merely models as a
    /// struct, which has no such Rust type.
    pub(crate) fn is_user_struct(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Struct { .. }))
    }

    /// The enum counterpart of `is_user_struct`.
    pub(crate) fn is_user_enum(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Enum { .. }))
    }

    pub(crate) fn unit_dimension(&self, name: &str) -> Option<crate::AST::Dimension> {
        self.unit_facts.get(name).and_then(|fact| fact.dimension.clone())
    }

    pub(crate) fn is_unit_type(&self, name: &str) -> bool {
        self.unit_types.contains(name)
    }

    pub(crate) fn unit_fact(&self, name: &str) -> Option<&UnitFact> {
        self.unit_facts.get(name)
    }

    fn struct_fields(&self, name: &str) -> Option<&[(String, Span, Type, bool)]> {
        match self.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => Some(fields.as_slice()),
            _ => None,
        }
    }

    /// D-FIELDPOL1: `name`'s computed fields (field name → span + declared
    /// type), or `None` when `name` isn't a struct / has none.
    pub(crate) fn computed_field_types(
        &self,
        name: &str,
    ) -> Option<&HashMap<String, (Span, Type)>> {
        self.computed_fields.get(name)
    }

    /// D-FIELDDEF1=C: default expressions for omitted struct-literal fields.
    pub(crate) fn field_defaults(
        &self,
        name: &str,
    ) -> Option<&HashMap<String, crate::AST::Expr>> {
        self.field_defaults.get(name)
    }

    /// D-SOA1: true when `name` is a `#layout(columnar)` struct (its `[name]`
    /// collections are stored struct-of-arrays).
    fn is_columnar(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct { columnar: true, .. })
        )
    }

    fn enum_variants(&self, name: &str) -> Option<&HashMap<String, (Span, VariantPayload)>> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variants, .. }) => Some(variants),
            _ => None,
        }
    }

    fn enum_variant_order(&self, name: &str) -> Option<&[String]> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variant_order, .. }) => Some(variant_order.as_slice()),
            _ => None,
        }
    }

    /// D-TAG1: the enum's variant groups (group path → span + leaf paths).
    fn enum_groups(&self, name: &str) -> Option<&HashMap<String, (Span, Vec<String>)>> {
        match self.types.get(name) {
            Some(TypeDef::Enum { groups, .. }) => Some(groups),
            _ => None,
        }
    }

    fn method(&self, type_name: &str, method: &str) -> Option<&MethodSig> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { methods, .. }) | Some(TypeDef::Enum { methods, .. }) => {
                methods.get(method)
            }
            _ => None,
        }
    }

    fn field_names(&self, type_name: &str) -> Vec<String> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { fields, .. }) => {
                fields.iter().map(|(n, ..)| n.clone()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// D-DIST1: true when `name` is a registered distinct type.
    pub(crate) fn is_distinct(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Distinct { .. }))
    }

    /// D-TYPEALIAS1: true when `name` is a registered transparent type alias.
    pub(crate) fn is_type_alias(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Alias { .. }))
    }

    pub(crate) fn type_alias(&self, name: &str) -> Option<(&[crate::AST::TypeParam], &Type)> {
        match self.types.get(name) {
            Some(TypeDef::Alias { params, target, .. }) => Some((params.as_slice(), target)),
            _ => None,
        }
    }

    /// D-LIN1 (ratified 2026-06-21): true when `name` is a `#SingleUse` struct/enum.
    /// Values of such a type must be consumed exactly once and may not be aliased.
    pub(crate) fn is_single_use(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct {
                single_use: true,
                ..
            }) | Some(TypeDef::Enum {
                single_use: true,
                ..
            })
        )
    }

    /// D-MUSTUSE1 (c18iwxqx): true when `name` is a `#MustUse` struct/enum.
    pub(crate) fn is_must_use(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct { must_use: true, .. })
                | Some(TypeDef::Enum { must_use: true, .. })
        )
    }

    /// D-DIST1: the base type of a distinct type (None if `name` is not distinct).
    pub(crate) fn distinct_base(&self, name: &str) -> Option<&Type> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { base, .. }) => Some(base),
            _ => None,
        }
    }

    /// D-DIST3: true when the distinct type has `#Numeric`.
    pub(crate) fn distinct_is_numeric(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_numeric: true,
                ..
            })
        )
    }

    /// D-RANGETYPE1: the declared `(lo, hi)` inclusive bounds of a range type
    /// (`distinct Int(0..10)`), or `None` for a plain distinct type / a name
    /// that isn't distinct.
    pub(crate) fn distinct_range(&self, name: &str) -> Option<(i64, i64)> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { range, .. }) => *range,
            _ => None,
        }
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#Comparable`.
    pub(crate) fn distinct_is_comparable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_comparable: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#Printable`.
    pub(crate) fn distinct_is_printable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_printable: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#CodableAsBase`.
    pub(crate) fn distinct_is_codable_as_base(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                is_codable_as_base: true,
                ..
            })
        )
    }

    /// D-CAPBUNDLE1: the names of every capability bundle granted to distinct
    /// type `name`, in fixed order — used to compose the E0138 "has" clause.
    pub(crate) fn distinct_granted_bundles(&self, name: &str) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.distinct_is_numeric(name) {
            out.push("#Numeric");
        }
        if self.distinct_is_comparable(name) {
            out.push("#Comparable");
        }
        if self.distinct_is_printable(name) {
            out.push("#Printable");
        }
        if self.distinct_is_codable_as_base(name) {
            out.push("#CodableAsBase");
        }
        out
    }
}

fn func_to_method_sig(f: &Func) -> MethodSig {
    let self_param = f.self_param();
    // param_info and defaults exclude `self` — they parallel the args a
    // caller provides (no `self` in the call-site arg list).
    let non_self_params = f.params.iter().filter(|p| p.name != "self");
    let return_view_provenance = crate::AST::ViewProvenanceCell::new();
    if let Some(provenance) = &f.return_view_provenance {
        let _ = return_view_provenance.set(provenance.clone());
    }
    MethodSig {
        name_span: f.name_span,
        is_pub: f.is_pub,
        params: f
            .params
            .iter()
            .map(|p| {
                let ty = if p.variadic {
                    Type::List(Box::new(p.ty.clone()))
                } else {
                    p.ty.clone()
                };
                (p.convention, ty)
            })
            .collect(),
        return_type: f.return_type.clone(),
        type_params: f.type_params.clone(),
        is_static: self_param.is_none(),
        self_conv: self_param.map(|p| p.convention),
        param_info: non_self_params
            .clone()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        param_call: non_self_params
            .clone()
            .map(|p| (p.call_label().to_string(), p.zone))
            .collect(),
        param_variadic: non_self_params.clone().map(|p| p.variadic).collect(),
        defaults: non_self_params
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
        must_use: f.is_must_use,
        return_view_provenance,
    }
}

fn func_to_sig(f: &Func) -> FuncSig {
    let param_variadic: Vec<bool> = f.params.iter().map(|p| p.variadic).collect();
    let return_view_provenance = crate::AST::ViewProvenanceCell::new();
    if let Some(provenance) = &f.return_view_provenance {
        let _ = return_view_provenance.set(provenance.clone());
    }
    FuncSig {
        params: f
            .params
            .iter()
            .map(|p| {
                let ty = if p.variadic {
                    Type::List(Box::new(p.ty.clone()))
                } else {
                    p.ty.clone()
                };
                (p.convention, ty)
            })
            .collect(),
        root_param: f.params.first().is_some_and(|p| p.root),
        param_info: f
            .params
            .iter()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        param_call: f
            .params
            .iter()
            .map(|p| (p.call_label().to_string(), p.zone))
            .collect(),
        defaults: f
            .params
            .iter()
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
        param_variadic,
        variadic_bounds: f.params.last().and_then(|p| p.variadic_bound_list.clone()),
        return_type: f.return_type.clone(),
        return_view_provenance,
        is_extern: false,
        is_c_abi: false,
        c_abi_name: None,
        foreign_effect_root: None,
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        is_foreign_thread_safe: foreign_thread_safe_func(f),
        is_sanitizer: f.is_sanitizer,
        is_must_use: f.is_must_use,
        param_view_from_names: f
            .params
            .iter()
            .map(|p| p.declared_view_from_names.clone())
            .collect(),
    }
}

fn extern_to_sig(ef: &ExternFn, is_c_abi: bool) -> FuncSig {
    FuncSig {
        params: ef
            .params
            .iter()
            .map(|p| {
                let ty = if p.variadic {
                    Type::List(Box::new(p.ty.clone()))
                } else {
                    p.ty.clone()
                };
                (p.convention, ty)
            })
            .collect(),
        // D-CALLDUAL1=E: foreign functions use the same first bare-read
        // receiver contract as ordinary top-level functions. Keep the marker
        // in the signature so dot-call lookup cannot silently discard it.
        root_param: ef.params.first().is_some_and(|p| p.root),
        param_info: ef.params.iter().map(|p| (p.name.clone(), false)).collect(),
        param_call: ef
            .params
            .iter()
            .map(|p| (p.call_label().to_string(), p.zone))
            .collect(),
        defaults: ef.params.iter().map(|_| None).collect(),
        param_variadic: ef.params.iter().map(|p| p.variadic).collect(),
        variadic_bounds: ef.params.last().and_then(|p| p.variadic_bound_list.clone()),
        return_type: ef.return_type.clone(),
        return_view_provenance: crate::AST::ViewProvenanceCell::new(),
        is_extern: true,
        is_c_abi,
        c_abi_name: ef.abi.as_ref().map(|(name, _)| name.clone()),
        foreign_effect_root: ef.effect_root.clone(),
        // D-CABI-RESULT1=C: any raw out-pointer declaration is callable only
        // from an audited `#Unsafe` region. The declaration remains the exact
        // C status/out shape; no Result adapter is invented.
        is_unsafe: is_c_abi
            && ef.params.iter().any(|p| {
                matches!(&p.ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_PTR)
            }),
        is_pure: false,      // extern functions are always considered impure
        is_foreign_thread_safe: false,
        is_sanitizer: false, // extern functions can't be sanitizers
        is_must_use: false,
        param_view_from_names: ef.params.iter().map(|_| None).collect(),
    }
}

fn foreign_thread_safe_expr(e: &Expr) -> bool {
        match e {
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Char(..) | Expr::Ident(..) => true,
            Expr::Unary(_, a, _) | Expr::Copy(a, _) => foreign_thread_safe_expr(a),
            Expr::Binary(_, a, b, _) => foreign_thread_safe_expr(a) && foreign_thread_safe_expr(b),
            Expr::CompareChain { operands, .. } => operands.iter().all(foreign_thread_safe_expr),
            _ => false,
        }
}

fn foreign_thread_safe_stmt(s: &Stmt) -> bool {
        match s {
            Stmt::Return(v, _) => v.as_ref().is_none_or(foreign_thread_safe_expr),
            Stmt::Expr(e) => foreign_thread_safe_expr(e),
            Stmt::Val(b) => foreign_thread_safe_expr(&b.init),
            _ => false,
        }
}

fn foreign_thread_safe_func(f: &Func) -> bool {
    f.is_pure && f.type_params.is_empty() && f.pre.is_empty() && f.post.is_empty()
        && f.body.iter().all(foreign_thread_safe_stmt)
}

pub(crate) fn foreign_thread_safe_lambda(lam: &crate::AST::Lambda) -> bool {
    lam.take_names.is_empty()
        && lam.meta.mut_captures.is_empty()
        && lam.meta.cloned_captures.is_empty()
        && match &lam.body {
            crate::AST::LambdaBody::Expr(e) => foreign_thread_safe_expr(e),
            crate::AST::LambdaBody::Block(stmts) => stmts.iter().all(foreign_thread_safe_stmt),
        }
}

/// D-NARG-D2: Walk a default expression and substitute any `Ident` that names
/// an earlier parameter with the corresponding supplied argument expression.
/// `param_names` is the slice of earlier param names (index-aligned with `args`).
/// Returns the rewritten expression.
pub(crate) fn substitute_param_refs(
    expr: crate::AST::Expr,
    param_names: &[String],
    args: &[crate::AST::CallArg],
) -> crate::AST::Expr {
    use crate::AST::Expr;
    match expr {
        Expr::Ident(ref name, _) => {
            if let Some(idx) = param_names.iter().position(|n| n == name) {
                if let Some(arg) = args.get(idx) {
                    return arg.expr.clone();
                }
            }
            expr
        }
        Expr::Unary(op, inner, span) => Expr::Unary(
            op,
            Box::new(substitute_param_refs(*inner, param_names, args)),
            span,
        ),
        Expr::Binary(op, lhs, rhs, span) => Expr::Binary(
            op,
            Box::new(substitute_param_refs(*lhs, param_names, args)),
            Box::new(substitute_param_refs(*rhs, param_names, args)),
            span,
        ),
        Expr::Field(base, field, span) => Expr::Field(
            Box::new(substitute_param_refs(*base, param_names, args)),
            field,
            span,
        ),
        // All other expression forms don't mention parameter names directly
        // (calls, literals, etc.) — leave them as-is. A complex default
        // expression containing a nested call with a param ident would need
        // recursive handling, but the parser only allows simple expressions in
        // defaults (literals, idents, field access, arithmetic).
        other => other,
    }
}

/// D-NARG-D2: Check whether a default expression references any parameter
/// that appears *after* the current parameter index. Collects the names of
/// any forward-referenced parameters found. `all_param_names` is the full
/// list of non-self parameter names for this function (in order).
/// `default_param_idx` is the index of the parameter whose default we're checking.
pub(crate) fn find_forward_refs(
    expr: &crate::AST::Expr,
    all_param_names: &[String],
    default_param_idx: usize,
) -> Vec<(String, crate::Diagnostics::Span)> {
    let mut found = Vec::new();
    find_forward_refs_inner(expr, all_param_names, default_param_idx, &mut found);
    found
}

fn find_forward_refs_inner(
    expr: &crate::AST::Expr,
    all_param_names: &[String],
    default_param_idx: usize,
    found: &mut Vec<(String, crate::Diagnostics::Span)>,
) {
    use crate::AST::Expr;
    match expr {
        Expr::Ident(name, span) => {
            // A forward ref: name is in param list at an index >= default_param_idx
            if let Some(idx) = all_param_names.iter().position(|n| n == name) {
                if idx >= default_param_idx {
                    found.push((name.clone(), *span));
                }
            }
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. } => {
            find_forward_refs_inner(inner, all_param_names, default_param_idx, found);
        }
        Expr::Binary(_, lhs, rhs, _) => {
            find_forward_refs_inner(lhs, all_param_names, default_param_idx, found);
            find_forward_refs_inner(rhs, all_param_names, default_param_idx, found);
        }
        Expr::Field(base, _, _) => {
            find_forward_refs_inner(base, all_param_names, default_param_idx, found);
        }
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    def_span: Span,
    ty: Type,
    mutable: bool,
    /// Set when the name is a parameter (with its access convention).
    param_conv: Option<AccessConvention>,
    /// Loop nesting depth where the name was declared (for move-in-loop).
    decl_loop_depth: usize,
    /// Whether this local can cross a task/channel boundary. For ordinary
    /// values this follows the type; for lambdas it also includes captures.
    sendable: bool,
    /// D-DATARACE1=C: `#Local` pin on a reactive binding.
    reactive_local: bool,
    /// D-DATARACE1=C: `#Shared` pin on a reactive binding.
    reactive_shared: bool,
    /// Binding span for a Task value that must be consumed with `.join()`.
    task_lint_span: Option<Span>,
    /// D-LIN1 (ratified 2026-06-21): set (to the binding name's span) when this
    /// local owns a `#SingleUse` value that must be consumed exactly once. `None`
    /// for ordinary values, for parameters (the caller owns the consume duty), and
    /// for `view`/`&` borrows (which never own). When still in scope and not in
    /// `moved` at scope end, E0140 fires.
    single_use_span: Option<Span>,
    /// Pure compile-time value for immutable-local diagnostics and folding.
    /// This does not make an ordinary local available to `comptime` code.
    constant_value: Option<crate::Comptime::CtValue>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopValueKind {
    Effect,
    Collecting,
    Result,
}

struct LoopValueFrame {
    label: Option<String>,
    kind: LoopValueKind,
    ty: Option<Type>,
}

/// D-MEM1 S9 / #649: one source-level fact graph for every borrowed window.
/// Representation-specific flags may still guide lowering, but soundness uses
/// only these sema facts and never asks rustc to discover an invalid alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewKind {
    Arena,
    FixedBacking,
    List,
    String,
    Buffer,
    Matrix,
    /// D-PIN1=A: `mem.pin(&place)`. Same owner graph as every other window —
    /// the only difference is the product diagnostic, which talks about the
    /// address-stability promise instead of an aliasing view.
    Pin,
}

impl ViewKind {
    fn is_named_window(self) -> bool {
        matches!(self, Self::List | Self::Buffer | Self::Matrix)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewAccess {
    Read,
    Write,
}

#[derive(Debug, Clone)]
pub(crate) struct CallPlaceAccess {
    place: ViewPlace,
    access: ViewAccess,
    /// Rust two-phase mutable receiver borrow: reads may occur while reserved,
    /// but another write/reservation may not.
    reserved: bool,
}

#[derive(Debug, Default)]
pub(crate) struct CallAccessFrame {
    accesses: Vec<CallPlaceAccess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewOwnerOrigin {
    Local,
    Receiver,
    Parameter(usize),
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ViewOwnerId {
    name: String,
    def_span: Span,
    origin: ViewOwnerOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ViewProjection {
    Field(String),
    Index { value: Option<i64>, span: Span },
    Range {
        start: Option<i64>,
        end: Option<i64>,
        span: Span,
    },
    /// Arena allocation sites create disjoint fresh storage by definition.
    Fresh(Span),
}

#[derive(Debug, Clone)]
pub(crate) struct ViewPlace {
    owner: ViewOwnerId,
    projections: Vec<ViewProjection>,
}

#[derive(Debug, Clone)]
pub(crate) struct ViewFact {
    /// Definition identity of the view binding itself. Prevents a non-view
    /// shadow with the same spelling from exposing an outer fact.
    binding_span: Span,
    /// Returned aggregate slot this fact describes; empty for a direct view.
    output_path: Vec<String>,
    place: ViewPlace,
    kind: ViewKind,
    access: ViewAccess,
    /// `scopes.len()` at declaration; facts disappear with their binding.
    scope_len: usize,
    /// Owner operation that invalidated storage, when invalidation is allowed
    /// to happen (arena reset). Ordinary owner writes/moves are rejected.
    invalidated: Option<(String, Span)>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ViewFactGraph {
    /// Ordered by declaration. Reverse lookup preserves shadowing; scope pop
    /// removes only facts declared at that depth and reveals outer facts again.
    bindings: Vec<(String, ViewFact)>,
}

impl ViewFactGraph {
    fn current(&self, name: &str) -> Option<&ViewFact> {
        self.bindings
            .iter()
            .rev()
            .find_map(|(binding, fact)| (binding == name).then_some(fact))
    }

    fn current_for_binding(&self, name: &str, binding_span: Span) -> Option<&ViewFact> {
        self.current(name)
            .filter(|fact| fact.binding_span == binding_span)
    }

    fn all_for_binding(&self, name: &str, binding_span: Span) -> Vec<&ViewFact> {
        self.bindings
            .iter()
            .filter_map(|(binding, fact)| {
                (binding == name && fact.binding_span == binding_span).then_some(fact)
            })
            .collect()
    }

    fn push(&mut self, name: String, fact: ViewFact) {
        self.bindings.push((name, fact));
    }

    fn leave_scope(&mut self, depth: usize) {
        self.bindings.retain(|(_, fact)| fact.scope_len < depth);
    }

    fn invalidate_owner(&mut self, owner: &ViewOwnerId, verb: &str, span: Span) {
        for (_, fact) in &mut self.bindings {
            if &fact.place.owner == owner && fact.invalidated.is_none() {
                fact.invalidated = Some((verb.to_string(), span));
            }
        }
    }
}

impl ViewPlace {
    /// Conservative source-place overlap. Different fields and distinct fresh
    /// arena allocations are disjoint; dynamic indexes and ranges overlap.
    fn overlaps(&self, other: &ViewPlace) -> bool {
        if self.owner != other.owner {
            return false;
        }
        for (left, right) in self.projections.iter().zip(&other.projections) {
            match (left, right) {
                (ViewProjection::Field(a), ViewProjection::Field(b)) if a != b => return false,
                (ViewProjection::Index { value: Some(a), .. }, ViewProjection::Index { value: Some(b), .. }) if a != b => return false,
                (
                    ViewProjection::Index { value: Some(index), .. },
                    ViewProjection::Range { start: Some(start), end: Some(end), .. },
                )
                | (
                    ViewProjection::Range { start: Some(start), end: Some(end), .. },
                    ViewProjection::Index { value: Some(index), .. },
                ) if index < start || index > end => return false,
                (
                    ViewProjection::Range { start: Some(a_start), end: Some(a_end), .. },
                    ViewProjection::Range { start: Some(b_start), end: Some(b_end), .. },
                ) if a_end < b_start || b_end < a_start => return false,
                (ViewProjection::Fresh(a), ViewProjection::Fresh(b)) if a != b => return false,
                _ => {}
            }
        }
        true
    }

    /// D-PIN2=A / D-PIN3=A: `self` is `other` or a deeper projection of it —
    /// the shape of a structural pin projection (`pinned.next` under `pinned`).
    fn extends(&self, other: &ViewPlace) -> bool {
        self.owner == other.owner
            && self.projections.len() >= other.projections.len()
            && self.overlaps(other)
    }
}

#[cfg(test)]
mod view_fact_graph_tests {
    use super::*;

    fn fact(owner_span: usize, scope_len: usize, kind: ViewKind) -> ViewFact {
        ViewFact {
            binding_span: Span::new(owner_span + 10, owner_span + 11),
            output_path: Vec::new(),
            place: ViewPlace {
                owner: ViewOwnerId {
                    name: "owner".to_string(),
                    def_span: Span::new(owner_span, owner_span + 1),
                    origin: ViewOwnerOrigin::Local,
                },
                projections: Vec::new(),
            },
            kind,
            access: ViewAccess::Read,
            scope_len,
            invalidated: None,
        }
    }

    #[test]
    fn nested_shadow_reveals_outer_fact_after_scope_exit() {
        let mut graph = ViewFactGraph::default();
        graph.push("window".to_string(), fact(1, 1, ViewKind::List));
        graph.push("window".to_string(), fact(2, 2, ViewKind::String));
        assert_eq!(graph.current("window").map(|f| f.kind), Some(ViewKind::String));
        graph.leave_scope(2);
        assert_eq!(graph.current("window").map(|f| f.kind), Some(ViewKind::List));
    }

    #[test]
    fn non_view_binding_identity_hides_stale_same_spelling_fact() {
        let mut graph = ViewFactGraph::default();
        graph.push("window".to_string(), fact(1, 1, ViewKind::List));
        assert!(graph
            .current_for_binding("window", Span::new(99, 100))
            .is_none());
    }

    #[test]
    fn stable_owner_identity_distinguishes_same_spelling() {
        let a = fact(1, 1, ViewKind::Buffer).place;
        let b = fact(2, 1, ViewKind::Matrix).place;
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn invalidation_uses_owner_identity_not_spelling() {
        let mut graph = ViewFactGraph::default();
        let outer = fact(1, 1, ViewKind::Arena);
        let inner = fact(2, 2, ViewKind::Arena);
        let inner_owner = inner.place.owner.clone();
        graph.push("outer".to_string(), outer);
        graph.push("inner".to_string(), inner);

        graph.invalidate_owner(&inner_owner, "reset", Span::new(40, 41));

        assert!(graph.current("outer").unwrap().invalidated.is_none());
        assert!(graph.current("inner").unwrap().invalidated.is_some());
    }

    #[test]
    fn different_fields_do_not_alias() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Field("left".to_string()));
        b.projections.push(ViewProjection::Field("right".to_string()));
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn dynamic_indexes_are_conservatively_overlapping() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Index { value: None, span: Span::new(20, 21) });
        b.projections.push(ViewProjection::Index { value: None, span: Span::new(30, 31) });
        assert!(a.overlaps(&b));
    }

    #[test]
    fn distinct_constant_indexes_are_disjoint() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Index {
            value: Some(0),
            span: Span::new(20, 21),
        });
        b.projections.push(ViewProjection::Index {
            value: Some(1),
            span: Span::new(30, 31),
        });
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn constant_index_outside_range_is_disjoint() {
        let mut index = fact(1, 1, ViewKind::List).place;
        let mut range = index.clone();
        index.projections.push(ViewProjection::Index {
            value: Some(0),
            span: Span::new(20, 21),
        });
        range.projections.push(ViewProjection::Range {
            start: Some(1),
            end: Some(3),
            span: Span::new(30, 34),
        });
        assert!(!index.overlaps(&range));
    }

    #[test]
    fn separated_constant_ranges_are_disjoint() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Range {
            start: Some(0),
            end: Some(1),
            span: Span::new(20, 24),
        });
        b.projections.push(ViewProjection::Range {
            start: Some(2),
            end: Some(3),
            span: Span::new(30, 34),
        });
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn inclusive_ranges_sharing_boundary_overlap() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Range {
            start: Some(0),
            end: Some(1),
            span: Span::new(20, 24),
        });
        b.projections.push(ViewProjection::Range {
            start: Some(1),
            end: Some(2),
            span: Span::new(30, 34),
        });
        assert!(a.overlaps(&b));
    }

    #[test]
    fn fresh_arena_allocations_are_disjoint() {
        let mut a = fact(1, 1, ViewKind::Arena).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Fresh(Span::new(20, 21)));
        b.projections.push(ViewProjection::Fresh(Span::new(30, 31)));
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn buffer_and_matrix_use_named_window_boundary() {
        assert!(ViewKind::Buffer.is_named_window());
        assert!(ViewKind::Matrix.is_named_window());
        assert!(!ViewKind::Arena.is_named_window());
        assert!(!ViewKind::FixedBacking.is_named_window());
        assert!(!ViewKind::String.is_named_window());
    }
}

#[derive(Debug, Clone)]
pub(crate) enum SendProblemKind {
    ClosureNeedsTake,
    ClosureCaptures,
    TraitValue(String),
    ThreadConfined(String),
    ViewBorrow,
}

#[derive(Debug, Clone)]
pub(crate) struct SendabilityProblem {
    root: Option<String>,
    path: Vec<String>,
    kind: SendProblemKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SendCrossing {
    TaskCapture,
    TaskResult,
    ChannelSend,
}

/// What the driver is compiling — affects `run` / test requirements (M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    /// `jet run` / `jet build` — needs `run`, ignores test blocks in codegen.
    Run,
    /// `jet test` — needs at least one test; `run` is optional.
    Test,
    /// `jet bench` (D-BENCH1) — type-check `#Bench` block bodies and emit the
    /// timing harness; `run` is optional, like `Test`.
    Bench,
    /// `jet check` / LSP — type-check only; imported modules and library files
    /// need not define `run`.
    Check,
    /// `jet eval` — full sema type-checking, but `run` may return a non-`()`
    /// type (E0122 is relaxed). The entry still requires a `run` function.
    Eval,
}

pub(crate) struct ModuleState {
    module_path: String,
    module_alias: String,
    /// True only for the explicitly selected package/workspace build entry.
    /// Ordinary `fn build` names do not grant compiler-host capabilities.
    allow_compiler_api: bool,
    func_spans: HashMap<String, Span>,
    const_spans: HashMap<String, Span>,
    import_spans: HashMap<String, Span>,
    /// D-PUBPKG1=A: modules under the same project package/workspace root may
    /// see `pub(package)` items. Dependency/hangar modules get their own root.
    package_scope: PathBuf,
    funcs: HashMap<String, FuncSig>,
    func_pub: HashMap<String, bool>,
    func_pkg_pub: HashMap<String, bool>,
    type_pub: HashMap<String, bool>,
    type_pkg_pub: HashMap<String, bool>,
    method_pub: HashMap<(String, String), bool>,
    method_pkg_pub: HashMap<(String, String), bool>,
    field_pub: HashMap<(String, String), bool>,
    field_pkg_pub: HashMap<(String, String), bool>,
    registry: TypeRegistry,
    consts: HashMap<String, Type>,
    imports: HashMap<String, usize>,
    core_imports: HashMap<String, String>,
    tests: HashMap<String, Span>,
    trait_reg: TraitRegistry,
    /// D-STATE-DECL: declared typestate labels by owning type.
    declared_states: HashMap<String, Vec<String>>,
    policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    /// D-MOD2: inline code module aliases present in this file (alias → module name).
    /// `math.double(x)` resolves to `user_math__double(x)` when `math` is in here.
    code_modules: HashMap<String, String>,
    /// Inline module spelling -> compiler semantic identity. Ordinary modules
    /// use their module identity; generic instances use `instance:<digest>`.
    code_module_identities: HashMap<String, String>,
    /// D-MOD3: unqualified items imported via `use alias.Item` (inline modules).
    /// Maps unqualified name → mangled name (e.g. "clamp" → "math__clamp").
    unqualified: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items imported via `use alias.Item`.
    /// Maps name → (function_name, module_idx).
    unqualified_file: HashMap<String, (String, usize)>,
    /// D-MOD4: `pub use alias.Item` re-exports — items this module exposes on its
    /// own public surface even though they're defined elsewhere. Maps the
    /// exported name → (target_function_name, target_module_idx). A caller doing
    /// `thismod.Item` resolves through here to the real definition.
    reexports: HashMap<String, (String, usize)>,
}

pub(crate) struct Checker<'a> {
    funcs: &'a HashMap<String, FuncSig>,
    registry: &'a TypeRegistry,
    effect_facts: &'a jet_foundation::Facts::FactRegistry,
    consts: &'a HashMap<String, Type>,
    modules: Option<&'a [ModuleState]>,
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    core_imports: &'a HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    code_modules: &'a HashMap<String, String>,
    code_module_identities: &'a HashMap<String, String>,
    /// D-MOD3: unqualified inline-module items in scope (name → mangled name).
    unqualified: &'a HashMap<String, String>,
    /// D-MOD3: unqualified file-module items in scope (name → (fn_name, module_idx)).
    unqualified_file: &'a HashMap<String, (String, usize)>,
    /// D-MOD2: pub flags for this module's functions, including inline-module
    /// items mangled as `M__item`. Used to reject `M.private()` from outside.
    func_pub: &'a HashMap<String, bool>,
    /// D-PUBPKG1=A: package-scoped function visibility flags.
    func_pkg_pub: &'a HashMap<String, bool>,
    module_path: &'a str,
    policy_declarations: &'a [crate::Policy::PolicyDeclaration],
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    current_function_span: Span,
    reference_anchors: &'a mut HashMap<(String, usize, usize), Effects::DefinitionAnchorFact>,
    diags: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    concrete_unit_values: Vec<HashMap<String, f64>>,
    /// name -> span of the use that gave the value away.
    moved: HashMap<String, Span>,
    /// Field inference reads its base only to discover its type. Suppress a
    /// partial-root move report there; the complete field place is checked first.
    suppress_partial_move_root_read: bool,
    loop_depth: usize,
    source_nesting: usize,
    /// D-LOOPLABEL3=A: stack of `name :: loop` names; scope, innermost last.
    loop_labels: Vec<String>,
    /// D-LOOPEVAL1: item type under each compiler-private collecting loop.
    /// Nested yielding loops push independent slots.
    collect_item_types: Vec<Option<Type>>,
    loop_value_frames: Vec<LoopValueFrame>,
    pending_loop_value: Option<(LoopValueKind, Option<String>)>,
    last_loop_result_type: Option<Type>,
    /// D-EFF1: effects this function body reaches directly (Core calls, impure
    /// builtins). Accumulated during the walk; rolled into the per-function
    /// `EffectSummary` after the body is checked.
    fx_direct: EffectSet,
    /// First source span that introduced each direct effect, for inspect/LSP
    /// provenance. Duplicate uses keep the earliest stable witness.
    fx_direct_spans: HashMap<String, Span>,
    /// D-EFF1: user functions called in this body (call-graph edges for the
    /// whole-program transitive fixpoint).
    fx_edges: BTreeSet<String>,
    /// D-EFF1: a foreign (`extern`) call was reached — the body's effects are
    /// the maximal set (an un-inspectable body may do anything).
    fx_maximal: bool,
    /// First source span that forced the row maximal.
    fx_maximal_span: Option<Span>,
    /// D-EFF1: stack of active `#Caps(…)` regions, innermost last. Every effect
    /// or edge recorded while one is open is also added to it (and all enclosing
    /// regions) so the region's own effect set can be checked against its caps.
    region_stack: Vec<RegionAccum>,
    /// D-EFF1: completed `#Caps(…)` regions in this body, rolled into the
    /// `EffectSummary` for the post-pass E0741 check.
    fx_regions: Vec<RegionSummary>,
    /// D-EFF2: callback-bound obligations recorded at higher-order call sites
    /// where the function-typed parameter carries a `#Pure`/`#(…)` bound. Rolled
    /// into the `EffectSummary` for the post-pass E0747 check.
    fx_callback_obligations: Vec<CallbackObligation>,
    /// D-CRYPTO-DIAG1: compiler-known crypto facts wait for the function's
    /// syntax/type/effect phases to finish before becoming diagnostics.
    fx_pending_diagnostics: Vec<Diagnostic>,
    /// D-MEM-FACTS1 direct, source-spanned memory evidence accumulated beside
    /// effects so both policies share one pre-TIR call graph.
    fx_memory_events: Vec<MemoryFacts::MemoryEvent>,
    fx_memory_open: Vec<MemoryFacts::OpenMemoryDispatch>,
    memory_policy_stack: Vec<MemoryFacts::MemoryPolicyRegion>,
    fx_memory_regions: Vec<MemoryFacts::MemoryPolicyRegion>,
    fx_memory_unbounded_control: Vec<Span>,
    fx_memory_calls: Vec<MemoryFacts::MemoryCall>,
    memory_control_multiplier: Option<u64>,
    /// D-TXN2: nesting depth of `#Transact(name) { … }` blocks whose body is
    /// being checked **directly** (not inside a deferred lambda). While `> 0`, an
    /// irreversible Core effect (Net/FS/Exec) reached directly in the block is
    /// E0746 at the call site — the fix is to move it after the block or register
    /// it via `name.on_commit(…)`. Zeroed and restored around every lambda body
    /// (effects inside an `on_commit`/other lambda are deferred, not rejected).
    txn_depth: usize,
    /// D-DET1: nesting depth of `assume_deterministic { … }` blocks currently
    /// being checked. While `> 0`, the determinism rejections inside a `#Pure fn`
    /// (E3403 non-deterministic Core call, E3401 impure Core call) are suspended —
    /// the expert "I know this is deterministic" escape. A semantic footgun
    /// (v1-legal per the card); does not relax memory/type safety, only the
    /// determinism check. Zeroed/restored around lambda bodies like `txn_depth`.
    det_suppress: usize,
    /// D-CTX1 / c26: nesting depth of `#Context { … }` blocks (for L0506).
    context_depth: usize,
    /// True while inside a `#Context` block that set an `allocator` field.
    context_allocator_active: bool,
    in_unsafe: bool,
    /// D-IGNORERET2=A: true while inside a `#Suppress(MustUse) { … }` block.
    /// Suppresses E0402 / E0419 for fallible / `#MustUse` results dropped as statements.
    suppress_must_use: bool,
    /// True while checking a `pure fn` body, so E3403 can fire on a
    /// non-deterministic std call (time/random) reached from pure code.
    in_pure: bool,
    /// D-PRELUDEX1=A: true when the enclosing file declared `#NoPrelude`.
    /// Disables ambient `print`/`input` resolution for this body.
    no_prelude: bool,
    /// D-PREPOST1: true while type-checking a `#Pre` clause's condition —
    /// `result` isn't bound yet at function entry, so a reference to it here
    /// is E0144 instead of the normal "undefined name" error.
    in_pre_clause: bool,
    /// True while inferring a comptime binding's RHS or inside a comptime
    /// context — suppresses E2712 for `$name` comptime splice expressions.
    in_comptime: bool,
    /// True only while checking the selected package/workspace `fn build`.
    /// This is passed by the Driver's build authority, never inferred from a
    /// user-chosen function name.
    compiler_api_allowed: bool,
    ret: Option<Type>,
    fn_name: String,
    /// Canonical caller-visible parameter order; excludes `self`.
    current_param_names: Vec<String>,
    /// Context type for bare `null` (E0308).
    expected_type: Option<Type>,
    /// Collections currently read by an active `for x in xs` loop (E0507).
    iter_borrowed: HashSet<String>,
    /// Card #1440: NoElse-terminated dispatch chains already coverage-checked
    /// (chain span starts) — every level shares one span, the outermost wins.
    noelse_chains_checked: HashSet<usize>,
    /// Loop bindings that lend one `ViewMut<T>` element from a collection.
    /// These values may edit during the iteration but may not be retained.
    lending_view_loop_vars: HashSet<String>,
    /// D-MEM1 S9 / #649: sole provenance/alias state for arena, list, string,
    /// buffer, matrix, and future named mutable views.
    view_facts: ViewFactGraph,
    /// D-MEM-VIEWRET1=B: one canonical public source inferred from every
    /// successful named-view return in this function.
    return_view_provenance: Option<crate::AST::ViewProvenanceMap>,
    /// View names read in the statement currently being checked. Together
    /// with the existing statement-tail analysis, this makes local window
    /// conflicts end at last use instead of lexical scope end.
    views_used_in_stmt: HashSet<String>,
    /// One scoped-loan read report per statement. Nested reads of the same
    /// place (`ps[0].position` is a Field over an Index) would otherwise each
    /// report the one mistake.
    scoped_loan_read_reported: bool,
    /// #1196: argument and receiver loans that remain active until their call
    /// finishes. Nested calls see every outer frame.
    call_access_frames: Vec<CallAccessFrame>,
    /// D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL2:
    /// `Type.{ uninit }` bindings not yet definitely written — maps name → the
    /// decl span. A read while still in this map is E0420 (write-before-read
    /// proof); a write clears it. Branch-merged in `check_if` (intersection of
    /// "initialized").
    uninit: HashMap<String, UninitState>,
    /// True while inferring an expression that the generated Rust will only
    /// borrow (method receivers, field/index bases, lvalues). Field reads in
    /// borrow position must NOT be rewritten to `.clone()`.
    borrow_ctx: bool,
    /// `Fixed.new` / `Fixed.over` must be the whole initializer of one lexical
    /// binding so codegen can place and lifetime-order its inline backing.
    allow_fixed_constructor: bool,
    /// D-MEM1 stage S5: true only while inferring a string-view fact in one of
    /// the TWO positions its bare `&str` Rust place
    /// actually supports — the receiver of a chained `.trim()`/`.after()`/
    /// `.before()`, or the operand of `copy`. The general `Expr::Ident` arm
    /// reports E2307 whenever it reads a string-view name and this is false —
    /// a single, general choke point instead of hunting down every possible
    /// consuming context (list/tuple literal element, call argument, plain
    /// assignment, …) one at a time.
    allow_string_view_read: bool,
    /// M8: when false, a lambda is consumed inline (collection methods / borrow).
    lambda_escapes: bool,
    /// True only while checking a lambda body. Distinguishes an escaping lambda
    /// from the checker's ordinary top-level default escape policy.
    in_lambda_body: bool,
    /// Builtin mutating receivers discovered while inferring the current
    /// lambda body. The builtin table supplies the mutation fact; inline
    /// callbacks fold these roots into `LambdaMeta::mut_captures`.
    inferred_lambda_mut_captures: HashSet<String>,
    /// `edit_disjoint` lends each callback parameter for that invocation only.
    /// Any store, return, or retaining call from the callback is E0212.
    lambda_params_are_lending_views: bool,
    /// M11: when true, lambda is being passed to tasks.spawn — stricter capture rules (E1101).
    is_task_spawn: bool,
    /// D-MEM1 S6 (D-SHARED-API1=A): true only while binding `Shared<T>.edit(f)`'s
    /// closure parameter — grants it write access with no `&` sigil (the API
    /// contract IS the exclusive lock; `check_lambda` reads this once, at bind
    /// time, then it's irrelevant for the rest of the closure body).
    lambda_param_mutable: bool,
    /// `ExpiringSecret.with` lends its callback parameter without transferring
    /// ownership. The ordinary parameter convention machinery then rejects
    /// moves and captures before codegen.
    lambda_param_is_secret_loan: bool,
    /// D-DETACH1: task names whose spawn lambda had a non-view sendability error (E1102 fired).
    /// At `.detach()`, if the task is in this set and NOT in view_borrow_escape_tasks, E1103 fires.
    view_capture_tasks: HashSet<String>,
    /// D-DATARACE1=C: human-readable upgrade lines for `jet report` / CompileOutput.
    reactive_upgrades: Vec<String>,
    /// D-DATARACE1=C: binding names that crossed a concurrency boundary this function.
    reactive_upgrade_names: HashSet<String>,
    /// D-DETACH1: task names whose spawn lambda captured a `view` borrow specifically (E1102/ViewBorrow).
    /// At `.detach()`, if the task is in this set, E1106 fires instead of E1103.
    view_borrow_escape_tasks: HashSet<String>,
    /// D-DETACH1: the binding name currently being elaborated (set at check_binding
    /// entry, cleared after). Used to record view-capturing task names.
    current_binding_name: Option<String>,
    /// M8: binding name when checking `f :: (…) => …` (E0804 self-call).
    lambda_binding: Option<String>,
    /// Names mutably captured by an escaping lambda still in scope (E0204).
    lambda_mut_borrow_stack: Vec<HashSet<String>>,
    /// M9: generic/trait metadata for this program.
    trait_reg: &'a TraitRegistry,
    /// M9.5: local comptime evaluation context.
    ct_funcs: &'a HashMap<String, Func>,
    ct_externs: &'a HashSet<String>,
    ct_base_dir: &'a std::path::Path,
    ct_globals: &'a HashMap<String, crate::Comptime::CtValue>,
    ct_scopes: Vec<HashMap<String, crate::Comptime::CtValue>>,
    /// Active generic type parameters while checking a generic item.
    type_param_scope: Vec<crate::AST::TypeParam>,
    /// E2-M15: reject OS-dependent std APIs in `--freestanding` builds (E3301).
    freestanding: bool,
    /// D-CTEFFECT1: `--allow-impure` was passed — `#Impure` blocks may execute
    /// Tier-2 ambient comptime effects (FS/Env/Exec/IO) at compile time.
    allow_impure: bool,
    /// D-CTEFFECT1: nesting depth of `#Impure` blocks currently being checked.
    /// Passed as `initial_impure_depth` to comptime evaluation of bindings
    /// inside, so the interpreter starts with the gate already open.
    ct_impure_depth: usize,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated while
    /// checking this function body. Drained into `CompileOutput.comptime_inputs`
    /// by Bundle.rs after the full bundle is checked.
    pub(super) ct_embed_inputs: Vec<crate::AST::ComptimeInput>,
    /// D-WHEN2 (ratified 2026-06-19): when true, we are inside a dropped
    /// `#Known if` arm — name-resolution runs normally (so unknown-name
    /// typos are caught) but all other diagnostics are suppressed and the arm
    /// is never lowered to codegen.
    in_dropped_comptime_arm: bool,
    /// E0209 liveness gate (was D-L0201) — pointer to the statements that follow the
    /// currently-executing statement in the innermost block, plus the count.
    /// Set by `check_block` before each call to `check_stmt`.
    /// Safety: valid for the duration of `check_stmt` — the slice lives in
    /// the `Program` AST which outlives the checker.
    stmt_tail_ptr: *const crate::AST::Stmt,
    stmt_tail_len: usize,
    /// E0209 liveness gate (was D-L0201): stack of enclosing block tails, from outermost to innermost.
    /// Each entry is the (ptr, len) of the tail saved before entering a nested
    /// block, so `is_name_live_after` can walk up through all enclosing scopes.
    liveness_frames: Vec<(*const crate::AST::Stmt, usize)>,
    /// D-TASKSCOPE1=A: stack of active `taskgroup` scopes (innermost last).
    taskgroup_stack: Vec<TaskGroupCtx>,
    /// True while inferring the body passed to `g.task => …` — suppresses L1101
    /// (the taskgroup owns the handle until scope exit or an explicit join).
    in_taskgroup_spawn: bool,
    /// D-METHODMACRO1=A: top-level function names whose bare identifier was
    /// read as a VALUE (not called directly) while checking this one function
    /// body — see `CheckerInfer/expr.rs`'s `Expr::Ident` arm, the single spot
    /// that resolves a bare name to a global function's signature. Rolled up
    /// into a whole-program accumulator (`check_func_body`'s
    /// `global_addr_taken` parameter) so `#Inline(Always)` (E0918) can be
    /// checked once every function has run through here.
    inline_addr_taken: HashSet<String>,
}

impl<'a> Checker<'a> {
    pub(crate) fn enter_source_nesting(&mut self, span: Span) -> bool {
        self.source_nesting += 1;
        if self.source_nesting <= crate::Diagnostics::MAX_SOURCE_NESTING {
            return true;
        }
        self.diags.push(Diagnostic::source_nesting_exceeded(
            self.source_nesting,
            span,
        ));
        self.source_nesting -= 1;
        false
    }

    pub(crate) fn leave_source_nesting(&mut self) {
        self.source_nesting -= 1;
    }

    fn concrete_unit_value(&self, expr: &Expr) -> Option<f64> {
        match expr {
            Expr::Ident(name, _) => self
                .concrete_unit_values
                .iter()
                .rev()
                .find_map(|scope| scope.get(name).copied()),
            Expr::MethodCall { method, args, .. }
                if method == crate::Syntax::numeric_conversion_method("Float")?
                    && args.len() == 1 =>
            {
                match &args[0].expr {
                    Expr::Float(value, _, _) => Some(*value),
                    Expr::Unary(crate::AST::UnOp::Neg, inner, _) => match &**inner {
                        Expr::Float(value, _, _) => Some(-*value),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn unit_fact_for_type(&self, ty: &Type) -> Option<(String, UnitFact)> {
        let Type::Named(name) = ty else { return None };
        if let Some(fact) = self.registry.unit_fact(name) {
            return Some((name.clone(), fact.clone()));
        }
        if let Some((module, leaf)) = name.split_once('.') {
            return self.modules.and_then(|modules| {
                self.imports
                    .get(module)
                    .and_then(|index| modules.get(*index))
                    .and_then(|candidate| candidate.registry.unit_fact(leaf))
                    .map(|fact| (name.clone(), fact.clone()))
            });
        }
        None
    }

    pub(crate) fn is_unit_type_name(&self, name: &str) -> bool {
        if self.registry.is_unit_type(name) {
            return true;
        }
        let Some((module, leaf)) = name.split_once('.') else {
            return false;
        };
        let Some(modules) = self.modules else {
            return false;
        };
        let index = self.imports.get(module).copied().or_else(|| {
            self.imports
                .values()
                .copied()
                .find(|&index| modules[index].module_alias == module)
        });
        let Some(index) = index else {
            return false;
        };
        modules.get(index).is_some_and(|candidate| {
            candidate.registry.is_unit_type(leaf) && self.type_is_pub_in(index, leaf)
        })
    }

    pub(crate) fn is_unit_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(name) if self.is_unit_type_name(name))
            || ty.quantity_parts().is_some()
    }

    pub(crate) fn counterpart_unit_type(
        &self,
        source_name: &str,
        fact: &UnitFact,
        kind: QuantityKind,
    ) -> Option<String> {
        let stem = crate::AST::UnitFamilyDef::type_name(&fact.member);
        let leaf = match kind {
            QuantityKind::Linear => stem,
            QuantityKind::Point => format!("{stem}Point"),
            QuantityKind::Delta => format!("{stem}Delta"),
        };
        let candidate = source_name
            .split_once('.')
            .map_or_else(|| leaf.clone(), |(module, _)| format!("{module}.{leaf}"));
        self.unit_fact_for_type(&Type::Named(candidate.clone()))
            .filter(|(_, other)| {
                other.package == fact.package && other.family == fact.family && other.kind == kind
            })
            .map(|_| candidate)
    }

    pub(crate) fn type_satisfies_bound(&self, ty: &Type, bound: &str) -> bool {
        if let Some((dimension, kind)) = crate::Generics::parse_quantity_bound(bound) {
            if let Some((_, fact)) = self.unit_fact_for_type(ty) {
                return fact.family == dimension && fact.kind.name() == kind;
            }
            let Some((_, actual_dimension)) = ty.quantity_parts() else {
                return false;
            };
            return kind == QuantityKind::Linear.name()
                && (self.registry.unit_facts.values().chain(
                    self.modules
                        .into_iter()
                        .flatten()
                        .flat_map(|module| module.registry.unit_facts.values()),
                ))
                    .any(|fact| {
                        fact.family == dimension
                            && fact.kind == QuantityKind::Linear
                            && fact.dimension.as_ref() == Some(&actual_dimension)
                    });
        }
        self.trait_reg.type_implements_trait(ty, bound)
    }

    pub(crate) fn unit_conversion_overflow_diagnostic(&self, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0127",
            "this unit conversion overflows its runtime representation".to_string(),
            "the declared exact scale or offset is outside Float's finite range".to_string(),
            "choose units with a finite conversion boundary".to_string(),
            Some(span),
        )
    }

    pub(crate) fn implicitly_convert_unit(
        &mut self,
        expr: &mut Expr,
        destination_ty: &Type,
        source_ty: &Type,
    ) -> bool {
        let Some((destination_name, destination_fact)) = self.unit_fact_for_type(destination_ty)
        else {
            return false;
        };
        if let Some((base, dimension)) = source_ty.quantity_parts() {
            if base == &Type::Float
                && destination_fact.dimension.as_ref() == Some(&dimension)
                && destination_fact.kind == QuantityKind::Linear
                && destination_fact.scale == crate::AST::UnitRatio::integer(1)
                && destination_fact.offset == crate::AST::UnitRatio::zero()
            {
                let span = expr.span();
                let value = std::mem::replace(expr, Expr::Absent(span));
                *expr = Expr::MethodCall {
                    receiver: Box::new(Expr::Ident(destination_name.clone(), span)),
                    method: crate::Syntax::numeric_conversion_method("Float")
                        .expect("Float conversion is registered")
                        .to_string(),
                    method_span: span,
                    owner_type_args: Vec::new(),
                    type_args: Vec::new(),
                    args: vec![crate::AST::CallArg {
                        convention: crate::AST::AccessConvention::Read,
                        expr: value,
                        span,
                        flags: crate::AST::CallArgFlags::default(),
                        label: None,
                        spread: false,
                    }],
                    recv_type: None,
                    resolved_ret: Some(destination_ty.clone()),
                };
                return true;
            }
        }
        let Some((source_name, source_fact)) = self.unit_fact_for_type(source_ty) else {
            return false;
        };
        if destination_fact.package != source_fact.package
            || destination_fact.family != source_fact.family
            || destination_fact.dimension != source_fact.dimension
            || destination_fact.kind != source_fact.kind
        {
            return false;
        }
        if source_fact.is_measured() || destination_fact.is_measured() {
            self.diags.push(self.measured_unit_diagnostic(
                &destination_name,
                &source_name,
                &source_fact,
                &destination_fact,
                expr.span(),
            ));
            return true;
        }
        if !source_fact.conversion_is_finite_to(&destination_fact) {
            self.diags
                .push(self.unit_conversion_overflow_diagnostic(expr.span()));
            return true;
        }
        let exact = source_fact.is_symbolic()
            || destination_fact.is_symbolic()
            || self.concrete_unit_value(expr).map_or_else(
                || source_fact.conversion_is_total_to(&destination_fact),
                |value| source_fact.converted_value_is_exact_to(&destination_fact, value),
            );
        if !exact {
            self.diags.push(self.inexact_unit_diagnostic(
                &destination_name,
                &source_name,
                expr.span(),
            ));
            return true;
        }
        if self.explicit_units_enabled() && destination_name != source_name {
            self.diags.push(self.explicit_units_diagnostic(
                &destination_name,
                &source_name,
                expr.span(),
            ));
            return true;
        }
        let span = expr.span();
        let source_expr = std::mem::replace(expr, Expr::Absent(span));
        *expr = self.unit_conversion_expr(
            source_expr,
            &source_name,
            &source_fact,
            &destination_name,
            &destination_fact,
            span,
        );
        true
    }

    pub(crate) fn explicit_units_enabled(&self) -> bool {
        let base = self.policy_declarations.iter().any(|declaration| {
            declaration.key == crate::Policy::PolicyKey::ExplicitUnits
                && (matches!(
                    declaration.scope,
                    crate::Policy::PolicyScope::Package | crate::Policy::PolicyScope::Module
                ) || (declaration.scope == crate::Policy::PolicyScope::Function
                    && declaration.target == Some(self.current_function_span)))
        });
        base || self.memory_policy_stack.last().is_some_and(|region| {
            region
                .declarations
                .iter()
                .any(|declaration| declaration.key == crate::Policy::PolicyKey::ExplicitUnits)
        })
    }

    pub(crate) fn explicit_units_diagnostic(
        &self,
        destination_name: &str,
        source_name: &str,
        span: Span,
    ) -> Diagnostic {
        let source_leaf = source_name.rsplit('.').next().unwrap_or(source_name);
        let method = crate::Syntax::conversion_method_for_source(source_leaf);
        Diagnostic::error(
            "E0127",
            "`explicit_units` requires a written conversion".to_string(),
            format!(
                "this scope does not convert `{source_name}` to `{destination_name}` implicitly"
            ),
            format!("write `{destination_name}.{method}(expr)`"),
            Some(span),
        )
    }

    fn measured_unit_diagnostic(
        &self,
        destination_name: &str,
        source_name: &str,
        source: &UnitFact,
        destination: &UnitFact,
        span: Span,
    ) -> Diagnostic {
        let measured = if source.is_measured() { source } else { destination };
        let crate::AST::UnitScaleProvenance::Measured {
            central_value,
            standard_uncertainty,
            source,
        } = &measured.scale_provenance
        else {
            unreachable!()
        };
        Diagnostic::error(
            "E0127",
            format!("`{source_name}` to `{destination_name}` crosses a measured scale"),
            format!(
                "the pinned central value is {central_value} with standard uncertainty {standard_uncertainty} ({source})"
            ),
            format!(
                "write an explicit rounded conversion to `{destination_name}` and audit the pinned source"
            ),
            Some(span),
        )
    }

    pub(crate) fn reject_implicit_unit_conversion(
        &mut self,
        destination_name: &str,
        source_name: &str,
        source_expr: &Expr,
        span: Span,
    ) -> bool {
        if let (Some((_, destination)), Some((_, source))) = (
            self.unit_fact_for_type(&Type::Named(destination_name.to_string())),
            self.unit_fact_for_type(&Type::Named(source_name.to_string())),
        ) {
            if source.is_measured() || destination.is_measured() {
                self.diags.push(self.measured_unit_diagnostic(
                    destination_name,
                    source_name,
                    &source,
                    &destination,
                    span,
                ));
                return true;
            }
            if !source.conversion_is_finite_to(&destination) {
                self.diags
                    .push(self.unit_conversion_overflow_diagnostic(span));
                return true;
            }
            let exact = source.is_symbolic()
                || destination.is_symbolic()
                || self.concrete_unit_value(source_expr).map_or_else(
                    || source.conversion_is_total_to(&destination),
                    |value| source.converted_value_is_exact_to(&destination, value),
                );
            if !exact {
                self.diags.push(self.inexact_unit_diagnostic(
                    destination_name,
                    source_name,
                    span,
                ));
                return true;
            }
        }
        if self.explicit_units_enabled() && destination_name != source_name {
            self.diags.push(self.explicit_units_diagnostic(
                destination_name,
                source_name,
                span,
            ));
            true
        } else {
            false
        }
    }

    fn inexact_unit_diagnostic(
        &self,
        destination_name: &str,
        source_name: &str,
        span: Span,
    ) -> Diagnostic {
        let source_leaf = source_name.rsplit('.').next().unwrap_or(source_name);
        let method = crate::Syntax::conversion_method_for_source(source_leaf);
        Diagnostic::error(
            "E0127",
            "this implicit unit conversion would round".to_string(),
            "the exact scale or offset cannot be represented as an integer conversion at this boundary".to_string(),
            format!(
                "write `{destination_name}.{method}_rounded(expr, .NearestEven, digits: 0)?` or choose another rounding policy"
            ),
            Some(span),
        )
    }
}

pub mod ApiFreeze;
mod Bundle;
/// D-APILABEL1=A: the one argument binder every call form goes through.
pub(crate) mod CallBinder;
mod Captures;
mod CheckerCli;
mod CheckerCore;
mod CheckerCoreLib;
mod CheckerFieldPolicy;
mod CheckerInfer;
mod CheckerInline;
mod CheckerKernel;
mod CheckerItems;
mod CheckerMarkers;
mod CheckerOwnership;
mod CheckerPatchable;
mod CheckerSchedule;
mod CheckerTaskGroup;
use CheckerTaskGroup::TaskGroupCtx;
mod CheckerValidate;
mod Diagnostics;
mod Edition;
mod Effects;
mod MemoryFacts;
mod MemberSpread;
pub mod UnsafeObligations;
mod FFI;
pub mod HotSwap;
mod OSTarget;
mod Protocol;
mod Purity;
mod Registration;
pub mod Schema;
mod SchemaMigration;
mod ScopeMembers;
mod PolicyFacts;
mod BudgetSpecs;
pub use BudgetSpecs::{collect_budget_specs, collect_budget_specs_bundle, collect_located_budget_specs_bundle, BudgetApplicability, BudgetAxis, BudgetComparisonFact, BudgetLimitFact, BudgetQuantity, BudgetRawQuantity, BudgetSpec, LocatedBudgetSpec};
mod CheckerReferences;
mod State;
mod Taint;
mod WebApp;
mod WebPartition;

pub(crate) use Bundle::*;
pub(crate) use Captures::*;

/// Exact AST name-reference query used by codegen planning. Unlike conservative
/// feature-discovery walkers, this is exhaustive over every statement/expression form.
pub fn stmt_references_name_exact(stmt: &Stmt, name: &str) -> bool {
    Captures::stmt_refs_name(stmt, name)
}

/// Same question as `stmt_references_name_exact`, but a use inside a lambda
/// body counts. D-TASKBORROW1=A: a `taskgroup` child borrows through a lambda.
pub fn stmt_references_name_deep(stmt: &Stmt, name: &str) -> bool {
    Captures::stmt_uses_name_through_lambdas(stmt, name)
}

pub(crate) use CheckerCli::*;
pub use CheckerCoreLib::*;
pub(crate) use CheckerFieldPolicy::*;
pub(crate) use CheckerPatchable::*;
pub(crate) use CheckerValidate::*;
pub(crate) use Diagnostics::*;
/// Shared by TIR lowering: a loop consumes any collection whose element type
/// holds a task handle (see `type_holds_task_handle`).
pub use Diagnostics::type_holds_task_handle;
pub(crate) use Effects::*;
pub(crate) use Purity::*;
pub use Registration::*;
pub(crate) use Taint::check_func_taint;
pub(crate) use FFI::*;
// D-STATE1: typestate pass — wrong-state operation (E0150).
pub(crate) use State::{check_items_state, StateTable};
// D-LIN1: single-use (must-consume) diagnostics live in CheckerOwnership.
pub use WebApp::extract_web_app_graph;
pub(crate) use WebPartition::check_web_partition;
// D-OSTARGET1=A: native OS platform gating (mixed-axis + unmatched-call).
pub(crate) use MemberSpread::desugar_member_spreads;
pub(crate) use OSTarget::{check_os_target, desugar_os_switches};

// Public entry points (preserve `jet::Sema::<item>` paths).
pub use Bundle::{
    bundle_has_comptime_evaluation, check_bundle, check_bundle_allow_impure, check_bundle_for_output,
    check_bundle_for_output_opts, check_bundle_freestanding, check_bundle_with_effect_facts,
    check_bundle_with_effect_facts_for_build, check_bundle_with_effect_facts_incremental,
    specialize_function_types,
    IncrementalSemaCache, IncrementalSemaStats,
};
pub use Effects::{DefinitionAnchorFact, EffectSummary, SemIndexEffectFacts};
pub use MemoryFacts::{
    check_memory_facts, project_memory_fact, MemoryCall, MemoryEvent, MemoryEventKind, MemoryFact,
    MemoryFactDeclaration, MemoryPolicyRegion, MemoryProjection, MemorySummary,
    OpenMemoryDispatch,
};
pub use PolicyFacts::{
    collect_policy_facts, collect_policy_facts_from_program, PolicyDomain, PolicyFact,
    PolicyFactGraph,
};
// D-EFFBUDGET1: the closed effect vocabulary, exposed so jet-driver can
// validate `pkg.jet` `effects:`/`grants:` manifest keys against it.
// D-EFFTREE1: also export the tree helpers — jet-driver's EffectBudget and
// manifest parsing need root validation and ancestor-subsumption coverage
// too, not just the bare enum.
pub(crate) use CheckerInline::{check_inline_always_fn, e0918_address_taken};
pub(crate) use CheckerMarkers::check_marker_vocabulary;
pub(crate) use CheckerSchedule::check_every_marker;
pub use Effects::{
    builtin_effect, core_effect, effect_covers, effect_root, effect_row_var, parse_effect_name,
    resolve_effect_name, show_set, undeclared_effect, Effect, EffectSet,
};
pub use Purity::{check_pure_fn, check_pure_program_root, e3401, e3402, e3403};
pub use Registration::effect_key;
pub use FFI::{e3202, e3301, e3302, e3303};
// D-MIGRATE2C: `jet inspect schema status` reuses the schema-migration diff.
pub use SchemaMigration::{check_schema_migrations, desugar_migrations};

/// D-REACTCORE1: free variable reads in a statement block (for reactive-scope capture cloning).
pub fn block_free_var_reads(stmts: &[crate::AST::Stmt]) -> HashSet<String> {
    let mut bound = HashSet::new();
    let mut read = HashSet::new();
    let mut mut_cap = HashSet::new();
    Captures::block_collect_captures(stmts, &mut bound, &mut read, &mut mut_cap);
    read.extend(mut_cap);
    read
}
