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
        "standard library modules expose only their documented M10 items".to_string(),
        fix,
        Some(span),
    )
}

/// E2411 (D-SERDE): a type used with an encoding verb can't be (de)serialized — it
/// holds something with no wire form (a closure, handle, …), or a user type that
/// hasn't opted in with `@[Codable]`/`@[Encode]`/`@[Decode]`.
pub(crate) fn e2411(ty: &str, encode: bool, span: Span) -> Diagnostic {
    let (verb, marker) = if encode {
        ("serialized", "`@[Codable]` or `@[Encode]`")
    } else {
        ("decoded", "`@[Codable]` or `@[Decode]`")
    };
    Diagnostic::error(
        "E2411",
        format!("{ty} can't be {verb}"),
        format!("only types that opt in (and their fields) have a wire form; {ty} does not"),
        format!("add {marker} to {ty}, or remove it from the encoded value"),
        Some(span),
    )
}

/// E2407: `#[Rename(...)]` needs a single string-literal wire key.
pub(crate) fn e2407(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2407",
        "`#[Rename(...)]` needs a string literal".to_string(),
        "the wire key is a constant string, e.g. `#[Rename(\"customer\")]`".to_string(),
        "pass one quoted string — `#[Rename(\"wire_name\")]`".to_string(),
        Some(span),
    )
}

/// E2408: `#[Flatten]` needs a field whose type is itself a Codable struct.
pub(crate) fn e2408(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2408",
        format!("`#[Flatten]` on `{field}` needs a struct-typed field"),
        "flatten splices another struct's keys into this object, so the field must be a `@[Codable]` struct — not a primitive, list, or map".to_string(),
        format!("give `{field}` a `@[Codable]` struct type, or drop `#[Flatten]`"),
        Some(span),
    )
}

/// E2414 (Card #131 / D-SERDE5): a `#[Default(expr)]` argument didn't evaluate
/// to a compile-time constant. The default is baked into the program to fill a
/// missing field during decode, so it must be known at compile time — a literal
/// or a `comptime`-evaluable expression — not a value read from runtime state.
pub(crate) fn e2414(field: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2414",
        format!("`#[Default(...)]` on `{field}` must be a compile-time constant"),
        "a decode default is baked into the program, so its value has to be known at compile time; this expression can only be computed at runtime".to_string(),
        "use a literal or a `comptime`-evaluable expression — e.g. `#[Default(8080)]`, `#[Default(Color.Red)]`, or `#[Default([1, 2])]`".to_string(),
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
fn apply_serde_ok(
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

pub(crate) fn is_encodable_ty(ty: &Type, reg: &TraitRegistry) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => true,
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_encodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_encodable_ty(elem, reg),
        Type::Map { key, value, .. } => matches!(**key, Type::String) && is_encodable_ty(value, reg),
        // A non-local type (imported) is trusted; a local one must derive Encode.
        Type::Named(n) => {
            is_json_type_name(n)
                || !reg.local_types.contains(n)
                || reg.implements_trait(n, crate::Generics::ENCODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::ENCODE, &|t| {
                is_encodable_ty(t, reg)
            })
        }
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
        | Type::IntN { .. }
        | Type::Float32 => true,
        Type::List(e) | Type::Option(e) | Type::Shared(e) => is_decodable_ty(e, reg),
        Type::FixedList { elem, .. } => is_decodable_ty(elem, reg),
        Type::Map { key, value, .. } => matches!(**key, Type::String) && is_decodable_ty(value, reg),
        Type::Named(n) => {
            !reg.local_types.contains(n) || reg.implements_trait(n, crate::Generics::DECODE)
        }
        Type::Apply { name, args } => {
            apply_serde_ok(name, args, reg, crate::Generics::DECODE, &|t| {
                is_decodable_ty(t, reg)
            })
        }
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
                                Syntax::ATTR_SKIP | Syntax::ATTR_FLATTEN
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

fn marker_is_string_literal(m: &crate::AST::Marker) -> bool {
    matches!(
        m.args.first(),
        Some(Expr::Str(parts, _)) if parts.len() == 1 && matches!(parts[0], crate::AST::StrPart::Lit(_))
    )
}

/// D-SERDE: validate serde markers on every `@[Codable]`/`@[Encode]`/`@[Decode]` type
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
        // D-SERDE12: generic `@[Codable]` is first-class — no `type_params > 0`
        // gate. The per-field checks below run on generic types unchanged; a type
        // param `T` reads as a non-local `Type::Named`, so it's trusted here and
        // the codability obligation falls on the use site (E0905).
        // Container `#[RenameAll(style)]` casing menu (E2409).
        for m in container {
            if m.name == Syntax::ATTR_RENAME_ALL {
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
                let skip = f.serde_markers.iter().any(|m| m.name == Syntax::ATTR_SKIP);
                let flatten = f
                    .serde_markers
                    .iter()
                    .any(|m| m.name == Syntax::ATTR_FLATTEN);
                for m in &f.serde_markers {
                    // E2407: `#[Rename]` needs a string literal.
                    if m.name == Syntax::ATTR_RENAME && !marker_is_string_literal(m) {
                        out.push(e2407(m.span));
                    }
                }
                if flatten && !is_struct_named(&f.ty) {
                    out.push(e2408(&f.name, f.name_span));
                    continue;
                }
                if skip || flatten {
                    continue;
                }
                // E2411: every encoded/decoded field must have a wire form.
                if enc && !is_encodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty.show(), true, f.name_span));
                }
                if dec && !is_decodable_ty(&f.ty, reg) {
                    out.push(e2411(&f.ty.show(), false, f.name_span));
                }
            }
        }
        if let Item::Enum(e) = item {
            for v in &e.variants {
                for m in &v.serde_markers {
                    if m.name == Syntax::ATTR_RENAME && !marker_is_string_literal(m) {
                        out.push(e2407(m.span));
                    }
                }
                let tys: Vec<&Type> = match &v.payload {
                    crate::AST::VariantPayload::Unit => vec![],
                    crate::AST::VariantPayload::Single(t, _) => vec![t],
                    crate::AST::VariantPayload::Named(fs) => fs.iter().map(|f| &f.ty).collect(),
                };
                for t in tys {
                    if enc && !is_encodable_ty(t, reg) {
                        out.push(e2411(&t.show(), true, v.name_span));
                    }
                    if dec && !is_decodable_ty(t, reg) {
                        out.push(e2411(&t.show(), false, v.name_span));
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
fn is_empty_string_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Str(parts, _) if parts.iter().all(|p| matches!(p, crate::AST::StrPart::Lit(s) if s.is_empty()))
    )
}

/// D-A11YGATE1=B: the literal text of `expr` when it's a plain (non-interpolated)
/// string literal, else `None`.
fn literal_string_value(expr: &Expr) -> Option<String> {
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
