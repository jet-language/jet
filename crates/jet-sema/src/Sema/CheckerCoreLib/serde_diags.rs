use crate::AST::{Expr, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Diagnostics::suggest_field;
use crate::Syntax;
use crate::Traits::TraitRegistry;
use super::core_types::is_json_type_name;
use super::module_items::core_module_items;

/// E2-M15: modules that require an OS and are forbidden in `--freestanding` builds.
pub(crate) fn is_freestanding_forbidden(module: &str) -> bool {
    matches!(
        module,
        "core.files" | "core.watcher" | "core.io" | "core.net" | "core.tls" | "core.tasks"
            | "core.process" | "core.time" | "jet.http" | "jet.log"
            // D-TERM1: terminal I/O requires an OS terminal device.
            | "core.term"
            // U13 (D-JPK-SECRETCRYPTO1): reading the encrypted repo store is
            // filesystem I/O — same OS dependency as `core.files`.
            | "core.vault"
    )
}

/// Return a short display name for the module alias (the part after the dot).
pub(crate) fn module_short_name(module: &str) -> &str {
    module.split('.').last().unwrap_or(module)
}

/// Fix hint for E3301 depending on the forbidden module.
pub(crate) fn freestanding_hint(module: &str) -> &'static str {
    match module {
        "core.files" => {
            "Embed the data at compile time with `@embed(\"file\")`, or build without `--freestanding`."
        }
        "core.net" | "core.tls" | "jet.http" => {
            "Freestanding targets have no network stack. Build without `--freestanding`, or use a bare-metal driver."
        }
        "core.tasks" => {
            "OS threads are not available without an OS. Use cooperative or interrupt-driven concurrency."
        }
        "core.io" => {
            "Standard I/O requires an OS. Use a platform-specific write routine or build without `--freestanding`."
        }
        "core.process" | "core.time" => {
            "System calls are not available in a freestanding build. Build without `--freestanding`."
        }
        "jet.log" => {
            "The log module writes to stderr (an OS resource). Use a bare-metal write routine or build without `--freestanding`."
        }
        "core.term" => {
            "Terminal I/O requires an OS terminal device. Build without `--freestanding`."
        }
        _ => "Build without `--freestanding`, or replace this call with a core-level alternative.",
    }
}

pub(crate) fn unknown_core_item(module: &str, name: &str, span: Span) -> Diagnostic {
    if module == "core.time" && name == "clock" {
        return Diagnostic::error(
            "E1004",
            "`time.clock` was retired".to_string(),
            "deterministic fresh values use a type-owned `new` constructor (D-SHAPE-CTORVERB1)".to_string(),
            "use `Clock.new(seed)`".to_string(),
            Some(span),
        );
    }
    if module == "core.time.expiring" && name == "new" {
        return Diagnostic::error(
            "E1004",
            "`expiring.new` was retired".to_string(),
            "fresh values use a type-owned `new` constructor (D-SHAPE-CTORVERB1)".to_string(),
            "use `ExpiringValue.new(value, ttl, clock)`".to_string(),
            Some(span),
        );
    }
    if module == "core.event" && name == "policy_async" {
        return Diagnostic::error(
            "E1004",
            "`event.policy_async` was retired".to_string(),
            "a synchronous `Event<T>` cannot provide real queued dispatch; `AsyncEvent<T, E>` is the sole asynchronous event model".to_string(),
            "use `event.async_result<T, E>(AsyncPolicy.{ capacity: n, overflow: .Block }, FailurePolicy.Collect)`".to_string(),
            Some(span),
        );
    }
    if module == "jet.crypto" && name == "constant_time_eq" {
        return Diagnostic::error(
            "E1004",
            "`crypto.constant_time_eq` was retired".to_string(),
            "`eq` is not one of Jet's blessed API abbreviations".to_string(),
            "use `crypto.constant_time_equal_bytes(a, b)`".to_string(),
            Some(span),
        );
    }
    let items = core_module_items(module);
    let mut fix = if items.is_empty() {
        "import a specific core module, like `import core.files as fs;`".to_string()
    } else {
        format!("use one of: {}", items.join(", "))
    };
    if let Some(s) = suggest_field(name, &items) {
        fix = format!("did you mean `{}`?", s);
    }
    Diagnostic::error(
        "E1004",
        format!("`{}` has no item `{}`", module, name),
        "standard library modules expose a fixed set of public items".to_string(),
        fix,
        Some(span),
    )
}

/// E2411 (D-SERDE): a type used with an encoding verb can't be (de)serialized — it
/// holds something with no wire form (a closure, handle, …), or a user type that
/// hasn't opted in with `#[Codable]`/`#[Encode]`/`#[Decode]`.
pub(crate) fn e2411(ty: &Type, encode: bool, span: Span) -> Diagnostic {
    let shown = ty.show();
    let ty = shown.as_str();
    if matches!(ty_name_of(&shown), name if name == crate::Syntax::TYPE_U64) {
        let (verb, fix) = if encode {
            (
                "serialized",
                "convert the U64 to Int after checking it fits, or encode it as Text explicitly",
            )
        } else {
            (
                "decoded",
                "decode an Int or Text and convert it to U64 explicitly",
            )
        };
        return Diagnostic::error(
            "E2411",
            format!("U64 can't be {verb}"),
            "Codable uses the shared DataTree model, whose Int values are signed 64-bit; U64 cannot round-trip every value".to_string(),
            fix.to_string(),
            Some(span),
        );
    }
    let (verb, marker) = if encode {
        ("serialized", "`#[Codable]` or `#[Encode]`")
    } else {
        ("decoded", "`#[Codable]` or `#[Decode]`")
    };
    Diagnostic::error(
        "E2411",
        format!("{ty} can't be {verb}"),
        format!("only types that opt in (and their fields) have a wire form; {ty} does not"),
        format!("add {marker} to {ty}, or remove it from the encoded value"),
        Some(span),
    )
}

/// The bare type name inside a shown type, i.e. `U64` in
/// `U64 (a 64-bit whole number, 0 to 18446744073709551615)`.
fn ty_name_of(shown: &str) -> &str {
    shown.split_once(" (").map_or(shown, |(name, _)| name)
}

fn e2411_unknown_union_shape(union_ty: &str, member: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2411",
        format!(
            "union `{union_ty}` can't be decoded — `{member}` has no compiler-known wire shape"
        ),
        format!(
            "`{member}` uses a custom or imported decoder; anonymous unions need each member's outer wire shape before codegen"
        ),
        "use a compiler-derived codec, a configured tagged enum, or a #CodableAsBase distinct type"
            .to_string(),
        Some(span),
    )
}

/// E2415 (D-UNIONTYPE1=A): two Codable union members share a primary wire shape,
/// so decode cannot pick a unique member without declaration-order guessing.
pub(crate) fn e2415(union_ty: &str, a: &str, b: &str, shape: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2415",
        format!("union `{union_ty}` can't be decoded — `{a}` and `{b}` share wire shape `{shape}`"),
        "anonymous-union decode picks a member by wire shape; two members with the same shape would force an arbitrary order".to_string(),
        "use a named enum with an explicit tag, or change the members so each has a distinct wire shape".to_string(),
        Some(span),
    )
}

fn validate_union_decode_shapes(
    items: &[crate::AST::Item],
    ty: &Type,
    span: Span,
    out: &mut Vec<Diagnostic>,
) {
    fn label(ty: &Type) -> String {
        ty.show().trim_matches('`').to_string()
    }

    if let Type::Union(members) = ty {
        let mut seen: Vec<(crate::AST::SerdeWireShape, &Type)> = Vec::new();
        for member in members {
            let Some(shapes) = crate::AST::resolved_decode_wire_shapes(items, member) else {
                out.push(e2411_unknown_union_shape(
                    &ty.show(),
                    &label(member),
                    span,
                ));
                continue;
            };
            for shape in shapes {
                if let Some((_, previous)) = seen.iter().find(|(known, _)| *known == shape) {
                    out.push(e2415(
                        &ty.show(),
                        &label(previous),
                        &label(member),
                        shape.name(),
                        span,
                    ));
                } else {
                    seen.push((shape, member));
                }
            }
        }
    }
    match ty {
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. }
        | Type::FixedList { elem: inner, .. } => {
            validate_union_decode_shapes(items, inner, span, out)
        }
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            validate_union_decode_shapes(items, key, span, out);
            validate_union_decode_shapes(items, value, span, out);
        }
        Type::Apply { args, .. } | Type::Union(args) => {
            for arg in args {
                validate_union_decode_shapes(items, arg, span, out);
            }
        }
        Type::Tuple(fields) => {
            for (_, field) in fields {
                validate_union_decode_shapes(items, field, span, out);
            }
        }
        Type::Fn { params, ret, .. } => {
            for param in params {
                validate_union_decode_shapes(items, param, span, out);
            }
            if let Some(ret) = ret {
                validate_union_decode_shapes(items, ret, span, out);
            }
        }
        _ => {}
    }
}

/// E2408: `#[Flatten]` needs a field whose type is itself a Codable struct.
pub(crate) fn e2408(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2408",
        format!("`#[Flatten]` on `{field}` needs a struct-typed field"),
        "flatten splices another struct's keys into this object, so the field must be a `#[Codable]` struct — not a primitive, list, or map".to_string(),
        format!("give `{field}` a `#[Codable]` struct type, or drop `#[Flatten]`"),
        Some(span),
    )
}

/// E2414 (Card #131 / D-SERDE5 / D-FIELDDEF1): a field `=` default didn't evaluate
/// to a compile-time constant. The default is baked into the program to fill a
/// missing field during decode, so it must be known at compile time — a literal
/// or a `comptime`-evaluable expression — not a value read from runtime state.
pub(crate) fn e2414(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2414",
        format!("`{field}`'s `=` default must be a compile-time constant"),
        "a decode default is baked into the program, so its value has to be known at compile time; this expression can only be computed at runtime".to_string(),
        "use a literal or a `comptime`-evaluable expression — e.g. `port: Int = 8080` or `ports: [Int] = [80, 443]`".to_string(),
        Some(span),
    )
}

/// E2409: `#[RenameAll(...)]` names a casing style outside the closed menu.
pub(crate) fn e2409(style: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2409",
        format!("`#[RenameAll({style})]` isn't a known casing style"),
        "the wire-casing menu is `camel`, `snake`, `pascal`, `kebab`, `screaming`".to_string(),
        "pick one of `camel` / `snake` / `pascal` / `kebab` / `screaming`".to_string(),
        Some(span),
    )
}

/// D-SERDE9/10: `Name<args>` satisfies the serde `trait_name` when `Name` derives
/// it (or is imported/non-local, hence trusted) and every type arg at a
/// wire-reaching position satisfies it too (`elem_ok`). A phantom/skip-only param
/// position imposes no obligation, so `Id<Kind>` is fine for any `Kind`.
pub(super) fn apply_serde_ok(
    name: &str,
    args: &[Type],
    reg: &TraitRegistry,
    trait_name: &str,
    elem_ok: &dyn Fn(&Type) -> bool,
) -> bool {
    let head_ok = !reg.local_types.contains(name) || reg.implements_trait(name, trait_name);
    if !head_ok {
        return false;
    }
    match reg.serde_wire_params.get(name) {
        // Local generic Codable type: only the wire-reaching args must be codable.
        Some(idxs) => idxs
            .iter()
            .all(|&i| args.get(i).map_or(true, |t| elem_ok(t))),
        // No recorded wire params (imported/non-generic): trust every arg is fine
        // only if each is codable — be conservative and check them all.
        None => args.iter().all(|t| elem_ok(t)),
    }
}

pub(crate) fn sized_int_has_datatree_form(ty: &Type) -> bool {
    !matches!(ty, Type::IntN { signed: false, bits: 64 })
}

pub(crate) fn is_encodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32 => true,
        Type::IntN { .. } => sized_int_has_datatree_form(ty),
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_encodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_encodable_ty(elem, reg),
        Type::Map { key, value, .. } => matches!(**key, Type::String) && is_encodable_ty(value, reg),
        // A non-local type (imported) is trusted; a local one must derive Encode.
        Type::Named(n) => {
            n == "Decimal"
                || is_json_type_name(n)
                || !reg.local_types.contains(n)
                || reg.implements_trait(n, crate::Generics::ENCODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::ENCODE, &|t| {
                is_encodable_ty(t, reg)
            })
        }
        Type::Union(members) => members.iter().all(|m| is_encodable_ty(m, reg)),
        _ => false,
    }
}

pub(crate) fn is_decodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32 => true,
        Type::IntN { .. } => sized_int_has_datatree_form(ty),
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_decodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_decodable_ty(elem, reg),
        Type::Map { key, value, .. } => matches!(**key, Type::String) && is_decodable_ty(value, reg),
        Type::Named(n) => {
            n == "Decimal" || !reg.local_types.contains(n) || reg.implements_trait(n, crate::Generics::DECODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::DECODE, &|t| {
                is_decodable_ty(t, reg)
            })
        }
        Type::Union(members) => members.iter().all(|m| is_decodable_ty(m, reg)),
        _ => false,
    }
}

/// Generated serde bodies are implementation detail. When the declaration pass
/// already reports E2411 for a field, checking that synthetic body would only
/// repeat the same problem as E0905 at a generated-code span.
pub(crate) fn invalid_serde_derive_impls(
    items: &[crate::AST::Item],
    reg: &TraitRegistry,
) -> std::collections::HashSet<(String, String)> {
    use crate::AST::Item;
    let mut invalid = std::collections::HashSet::new();
    for item in items {
        match item {
            Item::Struct(s) => {
                let fields = s
                    .fields
                    .iter()
                    .filter(|f| {
                        !f.serde_markers.iter().any(|m| {
                            matches!(
                                m.name.as_str(),
                                Syntax::MARKER_SKIP | Syntax::MARKER_FLATTEN
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if s.derives.iter().any(|(t, _)| t == crate::Generics::ENCODE)
                    && fields.iter().any(|f| !is_encodable_ty(&f.ty, reg))
                {
                    invalid.insert((s.name.clone(), crate::Generics::ENCODE.to_string()));
                }
                if s.derives.iter().any(|(t, _)| t == crate::Generics::DECODE)
                    && fields.iter().any(|f| !is_decodable_ty(&f.ty, reg))
                {
                    invalid.insert((s.name.clone(), crate::Generics::DECODE.to_string()));
                }
            }
            Item::Enum(e) => {
                let payloads = e
                    .variants
                    .iter()
                    .flat_map(|v| match &v.payload {
                        crate::AST::VariantPayload::Unit => Vec::new(),
                        crate::AST::VariantPayload::Single(t, _) => vec![t],
                        crate::AST::VariantPayload::Named(fields) => {
                            fields.iter().map(|f| &f.ty).collect()
                        }
                    })
                    .collect::<Vec<_>>();
                if e.derives.iter().any(|(t, _)| t == crate::Generics::ENCODE)
                    && payloads.iter().any(|t| !is_encodable_ty(t, reg))
                {
                    invalid.insert((e.name.clone(), crate::Generics::ENCODE.to_string()));
                }
                if e.derives.iter().any(|(t, _)| t == crate::Generics::DECODE)
                    && payloads.iter().any(|t| !is_decodable_ty(t, reg))
                {
                    invalid.insert((e.name.clone(), crate::Generics::DECODE.to_string()));
                }
            }
            _ => {}
        }
    }
    invalid
}

/// True for a `#[Flatten]`-able field type: a named struct (not a primitive/list/map).
fn is_struct_named(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if !is_json_type_name(n))
}

/// D-SERDE: validate serde markers on every `#[Codable]`/`#[Encode]`/`#[Decode]` type
/// (E2407–E2412). Runs after the trait registry is built so field types resolve. This
/// keeps generated code rustc-clean (I2): a field with no wire form is caught here, not
/// by rustc on the emitted `impl`.
pub(crate) fn validate_serde_items(
    items: &[crate::AST::Item],
    reg: &TraitRegistry,
) -> Vec<Diagnostic> {
    use crate::AST::Item;
    let mut out = Vec::new();
    for item in items {
        let (derives, container): (&[(String, Span)], &[crate::AST::Marker]) = match item {
            Item::Struct(s) => (&s.derives, &s.serde_markers),
            Item::Enum(e) => (&e.derives, &e.serde_markers),
            _ => continue,
        };
        let enc = derives.iter().any(|(t, _)| t == crate::Generics::ENCODE);
        let dec = derives.iter().any(|(t, _)| t == crate::Generics::DECODE);
        if !enc && !dec {
            continue;
        }
        // D-SERDE12: generic `#[Codable]` is first-class — no `type_params > 0`
        // gate. The per-field checks below run on generic types unchanged; a type
        // param `T` reads as a non-local `Type::Named`, so it's trusted here and
        // the codability obligation falls on the use site (E0905).
        // Container `#[RenameAll(style)]` casing menu (E2409).
        for m in container {
            if m.name == Syntax::MARKER_RENAME_ALL {
                match m.args.first() {
                    Some(Expr::Ident(style, sp)) => {
                        if !matches!(
                            style.as_str(),
                            Syntax::RENAME_ALL_CAMEL
                                | Syntax::RENAME_ALL_SNAKE
                                | Syntax::RENAME_ALL_PASCAL
                                | Syntax::RENAME_ALL_KEBAB
                                | Syntax::RENAME_ALL_SCREAMING
                        ) {
                            out.push(e2409(style, *sp));
                        }
                    }
                    _ => out.push(e2409("?", m.span)),
                }
            }
        }
        if let Item::Struct(s) = item {
            for f in &s.fields {
                let skip = f.serde_markers.iter().any(|m| m.name == Syntax::MARKER_SKIP);
                let flatten = f
                    .serde_markers
                    .iter()
                    .any(|m| m.name == Syntax::MARKER_FLATTEN);
                if flatten && !is_struct_named(&f.ty) {
                    out.push(e2408(&f.name, f.name_span));
                    continue;
                }
                if skip || flatten {
                    continue;
                }
                // E2411: every encoded/decoded field must have a wire form.
                if enc && !is_encodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty, true, f.name_span));
                }
                if dec && !is_decodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty, false, f.name_span));
                }
                if dec {
                    validate_union_decode_shapes(items, &f.ty, f.name_span, &mut out);
                }
            }
        }
        if let Item::Enum(e) = item {
            for v in &e.variants {
                let tys: Vec<&Type> = match &v.payload {
                    crate::AST::VariantPayload::Unit => vec![],
                    crate::AST::VariantPayload::Single(t, _) => vec![t],
                    crate::AST::VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
                };
                for t in tys {
                    if enc && !is_encodable_ty(t, reg) {
                        out.push(e2411(&t, true, v.name_span));
                    }
                    if dec && !is_decodable_ty(t, reg) {
                        out.push(e2411(&t, false, v.name_span));
                    }
                    if dec {
                        validate_union_decode_shapes(items, t, v.name_span, &mut out);
                    }
                }
            }
        }
    }
    out
}

pub(crate) fn wrong_core_arity(name: &str, want: usize, got: usize, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0104",
        format!(
            "`{}` expects {} argument{}, got {}",
            name,
            want,
            if want == 1 { "" } else { "s" },
            got
        ),
        "every argument must match a standard library function parameter".to_string(),
        format!("check the call to `{}`", name),
        Some(span),
    )
}

/// D-A11YGATE1=B: is `expr` a single, non-interpolated literal string part?
/// Shared by E2930 (empty-label check) and E2931 (duplicate-label check).
pub(super) fn is_empty_string_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Str(parts, _) if parts.iter().all(|p| matches!(p, crate::AST::StrPart::Lit(s) if s.is_empty()))
    )
}

/// D-A11YGATE1=B: the literal text of `expr` when it's a plain (non-interpolated)
/// string literal, else `None`.
pub(super) fn literal_string_value(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else {
        return None;
    };
    if parts.len() != 1 {
        return None;
    }
    match &parts[0] {
        crate::AST::StrPart::Lit(s) => Some(s.clone()),
        _ => None,
    }
}

/// D-A11YGATE1=B (c134 Phase 6, E2930): an interactive-role `UiNode` with an
/// empty accessible label.
pub(crate) fn a11y_unlabeled_control(role: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "E2930",
        format!("this {role} has no accessible label"),
        "screen readers announce a control by its accessible label — an empty label is invisible to assistive tech".to_string(),
        "pass a real label, e.g. `ui.node_role(\"Submit\", w, h, ui.aria_role_button())`".to_string(),
        Some(span),
    )
}

/// D-A11YGATE1=B (c134 Phase 6, E2931): two interactive nodes in the same
/// inline focus group share an accessible label.
pub(crate) fn a11y_duplicate_label(label: &str, span: Span) -> Diagnostic {
    Diagnostic::lint(
        "E2931",
        format!("two interactive nodes both have the label \"{label}\""),
        "assistive tech announces controls by their label — identical labels make them indistinguishable (WCAG 2.5.3)".to_string(),
        "give each interactive node a distinct, descriptive label".to_string(),
        Some(span),
    )
}

/// D-REACT1=B (E2910): a `reactive.derived`/`effect` argument that isn't a lambda.
pub(crate) fn reactive_not_lambda(kind: &str, got: &Type, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2910",
        format!("`reactive.{kind}` needs a lambda, not {}", got.show()),
        format!(
            "a reactive {} is built from a `() => …` body so it can re-run when a signal changes",
            kind
        ),
        format!("write `reactive.{kind}(() => {{ … }})`"),
        Some(span),
    )
}

/// D-REACT1=B (E2911): a `reactive.derived`/`effect` lambda that takes parameters.
pub(crate) fn reactive_lambda_arity(kind: &str, n: usize, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2911",
        format!(
            "`reactive.{kind}` needs a zero-parameter lambda, got {} parameter{}",
            n,
            if n == 1 { "" } else { "s" }
        ),
        "the body takes no arguments — it reads the signals it depends on via `.get()`".to_string(),
        format!("write `reactive.{kind}(() => {{ … }})` with no parameters"),
        Some(span),
    )
}

/// D-REACT1=B (E2912): a `reactive.derived` whose lambda returns nothing.
pub(crate) fn reactive_derived_unit(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2912",
        "`reactive.derived` must compute and return a value".to_string(),
        "a derived value is recomputed from its signals; its lambda has to return the value"
            .to_string(),
        "return a value from the body, or use `reactive.effect(() => { … })` for a side effect"
            .to_string(),
        Some(span),
    )
}

/// D-REACT1=B (E2913): a reactive value type the library can't hold (e.g. a function).
pub(crate) fn reactive_bad_value_type(kind: &str, ty: &Type, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2913",
        format!("a reactive {} can't hold a {}", kind, ty.show()),
        "signals and derived values hold ordinary data so they can be copied to dependents".to_string(),
        "use a data value (number, text, list, struct, …); wrap behaviour in `reactive.effect` instead".to_string(),
        Some(span),
    )
}

/// D-NUMOPS1 (E1005): a `wrapping`/`saturating`/`checked` opt-in wasn't given a
/// single integer `+`/`-`/`*`/`/` to wrap.
pub(crate) fn overflow_opt_in_error(kind: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1005",
        format!("`{kind}` must wrap a single integer `+`, `-`, `*`, or `/`"),
        "the overflow opt-ins apply to one arithmetic operation on whole numbers".to_string(),
        format!("write it around one operation, e.g. `{kind}(a + b)`"),
        Some(span),
    )
}

/// D-NUMOPS1: the type of a numeric type-constant — `MIN`/`MAX` on any numeric
/// type, `INFINITY`/`NAN`/`EPSILON` on floats. `None` if `member` isn't one.
pub(crate) fn numeric_const_type(nt: &Type, member: &str) -> Option<Type> {
    match member {
        "MIN" | "MAX" => Some(nt.clone()),
        "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON" if nt.is_float() => Some(nt.clone()),
        _ => None,
    }
}

/// D-SG9/D-NUMOPS1 (E1003): an integer literal doesn't fit its fixed-width type.
/// `U8` keeps its byte-framed wording; other widths get the general range message.
pub(crate) fn int_range_error(signed: bool, bits: u8, span: Span) -> Diagnostic {
    let (lo, hi) = crate::AST::int_range(signed, bits);
    let spelling = crate::AST::int_spelling(signed, bits);
    // "an I8" (the letter I reads as a vowel) vs "a U8".
    let article = if signed { "an" } else { "a" };
    let why = if !signed && bits == 8 {
        "binary APIs use one byte per value".to_string()
    } else {
        format!("{article} {spelling} is a fixed-width number — values outside its range can't fit")
    };
    Diagnostic::error(
        "E1003",
        format!("{article} {spelling} holds {lo}..{hi}"),
        why,
        format!("use a number from {lo} through {hi}"),
        Some(span),
    )
}
