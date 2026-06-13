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
    AccessConvention, BinOp, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr, Field, ForKind,
    Func, IfStmt, ImplDef, IndexKind, Item, Lambda, LambdaBody, LValue, OrFallback, Pattern,
    Program, ProgramBundle, RustConstKind, Stmt, StrPart, StructDef, TestDef,
    TraitImplBlock, Type, UnOp, VariantPayload,
};
use crate::ffi::FfiLink;
use crate::generics;
use crate::loader;
use crate::m9;
use crate::sema::CompileMode;
use crate::diag::{span_line_col, Span};
use crate::syntax;
use std::collections::{HashMap, HashSet};

/// Emitted at the top of every program: core runtime helpers used by generated Rust.
const PRELUDE: &str = include_str!("prelude/core.rs");

/// Extra helpers for `jet test` harnesses only (M6/S43).
const TEST_PRELUDE: &str = "";
const STD_PRELUDE: &str = include_str!("prelude/std.rs");

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
    /// Top-level function name -> function value type (M8).
    fn_types: HashMap<String, Type>,
    /// `(TypeName, method)` -> parameter conventions+types (including `self`).
    method_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    consts: HashMap<String, String>,
    type_names: HashSet<String>,
    trait_names: HashSet<String>,
    struct_fields: HashMap<String, Vec<(String, Type)>>,
    enum_variants: HashMap<String, Vec<(String, VariantPayload)>>,
    /// variant name -> owning enum type (for pattern lowering)
    variant_owner: HashMap<String, String>,
    /// Recursive-type edges that need `Box<…>` in Rust (`(owner, edge_key)`).
    boxed_edges: HashSet<(String, String)>,
    cloneable: HashSet<String>,
    comparable: HashSet<String>,
    /// S55: explicit `derive Comparable;` → PartialOrd in Rust.
    partial_ord: HashSet<String>,
    src: String,
    file: String,
    /// When true, `require`/`require_eq` unwind instead of exiting (test bodies).
    test_mode: bool,
    /// Import alias -> Rust module name (`user_scoring`).
    import_mods: HashMap<String, String>,
    /// `(import alias, function)` -> parameter conventions for cross-module calls.
    import_sigs: HashMap<(String, String), Vec<(AccessConvention, Type)>>,
    /// Import alias -> compiler-known std module (`std.fs`, `std.json`, ...).
    std_imports: HashMap<String, String>,
    /// M10 helpers proven reachable by sema.
    used_std: HashSet<String>,
    /// Empty at the entry module, `super::` inside generated import modules.
    root_prefix: String,
    /// M7: rustc crate name for the FFI bridge (`jet_ffi_…`).
    ffi_crate: Option<String>,
    /// M7: Jet function name -> wrapper symbol in the FFI crate.
    extern_funcs: HashMap<String, String>,
}

const MOD_USE: &str = "use super::{JetShow, jet_panic, jet_index_vec, jet_slice_vec, jet_index_map, jet_map_insert, jet_char_len, jet_string_split, jet_string_slice, jet_list_map, jet_list_map_mut, jet_list_filter, jet_list_each, jet_list_each_ref, jet_list_each_mut, jet_list_find, jet_list_any, jet_list_all, jet_list_sort_by, jet_list_reduce, jet_map_each};\n\n";

impl Cx {
    fn field_rust_type(&self, owner: &str, edge: &str, ty: &Type) -> String {
        let base = self.rust_type(ty);
        if self.boxed_edges.contains(&(owner.to_string(), edge.to_string())) {
            format!("Box<{}>", base)
        } else {
            base
        }
    }

    fn struct_field_rust(&self, s: &StructDef, edge: &str, ty: &Type) -> String {
        let base = match ty {
            Type::Named(n) if s.type_params.iter().any(|p| p.name == *n) => n.clone(),
            _ => self.rust_type(ty),
        };
        if self.boxed_edges.contains(&(s.name.clone(), edge.to_string())) {
            format!("Box<{base}>")
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
            // Items inside an imported file live in `mod user_<alias>`; the
            // module provides the namespace, so item names stay plain.
            Type::Named(name) if generics::is_type_var_name(name) && !self.type_names.contains(name) => {
                name.clone()
            }
            Type::Named(name) if name == "Unit" => "()".to_string(),
            Type::Named(name) if name == "U8" => "u8".to_string(),
            Type::Named(name)
                if matches!(
                    name.as_str(),
                    "IoError" | "Utf8Error" | "ProcessResult" | "Stopwatch" | "Json"
                        | "JsonError"
                ) =>
            {
                format!("{}jet_std::{name}", self.root_prefix)
            }
            Type::Named(name) if self.trait_names.contains(name) => {
                format!("Box<dyn {}>", generics::user_trait_rust(name))
            }
            Type::Named(name) => format!("user_{name}"),
            Type::Apply { name, args } => {
                if args.is_empty() {
                    format!("user_{name}")
                } else {
                    format!(
                        "user_{name}<{args}>",
                        args = args
                            .iter()
                            .map(|a| self.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            }
            Type::TraitObject(t) => format!("Box<dyn {}>", generics::user_trait_rust(t)),
            Type::Fn { params, ret } => self.rust_fn_trait(params, ret.as_deref(), false),
        }
    }

    fn rust_fn_trait(&self, params: &[Type], ret: Option<&Type>, mut_capture: bool) -> String {
        let ps = params
            .iter()
            .map(|p| self.rust_type(p))
            .collect::<Vec<_>>()
            .join(", ");
        let r = ret
            .map(|t| self.rust_type(t))
            .unwrap_or_else(|| "()".to_string());
        let trait_name = if mut_capture { "FnMut" } else { "Fn" };
        format!("Box<dyn {}({}) -> {}>", trait_name, ps, r)
    }

    fn mangle_name(&self, name: &str) -> String {
        mangle(name)
    }

    fn type_prefix(&self, type_name: &str) -> String {
        format!("user_{}", type_name)
    }
}

fn rust_param_type(cx: &Cx, convention: AccessConvention, ty: &Type) -> String {
    let base = cx.rust_type(ty);
    if matches!(ty, Type::Named(n) if cx.trait_names.contains(n))
        || matches!(ty, Type::TraitObject(_))
    {
        return match convention {
            AccessConvention::Read => format!("&{base}"),
            AccessConvention::Mutate => format!("&mut {base}"),
            AccessConvention::Move => base,
        };
    }
    if matches!(ty, Type::Named(n) if generics::is_type_var_name(n)) {
        return base;
    }
    if matches!(ty, Type::Fn { .. }) {
        return base;
    }
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

    let cx = build_cx(prog, src, file);

    for item in &prog.items {
        match item {
            Item::Trait(t) => m9::emit_trait_def(t, &mut out),
            Item::Struct(s) => emit_struct(&cx, s, &mut out),
            Item::Enum(e) => emit_enum(&cx, e, &mut out),
            Item::Const(c) => emit_const(c, &mut out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::ExternRust(_) => {}
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, &mut out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(&cx, i, &mut out);
                } else {
                    emit_type_impl(&cx, &i.type_name, &[], &i.methods, &mut out);
                }
            }
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

/// Emit a test harness binary: all definitions plus one `main` that runs
/// every `test "…" { }` block (M6 phase 2).
pub fn emit_tests(prog: &Program, src: &str, file: &str) -> String {
    let tests: Vec<&TestDef> = prog
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(!tests.is_empty(), "emit_tests called with no test blocks");

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    out.push_str(PRELUDE);
    out.push_str(TEST_PRELUDE);
    out.push('\n');

    let mut cx = build_cx(prog, src, file);
    cx.test_mode = true;

    for item in &prog.items {
        match item {
            Item::Trait(t) => m9::emit_trait_def(t, &mut out),
            Item::Struct(s) => emit_struct(&cx, s, &mut out),
            Item::Enum(e) => emit_enum(&cx, e, &mut out),
            Item::Const(c) => emit_const(c, &mut out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::ExternRust(_) => {}
        }
    }

    for item in &prog.items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(&cx, &s.name, &s.type_params, &s.methods, &mut out);
                for block in &s.trait_impls {
                    emit_trait_impl(&cx, &s.name, &s.type_params, block, &mut out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(&cx, &e.name, &e.type_params, &e.methods, &mut out);
                for block in &e.trait_impls {
                    emit_trait_impl(&cx, &e.name, &e.type_params, block, &mut out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(&cx, i, &mut out);
                } else {
                    emit_type_impl(&cx, &i.type_name, &[], &i.methods, &mut out);
                }
            }
            _ => {}
        }
    }

    for item in &prog.items {
        if let Item::Func(f) = item {
            if f.name != "main" {
                emit_func(&cx, f, &mut out);
            }
        }
    }

    for (i, test) in tests.iter().enumerate() {
        out.push_str(&format!("fn jet_test_{}() -> Result<(), String> {{\n", i));
        let mut env: HashMap<String, Slot> = HashMap::new();
        emit_stmts(&cx, &test.body, &mut env, &mut out, 1, false);
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");
    }

    out.push_str("fn main() {\n");
    out.push_str("    let mut passed = 0usize;\n");
    out.push_str("    let mut failed = 0usize;\n");
    for (i, test) in tests.iter().enumerate() {
        let name = escape_rust_str(&test.name);
        out.push_str(&format!("    match jet_test_{}() {{\n", i));
        out.push_str("        Ok(()) => {\n");
        out.push_str(&format!("            println!(\"{{}}: pass\", {});\n", name));
        out.push_str("            passed += 1;\n");
        out.push_str("        }\n");
        out.push_str("        Err(msg) => {\n");
        out.push_str(&format!("            println!(\"{{}}: FAIL\", {});\n", name));
        out.push_str("            eprintln!(\"  {}\", msg);\n");
        out.push_str("            failed += 1;\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }
    out.push_str("    println!(\"{} passed, {} failed\", passed, failed);\n");
    out.push_str("    if failed > 0 { std::process::exit(1); }\n");
    out.push_str("}\n");
    out
}

fn build_cx(prog: &Program, src: &str, file: &str) -> Cx {
    let extern_funcs = extern_func_map(&prog.items);
    build_cx_items(&prog.items, src, file, None, &extern_funcs)
}

fn extern_func_map(items: &[Item]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in items {
        if let Item::ExternRust(block) = item {
            for ef in &block.functions {
                map.insert(ef.name.clone(), format!("jet_ffi_{}", ef.name));
            }
        }
    }
    map
}

fn bundle_extern_funcs(bundle: &ProgramBundle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        map.extend(extern_func_map(&module.items));
    }
    map
}

fn build_cx_items(
    items: &[Item],
    src: &str,
    file: &str,
    link: Option<&FfiLink>,
    extern_funcs: &HashMap<String, String>,
) -> Cx {
    let mut cx = Cx {
        sigs: HashMap::new(),
        fn_types: HashMap::new(),
        method_sigs: HashMap::new(),
        consts: HashMap::new(),
        type_names: HashSet::new(),
        trait_names: HashSet::new(),
        struct_fields: HashMap::new(),
        enum_variants: HashMap::new(),
        variant_owner: HashMap::new(),
        boxed_edges: HashSet::new(),
        cloneable: HashSet::new(),
        comparable: HashSet::new(),
        partial_ord: HashSet::new(),
        src: src.to_string(),
        file: file.to_string(),
        test_mode: false,
        import_mods: HashMap::new(),
        import_sigs: HashMap::new(),
        std_imports: HashMap::new(),
        used_std: HashSet::new(),
        root_prefix: String::new(),
        ffi_crate: link.map(|l| l.crate_name.clone()),
        extern_funcs: extern_funcs.clone(),
    };

    for item in items {
        match item {
            Item::Func(f) => {
                let type_params: HashSet<String> =
                    f.type_params.iter().map(|p| p.name.clone()).collect();
                cx.sigs.insert(
                    f.name.clone(),
                    f.params
                        .iter()
                        .map(|p| {
                            let conv = if matches!(&p.ty, Type::Named(n) if type_params.contains(n))
                            {
                                AccessConvention::Move
                            } else {
                                p.convention
                            };
                            (conv, p.ty.clone())
                        })
                        .collect(),
                );
                cx.fn_types.insert(
                    f.name.clone(),
                    Type::Fn {
                        params: f.params.iter().map(|p| p.ty.clone()).collect(),
                        ret: f.return_type.clone().map(Box::new),
                    },
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
                if c.is_comptime {
                    // Inline the evaluated literal at every reference.
                    let serialized = c
                        .ct
                        .as_ref()
                        .map(|v| v.serialize())
                        .unwrap_or_else(|| "Default::default()".to_string());
                    cx.consts.insert(c.name.clone(), serialized);
                } else {
                    cx.consts
                        .insert(c.name.clone(), mangle(&c.name).to_uppercase());
                }
            }
            Item::ExternRust(block) => {
                for ef in &block.functions {
                    cx.sigs.insert(
                        ef.name.clone(),
                        ef.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
            Item::Trait(t) => {
                cx.trait_names.insert(t.name.clone());
            }
            Item::Impl(_) | Item::Test(_) => {}
        }
    }

    for item in items {
        match item {
            Item::Struct(s) => {
                cx.boxed_edges.extend(find_struct_box_edges(s, &cx));
                if type_is_cloneable_struct(s, &cx.type_names) {
                    cx.cloneable.insert(s.name.clone());
                }
                if type_is_comparable_struct(s, &cx.type_names) {
                    cx.comparable.insert(s.name.clone());
                }
                for (t, _) in &s.derives {
                    if t == generics::COMPARABLE {
                        cx.partial_ord.insert(s.name.clone());
                        cx.comparable.insert(s.name.clone());
                    }
                }
                for m in &s.methods {
                    cx.method_sigs
                        .insert((s.name.clone(), m.name.clone()), method_sig_params(m));
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
                    cx.method_sigs
                        .insert((e.name.clone(), m.name.clone()), method_sig_params(m));
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    cx.method_sigs
                        .insert((i.type_name.clone(), m.name.clone()), method_sig_params(m));
                }
            }
            _ => {}
        }
    }

    cx
}

/// Parameter conventions for a method, excluding `self` — call-site args
/// align positionally with this list (the receiver is emitted separately).
fn method_sig_params(f: &Func) -> Vec<(AccessConvention, Type)> {
    f.params
        .iter()
        .filter(|p| p.name != syntax::KW_SELF)
        .map(|p| (p.convention, p.ty.clone()))
        .collect()
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
        Type::Named(n) if generics::is_type_var_name(n) => true,
        Type::Named(n) => types.contains(n),
        Type::Apply { args, .. } => args.iter().all(|a| field_type_cloneable(a, types)),
        Type::TraitObject(_) | Type::Fn { .. } => false,
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
        Type::Named(n) if generics::is_type_var_name(n) => true,
        Type::Named(n) => types.contains(n),
        Type::Apply { args, .. } => args.iter().all(|a| field_type_comparable(a, types)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
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
    let clone_extra = if !s.type_params.is_empty() && cx.cloneable.contains(&s.name) {
        generics::rust_extra_clone_bounds(&s.type_params)
    } else {
        HashMap::new()
    };
    let gen = if s.type_params.is_empty() {
        String::new()
    } else {
        generics::rust_type_param_list(&s.type_params, &clone_extra)
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
    if derives.is_empty() {
        out.push_str(&format!("pub struct user_{}{} {{\n", s.name, type_params));
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
        let jetshow_extra = generics::rust_extra_jetshow_bounds(&s.type_params);
        let mut impl_bounds = jetshow_extra.clone();
        for (k, v) in &clone_extra {
            impl_bounds.entry(k.clone()).or_default().extend(v.iter().cloned());
        }
        let tp_bounds = generics::rust_type_param_list(&s.type_params, &impl_bounds);
        let tp_plain = generics::type_param_rust_list(&s.type_params);
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
            format!(
                "format!(\"{}({})\", {})",
                s.name, fmt_fields, show_fields
            )
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

fn emit_enum(cx: &Cx, e: &EnumDef, out: &mut String) {
    let mut derives = vec!["Debug"];
    if cx.cloneable.contains(&e.name) {
        derives.push("Clone");
    }
    if cx.comparable.contains(&e.name) {
        derives.push("PartialEq");
    }
    out.push_str(&format!("#[derive({})]\npub enum user_{} {{\n", derives.join(", "), e.name));
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

fn emit_type_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::ast::TypeParam],
    methods: &[Func],
    out: &mut String,
) {
    if methods.is_empty() {
        return;
    }
    let tp = generics::type_param_rust_list(type_params);
    out.push_str(&format!("impl{} user_{}{} {{\n", tp, type_name, tp));
    for m in methods {
        emit_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
}

fn emit_trait_impl(
    cx: &Cx,
    type_name: &str,
    type_params: &[crate::ast::TypeParam],
    block: &TraitImplBlock,
    out: &mut String,
) {
    let tp = generics::type_param_rust_list(type_params);
    out.push_str(&format!(
        "impl{} {} for user_{}{} {{\n",
        tp,
        generics::user_trait_rust(&block.trait_name),
        type_name,
        tp
    ));
    for m in &block.methods {
        emit_trait_method(cx, type_name, m, out, 1);
    }
    out.push_str("}\n\n");
}

fn emit_external_trait_impl(cx: &Cx, i: &ImplDef, out: &mut String) {
    let trait_name = i.trait_name.as_deref().unwrap_or("");
    out.push_str(&format!(
        "impl {} for user_{} {{\n",
        generics::user_trait_rust(trait_name),
        i.type_name
    ));
    for m in &i.methods {
        emit_trait_method(cx, &i.type_name, m, out, 1);
    }
    out.push_str("}\n\n");
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
            if p.name == syntax::KW_SELF {
                "&self".to_string()
            } else {
                format!(
                    "_{}: {}",
                    cx.mangle_name(&p.name),
                    rust_param_type(cx, p.convention, &p.ty)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "{pad}fn {}({}){ret_clause} {{\n",
        f.name,
        params,
        ret_clause = ret_clause
    ));
    let mut env: HashMap<String, Slot> = HashMap::new();
    for p in &f.params {
        if p.name == syntax::KW_SELF {
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
        "{}pub fn {}({}){} {{\n",
        pad,
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

fn emit_func(cx: &Cx, f: &Func, out: &mut String) {
    let extra = if f.type_params.is_empty() {
        HashMap::new()
    } else {
        generics::rust_extra_clone_bounds(&f.type_params)
    };
    let gen = generics::rust_type_param_list(&f.type_params, &extra);
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
    out.push_str(&format!(
        "{vis}fn {name}{gen}({params}){ret} {{\n",
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
            let mut_fn = matches!(
                &b.init,
                Expr::Lambda(l) if l.meta.escapes && l.meta.needs_fn_mut
            );
            let mut init = if b.is_comptime {
                b.ct
                    .as_ref()
                    .map(|v| v.serialize())
                    .unwrap_or_else(|| "Default::default()".to_string())
            } else {
                emit_expr(cx, &b.init, env)
            };
            if matches!(b.ty, Some(Type::Named(ref n)) if n == "U8")
                && matches!(b.init, Expr::Int(_, _))
            {
                init = format!("({}) as u8", init);
            }
            if mut_fn {
                if let Some(Type::Fn { params, ret }) = &b.ty {
                    init = format!(
                        "{} as {}",
                        init,
                        cx.rust_fn_trait(params, ret.as_deref(), true)
                    );
                }
            }
            let kw = if (b.mutable && !b.is_comptime) || mut_fn {
                "let mut"
            } else {
                "let"
            };
            let ty = b.ty.as_ref().map(|t| {
                if let Type::Fn { params, ret } = t {
                    format!(
                        ": {}",
                        cx.rust_fn_trait(params, ret.as_deref(), mut_fn)
                    )
                } else {
                    format!(": {}", cx.rust_type(t))
                }
            }).unwrap_or_default();
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
                LValue::Index { base, index, kind, .. } => {
                    // Sema resolved the collection kind (R2); fall back to
                    // the env type only for un-checked trees (tests).
                    let is_map = matches!(kind, IndexKind::Map)
                        || (matches!(kind, IndexKind::Unknown)
                            && matches!(expr_jet_ty(base, env), Some(Type::Map { .. })));
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
        .unwrap_or_else(|| {
            if is_json_variant(variant) {
                format!("{}jet_std::Json", cx.root_prefix)
            } else {
                "user_TYPE".to_string()
            }
        })
}

fn is_json_variant(variant: &str) -> bool {
    matches!(
        variant,
        "Null" | "Boolean" | "Number" | "Text" | "Array" | "Object"
    )
}

fn variant_rust_name(variant: &str) -> String {
    if is_json_variant(variant) {
        variant.to_string()
    } else {
        mangle(variant)
    }
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
                format!("matches!({}, {}::{})", subject, prefix, variant_rust_name(variant))
            } else if bindings.len() == 1 {
                format!(
                    "matches!({}, {}::{}({}))",
                    subject,
                    prefix,
                    variant_rust_name(variant),
                    mangle(&bindings[0])
                )
            } else {
                format!(
                    "matches!({}, {}::{} {{ {} }})",
                    subject,
                    prefix,
                    variant_rust_name(variant),
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
                format!("{}::{}", prefix, variant_rust_name(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    variant_rust_name(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}::{} {{ {} }}", prefix, variant_rust_name(variant), fields)
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
    let msg = if call.args.len() == 2 {
        emit_panic_message(cx, &call.args[1].expr, env)
    } else {
        "\"condition failed\"".to_string()
    };
    if cx.test_mode {
        let msg_expr = if call.args.len() == 2 {
            msg
        } else {
            "\"condition failed\".to_string()".to_string()
        };
        return format!("{{ if !({}) {{ return Err({}); }} }}", cond, msg_expr);
    }
    let (line, _) = span_line_col(&cx.src, call.name_span.start);
    format!(
        "{{ if !({}) {{ jet_panic({}, {}, &{}); }} }}",
        cond,
        escape_rust_str(&cx.file),
        line,
        if call.args.len() == 2 {
            msg
        } else {
            "\"condition failed\".to_string()".to_string()
        }
    )
}

fn emit_require_eq(cx: &Cx, call: &crate::ast::Call, env: &HashMap<String, Slot>) -> String {
    let left = emit_expr(cx, &call.args[0].expr, env);
    let right = emit_expr(cx, &call.args[1].expr, env);
    if cx.test_mode {
        return format!(
            "{{ let _jet_left = ({}); let _jet_right = ({}); if !(_jet_left == _jet_right) {{ return Err(format!(\"left: {{}}, right: {{}}\", _jet_left.jet_show(), _jet_right.jet_show())); }} }}",
            left, right
        );
    }
    let (line, _) = span_line_col(&cx.src, call.name_span.start);
    format!(
        "{{ if !({left} == {right}) {{ jet_panic({}, {}, &format!(\"left: {{}}, right: {{}}\", ({left}).jet_show(), ({right}).jet_show())); }} }}",
        escape_rust_str(&cx.file),
        line,
        left = left,
        right = right,
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
                format!("{}::{}", prefix, variant_rust_name(variant))
            } else if bindings.len() == 1 {
                format!(
                    "{}::{}({})",
                    prefix,
                    variant_rust_name(variant),
                    mangle(&bindings[0])
                )
            } else {
                let fields = bindings
                    .iter()
                    .map(|b| format!("{}: {}", mangle(b), mangle(b)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}::{} {{ {} }}", prefix, variant_rust_name(variant), fields)
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
    }
}

fn emit_named_fn_value(cx: &Cx, name: &str, ft: &Type) -> String {
    let rust_name = mangle(name);
    let Type::Fn { params, ret } = ft else {
        return rust_name;
    };
    let arg_decls: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| format!("__jet_a{}: {}", i, cx.rust_type(p)))
        .collect();
    let arg_calls: Vec<String> = (0..params.len()).map(|i| format!("__jet_a{i}")).collect();
    let _ = ret;
    format!(
        "Box::new(move |{}| {}({})) as {}",
        arg_decls.join(", "),
        rust_name,
        arg_calls.join(", "),
        cx.rust_type(ft)
    )
}

fn receiver_struct_type(receiver: &Expr, env: &HashMap<String, Slot>) -> Option<String> {
    match receiver {
        Expr::Ident(name, _) => env.get(name).and_then(|s| s.jet_ty.as_ref()).and_then(|t| {
            if let Type::Named(n) = t {
                Some(n.clone())
            } else {
                None
            }
        }),
        _ => None,
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

fn user_type_apply_rust(cx: &Cx, name: &str, type_args: &[Type]) -> String {
    if type_args.is_empty() {
        format!("user_{name}")
    } else {
        format!(
            "user_{name}::<{}>",
            type_args
                .iter()
                .map(|a| cx.rust_type(a))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
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
            if env.get(name).is_none() {
                if let Some(ft) = cx.fn_types.get(name) {
                    return emit_named_fn_value(cx, name, ft);
                }
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
            } else if let Expr::Ident(alias, _) = &**inner {
                if let Some(module) = cx.std_imports.get(alias) {
                    emit_std_field(module, member)
                } else if alias == "Json" && member == "Null" {
                    format!("{}jet_std::Json::Null", cx.root_prefix)
                } else if cx.enum_variants.contains_key(alias) {
                    format!("user_{}::{}", alias, mangle(member))
                } else {
                    format!(
                        "({}).{}",
                        emit_expr(cx, inner, env),
                        mangle(member)
                    )
                }
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
            recv_type,
            ..
        } => emit_method_call(cx, receiver, method, args, recv_type.as_deref(), env),
        Expr::StructLit {
            type_name,
            type_args,
            import_ns,
            as_trait,
            fields,
            ..
        } => {
            let mut parts = Vec::new();
            if let Some(ns) = import_ns {
                let mod_name = cx
                    .import_mods
                    .get(ns)
                    .map(|s| s.as_str())
                    .unwrap_or("user_unknown");
                let rust_type = if type_args.is_empty() {
                    format!("{}::{}", mod_name, mangle(type_name))
                } else {
                    format!(
                        "{}::{}::<{}>",
                        mod_name,
                        mangle(type_name),
                        type_args
                            .iter()
                            .map(|a| cx.rust_type(a))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                for (n, _, e) in fields {
                    parts.push(format!("{}: {}", mangle(n), emit_expr(cx, e, env)));
                }
                let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
                if let Some(trait_name) = as_trait {
                    return format!(
                        "Box::new({lit}) as Box<dyn {}>",
                        generics::user_trait_rust(trait_name)
                    );
                }
                return lit;
            }
            let rust_type = user_type_apply_rust(cx, type_name, type_args);
            for (n, _, e) in fields {
                parts.push(format!("{}: {}", mangle(n), emit_expr(cx, e, env)));
            }
            let lit = format!("{} {{ {} }}", rust_type, parts.join(", "));
            if let Some(trait_name) = as_trait {
                format!(
                    "Box::new({lit}) as Box<dyn {}>",
                    generics::user_trait_rust(trait_name)
                )
            } else {
                lit
            }
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
        Expr::Lambda(lam) => emit_lambda(cx, lam, env),
        Expr::CallValue { callee, args, .. } => {
            let f = emit_expr(cx, callee, env);
            let arg_str = emit_call_args(cx, None, args, env);
            format!("({})({})", f, arg_str)
        }
    }
}

fn emit_lambda(cx: &Cx, lam: &Lambda, env: &HashMap<String, Slot>) -> String {
    let mut prep = String::new();
    let mut lam_env = env.clone();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            place_of(env, name)
        ));
        lam_env.insert(
            name.clone(),
            Slot {
                rust_name: cap,
                deref: false,
                jet_ty: None,
            },
        );
    }
    for p in &lam.params {
        lam_env.insert(
            p.name.clone(),
            Slot {
                rust_name: mangle(&p.name),
                deref: false,
                jet_ty: p.ty.clone(),
            },
        );
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty = p
                .ty
                .as_ref()
                .map(|t| format!(": {}", cx.rust_type(t)))
                .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_expr(cx, e, &lam_env),
        LambdaBody::Block(stmts) => {
            let mut inner = String::new();
            emit_stmts(cx, stmts, &mut lam_env, &mut inner, 1, false);
            format!("{{ {} }}", inner)
        }
    };
    let move_kw = if lam.meta.needs_fn_mut && !lam.meta.escapes {
        ""
    } else {
        "move "
    };
    let closure = format!("{}|{}| {}", move_kw, params.join(", "), body);
    let wrapped = if lam.meta.escapes {
        format!("Box::new({})", closure)
    } else {
        closure
    };
    if prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", prep, wrapped)
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

fn list_carries_trait(cx: &Cx, inner: &Type) -> bool {
    matches!(inner, Type::TraitObject(_))
        || matches!(inner, Type::Named(n) if cx.trait_names.contains(n))
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
            (syntax::TYPE_STRING, "from_bytes") => {
                return Some(format!("{}jet_string_from_bytes(&({}))", cx.root_prefix, arg(0)));
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
        "bytes" => Some(format!("{}jet_string_bytes(&({}))", cx.root_prefix, recv)),
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
        "to_u8" => Some(format!("{}jet_int_to_u8({})", cx.root_prefix, recv)),
        "elapsed_millis" => Some(format!("{}jet_stopwatch_elapsed_millis(&({}))", cx.root_prefix, recv)),
        "map" => {
            if let Expr::Lambda(l) = &args[0].expr {
                if l.meta.needs_fn_mut {
                    Some(format!("jet_list_map_mut(({}).clone(), {})", recv, arg(0)))
                } else {
                    Some(format!("jet_list_map(({}).clone(), {})", recv, arg(0)))
                }
            } else {
                Some(format!("jet_list_map(({}).clone(), {})", recv, arg(0)))
            }
        }
        "filter" => Some(format!("jet_list_filter(({}).clone(), {})", recv, arg(0))),
        "each" => {
            let trait_obj_list = matches!(rty, Some(Type::List(ref inner)) if list_carries_trait(cx, inner));
            let list_each = |a: &str| {
                if trait_obj_list {
                    format!("jet_list_each_ref(&({recv}), {a})")
                } else if let Expr::Lambda(l) = &args[0].expr {
                    if l.meta.needs_fn_mut {
                        format!("jet_list_each_mut(({}).clone(), {})", recv, a)
                    } else {
                        format!("jet_list_each(({}).clone(), {})", recv, a)
                    }
                } else {
                    format!("jet_list_each(({}).clone(), {})", recv, a)
                }
            };
            match rty {
                Some(Type::Map { .. }) => Some(format!("jet_map_each(({}).clone(), {})", recv, arg(0))),
                _ => Some(list_each(&arg(0))),
            }
        }
        "find" => Some(format!("jet_list_find(({}).clone(), {})", recv, arg(0))),
        "any" => Some(format!("jet_list_any(({}).clone(), {})", recv, arg(0))),
        "all" => Some(format!("jet_list_all(({}).clone(), {})", recv, arg(0))),
        "sort_by" => Some(format!("{{ jet_list_sort_by(&mut {}, {}); }}", recv, arg(0))),
        "reduce" => Some(format!(
            "jet_list_reduce(({}).clone(), {}, {})",
            recv,
            arg(0),
            arg(1)
        )),
        _ => None,
    }
}

fn emit_std_field(module: &str, name: &str) -> String {
    match (module, name) {
        ("std.math", "pi") => "std::f64::consts::PI".to_string(),
        ("std.math", "e") => "std::f64::consts::E".to_string(),
        _ => "/* unknown std field */".to_string(),
    }
}

fn emit_std_call(
    cx: &Cx,
    module: &str,
    method: &str,
    args: &[crate::ast::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    let arg = |i: usize| {
        args.get(i)
            .map(|a| emit_expr(cx, &a.expr, env))
            .unwrap_or_default()
    };
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    match (module, method) {
        ("std.fs", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("std.fs", "read_bytes") => format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0)),
        ("std.fs", "write") => format!("{}(&({}), &({}))", helper("jet_std_fs_write"), arg(0), arg(1)),
        ("std.fs", "append") => format!("{}(&({}), &({}))", helper("jet_std_fs_append"), arg(0), arg(1)),
        ("std.fs", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("std.fs", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("std.fs", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("std.fs", "create_dir") => format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0)),
        ("std.fs", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("std.fs", "copy") => format!("{}(&({}), &({}))", helper("jet_std_fs_copy"), arg(0), arg(1)),
        ("std.fs", "rename") => format!("{}(&({}), &({}))", helper("jet_std_fs_rename"), arg(0), arg(1)),
        ("std.io", "args") => format!("{}()", helper("jet_std_io_args")),
        ("std.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        ("std.io", "read_all_input") => format!("{}()", helper("jet_std_io_read_all_input")),
        ("std.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        ("std.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("std.env", "set") => format!("{}(&({}), &({}))", helper("jet_std_env_set"), arg(0), arg(1)),
        ("std.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("std.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("std.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("std.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        ("std.math", "sqrt") => format!("{}({})", helper("jet_std_math_sqrt"), arg(0)),
        ("std.math", "pow") => format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1)),
        ("std.math", "abs") => format!("({}).abs()", arg(0)),
        ("std.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("std.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("std.math", "floor") => format!("{}({})", helper("jet_std_math_floor"), arg(0)),
        ("std.math", "ceil") => format!("{}({})", helper("jet_std_math_ceil"), arg(0)),
        ("std.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        ("std.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        ("std.random", "int") => format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1)),
        ("std.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("std.random", "pick") => format!("{}(&({}))", helper("jet_std_random_pick"), arg(0)),
        ("std.random", "shuffle") => format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0)),
        ("std.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        ("std.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("std.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("std.time", "start") => format!("{}()", helper("jet_std_time_start")),
        ("std.json", "parse") => format!("{}(&({}))", helper("jet_std_json_parse"), arg(0)),
        ("std.json", "render") => format!("{}(&({}))", helper("jet_std_json_render"), arg(0)),
        ("std.json", "render_pretty") => format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0)),
        _ => "/* unknown std call */".to_string(),
    }
}

fn emit_std_json_lit(
    cx: &Cx,
    variant: &str,
    args: &[crate::ast::CallArg],
    env: &HashMap<String, Slot>,
) -> String {
    let arg = |i: usize| {
        args.get(i)
            .map(|a| emit_expr(cx, &a.expr, env))
            .unwrap_or_default()
    };
    let prefix = format!("{}jet_std::Json", cx.root_prefix);
    match variant {
        "Null" => format!("{prefix}::Null"),
        "Boolean" => format!("{prefix}::Boolean({})", arg(0)),
        "Number" => format!("{prefix}::Number({})", arg(0)),
        "Text" => format!("{prefix}::Text({})", arg(0)),
        "Array" => format!("{prefix}::Array({})", arg(0)),
        "Object" => format!("{prefix}::Object({})", arg(0)),
        _ => "/* unknown Json variant */".to_string(),
    }
}

fn emit_method_call(
    cx: &Cx,
    receiver: &Expr,
    method: &str,
    args: &[crate::ast::CallArg],
    recv_type: Option<&str>,
    env: &HashMap<String, Slot>,
) -> String {
    if method == "clone" {
        return format!("({}).clone()", emit_expr(cx, receiver, env));
    }
    let struct_ty = recv_type
        .map(|s| s.to_string())
        .or_else(|| receiver_struct_type(receiver, env));
    if let Some(type_name) = struct_ty {
        if let Some(fields) = cx.struct_fields.get(&type_name) {
            if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == method) {
                if matches!(field_ty, Type::Fn { .. }) {
                    let recv = emit_expr(cx, receiver, env);
                    let arg_str = emit_call_args(cx, None, args, env);
                    return format!("(({}).{})({})", recv, mangle(method), arg_str);
                }
            }
        }
    }
    if let Expr::Ident(alias, _) = receiver {
        if let Some(module) = cx.std_imports.get(alias) {
            return emit_std_call(cx, module, method, args, env);
        }
        if let Some(mod_name) = cx.import_mods.get(alias) {
            let sig = cx
                .import_sigs
                .get(&(alias.clone(), method.to_string()))
                .map(|s| s.as_slice());
            let arg_str = emit_call_args(cx, sig, args, env);
            return format!("{}::{}({})", mod_name, mangle(method), arg_str);
        }
    }
    // Built-in collection/string methods take precedence when they match.
    if let Some(s) = emit_builtin_method(cx, receiver, method, args, env) {
        return s;
    }
    if let Expr::Ident(type_name, _) = receiver {
        if type_name == "Json" {
            return emit_std_json_lit(cx, method, args, env);
        }
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
            let sig = cx
                .method_sigs
                .get(&(type_name.clone(), method.to_string()));
            let arg_str = emit_call_args(cx, sig.map(|s| s.as_slice()), args, env);
            return format!(
                "{}::{}({})",
                cx.type_prefix(type_name),
                cx.mangle_name(method),
                arg_str
            );
        }
    }
    let recv = emit_expr(cx, receiver, env);
    if let Some(rt) = recv_type {
        if cx.trait_names.contains(rt) {
            let arg_str = emit_call_args(cx, None, args, env);
            return format!("({}).{}({})", recv, method, arg_str);
        }
    }
    let sig = recv_type
        .and_then(|t| cx.method_sigs.get(&(t.to_string(), method.to_string())));
    let arg_str = emit_call_args(cx, sig.map(|s| s.as_slice()), args, env);
    format!("({}).{}({})", recv, mangle(method), arg_str)
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
            if let Some((_, Type::Fn { .. })) = &conv {
                let already_boxed = s.starts_with("Box::new(")
                    || matches!(
                        &a.expr,
                        Expr::Ident(name, _)
                            if env
                                .get(name)
                                .and_then(|slot| slot.jet_ty.as_ref())
                                .is_some_and(|t| matches!(t, Type::Fn { .. }))
                    );
                if !already_boxed {
                    s = format!("Box::new({})", s);
                }
                if let Some((_, ty)) = &conv {
                    s = format!("{} as {}", s, cx.rust_type(ty));
                }
            }
            match conv {
                Some((AccessConvention::Read, t))
                    if !t.is_scalar() && !matches!(t, Type::Fn { .. }) =>
                {
                    format!("&({})", s)
                }
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
    if call.name == syntax::BUILTIN_REQUIRE_EQ {
        return emit_require_eq(cx, call, env);
    }
    if env.contains_key(&call.name) && !cx.consts.contains_key(&call.name) {
        let callee = place_of(env, &call.name);
        let arg_str = emit_call_args(cx, None, &call.args, env);
        return format!("({})({})", callee, arg_str);
    }
    let sig = cx.sigs.get(&call.name);
    let args = if cx.extern_funcs.contains_key(&call.name) {
        emit_extern_call_args(cx, sig.map(|s| s.as_slice()), &call.args, env)
    } else {
        emit_call_args(cx, sig.map(|s| s.as_slice()), &call.args, env)
    };
    if let Some(wrapper) = cx.extern_funcs.get(&call.name) {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        return format!("{}::{}({})", crate_name, wrapper, args);
    }
    format!("{}({})", cx.mangle_name(&call.name), args)
}

fn emit_extern_call_args(
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
            if let Some((_, ty)) = sig.and_then(|ps| ps.get(i)) {
                if !ty.is_scalar() && !a.flags.implicit_clone {
                    s = format!("({}).clone()", s);
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn import_mod_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        let alias = loader::import_alias(imp);
        if let Ok(target) = loader::resolve_import_target(bundle, module_idx, imp) {
            let stem = &bundle.modules[target].alias;
            map.insert(alias, format!("user_{}", stem));
        }
    }
    map
}

fn std_import_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        if let Some(std_module) = loader::std_module_path(imp) {
            map.insert(loader::import_alias(imp), std_module);
        }
    }
    map
}

fn import_sig_map(bundle: &ProgramBundle, module_idx: usize) -> HashMap<(String, String), Vec<(AccessConvention, Type)>> {
    let mut map = HashMap::new();
    let module = &bundle.modules[module_idx];
    for imp in &module.imports {
        let alias = loader::import_alias(imp);
        let Ok(target) = loader::resolve_import_target(bundle, module_idx, imp) else {
            continue;
        };
        for item in &bundle.modules[target].items {
            if let Item::Func(f) = item {
                if f.is_pub {
                    map.insert(
                        (alias.clone(), f.name.clone()),
                        f.params
                            .iter()
                            .map(|p| (p.convention, p.ty.clone()))
                            .collect(),
                    );
                }
            }
        }
    }
    map
}

fn emit_program_items(cx: &Cx, items: &[Item], out: &mut String, include_main: bool) {
    for item in items {
        match item {
            Item::Trait(t) => m9::emit_trait_def(t, out),
            Item::Struct(s) => emit_struct(cx, s, out),
            Item::Enum(e) => emit_enum(cx, e, out),
            Item::Const(c) => emit_const(c, out),
            Item::Func(_) | Item::Impl(_) | Item::Test(_) | Item::ExternRust(_) => {}
        }
    }
    for item in items {
        match item {
            Item::Struct(s) => {
                emit_type_impl(cx, &s.name, &s.type_params, &s.methods, out);
                for block in &s.trait_impls {
                    emit_trait_impl(cx, &s.name, &s.type_params, block, out);
                }
            }
            Item::Enum(e) => {
                emit_type_impl(cx, &e.name, &e.type_params, &e.methods, out);
                for block in &e.trait_impls {
                    emit_trait_impl(cx, &e.name, &e.type_params, block, out);
                }
            }
            Item::Impl(i) => {
                if i.trait_name.is_some() {
                    emit_external_trait_impl(cx, i, out);
                } else {
                    emit_type_impl(cx, &i.type_name, &[], &i.methods, out);
                }
            }
            _ => {}
        }
    }
    for item in items {
        if let Item::Func(f) = item {
            if f.name == "main" && !include_main {
                continue;
            }
            emit_func(cx, f, out);
        }
    }
}

pub fn emit_bundle(bundle: &ProgramBundle, _mode: CompileMode, link: Option<&FfiLink>) -> String {
    let entry = &bundle.modules[bundle.entry];
    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} — do not edit. Edit the .{} source instead.\n",
        syntax::BINARY_NAME,
        syntax::FILE_EXT
    ));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    out.push_str(PRELUDE);
    if !bundle.used_std.is_empty() {
        out.push_str(STD_PRELUDE);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod user_{ns} {{\n"));
        out.push_str(MOD_USE);
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.import_mods = import_mod_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.std_imports = std_import_map(bundle, i);
        cx.used_std = bundle.used_std.clone();
        cx.root_prefix = "super::".to_string();
        emit_program_items(&cx, &module.items, &mut out, true);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.import_mods = import_mods;
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.std_imports = std_import_map(bundle, bundle.entry);
    cx.used_std = bundle.used_std.clone();
    emit_program_items(&cx, &entry.items, &mut out, true);
    out
}

pub fn emit_bundle_tests(bundle: &ProgramBundle, link: Option<&FfiLink>) -> String {
    let entry = &bundle.modules[bundle.entry];
    let tests: Vec<&TestDef> = entry
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Test(t) => Some(t),
            _ => None,
        })
        .collect();
    assert!(!tests.is_empty(), "emit_bundle_tests called with no test blocks");

    let mut out = String::new();
    out.push_str(&format!(
        "// Generated by {} test harness — do not edit.\n",
        syntax::BINARY_NAME
    ));
    out.push_str("#![allow(warnings)]\n\n");
    if let Some(ffi) = link {
        out.push_str(&format!("extern crate {};\n\n", ffi.crate_name));
    }
    out.push_str(PRELUDE);
    out.push_str(TEST_PRELUDE);
    if !bundle.used_std.is_empty() {
        out.push_str(STD_PRELUDE);
    }
    out.push('\n');

    let import_mods = import_mod_map(bundle, bundle.entry);
    let extern_funcs = bundle_extern_funcs(bundle);

    for (i, module) in bundle.modules.iter().enumerate() {
        if i == bundle.entry {
            continue;
        }
        let ns = module.alias.clone();
        out.push_str(&format!("mod user_{ns} {{\n"));
        out.push_str(MOD_USE);
        let mut cx = build_cx_items(
            &module.items,
            &module.source,
            &module.display,
            link,
            &extern_funcs,
        );
        cx.test_mode = true;
        cx.import_mods = import_mod_map(bundle, i);
        cx.import_sigs = import_sig_map(bundle, i);
        cx.std_imports = std_import_map(bundle, i);
        cx.used_std = bundle.used_std.clone();
        cx.root_prefix = "super::".to_string();
        emit_program_items(&cx, &module.items, &mut out, false);
        out.push_str("}\n\n");
    }

    let mut cx = build_cx_items(
        &entry.items,
        &entry.source,
        &entry.display,
        link,
        &extern_funcs,
    );
    cx.test_mode = true;
    cx.import_mods = import_mods;
    cx.import_sigs = import_sig_map(bundle, bundle.entry);
    cx.std_imports = std_import_map(bundle, bundle.entry);
    cx.used_std = bundle.used_std.clone();
    emit_program_items(&cx, &entry.items, &mut out, false);

    for (i, test) in tests.iter().enumerate() {
        out.push_str(&format!("fn jet_test_{}() -> Result<(), String> {{\n", i));
        let mut env: HashMap<String, Slot> = HashMap::new();
        emit_stmts(&cx, &test.body, &mut env, &mut out, 1, false);
        out.push_str("    Ok(())\n");
        out.push_str("}\n\n");
    }

    out.push_str("fn main() {\n");
    out.push_str("    let mut passed = 0usize;\n");
    out.push_str("    let mut failed = 0usize;\n");
    for (i, test) in tests.iter().enumerate() {
        let name = escape_rust_str(&test.name);
        out.push_str(&format!("    match jet_test_{}() {{\n", i));
        out.push_str("        Ok(()) => {\n");
        out.push_str(&format!("            println!(\"{{}}: pass\", {});\n", name));
        out.push_str("            passed += 1;\n");
        out.push_str("        }\n");
        out.push_str("        Err(msg) => {\n");
        out.push_str(&format!("            println!(\"{{}}: FAIL\", {});\n", name));
        out.push_str("            eprintln!(\"  {}\", msg);\n");
        out.push_str("            failed += 1;\n");
        out.push_str("        }\n");
        out.push_str("    }\n");
    }
    out.push_str("    println!(\"{} passed, {} failed\", passed, failed);\n");
    out.push_str("    if failed > 0 { std::process::exit(1); }\n");
    out.push_str("}\n");
    out
}
