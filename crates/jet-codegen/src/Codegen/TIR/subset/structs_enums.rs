use crate::AST::{Type, VariantPayload};
use crate::Codegen::alloc_handle_rust_type;
use crate::Codegen::Cx;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_json_type_name;
use crate::Codegen::net_handle_rust_type;
use crate::Codegen::TIR::is_covered_distinct_ty;
use crate::Codegen::TIR::is_covered_cell_ty;
use crate::Codegen::TIR::is_covered_fallible_ty;
use crate::Codegen::TIR::is_covered_foreign_value_ty;
use crate::Codegen::TIR::is_covered_pool_ty;
use crate::Codegen::TIR::is_covered_shared_guard_ty;
use crate::Codegen::TIR::is_covered_shared_weak_ty;
use crate::Codegen::TIR::is_covered_shared_ty;
use crate::Codegen::TIR::is_type_var_param_ty;
use std::collections::HashSet;

/// c109 Phase 4: `ty` is a plain user enum the subset can lower. It must be a
/// bare `Type::Named(E)` that:
///  - is a known enum (`cx.enum_variants` has it), not a struct/trait/foreign/core
///    type (JSON, prelude, imported enums use different Rust heads/spellings);
///  - is NOT generic and has NO boxed (recursive) edge — a `Box<…>` payload needs
///    box/deref handling the subset deliberately avoids (recursive enums → later);
///  - is derivable `Clone` (`cx.cloneable`) — the exhaustive-match lowering clones a
///    by-reference subject (`(subj).clone()`), so the enum must be Clone in Rust;
///  - has every variant payload restricted to scalar/Char fields. A String/struct/
///    list/option payload would need clone/box decisions at the literal site and in
///    pattern bindings (`emit_boxed_enum_arg`, borrowed-payload clone) that the
///    subset cannot reproduce from total facts — exclude the whole enum on any.
pub(crate) fn is_covered_enum_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    enum_is_covered(name, cx)
}

pub(crate) fn enum_is_covered(name: &str, cx: &Cx) -> bool {
    enum_is_covered_inner(name, cx, &mut HashSet::new())
}

/// Core enum values with a concrete `PartialEq` representation in the shared
/// Prelude. Sema resolves `==` on these values to the ordinary `Equatable`
/// method shape, but they are not user enum items in `cx.enum_variants`.
pub(crate) fn core_enum_equal_type(name: &str) -> bool {
    matches!(
        name,
        "ProcessStreamMode"
            | "TerminalMode"
            | "EncodingFormat"
            | "EncodingErrorKind"
            | "DataEvent"
            | "CBORErrorKind"
            | "XMLReason"
            | "XMLEntityPolicy"
            | "XMLEncoding"
            | "XMLLexicalPolicy"
            | "XMLCanonicalMode"
            | "DataErrorKind"
            | "DurationUnit"
            | "LocalDate"
    )
}

/// c109 Phase 16: an enum is covered when every variant payload field is a covered
/// VALUE type — scalar/Char/String, a covered struct, a covered collection, or
/// (recursively) another covered enum (the recursion may go through a `boxed_edge`,
/// reproduced as a `Box::new(…)` at the literal site via `TEnumArg.boxed`). The
/// `seen` set terminates on a recursive (boxed) edge: a self-reference admits the
/// enum (it's already being checked), so a linked-list / expr-AST enum is covered.
/// String/struct/collection payloads route through `emit_boxed_enum_arg`'s borrowed
/// `.clone()` (reproduced at lowering), so they are byte-parity safe.
pub(crate) fn enum_is_covered_inner(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if name == "DataEvent" {
        return true;
    }
    // D-CONC-FAIL1=A: TaskFailure is the shared Prelude enum published by
    // every task join/combinator. Its variants are registered in the TIR
    // context even though no user `enum` item owns them, so variant matches
    // must stay on the same typed-IR path as ordinary covered enums.
    if name == crate::Syntax::TYPE_TASK_FAILURE {
        return true;
    }
    if (crate::Generics::is_type_var_name(name) && !cx.enum_variants.contains_key(name))
        || is_json_type_name(name)
        || is_db_value_type_name(name)
        || core_enum_or_prelude(name)
    {
        return false;
    }
    // c109 Phase 24: a FOREIGN (imported) enum (`NoteType`/`ParseError`, matched in
    // search.jet/index.jet). Its variants ARE registered in `cx.enum_variants` /
    // `cx.variant_owner` (`register_foreign_enum_variants`, Imports.rs), so matching it
    // resolves the owning enum + variant prefix (`emit_match_pattern` emits the foreign
    // `{root}{mod}::__jet_<T>::__jet_<V>` head via `cx.foreign_types`). A foreign enum is
    // NOT in `cx.cloneable` (that set tracks only local types), so we DON'T require it
    // here; instead we require every variant payload to be a covered VALUE type — a
    // covered payload (scalar/String/covered struct/enum/collection) is itself always
    // `Clone` in Rust, so the foreign enum's generated `#[derive(Clone)]` holds and the
    // match scrutinee's unconditional `(subj).clone()` (the AST `emit_pattern_match_switch`
    // clones a by-ref subject regardless of `cx.cloneable`) is valid. The construction
    // side has no reachable cross-module literal syntax (`note.NoteType.User` is E0107),
    // so a foreign enum is only ever MATCHED / passed, never constructed in another module.
    let canonical_name = cx
        .foreign_type_identity("", name)
        .unwrap_or_else(|| name.to_string());
    let is_foreign = super::types::foreign_type_module(&canonical_name, cx).is_some();
    let Some(variants) = cx.enum_variants.get(&canonical_name) else {
        return false;
    };
    let carries_mutable_view =
        cx.type_contains_mutable_view(&Type::Named(name.to_string()));
    // A recursive edge back to this enum admits it (already under check) — the box
    // decision is total. Insert before recursing so a self-reference terminates here.
    if !seen.insert(canonical_name.clone()) {
        return true;
    }
    let payloads_covered = variants.iter().all(|(_vname, payload)| {
        let payload_tys: Vec<&Type> = match payload {
            VariantPayload::Unit => Vec::new(),
            VariantPayload::Single(t, _) => vec![t],
            VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
        };
        payload_tys
            .iter()
            .all(|t| enum_payload_ty_covered(t, cx, seen))
    });
    // A local enum carrying an imported value may be absent from the precomputed
    // cloneability set because the source type is still the visible leaf while
    // the codegen registry stores its canonical identity. A covered payload is
    // itself the stronger Clone fact needed by the generated enum.
    let ok = payloads_covered
        && (is_foreign || cx.cloneable.contains(name) || carries_mutable_view);
    seen.remove(&canonical_name);
    ok
}

/// c109 Phase 16: an enum-variant payload field type the subset can lower —
/// scalar/Char/String, a sema-proved View/ViewMut, a covered struct, a covered
/// collection, or another covered enum (recursion permitted; the boxed edge is
/// reproduced at the literal site).
/// The `seen` set is threaded through every enum reference (including ones reached
/// via a nested collection element) so a `[Self]` / recursive-through-collection
/// payload terminates instead of looping.
pub(crate) fn enum_payload_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    // D-MEMPROVENANCE2=A: sema already proved every stored view's bounded owner
    // set. TIR carries the slice value unchanged; the hidden Rust lifetime is
    // rendered on the enum declaration and containing signatures.
    if matches!(ty, Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut" | crate::Syntax::TYPE_PIN) && args.len() == 1)
    {
        return true;
    }
    // c109 Phase 24: a FOREIGN (imported) struct/enum payload (`Query.Kind(NoteType)`
    // where `NoteType` lives in another module). It renders via `cx.rust_type` to
    // `{root}{mod}::__jet_<Name>`; a payload arg is moved/cloned by `lower_enum_arg`
    // (the borrowed-`.clone()` decision is total), so a foreign payload is byte-parity
    // safe. (A foreign METHOD is still out of subset — the recurring value-type seam.)
    if is_covered_foreign_value_ty(ty, cx) {
        return true;
    }
    // D-STYLEUNIT1 (Tower c134): a DISTINCT-typed enum payload (`Length(Px)`
    // where `Px` is a `#UnitFamily` member). Same rationale as the struct-field
    // case in `field_ty_covered`: the distinct newtype is a covered value type,
    // moved/cloned by `lower_enum_arg` with no new decision.
    if is_covered_distinct_ty(ty, cx) {
        return true;
    }
    match ty {
        Type::Union(members) => {
            !members.is_empty()
                && members
                    .iter()
                    .all(|member| enum_payload_ty_covered(member, cx, seen))
        }
        Type::Named(n) => {
            if cx.enum_variants.contains_key(n) {
                enum_is_covered_inner(n, cx, seen)
            } else {
                is_covered_struct_ty(ty, cx)
            }
        }
        // A collection payload: its element/key/value types must each be a covered
        // value type, with enum references re-checked under the SAME `seen` guard.
        Type::List(inner) => enum_payload_ty_covered(inner, cx, seen),
        Type::Option(inner) => enum_payload_ty_covered(inner, cx, seen),
        Type::Map { key, value, .. } => {
            enum_payload_ty_covered(key, cx, seen) && enum_payload_ty_covered(value, cx, seen)
        }
        // D-MEM1 S6: a `Pool<T>`/`Id<T>`/`Shared<T>` enum payload.
        Type::Apply { .. } => {
            is_covered_pool_ty(ty, cx)
                || is_covered_shared_guard_ty(ty, cx)
                || is_covered_shared_weak_ty(ty, cx)
        }
        Type::Shared(_) => is_covered_shared_ty(ty, cx),
        _ => false,
    }
}

/// A name that resolves to a compiler/core/prelude enum or opaque type rather
/// than a plain user enum — those are excluded from the enum subset.
pub(crate) fn core_enum_or_prelude(name: &str) -> bool {
    net_handle_rust_type(name).is_some() || alloc_handle_rust_type(name).is_some()
}

/// c109 Phase 3: `ty` is a plain user struct the subset can lower. A qualified
/// foreign `Type::Named(alias.S)` uses the foreign-value gate below; local user
/// structs must be bare and:
///  - is a known struct (`cx.struct_fields` has it), not an enum/trait/generic;
///  - is NOT a compiler/prelude/foreign/core type (those use different Rust
///    heads and field spellings the subset does not emit);
///  - is NOT generic and has NO boxed (recursive) edge — a `Box<…>` field read
///    needs deref handling the subset deliberately avoids.
/// Field types may themselves be scalars/String/Char or another covered struct
/// (checked recursively, with a visited set to terminate); a non-covered field
/// type (list/map/option/enum/fn/boxed) excludes the owning struct.
pub(crate) fn is_covered_struct_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    if super::types::foreign_type_module(name, cx).is_some() {
        return is_covered_foreign_value_ty(ty, cx);
    }
    struct_is_covered(name, cx, &mut HashSet::new())
}

/// c109: is `name` a user struct the subset can CONSTRUCT (a struct literal)? Admits
/// self-referential (boxed) fields — a recursive struct such as
/// `Tree { value: Int, child: Tree? }`. Construction is byte-identical to the AST path once
/// the `Box::new(…)` field wrap is reproduced at lowering (a total `boxed` flag from
/// `cx.boxed_edges`); a boxed field READ derefs the `Box` (`TExprKind::Field { boxed }`).
/// A boxed-edge field's value is checked separately by `expr_in_subset` at the gate site;
/// here we only verify the struct's field TYPES are admissible. Generic/foreign/prelude
/// structs are out. The visited set terminates the recursion at a boxed cycle.
pub(crate) fn struct_lit_constructible(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A name that is a genuinely-declared user struct (in `cx.struct_fields`) is a
    // concrete type, never a type variable — even a single-uppercase-letter name like
    // `P`. The `is_type_var_name` heuristic only excludes *undeclared* single-letter
    // names (true generic type vars `T`/`U`), so guard it on non-declaration.
    let is_type_var =
        crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
    if cx.trait_names.contains(name)
        || cx.enum_variants.contains_key(name)
        || super::types::foreign_type_module(name, cx).is_some()
        || net_handle_rust_type(name).is_some()
        || is_type_var
        || struct_is_generic(name, cx)
    {
        return false;
    }
    let Some(fields) = cx.struct_fields.get(name) else {
        return false;
    };
    if !seen.insert(name.to_string()) {
        // A cycle through a boxed edge — admitted (the field below proved boxed).
        return true;
    }
    let ok = fields.iter().all(|(fname, fty)| {
        // A boxed (recursive) edge: its payload struct must itself be constructible.
        // The boxed-payload type unwraps Option/the bare Named to the struct name.
        if cx.boxed_edges.contains(&(name.to_string(), fname.clone())) {
            return boxed_field_payload_constructible(fty, cx, seen);
        }
        // A non-boxed field: the ordinary covered-field rule.
        field_ty_covered(fty, cx, seen)
    });
    seen.remove(name);
    ok
}

/// The payload of a boxed (recursive) struct field — `Tree` in `child: Tree?`
/// (`Option<Tree>`) or a bare `Tree`. The payload struct must be constructible.
pub(crate) fn boxed_field_payload_constructible(
    ty: &Type,
    cx: &Cx,
    seen: &mut HashSet<String>,
) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_constructible(inner, cx, seen),
        Type::Named(n) => struct_lit_constructible(n, cx, seen),
        _ => false,
    }
}

/// c109 Phase 19: is `name` a GENERIC user struct (one with declared type params)? A generic
/// struct's fields reference type vars (`first: T`); `struct_is_covered` admits those
/// so turbofish construction, `Type::Apply` params, and inherent methods all lower
/// through TIR. Imported struct shapes use the same registered field table after
/// qualification, so a generic foreign application follows the same value path.
/// Trait methods keep their separate trait-specific admission rules.
///
/// c148: uses `cx.struct_type_params` (populated from `StructDef.type_params`) rather
/// than `ty_mentions_type_var`, so multi-char type params (`Kind`, `Elem`) are recognized.
pub(crate) fn struct_is_generic(name: &str, cx: &Cx) -> bool {
    cx.struct_type_params
        .get(name)
        .map(|params| !params.is_empty())
        .unwrap_or(false)
}

pub(crate) fn struct_is_covered(name: &str, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    // A struct that is a trait/enum or a non-user core/prelude type is out.
    // Imported user structs are registered in `cx.struct_fields` under their
    // canonical qualified name, so they share this field-coverage path.
    // A declared struct is a concrete type, never a type var (see
    // `struct_lit_constructible`): a single-uppercase-letter struct name (`P`) is real.
    let is_type_var =
        crate::Generics::is_type_var_name(name) && !cx.struct_fields.contains_key(name);
    if cx.trait_names.contains(name)
        || cx.enum_variants.contains_key(name)
        || net_handle_rust_type(name).is_some()
        || is_type_var
    {
        return false;
    }
    let Some(fields) = cx.struct_fields.get(name) else {
        return false;
    };
    if !seen.insert(name.to_string()) {
        // A cycle through a boxed edge — admitted (the edge below proved boxed and the
        // payload struct is itself covered). The boxed field READ derefs the `Box`
        // (`TExprKind::Field { boxed: true }`); construction wraps `Box::new(…)`.
        return true;
    }
    let declared_params = cx.struct_type_params.get(name);
    let ok = fields.iter().all(|(fname, fty)| {
        // A boxed (recursive) edge: its payload struct must itself be covered. The read
        // derefs the `Box` (total fact), so the edge is now in-subset.
        if cx.boxed_edges.contains(&(name.to_string(), fname.clone())) {
            return boxed_field_payload_covered(fty, cx, seen);
        }
        if matches!(fty, Type::Named(param)
            if declared_params.is_some_and(|params| params.contains(param)))
        {
            return true;
        }
        field_ty_covered(fty, cx, seen)
    });
    seen.remove(name);
    ok
}

/// The payload type of a boxed (recursive) struct edge — `Tree` in `child: Tree?`
/// (`Option<Tree>`) or a bare `Tree`. The payload struct must itself be a covered
/// value type (so its own reads/construction lower). Mirrors
/// `boxed_field_payload_constructible` but uses the value-coverage rule.
pub(crate) fn boxed_field_payload_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    match ty {
        Type::Option(inner) => boxed_field_payload_covered(inner, cx, seen),
        Type::Named(n) => struct_is_covered(n, cx, seen),
        _ => false,
    }
}

/// A struct *field* type the subset can lower: scalar/String/Char, or another
/// covered struct. Compound/optional/enum/fn field types exclude the struct.
pub(crate) fn field_ty_covered(ty: &Type, cx: &Cx, seen: &mut HashSet<String>) -> bool {
    if ty.is_scalar() || matches!(ty, Type::Char | Type::String) {
        return true;
    }
    // D-MEM-VIEWRET1=B: sema is the sole authority for whether a stored view
    // has a stable owner. Once admitted, a View field is an ordinary borrowed
    // slice value for TIR; codegen only threads the hidden Rust lifetime.
    if matches!(ty, Type::Apply { name, args } if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut" | crate::Syntax::TYPE_PIN) && args.len() == 1)
    {
        return true;
    }
    // c109 Phase 19: a generic struct's field may be a bare type VARIABLE (`first: T`
    // in `Pair<T>`). It renders to the bare `T` via `cx.rust_type` and a struct-lit
    // field value is the type-var value itself (by value), so a type-var field needs no
    // clone/deref decision — admit it. (A struct with a type-var field is only ever
    // *used* as a `Type::Apply` — `Pair<Int>` — which `is_covered_generic_struct_ty`
    // gates; a bare `Pair` never type-checks in sema.)
    // c148: pass cx for multi-char type param recognition.
    if is_type_var_param_ty(ty, cx) {
        return true;
    }
    // c109 Phase 24: a FOREIGN value-type field/element — a cross-module imported user
    // struct/enum or the prelude JSON enum. It renders via `cx.rust_type` to its own
    // Rust head; a struct-lit field value / collection element is the value itself (by
    // value), so a foreign-typed field needs no clone/deref decision at the field site.
    // (Reading a foreign field is in-subset; a foreign METHOD still excludes the fn.)
    if is_covered_foreign_value_ty(ty, cx) {
        return true;
    }
    // D-UNIONTYPE1=A: anonymous unions are structural enum sugar (`Int | String` →
    // `user___JetUnion_Int_String`). A union field is covered when every member is;
    // codegen emits the generated enum + Encode/Decode, and field reads/writes are
    // ordinary places (member→union widen injects at binding/arg/return/field sites).
    // Checked before `is_covered_enum_ty` so we do not recurse through Named-only
    // enum coverage for a non-Named `Type::Union`.
    if let Type::Union(members) = ty {
        return !members.is_empty() && members.iter().all(|m| field_ty_covered(m, cx, seen));
    }
    // c109 Phase 24: a covered ENUM field (`note_type: NoteType` on a `Note` struct). An
    // enum field renders to `__jet_<Enum>` and a field read is a plain place / sema-cloned
    // `.clone()` (the Phase-3/6 owning-field rewrite) — byte-identical, no new decision.
    // (Previously `field_ty_covered` admitted only scalar/String/struct/collection fields,
    // so any struct with an enum field stayed on the AST path.)
    if is_covered_enum_ty(ty, cx) {
        return true;
    }
    // D-STYLEUNIT1 (Tower c134): a DISTINCT-typed field (`m: Meters` where
    // `Meters :: distinct Float`, or a `#UnitFamily` member like `width: Px`).
    // A distinct type renders via `cx.rust_type` to its generated newtype
    // (`struct __jet_Meters(f64)`); a struct-lit field value is a distinct
    // constructor call (`Meters(10.0)` → the newtype, a covered expr) and a
    // field read is a plain by-value place — byte-identical to a scalar field,
    // no clone/deref decision. (Previously a distinct field routed to
    // `struct_is_covered`, which only knows plain user structs and returned
    // false, so any struct/enum carrying a unit-family field ICEd once TIR
    // became the sole codegen path — the standing bug this decision fixes.)
    if is_covered_distinct_ty(ty, cx) {
        return true;
    }
    // c109 Phase 24: an OPTIONAL / FALLIBLE field (`note: Note?`, `error_msg: String?` on
    // a `ParsedResult` struct) whose payload is a covered value type. It renders via
    // `cx.rust_type` to `Option<…>`/`Result<…,…>`; a struct-lit field value (`value(n)` →
    // `Some(n)`, `null` → `None`) is in-subset and emitted as-is, and a field read is a
    // plain place / sema-cloned `.clone()` — byte-identical, no new field-site decision.
    if is_covered_fallible_ty(ty, cx) {
        return true;
    }
    // c109 Phase 27: a FUNCTION-typed field (`step: fn(Int) => Int` on a `Worker`
    // struct). It renders via `cx.rust_type` to `Box<dyn Fn(...) -> ...>` exactly as
    // the AST `struct_field_rust` does; a struct-lit field value (a lambda / a bare
    // fn-name) lowers in-subset and is emitted as-is (NO ` as <fn-type>` coercion at
    // the literal site — the AST `emit_struct_lit` field value is a plain `emit_expr`),
    // and a fn-field READ / CALL routes through the Phase-27 `FnFieldCall` shape. The
    // param/ret types are only RENDERED (never inspected for a decision), so any Fn
    // signature is admissible.
    if matches!(ty, Type::Fn { .. }) {
        return true;
    }
    match ty {
        Type::Named(n) => struct_is_covered(n, cx, seen),
        // c109 Phase 16: a collection field (`[E]` / `[K: V]`) whose element/key/value
        // types are covered value types. The struct-literal emit is plain
        // (`field: vec![…]`), byte-identical to the AST path. A list/map *element*
        // that is itself a covered struct/enum/collection is admitted (the Phase-5
        // collection coverage), so no clone/box decision arises at the field site.
        Type::List(inner) => field_ty_covered(inner, cx, seen),
        // c109 (B2): a fixed-size-list field (`row: [Int#3]`). It renders to `Vec<E>`
        // like a list field (`cx.rust_type`), so a struct-lit field value / field read
        // is byte-identical to the list case once its element type is covered.
        Type::FixedList { elem, .. } => field_ty_covered(elem, cx, seen),
        Type::Map { key, value, .. } => {
            field_ty_covered(key, cx, seen) && field_ty_covered(value, cx, seen)
        }
        Type::Tuple(fields) => fields
            .iter()
            .all(|(_, ty)| field_ty_covered(ty, cx, seen)),
        Type::Tagged { inner, .. } => field_ty_covered(inner, cx, seen),
        // D-MEM1 S6 / D-LOCALCELL1=A: a bare core memory-handle field.
        Type::Apply { .. } => {
            is_covered_pool_ty(ty, cx)
                || is_covered_shared_guard_ty(ty, cx)
                || is_covered_shared_weak_ty(ty, cx)
                || is_covered_cell_ty(ty, cx)
        }
        Type::Shared(_) => is_covered_shared_ty(ty, cx),
        _ => false,
    }
}
