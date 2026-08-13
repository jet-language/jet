//! Core-usage collection: walk function bodies to find every Core symbol a
//! module reaches for, plus the compiler-owned source/intrinsic closure that
//! rides along with it. Split out of `Validation.rs` to keep the module
//! under the card #510 boundary.

use super::*;
use crate::AST::Type;

pub(crate) const CORE_SOURCE_MARKER_PREFIX: &str = "__core_source::";
pub(crate) const CORE_INTRINSIC_MARKER_PREFIX: &str = "__core_intrinsic::";

fn is_core_closure_marker(usage: &str) -> bool {
    usage.starts_with(CORE_SOURCE_MARKER_PREFIX)
        || usage.starts_with(CORE_INTRINSIC_MARKER_PREFIX)
}

/// `__core_source` is reserved for a package whose source tree is actually
/// available to the Core provider. Compiler-owned runtime fragments use only
/// the intrinsic marker; claiming a source package for them would make cache
/// and provenance records lie about their authority.
fn has_core_source_package(module: &str) -> bool {
    // D-CORE-SOURCE-AUTHORITY1=A: archive is the current Core package boundary.
    // Keep this list explicit until each remaining compiler-owned surface has a
    // real package source tree and the sema loader can consume it.
    module == "core.archive"
}

/// Attach the semantic Core source and intrinsic closure to the direct helper
/// set. These entries are compiler metadata, not user-callable helpers: codegen
/// uses them to select the owning package or audited ABI kernel, and the cache
/// salts them into the artifact identity.
pub(crate) fn expand_core_reachable_closure(used: &mut HashSet<String>) {
    let direct: Vec<String> = used
        .iter()
        .filter(|usage| !is_core_closure_marker(usage))
        .cloned()
        .collect();
    for usage in direct {
        let (module, helper) = usage
            .split_once("::")
            .map_or((usage.as_str(), None), |(module, helper)| {
                (module, Some(helper))
            });
        if has_core_source_package(module) {
            used.insert(format!("{CORE_SOURCE_MARKER_PREFIX}{module}"));
            if helper.is_some_and(|helper| !helper.is_empty()) {
                used.insert(format!("{CORE_SOURCE_MARKER_PREFIX}{usage}"));
            }
        }

        let intrinsic = if module == "core.archive" {
            "archive.abi"
        } else {
            module
        };
        used.insert(format!("{CORE_INTRINSIC_MARKER_PREFIX}{intrinsic}"));
        if let Some(helper) = helper.filter(|helper| !helper.is_empty()) {
            used.insert(format!(
                "{CORE_INTRINSIC_MARKER_PREFIX}{intrinsic}::{helper}"
            ));
        }
    }
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
    // `#Extern` call site anywhere in the bundle. Collected in this same
    // whole-program walk (not a second traversal) so codegen knows, before it
    // emits ANY function, which ones must be `extern "C" fn` — never every
    // `#Pure fn` (that leaked the purity lever into codegen and broke I3
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
        let mut module_imports = imports.clone();
        for ((scope, name), (core_module, core_item)) in &states[idx].inline_reexport_core {
            module_imports.insert(
                format!("{scope}.{name}"),
                format!("{core_module}::{core_item}"),
            );
        }
        for item in &module.items {
            match item {
                Item::Func(f) => collect_core_stmts(&f.body, &module_imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Struct(s) => {
                    for m in &s.methods {
                        collect_core_stmts(&m.body, &module_imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        collect_core_stmts(&m.body, &module_imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        collect_core_stmts(&m.body, &module_imports, &mut used, &mut spans, &mut ffi_cb);
                    }
                }
                Item::Test(t) => collect_core_stmts(&t.body, &module_imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Bench(b) => collect_core_stmts(&b.body, &module_imports, &mut used, &mut spans, &mut ffi_cb),
                Item::Const(c) => collect_core_expr(&c.value, &module_imports, &mut used, &mut spans, &mut ffi_cb),
                Item::CodeModule(cm) => {
                    let Some(body) = &cm.body else { continue };
                    let mut scoped_imports = module_imports.clone();
                    for ((scope, name), core_module) in &states[idx].inline_core_imports {
                        if scope == &cm.name {
                            scoped_imports.insert(name.clone(), core_module.clone());
                        }
                    }
                    for ((scope, name), (core_module, core_item)) in
                        &states[idx].inline_reexport_core
                    {
                        if scope == &cm.name {
                            scoped_imports.insert(
                                format!("{scope}.{name}"),
                                format!("{core_module}::{core_item}"),
                            );
                        }
                    }
                    for inner in body {
                        match inner {
                            Item::Func(f) => collect_core_stmts(
                                &f.body,
                                &scoped_imports,
                                &mut used,
                                &mut spans,
                                &mut ffi_cb,
                            ),
                            Item::Struct(s) => {
                                for method in &s.methods {
                                    collect_core_stmts(
                                        &method.body,
                                        &scoped_imports,
                                        &mut used,
                                        &mut spans,
                                        &mut ffi_cb,
                                    );
                                }
                            }
                            Item::Impl(i) => {
                                for method in &i.methods {
                                    collect_core_stmts(
                                        &method.body,
                                        &scoped_imports,
                                        &mut used,
                                        &mut spans,
                                        &mut ffi_cb,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Item::EffectDecl(_)
                | Item::MarkerDecl(_)
                | Item::FactDecl(_)
                | Item::Trait(_)
                | Item::Tag(_) // D-QUAL2: tags use no core imports
                | Item::ExternRust(_)
                | Item::Module(_)
                | Item::Distinct(_)
                | Item::TypeAlias(_) // D-TYPEALIAS1: erases
                | Item::UnitFamily(_) // D-QUAL3: lowered to distinct types
                | Item::CModule(_)
                | Item::ErrorConv(_)
                | Item::Migration(_) // D-MIGRATE1
                | Item::StateDecl(_) // D-STATE-DECL: uses no core imports
                | Item::ProtocolDecl(_) // D-PROTO1/D-PROTO2: erases
                | Item::UserDerive(_) // D-METADERIVE1=A: already expanded
                | Item::GenericModule(_) // D-CONF-GENSPELL1=A: template — erases
                | Item::ModuleAlias(_) => {} // D-CONF-GENSPELL1=A: alias — erases after expansion
            }
        }
    }
    expand_core_reachable_closure(&mut used);
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

fn note_typed_boundary_core_usage(
    used: &mut HashSet<String>,
    spans: &mut HashMap<String, crate::Diagnostics::Span>,
    name: &str,
    span: Option<crate::Diagnostics::Span>,
) {
    let Some(kind) = Syntax::typed_head_kind(name).filter(|kind| kind.is_boundary()) else {
        return;
    };
    let key = match kind {
        Syntax::TypedHeadKind::URL => "core.url::typed_head",
        Syntax::TypedHeadKind::Path => "core.files::typed_head",
        Syntax::TypedHeadKind::DateTime => "core.time::typed_head",
        _ => unreachable!("typed boundary usage descriptor is complete"),
    };
    note_core_usage(used, spans, key, span);
}

fn is_http_nominal_type(name: &str) -> bool {
    matches!(
        name,
        "HTTPMethod"
            | "HTTPStatus"
            | "HTTPVersion"
            | "HTTPHeaderName"
            | "HTTPHeaderValue"
            | "HTTPHeaders"
            | "HTTPBody"
    )
}

/// D-RINGLAYER1=A M2: bump inferred layer from emitted helper usage and enforce ceiling.
pub(crate) fn apply_helper_layer_inference(
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
        if is_core_closure_marker(usage) {
            continue;
        }
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
            Stmt::Expr(e)
            | Stmt::Yield(e, _)
            | Stmt::DeferClose { close: e, .. } => collect_core_expr(e, imports, used, spans, ffi_cb),
            Stmt::Val(b) => collect_core_expr(&b.init, imports, used, spans, ffi_cb),
            Stmt::Assign { target, value, .. } => {
                collect_core_lvalue(target, imports, used, spans, ffi_cb);
                collect_core_expr(value, imports, used, spans, ffi_cb);
            }
            Stmt::Return(Some(e), _) => collect_core_expr(e, imports, used, spans, ffi_cb),
            Stmt::BreakValue(e, _) | Stmt::BreakLabelValue(_, _, e, _) => {
                collect_core_expr(e, imports, used, spans, ffi_cb)
            }
            Stmt::Return(None, _) => {}
            Stmt::While { cond, body, .. } => {
                collect_core_expr(cond, imports, used, spans, ffi_cb);
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step, exclusive: _ } => {
                        collect_core_expr(start, imports, used, spans, ffi_cb);
                        collect_core_expr(end, imports, used, spans, ffi_cb);
                        if let Some(step) = step {
                            collect_core_expr(step, imports, used, spans, ffi_cb);
                        }
                    }
                    ForKind::In { collection, step } => {
                        collect_core_expr(collection, imports, used, spans, ffi_cb);
                        if let Some(step) = step {
                            collect_core_expr(step, imports, used, spans, ffi_cb);
                        }
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
                if let Some(step) = step {
                    collect_core_stmts(std::slice::from_ref(step.as_ref()), imports, used, spans, ffi_cb);
                }
            }
            Stmt::TaskGroup { body, span, .. } => {
                // D-CONC-SPAWN1=D: the canonical `task.group` surface reaches
                // the same embedded scheduler kernel as task combinators. It
                // has no Core import for `collect_used_core` to observe, so
                // record the compiler-owned runtime seam explicitly.
                note_core_usage(used, spans, "core.concurrency::task", Some(*span));
                collect_core_stmts(body, imports, used, spans, ffi_cb);
            }
            Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
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
            // D-META-STAGE1=B (formerly D-CTMARKER1): collect Core usage from comptime block body.
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
            if c.name == crate::Syntax::TYPE_BIGINT
                || c.name == crate::Syntax::TYPE_DECIMAL
                || c.name == crate::Syntax::TYPE_FRACTION =>
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
                    note_core_usage(used, spans, "core.files::__pathapi__", Some(*method_span));
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
            // D-CONC-SPAWN1=D: bare `task expr`/`task.all|race|any { … }` parse
            // into a method call on the parser-private `INTERNAL_TASK_RECEIVER`
            // ident (set before sema ever runs), so this is available even
            // though `recv_type` isn't filled in until inference. Needs the
            // same embedded `jet_std` runtime as `task.group` above, with no
            // `use core.X` import required.
            if matches!(receiver.as_ref(), Expr::Ident(n, _) if n == crate::Syntax::INTERNAL_TASK_RECEIVER)
            {
                note_core_usage(used, spans, "core.concurrency::task", Some(*method_span));
            }
            // D-CONC-SPAWN1=D: `task`/`task.all`/`task.race`/`task.any`
            // are compiler-owned syntax, not Core imports. Sema records their
            // private dispatch type so this late reachability walk can pull in
            // the scheduler kernel without relying on source spelling.
            if matches!(
                recv_type.as_deref(),
                Some(Syntax::INTERNAL_TASK_SURFACE_TYPE)
                    | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
            ) {
                note_core_usage(used, spans, "core.concurrency::task", Some(*method_span));
            }
            // Epoch 3 String surface delegates Unicode classification, title
            // casing, trimming, and display-width padding to the pinned
            // `core.text` implementation. Mark that shared prelude reachable
            // even though these are ambient String methods rather than a
            // qualified core call.
            if matches!(
                method.as_str(),
                "trim_start"
                    | "trim_end"
                    | "pad_start"
                    | "pad_end"
                    | "is_alphabetic"
                    | "is_numeric"
                    | "is_whitespace"
                    | "is_ascii"
                    | "to_title"
                    | "is_lower"
                    | "is_upper"
                    | "capitalize"
                    | "swapcase"
                    | "normalize"
                    | "last_index_of"
                    | "remove_prefix"
                    | "remove_suffix"
            )
            {
                note_core_usage(
                    used,
                    spans,
                    "core.text::__string_surface__",
                    Some(*method_span),
                );
            }
            if matches!(
                recv_type.as_deref(),
                Some(crate::Syntax::TYPE_BIGINT)
                    | Some(crate::Syntax::TYPE_DECIMAL)
                    | Some(crate::Syntax::TYPE_FRACTION)
            ) {
                note_core_usage(
                    used,
                    spans,
                    "core.math::__precise__",
                    Some(*method_span),
                );
            }
            if matches!(
                recv_type.as_deref(),
                Some(
                    "Secret"
                        | "SigningKey"
                        | "VerifyKey"
                        | "X25519SecretKey"
                        | "X25519PublicKey"
                        | "SharedSecret"
                        | "Signature"
                        | "Sealed"
                        | "WrappedKey"
                        | "Digest256"
                        | "Digest512"
                        | "PasswordHash"
                )
            ) {
                note_core_usage(
                    used,
                    spans,
                    format!("core.crypto::__nominal__.{method}"),
                    Some(*method_span),
                );
            }
            // D-NETDEP1=A: sema normalizes `http.Body.bytes(...)` and the other
            // static HTTP nominal calls to a bare `HTTP*` receiver before this
            // whole-program walk. Preserve the module reachability marker so
            // AOT embeds the same HTTPMessage Prelude source used by JIT.
            if recv_type.as_deref().is_some_and(is_http_nominal_type)
                || matches!(receiver.as_ref(), Expr::Ident(name, _) if is_http_nominal_type(name))
            {
                note_core_usage(used, spans, "core.http::__nominal__", Some(*method_span));
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
                if let Some(module) = imports.get(&format!("{alias}.{method}")) {
                    if let Some((module, item)) = module.split_once("::") {
                        note_core_usage(
                            used,
                            spans,
                            format!("{module}::{item}"),
                            Some(*method_span),
                        );
                    }
                } else if let Some(module) = imports.get(alias) {
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
                // D-CABI-CALLBACK1: a qualified `#Extern`-module call
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
            // D-NAME-ALIAS1=A: bare `input(...)` is prelude-ambient; mark core.io so
            // CORELIB_PRELUDE is emitted and jet_std_io_input is in scope for codegen.
            if c.name == Syntax::BUILTIN_INPUT {
                note_core_usage(used, spans, "core.io::input", Some(c.name_span));
            }
            // D-BOUND-HEAD1=A: sema rewrites typed boundary heads to their
            // ordinary alternating literal/hole call before this reachability
            // walk. The Syntax descriptor selects the owning prelude fragment.
            note_typed_boundary_core_usage(used, spans, &c.name, Some(c.name_span));
            for arg in &c.args {
                // D-CABI-CALLBACK1: `arg.flags.c_callback_symbol` means sema
                // already proved this bare function name is passed as a stable
                // C callback at a `#Extern` call site — record the referenced
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
        | Expr::Err(inner, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
        Expr::Try(inner, _, _, note) => {
            collect_core_expr(inner, imports, used, spans, ffi_cb);
            if let Some(note) = note {
                collect_core_expr(note, imports, used, spans, ffi_cb);
            }
        }
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
            base, start, end, range, ..
        } => {
            collect_core_expr(base, imports, used, spans, ffi_cb);
            if let Some(range) = range {
                collect_core_expr(range, imports, used, spans, ffi_cb);
            } else {
                collect_core_expr(start, imports, used, spans, ffi_cb);
                collect_core_expr(end, imports, used, spans, ffi_cb);
            }
        }
        Expr::Range { start, end, .. } => {
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
        Expr::TypedLit { head, body, span } => {
            if let Some(Type::Named(name)) = head {
                note_typed_boundary_core_usage(used, spans, name, Some(*span));
            }
            body.for_each_expr(|e| collect_core_expr(e, imports, used, spans, ffi_cb));
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
                OrFallback::Block { body, value, .. } => {
                    collect_core_stmts(body, imports, used, spans, ffi_cb);
                    collect_core_expr(value, imports, used, spans, ffi_cb);
                }
                OrFallback::Return(Some(e), _) => collect_core_expr(e, imports, used, spans, ffi_cb),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_core_expr(&arg.expr, imports, used, spans, ffi_cb);
                    }
                }
                OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
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
        Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_)
        | Expr::ReduceMarker(_, _)
        | Expr::Todo { .. }
        | Expr::NoElse(_)
        | Expr::UnitLit { .. }
        | Expr::ComptimeName { .. }
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): a leaf
        // literal, no nested `Expr` to recurse into.
        | Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _) => {}
        Expr::Paren(inner, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
        Expr::Spread(inner, _) => collect_core_expr(inner, imports, used, spans, ffi_cb),
        Expr::MemberSpread { base, .. } => collect_core_expr(base, imports, used, spans, ffi_cb),
    }
}
