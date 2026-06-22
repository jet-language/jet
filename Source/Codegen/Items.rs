use super::*;
use crate::AST::{
    AccessConvention, ConstAttr, DistinctDef, EnumDef, Expr, Field,
    Func, ImplDef, RustConstKind, StructDef, TraitImplBlock, Type, VariantPayload,
};
use crate::Generics;
use crate::Syntax;
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
            emit_delegation_method(cx, m, field, out);
        }
    } else {
        for m in &i.methods {
            emit_trait_method(cx, &i.type_name, m, out, 1);
        }
    }
    out.push_str("}\n\n");
}

fn emit_delegation_method(cx: &Cx, f: &Func, field: &str, out: &mut String) {
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    // Emit signature.
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| {
            if p.name == Syntax::KW_SELF {
                "&self".to_string()
            } else {
                format!("{}: {}", mangle(&p.name), rust_param_type(cx, p.convention, &p.ty))
            }
        })
        .collect();
    out.push_str(&format!(
        "    fn {}({}){}  {{\n",
        f.name,
        params.join(", "),
        if ret_clause.is_empty() { String::new() } else { format!(" {}", ret_clause.trim()) }
    ));
    // Emit forwarding call: self.<field>.<method>(args...) using trait method name (no mangle).
    // The target trait method has the same convention, so forward args as-is.
    let fwd_args: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| mangle(&p.name).to_string())
        .collect();
    let field_rust = mangle(field);
    let fwd = format!("(self).{}.{}({})", field_rust, f.name, fwd_args.join(", "));
    if f.return_type.is_some() {
        out.push_str(&format!("        {}\n", fwd));
    } else {
        out.push_str(&format!("        {};\n", fwd));
    }
    out.push_str("    }\n");
}

fn emit_trait_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    let params = f
        .params
        .iter()
        .map(|p| {
            if p.name == Syntax::KW_SELF {
                "&self".to_string()
            } else {
                // No underscore prefix — the body references the same mangled name.
                format!(
                    "{}: {}",
                    cx.mangle_name(&p.name),
                    rust_param_type(cx, p.convention, &p.ty)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    // S58 (E2-M13, D-LL1): an `@unsafe fn` lowers to a Rust `unsafe fn` so its
    // body may use gated pointer ops directly; calling it is already gated to an
    // `@unsafe` block in sema (E3103). Codegen stays dumb.
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    out.push_str(&format!(
        "{pad}{unsafe_kw}fn {}({}){ret_clause} {{\n",
        f.name,
        params,
        ret_clause = ret_clause
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            env.insert(
                p.name.clone(),
                Slot {
                    rust_name: "self".to_string(),
                    deref: false,
                    jet_ty: Some(Type::Named(type_name.to_string())),
                },
            );
            continue;
        }
        let deref = match p.convention {
            AccessConvention::Read => !p.ty.is_scalar(),
            AccessConvention::Mutate => true,
            AccessConvention::Move => false,
        };
        env.insert(
            p.name.clone(),
            Slot {
                rust_name: cx.mangle_name(&p.name),
                deref,
                jet_ty: Some(p.ty.clone()),
            },
        );
    }
    emit_stmts(cx, &f.body, &mut env, out, indent + 1, f.is_view_return);
    out.push_str(&format!("{pad}}}\n"));
}

fn emit_method(cx: &Cx, type_name: &str, f: &Func, out: &mut String, indent: usize) {
    let pad = "    ".repeat(indent);
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    let params = f
        .params
        .iter()
        .map(|p| {
            if p.name == Syntax::KW_SELF {
                match p.convention {
                    AccessConvention::Read => "&self".to_string(),
                    AccessConvention::Mutate => "&mut self".to_string(),
                    AccessConvention::Move => "self".to_string(),
                }
            } else {
                format!(
                    "{}: {}",
                    mangle(&p.name),
                    rust_param_type(cx, p.convention, &p.ty)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    out.push_str(&format!(
        "{}pub {}fn {}({}){} {{\n",
        pad,
        unsafe_kw,
        mangle(&f.name),
        params,
        ret_clause
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
        if p.name == Syntax::KW_SELF {
            env.insert(
                Syntax::KW_SELF.to_string(),
                Slot {
                    rust_name: "self".to_string(),
                    deref: false,
                    jet_ty: None,
                },
            );
            continue;
        }
        let deref = match p.convention {
            AccessConvention::Read => !p.ty.is_scalar(),
            AccessConvention::Mutate => true,
            AccessConvention::Move => false,
        };
        env.insert(
            p.name.clone(),
            Slot {
                rust_name: mangle(&p.name),
                deref,
                jet_ty: Some(p.ty.clone()),
            },
        );
    }
    emit_stmts(cx, &f.body, &mut env, out, indent + 1, f.is_view_return);
    out.push_str(&format!("{}}}\n", pad));
    let _ = type_name;
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
        Expr::Int(n, _) => (format!("{}i64", n), "i64"),
        Expr::Float(v, _) => (format!("{:?}f64", v), "f64"),
        Expr::Bool(b, _) => (b.to_string(), "bool"),
        _ => ("0i64".to_string(), "i64"),
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
    let extra = if f.type_params.is_empty() {
        HashMap::new()
    } else {
        Generics::rust_extra_clone_bounds(&f.type_params)
    };
    let gen = Generics::rust_type_param_list(&f.type_params, &extra);
    let ret = f
        .return_type
        .as_ref()
        .map(|t| rust_return_type(cx, t, f.is_view_return))
        .unwrap_or_default();
    let ret_clause = if ret.is_empty() {
        String::new()
    } else {
        format!(" -> {}", ret)
    };
    let params = f
        .params
        .iter()
        .map(|p| {
            format!(
                "{}: {}",
                cx.mangle_name(&p.name),
                rust_param_type(cx, p.convention, &p.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let vis = if f.name == "main" { "" } else { "pub " };
    // S58 (E2-M13, D-LL1): an `@unsafe fn` lowers to a Rust `unsafe fn` so its
    // body may use gated ops directly (callers are gated to an `@unsafe` block
    // in sema, E3103). Codegen stays dumb.
    let unsafe_kw = if f.is_unsafe { "unsafe " } else { "" };
    // E2-M12 D-OBS1: track the current function name for rich panic reports.
    *cx.current_fn.borrow_mut() = f.name.clone();
    out.push_str(&format!(
        "{vis}{unsafe_kw}fn {name}{gen}({params}){ret} {{\n",
        name = cx.mangle_name(&f.name),
        gen = gen,
        params = params,
        ret = ret_clause,
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
        let is_type_param = f
            .type_params
            .iter()
            .any(|tp| matches!(&p.ty, Type::Named(n) if n == &tp.name));
        let conv = if is_type_param {
            AccessConvention::Move
        } else {
            p.convention
        };
        let deref = match conv {
            AccessConvention::Read if p.ty.is_scalar() => false,
            AccessConvention::Read => true,
            AccessConvention::Mutate => true,
            AccessConvention::Move => false,
        };
        env.insert(
            p.name.clone(),
            Slot {
                rust_name: cx.mangle_name(&p.name),
                deref,
                jet_ty: Some(p.ty.clone()),
            },
        );
    }
    emit_stmts(cx, &f.body, &mut env, out, 1, f.is_view_return);
    out.push_str("}\n\n");
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
    let mut env: HashMap<String, Slot> = HashMap::new();
    // `self` maps to `user_self` (Move convention, named type).
    env.insert(
        crate::Syntax::KW_SELF.to_string(),
        Slot {
            rust_name: "user_self".to_string(),
            deref: false,
            jet_ty: Some(crate::AST::Type::Named(ec.from_ty.clone())),
        },
    );
    emit_stmts(cx, &ec.body, &mut env, out, 1, false);
    out.push_str("}\n\n");
}
