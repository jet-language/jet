use super::*;
use crate::AST::{
    ConstAttr, DistinctDef, EnumDef, Expr, Field,
    Func, ImplDef, RustConstKind, StructDef, TraitImplBlock, Type, VariantPayload,
};
use crate::Generics;
use std::collections::HashMap;

fn struct_lifetimes(fields: &[Field]) -> Vec<String> {
    let mut labels: Vec<String> = Vec::new();
    for f in fields {
        if !f.is_stored_ref {
            continue;
        }
        let label = f
            .stored_ref_label
            .clone()
            .unwrap_or_else(|| "src".to_string());
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

pub(crate) fn emit_struct(cx: &Cx, s: &StructDef, out: &mut String) {
    let lifetimes = struct_lifetimes(&s.fields);
    let clone_extra = if !s.type_params.is_empty() && cx.cloneable.contains(&s.name) {
        Generics::rust_extra_clone_bounds(&s.type_params)
    } else {
        HashMap::new()
    };
    let gen = if s.type_params.is_empty() {
        String::new()
    } else {
        Generics::rust_type_param_list(&s.type_params, &clone_extra)
    };
    let lt_params = if lifetimes.is_empty() {
        String::new()
    } else {
        format!(
            "<{}>",
            lifetimes
                .iter()
                .map(|l| format!("'{}", l))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let type_params = if gen.is_empty() {
        lt_params.clone()
    } else if lt_params.is_empty() {
        gen
    } else {
        format!(
            "<{}, {}>",
            &gen[1..gen.len() - 1],
            &lt_params[1..lt_params.len() - 1]
        )
    };
    let has_fn_field = s.fields.iter().any(|f| matches!(f.ty, Type::Fn { .. }));
    let mut derives: Vec<&str> = Vec::new();
    if !has_fn_field && s.type_params.is_empty() {
        derives.push("Debug");
    }
    if cx.cloneable.contains(&s.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&s.name) {
        derives.push("PartialEq");
    }
    if cx.partial_ord.contains(&s.name) {
        derives.push("PartialOrd");
    }
    // Visibility is enforced by sema (E0605); Rust-level `pub` everywhere
    // keeps cross-module references compiling (R2: sema is the gatekeeper).
    // D-REPRC1: `#layout(c)` stamps `#[repr(C)]` before any `#[derive(...)]`.
    let repr_c = s.layout == Some(crate::AST::StructLayout::C);
    if derives.is_empty() {
        if repr_c {
            out.push_str(&format!("#[repr(C)]\npub struct user_{}{} {{\n", s.name, type_params));
        } else {
            out.push_str(&format!("pub struct user_{}{} {{\n", s.name, type_params));
        }
    } else if repr_c {
        out.push_str(&format!(
            "#[repr(C)]\n#[derive({})]\npub struct user_{}{} {{\n",
            derives.join(", "),
            s.name,
            type_params
        ));
    } else {
        out.push_str(&format!(
            "#[derive({})]\npub struct user_{}{} {{\n",
            derives.join(", "),
            s.name,
            type_params
        ));
    }
    for f in &s.fields {
        let field_ty = if f.is_stored_ref {
            let label = f
                .stored_ref_label
                .clone()
                .unwrap_or_else(|| "src".to_string());
            format!("&'{} {}", label, cx.rust_type(&f.ty))
        } else {
            cx.struct_field_rust(s, &f.name, &f.ty)
        };
        out.push_str(&format!("    pub {}: {},\n", mangle(&f.name), field_ty));
    }
    out.push_str("}\n\n");
    if !s.type_params.is_empty() {
        let jetshow_extra = Generics::rust_extra_jetshow_bounds(&s.type_params);
        let mut impl_bounds = jetshow_extra.clone();
        for (k, v) in &clone_extra {
            impl_bounds
                .entry(k.clone())
                .or_default()
                .extend(v.iter().cloned());
        }
        let tp_bounds = Generics::rust_type_param_list(&s.type_params, &impl_bounds);
        let tp_plain = Generics::type_param_rust_list(&s.type_params);
        let show_body = if has_fn_field {
            format!("\"{} {{ ... }}\".to_string()", s.name)
        } else {
            let fmt_fields: String = s
                .fields
                .iter()
                .map(|f| format!("{}: {{}}", f.name))
                .collect::<Vec<_>>()
                .join(", ");
            let show_fields: String = s
                .fields
                .iter()
                .map(|f| format!("((self).{}).jet_show()", mangle(&f.name)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("format!(\"{}({})\", {})", s.name, fmt_fields, show_fields)
        };
        out.push_str(&format!(
            "impl{} JetShow for user_{}{} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
            tp_bounds, s.name, tp_plain, show_body
        ));
    } else if lifetimes.is_empty() {
        let show_body = if has_fn_field {
            format!("\"{} {{ ... }}\".to_string()", s.name)
        } else {
            "format!(\"{:?}\", self)".to_string()
        };
        out.push_str(&format!(
            "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{ {} }}\n}}\n\n",
            s.name, show_body
        ));
    } else {
        let lt = lifetimes
            .iter()
            .map(|l| format!("'{}", l))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "impl<{}> JetShow for user_{}<{}> {{\n    fn jet_show(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
            lt, s.name, lt
        ));
    }
}

pub(crate) fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&e.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&e.name) {
        derives.push("PartialEq");
    }
    out.push_str(&format!(
        "#[derive({})]\npub enum user_{} {{\n",
        derives.join(", "),
        e.name
    ));
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {
                out.push_str(&format!("    {},\n", mangle(&v.name)));
            }
            VariantPayload::Single(t, _) => {
                let ty = cx.field_rust_type(&e.name, &v.name, t);
                out.push_str(&format!("    {}({}),\n", mangle(&v.name), ty));
            }
            VariantPayload::Named(fs) => {
                out.push_str(&format!("    {} {{\n", mangle(&v.name)));
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    let ty = cx.field_rust_type(&e.name, &key, &f.ty);
                    out.push_str(&format!("        {}: {},\n", mangle(&f.name), ty));
                }
                out.push_str("    },\n");
            }
        }
    }
    out.push_str("}\n\n");
    out.push_str(&format!(
        "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
        e.name
    ));
}

pub(crate) fn emit_type_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::AST::TypeParam],
    methods: &[Func],
    out: &mut String,
) {
    if methods.is_empty() {
        return;
    }
    let tp = Generics::type_param_rust_list(type_params);
    out.push_str(&format!("impl{} user_{}{} {{\n", tp, type_name, tp));
    for m in methods {
        emit_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
}

pub(crate) fn emit_trait_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::AST::TypeParam],
    block: &TraitImplBlock,
    out: &mut String,
) {
    let tp = Generics::type_param_rust_list(type_params);
    out.push_str(&format!(
        "impl{} {} for user_{}{} {{\n",
        tp,
        Generics::user_trait_rust(&block.trait_name),
        type_name,
        tp
    ));
    for m in &block.methods {
        emit_trait_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
}

pub(crate) fn emit_external_trait_impl(cx: &Cx, i: &ImplDef, out: &mut String) {
    let trait_name = i.trait_name.as_deref().unwrap_or("");
    out.push_str(&format!(
        "impl {} for user_{} {{\n",
        Generics::user_trait_rust(trait_name),
        i.type_name
    ));
    if let Some(field) = &i.delegation_field {
        // S62: delegation — emit forwarding methods directly to avoid the method
        // call mangling that the standard path applies (trait methods are not
        // prefixed with `user_` in Rust).
        for m in &i.methods {
            // c109 Phase 15: route a covered delegation method through the typed IR.
            // Byte-identical Rust (golden parity); the whole method is structural so
            // every fact is resolved at lowering (I3).
            if TIR::tir_covers_delegation_method(m, field, cx) {
                let tir = TIR::lower_delegation_method(m, field, cx);
                TIR::emit_tir_func(&tir, cx, out);
                continue;
            }
            // c109 Phase N: the TIR is the only codegen seam (R7). A gate-miss here is
            // a construct the typed IR does not cover — an internal compiler error
            // (I2-class, exit 101), never an AST fallback.
            panic!(
                "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
                m.name
            );
        }
    } else {
        for m in &i.methods {
            emit_trait_method(cx, &i.type_name, m, out, 1);
        }
    }
    out.push_str("}\n\n");
}

fn emit_trait_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    // c109 Phase N: the typed IR is the only codegen seam (R7). A trait-impl
    // method always emits at indent 1 inside the `impl Trait for user_<T>` block
    // the caller opened; it lowers + emits through the TIR. A gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    debug_assert_eq!(indent, 1, "trait methods always emit at impl-block indent 1");
    if TIR::tir_covers_trait_method(f, type_name, cx) {
        let tir = TIR::lower_trait_method(f, type_name, cx);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

fn emit_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    // c109 Phase N: the typed IR is the only codegen seam (R7). An inherent
    // method always emits at indent 1 inside the `impl` block the caller opened;
    // it lowers + emits through the TIR. A gate-miss is an internal compiler
    // error (I2-class), never an AST fallback.
    debug_assert_eq!(indent, 1, "inherent methods always emit at impl-block indent 1");
    if TIR::tir_covers_method(f, type_name, cx) {
        let tir = TIR::lower_method(f, type_name, cx);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

/// D-DIST1/D-DIST3 (ratified 2026-06-19/20): emit a `#[repr(transparent)]`
/// newtype for a distinct type declaration. The inner field is `pub` so
/// codegen can access it for `.raw()` (lowers to `.0`).
pub(crate) fn emit_distinct(cx: &Cx, d: &DistinctDef, out: &mut String) {
    let base_rust = cx.rust_type(&d.base);
    // All distinct types are Debug + Clone + PartialEq (always comparable with
    // their own kind) + Copy when the base is Copy.
    let base_is_copy = matches!(d.base, Type::Int | Type::Float | Type::Bool | Type::Char);
    let mut derives = vec!["Debug", "Clone", "PartialEq"];
    if base_is_copy {
        derives.push("Copy");
    }
    if d.is_numeric {
        // PartialOrd needed for ordered comparisons; also useful for #Numeric types.
        derives.push("PartialOrd");
    }
    out.push_str(&format!(
        "#[repr(transparent)]\n#[derive({})]\npub struct user_{}(pub {});\n\n",
        derives.join(", "),
        d.name,
        base_rust
    ));
    // JetShow: display the base value wrapped in the type name.
    out.push_str(&format!(
        "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{\n        format!(\"{}({{}})\", (self.0).jet_show())\n    }}\n}}\n\n",
        d.name, d.name
    ));
    // .raw() method: unwrap to the base type.
    out.push_str(&format!(
        "impl user_{} {{\n    pub fn raw(&self) -> {} {{ self.0 }}\n}}\n\n",
        d.name, base_rust
    ));
    // #Numeric: implement Add, Sub, Mul, Div (same-type arithmetic).
    if d.is_numeric {
        for (trait_name, op) in &[("Add", "+"), ("Sub", "-"), ("Mul", "*"), ("Div", "/")] {
            out.push_str(&format!(
                "impl std::ops::{}<user_{n}> for user_{n} {{\n    type Output = user_{n};\n    fn {lc}(self, rhs: user_{n}) -> user_{n} {{ user_{n}(self.0 {op} rhs.0) }}\n}}\n\n",
                trait_name,
                n = d.name,
                lc = trait_name.to_lowercase(),
                op = op
            ));
        }
    }
}

pub(crate) fn emit_const(c: &crate::AST::ConstDef, out: &mut String) {
    // S57 (M9.5): comptime values are inlined at use sites (registered into
    // `cx.consts`), so there is no top-level item to emit.
    if c.is_comptime {
        return;
    }
    let (val, ty) = match &c.value {
        // D-SG9: a const declared at a fixed width keeps that width.
        Expr::Int(n, _, Some((signed, bits))) => {
            let rust = format!("{}{}", if *signed { 'i' } else { 'u' }, bits);
            (format!("{n}{rust}"), rust)
        }
        Expr::Int(n, _, None) => (format!("{}i64", n), "i64".to_string()),
        Expr::Float(v, _) => (format!("{:?}f64", v), "f64".to_string()),
        Expr::Bool(b, _) => (b.to_string(), "bool".to_string()),
        _ => ("0i64".to_string(), "i64".to_string()),
    };
    let inline = if c.attrs.contains(&ConstAttr::ForceInline) {
        "#[inline]\n"
    } else {
        ""
    };
    let kw = match c.rust_kind {
        RustConstKind::Const => "const",
        RustConstKind::Static => "static",
    };
    out.push_str(&format!(
        "{}{} {}: {} = {};\n\n",
        inline,
        kw,
        mangle(&c.name).to_uppercase(),
        ty,
        val
    ));
}

pub(crate) fn emit_func(cx: &Cx, f: &Func, out: &mut String) {
    // c109 Phase N: the typed IR (TIR) is the only codegen seam (R7). Every
    // reachable function lowers + emits through the TIR; a gate-miss is an
    // internal compiler error (I2-class), never an AST fallback.
    if TIR::tir_covers(f, cx) {
        let tir = TIR::lower_func(f, cx);
        TIR::emit_tir_func(&tir, cx, out);
        return;
    }
    panic!(
        "internal compiler error: codegen reached a construct the typed IR does not cover ({}) — compiler bug (I2/R7)",
        f.name
    );
}

/// D-ERR-CONV: emit a standalone Rust function for `impl Source -> Target { body }`.
/// The function is called by the `map_err` closure emitted in `Expression.rs`
/// when a `TryConvert::Typed` node is encountered.
pub(crate) fn emit_error_conv(cx: &Cx, ec: &crate::AST::ErrorConvDef, out: &mut String) {
    let fn_name = crate::Sema::error_conv_fn_name(&ec.from_ty, &ec.to_ty);
    let from_rust = cx.rust_type(&crate::AST::Type::Named(ec.from_ty.clone()));
    let to_rust = cx.rust_type(&crate::AST::Type::Named(ec.to_ty.clone()));
    *cx.current_fn.borrow_mut() = fn_name.clone();
    out.push_str(&format!(
        "pub fn {fn_name}(user_self: {from}) -> {to} {{\n",
        fn_name = fn_name,
        from = from_rust,
        to = to_rust,
    ));
    // c109: the TIR is the only codegen seam (R7). The body lowers + emits through the
    // TIR; a gate-miss is an internal compiler error (I2-class), never an AST fallback.
    if TIR::tir_covers_error_conv_body(&ec.body, cx) {
        TIR::emit_tir_error_conv_body(&ec.body, &ec.from_ty, cx, out);
        out.push_str("}\n\n");
        return;
    }
    panic!(
        "internal compiler error: codegen reached an error-conversion body construct the typed IR does not cover ({} -> {}) — compiler bug (I2/R7)",
        ec.from_ty, ec.to_ty
    );
}
