use crate::AST::{AccessConvention, Type, ViewSource};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::rust_param_type;
use crate::Codegen::rust_return_type;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::SerdeCodec;
use crate::Codegen::TIR::TFunc;
use crate::Codegen::TIR::TFuncKind;
use crate::Codegen::TIR::TStmt;

/// Emit a covered function from its TIR, reusing the same pure formatting helpers
/// as `emit_func` so the output is byte-identical to the AST path (golden parity).
/// The only difference is that every decision is *read off the TIR* rather than
/// recomputed — there is no `expr_jet_ty` / `operand_is_integer` call anywhere.

pub(crate) fn emit_tir_func(tir: &TFunc, cx: &Cx, out: &mut String) {
    match &tir.kind {
        TFuncKind::TopLevel => emit_tir_toplevel(tir, cx, out),
        TFuncKind::Method { self_conv, .. } => emit_tir_method(tir, *self_conv, cx, out),
        TFuncKind::TraitMethod {
            is_unsafe,
            self_conv,
            serde,
        } => emit_tir_trait_method(tir, *is_unsafe, *self_conv, *serde, cx, out),
        TFuncKind::Delegation {
            sig,
            fwd,
            has_return,
        } => emit_tir_delegation(tir, sig, fwd, *has_return, cx, out),
    }
}

/// A module-level free function: `pub fn name(params) -> ret { … }`.
/// Byte-identical to `emit_func`'s output.
pub(crate) fn emit_tir_toplevel(tir: &TFunc, cx: &Cx, out: &mut String) {
    let view_provenance = tir.return_view_provenance.as_ref();
    let view_owner_params = view_provenance
        .into_iter()
        .flat_map(|map| map.values())
        .flat_map(|provenance| provenance.sources.iter())
        .filter_map(|source| match source.source {
            ViewSource::Parameter(index) => Some(index),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let ret_clause = match &tir.ret {
        Some(t) => {
            let rust = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let rust = if tir.gc_return {
                format!("jet_gc::AutomaticRoot<{rust}>")
            } else {
                rust
            };
            format!(" -> {rust}")
        }
        None => String::new(),
    };
    let params = tir
        .params
        .iter()
        .enumerate()
        .map(|(index, (rust_name, ty, conv))| {
            let rust = rust_param_type(cx, *conv, ty);
            let rust = if view_owner_params.contains(&index) {
                add_hidden_view_lifetime(rust)
            } else {
                rust
            };
            format!("{rust_name}: {rust}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if tir.is_main { "" } else { "pub " };
    // c109 Phase 18: an `#Unsafe fn` lowers to `unsafe fn` — the prefix sits right after
    // `vis`, exactly as `emit_func` (`{vis}{unsafe_kw}fn …`). I1: emitted ONLY when the
    // source was `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-CABI-CALLBACK1: `extern "C" fn` ONLY for a function sema proved is
    // actually passed as a native callback symbol somewhere (`cx.ffi_callback_fns`,
    // built from `CallArgFlags::c_callback_symbol` — see
    // `crates/jet-sema/src/Sema/Bundle.rs::collect_core_expr`). Never every
    // `#Pure fn`: that leaked the purity lever into codegen and broke I3
    // erasure (`effect_annotations_are_erased`, `eff2_levers_are_erased`,
    // fixed by 14dd68a5) — but a bare fn reference handed to a `#Extern`
    // C-ABI callback parameter (`callback_twice(increment, x)`) genuinely
    // needs the C calling convention: the referenced Rust item's own type
    // must match the raw `extern "C" fn` pointer type the C side expects.
    let abi = if cx.ffi_callback_fns.contains(&tir.name) && tir.generics.is_empty() {
        "extern \"C\" "
    } else {
        ""
    };
    // D-METHODMACRO1=A: `#Inline`/`#Inline(Always)` lower to a Rust `#[inline]`/
    // `#[inline(always)]` attribute right above the signature. `is_inline_always`
    // is only ever `true` here once sema has confirmed the function can actually
    // inline (E0917/E0918/E0919 would have failed the build otherwise) — I3:
    // sema decides, codegen just emits.
    let inline_attr = if tir.is_inline_always {
        "#[inline(always)]\n"
    } else if tir.is_inline {
        "#[inline]\n"
    } else {
        ""
    };
    let kernel_proof = tir
        .kernel_proof
        .map(|proof| {
            format!(
                "const _: () = assert!({}, \"Jet kernel proof must be complete\");\n\
/* jet-kernel-proof: mode={} bounds={} alias_free={} captures={} race_free={} barriers_uniform={} control_flow={} */\n",
                proof.is_complete(),
                proof.mode.as_str(),
                proof.bounds,
                proof.alias_free,
                proof.captures,
                proof.race_free,
                proof.barriers_uniform,
                proof.control_flow,
            )
        })
        .unwrap_or_default();
    // E2-M12 D-OBS1: track the current function name for rich panic reports —
    // matches `emit_func` so panic output is identical.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    let generics = if has_view_return {
        add_hidden_view_generic(&tir.generics)
    } else {
        tir.generics.clone()
    };
    // D-DATARACE1=C: surface synchronized-form upgrades before the Rust fn.
    for line in &tir.reactive_upgrades {
        out.push_str(&format!("/* jet-reactive-upgrade: {line} */\n"));
    }
    out.push_str(&format!(
        "{kernel_proof}{inline_attr}{vis}{unsafe_kw}{abi}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&tir.name),
        gen = generics,
        params = params,
        ret = ret_clause,
        abi = abi,
    ));
    // D-COV1: probe at the function head (skip the synthetic `main`).
    if cx.coverage && !tir.is_main {
        out.push_str(&format!("    jet_cov({});\n", tir.line));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, 1);
    } else if matches!(&tir.ret, Some(Type::Apply { name, .. }) if name == "Stream") {
        emit_generator_wrapped_body(&tir.body, cx, out, 1);
    } else {
        emit_tir_stmts(&tir.body, cx, out, 1);
    }
    if is_fallible_void_return(&tir.ret, cx) {
        out.push_str("    Ok(())\n");
    }
    out.push_str("}\n\n");
}

fn add_hidden_view_lifetime(rust_type: String) -> String {
    if let Some(rest) = rust_type.strip_prefix("&mut ") {
        format!("&'__jet_view mut {rest}")
    } else if let Some(rest) = rust_type.strip_prefix('&') {
        format!("&'__jet_view {rest}")
    } else {
        rust_type
    }
}

fn add_hidden_view_generic(generics: &str) -> String {
    if generics.is_empty() {
        "<'__jet_view>".to_string()
    } else if let Some(rest) = generics.strip_prefix('<') {
        format!("<'__jet_view, {rest}")
    } else {
        generics.to_string()
    }
}

fn is_fallible_void_return(ret: &Option<Type>, cx: &Cx) -> bool {
    matches!(
        ret,
        Some(Type::Result { ok, err })
            if matches!(ok.as_ref(), Type::Named(n) if n == crate::Syntax::TYPE_VOID)
                && matches!(err.as_ref(), Type::Named(n)
                    if n == crate::Syntax::TYPE_ERROR
                        || (n == "CryptoError" && !cx.type_names.contains(n)))
    )
}

/// D-STREAMYIELD1: a generator (`=> Stream<T>`) spawns its body on its own
/// thread and hands the caller the channel receiver immediately — `yield`
/// (lowered to `__jet_yield_tx.send(...)`) blocks on the rendezvous channel
/// until the consumer's `loop x; stream { }` pulls the next value. No
/// coroutine/async machinery: a real OS thread IS the suspended generator.
fn emit_generator_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&format!(
        "{}let (__jet_yield_tx, __jet_yield_rx) = std::sync::mpsc::sync_channel(0);\n",
        pad
    ));
    out.push_str(&format!("{}std::thread::spawn(move || {{\n", pad));
    emit_tir_stmts(body, cx, out, inner);
    out.push_str(&format!("{}}});\n", pad));
    out.push_str(&format!("{}__jet_yield_rx\n", pad));
}

fn emit_reactive_wrapped_body(body: &[TStmt], cx: &Cx, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let inner = indent + 1;
    out.push_str(&format!(
        "{}{}jet_std::jet_reactive_effect_rooted({});\n",
        pad,
        cx.root_prefix,
        render_reactive_tir_closure(body, cx, inner)
    ));
}

fn render_reactive_tir_closure(body: &[TStmt], cx: &Cx, indent: usize) -> String {
    let mut inner = String::new();
    emit_tir_stmts(body, cx, &mut inner, indent);
    format!("move || {{ {} }}", inner)
}

/// c109 Phase 7: an inherent method, emitted INSIDE an `impl user_<T> { … }` block
/// (the caller `emit_type_impl` already opened it). Byte-identical to `emit_method`:
/// `    pub fn user_<name>(<self>, <params>) -> <ret> {\n … \n    }\n`. The `self`
/// receiver form comes from `self_conv` (`Read`→`&self`, `Mutate`→`&mut self`,
/// `Move`→`self`); a static method (`self_conv == None`) emits no receiver.
pub(crate) fn emit_tir_method(
    tir: &TFunc,
    self_conv: Option<AccessConvention>,
    cx: &Cx,
    out: &mut String,
) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    let view_provenance = tir.return_view_provenance.as_ref();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let borrows_receiver = view_provenance.is_some_and(|map| {
        map.values().any(|provenance| {
            provenance
                .sources
                .iter()
                .any(|source| matches!(source.source, ViewSource::Receiver))
        })
    });
    let ret_clause = match &tir.ret {
        Some(t) => {
            let rust = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let rust = if tir.gc_return {
                format!("jet_gc::AutomaticRoot<{rust}>")
            } else {
                rust
            };
            format!(" -> {rust}")
        }
        None => String::new(),
    };
    let mut params: Vec<String> = Vec::new();
    if let Some(conv) = self_conv {
        params.push(
            match conv {
                AccessConvention::Read
                    if borrows_receiver =>
                {
                    "&'__jet_view self"
                }
                AccessConvention::Write
                    if borrows_receiver =>
                {
                    "&'__jet_view mut self"
                }
                AccessConvention::Read => "&self",
                AccessConvention::Write => "&mut self",
                AccessConvention::Move => "self",
            }
            .to_string(),
        );
    }
    for (index, (rust_name, ty, conv)) in tir.params.iter().enumerate() {
        let rust = rust_param_type(cx, *conv, ty);
        let rust = if view_provenance.is_some_and(|map| map.values().any(|provenance| {
            provenance.sources.iter().any(
                |source| matches!(source.source, ViewSource::Parameter(owner) if owner == index),
            )
        })) {
            add_hidden_view_lifetime(rust)
        } else {
            rust
        };
        params.push(format!("{rust_name}: {rust}"));
    }
    // c109 Phase 18: an `#Unsafe fn` inherent method lowers to `pub unsafe fn` — the
    // prefix sits between `pub ` and `fn`, exactly as `emit_method` (`pub {unsafe_kw}fn`).
    // I1: emitted ONLY for a source `#Unsafe fn` (`tir.is_unsafe`).
    let unsafe_kw = if tir.is_unsafe { "unsafe " } else { "" };
    // D-METHODMACRO1=A: `#Inline`/`#Inline(Always)` on a method — same attribute,
    // indented to the method's own line (see `emit_tir_toplevel` for the free-
    // function form).
    let inline_attr = if tir.is_inline_always {
        format!("{pad}#[inline(always)]\n")
    } else if tir.is_inline {
        format!("{pad}#[inline]\n")
    } else {
        String::new()
    };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    for line in &tir.reactive_upgrades {
        out.push_str(&format!("{pad}/* jet-reactive-upgrade: {line} */\n"));
    }
    out.push_str(&format!(
        "{inline_attr}{pad}pub {unsafe_kw}fn {name}{view_generic}({params}){ret}{where_clause} {{\n",
        name = mangle(&tir.name),
        view_generic = if has_view_return { "<'__jet_view>" } else { "" },
        params = params.join(", "),
        ret = ret_clause,
        where_clause = tir.generics,
    ));
    // D-COV1: probe at the method head.
    if cx.coverage {
        out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
    }
    if tir.is_reactive {
        emit_reactive_wrapped_body(&tir.body, cx, out, indent + 1);
    } else {
        emit_tir_stmts(&tir.body, cx, out, indent + 1);
    }
    out.push_str(&format!("{pad}}}\n"));
}

/// c109 Phase 12: a trait-impl method, emitted INSIDE an `impl Trait for user_<T> { … }`
/// block (the caller `emit_trait_impl`/`emit_external_trait_impl` opened it).
/// Byte-identical to `emit_trait_method` (Source/Codegen/Items.rs): a BARE method name
/// (no `user_` mangle — the trait owns it), NO `pub`, an always-`&self` receiver, and
/// an `unsafe ` prefix iff the source was an `#Unsafe fn`.
pub(crate) fn emit_tir_trait_method(
    tir: &TFunc,
    is_unsafe: bool,
    self_conv: AccessConvention,
    serde: Option<SerdeCodec>,
    cx: &Cx,
    out: &mut String,
) {
    // D-SERDE2 (card #131 S1-bridge): a hand `impl T.Encode`/`impl T.Decode` method is
    // bridged to the Rust `user_Encode`/`user_Decode` trait's method name + signature.
    // The user wrote the verbs `encode`/`decode` with Jet-facing signatures; the trait
    // declares `jet_encode(&self) -> jet_std::DataTree` /
    // `jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>`.
    if let Some(codec) = serde {
        emit_tir_serde_method(tir, codec, cx, out);
        return;
    }
    let indent = 1;
    let pad = "    ".repeat(indent);
    let view_provenance = tir.return_view_provenance.as_ref();
    let has_view_return = view_provenance.is_some_and(|map| !map.is_empty());
    let borrows_receiver = view_provenance.is_some_and(|map| {
        map.values().any(|provenance| {
            provenance
                .sources
                .iter()
                .any(|source| matches!(source.source, ViewSource::Receiver))
        })
    });
    let ret_clause = match &tir.ret {
        // `emit_trait_method` computes `ret = rust_return_type(...)` then, if non-empty,
        // ` -> ret`. A unit return yields the empty clause.
        Some(t) => {
            let ret = if has_view_return {
                cx.rust_type_with_view_lifetime(t)
            } else {
                rust_return_type(cx, t)
            };
            let ret = if tir.gc_return {
                format!("jet_gc::AutomaticRoot<{ret}>")
            } else {
                ret
            };
            if ret.is_empty() {
                String::new()
            } else {
                format!(" -> {}", ret)
            }
        }
        None => String::new(),
    };
    // D-MUTSELF1: the receiver honors the source convention — `&self` / `&mut self` /
    // `self` — matching `emit_trait_method` and the trait declaration (emit_trait_def).
    let self_recv = match self_conv {
        AccessConvention::Read if borrows_receiver => {
            "&'__jet_view self"
        }
        AccessConvention::Write if borrows_receiver => {
            "&'__jet_view mut self"
        }
        AccessConvention::Read => "&self",
        AccessConvention::Write => "&mut self",
        AccessConvention::Move => "self",
    };
    let mut params: Vec<String> = vec![self_recv.to_string()];
    for (index, (rust_name, ty, conv)) in tir.params.iter().enumerate() {
        let rust = rust_param_type(cx, *conv, ty);
        let rust = if view_provenance.is_some_and(|map| map.values().any(|provenance| {
            provenance.sources.iter().any(
                |source| matches!(source.source, ViewSource::Parameter(owner) if owner == index),
            )
        })) {
            add_hidden_view_lifetime(rust)
        } else {
            rust
        };
        params.push(format!("{rust_name}: {rust}"));
    }
    let unsafe_kw = if is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(&format!(
        "{pad}{unsafe_kw}fn {name}{view_generic}({params}){ret} {{\n",
        name = tir.name,
        view_generic = if has_view_return { "<'__jet_view>" } else { "" },
        params = params.join(", "),
        ret = ret_clause,
    ));
    // D-COV1: probe at the trait-method head.
    if cx.coverage {
        out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
    }
    emit_tir_stmts(&tir.body, cx, out, indent + 1);
    out.push_str(&format!("{pad}}}\n"));
}

/// D-SERDE2 (card #131 S1-bridge): emit a hand `impl T.Encode`/`impl T.Decode` method,
/// bridged to the Rust `user_Encode`/`user_Decode` trait signature. Body is lowered
/// through the same TIR as any trait method; only the header (name + receiver/params +
/// return) is the trait's, not the user's Jet-facing spelling.
///
/// - `Encode`: `fn jet_encode(&self) -> jet_std::DataTree { <body> }`. The user wrote
///   `fn encode(self) => Data`; bare `self` already lowers to `&self` and `Data` to
///   `jet_std::DataTree`, so only the method NAME is bridged.
/// - `Decode`: `fn jet_decode(<tree>: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>`.
///   The user wrote a STATIC `fn decode(tree: Data) => T ? [FieldError]`; the by-value
///   `Data` param becomes a borrow with an owned clone re-bound at the head (`let <tree> =
///   <tree>.clone();`), so the body reads an owned `Data` local exactly as written.
pub(crate) fn emit_tir_serde_method(tir: &TFunc, codec: SerdeCodec, cx: &Cx, out: &mut String) {
    let indent = 1;
    let pad = "    ".repeat(indent);
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = tir.name.clone();
    match codec {
        SerdeCodec::Encode => {
            out.push_str(&format!(
                "{pad}fn jet_encode(&self) -> jet_std::DataTree {{\n"
            ));
            if cx.coverage {
                out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
            }
            emit_tir_stmts(&tir.body, cx, out, indent + 1);
            out.push_str(&format!("{pad}}}\n"));
        }
        SerdeCodec::Decode => {
            // The single non-self param is the `tree: Data` argument. Render it as a
            // borrow and re-bind an owned clone so the lowered body (which reads the bare
            // name) sees an owned `Data`.
            let tree = tir
                .params
                .first()
                .map(|(n, _, _)| n.clone())
                .unwrap_or_else(|| "tree".to_string());
            let ret = match &tir.ret {
                Some(t) => rust_return_type(cx, t),
                None => "Result<Self, Vec<jet_std::FieldError>>".to_string(),
            };
            out.push_str(&format!(
                "{pad}fn jet_decode({tree}: &jet_std::DataTree) -> {ret} {{\n"
            ));
            if cx.coverage {
                out.push_str(&format!("{pad}    jet_cov({});\n", tir.line));
            }
            out.push_str(&format!("{pad}    let {tree} = ({tree}).clone();\n"));
            emit_tir_stmts(&tir.body, cx, out, indent + 1);
            out.push_str(&format!("{pad}}}\n"));
        }
    }
}

/// c109 Phase 15: a DELEGATION trait method (`using field`), emitted INSIDE the
/// `impl Trait for user_<T> { … }` block `emit_external_trait_impl` opened. Byte-for-byte
/// `emit_delegation_method` (Source/Codegen/Items.rs): the pre-rendered signature line,
/// then the single forwarding call (`(self).<field>.<method>(args)`) at 8-space indent —
/// with a trailing `;` for a unit method, none for a returning one — then `    }`.
pub(crate) fn emit_tir_delegation(
    tir: &TFunc,
    sig: &str,
    fwd: &str,
    has_return: bool,
    cx: &Cx,
    out: &mut String,
) {
    // E2-M12 D-OBS1: track the current function name (parity with the AST path, though a
    // delegation body has no panic site of its own).
    *cx.current_fn.borrow_mut() = tir.name.clone();
    out.push_str(sig);
    if has_return {
        out.push_str(&format!("        {}\n", fwd));
    } else {
        out.push_str(&format!("        {};\n", fwd));
    }
    out.push_str("    }\n");
}
