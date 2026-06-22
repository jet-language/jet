use super::*;
use crate::AST::{
    AccessConvention, ConstAttr, DistinctDef, EnumDef,
    Expr, Func, Item, Param, Program, RustConstKind, StructDef, Type,
};
use crate::Collections::is_reserved_type;
use crate::Diagnostics::{Diagnostic, Span};
use crate::M9::M9Registry;
use crate::Syntax;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    /// Shared tail of `check_func_body` / `check_func_body_bundle`:
    /// declare parameters, check the body, enforce definite return.
    pub(crate) fn check_params_and_body(&mut self, f: &mut Func, owner_type: Option<&str>) {
        for p in &f.params {
            let skip_type_check =
                p.name == Syntax::KW_SELF && matches!(&p.ty, Type::Named(n) if n.is_empty());
            if !skip_type_check {
                let pty = self.resolve_type(p.ty.clone());
                self.check_declared_type(&pty, p.ty_span);
            }
            if p.name == Syntax::KW_SELF {
                if let Some(owner) = owner_type {
                    let self_ty = Type::Named(owner.to_string());
                    self.scopes.last_mut().unwrap().insert(
                        p.name.clone(),
                        LocalInfo {
                            ty: self_ty,
                            mutable: matches!(p.convention, AccessConvention::Mutate),
                            param_conv: Some(p.convention),
                            decl_loop_depth: 0,
                            sendable: true,
                            task_lint_span: None,
                            task_has_view_capture: false,
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
                        sendable: true,
                        task_lint_span: None,
                        task_has_view_capture: false,
                    },
                );
            }
        }
        self.check_block(&mut f.body, false);
        self.lint_unjoined_tasks_in_current_scope();
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
                    Syntax::KW_ELSE
                ),
                Some(f.name_span),
            ));
        }
    }
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
                if f.name == Syntax::BUILTIN_PRINT
                    || f.name == Syntax::BUILTIN_PANIC
                    || f.name == Syntax::BUILTIN_REQUIRE
                    || f.name == Syntax::BUILTIN_REQUIRE_EQ
                    || f.name == Syntax::BUILTIN_EXPECT
                {
                    diags.push(Diagnostic::error(
                        "E0106",
                        format!("the name `{}` is built in and can't be redefined", f.name),
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
                    // L2401: advisory — public fn with a positional Bool parameter.
                    if f.is_pub {
                        for (idx, p) in f.params.iter().enumerate() {
                            if matches!(p.ty, Type::Bool)
                                && p.name != Syntax::KW_SELF
                                && p.default.is_none()
                            {
                                diags.push(Diagnostic::lint(
                                    "L2401",
                                    format!(
                                        "public function `{}` has a positional `Bool` parameter `{}`",
                                        f.name, p.name
                                    ),
                                    "positional booleans are easy to transpose at the call site"
                                        .to_string(),
                                    format!(
                                        "callers can write `{}: true` to make the intent clear (S61 labels)",
                                        p.name
                                    ),
                                    Some(p.name_span),
                                ));
                                let _ = idx;
                            }
                        }
                    }
                    // D-NARG-D2 (E0126): check defaults don't ref later params.
                    check_default_forward_refs(&f.params, &f.name, &mut diags);
                    funcs.insert(f.name.clone(), func_to_sig(f));
                }
            }
            Item::Struct(s) => register_struct(
                s,
                &mut registry,
                &mut struct_fields_legacy,
                &mut diags,
                &funcs,
                &consts,
            ),
            Item::Enum(e) => register_enum(e, &mut registry, &mut diags, &funcs, &consts),
            Item::Impl(i) => {
                if !registry.contains(&i.type_name) {
                    diags.push(Diagnostic::error(
                        "E0301",
                        format!("`impl {}` names a type that doesn't exist", i.type_name),
                        format!("`{}` hasn't been defined as a struct or enum", i.type_name),
                        format!(
                            "define `struct {}` or `enum {}` first",
                            i.type_name, i.type_name
                        ),
                        Some(i.type_span),
                    ));
                }
            }
            Item::Distinct(d) => register_distinct(d, &mut registry, &mut diags, &funcs, &consts),
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
            // Stage 1a: modules are parsed but not yet type-checked; the U5
            // merge / eval pipeline consumes them. No runtime contribution.
            Item::Module(_) | Item::CodeModule(_) => {}
            // S59: C FFI modules are folded by CFFI::assemble before the
            // bundle path runs; this legacy single-Program path ignores them.
            Item::CModule(_) => {}
            // D-ERR-CONV: registration happens in m9.register_items below.
            Item::ErrorConv(_) => {}
            // D-MIGRATE1: migration decls are handled by the schema diff pass.
            Item::Migration(_) => {}
        }
    }

    register_type_methods(&prog.items, &mut registry, &mut diags);
    // S62 + D-LIB2: synthesise before register_impl_methods so synthesised
    // Func nodes are visible when method lookup is registered.
    synthesize_impls(&mut prog.items);
    register_impl_methods(&prog.items, &mut registry, &mut diags);
    m9.register_items(&prog.items, &mut diags);

    // S62: delegation validation — check the field exists and implements the trait.
    // Runs after m9.register_items so implements_trait is populated.
    for item in &prog.items {
        if let Item::Impl(i) = item {
            if let (Some(trait_name), Some(field_name)) =
                (&i.trait_name, &i.delegation_field)
            {
                if let Some(fields) = registry.struct_fields(&i.type_name) {
                    if let Some((_, _, field_ty, _, _)) = fields.iter().find(|(n, _, _, _, _)| n == field_name) {
                        let field_type_name = field_ty.name();
                        if !m9.implements_trait(&field_type_name, trait_name) {
                            diags.push(Diagnostic::error(
                                "E2401",
                                format!(
                                    "`{}` doesn't implement `{}`, so it can't delegate",
                                    field_type_name, trait_name
                                ),
                                format!(
                                    "`impl {}: {} using {}` forwards `{}` methods to the `{}` field, but `{}` doesn't implement `{}`",
                                    i.type_name, trait_name, field_name,
                                    trait_name, field_name,
                                    field_type_name, trait_name
                                ),
                                format!(
                                    "implement `impl {}: {}` on the field's type, or choose a different field",
                                    field_type_name, trait_name
                                ),
                                Some(i.type_span),
                            ));
                        }
                    } else {
                        diags.push(Diagnostic::error(
                            "E2401",
                            format!("`{}` has no field `{}`", i.type_name, field_name),
                            format!(
                                "`impl {}: {} using {}` needs `{}` to have a field named `{}`",
                                i.type_name, trait_name, field_name, i.type_name, field_name
                            ),
                            format!("add `{}: Type` to `struct {}`", field_name, i.type_name),
                            Some(i.type_span),
                        ));
                    }
                }
            }
        }
    }

    if mode == CompileMode::Run || mode == CompileMode::Eval {
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
                // E0122: in Run mode main must be `fn main()` with no params and no return.
                // In Eval mode we allow a return type (e.g. `pure fn main() -> Int`).
                if mode == CompileMode::Run && (!sig.params.is_empty() || sig.return_type.is_some()) {
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
                format!("no `#{}` blocks found to run", Syntax::KW_TEST),
                format!(
                    "add at least one top-level block: #{} \"describes what this checks\" {{ ... }}",
                    Syntax::KW_TEST
                ),
                format!(
                    "use `{}` and `{}` inside the block to check results",
                    Syntax::BUILTIN_REQUIRE,
                    Syntax::BUILTIN_REQUIRE_EQ
                ),
                None,
            ));
        }
        CompileMode::Test | CompileMode::Run | CompileMode::Check | CompileMode::Eval => {}
    }

    // S57 (M9.5): evaluate comptime bindings before bodies are checked, so
    // references to them resolve. Single-file mode has no path; embed_file
    // resolves against the current directory.
    eval_comptime_items(
        &mut prog.items,
        &mut consts,
        std::path::Path::new("."),
        &mut diags,
    );

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
                    false,
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
                        false,
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
                        false,
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
                            false,
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
                        false,
                    ));
                }
            }
            Item::Test(t) => {
                let mut synthetic = crate::AST::Func {
                    is_pub: false,
                    name: format!("__test_{}", t.name),
                    name_span: t.name_span,
                    type_params: Vec::new(),
                    params: Vec::new(),
                    return_type: None,
                    is_view_return: false,
                    is_unsafe: false,
                    is_pure: false,
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
                    false,
                ));
                t.body = synthetic.body;
            }
            // D-ERR-CONV: type-check the conversion body with `self: from_ty`, return `to_ty`.
            Item::ErrorConv(ec) => {
                diags.extend(check_error_conv_body(
                    ec,
                    &funcs,
                    &registry,
                    &struct_fields_legacy,
                    &consts,
                    &m9,
                    &ct_funcs,
                    &ct_externs,
                    ct_base_dir,
                    &ct_globals,
                ));
            }
            Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Module(_)
            | Item::CModule(_)
            | Item::CodeModule(_)
            | Item::Distinct(_)
            | Item::Migration(_) => {} // D-MIGRATE1
        }
    }

    diags
}

pub(crate) fn name_defined(
    name: &str,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    consts: &HashMap<String, Type>,
) -> bool {
    funcs.contains_key(name) || registry.contains(name) || consts.contains_key(name)
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
    registry.types.insert(
        d.name.clone(),
        TypeDef::Distinct {
            name_span: d.name_span,
            base: d.base.clone(),
            is_numeric: d.is_numeric,
        },
    );
}

/// S57 (M9.5): evaluate every `comptime NAME = expr;` in `items`. Purity and
/// fuel are enforced by the interpreter (E0951/E0952); panics surface as
/// E0953. Each result's Jet type is registered in `consts` so references
/// type-check, and the value is stashed on the item for codegen to inline.
pub(crate) fn eval_comptime_items(
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
    let mut results: Vec<(String, crate::Comptime::CtValue)> = Vec::new();
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
        let mut globals: HashMap<String, crate::Comptime::CtValue> = HashMap::new();
        for item in items.iter() {
            if let Item::Const(c) = item {
                if c.is_comptime {
                    match crate::Comptime::evaluate(&c.value, &funcs, &externs, base_dir, &globals)
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
            Item::Test(_)
            | Item::Const(_)
            | Item::Trait(_)
            | Item::Module(_)
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::Distinct(_)
            | Item::ErrorConv(_)
            | Item::Migration(_) => {} // D-MIGRATE1
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

pub(crate) fn register_struct(
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
    // D-REPRC1: `#layout(c)` structs may not contain growable fields.
    if s.layout == Some(crate::AST::StructLayout::C) {
        for f in &s.fields {
            let growable = matches!(
                &f.ty,
                Type::List(_) | Type::Map { .. } | Type::String
            );
            if growable {
                let layout_span = s.layout_span.unwrap_or(s.name_span);
                diags.push(Diagnostic::error(
                    "E1104",
                    format!(
                        "`#layout(c)` struct `{}` has a growable field `{}` ({})",
                        s.name,
                        f.name,
                        f.ty.name()
                    ),
                    "growable types (`[T]`, `Map`, `String`) don't have a stable C layout".to_string(),
                    "use a fixed-size array `[T#N]` instead, or remove `#layout(c)`".to_string(),
                    Some(layout_span),
                ));
            }
        }
    }
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

pub(crate) fn register_type_methods(items: &[Item], registry: &mut TypeRegistry, diags: &mut Vec<Diagnostic>) {
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
            TypeDef::Distinct { .. } => continue,
        };
        for m in methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(&m.name, type_name, m.name_span, is_ctor));
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
                let non_self: Vec<_> = m.params.iter().filter(|p| p.name != "self").cloned().collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

pub(crate) fn register_impl_methods(items: &[Item], registry: &mut TypeRegistry, diags: &mut Vec<Diagnostic>) {
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
            TypeDef::Distinct { .. } => continue,
        };
        for m in &i.methods {
            if field_names.iter().any(|f| f == &m.name) {
                diags.push(method_field_clash(&m.name, &i.type_name, m.name_span));
            }
            if methods_map.contains_key(&m.name) {
                let is_ctor = m.self_param().is_none();
                diags.push(method_defined_twice(&m.name, &i.type_name, m.name_span, is_ctor));
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
                let non_self: Vec<_> = m.params.iter().filter(|p| p.name != "self").cloned().collect();
                check_default_forward_refs(&non_self, &m.name, diags);
                methods_map.insert(m.name.clone(), func_to_method_sig(m));
            }
        }
    }
}

pub(crate) fn check_func_body(
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
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
    freestanding: bool,
) -> Vec<Diagnostic> {
    let empty_imports = HashMap::new();
    let empty_core_imports = HashMap::new();
    let empty_code_modules = HashMap::new();
    let empty_unqualified: HashMap<String, String> = HashMap::new();
    let empty_unqualified_file: HashMap<String, (String, usize)> = HashMap::new();
    let empty_func_pub: HashMap<String, bool> = HashMap::new();
    let mut ck = Checker {
        funcs,
        registry,
        structs,
        consts,
        modules: None,
        module_idx: 0,
        imports: &empty_imports,
        core_imports: &empty_core_imports,
        code_modules: &empty_code_modules,
        unqualified: &empty_unqualified,
        unqualified_file: &empty_unqualified_file,
        func_pub: &empty_func_pub,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        loop_labels: Vec::new(),
        // S58 (E2-M13): an `@unsafe fn` body is itself an audited region — its
        // statements may use low-level ops directly without a nested `@unsafe`
        // block. Calling such a fn is gated separately (E3103).
        in_unsafe: f.is_unsafe,
        in_pure: f.is_pure,
        ret: f.return_type.clone(),
        view_return: f.is_view_return,
        fn_name: f.name.clone(),
        expected_type: None,
        iter_borrowed: HashSet::new(),
        freed_allocators: HashMap::new(),
        arena_views: HashMap::new(),
        uninit: HashMap::new(),
        borrow_ctx: false,
        lambda_escapes: true,
        is_task_spawn: false,
        view_capture_tasks: HashSet::new(),
        current_binding_name: None,
        lambda_binding: None,
        lambda_mut_borrow_stack: vec![HashSet::new()],
        m9,
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        ct_scopes: vec![HashMap::new()],
        type_param_scope: f.type_params.clone(),
        freestanding,
        in_dropped_comptime_arm: false,
        stmt_tail_ptr: std::ptr::null(),
        stmt_tail_len: 0,
        liveness_frames: Vec::new(),
    };
    ck.check_params_and_body(f, owner_type);
    // S60 (E2-M16): purity enforcement for `pure fn` bodies.
    if f.is_pure {
        ck.diags.extend(check_pure_fn(f, funcs));
    }
    ck.diags
}

/// D-ERR-CONV: type-check an `impl Source -> Target { body }` conversion body.
/// `self` is bound to the source error type; the block must return the target type.
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_error_conv_body(
    ec: &mut crate::AST::ErrorConvDef,
    funcs: &HashMap<String, FuncSig>,
    registry: &TypeRegistry,
    structs: &HashMap<String, Vec<(Option<String>, Type)>>,
    consts: &HashMap<String, Type>,
    m9: &M9Registry,
    ct_funcs: &HashMap<String, Func>,
    ct_externs: &HashSet<String>,
    ct_base_dir: &std::path::Path,
    ct_globals: &HashMap<String, crate::Comptime::CtValue>,
) -> Vec<Diagnostic> {
    // Synthesise a pseudo-function to reuse check_func_body.
    let mut synthetic = Func {
        is_pub: false,
        name: format!("__errconv_{}_to_{}", ec.from_ty.replace('.', "_"), ec.to_ty.replace('.', "_")),
        name_span: ec.from_span,
        type_params: Vec::new(),
        params: vec![Param {
            name: crate::Syntax::KW_SELF.to_string(),
            name_span: ec.from_span,
            ty: Type::Named(String::new()), // sema fills self type from owner_type
            ty_span: ec.from_span,
            convention: AccessConvention::Move,
            default: None,
        }],
        return_type: Some(Type::Named(ec.to_ty.clone())),
        is_view_return: false,
        is_unsafe: false,
        is_pure: false,
        body: std::mem::take(&mut ec.body),
    };
    let d = check_func_body(
        &mut synthetic,
        funcs,
        registry,
        structs,
        consts,
        m9,
        Some(&ec.from_ty),
        ct_funcs,
        ct_externs,
        ct_base_dir,
        ct_globals,
        false,
    );
    ec.body = synthetic.body;
    d
}

/// D-ERR-CONV: canonical Rust function name for the `impl From -> To` conversion.
/// Used by sema (to stamp into `TryConvert::Typed`) and codegen (to define + call it).
pub(crate) fn error_conv_fn_name(from: &str, to: &str) -> String {
    let f = from.replace('.', "_");
    let t = to.replace('.', "_");
    format!("__jet_errconv_{}_to_{}", f, t)
}

pub(crate) fn already_defined(name: &str, span: Span) -> Diagnostic {
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
pub(crate) fn defined_twice(name: &str, why: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!("`{}` is defined twice", name),
        why.to_string(),
        "rename or remove one of the definitions".to_string(),
        Some(span),
    )
}

/// E0105: a method's name collides with a field on the same type.
pub(crate) fn method_field_clash(method: &str, type_name: &str, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0105",
        format!(
            "method `{}` can't share a name with a field on `{}`",
            method, type_name
        ),
        "a type's methods and fields must have different names".to_string(),
        "rename the method or the field".to_string(),
        Some(span),
    )
}

/// E0105: a method name appears twice on the same type.
/// `is_ctor` is true when the duplicate is a no-`self` static (a named
/// constructor per D-CTOR1), so the fix hint teaches constructor naming.
pub(crate) fn method_defined_twice(method: &str, type_name: &str, span: Span, is_ctor: bool) -> Diagnostic {
    let fix = if is_ctor {
        format!(
            "named constructors must each have a unique name — call them `{}.{}` and `{}.other_name`",
            type_name, method, type_name
        )
    } else {
        "rename or remove one of the definitions".to_string()
    };
    Diagnostic::error(
        "E0105",
        format!("method `{}` is defined twice on `{}`", method, type_name),
        "each method name may appear only once on a type".to_string(),
        fix,
        Some(span),
    )
}

pub(crate) fn impl_type_exists(
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

pub(crate) fn synthesize_impls(items: &mut Vec<Item>) {
    // Build trait_name -> method sigs from the AST (no m9 needed).
    let mut trait_methods: HashMap<String, Vec<crate::AST::TraitMethodSig>> = HashMap::new();
    for item in items.iter() {
        if let Item::Trait(t) = item {
            trait_methods.insert(t.name.clone(), t.methods.clone());
        }
    }

    // Build (type_name, trait_name) impl pairs and struct field types from the AST.
    // Used to guard delegation synthesis — only synthesize if the field type actually
    // implements the trait (error is emitted later by E2401 validation if not).
    let mut impl_pairs: std::collections::HashSet<(String, String)> = Default::default();
    let mut struct_field_types: HashMap<String, HashMap<String, String>> = HashMap::new();
    for item in items.iter() {
        match item {
            Item::Impl(i) => {
                if let Some(trait_name) = &i.trait_name {
                    if i.delegation_field.is_none() {
                        impl_pairs.insert((i.type_name.clone(), trait_name.clone()));
                    }
                }
            }
            Item::Struct(s) => {
                // Also check inline trait impls (impl Trait { … } inside struct body)
                for block in &s.trait_impls {
                    impl_pairs.insert((s.name.clone(), block.trait_name.clone()));
                }
                let fields: HashMap<String, String> = s
                    .fields
                    .iter()
                    .map(|f| (f.name.clone(), f.ty.name()))
                    .collect();
                struct_field_types.insert(s.name.clone(), fields);
            }
            _ => {}
        }
    }

    // S62: delegation — build forwarding Func nodes only when the field type
    // implements the trait (guards against generating invalid code for E2401 cases).
    let mut delegations: Vec<(usize, String, String, String)> = Vec::new(); // (idx, type_name, trait_name, field_name)
    for (idx, item) in items.iter().enumerate() {
        if let Item::Impl(i) = item {
            if let (Some(trait_name), Some(field_name)) = (&i.trait_name, &i.delegation_field) {
                delegations.push((idx, i.type_name.clone(), trait_name.clone(), field_name.clone()));
            }
        }
    }
    for (idx, type_name, trait_name, field_name) in delegations {
        // Check if the field type implements the trait in the AST.
        let field_type_name = struct_field_types
            .get(&type_name)
            .and_then(|fields| fields.get(&field_name))
            .cloned();
        let can_delegate = field_type_name.as_ref().is_some_and(|ft| {
            impl_pairs.contains(&(ft.clone(), trait_name.clone()))
        });
        if !can_delegate {
            // Skip synthesis; E2401 validation will emit the appropriate error.
            continue;
        }
        if let Some(sigs) = trait_methods.get(&trait_name) {
            let synthesized: Vec<crate::AST::Func> = sigs
                .iter()
                .map(|m| synthesize_delegation_method(m, &field_name))
                .collect();
            if let Item::Impl(i) = &mut items[idx] {
                i.methods = synthesized;
            }
        }
    }

    // D-LIB2: default method body injection.
    let mut trait_impls_to_fill: Vec<(usize, String)> = Vec::new();
    for (idx, item) in items.iter().enumerate() {
        if let Item::Impl(i) = item {
            if let Some(trait_name) = &i.trait_name {
                if i.delegation_field.is_none() {
                    trait_impls_to_fill.push((idx, trait_name.clone()));
                }
            }
        }
    }
    for (idx, trait_name) in trait_impls_to_fill {
        if let Some(sigs) = trait_methods.get(&trait_name) {
            let mut extras: Vec<crate::AST::Func> = Vec::new();
            if let Item::Impl(i) = &items[idx] {
                let provided: std::collections::HashSet<String> =
                    i.methods.iter().map(|m| m.name.clone()).collect();
                for sig in sigs {
                    if !provided.contains(&sig.name) {
                        if let Some(body) = &sig.default_body {
                            extras.push(synthesize_default_method(sig, body));
                        }
                    }
                }
            }
            if !extras.is_empty() {
                if let Item::Impl(i) = &mut items[idx] {
                    i.methods.extend(extras);
                }
            }
        }
    }
}

// S62: build a forwarding `Func` for one trait method sig, delegating to
// `self.<field>.<method>(args…)`.
pub(crate) fn synthesize_delegation_method(
    sig: &crate::AST::TraitMethodSig,
    field_name: &str,
) -> crate::AST::Func {
    use crate::AST::{AccessConvention, CallArg, CallArgFlags, Expr, Func, Param, Stmt, Type};
    use crate::Diagnostics::Span;

    let zero = Span::new(0, 0);

    // Build the forwarding call: self.<field>.<method>(non-self params...)
    let args: Vec<CallArg> = sig
        .params
        .iter()
        .filter(|p| p.name != Syntax::KW_SELF)
        .map(|p| CallArg {
            convention: p.convention,
            expr: Expr::Ident(p.name.clone(), zero),
            span: zero,
            flags: CallArgFlags::default(),
            label: None,
        })
        .collect();

    let forward_call = Expr::MethodCall {
        receiver: Box::new(Expr::Field(
            Box::new(Expr::Ident(Syntax::KW_SELF.to_string(), zero)),
            field_name.to_string(),
            zero,
        )),
        method: sig.name.clone(),
        method_span: zero,
        args,
        recv_type: None,
    };

    // Wrap in a return stmt if there's a return type; otherwise a bare expr stmt.
    let body_stmt = if sig.return_type.is_some() {
        Stmt::Return(Some(forward_call), zero)
    } else {
        Stmt::Expr(forward_call)
    };

    // Build the `self` param.
    let self_param = Param {
        convention: AccessConvention::Read,
        name: Syntax::KW_SELF.to_string(),
        name_span: zero,
        ty: Type::Named(String::new()), // S27: sema fills in the actual type name
        ty_span: zero,
        default: None,
    };

    let mut params = vec![self_param];
    params.extend(sig.params.iter().filter(|p| p.name != Syntax::KW_SELF).cloned());

    Func {
        is_pub: false,
        name: sig.name.clone(),
        name_span: sig.name_span,
        type_params: vec![],
        params,
        return_type: sig.return_type.clone(),
        is_view_return: sig.is_view_return,
        is_unsafe: false,
        is_pure: false,
        body: vec![body_stmt],
    }
}

// D-LIB2: build a Func that uses the default body from the trait definition.
pub(crate) fn synthesize_default_method(
    sig: &crate::AST::TraitMethodSig,
    body: &[crate::AST::Stmt],
) -> crate::AST::Func {
    use crate::AST::{AccessConvention, Func, Param, Type};
    use crate::Diagnostics::Span;

    let zero = Span::new(0, 0);
    let self_param = Param {
        convention: AccessConvention::Read,
        name: Syntax::KW_SELF.to_string(),
        name_span: zero,
        ty: Type::Named(String::new()), // S27: sema fills in the actual type name
        ty_span: zero,
        default: None,
    };
    let mut params = vec![self_param];
    params.extend(sig.params.iter().filter(|p| p.name != Syntax::KW_SELF).cloned());

    Func {
        is_pub: false,
        name: sig.name.clone(),
        name_span: sig.name_span,
        type_params: vec![],
        params,
        return_type: sig.return_type.clone(),
        is_view_return: sig.is_view_return,
        is_unsafe: false,
        is_pure: false,
        body: body.to_vec(),
    }
}

// ─── D-NARG-D2: default-expression forward-reference check (E0126) ───────────

/// Check every default expression in a function/method for forward references —
/// i.e. an Ident that names a parameter declared *after* the one being defaulted.
/// Emits E0126 for each forward reference found.
/// `params` excludes `self`; `fn_name` is for the error message.
pub(crate) fn check_default_forward_refs(
    params: &[crate::AST::Param],
    fn_name: &str,
    diags: &mut Vec<Diagnostic>,
) {
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    for (i, p) in params.iter().enumerate() {
        let Some(default) = &p.default else { continue };
        let forward_refs = super::find_forward_refs(default, &param_names, i);
        for (fwd_name, fwd_span) in forward_refs {
            diags.push(Diagnostic::error(
                "E0126",
                format!(
                    "default for `{}` in `{}` references `{}`, which comes after it",
                    p.name, fn_name, fwd_name
                ),
                "defaults fill left to right; a parameter isn't in scope until it appears in the list".to_string(),
                format!(
                    "reorder the parameters so `{}` comes before `{}`, or use a constant default",
                    fwd_name, p.name
                ),
                Some(fwd_span),
            ));
        }
    }
}

// ─── S60 / E2-M16: purity checking ───────────────────────────────────────────

