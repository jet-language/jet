//! Semantic checks. Everything here exists so that codegen can stay "dumb"
//! (invariant I3): by the time a Program reaches codegen, it must be
//! impossible for the generated Rust to fail to compile (invariant I2).
//!
//! M1: type inference, mutability, comparison distribution (S25),
//! definite-return analysis. M2: ownership — moves, call-site `mut`/`take`,
//! view returns, use-after-move, and borrow rules that keep generated Rust
//! sound without surfacing Rust concepts to users.

use crate::ast::{
    AccessConvention, BinOp, Binding, Call, ConstAttr, ElseBranch, EnumDef, EnumLitArg, Expr,
    ExternFn, ExternRustBlock, ForKind, Func, IfStmt, IndexKind, Item, Lambda, LambdaBody, LValue,
    OrFallback, Pattern, Program, ProgramBundle, RustConstKind, Stmt, StrPart, StructDef, Type,
    UnOp, VariantPayload,
};
use crate::loader;
use crate::collections::{self, is_map_key_type, is_reserved_type};
use crate::diag::{Diagnostic, Span};
use crate::generics::{e0901, e0904, e0905, e0909, generic_depth_exceeded, is_type_var_name, COMPARABLE};
use crate::m9::M9Registry;
use crate::syntax;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    /// S50: declared in `extern rust`, implemented by the FFI bridge.
    pub is_extern: bool,
}

#[derive(Debug, Clone)]
struct MethodSig {
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    is_view_return: bool,
    is_static: bool,
    self_conv: Option<AccessConvention>,
}

#[derive(Debug, Clone)]
enum TypeDef {
    Struct {
        name_span: Span,
        fields: Vec<(String, Span, Type, bool, bool)>,
        methods: HashMap<String, MethodSig>,
    },
    Enum {
        name_span: Span,
        variants: HashMap<String, (Span, VariantPayload)>,
        variant_order: Vec<String>,
        methods: HashMap<String, MethodSig>,
    },
}

struct TypeRegistry {
    types: HashMap<String, TypeDef>,
}

impl TypeRegistry {
    fn contains(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }

    fn struct_fields(&self, name: &str) -> Option<&[(String, Span, Type, bool, bool)]> {
        match self.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => Some(fields.as_slice()),
            _ => None,
        }
    }

    fn enum_variants(&self, name: &str) -> Option<&HashMap<String, (Span, VariantPayload)>> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variants, .. }) => Some(variants),
            _ => None,
        }
    }

    fn enum_variant_order(&self, name: &str) -> Option<&[String]> {
        match self.types.get(name) {
            Some(TypeDef::Enum { variant_order, .. }) => Some(variant_order.as_slice()),
            _ => None,
        }
    }

    fn method(&self, type_name: &str, method: &str) -> Option<&MethodSig> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { methods, .. }) | Some(TypeDef::Enum { methods, .. }) => {
                methods.get(method)
            }
            _ => None,
        }
    }

    fn field_names(&self, type_name: &str) -> Vec<String> {
        match self.types.get(type_name) {
            Some(TypeDef::Struct { fields, .. }) => fields.iter().map(|(n, ..)| n.clone()).collect(),
            _ => Vec::new(),
        }
    }
}

fn func_to_method_sig(f: &Func) -> MethodSig {
    let self_param = f.self_param();
    MethodSig {
        params: f
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        is_view_return: f.is_view_return,
        is_static: self_param.is_none(),
        self_conv: self_param.map(|p| p.convention),
    }
}

fn func_to_sig(f: &Func) -> FuncSig {
    let type_params: HashSet<String> = f.type_params.iter().map(|p| p.name.clone()).collect();
    FuncSig {
        params: f
            .params
            .iter()
            .map(|p| {
                let conv = if matches!(&p.ty, Type::Named(n) if type_params.contains(n)) {
                    AccessConvention::Move
                } else {
                    p.convention
                };
                (conv, p.ty.clone())
            })
            .collect(),
        return_type: f.return_type.clone(),
        is_view_return: f.is_view_return,
        is_extern: false,
    }
}

fn extern_to_sig(ef: &ExternFn) -> FuncSig {
    FuncSig {
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: ef.return_type.clone(),
        is_view_return: ef.is_view_return,
        is_extern: true,
    }
}

fn check_extern_block(
    block: &ExternRustBlock,
    registry: &TypeRegistry,
    diags: &mut Vec<Diagnostic>,
) -> bool {
    let mut ok = true;
    if crate::ffi::crate_spec_needs_version(&block.crate_spec) {
        diags.push(Diagnostic::error(
            "E0701",
            format!(
                "the crate `{}` needs a version pin",
                block.crate_spec
            ),
            "every non-`std` `extern rust` crate must pin an exact version so builds stay reproducible"
                .to_string(),
            format!(
                "write: extern rust \"{}@0.1\" {{ ... }}",
                block.crate_spec
            ),
            Some(block.crate_span),
        ));
        ok = false;
    }
    for ef in &block.functions {
        if !check_extern_fn(ef, registry, diags) {
            ok = false;
        }
    }
    ok
}

fn check_extern_fn(ef: &ExternFn, registry: &TypeRegistry, diags: &mut Vec<Diagnostic>) -> bool {
    let mut ok = true;
    if ef.is_view_return {
        diags.push(ffi_type_error(
            "a `view` return can't cross into Rust",
            "foreign functions must return owned values — nothing borrowed across the boundary",
            "return the value directly, or wrap it in a `List` or `String`",
            ef.name_span,
        ));
        ok = false;
    }
    for p in &ef.params {
        if p.convention != AccessConvention::Read {
            diags.push(ffi_type_error(
                &format!("`{}` can't use `{}` at the FFI boundary", p.name, access_keyword(p.convention)),
                "foreign functions take owned copies — `mut`, `take`, and `view` aren't allowed here",
                "remove the access keyword and pass by value",
                p.name_span,
            ));
            ok = false;
        }
        if !is_ffi_type(&p.ty, registry) {
            diags.push(ffi_type_error(
                &format!("`{}` has type `{}`, which can't cross into Rust", p.name, p.ty.name()),
                "only plain value types can cross the `extern rust` boundary — no references, callbacks, or trait objects",
                "use `Int`, `Float`, `Bool`, `String`, `Char`, collections of those, or a struct whose fields are allowed",
                p.ty_span,
            ));
            ok = false;
        }
    }
    if let Some(rt) = &ef.return_type {
        if !is_ffi_type(rt, registry) {
            diags.push(ffi_type_error(
                &format!("the return type `{}` can't cross from Rust", rt.name()),
                "foreign functions must return owned values Jet understands",
                "use an allowed return type, or flatten the result into simpler parts",
                ef.name_span,
            ));
            ok = false;
        }
    }
    ok
}

fn access_keyword(c: AccessConvention) -> &'static str {
    match c {
        AccessConvention::Read => "read",
        AccessConvention::Mutate => syntax::KW_MUTATE,
        AccessConvention::Move => syntax::KW_MOVE,
    }
}

fn ffi_type_error(what: &str, why: &str, fix: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0702",
        what.to_string(),
        why.to_string(),
        fix.to_string(),
        Some(span),
    )
}

fn is_ffi_type(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::Shared(_) => false,
        Type::List(inner) | Type::Option(inner) => is_ffi_type(inner, registry),
        Type::Map { key, value } => is_ffi_type(key, registry) && is_ffi_type(value, registry),
        Type::Result { ok, err } => is_ffi_type(ok, registry) && is_ffi_type(err, registry),
        Type::Named(name) => ffi_named_type_ok(name, registry),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Fn { .. } => false,
    }
}

fn ffi_named_type_ok(name: &str, registry: &TypeRegistry) -> bool {
    match registry.types.get(name) {
        Some(TypeDef::Struct { fields, .. }) => fields
            .iter()
            .all(|(_, _, ty, _, _)| is_ffi_type(ty, registry)),
        Some(TypeDef::Enum { variants, .. }) => variants.values().all(|(_, payload)| match payload {
            VariantPayload::Unit => true,
            VariantPayload::Single(ty, _) => is_ffi_type(ty, registry),
            VariantPayload::Named(fs) => fs.iter().all(|f| is_ffi_type(&f.ty, registry)),
        }),
        None => false,
    }
}

fn register_extern_fn(
    ef: &ExternFn,
    funcs: &mut HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
    diags: &mut Vec<Diagnostic>,
) {
    if ef.name == syntax::BUILTIN_PRINT
        || ef.name == syntax::BUILTIN_PANIC
        || ef.name == syntax::BUILTIN_REQUIRE
        || ef.name == syntax::BUILTIN_REQUIRE_EQ
    {
        diags.push(Diagnostic::error(
            "E0106",
            format!("the name `{}` is built in and can't be redefined", ef.name),
            format!("`{}` is provided by the language itself", ef.name),
            "choose a different name for this foreign function".to_string(),
            Some(ef.name_span),
        ));
        return;
    }
    if name_defined(&ef.name, funcs, registry, consts) {
        diags.push(defined_twice(
            &ef.name,
            "every function needs a unique name so calls aren't ambiguous",
            ef.name_span,
        ));
        return;
    }
    funcs.insert(ef.name.clone(), extern_to_sig(ef));
}

#[derive(Debug, Clone)]
struct LocalInfo {
    ty: Type,
    mutable: bool,
    /// Set when the name is a parameter (with its access convention).
    param_conv: Option<AccessConvention>,
    /// Loop nesting depth where the name was declared (for move-in-loop).
    decl_loop_depth: usize,
}

/// What the driver is compiling — affects `main` / test requirements (M6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileMode {
    /// `jet run` / `jet build` — needs `main`, ignores test blocks in codegen.
    Run,
    /// `jet test` — needs at least one test; `main` is optional.
    Test,
    /// `jet check` / LSP — type-check only; imported modules and library files
    /// need not define `main`.
    Check,
}

pub fn check(prog: &mut Program) -> Vec<Diagnostic> {
    check_with_mode(prog, CompileMode::Run)
}

pub fn check_with_mode(prog: &mut Program, mode: CompileMode) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut funcs: HashMap<String, FuncSig> = HashMap::new();
    let mut tests: HashMap<String, Span> = HashMap::new();
    let mut registry = TypeRegistry {
        types: HashMap::new(),
    };
    let mut consts: HashMap<String, Type> = HashMap::new();
    let mut m9 = M9Registry::default();
    // Legacy M2 struct map for ref-field checks and cloneable helper.
    let mut struct_fields_legacy: HashMap<String, Vec<(Option<String>, Type)>> = HashMap::new();

    // --- registration pass (M3) -----------------------------------------
    for item in &prog.items {
        match item {
            Item::Trait(t) => {
                if name_defined(&t.name, &funcs, &registry, &consts) {
                    diags.push(defined_twice(
                        &t.name,
                        "every trait needs a unique name",
                        t.name_span,
                    ));
                }
            }
            Item::Func(f) => {
                if f.name == syntax::BUILTIN_PRINT
                    || f.name == syntax::BUILTIN_PANIC
                    || f.name == syntax::BUILTIN_REQUIRE
                    || f.name == syntax::BUILTIN_REQUIRE_EQ
                {
                    diags.push(Diagnostic::error(
                        "E0106",
                        format!(
                            "the name `{}` is built in and can't be redefined",
                            f.name
                        ),
                        format!("`{}` is provided by the language itself", f.name),
                        "choose a different name for this function".to_string(),
                        Some(f.name_span),
                    ));
                } else if name_defined(&f.name, &funcs, &registry, &consts) {
                    diags.push(defined_twice(
                        &f.name,
                        "every function needs a unique name so calls aren't ambiguous",
                        f.name_span,
                    ));
                } else {
                    funcs.insert(f.name.clone(), func_to_sig(f));
                }
            }
            Item::Struct(s) => register_struct(s, &mut registry, &mut struct_fields_legacy, &mut diags, &funcs, &consts),
            Item::Enum(e) => register_enum(e, &mut registry, &mut diags, &funcs, &consts),
            Item::Impl(i) => {
                if !registry.contains(&i.type_name) {
                    diags.push(Diagnostic::error(
                        "E0301",
                        format!("`impl {}` names a type that doesn't exist", i.type_name),
                        format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                        format!("define `struct {}` or `enum {}` first", i.type_name, i.type_name),
                        Some(i.type_span),
                    ));
                }
            }
            Item::Const(c) => register_const(c, &mut consts, &mut diags, &funcs, &registry),
            Item::Test(t) => {
                if name_defined(&t.name, &funcs, &registry, &consts) || tests.contains_key(&t.name)
                {
                    diags.push(defined_twice(
                        &t.name,
                        "every test needs a unique name so failures are easy to find",
                        t.name_span,
                    ));
                } else {
                    tests.insert(t.name.clone(), t.name_span);
                }
            }
            Item::ExternRust(block) => {
                if check_extern_block(block, &registry, &mut diags) {
                    for ef in &block.functions {
                        register_extern_fn(ef, &mut funcs, &registry, &consts, &mut diags);
                    }
                }
            }
        }
    }

    register_type_methods(&prog.items, &mut registry, &mut diags);
    register_impl_methods(&prog.items, &mut registry, &mut diags);
    m9.register_items(&prog.items, &mut diags);

    if mode == CompileMode::Run {
        match funcs.get("main") {
            None => {
                diags.push(Diagnostic::error(
                    "E0101",
                    "this program has no `main` function".to_string(),
                    "running a program starts at `fn main`, and this file doesn't define one"
                        .to_string(),
                    "add one to this file: fn main() { ... }".to_string(),
                    None,
                ));
            }
            Some(sig) => {
                if !sig.params.is_empty() || sig.return_type.is_some() {
                    let span = prog.items.iter().find_map(|i| match i {
                        Item::Func(f) if f.name == "main" => Some(f.name_span),
                        _ => None,
                    });
                    diags.push(Diagnostic::error(
                        "E0122",
                        "`main` takes no parameters and returns nothing".to_string(),
                        "`main` is where running starts; nothing calls it with values".to_string(),
                        "write it as: fn main() { ... }".to_string(),
                        span,
                    ));
                }
            }
        }
    }
    match mode {
        CompileMode::Test if tests.is_empty() => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `{}` blocks found to run", syntax::KW_TEST),
                format!(
                    "add at least one top-level block: {} \"describes what this checks\" {{ ... }}",
                    syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    syntax::BUILTIN_REQUIRE,
                    syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        CompileMode::Test | CompileMode::Run | CompileMode::Check => {}
    }

    // S57 (M9.5): evaluate comptime bindings before bodies are checked, so
    // references to them resolve. Single-file mode has no path; embed_file
    // resolves against the current directory.
    eval_comptime_items(&mut prog.items, &mut consts, std::path::Path::new("."), &mut diags);

    let const_names: Vec<String> = consts.keys().cloned().collect();
    let mut address_taken: HashSet<String> = HashSet::new();
    for item in &prog.items {
        match item {
            Item::Func(f) => walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken),
            Item::Struct(s) => {
                for m in &s.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                }
            }
            _ => {}
        }
    }
    for item in &mut prog.items {
        if let Item::Const(c) = item {
            let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
            c.rust_kind = if force_static || address_taken.contains(&c.name) {
                RustConstKind::Static
            } else {
                RustConstKind::Const
            };
        }
    }

    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&prog.items);
    let ct_base_dir = std::path::Path::new(".");

    // --- per-item body checks ---------------------------------------------
    for item in &mut prog.items {
        match item {
            Item::Func(f) => {
                diags.extend(check_func_body(
                    f,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &m9,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                ));
            }
            Item::Impl(i) => {
                for m in &mut i.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &m9,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                    ));
                }
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &m9,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                    ));
                }
                for block in &mut s.trait_impls {
                    for m in &mut block.methods {
                        diags.extend(check_func_body(
                            m,
                            &funcs,
                            &registry,
                            &struct_fields_legacy,
                            &consts,
                            &m9,
                            Some(&s.name),
                            &ct_funcs,
                            &ct_externs,
                            ct_base_dir,
                            &ct_globals,
                        ));
                    }
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    diags.extend(check_func_body(
                        m,
                        &funcs,
                        &registry,
                        &struct_fields_legacy,
                        &consts,
                        &m9,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        ct_base_dir,
                        &ct_globals,
                    ));
                }
            }
            Item::Test(t) => {
                let mut synthetic = crate::ast::Func {
                    is_pub: false,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    is_view_return: false,
                    body: std::mem::take(&mut t.body),
                };
                diags.extend(check_func_body(
                    &mut synthetic,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &m9,
                    None,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                ));
                t.body = synthetic.body;
            }
            Item::Const(_) | Item::ExternRust(_) | Item::Trait(_) => {}
        }
    }

    diags
}

fn name_defined(
    name: &str,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
) -> bool {
    funcs.contains_key(name) || registry.contains(name) || consts.contains_key(name)
}

/// S57 (M9.5): evaluate every `comptime NAME = expr;` in `items`. Purity and
/// fuel are enforced by the interpreter (E0951/E0952); panics surface as
/// E0953. Each result's Jet type is registered in `consts` so references
/// type-check, and the value is stashed on the item for codegen to inline.
fn eval_comptime_items(
    items: &mut [Item],
    consts: &mut HashMap<String, Type>,
    base_dir: &std::path::Path,
    diags: &mut Vec<Diagnostic>,
) {
    if !items
        .iter()
        .any(|i| matches!(i, Item::Const(c) if c.is_comptime))
    {
        return;
    }
    let mut results: Vec<(String, crate::comptime::CtValue)> = Vec::new();
    {
        let mut funcs: HashMap<String, &Func> = HashMap::new();
        let mut externs: HashSet<String> = HashSet::new();
        for item in items.iter() {
            match item {
                Item::Func(f) => {
                    funcs.insert(f.name.clone(), f);
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
        let mut globals: HashMap<String, crate::comptime::CtValue> = HashMap::new();
        for item in items.iter() {
            if let Item::Const(c) = item {
                if c.is_comptime {
                    match crate::comptime::evaluate(&c.value, &funcs, &externs, base_dir, &globals)
                    {
                        Ok(v) => {
                            consts.insert(c.name.clone(), v.jet_type());
                            globals.insert(c.name.clone(), v.clone());
                            results.push((c.name.clone(), v));
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
                if let Some(pos) = results.iter().position(|(n, _)| n == &c.name) {
                    c.ct = Some(results.remove(pos).1);
                }
            }
        }
    }
}

fn comptime_context_from_items(
    items: &[Item],
) -> (
    HashMap<String, Func>,
    HashSet<String>,
    HashMap<String, crate::comptime::CtValue>,
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
            Item::Test(_) | Item::Const(_) | Item::Trait(_) => {}
        }
    }
    (funcs, externs, globals)
}

fn register_const(
    c: &crate::ast::ConstDef,
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
    let ty = match &c.value {
        Expr::Int(_, _) => Some(Type::Int),
        Expr::Float(_, _) => Some(Type::Float),
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
                "give the const a number, like `const LIMIT = 10;`".to_string(),
                Some(c.value.span()),
            ));
        }
    }
}

fn register_struct(
    s: &StructDef,
    registry: &mut TypeRegistry,
    legacy: &mut HashMap<String, Vec<(Option<String>, Type)>>,
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
        fields.push((
            f.name.clone(),
            f.name_span,
            f.ty.clone(),
            f.is_stored_ref,
            f.is_pub,
        ));
    }
    registry.types.insert(
        s.name.clone(),
        TypeDef::Struct {
            name_span: s.name_span,
            fields,
            methods: HashMap::new(),
        },
    );
    legacy.insert(
        s.name.clone(),
        s.fields
            .iter()
            .map(|f| (f.stored_ref_label.clone(), f.ty.clone()))
            .collect(),
    );
    let ref_fields: Vec<_> = s.fields.iter().filter(|f| f.is_stored_ref).collect();
    if ref_fields.len() >= 2 {
        let unlabeled = ref_fields
            .iter()
            .filter(|f| f.stored_ref_label.is_none())
            .count();
        if unlabeled >= 2 {
            diags.push(Diagnostic::error(
                "E0207",
                "this struct has more than one stored reference without a label".to_string(),
                "when two `ref` fields may come from different places, each needs a label like `ref[src]`".to_string(),
                "add labels: `ref[a] x: String` and `ref[b] y: String`".to_string(),
                Some(s.name_span),
            ));
        }
    }
}

fn register_enum(
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
        variant_order.push(v.name.clone());
        variants.insert(v.name.clone(), (v.name_span, v.payload.clone()));
    }
    registry.types.insert(
        e.name.clone(),
        TypeDef::Enum {
            name_span: e.name_span,
            variants,
            variant_order,
            methods: HashMap::new(),
        },
    );
}

fn register_type_methods(items: &[Item], registry: &mut TypeRegistry, diags: &mut Vec<Diagnostic>) {
    for item in items {
        let (type_name, methods, field_names) = match item {
            Item::Struct(s) => (
                s.name.as_str(),
                &s.methods,
                registry.field_names(&s.name),
            ),
            Item::Enum(e) => (e.name.as_str(), &e.methods, Vec::new()),
            _ => continue,
        };
        let Some(type_def) = registry.types.get_mut(type_name) else {
            continue;
        };
        let methods_map = match type_def {
            TypeDef::Struct { methods, .. } | TypeDef::Enum { methods, .. } => methods,
        };
        for m in methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                diags.push(method_defined_twice(&m.name, type_name, m.name_span));
            } else {
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

fn register_impl_methods(items: &[Item], registry: &mut TypeRegistry, diags: &mut Vec<Diagnostic>) {
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
        };
        for m in &i.methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, &i.type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                diags.push(method_defined_twice(&m.name, &i.type_name, m.name_span));
            } else {
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

fn check_func_body(
    f: &mut Func,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<(Option<String>, Type)>>,
    consts: &HashMap<String, Type>,
    m9: &M9Registry,
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::comptime::CtValue>,
) -> Vec<Diagnostic> {
    let empty_imports = HashMap::new();
    let empty_std_imports = HashMap::new();
    let mut ck = Checker {
        funcs,
        registry,
        structs,
        consts,
        modules: None,
        module_idx: 0,
        imports: &empty_imports,
        std_imports: &empty_std_imports,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        in_unsafe: false,
        ret: f.return_type.clone(),
        view_return: f.is_view_return,
        fn_name: f.name.clone(),
        expected_type: None,
        owner_type: owner_type.map(str::to_string),
        iter_borrowed: HashSet::new(),
        borrow_ctx: false,
        lambda_escapes: true,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        m9,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
    };
    ck.check_params_and_body(f, owner_type);
    ck.diags
}

impl<'a> Checker<'a> {
    /// Shared tail of `check_func_body` / `check_func_body_bundle`:
    /// declare parameters, check the body, enforce definite return.
    fn check_params_and_body(&mut self, f: &mut Func, owner_type: Option<&str>) {
        for p in &f.params {
            let skip_type_check = p.name == syntax::KW_SELF
                && matches!(&p.ty, Type::Named(n) if n.is_empty());
            if !skip_type_check {
                let pty = self.resolve_type(p.ty.clone());
                self.check_declared_type(&pty, p.ty_span);
            }
            if p.name == syntax::KW_SELF {
                if let Some(owner) = owner_type {
                    let self_ty = Type::Named(owner.to_string());
                    self.scopes.last_mut().unwrap().insert(
                        p.name.clone(),
                        LocalInfo {
                            ty: self_ty,
                            mutable: matches!(p.convention, AccessConvention::Mutate),
                            param_conv: Some(p.convention),
                            decl_loop_depth: 0,
                        },
                    );
                }
                continue;
            }
            if self.lookup(&p.name).is_some() {
                self.diags.push(already_defined(&p.name, p.name_span));
            } else {
                let pty = self.resolve_type(p.ty.clone());
                self.scopes.last_mut().unwrap().insert(
                    p.name.clone(),
                    LocalInfo {
                        ty: pty,
                        mutable: matches!(p.convention, AccessConvention::Mutate),
                        param_conv: Some(p.convention),
                        decl_loop_depth: 0,
                    },
                );
            }
        }
        self.check_block(&mut f.body, false);
        if f.return_type.is_some() && !block_definitely_returns(&f.body) {
            let rt = f.return_type.clone().unwrap();
            self.diags.push(Diagnostic::error(
                "E0114",
                format!(
                    "`{}` promises to return {}, but a path can reach the end without `return`",
                    f.name,
                    rt.show()
                ),
                "every way through the function must hand back a value".to_string(),
                format!(
                    "add a final `return ...;`, or an `{}` branch that returns",
                    syntax::KW_ELSE
                ),
                Some(f.name_span),
            ));
        }
    }
}

fn already_defined(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0118",
        format!("the name `{}` is already taken here", name),
        "inside one function, each name refers to exactly one thing".to_string(),
        format!(
            "pick a different name, or assign to the existing one with `{} = ...`",
            name
        ),
        Some(span),
    )
}

/// E0105: a top-level definition's name collides with another item. Every
/// item kind shares the same `what` and `fix`; callers pass the kind-specific
/// `why` (functions, structs, enums, consts, traits, tests, …).
fn defined_twice(name: &str, why: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!("`{}` is defined twice", name),
        why.to_string(),
        "rename or remove one of the definitions".to_string(),
        Some(span),
    )
}

/// E0105: a method's name collides with a field on the same type.
fn method_field_clash(method: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!("method `{}` can't share a name with a field on `{}`", method, type_name),
        "a type's methods and fields must have different names".to_string(),
        "rename the method or the field".to_string(),
        Some(span),
    )
}

/// E0105: a method name appears twice on the same type.
fn method_defined_twice(method: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!("method `{}` is defined twice on `{}`", method, type_name),
        "each method name may appear only once on a type".to_string(),
        "rename or remove one of the definitions".to_string(),
        Some(span),
    )
}

struct ModuleState {
    funcs: HashMap<String, FuncSig>,
    func_pub: HashMap<String, bool>,
    type_pub: HashMap<String, bool>,
    method_pub: HashMap<(String, String), bool>,
    field_pub: HashMap<(String, String), bool>,
    registry: TypeRegistry,
    structs: HashMap<String, Vec<(Option<String>, Type)>>,
    consts: HashMap<String, Type>,
    imports: HashMap<String, usize>,
    std_imports: HashMap<String, String>,
    tests: HashMap<String, Span>,
    m9: M9Registry,
}

fn impl_type_exists(
    type_name: &str,
    registry: &TypeRegistry,
    imports: &HashMap<String, usize>,
    states: Option<&[ModuleState]>,
) -> bool {
    if let Some((alias, local)) = type_name.rsplit_once('.') {
        let Some(states) = states else {
            return false;
        };
        let Some(&idx) = imports.get(alias) else {
            return false;
        };
        return states[idx].registry.contains(local);
    }
    registry.contains(type_name)
}

struct Checker<'a> {
    funcs: &'a HashMap<String, FuncSig>,
    registry: &'a TypeRegistry,
    structs: &'a HashMap<String, Vec<(Option<String>, Type)>>,
    consts: &'a HashMap<String, Type>,
    modules: Option<&'a [ModuleState]>,
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    std_imports: &'a HashMap<String, String>,
    diags: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    /// name -> span of the use that gave the value away.
    moved: HashMap<String, Span>,
    loop_depth: usize,
    in_unsafe: bool,
    ret: Option<Type>,
    /// `-> view T` on this function (borrowed return).
    view_return: bool,
    fn_name: String,
    /// Context type for bare `null` (E0308).
    expected_type: Option<Type>,
    /// Enclosing type when checking a method body.
    owner_type: Option<String>,
    /// Collections currently read by an active `for x in xs` loop (E0507).
    iter_borrowed: HashSet<String>,
    /// True while inferring an expression that the generated Rust will only
    /// borrow (method receivers, field/index bases, lvalues). Field reads in
    /// borrow position must NOT be rewritten to `.clone()`.
    borrow_ctx: bool,
    /// M8: when false, a lambda is consumed inline (collection methods / borrow).
    lambda_escapes: bool,
    /// M8: binding name when checking `val f = (…) => …` (E0804 self-call).
    lambda_binding: Option<String>,
    /// Names mutably captured by an escaping lambda still in scope (E0204).
    lambda_mut_borrow_stack: Vec<HashSet<String>>,
    /// M9: generic/trait metadata for this program.
    m9: &'a M9Registry,
    /// M9.5: local comptime evaluation context.
    ct_funcs: &'a HashMap<String, Func>,
    ct_externs: &'a HashSet<String>,
    ct_base_dir: &'a std::path::Path,
    ct_globals: &'a HashMap<String, crate::comptime::CtValue>,
    ct_scopes: Vec<HashMap<String, crate::comptime::CtValue>>,
    /// Active generic type parameters while checking a generic item.
    type_param_scope: Vec<crate::ast::TypeParam>,
}

impl<'a> Checker<'a> {
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.lambda_mut_borrow_stack.push(HashSet::new());
        self.ct_scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.lambda_mut_borrow_stack.pop();
        self.ct_scopes.pop();
    }

    fn lambda_mut_borrow_active(&self, name: &str) -> bool {
        self.lambda_mut_borrow_stack
            .iter()
            .any(|s| s.contains(name))
    }

    fn current_ct_globals(&self) -> HashMap<String, crate::comptime::CtValue> {
        let mut globals = self.ct_globals.clone();
        for scope in &self.ct_scopes {
            for (name, value) in scope {
                globals.insert(name.clone(), value.clone());
            }
        }
        globals
    }

    fn lookup(&self, name: &str) -> Option<&LocalInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    fn declare(&mut self, name: &str, name_span: Span, info: LocalInfo) {
        if self.lookup(name).is_some() || self.consts.contains_key(name) {
            self.diags.push(already_defined(name, name_span));
        }
        self.moved.remove(name);
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), info);
    }

    fn declare_loop_var(&mut self, name: String, name_span: Span, ty: &Type) {
        if self.lookup(&name).is_some() || self.consts.contains_key(&name) {
            self.diags.push(already_defined(&name, name_span));
        } else {
            self.scopes.last_mut().unwrap().insert(
                name,
                LocalInfo {
                    ty: ty.clone(),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                },
            );
        }
    }

    fn check_declared_type(&mut self, ty: &Type, span: Span) {
        if let Some(chain) = generic_depth_exceeded(ty) {
            self.diags.push(e0909(&chain, span));
        }
        match ty {
            Type::Named(n) => {
                if std_type_known(n) {
                    return;
                }
                if self.type_param_scope.iter().any(|p| p.name == *n) {
                    return;
                }
                if self.m9.is_trait_name(n) {
                    return;
                }
                if self.registry.contains(n) {
                    return;
                }
                self.diags.push(Diagnostic::error(
                    "E0119",
                    format!("there's no type called `{}`", n),
                    format!(
                        "the types are `{}`, `{}`, `{}`, and `{}` (plus types you define)",
                        syntax::TYPE_INT,
                        syntax::TYPE_FLOAT,
                        syntax::TYPE_BOOL,
                        syntax::TYPE_STRING
                    ),
                    "check the spelling, or define the struct or enum first".to_string(),
                    Some(span),
                ));
            }
            Type::Apply { name, args } => {
                if !self.registry.contains(name) {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("there's no type called `{}`", name),
                        "generic types must name a struct or enum you defined".to_string(),
                        "check the spelling, or define the type first".to_string(),
                        Some(span),
                    ));
                }
                let expected = self
                    .m9
                    .struct_params
                    .get(name)
                    .or_else(|| self.m9.enum_params.get(name));
                if let Some(params) = expected {
                    if params.len() != args.len() {
                        self.diags.push(Diagnostic::error(
                            "E0119",
                            format!(
                                "`{}` expects {} type argument{}, got {}",
                                name,
                                params.len(),
                                if params.len() == 1 { "" } else { "s" },
                                args.len()
                            ),
                            "every generic parameter needs a matching type argument".to_string(),
                            format!("write `{}`<{}>", name, params.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")),
                            Some(span),
                        ));
                    }
                } else if !args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("`{}` isn't generic", name),
                        "only types declared with type parameters accept `<…>`".to_string(),
                        format!("use `{}` without type arguments", name),
                        Some(span),
                    ));
                }
                for arg in args {
                    self.check_declared_type(arg, span);
                }
            }
            Type::TraitObject(t) => {
                if !self.m9.is_trait_name(t) {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("there's no trait called `{t}`"),
                        "a trait name in type position must name a declared trait".to_string(),
                        format!("add `trait {t} {{ … }}` first"),
                        Some(span),
                    ));
                }
            }
            Type::Option(inner) => {
                if matches!(**inner, Type::Option(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0309",
                        "an optional type can't hold another optional type".to_string(),
                        format!("`{}??` isn't supported — use one `?` only (S32)", inner.name()),
                        "drop the inner `?` or unwrap before wrapping again".to_string(),
                        Some(span),
                    ));
                }
                self.check_declared_type(inner, span);
            }
            Type::List(inner) | Type::Shared(inner) => self.check_declared_type(inner, span),
            Type::Map { key, value } => {
                self.check_declared_type(key, span);
                self.check_declared_type(value, span);
                if !is_map_key_type(key) {
                    self.diags.push(Diagnostic::error(
                        "E0502",
                        format!("`{}` can't be a map key type yet", key.name()),
                        "map keys must be Int, String, Bool, Char, or a payload-free enum".to_string(),
                        "pick a simpler key type, or store a struct as the value".to_string(),
                        Some(span),
                    ));
                }
            }
            Type::Char => {}
            Type::Result { ok, err } => {
                self.check_declared_type(ok, span);
                self.check_declared_type(err, span);
            }
            Type::Fn { params, ret } => {
                for p in params {
                    self.check_declared_type(p, span);
                }
                if let Some(r) = ret {
                    self.check_declared_type(r, span);
                }
            }
            _ => {}
        }
    }

    fn type_known(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(n) => self.registry.contains(n) || std_type_known(n),
            Type::Option(inner) | Type::List(inner) | Type::Shared(inner) => self.type_known(inner),
            Type::Map { key, value } => self.type_known(key) && self.type_known(value),
            Type::Char => true,
            Type::Result { ok, err } => self.type_known(ok) && self.type_known(err),
            Type::Fn { params, ret } => {
                params.iter().all(|p| self.type_known(p))
                    && ret.as_ref().map_or(true, |r| self.type_known(r))
            }
            _ => true,
        }
    }

    /// Returns true when a diagnostic was emitted (the mismatch is already
    /// reported); callers may add a context-specific error otherwise.
    fn check_type_assignable(&mut self, want: &Type, got: &Type, span: Span) -> bool {
        if want == got {
            return false;
        }
        if is_u8_ty(want) && *got == Type::Int {
            return false;
        }
        if result_used_where_plain_expected(want, got) {
            self.diags.push(Diagnostic::error(
                "E0401",
                format!("this needs {}, but the value is {}", want.show(), got.show()),
                "a fallible result must be checked before its value is used".to_string(),
                format!(
                    "use `{}`, `{}`, or test with `== {}(...)` / `== {}(...)`",
                    syntax::OP_TRY_SUFFIX,
                    syntax::OP_OR_FALLBACK,
                    syntax::LIT_OK,
                    syntax::LIT_ERR
                ),
                Some(span),
            ));
            return true;
        }
        if option_used_where_plain_expected(want, got) {
            self.diags.push(Diagnostic::error(
                "E0310",
                format!("this needs {}, but the value is {}", want.show(), got.show()),
                "a plain value is required here, not an optional one".to_string(),
                format!(
                    "test with `== {}(...)` or `== {}` first, e.g. `if x == {}(n) {{ ... }}`",
                    syntax::LIT_VALUE,
                    syntax::LIT_NULL,
                    syntax::LIT_VALUE
                ),
                Some(span),
            ));
            return true;
        }
        if let Type::Option(inner) = got {
            if want.unwrap_option().is_some() {
                if let Some(want_inner) = want.unwrap_option() {
                    if **inner != *want_inner {
                        self.report_option_mismatch(want, got, span);
                        return true;
                    }
                }
            } else if **inner != *want {
                self.report_option_mismatch(want, got, span);
                return true;
            }
            return false;
        }
        if want.unwrap_option().is_some() && got.unwrap_option().is_none() {
            self.diags.push(Diagnostic::error(
                "E0108",
                format!("this needs {}, but the value is {}", want.show(), got.show()),
                "an optional value is required here".to_string(),
                format!("wrap it with `{}(...)`", syntax::LIT_VALUE),
                Some(span),
            ));
            return true;
        }
        match (want, got) {
            (Type::TraitObject(trait_name), Type::Named(type_name)) => {
                if self.m9.implements_trait(type_name, trait_name) {
                    return false;
                }
                let needs_derive = trait_name == COMPARABLE || trait_name == "Serialize";
                self.diags.push(e0905(type_name, trait_name, span, needs_derive));
                return true;
            }
            (Type::TraitObject(trait_name), Type::Apply { name, .. }) => {
                if self.m9.implements_trait(name, trait_name) {
                    return false;
                }
                let needs_derive = trait_name == COMPARABLE || trait_name == "Serialize";
                self.diags.push(e0905(name, trait_name, span, needs_derive));
                return true;
            }
            _ => {}
        }
        false
    }

    fn report_option_mismatch(&mut self, want: &Type, got: &Type, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0108",
            format!(
                "this needs {}, but the value is {}",
                want.show(),
                got.show()
            ),
            "the types must match".to_string(),
            type_fix_hint(want, got),
            Some(span),
        ));
    }

    // --- statements -----------------------------------------------------

    fn check_block(&mut self, stmts: &mut [Stmt], new_scope: bool) {
        if new_scope {
            self.push_scope();
        }
        for stmt in stmts.iter_mut() {
            self.check_stmt(stmt);
        }
        if new_scope {
            self.pop_scope();
        }
    }

    /// Check two alternative branches with independent move states, then
    /// keep the union (a value moved in either branch counts as gone).
    fn check_branches(&mut self, branches: &mut [&mut Vec<Stmt>]) {
        let before = self.moved.clone();
        let mut after = self.moved.clone();
        for body in branches.iter_mut() {
            self.moved = before.clone();
            self.check_block(body, true);
            for (k, v) in self.moved.drain() {
                after.entry(k).or_insert(v);
            }
        }
        self.moved = after;
    }

    fn check_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Val(b) => self.check_binding(b),
            Stmt::Assign {
                target,
                op,
                op_span: _,
                value,
            } => {
                if let (Some(op), LValue::Index { span, .. }) = (op, &*target) {
                    self.diags.push(Diagnostic::error(
                        "E0003",
                        "compound assignment can't target an indexed slot".to_string(),
                        "write the full new value: `map[key] = map[key] + 1`".to_string(),
                        format!("use `=` with the whole right-hand side"),
                        Some(*span),
                    ));
                    let _ = op;
                    self.infer(value);
                    return;
                }
                let vt = self.infer(value);
                self.note_move_if_direct_ident(value);
                match target {
                    LValue::Local { name, name_span } => {
                        let name_span = *name_span;
                        if self.lambda_mut_borrow_active(name) {
                            self.diags.push(aliasing_while_mut(name, name_span));
                        }
                        let Some(info) = self.lookup(name).cloned() else {
                            if self.consts.contains_key(name.as_str()) {
                                self.diags.push(Diagnostic::error(
                                    "E0111",
                                    format!("`{}` is a const and can never change", name),
                                    "a const is fixed for the whole program".to_string(),
                                    format!(
                                        "use a `{}` binding if it needs to change",
                                        syntax::KW_VAR
                                    ),
                                    Some(name_span),
                                ));
                            } else {
                                self.unknown_name(name, name_span);
                            }
                            return;
                        };
                        if !info.mutable {
                            let what = if info.param_conv.is_some() {
                                format!("the parameter `{}` can't be changed here", name)
                            } else {
                                format!(
                                    "`{}` was made with `{}`, so it can't change",
                                    name,
                                    syntax::KW_VAL
                                )
                            };
                            let fix = if info.param_conv.is_some() {
                                format!(
                                    "mark the parameter `{} {}: {}` if the function should change it",
                                    syntax::KW_MUTATE,
                                    name,
                                    info.ty.name()
                                )
                            } else {
                                format!(
                                    "declare it with `{} {} = ...` instead",
                                    syntax::KW_VAR, name
                                )
                            };
                            self.diags.push(Diagnostic::error(
                                "E0111",
                                what,
                                format!(
                                    "only `{}` bindings (and `{}` parameters) can be changed",
                                    syntax::KW_VAR,
                                    syntax::KW_MUTATE
                                ),
                                fix,
                                Some(name_span),
                            ));
                        }
                        self.moved.remove(name);
                        if let (Some(vt), false) =
                            (vt.clone(), info.ty == Type::Named(String::new()))
                        {
                            if vt != info.ty {
                                self.diags.push(Diagnostic::error(
                                    "E0108",
                                    format!(
                                        "`{}` holds {}, but this value is {}",
                                        name,
                                        info.ty.show(),
                                        vt.show()
                                    ),
                                    "a binding keeps one type for its whole life".to_string(),
                                    type_fix_hint(&info.ty, &vt),
                                    Some(value.span()),
                                ));
                            }
                        }
                    }
                    LValue::Index { base, index, span, kind } => {
                        self.borrow_ctx = true;
                        let base_ty = self.infer(base);
                        let idx_ty = self.infer(index);
                        match &base_ty {
                            Some(Type::Map { .. }) => *kind = IndexKind::Map,
                            Some(Type::List(_)) => *kind = IndexKind::List,
                            _ => {}
                        }
                        // Writing through `[ ]` changes the owner: the root
                        // name must be changeable and not under a `for` borrow.
                        if matches!(base_ty, Some(Type::Map { .. }) | Some(Type::List(_))) {
                            if let Some(root) = expr_root_ident(base) {
                                let root = root.to_string();
                                if self.iter_borrowed.contains(&root) {
                                    self.diags.push(collection_changed_in_loop(&root, *span));
                                }
                                if let Some(info) = self.lookup(&root) {
                                    if !info.mutable {
                                        self.diags.push(Diagnostic::error(
                                            "E0202",
                                            format!(
                                                "`{}` must be declared with `{}` to change it",
                                                root, syntax::KW_VAR
                                            ),
                                            "assigning into a collection changes it".to_string(),
                                            format!("declare `var {}: ...`", root),
                                            Some(*span),
                                        ));
                                    }
                                }
                            }
                        }
                        if idx_ty.as_ref() != Some(&Type::Int)
                            && !matches!(base_ty, Some(Type::Map { .. }))
                        {
                            if let Some(ref it) = idx_ty {
                                self.diags.push(Diagnostic::error(
                                    "E0505",
                                    format!(
                                        "list indexes must be {}, not {}",
                                        Type::Int.show(),
                                        it.show()
                                    ),
                                    "count positions with a whole number starting at 0".to_string(),
                                    "use an Int index, like `items[0]`".to_string(),
                                    Some(index.span()),
                                ));
                            }
                        }
                        if let Some(Type::Map { key, value: map_val_ty }) = base_ty {
                            if let Some(kt) = idx_ty {
                                if kt != *key {
                                    self.diags.push(Diagnostic::error(
                                        "E0505",
                                        format!(
                                            "this map holds keys of type {}, not {}",
                                            key.show(),
                                            kt.show()
                                        ),
                                        "the key in `map[key]` must match the map's key type"
                                            .to_string(),
                                        format!("use a {} key here", key.name()),
                                        Some(index.span()),
                                    ));
                                }
                            }
                            if let Some(vt) = vt {
                                if vt != *map_val_ty {
                                    self.diags.push(Diagnostic::error(
                                        "E0108",
                                        format!(
                                            "this map holds values of type {}, not {}",
                                            map_val_ty.show(),
                                            vt.show()
                                        ),
                                        "every value stored in a map must have the same type"
                                            .to_string(),
                                        type_fix_hint(&map_val_ty, &vt),
                                        Some(value.span()),
                                    ));
                                }
                            }
                        } else if let Some(Type::List(elem_ty)) = base_ty {
                            if let Some(vt) = vt {
                                if vt != *elem_ty {
                                    self.diags.push(Diagnostic::error(
                                        "E0108",
                                        format!(
                                            "this list holds {}, not {}",
                                            elem_ty.show(),
                                            vt.show()
                                        ),
                                        "every item stored in a list must have the same type"
                                            .to_string(),
                                        type_fix_hint(&elem_ty, &vt),
                                        Some(value.span()),
                                    ));
                                }
                            }
                        } else if let Some(Type::String) = base_ty {
                            self.diags.push(Diagnostic::error(
                                "E0503",
                                "strings aren't indexed with `[ ]`".to_string(),
                                "text is counted in characters — walk them with `.chars()` or take a piece with `.slice(start..end)`".to_string(),
                                "e.g. `for c in s.chars()` or `s.slice(0..2)`".to_string(),
                                Some(*span),
                            ));
                        }
                    }
                }
            }
            Stmt::Expr(expr) => {
                if let Some(ty) = self.infer_fallible_stmt(expr) {
                    if ty.is_fallible() {
                        self.diags.push(Diagnostic::error(
                            "E0402",
                            "this call can fail and nothing checks it".to_string(),
                            "a fallible result can't be ignored — handle it or say failure is impossible"
                                .to_string(),
                            format!(
                                "use `{}`, `{}`, or `{} ...` if failure can't happen here",
                                syntax::OP_TRY_SUFFIX,
                                syntax::OP_OR_FALLBACK,
                                syntax::BUILTIN_PANIC
                            ),
                            Some(expr.span()),
                        ));
                    }
                }
            }
            Stmt::Return(expr, span) => {
                match (&mut *expr, self.ret.clone()) {
                    (Some(e), Some(rt)) => {
                        let saved_expected = self.expected_type.clone();
                        self.expected_type = Some(rt.clone());
                        // In a `-> view` function the returned value stays a
                        // borrow, so a view call may flow straight through.
                        self.borrow_ctx = self.view_return;
                        let et = self.infer(e);
                        self.expected_type = saved_expected;
                        // Returning a borrowed parameter would move out of a
                        // borrow in the generated Rust (I2) — require a copy.
                        if let Expr::Ident(n, nspan) = &*e {
                            if let Some(info) = self.lookup(n) {
                                if !self.view_return
                                    && !info.ty.is_scalar()
                                    && matches!(
                                        info.param_conv,
                                        Some(AccessConvention::Read)
                                            | Some(AccessConvention::Mutate)
                                    )
                                {
                                    self.diags.push(Diagnostic::error(
                                        "E0120",
                                        format!(
                                            "`{}` is only borrowed here, so it can't be given back as-is",
                                            n
                                        ),
                                        "this function reads the value but doesn't own it"
                                            .to_string(),
                                        format!(
                                            "return a copy: `return {}.clone();` — or take ownership with `{} {}: {}`",
                                            n,
                                            syntax::KW_MOVE,
                                            n,
                                            info.ty.name()
                                        ),
                                        Some(*nspan),
                                    ));
                                }
                            }
                        }
                        if self.view_return && !self.expr_ok_for_view_return(e) {
                            self.diags.push(Diagnostic::error(
                                "E0206",
                                "this value can't be handed back as a shared view".to_string(),
                                "a `view` return may only point at a parameter, a whole-number or yes/no name, or a const — not at fresh text you just made here".to_string(),
                                "return a parameter or const, copy with `.clone()` into an owned return type, or change `-> view` to `->`".to_string(),
                                Some(e.span()),
                            ));
                        }
                        self.note_move_if_direct_ident(e);
                        if let Some(et) = et {
                            if et != rt {
                                self.diags.push(Diagnostic::error(
                                    "E0113",
                                    format!(
                                        "`{}` promises to return {}, but this returns {}",
                                        self.fn_name,
                                        rt.show(),
                                        et.show()
                                    ),
                                    "the value handed back must match the type after `->`"
                                        .to_string(),
                                    type_fix_hint(&rt, &et),
                                    Some(e.span()),
                                ));
                            }
                        }
                    }
                    (Some(e), None) => {
                        let ty_name = self.infer_name_or(e, "Int");
                        self.diags.push(Diagnostic::error(
                            "E0113",
                            format!("`{}` doesn't return a value", self.fn_name),
                            "a function only hands back a value if it declares one with `-> Type`"
                                .to_string(),
                            format!(
                                "remove the value (`return;`), or declare `-> {}` on the function",
                                ty_name
                            ),
                            Some(e.span()),
                        ));
                    }
                    (None, Some(rt)) => {
                        self.diags.push(Diagnostic::error(
                            "E0113",
                            format!(
                                "`{}` promises to return {}, but this `return` is empty",
                                self.fn_name,
                                rt.show()
                            ),
                            "the value handed back must match the type after `->`".to_string(),
                            "add the value: `return ...;`".to_string(),
                            Some(*span),
                        ));
                    }
                    (None, None) => {}
                }
            }
            Stmt::If(ifs) => self.check_if(ifs),
            Stmt::While { cond, body, span: _ } => {
                self.require_bool(cond, "a `while` condition");
                self.loop_depth += 1;
                self.check_block(body, true);
                self.loop_depth -= 1;
            }
            Stmt::For {
                var,
                var_span,
                var2,
                kind,
                body,
                span: _,
            } => {
                match kind {
                    ForKind::Range { start, end } => {
                        for (e, which) in [(&mut *start, "start"), (&mut *end, "end")] {
                            let t = self.infer(e);
                            if let Some(t) = t {
                                if t != Type::Int {
                                    self.diags.push(Diagnostic::error(
                                        "E0109",
                                        format!(
                                            "the {} of a `for` range must be {}, not {}",
                                            which,
                                            Type::Int.show(),
                                            t.show()
                                        ),
                                        "`for` counts whole numbers between two ends (both included, S22)"
                                            .to_string(),
                                        "use Int values for both ends, like `1..10`".to_string(),
                                        Some(e.span()),
                                    ));
                                }
                            }
                        }
                        self.loop_depth += 1;
                        self.push_scope();
                        let vs = *var_span;
                        let v = var.clone();
                        if self.lookup(&v).is_some() || self.consts.contains_key(&v) {
                            self.diags.push(already_defined(&v, vs));
                        }
                        self.scopes.last_mut().unwrap().insert(
                            v,
                            LocalInfo {
                                ty: Type::Int,
                                mutable: false,
                                param_conv: None,
                                decl_loop_depth: self.loop_depth,
                            },
                        );
                        for s in body.iter_mut() {
                            self.check_stmt(s);
                        }
                        self.pop_scope();
                        self.loop_depth -= 1;
                    }
                    ForKind::In { collection } => {
                        let coll_ty = self.infer(collection);
                        let borrowed = collection_root_name(collection);
                        self.loop_depth += 1;
                        if let Some(n) = borrowed.clone() {
                            self.iter_borrowed.insert(n);
                        }
                        self.push_scope();
                        match &coll_ty {
                            Some(Type::List(inner)) => {
                                self.declare_loop_var(var.clone(), *var_span, inner);
                            }
                            Some(Type::Map { key, value }) => {
                                if var2.is_none() {
                                    self.diags.push(Diagnostic::error(
                                        "E0003",
                                        "a map needs two loop names: `for key, value in map`"
                                            .to_string(),
                                        "maps carry a key and a value on each step".to_string(),
                                        format!(
                                            "write `for key, value in {}`",
                                            if let Expr::Ident(n, _) = &*collection {
                                                n.clone()
                                            } else {
                                                "the_map".to_string()
                                            }
                                        ),
                                        Some(collection.span()),
                                    ));
                                } else if let Some((v2, v2s)) = var2.as_ref() {
                                    self.declare_loop_var(var.clone(), *var_span, key);
                                    self.declare_loop_var(v2.clone(), *v2s, value);
                                }
                            }
                            Some(other) => {
                                self.diags.push(Diagnostic::error(
                                    "E0109",
                                    format!(
                                        "`for x in` needs a list or map, not {}",
                                        other.show()
                                    ),
                                    "walk items with `for item in items` or characters with `for c in s.chars()`".to_string(),
                                    "use a `List`, `Map`, or `s.chars()`".to_string(),
                                    Some(collection.span()),
                                ));
                            }
                            None => {}
                        }
                        for s in body.iter_mut() {
                            self.check_stmt(s);
                        }
                        self.pop_scope();
                        if let Some(n) = borrowed {
                            self.iter_borrowed.remove(&n);
                        }
                        self.loop_depth -= 1;
                    }
                }
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                span,
            } => self.check_switch(subject, arms, else_body, *span),
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.diags.push(loop_control_outside(syntax::KW_BREAK, *span));
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(syntax::KW_CONTINUE, *span));
                }
            }
            Stmt::Loop(inner, _) => {
                self.loop_depth += 1;
                self.check_block(inner, true);
                self.loop_depth -= 1;
            }
            Stmt::Unsafe(inner, _) => {
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(inner, true);
                self.in_unsafe = prev;
            }
        }
    }

    fn check_if(&mut self, ifs: &mut IfStmt) {
        let before = self.moved.clone();
        let mut after = before.clone();
        let bindings = self.check_condition_with_bindings(&mut ifs.cond);
        self.push_scope();
        for (name, ty) in bindings {
            self.declare(
                &name,
                ifs.span,
                LocalInfo {
                    ty,
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                },
            );
        }
        self.check_block(&mut ifs.then_body, false);
        self.pop_scope();
        for (k, v) in self.moved.drain() {
            after.entry(k).or_insert(v);
        }
        self.moved = before.clone();
        match &mut ifs.else_branch {
            None => {}
            Some(ElseBranch::Else(else_body)) => {
                self.check_block(else_body, true);
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
            }
            Some(ElseBranch::ElseIf(next)) => {
                self.check_if(next);
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
            }
        }
        self.moved = after;
    }

    fn check_condition_with_bindings(&mut self, cond: &mut Expr) -> HashMap<String, Type> {
        match cond {
            Expr::PatternTest { subject, pattern, span } => {
                self.check_pattern_test(subject, pattern, *span)
            }
            Expr::Binary(BinOp::Eq, l, r, span) => {
                let subj_name = match l.as_ref() {
                    Expr::Ident(n, _) => Some(n.clone()),
                    _ => None,
                };
                if let Some(lt) = self.infer(l) {
                    if let Some(pattern) =
                        self.eq_unit_variant_pattern(l, r, subj_name.as_deref(), &lt)
                    {
                        return self.validate_pattern(&lt, &pattern, *span);
                    }
                }
                self.require_bool(cond, "a condition");
                HashMap::new()
            }
            Expr::Binary(BinOp::And, l, r, _) => {
                let left_bindings = self.check_condition_with_bindings(l);
                let mut right_bindings = self.check_condition_with_bindings(r);
                left_bindings.into_iter().for_each(|(k, v)| {
                    right_bindings.entry(k).or_insert(v);
                });
                right_bindings
            }
            _ => {
                self.require_bool(cond, "a condition");
                HashMap::new()
            }
        }
    }

    fn check_switch(
        &mut self,
        subject: &mut Expr,
        arms: &mut [crate::ast::SwitchArm],
        else_body: &mut Option<Vec<Stmt>>,
        span: Span,
    ) {
        let subj_ty = self.infer(subject);
        let subj_name = match &*subject {
            Expr::Ident(n, _) => Some(n.clone()),
            _ if subj_ty.as_ref().is_some_and(|t| t.is_fallible()) => {
                Some(syntax::KW_IT.to_string())
            }
            _ => None,
        };
        let it_scope = subj_name.as_deref() == Some(syntax::KW_IT);
        if it_scope {
            self.push_scope();
            if let Some(st) = subj_ty.clone() {
                self.declare(
                    syntax::KW_IT,
                    span,
                    LocalInfo {
                        ty: st,
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                    },
                );
            }
        }
        let all_pattern = subj_ty.is_some()
            && !arms.is_empty()
            && arms.iter().all(|a| {
                self.switch_arm_pattern(
                    &a.cond,
                    subj_name.as_deref(),
                    subj_ty.as_ref().unwrap(),
                )
                .is_some()
            });
        let mut covered = HashSet::new();
        let move_before = self.moved.clone();
        let mut move_after = move_before.clone();
        for arm in arms.iter_mut() {
            self.moved = move_before.clone();
            if all_pattern {
                if let Some(ref st) = subj_ty {
                    let Some(pattern) =
                        self.switch_arm_pattern(&arm.cond, subj_name.as_deref(), st)
                    else {
                        continue;
                    };
                    let pspan = pattern.span();
                    if let Some(variant) = pattern_variant_name(&pattern) {
                        if covered.contains(&variant) {
                            self.diags.push(Diagnostic::lint(
                                "L0301",
                                format!("arm `{}` is unreachable — that case is already handled", variant),
                                "every earlier arm already covers this pattern".to_string(),
                                "remove this arm or merge it with the one above".to_string(),
                                Some(pspan),
                            ));
                        } else {
                            covered.insert(variant);
                        }
                    }
                    let bindings = self.validate_pattern(st, &pattern, pspan);
                    self.mark_pattern_subject_moved(subject, &bindings);
                    self.push_scope();
                    for (name, ty) in bindings {
                        self.declare(
                            &name,
                            pspan,
                            LocalInfo {
                                ty,
                                mutable: false,
                                param_conv: None,
                                decl_loop_depth: self.loop_depth,
                            },
                        );
                    }
                    self.check_block(&mut arm.body, false);
                    self.pop_scope();
                    for (k, v) in self.moved.drain() {
                        move_after.entry(k).or_insert(v);
                    }
                    continue;
                }
            }
            let bindings = self.check_condition_with_bindings(&mut arm.cond);
            if bindings.is_empty() {
                self.require_bool(&mut arm.cond, "a `switch` arm's condition");
                self.check_block(&mut arm.body, true);
            } else {
                self.push_scope();
                for (name, ty) in bindings {
                    self.declare(
                        &name,
                        arm.cond.span(),
                        LocalInfo {
                            ty,
                            mutable: false,
                            param_conv: None,
                            decl_loop_depth: self.loop_depth,
                        },
                    );
                }
                self.check_block(&mut arm.body, false);
                self.pop_scope();
            }
            for (k, v) in self.moved.drain() {
                move_after.entry(k).or_insert(v);
            }
        }
        if it_scope {
            self.pop_scope();
        }
        if all_pattern {
            if let Some(st) = subj_ty {
                if let Some(missing) = missing_pattern_coverage(&st, &covered, self.registry) {
                    if else_body.is_none() {
                        self.diags.push(Diagnostic::error(
                            "E0307",
                            format!("`switch` doesn't cover every case — missing: {}", missing.join(", ")),
                            "when every arm is a pattern test, each variant must appear once".to_string(),
                            format!("add an arm for: {}", missing.join(", ")),
                            Some(span),
                        ));
                    }
                }
            }
        } else if else_body.is_none() {
            self.diags.push(Diagnostic::error(
                "E0003",
                "this `switch` needs an `else` branch".to_string(),
                "mixed condition arms (or non-pattern arms) must always have a fallback (S24)".to_string(),
                format!("add `{} {{ ... }};` after the last arm", syntax::KW_ELSE),
                Some(span),
            ));
        }
        if let Some(body) = else_body {
            self.moved = move_before.clone();
            self.check_block(body, true);
            for (k, v) in self.moved.drain() {
                move_after.entry(k).or_insert(v);
            }
        }
        self.moved = move_after;
    }

    fn resolve_type(&self, ty: Type) -> Type {
        match ty {
            Type::Named(n)
                if self.m9.is_trait_name(&n) && !self.registry.contains(&n) =>
            {
                Type::TraitObject(n)
            }
            Type::List(inner) => Type::List(Box::new(self.resolve_type(*inner))),
            Type::Apply { name, args } => Type::Apply {
                name,
                args: args.into_iter().map(|a| self.resolve_type(a)).collect(),
            },
            Type::Option(inner) => Type::Option(Box::new(self.resolve_type(*inner))),
            Type::Map { key, value } => Type::Map {
                key: Box::new(self.resolve_type(*key)),
                value: Box::new(self.resolve_type(*value)),
            },
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(self.resolve_type(*ok)),
                err: Box::new(self.resolve_type(*err)),
            },
            other => other,
        }
    }

    fn type_param_has_bound(&self, ty: &Type, bound: &str) -> bool {
        match ty {
            Type::Named(n) => self
                .type_param_scope
                .iter()
                .find(|p| p.name == *n)
                .is_some_and(|p| p.bounds.iter().any(|b| b == bound)),
            _ => false,
        }
    }

    fn struct_subst(&self, type_name: &str, type_args: &[Type]) -> HashMap<String, Type> {
        let params = self
            .m9
            .struct_params
            .get(type_name)
            .cloned()
            .unwrap_or_default();
        if params.is_empty() {
            return HashMap::new();
        }
        if type_args.is_empty() {
            params
                .iter()
                .map(|p| (p.name.clone(), Type::Named(p.name.clone())))
                .collect()
        } else {
            params
                .iter()
                .zip(type_args.iter())
                .map(|(p, a)| (p.name.clone(), a.clone()))
                .collect()
        }
    }

    fn check_binding(&mut self, b: &mut Binding) {
        let mut annot_valid = true;
        let saved_expected = self.expected_type.clone();
        if let (Some(ty), Some(span)) = (&mut b.ty, b.ty_span) {
            let t = self.resolve_type(ty.clone());
            *ty = t.clone();
            self.expected_type = Some(t.clone());
            let before = self.diags.len();
            self.check_declared_type(&t, span);
            if self.diags.len() > before {
                annot_valid = false;
            }
        }
        if let Expr::Ident(n, nspan) = &mut b.init {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() {
                    if matches!(info.param_conv, Some(AccessConvention::Read))
                        && is_cloneable(&info.ty, self.registry, self.structs)
                    {
                        let span = *nspan;
                        let old = std::mem::replace(&mut b.init, Expr::Absent(span));
                        b.init = Expr::MethodCall {
                            receiver: Box::new(old),
                            method: "clone".to_string(),
                            method_span: span,
                            args: Vec::new(),
                            recv_type: None,
                        };
                    } else if matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Mutate)
                    ) {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!("`{}` is only borrowed here, so it can't be moved", n),
                            "this function reads the value but doesn't own it".to_string(),
                            format!(
                                "copy it instead: `{} {} = {}.clone();`",
                                if b.mutable {
                                    syntax::KW_VAR
                                } else {
                                    syntax::KW_VAL
                                },
                                b.name,
                                n
                            ),
                            Some(*nspan),
                        ));
                    }
                }
            }
        }
        let saved_esc = self.lambda_escapes;
        let saved_bind = self.lambda_binding.clone();
        if matches!(&b.init, Expr::Lambda(_)) {
            self.lambda_escapes = true;
            self.lambda_binding = Some(b.name.clone());
        }
        let it = self.infer(&mut b.init);
        self.lambda_escapes = saved_esc;
        self.lambda_binding = saved_bind;
        self.expected_type = saved_expected;

        if let Expr::Lambda(lam) = &b.init {
            if lam.meta.escapes {
                for name in &lam.meta.mut_captures {
                    self.lambda_mut_borrow_stack
                        .last_mut()
                        .unwrap()
                        .insert(name.clone());
                }
            }
        }

        // `val a = b;` moves `b` when the type isn't a scalar (M2 model:
        // assignment moves). Borrowed parameters can't be moved at all.
        if let Expr::Ident(n, nspan) = &b.init {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() {
                    if info.param_conv.is_none() {
                        self.mark_moved(n.clone(), *nspan);
                    }
                }
            }
        }

        let final_ty = match (&b.ty, it) {
            (Some(_), Some(actual)) if !annot_valid => actual,
            (Some(annot), Some(actual)) => {
                let annot = self.resolve_type(annot.clone());
                let actual = self.resolve_type(actual.clone());
                if is_u8_ty(&annot) && actual == Type::Int {
                    if let Expr::Int(n, span) = b.init {
                        if !(0..=255).contains(&n) {
                            self.diags.push(u8_range_error(span));
                        }
                    }
                } else if annot != actual {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`{}` says it holds {}, but the value is {}",
                            b.name,
                            annot.show(),
                            actual.show()
                        ),
                        "the type written after `:` must match the value".to_string(),
                        type_fix_hint(&annot, &actual),
                        Some(b.init.span()),
                    ));
                }
                annot
            }
            (Some(annot), None) => self.resolve_type(annot.clone()),
            (None, Some(actual)) => actual,
            (None, None) => Type::Int, // an error was already reported
        };
        if b.ty.is_none() {
            b.ty = Some(final_ty.clone());
        }
        if b.is_comptime {
            let globals = self.current_ct_globals();
            match crate::comptime::evaluate_owned(
                &b.init,
                self.ct_funcs,
                self.ct_externs,
                self.ct_base_dir,
                &globals,
            ) {
                Ok(v) => {
                    b.ct = Some(v.clone());
                    self.ct_scopes
                        .last_mut()
                        .unwrap()
                        .insert(b.name.clone(), v);
                }
                Err(d) => self.diags.push(d),
            }
        }
        self.declare(
            &b.name,
            b.name_span,
            LocalInfo {
                ty: final_ty,
                mutable: b.mutable && !b.is_comptime,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
            },
        );
    }

    // --- expressions ------------------------------------------------------

    fn require_bool(&mut self, e: &mut Expr, what: &str) {
        if let Some(t) = self.infer(e) {
            if t != Type::Bool {
                self.diags.push(Diagnostic::error(
                    "E0110",
                    format!("{} must be {}, but this is {}", what, Type::Bool.show(), t.show()),
                    "the program needs a clear yes or no here".to_string(),
                    "compare the value to something, e.g. `x > 0` or `name == \"ok\"`"
                        .to_string(),
                    Some(e.span()),
                ));
            }
        }
    }

    fn unknown_name(&mut self, name: &str, span: Span) {
        let mut fix = format!(
            "declare it first: `{} {} = ...;`",
            syntax::KW_VAL,
            name
        );
        let mut best: Option<(String, usize)> = None;
        let candidates: Vec<String> = self
            .scopes
            .iter()
            .flat_map(|s| s.keys().cloned())
            .chain(self.consts.keys().cloned())
            .collect();
        for cand in candidates {
            let d = edit_distance(name, &cand);
            if d <= 2 && best.as_ref().map_or(true, |(_, bd)| d < *bd) {
                best = Some((cand, d));
            }
        }
        if let Some((cand, _)) = best {
            fix = format!("did you mean `{}`?", cand);
        }
        self.diags.push(Diagnostic::error(
            "E0107",
            format!("nothing named `{}` exists here", name),
            "a name must be declared before it's used".to_string(),
            fix,
            Some(span),
        ));
    }

    /// Whether `e` may be returned through `-> view T` (reference-safe).
    fn expr_ok_for_view_return(&self, e: &Expr) -> bool {
        match e {
            Expr::Ident(name, _) => {
                if self.consts.contains_key(name) {
                    return true;
                }
                if let Some(info) = self.lookup(name) {
                    return info.ty.is_scalar() || info.param_conv.is_some();
                }
                false
            }
            _ => false,
        }
    }

    fn mark_moved(&mut self, name: String, span: Span) {
        if let Some(info) = self.lookup(&name) {
            if info.decl_loop_depth < self.loop_depth {
                self.diags.push(Diagnostic::error(
                    "E0121",
                    format!("`{}` is given away inside a loop that may run again", name),
                    "after a value is given away it's gone, but the next time around the loop would need it again".to_string(),
                    format!("give away a copy instead: `{}.clone()`", name),
                    Some(span),
                ));
                return;
            }
        }
        self.moved.insert(name, span);
    }

    /// `x = y;` / `val a = y;` / `return y;` where `y` is a plain name of a
    /// non-scalar type gives the value away (assignment moves, see C1).
    fn note_move_if_direct_ident(&mut self, e: &Expr) {
        if let Expr::Ident(n, span) = e {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() && info.param_conv.is_none() {
                    self.mark_moved(n.clone(), *span);
                }
            }
        }
    }

    fn infer_name_or(&mut self, e: &mut Expr, fallback: &str) -> String {
        self.infer(e).map(|t| t.name()).unwrap_or_else(|| fallback.to_string())
    }

    /// Infer and check an expression. Returns None when a problem was
    /// already reported (avoids error cascades).
    ///
    /// This wrapper owns two rules that depend on *where* the expression
    /// appears (`borrow_ctx`):
    ///  - a struct-field read in owning position is rewritten to `.clone()`
    ///    so the generated Rust never moves a field out of its struct;
    ///  - a `-> view` call result may only be read in place (borrow
    ///    positions); storing or giving it away is E0206.
    fn infer(&mut self, e: &mut Expr) -> Option<Type> {
        let borrowed = std::mem::take(&mut self.borrow_ctx);
        let ty = self.infer_inner(e);
        if !borrowed {
            if self.is_view_call(e) {
                self.diags.push(Diagnostic::error(
                    "E0206",
                    "this borrowed view can only be read where it is".to_string(),
                    "a `view` result points into someone else's value, so it can't be stored or given away".to_string(),
                    "read it in place (print it, compare a field, call a method on it), or call a function that returns an owned value".to_string(),
                    Some(e.span()),
                ));
                return None;
            }
            if let Some(t) = &ty {
                if !type_is_copy(t) && field_read_to_clone(e, self.registry, self.imports) {
                    let span = e.span();
                    let old = std::mem::replace(e, Expr::Absent(span));
                    *e = Expr::MethodCall {
                        receiver: Box::new(old),
                        method: "clone".to_string(),
                        method_span: span,
                        args: Vec::new(),
                        recv_type: None,
                    };
                }
            }
        }
        ty
    }

    /// Whether `e` is a call to something declared `-> view T` (its Rust
    /// value is a reference).
    fn is_view_call(&self, e: &Expr) -> bool {
        match e {
            Expr::Call(c) => self
                .funcs
                .get(&c.name)
                .is_some_and(|s| s.is_view_return),
            Expr::MethodCall {
                recv_type: Some(t),
                method,
                ..
            } => self
                .registry
                .method(t, method)
                .is_some_and(|m| m.is_view_return),
            Expr::MethodCall { receiver, method, .. } => {
                // Cross-file call through an import alias.
                if let Expr::Ident(alias, _) = receiver.as_ref() {
                    if let (Some(&idx), Some(mods)) = (self.imports.get(alias), self.modules) {
                        return mods[idx]
                            .funcs
                            .get(method)
                            .is_some_and(|s| s.is_view_return);
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn infer_inner(&mut self, e: &mut Expr) -> Option<Type> {
        match e {
            Expr::Int(_, _) => Some(Type::Int),
            Expr::Float(_, _) => Some(Type::Float),
            Expr::Bool(_, _) => Some(Type::Bool),
            Expr::Str(parts, _) => {
                for p in parts.iter_mut() {
                    if let StrPart::Interp(inner) = p {
                        // Interpolation borrows (`.jet_show()`); never moves.
                        self.borrow_ctx = true;
                        let t = self.infer(inner);
                        if let Some(t) = t {
                            if !is_printable(&t, self.registry) {
                                self.diags.push(Diagnostic::error(
                                    "E0112",
                                    format!("{} can't be put into text yet", t.show()),
                                    "interpolation shows printable values".to_string(),
                                    "show one of its parts instead".to_string(),
                                    Some(inner.span()),
                                ));
                            }
                        }
                    }
                }
                Some(Type::String)
            }
            Expr::Ident(name, span) => {
                if let Some(moved_at) = self.moved.get(name).copied() {
                    let (line_note, _) = (moved_at, ());
                    let _ = line_note;
                    self.diags.push(Diagnostic::error(
                        "E0121",
                        format!("`{}` was given away earlier, so it can't be used here", name),
                        "after a value moves somewhere else, the old name no longer holds it"
                            .to_string(),
                        format!("give away a copy instead (`{}.clone()`) where it moved", name),
                        Some(*span),
                    ));
                    self.moved.remove(name); // report once
                    return None;
                }
                if let Some(info) = self.lookup(name) {
                    return Some(info.ty.clone());
                }
                if let Some(t) = self.consts.get(name) {
                    return Some(t.clone());
                }
                if let Some(sig) = self.funcs.get(name) {
                    return Some(func_sig_to_fn_type(sig));
                }
                self.unknown_name(name, *span);
                None
            }
            Expr::Char(_, _) => Some(Type::Char),
            Expr::ListLit(elems, span) => self.infer_list_lit(elems, *span),
            Expr::MapLit(entries, span) => self.infer_map_lit(entries, *span),
            Expr::Index { base, index, span, kind } => self.infer_index(base, index, span, kind),
            Expr::Slice { base, start, end, span } => self.infer_slice(base, start, end, *span),
            Expr::Call(call) => {
                let span = call.name_span;
                match self.check_call(call, true) {
                    Some(Some(t)) => Some(t),
                    Some(None) => {
                        self.diags.push(Diagnostic::error(
                            "E0116",
                            format!("`{}` doesn't hand back a value", call.name),
                            "only calls that declare `-> Type` can be used as a value"
                                .to_string(),
                            format!(
                                "call `{}` on its own line, or give it a return type",
                                call.name
                            ),
                            Some(span),
                        ));
                        None
                    }
                    None => None,
                }
            }
            Expr::Unary(op, inner, span) => {
                let t = self.infer(inner)?;
                match op {
                    UnOp::Neg => {
                        if matches!(t, Type::Int | Type::Float) {
                            Some(t)
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0109",
                                format!("`-` needs a number, but this is {}", t.show()),
                                "only Int and Float values can be negated".to_string(),
                                "use a number here".to_string(),
                                Some(*span),
                            ));
                            None
                        }
                    }
                    UnOp::Not => {
                        if t == Type::Bool {
                            Some(Type::Bool)
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0109",
                                format!("`!` needs {}, but this is {}", Type::Bool.show(), t.show()),
                                "`!` flips a yes to a no and back".to_string(),
                                "compare the value to something first, e.g. `!(x > 0)`"
                                    .to_string(),
                                Some(*span),
                            ));
                            None
                        }
                    }
                }
            }
            Expr::Binary(op, lhs, rhs, span) => {
                let (op, span) = (*op, *span);
                self.infer_binary(op, lhs, rhs, span)
            }
            Expr::Deref(inner, span) => {
                if !self.in_unsafe {
                    self.diags.push(Diagnostic::error(
                        "E0208",
                        "`*` isn't allowed here".to_string(),
                        "dereferencing with `*` is only for expert code inside `unsafe`"
                            .to_string(),
                        "remove `*`, or wrap this code in `unsafe { ... }`".to_string(),
                        Some(*span),
                    ));
                }
                self.infer(inner)
            }
            Expr::Field(inner, member, span) => self.infer_field(inner, member, *span),
            Expr::MethodCall {
                receiver,
                method,
                method_span,
                args,
                recv_type,
            } => self.infer_method_call(receiver, method, *method_span, args, recv_type),
            Expr::StructLit {
                type_name,
                type_args,
                import_ns,
                fields,
                span,
                ..
            } => Some(self.check_struct_lit(
                type_name,
                type_args,
                import_ns.as_deref(),
                fields,
                *span,
            )),
            Expr::EnumLit {
                type_name,
                variant,
                args,
                span,
            } => Some(self.check_enum_lit(type_name, variant, args, *span)),
            Expr::Present(inner, _span) => {
                let t = self.infer(inner)?;
                Some(Type::Option(Box::new(t)))
            }
            Expr::Absent(span) => {
                if let Some(expected) = self.expected_type.clone() {
                    if expected.unwrap_option().is_some() {
                        Some(expected)
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0308",
                            "bare `null` needs a known optional type here".to_string(),
                            format!("`{}` only fits where a `T?` is expected (S32)", syntax::LIT_NULL),
                            "add a type annotation, or use `null` where the type is already known".to_string(),
                            Some(*span),
                        ));
                        None
                    }
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0308",
                        "bare `null` needs a known optional type here".to_string(),
                        format!("`{}` only fits where a `T?` is expected (S32)", syntax::LIT_NULL),
                        "add a type annotation, or use `null` where the type is already known".to_string(),
                        Some(*span),
                    ));
                    None
                }
            }
            Expr::PatternTest {
                subject,
                pattern,
                span,
            } => {
                self.check_pattern_test(subject, pattern, *span);
                Some(Type::Bool)
            }
            Expr::Ok(inner, span) => self.infer_ok(inner, *span),
            Expr::Err(inner, span) => self.infer_err(inner, *span),
            Expr::Try(inner, span) => self.infer_try(inner, *span),
            Expr::OrFallback {
                value,
                fallback,
                span,
                is_option,
            } => self.infer_or_fallback(value, fallback, *span, is_option),
            Expr::Lambda(lam) => {
                let expected = self.expected_type.clone();
                self.check_lambda(lam, expected.as_ref())
            }
            Expr::CallValue { callee, args, span } => self.infer_call_value(callee, args, *span),
        }
    }

    fn infer_ok(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                if payload != *ok_ty {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            syntax::LIT_OK,
                            payload.show(),
                            ok_ty.show()
                        ),
                        "the success value must match the result's value type".to_string(),
                        type_fix_hint(ok_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", syntax::LIT_OK),
            format!(
                "`{}` builds the success side of `Result<T, E>`",
                syntax::LIT_OK
            ),
            "use it in a return type, a binding annotated with `Result<...>`, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    fn infer_err(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let payload = self.infer(inner)?;
        if let Some(expected) = self.expected_type.clone() {
            if let Some((ok_ty, err_ty)) = expected.unwrap_result() {
                if payload != *err_ty {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "this `{}` holds {}, but {} was expected",
                            syntax::LIT_ERR,
                            payload.show(),
                            err_ty.show()
                        ),
                        "the failure value must match the result's error type".to_string(),
                        type_fix_hint(err_ty, &payload),
                        Some(span),
                    ));
                }
                return Some(Type::Result {
                    ok: Box::new(ok_ty.clone()),
                    err: Box::new(err_ty.clone()),
                });
            }
        }
        self.diags.push(Diagnostic::error(
            "E0404",
            format!("`{}(...)` only fits where a fallible result is expected", syntax::LIT_ERR),
            format!(
                "`{}` builds the failure side of `Result<T, E>`",
                syntax::LIT_ERR
            ),
            "use it in a return type, a binding annotated with `Result<...>`, or a call that expects one"
                .to_string(),
            Some(span),
        ));
        None
    }

    fn infer_try(&mut self, inner: &mut Box<Expr>, span: Span) -> Option<Type> {
        let inner_ty = self.infer(inner)?;
        match inner_ty {
            Type::Result { ok, err } => {
                let ret = self.ret.clone().unwrap_or(Type::Int);
                match &ret {
                    Type::Result {
                        ok: ret_ok,
                        err: ret_err,
                    } if *ret_ok == ok && *ret_err == err => Some((*ok).clone()),
                    Type::Result { err: ret_err, .. } => {
                        self.diags.push(Diagnostic::error(
                            "E0403",
                            format!(
                                "`{}` can't pass a {} error into a function that returns {}",
                                syntax::OP_TRY_SUFFIX,
                                err.show(),
                                ret_err.show()
                            ),
                            "the error type must match exactly — there's no conversion in v1"
                                .to_string(),
                            format!(
                                "handle the failure here with `== {}`, or change the return type to match",
                                syntax::LIT_ERR
                            ),
                            Some(span),
                        ));
                        None
                    }
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0403",
                            format!(
                                "`{}` only works inside a function that returns a fallible result",
                                syntax::OP_TRY_SUFFIX
                            ),
                            "propagation early-returns the failure to the caller".to_string(),
                            format!(
                                "add `-> Result<..., {}>` to this function, or handle the result with `{}`",
                                err.name(),
                                syntax::OP_OR_FALLBACK
                            ),
                            Some(span),
                        ));
                        None
                    }
                }
            }
            Type::Option(ref inner) => {
                let ret = self.ret.clone().unwrap_or(Type::Int);
                if let Type::Option(ret_inner) = &ret {
                    if **ret_inner == **inner {
                        return Some((**inner).clone());
                    }
                }
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` on `{}` needs a function that returns the same optional type",
                        syntax::OP_TRY_SUFFIX,
                        inner_ty.name()
                    ),
                    "propagation passes `null` back to the caller".to_string(),
                    format!(
                        "add `-> {}` to this function, or handle it with `{}`",
                        inner_ty.name(),
                        syntax::OP_OR_FALLBACK
                    ),
                    Some(span),
                ));
                None
            }
            other => {
                self.diags.push(Diagnostic::error(
                    "E0403",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        syntax::OP_TRY_SUFFIX,
                        other.show()
                    ),
                    "postfix `?` unwraps success or returns early with the failure".to_string(),
                    format!(
                        "call something that returns `Result<T, E>` or `T?`, or remove `{}`",
                        syntax::OP_TRY_SUFFIX
                    ),
                    Some(span),
                ));
                None
            }
        }
    }

    fn infer_or_fallback(
        &mut self,
        value: &mut Box<Expr>,
        fallback: &mut OrFallback,
        span: Span,
        is_option: &mut bool,
    ) -> Option<Type> {
        let val_ty = self.infer(value)?;
        *is_option = matches!(val_ty, Type::Option(_));
        let payload = match &val_ty {
            Type::Result { ok, .. } if !*is_option => (**ok).clone(),
            Type::Option(inner) if *is_option => (**inner).clone(),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0405",
                    format!(
                        "`{}` only works on a fallible value, not {}",
                        syntax::OP_OR_FALLBACK,
                        other.show()
                    ),
                    "the left side must be a `Result` or optional value".to_string(),
                    format!(
                        "call something that can fail, then write `... {} fallback`",
                        syntax::OP_OR_FALLBACK
                    ),
                    Some(span),
                ));
                return None;
            }
        };
        match fallback {
            OrFallback::Value(e) => {
                // Infer in place: sema rewrites inside the fallback (index
                // kinds, S25 distribution, field clones) must reach codegen.
                let ft = self.infer(e)?;
                if ft != payload {
                    self.diags.push(Diagnostic::error(
                        "E0405",
                        format!(
                            "the fallback is {}, but the success value is {}",
                            ft.show(),
                            payload.show()
                        ),
                        format!(
                            "both sides of `{}` must be the same type",
                            syntax::OP_OR_FALLBACK
                        ),
                        type_fix_hint(&payload, &ft),
                        Some(e.span()),
                    ));
                }
                Some(payload)
            }
            OrFallback::Return(ret_expr, ret_span) => {
                let ret = self.ret.clone();
                match (&ret, ret_expr) {
                    (Some(rt), Some(e)) => {
                        let saved = self.expected_type.clone();
                        self.expected_type = Some(rt.clone());
                        let et = self.infer(e);
                        self.expected_type = saved;
                        if let Some(et) = et {
                            let espan = e.span();
                            self.check_type_assignable(rt, &et, espan);
                        }
                    }
                    (Some(_), None) => {}
                    (None, _) => {
                        self.diags.push(Diagnostic::error(
                            "E0405",
                            format!("`{} return` can't leave this function early", syntax::OP_OR_FALLBACK),
                            "a bare return needs a function with a return type".to_string(),
                            "add `-> Type` to the function, or give a fallback value instead"
                                .to_string(),
                            Some(*ret_span),
                        ));
                    }
                }
                Some(payload)
            }
            OrFallback::Panic { name_span, args } => {
                let mut call = Call {
                    name: syntax::BUILTIN_PANIC.to_string(),
                    name_span: *name_span,
                    args: std::mem::take(args),
                };
                self.check_panic_call(&mut call);
                *args = call.args;
                Some(payload)
            }
        }
    }

    fn infer_fallible_stmt(&mut self, expr: &mut Expr) -> Option<Type> {
        match expr {
            Expr::Call(call) => match self.check_call(call, false) {
                Some(Some(t)) => Some(t),
                _ => None,
            },
            Expr::MethodCall { .. } => self.infer(expr),
            _ => self.infer(expr),
        }
    }

    fn infer_call_value(
        &mut self,
        callee: &mut Box<Expr>,
        args: &mut [crate::ast::CallArg],
        span: Span,
    ) -> Option<Type> {
        let callee_ty = self.infer(callee)?;
        let Type::Fn { params, ret } = callee_ty.clone() else {
            self.diags.push(Diagnostic::error(
                "E0803",
                format!("this is {}, not a function", callee_ty.show()),
                "only a function value can be called with `(…)`".to_string(),
                "call a defined `fn` by name, or store a lambda in a binding first".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if args.len() != params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "this function wants {} argument{}, got {}",
                    params.len(),
                    if params.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "every argument must match a parameter".to_string(),
                "check how many values this function expects".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(param_ty) = params.get(i) {
                let saved = self.expected_type.clone();
                self.expected_type = Some(param_ty.clone());
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved;
                if let Some(got) = got {
                    if got != *param_ty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "argument {} should be {}, not {}",
                                i + 1,
                                param_ty.show(),
                                got.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(param_ty, &got),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            } else {
                self.infer(&mut arg.expr);
            }
        }
        ret.map(|r| *r)
    }

    fn check_lambda(&mut self, lam: &mut Lambda, expected: Option<&Type>) -> Option<Type> {
        let (exp_params, exp_ret) = match expected {
            Some(Type::Fn { params, ret }) => (Some(params.as_slice()), ret.as_ref()),
            _ => (None, None),
        };

        if let Some(ep) = exp_params {
            if lam.params.len() != ep.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "this lambda has {} parameter{}, but {} {} expected",
                        lam.params.len(),
                        if lam.params.len() == 1 { "" } else { "s" },
                        ep.len(),
                        if ep.len() == 1 { "was" } else { "were" }
                    ),
                    "parameter count must match the function type at this spot".to_string(),
                    "add or remove parameters, or fix the surrounding type".to_string(),
                    Some(lam.span),
                ));
            }
        }

        let mut param_types = Vec::new();
        for (i, p) in lam.params.iter_mut().enumerate() {
            let pty = if let Some(ty) = &p.ty {
                self.check_declared_type(ty, p.ty_span.unwrap_or(p.name_span));
                ty.clone()
            } else if let Some(ep) = exp_params.and_then(|ps| ps.get(i)) {
                ep.clone()
            } else {
                self.diags.push(Diagnostic::error(
                    "E0801",
                    format!("tell me the type of `{}`", p.name),
                    "this lambda parameter has no type to go on".to_string(),
                    format!("write `({}: Int) => …` (or whatever type fits)", p.name),
                    Some(p.name_span),
                ));
                Type::Int
            };
            param_types.push(pty);
        }

        if let Some(binding) = &self.lambda_binding {
            if lambda_body_refs_name(&lam.body, binding) {
                self.diags.push(Diagnostic::error(
                    "E0804",
                    format!("a lambda can't call itself as `{}`", binding),
                    "short functions stored in a binding can't recurse in v1".to_string(),
                    format!(
                        "write a named `{}` instead of assigning the lambda to `{}`",
                        syntax::KW_FN,
                        binding
                    ),
                    Some(lam.span),
                ));
            }
        }

        let escapes = self.lambda_escapes;
        lam.meta.escapes = escapes;

        let param_names: HashSet<String> =
            lam.params.iter().map(|p| p.name.clone()).collect();
        let take_set: HashSet<String> = lam.take_names.iter().map(|(n, _)| n.clone()).collect();

        let mut read_caps = HashSet::new();
        let mut mut_caps = HashSet::new();
        lambda_collect_captures(&lam.body, &param_names, &mut read_caps, &mut mut_caps);

        for name in read_caps.iter().chain(mut_caps.iter()) {
            if take_set.contains(name) || param_names.contains(name) {
                continue;
            }
            if self.lookup(name).is_none() && !self.consts.contains_key(name) {
                self.unknown_name(name, lam.span);
            }
        }

        for name in &mut_caps {
            if param_names.contains(name) || take_set.contains(name) {
                continue;
            }
            if let Some(info) = self.lookup(name) {
                if !info.mutable {
                    self.diags.push(Diagnostic::error(
                        "E0111",
                        format!("`{}` can't be changed inside this lambda", name),
                        "changing a value inside a short function requires a `var` binding"
                            .to_string(),
                        format!("declare `var {}: …` instead of `val`", name),
                        Some(lam.span),
                    ));
                }
            }
        }

        lam.meta.needs_fn_mut = !mut_caps.is_empty();
        lam.meta.mut_captures = mut_caps
            .iter()
            .filter(|n| !take_set.contains(*n) && !param_names.contains(*n))
            .cloned()
            .collect();

        if escapes {
            for name in read_caps.iter().chain(mut_caps.iter()) {
                if param_names.contains(name) || take_set.contains(name) {
                    continue;
                }
                let cap_ty = self
                    .lookup(name)
                    .map(|i| i.ty.clone())
                    .or_else(|| self.consts.get(name).cloned());
                let Some(cap_ty) = cap_ty else {
                    continue;
                };
                if mut_caps.contains(name) {
                    continue; // taken by move into closure via mut borrow path
                }
                if !is_cloneable(&cap_ty, self.registry, self.structs) {
                    if !take_set.contains(name) {
                        self.diags.push(Diagnostic::error(
                            "E0802",
                            format!(
                                "`{}` can't be copied into a stored lambda",
                                name
                            ),
                            "a lambda that outlives this line must own its captures".to_string(),
                            format!(
                                "prefix the lambda with `take({})` to move `{}` in",
                                name, name
                            ),
                            Some(lam.span),
                        ));
                    }
                } else if !take_set.contains(name) {
                    lam.meta.cloned_captures.push(name.clone());
                    self.diags.push(Diagnostic::lint(
                        "L0801",
                        format!(
                            "lambda stored a copy of `{}`; write `take({})` on the lambda to move it instead",
                            name, name
                        ),
                        "a stored lambda owns its captures — clonable values are copied silently"
                            .to_string(),
                        format!(
                            "use `take({}) (…) => …` to move `{}`, or `.clone()` at the call site to copy on purpose",
                            name, name
                        ),
                        Some(lam.span),
                    ));
                }
            }
        }

        self.push_scope();
        for (p, pty) in lam.params.iter().zip(param_types.iter()) {
            self.scopes.last_mut().unwrap().insert(
                p.name.clone(),
                LocalInfo {
                    ty: pty.clone(),
                    mutable: false,
                    param_conv: None,
                    decl_loop_depth: self.loop_depth,
                },
            );
        }

        let body_ret = match &mut lam.body {
            LambdaBody::Expr(e) => self.infer(e),
            LambdaBody::Block(stmts) => {
                self.check_block(stmts, false);
                let mut last_ret = None;
                for s in stmts.iter_mut().rev() {
                    match s {
                        Stmt::Return(Some(e), _) => {
                            last_ret = self.infer(e);
                            break;
                        }
                        Stmt::Expr(e) => {
                            last_ret = self.infer_fallible_stmt(e);
                            break;
                        }
                        _ => {}
                    }
                }
                last_ret
            }
        };

        self.pop_scope();

        let ret_ty = if let Some(er) = exp_ret {
            if let Some(br) = &body_ret {
                if br != er.as_ref() {
                    self.diags.push(Diagnostic::error(
                        "E0113",
                        format!(
                            "this lambda should return {}, not {}",
                            er.show(),
                            br.show()
                        ),
                        "the lambda's return type must match what's expected here".to_string(),
                        type_fix_hint(er, br),
                        Some(lam.span),
                    ));
                }
            }
            Some((**er).clone())
        } else {
            body_ret
        };

        Some(Type::Fn {
            params: param_types,
            ret: ret_ty.map(Box::new),
        })
    }

    fn finish_builtin_method(
        &mut self,
        receiver: &Expr,
        method: &str,
        recv_ty: &Type,
        args: &mut [crate::ast::CallArg],
        span: Span,
        ret: Option<Type>,
    ) -> Option<Type> {
        if collections::builtin_needs_mut_receiver(recv_ty, method) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                let rspan = receiver.span();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, rspan));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` changes `{}`, but this method only reads it",
                                    method,
                                    syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{} {}`",
                                    syntax::KW_MUTATE,
                                    syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "`{}` must be declared with `{}` before calling `.{}()`",
                                    root, syntax::KW_VAR, method
                                ),
                                format!("declare `var {}: ...`", root),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method changes the collection".to_string(),
                            fix,
                            Some(rspan),
                        ));
                    }
                }
            }
        }
        let mut refined_ret = ret.clone();
        if let Some(expected) = collections::builtin_method_arg_types(recv_ty, method) {
            for (i, arg) in args.iter_mut().enumerate() {
                let saved_esc = self.lambda_escapes;
                if collections::is_closure_method(method) {
                    self.lambda_escapes = false;
                }
                let saved_exp = self.expected_type.clone();
                if let Some(et) = expected.get(i) {
                    self.expected_type = Some(et.clone());
                }
                let got = self.infer(&mut arg.expr);
                self.expected_type = saved_exp;
                self.lambda_escapes = saved_esc;
                if let (Some(et), Some(gt)) = (expected.get(i), got) {
                    if collections::is_closure_method(method) && i == 0 && method == "map" {
                        if let Type::Fn { ret: Some(ref r), .. } = gt {
                            if let Type::List(inner) = recv_ty {
                                refined_ret = Some(Type::List(Box::new((**r).clone())));
                                let _ = inner;
                            }
                        }
                    }
                    if method == "reduce" && i == 1 {
                        if let Type::Fn { ret: Some(ref r), .. } = gt {
                            refined_ret = Some((**r).clone());
                        }
                    }
                    if !fn_types_compatible(et, &gt) && gt != *et {
                        self.diags.push(Diagnostic::error(
                            "E0108",
                            format!(
                                "argument {} to `.{}()` should be {}, not {}",
                                i + 1,
                                method,
                                et.show(),
                                gt.show()
                            ),
                            "built-in methods need arguments of the right type".to_string(),
                            type_fix_hint(et, &gt),
                            Some(arg.expr.span()),
                        ));
                    }
                }
            }
        } else {
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
        }
        let _ = span;
        refined_ret
    }

    fn infer_list_lit(&mut self, elems: &mut [Expr], span: Span) -> Option<Type> {
        if elems.is_empty() {
            if let Some(expected) = self.expected_type.clone() {
                if let Type::List(inner) = expected {
                    return Some(Type::List(inner));
                }
            }
            self.diags.push(Diagnostic::error(
                "E0501",
                "an empty list needs a type".to_string(),
                "write `[]` only where the list type is already known, like `val xs: List<Int> = []`"
                    .to_string(),
                "add a type annotation on the binding".to_string(),
                Some(span),
            ));
            return None;
        }
        if let Some(Type::List(expected_inner)) = self.expected_type.clone() {
            if let Type::TraitObject(trait_name) = expected_inner.as_ref() {
                for e in elems.iter_mut() {
                    if let Some(t) = self.infer(e) {
                        match &t {
                            Type::Named(n) if self.m9.implements_trait(n, trait_name) => {
                                if let Expr::StructLit { as_trait, .. } = e {
                                    *as_trait = Some(trait_name.clone());
                                }
                            }
                            Type::Apply { name, .. }
                                if self.m9.implements_trait(name, trait_name) =>
                            {
                                if let Expr::StructLit { as_trait, .. } = e {
                                    *as_trait = Some(trait_name.clone());
                                }
                            }
                            _ => {
                                self.check_type_assignable(&expected_inner, &t, e.span());
                            }
                        }
                    }
                }
                return Some(Type::List(expected_inner));
            }
        }
        let mut elem_types = Vec::new();
        for e in elems.iter_mut() {
            if let Some(t) = self.infer(e) {
                elem_types.push(t);
            }
        }
        let first = elem_types.first()?.clone();
        for (i, t) in elem_types.iter().enumerate().skip(1) {
            if *t != first {
                self.diags.push(Diagnostic::error(
                    "E0504",
                    format!(
                        "this list started as `{}` but item {} is `{}`",
                        first.name(),
                        i + 1,
                        t.name()
                    ),
                    "every item in a list literal must have the same type".to_string(),
                    "make every element the same type, or build the list in steps".to_string(),
                    Some(elems[i].span()),
                ));
            }
        }
        Some(Type::List(Box::new(first)))
    }

    fn infer_map_lit(&mut self, entries: &mut [(Expr, Expr)], span: Span) -> Option<Type> {
        if entries.is_empty() {
            if let Some(expected) = self.expected_type.clone() {
                if let Type::Map { key, value } = expected {
                    return Some(Type::Map { key, value });
                }
            }
            self.diags.push(Diagnostic::error(
                "E0501",
                "an empty map needs a type".to_string(),
                "write `[:]` only where the map type is already known, like `var m: Map<String, Int> = [:]`"
                    .to_string(),
                "add a type annotation on the binding".to_string(),
                Some(span),
            ));
            return None;
        }
        let mut key_ty = None;
        let mut val_ty = None;
        for (k, v) in entries.iter_mut() {
            let Some(kt) = self.infer(k) else {
                continue;
            };
            let Some(vt) = self.infer(v) else {
                continue;
            };
            if !is_map_key_type(&kt) {
                self.diags.push(Diagnostic::error(
                    "E0502",
                    format!("`{}` can't be a map key", kt.name()),
                    "map keys must be Int, String, Bool, Char, or a payload-free enum".to_string(),
                    "use a simpler key type".to_string(),
                    Some(k.span()),
                ));
            }
            if let Some(ref fk) = key_ty {
                if kt != *fk {
                    self.diags.push(Diagnostic::error(
                        "E0504",
                        format!(
                            "this map started with `{}` keys but another key is `{}`",
                            fk.name(),
                            kt.name()
                        ),
                        "every key in a map literal must have the same type".to_string(),
                        "use the same key type throughout".to_string(),
                        Some(k.span()),
                    ));
                }
            } else {
                key_ty = Some(kt);
            }
            if let Some(ref fv) = val_ty {
                if vt != *fv {
                    self.diags.push(Diagnostic::error(
                        "E0504",
                        format!(
                            "this map started with `{}` values but another value is `{}`",
                            fv.name(),
                            vt.name()
                        ),
                        "every value in a map literal must have the same type".to_string(),
                        "use the same value type throughout".to_string(),
                        Some(v.span()),
                    ));
                }
            } else {
                val_ty = Some(vt);
            }
        }
        match (key_ty, val_ty) {
            (Some(k), Some(v)) => Some(Type::Map {
                key: Box::new(k),
                value: Box::new(v),
            }),
            _ => None,
        }
    }

    fn infer_index(
        &mut self,
        base: &mut Box<Expr>,
        index: &mut Box<Expr>,
        span: &Span,
        kind: &mut IndexKind,
    ) -> Option<Type> {
        self.borrow_ctx = true;
        let base_ty = self.infer(base)?;
        let idx_ty = self.infer(index)?;
        match &base_ty {
            Type::List(inner) => {
                *kind = IndexKind::List;
                if idx_ty != Type::Int {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "list indexes must be {}, not {}",
                            Type::Int.show(),
                            idx_ty.show()
                        ),
                        "count positions with a whole number starting at 0".to_string(),
                        "use an Int index, like `items[0]`".to_string(),
                        Some(index.span()),
                    ));
                }
                Some((**inner).clone())
            }
            Type::Map { key, value } => {
                *kind = IndexKind::Map;
                if idx_ty != **key {
                    self.diags.push(Diagnostic::error(
                        "E0505",
                        format!(
                            "this map holds keys of type {}, not {}",
                            key.show(),
                            idx_ty.show()
                        ),
                        "the key in `map[key]` must match the map's key type".to_string(),
                        format!("use a {} key here", key.name()),
                        Some(index.span()),
                    ));
                }
                Some((**value).clone())
            }
            Type::String => {
                self.diags.push(Diagnostic::error(
                    "E0503",
                    "strings aren't indexed with `[ ]`".to_string(),
                    "text is counted in characters — walk them with `.chars()` or take a piece with `.slice(start..end)`".to_string(),
                    "e.g. `for c in s.chars()` or `s.slice(0..2)`".to_string(),
                    Some(*span),
                ));
                None
            }
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("only lists and maps can be indexed, not {}", base_ty.show()),
                    "use `[ ]` on a `List` or `Map` value".to_string(),
                    "check the value before `[`".to_string(),
                    Some(*span),
                ));
                None
            }
        }
    }

    fn infer_slice(
        &mut self,
        base: &mut Box<Expr>,
        start: &mut Box<Expr>,
        end: &mut Box<Expr>,
        span: Span,
    ) -> Option<Type> {
        if self.loop_depth > 0 {
            self.diags.push(Diagnostic::lint(
                "L0501",
                "slicing inside a loop copies every time".to_string(),
                "each slice makes a fresh copy of the range — that adds up in a loop".to_string(),
                "build indices outside the loop, or collect into one list".to_string(),
                Some(span),
            ));
        }
        self.borrow_ctx = true;
        let base_ty = self.infer(base)?;
        for e in [start.as_mut(), end.as_mut()] {
            let t = self.infer(e)?;
            if t != Type::Int {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("slice bounds must be {}, not {}", Type::Int.show(), t.show()),
                    "both ends of `a..b` must be whole numbers (S22, inclusive)".to_string(),
                    "use Int positions".to_string(),
                    Some(e.span()),
                ));
            }
        }
        match base_ty {
            Type::List(inner) => Some(Type::List(inner)),
            Type::String => Some(Type::String),
            other => {
                self.diags.push(Diagnostic::error(
                    "E0505",
                    format!("only lists and strings can be sliced, not {}", other.show()),
                    "slicing copies a range (S40)".to_string(),
                    "use `xs[a..b]` on a list or `s.slice(a..b)` on text".to_string(),
                    Some(span),
                ));
                None
            }
        }
    }

    fn infer_field(&mut self, inner: &mut Box<Expr>, member: &str, span: Span) -> Option<Type> {
        if member == "clone" {
            return self.infer(inner);
        }
        if let Expr::Ident(root, _) = &**inner {
            if root == syntax::FOREIGN_OS && member == "environ" {
                self.diags.push(Diagnostic::error(
                    "E0039",
                    "`os.environ` is written `env.get` in Jet".to_string(),
                    "environment access lives in the `std.env` module".to_string(),
                    "import `std.env as env` and call `env.get(name)`".to_string(),
                    Some(span),
                ));
                return None;
            }
        }
        if let Expr::Ident(alias, alias_span) = &**inner {
            if let Some(module) = self.std_imports.get(alias).cloned() {
                return self.infer_std_field(&module, member, *alias_span, span);
            }
        }
        if let Expr::Ident(type_name, _) = &**inner {
            if type_name == "Json" {
                if let Some(ret) = self.check_std_json_lit(member, &mut [], span) {
                    return Some(ret);
                }
            }
            if self.registry.enum_variants(type_name).is_some() {
                let mut empty = Vec::new();
                return Some(self.check_enum_lit(type_name, member, &mut empty, span));
            }
        }
        self.borrow_ctx = true;
        let t = self.infer(inner)?;
        if let Type::Named(type_name) = &t {
            if let Some(fty) = std_struct_field(type_name, member) {
                return Some(fty);
            }
            if let Some(owner_mod) = self.struct_owner_module(type_name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, type_name) {
                    for (fname, _, fty, is_ref, _) in fields {
                        if fname == member {
                            if *is_ref {
                                return None;
                            }
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, type_name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            }
                            return Some(fty.clone());
                        }
                    }
                    let field_names: Vec<String> =
                        fields.iter().map(|(n, ..)| n.clone()).collect();
                    let mut fix = format!("check the field names on `{}`", type_name);
                    if let Some(suggest) = suggest_field(member, &field_names) {
                        fix = format!("did you mean `{}`?", suggest);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0302",
                        format!("`{}` has no field `{}`", type_name, member),
                        "field access only works on names declared in the struct".to_string(),
                        fix,
                        Some(span),
                    ));
                    return None;
                }
            }
        }
        if let Type::Apply { name, args } = &t {
            if let Some(owner_mod) = self.struct_owner_module(name, None) {
                if let Some(fields) = self.struct_fields_of(owner_mod, name) {
                    let subst = self.struct_subst(name, args);
                    for (fname, _, fty, is_ref, _) in fields {
                        if fname == member {
                            if *is_ref {
                                return None;
                            }
                            if owner_mod != self.module_idx
                                && !self.field_is_pub_in(owner_mod, name, member)
                            {
                                self.diags.push(private_item(member, span));
                                return None;
                            }
                            return Some(self.m9.instantiate_type(fty, &subst));
                        }
                    }
                    let field_names: Vec<String> =
                        fields.iter().map(|(n, ..)| n.clone()).collect();
                    let mut fix = format!("check the field names on `{}`", name);
                    if let Some(suggest) = suggest_field(member, &field_names) {
                        fix = format!("did you mean `{}`?", suggest);
                    }
                    self.diags.push(Diagnostic::error(
                        "E0302",
                        format!("`{}` has no field `{}`", name, member),
                        "field access only works on names declared in the struct".to_string(),
                        fix,
                        Some(span),
                    ));
                    return None;
                }
            }
        }
        self.diags.push(Diagnostic::error(
            "E0302",
            format!("`.{}` only works on struct values", member),
            "enums and other values use methods or pattern tests instead".to_string(),
            format!("use a struct value before `.{}`", member),
            Some(span),
        ));
        None
    }

    fn infer_method_call(
        &mut self,
        receiver: &mut Box<Expr>,
        method: &str,
        span: Span,
        args: &mut [crate::ast::CallArg],
        recv_type_out: &mut Option<String>,
    ) -> Option<Type> {
        if method == "clone" {
            self.borrow_ctx = true;
            return self.infer(receiver);
        }
        if let Expr::Ident(root, _) = &**receiver {
            if root == "File" && method == syntax::FOREIGN_OPEN {
                self.diags.push(Diagnostic::error(
                    "E0038",
                    "`File.open` is not the M10 file API".to_string(),
                    "M10 uses whole-file helpers in `std.fs`; file handles are out of scope"
                        .to_string(),
                    "import `std.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                        .to_string(),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        }
        if let Expr::Ident(alias, alias_span) = &**receiver {
            if let Some(module) = self.std_imports.get(alias).cloned() {
                return self.infer_std_call(&module, method, *alias_span, span, args);
            }
            if let Some(&mod_idx) = self.imports.get(alias) {
                return self.infer_import_call(mod_idx, method, *alias_span, span, args);
            }
        }
        if let Expr::Ident(type_name, _) = &**receiver {
            if type_name == "Json" {
                if let Some(ret) = self.check_std_json_lit(method, args, span) {
                    return Some(ret);
                }
            }
            if let Some(variants) = self.registry.enum_variants(type_name) {
                if variants.contains_key(method) {
                    let saved: Vec<Expr> = args
                        .iter_mut()
                        .map(|a| {
                            std::mem::replace(&mut a.expr, Expr::Int(0, a.span))
                        })
                        .collect();
                    let mut enum_args: Vec<EnumLitArg> = saved
                        .into_iter()
                        .map(EnumLitArg::Positional)
                        .collect();
                    let ty = self.check_enum_lit(type_name, method, &mut enum_args, span);
                    for (a, ea) in args.iter_mut().zip(enum_args) {
                        if let EnumLitArg::Positional(e) = ea {
                            a.expr = e;
                        }
                    }
                    return Some(ty);
                }
            }
            if self.registry.method(type_name, method).is_some() {
                return self.check_static_method(type_name, method, span, args);
            }
            if let Some(ty) = builtin_type_from_ident(type_name) {
                if let Some(ret) =
                    collections::builtin_method_return(&ty, method, args.len(), true)
                {
                    return self.finish_builtin_method(receiver, method, &ty, args, span, ret);
                }
            }
        }
        self.borrow_ctx = true;
        let recv_ty = self.infer(receiver)?;
        if let Type::Named(n) = &recv_ty {
            if let Some(param) = self.type_param_scope.iter().find(|p| p.name == *n) {
                for (trait_name, info) in &self.m9.traits {
                    if let Some(msig) = info.methods.get(method) {
                        if !param.bounds.iter().any(|b| b == trait_name) {
                            self.diags.push(e0901(method, trait_name, span));
                        }
                        *recv_type_out = Some(n.clone());
                        for arg in args.iter_mut() {
                            self.infer(&mut arg.expr);
                        }
                        return msig.return_type.clone();
                    }
                }
            }
        }
        if let Some(ret) = collections::builtin_method_return(&recv_ty, method, args.len(), false) {
            return self.finish_builtin_method(receiver, method, &recv_ty, args, span, ret);
        }
        if let Type::TraitObject(trait_name) = &recv_ty {
            let sig = self.m9.traits.get(trait_name).and_then(|t| t.methods.get(method));
            if let Some(msig) = sig {
                *recv_type_out = Some(trait_name.clone());
                for arg in args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return msig.return_type.clone();
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("trait `{trait_name}` has no method `{method}`"),
                "check the method name on this trait value".to_string(),
                format!("add `fn {method}(…)` to `trait {trait_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        }
        let type_name = match &recv_ty {
            Type::Named(n) => n.clone(),
            Type::Option(inner) => match inner.as_ref() {
                Type::Named(n) => n.clone(),
                _ => {
                    self.diags.push(Diagnostic::error(
                        "E0311",
                        format!("`{}` isn't a method on this value", method),
                        "instance methods belong to struct or enum values".to_string(),
                        format!(
                            "call it on the type: `{}.{method}(...)` if it's static",
                            recv_ty.name()
                        ),
                        Some(span),
                    ));
                    for a in args.iter_mut() {
                        self.infer(&mut a.expr);
                    }
                    return None;
                }
            },
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0311",
                    format!("`{}` isn't a method on this value", method),
                    "only struct and enum values have instance methods".to_string(),
                    format!("check the spelling of `{}`", method),
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
        };
        if let Some(fields) = self.registry.struct_fields(&type_name) {
            if let Some((_, _, field_ty, _, _)) =
                fields.iter().find(|(fname, _, _, _, _)| fname == method)
            {
                if matches!(field_ty, Type::Fn { .. }) {
                    *recv_type_out = Some(type_name.clone());
                    let mut callee = Box::new(Expr::Field(
                        receiver.clone(),
                        method.to_string(),
                        span,
                    ));
                    let end = args
                        .last()
                        .map(|a| a.expr.span().end)
                        .unwrap_or(span.end);
                    let call_span = Span::new(span.start, end);
                    return self.infer_call_value(&mut callee, args, call_span);
                }
            }
        }
        let Some(msig) = self.registry.method(&type_name, method).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                "check the method name on this type".to_string(),
                format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is a static method on `{}`", method, type_name),
                "static methods belong to the type name, not a value".to_string(),
                format!("write `{}.{method}(...)` instead", type_name),
                Some(span),
            ));
        }
        *recv_type_out = Some(type_name.clone());
        // `mut self` methods change the receiver: it must be changeable,
        // free of an active `for` borrow, and not aliased by an argument.
        if msig.self_conv == Some(AccessConvention::Mutate) {
            if let Some(root) = expr_root_ident(receiver) {
                let root = root.to_string();
                if self.iter_borrowed.contains(&root) {
                    self.diags.push(collection_changed_in_loop(&root, span));
                }
                if let Some(info) = self.lookup(&root) {
                    if !info.mutable {
                        let (what, fix) = if root == syntax::KW_SELF {
                            (
                                format!(
                                    "`.{}()` changes `{}`, but this method only reads it",
                                    method,
                                    syntax::KW_SELF
                                ),
                                format!(
                                    "declare the enclosing method with `{} {}`",
                                    syntax::KW_MUTATE,
                                    syntax::KW_SELF
                                ),
                            )
                        } else {
                            (
                                format!(
                                    "`{}` must be declared with `{}` before calling `.{}()`",
                                    root,
                                    syntax::KW_VAR,
                                    method
                                ),
                                format!("declare `var {} = ...`", root),
                            )
                        };
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            what,
                            "this method changes the value it's called on".to_string(),
                            fix,
                            Some(span),
                        ));
                    }
                }
                for arg in args.iter() {
                    if matches!(&arg.expr, Expr::Ident(n, _) if *n == root) {
                        self.diags.push(aliasing_while_mut(&root, arg.expr.span()));
                    }
                }
            }
        }
        if msig.self_conv == Some(AccessConvention::Move) {
            if let Expr::Ident(n, nspan) = &**receiver {
                // A borrowed parameter can't be consumed (the generated Rust
                // would move out of a `&T`/`&mut T`).
                if let Some(info) = self.lookup(n) {
                    if !type_is_copy(&info.ty)
                        && matches!(
                            info.param_conv,
                            Some(AccessConvention::Read) | Some(AccessConvention::Mutate)
                        )
                    {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!(
                                "`{}` is only borrowed here, so `.{}()` can't consume it",
                                n, method
                            ),
                            "this function reads the value but doesn't own it".to_string(),
                            format!(
                                "call it on a copy: `{}.clone().{}(...)` — or take ownership with `{} {}: {}`",
                                n,
                                method,
                                syntax::KW_MOVE,
                                n,
                                info.ty.name()
                            ),
                            Some(*nspan),
                        ));
                    }
                }
                self.mark_moved(n.clone(), *nspan);
            }
        }
        self.check_method_args(&type_name, method, &msig, args, span)?;
        msig.return_type.clone()
    }

    fn infer_import_call(
        &mut self,
        mod_idx: usize,
        name: &str,
        alias_span: Span,
        span: Span,
        args: &mut [crate::ast::CallArg],
    ) -> Option<Type> {
        let Some(mods) = self.modules else {
            return None;
        };
        let target = &mods[mod_idx];
        if target.funcs.contains_key(name) {
            let is_pub = target.func_pub.get(name).copied().unwrap_or(false);
            if !is_pub && mod_idx != self.module_idx {
                self.diags.push(private_item(name, span));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return None;
            }
            let sig = target.funcs.get(name).unwrap().clone();
            if args.len() != sig.params.len() {
                self.diags.push(Diagnostic::error(
                    "E0104",
                    format!(
                        "`{}` expects {} argument{}, got {}",
                        name,
                        sig.params.len(),
                        if sig.params.len() == 1 { "" } else { "s" },
                        args.len()
                    ),
                    "every argument must match a parameter".to_string(),
                    format!("check the definition of `{}` in the imported file", name),
                    Some(span),
                ));
            }
            for (arg, (pconv, pty)) in args.iter_mut().zip(sig.params.iter()) {
                if matches!(pconv, AccessConvention::Read) && !pty.is_scalar() {
                    self.borrow_ctx = true;
                }
                if let Some(aty) = self.infer(&mut arg.expr) {
                    let span = arg.expr.span();
                    let reported = self.check_type_assignable(pty, &aty, span);
                    if !reported && aty != *pty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` wants {} here, but this is {}",
                                name,
                                pty.show(),
                                aty.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(pty, &aty),
                            Some(span),
                        ));
                    }
                }
                // Cross-file calls follow the same ownership rules.
                if let Expr::Ident(n, nspan) = &arg.expr {
                    match (pconv, arg.convention) {
                        (AccessConvention::Move, AccessConvention::Move) => {
                            if !pty.is_scalar() {
                                self.mark_moved(n.clone(), *nspan);
                            }
                        }
                        (AccessConvention::Move, AccessConvention::Read) => {
                            if !pty.is_scalar() {
                                arg.flags.implicit_clone = true;
                            }
                        }
                        (AccessConvention::Mutate, AccessConvention::Read) => {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!(
                                    "parameter `{}` requires `{}` at the call site",
                                    n,
                                    syntax::KW_MUTATE
                                ),
                                format!("`{}` needs to change this value while it borrows it", name),
                                format!("write `{} {}` when calling `{}`", syntax::KW_MUTATE, n, name),
                                Some(*nspan),
                            ));
                        }
                        _ => {}
                    }
                }
            }
            return sig.return_type.clone();
        }
        if target.registry.contains(name) {
            let is_pub = target.type_pub.get(name).copied().unwrap_or(false);
            if !is_pub && mod_idx != self.module_idx {
                self.diags.push(private_item(name, span));
            } else {
                self.diags.push(Diagnostic::error(
                    "E0102",
                    format!("nothing named `{}` exists in this import", name),
                    "only `pub` functions and types from the other file are reachable here"
                        .to_string(),
                    "check the spelling, or mark the item `pub` in its file".to_string(),
                    Some(span),
                ));
            }
        } else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("nothing named `{}` exists in this import", name),
                "only `pub` functions and types from the other file are reachable here".to_string(),
                "check the spelling, or mark the item `pub` in its file".to_string(),
                Some(alias_span),
            ));
        }
        for a in args.iter_mut() {
            self.infer(&mut a.expr);
        }
        None
    }

    fn infer_std_field(
        &mut self,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
    ) -> Option<Type> {
        match (module, name) {
            ("std.math", "pi" | "e") => Some(Type::Float),
            _ => {
                self.diags.push(unknown_std_item(module, name, span));
                let _ = alias_span;
                None
            }
        }
    }

    fn infer_std_call(
        &mut self,
        module: &str,
        name: &str,
        alias_span: Span,
        span: Span,
        args: &mut [crate::ast::CallArg],
    ) -> Option<Type> {
        let sig = std_fixed_sig(module, name);
        match (module, name) {
            ("std.io", "eprint") => {
                if args.len() != 1 {
                    self.diags.push(wrong_std_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.borrow_ctx = true;
                    if let Some(ty) = self.infer(&mut arg.expr) {
                        if !is_printable(&ty, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("{} can't be printed yet", ty.show()),
                                "`io.eprint` prints the same values as `print`, but writes to stderr"
                                    .to_string(),
                                "print one of its fields, or make it a printable type".to_string(),
                                Some(arg.expr.span()),
                            ));
                        }
                    }
                }
                return None;
            }
            ("std.io", "input") => {
                if args.len() > 1 {
                    self.diags.push(wrong_std_arity(name, 1, args.len(), span));
                }
                if let Some(arg) = args.get_mut(0) {
                    self.expect_std_arg(name, 0, &Type::String, arg);
                }
                return Some(result_ty(Type::String, Type::Named("IoError".to_string())));
            }
            ("std.math", "abs") => {
                if args.len() != 1 {
                    self.diags.push(wrong_std_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else { return Some(Type::Int) };
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::Int | Type::Float) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`abs` needs Int or Float, not {}", ty.show()),
                        "absolute value is only defined for numbers".to_string(),
                        "pass an Int or Float".to_string(),
                        Some(arg.expr.span()),
                    ));
                    return None;
                }
                return Some(ty);
            }
            ("std.math", "min" | "max") => {
                if args.len() != 2 {
                    self.diags.push(wrong_std_arity(name, 2, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        format!("`{}` needs comparable values", name),
                        "min/max compare their two arguments".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                if let Some(second) = args.get_mut(1).and_then(|a| self.infer(&mut a.expr)) {
                    if second != first {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!("`{}` needs two values of the same type", name),
                            "min/max compare like with like".to_string(),
                            type_fix_hint(&first, &second),
                            Some(args[1].expr.span()),
                        ));
                    }
                }
                return Some(first);
            }
            ("std.math", "clamp") => {
                if args.len() != 3 {
                    self.diags.push(wrong_std_arity(name, 3, args.len(), span));
                }
                let Some(first) = args.get_mut(0).and_then(|a| self.infer(&mut a.expr)) else {
                    for a in args.iter_mut().skip(1) {
                        self.infer(&mut a.expr);
                    }
                    return None;
                };
                if !types_comparable(&first, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0905",
                        "`clamp` needs comparable values".to_string(),
                        "clamp compares the value with its lower and upper bounds".to_string(),
                        "use Int, Float, String, Char, Bool, or a comparable type".to_string(),
                        Some(args[0].expr.span()),
                    ));
                }
                for i in 1..3 {
                    if let Some(got) = args.get_mut(i).and_then(|a| self.infer(&mut a.expr)) {
                        if got != first {
                            self.diags.push(Diagnostic::error(
                                "E0112",
                                format!("`clamp` needs all three values to have the same type"),
                                "the value and both bounds are compared together".to_string(),
                                type_fix_hint(&first, &got),
                                Some(args[i].expr.span()),
                            ));
                        }
                    }
                }
                return Some(first);
            }
            ("std.random", "pick") => {
                if args.len() != 1 {
                    self.diags.push(wrong_std_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else {
                    return Some(Type::Option(Box::new(Type::Int)));
                };
                let ty = self.infer(&mut arg.expr)?;
                if let Type::List(inner) = ty {
                    return Some(Type::Option(inner));
                }
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!("`pick` needs a list, not {}", ty.show()),
                    "random.pick chooses one item from a List".to_string(),
                    "pass a `List<T>` value".to_string(),
                    Some(arg.expr.span()),
                ));
                return None;
            }
            ("std.random", "shuffle") => {
                if args.len() != 1 {
                    self.diags.push(wrong_std_arity(name, 1, args.len(), span));
                }
                let Some(arg) = args.get_mut(0) else { return None };
                if arg.convention != AccessConvention::Mutate {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        "`shuffle` changes its list".to_string(),
                        "a changing argument must be passed with `mut`".to_string(),
                        "write `random.shuffle(mut xs)`".to_string(),
                        Some(arg.span),
                    ));
                }
                let ty = self.infer(&mut arg.expr)?;
                if !matches!(ty, Type::List(_)) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`shuffle` needs a list, not {}", ty.show()),
                        "random.shuffle reorders a List in place".to_string(),
                        "pass a `List<T>` value".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
                return None;
            }
            _ => {}
        }

        let Some((params, ret)) = sig else {
            self.diags.push(unknown_std_item(module, name, span));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            let _ = alias_span;
            return None;
        };
        if args.len() != params.len() {
            self.diags
                .push(wrong_std_arity(name, params.len(), args.len(), span));
        }
        for (i, ((conv, param_ty), arg)) in params.iter().zip(args.iter_mut()).enumerate() {
            if *conv == AccessConvention::Mutate && arg.convention != AccessConvention::Mutate {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("argument {} to `{}` must be passed with `mut`", i + 1, name),
                    "this standard library call changes that value".to_string(),
                    format!("write `{} value` for this argument", syntax::KW_MUTATE),
                    Some(arg.span),
                ));
            }
            self.expect_std_arg(name, i, param_ty, arg);
        }
        for arg in args.iter_mut().skip(params.len()) {
            self.infer(&mut arg.expr);
        }
        ret
    }

    fn check_std_json_lit(
        &mut self,
        variant: &str,
        args: &mut [crate::ast::CallArg],
        span: Span,
    ) -> Option<Type> {
        let json = Type::Named("Json".to_string());
        let expected = match variant {
            "Null" => Vec::new(),
            "Boolean" => vec![Type::Bool],
            "Number" => vec![Type::Float],
            "Text" => vec![Type::String],
            "Array" => vec![Type::List(Box::new(json.clone()))],
            "Object" => vec![Type::Map {
                key: Box::new(Type::String),
                value: Box::new(json.clone()),
            }],
            _ => {
                let candidates = [
                    "Null", "Boolean", "Number", "Text", "Array", "Object",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
                let mut fix = "check the variant name".to_string();
                if let Some(s) = suggest_field(variant, &candidates) {
                    fix = format!("did you mean `{}`?", s);
                }
                self.diags.push(Diagnostic::error(
                    "E0304",
                    format!("`Json` has no variant `{}`", variant),
                    "std.json exposes the dynamic JSON variants from the M10 API".to_string(),
                    fix,
                    Some(span),
                ));
                for a in args.iter_mut() {
                    self.infer(&mut a.expr);
                }
                return Some(json);
            }
        };
        if args.len() != expected.len() {
            self.diags.push(Diagnostic::error(
                "E0306",
                format!(
                    "`Json.{}` expects {} value{}, got {}",
                    variant,
                    expected.len(),
                    if expected.len() == 1 { "" } else { "s" },
                    args.len()
                ),
                "each JSON variant has the payload listed in the M10 std API".to_string(),
                "check the variant payload".to_string(),
                Some(span),
            ));
        }
        for (i, arg) in args.iter_mut().enumerate() {
            if let Some(want) = expected.get(i) {
                self.expect_std_arg(variant, i, want, arg);
            } else {
                self.infer(&mut arg.expr);
            }
        }
        Some(json)
    }

    fn expect_std_arg(
        &mut self,
        call_name: &str,
        idx: usize,
        param_ty: &Type,
        arg: &mut crate::ast::CallArg,
    ) {
        if matches!(arg.convention, AccessConvention::Move)
            && !matches!(param_ty, Type::Named(n) if n == "Unit")
        {
            self.diags.push(Diagnostic::error(
                "E0203",
                format!("`{}` passed to a parameter that does not consume", syntax::KW_MOVE),
                "standard library functions in M10 read their ordinary arguments unless documented otherwise"
                    .to_string(),
                format!("remove `{}` here", syntax::KW_MOVE),
                Some(arg.span),
            ));
        }
        if matches!(param_ty, Type::String | Type::List(_) | Type::Map { .. }) {
            self.borrow_ctx = true;
        }
        let got = self.infer(&mut arg.expr);
        if let Some(got) = got {
            if is_u8_ty(param_ty) && got == Type::Int {
                if let Expr::Int(n, span) = arg.expr {
                    if !(0..=255).contains(&n) {
                        self.diags.push(u8_range_error(span));
                    }
                }
                return;
            }
            let reported = self.check_type_assignable(param_ty, &got, arg.expr.span());
            if !reported && got != *param_ty {
                self.diags.push(Diagnostic::error(
                    "E0112",
                    format!(
                        "`{}` wants {} for argument {}, but this is {}",
                        call_name,
                        param_ty.show(),
                        idx + 1,
                        got.show()
                    ),
                    "every argument must match its parameter's type".to_string(),
                    type_fix_hint(param_ty, &got),
                    Some(arg.expr.span()),
                ));
            }
        }
    }

    fn check_static_method(
        &mut self,
        type_name: &str,
        method: &str,
        span: Span,
        args: &mut [crate::ast::CallArg],
    ) -> Option<Type> {
        let Some(msig) = self.registry.method(type_name, method).cloned() else {
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("`{}` has no method `{}`", type_name, method),
                "check the method name on this type".to_string(),
                format!("define it inside `struct {type_name}` or `impl {type_name}`"),
                Some(span),
            ));
            for a in args.iter_mut() {
                self.infer(&mut a.expr);
            }
            return None;
        };
        if !msig.is_static {
            self.diags.push(Diagnostic::error(
                "E0311",
                format!("`{}` is an instance method on `{}`", method, type_name),
                "instance methods need a value before the dot".to_string(),
                format!("call it on a `{type_name}` value: `x.{method}(...)`"),
                Some(span),
            ));
        }
        self.check_method_args(type_name, method, &msig, args, span)
    }

    fn check_method_args(
        &mut self,
        type_name: &str,
        method: &str,
        sig: &MethodSig,
        args: &mut [crate::ast::CallArg],
        span: Span,
    ) -> Option<Type> {
        let _ = (type_name, method, span);
        let expected_args = if sig.self_conv.is_some() {
            sig.params.len().saturating_sub(1)
        } else {
            sig.params.len()
        };
        if args.len() != expected_args {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    method,
                    expected_args,
                    if expected_args == 1 { "" } else { "s" },
                    args.len()
                ),
                if sig.self_conv.is_some() {
                    "every argument must match a parameter (not counting `self`)".to_string()
                } else {
                    "every argument must match a parameter".to_string()
                },
                format!("check the definition of `{method}` on `{type_name}`"),
                Some(span),
            ));
        }
        let mut arg_idx = 0;
        for (i, (param_conv, param_ty)) in sig.params.iter().enumerate() {
            if i == 0 && sig.self_conv.is_some() {
                continue;
            }
            if let Some(arg) = args.get_mut(arg_idx) {
                if matches!(param_conv, AccessConvention::Read) && !param_ty.is_scalar() {
                    self.borrow_ctx = true;
                }
                let arg_ty = self.infer(&mut arg.expr);
                if let Some(arg_ty) = arg_ty {
                    let reported =
                        self.check_type_assignable(param_ty, &arg_ty, arg.expr.span());
                    if !reported && arg_ty != *param_ty {
                        self.diags.push(Diagnostic::error(
                            "E0112",
                            format!(
                                "`{}` wants {} for argument {}, but this is {}",
                                method,
                                param_ty.show(),
                                arg_idx + 1,
                                arg_ty.show()
                            ),
                            "every argument must match its parameter's type".to_string(),
                            type_fix_hint(param_ty, &arg_ty),
                            Some(arg.expr.span()),
                        ));
                    }
                }
                if arg.convention == AccessConvention::Mutate
                    && !matches!(arg.expr, Expr::Ident(_, _))
                {
                    self.diags.push(Diagnostic::error(
                        "E0202",
                        format!("`{}` needs a plain `var` name after it", syntax::KW_MUTATE),
                        "only a named binding can be handed out for changing".to_string(),
                        format!(
                            "bind the value first: `{} x = ...;` then pass `{} x`",
                            syntax::KW_VAR,
                            syntax::KW_MUTATE
                        ),
                        Some(arg.span),
                    ));
                }
                // Same ownership rules as plain calls (E0201/E0202/E0203).
                match (param_conv, arg.convention) {
                    (AccessConvention::Move, AccessConvention::Read) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if is_cloneable(param_ty, self.registry, self.structs) {
                                arg.flags.implicit_clone = true;
                                self.diags.push(Diagnostic::lint(
                                    "L0201",
                                    format!(
                                        "implicit clone of `{}`; write `{} {}` to transfer ownership or `.clone()` to silence this warning",
                                        name,
                                        syntax::KW_MOVE,
                                        name
                                    ),
                                    format!(
                                        "`{}` expects to take ownership of this value",
                                        method
                                    ),
                                    format!(
                                        "write `{} {}` to move, or `{} .clone()` to copy explicitly",
                                        syntax::KW_MOVE,
                                        name,
                                        name
                                    ),
                                    Some(*span),
                                ));
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0201",
                                    format!(
                                        "`{}` needs `{}` here — this value can't be copied",
                                        method,
                                        syntax::KW_MOVE
                                    ),
                                    format!(
                                        "parameter `{}` takes ownership; passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                        arg_idx + 1,
                                        name,
                                        syntax::KW_MOVE
                                    ),
                                    format!(
                                        "write `{} {}` to transfer ownership",
                                        syntax::KW_MOVE,
                                        name
                                    ),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                    (AccessConvention::Move, AccessConvention::Move) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if !param_ty.is_scalar() {
                                self.mark_moved(name.clone(), *span);
                            }
                        }
                    }
                    (AccessConvention::Mutate, AccessConvention::Read) => {
                        if let Expr::Ident(name, nspan) = &arg.expr {
                            self.diags.push(Diagnostic::error(
                                "E0202",
                                format!("parameter `{}` requires `{}` at the call site", name, syntax::KW_MUTATE),
                                format!("`{method}` needs to change this value while it borrows it"),
                                format!("write `{} {}` when calling `{method}`", syntax::KW_MUTATE, name),
                                Some(*nspan),
                            ));
                        }
                    }
                    (AccessConvention::Mutate, AccessConvention::Mutate) => {
                        if let Expr::Ident(name, span) = &arg.expr {
                            if let Some(info) = self.lookup(name) {
                                if !info.mutable {
                                    self.diags.push(Diagnostic::error(
                                        "E0111",
                                        format!(
                                            "`{}` was made with `{}`, so it can't be changed",
                                            name,
                                            syntax::KW_VAL
                                        ),
                                        format!(
                                            "`{}` will change this value, so it must be a `{}`",
                                            method,
                                            syntax::KW_VAR
                                        ),
                                        format!("declare it with `{} {} = ...`", syntax::KW_VAR, name),
                                        Some(*span),
                                    ));
                                }
                            }
                        }
                    }
                    (
                        AccessConvention::Read | AccessConvention::Mutate,
                        AccessConvention::Move,
                    ) => {
                        self.diags.push(Diagnostic::error(
                            "E0203",
                            format!(
                                "`{}` passed to a parameter that does not consume",
                                syntax::KW_MOVE
                            ),
                            "only `take` parameters accept a moved value at the call site"
                                .to_string(),
                            format!(
                                "remove `{}` or change the parameter to `take`",
                                syntax::KW_MOVE
                            ),
                            Some(arg.span),
                        ));
                    }
                    _ => {}
                }
                arg_idx += 1;
            }
        }
        sig.return_type.clone()
    }

    fn struct_owner_module(&self, type_name: &str, import_ns: Option<&str>) -> Option<usize> {
        if let Some(alias) = import_ns {
            let mod_idx = *self.imports.get(alias)?;
            let mods = self.modules?;
            if mods[mod_idx].registry.contains(type_name) {
                return Some(mod_idx);
            }
            return None;
        }
        if self.registry.contains(type_name) {
            return Some(self.module_idx);
        }
        let mods = self.modules?;
        let mut found = None;
        for (idx, st) in mods.iter().enumerate() {
            if st.registry.contains(type_name)
                && st.type_pub.get(type_name).copied().unwrap_or(false)
            {
                found = Some(idx);
            }
        }
        found
    }

    fn struct_fields_of(
        &self,
        owner_mod: usize,
        type_name: &str,
    ) -> Option<&[(String, Span, Type, bool, bool)]> {
        if owner_mod == self.module_idx {
            self.registry.struct_fields(type_name)
        } else {
            self.modules?
                .get(owner_mod)?
                .registry
                .struct_fields(type_name)
        }
    }

    fn field_is_pub_in(&self, owner_mod: usize, type_name: &str, field: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        self.modules
            .and_then(|mods| mods.get(owner_mod))
            .and_then(|st| st.field_pub.get(&(type_name.to_string(), field.to_string())).copied())
            .unwrap_or(false)
    }

    fn type_is_pub_in(&self, owner_mod: usize, type_name: &str) -> bool {
        if owner_mod == self.module_idx {
            return true;
        }
        self.modules
            .and_then(|mods| mods.get(owner_mod))
            .and_then(|st| st.type_pub.get(type_name).copied())
            .unwrap_or(false)
    }

    fn check_struct_lit(
        &mut self,
        type_name: &str,
        type_args: &[Type],
        import_ns: Option<&str>,
        fields: &mut [(String, Span, Expr)],
        span: Span,
    ) -> Type {
        let Some(owner_mod) = self.struct_owner_module(type_name, import_ns) else {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no type called `{}`", type_name),
                "struct literals need a struct type name".to_string(),
                "define the struct first, or check the spelling".to_string(),
                Some(span),
            ));
            for (_, _, e) in fields.iter_mut() {
                self.infer(e);
            }
            return Type::Named(type_name.to_string());
        };
        if owner_mod != self.module_idx && !self.type_is_pub_in(owner_mod, type_name) {
            self.diags.push(private_item(type_name, span));
        }
        let def_fields: Vec<(String, Span, Type, bool, bool)> = self
            .struct_fields_of(owner_mod, type_name)
            .map(|fields| fields.to_vec())
            .unwrap_or_default();
        if def_fields.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no type called `{}`", type_name),
                "struct literals need a struct type name".to_string(),
                "define the struct first, or check the spelling".to_string(),
                Some(span),
            ));
            for (_, _, e) in fields.iter_mut() {
                self.infer(e);
            }
            return Type::Named(type_name.to_string());
        };
        let subst = self.struct_subst(type_name, type_args);
        let field_names: Vec<String> = def_fields.iter().map(|(n, ..)| n.clone()).collect();
        let mut provided = HashMap::new();
        for (name, name_span, expr) in fields.iter_mut() {
            if provided.insert(name.clone(), ()).is_some() {
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!("field `{}` appears more than once", name),
                    "each field may be written only once in a struct literal".to_string(),
                    "remove the duplicate field".to_string(),
                    Some(*name_span),
                ));
            }
            if owner_mod != self.module_idx
                && !self.field_is_pub_in(owner_mod, type_name, name)
            {
                self.diags.push(private_item(name, *name_span));
            }
            let field_def = def_fields.iter().find(|(n, ..)| n == name);
            let saved_expected = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            if let Some((_, _, fty, _, _)) = field_def {
                let inst = self.m9.instantiate_type(fty, &subst);
                self.expected_type = Some(inst);
            }
            if matches!(expr, Expr::Lambda(_)) {
                self.lambda_escapes = true;
            }
            let et = self.infer(expr);
            self.expected_type = saved_expected;
            self.lambda_escapes = saved_esc;
            if let Some((_, _, fty, _, _)) = field_def {
                let inst = self.m9.instantiate_type(fty, &subst);
                if let Some(et) = et {
                    self.check_type_assignable(&inst, &et, expr.span());
                }
            } else {
                self.diags.push(Diagnostic::error(
                    "E0303",
                    format!("struct literal for `{}` has no field `{}`", type_name, name),
                    "struct literals may only set fields that exist on the type".to_string(),
                    suggest_field(name, &field_names)
                        .map(|s| format!("did you mean `{}`?", s))
                        .unwrap_or_else(|| "remove this field".to_string()),
                    Some(*name_span),
                ));
            }
        }
        let missing: Vec<_> = def_fields
            .iter()
            .filter(|(n, _, _, is_ref, _)| !*is_ref && !provided.contains_key(n))
            .map(|(n, ..)| n.clone())
            .collect();
        if !missing.is_empty() {
            self.diags.push(Diagnostic::error(
                "E0303",
                format!("struct literal for `{}` is missing fields: {}", type_name, missing.join(", ")),
                "every non-`ref` field must appear exactly once".to_string(),
                format!("add: {}", missing.join(", ")),
                Some(span),
            ));
        }
        if !type_args.is_empty() {
            Type::Apply {
                name: type_name.to_string(),
                args: type_args.to_vec(),
            }
        } else if self
            .m9
            .struct_params
            .get(type_name)
            .is_some_and(|p| !p.is_empty())
        {
            Type::Apply {
                name: type_name.to_string(),
                args: self
                    .m9
                    .struct_params
                    .get(type_name)
                    .unwrap()
                    .iter()
                    .map(|p| Type::Named(p.name.clone()))
                    .collect(),
            }
        } else {
            Type::Named(type_name.to_string())
        }
    }

    fn check_enum_lit(
        &mut self,
        type_name: &str,
        variant: &str,
        args: &mut [EnumLitArg],
        span: Span,
    ) -> Type {
        let ty = Type::Named(type_name.to_string());
        let Some(variants) = self.registry.enum_variants(type_name) else {
            self.diags.push(Diagnostic::error(
                "E0119",
                format!("there's no enum called `{}`", type_name),
                "enum literals need an enum type name".to_string(),
                "define the enum first, or check the spelling".to_string(),
                Some(span),
            ));
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        self.infer(e);
                    }
                }
            }
            return ty;
        };
        let Some((_, payload)) = variants.get(variant) else {
            let mut fix = "check the variant name".to_string();
            if let Some(s) = suggest_field(variant, &variants.keys().cloned().collect::<Vec<_>>()) {
                fix = format!("did you mean `{}`?", s);
            }
            self.diags.push(Diagnostic::error(
                "E0304",
                format!("`{}` has no variant `{}`", type_name, variant),
                "enum literals must name a variant on the type".to_string(),
                fix,
                Some(span),
            ));
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        self.infer(e);
                    }
                }
            }
            return ty;
        };
        match payload {
            VariantPayload::Unit => {
                if !args.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` takes no payload", variant),
                        "unit variants are written without parentheses".to_string(),
                        format!("write `{type_name}.{variant}` with no `(...)`"),
                        Some(span),
                    ));
                }
            }
            VariantPayload::Single(expected, _) => {
                if args.len() != 1 {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` expects one value", variant),
                        "single-payload variants take one positional argument (S30)".to_string(),
                        format!("write `{type_name}.{variant}(...)`"),
                        Some(span),
                    ));
                }
                if let Some(EnumLitArg::Positional(e)) = args.first_mut() {
                    if let Some(et) = self.infer(e) {
                        self.check_type_assignable(expected, &et, e.span());
                    }
                } else if let Some(EnumLitArg::Named { label, .. }) = args.first() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` expects a positional value, not `{}:`", variant, label),
                        "single-payload variants use positional args only (S30)".to_string(),
                        format!("write `{type_name}.{variant}(value)`"),
                        Some(span),
                    ));
                }
            }
            VariantPayload::Named(fields) => {
                let mut seen = HashSet::new();
                for a in args.iter_mut() {
                    match a {
                        EnumLitArg::Positional(_) => {
                            self.diags.push(Diagnostic::error(
                                "E0303",
                                format!("variant `{}` requires labeled fields", variant),
                                "multi-payload variants need `name: value` at the call site (S30)".to_string(),
                                format!("write `{type_name}.{variant}(w: 1.0, h: 2.0)`"),
                                Some(span),
                            ));
                        }
                        EnumLitArg::Named { label, expr } => {
                            if !seen.insert(label.clone()) {
                                self.diags.push(Diagnostic::error(
                                    "E0303",
                                    format!("field `{}` appears more than once", label),
                                    "each payload field may be written only once".to_string(),
                                    "remove the duplicate label".to_string(),
                                    Some(expr.span()),
                                ));
                            }
                            let et = self.infer(expr);
                            if let Some(f) = fields.iter().find(|f| f.name == *label) {
                                if let Some(et) = et {
                                    self.check_type_assignable(&f.ty, &et, expr.span());
                                }
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0302",
                                    format!("variant `{}` has no field `{}`", variant, label),
                                    "check the field names on this variant".to_string(),
                                    suggest_field(
                                        label,
                                        &fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
                                    )
                                        .map(|s| format!("did you mean `{}`?", s))
                                        .unwrap_or_else(|| "remove this label".to_string()),
                                    Some(expr.span()),
                                ));
                            }
                        }
                    }
                }
                let missing: Vec<_> = fields
                    .iter()
                    .filter(|f| !seen.contains(&f.name))
                    .map(|f| f.name.clone())
                    .collect();
                if !missing.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0303",
                        format!("variant `{}` is missing fields: {}", variant, missing.join(", ")),
                        "every payload field must appear exactly once".to_string(),
                        format!("add: {}", missing.join(", ")),
                        Some(span),
                    ));
                }
            }
        }
        ty
    }

    /// S31: `subject == Red` when `Red` is a unit variant, not a variable.
    fn eq_unit_variant_pattern(
        &self,
        lhs: &Expr,
        rhs: &Expr,
        subject_name: Option<&str>,
        subj_ty: &Type,
    ) -> Option<Pattern> {
        if !subject_name.is_some_and(|n| expr_is_same_ident(lhs, n)) {
            return None;
        }
        let Expr::Ident(variant, rhs_span) = rhs else {
            return None;
        };
        if self.lookup(variant).is_some() || self.consts.contains_key(variant) {
            return None;
        }
        let Type::Named(enum_name) = subj_ty else {
            return None;
        };
        if !self
            .registry
            .enum_variants(enum_name)
            .is_some_and(|variants| variants.contains_key(variant))
        {
            return None;
        }
        Some(Pattern::Variant {
            variant: variant.clone(),
            bindings: Vec::new(),
            span: *rhs_span,
        })
    }

    /// S31: pattern carried as `PatternTest` or as `subject == UnitVariant`.
    fn switch_arm_pattern(
        &self,
        cond: &Expr,
        subject_name: Option<&str>,
        subj_ty: &Type,
    ) -> Option<Pattern> {
        match cond {
            Expr::PatternTest { subject, pattern, .. } => {
                if subject_name.is_some_and(|n| expr_is_same_ident(subject, n)) {
                    Some(pattern.clone())
                } else {
                    None
                }
            }
            Expr::Binary(BinOp::Eq, lhs, rhs, _) => {
                self.eq_unit_variant_pattern(lhs, rhs, subject_name, subj_ty)
            }
            _ => None,
        }
    }

    fn check_pattern_test(
        &mut self,
        subject: &mut Box<Expr>,
        pattern: &Pattern,
        span: Span,
    ) -> HashMap<String, Type> {
        let subj_ty = self.infer(subject);
        let Some(st) = subj_ty else {
            return HashMap::new();
        };
        let bindings = self.validate_pattern(&st, pattern, span);
        self.mark_pattern_subject_moved(subject, &bindings);
        bindings
    }

    /// Binding a non-copy payload out of a pattern gives the subject away in
    /// the generated Rust (`if let` / `matches!` move the place), so the old
    /// name must stop being usable — otherwise rustc rejects the output (I2).
    fn mark_pattern_subject_moved(
        &mut self,
        subject: &Expr,
        bindings: &HashMap<String, Type>,
    ) {
        if bindings.values().all(type_is_copy) {
            return;
        }
        if let Expr::Ident(n, nspan) = subject {
            if n != syntax::KW_IT && self.lookup(n).is_some() {
                self.mark_moved(n.clone(), *nspan);
            }
        }
    }

    fn validate_pattern(
        &mut self,
        subject_ty: &Type,
        pattern: &Pattern,
        span: Span,
    ) -> HashMap<String, Type> {
        match (subject_ty, pattern) {
            (Type::Option(inner), Pattern::Present { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**inner).clone());
                map
            }
            (Type::Option(_), Pattern::Absent(_)) => HashMap::new(),
            (Type::Result { ok, .. }, Pattern::Ok { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**ok).clone());
                map
            }
            (Type::Result { err, .. }, Pattern::Err { binding, .. }) => {
                let mut map = HashMap::new();
                map.insert(binding.clone(), (**err).clone());
                map
            }
            (Type::Result { .. }, Pattern::Present { .. } | Pattern::Absent(_)) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "this pattern belongs to an optional value, not {}",
                        subject_ty.name()
                    ),
                    "use `== ok(...)` or `== err(...)` on a fallible result".to_string(),
                    format!(
                        "write `== {}(...)` or `== {}(...)` instead",
                        syntax::LIT_OK,
                        syntax::LIT_ERR
                    ),
                    Some(span),
                ));
                HashMap::new()
            }
            (Type::Named(enum_name), Pattern::Variant { variant, bindings, .. }) => {
                if enum_name == "Json" {
                    let Some(expected) = std_json_pattern_types(variant) else {
                        self.diags.push(Diagnostic::error(
                            "E0305",
                            format!("pattern `{}` doesn't belong to `Json`", variant),
                            "pattern tests must name a variant on the value's enum type"
                                .to_string(),
                            "check the Json variant spelling".to_string(),
                            Some(span),
                        ));
                        return HashMap::new();
                    };
                    if bindings.len() != expected.len() {
                        self.diags.push(Diagnostic::error(
                            "E0306",
                            format!(
                                "pattern `{}` expects {} binding{}, got {}",
                                variant,
                                expected.len(),
                                if expected.len() == 1 { "" } else { "s" },
                                bindings.len()
                            ),
                            "each payload field needs its own binding name".to_string(),
                            format!(
                                "write `{}({})",
                                variant,
                                (0..expected.len())
                                    .map(|i| format!("v{i}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            Some(span),
                        ));
                    }
                    return bindings
                        .iter()
                        .zip(expected.iter())
                        .map(|(b, t)| (b.clone(), t.clone()))
                        .collect();
                }
                let Some(variants) = self.registry.enum_variants(enum_name) else {
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!("pattern `{}` doesn't match this value's type", variant),
                        format!("`{}` is a struct, not an enum", enum_name),
                        "use a struct field access instead of a variant pattern".to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                let Some((_, payload)) = variants.get(variant) else {
                    self.diags.push(Diagnostic::error(
                        "E0305",
                        format!("pattern `{}` doesn't belong to `{}`", variant, enum_name),
                        "pattern tests must name a variant on the value's enum type".to_string(),
                        "check the variant spelling".to_string(),
                        Some(span),
                    ));
                    return HashMap::new();
                };
                let expected = pattern_binding_types(payload);
                if bindings.len() != expected.len() {
                    self.diags.push(Diagnostic::error(
                        "E0306",
                        format!(
                            "pattern `{}` expects {} binding{}, got {}",
                            variant,
                            expected.len(),
                            if expected.len() == 1 { "" } else { "s" },
                            bindings.len()
                        ),
                        "each payload field needs its own binding name".to_string(),
                        format!("write `{}({})", variant, (0..expected.len()).map(|i| format!("v{i}")).collect::<Vec<_>>().join(", ")),
                        Some(span),
                    ));
                }
                bindings
                    .iter()
                    .zip(expected.iter())
                    .map(|(b, t)| (b.clone(), t.clone()))
                    .collect()
            }
            (_, Pattern::Variant { variant, .. }) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!("pattern `{}` doesn't match {}", variant, subject_ty.show()),
                    "variant patterns only work on enum values".to_string(),
                    format!("test an enum value, or use `{}` / `{}` for optionals", syntax::LIT_VALUE, syntax::LIT_NULL),
                    Some(span),
                ));
                HashMap::new()
            }
            (Type::Named(_), Pattern::Present { .. } | Pattern::Absent(_)) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    "this pattern doesn't match the value's type".to_string(),
                    format!(
                        "`{}` / `{}` patterns work on `T?` values only",
                        syntax::LIT_VALUE,
                        syntax::LIT_NULL
                    ),
                    "use a variant pattern for enum values".to_string(),
                    Some(span),
                ));
                HashMap::new()
            }
            (_, Pattern::Ok { .. } | Pattern::Err { .. }) => {
                self.diags.push(Diagnostic::error(
                    "E0305",
                    format!(
                        "this pattern belongs to a fallible result, not {}",
                        subject_ty.name()
                    ),
                    format!(
                        "use `== {}(...)` or `== {}(...)` on `Result<T, E>`",
                        syntax::LIT_OK,
                        syntax::LIT_ERR
                    ),
                    "check the type of the value being tested".to_string(),
                    Some(span),
                ));
                HashMap::new()
            }
            _ => HashMap::new(),
        }
    }

    fn check_panic_call(&mut self, call: &mut Call) {
        if call.args.len() != 1 {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!("`{}` needs exactly one message", syntax::BUILTIN_PANIC),
                "a panic report needs something to show the user".to_string(),
                format!("e.g. {}(\"something went wrong\")", syntax::BUILTIN_PANIC),
                Some(call.name_span),
            ));
        }
        for arg in call.args.iter_mut() {
            self.borrow_ctx = true; // panic shows the message via `.jet_show()`
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` needs text, but this is {}",
                            syntax::BUILTIN_PANIC,
                            t.show()
                        ),
                        "the panic message is shown to the user as text".to_string(),
                        "put the message in quotes".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
    }

    fn check_require_call(&mut self, call: &mut Call) {
        if call.args.is_empty() || call.args.len() > 2 {
            self.diags.push(Diagnostic::error(
                "E0103",
                format!(
                    "`{}` needs one condition, or a condition and a message",
                    syntax::BUILTIN_REQUIRE
                ),
                "require checks a yes/no condition and stops when it's false".to_string(),
                format!(
                    "e.g. {}(x > 0) or {}(x > 0, \"x must be positive\")",
                    syntax::BUILTIN_REQUIRE,
                    syntax::BUILTIN_REQUIRE
                ),
                Some(call.name_span),
            ));
        }
        if let Some(arg) = call.args.first_mut() {
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::Bool {
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "`{}` needs {}, but this is {}",
                            syntax::BUILTIN_REQUIRE,
                            Type::Bool.show(),
                            t.show()
                        ),
                        "the condition must be true or false".to_string(),
                        "compare values first, e.g. `x > 0`".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
        if let Some(arg) = call.args.get_mut(1) {
            let t = self.infer(&mut arg.expr);
            if let Some(t) = t {
                if t != Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` message must be text, but this is {}",
                            syntax::BUILTIN_REQUIRE,
                            t.show()
                        ),
                        "the optional message is shown when the condition is false".to_string(),
                        "put the message in quotes".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
        }
    }

    fn check_require_eq_call(&mut self, call: &mut Call) {
        if call.args.len() != 2 {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` needs exactly two values to compare",
                    syntax::BUILTIN_REQUIRE_EQ
                ),
                "require_eq checks that two values are equal".to_string(),
                format!(
                    "e.g. {}(got, expected)",
                    syntax::BUILTIN_REQUIRE_EQ
                ),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return;
        }
        // require_eq compares and shows by reference in the generated Rust.
        self.borrow_ctx = true;
        let lt = self.infer(&mut call.args[0].expr);
        self.borrow_ctx = true;
        let rt = self.infer(&mut call.args[1].expr);
        match (lt, rt) {
            (Some(lt), Some(rt)) => {
                if lt != rt {
                    self.diags.push(Diagnostic::error(
                        "E0108",
                        format!(
                            "`{}` compared {} and {}, which don't match",
                            syntax::BUILTIN_REQUIRE_EQ,
                            lt.show(),
                            rt.show()
                        ),
                        "both sides must be the same type to compare them".to_string(),
                        "convert one side, or compare fields that have the same type".to_string(),
                        Some(call.name_span),
                    ));
                } else if !types_comparable(&lt, self.registry) {
                    if let Some(field) = incomparable_field(&lt, self.registry) {
                        self.diags.push(Diagnostic::error(
                            "E0312",
                            format!(
                                "`{}` can't compare values of type `{}` (field `{}` isn't comparable)",
                                syntax::BUILTIN_REQUIRE_EQ,
                                lt.name(),
                                field
                            ),
                            "equality needs types whose fields can all be compared".to_string(),
                            "compare the fields you care about instead".to_string(),
                            Some(call.name_span),
                        ));
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0312",
                            format!(
                                "`{}` can't compare values of type `{}`",
                                syntax::BUILTIN_REQUIRE_EQ,
                                lt.show()
                            ),
                            "this type doesn't support `==`".to_string(),
                            "compare fields individually, or use a different check".to_string(),
                            Some(call.name_span),
                        ));
                    }
                }
            }
            _ => {}
        }
    }

    /// Binary operators, including comparison distribution (S25):
    /// `day == "mon" || "tue"` re-applies the nearest comparison.
    fn infer_binary(
        &mut self,
        op: BinOp,
        lhs: &mut Box<Expr>,
        rhs: &mut Box<Expr>,
        span: Span,
    ) -> Option<Type> {
        if matches!(op, BinOp::And | BinOp::Or) {
            let lt = self.infer(lhs);
            if let Some(lt) = lt {
                if lt != Type::Bool {
                    self.diags.push(Diagnostic::error(
                        "E0110",
                        format!(
                            "the left side of `{}` must be {}, but this is {}",
                            op.spell(),
                            Type::Bool.show(),
                            lt.show()
                        ),
                        "logic joins yes/no values".to_string(),
                        "compare the value to something first".to_string(),
                        Some(lhs.span()),
                    ));
                }
            }
            let rt = self.infer(rhs);
            if let Some(rt) = rt {
                if rt != Type::Bool {
                    // S25: a plain value re-applies the nearest comparison.
                    if let Some((subject, cmp_op)) = rightmost_comparison(lhs) {
                        let rhs_span = rhs.span();
                        let new_span = Span::new(subject.span().start, rhs_span.end);
                        let old_rhs = std::mem::replace(
                            rhs.as_mut(),
                            Expr::Bool(false, rhs_span),
                        );
                        **rhs = Expr::Binary(
                            cmp_op,
                            Box::new(subject),
                            Box::new(old_rhs),
                            new_span,
                        );
                        // Re-check the rebuilt comparison; this reports a
                        // mismatch (E0109) if the value's type doesn't fit.
                        self.infer_rebuilt(rhs);
                    } else {
                        self.diags.push(Diagnostic::error(
                            "E0110",
                            format!(
                                "the right side of `{}` must be {}, but this is {}",
                                op.spell(),
                                Type::Bool.show(),
                                rt.show()
                            ),
                            format!(
                                "right after a comparison, a plain value repeats it (`x == 1 {} 2` means `x == 1 {} x == 2`, S25) — but there's no comparison before this one",
                                op.spell(),
                                op.spell()
                            ),
                            "compare the value to something, e.g. `x == 2`".to_string(),
                            Some(rhs.span()),
                        ));
                    }
                }
            }
            return Some(Type::Bool);
        }

        let lt = self.infer(lhs);

        // S31: pattern-shaped `==` before RHS name lookup.
        if op == BinOp::Eq {
            let subj_name = match lhs.as_ref() {
                Expr::Ident(n, _) => Some(n.as_str()),
                _ => None,
            };
            if let Some(lt) = &lt {
                if let Some(pattern) = self.eq_unit_variant_pattern(lhs, rhs, subj_name, lt) {
                    self.validate_pattern(lt, &pattern, span);
                    return Some(Type::Bool);
                }
                if let Expr::Ident(name, rhs_span) = rhs.as_ref() {
                    if self.lookup(name).is_none() && !self.consts.contains_key(name) {
                        if matches!(lt, Type::Option(_) | Type::Named(_)) {
                            let pattern = Pattern::Variant {
                                variant: name.clone(),
                                bindings: Vec::new(),
                                span: *rhs_span,
                            };
                            self.validate_pattern(lt, &pattern, span);
                            return Some(Type::Bool);
                        }
                    }
                }
            }
        }

        let rt = self.infer(rhs);
        let (lt, rt) = (lt?, rt?);

        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                if lt == rt && matches!(lt, Type::Int | Type::Float) {
                    Some(lt)
                } else if lt == Type::String && op == BinOp::Add {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        "text isn't joined with `+`".to_string(),
                        "there's one way to build text: interpolation (S8)".to_string(),
                        "write the pieces inside one string: \"{a}{b}\"".to_string(),
                        Some(span),
                    ));
                    None
                } else if (lt == Type::Int && rt == Type::Float)
                    || (lt == Type::Float && rt == Type::Int)
                {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "`{}` can't mix {} and {}",
                            op.spell(),
                            lt.show(),
                            rt.show()
                        ),
                        "Jet never converts numbers silently; the two sides must match"
                            .to_string(),
                        "make both sides the same kind of number (write `2.0` instead of `2`, or drop the `.0`)".to_string(),
                        Some(span),
                    ));
                    None
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Rem | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl
            | BinOp::Shr => {
                if lt == Type::Int && rt == Type::Int {
                    Some(Type::Int)
                } else {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!(
                            "`{}` works on {} only, but this has {} and {}",
                            op.spell(),
                            Type::Int.show(),
                            lt.show(),
                            rt.show()
                        ),
                        compound_why(op),
                        "use whole numbers here".to_string(),
                        Some(span),
                    ));
                    None
                }
            }
            BinOp::Eq | BinOp::Ne => {
                if lt == rt {
                    if !types_comparable(&lt, self.registry) {
                        if let Some(field) = incomparable_field(&lt, self.registry) {
                            self.diags.push(Diagnostic::error(
                                "E0312",
                                format!("`{}` can't be compared with `{}` because field `{}` doesn't support `{}`", lt.name(), rt.name(), field, op.spell()),
                                "value equality needs every field to support the comparison".to_string(),
                                "compare individual fields instead".to_string(),
                                Some(span),
                            ));
                        } else {
                            self.op_mismatch(op, &lt, &rt, span);
                        }
                        return None;
                    }
                    Some(Type::Bool)
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                if lt == rt && matches!(lt, Type::Int | Type::Float) {
                    Some(Type::Bool)
                } else if lt == rt
                    && (types_comparable(&lt, self.registry)
                        || self.type_param_has_bound(&lt, COMPARABLE))
                {
                    Some(Type::Bool)
                } else if lt == rt && lt == Type::String {
                    self.diags.push(Diagnostic::error(
                        "E0109",
                        format!("text isn't ordered with `{}`", op.spell()),
                        "comparing text for order isn't supported yet".to_string(),
                        "compare with `==` or `!=`, or compare lengths/numbers instead"
                            .to_string(),
                        Some(span),
                    ));
                    None
                } else {
                    self.op_mismatch(op, &lt, &rt, span);
                    None
                }
            }
            BinOp::And | BinOp::Or => unreachable!(),
        }
    }

    /// Re-infer a node we just built ourselves (S25); it can still report
    /// a type mismatch, but never duplicates earlier errors because both
    /// halves were already clean.
    fn infer_rebuilt(&mut self, e: &mut Expr) {
        self.infer(e);
    }

    fn op_mismatch(&mut self, op: BinOp, lt: &Type, rt: &Type, span: Span) {
        self.diags.push(Diagnostic::error(
            "E0109",
            format!(
                "`{}` can't compare or combine {} and {}",
                op.spell(),
                lt.show(),
                rt.show()
            ),
            "the two sides of an operator must be the same type".to_string(),
            "make both sides the same type".to_string(),
            Some(span),
        ));
    }

    // --- calls -----------------------------------------------------------

    /// Check a call. Returns:
    ///   None             — problem already reported
    ///   Some(None)       — fine, no value handed back
    ///   Some(Some(ty))   — fine, hands back `ty`
    fn check_call(&mut self, call: &mut Call, _as_value: bool) -> Option<Option<Type>> {
        if call.name == syntax::FOREIGN_PRINTLN || call.name == syntax::FOREIGN_EPRINTLN {
            let target = if call.name == syntax::FOREIGN_EPRINTLN {
                "io.eprint"
            } else {
                syntax::BUILTIN_PRINT
            };
            self.diags.push(Diagnostic::error(
                "E0037",
                format!(
                    "{} calls it `{}`, not `{}`",
                    syntax::LANG_NAME,
                    target,
                    call.name
                ),
                "`print` writes to stdout; `io.eprint` is the stderr twin in `std.io`"
                    .to_string(),
                format!("replace `{}` with `{}`", call.name, target),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == syntax::FOREIGN_OPEN {
            self.diags.push(Diagnostic::error(
                "E0038",
                "`open` is not the M10 file API".to_string(),
                "M10 uses whole-file helpers in `std.fs`; file handles are out of scope".to_string(),
                "import `std.fs as fs` and call `fs.read(path)` or `fs.write(path, text)`"
                    .to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == syntax::FOREIGN_GETENV {
            self.diags.push(Diagnostic::error(
                "E0039",
                "`getenv` is written `env.get` in Jet".to_string(),
                "environment access lives in the `std.env` module".to_string(),
                "import `std.env as env` and call `env.get(name)`".to_string(),
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        }

        if call.name == syntax::BUILTIN_PRINT {
            if call.args.len() != 1 {
                self.diags.push(Diagnostic::error(
                    "E0103",
                    format!("`{}` needs exactly one thing to print", syntax::BUILTIN_PRINT),
                    "printing nothing isn't meaningful".to_string(),
                    format!("e.g. {}(\"hello\")", syntax::BUILTIN_PRINT),
                    Some(call.name_span),
                ));
                for arg in call.args.iter_mut() {
                    self.infer(&mut arg.expr);
                }
                return None;
            }
            let arg = &mut call.args[0];
            self.borrow_ctx = true; // print borrows via `.jet_show()`
            if let Some(t) = self.infer(&mut arg.expr) {
                if !is_printable(&t, self.registry) {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!("`{}` doesn't know how to show {}", syntax::BUILTIN_PRINT, t.show()),
                        "print shows values that have a display".to_string(),
                        "print one of its parts instead".to_string(),
                        Some(arg.expr.span()),
                    ));
                }
            }
            return Some(None);
        }

        if call.name == syntax::BUILTIN_PANIC {
            self.check_panic_call(call);
            return Some(None);
        }

        if call.name == syntax::BUILTIN_REQUIRE {
            self.check_require_call(call);
            return Some(None);
        }

        if call.name == syntax::BUILTIN_REQUIRE_EQ {
            self.check_require_eq_call(call);
            return Some(None);
        }

        if self.funcs.get(&call.name).is_none() {
            if let Some(info) = self.lookup(&call.name) {
                if matches!(info.ty, Type::Fn { .. }) {
                    let name_span = call.name_span;
                    let mut callee =
                        Box::new(Expr::Ident(call.name.clone(), name_span));
                    let mut args = std::mem::take(&mut call.args);
                    let end = args
                        .last()
                        .map(|a| a.expr.span().end)
                        .unwrap_or(name_span.end);
                    let span = Span::new(name_span.start, end);
                    let ret = self.infer_call_value(&mut callee, &mut args, span);
                    call.args = args;
                    return Some(ret);
                }
            }
        }

        let Some(sig) = self.funcs.get(&call.name).cloned() else {
            let mut fix = format!(
                "define it first ({} {}() {{ ... }}), or call one that exists",
                syntax::KW_FN,
                call.name
            );
            let mut best: Option<(&str, usize)> = None;
            for cand in self
                .funcs
                .keys()
                .map(|s| s.as_str())
                .chain([syntax::BUILTIN_PRINT])
            {
                let d = edit_distance(&call.name, cand);
                if d <= 2 && best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((cand, d));
                }
            }
            if let Some((cand, _)) = best {
                fix = format!("did you mean `{}`?", cand);
            }
            self.diags.push(Diagnostic::error(
                "E0102",
                format!("nothing named `{}` exists here", call.name),
                format!(
                    "only functions that have been defined (or built in, like `{}`) can be called",
                    syntax::BUILTIN_PRINT
                ),
                fix,
                Some(call.name_span),
            ));
            for arg in call.args.iter_mut() {
                self.infer(&mut arg.expr);
            }
            return None;
        };

        if call.args.len() != sig.params.len() {
            self.diags.push(Diagnostic::error(
                "E0104",
                format!(
                    "`{}` expects {} argument{}, got {}",
                    call.name,
                    sig.params.len(),
                    if sig.params.len() == 1 { "" } else { "s" },
                    call.args.len()
                ),
                "every argument must match a parameter".to_string(),
                format!("check the definition of `{}`", call.name),
                Some(call.name_span),
            ));
        }

        let fn_type_params = self
            .m9
            .fn_params
            .get(&call.name)
            .cloned()
            .unwrap_or_default();
        let mut generic_subst = HashMap::new();
        let mut pre_inferred: Vec<Option<Type>> = Vec::new();
        if !fn_type_params.is_empty() {
            for arg in call.args.iter_mut() {
                pre_inferred.push(self.infer(&mut arg.expr));
            }
            let arg_types: Vec<Type> = pre_inferred.iter().filter_map(|t| t.clone()).collect();
            if arg_types.len() == call.args.len() {
                match self.m9.infer_fn_subst(
                    &sig,
                    &arg_types,
                    &fn_type_params,
                    self.expected_type.as_ref(),
                ) {
                    Ok(s) => generic_subst = s,
                    Err(p) => self.diags.push(e0904(call.name_span, &p)),
                }
            }
        }
        let effective_params: Vec<(AccessConvention, Type)> = if generic_subst.is_empty() {
            sig.params.clone()
        } else {
            sig
                .params
                .iter()
                .map(|(c, t)| (*c, self.m9.instantiate_type(t, &generic_subst)))
                .collect()
        };
        let args_pre_inferred =
            !generic_subst.is_empty() && pre_inferred.len() == call.args.len();

        let mut mut_borrowed: HashSet<String> = HashSet::new();
        let mut read_borrowed: HashSet<String> = HashSet::new();

        for (i, arg) in call.args.iter_mut().enumerate() {
            if let Expr::Ident(name, span) = &arg.expr {
                if mut_borrowed.contains(name) {
                    self.diags.push(aliasing_while_mut(name, *span));
                } else if arg.convention == AccessConvention::Mutate
                    && read_borrowed.contains(name)
                {
                    self.diags.push(aliasing_mut_after_read(name, *span));
                }
            }
            if !sig.is_extern {
                if let Some((AccessConvention::Read, pty)) = effective_params.get(i) {
                    if !pty.is_scalar() {
                        self.borrow_ctx = true;
                    }
                }
            } else if let Some((_, pty)) = effective_params.get(i) {
                if !pty.is_scalar() {
                    arg.flags.implicit_clone = true;
                }
            }
            let saved_exp = self.expected_type.clone();
            let saved_esc = self.lambda_escapes;
            if let Some((param_conv, param_ty)) = effective_params.get(i) {
                if matches!(param_ty, Type::Fn { .. }) {
                    self.expected_type = Some(param_ty.clone());
                    self.lambda_escapes = matches!(param_conv, AccessConvention::Move);
                }
            }
            let arg_ty = if args_pre_inferred {
                pre_inferred.get(i).and_then(|t| t.clone())
            } else {
                self.infer(&mut arg.expr)
            };
            self.expected_type = saved_exp;
            self.lambda_escapes = saved_esc;
            let Some((param_conv, param_ty)) = effective_params.get(i) else {
                continue;
            };
            if arg.convention == AccessConvention::Mutate
                && !matches!(arg.expr, Expr::Ident(_, _))
            {
                self.diags.push(Diagnostic::error(
                    "E0202",
                    format!("`{}` needs a plain `var` name after it", syntax::KW_MUTATE),
                    "only a named binding can be handed out for changing".to_string(),
                    format!(
                        "bind the value first: `{} x = ...;` then pass `{} x`",
                        syntax::KW_VAR,
                        syntax::KW_MUTATE
                    ),
                    Some(arg.span),
                ));
            }

            if let Some(arg_ty) = &arg_ty {
                let param_ty = self.resolve_type(param_ty.clone());
                let arg_ty = self.resolve_type(arg_ty.clone());
                let reported = self.check_type_assignable(&param_ty, &arg_ty, arg.expr.span());
                let compatible = arg_ty == param_ty
                    || (matches!(&param_ty, Type::Fn { .. })
                        && matches!(&arg_ty, Type::Fn { .. })
                        && fn_types_compatible(&param_ty, &arg_ty));
                if !reported && !compatible {
                    self.diags.push(Diagnostic::error(
                        "E0112",
                        format!(
                            "`{}` wants {} for argument {}, but this is {}",
                            call.name,
                            param_ty.show(),
                            i + 1,
                            arg_ty.show()
                        ),
                        "every argument must match its parameter's type".to_string(),
                        type_fix_hint(&param_ty, &arg_ty),
                        Some(arg.expr.span()),
                    ));
                }
            }

            match (param_conv, arg.convention) {
                (AccessConvention::Move, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        if is_cloneable(param_ty, self.registry, self.structs) {
                            arg.flags.implicit_clone = true;
                            self.diags.push(Diagnostic::lint(
                                "L0201",
                                format!(
                                    "implicit clone of `{}`; write `{} {}` to transfer ownership or `.clone()` to silence this warning",
                                    name,
                                    syntax::KW_MOVE,
                                    name
                                ),
                                format!(
                                    "`{}` expects to take ownership of this value",
                                    call.name
                                ),
                                format!(
                                    "write `{} {}` to move, or `{} .clone()` to copy explicitly",
                                    syntax::KW_MOVE,
                                    name,
                                    name
                                ),
                                Some(*span),
                            ));
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0201",
                                format!(
                                    "`{}` needs `{}` here — this value can't be copied",
                                    call.name,
                                    syntax::KW_MOVE
                                ),
                                format!(
                                    "parameter `{}` takes ownership; passing `{}` without `{}` would have to copy it, but this type can't be copied",
                                    i + 1,
                                    name,
                                    syntax::KW_MOVE
                                ),
                                format!(
                                    "write `{} {}` to transfer ownership",
                                    syntax::KW_MOVE,
                                    name
                                ),
                                Some(*span),
                            ));
                        }
                    }
                }
                (AccessConvention::Move, AccessConvention::Move) => {
                    // The value is given away for real.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if !param_ty.is_scalar() {
                            self.mark_moved(name.clone(), *span);
                        }
                    }
                }
                (AccessConvention::Mutate, AccessConvention::Read) => {
                    if let Expr::Ident(name, span) = &arg.expr {
                        self.diags.push(Diagnostic::error(
                            "E0202",
                            format!(
                                "parameter `{}` requires `{}` at the call site",
                                name,
                                syntax::KW_MUTATE
                            ),
                            format!(
                                "`{}` needs to change this value while it borrows it",
                                call.name
                            ),
                            format!(
                                "write `{} {}` when calling `{}`",
                                syntax::KW_MUTATE,
                                name,
                                call.name
                            ),
                            Some(*span),
                        ));
                    }
                }
                (AccessConvention::Mutate, AccessConvention::Mutate) => {
                    // `mut x` at the call site: x itself must be changeable.
                    if let Expr::Ident(name, span) = &arg.expr {
                        if let Some(info) = self.lookup(name) {
                            if !info.mutable {
                                self.diags.push(Diagnostic::error(
                                    "E0111",
                                    format!(
                                        "`{}` was made with `{}`, so it can't be changed",
                                        name,
                                        syntax::KW_VAL
                                    ),
                                    format!(
                                        "`{}` will change this value, so it must be a `{}`",
                                        call.name,
                                        syntax::KW_VAR
                                    ),
                                    format!("declare it with `{} {} = ...`", syntax::KW_VAR, name),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                }
                (
                    AccessConvention::Read | AccessConvention::Mutate,
                    AccessConvention::Move,
                ) => {
                    self.diags.push(Diagnostic::error(
                        "E0203",
                        format!(
                            "`{}` passed to a parameter that does not consume",
                            syntax::KW_MOVE
                        ),
                        "only `take` parameters accept a moved value at the call site"
                            .to_string(),
                        format!(
                            "remove `{}` or change the parameter to `take`",
                            syntax::KW_MOVE
                        ),
                        Some(arg.span),
                    ));
                }
                _ => {}
            }

            if arg.convention == AccessConvention::Mutate {
                if let Expr::Ident(name, _) = &arg.expr {
                    mut_borrowed.insert(name.clone());
                }
            }
            if let (Some((param_conv, param_ty)), Expr::Ident(name, _)) =
                (effective_params.get(i), &arg.expr)
            {
                if matches!(param_conv, AccessConvention::Read)
                    && arg.convention == AccessConvention::Read
                    && !param_ty.is_scalar()
                {
                    read_borrowed.insert(name.clone());
                }
            }

            if self.loop_depth > 0 {
                if let Expr::Ident(name, span) = &arg.expr {
                    if let Some(info) = self.lookup(name) {
                        if matches!(info.ty, Type::Shared(_)) {
                            arg.flags.shared_auto_clone = true;
                            self.diags.push(Diagnostic::lint(
                                "L0202",
                                format!(
                                    "auto-cloned `{}` inside a loop; consider hoisting or caching",
                                    name
                                ),
                                "shared handles are cloned when used across a loop boundary"
                                    .to_string(),
                                format!(
                                    "hoist `{}` before the loop, or clone once outside",
                                    name
                                ),
                                Some(*span),
                            ));
                        }
                    }
                }
            }
        }

        Some(
            sig.return_type
                .as_ref()
                .map(|t| {
                    if generic_subst.is_empty() {
                        t.clone()
                    } else {
                        self.m9.instantiate_type(t, &generic_subst)
                    }
                }),
        )
    }
}

/// Find the comparison that distribution (S25) should re-apply: descend the
/// right spine of `&&`/`||` chains; clone the comparison's left side.
fn rightmost_comparison(e: &Expr) -> Option<(Expr, BinOp)> {
    match e {
        Expr::Binary(op, _, rhs, _) if matches!(op, BinOp::And | BinOp::Or) => {
            rightmost_comparison(rhs)
        }
        Expr::Binary(op, lhs, _, _) if op.is_comparison() => Some(((**lhs).clone(), *op)),
        _ => None,
    }
}

fn compound_why(op: BinOp) -> String {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
            "`+ - * /` work on Int and Float".to_string()
        }
        _ => format!("`{}` is a whole-number operation (Int only)", op.spell()),
    }
}

/// `T?` passed where plain `T` is expected (E0310).
fn option_used_where_plain_expected(want: &Type, got: &Type) -> bool {
    matches!(got, Type::Option(inner) if want.unwrap_option().is_none() && **inner == *want)
}

fn type_fix_hint(want: &Type, got: &Type) -> String {
    match (want, got) {
        (Type::Float, Type::Int) => "write the number with a decimal part, like `2.0`".to_string(),
        (Type::Int, Type::Float) => "drop the decimal part, like `2`".to_string(),
        (Type::String, _) => "put the value in text with interpolation: \"{x}\"".to_string(),
        _ => format!("use {} here", want.show()),
    }
}

fn aliasing_while_mut(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0204",
        format!(
            "`{}` is being changed in this call, so it can't be used again here",
            name
        ),
        "while something is being changed, nobody else may be looking at it"
            .to_string(),
        format!(
            "pass `{} {}` only once, or copy first with `{} .clone()`",
            syntax::KW_MUTATE,
            name,
            name
        ),
        Some(span),
    )
}

fn aliasing_mut_after_read(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0204",
        format!(
            "`{}` is already shared in this call, so it can't be changed here too",
            name
        ),
        "while something is being looked at, nobody else may be changing it"
            .to_string(),
        format!(
            "drop the extra use of `{}`, or copy first with `{} .clone()`",
            name,
            name
        ),
        Some(span),
    )
}

fn loop_control_outside(kw: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0115",
        format!("`{}` only works inside a loop", kw),
        format!(
            "`{}` and `{}` steer the nearest `{}` or `{}` loop",
            syntax::KW_BREAK,
            syntax::KW_CONTINUE,
            syntax::KW_WHILE,
            syntax::KW_FOR
        ),
        "move this inside a loop, or remove it".to_string(),
        Some(span),
    )
}

/// Does this block definitely hit a `return` on every path?
fn block_definitely_returns(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_definitely_returns)
}

fn stmt_definitely_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_, _) => true,
        Stmt::If(ifs) => if_definitely_returns(ifs),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            arms.iter().all(|a| block_definitely_returns(&a.body))
                && else_body
                    .as_ref()
                    .map(|b| block_definitely_returns(b))
                    .unwrap_or(true)
        }
        _ => false,
    }
}

fn if_definitely_returns(ifs: &IfStmt) -> bool {
    if !block_definitely_returns(&ifs.then_body) {
        return false;
    }
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => block_definitely_returns(b),
        Some(ElseBranch::ElseIf(next)) => if_definitely_returns(next),
        None => false,
    }
}

fn is_cloneable(
    ty: &Type,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<(Option<String>, Type)>>,
) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::List(inner) | Type::Shared(inner) | Type::Option(inner) => {
            is_cloneable(inner, registry, structs)
        }
        Type::Map { key, value } => {
            is_cloneable(key, registry, structs) && is_cloneable(value, registry, structs)
        }
        Type::Result { ok, err } => is_cloneable(ok, registry, structs) && is_cloneable(err, registry, structs),
        Type::Fn { .. } => false,
        Type::Named(name) if is_type_var_name(name) || std_type_known(name) => true,
        Type::Named(name) => {
            registry.contains(name)
                && match registry.types.get(name) {
                    Some(TypeDef::Struct { fields, .. }) => fields
                        .iter()
                        .all(|(_, _, fty, is_ref, _)| !*is_ref && is_cloneable(fty, registry, structs)),
                    Some(TypeDef::Enum { variants, .. }) => variants.values().all(|(_, p)| {
                        match p {
                            VariantPayload::Unit => true,
                            VariantPayload::Single(t, _) => is_cloneable(t, registry, structs),
                            VariantPayload::Named(fs) => fs
                                .iter()
                                .all(|f| is_cloneable(&f.ty, registry, structs)),
                        }
                    }),
                    None => false,
                }
        }
        Type::Apply { args, .. } => args.iter().all(|a| is_cloneable(a, registry, structs)),
        Type::TraitObject(_) => false,
    }
}

fn walk_stmts_for_const_refs(
    stmts: &[Stmt],
    const_names: &[String],
    taken: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => walk_expr_for_const_refs(e, const_names, taken),
            Stmt::Val(b) => walk_expr_for_const_refs(&b.init, const_names, taken),
            Stmt::Assign { value, .. } => walk_expr_for_const_refs(value, const_names, taken),
            Stmt::Return(Some(e), _) => walk_expr_for_const_refs(e, const_names, taken),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => walk_if_for_const_refs(ifs, const_names, taken),
            Stmt::While { cond, body, .. } => {
                walk_expr_for_const_refs(cond, const_names, taken);
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end } => {
                        walk_expr_for_const_refs(start, const_names, taken);
                        walk_expr_for_const_refs(end, const_names, taken);
                    }
                    ForKind::In { collection } => {
                        walk_expr_for_const_refs(collection, const_names, taken);
                    }
                }
                walk_stmts_for_const_refs(body, const_names, taken);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                walk_expr_for_const_refs(subject, const_names, taken);
                for a in arms {
                    walk_expr_for_const_refs(&a.cond, const_names, taken);
                    walk_stmts_for_const_refs(&a.body, const_names, taken);
                }
                walk_stmts_for_const_refs(else_body.as_deref().unwrap_or(&[]), const_names, taken);
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Loop(inner, _) | Stmt::Unsafe(inner, _) => {
                walk_stmts_for_const_refs(inner, const_names, taken);
            }
        }
    }
}

fn walk_if_for_const_refs(ifs: &IfStmt, const_names: &[String], taken: &mut HashSet<String>) {
    walk_expr_for_const_refs(&ifs.cond, const_names, taken);
    walk_stmts_for_const_refs(&ifs.then_body, const_names, taken);
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => walk_stmts_for_const_refs(b, const_names, taken),
        Some(ElseBranch::ElseIf(next)) => walk_if_for_const_refs(next, const_names, taken),
        None => {}
    }
}

fn walk_expr_for_const_refs(expr: &Expr, const_names: &[String], taken: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name, _) => {
            if const_names.iter().any(|c| c == name) {
                taken.insert(name.clone());
            }
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(e) = p {
                    walk_expr_for_const_refs(e, const_names, taken);
                }
            }
        }
        Expr::Call(c) => {
            for a in &c.args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::Unary(_, inner, _) | Expr::Deref(inner, _) | Expr::Field(inner, _, _) => {
            walk_expr_for_const_refs(inner, const_names, taken)
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_for_const_refs(receiver, const_names, taken);
            for a in args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        walk_expr_for_const_refs(e, const_names, taken);
                    }
                }
            }
        }
        Expr::Present(inner, _) => walk_expr_for_const_refs(inner, const_names, taken),
        Expr::Absent(_) => {}
        Expr::Ok(inner, _) | Expr::Err(inner, _) | Expr::Try(inner, _) => {
            walk_expr_for_const_refs(inner, const_names, taken);
        }
        Expr::OrFallback { value, fallback, is_option, .. } => {
            walk_expr_for_const_refs(value, const_names, taken);
            match fallback {
                OrFallback::Value(e) => walk_expr_for_const_refs(e, const_names, taken),
                OrFallback::Return(Some(e), _) => walk_expr_for_const_refs(e, const_names, taken),
                OrFallback::Return(None, _) | OrFallback::Panic { .. } => {}
            }
            let _ = is_option;
        }
        Expr::PatternTest { subject, .. } => walk_expr_for_const_refs(subject, const_names, taken),
        Expr::Binary(_, l, r, _) => {
            walk_expr_for_const_refs(l, const_names, taken);
            walk_expr_for_const_refs(r, const_names, taken);
        }
        Expr::Char(_, _)
        | Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _) => {}
        Expr::ListLit(elems, _) => {
            for e in elems {
                walk_expr_for_const_refs(e, const_names, taken);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                walk_expr_for_const_refs(k, const_names, taken);
                walk_expr_for_const_refs(v, const_names, taken);
            }
        }
        Expr::Index { base, index, .. } => {
            walk_expr_for_const_refs(base, const_names, taken);
            walk_expr_for_const_refs(index, const_names, taken);
        }
        Expr::Slice { base, start, end, .. } => {
            walk_expr_for_const_refs(base, const_names, taken);
            walk_expr_for_const_refs(start, const_names, taken);
            walk_expr_for_const_refs(end, const_names, taken);
        }
        Expr::CallValue { callee, args, .. } => {
            walk_expr_for_const_refs(callee, const_names, taken);
            for a in args {
                walk_expr_for_const_refs(&a.expr, const_names, taken);
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => walk_expr_for_const_refs(e, const_names, taken),
            LambdaBody::Block(stmts) => walk_stmts_for_const_refs(stmts, const_names, taken),
        },
    }
}

fn expr_is_same_ident(a: &Expr, name: &str) -> bool {
    matches!(a, Expr::Ident(n, _) if n == name)
}

fn pattern_variant_name(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Variant { variant, .. } => Some(variant.clone()),
        Pattern::Present { .. } => Some(syntax::LIT_VALUE.to_string()),
        Pattern::Absent(_) => Some(syntax::LIT_NULL.to_string()),
        Pattern::Ok { .. } => Some(syntax::LIT_OK.to_string()),
        Pattern::Err { .. } => Some(syntax::LIT_ERR.to_string()),
    }
}

fn missing_pattern_coverage(
    subject_ty: &Type,
    covered: &HashSet<String>,
    registry: &TypeRegistry,
) -> Option<Vec<String>> {
    match subject_ty {
        Type::Named(name) => {
            let order = registry.enum_variant_order(name)?;
            let missing: Vec<_> = order
                .iter()
                .filter(|v| !covered.contains(*v))
                .cloned()
                .collect();
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        Type::Option(_) => {
            let mut missing = Vec::new();
            if !covered.contains(syntax::LIT_VALUE) {
                missing.push(syntax::LIT_VALUE.to_string());
            }
            if !covered.contains(syntax::LIT_NULL) {
                missing.push(syntax::LIT_NULL.to_string());
            }
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        Type::Result { .. } => {
            let mut missing = Vec::new();
            if !covered.contains(syntax::LIT_OK) {
                missing.push(format!("{}(...)", syntax::LIT_OK));
            }
            if !covered.contains(syntax::LIT_ERR) {
                missing.push(format!("{}(...)", syntax::LIT_ERR));
            }
            if missing.is_empty() {
                None
            } else {
                Some(missing)
            }
        }
        _ => None,
    }
}

/// `Result<T, E>` passed where plain `T` is expected (E0401).
fn result_used_where_plain_expected(want: &Type, got: &Type) -> bool {
    matches!(got, Type::Result { ok, .. } if want.unwrap_result().is_none() && **ok == *want)
}

fn pattern_binding_types(payload: &VariantPayload) -> Vec<Type> {
    match payload {
        VariantPayload::Unit => Vec::new(),
        VariantPayload::Single(t, _) => vec![t.clone()],
        VariantPayload::Named(fs) => fs.iter().map(|f| f.ty.clone()).collect(),
    }
}

fn suggest_field(name: &str, candidates: &[String]) -> Option<String> {
    let mut best: Option<(String, usize)> = None;
    for cand in candidates {
        let d = edit_distance(name, cand);
        if d <= 2 && best.as_ref().map_or(true, |(_, bd)| d < *bd) {
            best = Some((cand.clone(), d));
        }
    }
    best.map(|(s, _)| s)
}

fn is_printable(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::String | Type::Char => true,
        Type::Option(inner) => is_printable(inner, registry),
        Type::Result { ok, err } => is_printable(ok, registry) && is_printable(err, registry),
        Type::List(inner) => is_printable(inner, registry),
        Type::Map { value, .. } => is_printable(value, registry),
        Type::Named(n) => registry.contains(n) || std_type_known(n),
        Type::Apply { args, .. } => args.iter().all(|a| is_printable(a, registry)),
        Type::TraitObject(_) | Type::Shared(_) | Type::Fn { .. } => false,
    }
}

fn types_comparable(ty: &Type, registry: &TypeRegistry) -> bool {
    match ty {
        Type::Int | Type::Bool | Type::Float | Type::String | Type::Char => true,
        Type::Option(inner) => types_comparable(inner, registry),
        Type::Result { ok, err } => {
            types_comparable(ok, registry) && types_comparable(err, registry)
        }
        Type::List(inner) => types_comparable(inner, registry),
        Type::Named(name) if name == "U8" => true,
        Type::Named(name) => registry.contains(name) && incomparable_field(ty, registry).is_none(),
        Type::Apply { args, .. } => args.iter().all(|a| types_comparable(a, registry)),
        Type::TraitObject(_) | Type::Map { .. } | Type::Shared(_) | Type::Fn { .. } => false,
    }
}

fn incomparable_field(ty: &Type, registry: &TypeRegistry) -> Option<String> {
    match ty {
        Type::Named(name) => match registry.types.get(name) {
            Some(TypeDef::Struct { fields, .. }) => fields.iter().find_map(|(fname, _, fty, is_ref, _)| {
                if *is_ref || !types_comparable(fty, registry) {
                    Some(fname.clone())
                } else {
                    None
                }
            }),
            Some(TypeDef::Enum { variants, .. }) => variants.values().find_map(|(_, payload)| {
                match payload {
                    VariantPayload::Unit => None,
                    VariantPayload::Single(t, _) if !types_comparable(t, registry) => {
                        Some("payload".to_string())
                    }
                    VariantPayload::Named(fs) => fs.iter().find_map(|f| {
                        if types_comparable(&f.ty, registry) {
                            None
                        } else {
                            Some(f.name.clone())
                        }
                    }),
                    _ => None,
                }
            }),
            None => Some("?".to_string()),
        },
        Type::Option(inner) => incomparable_field(inner, registry),
        Type::Result { ok, err } => {
            incomparable_field(ok, registry).or_else(|| incomparable_field(err, registry))
        }
        _ => Some("?".to_string()),
    }
}

fn collection_changed_in_loop(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0507",
        format!("while the loop is reading `{}`, nothing may change it", name),
        "a `for` loop borrows the whole collection until the body finishes".to_string(),
        format!(
            "collect changes into a second list, or loop over indices: `for i in 0..{}.len()-1`",
            name
        ),
        Some(span),
    )
}

fn collection_root_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(n, _) => Some(n.clone()),
        Expr::MethodCall { receiver, method, .. } if method == "chars" => collection_root_name(receiver),
        _ => None,
    }
}

/// Walk `a.b[i].c` down to the root name (`a`, possibly `self`).
fn expr_root_ident(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(n, _) => Some(n),
        Expr::Field(inner, _, _) => expr_root_ident(inner),
        Expr::Index { base, .. } | Expr::Slice { base, .. } => expr_root_ident(base),
        _ => None,
    }
}

/// Types the generated Rust copies implicitly (no move on read).
fn type_is_copy(ty: &Type) -> bool {
    ty.is_scalar() || matches!(ty, Type::Char) || is_u8_ty(ty)
}

/// True when `e` is a struct-field *value* read (not enum-literal sugar like
/// `Color.Red`, not `.clone`, not an import-alias path).
fn field_read_to_clone(
    e: &Expr,
    registry: &TypeRegistry,
    imports: &HashMap<String, usize>,
) -> bool {
    match e {
        Expr::Field(inner, member, _) => {
            if member == "clone" {
                return false;
            }
            match inner.as_ref() {
                Expr::Ident(n, _) => {
                    registry.enum_variants(n).is_none() && !imports.contains_key(n)
                }
                _ => true,
            }
        }
        _ => false,
    }
}

fn builtin_type_from_ident(name: &str) -> Option<Type> {
    match name {
        syntax::TYPE_INT => Some(Type::Int),
        syntax::TYPE_FLOAT => Some(Type::Float),
        syntax::TYPE_BOOL => Some(Type::Bool),
        syntax::TYPE_STRING => Some(Type::String),
        syntax::TYPE_CHAR => Some(Type::Char),
        _ => None,
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn private_item(name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0605",
        format!("`{}` exists but is private to its file", name),
        "only names marked `pub` can be used from another file (S18)".to_string(),
        format!("add `pub` before `{}`, or don't reach across files here", name),
        Some(span),
    )
}

fn unit_ty() -> Type {
    Type::Named("Unit".to_string())
}

fn u8_ty() -> Type {
    Type::Named("U8".to_string())
}

fn is_u8_ty(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "U8")
}

fn std_type_known(name: &str) -> bool {
    matches!(
        name,
        "Unit" | "U8" | "IoError" | "Utf8Error" | "ProcessResult" | "Stopwatch"
            | "Json" | "JsonError"
    )
}

fn std_struct_field(type_name: &str, field: &str) -> Option<Type> {
    match (type_name, field) {
        ("ProcessResult", "code") => Some(Type::Int),
        ("ProcessResult", "output" | "errors") => Some(Type::String),
        ("JsonError", "line") => Some(Type::Int),
        ("JsonError", "message") => Some(Type::String),
        ("Utf8Error", "message") => Some(Type::String),
        _ => None,
    }
}

fn std_json_pattern_types(variant: &str) -> Option<Vec<Type>> {
    let json = Type::Named("Json".to_string());
    match variant {
        "Null" => Some(Vec::new()),
        "Boolean" => Some(vec![Type::Bool]),
        "Number" => Some(vec![Type::Float]),
        "Text" => Some(vec![Type::String]),
        "Array" => Some(vec![Type::List(Box::new(json.clone()))]),
        "Object" => Some(vec![Type::Map {
            key: Box::new(Type::String),
            value: Box::new(json),
        }]),
        _ => None,
    }
}

fn io_error_ty() -> Type {
    Type::Named("IoError".to_string())
}

fn json_error_ty() -> Type {
    Type::Named("JsonError".to_string())
}

fn result_ty(ok: Type, err: Type) -> Type {
    Type::Result {
        ok: Box::new(ok),
        err: Box::new(err),
    }
}

fn std_fixed_sig(
    module: &str,
    name: &str,
) -> Option<(Vec<(AccessConvention, Type)>, Option<Type>)> {
    let read = AccessConvention::Read;
    let string = Type::String;
    let int = Type::Int;
    let float = Type::Float;
    let bool_ = Type::Bool;
    let unit = unit_ty();
    let io = io_error_ty();
    let json = Type::Named("Json".to_string());
    let list_string = Type::List(Box::new(Type::String));
    let list_u8 = Type::List(Box::new(u8_ty()));
    let io_unit = result_ty(unit.clone(), io.clone());
    match (module, name) {
        ("std.fs", "read") => Some((vec![(read, string.clone())], Some(result_ty(string, io)))),
        ("std.fs", "read_bytes") => {
            Some((vec![(read, Type::String)], Some(result_ty(list_u8, io_error_ty()))))
        }
        ("std.fs", "write" | "append") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(io_unit),
        )),
        ("std.fs", "exists" | "is_dir") => {
            Some((vec![(read, Type::String)], Some(bool_)))
        }
        ("std.fs", "remove" | "create_dir") => {
            Some((vec![(read, Type::String)], Some(result_ty(unit_ty(), io_error_ty()))))
        }
        ("std.fs", "list_dir") => {
            Some((vec![(read, Type::String)], Some(result_ty(list_string, io_error_ty()))))
        }
        ("std.fs", "copy" | "rename") => Some((
            vec![(read, Type::String), (read, Type::String)],
            Some(result_ty(unit_ty(), io_error_ty())),
        )),
        ("std.io", "args") => Some((vec![], Some(Type::List(Box::new(Type::String))))),
        ("std.io", "read_all_input") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        ("std.env", "get") => Some((
            vec![(read, Type::String)],
            Some(Type::Option(Box::new(Type::String))),
        )),
        ("std.env", "set") => Some((vec![(read, Type::String), (read, Type::String)], None)),
        ("std.env", "current_dir") => {
            Some((vec![], Some(result_ty(Type::String, io_error_ty()))))
        }
        ("std.env", "home_dir") => Some((vec![], Some(Type::Option(Box::new(Type::String))))),
        ("std.process", "exit") => Some((vec![(read, int)], None)),
        ("std.process", "run") => Some((
            vec![(read, Type::List(Box::new(Type::String)))],
            Some(result_ty(Type::Named("ProcessResult".to_string()), io_error_ty())),
        )),
        ("std.math", "sqrt" | "floor" | "ceil") => {
            Some((vec![(read, float.clone())], Some(float)))
        }
        ("std.math", "pow") => Some((vec![(read, Type::Float), (read, Type::Float)], Some(Type::Float))),
        ("std.math", "round") => Some((vec![(read, Type::Float)], Some(Type::Int))),
        ("std.random", "int") => Some((vec![(read, Type::Int), (read, Type::Int)], Some(Type::Int))),
        ("std.random", "float") => Some((vec![], Some(Type::Float))),
        ("std.random", "seed") => Some((vec![(read, Type::Int)], None)),
        ("std.time", "now") => Some((vec![], Some(Type::Int))),
        ("std.time", "sleep") => Some((vec![(read, Type::Int)], None)),
        ("std.time", "start") => Some((vec![], Some(Type::Named("Stopwatch".to_string())))),
        ("std.json", "parse") => Some((
            vec![(read, Type::String)],
            Some(result_ty(json.clone(), json_error_ty())),
        )),
        ("std.json", "render" | "render_pretty") => Some((vec![(read, json)], Some(Type::String))),
        _ => {
            None
        }
    }
}

fn std_module_items(module: &str) -> Vec<String> {
    let items: &[&str] = match module {
        "std.fs" => &[
            "read", "read_bytes", "write", "append", "exists", "remove", "list_dir",
            "create_dir", "is_dir", "copy", "rename",
        ],
        "std.io" => &["args", "input", "read_all_input", "eprint"],
        "std.env" => &["get", "set", "current_dir", "home_dir"],
        "std.process" => &["exit", "run"],
        "std.math" => &[
            "sqrt", "pow", "abs", "min", "max", "floor", "ceil", "round", "pi", "e",
            "clamp",
        ],
        "std.random" => &["int", "float", "pick", "shuffle", "seed"],
        "std.time" => &["now", "sleep", "start"],
        "std.json" => &["parse", "render", "render_pretty"],
        "std" => &[],
        _ => &[],
    };
    items.iter().map(|s| s.to_string()).collect()
}

fn unknown_std_item(module: &str, name: &str, span: Span) -> Diagnostic {
    let items = std_module_items(module);
    let mut fix = if items.is_empty() {
        "import a specific std module, like `import std.fs as fs;`".to_string()
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

fn wrong_std_arity(name: &str, want: usize, got: usize, span: Span) -> Diagnostic {
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

fn u8_range_error(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E1003",
        "a U8 holds 0..255".to_string(),
        "binary APIs use one byte per value".to_string(),
        "use a number from 0 through 255".to_string(),
        Some(span),
    )
}

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    let mut states: Vec<ModuleState> = (0..bundle.modules.len())
        .map(|_| ModuleState {
            funcs: HashMap::new(),
            func_pub: HashMap::new(),
            type_pub: HashMap::new(),
            method_pub: HashMap::new(),
            field_pub: HashMap::new(),
            registry: TypeRegistry {
                types: HashMap::new(),
            },
            structs: HashMap::new(),
            consts: HashMap::new(),
            imports: HashMap::new(),
            std_imports: HashMap::new(),
            tests: HashMap::new(),
            m9: M9Registry::default(),
        })
        .collect();

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for item in &module.items {
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags),
                Item::Struct(s) => {
                    register_struct(s, &mut st.registry, &mut st.structs, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(s.name.clone(), s.is_pub);
                    for fld in &s.fields {
                        st.field_pub.insert((s.name.clone(), fld.name.clone()), fld.is_pub);
                    }
                    for m in &s.methods {
                        st.method_pub.insert((s.name.clone(), m.name.clone()), m.is_pub);
                    }
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(e.name.clone(), e.is_pub);
                    for m in &e.methods {
                        st.method_pub.insert((e.name.clone(), m.name.clone()), m.is_pub);
                    }
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
                        diags.push(Diagnostic::error(
                            "E0301",
                            format!("`impl {}` names a type that doesn't exist", i.type_name),
                            format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                            format!("define `struct {}` or `enum {}` first", i.type_name, i.type_name),
                            Some(i.type_span),
                        ));
                    } else if !i.type_name.contains('.') {
                        for m in &i.methods {
                            st.method_pub.insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                        }
                    }
                }
                Item::Const(c) => register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry),
                Item::Test(t) => {
                    if name_defined(&t.name, &st.funcs, &st.registry, &st.consts)
                        || st.tests.contains_key(&t.name)
                    {
                        diags.push(defined_twice(
                            &t.name,
                            "every test needs a unique name so failures are easy to find",
                            t.name_span,
                        ));
                    } else {
                        st.tests.insert(t.name.clone(), t.name_span);
                    }
                }
                Item::ExternRust(block) => {
                    if check_extern_block(block, &st.registry, &mut diags) {
                        for ef in &block.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                            );
                        }
                    }
                }
                Item::Trait(_) => {}
            }
        }
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        st.m9.register_items(&module.items, &mut diags);
    }

    // S57 (M9.5): evaluate comptime bindings per module. `embed_file` paths
    // resolve against each module file's own directory (S16 convention).
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let base = module
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_comptime_items(&mut module.items, &mut states[idx].consts, &base, &mut diags);
    }

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            let alias = loader::import_alias(imp);
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.std_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let Some(module) = loader::std_module_path(imp) {
                if !loader::is_known_std_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no standard module `{}`", module),
                        "`std` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", loader::std_modules_list()),
                        Some(imp.span),
                    ));
                    continue;
                }
                st.std_imports.insert(alias, module);
                continue;
            }
            match loader::resolve_import_target(bundle, idx, imp) {
                Ok(target) => {
                    st.imports.insert(alias, target);
                }
                Err(d) => diags.push(d),
            }
        }
    }

    for idx in 0..bundle.modules.len() {
        for item in &bundle.modules[idx].items {
            let Item::Impl(i) = item else { continue };
            if !i.type_name.contains('.') {
                continue;
            }
            if !impl_type_exists(
                &i.type_name,
                &states[idx].registry,
                &states[idx].imports,
                Some(&states),
            ) {
                diags.push(Diagnostic::error(
                    "E0301",
                    format!("`impl {}` names a type that doesn't exist", i.type_name),
                    format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                    format!("define `struct {}` or `enum {}` first", i.type_name, i.type_name),
                    Some(i.type_span),
                ));
            } else {
                for m in &i.methods {
                    states[idx]
                        .method_pub
                        .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                }
            }
        }
    }

    // Parity with the single-file path: `@static` and address-taken consts
    // must lower to Rust `static` in bundle mode too.
    for module in bundle.modules.iter_mut() {
        let const_names: Vec<String> = module
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Const(c) => Some(c.name.clone()),
                _ => None,
            })
            .collect();
        let mut address_taken: HashSet<String> = HashSet::new();
        for item in &module.items {
            match item {
                Item::Func(f) => {
                    walk_stmts_for_const_refs(&f.body, &const_names, &mut address_taken)
                }
                Item::Struct(s) => {
                    for m in &s.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        walk_stmts_for_const_refs(&m.body, &const_names, &mut address_taken);
                    }
                }
            Item::Test(t) => {
                walk_stmts_for_const_refs(&t.body, &const_names, &mut address_taken)
            }
            Item::Const(_) | Item::ExternRust(_) | Item::Trait(_) => {}
        }
    }
        for item in &mut module.items {
            if let Item::Const(c) = item {
                let force_static = c.attrs.contains(&ConstAttr::ForceStatic);
                c.rust_kind = if force_static || address_taken.contains(&c.name) {
                    RustConstKind::Static
                } else {
                    RustConstKind::Const
                };
            }
        }
    }

    // Each non-entry module becomes a Rust `mod user_<alias>`; a type in the
    // entry file with the same name would collide in the type namespace.
    for (idx, m) in bundle.modules.iter().enumerate() {
        if idx == bundle.entry {
            continue;
        }
        if states[bundle.entry].registry.contains(&m.alias) {
            diags.push(Diagnostic::error(
                "E0105",
                format!(
                    "the type `{}` clashes with the imported file `{}`",
                    m.alias, m.display
                ),
                "a type and an imported module can't share a name".to_string(),
                format!("rename the type, or import with `{} other_name`", syntax::KW_AS),
                None,
            ));
        }
    }

    let entry = &states[bundle.entry];
    if mode == CompileMode::Run {
        if !entry.funcs.contains_key("main") {
            diags.push(Diagnostic::error(
                "E0101",
                "this program has no `main` function".to_string(),
                "running a program starts at `fn main`, and the entry file doesn't define one"
                    .to_string(),
                "add `fn main() { ... }` to the entry file".to_string(),
                None,
            ));
        } else if let Some(sig) = entry.funcs.get("main") {
            if !sig.params.is_empty() || sig.return_type.is_some() {
                diags.push(Diagnostic::error(
                    "E0122",
                    "`main` takes no parameters and returns nothing".to_string(),
                    "`main` is where running starts; nothing calls it with values".to_string(),
                    "write it as: fn main() { ... }".to_string(),
                    None,
                ));
            }
        }
    }
    match mode {
        CompileMode::Test if entry.tests.is_empty() => {
            diags.push(Diagnostic::error(
                "E0601",
                format!("no `{}` blocks found to run", syntax::KW_TEST),
                format!(
                    "add at least one top-level block: {} \"describes what this checks\" {{ ... }}",
                    syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    syntax::BUILTIN_REQUIRE,
                    syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        CompileMode::Test | CompileMode::Run | CompileMode::Check => {}
    }

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        diags.extend(check_module_bodies(
            module,
            idx,
            &states,
            mode,
        ));
    }
    bundle.used_std = collect_used_std(bundle, &states);
    diags
}

fn register_func_item(f: &Func, st: &mut ModuleState, diags: &mut Vec<Diagnostic>) {
    if f.name == syntax::BUILTIN_PRINT
        || f.name == syntax::BUILTIN_PANIC
        || f.name == syntax::BUILTIN_REQUIRE
        || f.name == syntax::BUILTIN_REQUIRE_EQ
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
    st.func_pub.insert(f.name.clone(), f.is_pub);
    st.funcs.insert(f.name.clone(), func_to_sig(f));
}

fn collect_used_std(bundle: &ProgramBundle, states: &[ModuleState]) -> HashSet<String> {
    let mut used = HashSet::new();
    for (idx, module) in bundle.modules.iter().enumerate() {
        let imports = &states[idx].std_imports;
        for item in &module.items {
            match item {
                Item::Func(f) => collect_std_stmts(&f.body, imports, &mut used),
                Item::Struct(s) => {
                    for m in &s.methods {
                        collect_std_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        collect_std_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        collect_std_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Test(t) => collect_std_stmts(&t.body, imports, &mut used),
                Item::Const(c) => collect_std_expr(&c.value, imports, &mut used),
                Item::Trait(_) | Item::ExternRust(_) => {}
            }
        }
    }
    used
}

fn collect_std_stmts(
    stmts: &[Stmt],
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => collect_std_expr(e, imports, used),
            Stmt::Val(b) => collect_std_expr(&b.init, imports, used),
            Stmt::Assign { target, value, .. } => {
                collect_std_lvalue(target, imports, used);
                collect_std_expr(value, imports, used);
            }
            Stmt::Return(Some(e), _) => collect_std_expr(e, imports, used),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => collect_std_if(ifs, imports, used),
            Stmt::While { cond, body, .. } => {
                collect_std_expr(cond, imports, used);
                collect_std_stmts(body, imports, used);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end } => {
                        collect_std_expr(start, imports, used);
                        collect_std_expr(end, imports, used);
                    }
                    ForKind::In { collection } => collect_std_expr(collection, imports, used),
                }
                collect_std_stmts(body, imports, used);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                collect_std_expr(subject, imports, used);
                for arm in arms {
                    collect_std_expr(&arm.cond, imports, used);
                    collect_std_stmts(&arm.body, imports, used);
                }
                if let Some(body) = else_body {
                    collect_std_stmts(body, imports, used);
                }
            }
            Stmt::Loop(body, _) | Stmt::Unsafe(body, _) => collect_std_stmts(body, imports, used),
            Stmt::Break(_) | Stmt::Continue(_) => {}
        }
    }
}

fn collect_std_if(ifs: &IfStmt, imports: &HashMap<String, String>, used: &mut HashSet<String>) {
    collect_std_expr(&ifs.cond, imports, used);
    collect_std_stmts(&ifs.then_body, imports, used);
    match &ifs.else_branch {
        Some(ElseBranch::Else(body)) => collect_std_stmts(body, imports, used),
        Some(ElseBranch::ElseIf(next)) => collect_std_if(next, imports, used),
        None => {}
    }
}

fn collect_std_lvalue(
    lv: &LValue,
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
) {
    match lv {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => {
            collect_std_expr(base, imports, used);
            collect_std_expr(index, imports, used);
        }
    }
}

fn collect_std_expr(expr: &Expr, imports: &HashMap<String, String>, used: &mut HashSet<String>) {
    match expr {
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if matches!(receiver.as_ref(), Expr::Ident(n, _) if n == "Json") {
                used.insert("core::json".to_string());
            }
            if matches!(
                method.as_str(),
                "bytes" | "from_bytes" | "to_u8" | "elapsed_millis"
            ) {
                used.insert(format!("core::{method}"));
            }
            if let Expr::Ident(alias, _) = receiver.as_ref() {
                if let Some(module) = imports.get(alias) {
                    used.insert(format!("{module}::{method}"));
                }
            }
            collect_std_expr(receiver, imports, used);
            for arg in args {
                collect_std_expr(&arg.expr, imports, used);
            }
        }
        Expr::Call(c) => {
            for arg in &c.args {
                collect_std_expr(&arg.expr, imports, used);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_std_expr(callee, imports, used);
            for arg in args {
                collect_std_expr(&arg.expr, imports, used);
            }
        }
        Expr::Field(inner, member, _) => {
            if matches!(inner.as_ref(), Expr::Ident(n, _) if n == "Json")
                && member == "Null"
            {
                used.insert("core::json".to_string());
            }
            collect_std_expr(inner, imports, used);
        }
        Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _) => collect_std_expr(inner, imports, used),
        Expr::Binary(_, lhs, rhs, _)
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            collect_std_expr(lhs, imports, used);
            collect_std_expr(rhs, imports, used);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_std_expr(base, imports, used);
            collect_std_expr(start, imports, used);
            collect_std_expr(end, imports, used);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(e) = part {
                    collect_std_expr(e, imports, used);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for e in items {
                collect_std_expr(e, imports, used);
            }
        }
        Expr::MapLit(items, _) => {
            for (k, v) in items {
                collect_std_expr(k, imports, used);
                collect_std_expr(v, imports, used);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                collect_std_expr(e, imports, used);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    EnumLitArg::Positional(e) => collect_std_expr(e, imports, used),
                    EnumLitArg::Named { expr, .. } => collect_std_expr(expr, imports, used),
                }
            }
        }
        Expr::PatternTest { subject, .. } => collect_std_expr(subject, imports, used),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_std_expr(value, imports, used);
            match fallback {
                OrFallback::Value(e) => collect_std_expr(e, imports, used),
                OrFallback::Return(Some(e), _) => collect_std_expr(e, imports, used),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_std_expr(&arg.expr, imports, used);
                    }
                }
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => collect_std_expr(e, imports, used),
            LambdaBody::Block(stmts) => collect_std_stmts(stmts, imports, used),
        },
        Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_) => {}
    }
}

fn check_module_bodies(
    module: &mut crate::ast::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    mode: CompileMode,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut diags = Vec::new();
    let (ct_funcs, ct_externs, ct_globals) = comptime_context_from_items(&module.items);
    let ct_base_dir = module
        .path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
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
                ));
            }
            Item::Struct(s) => {
                for m in &mut s.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&s.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                    ));
                }
            }
            Item::Enum(e) => {
                for m in &mut e.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&e.name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                    ));
                }
            }
            Item::Impl(i) => {
                for m in &mut i.methods {
                    diags.extend(check_func_body_bundle(
                        m,
                        module_idx,
                        states,
                        Some(&i.type_name),
                        &ct_funcs,
                        &ct_externs,
                        &ct_base_dir,
                        &ct_globals,
                    ));
                }
            }
            Item::Test(t) if mode == CompileMode::Test => {
                let mut synthetic = Func {
                    is_pub: false,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    is_view_return: false,
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
                ));
                t.body = synthetic.body;
            }
            _ => {}
        }
    }
    let _ = st;
    diags
}

fn check_func_body_bundle(
    f: &mut Func,
    module_idx: usize,
    states: &[ModuleState],
    owner_type: Option<&str>,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::comptime::CtValue>,
) -> Vec<Diagnostic> {
    let st = &states[module_idx];
    let mut ck = Checker {
        funcs: &st.funcs,
        registry: &st.registry,
        structs: &st.structs,
        consts: &st.consts,
        modules: Some(states),
        module_idx,
        imports: &st.imports,
        std_imports: &st.std_imports,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        in_unsafe: false,
        ret: f.return_type.clone(),
        view_return: f.is_view_return,
        fn_name: f.name.clone(),
        expected_type: None,
        owner_type: owner_type.map(str::to_string),
        iter_borrowed: HashSet::new(),
        borrow_ctx: false,
        lambda_escapes: true,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        m9: &st.m9,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
    };
    ck.check_params_and_body(f, owner_type);
    ck.diags
}

fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
        ret: sig.return_type.clone().map(Box::new),
    }
}

fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let (Type::Fn {
        params: wp,
        ret: wr,
    }, Type::Fn {
        params: gp,
        ret: gr,
    }) = (want, got)
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

fn lambda_body_refs_name(body: &LambdaBody, name: &str) -> bool {
    match body {
        LambdaBody::Expr(e) => expr_refs_name(e, name),
        LambdaBody::Block(stmts) => stmts.iter().any(|s| stmt_refs_name(s, name)),
    }
}

fn expr_refs_name(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n, _) => n == name,
        Expr::Unary(_, inner, _) => expr_refs_name(inner, name),
        Expr::Binary(_, l, r, _) => expr_refs_name(l, name) || expr_refs_name(r, name),
        Expr::Call(c) => c.args.iter().any(|a| expr_refs_name(&a.expr, name)),
        Expr::CallValue { callee, args, .. } => {
            expr_refs_name(callee, name) || args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::Field(inner, _, _) | Expr::Present(inner, _) | Expr::Try(inner, _) => {
            expr_refs_name(inner, name)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_refs_name(receiver, name) || args.iter().any(|a| expr_refs_name(&a.expr, name))
        }
        Expr::Index { base, index, .. } => expr_refs_name(base, name) || expr_refs_name(index, name),
        Expr::Slice { base, start, end, .. } => {
            expr_refs_name(base, name) || expr_refs_name(start, name) || expr_refs_name(end, name)
        }
        Expr::ListLit(elems, _) => elems.iter().any(|el| expr_refs_name(el, name)),
        Expr::MapLit(entries, _) => entries
            .iter()
            .any(|(k, v)| expr_refs_name(k, name) || expr_refs_name(v, name)),
        Expr::StructLit { fields, .. } => fields.iter().any(|(_, _, f)| expr_refs_name(f, name)),
        Expr::EnumLit { args, .. } => args.iter().any(|a| match a {
            EnumLitArg::Positional(e) => expr_refs_name(e, name),
            EnumLitArg::Named { expr, .. } => expr_refs_name(expr, name),
        }),
        Expr::Ok(inner, _) | Expr::Err(inner, _) => expr_refs_name(inner, name),
        Expr::OrFallback { value, fallback, .. } => {
            expr_refs_name(value, name)
                || match fallback {
                    OrFallback::Value(e) => expr_refs_name(e, name),
                    OrFallback::Return(Some(e), _) => expr_refs_name(e, name),
                    _ => false,
                }
        }
        Expr::PatternTest { subject, .. } => expr_refs_name(subject, name),
        Expr::Lambda(_) => false,
        Expr::Str(parts, _) => parts.iter().any(|p| {
            if let StrPart::Interp(e) = p {
                expr_refs_name(e, name)
            } else {
                false
            }
        }),
        Expr::Int(_, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Absent(_)
        | Expr::Deref(_, _) => false,
    }
}

fn stmt_refs_name(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(e) => expr_refs_name(e, name),
        Stmt::Val(b) => expr_refs_name(&b.init, name),
        Stmt::Assign { target, value, .. } => {
            lvalue_refs_name(target, name) || expr_refs_name(value, name)
        }
        Stmt::Return(Some(e), _) => expr_refs_name(e, name),
        Stmt::If(i) => {
            expr_refs_name(&i.cond, name)
                || i.then_body.iter().any(|s| stmt_refs_name(s, name))
                || i.else_branch.as_ref().is_some_and(|e| else_refs_name(e, name))
        }
        Stmt::While { cond, body, .. } => {
            expr_refs_name(cond, name) || body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::For { kind, body, .. } => {
            let coll = match kind {
                ForKind::Range { start, end } => {
                    expr_refs_name(start, name) || expr_refs_name(end, name)
                }
                ForKind::In { collection } => expr_refs_name(collection, name),
            };
            coll || body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_refs_name(subject, name)
                || arms.iter().any(|a| {
                    expr_refs_name(&a.cond, name)
                        || a.body.iter().any(|s| stmt_refs_name(s, name))
                })
                || else_body
                    .as_ref()
                    .is_some_and(|b| b.iter().any(|s| stmt_refs_name(s, name)))
        }
        Stmt::Loop(body, _) | Stmt::Unsafe(body, _) => {
            body.iter().any(|s| stmt_refs_name(s, name))
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Return(None, _) => false,
    }
}

fn else_refs_name(e: &ElseBranch, name: &str) -> bool {
    match e {
        ElseBranch::Else(stmts) => stmts.iter().any(|s| stmt_refs_name(s, name)),
        ElseBranch::ElseIf(i) => {
            expr_refs_name(&i.cond, name)
                || i.then_body.iter().any(|s| stmt_refs_name(s, name))
                || i.else_branch.as_ref().is_some_and(|e| else_refs_name(e, name))
        }
    }
}

fn lvalue_refs_name(lv: &LValue, name: &str) -> bool {
    match lv {
        LValue::Local { name: n, .. } => n == name,
        LValue::Index { base, index, .. } => {
            expr_refs_name(base, name) || expr_refs_name(index, name)
        }
    }
}

fn lambda_collect_captures(
    body: &LambdaBody,
    params: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match body {
        LambdaBody::Expr(e) => expr_collect_captures(e, params, read, mut_cap),
        LambdaBody::Block(stmts) => {
            for s in stmts {
                stmt_collect_captures(s, params, read, mut_cap);
            }
        }
    }
}

fn expr_collect_captures(
    e: &Expr,
    params: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match e {
        Expr::Ident(n, _) if !params.contains(n) => {
            read.insert(n.clone());
        }
        Expr::Unary(_, inner, _) => expr_collect_captures(inner, params, read, mut_cap),
        Expr::Binary(_, l, r, _) => {
            expr_collect_captures(l, params, read, mut_cap);
            expr_collect_captures(r, params, read, mut_cap);
        }
        Expr::Call(c) => {
            for a in &c.args {
                expr_collect_captures(&a.expr, params, read, mut_cap);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            expr_collect_captures(callee, params, read, mut_cap);
            for a in args {
                expr_collect_captures(&a.expr, params, read, mut_cap);
            }
        }
        Expr::Field(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Try(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _) => expr_collect_captures(inner, params, read, mut_cap),
        Expr::MethodCall { receiver, args, .. } => {
            expr_collect_captures(receiver, params, read, mut_cap);
            for a in args {
                expr_collect_captures(&a.expr, params, read, mut_cap);
            }
        }
        Expr::Index { base, index, .. } => {
            expr_collect_captures(base, params, read, mut_cap);
            expr_collect_captures(index, params, read, mut_cap);
        }
        Expr::Slice { base, start, end, .. } => {
            expr_collect_captures(base, params, read, mut_cap);
            expr_collect_captures(start, params, read, mut_cap);
            expr_collect_captures(end, params, read, mut_cap);
        }
        Expr::ListLit(elems, _) => {
            for el in elems {
                expr_collect_captures(el, params, read, mut_cap);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries {
                expr_collect_captures(k, params, read, mut_cap);
                expr_collect_captures(v, params, read, mut_cap);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, f) in fields {
                expr_collect_captures(f, params, read, mut_cap);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args {
                match a {
                    EnumLitArg::Positional(ex) => expr_collect_captures(ex, params, read, mut_cap),
                    EnumLitArg::Named { expr, .. } => {
                        expr_collect_captures(expr, params, read, mut_cap);
                    }
                }
            }
        }
        Expr::OrFallback { value, fallback, .. } => {
            expr_collect_captures(value, params, read, mut_cap);
            match fallback {
                OrFallback::Value(ex) => expr_collect_captures(ex, params, read, mut_cap),
                OrFallback::Return(Some(ex), _) => {
                    expr_collect_captures(ex, params, read, mut_cap);
                }
                _ => {}
            }
        }
        Expr::PatternTest { subject, .. } => expr_collect_captures(subject, params, read, mut_cap),
        Expr::Str(parts, _) => {
            for p in parts {
                if let StrPart::Interp(ex) = p {
                    expr_collect_captures(ex, params, read, mut_cap);
                }
            }
        }
        Expr::Lambda(_) => {}
        _ => {}
    }
}

fn stmt_collect_captures(
    stmt: &Stmt,
    params: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match stmt {
        Stmt::Expr(e) => expr_collect_captures(e, params, read, mut_cap),
        Stmt::Val(b) => expr_collect_captures(&b.init, params, read, mut_cap),
        Stmt::Assign { target, value, .. } => {
            if let LValue::Local { name, .. } = target {
                if !params.contains(name) {
                    mut_cap.insert(name.clone());
                }
            } else if let LValue::Index { base, index, .. } = target {
                expr_collect_captures(base, params, read, mut_cap);
                expr_collect_captures(index, params, read, mut_cap);
                if let Expr::Ident(n, _) = base.as_ref() {
                    if !params.contains(n) {
                        mut_cap.insert(n.clone());
                    }
                }
            }
            expr_collect_captures(value, params, read, mut_cap);
        }
        Stmt::Return(Some(e), _) => expr_collect_captures(e, params, read, mut_cap),
        Stmt::If(i) => {
            expr_collect_captures(&i.cond, params, read, mut_cap);
            for s in &i.then_body {
                stmt_collect_captures(s, params, read, mut_cap);
            }
            if let Some(e) = &i.else_branch {
                else_collect_captures(e, params, read, mut_cap);
            }
        }
        Stmt::While { cond, body, .. } => {
            expr_collect_captures(cond, params, read, mut_cap);
            for s in body {
                stmt_collect_captures(s, params, read, mut_cap);
            }
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range { start, end } => {
                    expr_collect_captures(start, params, read, mut_cap);
                    expr_collect_captures(end, params, read, mut_cap);
                }
                ForKind::In { collection } => {
                    expr_collect_captures(collection, params, read, mut_cap);
                }
            }
            for s in body {
                stmt_collect_captures(s, params, read, mut_cap);
            }
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_collect_captures(subject, params, read, mut_cap);
            for a in arms {
                expr_collect_captures(&a.cond, params, read, mut_cap);
                for s in &a.body {
                    stmt_collect_captures(s, params, read, mut_cap);
                }
            }
            if let Some(b) = else_body {
                for s in b {
                    stmt_collect_captures(s, params, read, mut_cap);
                }
            }
        }
        Stmt::Loop(body, _) | Stmt::Unsafe(body, _) => {
            for s in body {
                stmt_collect_captures(s, params, read, mut_cap);
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Return(None, _) => {}
    }
}

fn else_collect_captures(
    e: &ElseBranch,
    params: &HashSet<String>,
    read: &mut HashSet<String>,
    mut_cap: &mut HashSet<String>,
) {
    match e {
        ElseBranch::Else(stmts) => {
            for s in stmts {
                stmt_collect_captures(s, params, read, mut_cap);
            }
        }
        ElseBranch::ElseIf(i) => {
            expr_collect_captures(&i.cond, params, read, mut_cap);
            for s in &i.then_body {
                stmt_collect_captures(s, params, read, mut_cap);
            }
            if let Some(e) = &i.else_branch {
                else_collect_captures(e, params, read, mut_cap);
            }
        }
    }
}
