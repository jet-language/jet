use super::*;
use crate::AST::{
    ConstAttr, ElseBranch, EnumLitArg,
    Expr, ForKind, Func, IfStmt, ImportKind, Item, LValue, LambdaBody,
    OrFallback, ProgramBundle, RustConstKind, Stmt, StrPart, Type,
};
use crate::Diagnostics::Diagnostic;
use crate::Loader;
use crate::Traits::TraitRegistry;
use crate::Syntax;
use std::collections::{HashMap, HashSet};

/// D-MOD2: inside an inline `module M { … }`, a call to a sibling function
/// `helper(x)` must lower to the mangled `M__helper`. This pre-pass rewrites
/// such call names so registration, body-checking, and codegen all agree.
/// Only callee names are rewritten (the unambiguous case); a sibling referenced
/// as a value resolves through normal name lookup and yields a clean Jet error
/// rather than leaking to rustc.
pub(crate) fn mangle_inline_sibling_calls(bundle: &mut ProgramBundle) {
    for module in bundle.modules.iter_mut() {
        for item in module.items.iter_mut() {
            let Item::CodeModule(cm) = item else { continue };
            let Some(body) = &mut cm.body else { continue };
            let siblings: HashSet<String> = body
                .iter()
                .filter_map(|i| match i {
                    Item::Func(f) => Some(f.name.clone()),
                    _ => None,
                })
                .collect();
            if siblings.is_empty() {
                continue;
            }
            for inner in body.iter_mut() {
                if let Item::Func(f) = inner {
                    rewrite_inline_calls_stmts(&mut f.body, &siblings, &cm.name);
                }
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_stmts(stmts: &mut [Stmt], siblings: &HashSet<String>, modname: &str) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Val(b) => rewrite_inline_calls_expr(&mut b.init, siblings, modname),
            Stmt::Assign { value, .. } => rewrite_inline_calls_expr(value, siblings, modname),
            Stmt::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
            Stmt::Return(None, _) | Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
            Stmt::If(ifs) => rewrite_inline_calls_if(ifs, siblings, modname),
            Stmt::While { cond, body, .. } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        rewrite_inline_calls_expr(start, siblings, modname);
                        rewrite_inline_calls_expr(end, siblings, modname);
                        if let Some(step) = step {
                            rewrite_inline_calls_expr(step, siblings, modname);
                        }
                    }
                    ForKind::In { collection } => {
                        rewrite_inline_calls_expr(collection, siblings, modname);
                    }
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            Stmt::Switch { subject, arms, else_body, .. } => {
                rewrite_inline_calls_expr(subject, siblings, modname);
                for a in arms.iter_mut() {
                    rewrite_inline_calls_expr(&mut a.cond, siblings, modname);
                    rewrite_inline_calls_stmts(&mut a.body, siblings, modname);
                }
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            Stmt::Loop { body: inner, .. } | Stmt::Unsafe { body: inner, .. } | Stmt::Region { body: inner, .. } | Stmt::Caps { body: inner, .. } => {
                rewrite_inline_calls_stmts(inner, siblings, modname);
            }
            // D-WHEN1: rewrite calls in both arms so sibling resolution works
            // regardless of which arm is selected at comptime.
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
                rewrite_inline_calls_expr(cond, siblings, modname);
                rewrite_inline_calls_stmts(then_body, siblings, modname);
                if let Some(eb) = else_body {
                    rewrite_inline_calls_stmts(eb, siblings, modname);
                }
            }
            // D-CTX1: rewrite inline calls in field values and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields.iter_mut() {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
            // D-TERM1 (ratified 2026-06-22): rewrite inline calls in live block body.
            Stmt::Live { body, .. } => {
                rewrite_inline_calls_stmts(body, siblings, modname);
            }
        }
    }
}

pub(crate) fn rewrite_inline_calls_if(ifs: &mut IfStmt, siblings: &HashSet<String>, modname: &str) {
    rewrite_inline_calls_expr(&mut ifs.cond, siblings, modname);
    rewrite_inline_calls_stmts(&mut ifs.then_body, siblings, modname);
    match &mut ifs.else_branch {
        Some(ElseBranch::Else(b)) => rewrite_inline_calls_stmts(b, siblings, modname),
        Some(ElseBranch::ElseIf(next)) => rewrite_inline_calls_if(next, siblings, modname),
        None => {}
    }
}

pub(crate) fn rewrite_inline_calls_expr(expr: &mut Expr, siblings: &HashSet<String>, modname: &str) {
    match expr {
        Expr::Call(c) => {
            if siblings.contains(&c.name) {
                c.name = format!("{}__{}", modname, c.name);
            }
            for a in c.args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::PtrFromAddr { addr, .. } => rewrite_inline_calls_expr(addr, siblings, modname),
        Expr::Ident(_, _)
        | Expr::Char(_, _)
        | Expr::Int(_, _, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. } => {}
        Expr::Str(parts, _) => {
            for p in parts.iter_mut() {
                if let StrPart::Interp(e) = p {
                    rewrite_inline_calls_expr(e, siblings, modname);
                }
            }
        }
        Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::Field(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => rewrite_inline_calls_expr(inner, siblings, modname),
        Expr::OptField { base, .. } => rewrite_inline_calls_expr(base, siblings, modname),
        Expr::MethodCall { receiver, args, .. } => {
            rewrite_inline_calls_expr(receiver, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::EnumLit { args, .. } => {
            for a in args.iter_mut() {
                match a {
                    EnumLitArg::Positional(e) | EnumLitArg::Named { expr: e, .. } => {
                        rewrite_inline_calls_expr(e, siblings, modname);
                    }
                }
            }
        }
        Expr::OrFallback { value, fallback, .. } => {
            rewrite_inline_calls_expr(value, siblings, modname);
            match fallback {
                OrFallback::Value(e) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(Some(e), _) => rewrite_inline_calls_expr(e, siblings, modname),
                OrFallback::Return(None, _) | OrFallback::Panic { .. } => {}
            }
        }
        Expr::PatternTest { subject, .. } => {
            rewrite_inline_calls_expr(subject, siblings, modname)
        }
        Expr::Binary(_, l, r, _) => {
            rewrite_inline_calls_expr(l, siblings, modname);
            rewrite_inline_calls_expr(r, siblings, modname);
        }
        Expr::ListLit(elems, _) => {
            for e in elems.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields.iter_mut() {
                rewrite_inline_calls_expr(e, siblings, modname);
            }
        }
        Expr::MapLit(entries, _) => {
            for (k, v) in entries.iter_mut() {
                rewrite_inline_calls_expr(k, siblings, modname);
                rewrite_inline_calls_expr(v, siblings, modname);
            }
        }
        Expr::Index { base, index, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(index, siblings, modname);
        }
        Expr::Slice { base, start, end, .. } => {
            rewrite_inline_calls_expr(base, siblings, modname);
            rewrite_inline_calls_expr(start, siblings, modname);
            rewrite_inline_calls_expr(end, siblings, modname);
        }
        Expr::CallValue { callee, args, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for a in args.iter_mut() {
                rewrite_inline_calls_expr(&mut a.expr, siblings, modname);
            }
        }
        Expr::Lambda(lam) => match &mut lam.body {
            LambdaBody::Expr(e) => rewrite_inline_calls_expr(e, siblings, modname),
            LambdaBody::Block(stmts) => rewrite_inline_calls_stmts(stmts, siblings, modname),
        },
        Expr::If { cond, then_body, then_value, else_body, else_value, .. } => {
            rewrite_inline_calls_expr(cond, siblings, modname);
            rewrite_inline_calls_stmts(then_body, siblings, modname);
            rewrite_inline_calls_expr(then_value, siblings, modname);
            rewrite_inline_calls_stmts(else_body, siblings, modname);
            rewrite_inline_calls_expr(else_value, siblings, modname);
        }
        Expr::FanOut { callee, items, .. } => {
            rewrite_inline_calls_expr(callee, siblings, modname);
            for item in items.iter_mut() {
                rewrite_inline_calls_expr(item, siblings, modname);
            }
        }
    }
}

pub fn check_bundle(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, false)
}

/// Like `check_bundle` but with extra build options (E2-M15).
pub fn check_bundle_freestanding(bundle: &mut ProgramBundle, mode: CompileMode) -> Vec<Diagnostic> {
    check_bundle_opts(bundle, mode, true)
}

pub(crate) fn check_bundle_opts(bundle: &mut ProgramBundle, mode: CompileMode, freestanding: bool) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    // D-MOD2: rewrite inline-module sibling calls to their mangled names before any
    // registration/checking/codegen sees the bodies.
    mangle_inline_sibling_calls(bundle);
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
            core_imports: HashMap::new(),
            tests: HashMap::new(),
            trait_reg: TraitRegistry::default(),
            code_modules: HashMap::new(),
            unqualified: HashMap::new(),
            unqualified_file: HashMap::new(),
            reexports: HashMap::new(),
        })
        .collect();

    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let st = &mut states[idx];
        for item in &module.items {
            match item {
                Item::Func(f) => register_func_item(f, st, &mut diags),
                Item::Struct(s) => {
                    register_struct(
                        s,
                        &mut st.registry,
                        &mut st.structs,
                        &mut diags,
                        &st.funcs,
                        &st.consts,
                    );
                    st.type_pub.insert(s.name.clone(), s.is_pub);
                    for fld in &s.fields {
                        st.field_pub
                            .insert((s.name.clone(), fld.name.clone()), fld.is_pub);
                    }
                    for m in &s.methods {
                        st.method_pub
                            .insert((s.name.clone(), m.name.clone()), m.is_pub);
                    }
                }
                Item::Enum(e) => {
                    register_enum(e, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(e.name.clone(), e.is_pub);
                    for m in &e.methods {
                        st.method_pub
                            .insert((e.name.clone(), m.name.clone()), m.is_pub);
                    }
                }
                Item::Impl(i) => {
                    if !i.type_name.contains('.') && !st.registry.contains(&i.type_name) {
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
                    } else if !i.type_name.contains('.') {
                        for m in &i.methods {
                            st.method_pub
                                .insert((i.type_name.clone(), m.name.clone()), m.is_pub);
                        }
                    }
                }
                Item::Const(c) => {
                    register_const(c, &mut st.consts, &mut diags, &st.funcs, &st.registry)
                }
                Item::Distinct(d) => {
                    register_distinct(d, &mut st.registry, &mut diags, &st.funcs, &st.consts);
                    st.type_pub.insert(d.name.clone(), d.is_pub);
                }
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
                Item::CModule(cm) => {
                    if check_c_module(cm, &st.registry, &mut diags) {
                        for ef in &cm.functions {
                            register_extern_fn(
                                ef,
                                &mut st.funcs,
                                &st.registry,
                                &st.consts,
                                &mut diags,
                            );
                            // C FFI functions are callable across the `use c.<lib>`
                            // alias — expose them like any pub item.
                            st.func_pub.insert(ef.name.clone(), true);
                        }
                    }
                }
                Item::Trait(_) => {}
                Item::Module(_) => {}
                Item::CodeModule(cm) => {
                    if let Some(body) = &cm.body {
                        // D-MOD2: register inline module functions under mangled names
                        // (`math__double`) so call-site sema can check them.
                        st.code_modules.insert(cm.name.clone(), cm.name.clone());
                        for inner in body {
                            if let Item::Func(f) = inner {
                                let mangled = format!("{}__{}", cm.name, f.name);
                                st.funcs.insert(mangled.clone(), func_to_sig(f));
                                st.func_pub.insert(mangled, f.is_pub);
                            }
                        }
                    }
                }
                Item::ErrorConv(_) => {}
                // D-MIGRATE1: migration decls are handled by the schema diff pass; no registration needed.
                Item::Migration(_) => {}
            }
        }
        // S62 + D-LIB2: synthesis must happen before register_impl_methods
        // so the synthesised Func nodes appear in the type registry.
        synthesize_impls(&mut module.items);
        register_type_methods(&module.items, &mut st.registry, &mut diags);
        register_impl_methods(&module.items, &mut st.registry, &mut diags);
        st.trait_reg.register_items(&module.items, &mut diags);
        // D-MIGRATE1: schema diff pass (E0910) — runs after struct registration (I3).
        diags.extend(check_schema_migrations(&module.items, &bundle.project_root));
    }

    // S62 E2401: delegation validation — check field exists and implements trait.
    // Runs after all m9 registrations so implements_trait is populated.
    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &states[idx];
        for item in &module.items {
            if let Item::Impl(i) = item {
                if let (Some(trait_name), Some(field_name)) =
                    (&i.trait_name, &i.delegation_field)
                {
                    if let Some(fields) = st.registry.struct_fields(&i.type_name) {
                        if let Some((_, _, field_ty, _, _)) =
                            fields.iter().find(|(n, _, _, _, _)| n == field_name)
                        {
                            let field_type_name = field_ty.name();
                            if !st.trait_reg.implements_trait(&field_type_name, trait_name) {
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
                                format!(
                                    "add `{}: Type` to `struct {}`",
                                    field_name, i.type_name
                                ),
                                Some(i.type_span),
                            ));
                        }
                    }
                }
            }
        }
    }

    // S57 (M9.5): evaluate comptime bindings per module. `embed_file` paths
    // resolve against each module file's own directory (S16 convention).
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        let base = module
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        eval_comptime_items(
            &mut module.items,
            &mut states[idx].consts,
            &base,
            &mut diags,
        );
    }

    // D-MOD3/4: Unqualified imports (`use alias.Item`) are processed in a
    // dedicated pass *after* file-module aliases land in `st.imports` below.

    for (idx, module) in bundle.modules.iter().enumerate() {
        let st = &mut states[idx];
        for imp in &module.imports {
            // Unqualified imports are handled in the dedicated pass below.
            if matches!(&imp.kind, ImportKind::Unqualified { .. }) {
                continue;
            }
            let alias = Loader::import_alias(imp);
            if st.imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if st.core_imports.contains_key(&alias) {
                diags.push(Diagnostic::error(
                    "E0105",
                    format!("the import name `{}` is used twice", alias),
                    "each import needs a unique namespace name in this file".to_string(),
                    format!("rename one with `{} alias`", Syntax::KW_AS),
                    Some(imp.alias_span),
                ));
                continue;
            }
            if let ImportKind::Module(name, _) = &imp.kind {
                if Loader::is_legacy_std_import(name) {
                    diags.push(Diagnostic::error(
                        "E0019",
                        format!("`{name}` is the old standard-library import spelling"),
                        "the standard library module was renamed to `core`".to_string(),
                        format!(
                            "use `import {}` or `import {}.fs as fs`",
                            Syntax::CORE_SHORT,
                            Syntax::CORE_SHORT
                        ),
                        Some(imp.span),
                    ));
                    continue;
                }
            }
            if let Some(module) = Loader::core_module_path(imp) {
                if !Loader::is_known_core_module(&module) {
                    diags.push(Diagnostic::error(
                        "E1001",
                        format!("there is no core module `{}`", module),
                        "`core` is compiler-known in M10, and only the frozen core modules exist"
                            .to_string(),
                        format!("import one of: {}", Loader::core_modules_list()),
                        Some(imp.span),
                    ));
                    continue;
                }
                st.core_imports.insert(alias, module);
                continue;
            }
            // S59 (E2-M14): C `use` forms bind to a synthetic merged module
            // resolved by `CFFI::assemble` (E3204 already reported there).
            if crate::CFFI::is_c_import(imp) {
                if let Some(target) = bundle.cffi.target_for(idx, &alias) {
                    st.imports.insert(alias, target);
                }
                continue;
            }
            match Loader::resolve_import_target(bundle, idx, imp) {
                Ok(target) => {
                    st.imports.insert(alias, target);
                }
                Err(d) => diags.push(d),
            }
        }
    }

    // D-MOD3/4: process `use alias.Item` unqualified imports now that file-module
    // aliases are registered in `st.imports`. `pub use` additionally re-exports the
    // item onto this module's public surface (`reexports`).
    for (idx, module) in bundle.modules.iter().enumerate() {
        for imp in &module.imports {
            let ImportKind::Unqualified { module_alias, module_alias_span, items, .. } = &imp.kind else {
                continue;
            };
            let st = &mut states[idx];
            if st.code_modules.contains_key(module_alias.as_str()) {
                // Inline module: items are mangled as `{alias}__{item}`.
                for item in items {
                    let mangled = format!("{}__{}", module_alias, item);
                    if !st.funcs.contains_key(&mangled) {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", item, module_alias),
                            "check the module body for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !st.func_pub.get(&mangled).copied().unwrap_or(false) {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", item, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in module `{}`", item, module_alias),
                            Some(*module_alias_span),
                        ));
                    } else {
                        st.unqualified.insert(item.clone(), mangled.clone());
                        if imp.is_pub {
                            st.reexports.insert(item.clone(), (mangled, idx));
                        }
                    }
                }
            } else if module_alias == "core" || module_alias == "jet" {
                // Std namespace prefix: `use core.mem` → bind each item as a Core import.
                // Each item `x` becomes `core.x` in the known-modules table.
                let st = &mut states[idx];
                for item in items {
                    let full = format!("core.{}", item);
                    if !Loader::is_known_core_module(&full) {
                        diags.push(Diagnostic::error(
                            "E1001",
                            format!("there is no core module `{}`", full),
                            "`core` is compiler-known in M10, and only the frozen core modules exist".to_string(),
                            format!("import one of: {}", Loader::core_modules_list()),
                            Some(*module_alias_span),
                        ));
                    } else if st.core_imports.contains_key(item) {
                        diags.push(Diagnostic::error(
                            "E0105",
                            format!("the import name `{}` is used twice", item),
                            "each import needs a unique namespace name in this file".to_string(),
                            format!("rename one with `{} alias`", Syntax::KW_AS),
                            Some(imp.alias_span),
                        ));
                    } else {
                        st.core_imports.insert(item.clone(), full);
                    }
                }
            } else if st.imports.contains_key(module_alias.as_str()) {
                // File module: look up items in the target module's state.
                let target_idx = st.imports[module_alias.as_str()];
                let is_reexport = imp.is_pub;
                for item in items {
                    let is_pub = states[target_idx].func_pub.get(item).copied().unwrap_or(false);
                    let exists = states[target_idx].funcs.contains_key(item);
                    if !exists {
                        diags.push(Diagnostic::error(
                            "E0611",
                            format!("`{}` is not defined in module `{}`", item, module_alias),
                            "check the module for the item you're importing".to_string(),
                            "make sure the name is spelled correctly".to_string(),
                            Some(*module_alias_span),
                        ));
                    } else if !is_pub {
                        diags.push(Diagnostic::error(
                            "E0609",
                            format!("`{}` is private in module `{}`", item, module_alias),
                            "only `pub` items can be brought into scope with `use`".to_string(),
                            format!("add `pub` before `fn {}` in the imported file", item),
                            Some(*module_alias_span),
                        ));
                    } else {
                        states[idx].unqualified_file.insert(item.clone(), (item.clone(), target_idx));
                        if is_reexport {
                            states[idx].reexports.insert(item.clone(), (item.clone(), target_idx));
                        }
                    }
                }
            } else {
                // Module alias not found — E0610.
                diags.push(Diagnostic::error(
                    "E0610",
                    format!("no module named `{}` in scope", module_alias),
                    "the alias must refer to a module imported earlier in this file".to_string(),
                    format!("add `import … as {}`  before this `use`", module_alias),
                    Some(*module_alias_span),
                ));
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
                    format!(
                        "define `struct {}` or `enum {}` first",
                        i.type_name, i.type_name
                    ),
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
                Item::Const(_)
            | Item::ExternRust(_)
            | Item::Trait(_)
            | Item::Module(_)
            | Item::Distinct(_)
            | Item::CModule(_) | Item::CodeModule(_)
            | Item::ErrorConv(_)
            | Item::Migration(_) => {} // D-MIGRATE1
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
                format!(
                    "rename the type, or import with `{} other_name`",
                    Syntax::KW_AS
                ),
                None,
            ));
        }
    }

    let entry = &states[bundle.entry];
    if mode == CompileMode::Run || mode == CompileMode::Eval {
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
            // E0122: in Run mode main must have no params and no return type.
            // In Eval mode a return type is allowed (e.g. `pure fn main() -> Int`).
            if mode == CompileMode::Run && (!sig.params.is_empty() || sig.return_type.is_some()) {
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

    // D-EFF1: collect effect summaries across every module, then run the
    // whole-program fixpoint and enforce each `#(…)` bound once.
    let mut effect_summaries: HashMap<String, EffectSummary> = HashMap::new();
    for (idx, module) in bundle.modules.iter_mut().enumerate() {
        diags.extend(check_module_bodies(
            module,
            idx,
            &states,
            mode,
            freestanding,
            &mut effect_summaries,
        ));
    }
    let solved = solve(&effect_summaries);
    for module in &bundle.modules {
        check_effect_boundaries(&module.items, &solved, &mut diags);
    }
    check_region_caps(&effect_summaries, &solved, &mut diags);
    bundle.used_core = collect_used_core(bundle, &states);
    diags
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
            }
        }
    }
    // D-NARG-D2 (E0126): check defaults don't ref later params.
    check_default_forward_refs(&f.params, &f.name, diags);
    st.func_pub.insert(f.name.clone(), f.is_pub);
    st.funcs.insert(f.name.clone(), func_to_sig(f));
}

pub(crate) fn collect_used_core(bundle: &ProgramBundle, states: &[ModuleState]) -> HashSet<String> {
    let mut used = HashSet::new();
    for (idx, module) in bundle.modules.iter().enumerate() {
        let imports = &states[idx].core_imports;
        for item in &module.items {
            match item {
                Item::Func(f) => collect_core_stmts(&f.body, imports, &mut used),
                Item::Struct(s) => {
                    for m in &s.methods {
                        collect_core_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Enum(e) => {
                    for m in &e.methods {
                        collect_core_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Impl(i) => {
                    for m in &i.methods {
                        collect_core_stmts(&m.body, imports, &mut used);
                    }
                }
                Item::Test(t) => collect_core_stmts(&t.body, imports, &mut used),
                Item::Const(c) => collect_core_expr(&c.value, imports, &mut used),
                Item::Trait(_)
                | Item::ExternRust(_)
                | Item::Module(_)
                | Item::Distinct(_)
                | Item::CModule(_) | Item::CodeModule(_)
                | Item::ErrorConv(_)
                | Item::Migration(_) => {} // D-MIGRATE1
            }
        }
    }
    used
}

pub(crate) fn collect_core_stmts(
    stmts: &[Stmt],
    imports: &HashMap<String, String>,
    used: &mut HashSet<String>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) => collect_core_expr(e, imports, used),
            Stmt::Val(b) => collect_core_expr(&b.init, imports, used),
            Stmt::Assign { target, value, .. } => {
                collect_core_lvalue(target, imports, used);
                collect_core_expr(value, imports, used);
            }
            Stmt::Return(Some(e), _) => collect_core_expr(e, imports, used),
            Stmt::Return(None, _) => {}
            Stmt::If(ifs) => collect_core_if(ifs, imports, used),
            Stmt::While { cond, body, .. } => {
                collect_core_expr(cond, imports, used);
                collect_core_stmts(body, imports, used);
            }
            Stmt::For { kind, body, .. } => {
                match kind {
                    ForKind::Range { start, end, step } => {
                        collect_core_expr(start, imports, used);
                        collect_core_expr(end, imports, used);
                        if let Some(step) = step {
                            collect_core_expr(step, imports, used);
                        }
                    }
                    ForKind::In { collection } => collect_core_expr(collection, imports, used),
                }
                collect_core_stmts(body, imports, used);
            }
            Stmt::Switch {
                subject,
                arms,
                else_body,
                ..
            } => {
                collect_core_expr(subject, imports, used);
                for arm in arms {
                    collect_core_expr(&arm.cond, imports, used);
                    collect_core_stmts(&arm.body, imports, used);
                }
                if let Some(body) = else_body {
                    collect_core_stmts(body, imports, used);
                }
            }
            Stmt::Loop { body, .. } | Stmt::Unsafe { body, .. } | Stmt::Region { body, .. } | Stmt::Caps { body, .. } => collect_core_stmts(body, imports, used),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
            // D-WHEN1: collect Core usage from both arms (we don't know which is
            // selected until sema runs; over-collecting is harmless here).
            Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
                collect_core_expr(cond, imports, used);
                collect_core_stmts(then_body, imports, used);
                if let Some(eb) = else_body {
                    collect_core_stmts(eb, imports, used);
                }
            }
            // D-CTX1: collect Core usage from context block fields and body.
            Stmt::ContextBlock { fields, body, .. } => {
                for (_, e, _) in fields {
                    collect_core_expr(e, imports, used);
                }
                collect_core_stmts(body, imports, used);
            }
            // D-TERM1 (ratified 2026-06-22): collect Core usage from live block body.
            // The live block implicitly uses `core.term` (jet_term_enter/leave), so
            // we mark it as used here.
            Stmt::Live { body, .. } => {
                used.insert("core.term".to_string());
                collect_core_stmts(body, imports, used);
            }
        }
    }
}

pub(crate) fn collect_core_if(ifs: &IfStmt, imports: &HashMap<String, String>, used: &mut HashSet<String>) {
    collect_core_expr(&ifs.cond, imports, used);
    collect_core_stmts(&ifs.then_body, imports, used);
    match &ifs.else_branch {
        Some(ElseBranch::Else(body)) => collect_core_stmts(body, imports, used),
        Some(ElseBranch::ElseIf(next)) => collect_core_if(next, imports, used),
        None => {}
    }
}

pub(crate) fn collect_core_lvalue(lv: &LValue, imports: &HashMap<String, String>, used: &mut HashSet<String>) {
    match lv {
        LValue::Local { .. } => {}
        LValue::Index { base, index, .. } => {
            collect_core_expr(base, imports, used);
            collect_core_expr(index, imports, used);
        }
        // D-MUTSELF1: `place.field = v` — the base place may use a core import.
        LValue::Field { base, .. } => collect_core_expr(base, imports, used),
    }
}

pub(crate) fn collect_core_expr(expr: &Expr, imports: &HashMap<String, String>, used: &mut HashSet<String>) {
    match expr {
        Expr::PtrFromAddr { addr, .. } => collect_core_expr(addr, imports, used),
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            if matches!(receiver.as_ref(), Expr::Ident(n, _) if is_json_type_name(n)) {
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
            collect_core_expr(receiver, imports, used);
            for arg in args {
                collect_core_expr(&arg.expr, imports, used);
            }
        }
        Expr::Call(c) => {
            // D-PRELUDE1 = B: bare `input(...)` is prelude-ambient; mark core.io so
            // CORELIB_PRELUDE is emitted and jet_std_io_input is in scope for codegen.
            if c.name == Syntax::BUILTIN_INPUT {
                used.insert("core.io::input".to_string());
            }
            for arg in &c.args {
                collect_core_expr(&arg.expr, imports, used);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            collect_core_expr(callee, imports, used);
            for arg in args {
                collect_core_expr(&arg.expr, imports, used);
            }
        }
        Expr::Field(inner, member, _) => {
            if matches!(inner.as_ref(), Expr::Ident(n, _) if is_json_type_name(n))
                && member == "Null"
            {
                used.insert("core::json".to_string());
            }
            collect_core_expr(inner, imports, used);
        }
        Expr::OptField { base, .. } => collect_core_expr(base, imports, used),
        Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => collect_core_expr(inner, imports, used),
        Expr::Binary(_, lhs, rhs, _)
        | Expr::Index {
            base: lhs,
            index: rhs,
            ..
        } => {
            collect_core_expr(lhs, imports, used);
            collect_core_expr(rhs, imports, used);
        }
        Expr::Slice {
            base, start, end, ..
        } => {
            collect_core_expr(base, imports, used);
            collect_core_expr(start, imports, used);
            collect_core_expr(end, imports, used);
        }
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(e) = part {
                    collect_core_expr(e, imports, used);
                }
            }
        }
        Expr::ListLit(items, _) => {
            for e in items {
                collect_core_expr(e, imports, used);
            }
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, e) in fields {
                collect_core_expr(e, imports, used);
            }
        }
        Expr::MapLit(items, _) => {
            for (k, v) in items {
                collect_core_expr(k, imports, used);
                collect_core_expr(v, imports, used);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, e) in fields {
                collect_core_expr(e, imports, used);
            }
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                match arg {
                    EnumLitArg::Positional(e) => collect_core_expr(e, imports, used),
                    EnumLitArg::Named { expr, .. } => collect_core_expr(expr, imports, used),
                }
            }
        }
        Expr::PatternTest { subject, .. } => collect_core_expr(subject, imports, used),
        Expr::OrFallback {
            value, fallback, ..
        } => {
            collect_core_expr(value, imports, used);
            match fallback {
                OrFallback::Value(e) => collect_core_expr(e, imports, used),
                OrFallback::Return(Some(e), _) => collect_core_expr(e, imports, used),
                OrFallback::Return(None, _) => {}
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        collect_core_expr(&arg.expr, imports, used);
                    }
                }
            }
        }
        Expr::Lambda(lam) => match &lam.body {
            LambdaBody::Expr(e) => collect_core_expr(e, imports, used),
            LambdaBody::Block(stmts) => collect_core_stmts(stmts, imports, used),
        },
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            collect_core_expr(cond, imports, used);
            collect_core_stmts(then_body, imports, used);
            collect_core_expr(then_value, imports, used);
            collect_core_stmts(else_body, imports, used);
            collect_core_expr(else_value, imports, used);
        }
        Expr::FanOut { callee, items, .. } => {
            collect_core_expr(callee, imports, used);
            for item in items {
                collect_core_expr(item, imports, used);
            }
        }
        Expr::Int(_, _, _)
        | Expr::Float(_, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::Absent(_)
        | Expr::Todo { .. } => {}
    }
}

pub(crate) fn check_module_bodies(
    module: &mut crate::AST::LoadedModule,
    module_idx: usize,
    states: &[ModuleState],
    mode: CompileMode,
    freestanding: bool,
    summaries: &mut HashMap<String, EffectSummary>,
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
                    freestanding,
                    summaries,
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
                        freestanding,
                        summaries,
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
                        freestanding,
                        summaries,
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
                        freestanding,
                        summaries,
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
                    is_unsafe: false,
                    is_pure: false,
                    declared_effects: None,
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
                    summaries,
                ));
                t.body = synthetic.body;
            }
            Item::CodeModule(cm) => {
                // D-MOD2: type-check inline-module function bodies. Sibling calls were
                // already rewritten to mangled names by `mangle_inline_sibling_calls`,
                // and the mangled signatures are registered in `st.funcs`.
                if let Some(body) = &mut cm.body {
                    for inner in body.iter_mut() {
                        if let Item::Func(f) = inner {
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
                                summaries,
                            ));
                        }
                    }
                }
            }
            Item::ErrorConv(ec) => {
                // D-ERR-CONV: type-check the conversion body in the bundle path.
                let st = &states[module_idx];
                diags.extend(crate::Sema::Registration::check_error_conv_body(
                    ec,
                    &st.funcs,
                    &st.registry,
                    &st.structs,
                    &st.consts,
                    &st.trait_reg,
                    &ct_funcs,
                    &ct_externs,
                    &ct_base_dir,
                    &ct_globals,
                ));
            }
            _ => {}
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
    summaries: &mut HashMap<String, EffectSummary>,
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
        core_imports: &st.core_imports,
        code_modules: &st.code_modules,
        unqualified: &st.unqualified,
        unqualified_file: &st.unqualified_file,
        func_pub: &st.func_pub,
        diags: Vec::new(),
        scopes: vec![HashMap::new()],
        moved: HashMap::new(),
        loop_depth: 0,
        loop_labels: Vec::new(),
        fx_direct: std::collections::BTreeSet::new(),
        fx_edges: std::collections::BTreeSet::new(),
        fx_maximal: false,
        region_stack: Vec::new(),
        fx_regions: Vec::new(),
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
        trait_reg: &st.trait_reg,
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
        ck.diags.extend(check_pure_fn(f, &st.funcs));
    }
    // D-EFF1: record this function's effect summary for the whole-program
    // fixpoint (keyed by bare name / `Type::method`; cross-module effect
    // propagation is a later slice — intra-module + Core/builtin direct effects
    // are inferred here).
    summaries.insert(
        effect_key(owner_type, &f.name),
        EffectSummary {
            direct: std::mem::take(&mut ck.fx_direct),
            edges: std::mem::take(&mut ck.fx_edges),
            maximal: ck.fx_maximal,
            regions: std::mem::take(&mut ck.fx_regions),
        },
    );
    ck.diags
}

pub(crate) fn func_sig_to_fn_type(sig: &FuncSig) -> Type {
    Type::Fn {
        params: sig.params.iter().map(|(_, t)| t.clone()).collect(),
        ret: sig.return_type.clone().map(Box::new),
    }
}

pub(crate) fn fn_types_compatible(want: &Type, got: &Type) -> bool {
    let (
        Type::Fn {
            params: wp,
            ret: wr,
        },
        Type::Fn {
            params: gp,
            ret: gr,
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

