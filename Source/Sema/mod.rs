//! Semantic checks. Everything here exists so that codegen can stay "dumb"
//! (invariant I3): by the time a Program reaches codegen, it must be
//! impossible for the generated Rust to fail to compile (invariant I2).
//!
//! M1: type inference, mutability, comparison distribution (S25),
//! definite-return analysis. M2: ownership — moves, call-site `mut`/`take`,
//! view returns, use-after-move, and borrow rules that keep generated Rust
//! sound without surfacing Rust concepts to users.

use crate::AST::{
    AccessConvention,
    ExternFn, Func, Type, VariantPayload,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::M9::M9Registry;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct FuncSig {
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub is_view_return: bool,
    /// S50: declared in `extern rust`, implemented by the FFI bridge.
    pub is_extern: bool,
    /// S58 (E2-M13): `@unsafe fn` — calling it requires an enclosing `@unsafe`
    /// block (E3103).
    pub is_unsafe: bool,
    /// S60 (E2-M16): `pure fn` — this function is free of ambient I/O and
    /// non-determinism. Call sites inside a `pure fn` must also be pure (E3401).
    pub is_pure: bool,
    /// S61: parameter names and default-value presence, parallel to `params`.
    /// Empty for extern/built-in functions (no label checking needed there).
    pub param_info: Vec<(String, bool)>,
    /// S61: default expressions for parameters that have them, parallel to `params`.
    /// `None` when no default; only trailing params may have defaults.
    pub defaults: Vec<Option<crate::AST::Expr>>,
}

#[derive(Debug, Clone)]
pub(crate) struct MethodSig {
    params: Vec<(AccessConvention, Type)>,
    return_type: Option<Type>,
    is_view_return: bool,
    is_static: bool,
    self_conv: Option<AccessConvention>,
}

#[derive(Debug, Clone)]
pub(crate) enum TypeDef {
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

pub(crate) struct TypeRegistry {
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
            Some(TypeDef::Struct { fields, .. }) => {
                fields.iter().map(|(n, ..)| n.clone()).collect()
            }
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
        param_info: f
            .params
            .iter()
            .map(|p| (p.name.clone(), p.default.is_some()))
            .collect(),
        defaults: f
            .params
            .iter()
            .map(|p| p.default.as_ref().map(|d| *d.clone()))
            .collect(),
        return_type: f.return_type.clone(),
        is_view_return: f.is_view_return,
        is_extern: false,
        is_unsafe: f.is_unsafe,
        is_pure: f.is_pure,
    }
}

fn extern_to_sig(ef: &ExternFn) -> FuncSig {
    FuncSig {
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        param_info: ef
            .params
            .iter()
            .map(|p| (p.name.clone(), false))
            .collect(),
        defaults: ef.params.iter().map(|_| None).collect(),
        return_type: ef.return_type.clone(),
        is_view_return: ef.is_view_return,
        is_extern: true,
        is_unsafe: false,
        is_pure: false, // extern functions are always considered impure
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LocalInfo {
    ty: Type,
    mutable: bool,
    /// Set when the name is a parameter (with its access convention).
    param_conv: Option<AccessConvention>,
    /// Loop nesting depth where the name was declared (for move-in-loop).
    decl_loop_depth: usize,
    /// Whether this local can cross a task/channel boundary. For ordinary
    /// values this follows the type; for lambdas it also includes captures.
    sendable: bool,
    /// Binding span for a Task value that must be consumed with `.join()`.
    task_lint_span: Option<Span>,
}

#[derive(Debug, Clone)]
pub(crate) enum SendProblemKind {
    RefField,
    ClosureNeedsTake,
    ClosureCaptures,
    TraitValue(String),
    ViewBorrow,
}

#[derive(Debug, Clone)]
pub(crate) struct SendabilityProblem {
    root: Option<String>,
    path: Vec<String>,
    kind: SendProblemKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SendCrossing {
    TaskCapture,
    TaskResult,
    ChannelSend,
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

pub(crate) struct ModuleState {
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
    /// D-MOD2: inline code module aliases present in this file (alias → module name).
    /// `math.double(x)` resolves to `user_math__double(x)` when `math` is in here.
    code_modules: HashMap<String, String>,
    /// D-MOD3: unqualified items imported via `use alias.Item` (inline modules).
    /// Maps unqualified name → mangled name (e.g. "clamp" → "math__clamp").
    unqualified: HashMap<String, String>,
    /// D-MOD3: unqualified file-module items imported via `use alias.Item`.
    /// Maps name → (function_name, module_idx).
    unqualified_file: HashMap<String, (String, usize)>,
    /// D-MOD4: `pub use alias.Item` re-exports — items this module exposes on its
    /// own public surface even though they're defined elsewhere. Maps the
    /// exported name → (target_function_name, target_module_idx). A caller doing
    /// `thismod.Item` resolves through here to the real definition.
    reexports: HashMap<String, (String, usize)>,
}

pub(crate) struct Checker<'a> {
    funcs: &'a HashMap<String, FuncSig>,
    registry: &'a TypeRegistry,
    structs: &'a HashMap<String, Vec<(Option<String>, Type)>>,
    consts: &'a HashMap<String, Type>,
    modules: Option<&'a [ModuleState]>,
    module_idx: usize,
    imports: &'a HashMap<String, usize>,
    std_imports: &'a HashMap<String, String>,
    /// D-MOD2: inline code module aliases in scope (alias → module name).
    code_modules: &'a HashMap<String, String>,
    /// D-MOD3: unqualified inline-module items in scope (name → mangled name).
    unqualified: &'a HashMap<String, String>,
    /// D-MOD3: unqualified file-module items in scope (name → (fn_name, module_idx)).
    unqualified_file: &'a HashMap<String, (String, usize)>,
    /// D-MOD2: pub flags for this module's functions, including inline-module
    /// items mangled as `M__item`. Used to reject `M.private()` from outside.
    func_pub: &'a HashMap<String, bool>,
    diags: Vec<Diagnostic>,
    scopes: Vec<HashMap<String, LocalInfo>>,
    /// name -> span of the use that gave the value away.
    moved: HashMap<String, Span>,
    loop_depth: usize,
    /// D-LABEL1: stack of `@name` loop labels in scope, innermost last.
    loop_labels: Vec<String>,
    in_unsafe: bool,
    /// True while checking a `pure fn` body, so E3403 can fire on a
    /// non-deterministic std call (time/random) reached from pure code.
    in_pure: bool,
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
    /// M11: when true, lambda is being passed to tasks.spawn — stricter capture rules (E1101).
    is_task_spawn: bool,
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
    ct_globals: &'a HashMap<String, crate::Comptime::CtValue>,
    ct_scopes: Vec<HashMap<String, crate::Comptime::CtValue>>,
    /// Active generic type parameters while checking a generic item.
    type_param_scope: Vec<crate::AST::TypeParam>,
    /// E2-M15: reject OS-dependent std APIs in `--freestanding` builds (E3301).
    freestanding: bool,
}


mod FFI;
mod Registration;
mod Bundle;
mod CheckerCore;
mod CheckerInfer;
mod CheckerStdlib;
mod CheckerOwnership;
mod CheckerItems;
mod Diagnostics;
mod Captures;
mod Purity;

pub(crate) use FFI::*;
pub(crate) use Registration::*;
pub(crate) use Bundle::*;
pub(crate) use CheckerStdlib::*;
pub(crate) use Diagnostics::*;
pub(crate) use Captures::*;
pub(crate) use Purity::*;

// Public entry points (preserve `jet::Sema::<item>` paths).
pub use Registration::{check, check_with_mode};
pub use Bundle::{check_bundle, check_bundle_freestanding};
pub use FFI::{e3202, e3301, e3302, e3303};
pub use Purity::{check_pure_fn, e3401, e3402, e3403};
