use crate::AST::{Type};
use crate::Codegen::alloc_handle_rust_type;
use crate::Codegen::core_rust_type_name;
use crate::Codegen::Cx;
use crate::Codegen::file_handle_rust_type;
use crate::Codegen::layout_handle_rust_type;
use crate::Codegen::net_handle_rust_type;
use crate::Codegen::TIR::is_covered_enum_ty;
use crate::Codegen::TIR::is_covered_struct_ty;
use crate::Codegen::TIR::struct_is_covered;
use std::collections::HashSet;

/// Resolve a `Self` type reference to the owning concrete type. Other types pass
/// through unchanged. (In current Jet a literal `Self` return rarely type-checks —
/// sema treats `Self` and the concrete name as distinct — but resolving it here
/// keeps the gate total if a future sema unifies them.)
pub(crate) fn resolve_self_ty(ty: &Type, type_name: &str) -> Type {
    match ty {
        Type::Named(n) if n == "Self" => Type::Named(type_name.to_string()),
        _ => ty.clone(),
    }
}

/// A param/return type the subset allows: scalar (Int/IntN/Float/F32/Bool),
/// Char, String, a covered *plain user struct* (c109 Phase 3) or generic
/// struct application (c109 Phase 19), a covered
/// *plain user enum* (c109 Phase 4), a covered collection (Phase 5), or a covered
/// *optional* `T?` / *fallible* `T ? E` (c109 Phase 8). Generic type variables
/// are admitted when active in the enclosing function; recursive (boxed) types
/// remain out of the value subset.
pub(crate) fn is_subset_param_ty(ty: &Type, cx: &Cx) -> bool {
    let ty = cx.expand_type_aliases(ty);
    // D-QUAL4=A: tagged types are transparent — strip the marker and check the inner type.
    if let Type::Tagged { inner, .. } = &ty {
        return is_subset_param_ty(inner, cx);
    }
    // D-TERM1 (ratified 2026-06-22): `Key` is a core enum (prelude, not user-registry).
    // It is always cloneable and has scalar/Char payloads only — fully covered.
    if matches!(&ty, Type::Named(n) if n == crate::Syntax::TYPE_KEY) {
        return true;
    }
    if matches!(&ty, Type::Named(n) if n == crate::Syntax::TYPE_TASKGROUP) {
        return true;
    }
    if matches!(&ty, Type::Named(n) if n == crate::Syntax::TYPE_CONDITION) {
        return true;
    }
    if matches!(&ty, Type::Named(n) if n == "DataEvent") {
        return true;
    }
    if matches!(&ty, Type::Named(n) if matches!(n.as_str(), "KeyStatus" | "VaultError" | "WrappedVaultKey" | "KeyUnlock" | "KeyWrapError")) {
        return true;
    }
    if matches!(&ty, Type::Named(n) if n == "HTTPHandler") {
        return true;
    }
    if matches!(&ty, Type::Named(n) if matches!(n.as_str(),
        "Effect" | "UiNode" | "Subscription" | "EventScope" | "EventPolicy" | "EventTrace" | "AsyncPolicy" | "HookPolicy"
        | "Overflow" | "FailurePolicy" | "DispatchState" | "EventConfigError"
        // D-WEBAPP1 / D-RENDERTGT*: opaque UI + web graph value types (prelude hosts).
        | "WebApp" | "WebPage" | "DevServer"
        | "EventResult" | "NullBackend" | "TuiBackend" | "GtkBackend"
        | "Point" | "Size" | "Rect" | "SizeConstraint" | "AriaRole" | "InputEvent")) {
        return true;
    }
    // D-UNIONTYPE1=A: anonymous unions are one generated enum of covered members.
    if let Type::Union(members) = &ty {
        return !members.is_empty() && members.iter().all(|m| is_subset_param_ty(m, cx));
    }
    ty.is_scalar()
        || matches!(&ty, Type::Char | Type::String)
        || is_type_var_param_ty(&ty, cx)
        || is_covered_trait_object_ty(&ty, cx)
        || is_covered_distinct_ty(&ty, cx)
        || is_covered_tuple_ty(&ty, cx)
        || is_covered_struct_ty(&ty, cx)
        || is_covered_enum_ty(&ty, cx)
        || is_covered_collection_ty(&ty, cx)
        || is_covered_view_ty(&ty, cx)
        || is_covered_expanded_collection_ty(&ty, cx)
        || is_covered_fallible_ty(&ty, cx)
        || is_covered_fn_ty(&ty, cx)
        || is_covered_foreign_value_ty(&ty, cx)
        || is_covered_generic_struct_ty(&ty, cx)
        || is_covered_concurrency_ty(&ty, cx)
        || is_covered_reactive_ty(&ty, cx)
        || is_covered_event_ty(&ty, cx)
        || is_covered_shared_ty(&ty, cx)
        || is_covered_shared_guard_ty(&ty, cx)
        || is_covered_shared_weak_ty(&ty, cx)
        || is_covered_pool_ty(&ty, cx)
        || is_covered_cell_ty(&ty, cx)
        || is_covered_data_ty(&ty, cx)
        || is_covered_compute_ty(&ty)
        || is_covered_vault_ty(&ty, cx)
}

/// Unit has no value representation for parameters or bindings, but it is a
/// valid function result. Keep that distinction explicit so a Unit-returning
/// function can stay on the same TIR path as its body and call sites.
pub(crate) fn is_subset_return_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(name) if name == "Unit") || is_subset_param_ty(ty, cx)
}

/// D-COMPUTE-TYPE1: all compute aliases use the same `JetTensor` value
/// representation. Sema owns the shape checks; this gate only admits the
/// resolved forms that `cx.rust_type` renders to that representation.
pub(crate) fn is_covered_compute_ty(ty: &Type) -> bool {
    ty.is_compute_tensor_family()
}

fn is_covered_vault_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Apply { name, args }
        if matches!(name.as_str(), "ExpiringSecret" | "KeyRef" | "MutationPlan" | "VaultWrite" | "Rotation" | "WrappedImportPlan")
            && args.len() == 1 && is_subset_param_ty(&args[0], cx))
}

/// D-MEM-VIEWRET1=B: views use the existing slice representation. Sema has
/// already proved their provenance; the TIR coverage gate only needs to know
/// that the element type is itself representable.
fn is_covered_view_ty(ty: &Type, cx: &Cx) -> bool {
    // D-PIN1=A: `Pin<T>` joins the family — same borrowed representation, same
    // "sema already proved provenance" argument.
    matches!(
        ty,
        Type::Apply { name, args }
            if matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut" | crate::Syntax::TYPE_PIN)
                && args.len() == 1
                && (matches!(&args[0], Type::Named(inner) if inner == "str")
                    || is_subset_param_ty(&args[0], cx))
    )
}

pub(crate) fn is_covered_event_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else { return false; };
    match name.as_str() {
        "Event" | "DispatchReport" | "DispatchFailure" => args.len() == 1 && is_subset_param_ty(&args[0], cx),
        "Hook" | "DecisionHook" | "HookDecision" | "HookOutcome" | "AsyncEvent" => args.len() == 2 && args.iter().all(|arg| is_subset_param_ty(arg, cx)),
        _ => false,
    }
}

/// D-DATAFRAME1=A: core.data value containers. They are plain owned prelude
/// structs over a covered row/value type; method-like behavior is exposed
/// through core.data functions, so binding/passing/returning the containers
/// needs no special emit beyond `cx.rust_type`.
pub(crate) fn is_covered_data_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    match name.as_str() {
        "Table" | "Series" | "LazyFrame" => {
            args.len() == 1 && is_subset_param_ty(&args[0], cx)
        }
        "DataJoin" => args.len() == 2 && args.iter().all(|arg| is_subset_param_ty(arg, cx)),
        _ => false,
    }
}

/// D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>` / `Id<T>` — the generational arena and
/// its index+generation handle. Same shape as `is_covered_concurrency_ty`:
/// `cx.rust_type` already renders these to `{root}jet_std::Jet{Pool,Id}<{T}>`, so
/// binding/passing/returning one is byte-identical to the AST path. `T` must
/// itself be a covered value type.
pub(crate) fn is_covered_pool_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    let Some(elem) = args.first() else {
        return false;
    };
    match name.as_str() {
        // `Id<T>` is a plain index+generation handle — it never actually stores
        // or embeds a `T` at runtime (`JetId<T>`'s only field is a zero-size
        // `PhantomData<fn() -> T>`). So its OWN coverage doesn't need `T`'s full
        // recursive struct-coverage — just that `T` names a real registered
        // type. This matters for the self-referential shape a parent-pointer
        // tree needs (`struct Node { parent: Id<Node>? }`): checking whether
        // `Node` is covered recurses into this field, which would recurse right
        // back into "is `Node` covered" through `concurrency_elem_covered` —
        // an infinite loop `is_covered_struct_ty` has no cycle guard for.
        "Id" => matches!(elem, Type::Named(n) if cx.type_names.contains(n)),
        "Pool" => concurrency_elem_covered(elem, cx),
        _ => false,
    }
}

/// D-LOCALCELL1=A: local cell values and projected guards use one covered
/// element type. `cx.rust_type` maps each handle to the matching Prelude type.
pub(crate) fn is_covered_cell_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(
        ty,
        Type::Apply { name, args }
            if matches!(
                name.as_str(),
                "Cell" | "CellReadGuard" | "CellEditGuard"
            )
                && args.len() == 1
                && is_subset_param_ty(&args[0], cx)
    )
}

/// c109 Phase 6b: a `Shared<T>` (`Type::Shared`) usable as a param/return/local value
/// type. `cx.rust_type` renders it to `{root}jet_std::JetShared<{T}>` (D-MEM1 S6 —
/// was a bare `std::sync::Arc<{T}>` before this stage, back when the type had no
/// real constructor; see the type's own doc comment) and `rust_param_type` borrows
/// a `Read` non-scalar to `&{that}` — both shared with the AST path, so the
/// signature is byte-identical. A `Read` param reads as `(*user_h)` (`param_place`'s
/// non-scalar deref). The element `T` must itself be a covered value type. Admitting
/// the type is what lets a fn with a `Shared<T>` param route; passing one to a free
/// call auto-clones the handle via `lower_one_call_arg`'s `arc_clone` (the gate now
/// admits it) — a plain `.clone()` on `JetShared<T>`'s own `Clone` impl, not
/// `Arc::clone` directly (see `emit_tir_call_args`).
pub(crate) fn is_covered_shared_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Shared(inner) if is_subset_param_ty(inner, cx))
}

pub(crate) fn is_covered_shared_guard_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(
        ty,
        Type::Apply { name, args }
            if name == crate::Syntax::TYPE_SHARED_GUARD
                && args.len() == 1
                && is_subset_param_ty(&args[0], cx)
    )
}

pub(crate) fn is_covered_shared_weak_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    if name != crate::Syntax::TYPE_SHARED_WEAK || args.len() != 1 {
        return false;
    }
    match &args[0] {
        // D-SHARED-CYCLE1=C: Weak of a user struct is always a sized handle.
        // Do not recurse into that struct's fields — a `Shared.Weak<Node>` field
        // on `Node` would otherwise loop forever through is_subset_param_ty.
        Type::Named(n) if cx.struct_fields.contains_key(n) => true,
        other => is_subset_param_ty(other, cx),
    }
}

/// c109 Phase 21 / D-TUPLE-DESTRUCT1: a concurrency handle type `Task<T>` /
/// `Receiver<T>` / `Sender<T>` (a `Type::Apply` with one type arg) usable as a
/// param/return/local *value* type. `cx.rust_type` (Source/Codegen/Context.rs)
/// already renders these to `{root}jet_std::Jet{Task,Receiver,Sender}<{T}>`, so
/// passing/binding/returning one is byte-identical to the AST path with no new emit.
/// The element type `T` must itself be a covered value type. A METHOD on one
/// (`join`/`detach`/`receive`/`send`) carries
/// `recv_type == None` (a Phase-9 builtin gap) and is covered by a dedicated shape — but
/// covering the value type never *forces* a method, so an uncovered method still excludes
/// its fn (the recurring "cover the value type, let the next uncovered node exclude its fn"
/// seam). These are NOT `Type::Named` (so they never match `emit_let`'s `is_file_handle`
/// set — their prelude methods take `&self`, so the binding stays a plain `let`, exactly as
/// the AST path renders).
pub(crate) fn is_covered_concurrency_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Receiver" | "Sender" | "Stream")
        && args.len() == 1
        && concurrency_elem_covered(&args[0], cx)
}

/// D-REACT1=B: a reactive handle type `Signal<T>` / `Derived<T>` (a single-arg
/// `Type::Apply`) usable as a param/return/local *value* type. Structurally the
/// same seam as `is_covered_concurrency_ty`: `cx.rust_type` renders these to
/// `{root}jet_std::Jet{Signal,Derived}<{T}>` (Source/Codegen/Context.rs), so
/// binding/passing/returning one is byte-identical to the AST path. The element `T`
/// must itself be a covered value type. Their methods (`get`/`set`) carry
/// `recv_type == None` and are covered by the reactive-method shape.
pub(crate) fn is_covered_reactive_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Signal" | "Derived" | "Computed")
        && args.len() == 1
        && is_subset_param_ty(&args[0], cx)
}

pub(crate) fn is_covered_expanded_collection_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Named(name)
            if name == crate::Syntax::TYPE_BIT_SET || name == crate::Syntax::TYPE_BYTE_BUFFER =>
        {
            true
        }
        Type::Apply { name, args }
            if matches!(
                name.as_str(),
                crate::Syntax::TYPE_SET
                    | crate::Syntax::TYPE_SORTED_SET
                    | crate::Syntax::TYPE_PRIORITY_QUEUE
                    | "Bag"
            ) =>
        {
            args.len() == 1 && is_subset_param_ty(&args[0], cx)
        }
        Type::Apply { name, args } if name == crate::Syntax::TYPE_LRU => {
            args.len() == 2 && is_subset_param_ty(&args[0], cx) && is_subset_param_ty(&args[1], cx)
        }
        _ => false,
    }
}

/// c109 Phase 21: a `Task<T>`/`Channel<T>`/`Sender<T>` element type. Any covered value
/// type, PLUS `Unit` (`Type::Named("Unit")`) — the result type of a `() => { … }` spawn
/// closure that returns nothing (`task { s.send(…) }` →
/// `Task<Unit>`, the `[Task<Unit>]` worker list in 34_parallel_scan). `Unit` renders via
/// `cx.rust_type` to `()` (Source/Codegen/Context.rs), so `JetTask<()>` is byte-identical
/// to the AST path. (`Unit` is not a covered value type generally — it has no binding/
/// param surface of its own — so it's admitted only here, where it can only appear as the
/// erased result of a unit-returning task.)
pub(crate) fn concurrency_elem_covered(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || is_subset_param_ty(ty, cx)
}

/// c109 Phase 19: a GENERIC struct application `Pair<T>` / `Stack<Int>` (a `Type::Apply`)
/// usable as a param/return/local value type. The base name must be a covered user struct
/// (`struct_is_covered` — which admits type-var fields, Phase 19), and every type argument
/// must itself be a covered value type OR a bare type variable. Imported struct shapes use
/// their canonical qualified names in the same tables, so `owner::Stack<Int>` follows the
/// identical TIR path. The Rust head is `user_<Name>::<args>` (the turbofish from
/// `user_type_apply_rust`), resolved at lowering. `cx.rust_type` already renders
/// `Type::Apply` to that head, so param/return/local typing is byte-identical to the AST
/// path. (A non-generic `Type::Apply` would be malformed; sema only produces `Apply` for a
/// generic struct/enum instantiation.)
pub(crate) fn is_covered_generic_struct_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    // The base must be a known struct (not an enum/trait/core/prelude type). Local
    // and imported user structs are both registered in `cx.struct_fields`.
    if !cx.struct_fields.contains_key(name) {
        return false;
    }
    if !struct_is_covered(name, cx, &mut HashSet::new()) {
        return false;
    }
    // Every type argument is a covered value type or a bare type variable (`T`).
    // c148: pass cx so multi-char type params are recognized.
    args.iter()
        .all(|a| is_type_var_param_ty(a, cx) || is_subset_param_ty(a, cx))
}

/// c109 Phase 23: a DISTINCT type (`UserId :: distinct Int`, D-DIST1) usable as a
/// param/return/local *value* type. A distinct type renders via `cx.rust_type` to its
/// newtype `user_<Name>` (the `Type::Named` fallthrough in Context.rs), and the emitted
/// `#[repr(transparent)]` newtype is `Copy` iff its base is (sema/codegen derive set) —
/// but the param convention (`Read`→deref for a non-scalar Named) is decided exactly as
/// for a struct, so passing/binding/returning one is byte-identical to the AST path with
/// no new emit. Construction is the `is_distinct_ctor` `Call` shape; `.raw()` is the
/// dedicated DistinctRaw method shape; `+`/`==` on a `#Numeric` distinct emit the native
/// operator (`ast_operand_is_integer` returns `None` for a distinct-typed operand, so the
/// overflow trap is never claimed — matching the AST path's plain `+`).
pub(crate) fn is_covered_distinct_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(name) if cx.is_distinct_type_name(name))
}

/// c109 Phase 23: a named-tuple type `(x: Int, y: Int)` (S73/D-SG7, `Type::Tuple`)
/// usable as a param/return/local value type. A tuple renders via `cx.rust_type` to a
/// generated `#[derive(Debug, Clone, PartialEq[, …])]` struct `JetTup_<hash>` (with
/// `user_<field>` fields) emitted by `Tuples.rs` for every tuple SHAPE the program uses
/// — so passing/binding/returning one is byte-identical to the AST path with no new
/// emit. A tuple field read is the generic `Field` shape (`(t).user_<f>`); construction
/// is the `TupleLit` shape; destructuring is the `BindPattern::Tuple` `let` form;
/// `==`/`!=` is native (the derived `PartialEq`). Every field type must itself be a
/// covered value type (so a field read / destructure element emits in-subset).
pub(crate) fn is_covered_tuple_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Tuple(fields) = ty else {
        return false;
    };
    !fields.is_empty() && fields.iter().all(|(_, t)| is_subset_param_ty(t, cx))
}

/// c109 Phase 17: a bare type-PARAMETER type (`T` in a generic `fn id<T>(x: T)`). A
/// single-uppercase `Type::Named` reads as a type var (`Generics::is_type_var_name`),
/// rendered by `cx.rust_type`/`rust_param_type` as the bare letter (by-value, no `&`).
/// Admitting it lets a generic free function whose params/return are type-vars (or covered
/// concrete types) route through the TIR. A generic STRUCT type (`Pair<T>`, `Type::Apply`)
/// is NOT admitted here — that surface (turbofish construction, `[T]`-field builtins) is
/// deferred, so such a function exits the gate at the param/return type check.
///
/// c148: also checks `cx.current_type_params` so multi-char params (`Kind`, `Elem`)
/// are treated identically to single-char ones.
pub(crate) fn is_type_var_param_ty(ty: &Type, cx: &Cx) -> bool {
    matches!(ty, Type::Named(n)
        if crate::Generics::is_type_var_name(n)
            || cx.current_type_params.borrow().contains(n.as_str()))
}

/// Resolve a user nominal through the one canonical codegen import map.
pub(crate) fn foreign_type_module<'a>(name: &str, cx: &'a Cx) -> Option<&'a str> {
    cx.foreign_types.get(name).map(String::as_str)
}

/// c109 Phase 17: a FOREIGN/PRELUDE type usable as a param/return/local *value* type.
/// These all render through `cx.rust_type` already (a prelude handle/core struct → its
/// `Jet…`/`jet_std::…` Rust name), so passing/binding/returning one is byte-identical to
/// the AST path with no new emit. Only the constructable PRELUDE STRUCTS
/// (HTTPRequest/HTTPResponse — `net_handle_rust_type` + a struct-literal form) and the
/// CORE structs (ProcessResult/Stopwatch/JSON/…) are admitted as value types here; a
/// foreign *imported user* struct/enum needs cross-module `import_ns` construction (a
/// Phase-14 surface) and stays excluded. A METHOD on any of these is still out of subset
/// (handle/prelude methods → Phase 13's residue), so a function that *calls* a method on
/// one is excluded by that call — covering the value type never reaches an uncovered
/// method form.
pub(crate) fn is_covered_foreign_value_ty(ty: &Type, cx: &Cx) -> bool {
    let Type::Named(name) = ty else {
        return false;
    };
    // c109 Phase 19: a FOREIGN (imported user) struct/enum used as a value type. It
    // renders via `cx.rust_type` to `{root}{mod}::__jet_<Name>` (Context.rs), and a field
    // read on it mangles (`(n).user_title`) exactly as `mangle` produces — byte-identical
    // to the AST path with no new emit. Construction (`alias.Note { … }`) routes via the
    // `import_ns` StructLit shape; a method on it is still out of subset, so a fn that
    // calls one is excluded by that call (the recurring "cover the value type, let the next
    // uncovered node exclude its fn" seam).
    if foreign_type_module(name, cx).is_some() {
        return true;
    }
    // D-REGEXENGINE1=A: a regex `Match` value (`if m == value(mat)` binds
    // `mat: Match`). It renders to `jet_std::JetRegexMatch`.
    if name == "Match" {
        return true;
    }
    if matches!(name.as_str(), "Claims" | "AuthError" | "Session" | "Auth" | "SyncText" | "SyncCounter" | "SyncMap" | "SyncList" | "RowPolicy" | "LiveQuery" | "DBScope") {
        return true;
    }
    // Core crypto values already have total `cx.rust_type` mappings. Admitting
    // them here lets read-only helpers accept ExpiringSecret callback loans;
    // each helper body still has to pass the ordinary expression/method gates.
    if matches!(
        name.as_str(),
        "Secret"
            | "SigningKey"
            | "VerifyKey"
            | "X25519SecretKey"
            | "X25519PublicKey"
            | "SharedSecret"
            | "Signature"
            | "Sealed"
            | "WrappedKey"
            | "PasswordHash"
            | "Digest256"
            | "Digest512"
            | "CryptoError"
            | "FileCryptoError"
            | "KeyWrapError"
    ) {
        return true;
    }
    // A prelude struct constructable via a struct literal, or a core/prelude struct that
    // renders to its own Rust name. (FileReader/TcpStream/Arena/… are opaque handles — no
    // literal form — but are valid value types; admit the constructable + core ones, plus
    // the opaque handles, all of which `cx.rust_type` renders.)
    is_prelude_struct_name(name)
        || core_rust_type_name(name).is_some()
        || cx.core_qualified_rust_type_name(name).is_some()
        || file_handle_rust_type(name).is_some()
        || net_handle_rust_type(name).is_some()
        || alloc_handle_rust_type(name).is_some()
        // D-LAYOUT1 / D-LAYOUT-GATES1: layout runtime types (opaque handles,
        // no literal form — like the alloc/file/net handles above).
        || layout_handle_rust_type(name).is_some()
}

/// c109 Phase 17: a PRELUDE STRUCT name with a struct-literal construction form — the
/// HTTP request/response types (`net_handle_rust_type` + the `is_prelude_struct` branch in
/// `emit_struct_lit`). These get a Rust head `<root>Jet…` with PLAIN (unmangled) fields,
/// and HTTPRequest additionally an injected `params: BTreeMap::new()` field.
pub(crate) fn is_prelude_struct_name(name: &str) -> bool {
    // D-TEXTWIDTH1=B: `TextWidth` is a plain dot-ctor core struct (no auto
    // fields, unlike HTTPRequest's `params`) — see the lowering branch keyed
    // on `type_name == "TextWidth"` in `lower_expr`'s StructLit arm.
    matches!(
        name,
        "Err" | "HTTPRequest" | "HTTPResponse" | "Range" | "TextWidth" | "TerminalSize" | "TerminalPolicy"
            | "DataLineOptions"
            | "AsyncPolicy" | "FieldError"
            | "EncodingLimits" | "EncodingCause" | "EncodingError"
            | "CBOROptions" | "CBORError" | "XMLLimits" | "XMLParseOptions"
            | "XMLRenderOptions" | "XMLCanonical" | "XMLError"
            | "RecipientReport" | "SendReport" | "Limits" | "DkimConfig" | "SMTPConfig"
    )
}

/// c109 Phase 19: is a FOREIGN (imported user) struct literal `alias.Type { … }` in
/// subset? The `import_ns` struct-literal branch emits
/// `{root}{import_mods[alias]}::{mangle(Type)}[::<args>]` with MANGLED field names.
/// Cover it when: the import alias resolves in `cx.import_mods` (so the module head is
/// total), the owner-qualified shape is registered, and every
/// turbofish type arg is a covered/type-var value. The field VALUES are checked in-subset
/// by the caller; the foreign struct's field *types* live in another module and don't
/// affect the emit (the head + mangled field names are the whole shape). A trait-coerced
/// foreign literal (`as_trait`) is excluded by the caller.
pub(crate) fn foreign_struct_lit_in_subset(
    type_name: &str,
    type_args: &[Type],
    import_ns: Option<&str>,
    cx: &Cx,
) -> bool {
    let alias = import_ns.unwrap_or("");
    let Some(qualified) = cx.foreign_type_identity(alias, type_name) else {
        return false;
    };
    if let Some(alias) = import_ns {
        let Some(import_mod) = cx.import_mods.get(alias) else {
            return false;
        };
        if cx.foreign_types.get(&qualified) != Some(import_mod) {
            return false;
        }
    }
    if !cx.struct_fields.contains_key(&qualified) {
        return false;
    }
    type_args
        .iter()
        .all(|a| is_type_var_param_ty(a, cx) || is_subset_param_ty(a, cx))
}

/// c109 Phase 13: a Jet `fn(…) => …` parameter/return type the subset lowers. The fn-type
/// renders via `cx.rust_type` (`Box<dyn Fn(…) -> … [+ Send + Sync]>`) exactly as the
/// AST `rust_param_type`/`rust_return_type` do — passed/returned by value (no `&`,
/// `param_place`'s deref matches `emit_func`'s slot). The param/return + arg types must
/// themselves be covered value types so the rendered fn-trait is well-formed and the
/// arg lowering can wrap it. A higher-order fn param (a fn taking/returning a fn) is
/// admitted recursively.
pub(crate) fn is_covered_fn_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Fn { params, ret, .. } => {
            params.iter().all(|p| is_subset_param_ty(p, cx))
                && ret
                    .as_ref()
                    .map(|r| is_subset_param_ty(r, cx))
                    .unwrap_or(true)
        }
        _ => false,
    }
}

/// c109 Phase 30: a TRAIT-OBJECT param/return/local type (`s: Shape` where `Shape` is a
/// user trait → `Type::TraitObject("Shape")`, or a bare `Type::Named("Shape")` naming a
/// trait). It renders via `cx.rust_type` to `Box<dyn user_Shape>` (Context.rs), and the
/// param convention is decided by `rust_param_type`'s trait-object arm (`Read` → `&Box<dyn
/// …>`, the slot deref'd to `(*user_s)` by `param_place` — a non-scalar `Read` param). A
/// METHOD on it is the dedicated trait-object dispatch shape (`recv_type == Some(<trait>)`,
/// dynamic dispatch via the bare method name); a non-method use (pass/bind/return) is
/// byte-identical to the AST path. The trait must be a known user trait (`cx.trait_names`),
/// never a foreign/prelude name.
pub(crate) fn is_covered_trait_object_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::TraitObject(t) => t.iter().all(|n| cx.trait_names.contains(n)),
        Type::Named(n) => cx.trait_names.contains(n),
        _ => false,
    }
}

/// c109 Phase 8: `ty` is an optional `T?` (`Type::Option`) or a fallible `T ? E`
/// (`Type::Result`) whose payload(s) are themselves covered *value* types. Both
/// lower through `cx.rust_type` (`Option<…>` / `Result<…, …>`) exactly as the AST
/// path does, so a covered-payload optional/fallible param/return needs no special
/// emit. A nested `T??` (Option of Option) never reaches here — sema rejects it —
/// but the recursion would handle it anyway. A list/map *of* options is still
/// excluded (`collection_elem_covered` does not admit `Option`/`Result`), because
/// element clone/coercion for those is deferred.
pub(crate) fn is_covered_fallible_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::Option(inner) => fallible_payload_covered(inner, cx),
        Type::Result { ok, err } => {
            fallible_payload_covered(ok, cx) && fallible_payload_covered(err, cx)
        }
        _ => false,
    }
}

/// An optional/fallible payload (`T` in `T?`, or `ok`/`err` in `T ? E`) the subset
/// can lower: a scalar, Char, String, a covered struct/enum, a covered collection,
/// `()` (the ok payload of fallible `run`, rendered as `()`), or sema's
/// default error type `Err` (`Type::Named("Err")`, which `cx.rust_type`
/// lowers to the Prelude-owned `JetErr`).
pub(crate) fn fallible_payload_covered(ty: &Type, cx: &Cx) -> bool {
    // c109 Phase 30: a type-variable payload (`T` in a generic fn's `T?` return —
    // `largest<T: Comparable>() -> (T?)`). A type var renders via `cx.rust_type` to the
    // bare letter (`Option<T>`), and `value(best)`/`null` lower to `Some(user_best)`/`None`
    // byte-identically (no clone/box decision). A type var only appears where a type param
    // is in scope (sema guarantees), so an `Option<T>` payload is total.
    if is_type_var_param_ty(ty, cx) {
        return true;
    }
    if let Type::Named(n) = ty {
        if n == "Unit" {
            return true;
        }
        if n == crate::Syntax::TYPE_ERR {
            return true;
        }
        if n == "CryptoError" {
            return true;
        }
        // c109 Phase 21 / D-TUPLE-DESTRUCT1: `Closed` is the err type of
        // `Receiver.receive()` → `Result<T, Closed>` (Source/Collections.rs
        // `receiver_method_return`). It renders
        // via `cx.rust_type` to `{root}jet_std::Closed` (`core_rust_type_name`), so a
        // `T ? Closed` payload (the unwrap target of `ch.receive() ?? …`) is byte-identical.
        if n == "Closed" {
            return true;
        }
        // D-REGEXENGINE1=A: a regex `Match` (the payload of `re.match()`'s `Match?`).
        // It renders via `cx.rust_type` to `jet_std::JetRegexMatch`.
        if n == "Match" {
            return true;
        }
    }
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || is_covered_distinct_ty(ty, cx)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_generic_struct_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        || is_covered_view_ty(ty, cx)
        || is_covered_data_ty(ty, cx)
        // c109 Phase 24: a FOREIGN value-type payload (`Note?` on a `ParsedResult` field —
        // `Note` is an imported struct). It renders via `cx.rust_type` to its own Rust
        // head; an `Option<Note>` is byte-identical (the `value(n)`/`null` constructor is
        // in-subset, the field read plain/sema-cloned).
        || is_covered_foreign_value_ty(ty, cx)
        // D-MEM1 S6: a `Pool<T>`/`Id<T>`/`Shared<T>` payload (`parent: Id<Node>?`'s
        // `Id<Node>` — the parent-pointer-tree shape D-POOLID-API1's ballot named).
        || is_covered_pool_ty(ty, cx)
        || is_covered_shared_ty(ty, cx)
        || is_covered_shared_guard_ty(ty, cx)
        || is_covered_shared_weak_ty(ty, cx)
}

/// c109 Phase 5: `ty` is a list `[E]` or map `[K: V]` the subset can lower. The
/// element/key/value types must themselves be covered *value* types — scalar,
/// Char, String, a covered distinct/struct/enum, or a nested covered collection — so the
/// literal/index/iteration lowerings reproduce the AST path without any clone/box
/// decision the subset can't make from total facts. A `FixedList` (`[E#N]`, D-FIXARR1) is
/// covered exactly like a `List`: indexing reads the element type off the base, and a fan-out
/// expression already produces a `[T#N]` value (Rust `[E; N]`). Widening to `[T]` (Vec)
/// when passed to a List slot is handled by `TCallArg.widen_to_vec` — so a `[E#N]`
/// param/return/element is covered once its element type is covered.
pub(crate) fn is_covered_collection_ty(ty: &Type, cx: &Cx) -> bool {
    match ty {
        Type::List(inner) => collection_elem_covered(inner, cx),
        Type::FixedList { elem, .. } => collection_elem_covered(elem, cx),
        Type::Map { key, value, .. } => {
            collection_elem_covered(key, cx) && collection_elem_covered(value, cx)
        }
        _ => false,
    }
}

/// A list/map element, key, or value type the subset can lower: a scalar, Char,
/// String, a sema-proved view, a covered distinct/struct/enum, or a nested
/// covered collection. Anything else excludes the owning collection.
pub(crate) fn collection_elem_covered(ty: &Type, cx: &Cx) -> bool {
    ty.is_scalar()
        || matches!(ty, Type::Char | Type::String)
        || matches!(ty, Type::Named(name) if name == "UiNode")
        // c109 Phase 17: a type-variable element (`[T]` in a generic fn). A type var only
        // appears where a type param is in scope (sema guarantees), and renders by value
        // via `cx.rust_type` (`Vec<T>`), so a `[T]` list param/return/local is covered.
        // c148: pass cx so multi-char params are recognized.
        || is_type_var_param_ty(ty, cx)
        || is_covered_distinct_ty(ty, cx)
        || is_covered_struct_ty(ty, cx)
        || is_covered_enum_ty(ty, cx)
        || is_covered_collection_ty(ty, cx)
        || is_covered_view_ty(ty, cx)
        // c109 Phase 21: a `[Task<Unit>]` worker list (34_parallel_scan) — a concurrency
        // handle element renders via `cx.rust_type` (`Vec<Jet…<…>>`) like any value type.
        || is_covered_concurrency_ty(ty, cx)
        // c109 Phase 30: a TRAIT-OBJECT element (`[Shape]` → `Vec<Box<dyn user_Shape>>`).
        // Each element is a `Box::new(<lit>) as Box<dyn …>` (the trait-coerced literal),
        // and `.each` over such a list dispatches via `jet_list_each_ref` (the `EachRef`
        // closure op, already built — `list_carries_trait`). The element renders via
        // `cx.rust_type` to `Box<dyn user_<Trait>>`, byte-identical to the AST path.
        || is_covered_trait_object_ty(ty, cx)
        // c109 Phase 24: a FOREIGN value-type element — the prelude JSON enum (`[JSON]` /
        // `[String: JSON]`) OR a cross-module imported user struct/enum (`[String: Note]`
        // where `Note` is an `import_ns` struct). These render via `cx.rust_type` to their
        // own Rust head ({root}jet_std::JSON / {root}{mod}::user_<Name>), and a foreign
        // element is moved/cloned by its own sub-expression (a construction or a bound
        // value), so the owning collection's `.iter().cloned()` / per-key/value clone is
        // byte-identical. (A foreign METHOD is still out of subset, so a fn that calls one
        // is excluded by that call — the recurring "cover the value type, let the next
        // uncovered node exclude its fn" seam.)
        || is_covered_foreign_value_ty(ty, cx)
        // D-MEM1 S6: a `Pool<T>`/`Id<T>`/`Shared<T>` element (`[Id<Node>]` — a
        // parent-pointer tree's `children` field, or `.ids()`'s `[Id<T>]` result).
        || is_covered_pool_ty(ty, cx)
        || is_covered_shared_ty(ty, cx)
        || is_covered_shared_guard_ty(ty, cx)
        || is_covered_shared_weak_ty(ty, cx)
}
