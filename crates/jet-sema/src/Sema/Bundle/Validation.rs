use super::*;
use crate::AST::Param;

pub(super) fn qualified_effect_facts(
    modules: &[(String, HashMap<String, EffectSummary>)],
) -> (HashMap<String, EffectSummary>, HashMap<String, EffectSet>) {
    let mut locations = HashMap::<String, Vec<String>>::new();
    let aliases = modules.iter().map(|(alias, _)| alias.as_str()).collect::<HashSet<_>>();
    for (alias, summaries) in modules {
        for key in summaries.keys() {
            locations.entry(key.clone()).or_default().push(format!("{alias}::{key}"));
        }
    }
    let mut qualified = HashMap::new();
    for (alias, summaries) in modules {
        for (key, summary) in summaries {
            let mut summary = summary.clone();
            let resolve_edge = |edge: &String| {
                if edge == "__jet_panic__" { return edge.clone(); }
                if summaries.contains_key(edge) { return format!("{alias}::{edge}"); }
                if let Some((module, symbol)) = edge.split_once('.') {
                    if aliases.contains(module) { return format!("{module}::{symbol}"); }
                }
                locations.get(edge).and_then(|values| (values.len() == 1).then(|| values[0].clone())).unwrap_or_else(|| edge.clone())
            };
            summary.edges = summary.edges.iter().map(&resolve_edge).collect();
            for region in &mut summary.regions {
                region.edges = region.edges.iter().map(&resolve_edge).collect();
            }
            for obligation in &mut summary.callback_obligations {
                obligation.edges = obligation.edges.iter().map(&resolve_edge).collect();
            }
            for call in &mut summary.memory.calls {
                call.callee = resolve_edge(&call.callee);
            }
            for region in &mut summary.memory.regions {
                region.edges = region.edges.iter().map(&resolve_edge).collect();
                for call in &mut region.calls {
                    call.callee = resolve_edge(&call.callee);
                }
            }
            qualified.insert(format!("{alias}::{key}"), summary);
        }
    }
    let mut solved = solve(&qualified);
    for (short, values) in locations.iter().filter(|(_, values)| values.len() == 1) {
        let qualified_key = &values[0];
        if let Some(summary) = qualified.get(qualified_key).cloned() {
            qualified.insert(short.clone(), summary);
        }
        if let Some(effects) = solved.get(qualified_key).cloned() {
            solved.insert(short.clone(), effects);
        }
    }
    (qualified, solved)
}

#[cfg(test)]
mod effect_qualification_tests {
    use super::*;

    #[test]
    fn nested_region_and_callback_edges_are_module_qualified() {
        let root = EffectSummary {
            regions: vec![RegionSummary {
                caps: EffectSet::new(),
                direct: EffectSet::new(),
                edges: ["left.same".to_string()].into_iter().collect(),
                maximal: false,
                caps_span: Span::new(1, 2),
                grant: false,
            }],
            callback_obligations: vec![CallbackObligation {
                bound: EffectSet::new(),
                direct: EffectSet::new(),
                edges: ["right.same".to_string()].into_iter().collect(),
                maximal: false,
                span: Span::new(3, 4),
            }],
            ..Default::default()
        };
        let modules = vec![
            ("main".to_string(), HashMap::from([("root".to_string(), root)])),
            (
                "left".to_string(),
                HashMap::from([("same".to_string(), EffectSummary::default())]),
            ),
            (
                "right".to_string(),
                HashMap::from([("same".to_string(), EffectSummary::default())]),
            ),
        ];

        let (summaries, _) = qualified_effect_facts(&modules);
        let root = &summaries["main::root"];
        assert_eq!(
            root.regions[0].edges,
            EffectSet::from(["left::same".to_string()])
        );
        assert_eq!(
            root.callback_obligations[0].edges,
            EffectSet::from(["right::same".to_string()])
        );
    }
}

/// D-TAINT1: run the taint pass over one item's function/method bodies in the
/// bundle path, using `core_imports` to classify sink calls.
pub(super) fn taint_check_item(
    item: &Item,
    sanitizers: &std::collections::HashSet<String>,
    core_imports: &HashMap<String, String>,
    diags: &mut Vec<Diagnostic>,
) {
    match item {
        Item::Func(f) => diags.extend(check_func_taint(&f.body, sanitizers, core_imports)),
        Item::Impl(i) => {
            for m in &i.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
        }
        Item::Struct(s) => {
            for m in &s.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
            for block in &s.trait_impls {
                for m in &block.methods {
                    diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
                }
            }
        }
        Item::Enum(e) => {
            for m in &e.methods {
                diags.extend(check_func_taint(&m.body, sanitizers, core_imports));
            }
        }
        Item::Test(t) => diags.extend(check_func_taint(&t.body, sanitizers, core_imports)),
        Item::ErrorConv(ec) => diags.extend(check_func_taint(&ec.body, sanitizers, core_imports)),
        _ => {}
    }
}

pub(crate) fn register_func_item(f: &Func, st: &mut ModuleState, diags: &mut Vec<Diagnostic>) {
    if f.name == Syntax::BUILTIN_PRINT
        || f.name == Syntax::BUILTIN_PANIC
        || f.name == Syntax::BUILTIN_REQUIRE
        || f.name == Syntax::BUILTIN_REQUIRE_EQ
    {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", f.name),
            format!("`{}` is provided by the language itself", f.name),
            "choose a different name for this function".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    if name_defined(&f.name, &st.funcs, &st.registry, &st.consts) {
        diags.push(Diagnostic::error(
            "E0105",
            format!("`{}` is defined twice", f.name),
            "every function needs a unique name so calls aren't ambiguous".to_string(),
            "rename or remove one of the definitions".to_string(),
            Some(f.name_span),
        ));
        return;
    }
    // L2401: advisory — public fn with a positional Bool parameter.
    if f.is_pub {
        for p in &f.params {
            if matches!(p.ty, Type::Bool) && p.name != Syntax::KW_SELF && p.default.is_none() {
                diags.push(Diagnostic::lint(
                    "L2401",
                    format!(
                        "public function `{}` has a positional `Bool` parameter `{}`",
                        f.name, p.name
                    ),
                    "positional booleans are easy to transpose at the call site".to_string(),
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
    check_default_forward_refs(&f.params, &f.name, diags);
    st.func_pub
        .insert(f.name.clone(), f.is_pub && !f.is_package_pub);
    st.func_pkg_pub.insert(f.name.clone(), f.is_package_pub);
    st.funcs.insert(f.name.clone(), func_to_sig(f));
}

/// Core value/container + opaque-handle type names backed by `jet_std`.
/// Naming one in an annotation needs the Core prelude
/// even without a method call for the expression walker to observe.
fn is_encoding_surface_type(name: &str) -> bool {
    // Annotations may spell the type module-qualified (`encoding.EncodingError`,
    // `json.JSONReader`); match on the final path segment.
    let base = name.rsplit('.').next().unwrap_or(name);
    matches!(
        base,
        "DataTree"
            | "Table"
            | "Series"
            | "LazyFrame"
            | "DataJoin"
            | "EncodingLimits"
            | "EncodingError"
            | "CBOROptions"
            | "CBORError"
            | "CBORErrorKind"
            | "EncodingCause"
            | "EncodingFormat"
            | "EncodingErrorKind"
            | "DataEvent"
            | "JSONReader"
            | "JSONWriter"
            | "JSONLReader"
            | "JSONLWriter"
            | "CSVReader"
            | "CSVWriter"
            | "XMLReader"
            | "XMLWriter"
            | "CBORReader"
            | "CBORWriter"
    )
}

/// True when `ty` (or any type nested inside it) names a `core.encoding` surface
/// type. Recurses through every type-carrying `Type` variant.
fn type_mentions_encoding_surface(ty: &Type) -> bool {
    match ty {
        Type::Named(name) => is_encoding_surface_type(name),
        Type::Apply { name, args } => {
            is_encoding_surface_type(name) || args.iter().any(type_mentions_encoding_surface)
        }
        Type::TraitObject(names) => names.iter().any(|n| is_encoding_surface_type(n)),
        Type::List(inner)
        | Type::Shared(inner)
        | Type::Option(inner)
        | Type::Tagged { inner, .. } => type_mentions_encoding_surface(inner),
        Type::FixedList { elem, .. } => type_mentions_encoding_surface(elem),
        Type::Map { key, value, .. } => {
            type_mentions_encoding_surface(key) || type_mentions_encoding_surface(value)
        }
        Type::Result { ok, err } => {
            type_mentions_encoding_surface(ok) || type_mentions_encoding_surface(err)
        }
        Type::Fn { params, ret, .. } => {
            params.iter().any(type_mentions_encoding_surface)
                || ret.as_deref().is_some_and(type_mentions_encoding_surface)
        }
        Type::Tuple(fields) => fields.iter().any(|(_, t)| type_mentions_encoding_surface(t)),
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::IntN { .. }
        | Type::Float32 => false,
    }
}

/// A function/method signature (params + return) names an encoding surface type.
fn func_sig_mentions_encoding_surface(f: &Func) -> bool {
    f.params.iter().any(|p| type_mentions_encoding_surface(&p.ty))
        || f.return_type
            .as_ref()
            .is_some_and(type_mentions_encoding_surface)
}

/// Scan every annotation position in a module for a `core.encoding` surface type
/// (struct fields, enum payloads, function/method/trait signatures, type-alias
/// targets, associated-type impls). Runtime usage always constructs handles via
/// a format-module call the expression walker already sees; this only covers the
/// annotation-only case (a signature that names a handle constructed elsewhere).
fn module_annotations_mention_encoding_surface(module: &crate::AST::LoadedModule) -> bool {
    fn variant_payload_mentions(payload: &VariantPayload) -> bool {
        match payload {
            VariantPayload::Unit => false,
            VariantPayload::Single(ty, _) => type_mentions_encoding_surface(ty),
            VariantPayload::Named(fields) => {
                fields.iter().any(|f| type_mentions_encoding_surface(&f.ty))
            }
        }
    }
    module.items.iter().any(|item| match item {
        Item::Func(f) => func_sig_mentions_encoding_surface(f),
        Item::Struct(s) => {
            s.fields.iter().any(|f| type_mentions_encoding_surface(&f.ty))
                || s.methods.iter().any(func_sig_mentions_encoding_surface)
                || s.trait_impls
                    .iter()
                    .any(|b| b.methods.iter().any(func_sig_mentions_encoding_surface))
        }
        Item::Enum(e) => {
            e.variants.iter().any(|v| variant_payload_mentions(&v.payload))
                || e.methods.iter().any(func_sig_mentions_encoding_surface)
                || e.trait_impls
                    .iter()
                    .any(|b| b.methods.iter().any(func_sig_mentions_encoding_surface))
        }
        Item::Impl(i) => {
            i.methods.iter().any(func_sig_mentions_encoding_surface)
                || i.assoc_type_impls
                    .iter()
                    .any(|(_, _, ty)| type_mentions_encoding_surface(ty))
        }
        Item::Trait(t) => t.methods.iter().any(|m| {
            m.params.iter().any(|p| type_mentions_encoding_surface(&p.ty))
                || m.return_type.as_ref().is_some_and(type_mentions_encoding_surface)
        }),
        Item::TypeAlias(a) => type_mentions_encoding_surface(&a.target),
        _ => false,
    })
}

pub(crate) fn collect_used_core(
    bundle: &ProgramBundle,
    states: &[ModuleState],
) -> (
    HashSet<String>,
    HashMap<String, crate::Diagnostics::Span>,
    HashSet<String>,
) {
    let mut used = HashSet::new();
    let mut spans = HashMap::new();
    // D-CABI-CALLBACK1: names of top-level functions sema proved are passed as
    // a stable C callback symbol (`arg.flags.c_callback_symbol`) at some
    // `@Extern` call site anywhere in the bundle. Collected in this same
    // whole-program walk (not a second traversal) so codegen knows, before it
    // emits ANY function, which ones must be `extern "C" fn` — never every
    // `@Pure fn` (that leaked the purity lever into codegen and broke I3
    // erasure; see 14dd68a5), only the ones actually crossing the C boundary
    // as a raw function pointer.
    let mut ffi_cb = HashSet::new();
    for (idx, module) in bundle.modules.iter().enumerate() {
        let imports = &states[idx].core_imports;
        // Annotation-only core values still need their Rust definitions even
        // when globally named rather than reached through a core-module call.
        // A bare core import remains free: only an annotation makes this true.
        if module_annotations_mention_encoding_surface(module) {
            used.insert("core.encoding::types".to_string());
        }
        for item in &module.items {
            match item {
                Item::Func(f) => collect_core_stmts(&f.body, imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Struct(s) => {
                    for m in &s.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        collect_core_stmts(&m.body, imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Test(t) => collect_core_stmts(&t.body, imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Bench(b) => collect_core_stmts(&b.body, imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Const(c) => collect_core_expr(&c.value, imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Trait(_)
                | Item::Tag(_) // D-QUAL2: tags use no core imports
                | Item::ExternRust(_)
                | Item::Module(_)
                | Item::Distinct(_)
                | Item::TypeAlias(_) // D-TYPEALIAS1: erases
                | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
                | Item::CModule(_) | Item::CodeModule(_)
                | Item::ErrorConv(_)
                | Item::Migration(_) // D-MIGRATE1
                | Item::StateDecl(_) // D-STATE-DECL: uses no core imports
                | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
            | Item::UserDerive(_) // D-METADERIVE1=A: already expanded
            | Item::GenericModule(_) // D-GENMOD2=A: template — erases
            | Item::ModuleAlias(_) => {} // D-GENMOD2=A: alias — erases after expansion
            }
        }
    }
    (used, spans, ffi_cb)
}

fn note_core_usage(
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    key: impl Into<String>,
    span: Option<crate::Diagnostics::Span>,
) {
    let key = key.into();
    used.insert(key.clone());
    if let Some(s) = span {
        spans.entry(key).or_insert(s);
    }
}

/// D-RINGLAYER1=A M2: bump inferred layer from emitted helper usage and enforce ceiling.
pub(super) fn apply_helper_layer_inference(
    bundle: &mut ProgramBundle,
    states: &[ModuleState],
    usage_spans: &HashMap<String, crate::Diagnostics::Span>,
    diags: &mut Vec<Diagnostic>,
) {
    let core_imports: HashMap<String, String> = states
        .iter()
        .flat_map(|st| st.core_imports.iter().map(|(a, m)| (a.clone(), m.clone())))
        .collect();
    for usage in &bundle.used_core {
        let Some(mod_layer) = crate::Syntax::core_usage_layer(usage) else {
            continue;
        };
        if mod_layer > bundle.inferred_layer {
            bundle.inferred_layer = mod_layer;
        }
        let Some(ceiling) = bundle.layer_ceiling else {
            continue;
        };
        if mod_layer <= ceiling {
            continue;
        }
        let span = usage_spans.get(usage).copied();
        let chain = helper_import_chain(usage, &core_imports);
        diags.push(crate::Syntax::layer_ceiling_exceeded(
            usage,
            mod_layer,
            ceiling,
            span,
            Some(&chain),
        ));
    }
}

fn helper_import_chain(usage: &str, core_imports: &HashMap<String, String>) -> String {
    if usage == "core.io::input" {
        return format!("ambient `input()` (helper `{usage}`)");
    }
    if let Some((module, _)) = usage.split_once("::") {
        if let Some((alias, imported)) = core_imports.iter().find(|(_, m)| m.as_str() == module) {
            return format!("`use {imported} as {alias}` → `{usage}`");
        }
        return format!("prelude helper `{usage}`");
    }
    format!("prelude helper `{usage}`")
}

pub(crate) fn collect_core_stmts(
    stmts: &[Stmt],
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    ffi_cb: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Yield(e, _) => collect_core_expr(e, imports, used, spans, ffi_cb),
            Stmt::Val(b) => collect_core_expr(&b.init, imports, used, spans, ffi_cb),
            Stmt::Assign { target, value, .. } => {
                collect_core_lvalue(target, imports, used, spans, ffi_cb);
                collect_core_expr(value, imports, used, spans, ffi_cb);
            }
            Stmt::Return(Some(e), _) => collect_core_expr(e, imports, used, spans, ffi_cb),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => collect_core_if(ifs, imports, used, spans, ffi_cb),
            Stmt::While { cond, body, .. } => {
                collect_core_expr(cond, imports, used, spans, ffi_cb);
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        collect_core_expr(start, imports, used, spans, ffi_cb);
                        collect_core_expr(end, imports, used, spans, ffi_cb);
                        if let Some(step) = step {
                            collect_core_expr(step, imports, used, spans, ffi_cb);
                        }
                    }
                    ForKind::In { collection } => {
                        collect_core_expr(collection, imports, used, spans, ffi_cb)
                    }
                }
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            }
            | Stmt::ComptimeSwitch {
                subject,
                arms,
                else_body,
                ..
            } => {
                collect_core_expr(subject, imports, used, spans, ffi_cb);
                for arm in arms {
                    collect_core_expr(&arm.cond, imports, used, spans, ffi_cb);
                    collect_core_stmts(&arm.body, imports, used, spans, ffi_cb);
                }
                if let Some(body) = else_body {
                    collect_core_stmts(body, imports, used, spans, ffi_cb);
                }
            }
            Stmt::CountedLoop {
                init,
                cond,
                step,
                body,
                ..
            } => {
                collect_core_expr(&init.init, imports, used, spans, ffi_cb);
                collect_core_expr(cond, imports, used, spans, ffi_cb);
                collect_core_stmts(body, imports, used, spans, ffi_cb);
                collect_core_stmts(std::slice::from_ref(step.as_ref()), imports, used, spans, ffi_cb);
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Off { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_core_stmts(body, imports, used, spans, ffi_cb),
            // D-SHIELDNAME1=A: parsed syntax, not raw source text, owns the
            // scheduler-prelude capability. This recognizes legal whitespace
            // such as `# Shield` and cannot be fooled by comments or strings.
            Stmt::Shield { body, span } => {
                note_core_usage(used, spans, "core.concurrency::shield", Some(*span));
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            // D-REACTCORE1: reactive blocks implicitly use `core.reactive`.
            Stmt::Reactive { body, span, .. } => {
                note_core_usage(used, spans, "core.reactive", Some(*span));
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {
            }
            // D-CTMARKER1: collect Core usage from comptime block body.
            Stmt::ComptimeBlock { body, .. } => collect_core_stmts(body, imports, used, spans, ffi_cb),
            // D-WHEN1: collect Core usage from both arms (we don't know which is
            // selected until sema runs; over-collecting is harmless here).
            Stmt::ComptimeIf {
                cond,
                then_body,
                else_body,
                ..
            } => {
                collect_core_expr(cond, imports, used, spans, ffi_cb);
                collect_core_stmts(then_body, imports, used, spans, ffi_cb);
                if let Some(eb) = else_body {
                    collect_core_stmts(eb, imports, used, spans, ffi_cb);
                }
            }
            // D-CTX1: collect Core usage from context block fields and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    collect_core_expr(e, imports, used, spans, ffi_cb);
                }
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            // D-TERM1 (ratified 2026-06-22): collect Core usage from live block body.
            // The live block implicitly uses `core.term` (jet_term_enter/leave), so
            // we mark it as used here.
            Stmt::Live { body, span, .. } => {
                note_core_usage(used, spans, "core.term", Some(*span));
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            // D-DOTSCOPE1: collect core usage in a scope-member region body.
            Stmt::ScopeMember { body, .. } => {
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
        }
    }
}

pub(crate) fn collect_core_if(
    ifs: &IfStmt,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    ffi_cb: &mut HashSet<String>,
) {
    collect_core_expr(&ifs.cond, imports, used, spans, ffi_cb);
    collect_core_stmts(&ifs.then_body, imports, used, spans, ffi_cb);
    match &ifs.else_branch {
        Some(ElseBranch::Else(body)) => collect_core_stmts(body, imports, used, spans, ffi_cb),
        Some(ElseBranch::ElseIf(next)) => collect_core_if(next, imports, used, spans, ffi_cb),
        None => {}
    }
}

pub(crate) fn collect_core_lvalue(
    lv: &LValue,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    ffi_cb: &mut HashSet<String>,
) {
    match lv {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => {
            collect_core_expr(base, imports, used, spans, ffi_cb);
            collect_core_expr(index, imports, used, spans, ffi_cb);
        }
        // D-MUTSELF1: `place.field = v` — the base place may use a core import.
        LValue::Field { base, .. } => collect_core_expr(base, imports, used, spans, ffi_cb),
    }
}

pub(crate) fn collect_core_expr(
    expr: &Expr,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    ffi_cb: &mut HashSet<String>,
) {
    // D-SIMD2 / D-LINALG1: a built-in math type used anywhere (constructor, static
    // method, or instance method on a math-typed receiver-by-name) pulls in the
    // CoreLib prelude that defines the `jet_math_*` helpers. Detect it syntactically;
    // a math-type *constructor* call and a static `T.method(...)` both surface the
    // type NAME, which is enough to require the prelude.
    match expr {
        Expr::Call(c) if is_math_type(&c.name) => {
            note_core_usage(used, spans, "core.math::__mathtypes__", Some(c.name_span));
        }
        Expr::Call(c)
            if c.name == crate::Syntax::TYPE_BIGINT || c.name == crate::Syntax::TYPE_DECIMAL =>
        {
            note_core_usage(used, spans, "core.math::__precise__", Some(c.name_span));
        }
        Expr::MethodCall {
            receiver,
            method_span,
            ..
        } => {
            if let Expr::Ident(n, _) = receiver.as_ref() {
                if is_math_type(n) {
                    note_core_usage(used, spans, "core.math::__mathtypes__", Some(*method_span));
                }
                // D-PATHFS1: `Path.from(...)` or any Path static call triggers path prelude.
                if n == "Path" {
                    note_core_usage(used, spans, "core.path::__pathapi__", Some(*method_span));
                }
            }
        }
        _ => {}
    }
    match expr {
        Expr::PtrFromAddr { addr, .. } => collect_core_expr(addr, imports, used, spans, ffi_cb),
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            args,
            recv_type,
            ..
        } => {
            if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
                && method == Syntax::TASKGROUP_SPAWN_METHOD
            {
                note_core_usage(
                    used,
                    spans,
                    "core.tasks::spawn",
                    Some(*method_span),
                );
            }
            if matches!(
                recv_type.as_deref(),
                Some(crate::Syntax::TYPE_BIGINT) | Some(crate::Syntax::TYPE_DECIMAL)
            ) {
                note_core_usage(
                    used,
                    spans,
                    "core.math::__precise__",
                    Some(*method_span),
                );
            }
            if recv_type.as_deref() == Some(crate::Syntax::DURATION_TYPE)
                || matches!(receiver.as_ref(), Expr::Ident(n, _) if n == crate::Syntax::DURATION_TYPE)
            {
                note_core_usage(
                    used,
                    spans,
                    "core.time::__duration__",
                    Some(*method_span),
                );
            }
            if recv_type.as_deref() == Some(crate::Syntax::SOLVER_TYPE) {
                note_core_usage(
                    used,
                    spans,
                    format!("{}::{method}", crate::Syntax::CORE_SOLVE_MODULE),
                    Some(*method_span),
                );
            }
            if matches!(receiver.as_ref(), Expr::Ident(n, _) if is_json_type_name(n)) {
                note_core_usage(used, spans, "core::json", Some(*method_span));
            }
            if matches!(
                method.as_str(),
                "bytes" | "from_bytes" | "elapsed_millis"
            ) {
                note_core_usage(used, spans, format!("core::{method}"), Some(*method_span));
            }
            if let Expr::Ident(alias, _) = receiver.as_ref() {
                if let Some(module) = imports.get(alias) {
                    note_core_usage(
                        used,
                        spans,
                        format!("{module}::{method}"),
                        Some(*method_span),
                    );
                }
            }
            // D-ENC1: nested-namespace core call `<alias>.<leaf>.method(...)` (e.g.
            // `encoding.json.to_string(x)`). Record `<ns>.<leaf>::method` so the CoreLib
            // prelude is emitted and the backing helper is in scope.
            if let Expr::Field(base, leaf, _) = receiver.as_ref() {
                if let Expr::Ident(alias, _) = base.as_ref() {
                    if let Some(ns) = imports.get(alias) {
                        let submodule = format!("{ns}.{leaf}");
                        if crate::Syntax::is_known_core_module(&submodule) {
                            note_core_usage(
                                used,
                                spans,
                                format!("{submodule}::{method}"),
                                Some(*method_span),
                            );
                        } else if crate::Sema::CheckerCoreLib::core_module_type_item(ns, leaf) {
                            // Qualified Core type constructor, e.g.
                            // `email.Limits.safe()`: it still needs CoreLib's
                            // runtime prelude even though `<ns>.<leaf>` is a
                            // type, not a nested module.
                            note_core_usage(
                                used,
                                spans,
                                format!("{ns}::{leaf}.{method}"),
                                Some(*method_span),
                            );
                        }
                    }
                }
            }
            collect_core_expr(receiver, imports, used, spans, ffi_cb);
            for arg in args {
                // D-CABI-CALLBACK1: a qualified `@Extern`-module call
                // (`c.callback_twice(increment, x)`) resolves through
                // `infer_import_call` (CheckerCoreLib/imports.rs), a separate
                // path from the bare-name call below — same flag, same fix.
                if arg.flags.c_callback_symbol {
                    if let Expr::Ident(name, _) = &arg.expr {
                        ffi_cb.insert(name.clone());
                    }
                }
                collect_core_expr(&arg.expr, imports, used, spans, ffi_cb);
            }
        }
        Expr::Call(c) => {
            // D-PRELUDE1 = B: bare `input(...)` is prelude-ambient; mark core.io so
            // CORELIB_PRELUDE is emitted and jet_std_io_input is in scope for codegen.
            if c.name == Syntax::BUILTIN_INPUT {
                note_core_usage(used, spans, "core.io::input", Some(c.name_span));
            }
            for arg in &c.args {
                // D-CABI-CALLBACK1: `arg.flags.c_callback_symbol` means sema
                // already proved this bare function name is passed as a stable
                // C callback at a `@Extern` call site — record the referenced
                // function so codegen emits its definition as `extern "C" fn`.
                if arg.flags.c_callback_symbol {
                    if let Expr::Ident(name, _) = &arg.expr {
                        ffi_cb.insert(name.clone());
                    }
                }
                collect_core_expr(&arg.expr, imports, used, spans, ffi_cb);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_core_expr(callee, imports, used, spans, ffi_cb);
            for arg in args {
                collect_core_expr(&arg.expr, imports, used, spans, ffi_cb);
            }
        }
        Expr::Field(inner, member, span) => {
            if matches!(inner.as_ref(), Expr::Ident(n, _) if is_json_type_name(n))
                && member == "Null"
            {
                note_core_usage(used, spans, "core::json", Some(*span));
            }
            collect_core_expr(inner, imports, used, spans, ffi_cb);
        }
        Expr::OptField { base, .. } => collect_core_expr(base, imports, used, spans, ffi_cb),
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _) // D-TAINT1: tag erased; recurse into the value.
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
        Expr::Binary(_, lhs, rhs, _)
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            collect_core_expr(lhs, imports, used, spans, ffi_cb);
            collect_core_expr(rhs, imports, used, spans, ffi_cb);
        }
        Expr::CompareChain { operands, .. } => {
            for e in operands.iter() {
                collect_core_expr(e, imports, used, spans, ffi_cb);
            }
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_core_expr(base, imports, used, spans, ffi_cb);
            collect_core_expr(start, imports, used, spans, ffi_cb);
            collect_core_expr(end, imports, used, spans, ffi_cb);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(e, _) = part {
                    collect_core_expr(e, imports, used, spans, ffi_cb);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for e in items {
                collect_core_expr(e, imports, used, spans, ffi_cb);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                collect_core_expr(e, imports, used, spans, ffi_cb);
            }
        }
        Expr::MapLit(items, _) => {
            for (k, v) in items {
                collect_core_expr(k, imports, used, spans, ffi_cb);
                collect_core_expr(v, imports, used, spans, ffi_cb);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                collect_core_expr(e, imports, used, spans, ffi_cb);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    EnumLitArg::Positional(e) => collect_core_expr(e, imports, used, spans, ffi_cb),
                    EnumLitArg::Named { expr, .. } => collect_core_expr(expr, imports, used, spans, ffi_cb),
                }
            }
        }
        Expr::PatternTest { subject, .. } => collect_core_expr(subject, imports, used, spans, ffi_cb),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_core_expr(value, imports, used, spans, ffi_cb);
            match fallback {
                OrFallback::Value(e) => collect_core_expr(e, imports, used, spans, ffi_cb),
                OrFallback::Return(Some(e), _) => collect_core_expr(e, imports, used, spans, ffi_cb),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_core_expr(&arg.expr, imports, used, spans, ffi_cb);
                    }
                }
                OrFallback::Break(_) | OrFallback::Continue(_) => {}
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => collect_core_expr(e, imports, used, spans, ffi_cb),
            LambdaBody::Block(stmts) => collect_core_stmts(stmts, imports, used, spans, ffi_cb),
        },
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            collect_core_expr(cond, imports, used, spans, ffi_cb);
            collect_core_stmts(then_body, imports, used, spans, ffi_cb);
            collect_core_expr(then_value, imports, used, spans, ffi_cb);
            collect_core_stmts(else_body, imports, used, spans, ffi_cb);
            collect_core_expr(else_value, imports, used, spans, ffi_cb);
        }
        Expr::FanOut { callee, items, .. } => {
            collect_core_expr(callee, imports, used, spans, ffi_cb);
            for item in items {
                collect_core_expr(item, imports, used, spans, ffi_cb);
            }
        }
        Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::UnitLit { .. }
        | Expr::ComptimeSplice { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Paren(inner, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
        Expr::Spread(inner, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
    }
}

pub(crate) fn check_module_bodies(
    module: &mut crate::AST::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    mode: CompileMode,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    reference_anchors: &mut HashMap<(String, usize, usize), DefinitionAnchorFact>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): captured once — every function body check
    // below for this module gets the same file-scoped `policy no_alloc` state.
    let no_alloc = module.no_alloc_policy.is_some();
    let no_prelude = module.no_prelude;
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let invalid_serde_impls = invalid_serde_derive_impls(&module.items, &st.trait_reg);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // D-MEM-VIEWRET1=B: resolve callable view summaries before the real body
    // pass so declaration order cannot affect a public owner contract. Each
    // iteration checks pristine clones and publishes only the canonical fact;
    // diagnostics and other analysis products are discarded. Tentative facts
    // let mutually recursive SCCs converge; the real pass below still rejects
    // any path that ultimately conflicts or cannot stabilize.
    #[derive(Clone)]
    struct ViewSummaryJob {
        key: String,
        owner: Option<String>,
        trait_name: Option<String>,
        function: Func,
    }
    let mut view_jobs = Vec::new();
    for item in &module.items {
        match item {
            Item::Func(function) => view_jobs.push(ViewSummaryJob {
                key: function.name.clone(),
                owner: None,
                trait_name: None,
                function: function.clone(),
            }),
            Item::Struct(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
                        function: function.clone(),
                    });
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        view_jobs.push(ViewSummaryJob {
                            key: format!(
                                "{}::{}::{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            owner: Some(definition.name.clone()),
                            trait_name: Some(implementation.trait_name.clone()),
                            function: function.clone(),
                        });
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!("{}::{}", definition.name, function.name),
                        owner: Some(definition.name.clone()),
                        trait_name: None,
                        function: function.clone(),
                    });
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        view_jobs.push(ViewSummaryJob {
                            key: format!(
                                "{}::{}::{}",
                                definition.name, implementation.trait_name, function.name
                            ),
                            owner: Some(definition.name.clone()),
                            trait_name: Some(implementation.trait_name.clone()),
                            function: function.clone(),
                        });
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    view_jobs.push(ViewSummaryJob {
                        key: format!(
                            "{}::{}::{}",
                            implementation.type_name,
                            implementation.trait_name.as_deref().unwrap_or("inherent"),
                            function.name
                        ),
                        owner: Some(implementation.type_name.clone()),
                        trait_name: implementation.trait_name.clone(),
                        function: function.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    fn contains_view(registry: &TypeRegistry, ty: &Type, seen: &mut HashSet<String>) -> bool {
        match ty {
            Type::Apply { name, args }
                if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 => true,
            Type::Named(name) => {
                seen.insert(name.clone())
                    && registry.struct_fields(name).is_some_and(|fields| {
                        fields.iter().any(|(_, _, field_ty, _)| {
                            contains_view(registry, field_ty, seen)
                        })
                    })
            }
            Type::Apply { name, args } => {
                args.iter().any(|arg| contains_view(registry, arg, seen))
                    || (seen.insert(name.clone())
                        && registry.struct_fields(name).is_some_and(|fields| {
                            fields.iter().any(|(_, _, field_ty, _)| {
                                contains_view(registry, field_ty, seen)
                            })
                        }))
            }
            Type::Option(inner)
            | Type::List(inner)
            | Type::Shared(inner)
            | Type::Tagged { inner, .. } => contains_view(registry, inner, seen),
            Type::Result { ok, err } => {
                contains_view(registry, ok, seen) || contains_view(registry, err, seen)
            }
            Type::Map { key, value, .. } => {
                contains_view(registry, key, seen) || contains_view(registry, value, seen)
            }
            Type::Tuple(fields) => fields
                .iter()
                .any(|(_, field_ty)| contains_view(registry, field_ty, seen)),
            Type::FixedList { elem, .. } => contains_view(registry, elem, seen),
            Type::Fn { params, ret, .. } => {
                params
                    .iter()
                    .any(|param| contains_view(registry, param, seen))
                    || ret
                        .as_deref()
                        .is_some_and(|ret| contains_view(registry, ret, seen))
            }
            _ => false,
        }
    }
    view_jobs.retain(|job| {
        job.function.return_type.as_ref().is_some_and(|return_type| {
            contains_view(&st.registry, return_type, &mut HashSet::new())
        })
    });
    view_jobs.sort_by(|left, right| left.key.cmp(&right.key));
    let trait_job_counts = view_jobs.iter().fold(
        HashMap::<(String, String), usize>::new(),
        |mut counts, job| {
            if let Some(trait_name) = &job.trait_name {
                *counts
                    .entry((trait_name.clone(), job.function.name.clone()))
                    .or_default() += 1;
            }
            counts
        },
    );
    for _ in 0..=view_jobs.len() {
        let mut trait_candidates = HashMap::<
            (String, String),
            Vec<crate::AST::ViewProvenanceMap>,
        >::new();
        for job in &view_jobs {
            let mut function = job.function.clone();
            let mut scratch_summaries = HashMap::new();
            let mut scratch_inputs = Vec::new();
            let mut scratch_addr_taken = HashSet::new();
            let mut scratch_anchors = HashMap::new();
            let _ = check_func_body_bundle(
                &mut function,
                module_idx,
                states,
                job.owner.as_deref(),
                &ct_funcs,
                &ct_externs,
                &ct_base_dir,
                &ct_globals,
                freestanding,
                allow_impure,
                &mut scratch_summaries,
                &mut scratch_inputs,
                &mut scratch_addr_taken,
                no_alloc,
                no_prelude,
                &mut scratch_anchors,
            );
            if let (Some(trait_name), Some(provenance)) =
                (&job.trait_name, function.return_view_provenance)
            {
                trait_candidates
                    .entry((trait_name.clone(), function.name.clone()))
                    .or_default()
                    .push(provenance);
            }
        }
        for (key, candidates) in trait_candidates {
            if candidates.len() != trait_job_counts.get(&key).copied().unwrap_or(0) {
                continue;
            }
            let Some(first) = candidates.first() else {
                continue;
            };
            if !candidates.iter().all(|candidate| candidate == first) {
                continue;
            }
            if let Some(signature) = st
                .trait_reg
                .traits
                .get(&key.0)
                .and_then(|info| info.methods.get(&key.1))
            {
                let _ = signature.return_view_provenance.set(first.clone());
            }
        }
    }
    for item in &mut module.items {
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body_bundle(
                    f,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                no_prelude,
                reference_anchors,
                ));
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    if own_params.is_empty() {
                        m.type_params = s.type_params.clone();
                    }
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        reference_anchors,
                    ));
                    m.type_params = own_params;
                }
                // Trait impls nested in a struct are real method bodies too.
                // They inherit the struct's generic parameters, just as the
                // Rust impl emitted for them does.  Temporarily expose those
                // parameters to the ordinary body checker while preserving the
                // parsed method signature for codegen.
                for block in &mut s.trait_impls {
                    if matches!(
                        block.trait_name.as_str(),
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) {
                        // E0903 already rejected this built-in impl. Its body is
                        // not a valid checking context, so don't emit cascades.
                        continue;
                    }
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() {
                            s.type_params.clone()
                        } else {
                            own_params.clone()
                        };
                        diags.extend(check_func_body_bundle(
                            m,
                            module_idx,
                            states,
                            Some(&s.name),
                            &ct_funcs,
                            &ct_externs,
                            &ct_base_dir,
                            &ct_globals,
                            freestanding,
                            allow_impure,
                            summaries,
                            embed_inputs_out,
                            global_addr_taken,
                            no_alloc,
                            no_prelude,
                            reference_anchors,
                        ));
                        // Generated serde methods temporarily carry inherited,
                        // inferred bounds solely for sema. Their Rust generics
                        // belong on the enclosing impl, not on the method.
                        m.type_params = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) {
                            Vec::new()
                        } else {
                            own_params
                        };
                    }
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    if own_params.is_empty() {
                        m.type_params = e.type_params.clone();
                    }
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        reference_anchors,
                    ));
                    m.type_params = own_params;
                }
                for block in &mut e.trait_impls {
                    if matches!(
                        block.trait_name.as_str(),
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) {
                        continue;
                    }
                    for m in &mut block.methods {
                        let own_params = std::mem::take(&mut m.type_params);
                        m.type_params = if own_params.is_empty() { e.type_params.clone() } else { own_params.clone() };
                        diags.extend(check_func_body_bundle(
                            m, module_idx, states, Some(&e.name), &ct_funcs, &ct_externs,
                            &ct_base_dir, &ct_globals, freestanding, allow_impure, summaries,
                            embed_inputs_out, global_addr_taken, no_alloc, no_prelude,
                            reference_anchors,
                        ));
                        m.type_params = if matches!(block.trait_name.as_str(), crate::Generics::ENCODE | crate::Generics::DECODE) { Vec::new() } else { own_params };
                    }
                }
            }
            Item::Impl(i) => {
                if i.trait_name.as_deref().is_some_and(|trait_name| {
                    matches!(
                        trait_name,
                        crate::Generics::COMPARABLE | crate::Generics::EQUATABLE
                    ) || (i.is_generated_serde
                        && invalid_serde_impls
                            .contains(&(i.type_name.clone(), trait_name.to_string())))
                }) {
                    continue;
                }
                let owner_params = st
                    .trait_reg
                    .struct_params
                    .get(&i.type_name)
                    .or_else(|| st.trait_reg.enum_params.get(&i.type_name));
                for m in &mut i.methods {
                    let own_params = std::mem::take(&mut m.type_params);
                    if i.trait_name.is_none() && own_params.is_empty() {
                        m.type_params = owner_params.cloned().unwrap_or_default();
                    } else {
                        m.type_params = own_params.clone();
                    }
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                        freestanding,
                        allow_impure,
                        summaries,
                        embed_inputs_out,
                        global_addr_taken,
                        no_alloc,
                        no_prelude,
                        reference_anchors,
                    ));
                    m.type_params = own_params;
                }
            }
            Item::Test(t) if mode == CompileMode::Test => {
                // D-TEST1: a parameterized `@Test fn` is a property test — its
                // params must be generatable types so the runner can synthesize
                // inputs. Validate before checking the body so the error points at
                // the offending param type.
                for p in &t.params {
                    if let Some(d) = property_param_unsupported(&p.ty, p.ty_span) {
                        diags.push(d);
                    }
                }
                let mut synthetic = Func {
                    span: t.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: t.params.clone(),
                    return_type: None,
                    return_type_span: None,
                    return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    inline_foreign: None,
                    body: std::mem::take(&mut t.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                no_prelude,
                reference_anchors,
                ));
                t.body = synthetic.body;
            }
            // D-BENCH1: a `@Bench` body type-checks exactly like a `@Test` body
            // (a bare statement list, no params, unit context) — only the mode
            // gate differs.
            Item::Bench(b) if mode == CompileMode::Bench => {
                let mut synthetic = Func {
                    span: b.name_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!("__bench_{}", b.name),
                    name_span: b.name_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    return_type_span: None,
                    return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    inline_foreign: None,
                    body: std::mem::take(&mut b.body),
                };
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    freestanding,
                    allow_impure,
                    summaries,
                    embed_inputs_out,
                    global_addr_taken,
                    no_alloc,
                no_prelude,
                reference_anchors,
                ));
                b.body = synthetic.body;
            }
            Item::CodeModule(cm) => {
                // D-MOD2: type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                if let Some(body) = &mut cm.body {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
                            // Inline-module calls use their registered mangled
                            // identity (`module__fn`). Preserve any top-level
                            // same-name summary while the shared body checker
                            // emits this function's local summary.
                            let previous = summaries.remove(&f.name);
                            diags.extend(check_func_body_bundle(
                                f,
                                module_idx,
                                states,
                                None,
                                &ct_funcs,
                                &ct_externs,
                                &ct_base_dir,
                                &ct_globals,
                                freestanding,
                                allow_impure,
                                summaries,
                                embed_inputs_out,
                                global_addr_taken,
                                no_alloc,
                            no_prelude,
                            reference_anchors,
                            ));
                            if let Some(summary) = summaries.remove(&f.name) {
                                summaries.insert(format!("{}__{}", cm.name, f.name), summary);
                            }
                            if let Some(summary) = previous {
                                summaries.insert(f.name.clone(), summary);
                            }
                        }
                    }
                }
            }
            Item::ErrorConv(ec) => {
                let mut synthetic = Func {
                    span: ec.body_span,
                    is_pub: false,
                    is_package_pub: false,
                    external_type: None,
                    name: format!(
                        "__errconv_{}_to_{}",
                        ec.from_ty.replace('.', "_"),
                        ec.to_ty.replace('.', "_")
                    ),
                    name_span: ec.from_span,
                    meta: None,
                    type_params: Vec::new(),
                    params: vec![Param {
                        name: crate::Syntax::KW_SELF.to_string(),
                        name_span: ec.from_span,
                        ty: Type::Named(String::new()),
                        ty_span: ec.from_span,
                        convention: AccessConvention::Move,
                        default: None,
                        variadic: false,
                        variadic_bound_list: None,
                    }],
                    return_type: Some(Type::Named(ec.to_ty.clone())),
                    return_type_span: Some(ec.to_span),
                    return_view_provenance: None,
            gc_return: false,
            gc_scope: false,
                    is_unsafe: false,
                    unsafe_reason: None,
                    unsafe_span: None,
                    is_pure: false,
                    is_reactive: false,
                    is_replayable: false,
                    replayable_span: None,
                    is_task: false,
                    task_span: None,
                    every: None,
                    is_must_use: false,
                    must_use_span: None,
                    maturity: None,
                    maturity_span: None,
                    is_inline: false,
                    is_inline_always: false,
                    inline_span: None,
                    is_sanitizer: false,
                    declared_effects: None,
                    effect_via: None,
                    state_requires: None,
                    state_transition: None,
                    web_marker: None,
                    pre: Vec::new(),
                    post: Vec::new(),
                    inline_foreign: None,
                    body: std::mem::take(&mut ec.body),
                };
                // Error-conversion bodies are checked like functions, but they are
                // not functions: do not publish their synthetic names or local
                // analysis artifacts into the program-wide accumulators.
                let mut conversion_summaries = HashMap::new();
                let mut conversion_inputs = Vec::new();
                let mut conversion_addr_taken = HashSet::new();
                let mut conversion_anchors = HashMap::new();
                diags.extend(check_func_body_bundle(
                    &mut synthetic,
                    module_idx,
                    states,
                    Some(&ec.from_ty),
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                    false,
                    false,
                    &mut conversion_summaries,
                    &mut conversion_inputs,
                    &mut conversion_addr_taken,
                    no_alloc,
                    no_prelude,
                    &mut conversion_anchors,
                ));
                ec.body = synthetic.body;
            }
            _ => {}
        }
    }
    // D-MEM-VIEWRET1=B: a trait method has one public owner contract. Infer
    // it from checked implementations and reject disagreement before TIR.
    let mut trait_view_contracts: HashMap<
        (String, String),
        (crate::AST::ViewProvenanceMap, crate::Diagnostics::Span),
    > = HashMap::new();
    let mut record_trait_methods =
        |trait_name: &str, methods: &[Func], diags: &mut Vec<Diagnostic>| {
            for method in methods {
                let Some(provenance) = method.return_view_provenance.clone() else {
                    continue;
                };
                if provenance.is_empty() {
                    continue;
                }
                let key = (trait_name.to_string(), method.name.clone());
                if let Some((existing, _)) = trait_view_contracts.get(&key) {
                    if existing != &provenance {
                        diags.push(Diagnostic::error(
                            "E2305",
                            format!(
                                "implementations of `{}.{}` disagree about the returned view owner",
                                trait_name, method.name
                            ),
                            "one trait method has one public owner contract for every static or dynamic call"
                                .to_string(),
                            "return each view-bearing output slot from the same receiver or parameter position in every implementation"
                                .to_string(),
                            Some(method.name_span),
                        ));
                    }
                } else {
                    trait_view_contracts.insert(key, (provenance, method.name_span));
                }
            }
        };
    for item in &module.items {
        match item {
            Item::Impl(implementation) => {
                if let Some(trait_name) = implementation.trait_name.as_deref() {
                    record_trait_methods(trait_name, &implementation.methods, &mut diags);
                }
            }
            Item::Struct(definition) => {
                for implementation in &definition.trait_impls {
                    record_trait_methods(
                        &implementation.trait_name,
                        &implementation.methods,
                        &mut diags,
                    );
                }
            }
            Item::Enum(definition) => {
                for implementation in &definition.trait_impls {
                    record_trait_methods(
                        &implementation.trait_name,
                        &implementation.methods,
                        &mut diags,
                    );
                }
            }
            _ => {}
        }
    }
    for ((trait_name, method_name), (provenance, _)) in trait_view_contracts {
        if let Some(signature) = st
            .trait_reg
            .traits
            .get(&trait_name)
            .and_then(|info| info.methods.get(&method_name))
        {
            let _ = signature.return_view_provenance.set(provenance);
        }
    }
    let _ = st;
    diags
}

pub(crate) fn check_func_body_bundle(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
    allow_impure: bool,
    summaries: &mut HashMap<String, EffectSummary>,
    embed_inputs_out: &mut Vec<crate::AST::ComptimeInput>,
    global_addr_taken: &mut HashSet<String>,
    // D-MEM1/S7 (D-NOALLOC-SEM1=A): this module's `policy no_alloc` state.
    _no_alloc: bool,
    // D-PRELUDEX1=A: this file's `@NoPrelude` state.
    no_prelude: bool,
    reference_anchors: &mut HashMap<(String, usize, usize), DefinitionAnchorFact>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut ck = Checker {
        funcs: &st.funcs,
        registry: &st.registry,
        consts: &st.consts,
        modules: Some(states),
        module_idx,
        imports: &st.imports,
        core_imports: &st.core_imports,
        code_modules: &st.code_modules,
        code_module_identities: &st.code_module_identities,
        unqualified: &st.unqualified,
        unqualified_file: &st.unqualified_file,
        func_pub: &st.func_pub,
        func_pkg_pub: &st.func_pkg_pub,
        module_path: &st.module_path,
        reference_anchors,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        loop_labels: Vec::new(),
        fx_direct: std::collections::BTreeSet::new(),
        fx_direct_spans: HashMap::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        fx_maximal_span: None,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
        fx_callback_obligations: Vec::new(),
        fx_memory_events: Vec::new(),
        fx_memory_open: Vec::new(),
        memory_policy_stack: Vec::new(),
        fx_memory_regions: Vec::new(),
        fx_memory_unbounded_control: Vec::new(),
        fx_memory_calls: Vec::new(),
        memory_control_multiplier: Some(1),
        txn_depth: 0,
        det_suppress: 0,
        context_depth: 0,
        context_allocator_active: false,
        // S58 (E2-M13): an `@Unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `@Unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        suppress_must_use: false,
        in_pure: f.is_pure,
        no_prelude,
        in_pre_clause: false,
        in_comptime: false,
        ret: f.return_type.clone(),
        fn_name: f.name.clone(),
        current_param_names: f
            .params
            .iter()
            .filter(|param| param.name != crate::Syntax::KW_SELF)
            .map(|param| param.name.clone())
            .collect(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        view_facts: Default::default(),
        return_view_provenance: None,
        views_used_in_stmt: Default::default(),
        uninit: HashMap::new(),
        borrow_ctx: false,
        allow_fixed_constructor: false,
        allow_string_view_read: false,
        lambda_escapes: true,
        in_lambda_body: false,
        is_task_spawn: false,
        lambda_param_mutable: false,
        view_capture_tasks: HashSet::new(),
        view_borrow_escape_tasks: HashSet::new(),
        current_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        trait_reg: &st.trait_reg,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
        freestanding,
        allow_impure,
        ct_impure_depth: 0,
        ct_embed_inputs: Vec::new(),
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
        taskgroup_stack: Vec::new(),
        in_taskgroup_spawn: false,
        inline_addr_taken: HashSet::new(),
    };
    for (active, name, span) in [
        (f.is_pure, crate::Syntax::KW_PURE, f.name_span),
        (f.is_sanitizer, crate::Syntax::KW_SANITIZER, f.name_span),
        (f.is_unsafe, crate::Syntax::KW_UNSAFE, f.unsafe_span.unwrap_or(f.name_span)),
        (f.is_replayable, crate::Syntax::ATTR_REPLAYABLE, f.replayable_span.unwrap_or(f.name_span)),
        (f.is_must_use, crate::Syntax::ATTR_MUST_USE, f.must_use_span.unwrap_or(f.name_span)),
        (f.is_inline, crate::Syntax::CONTRACT_INLINE, f.inline_span.unwrap_or(f.name_span)),
        (f.is_inline_always, crate::Syntax::CONTRACT_INLINE_ALWAYS, f.inline_span.unwrap_or(f.name_span)),
        (f.is_reactive, crate::Syntax::KW_REACTIVE, f.name_span),
    ] {
        if active && !crate::Policy::rule_allows(name, crate::Policy::RuleSite::Function) {
            ck.diags.push(Diagnostic::error("E0355", format!("`@{name}` cannot attach to a function"), "the compiler-owned applicability registry is shared by parser, sema, formatter, semantic index, and explain".to_string(), "move the rule to one of its registered sites".to_string(), Some(span)));
        }
    }
    ck.check_params_and_body(f, owner_type);
    f.return_view_provenance = ck.return_view_provenance.clone();
    if let Some(owner) = owner_type {
        if let (Some(signature), Some(provenance)) =
            (st.registry.method(owner, &f.name), f.return_view_provenance.clone())
        {
            let _ = signature.return_view_provenance.set(provenance);
        }
    } else {
        if let (Some(signature), Some(provenance)) =
            (st.funcs.get(&f.name), f.return_view_provenance.clone())
        {
            let _ = signature.return_view_provenance.set(provenance);
        }
    }
    // Direct ambient/foreign operations keep their precise body diagnostic.
    // User callees are checked after the whole-program effect fixpoint so an
    // inferred-pure callee need not repeat `--[]->`.
    if f.is_pure {
        ck.diags.extend(check_pure_fn(f, &st.funcs));
    }
    // D-METHODMACRO1=A: the local half of the `@InlineAlways` check (self-
    // recursion E0917 + size ceiling E0919); roll this function's
    // address-taken names into the whole-program accumulator so the E0918
    // pass after the full bundle check can see them.
    if f.is_inline_always {
        ck.diags.extend(check_inline_always_fn(f));
    }
    // D-SCHEDULE1 (card #505): a bad `@Every(…)` value is E0926.
    ck.diags.extend(check_every_marker(f));
    global_addr_taken.extend(std::mem::take(&mut ck.inline_addr_taken));
    // D-EXPANDCLI1 (card #183): roll this function's resolved ref-owner facts
    // into the whole-bundle accumulator for `jet inspect expand --facts refs`.
    // D-CTEFFECT1 Tier-1: drain embed inputs into the caller's accumulator.
    embed_inputs_out.extend(std::mem::take(&mut ck.ct_embed_inputs));
    // D-EFFECT-OMIT1/D-EFF3: an explicit row is an upper bound, not an effect
    // declaration. Static calls propagate the implementation's inferred body
    // row; dynamic trait calls use the trait method bound separately.
    let direct = std::mem::take(&mut ck.fx_direct);
    for event in &mut ck.fx_memory_events {
        event.source = st.module_path.clone();
        event.provenance = format!("{} in {}", effect_key(owner_type, &f.name), st.module_path);
    }
    for region in &mut ck.fx_memory_regions {
        for event in &mut region.events {
            event.source = st.module_path.clone();
            event.provenance = format!(
                "{} block policy in {}",
                effect_key(owner_type, &f.name),
                st.module_path
            );
        }
    }
    summaries.insert(
        effect_key(owner_type, &f.name),
        EffectSummary {
            direct,
            direct_spans: std::mem::take(&mut ck.fx_direct_spans),
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            maximal_span: ck.fx_maximal_span,
            unbounded_trait_dispatch: false,
            regions: std::mem::take(&mut ck.fx_regions),
            callback_obligations: std::mem::take(&mut ck.fx_callback_obligations),
            memory: super::MemoryFacts::MemorySummary {
                events: std::mem::take(&mut ck.fx_memory_events),
                open_dispatches: std::mem::take(&mut ck.fx_memory_open),
                regions: std::mem::take(&mut ck.fx_memory_regions),
                unbounded_control: std::mem::take(&mut ck.fx_memory_unbounded_control),
                calls: std::mem::take(&mut ck.fx_memory_calls),
            },
        },
    );
    ck.diags
}

pub(crate) fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
        ret: sig.return_type.clone().map(Box::new),
        effect_bound: None,
    }
}

pub(crate) fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let (
        Type::Fn {
            params: wp,
            ret: wr,
            ..
        },
        Type::Fn {
            params: gp,
            ret: gr,
            ..
        },
    ) = (want, got)
    else {
        return false;
    };
    if wp.len() != gp.len() {
        return false;
    }
    for (a, b) in wp.iter().zip(gp.iter()) {
        if a != b {
            return false;
        }
    }
    match (wr, gr) {
        (None, None) => true,
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// D-TEST1: which parameter types the property-test runner can synthesize inputs
/// for. The generator (codegen) covers the scalar value types plus `[T]` and
/// `T?` of a generatable element. Anything else (user structs/enums, `Map`,
/// functions, trait objects) has no automatic generator yet, so reject it with a
/// clear error rather than miscompile (I3 — checking lives in sema).
fn property_param_generatable(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Float32
        | Type::IntN { .. } => true,
        Type::List(inner) | Type::Option(inner) => property_param_generatable(inner),
        Type::FixedList { elem, .. } => property_param_generatable(elem),
        _ => false,
    }
}

/// E0613: a property-test parameter type with no automatic value generator.
pub(super) fn property_param_unsupported(ty: &Type, span: Span) -> Option<Diagnostic> {
    if property_param_generatable(ty) {
        return None;
    }
    Some(Diagnostic::error(
        "E0613",
        format!(
            "a property test can't generate values of type `{}`",
            ty.name()
        ),
        format!(
            "a parameterized `@{} fn` is a property test (D-TEST1): {} generates inputs from each parameter's type, but this type has no built-in generator",
            Syntax::KW_TEST,
            Syntax::LANG_NAME
        ),
        "use a generatable type (Int, Float, Bool, String, Char, a sized integer, or a list/optional of those), or write a plain `@Test \"name\" { … }` block and construct the value yourself".to_string(),
        Some(span),
    ))
}
