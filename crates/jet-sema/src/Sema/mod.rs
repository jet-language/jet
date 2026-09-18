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
    AccessConvention, CallablePolicyChain, Deprecation, Expr, ExternFn, Func, KnowledgeVector,
    Marker, QuantityKind, Stmt, StrPart, Type, VariantPayload,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Source bounds for a semantic edit whose AST node stores only its method
/// token. The checker uses these helpers only after a typed shape has been
/// proved; they never participate in recognizing a lint.
pub(crate) fn source_expr_start(expr: &Expr) -> usize {
    match expr {
        Expr::Paren(_, span) => span.start,
        Expr::Field(base, _, _) => source_expr_start(base),
        Expr::OptField { base, .. } => source_expr_start(base),
        Expr::MethodCall { receiver, .. } => source_expr_start(receiver),
        _ => expr.span().start,
    }
}

pub(crate) fn source_call_end(source: &str, method_span: Span) -> Option<usize> {
    let open = method_span.end + source.get(method_span.end..)?.find('(')?;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in source.get(open..)?.char_indices() {
        let index = open + offset;
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => quoted = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn source_expr_end(source: &str, expr: &Expr) -> Option<usize> {
    match expr {
        Expr::MethodCall { method_span, .. } => source_call_end(source, *method_span),
        Expr::Paren(_, span) => Some(span.end),
        _ => Some(expr.span().end),
    }
}

mod DataPlan;

pub use DataPlan::{
    classify_data_call, classify_data_receiver_call, callable_fact, physical_facts, scalar_schema,
    DataPlanBuilder, DataPlanCallKind,
};

mod ResourceSchedule;
pub(crate) use ResourceSchedule::check_game_frame_lambda;

mod Casing;

/// Re-export so existing callers (`jet::Sema::FuncSig`) keep working.
pub use crate::AST::FuncSig;

#[derive(Debug, Clone)]
pub(crate) struct UninitState {
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

    /// D-UNINIT1 branch merge: a place is initialised after a branch only where
    /// every path initialised it, so the initialised parts intersect. Called by
    /// the [`FlowFacts::Uninit`] plane's join rule, nowhere else.
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
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    /// D-STRUCT-LIFE1=A: the method's retiring lifecycle marker.
    deprecation: Option<Deprecation>,
    /// D-GENERIC-CALL1=A: method-owned type parameters, distinct from the
    /// receiver's generic arguments.
    type_params: Vec<crate::AST::TypeParam>,
    is_static: bool,
    self_conv: Option<AccessConvention>,
    /// D-NARG1 / D-DEFAULT-SHAPE1: parameter names and default-value presence, parallel to
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
    /// D-DEFAULT-SHAPE1=B: default expressions for parameters, parallel to param_info.
    /// `None` when no default; only trailing params may have defaults.
    pub(crate) defaults: Vec<Option<crate::AST::Expr>>,
    /// D-MUSTUSE1 (c18iwxqx): `#MustUse` method — return cannot be silently ignored (E0419).
    pub(crate) must_use: bool,
    pub(crate) return_view_provenance: crate::AST::ViewProvenanceCell,
}

impl MethodSig {
    /// Keep method calls on the same shared callable failure projection as
    /// free functions and trait methods.
    pub(crate) fn failure_contract(&self) -> crate::AST::FailureContract {
        crate::AST::FailureContract::from_return_type(self.return_type.as_ref())
    }

    pub(crate) fn effective_return_type(&self) -> Type {
        self.failure_contract().effective_type()
    }
}

/// Select the return contract used while checking a function body. Ordinary
/// Jet callables use the shared failure carrier; compiler-synthesized trait
/// protocol methods keep the raw return ABI declared by their trait bridge.
pub(crate) fn checked_body_return_type(function: &Func, raw_protocol_return: bool) -> Option<Type> {
    if raw_protocol_return || function.name.starts_with("__errconv_") {
        function.return_type.clone()
    } else {
        Some(function.effective_return_type())
    }
}

#[derive(Debug, Clone)]
pub(crate) enum TypeDef {
    Struct {
        fields: Vec<(String, Span, Type)>,
        methods: HashMap<String, MethodSig>,
        /// D-STRUCT-LIFE1=A: type-level retiring lifecycle marker.
        deprecation: Option<Deprecation>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before
        /// `struct`. Values of this type must be consumed exactly once
        /// (E0140/E0141) and may not be aliased (E0142).
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `#MustUse` was present before `struct`.
        /// Values of this type cannot be silently ignored as a bare expression
        /// statement (E0419).
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
        /// D-MIGRATE1: `#PublishedSchema` was present (either the standalone
        /// prefix or the grouped `#[PublishedSchema, …]` spelling). Codegen
        /// adds a hidden `__jet_unknown_fields` carrier field to the generated
        /// Rust struct, so a comptime-folded value of this type has no
        /// complete literal form (`ct_value_is_emittable`).
        published: bool,
    },
    Enum {
        variants: HashMap<String, (Span, VariantPayload)>,
        variant_order: Vec<String>,
        /// D-TAG1: variant groups — group path → (span, ordered leaf paths in
        /// its subtree). A group name matches its whole subtree in patterns.
        groups: HashMap<String, (Span, Vec<String>)>,
        methods: HashMap<String, MethodSig>,
        /// D-STRUCT-LIFE1=A: type-level retiring lifecycle marker.
        deprecation: Option<Deprecation>,
        /// D-LIN1 (ratified 2026-06-21): `#SingleUse` was present before `enum`.
        single_use: bool,
        /// D-MUSTUSE1 (c18iwxqx): `#MustUse` was present before `enum`.
        must_use: bool,
        /// D-REPRC2: present only for `#Layout(c[, tag: Width])`.
        c_layout_tag: Option<crate::AST::CEnumTag>,
    },
    /// D-DIST1 (ratified 2026-06-19): a distinct type — a nominal wrapper over
    /// a base type. No implicit coercion either direction (E0128). Ability
    /// requests remain ordinary derive rows after registration.
    Distinct {
        base: Type,
        derives: Vec<String>,
        /// D-STRUCT-LIFE1=A: type-level retiring lifecycle marker.
        deprecation: Option<Deprecation>,
        /// D-TYPE2-FOUND1: semantic facts live on the one type knowledge
        /// vector. The interval plane is projected here for existing checks.
        knowledge: KnowledgeVector,
    },
    /// D-TYPEALIAS1 / D-ALIAS-OP1=B (ratified 2026-06-28): `alias Name<T> :: …` — transparent
    /// generic shortcut; expands in sema, erases at codegen.
    Alias {
        params: Vec<crate::AST::TypeParam>,
        target: Type,
        deprecation: Option<Deprecation>,
    },
}

/// D-FOUND-RECEIPT1: checked metadata for one named typed receipt section.
/// This stays in the declaration registry; execution tiers only marshal the
/// already-proven name, type identity, and schema digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReceiptSectionMeta {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) schema_digest: String,
    pub(crate) span: Span,
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
        self.conversion_parts_to(destination)
            .is_some_and(|(scale, offset)| {
                scale == crate::AST::UnitRatio::integer(1)
                    && offset == crate::AST::UnitRatio::zero()
            })
    }

    fn conversion_is_finite_to(&self, destination: &Self) -> bool {
        self.conversion_parts_to(destination)
            .is_some_and(|(scale, offset)| {
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

#[derive(Clone)]
pub(crate) struct TypeRegistry {
    types: HashMap<String, TypeDef>,
    /// D-FAILURE-FOUNDATION1=A: user-named types carrying `#Error` may be
    /// used as explicit callable failure domains.
    error_types: HashSet<String>,
    /// D-QUANTITY-PRINT1: all concrete types minted by `#UnitFamily`.
    unit_types: HashSet<String>,
    /// #603: the one normalized source of truth for concrete unit conversion,
    /// package identity, dimension, affine kind, and algebra facts.
    unit_facts: HashMap<String, UnitFact>,
    /// D-TYPE2-PLANE1=A: source-declared unit members are the one literal
    /// fact registry. Canonical Time literals use these registered facts;
    /// there is no compiler-owned suffix parser or duration table.
    literal_facts: HashMap<String, UnitFact>,
    /// D-FIELDPOL1: struct name → computed field name → (span, declared
    /// type). A computed field never appears in `TypeDef::Struct::fields`
    /// (it's not a stored field, and is never required/allowed in a struct
    /// literal — E0339); this side table is the only place sema resolves its
    /// type for a *read* (`field_type`).
    computed_fields: HashMap<String, HashMap<String, (Span, Type)>>,
    /// D-DEFAULT-SHAPE1=B: struct name → field name → default expression for omitted
    /// `Type.{ … }` construction and wire/CLI absence.
    field_defaults: HashMap<String, HashMap<String, crate::AST::Expr>>,
    /// D-DX-PLUGIN1=D: checked `core.devtools.publish` facts are owned by
    /// the module's existing type registry until bundle completion projects
    /// them into the shared panel registry. Interior mutability keeps the
    /// body checker read-only over the nominal registry while still allowing
    /// one typed fact sink.
    devtools_publications: std::cell::RefCell<Vec<jet_foundation::AST::DevtoolsFactPublication>>,
    /// Named checked receipt declarations keyed by canonical type identity.
    receipt_sections: HashMap<String, ReceiptSectionMeta>,
}

impl TypeRegistry {

    pub(crate) fn receipt_section(&self, type_name: &str) -> Option<&ReceiptSectionMeta> {
        self.receipt_sections.get(type_name)
    }
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    pub(crate) fn is_error_type(&self, name: &str) -> bool {
        self.error_types.contains(name)
    }

    /// Validate the error side of a prefix failure contract. Anonymous unions
    /// are valid only when every member is itself an allowed domain; aliases
    /// are followed with a cycle guard so invalid shapes fail in sema rather
    /// than at codegen.
    pub(crate) fn is_error_domain(&self, ty: &Type) -> bool {
        fn visit(registry: &TypeRegistry, ty: &Type, seen: &mut HashSet<String>) -> bool {
            match ty {
                Type::Named(name) => {
                    if name == crate::Syntax::TYPE_ERR
                        || name == crate::Syntax::TYPE_NEVER
                        || registry.is_error_type(name)
                        || (!registry.contains(name)
                            && crate::Sema::Diagnostics::is_core_error_type(name))
                    {
                        return true;
                    }
                    let Some((params, target)) = registry.type_alias(name) else {
                        return false;
                    };
                    if !params.is_empty() || !seen.insert(name.clone()) {
                        return false;
                    }
                    let valid = visit(registry, target, seen);
                    seen.remove(name);
                    valid
                }
                // A generic `#Error` declaration is still one named error
                // domain. Generic aliases are expanded here as well when a
                // caller reaches this registry before the normal type
                // resolver has expanded them.
                Type::Apply { name, args } => {
                    if registry.is_error_type(name)
                        || (!registry.contains(name)
                            && crate::Sema::Diagnostics::is_core_error_type(name))
                    {
                        return true;
                    }
                    let Some((params, target)) = registry.type_alias(name) else {
                        return false;
                    };
                    if params.len() != args.len() || !seen.insert(name.clone()) {
                        return false;
                    }
                    let substitutions = params
                        .iter()
                        .zip(args.iter().cloned())
                        .map(|(param, argument)| (param.name.clone(), argument))
                        .collect();
                    let expanded = crate::Generics::substitute_type(target, &substitutions);
                    let valid = visit(registry, &expanded, seen);
                    seen.remove(name);
                    valid
                }
                Type::Union(members) => {
                    !members.is_empty()
                        && members.iter().all(|member| visit(registry, member, seen))
                }
                // D-VALIDATE-DECODE1=B: typed decoding reports one
                // accumulated Core error list, not an arbitrary list-shaped
                // error domain.
                Type::List(inner) => {
                    matches!(inner.as_ref(), Type::Named(name)
                        if name == "FieldError" && !registry.contains(name))
                }
                _ => false,
            }
        }
        visit(self, ty, &mut HashSet::new())
    }

    /// A struct the user declared, so codegen emits a `__jet_<Name>` Rust type
    /// for it. False for a builtin the comptime evaluator merely models as a
    /// struct, which has no such Rust type.
    pub(crate) fn is_user_struct(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Struct { .. }))
    }

    /// The enum counterpart of `is_user_struct`.
    pub(crate) fn is_user_enum(&self, name: &str) -> bool {
        matches!(self.types.get(name), Some(TypeDef::Enum { .. }))
    }

    /// D-MIGRATE1: true when `name` is a `#PublishedSchema` struct. Its
    /// generated Rust struct carries a hidden `__jet_unknown_fields` field, so
    /// no source-shaped literal of it is complete.
    pub(crate) fn is_published_schema_struct(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Struct {
                published: true,
                ..
            })
        )
    }

    pub(crate) fn unit_dimension(&self, name: &str) -> Option<crate::AST::Dimension> {
        self.unit_facts
            .get(name)
            .and_then(|fact| fact.dimension.clone())
    }

    pub(crate) fn is_unit_type(&self, name: &str) -> bool {
        self.unit_types.contains(name)
    }

    pub(crate) fn unit_fact(&self, name: &str) -> Option<&UnitFact> {
        self.unit_facts.get(name)
    }

    pub(crate) fn unit_literal(&self, family: &str, member: &str) -> Option<&UnitFact> {
        self.literal_facts.get(&format!("{family}::{member}"))
    }

    fn struct_fields(&self, name: &str) -> Option<&[(String, Span, Type)]> {
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

    /// D-DEFAULT-SHAPE1=B: default expressions for omitted struct-literal fields.
    pub(crate) fn field_defaults(&self, name: &str) -> Option<&HashMap<String, crate::AST::Expr>> {
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

    pub(crate) fn method_names(&self, type_name: &str) -> Vec<String> {
        let mut names = match self.types.get(type_name) {
            Some(TypeDef::Struct { methods, .. }) | Some(TypeDef::Enum { methods, .. }) => {
                methods.keys().cloned().collect()
            }
            _ => Vec::new(),
        };
        names.sort();
        names
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
                derives,
                ..
            }) if derives.iter().any(|derive| derive == crate::Syntax::MARKER_NUMERIC)
        )
    }

    /// D-RANGETYPE1: the declared `(lo, hi)` inclusive bounds of a range type
    /// (`distinct Int(0..10)`), or `None` for a plain distinct type / a name
    /// that isn't distinct.
    pub(crate) fn distinct_range(&self, name: &str) -> Option<(i64, i64)> {
        match self.types.get(name) {
            Some(TypeDef::Distinct { knowledge, .. }) => knowledge.interval(),
            _ => None,
        }
    }

    /// D-TYPE2-REFINE1: project one integer interval from the shared knowledge
    /// vector. Plain `Int` is deliberately not a proof: its full domain needs
    /// the ordinary runtime index check.
    pub(crate) fn integer_interval(&self, ty: &Type) -> Option<(i128, i128)> {
        let ty = ty.without_user_tags();
        match ty {
            Type::IntN { .. } => ty.integer_range(),
            Type::Named(name) => match self.types.get(name) {
                Some(TypeDef::Distinct {
                    base, knowledge, ..
                }) if base.is_integer() => knowledge.interval_i128(),
                _ => None,
            },
            _ => None,
        }
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#Comparable`.
    pub(crate) fn distinct_is_comparable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                derives,
                ..
            }) if derives.iter().any(|derive| derive == crate::Generics::COMPARABLE)
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#Printable`.
    pub(crate) fn distinct_is_printable(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                derives,
                ..
            }) if derives.iter().any(|derive| derive == crate::Generics::PRINTABLE)
        )
    }

    /// D-CAPBUNDLE1: true when the distinct type has `#CodableAsBase`.
    pub(crate) fn distinct_is_codable_as_base(&self, name: &str) -> bool {
        matches!(
            self.types.get(name),
            Some(TypeDef::Distinct {
                derives,
                ..
            }) if derives.iter().any(|derive| derive == crate::Generics::ENCODE)
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
    /// Store one sema-typed publication fact in the module's canonical
    /// semantic-facts owner. Duplicate inference visits at one source span do
    /// not create duplicate protocol rows.
    pub(crate) fn record_devtools_publication(
        &self,
        publication: jet_foundation::AST::DevtoolsFactPublication,
    ) {
        let mut facts = self.devtools_publications.borrow_mut();
        if !facts.iter().any(|existing| existing.span == publication.span) {
            facts.push(publication);
        }
    }

    /// Snapshot the checked publication facts for bundle completion. The
    /// caller projects these rows into the shared panel registry exactly once.
    pub(crate) fn devtools_publications(
        &self,
    ) -> Vec<jet_foundation::AST::DevtoolsFactPublication> {
        self.devtools_publications.borrow().clone()
    }
    /// Restore the publication facts that preceded an erased scope. The body
    /// checker shares this per-module registry through interior mutability, so
    /// the erased-scope boundary must roll back writes made by nested calls too.
    pub(crate) fn restore_devtools_publications(
        &self,
        publications: Vec<jet_foundation::AST::DevtoolsFactPublication>,
    ) {
        *self.devtools_publications.borrow_mut() = publications;
    }

}

fn marker_argument<'a>(marker: &'a Marker, name: &str, positional: usize) -> Option<&'a Expr> {
    marker
        .arg_labels
        .iter()
        .enumerate()
        .find_map(|(index, label)| {
            label
                .as_ref()
                .filter(|(label, _)| label == name)
                .and_then(|_| marker.expr_arg(index))
        })
        .or_else(|| marker.expr_arg(positional))
}

fn marker_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Str(parts, _) => parts
            .iter()
            .try_fold(String::new(), |mut text, part| match part {
                StrPart::Lit(value) => {
                    text.push_str(value);
                    Some(text)
                }
                StrPart::Interp(..) => None,
            }),
        Expr::Ident(name, _) if name == "none" => None,
        _ => None,
    }
}

fn deprecation_from_markers(markers: &[Marker]) -> Option<Deprecation> {
    let marker = markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_DEPRECATED)?;
    Some(Deprecation {
        since: marker_string(marker_argument(marker, "since", 0)?)?,
        replacement: marker_string(marker_argument(marker, "use", 1)?)?,
        removed_in: marker_argument(marker, "removed_in", 2).and_then(marker_string),
    })
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
        params: f
            .params
            .iter()
            .map(|p| {
                let base = p.ty.with_effective_fn_returns();
                let ty = if p.variadic {
                    Type::List(Box::new(base))
                } else {
                    base
                };
                (p.convention, ty)
            })
            .collect(),
        return_type: f.return_type.clone(),
        deprecation: deprecation_from_markers(&f.markers),
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

pub(crate) fn func_to_sig(f: &Func) -> FuncSig {
    let param_variadic: Vec<bool> = f.params.iter().map(|p| p.variadic).collect();
    let return_view_provenance = crate::AST::ViewProvenanceCell::new();
    if let Some(provenance) = &f.return_view_provenance {
        let _ = return_view_provenance.set(provenance.clone());
    }
    let callable_policies = f
        .markers
        .iter()
        .find(|marker| marker.name == crate::Syntax::MARKER_POLICY)
        .and_then(|marker| CallablePolicyChain::parse(&marker.expr_args_owned()).ok())
        .unwrap_or_default();
    FuncSig {
        params: f
            .params
            .iter()
            .map(|p| {
                let base = p.ty.with_effective_fn_returns();
                let ty = if p.variadic {
                    Type::List(Box::new(base))
                } else {
                    base
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
        deprecation: deprecation_from_markers(&f.markers),
        is_extern: f.inline_foreign.is_some(),
        is_c_abi: false,
        c_abi_name: None,
        callback_transport: None,
        callback_plan_digest: None,
        callback_identity: None,
        foreign_effect_root: f
            .inline_foreign
            .as_ref()
            .map(|foreign| inline_foreign_effect_root(&foreign.lang)),
        undo: f.undo.as_ref().map(|(name, _)| name.clone()),
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
        memo_bound: crate::AST::memo_bound_from_markers(&f.markers),
        is_foreign_thread_safe: foreign_thread_safe_func(f),
        is_sanitizer: f.is_sanitizer,
        is_must_use: f.is_must_use,
        param_view_from_names: f
            .params
            .iter()
            .map(|p| p.declared_view_from_names.clone())
            .collect(),
        callable_policies,
    }
}

fn inline_foreign_effect_root(lang: &str) -> String {
    match lang {
        "c" => "FFI.C".to_string(),
        "asm" => "FFI.Asm".to_string(),
        "cpp" => "FFI.Cpp".to_string(),
        other => format!("FFI.{other}"),
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
        deprecation: None,
        is_extern: true,
        is_c_abi,
        c_abi_name: ef.abi.as_ref().map(|(name, _)| name.clone()),
        callback_transport: ef.callback_transport.clone(),
        callback_plan_digest: ef.callback_plan_digest.clone(),
        callback_identity: ef.callback_identity.clone(),
        foreign_effect_root: ef.effect_root.clone(),
        undo: ef.undo.as_ref().map(|(name, _)| name.clone()),
        // raw calls require an audited `#Unsafe` boundary. C out-pointers keep
        // their existing E3103 gate; no Result adapter is invented here.
        is_unsafe: ef.params.iter().any(|p| p.convention != AccessConvention::Read)
            || (is_c_abi
                && ef.params.iter().any(|p| {
                    matches!(&p.ty, Type::Apply { name, .. } if name == crate::Syntax::TYPE_PTR)
                })),
        is_pure: false,      // extern functions are always considered impure
        memo_bound: None,
        is_foreign_thread_safe: false,
        is_sanitizer: false, // extern functions can't be sanitizers
        is_must_use: false,
        param_view_from_names: ef.params.iter().map(|_| None).collect(),
        callable_policies: CallablePolicyChain::default(),
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
    f.is_pure
        && f.type_params.is_empty()
        && f.pre.is_empty()
        && f.post.is_empty()
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

/// D-FFI-CALLBACK2=A: managed callbacks may retain immutable cloned captures,
/// but cannot move resources, mutate captured state, or carry an unbounded
/// effect row into a foreign thread. The registration runtime owns the cloned
/// captures until shutdown completion.
pub(crate) fn foreign_managed_callback_lambda(lam: &crate::AST::Lambda) -> bool {
    lam.take_names.is_empty()
        && lam.meta.mut_captures.is_empty()
        && !lam.meta.effect_maximal
        && lam
            .meta
            .effect_solved
            .iter()
            .all(|effect| matches!(effect.as_str(), "IO.Write" | "FFI.C"))
}

/// D-NARG-D2: Walk one default expression exhaustively and replace each
/// declaration-local reference with its compiler-private slot name. The
/// replacement is a slot read, never a supplied argument AST, so side effects
/// in a written argument cannot be duplicated by a default.
pub(crate) fn substitute_param_refs(
    mut expr: crate::AST::Expr,
    refs: &[(&str, String)],
) -> crate::AST::Expr {
    expr.for_each_expr_mut(|node| {
        let crate::AST::Expr::Ident(name, _) = node else {
            return;
        };
        if let Some((_, replacement)) = refs.iter().find(|(param, _)| *param == name) {
            *name = replacement.clone();
        }
    });
    expr
}

/// D-NARG-D2: Use the same exhaustive expression walker to check whether a
/// default expression references any parameter that appears *after* the
/// current parameter index. Collects the names of any forward-referenced
/// parameters found. `all_param_names` is the full list of non-self parameter
/// names for this function (in order).
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
    let mut copy = expr.clone();
    copy.for_each_expr_mut(|node| {
        let crate::AST::Expr::Ident(name, span) = node else {
            return;
        };
        if let Some(idx) = all_param_names
            .iter()
            .position(|candidate| candidate == name)
        {
            if idx >= default_param_idx {
                found.push((name.clone(), *span));
            }
        }
    });
}

#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    def_span: Span,
    /// Source span of this local binding's `::`/`:=` sigil, when it has one.
    /// Parameters and compiler-created locals have no binding sigil.
    binding_sigil_span: Option<Span>,
    ty: Type,
    mutable: bool,
    /// Set when the name is a parameter (with its access convention).
    param_conv: Option<AccessConvention>,
    /// Loop nesting depth where the name was declared (for move-in-loop).
    decl_loop_depth: usize,
    /// Whether a function value has the thread-safe representation required by
    /// `core.sys.on_interrupt`. Ordinary `fn` values use `Rc`; only named
    /// functions and already-proven callback-safe aliases may cross that
    /// boundary.
    interrupt_sendable: bool,
    /// D-DATARACE1=C: `#Local` pin on a reactive binding.
    reactive_local: bool,
    /// D-DATARACE1=C: `#Shared` pin on a reactive binding.
    reactive_shared: bool,
    /// D-LIN1 (ratified 2026-06-21): set (to the binding name's span) when this
    /// local owns a `#SingleUse` value that must be consumed exactly once. `None`
    /// for ordinary values, for parameters (the caller owns the consume duty), and
    /// for `view`/`&` borrows (which never own). When still in scope and not in
    /// `moved` at scope end, E0140 fires.
    single_use_span: Option<Span>,
    /// Pure compile-time value for immutable-local diagnostics and folding.
    /// This does not make an ordinary local available to `comptime` code.
    constant_value: Option<crate::Comptime::CtValue>,
    /// The initializer reported a type error, so this name has no usable
    /// meaning, even when inference supplied a recovery type. Reads stop here
    /// so the original diagnostic stays paired with the required use-site
    /// diagnostic. A well-typed value whose initializer failed an ownership,
    /// sendability, or contract rule is NOT invalid — its later reports stand.
    invalid: bool,
}

#[derive(Debug, Clone)]
struct UnusedBinding {
    name: String,
    span: Span,
    parameter: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LoopValueKind {
    Effect,
    Collecting,
    Result,
}

#[derive(Clone)]
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
/// D-FOUND-VIEW1: core mapped-file methods all return borrowed carriers rooted
/// at their `MappedFile` receiver. Keep this family predicate shared by
/// inference and ownership so neither tier invents a second method list.
pub(crate) fn is_mapped_file_view_method(handle: &str, method: &str) -> bool {
    handle == "MappedFile" && matches!(method, "window" | "window_len" | "lines")
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

#[derive(Debug, Clone, Default)]
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
    Index {
        value: Option<i64>,
        span: Span,
    },
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
    /// `FlowFacts::depth` at declaration; facts disappear with their binding.
    scope_len: usize,
    /// Order the window was opened in. The prover reports the newest window
    /// that matches, so the order has to survive the store, not the container.
    seq: usize,
    /// Owner operation that invalidated storage, when invalidation is allowed
    /// to happen (arena reset). Ordinary owner writes/moves are rejected.
    invalidated: Option<(String, Span)>,
}

/// D-MEM1 S9 / #649: the one store of open borrow windows, one plane of the
/// checker's flow facts. Reads live here; invalidation lives with the ownership
/// prover (D-FACT-OWN1).
pub(crate) type ViewStore = FlowFacts::Facts<FlowFacts::View>;

/// Record one window. The fact carries the scope that owns it, so it leaves
/// with that scope and reveals any window it shadowed.
pub(crate) fn push_view_fact(store: &mut ViewStore, name: &str, mut fact: ViewFact) {
    let depth = fact.scope_len;
    fact.seq = store
        .all()
        .flat_map(|(_, facts)| facts.iter())
        .map(|held| held.seq + 1)
        .max()
        .unwrap_or(0);
    store.entry_at(name, depth).push(fact);
}

/// Every open window, newest first. Several bindings can hold a window over one
/// place; the prover answers with the one opened last.
pub(crate) fn view_facts_newest_first(store: &ViewStore) -> Vec<(&str, &ViewFact)> {
    let mut all: Vec<(&str, &ViewFact)> = store
        .all()
        .flat_map(|(name, facts)| facts.iter().map(move |fact| (name, fact)))
        .collect();
    all.sort_by_key(|(_, fact)| std::cmp::Reverse(fact.seq));
    all
}

/// The innermost window this name currently opens.
pub(crate) fn current_view_fact<'a>(store: &'a ViewStore, name: &str) -> Option<&'a ViewFact> {
    store.get(name)?.last()
}

/// The innermost window this name opens, only when it belongs to this binding.
/// A non-view shadow with the same spelling never exposes an outer window.
pub(crate) fn current_view_fact_for_binding<'a>(
    store: &'a ViewStore,
    name: &str,
    binding_span: Span,
) -> Option<&'a ViewFact> {
    current_view_fact(store, name).filter(|fact| fact.binding_span == binding_span)
}

/// Every window this binding opens, at every depth.
pub(crate) fn view_facts_for_binding<'a>(
    store: &'a ViewStore,
    name: &str,
    binding_span: Span,
) -> Vec<&'a ViewFact> {
    store
        .facts_of(name)
        .flatten()
        .filter(|fact| fact.binding_span == binding_span)
        .collect()
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
                (
                    ViewProjection::Index { value: Some(a), .. },
                    ViewProjection::Index { value: Some(b), .. },
                ) if a != b => return false,
                (
                    ViewProjection::Index {
                        value: Some(index), ..
                    },
                    ViewProjection::Range {
                        start: Some(start),
                        end: Some(end),
                        ..
                    },
                )
                | (
                    ViewProjection::Range {
                        start: Some(start),
                        end: Some(end),
                        ..
                    },
                    ViewProjection::Index {
                        value: Some(index), ..
                    },
                ) if index < start || index > end => return false,
                (
                    ViewProjection::Range {
                        start: Some(a_start),
                        end: Some(a_end),
                        ..
                    },
                    ViewProjection::Range {
                        start: Some(b_start),
                        end: Some(b_end),
                        ..
                    },
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
            seq: 0,
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
        let mut graph = ViewStore::new();
        push_view_fact(&mut graph, "window", fact(1, 1, ViewKind::List));
        push_view_fact(&mut graph, "window", fact(2, 2, ViewKind::String));
        assert_eq!(
            current_view_fact(&graph, "window").map(|f| f.kind),
            Some(ViewKind::String)
        );
        graph.leave_depth(2);
        assert_eq!(
            current_view_fact(&graph, "window").map(|f| f.kind),
            Some(ViewKind::List)
        );
    }

    #[test]
    fn non_view_binding_identity_hides_stale_same_spelling_fact() {
        let mut graph = ViewStore::new();
        push_view_fact(&mut graph, "window", fact(1, 1, ViewKind::List));
        assert!(current_view_fact_for_binding(&graph, "window", Span::new(99, 100)).is_none());
    }

    #[test]
    fn stable_owner_identity_distinguishes_same_spelling() {
        let a = fact(1, 1, ViewKind::Buffer).place;
        let b = fact(2, 1, ViewKind::Matrix).place;
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn invalidation_uses_owner_identity_not_spelling() {
        let mut graph = ViewStore::new();
        let outer = fact(1, 1, ViewKind::Arena);
        let inner = fact(2, 2, ViewKind::Arena);
        let inner_owner = inner.place.owner.clone();
        push_view_fact(&mut graph, "outer", outer);
        push_view_fact(&mut graph, "inner", inner);

        CheckerOwnership::invalidate_view_owner(
            &mut graph,
            &inner_owner,
            "reset",
            Span::new(40, 41),
        );

        assert!(current_view_fact(&graph, "outer")
            .unwrap()
            .invalidated
            .is_none());
        assert!(current_view_fact(&graph, "inner")
            .unwrap()
            .invalidated
            .is_some());
    }

    #[test]
    fn different_fields_do_not_alias() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections
            .push(ViewProjection::Field("left".to_string()));
        b.projections
            .push(ViewProjection::Field("right".to_string()));
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn dynamic_indexes_are_conservatively_overlapping() {
        let mut a = fact(1, 1, ViewKind::List).place;
        let mut b = a.clone();
        a.projections.push(ViewProjection::Index {
            value: None,
            span: Span::new(20, 21),
        });
        b.projections.push(ViewProjection::Index {
            value: None,
            span: Span::new(30, 31),
        });
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
    CallableValue,
    TraitValue(String),
    ThreadConfined(String),
    LocalReactive(String),
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
    ParallelWorker,
    Kernel,
    InterruptCallback,
    /// HTTP handlers are retained by the server and invoked on request
    /// workers; their captures must be `Send + Sync`.
    HttpHandler,
}

/// What the driver is compiling — affects `run` / test requirements (M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    /// `jet run` / `jet build` — needs `run`, ignores test blocks in codegen.
    Run,
    /// `jet test` — needs at least one test; `run` is optional.
    Test,
    /// `jet test` with a user-owned `fn test` command adapter.
    TestOverride,
    /// `jet check` / LSP — type-check only; imported modules and library files
    /// need not define `run`.
    Check,
    /// `jet eval` — full sema type-checking, but `run` may return a non-`()`
    /// type. The entry still requires a `run` function.
    Eval,
}

#[derive(Clone)]
pub(crate) struct ModuleState {
    module_path: String,
    source: String,
    package_scope: String,
    module_alias: String,
    /// Source items retained for compile-time reflection folds. The same AST
    /// is the producer for `Type.reflect()` and direct registered fact reads;
    /// no reflection lookup table is kept beside it.
    items: Vec<crate::AST::Item>,
    /// The bundle-wide build fact snapshot, copied into each module state so
    /// the checker can fold facts without an engine or global lookup.
    build_facts: jet_foundation::Facts::BuildFactSnapshot,
    /// True only for the explicitly selected package/workspace build entry.
    /// Ordinary `fn build` names do not grant compiler-host capabilities.
    allow_compiler_api: bool,
    /// D-INTBIG1: a checked body or sema-typed constant can reach the exact
    /// `Int` runtime. Keep this fact in sema; codegen must not rediscover it
    /// by enumerating source expression shapes.
    exact_int_reachable: std::cell::Cell<bool>,
    /// D-NEVER1=C: names whose checked bodies have no returning path. This is
    /// a compiler-only bottom fact; it never becomes a public `Never` type.
    diverging_functions: std::collections::HashSet<String>,
    funcs: HashMap<String, FuncSig>,
    registry: TypeRegistry,
    consts: HashMap<String, Type>,
    imports: HashMap<String, usize>,
    /// D-NAME-WALK1=A / D-VERDICT-1867-1: foreign namespace aliases scoped
    /// to one inline module body.
    inline_foreign_imports: HashMap<(String, String), usize>,
    /// D-VERDICT-1867-1: foreign namespace aliases publicly re-exported by
    /// an inline module.
    inline_reexport_foreign: HashMap<(String, String), usize>,
    core_imports: HashMap<String, String>,
    tests: HashMap<String, Span>,
    /// D-MODEL-PACKAGE1=A: the Loader-owned ordinary `.Model` output registry.
    /// Sema consumes these neutral facts; it never imports `jet-pkg-model`.
    model_outputs: Vec<jet_foundation::AST::ModelOutputFact>,

    trait_reg: TraitRegistry,
    /// D-FACTMODEL1=A: the one erased fact registry visible to body folds.
    /// State rows and checked graphs are compile-time metadata only.
    fact_registry: jet_foundation::Facts::FactRegistry,
    policy_declarations: Vec<crate::Policy::PolicyDeclaration>,
    /// D-STRUCT-POLICY1=A: one bundle-local nominal setting table shared by
    /// marker and `apply` validation.
    callable_policy_declarations: BTreeMap<(String, String), (usize, crate::AST::UserPolicyDecl)>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    /// D-MOD2: inline code module aliases present in this file (alias → module name).
    /// `math.double(x)` resolves to `__jet_math__double(x)` when `math` is in here.
    code_modules: HashMap<String, String>,
    /// Inline module spelling -> compiler semantic identity. Ordinary modules
    /// use their module identity; generic instances use `instance:<digest>`.
    code_module_identities: HashMap<String, String>,
    /// D-MOD3: unqualified items imported via `use alias.Item` (inline modules).
    /// Maps unqualified name → mangled name (e.g. "clamp" → "__jet_math__clamp").
    unqualified: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items imported via `use alias.Item`.
    /// Maps name → (function_name, module_idx).
    unqualified_file: HashMap<String, (String, usize)>,
    /// D-CORE-USELIST1=A: unqualified Core items imported via `use core.X.[…]`.
    /// The value is the original Core member name; `core_imports` carries its
    /// module path so the call lowers through the ordinary Core method path.
    core_item_imports: HashMap<String, String>,
    /// D-MOD4: `pub use alias.Item` re-exports — items this module exposes on its
    /// own public surface even though they're defined elsewhere. Maps the
    /// exported name → (target_function_name, target_module_idx). A caller doing
    /// `thismod.Item` resolves through here to the real definition.
    reexports: HashMap<String, (String, usize)>,
    /// D-NAME-WALK1=A: unqualified imports declared inside an inline module.
    /// The first key is the inline module name; the second is the local item
    /// name. Inline bodies inherit the enclosing file's maps and overlay these
    /// entries for their own scope only.
    inline_unqualified: HashMap<(String, String), String>,
    /// D-NAME-WALK1=A: file-module items imported inside an inline module.
    inline_unqualified_file: HashMap<(String, String), (String, usize)>,
    /// D-NAME-WALK1=A: core modules imported by item name inside an inline
    /// module. Kept separate from the file-level map for scope safety.
    inline_core_imports: HashMap<(String, String), String>,
    /// D-NAME-WALK1=A: original Core member names for inline-module imports.
    inline_core_items: HashMap<(String, String), String>,
    /// D-NAME-WALK1=A: inline-module `pub use` of another inline function.
    inline_reexport_inline: HashMap<(String, String), (String, String)>,
    /// D-NAME-WALK1=A: inline-module `pub use` of a file-module function.
    inline_reexport_file: HashMap<(String, String), (String, usize)>,
    /// D-NAME-WALK1=A: inline-module `pub use` of a Core item. The first
    /// value is the Core module and the second is the original member name.
    inline_reexport_core: HashMap<(String, String), (String, String)>,
}

/// D-INTBIG1: whether a checked type carries Jet's default exact `Int`.
/// Fixed-width integers remain separate and must not pull in the exact runtime.
pub(crate) fn type_uses_default_int(ty: &Type) -> bool {
    match ty {
        Type::Int => true,
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::FixedList { elem: inner, .. }
        | Type::Tagged { inner, .. }
        | Type::InlineRange { base: inner, .. }
        | Type::Quantity { base: inner, .. } => type_uses_default_int(inner),
        Type::Map { key, value, .. }
        | Type::Result {
            ok: key,
            err: value,
        } => type_uses_default_int(key) || type_uses_default_int(value),
        Type::Tuple(fields) => fields.iter().any(|(_, field)| type_uses_default_int(field)),
        Type::Union(members) => members.iter().any(type_uses_default_int),
        Type::Fn { params, ret, .. } => {
            params.iter().any(type_uses_default_int)
                || ret.as_deref().is_some_and(type_uses_default_int)
        }
        Type::Apply { args, .. } => args.iter().any(type_uses_default_int),
        Type::IntN { .. }
        | Type::Float
        | Type::Float32
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Named(_)
        | Type::TraitObject(_)
        | Type::Measure(_) => false,
    }
}

pub(crate) struct Checker<'a> {
    funcs: &'a HashMap<String, FuncSig>,
    /// D-NEVER1=C: sema's fixed-point bottom facts. A call consults this
    /// registry before ordinary value joining; no engine infers divergence.
    diverging_functions: &'a std::collections::HashSet<String>,
    effect_facts: &'a jet_foundation::Facts::FactRegistry,
    consts: &'a HashMap<String, Type>,
    pub(crate) devtools_registry: &'a jet_foundation::AST::DevtoolsRegistry,
    registry: &'a TypeRegistry,
    plugin_interfaces: &'a PluginInterfaceRegistry,
    modules: Option<&'a [ModuleState]>,
    /// D-COMPILE-SPEED1: unqualified nominal lookup may otherwise scan every
    /// module for each expression. This cache belongs to one body checker and
    /// is rebuilt after registration, so immutable module states cannot make a
    /// cached owner stale while the checker is alive.
    nominal_owner_cache: std::cell::RefCell<HashMap<String, Option<usize>>>,
    items: &'a [crate::AST::Item],
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    core_imports: &'a HashMap<String, String>,
    /// Checked ordinary `.Model` outputs available to this module's source
    /// binding path. Kept as neutral foundation facts to preserve dependency
    /// direction.
    model_outputs: &'a [jet_foundation::AST::ModelOutputFact],
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    code_modules: &'a HashMap<String, String>,
    code_module_identities: &'a HashMap<String, String>,
    /// D-MOD3: unqualified inline-module items in scope (name → mangled name).
    unqualified: &'a HashMap<String, String>,
    /// D-MOD3: unqualified file-module items in scope (name → (fn_name, module_idx)).
    unqualified_file: &'a HashMap<String, (String, usize)>,
    /// D-CORE-USELIST1=A: unqualified Core items in scope (local name → member).
    core_item_imports: &'a HashMap<String, String>,
    /// D-NAME-WALK1=A: imports scoped to one inline-module body.
    inline_unqualified: &'a HashMap<(String, String), String>,
    inline_unqualified_file: &'a HashMap<(String, String), (String, usize)>,
    inline_module: Option<String>,
    /// D-NAME-WALK1=A: public re-exports declared inside inline modules.
    inline_reexport_inline: &'a HashMap<(String, String), (String, String)>,
    inline_reexport_file: &'a HashMap<(String, String), (String, usize)>,
    inline_reexport_core: &'a HashMap<(String, String), (String, String)>,
    inline_reexport_foreign: &'a HashMap<(String, String), usize>,
    module_path: &'a str,
    source: &'a str,
    package_scope: &'a str,
    policy_declarations: &'a [crate::Policy::PolicyDeclaration],
    callable_policy_declarations:
        &'a BTreeMap<(String, String), (usize, crate::AST::UserPolicyDecl)>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    /// D-RESOURCE-SCHEDULE1=A: aggregate access facts for this checked frame
    /// callback body. A second callback is checked as a candidate parallel
    /// system; the first callback's source order remains the reference.
    frame_schedule_systems: Vec<jet_foundation::ResourceSchedule::JetFrameOperation>,
    current_function_span: Span,
    name_ledger: &'a mut jet_foundation::Names::NameLedger,
    diags: Vec<Diagnostic>,
    /// D-SUBJECT-COHERE1=A: statement-local `#allow(lint)` facts collected
    /// while checking the following expression statement.
    statement_lint_allows: Vec<String>,
    /// D-STDLIB-SMALL1: complete replacement idioms are discovered at block
    /// entry and emitted while their first statement is checked, so a local
    /// `#allow` marker can suppress exactly that diagnostic.
    stdlib_lint_candidates: HashMap<usize, (Span, crate::Diagnostics::TextEdit)>,
    /// D-LINT-UNUSED1: source declarations whose successful body check found
    /// no read or write. The declaration identity is the sema `def_span`, so
    /// shadowed names never share a liveness fact.
    unused_bindings: Vec<UnusedBinding>,
    unused_binding_refs: HashSet<Span>,
    /// D-FACT-FLOW1: the one store of per-binding facts — declarations, flow
    /// narrowing, moves, uninitialised places and open borrow windows. Every
    /// plane joins through `FlowFacts`; nothing here keeps the last-walked path.
    flow: FlowFacts::FlowFacts,
    concrete_unit_values: Vec<HashMap<String, f64>>,
    /// Field inference reads its base only to discover its type. Suppress a
    /// partial-root move report there; the complete field place is checked first.
    suppress_partial_move_root_read: bool,
    loop_depth: usize,
    /// D-LOOP-SUBJECT1=A: active bindingless collecting-loop subjects.
    implicit_loop_subject_depth: usize,
    /// D-SUBJECT-COHERE1=A: active subject-shorthand lambda scopes.
    subject_shorthand_depth: usize,
    source_nesting: usize,
    /// D-LOOPLABEL3=A: stack of `name :: loop` names; scope, innermost last.
    loop_labels: Vec<String>,
    /// D-LOOPEVAL1: item type under each compiler-private collecting loop.
    /// Nested yielding loops push independent slots.
    collect_item_types: Vec<Option<Type>>,
    loop_value_frames: Vec<LoopValueFrame>,
    /// Flow facts captured by `break` exits, aligned with loop_value_frames.
    loop_break_flows: Vec<Vec<FlowFacts::FlowFacts>>,
    pending_loop_value: Option<(LoopValueKind, Option<String>)>,
    /// D-LOOP-STMT-ARROW1=C: the body currently being checked came from a
    /// statement-position loop arrow. Its ordinary expression statement may
    /// compute a value, but that value is intentionally discarded.
    arrow_loop_body: bool,
    last_loop_result_type: Option<Type>,
    /// D-EFF1: effects this function body reaches directly (Core calls, impure
    /// builtins). Accumulated during the walk; rolled into the per-function
    /// `EffectSummary` after the body is checked.
    fx_direct: EffectSet,
    /// First source span that introduced each direct effect, for inspect/LSP
    /// provenance. Duplicate uses keep the earliest stable witness.
    fx_direct_spans: HashMap<String, Span>,
    /// D-EFFECT-LAMBDA1: active lambda rows receive the same effect events as
    /// the enclosing body, preserving nested closure facts without a second
    /// analysis walk.
    lambda_effect_stack: Vec<Effects::LambdaEffectAccum>,
    /// D-EFF1: user functions called in this body (call-graph edges for the
    /// whole-program transitive fixpoint).
    fx_edges: BTreeSet<String>,
    /// D-EFF1: a foreign (`extern`) call was reached — the body's effects are
    /// the maximal set (an un-inspectable body may do anything).
    fx_maximal: bool,
    /// First source span that forced the row maximal.
    fx_maximal_span: Option<Span>,
    /// D-EFF1: stack of active `#FX(…)` regions, innermost last. Every effect
    /// or edge recorded while one is open is also added to it (and all enclosing
    /// regions) so the region's own effect set can be checked against its caps.
    region_stack: Vec<RegionAccum>,
    /// D-EFF1: completed `#FX(…)` regions in this body, rolled into the
    /// `EffectSummary` for the post-pass E0712 check.
    fx_regions: Vec<RegionSummary>,
    /// D-AUTHORITY-RECEIPT1: source facts for Authority handles delegated to
    /// approved Core boundary consumers.
    fx_authority_delegations: Vec<Effects::AuthorityDelegation>,
    /// D-EFF2: callback-bound obligations recorded at higher-order call sites
    /// where the function-typed parameter carries a `#Pure`/`#(…)` bound. Rolled
    /// into the `EffectSummary` for the post-pass E0747 check.
    fx_callback_obligations: Vec<CallbackObligation>,
    /// D-AUTODIFF1: named differentiated functions whose solved effect rows
    /// must be checked after the bundle effect graph closes.
    fx_autodiff_obligations: Vec<Effects::AutodiffObligation>,
    /// D-AUTODIFF1: compute consumers used by this function body.
    fx_compute_calls: Vec<Effects::ComputeCallFact>,
    /// D-AUTODIFF1: a `?? panic` directly attached to a Tensor-returning
    /// `core.compute` operation is a checked domain-failure boundary. Keep its
    /// provenance separate from arbitrary/direct panic sites so autodiff can
    /// admit only the former without erasing the D-NOPANIC effect fact.
    fx_autodiff_safe_panic: bool,
    fx_autodiff_unsafe_panic: bool,
    autodiff_safe_panic_context: bool,
    /// D-CRYPTO-DIAG1: compiler-known crypto facts wait for the function's
    /// syntax/type/effect phases to finish before becoming diagnostics.
    fx_pending_diagnostics: Vec<Diagnostic>,
    /// D-MEM-FACTS1 direct, source-spanned memory evidence accumulated beside
    /// effects so both policies share one pre-TIR call graph.
    fx_memory_events: Vec<MemoryFacts::MemoryEvent>,
    fx_memory_open: Vec<MemoryFacts::OpenMemoryDispatch>,
    memory_policy_stack: Vec<MemoryFacts::MemoryPolicyRegion>,
    /// D-WRAP-SCOPE1=A: active lexical fixed-width arithmetic modes. The last
    /// entry is the innermost block; function markers seed the stack once per
    /// body and never cross a call boundary.
    arithmetic_policy_stack: Vec<crate::AST::ArithmeticPolicyFact>,
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
    /// D-CONC-SHARE1=A (card #1561): nesting depth of transaction blocks the
    /// *user* opened. The D-TXN2 effect wall (E0746) keys off this, not
    /// `txn_depth`, so a compiler-synthesized one-statement transaction — the
    /// ordered-commit route for a plain statement touching several `Shared`
    /// cells — carries the commit plane without rejecting effects the author
    /// never put inside a transaction.
    txn_wall_depth: usize,
    /// D-TEST-WORLD1=A: depth of a `testing.world` callback. Calls reached
    /// here must use a scoped provider; external effects cannot be replayed.
    deterministic_world_depth: usize,
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
    /// Disables readable Core prelude resolution for this body.
    no_prelude: bool,
    /// D-PREPOST1: true while type-checking a `#Pre` clause's condition —
    /// `result` isn't bound yet at function entry, so a reference to it here
    /// is E0144 instead of the normal "undefined name" error.
    in_pre_clause: bool,
    /// D-FAIL-BIND1=A: `None` means no fallback is being checked; `Some(true)`
    /// is a fallible fallback with an ambient `err`; `Some(false)` has no
    /// failure report and rejects the spelling with E0408 or E0409.
    pub(crate) fallback_has_err: Option<bool>,
    /// D-FAILURE-FOUNDATION1=A: number of root inference calls that own their
    /// carrier explicitly. The token is consumed by that one `infer` call;
    /// recursive child expressions remain ordinary propagation sites.
    pub(crate) failure_auto_root_suppression: usize,
    /// D-FAILURE-FOUNDATION1=A: ambient carrier context retained for checks
    /// that need to distinguish an explicit carrier probe from ordinary value
    /// inference. Automatic propagation itself uses the root token above so
    /// nested call arguments can still propagate.
    pub(crate) failure_auto_depth: usize,
    /// D-CHOOSE-TEST1=A: distinguishes a pure pattern miss from an absent
    /// Optional while `fallback_has_err == Some(false)`.
    pub(crate) fallback_is_shape_miss: bool,
    /// True while inferring a comptime binding's RHS or inside a comptime
    /// context (D-META-STAGE1=B).
    in_comptime: bool,
    /// True only while checking the selected package/workspace `fn build`.
    /// This is passed by the Driver's build authority, never inferred from a
    /// user-chosen function name.
    compiler_api_allowed: bool,
    ret: Option<Type>,
    fn_name: String,
    /// Compiler-generated bodies already spell their Result inspection or
    /// propagation explicitly. Do not apply source-level transparent failure
    /// propagation to those synthetic ASTs.
    compiler_generated: bool,
    /// Compiler-generated trait protocol bodies implement a raw Rust ABI. This
    /// is distinct from `compiler_generated`: policy wrappers and validation
    /// builders remain ordinary Jet callables with the shared failure carrier.
    raw_protocol_return: bool,
    /// Source-declared return type. The checker keeps `ret` as the effective
    /// failure carrier, while generator checks use this declaration directly.
    declared_return_type: Option<Type>,
    current_return_type_span: Option<Span>,
    /// Canonical caller-visible parameter order; excludes `self`.
    current_param_names: Vec<String>,
    /// Compiler-private names in inserted defaults resolve to their declaration
    /// slot type here. This map is populated by the shared call binder.
    pub(crate) binder_ref_types: HashMap<String, Type>,
    /// Context type for bare `null` (E0308).
    expected_type: Option<Type>,
    /// D-INTBIG1: typed reachability fact for this checked/generated body.
    uses_exact_int: bool,
    /// The written bounded-arithmetic call currently checking its argument.
    /// This is a sema-only context; no gate marker reaches TIR or codegen.
    knowledge_gate: Option<KnowledgeGate>,
    /// Collections currently read by an active `for x in xs` loop (E0507).
    iter_borrowed: HashSet<String>,
    /// Card #1440: NoElse-terminated dispatch chains already coverage-checked
    /// (chain span starts) — every level shares one span, the outermost wins.
    noelse_chains_checked: HashSet<usize>,
    /// D-RESULT-DECON2=B: the parser keeps one receiver in both fixed Result
    /// pattern tests. Cache its checked type and elaborated expression by source
    /// span so sema checks effects and ownership once while both lowered copies
    /// retain the same resolved call metadata.
    result_handler_subject_types: HashMap<usize, (Type, Expr)>,
    /// Loop bindings that lend one `ViewMut<T>` element from a collection.
    /// These values may edit during the iteration but may not be retained.
    lending_view_loop_vars: HashSet<String>,
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
    /// True while inferring an expression that the generated Rust will only
    /// borrow (method receivers, field/index bases, lvalues). Field reads in
    /// borrow position must NOT be rewritten to `.clone()`.
    borrow_ctx: bool,
    /// D-MEM-COPYSEM1: depth of an owning value-if inference. Only the
    /// value-if arm boundary consults this marker to route its tail through
    /// the owning-position copy rule.
    owning_if_value_depth: usize,
    /// `Fixed.new` / `Fixed.over` must be the whole initializer of one lexical
    /// binding so codegen can place and lifetime-order its inline backing.
    allow_fixed_constructor: bool,
    /// D-MEM-COPYSEM1: true while inferring a string-view fact for a direct
    /// non-owning operation, the operand of `~`, or a read-only view entering
    /// an owning destination. The general `Expr::Ident` arm reports E2307
    /// when this is false and `copies: .Explicit` has disabled materialization.
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
    /// M11: when true, lambda is being passed to canonical `task` — stricter capture rules (E1101).
    is_task_spawn: bool,
    /// D-CONC-SPAWN1: set by `infer_try` when a `?` in the CURRENT lambda body
    /// is typed against the enclosing fallible return. `infer_lambda` scopes it
    /// per lambda and harvests it into `LambdaMeta::fallible_propagation`.
    task_body_propagates: bool,
    /// True while an open callback return is being inferred. A fallible callee
    /// in this position supplies the callback's own Result carrier; it must not
    /// be auto-unwrapped against the enclosing function's return row.
    failure_carrier_inference: bool,
    /// The carrier discovered for the current open callback, if its body has
    /// reached a fallible callee.
    failure_carrier: Option<Type>,
    /// Source-nesting depth of an untyped binding initializer. That root is an
    /// ordinary value position, so a fallible call auto-propagates even when
    /// the enclosing function returns Result. Nested inference must not inherit
    /// this exception.
    ordinary_binding_root_depth: Option<usize>,
    /// True while the root expression is being checked as a statement. A
    /// dispatch nested in a value expression must keep value-tail checking;
    /// only the statement root may make a braced arm's tail Unit.
    statement_expr_inference: bool,
    /// Source-nesting depth at which the root expression passed to
    /// `infer_statement_expr` is being inferred. Nested value expressions
    /// (for example an `if` passed to `print`) must not inherit the
    /// statement root's Unit result policy.
    statement_expr_root_depth: Option<usize>,
    /// HTTP handlers are retained by the server and invoked on request
    /// workers, so captured state must cross the `Send + Sync` boundary.
    http_handler_depth: usize,
    /// True while checking the callback stored by `core.sys.on_interrupt`.
    /// This boundary retains a callback for asynchronous signal delivery and
    /// therefore needs stricter capture facts than an ordinary higher-order call.
    interrupt_callback_depth: usize,
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
    /// D-DETACH1: task names whose spawn lambda captured a `view` or
    /// task-group-scoped borrow. At `.detach()`, if the task is in this set,
    /// E1106 fires instead of E1103.
    view_borrow_escape_tasks: HashSet<String>,
    /// D-DETACH1: the binding name currently being elaborated (set at check_binding
    /// entry, cleared after). Used to record view-capturing task names.
    current_binding_name: Option<String>,
    /// Binding owned by the task spawn whose lambda is being checked. Nested
    /// task bodies must not reuse that outer name for group auto-join tracking,
    /// but sendability diagnostics still need the outer detach target.
    task_spawn_binding_name: Option<String>,
    /// M8: binding name when checking `f :: (…) => …` (E0804 self-call).
    lambda_binding: Option<String>,
    /// Names mutably captured by an escaping lambda still in scope (E0204).
    lambda_mut_borrow_stack: Vec<HashSet<String>>,
    /// M9: generic/trait metadata for this program.
    trait_reg: &'a TraitRegistry,
    /// M9.5: local comptime declaration context. This retains every declared
    /// body for semantic lookup; execution uses only `ct_checked_funcs`.
    ct_funcs: &'a HashMap<String, Func>,
    /// Function bodies that have completed the semantic checker and are safe
    /// to hand to the canonical comptime MIR evaluator.
    ct_checked_funcs: &'a HashMap<String, Func>,
    /// Source item context supplied to the checked evaluator. It may retain
    /// declarations whose executable method bodies are filtered by
    /// `ct_checked_funcs`.
    ct_items: &'a [crate::AST::Item],
    ct_externs: &'a HashSet<String>,
    ct_base_dir: &'a std::path::Path,
    ct_globals: &'a HashMap<String, crate::Comptime::CtValue>,
    ct_scopes: Vec<HashMap<String, crate::Comptime::CtValue>>,
    /// During the targeted comptime staging pass, type-check bodies without
    /// executing any compile-time expression. The ordinary pass evaluates
    /// mandatory and opportunistic expressions after their callees are safe.
    defer_ct_evaluation: bool,
    /// Active generic type parameters while checking a generic item.
    type_param_scope: Vec<crate::AST::TypeParam>,
    /// E2-M15: reject OS-dependent APIs on a selected no-OS target (E3301).
    no_os: bool,
    /// D-CTEFFECT1: `--gate impure=allow` was passed — `#Impure` blocks may execute
    /// Tier-2 ambient comptime effects (FS/Env/Exec/IO) at compile time.
    gates: crate::Policy::GateSet,
    /// D-CTEFFECT1: nesting depth of `#Impure` blocks currently being checked.
    /// Passed as `initial_impure_depth` to comptime evaluation of bindings
    /// inside, so the interpreter starts with the gate already open.
    ct_impure_depth: usize,
    /// D-CTEFFECT1 Tier-1: embed_file/embed_bytes inputs accumulated while
    /// checking this function body. Drained into `CompileOutput.comptime_inputs`
    /// by Bundle.rs after the full bundle is checked.
    pub(super) ct_embed_inputs: Vec<crate::AST::ComptimeInput>,
    /// D-WHEN2 (ratified 2026-06-19): when true, we are inside a dropped
    /// `@if` arm — name-resolution runs normally (so unknown-name
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
    /// D-CONC-SPAWN1=D: stack of active `task.group` scopes (innermost last).
    taskgroup_stack: Vec<TaskGroupCtx>,
    /// True while inferring a child body passed to `task.group` — suppresses L1101
    /// (the group owns the handle until scope exit or an explicit join).
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

/// Mutable checker state that must not cross an erased `#Off`/`#DebugOnly`
/// body boundary. Diagnostics are deliberately not included: erased source is
/// still type-checked and its real diagnostics remain visible.
struct ErasedScopeSnapshot {
    nominal_owner_cache: HashMap<String, Option<usize>>,
    inline_module: Option<String>,
    rule_facts: Vec<crate::AST::AppliedRuleApplication>,
    frame_schedule_systems: Vec<jet_foundation::ResourceSchedule::JetFrameOperation>,
    current_function_span: Span,
    name_ledger: jet_foundation::Names::NameLedger,
    statement_lint_allows: Vec<String>,
    stdlib_lint_candidates: HashMap<usize, (Span, crate::Diagnostics::TextEdit)>,
    unused_bindings: Vec<UnusedBinding>,
    unused_binding_refs: HashSet<Span>,
    flow: FlowFacts::FlowFacts,
    concrete_unit_values: Vec<HashMap<String, f64>>,
    suppress_partial_move_root_read: bool,
    loop_depth: usize,
    implicit_loop_subject_depth: usize,
    subject_shorthand_depth: usize,
    source_nesting: usize,
    loop_labels: Vec<String>,
    collect_item_types: Vec<Option<Type>>,
    loop_value_frames: Vec<LoopValueFrame>,
    loop_break_flows: Vec<Vec<FlowFacts::FlowFacts>>,
    pending_loop_value: Option<(LoopValueKind, Option<String>)>,
    arrow_loop_body: bool,
    last_loop_result_type: Option<Type>,
    fx_direct: EffectSet,
    fx_direct_spans: HashMap<String, Span>,
    lambda_effect_stack: Vec<Effects::LambdaEffectAccum>,
    fx_edges: BTreeSet<String>,
    fx_maximal: bool,
    fx_maximal_span: Option<Span>,
    region_stack: Vec<RegionAccum>,
    fx_regions: Vec<RegionSummary>,
    fx_authority_delegations: Vec<Effects::AuthorityDelegation>,
    fx_callback_obligations: Vec<CallbackObligation>,
    fx_autodiff_obligations: Vec<Effects::AutodiffObligation>,
    fx_compute_calls: Vec<Effects::ComputeCallFact>,
    fx_autodiff_safe_panic: bool,
    fx_autodiff_unsafe_panic: bool,
    autodiff_safe_panic_context: bool,
    fx_pending_diagnostics: Vec<Diagnostic>,
    fx_memory_events: Vec<MemoryFacts::MemoryEvent>,
    fx_memory_open: Vec<MemoryFacts::OpenMemoryDispatch>,
    memory_policy_stack: Vec<MemoryFacts::MemoryPolicyRegion>,
    arithmetic_policy_stack: Vec<crate::AST::ArithmeticPolicyFact>,
    fx_memory_regions: Vec<MemoryFacts::MemoryPolicyRegion>,
    fx_memory_unbounded_control: Vec<Span>,
    fx_memory_calls: Vec<MemoryFacts::MemoryCall>,
    memory_control_multiplier: Option<u64>,
    txn_depth: usize,
    txn_wall_depth: usize,
    deterministic_world_depth: usize,
    det_suppress: usize,
    context_depth: usize,
    context_allocator_active: bool,
    in_unsafe: bool,
    suppress_must_use: bool,
    in_pure: bool,
    no_prelude: bool,
    in_pre_clause: bool,
    fallback_has_err: Option<bool>,
    failure_auto_root_suppression: usize,
    failure_auto_depth: usize,
    fallback_is_shape_miss: bool,
    in_comptime: bool,
    compiler_api_allowed: bool,
    ret: Option<Type>,
    fn_name: String,
    compiler_generated: bool,
    raw_protocol_return: bool,
    declared_return_type: Option<Type>,
    current_return_type_span: Option<Span>,
    current_param_names: Vec<String>,
    binder_ref_types: HashMap<String, Type>,
    expected_type: Option<Type>,
    uses_exact_int: bool,
    knowledge_gate: Option<KnowledgeGate>,
    iter_borrowed: HashSet<String>,
    noelse_chains_checked: HashSet<usize>,
    result_handler_subject_types: HashMap<usize, (Type, Expr)>,
    lending_view_loop_vars: HashSet<String>,
    return_view_provenance: Option<crate::AST::ViewProvenanceMap>,
    views_used_in_stmt: HashSet<String>,
    scoped_loan_read_reported: bool,
    call_access_frames: Vec<CallAccessFrame>,
    borrow_ctx: bool,
    owning_if_value_depth: usize,
    allow_fixed_constructor: bool,
    allow_string_view_read: bool,
    lambda_escapes: bool,
    in_lambda_body: bool,
    inferred_lambda_mut_captures: HashSet<String>,
    lambda_params_are_lending_views: bool,
    is_task_spawn: bool,
    task_body_propagates: bool,
    failure_carrier_inference: bool,
    failure_carrier: Option<Type>,
    ordinary_binding_root_depth: Option<usize>,
    statement_expr_inference: bool,
    statement_expr_root_depth: Option<usize>,
    http_handler_depth: usize,
    interrupt_callback_depth: usize,
    lambda_param_mutable: bool,
    lambda_param_is_secret_loan: bool,
    view_capture_tasks: HashSet<String>,
    reactive_upgrades: Vec<String>,
    reactive_upgrade_names: HashSet<String>,
    view_borrow_escape_tasks: HashSet<String>,
    current_binding_name: Option<String>,
    task_spawn_binding_name: Option<String>,
    lambda_binding: Option<String>,
    lambda_mut_borrow_stack: Vec<HashSet<String>>,
    ct_scopes: Vec<HashMap<String, crate::Comptime::CtValue>>,
    defer_ct_evaluation: bool,
    type_param_scope: Vec<crate::AST::TypeParam>,
    ct_impure_depth: usize,
    ct_embed_inputs: Vec<crate::AST::ComptimeInput>,
    in_dropped_comptime_arm: bool,
    stmt_tail_ptr: *const crate::AST::Stmt,
    stmt_tail_len: usize,
    liveness_frames: Vec<(*const crate::AST::Stmt, usize)>,
    taskgroup_stack: Vec<TaskGroupCtx>,
    in_taskgroup_spawn: bool,
    inline_addr_taken: HashSet<String>,
    /// D-DX-PLUGIN1=D: publication facts live in the shared per-module
    /// TypeRegistry, so erased scopes snapshot and restore them alongside the
    /// checker-local fact planes.
    devtools_publications: Vec<jet_foundation::AST::DevtoolsFactPublication>,

}

impl<'a> Checker<'a> {
    fn snapshot_erased_scope(&self) -> ErasedScopeSnapshot {
        ErasedScopeSnapshot {
            nominal_owner_cache: self.nominal_owner_cache.borrow().clone(),
            inline_module: self.inline_module.clone(),
            rule_facts: self.rule_facts.clone(),
            frame_schedule_systems: self.frame_schedule_systems.clone(),
            current_function_span: self.current_function_span,
            name_ledger: self.name_ledger.clone(),
            statement_lint_allows: self.statement_lint_allows.clone(),
            stdlib_lint_candidates: self.stdlib_lint_candidates.clone(),
            unused_bindings: self.unused_bindings.clone(),
            unused_binding_refs: self.unused_binding_refs.clone(),
            flow: self.flow.clone(),
            concrete_unit_values: self.concrete_unit_values.clone(),
            suppress_partial_move_root_read: self.suppress_partial_move_root_read,
            loop_depth: self.loop_depth,
            implicit_loop_subject_depth: self.implicit_loop_subject_depth,
            subject_shorthand_depth: self.subject_shorthand_depth,
            source_nesting: self.source_nesting,
            loop_labels: self.loop_labels.clone(),
            collect_item_types: self.collect_item_types.clone(),
            loop_value_frames: self.loop_value_frames.clone(),
            loop_break_flows: self.loop_break_flows.clone(),
            pending_loop_value: self.pending_loop_value.clone(),
            arrow_loop_body: self.arrow_loop_body,
            last_loop_result_type: self.last_loop_result_type.clone(),
            fx_direct: self.fx_direct.clone(),
            fx_direct_spans: self.fx_direct_spans.clone(),
            lambda_effect_stack: self.lambda_effect_stack.clone(),
            fx_edges: self.fx_edges.clone(),
            fx_maximal: self.fx_maximal,
            fx_maximal_span: self.fx_maximal_span,
            region_stack: self.region_stack.clone(),
            fx_regions: self.fx_regions.clone(),
            fx_authority_delegations: self.fx_authority_delegations.clone(),
            fx_callback_obligations: self.fx_callback_obligations.clone(),
            fx_autodiff_obligations: self.fx_autodiff_obligations.clone(),
            fx_compute_calls: self.fx_compute_calls.clone(),
            fx_autodiff_safe_panic: self.fx_autodiff_safe_panic,
            fx_autodiff_unsafe_panic: self.fx_autodiff_unsafe_panic,
            autodiff_safe_panic_context: self.autodiff_safe_panic_context,
            fx_pending_diagnostics: self.fx_pending_diagnostics.clone(),
            fx_memory_events: self.fx_memory_events.clone(),
            fx_memory_open: self.fx_memory_open.clone(),
            memory_policy_stack: self.memory_policy_stack.clone(),
            arithmetic_policy_stack: self.arithmetic_policy_stack.clone(),
            fx_memory_regions: self.fx_memory_regions.clone(),
            fx_memory_unbounded_control: self.fx_memory_unbounded_control.clone(),
            fx_memory_calls: self.fx_memory_calls.clone(),
            memory_control_multiplier: self.memory_control_multiplier,
            txn_depth: self.txn_depth,
            txn_wall_depth: self.txn_wall_depth,
            deterministic_world_depth: self.deterministic_world_depth,
            det_suppress: self.det_suppress,
            context_depth: self.context_depth,
            context_allocator_active: self.context_allocator_active,
            in_unsafe: self.in_unsafe,
            suppress_must_use: self.suppress_must_use,
            in_pure: self.in_pure,
            no_prelude: self.no_prelude,
            in_pre_clause: self.in_pre_clause,
            fallback_has_err: self.fallback_has_err,
            failure_auto_root_suppression: self.failure_auto_root_suppression,
            failure_auto_depth: self.failure_auto_depth,
            fallback_is_shape_miss: self.fallback_is_shape_miss,
            in_comptime: self.in_comptime,
            compiler_api_allowed: self.compiler_api_allowed,
            ret: self.ret.clone(),
            fn_name: self.fn_name.clone(),
            compiler_generated: self.compiler_generated,
            raw_protocol_return: self.raw_protocol_return,
            declared_return_type: self.declared_return_type.clone(),
            current_return_type_span: self.current_return_type_span,
            current_param_names: self.current_param_names.clone(),
            binder_ref_types: self.binder_ref_types.clone(),
            expected_type: self.expected_type.clone(),
            uses_exact_int: self.uses_exact_int,
            knowledge_gate: self.knowledge_gate,
            iter_borrowed: self.iter_borrowed.clone(),
            noelse_chains_checked: self.noelse_chains_checked.clone(),
            result_handler_subject_types: self.result_handler_subject_types.clone(),
            lending_view_loop_vars: self.lending_view_loop_vars.clone(),
            return_view_provenance: self.return_view_provenance.clone(),
            views_used_in_stmt: self.views_used_in_stmt.clone(),
            scoped_loan_read_reported: self.scoped_loan_read_reported,
            call_access_frames: self.call_access_frames.clone(),
            borrow_ctx: self.borrow_ctx,
            owning_if_value_depth: self.owning_if_value_depth,
            allow_fixed_constructor: self.allow_fixed_constructor,
            allow_string_view_read: self.allow_string_view_read,
            lambda_escapes: self.lambda_escapes,
            in_lambda_body: self.in_lambda_body,
            inferred_lambda_mut_captures: self.inferred_lambda_mut_captures.clone(),
            lambda_params_are_lending_views: self.lambda_params_are_lending_views,
            is_task_spawn: self.is_task_spawn,
            task_body_propagates: self.task_body_propagates,
            failure_carrier_inference: self.failure_carrier_inference,
            failure_carrier: self.failure_carrier.clone(),
            ordinary_binding_root_depth: self.ordinary_binding_root_depth,
            statement_expr_inference: self.statement_expr_inference,
            statement_expr_root_depth: self.statement_expr_root_depth,
            http_handler_depth: self.http_handler_depth,
            interrupt_callback_depth: self.interrupt_callback_depth,
            lambda_param_mutable: self.lambda_param_mutable,
            lambda_param_is_secret_loan: self.lambda_param_is_secret_loan,
            view_capture_tasks: self.view_capture_tasks.clone(),
            reactive_upgrades: self.reactive_upgrades.clone(),
            reactive_upgrade_names: self.reactive_upgrade_names.clone(),
            view_borrow_escape_tasks: self.view_borrow_escape_tasks.clone(),
            current_binding_name: self.current_binding_name.clone(),
            task_spawn_binding_name: self.task_spawn_binding_name.clone(),
            lambda_binding: self.lambda_binding.clone(),
            lambda_mut_borrow_stack: self.lambda_mut_borrow_stack.clone(),
            ct_scopes: self.ct_scopes.clone(),
            defer_ct_evaluation: self.defer_ct_evaluation,
            type_param_scope: self.type_param_scope.clone(),
            ct_impure_depth: self.ct_impure_depth,
            ct_embed_inputs: self.ct_embed_inputs.clone(),
            in_dropped_comptime_arm: self.in_dropped_comptime_arm,
            stmt_tail_ptr: self.stmt_tail_ptr,
            stmt_tail_len: self.stmt_tail_len,
            liveness_frames: self.liveness_frames.clone(),
            taskgroup_stack: self.taskgroup_stack.clone(),
            in_taskgroup_spawn: self.in_taskgroup_spawn,
            inline_addr_taken: self.inline_addr_taken.clone(),
            devtools_publications: self.registry.devtools_publications(),

        }
    }

    fn restore_erased_scope(&mut self, snapshot: ErasedScopeSnapshot) {
        *self.nominal_owner_cache.borrow_mut() = snapshot.nominal_owner_cache;
        self.inline_module = snapshot.inline_module;
        self.rule_facts = snapshot.rule_facts;
        self.frame_schedule_systems = snapshot.frame_schedule_systems;
        self.current_function_span = snapshot.current_function_span;
        *self.name_ledger = snapshot.name_ledger;
        self.statement_lint_allows = snapshot.statement_lint_allows;
        self.stdlib_lint_candidates = snapshot.stdlib_lint_candidates;
        self.unused_bindings = snapshot.unused_bindings;
        self.unused_binding_refs = snapshot.unused_binding_refs;
        self.flow = snapshot.flow;
        self.concrete_unit_values = snapshot.concrete_unit_values;
        self.suppress_partial_move_root_read = snapshot.suppress_partial_move_root_read;
        self.loop_depth = snapshot.loop_depth;
        self.implicit_loop_subject_depth = snapshot.implicit_loop_subject_depth;
        self.subject_shorthand_depth = snapshot.subject_shorthand_depth;
        self.source_nesting = snapshot.source_nesting;
        self.loop_labels = snapshot.loop_labels;
        self.collect_item_types = snapshot.collect_item_types;
        self.loop_value_frames = snapshot.loop_value_frames;
        self.loop_break_flows = snapshot.loop_break_flows;
        self.pending_loop_value = snapshot.pending_loop_value;
        self.arrow_loop_body = snapshot.arrow_loop_body;
        self.last_loop_result_type = snapshot.last_loop_result_type;
        self.fx_direct = snapshot.fx_direct;
        self.fx_direct_spans = snapshot.fx_direct_spans;
        self.lambda_effect_stack = snapshot.lambda_effect_stack;
        self.fx_edges = snapshot.fx_edges;
        self.fx_maximal = snapshot.fx_maximal;
        self.fx_maximal_span = snapshot.fx_maximal_span;
        self.region_stack = snapshot.region_stack;
        self.fx_regions = snapshot.fx_regions;
        self.fx_authority_delegations = snapshot.fx_authority_delegations;
        self.fx_callback_obligations = snapshot.fx_callback_obligations;
        self.fx_autodiff_obligations = snapshot.fx_autodiff_obligations;
        self.fx_compute_calls = snapshot.fx_compute_calls;
        self.fx_autodiff_safe_panic = snapshot.fx_autodiff_safe_panic;
        self.fx_autodiff_unsafe_panic = snapshot.fx_autodiff_unsafe_panic;
        self.autodiff_safe_panic_context = snapshot.autodiff_safe_panic_context;
        self.fx_pending_diagnostics = snapshot.fx_pending_diagnostics;
        self.fx_memory_events = snapshot.fx_memory_events;
        self.fx_memory_open = snapshot.fx_memory_open;
        self.memory_policy_stack = snapshot.memory_policy_stack;
        self.arithmetic_policy_stack = snapshot.arithmetic_policy_stack;
        self.fx_memory_regions = snapshot.fx_memory_regions;
        self.fx_memory_unbounded_control = snapshot.fx_memory_unbounded_control;
        self.fx_memory_calls = snapshot.fx_memory_calls;
        self.memory_control_multiplier = snapshot.memory_control_multiplier;
        self.txn_depth = snapshot.txn_depth;
        self.txn_wall_depth = snapshot.txn_wall_depth;
        self.deterministic_world_depth = snapshot.deterministic_world_depth;
        self.det_suppress = snapshot.det_suppress;
        self.context_depth = snapshot.context_depth;
        self.context_allocator_active = snapshot.context_allocator_active;
        self.in_unsafe = snapshot.in_unsafe;
        self.suppress_must_use = snapshot.suppress_must_use;
        self.in_pure = snapshot.in_pure;
        self.no_prelude = snapshot.no_prelude;
        self.in_pre_clause = snapshot.in_pre_clause;
        self.fallback_has_err = snapshot.fallback_has_err;
        self.failure_auto_root_suppression = snapshot.failure_auto_root_suppression;
        self.failure_auto_depth = snapshot.failure_auto_depth;
        self.fallback_is_shape_miss = snapshot.fallback_is_shape_miss;
        self.in_comptime = snapshot.in_comptime;
        self.compiler_api_allowed = snapshot.compiler_api_allowed;
        self.ret = snapshot.ret;
        self.fn_name = snapshot.fn_name;
        self.compiler_generated = snapshot.compiler_generated;
        self.raw_protocol_return = snapshot.raw_protocol_return;
        self.declared_return_type = snapshot.declared_return_type;
        self.current_return_type_span = snapshot.current_return_type_span;
        self.current_param_names = snapshot.current_param_names;
        self.binder_ref_types = snapshot.binder_ref_types;
        self.expected_type = snapshot.expected_type;
        self.uses_exact_int = snapshot.uses_exact_int;
        self.knowledge_gate = snapshot.knowledge_gate;
        self.iter_borrowed = snapshot.iter_borrowed;
        self.noelse_chains_checked = snapshot.noelse_chains_checked;
        self.result_handler_subject_types = snapshot.result_handler_subject_types;
        self.lending_view_loop_vars = snapshot.lending_view_loop_vars;
        self.return_view_provenance = snapshot.return_view_provenance;
        self.views_used_in_stmt = snapshot.views_used_in_stmt;
        self.scoped_loan_read_reported = snapshot.scoped_loan_read_reported;
        self.call_access_frames = snapshot.call_access_frames;
        self.borrow_ctx = snapshot.borrow_ctx;
        self.owning_if_value_depth = snapshot.owning_if_value_depth;
        self.allow_fixed_constructor = snapshot.allow_fixed_constructor;
        self.allow_string_view_read = snapshot.allow_string_view_read;
        self.lambda_escapes = snapshot.lambda_escapes;
        self.in_lambda_body = snapshot.in_lambda_body;
        self.inferred_lambda_mut_captures = snapshot.inferred_lambda_mut_captures;
        self.lambda_params_are_lending_views = snapshot.lambda_params_are_lending_views;
        self.is_task_spawn = snapshot.is_task_spawn;
        self.task_body_propagates = snapshot.task_body_propagates;
        self.failure_carrier_inference = snapshot.failure_carrier_inference;
        self.failure_carrier = snapshot.failure_carrier;
        self.ordinary_binding_root_depth = snapshot.ordinary_binding_root_depth;
        self.statement_expr_inference = snapshot.statement_expr_inference;
        self.statement_expr_root_depth = snapshot.statement_expr_root_depth;
        self.http_handler_depth = snapshot.http_handler_depth;
        self.interrupt_callback_depth = snapshot.interrupt_callback_depth;
        self.lambda_param_mutable = snapshot.lambda_param_mutable;
        self.lambda_param_is_secret_loan = snapshot.lambda_param_is_secret_loan;
        self.view_capture_tasks = snapshot.view_capture_tasks;
        self.reactive_upgrades = snapshot.reactive_upgrades;
        self.reactive_upgrade_names = snapshot.reactive_upgrade_names;
        self.view_borrow_escape_tasks = snapshot.view_borrow_escape_tasks;
        self.current_binding_name = snapshot.current_binding_name;
        self.task_spawn_binding_name = snapshot.task_spawn_binding_name;
        self.lambda_binding = snapshot.lambda_binding;
        self.lambda_mut_borrow_stack = snapshot.lambda_mut_borrow_stack;
        self.ct_scopes = snapshot.ct_scopes;
        self.defer_ct_evaluation = snapshot.defer_ct_evaluation;
        self.type_param_scope = snapshot.type_param_scope;
        self.ct_impure_depth = snapshot.ct_impure_depth;
        self.ct_embed_inputs = snapshot.ct_embed_inputs;
        self.in_dropped_comptime_arm = snapshot.in_dropped_comptime_arm;
        self.stmt_tail_ptr = snapshot.stmt_tail_ptr;
        self.stmt_tail_len = snapshot.stmt_tail_len;
        self.liveness_frames = snapshot.liveness_frames;
        self.taskgroup_stack = snapshot.taskgroup_stack;
        self.in_taskgroup_spawn = snapshot.in_taskgroup_spawn;
        self.inline_addr_taken = snapshot.inline_addr_taken;
        self.registry
            .restore_devtools_publications(snapshot.devtools_publications);
    }

    pub(crate) fn with_erased_scope<T>(
        &mut self,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let snapshot = self.snapshot_erased_scope();
        let result = check(self);
        self.restore_erased_scope(snapshot);
        result
    }

}

impl<'a> Checker<'a> {
    pub(crate) fn with_knowledge_gate<T>(
        &mut self,
        gate: KnowledgeGate,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.knowledge_gate.replace(gate);
        let result = check(self);
        self.knowledge_gate = previous;
        result
    }

    pub(crate) fn knowledge_gate_allows(&self, plane: KnowledgePlane) -> bool {
        self.knowledge_gate
            .is_some_and(|gate| KnowledgeLoss::allows_gate(plane, gate))
    }

    /// Apply the one knowledge law at a sema boundary. Callers identify the
    /// existing written operation that can make the move explicit; this
    /// method owns the shared diagnostic and keeps codegen out of the policy.
    pub(crate) fn require_knowledge_gate(
        &mut self,
        plane: KnowledgePlane,
        gate: KnowledgeGate,
        span: Span,
    ) {
        if !self.knowledge_gate_allows(plane) {
            self.diags
                .push(KnowledgeLoss::diagnostic(plane, gate.spelling(), span));
        }
    }

    pub(crate) fn register_binder_refs(&mut self, args: &[crate::AST::CallArg]) {
        for arg in args {
            for (name, _, ty) in &arg.flags.binder_refs {
                self.binder_ref_types.insert(name.clone(), ty.clone());
            }
        }
    }

    /// Project a resolved source name through the sema-owned name ledger.
    /// Diagnostics keep a leaf while it is unique and use the canonical path
    /// when another visible declaration makes the leaf ambiguous.
    pub(crate) fn display_type_name(&self, name: &str, resolved_module: Option<usize>) -> String {
        self.name_ledger
            .display_path(self.module_idx, name, resolved_module)
            .unwrap_or_else(|| name.to_string())
    }

    /// Project every nominal leaf in a diagnostic type through the same
    /// source-name ledger used by hover and completion.
    pub(crate) fn display_type(&self, ty: &Type) -> Type {
        ty.map_named_types(&|name| self.name_ledger.display_path(self.module_idx, name, None))
    }

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
                    Expr::Float(value, _, _, _) => Some(*value),
                    Expr::Unary(crate::AST::UnOp::Neg, inner, _) => match &**inner {
                        Expr::Float(value, _, _, _) => Some(-*value),
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
        let (import_ns, leaf) = Self::split_type_name(name);
        let modules = self.modules?;
        if let Some(namespace) = import_ns {
            let index = if namespace.contains("::") {
                let identity = format!("{namespace}::{leaf}");
                self.name_ledger.nominal_module(&identity)?
            } else {
                *self.imports.get(namespace)?
            };
            let fact = modules.get(index)?.registry.unit_fact(leaf)?.clone();
            let canonical = if index == self.module_idx {
                leaf.to_string()
            } else {
                self.canonical_nominal_name(index, leaf)
            };
            return Some((canonical, fact));
        }
        let mut owner = None;
        for (index, module) in modules.iter().enumerate() {
            if module.registry.is_unit_type(leaf) && self.type_is_pub_in(index, leaf) {
                if owner.is_some_and(|previous| previous != index) {
                    return None;
                }
                owner = Some(index);
            }
        }
        let index = owner?;
        let fact = modules.get(index)?.registry.unit_fact(leaf)?.clone();
        let canonical = if index == self.module_idx {
            leaf.to_string()
        } else {
            self.canonical_nominal_name(index, leaf)
        };
        Some((canonical, fact))
    }

    pub(crate) fn is_unit_type_name(&self, name: &str) -> bool {
        self.unit_fact_for_type(&Type::Named(name.to_string()))
            .is_some()
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
        let candidate = Self::split_type_name(source_name).0.map_or_else(
            || leaf.clone(),
            |namespace| {
                if namespace.contains("::") {
                    format!("{namespace}::{leaf}")
                } else {
                    format!("{namespace}.{leaf}")
                }
            },
        );
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
                    operator_rhs: None,
                    checked_widen: false,
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
            if crate::Sema::knowledge_loss_requires_gate(KnowledgePlane::Unit, None) {
                self.diags.push(self.inexact_unit_diagnostic(
                    &destination_name,
                    &source_name,
                    expr.span(),
                ));
                return true;
            }
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

    /// D-MEM-COPYSEM1=A: the default materializes a read-only view at an
    /// owning destination. `copies: .Explicit` is the opt-in refusal ladder;
    /// it follows the same package/module/function/block scope chain as the
    /// other source policies.
    pub(crate) fn copies_explicit(&self) -> bool {
        let declared = |declaration: &crate::Policy::PolicyDeclaration| {
            declaration.key == crate::Policy::PolicyKey::Copies
                && declaration.value == crate::Policy::PolicyValue::Explicit
                && (matches!(
                    declaration.scope,
                    crate::Policy::PolicyScope::Package | crate::Policy::PolicyScope::Module
                ) || (declaration.scope == crate::Policy::PolicyScope::Function
                    && declaration.target == Some(self.current_function_span)))
        };
        self.policy_declarations.iter().any(declared)
            || self.memory_policy_stack.last().is_some_and(|region| {
                region.declarations.iter().any(|declaration| {
                    declaration.key == crate::Policy::PolicyKey::Copies
                        && declaration.value == crate::Policy::PolicyValue::Explicit
                })
            })
    }

    pub(crate) fn audited_gate_allowed(
        &mut self,
        key: crate::Policy::PolicyKey,
        span: Span,
    ) -> bool {
        let declarations = self
            .policy_declarations
            .iter()
            .filter(|declaration| {
                declaration.key == key
                    && (matches!(
                        declaration.scope,
                        crate::Policy::PolicyScope::Organization
                            | crate::Policy::PolicyScope::Package
                            | crate::Policy::PolicyScope::Module
                    ) || declaration.target == Some(self.current_function_span)
                        || declaration.target == Some(span))
            })
            .cloned()
            .collect::<Vec<_>>();
        match crate::Policy::resolve_with_gates(key, declarations, &self.gates) {
            Ok(Some(policy)) if policy.value == crate::Policy::PolicyValue::Forbid => {
                self.diags.push(Diagnostic::error(
                    if key == crate::Policy::PolicyKey::Unsafe { "E3105" } else { "E3415" },
                    format!("the `{}` gate is denied by effective policy", key.name()),
                    "an organization or package policy can refuse an audited escape, including its invocation gate".to_string(),
                    format!("remove the `{}` escape or change the owning policy", key.name()),
                    Some(span),
                ));
                false
            }
            Ok(_) => true,
            Err(_error) => {
                let code = if self.gates.allows(key) {
                    "E3415"
                } else {
                    "E0355"
                };
                self.diags.push(Diagnostic::error(
                    code,
                    format!("the `{}` gate is denied by effective policy", key.name()),
                    "an organization or package policy can refuse an audited escape, including its invocation gate".to_string(),
                    format!("remove `--gate {}=allow` or change the owning policy", key.name()),
                    Some(span),
                ));
                false
            }
        }
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
        let measured = if source.is_measured() {
            source
        } else {
            destination
        };
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
                if crate::Sema::knowledge_loss_requires_gate(KnowledgePlane::Unit, None) {
                    self.diags.push(self.inexact_unit_diagnostic(
                        destination_name,
                        source_name,
                        span,
                    ));
                    return true;
                }
            }
        }
        if self.explicit_units_enabled() && destination_name != source_name {
            self.diags
                .push(self.explicit_units_diagnostic(destination_name, source_name, span));
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
mod CheckerItems;
mod CheckerKernel;
mod CheckerMarkers;
mod CheckerOwnership;
mod CheckerPatchable;
mod CheckerSchedule;
mod CheckerTaskGroup;
use CheckerTaskGroup::TaskGroupCtx;
mod CheckerValidate;
mod CognitiveComplexity;
mod DevtoolsPanel;
pub mod Diagnostics;
mod Edition;
// The REPL's effect-name gate reads parse_effect_name and effect_covers
// directly, the same way it reads ApiFreeze and GateLedger above.
mod BudgetSpecs;
pub mod Effects;
mod FFI;
mod FlowFacts;
pub mod GateLedger;
mod Guest;
pub mod HotSwap;
mod MemberSpread;
mod MemoryFacts;
mod OSTarget;
mod PolicyFacts;
mod Prelude;
mod Protocol;
mod Purity;
mod Registration;
pub mod Schema;
mod SchemaMigration;
mod ScopeMembers;
/// D-CONC-SHARE1=A: the one plain-access desugar for `Shared<T>`.
mod SharedAccess;
pub mod UnsafeObligations;
pub use BudgetSpecs::{
    collect_budget_specs, collect_budget_specs_bundle, collect_located_budget_specs_bundle,
    BudgetApplicability, BudgetAxis, BudgetComparisonFact, BudgetLimitFact, BudgetQuantity,
    BudgetRawQuantity, BudgetSpec, CompileWorkloadFact, LocatedBudgetSpec,
};
mod App;
mod CheckerReferences;
mod KnowledgeLoss;
mod State;
mod Taint;
mod TargetSurface;
mod WebPartition;
pub(crate) use CheckerReferences::record_comptime_import_alias_uses;

pub(crate) use KnowledgeLoss::{
    allows_gate as knowledge_gate_allows, requires_gate as knowledge_loss_requires_gate,
    KnowledgeGate, KnowledgePlane,
};

pub(crate) use Bundle::*;
pub(crate) use Captures::*;

/// Exact AST name-reference query used by codegen planning. Unlike conservative
/// feature-discovery walkers, this is exhaustive over every statement/expression form.
pub fn stmt_references_name_exact(stmt: &Stmt, name: &str) -> bool {
    Captures::stmt_refs_name(stmt, name)
}

/// Same question as `stmt_references_name_exact`, but a use inside a lambda
/// body counts. D-TASKBORROW1=A: a `task.group` child borrows through a lambda.
pub fn stmt_references_name_deep(stmt: &Stmt, name: &str) -> bool {
    Captures::stmt_uses_name_through_lambdas(stmt, name)
}

pub(crate) use CheckerCli::*;
pub use CheckerCoreLib::*;
pub(crate) use CheckerFieldPolicy::*;
pub(crate) use CheckerPatchable::*;
pub(crate) use CheckerValidate::*;
pub use CognitiveComplexity::{cognitive_complexity_reports, CognitiveComplexityReport};
pub(crate) use DevtoolsPanel::{check_devtools_publish, registry_error};
pub use DevtoolsPanel::{
    check_devtools_panels, check_unfed_state_fields,
    unfed_state_fields, E_DEVTOOLS_DUPLICATE_PANEL, E_DEVTOOLS_FIELD_TYPE,
    E_DEVTOOLS_INVALID_PANEL, E_DEVTOOLS_PUBLISH_GATE, E_DEVTOOLS_UNKNOWN_FIELD,
    E_DEVTOOLS_UNFED_FIELD,
};
pub(crate) use Diagnostics::*;
pub use Diagnostics::type_requires_owned_iteration;
pub(crate) use Effects::*;
pub(crate) use Guest::{
    check_guest_export_surface, check_guest_import_surface, check_guest_symbol_collisions,
};
pub use Registration::*;
pub(crate) use Taint::check_func_taint;
pub use TargetSurface::check_target_surface;
pub(crate) use FFI::*;
// D-STATE1: typestate pass — wrong-state operation (E0150).
pub(crate) use State::{check_items_state, checked_state_graphs, StateTable};
// D-LIN1: single-use (must-consume) diagnostics live in CheckerOwnership.
pub use App::extract_app_graph;
pub(crate) use WebPartition::check_web_partition;
pub use WebPartition::checked_web_bucket;
// D-OSTARGET1=A: native OS platform gating (mixed-axis + unmatched-call).
pub(crate) use MemberSpread::desugar_member_spreads;
pub(crate) use OSTarget::{check_os_target, desugar_os_switches};

// Public entry points (preserve `jet::Sema::<item>` paths).
pub use Bundle::{
    build_entry_signature_is_valid, bundle_has_comptime_evaluation, check_bundle,
    check_bundle_for_output, check_bundle_for_output_opts,
    check_bundle_for_output_opts_with_effect_facts, check_bundle_gates,
    check_bundle_gates_with_effect_facts, check_bundle_no_os, check_bundle_no_os_with_effect_facts,
    check_bundle_no_os_with_gates,
    check_bundle_with_effect_facts, check_bundle_with_effect_facts_for_build,
    check_bundle_with_effect_facts_incremental,
    check_target_machine, is_build_entry, specialize_function_types, strip_build_only_entries,
    target_hardware_capabilities, target_hardware_profile, target_hardware_profile_id,
    target_hardware_use, target_hardware_use_with_effect_facts, target_machine_use,
    validate_target_hardware, IncrementalSemaCache, IncrementalSemaStats,
};
pub use Effects::{AuthorityDelegation, EffectSummary, SemIndexEffectFacts};
pub use MemoryFacts::{
    check_memory_facts, project_memory_fact, MemoryCall, MemoryEvent, MemoryEventKind, MemoryFact,
    MemoryFactDeclaration, MemoryPolicyRegion, MemoryProjection, MemorySummary, OpenMemoryDispatch,
};
pub use PolicyFacts::{
    collect_policy_facts, collect_policy_facts_from_program, PolicyDomain, PolicyFact,
    PolicyFactGraph,
};
// D-EFFBUDGET1: the closed effect vocabulary, exposed so jet-driver can
// validate `pkg.jet` `authority.holds`/`authority.grants` manifest fields against it.
// D-EFFTREE1: also export the tree helpers — jet-driver's EffectBudget and
// manifest parsing need root validation and ancestor-subsumption coverage
// too, not just the bare enum.
pub(crate) use CheckerInline::{check_inline_always_fn, e0918_address_taken};
pub(crate) use CheckerMarkers::{
    check_declared_rule_facts, check_deprecated_visibility, check_marker_vocabulary,
};
pub(crate) use CheckerSchedule::{
    check_every_marker, check_job_collisions, check_job_graph,
};
pub use CheckerSchedule::checked_job_registry;
pub use Effects::{
    authority_delegations, builtin_effect, core_effect, effect_covers, effect_root, effect_row_var,
    effect_set_has_root, memory_allocation_bound, package_effect_policy_diagnostics,
    package_policy_path_covers, package_policy_source_path, parse_effect_name,
    reject_positive_deny_only_effect, resolve_effect_name, show_set, undeclared_effect, Effect,
    EffectSet,
};
pub(crate) use Guest::{PluginExportFact, PluginInterface, PluginInterfaceRegistry};
pub use Guest::{
    guest_export_native_symbol, guest_export_signature, guest_export_surface,
    guest_import_bridge_compatible, guest_import_function_signature, guest_import_signature,
    guest_import_surface, guest_import_symbol, guest_import_wrapper_name, guest_surface,
    is_guest_export, is_guest_export_marker, is_guest_import, is_guest_import_marker,
    sandbox_component_type, sandbox_export_signature, sandbox_export_surface, GuestDirection,
    GuestFunction, GuestScalar,
};
pub use Effects::{check_pure_fn, check_pure_program_root};
pub use Purity::{e3401, e3402, e3403};
pub use Registration::effect_key;
pub use FFI::{e3202, e3301, e3302, e3303};
// D-MIGRATE2C: `jet inspect schema status` reuses the schema-migration diff.
pub use SchemaMigration::{check_schema_migrations, desugar_migrations};

/// Free reads and direct calls in an expression, without copying its AST.
pub fn expr_free_reads_and_calls(
    expr: &crate::AST::Expr,
) -> (HashSet<String>, HashSet<String>) {
    let bound = HashSet::new();
    let mut read = HashSet::new();
    let mut mut_cap = HashSet::new();
    let mut called = HashSet::new();
    Captures::expr_collect_captures(expr, &bound, &mut read, &mut mut_cap, &mut called);
    read.extend(mut_cap);
    (read, called)
}

/// D-REACTCORE1: free variable reads in a statement block (for reactive-scope capture cloning).
pub fn block_free_var_reads(stmts: &[crate::AST::Stmt]) -> HashSet<String> {
    let mut bound = HashSet::new();
    let mut read = HashSet::new();
    let mut mut_cap = HashSet::new();
    let mut called = HashSet::new();
    Captures::block_collect_captures(stmts, &mut bound, &mut read, &mut mut_cap, &mut called);
    read.extend(mut_cap);
    read
}

/// [`block_free_var_reads`], split from the free names the block invokes with
/// direct-call syntax (`f(x)`).
///
/// A `Call` carries its callee in `Call::name` rather than an `Expr::Ident`, so
/// a fn-valued binding invoked this way is invisible to the read set even
/// though the lowered body reads that local. It is reported separately because
/// the same spelling names top-level functions and builtins, which are not
/// captured values; only the caller's scope can tell them apart.
pub fn block_free_reads_and_calls(
    stmts: &[crate::AST::Stmt],
) -> (HashSet<String>, HashSet<String>) {
    let mut bound = HashSet::new();
    let mut read = HashSet::new();
    let mut mut_cap = HashSet::new();
    let mut called = HashSet::new();
    Captures::block_collect_captures(stmts, &mut bound, &mut read, &mut mut_cap, &mut called);
    read.extend(mut_cap);
    (read, called)
}
