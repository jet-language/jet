use super::*;

pub(crate) fn name_defined(
    name: &str,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
) -> bool {
    funcs.contains_key(name) || registry.contains(name) || consts.contains_key(name)
}

fn declared_type_contains_cell_guard(ty: &Type) -> bool {
    match ty {
        Type::Apply { name, .. }
            if matches!(name.as_str(), "CellReadGuard" | "CellEditGuard") =>
        {
            true
        }
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            declared_type_contains_cell_guard(inner)
        }
        Type::Map { key, value, .. } | Type::Result { ok: key, err: value } => {
            declared_type_contains_cell_guard(key) || declared_type_contains_cell_guard(value)
        }
        Type::Fn { params, ret, .. } => {
            params.iter().any(declared_type_contains_cell_guard)
                || ret
                    .as_deref()
                    .is_some_and(declared_type_contains_cell_guard)
        }
        Type::Tuple(fields) => fields
            .iter()
            .any(|(_, field)| declared_type_contains_cell_guard(field)),
        Type::FixedList { elem, .. } | Type::Tagged { inner: elem, .. } => {
            declared_type_contains_cell_guard(elem)
        }
        Type::Union(members) | Type::Apply { args: members, .. } => {
            members.iter().any(declared_type_contains_cell_guard)
        }
        _ => false,
    }
}

fn cell_guard_storage_diagnostic(place: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0217",
        format!("a Cell guard cannot be stored in {place}"),
        "a Cell guard is a temporary loan handle; storing it inside another value could keep the loan after its local scope ends"
            .to_string(),
        "keep the guard in a local name or a tuple, and use `.map(...)` or `.split(...)` for projections"
            .to_string(),
        Some(span),
    )
}

/// D-DIST1/D-DIST3: register a distinct type declaration.
pub(crate) fn register_distinct(
    d: &DistinctDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&d.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", d.name),
            format!("`{}` is provided by the language itself", d.name),
            "choose a different name for this distinct type".to_string(),
            Some(d.name_span),
        ));
        return;
    }
    if name_defined(&d.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &d.name,
            "every distinct type needs a unique name",
            d.name_span,
        ));
        return;
    }
    // E0129: base must be a concrete value type, not itself a distinct type.
    // We can only detect pre-registered distinct bases here; forward-declared
    // bases are checked lazily in sema (resolve_type / type check).
    if let Type::Named(base_name) = &d.base {
        if registry.is_distinct(base_name) {
            diags.push(Diagnostic::error(
                "E0129",
                format!(
                    "`{}` can't be built on `{}` — `{}` is itself a distinct type",
                    d.name, base_name, base_name
                ),
                format!(
                    "`distinct`-over-`distinct` chaining is not allowed in v1; `{}` is already a distinct type",
                    base_name
                ),
                format!("use `{}` directly, or build on the shared base type", base_name),
                Some(d.base_span),
            ));
            return;
        }
    }
    // D-RANGETYPE1: a range constraint (`distinct Int(0..10)`) only makes
    // sense on `Int` — reject it on any other base rather than silently
    // ignoring it.
    if let Some((lo, hi, range_span)) = &d.range {
        if d.base != Type::Int {
            diags.push(Diagnostic::error(
                "E0003",
                format!(
                    "a range constraint only works on `Int`, but `{}` is {}",
                    d.name,
                    d.base.show()
                ),
                "`distinct Base(lo..hi)` provably bounds a whole-number value".to_string(),
                format!("use `distinct Int({}..{})`, or drop the range", lo, hi),
                Some(*range_span),
            ));
        }
    }
    registry.types.insert(
        d.name.clone(),
        TypeDef::Distinct {
            base: d.base.clone(),
            is_numeric: d.is_numeric,
            is_comparable: d.is_comparable,
            is_printable: d.is_printable,
            is_codable_as_base: d.is_codable_as_base,
            range: d.range.map(|(lo, hi, _)| (lo, hi)),
        },
    );
}

/// D-TYPEALIAS1: register `alias Name<T> = …` — generic shortcuts only.
pub(crate) fn register_type_alias(
    a: &crate::AST::TypeAliasDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&a.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", a.name),
            format!("`{}` is provided by the language itself", a.name),
            "choose a different name for this type alias".to_string(),
            Some(a.name_span),
        ));
        return;
    }
    if name_defined(&a.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &a.name,
            "every type alias needs a unique name",
            a.name_span,
        ));
        return;
    }
    if a.type_params.is_empty() {
        diags.push(crate::Generics::e0324(a.name_span));
        return;
    }
    registry.types.insert(
        a.name.clone(),
        TypeDef::Alias {
            params: a.type_params.clone(),
            target: a.target.clone(),
        },
    );
}

/// S57 (M9.5): evaluate every `comptime name = expr;` in `items`. Purity and
/// fuel are enforced by the interpreter (E0951/E0952); panics surface as
/// E0953. Each result's Jet type is registered in `consts` so references
/// type-check, and the value is stashed on the item for codegen to inline.
pub(crate) fn eval_comptime_items(
    items: &mut [Item],
    consts: &mut HashMap<String, Type>,
    base_dir: &std::path::Path,
    diags: &mut Vec<Diagnostic>,
    // D-CTCORE1: module alias → Core path so the interpreter can evaluate
    // whitelisted pure Core calls (e.g. `comptime value = math.sqrt(4.0)`).
    core_imports: &HashMap<String, String>,
    mut embed_inputs_out: Option<&mut Vec<crate::AST::ComptimeInput>>,
) {
    if !items
        .iter()
        .any(|i| matches!(i, Item::Const(c) if c.is_comptime))
    {
        return;
    }
    let mut results: Vec<(String, crate::Comptime::CtValue)> = Vec::new();
    {
        // Comptime runs before the production serde expansion pass. Expand a
        // clone so the canonical TIR evaluator can call the same generated
        // Encode/Decode bodies without mutating or duplicating module items.
        let mut eval_items = items.to_vec();
        let mut ignored_early_serde_diags = Vec::new();
        super::Serde::expand_builtin_serde_items(
            &mut eval_items,
            &mut ignored_early_serde_diags,
        );
        let mut funcs: HashMap<String, &Func> = HashMap::new();
        let mut structs = HashMap::new();
        let mut externs: HashSet<String> = HashSet::new();
        for item in &eval_items {
            match item {
                Item::Func(f) => {
                    funcs.insert(f.name.clone(), f);
                }
                Item::Impl(implementation)
                    if matches!(
                        implementation.trait_name.as_deref(),
                        Some(crate::Generics::ENCODE | crate::Generics::DECODE)
                    ) =>
                {
                    for method in &implementation.methods {
                        funcs.insert(
                            format!("{}::{}", implementation.type_name, method.name),
                            method,
                        );
                    }
                }
                Item::Struct(s) => {
                    structs.insert(s.name.clone(), s);
                }
                Item::ExternRust(b) => {
                    for ef in &b.functions {
                        externs.insert(ef.name.clone());
                    }
                }
                _ => {}
            }
        }
        // Earlier comptime bindings are in scope for later ones.
        let mut globals: HashMap<String, crate::Comptime::CtValue> = HashMap::new();
        for item in items.iter() {
            if let Item::Const(c) = item {
                if c.is_comptime {
                    // D-CTCORE1: evaluate_with_imports so Core whitelist calls work.
                    match crate::Comptime::evaluate_with_imports_opts_collecting_structs(
                        &c.value,
                        &funcs,
                        &externs,
                        base_dir,
                        &globals,
                        core_imports,
                        false,
                        0,
                        &structs,
                    ) {
                        Ok((v, inputs)) => {
                            // `v.jet_type()` reads the element type off the value's
                            // first element (D-BIGINT1-adjacent `CtValue::jet_type`
                            // convention). For a fixed-return-type comptime builtin
                            // (e.g. `find(glob)`, always `[String]`) whose result is
                            // an EMPTY collection, that read has nothing to sample and
                            // falls back to a placeholder — wrong, and (via codegen)
                            // an unannotated `vec![]` rustc rejects as E0282 (I2). Prefer
                            // the builtin's known static return type when one applies.
                            let ty = comptime_builtin_fixed_return_type(
                                &c.value,
                                core_imports,
                                &v,
                            )
                            .unwrap_or_else(|| v.jet_type());
                            consts.insert(c.name.clone(), ty);
                            globals.insert(c.name.clone(), v.clone());
                            results.push((c.name.clone(), v));
                            if let Some(out) = embed_inputs_out.as_deref_mut() {
                                out.extend(inputs);
                            }
                        }
                        Err(d) => diags.push(d),
                    }
                }
            }
        }
    }
    for item in items.iter_mut() {
        if let Item::Const(c) = item {
            if c.is_comptime {
                c.ty = consts.get(&c.name).cloned();
                if let Some(pos) = results.iter().position(|(n, _)| n == &c.name) {
                    c.ct = Some(results.remove(pos).1);
                }
            }
        }
    }
}

/// A handful of comptime builtins have a fixed, non-polymorphic return type
/// regardless of arguments or result size — `find(glob)` always returns
/// `[String]` (D-CTFIND1/2), even when the glob matches nothing. Naming that
/// type here (rather than trusting `CtValue::jet_type()`, which samples the
/// first element and has nothing to sample when the result is empty) is what
/// lets codegen render a correctly-typed empty Rust collection instead of an
/// ambiguous `vec![]` (see `ConstDef::ty`).
fn comptime_builtin_fixed_return_type(
    value: &Expr,
    core_imports: &HashMap<String, String>,
    evaluated: &crate::Comptime::CtValue,
) -> Option<Type> {
    match value {
        Expr::Call(call) if call.name == Syntax::BUILTIN_FIND => {
            Some(Type::List(Box::new(Type::String)))
        }
        Expr::MethodCall {
            receiver,
            method,
            ..
        } if matches!(receiver.as_ref(), Expr::Ident(alias, _)
            if core_imports.get(alias).map(String::as_str) == Some("core.reactive.loadable")) =>
        {
            let unit = Type::Named("Unit".to_string());
            let payload = match evaluated {
                crate::Comptime::CtValue::Enum { args, .. } => {
                    args.first().map(|(_, value)| value.jet_type())
                }
                _ => None,
            };
            match method.as_str() {
                "idle" | "loading" => Some(Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![unit.clone(), unit],
                }),
                "loaded" => payload.map(|value| Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![value, unit],
                }),
                "failed" => payload.map(|error| Type::Apply {
                    name: "Loadable".to_string(),
                    args: vec![unit, error],
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Card #131 / D-SERDE5: pre-evaluate every `#[Default(expr)]` argument on a
/// `#[Codable]`/`#[Encode]`/`#[Decode]` struct field to a compile-time value,
/// stashed on the marker (`Marker::ct`). Runs after `eval_comptime_items`, so a
/// default may reference a `comptime` const. Codegen serializes this value and
/// the comptime decode tier reuses it, so the two tiers bake the same default —
/// a non-primitive `#[Default(expr)]` never silently degrades to
/// `Default::default()` (R11/R12). A non-const argument is E2414.
pub(crate) fn comptime_context_from_items(
    items: &[Item],
) -> (
    HashMap<String, Func>,
    HashSet<String>,
    HashMap<String, crate::Comptime::CtValue>,
) {
    let mut funcs = HashMap::new();
    let mut externs = HashSet::new();
    let mut globals = HashMap::new();
    for item in items {
        match item {
            Item::Func(f) => {
                funcs.insert(f.name.clone(), f.clone());
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
                for block in &s.trait_impls {
                    for m in &block.methods {
                        funcs.insert(m.name.clone(), m.clone());
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    funcs.insert(m.name.clone(), m.clone());
                }
            }
            Item::Const(c) if c.is_comptime => {
                if let Some(v) = &c.ct {
                    globals.insert(c.name.clone(), v.clone());
                }
            }
            Item::ExternRust(b) => {
                for ef in &b.functions {
                    externs.insert(ef.name.clone());
                }
            }
            Item::EffectDecl(_)
            | Item::Test(_)
            | Item::Bench(_)
            | Item::Const(_)
            | Item::Trait(_)
            | Item::Tag(_) // D-QUAL2: tags contribute no comptime context
            | Item::Module(_)
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::Distinct(_)
            | Item::TypeAlias(_) // D-TYPEALIAS1: erases at codegen
            | Item::UnitFamily(_) // D-QUAL3: contributes no comptime context
            | Item::ErrorConv(_)
            | Item::Migration(_) // D-MIGRATE1
            | Item::StateDecl(_) // D-STATE-DECL
            | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2
            | Item::UserDerive(_) // D-METADERIVE1=A: expanded in Bundle.rs
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
        }
    }
    (funcs, externs, globals)
}

pub(crate) fn register_const(
    c: &crate::AST::ConstDef,
    consts: &mut HashMap<String, Type>,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
) {
    if name_defined(&c.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &c.name,
            "every const needs a unique name",
            c.name_span,
        ));
        return;
    }
    // S57 (M9.5): comptime bindings are evaluated by a dedicated pass
    // (`eval_comptime_items`), which registers their type from the result.
    if c.is_comptime {
        return;
    }
    // D-SHAPE-OUTPUT-CALLABLE1: typed Outputs are sema-only package graph
    // values. Bundle resolution below checks their closed shape and callable;
    // they are not runtime numeric constants.
    if matches!(&c.ty, Some(Type::Named(name)) if name == Syntax::TYPE_OUTPUT || name == Syntax::TYPE_OUTPUT_DEFAULTS) {
        consts.insert(c.name.clone(), c.ty.clone().unwrap());
        return;
    }
    let ty = match &c.value {
        Expr::Int(_, _, _, _) => Some(Type::Int),
        Expr::Float(_, _, _) => Some(Type::Float),
        Expr::Bool(_, _) => Some(Type::Bool),
        _ => None,
    };
    match ty {
        Some(t) => {
            consts.insert(c.name.clone(), t);
        }
        None => {
            diags.push(Diagnostic::error(
                "E0109",
                "a const holds a plain number or `true`/`false` for now".to_string(),
                "richer const values arrive with later milestones".to_string(),
                "give the const a number, like `const limit = 10;`".to_string(),
                Some(c.value.span()),
            ));
        }
    }
}

pub(crate) fn register_struct(
    s: &StructDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&s.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", s.name),
            format!("`{}` is provided by the language itself", s.name),
            "choose a different name for this struct".to_string(),
            Some(s.name_span),
        ));
        return;
    }
    if name_defined(&s.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &s.name,
            "every struct needs a unique name",
            s.name_span,
        ));
        return;
    }
    let mut field_names = HashSet::new();
    let mut fields = Vec::new();
    // D-FIELDPOL1: struct name → computed field name → (span, type). A
    // computed field is never a stored field — it's excluded from `fields`
    // entirely (so it's never required/allowed in a struct literal, E0339)
    // and resolved for reads through this side table instead.
    let mut computed_fields: HashMap<String, (Span, Type)> = HashMap::new();
    for f in &s.fields {
        if !field_names.insert(f.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("field `{}` is defined twice in `{}`", f.name, s.name),
                "each field name may appear only once".to_string(),
                "rename or remove the duplicate field".to_string(),
                Some(f.name_span),
            ));
        }
        if f.computed.is_some() {
            computed_fields.insert(f.name.clone(), (f.name_span, f.ty.clone()));
        } else {
            fields.push((f.name.clone(), f.name_span, f.ty.clone(), f.is_pub));
            if declared_type_contains_cell_guard(&f.ty) {
                diags.push(cell_guard_storage_diagnostic(
                    &format!("struct field `{}`", f.name),
                    f.name_span,
                ));
            }
        }
        if matches!(&f.ty, Type::Named(name) if name == Syntax::TYPE_TASKGROUP) {
            diags.push(Diagnostic::error(
                "E1110",
                "`TaskGroup` cannot be stored in a struct field".to_string(),
                "a taskgroup is a scoped spawn authority, not a value that can escape its call stack"
                    .to_string(),
                "pass `group: TaskGroup` directly to a named helper function instead".to_string(),
                Some(f.name_span),
            ));
        }
        if f.ty.is_float()
            && is_money_like_name(&f.name)
            && !allows_float_money(&f.serde_markers)
            && !allows_float_money(&s.serde_markers)
        {
            diags.push(Diagnostic::lint(
                "L0504",
                format!(
                    "field `{}` looks like money but has type `Float`",
                    f.name
                ),
                "floating-point money loses cents on common values like `0.1 + 0.2`".to_string(),
                "use `Decimal` for exact money, or suppress with `#[allow(float_money)]` on the field".to_string(),
                Some(f.name_span),
            ));
        }
    }
    registry.types.insert(
        s.name.clone(),
        TypeDef::Struct {
            fields,
            methods: HashMap::new(),
            single_use: s.is_single_use,
            must_use: s.is_must_use,
            columnar: s.layout == Some(crate::AST::StructLayout::Columnar),
            is_c_layout: s.layout == Some(crate::AST::StructLayout::C),
        },
    );
    if !computed_fields.is_empty() {
        registry
            .computed_fields
            .insert(s.name.clone(), computed_fields);
    }
    // D-REPRC1: `#layout(c)` structs may not contain growable fields.
    if s.layout == Some(crate::AST::StructLayout::C) {
        for f in &s.fields {
            let growable = matches!(&f.ty, Type::List(_) | Type::Map { .. } | Type::String);
            if growable {
                let layout_span = s.layout_span.unwrap_or(s.name_span);
                diags.push(Diagnostic::error(
                    "E1104",
                    format!(
                        "`#Layout(c)` struct `{}` has a growable field `{}` ({})",
                        s.name,
                        f.name,
                        f.ty.name()
                    ),
                    "growable types (`[T]`, `Map`, `String`) don't have a stable C layout"
                        .to_string(),
                    "use a fixed-size array `[T#N]` instead, or remove `#Layout(c)`".to_string(),
                    Some(layout_span),
                ));
            }
        }
    }
}

pub(crate) fn register_enum(
    e: &EnumDef,
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
    funcs: &HashMap<String, FuncSig>,
    consts: &HashMap<String, Type>,
) {
    if is_reserved_type(&e.name) {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", e.name),
            format!("`{}` is provided by the language itself", e.name),
            "choose a different name for this enum".to_string(),
            Some(e.name_span),
        ));
        return;
    }
    if name_defined(&e.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &e.name,
            "every enum needs a unique name",
            e.name_span,
        ));
        return;
    }
    let mut variants = HashMap::new();
    let mut variant_order = Vec::new();
    let mut seen = HashSet::new();
    // D-TAG1: leaf names are full dotted paths; the flattened Rust variant name
    // joins segments with `__`, so two distinct paths that mangle identically
    // (`Fire.Burn` vs `Fire__Burn`) must be rejected here, not by rustc (I2).
    let mut mangled = HashMap::new();
    for v in &e.variants {
        if !seen.insert(v.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("variant `{}` is defined twice in `{}`", v.name, e.name),
                "each variant name may appear only once".to_string(),
                "rename or remove the duplicate variant".to_string(),
                Some(v.name_span),
            ));
            continue;
        }
        if let Some(other) = mangled.insert(v.name.replace('.', "__"), v.name.clone()) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "variant `{}` collides with `{}` in `{}`",
                    v.name, other, e.name
                ),
                "a grouped path and an underscored name flatten to the same variant".to_string(),
                "rename one of the two variants".to_string(),
                Some(v.name_span),
            ));
            continue;
        }
        variant_order.push(v.name.clone());
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(ty, span) => {
                if declared_type_contains_cell_guard(ty) {
                    diags.push(cell_guard_storage_diagnostic(
                        &format!("enum variant `{}`", v.name),
                        *span,
                    ));
                }
            }
            VariantPayload::Named(fields) => {
                for field in fields {
                    if declared_type_contains_cell_guard(&field.ty) {
                        diags.push(cell_guard_storage_diagnostic(
                            &format!("enum field `{}`", field.name),
                            field.name_span,
                        ));
                    }
                }
            }
        }
        variants.insert(v.name.clone(), (v.name_span, v.payload.clone()));
    }
    // D-TAG1: record each group's subtree (ordered leaf paths). A group path
    // that also names a leaf is a duplicate definition (one name, two meanings).
    let mut groups: HashMap<String, (Span, Vec<String>)> = HashMap::new();
    for g in &e.groups {
        if variants.contains_key(&g.path) || groups.contains_key(&g.path) {
            diags.push(Diagnostic::error(
                "E0105",
                format!("variant `{}` is defined twice in `{}`", g.path, e.name),
                "a group name and a variant name share one namespace".to_string(),
                "rename or remove the duplicate variant".to_string(),
                Some(g.name_span),
            ));
            continue;
        }
        let prefix = format!("{}.", g.path);
        let leaves: Vec<String> = variant_order
            .iter()
            .filter(|v| v.starts_with(&prefix))
            .cloned()
            .collect();
        groups.insert(g.path.clone(), (g.name_span, leaves));
    }
    registry.types.insert(
        e.name.clone(),
        TypeDef::Enum {
            variants,
            variant_order,
            groups,
            methods: HashMap::new(),
            single_use: e.is_single_use,
            must_use: e.is_must_use,
            c_layout_tag: e.c_layout_tag(),
        },
    );
}

pub(crate) fn register_type_methods(
    items: &[Item],
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        let (type_name, methods, field_names) = match item {
            Item::Struct(s) => (s.name.as_str(), &s.methods, registry.field_names(&s.name)),
            Item::Enum(e) => (e.name.as_str(), &e.methods, Vec::new()),
            _ => continue,
        };
        let Some(type_def) = registry.types.get_mut(type_name) else {
            continue;
        };
        let methods_map = match type_def {
            TypeDef::Struct { methods, .. } | TypeDef::Enum { methods, .. } => methods,
            TypeDef::Distinct { .. } | TypeDef::Alias { .. } => continue,
        };
        for m in methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(
                    &m.name,
                    type_name,
                    m.name_span,
                    is_ctor,
                ));
            } else {
                // L2401 (D-NARG1): pub method with a positional Bool param.
                if m.is_pub {
                    for p in m.params.iter().filter(|p| p.name != "self") {
                        if matches!(p.ty, Type::Bool) && p.default.is_none() {
                            diags.push(Diagnostic::lint(
                                "L2401",
                                format!(
                                    "public method `{}` has a positional `Bool` parameter `{}`",
                                    m.name, p.name
                                ),
                                "positional booleans are easy to transpose at the call site"
                                    .to_string(),
                                format!(
                                    "callers can write `{}: true` to make the intent clear (S61 labels)",
                                    p.name
                                ),
                                Some(p.name_span),
                            ));
                        }
                    }
                }
                // D-NARG-D2 (E0126): check defaults don't ref later params.
                let non_self: Vec<_> = m
                    .params
                    .iter()
                    .filter(|p| p.name != "self")
                    .cloned()
                    .collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

pub(crate) fn register_impl_methods(
    items: &[Item],
    registry: &mut TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) {
    for item in items {
        let Item::Impl(i) = item else { continue };
        if !registry.contains(&i.type_name) {
            continue;
        }
        let field_names = registry.field_names(&i.type_name);
        let Some(type_def) = registry.types.get_mut(&i.type_name) else {
            continue;
        };
        let methods_map = match type_def {
            TypeDef::Struct { methods, .. } | TypeDef::Enum { methods, .. } => methods,
            TypeDef::Distinct { .. } | TypeDef::Alias { .. } => continue,
        };
        for m in &i.methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, &i.type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(
                    &m.name,
                    &i.type_name,
                    m.name_span,
                    is_ctor,
                ));
            } else {
                // L2401 (D-NARG1): pub method with a positional Bool param.
                if m.is_pub {
                    for p in m.params.iter().filter(|p| p.name != "self") {
                        if matches!(p.ty, Type::Bool) && p.default.is_none() {
                            diags.push(Diagnostic::lint(
                                "L2401",
                                format!(
                                    "public method `{}` has a positional `Bool` parameter `{}`",
                                    m.name, p.name
                                ),
                                "positional booleans are easy to transpose at the call site"
                                    .to_string(),
                                format!(
                                    "callers can write `{}: true` to make the intent clear (S61 labels)",
                                    p.name
                                ),
                                Some(p.name_span),
                            ));
                        }
                    }
                }
                // D-NARG-D2 (E0126): check defaults don't ref later params.
                let non_self: Vec<_> = m
                    .params
                    .iter()
                    .filter(|p| p.name != "self")
                    .cloned()
                    .collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}
