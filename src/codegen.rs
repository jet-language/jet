//! Codegen is deliberately dumb (invariant I3): no checking happens here.
//! If a Program reaches this module, emission must always succeed and the
//! resulting Rust must always compile.
//!
//! Type-alignment rules (so emitted Rust always typechecks):
//!   - scalar params (Int/Float/Bool) pass by value, String by `&String`;
//!     `mut` params are `&mut T`; `take` params are `T` by value
//!   - a name bound to a `&T`/`&mut T` parameter is always emitted as the
//!     place `(*user_x)`, so every name has its plain Jet type
//!   - every printed/interpolated value goes through the `JetShow` trait
//!     in the prelude (Float keeps its decimal part there, S21)
//!   - every operator result is fully parenthesized

use crate::ast::{
    AccessConvention, BinOp, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr, Field, ForKind, Func,
    IfStmt, IndexKind, Item, LValue, OrFallback, Pattern, Program, RustConstKind, Stmt, StrPart,
    StructDef, Type, UnOp, VariantPayload,
};
use crate::diag::{span_line_col, Span};
use crate::syntax;
use std::collections::{HashMap, HashSet};

/// Emitted at the top of every program: one tiny display trait so codegen
/// never needs to know a value's type to print it. Monomorphized by rustc;
/// zero runtime dispatch. Float printing keeps the decimal part (S21).
const PRELUDE: &str = r#"trait JetShow { fn jet_show(&self) -> String; }
impl JetShow for i64 { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for f64 { fn jet_show(&self) -> String { format!("{:?}", self) } }
impl JetShow for bool { fn jet_show(&self) -> String { self.to_string() } }
impl JetShow for String { fn jet_show(&self) -> String { self.clone() } }
impl<T: JetShow> JetShow for &T { fn jet_show(&self) -> String { (**self).jet_show() } }
impl JetShow for char { fn jet_show(&self) -> String { self.to_string() } }
fn jet_panic(file: &str, line: u32, msg: &str) -> ! {
    eprintln!("The program stopped: {}", msg);
    eprintln!("  --> {}:{}", file, line);
    std::process::exit(70);
}
fn jet_index_vec<T: Clone>(xs: &Vec<T>, i: i64, file: &str, line: u32) -> T {
    let len = xs.len() as i64;
    if i < 0 || i >= len {
        jet_panic(file, line, &format!("the list has {} items, so position {} doesn't exist", len, i));
    }
    xs[i as usize].clone()
}
fn jet_slice_vec<T: Clone>(xs: &Vec<T>, a: i64, b: i64, file: &str, line: u32) -> Vec<T> {
    let len = xs.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} items from {} to {} (inclusive)", len, a, b));
    }
    xs[a as usize..=b as usize].to_vec()
}
fn jet_index_map<K: Ord + Clone, V: Clone>(m: &std::collections::BTreeMap<K, V>, k: &K, file: &str, line: u32) -> V {
    match m.get(k) {
        Some(v) => v.clone(),
        None => jet_panic(file, line, &format!("the map has no entry for this key")),
    }
}
fn jet_map_insert<K: Ord, V>(m: &mut std::collections::BTreeMap<K, V>, k: K, v: V) {
    m.insert(k, v);
}
fn jet_char_len(s: &String) -> i64 { s.chars().count() as i64 }
fn jet_string_split(s: &String, sep: &str) -> Vec<String> { s.split(sep).map(|x| x.to_string()).collect() }
fn jet_string_slice(s: &String, a: i64, b: i64, file: &str, line: u32) -> String {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;
    if a < 0 || b < 0 || a > b || b >= len {
        jet_panic(file, line, &format!("can't slice {} characters from {} to {} (inclusive)", len, a, b));
    }
    chars[a as usize..=b as usize].iter().collect()
}
"#;

fn mangle(name: &str) -> String {
    if name == "main" {
        "main".to_string()
    } else {
        format!("user_{}", name)
    }
}

/// Shared codegen context built once per program.
struct Cx {
    /// Top-level function name -> parameter conventions+types.
    sigs: HashMap<String, Vec<(AccessConvention, Type)>>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    consts: HashMap<String, String>,
    type_names: HashSet<String>,
    struct_fields: HashMap<String, Vec<(String, Type)>>,
    enum_variants: HashMap<String, Vec<(String, VariantPayload)>>,
    /// variant name -> owning enum type (for pattern lowering)
    variant_owner: HashMap<String, String>,
    /// Recursive-type edges that need `Box<…>` in Rust (`(owner, edge_key)`).
    boxed_edges: HashSet<(String, String)>,
    cloneable: HashSet<String>,
    comparable: HashSet<String>,
    src: String,
    file: String,
}

impl Cx {
    fn field_rust_type(&self, owner: &str, edge: &str, ty: &Type) -> String {
        let base = self.rust_type(ty);
        if self.boxed_edges.contains(&(owner.to_string(), edge.to_string())) {
            format!("Box<{}>", base)
        } else {
            base
        }
    }

    fn rust_type(&self, ty: &Type) -> String {
        match ty {
            Type::Int => "i64".to_string(),
            Type::Float => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "char".to_string(),
            Type::List(inner) => format!("Vec<{}>", self.rust_type(inner)),
            Type::Map { key, value } => format!(
                "std::collections::BTreeMap<{}, {}>",
                self.rust_type(key),
                self.rust_type(value)
            ),
            Type::Shared(inner) => format!("std::sync::Arc<{}>", self.rust_type(inner)),
            Type::Option(inner) => format!("Option<{}>", self.rust_type(inner)),
            Type::Result { ok, err } => format!(
                "Result<{}, {}>",
                self.rust_type(ok),
                self.rust_type(err)
            ),
            Type::Named(name) => format!("user_{}", name),
        }
    }
}

fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    let base = cx.rust_type(ty);
    match convention {
        AccessConvention::Read if ty.is_scalar() => base,
        AccessConvention::Read => format!("&{}", base),
        AccessConvention::Mutate => format!("&mut {}", base),
        AccessConvention::Move => base,
    }
}

fn rust_return_type(cx: &Cx, ty: &Type, is_view: bool) -> String {
    let base = cx.rust_type(ty);
    if is_view {
        format!("&{}", base)
    } else {
        base
    }
}

/// What a Jet name looks like in Rust expression position.
#[derive(Clone)]
struct Slot {
    rust_name: String,
    /// The Rust binding is a reference; emit `(*name)` to get the value.
    deref: bool,
    jet_ty: Option<Type>,
}

pub fn emit(prog: &Program, src: &str, file: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} — do not edit. Edit the .{} source instead.\n",
        syntax::BINARY_NAME,
        syntax::FILE_EXT
    ));
    out.push_str(&format!(
        "// If rustc rejects this file, that is a bug in {} (invariant I2).\n",
        syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    out.push_str(PRELUDE);
    out.push('\n');

    let mut cx = build_cx(prog, src, file);

    for item in &prog.items {
        match item {
            Item::Struct(s) => emit_struct(&cx, s, &mut out),
            Item::Enum(e) => emit_enum(&cx, e, &mut out),
            Item::Const(c) => emit_const(c, &mut out),
            Item::Func(_) | Item::Impl(_) => {}
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => emit_type_impl(&cx, &s.name, &s.methods, &mut out),
            Item::Enum(e) => emit_type_impl(&cx, &e.name, &e.methods, &mut out),
            Item::Impl(i) => emit_type_impl(&cx, &i.type_name, &i.methods, &mut out),
            _ => {}
        }
    }

    for item in &prog.items {
        if let Item::Func(f) = item {
            emit_func(&cx, f, &mut out);
        }
    }
    out
}

fn build_cx(prog: &Program, src: &str, file: &str) -> Cx {
    let mut cx = Cx {
        sigs: HashMap::new(),
        method_sigs: HashMap::new(),
        consts: HashMap::new(),
        type_names: HashSet::new(),
        struct_fields: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        comparable: HashSet::new(),
        src: src.to_string(),
        file: file.to_string(),
    };

    for item in &prog.items {
        match item {
            Item::Func(f) => {
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| (p.convention, p.ty.clone()))
                        .collect(),
                );
            }
            Item::Struct(s) => {
                cx.type_names.insert(s.name.clone());
                cx.struct_fields.insert(
                    s.name.clone(),
                    s.fields
                        .iter()
                        .filter(|f| !f.is_stored_ref)
                        .map(|f| (f.name.clone(), f.ty.clone()))
                        .collect(),
                );
            }
            Item::Enum(e) => {
                cx.type_names.insert(e.name.clone());
                cx.enum_variants.insert(
                    e.name.clone(),
                    e.variants
                        .iter()
                        .map(|v| (v.name.clone(), v.payload.clone()))
                        .collect(),
                );
                for v in &e.variants {
                    cx.variant_owner
                        .insert(v.name.clone(), e.name.clone());
                }
            }
            Item::Const(c) => {
                cx.consts
                    .insert(c.name.clone(), mangle(&c.name).to_uppercase());
            }
            Item::Impl(_) => {}
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                cx.boxed_edges.extend(find_struct_box_edges(s, &cx));
                if type_is_cloneable_struct(s, &cx.type_names) {
                    cx.cloneable.insert(s.name.clone());
                }
                if type_is_comparable_struct(s, &cx.type_names) {
                    cx.comparable.insert(s.name.clone());
                }
                for m in &s.methods {
                    cx.method_sigs.insert(
                        (s.name.clone(), m.name.clone()),
                        m.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            Item::Enum(e) => {
                cx.boxed_edges.extend(find_enum_box_edges(e, &cx));
                if type_is_cloneable_enum(e, &cx.type_names) {
                    cx.cloneable.insert(e.name.clone());
                }
                if type_is_comparable_enum(e, &cx.type_names) {
                    cx.comparable.insert(e.name.clone());
                }
                for m in &e.methods {
                    cx.method_sigs.insert(
                        (e.name.clone(), m.name.clone()),
                        m.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    cx.method_sigs.insert(
                        (i.type_name.clone(), m.name.clone()),
                        m.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            _ => {}
        }
    }

    cx
}

fn type_is_cloneable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    s.fields
        .iter()
        .all(|f| !f.is_stored_ref && field_type_cloneable(&f.ty, types))
}

fn type_is_cloneable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_cloneable(t, types),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_type_cloneable(&f.ty, types)),
    })
}

fn field_type_cloneable(ty: &Type, types: &HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            field_type_cloneable(inner, types)
        }
        Type::Map { key, value } => {
            field_type_cloneable(key, types) && field_type_cloneable(value, types)
        }
        Type::Result { ok, err } => {
            field_type_cloneable(ok, types) && field_type_cloneable(err, types)
        }
        Type::Named(n) => types.contains(n),
    }
}

fn type_is_comparable_struct(s: &StructDef, types: &HashSet<String>) -> bool {
    s.fields
        .iter()
        .all(|f| !f.is_stored_ref && field_type_comparable(&f.ty, types))
}

fn type_is_comparable_enum(e: &EnumDef, types: &HashSet<String>) -> bool {
    e.variants.iter().all(|v| match &v.payload {
        VariantPayload::Unit => true,
        VariantPayload::Single(t, _) => field_type_comparable(t, types),
        VariantPayload::Named(fs) => fs.iter().all(|f| field_type_comparable(&f.ty, types)),
    })
}

fn field_type_comparable(ty: &Type, types: &HashSet<String>) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::Option(inner) => field_type_comparable(inner, types),
        Type::Result { ok, err } => {
            field_type_comparable(ok, types) && field_type_comparable(err, types)
        }
        Type::List(inner) => field_type_comparable(inner, types),
        Type::Named(n) => types.contains(n),
        Type::Map { .. } | Type::Shared(_) => false,
    }
}

fn find_struct_box_edges(s: &StructDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for f in &s.fields {
        if f.is_stored_ref {
            continue;
        }
        walk_type_edge(
            &s.name,
            &f.name,
            &f.ty,
            &mut vec![s.name.clone()],
            cx,
            &mut boxed,
        );
    }
    boxed
}

fn find_enum_box_edges(e: &EnumDef, cx: &Cx) -> HashSet<(String, String)> {
    let mut boxed = HashSet::new();
    for v in &e.variants {
        match &v.payload {
            VariantPayload::Unit => {}
            VariantPayload::Single(t, _) => walk_type_edge(
                &e.name,
                &v.name,
                t,
                &mut vec![e.name.clone()],
                cx,
                &mut boxed,
            ),
            VariantPayload::Named(fs) => {
                for f in fs {
                    let key = format!("{}.{}", v.name, f.name);
                    walk_type_edge(
                        &e.name,
                        &key,
                        &f.ty,
                        &mut vec![e.name.clone()],
                        cx,
                        &mut boxed,
                    );
                }
            }
        }
    }
    boxed
}

fn walk_type_edge(
    owner: &str,
    edge: &str,
    ty: &Type,
    stack: &mut Vec<String>,
    cx: &Cx,
    boxed: &mut HashSet<(String, String)>,
) {
    match ty {
        Type::Named(n) if cx.type_names.contains(n) => {
            if stack.iter().any(|s| s == n) {
                boxed.insert((owner.to_string(), edge.to_string()));
                return;
            }
            stack.push(n.clone());
            if let Some(fields) = cx.struct_fields.get(n) {
                for (fname, fty) in fields {
                    walk_type_edge(n, fname, fty, stack, cx, boxed);
                }
            }
            if let Some(vars) = cx.enum_variants.get(n) {
                for (vname, payload) in vars {
                    match payload {
                        VariantPayload::Unit => {}
                        VariantPayload::Single(t, _) => {
                            walk_type_edge(n, vname, t, stack, cx, boxed);
                        }
                        VariantPayload::Named(fs) => {
                            for f in fs {
                                let key = format!("{}.{}", vname, f.name);
                                walk_type_edge(n, &key, &f.ty, stack, cx, boxed);
                            }
                        }
                    }
                }
            }
            stack.pop();
        }
        Type::Option(inner) | Type::List(inner) | Type::Shared(inner) => {
            walk_type_edge(owner, edge, inner, stack, cx, boxed);
        }
        Type::Map { key, value } => {
            walk_type_edge(owner, edge, key, stack, cx, boxed);
            walk_type_edge(owner, edge, value, stack, cx, boxed);
        }
        Type::Char => {}
        _ => {}
    }
}

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

fn emit_struct(cx: &Cx, s: &StructDef, out: &mut String) {
    let lifetimes = struct_lifetimes(&s.fields);
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
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&s.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&s.name) {
        derives.push("PartialEq");
    }
    out.push_str(&format!(
        "#[derive({})]\nstruct user_{}{} {{\n",
        derives.join(", "),
        s.name,
        lt_params
    ));
    for f in &s.fields {
        let field_ty = if f.is_stored_ref {
            let label = f
                .stored_ref_label
                .clone()
                .unwrap_or_else(|| "src".to_string());
            format!("&'{} {}", label, cx.rust_type(&f.ty))
        } else {
            cx.field_rust_type(&s.name, &f.name, &f.ty)
        };
        out.push_str(&format!("    {}: {},\n", mangle(&f.name), field_ty));
    }
    out.push_str("}\n\n");
    if lifetimes.is_empty() {
        out.push_str(&format!(
            "impl JetShow for user_{} {{\n    fn jet_show(&self) -> String {{ format!(\"{{:?}}\", self) }}\n}}\n\n",
            s.name
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

fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&e.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&e.name) {
        derives.push("PartialEq");
    }
    out.push_str(&format!("#[derive({})]\nenum user_{} {{\n", derives.join(", "), e.name));
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

fn emit_type_impl(cx: &Cx, type_name: &str, methods: &[Func], out: &mut String) {
    if methods.is_empty() {
        return;
    }
    out.push_str(&format!("impl user_{} {{\n", type_name));
    for m in methods {
        emit_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
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
            if p.name == syntax::KW_SELF {
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
    out.push_str(&format!(
        "{}{}fn {}({}){} {{\n",
        pad,
        if f.is_pub { "pub " } else { "" },
        mangle(&f.name),
        params,
        ret_clause
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
        if p.name == syntax::KW_SELF {
            env.insert(
                syntax::KW_SELF.to_string(),
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

fn emit_const(c: &crate::ast::ConstDef, out: &mut String) {
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

fn emit_func(cx: &Cx, f: &Func, out: &mut String) {
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
                mangle(&p.name),
                rust_param_type(cx, p.convention, &p.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "fn {}({}){} {{\n",
        mangle(&f.name),
        params,
        ret_clause
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
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
    emit_stmts(cx, &f.body, &mut env, out, 1, f.is_view_return);
    out.push_str("}\n\n");
}

fn emit_stmts(
    cx: &Cx,
    stmts: &[Stmt],
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    for stmt in stmts {
        emit_stmt(cx, stmt, env, out, indent, view_return);
    }
}

fn emit_stmt(
    cx: &Cx,
    stmt: &Stmt,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    match stmt {
        Stmt::Val(b) => {
            let init = emit_expr(cx, &b.init, env);
            let kw = if b.mutable { "let mut" } else { "let" };
            let ty = b
                .ty
                .as_ref()
                .map(|t| format!(": {}", cx.rust_type(t)))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}{} {}{} = {};\n",
                pad,
                kw,
                mangle(&b.name),
                ty,
                init
            ));
            env.insert(
                b.name.clone(),
                Slot {
                    rust_name: mangle(&b.name),
                    deref: false,
                    jet_ty: b.ty.clone(),
                },
            );
        }
        Stmt::Assign { target, op, value, .. } => {
            let v = emit_expr(cx, value, env);
            match target {
                LValue::Local { name, .. } => {
                    let place = place_of(env, name);
                    match op {
                        Some(op) => out.push_str(&format!(
                            "{}{} {}= {};\n",
                            pad,
                            place,
                            op.spell(),
                            v
                        )),
                        None => out.push_str(&format!("{}{} = {};\n", pad, place, v)),
                    }
                }
                LValue::Index { base, index, .. } => {
                    let is_map = matches!(expr_jet_ty(base, env), Some(Type::Map { .. }));
                    let b = emit_expr(cx, base, env);
                    let i = emit_expr(cx, index, env);
                    if is_map {
                        out.push_str(&format!(
                            "{pad}{{ let __jet_v = {v}; jet_map_insert(&mut ({b}), ({i}).clone(), __jet_v); }}\n",
                        ));
                    } else {
                        out.push_str(&format!(
                            "{pad}{{ let __jet_v = {v}; ({b})[{i} as usize] = __jet_v; }}\n",
                        ));
                    }
                }
            }
        }
        Stmt::Expr(e) => {
            out.push_str(&format!("{}{};\n", pad, emit_expr_stmt(cx, e, env)));
        }
        Stmt::Return(Some(e), _) => {
            let val = if view_return {
                emit_view_return(cx, e, env)
            } else {
                emit_expr(cx, e, env)
            };
            out.push_str(&format!("{}return {};\n", pad, val));
        }
        Stmt::Return(None, _) => {
            out.push_str(&format!("{}return;\n", pad));
        }
        Stmt::If(ifs) => emit_if(cx, ifs, env, out, indent, view_return),
        Stmt::While { cond, body, .. } => {
            out.push_str(&format!("{}while {} {{\n", pad, emit_expr(cx, cond, env)));
            emit_stmts(cx, body, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            ..
        } => match kind {
            ForKind::Range { start, end } => {
                let s = emit_expr(cx, start, env);
                let e = emit_expr(cx, end, env);
                out.push_str(&format!(
                    "{}for {} in ({})..=({}) {{\n",
                    pad,
                    mangle(var),
                    s,
                    e
                ));
                let prev = env.insert(
                    var.clone(),
                    Slot {
                        rust_name: mangle(var),
                        deref: false,
                        jet_ty: Some(Type::Int),
                    },
                );
                emit_stmts(cx, body, env, out, indent + 1, view_return);
                match prev {
                    Some(p) => {
                        env.insert(var.clone(), p);
                    }
                    None => {
                        env.remove(var);
                    }
                }
                out.push_str(&format!("{}}}\n", pad));
            }
            ForKind::In { collection } => {
                emit_for_in(
                    cx,
                    var,
                    var2.as_ref(),
                    collection,
                    body,
                    env,
                    out,
                    indent,
                    view_return,
                );
            }
        },
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            if is_exhaustive_pattern_switch(cx, subject, arms) {
                emit_pattern_match_switch(cx, subject, arms, else_body, env, out, indent);
            } else {
                emit_mixed_switch(cx, subject, arms, else_body, env, out, indent, view_return);
            }
        }
        Stmt::Break(_) => out.push_str(&format!("{}break;\n", pad)),
        Stmt::Continue(_) => out.push_str(&format!("{}continue;\n", pad)),
        Stmt::Loop(inner, _) => {
            out.push_str(&format!("{}loop {{\n", pad));
            emit_stmts(cx, inner, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Stmt::Unsafe(inner, _) => {
            out.push_str(&format!("{}unsafe {{\n", pad));
            emit_stmts(cx, inner, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
    }
}

fn switch_arm_pattern_owned(cx: &Cx, cond: &Expr, subject: &Expr) -> Option<Pattern> {
    match cond {
        Expr::PatternTest {
            subject: s,
            pattern,
            ..
        } if pattern_subjects_match(s, subject) => Some(pattern.clone()),
        Expr::Binary(crate::ast::BinOp::Eq, lhs, rhs, span)
            if pattern_subjects_match(lhs, subject) =>
        {
            if let Expr::Ident(variant, rhs_span) = rhs.as_ref() {
                if cx.variant_owner.contains_key(variant) {
                    return Some(Pattern::Variant {
                        variant: variant.clone(),
                        bindings: Vec::new(),
                        span: *rhs_span,
                    });
                }
            }
            None
        }
        _ => None,
    }
}

fn is_exhaustive_pattern_switch(cx: &Cx, subject: &Expr, arms: &[crate::ast::SwitchArm]) -> bool {
    !arms.is_empty()
        && arms
            .iter()
            .all(|a| switch_arm_pattern_owned(cx, &a.cond, subject).is_some())
}

fn pattern_subjects_match(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(na, _), Expr::Ident(nb, _)) => na == nb,
        (Expr::Ident(n, _), _) if n == syntax::KW_IT => true,
        _ => false,
    }
}

fn emit_pattern_match_switch(
    cx: &Cx,
    subject: &Expr,
    arms: &[crate::ast::SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
) {
    let pad = "    ".repeat(indent);
    let subj = emit_expr(cx, subject, env);
    let enum_type = arms.iter().find_map(|a| {
        switch_arm_pattern_owned(cx, &a.cond, subject).and_then(|pattern| {
            if let Pattern::Variant { variant, .. } = pattern {
                cx.variant_owner.get(&variant).cloned()
            } else {
                None
            }
        })
    });
    out.push_str(&format!("{}match {} {{\n", pad, subj));
    for arm in arms {
        if let Some(pattern) = switch_arm_pattern_owned(cx, &arm.cond, subject) {
            let pat = emit_match_pattern(cx, &pattern, enum_type.as_deref());
            out.push_str(&format!("{}    {} => {{\n", pad, pat));
            emit_stmts(cx, &arm.body, env, out, indent + 2, false);
            out.push_str(&format!("{}    }}\n", pad));
        }
    }
    if let Some(body) = else_body {
        out.push_str(&format!("{}    _ => {{\n", pad));
        emit_stmts(cx, body, env, out, indent + 2, false);
        out.push_str(&format!("{}    }}\n", pad));
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_mixed_switch(
    cx: &Cx,
    subject: &Expr,
    arms: &[crate::ast::SwitchArm],
    else_body: &Option<Vec<Stmt>>,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    out.push_str(&format!("{}{{\n", pad));
    let inner_pad = "    ".repeat(indent + 1);
    out.push_str(&format!(
        "{}let _jet_switch_subject = &({});\n",
        inner_pad,
        emit_expr(cx, subject, env)
    ));
    for (i, arm) in arms.iter().enumerate() {
        let kw = if i == 0 { "if" } else { "} else if" };
        out.push_str(&format!(
            "{}{} {} {{\n",
            inner_pad,
            kw,
            emit_switch_arm_cond(cx, &arm.cond, env)
        ));
        emit_stmts(cx, &arm.body, env, out, indent + 2, view_return);
    }
    match else_body {
        None if !arms.is_empty() => {
            out.push_str(&format!("{}}}\n", inner_pad));
        }
        None => {}
        Some(body) if arms.is_empty() => {
            emit_stmts(cx, body, env, out, indent + 1, view_return);
        }
        Some(body) => {
            out.push_str(&format!("{}}} else {{\n", inner_pad));
            emit_stmts(cx, body, env, out, indent + 2, view_return);
            out.push_str(&format!("{}}}\n", inner_pad));
        }
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_switch_arm_cond(cx: &Cx, cond: &Expr, env: &HashMap<String, Slot>) -> String {
    let subject = match cond {
        Expr::PatternTest { subject, .. } => subject.as_ref(),
        Expr::Binary(crate::ast::BinOp::Eq, lhs, _, _) => lhs.as_ref(),
        _ => return emit_expr(cx, cond, env),
    };
    if let Some(pattern) = switch_arm_pattern_owned(cx, cond, subject) {
        let subj = emit_expr(cx, subject, env);
        return emit_pattern_matches(cx, &subj, &pattern);
    }
    emit_expr(cx, cond, env)
}

fn enum_type_prefix(cx: &Cx, variant: &str) -> String {
    cx.variant_owner
        .get(variant)
        .map(|t| format!("user_{}", t))
        .unwrap_or_else(|| "user_TYPE".to_string())
}

fn emit_pattern_matches(cx: &Cx, subject: &str, pattern: &Pattern) -> String {
    match pattern {
        Pattern::Variant {
            variant,
            bindings,
            ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            if bindings.is_empty() {
                format!("matches!({}, {}::{})", subject, prefix, mangle(variant))
            } else if bindings.len() == 1 {
                format!(
                    "matches!({}, {}::{}({}))",
                    subject,
                    prefix,
                    mangle(variant),
                    mangle(&bindings[0])
                )
            } else {
                let b = bindings
                    .iter()
                    .map(|n| mangle(n))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "matches!({}, {}::{} {{ {} }})",
                    subject,
                    prefix,
                    mangle(variant),
                    bindings
                        .iter()
                        .map(|n| format!("{}: {}", mangle(n), mangle(n)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
        Pattern::Present { binding, .. } => {
            format!("matches!({}, Some({}))", subject, mangle(binding))
        }
        Pattern::Absent(_) => format!("({}).is_none()", subject),
        Pattern::Ok { binding, .. } => {
            format!("matches!({}, Ok({}))", subject, mangle(binding))
        }
        Pattern::Err { binding, .. } => {
            format!("matches!({}, Err({}))", subject, mangle(binding))
        }
    }
}

fn emit_match_pattern(_cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    let prefix = enum_type
        .map(|t| format!("user_{}", t))
        .unwrap_or_else(|| "user_TYPE".to_string());
    match pattern {
        Pattern::Variant {
            variant,
            bindings,
            ..
        } => {
            if bindings.is_empty() {
                format!("{}::{}", prefix, mangle(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    mangle(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}::{} {{ {} }}", prefix, mangle(variant), fields)
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
    }
}

fn emit_or_fallback(
    cx: &Cx,
    value: &Expr,
    fallback: &OrFallback,
    is_option: bool,
    env: &HashMap<String, Slot>,
) -> String {
    if is_option {
        return emit_or_fallback_option(cx, value, fallback, env);
    }
    let v = emit_expr(cx, value, env);
    let fallback_expr = emit_or_fallback_rhs(cx, fallback, env);
    format!(
        "match {} {{ Ok(__jet_ok) => __jet_ok, Err(_) => {} }}",
        v, fallback_expr
    )
}

fn emit_or_fallback_option(
    cx: &Cx,
    value: &Expr,
    fallback: &OrFallback,
    env: &HashMap<String, Slot>,
) -> String {
    let v = emit_expr(cx, value, env);
    let fallback_expr = emit_or_fallback_rhs(cx, fallback, env);
    format!(
        "match {} {{ Some(__jet_v) => __jet_v, None => {} }}",
        v, fallback_expr
    )
}

fn emit_or_fallback_rhs(
    cx: &Cx,
    fallback: &OrFallback,
    env: &HashMap<String, Slot>,
) -> String {
    match fallback {
        OrFallback::Value(e) => emit_expr(cx, e, env),
        OrFallback::Return(None, _) => "return".to_string(),
        OrFallback::Return(Some(e), _) => format!("return {}", emit_expr(cx, e, env)),
        OrFallback::Panic { name_span, args } => {
            let call = crate::ast::Call {
                name: syntax::BUILTIN_PANIC.to_string(),
                name_span: *name_span,
                args: args.clone(),
            };
            emit_panic_stop(cx, &call, env)
        }
    }
}

fn emit_panic_stop(cx: &Cx, call: &crate::ast::Call, env: &HashMap<String, Slot>) -> String {
    let msg = emit_panic_message(cx, &call.args[0].expr, env);
    let (line, _) = span_line_col(&cx.src, call.name_span.start);
    format!(
        "{{ jet_panic({}, {}, &{}); }}",
        escape_rust_str(&cx.file),
        line,
        msg
    )
}

fn emit_panic_message(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        Expr::Str(parts, _) => emit_str(cx, parts, env),
        other => format!("({}).jet_show()", emit_expr(cx, other, env)),
    }
}

fn emit_require(cx: &Cx, call: &crate::ast::Call, env: &HashMap<String, Slot>) -> String {
    let cond = emit_expr(cx, &call.args[0].expr, env);
    let (line, _) = span_line_col(&cx.src, call.name_span.start);
    let msg = if call.args.len() == 2 {
        emit_panic_message(cx, &call.args[1].expr, env)
    } else {
        format!("\"condition failed\".to_string()")
    };
    format!(
        "{{ if !({}) {{ jet_panic({}, {}, &{}); }} }}",
        cond,
        escape_rust_str(&cx.file),
        line,
        msg
    )
}

fn escape_rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// `-> view T` returns a reference; emit `&place` or the existing borrow.
fn emit_view_return(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        Expr::Ident(name, _) => {
            if let Some(c) = cx.consts.get(name) {
                return format!("&{}", c);
            }
            if let Some(slot) = env.get(name) {
                if slot.deref {
                    return slot.rust_name.clone();
                }
                return format!("&{}", slot.rust_name);
            }
            place_of(env, name)
        }
        _ => emit_expr(cx, e, env),
    }
}

fn emit_if(
    cx: &Cx,
    ifs: &IfStmt,
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    if let Some((pat, subj)) = if_pattern_test(&ifs.cond) {
        let subj_expr = emit_expr(cx, subj, env);
        let pat_str = emit_if_let_pattern(cx, pat);
        out.push_str(&format!("{}if let {} = {} {{\n", pad, pat_str, subj_expr));
        let mut body_env = env.clone();
        add_pattern_bindings(pat, &mut body_env);
        emit_stmts(cx, &ifs.then_body, &mut body_env, out, indent + 1, view_return);
    } else if let Expr::PatternTest {
        subject,
        pattern: Pattern::Absent(_),
        ..
    } = &ifs.cond
    {
        let subj = emit_expr(cx, subject, env);
        out.push_str(&format!("{}if {}.is_none() {{\n", pad, subj));
        emit_stmts(cx, &ifs.then_body, env, out, indent + 1, view_return);
    } else {
        out.push_str(&format!("{}if {} {{\n", pad, emit_expr(cx, &ifs.cond, env)));
        emit_stmts(cx, &ifs.then_body, env, out, indent + 1, view_return);
    }
    match &ifs.else_branch {
        None => out.push_str(&format!("{}}}\n", pad)),
        Some(ElseBranch::Else(body)) => {
            out.push_str(&format!("{}}} else {{\n", pad));
            emit_stmts(cx, body, env, out, indent + 1, view_return);
            out.push_str(&format!("{}}}\n", pad));
        }
        Some(ElseBranch::ElseIf(next)) => {
            out.push_str(&format!("{}}} else ", pad));
            let mut nested = String::new();
            emit_if(cx, next, env, &mut nested, indent, view_return);
            let trimmed = nested.trim_start_matches(&pad).to_string();
            out.push_str(&trimmed);
        }
    }
}

fn if_pattern_test(cond: &Expr) -> Option<(&Pattern, &Expr)> {
    match cond {
        Expr::PatternTest { subject, pattern, .. } => match pattern {
            Pattern::Absent(_) => None,
            _ => Some((pattern, subject.as_ref())),
        },
        Expr::Binary(BinOp::And, l, r, _) => {
            if let Expr::PatternTest {
                subject,
                pattern,
                ..
            } = l.as_ref()
            {
                if matches!(pattern, Pattern::Absent(_)) {
                    return None;
                }
                if let Expr::PatternTest { .. } = r.as_ref() {
                    return None;
                }
                return Some((pattern, subject.as_ref()));
            }
            None
        }
        _ => None,
    }
}

fn add_pattern_bindings(pattern: &Pattern, env: &mut HashMap<String, Slot>) {
    match pattern {
        Pattern::Present { binding, .. } => {
            env.insert(
                binding.clone(),
                Slot {
                    rust_name: mangle(binding),
                    deref: false,
                    jet_ty: None,
                },
            );
        }
        Pattern::Variant { bindings, .. } => {
            for b in bindings {
                env.insert(
                    b.clone(),
                    Slot {
                        rust_name: mangle(b),
                        deref: false,
                        jet_ty: None,
                    },
                );
            }
        }
        Pattern::Absent(_) => {}
        Pattern::Ok { binding, .. } | Pattern::Err { binding, .. } => {
            env.insert(
                binding.clone(),
                Slot {
                    rust_name: mangle(binding),
                    deref: false,
                    jet_ty: None,
                },
            );
        }
    }
}

fn emit_for_in(
    cx: &Cx,
    var: &str,
    var2: Option<&(String, Span)>,
    collection: &Expr,
    body: &[Stmt],
    env: &mut HashMap<String, Slot>,
    out: &mut String,
    indent: usize,
    view_return: bool,
) {
    let pad = "    ".repeat(indent);
    let coll = emit_expr(cx, collection, env);
    if let Some((v2, _)) = var2 {
        out.push_str(&format!(
            "{}for (_jet_k, _jet_v) in ({coll}).iter() {{\n",
            pad
        ));
        out.push_str(&format!(
            "{}    let {} = _jet_k.clone();\n",
            pad,
            mangle(var)
        ));
        out.push_str(&format!(
            "{}    let {} = _jet_v.clone();\n",
            pad,
            mangle(v2)
        ));
    } else if let Expr::MethodCall { receiver, method, .. } = collection {
        if method == "chars" {
            let recv = emit_expr(cx, receiver, env);
            out.push_str(&format!(
                "{}for _jet_c in ({recv}).chars() {{\n    {}let {} = _jet_c;\n",
                pad,
                pad,
                mangle(var)
            ));
        } else {
            out.push_str(&format!(
                "{}for _jet_item in ({coll}).iter().cloned() {{\n    {}let {} = _jet_item;\n",
                pad,
                pad,
                mangle(var)
            ));
        }
    } else {
        out.push_str(&format!(
            "{}for _jet_item in ({coll}).iter().cloned() {{\n    {}let {} = _jet_item;\n",
            pad,
            pad,
            mangle(var)
        ));
    }
    env.insert(
        var.to_string(),
        Slot {
            rust_name: mangle(var),
            deref: false,
            jet_ty: None,
        },
    );
    if let Some((v2, _)) = var2 {
        env.insert(
            v2.clone(),
            Slot {
                rust_name: mangle(v2),
                deref: false,
                jet_ty: None,
            },
        );
    }
    emit_stmts(cx, body, env, out, indent + 1, view_return);
    env.remove(var);
    if let Some((v2, _)) = var2 {
        env.remove(v2);
    }
    out.push_str(&format!("{}}}\n", pad));
}

fn emit_if_let_pattern(cx: &Cx, pattern: &Pattern) -> String {
    match pattern {
        Pattern::Variant {
            variant,
            bindings,
            ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            if bindings.is_empty() {
                format!("{}::{}", prefix, mangle(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    mangle(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}::{} {{ {} }}", prefix, mangle(variant), fields)
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
    }
}

fn place_of(env: &HashMap<String, Slot>, name: &str) -> String {
    match env.get(name) {
        Some(slot) if slot.deref => format!("(*{})", slot.rust_name),
        Some(slot) => slot.rust_name.clone(),
        None => mangle(name),
    }
}

fn emit_expr_stmt(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    emit_expr(cx, e, env)
}

fn emit_expr(cx: &Cx, e: &Expr, env: &HashMap<String, Slot>) -> String {
    match e {
        Expr::Int(n, _) => format!("{}i64", n),
        Expr::Float(v, _) => format!("{:?}f64", v),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Char(ch, _) => format!("{:?}", ch),
        Expr::ListLit(elems, _) => {
            let parts = elems
                .iter()
                .map(|e| emit_expr(cx, e, env))
                .collect::<Vec<_>>()
                .join(", ");
            format!("vec![{}]", parts)
        }
        Expr::MapLit(entries, _) => {
            if entries.is_empty() {
                "std::collections::BTreeMap::new()".to_string()
            } else {
                let mut s = String::from("{ let mut _m = std::collections::BTreeMap::new(); ");
                for (k, v) in entries {
                    s.push_str(&format!(
                        "_m.insert(({}).clone(), {}); ",
                        emit_expr(cx, k, env),
                        emit_expr(cx, v, env)
                    ));
                }
                s.push_str("_m }");
                s
            }
        }
        Expr::Index {
            base,
            index,
            span,
            kind,
        } => {
            let (line, _) = span_line_col(&cx.src, span.start);
            let b = emit_expr(cx, base, env);
            let i = emit_expr(cx, index, env);
            match kind {
                IndexKind::Map => format!(
                    "jet_index_map(&({}), &({}), {:?}, {})",
                    b, i, cx.file, line
                ),
                _ => format!(
                    "jet_index_vec(&({}), {}, {:?}, {})",
                    b, i, cx.file, line
                ),
            }
        }
        Expr::Slice {
            base,
            start,
            end,
            span,
        } => {
            let (line, _) = span_line_col(&cx.src, span.start);
            let b = emit_expr(cx, base, env);
            let a = emit_expr(cx, start, env);
            let e = emit_expr(cx, end, env);
            format!(
                "jet_slice_vec(&({}), {}, {}, {:?}, {})",
                b, a, e, cx.file, line
            )
        }
        Expr::Str(parts, _) => emit_str(cx, parts, env),
        Expr::Ident(name, _) => {
            if let Some(c) = cx.consts.get(name) {
                return c.clone();
            }
            place_of(env, name)
        }
        Expr::Unary(op, inner, _) => {
            let i = emit_expr(cx, inner, env);
            match op {
                UnOp::Neg => format!("(-({}))", i),
                UnOp::Not => format!("(!({}))", i),
            }
        }
        Expr::Binary(op, l, r, _) => {
            let ls = emit_expr(cx, l, env);
            let rs = emit_expr(cx, r, env);
            format!("(({}) {} ({}))", ls, op.spell(), rs)
        }
        Expr::Call(call) => emit_call(cx, call, env),
        Expr::Deref(inner, _) => format!("*{}", emit_expr(cx, inner, env)),
        Expr::Field(inner, member, _) => {
            if member == "clone" {
                format!("({}).clone()", emit_expr(cx, inner, env))
            } else if let Expr::Ident(type_name, _) = &**inner {
                if cx.enum_variants.contains_key(type_name) {
                    format!("user_{}::{}", type_name, mangle(member))
                } else {
                    format!(
                        "({}).{}",
                        emit_expr(cx, inner, env),
                        mangle(member)
                    )
                }
            } else {
                format!(
                    "({}).{}",
                    emit_expr(cx, inner, env),
                    mangle(member)
                )
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => emit_method_call(cx, receiver, method, args, env),
        Expr::StructLit {
            type_name,
            fields,
            ..
        } => {
            let parts = fields
                .iter()
                .map(|(n, _, e)| format!("{}: {}", mangle(n), emit_expr(cx, e, env)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("user_{} {{ {} }}", type_name, parts)
        }
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => emit_enum_lit(cx, type_name, variant, args, env),
        Expr::Present(inner, _) => format!("Some({})", emit_expr(cx, inner, env)),
        Expr::Absent(_) => "None".to_string(),
        Expr::Ok(inner, _) => format!("Ok({})", emit_expr(cx, inner, env)),
        Expr::Err(inner, _) => format!("Err({})", emit_expr(cx, inner, env)),
        Expr::Try(inner, _) => format!("{}?", emit_expr(cx, inner, env)),
        Expr::OrFallback {
            value,
            fallback,
            is_option,
            ..
        } => emit_or_fallback(cx, value, fallback, *is_option, env),
        Expr::PatternTest { subject, pattern, .. } => {
            let subj = emit_expr(cx, subject, env);
            emit_pattern_matches(cx, &subj, pattern)
        }
    }
}

fn emit_boxed_enum_arg(
    cx: &Cx,
    type_name: &str,
    edge: &str,
    expr: &Expr,
    env: &HashMap<String, Slot>,
) -> String {
    let payload_ty = enum_variant_payload_type(cx, type_name, edge);
    let mut s = emit_expr(cx, expr, env);
    if payload_ty.is_some_and(|t| !t.is_scalar()) && expr_borrowed_in_env(expr, env) {
        s = format!("({}).clone()", s);
    }
    if cx
        .boxed_edges
        .contains(&(type_name.to_string(), edge.to_string()))
    {
        format!("Box::new({})", s)
    } else {
        s
    }
}

fn enum_variant_payload_type<'a>(cx: &'a Cx, type_name: &str, variant: &str) -> Option<&'a Type> {
    let variants = cx.enum_variants.get(type_name)?;
    let (_, payload) = variants.iter().find(|(v, _)| v == variant)?;
    match payload {
        VariantPayload::Single(t, _) => Some(t),
        VariantPayload::Named(fs) if fs.len() == 1 => Some(&fs[0].ty),
        _ => None,
    }
}

fn expr_borrowed_in_env(expr: &Expr, env: &HashMap<String, Slot>) -> bool {
    match expr {
        Expr::Ident(name, _) => env.get(name).is_some_and(|s| s.deref),
        _ => false,
    }
}

fn emit_enum_lit(
    cx: &Cx,
    type_name: &str,
    variant: &str,
    args: &[EnumLitArg],
    env: &HashMap<String, Slot>,
) -> String {
    let prefix = format!("user_{}::{}", type_name, mangle(variant));
    if args.is_empty() {
        return prefix;
    }
    if args.iter().all(|a| matches!(a, EnumLitArg::Positional(_))) {
        let pos = args
            .iter()
            .map(|a| match a {
                EnumLitArg::Positional(e) => {
                    emit_boxed_enum_arg(cx, type_name, variant, e, env)
                }
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{}({})", prefix, pos);
    }
    let fields = args
        .iter()
        .map(|a| match a {
            EnumLitArg::Named { label, expr } => {
                let edge = format!("{}.{}", variant, label);
                format!(
                    "{}: {}",
                    mangle(label),
                    emit_boxed_enum_arg(cx, type_name, &edge, expr, env)
                )
            }
            EnumLitArg::Positional(e) => emit_expr(cx, e, env),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{} {{ {} }}", prefix, fields)
}

fn expr_jet_ty(expr: &Expr, env: &HashMap<String, Slot>) -> Option<Type> {
    match expr {
        Expr::Ident(name, _) => env.get(name).and_then(|s| s.jet_ty.clone()),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::MethodCall { receiver, method, .. } => {
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            if method == "split" {
                return Some(Type::List(Box::new(Type::String)));
            }
            expr_jet_ty(receiver, env)
        }
        _ => None,
    }
}

fn emit_builtin_method(
    cx: &Cx,
    receiver: &Expr,
    method: &str,
    args: &[crate::ast::CallArg],
    env: &HashMap<String, Slot>,
) -> Option<String> {
    let recv = emit_expr(cx, receiver, env);
    let arg = |i: usize| {
        args.get(i)
            .map(|a| emit_expr(cx, &a.expr, env))
            .unwrap_or_default()
    };
    if let Expr::Ident(name, _) = receiver {
        match (name.as_str(), method) {
            (syntax::TYPE_INT, "parse") => {
                return Some(format!("({}).trim().parse::<i64>()", arg(0)));
            }
            (syntax::TYPE_FLOAT, "parse") => {
                return Some(format!("({}).trim().parse::<f64>()", arg(0)));
            }
            _ => {}
        }
    }
    let rty = expr_jet_ty(receiver, env);
    match method {
        "len" => Some(match rty {
            Some(Type::String) => format!("jet_char_len(&({}))", recv),
            _ => format!("({}).len() as i64", recv),
        }),
        "is_empty" => Some(format!("({}).is_empty()", recv)),
        "push" => Some(format!("({}).push({})", recv, arg(0))),
        "pop" => Some(format!("({}).pop()", recv)),
        "insert" => Some(match rty {
            Some(Type::Map { .. }) => format!("({}).insert(({}).clone(), {})", recv, arg(0), arg(1)),
            _ => format!("({}).insert({} as usize, {})", recv, arg(0), arg(1)),
        }),
        "remove" => Some(match rty {
            Some(Type::Map { .. }) => format!("({}).remove(&({}).clone())", recv, arg(0)),
            _ => format!("({}).remove({} as usize).unwrap()", recv, arg(0)),
        }),
        "get" => Some(match rty {
            Some(Type::Map { .. }) => format!("({}).get(&({}).clone()).cloned()", recv, arg(0)),
            _ => format!("({}).get({} as usize).cloned()", recv, arg(0)),
        }),
        "first" => Some(format!("({}).first().cloned()", recv)),
        "last" => Some(format!("({}).last().cloned()", recv)),
        "contains" => Some(format!("({}).contains(&{})", recv, arg(0))),
        "index_of" => Some(format!(
            "({}).iter().position(|x| *x == {}).map(|i| i as i64)",
            recv,
            arg(0)
        )),
        "reverse" => Some(format!("({}).reverse()", recv)),
        "sort" => Some(format!("({}).sort()", recv)),
        "join" => Some(format!(
            "({}).iter().map(|x| x.jet_show()).collect::<Vec<_>>().join(({}).as_str())",
            recv,
            arg(0)
        )),
        "clear" => Some(format!("({}).clear()", recv)),
        "chars" => Some(format!("({}).chars().collect::<Vec<char>>()", recv)),
        "trim" => Some(format!("({}).trim().to_string()", recv)),
        "split" => Some(format!("jet_string_split(&({}), &{})", recv, arg(0))),
        "starts_with" => Some(format!("({}).starts_with(&{})", recv, arg(0))),
        "ends_with" => Some(format!("({}).ends_with(&{})", recv, arg(0))),
        "replace" => Some(format!("({}).replace(&{}, &{})", recv, arg(0), arg(1))),
        "to_upper" => Some(format!("({}).to_uppercase()", recv)),
        "to_lower" => Some(format!("({}).to_lowercase()", recv)),
        "repeat" => Some(format!("({}).repeat({} as usize)", recv, arg(0))),
        "slice" => {
            let (line, _) = span_line_col(&cx.src, receiver.span().start);
            Some(format!(
                "jet_string_slice(&({}), {}, {}, {:?}, {})",
                recv,
                arg(0),
                arg(1),
                cx.file,
                line
            ))
        }
        "keys" => Some(format!("({}).keys().cloned().collect::<Vec<_>>()", recv)),
        "values" => Some(format!("({}).values().cloned().collect::<Vec<_>>()", recv)),
        "contains_key" => Some(format!("({}).contains_key(&{})", recv, arg(0))),
        "to_string" => Some(format!("({}).jet_show()", recv)),
        "to_float" => Some(format!("({}) as f64", recv)),
        "to_int" => Some(format!("({}) as i64", recv)),
        _ => None,
    }
}

fn emit_method_call(
    cx: &Cx,
    receiver: &Expr,
    method: &str,
    args: &[crate::ast::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    if method == "clone" {
        return format!("({}).clone()", emit_expr(cx, receiver, env));
    }
    if let Some(s) = emit_builtin_method(cx, receiver, method, args, env) {
        return s;
    }
    if let Expr::Ident(type_name, _) = receiver {
        if let Some(variants) = cx.enum_variants.get(type_name) {
            if variants.iter().any(|(v, _)| v == method) {
                let enum_args: Vec<EnumLitArg> = args
                    .iter()
                    .map(|a| EnumLitArg::Positional(a.expr.clone()))
                    .collect();
                return emit_enum_lit(cx, type_name, method, &enum_args, env);
            }
        }
        if cx.type_names.contains(type_name) {
            let arg_str = emit_call_args(cx, None, args, env);
            return format!(
                "user_{}::{}({})",
                type_name,
                mangle(method),
                arg_str
            );
        }
    }
    let recv = emit_expr(cx, receiver, env);
    let sig = receiver_type_name(receiver, cx)
        .and_then(|t| cx.method_sigs.get(&(t, method.to_string())));
    let arg_str = emit_call_args(cx, sig.map(|s| s.as_slice()), args, env);
    format!("({}).{}({})", recv, mangle(method), arg_str)
}

fn receiver_type_name(receiver: &Expr, cx: &Cx) -> Option<String> {
    match receiver {
        Expr::Ident(n, _) if cx.type_names.contains(n) => Some(n.clone()),
        _ => None,
    }
}

fn emit_call_args(
    cx: &Cx,
    sig: Option<&[(AccessConvention, Type)]>,
    args: &[crate::ast::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let mut s = emit_expr(cx, &a.expr, env);
            if a.flags.implicit_clone {
                s = format!("({}).clone()", s);
            } else if a.flags.shared_auto_clone {
                s = format!("std::sync::Arc::clone(&{})", s);
            }
            let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
            match conv {
                Some((AccessConvention::Read, t)) if !t.is_scalar() => format!("&({})", s),
                Some((AccessConvention::Mutate, _)) => format!("&mut ({})", s),
                _ => s,
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn emit_str(cx: &Cx, parts: &[StrPart], env: &HashMap<String, Slot>) -> String {
    if parts.len() == 1 {
        if let StrPart::Lit(s) = &parts[0] {
            return format!("{:?}.to_string()", s);
        }
    }
    let mut body = String::from("{ let mut _jet_s = String::new(); ");
    for p in parts {
        match p {
            StrPart::Lit(s) => {
                if !s.is_empty() {
                    body.push_str(&format!("_jet_s.push_str({:?}); ", s));
                }
            }
            StrPart::Interp(e) => {
                body.push_str(&format!(
                    "_jet_s.push_str(&({}).jet_show()); ",
                    emit_expr(cx, e, env)
                ));
            }
        }
    }
    body.push_str("_jet_s }");
    body
}

fn emit_call(cx: &Cx, call: &crate::ast::Call, env: &HashMap<String, Slot>) -> String {
    if call.name == syntax::BUILTIN_PRINT {
        let arg = emit_expr(cx, &call.args[0].expr, env);
        return format!("println!(\"{{}}\", ({}).jet_show())", arg);
    }
    if call.name == syntax::BUILTIN_PANIC {
        return emit_panic_stop(cx, call, env);
    }
    if call.name == syntax::BUILTIN_REQUIRE {
        return emit_require(cx, call, env);
    }
    let sig = cx.sigs.get(&call.name);
    let args = emit_call_args(cx, sig.map(|s| s.as_slice()), &call.args, env);
    format!("{}({})", mangle(&call.name), args)
}
