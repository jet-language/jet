use super::*;
use crate::ast::{
    AccessConvention, BinOp, BindPattern, Binding, ElseBranch,
    Expr, ForKind, IfStmt, IndexKind, LValue, Stmt, Type,
};
use crate::collections::is_map_key_type;
use crate::diag::{Diagnostic, Span, TextEdit};
use crate::generics::{
    e0905, e0909, generic_depth_exceeded, COMPARABLE,
};
use crate::syntax;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.lambda_mut_borrow_stack.push(HashSet::new());
        self.ct_scopes.push(HashMap::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        self.lint_unjoined_tasks_in_current_scope();
        self.scopes.pop();
        self.lambda_mut_borrow_stack.pop();
        self.ct_scopes.pop();
    }

    pub(crate) fn lambda_mut_borrow_active(&self, name: &str) -> bool {
        self.lambda_mut_borrow_stack
            .iter()
            .any(|s| s.contains(name))
    }

    pub(crate) fn current_ct_globals(&self) -> HashMap<String, crate::comptime::CtValue> {
        let mut globals = self.ct_globals.clone();
        for scope in &self.ct_scopes {
            for (name, value) in scope {
                globals.insert(name.clone(), value.clone());
            }
        }
        globals
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<&LocalInfo> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// A binding is borrowed (a `view`) when it is a `Read` parameter of a
    /// non-scalar type — in v1 those lower to `&T`, so the value can't be moved
    /// out of it. Used to decide where a consuming use must clone (B1).
    pub(crate) fn is_borrowed_binding(&self, name: &str) -> bool {
        self.lookup(name)
            .map(|info| {
                matches!(info.param_conv, Some(AccessConvention::Read)) && !info.ty.is_scalar()
            })
            .unwrap_or(false)
    }

    pub(crate) fn declare(&mut self, name: &str, name_span: Span, info: LocalInfo) {
        if self.lookup(name).is_some() || self.consts.contains_key(name) {
            self.diags.push(already_defined(name, name_span));
        }
        self.moved.remove(name);
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), info);
    }

    pub(crate) fn declare_loop_var(&mut self, name: String, name_span: Span, ty: &Type) {
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
                    sendable: true,
                    task_lint_span: None,
                },
            );
        }
    }

    pub(crate) fn check_declared_type(&mut self, ty: &Type, span: Span) {
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
                // Check imported file-module registries for pub types.
                if let Some(mods) = self.modules {
                    let found = self.imports.values().any(|&idx| {
                        mods[idx].registry.contains(n)
                            && mods[idx].type_pub.get(n).copied().unwrap_or(false)
                    });
                    if found {
                        return;
                    }
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
                let is_std_generic =
                    matches!(name.as_str(), "Task" | "Channel" | "Sender" | "Ptr");
                if !is_std_generic && !self.registry.contains(name) {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("there's no type called `{}`", name),
                        "generic types must name a struct or enum you defined".to_string(),
                        "check the spelling, or define the type first".to_string(),
                        Some(span),
                    ));
                }
                if !is_std_generic {
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
                                "every generic parameter needs a matching type argument"
                                    .to_string(),
                                format!(
                                    "write `{}`<{}>",
                                    name,
                                    params
                                        .iter()
                                        .map(|p| p.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
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
                        format!(
                            "`{}??` isn't supported — use one `?` only (S32)",
                            inner.name()
                        ),
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
                        "map keys must be Int, String, Bool, Char, or a payload-free enum"
                            .to_string(),
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
            Type::Tuple(fields) => {
                for (_, t) in fields {
                    self.check_declared_type(t, span);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn type_known(&self, ty: &Type) -> bool {
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
            Type::Tuple(fields) => fields.iter().all(|(_, t)| self.type_known(t)),
            _ => true,
        }
    }

    /// Returns true when a diagnostic was emitted (the mismatch is already
    /// reported); callers may add a context-specific error otherwise.
    pub(crate) fn check_type_assignable(&mut self, want: &Type, got: &Type, span: Span) -> bool {
        if want == got {
            return false;
        }
        if is_u8_ty(want) && *got == Type::Int {
            return false;
        }
        if result_used_where_plain_expected(want, got) {
            self.diags.push(Diagnostic::error(
                "E0401",
                format!(
                    "this needs {}, but the value is {}",
                    want.show(),
                    got.show()
                ),
                "a fallible result must be checked before its value is used".to_string(),
                format!(
                    "use `{}`, `{}`, or test with `== {}(...)` / `== {}(...)`",
                    syntax::OP_TRY_SUFFIX,
                    syntax::OP_FALLBACK,
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
                format!(
                    "this needs {}, but the value is {}",
                    want.show(),
                    got.show()
                ),
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
                format!(
                    "this needs {}, but the value is {}",
                    want.show(),
                    got.show()
                ),
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
                self.diags
                    .push(e0905(type_name, trait_name, span, needs_derive));
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

    pub(crate) fn report_option_mismatch(&mut self, want: &Type, got: &Type, span: Span) {
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

    pub(crate) fn check_block(&mut self, stmts: &mut [Stmt], new_scope: bool) {
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
    pub(crate) fn check_branches(&mut self, branches: &mut [&mut Vec<Stmt>]) {
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

    pub(crate) fn check_stmt(&mut self, stmt: &mut Stmt) {
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
                                        syntax::SIGIL_BIND_MUT
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
                                    syntax::SIGIL_BIND_IMMUT
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
                                    "declare it with `{} {} ...` instead",
                                    name,
                                    syntax::SIGIL_BIND_MUT
                                )
                            };
                            self.diags.push(Diagnostic::error(
                                "E0111",
                                what,
                                format!(
                                    "only `{}` bindings (and `{}` parameters) can be changed",
                                    syntax::SIGIL_BIND_MUT,
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
                    LValue::Index {
                        base,
                        index,
                        span,
                        kind,
                    } => {
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
                                                "`{}` must be declared mutable (`{}`) to change it",
                                                root,
                                                syntax::SIGIL_BIND_MUT
                                            ),
                                            "assigning into a collection changes it".to_string(),
                                            format!("declare `{} {} ...`", root, syntax::SIGIL_BIND_MUT),
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
                        if let Some(Type::Map {
                            key,
                            value: map_val_ty,
                        }) = base_ty
                        {
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
                                "e.g. `loop c in s.chars() { }` or `s.slice(0..2)`".to_string(),
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
                                syntax::OP_FALLBACK,
                                syntax::BUILTIN_PANIC
                            ),
                            Some(expr.span()),
                        ));
                    }
                    if is_task_type(&ty) {
                        self.diags.push(Diagnostic::lint(
                            "L1101",
                            "a spawned task is dropped without `.join()`".to_string(),
                            "the program may end before this task finishes".to_string(),
                            "store the task in a binding and call `.join()`".to_string(),
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
                        // Spawned task returns are checked separately by
                        // E1102, which avoids a generic E0206 cascade.
                        self.borrow_ctx = self.view_return || self.is_task_spawn;
                        let et = self.infer(e);
                        self.expected_type = saved_expected;
                        // E2302 (E2-M5): a `ref`-field struct built right here in
                        // a `return` is a construction site too — guard it like a
                        // `val` binding so a dangling stored ref never reaches
                        // codegen (which has no lifetime to lower it with).
                        self.check_stored_ref_fields(e);
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
                            // E2301 (tier-2 references, E2-M5): a `view` return that
                            // points into a *field of a local* names the owner that
                            // dies at the closing brace ("what owns this?"). The bare
                            // local case stays E0206. Only one diagnostic fires.
                            if matches!(e, Expr::Index { .. } | Expr::Slice { .. })
                                && self.view_return_local_owner(e).is_none()
                            {
                                // E2304 (E2-M5 zero-copy cell): an index/slice
                                // *into a parameter* the caller owns would be a
                                // sound borrow on paper, but the list/string
                                // helpers copy into a fresh value, so the view
                                // would point at a temporary. Reject in Jet
                                // words rather than let rustc choke (I2).
                                self.diags.push(Diagnostic::error(
                                    "E2304",
                                    "an indexed or sliced piece can't be handed back as a view"
                                        .to_string(),
                                    "indexing or slicing builds a fresh, owned piece, so there's no longer-lived value for a view to point at — the piece would vanish the moment this function returns"
                                        .to_string(),
                                    "return the piece owned (drop `view`; the caller keeps its own copy), or hand back a whole field with `view` and let the caller index it"
                                        .to_string(),
                                    Some(e.span()),
                                ));
                            } else if let Some(owner) = self.view_return_local_owner(e) {
                                self.diags.push(Diagnostic::error(
                                    "E2301",
                                    format!(
                                        "this view points into `{}`, which this function owns",
                                        owner
                                    ),
                                    format!(
                                        "`{}` is made here and freed when the function returns, so a view into its fields would outlive what owns it — there'd be nothing left to look at",
                                        owner
                                    ),
                                    "return an owned copy (`.clone()` the field into an owned return type), or accept the source as a parameter so the caller keeps owning it".to_string(),
                                    Some(e.span()),
                                ));
                            } else {
                                self.diags.push(Diagnostic::error(
                                    "E0206",
                                    "this value can't be handed back as a shared view".to_string(),
                                    "a `view` return may only point at a parameter, a whole-number or yes/no name, or a const — not at fresh text you just made here".to_string(),
                                    "return a parameter or const, copy with `.clone()` into an owned return type, or change `-> view` to `->`".to_string(),
                                    Some(e.span()),
                                ));
                            }
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
            Stmt::While {
                cond,
                body,
                span: _,
                label,
            } => {
                self.require_bool(cond, "a `while` condition");
                if let Some((n, _)) = label {
                    self.loop_labels.push(n.clone());
                }
                self.loop_depth += 1;
                self.check_block(body, true);
                self.loop_depth -= 1;
                if label.is_some() {
                    self.loop_labels.pop();
                }
            }
            Stmt::For {
                var,
                var_span,
                var2,
                kind,
                body,
                span: _,
                label,
            } => {
                if let Some((n, _)) = label {
                    self.loop_labels.push(n.clone());
                }
                match kind {
                ForKind::Range { start, end, step } => {
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
                    if let Some(step) = step {
                        // S22 (D-SG8): the stride must be a positive Int.
                        let t = self.infer(step);
                        if let Some(t) = t {
                            if t != Type::Int {
                                self.diags.push(Diagnostic::error(
                                    "E0123",
                                    format!(
                                        "a `for` range `step` must be {}, not {}",
                                        Type::Int.show(),
                                        t.show()
                                    ),
                                    "`step` is how far to count each turn, so it's a whole number (S22)"
                                        .to_string(),
                                    "use an Int step, like `0..10 step 2`".to_string(),
                                    Some(step.span()),
                                ));
                            }
                        }
                        if let Expr::Int(n, sp) = step {
                            if *n <= 0 {
                                self.diags.push(Diagnostic::error(
                                    "E0123",
                                    format!("a `for` range `step` must be positive, not {}", n),
                                    "a zero or negative step would never reach the end (S22)"
                                        .to_string(),
                                    "use a step of 1 or more, like `0..10 step 2`".to_string(),
                                    Some(*sp),
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
                            sendable: true,
                            task_lint_span: None,
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
                        // E2-M7: `loop line in handle.lines()` — streaming line iterator.
                        Some(Type::Named(n)) if n == "FileLines" => {
                            self.declare_loop_var(var.clone(), *var_span, &Type::String);
                        }
                        Some(other) => {
                            self.diags.push(Diagnostic::error(
                                    "E0109",
                                    format!(
                                        "`for x in` needs a list or map, not {}",
                                        other.show()
                                    ),
                                    "walk items with `loop item in items { }` or characters with `loop c in s.chars() { }`".to_string(),
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
                if label.is_some() {
                    self.loop_labels.pop();
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
                    self.diags
                        .push(loop_control_outside(syntax::KW_BREAK, *span));
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(syntax::KW_CONTINUE, *span));
                }
            }
            // D-LABEL1: `break @name` / `continue @name`.
            Stmt::BreakLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(syntax::KW_BREAK, *span));
                } else if !self.loop_labels.iter().any(|l| l == name) {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, *span));
                }
            }
            Stmt::ContinueLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(syntax::KW_CONTINUE, *span));
                } else if !self.loop_labels.iter().any(|l| l == name) {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, *span));
                }
            }
            Stmt::Loop {
                body: inner,
                label,
                ..
            } => {
                if let Some((n, _)) = label {
                    self.loop_labels.push(n.clone());
                }
                self.loop_depth += 1;
                self.check_block(inner, true);
                self.loop_depth -= 1;
                if label.is_some() {
                    self.loop_labels.pop();
                }
            }
            Stmt::Unsafe { audit, body, span } => {
                // L3101 (D-LL2): every `@unsafe` block needs an `@audit("…")`
                // reason on the line above so the safety case is on record.
                if audit.is_none() {
                    self.diags.push(Diagnostic::lint(
                        "L3101",
                        "this `@unsafe` block has no `@audit` reason".to_string(),
                        "every gated region records, in one line, why it can't break memory safety"
                            .to_string(),
                        "add `@audit(\"why this is safe\")` on the line above".to_string(),
                        Some(*span),
                    ));
                }
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(body, true);
                self.in_unsafe = prev;
            }
        }
    }

    pub(crate) fn check_if(&mut self, ifs: &mut IfStmt) {
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
                    sendable: true,
                    task_lint_span: None,
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

    pub(crate) fn check_condition_with_bindings(&mut self, cond: &mut Expr) -> HashMap<String, Type> {
        match cond {
            Expr::PatternTest {
                subject,
                pattern,
                span,
            } => self.check_pattern_test(subject, pattern, *span),
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

    pub(crate) fn check_switch(
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
                        sendable: true,
                        task_lint_span: None,
                    },
                );
            }
        }
        let all_pattern = subj_ty.is_some()
            && !arms.is_empty()
            && arms.iter().all(|a| {
                self.switch_arm_pattern(&a.cond, subj_name.as_deref(), subj_ty.as_ref().unwrap())
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
                                format!(
                                    "arm `{}` is unreachable — that case is already handled",
                                    variant
                                ),
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
                                sendable: true,
                                task_lint_span: None,
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
                self.require_bool(
                    &mut arm.cond,
                    &format!("an `{}` arm's condition", syntax::KW_IF),
                );
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
                            sendable: true,
                            task_lint_span: None,
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
                        let mut diag = Diagnostic::error(
                            "E0307",
                            format!(
                                "this `{}` doesn't cover every case — missing: {}",
                                syntax::KW_IF,
                                missing.join(", ")
                            ),
                            "every arm here is a pattern test, so each variant must appear once"
                                .to_string(),
                            format!("add an arm for: {}", missing.join(", ")),
                            Some(span),
                        );
                        // Attach a structured insert so LSP/CLI can add compilable arms.
                        if let Some(last_arm) = arms.last() {
                            let new_text = missing_arms_text(&st, &missing, subj_name.as_deref());
                            diag.edit = Some(TextEdit {
                                span: Span::new(last_arm.span.end, last_arm.span.end),
                                new_text,
                            });
                        }
                        self.diags.push(diag);
                    }
                }
            }
        } else if else_body.is_none() {
            self.diags.push(Diagnostic::error(
                "E0003",
                format!("this `{}` needs an `{}` arm", syntax::KW_IF, syntax::KW_ELSE),
                "mixed condition arms (or non-pattern arms) must always have a fallback (D-IF1)"
                    .to_string(),
                format!("add `{} {} {{ ... }}` after the last arm", syntax::KW_ELSE, syntax::OP_ARM_ARROW),
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

    pub(crate) fn resolve_type(&self, ty: Type) -> Type {
        match ty {
            Type::Named(n) if self.m9.is_trait_name(&n) && !self.registry.contains(&n) => {
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
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .into_iter()
                    .map(|(n, t)| (n, Box::new(self.resolve_type(*t))))
                    .collect(),
            ),
            other => other,
        }
    }

    pub(crate) fn type_param_has_bound(&self, ty: &Type, bound: &str) -> bool {
        match ty {
            Type::Named(n) => self
                .type_param_scope
                .iter()
                .find(|p| p.name == *n)
                .is_some_and(|p| p.bounds.iter().any(|b| b == bound)),
            _ => false,
        }
    }

    pub(crate) fn struct_subst(&self, type_name: &str, type_args: &[Type]) -> HashMap<String, Type> {
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

    pub(crate) fn check_binding(&mut self, b: &mut Binding) {
        if b.pattern.is_some() {
            self.check_destructuring_binding(b);
            return;
        }
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
                                "copy it instead: `{} {} {}.clone()`",
                                b.name,
                                if b.mutable {
                                    syntax::SIGIL_BIND_MUT
                                } else {
                                    syntax::SIGIL_BIND_IMMUT
                                },
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

        // E2302 (tier-2 references, E2-M5): a `ref` field stored from a value
        // that won't outlive the struct would dangle ("how long can this view
        // live?"). Inspected here at the binding site, read-only — the struct
        // literal itself is elaborated by check_struct_lit.
        self.check_stored_ref_fields(&b.init);

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
                    self.ct_scopes.last_mut().unwrap().insert(b.name.clone(), v);
                }
                Err(d) => self.diags.push(d),
            }
        }
        let binding_sendable = if let Expr::Lambda(lam) = &b.init {
            self.lambda_value_sendable(lam, &final_ty)
        } else {
            self.sendability_problem(&final_ty, true).is_none()
        };
        let task_lint_span = if is_task_type(&final_ty) {
            Some(b.name_span)
        } else {
            None
        };
        self.declare(
            &b.name,
            b.name_span,
            LocalInfo {
                ty: final_ty,
                mutable: b.mutable && !b.is_comptime,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                sendable: binding_sendable,
                task_lint_span,
            },
        );
    }

    /// S74: a `val`/`var` binding that destructures a struct (`Point { x, y }`)
    /// or a list (`[a, b]`). Each bound name is declared separately; move and
    /// mutability follow the per-name M2 rules. Struct destructuring is
    /// irrefutable (you may bind any subset of fields); list destructuring is
    /// guarded by a runtime length check in codegen, and a literal of the wrong
    /// length is caught here (E0315).
    pub(crate) fn check_destructuring_binding(&mut self, b: &mut Binding) {
        let inferred = self.infer(&mut b.init);
        let pattern = b.pattern.clone().expect("destructuring binding has a pattern");
        let Some(it) = inferred else {
            // The initializer itself didn't type-check; declare error
            // placeholders so the bound names don't cascade into E0107.
            for n in pattern.names() {
                self.declare_bound(&n.name, n.span, Type::Int, b.mutable);
            }
            return;
        };
        let it = self.resolve_type(it);
        match &pattern {
            BindPattern::Struct {
                type_name,
                type_span,
                fields,
                ..
            } => {
                let actual = match &it {
                    Type::Named(n) => Some(n.clone()),
                    Type::Apply { name, .. } => Some(name.clone()),
                    _ => None,
                };
                let is_struct = actual.as_deref().is_some_and(|n| {
                    self.struct_owner_module(n, None)
                        .and_then(|m| self.struct_fields_of(m, n))
                        .is_some()
                });
                if !is_struct {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "`{} {{ … }}` can only destructure a `{}` value, but this is {}",
                            type_name,
                            type_name,
                            it.show()
                        ),
                        "destructuring with `{ }` pulls fields out of a struct value"
                            .to_string(),
                        format!("destructure a `{}`, or bind the whole value with a name", type_name),
                        Some(*type_span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(&n.name, n.span, Type::Int, b.mutable);
                    }
                    return;
                }
                let actual = actual.unwrap();
                if actual != *type_name {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "this value is a `{}`, not a `{}`",
                            actual, type_name
                        ),
                        "the type named before `{ }` must match the value you destructure"
                            .to_string(),
                        format!("write `{} {{ … }}` to match the value", actual),
                        Some(*type_span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(&n.name, n.span, Type::Int, b.mutable);
                    }
                    return;
                }
                for f in fields {
                    // `field_type` resolves the field's type and reports E0302
                    // with a suggestion if the field name is unknown.
                    let fty = self.field_type(&it, &f.name, f.span).unwrap_or(Type::Int);
                    self.declare_bound(&f.name, f.span, fty, b.mutable);
                }
            }
            BindPattern::List { elems, span } => {
                let (elem_ty, fixed_len) = match &it {
                    Type::List(inner) => ((**inner).clone(), None),
                    // S76: [T#N] can be destructured; E0963 if count doesn't match.
                    Type::FixedList { elem, len } => ((**elem).clone(), Some(*len)),
                    _ => {
                        self.diags.push(Diagnostic::error(
                            "E0313",
                            format!(
                                "`[ … ]` can only destructure a list, but this is {}",
                                it.show()
                            ),
                            "destructuring with `[ ]` pulls elements out of a list value"
                                .to_string(),
                            "destructure a list, or bind the whole value with a name".to_string(),
                            Some(*span),
                        ));
                        for n in pattern.names() {
                            self.declare_bound(&n.name, n.span, Type::Int, b.mutable);
                        }
                        return;
                    }
                };
                // E0963: destructure count must match the fixed-size length.
                if let Some(fixed) = fixed_len {
                    if elems.len() as u64 != fixed {
                        self.diags.push(Diagnostic::error(
                            "E0963",
                            format!(
                                "destructuring with {} name{}, but this fixed-size list has {} element{}",
                                elems.len(),
                                if elems.len() == 1 { "" } else { "s" },
                                fixed,
                                if fixed == 1 { "" } else { "s" }
                            ),
                            "a fixed-size list `[T#N]` has a known length — the pattern must match exactly".to_string(),
                            format!(
                                "use {} name{} in the pattern",
                                fixed,
                                if fixed == 1 { "" } else { "s" }
                            ),
                            Some(*span),
                        ));
                    }
                }
                // A list literal has a known length: a mismatch is a compile
                // error rather than a runtime length failure.
                if let Expr::ListLit(items, _) = &b.init {
                    if items.len() != elems.len() {
                        self.diags.push(Diagnostic::error(
                            "E0315",
                            format!(
                                "this pattern binds {} item{}, but the list has {}",
                                elems.len(),
                                if elems.len() == 1 { "" } else { "s" },
                                items.len()
                            ),
                            "a list pattern must name exactly as many items as the list holds"
                                .to_string(),
                            format!(
                                "name {} item{} to match the list",
                                items.len(),
                                if items.len() == 1 { "" } else { "s" }
                            ),
                            Some(*span),
                        ));
                    }
                }
                for e in elems {
                    self.declare_bound(&e.name, e.span, elem_ty.clone(), b.mutable);
                }
            }
            BindPattern::Tuple { elems, span } => {
                let Type::Tuple(fields) = &it else {
                    self.diags.push(Diagnostic::error(
                        "E0313",
                        format!(
                            "`( … )` can only destructure a tuple, but this is {}",
                            it.show()
                        ),
                        "destructuring with `( )` pulls named members out of a tuple value"
                            .to_string(),
                        "destructure a tuple, or bind the whole value with a name".to_string(),
                        Some(*span),
                    ));
                    for n in pattern.names() {
                        self.declare_bound(&n.name, n.span, Type::Int, b.mutable);
                    }
                    return;
                };
                if elems.len() != fields.len() {
                    self.diags.push(Diagnostic::error(
                        "E0315",
                        format!(
                            "this pattern binds {} member{}, but the tuple has {}",
                            elems.len(),
                            if elems.len() == 1 { "" } else { "s" },
                            fields.len()
                        ),
                        "a tuple pattern must name exactly as many members as the tuple holds"
                            .to_string(),
                        format!(
                            "name {} member{} to match the tuple",
                            fields.len(),
                            if fields.len() == 1 { "" } else { "s" }
                        ),
                        Some(*span),
                    ));
                } else if let Expr::TupleLit(items, _, _) = &b.init {
                    if items.len() != elems.len() {
                        self.diags.push(Diagnostic::error(
                            "E0315",
                            format!(
                                "this pattern binds {} member{}, but the tuple literal has {}",
                                elems.len(),
                                if elems.len() == 1 { "" } else { "s" },
                                items.len()
                            ),
                            "a tuple pattern must name exactly as many members as the literal holds"
                                .to_string(),
                            format!(
                                "name {} member{} to match the tuple",
                                items.len(),
                                if items.len() == 1 { "" } else { "s" }
                            ),
                            Some(*span),
                        ));
                    }
                }
                for (e, (_, fty)) in elems.iter().zip(fields.iter()) {
                    self.declare_bound(&e.name, e.span, (**fty).clone(), b.mutable);
                }
            }
        }
        // Move the initializer when it's an owned, non-scalar local (M2): the
        // whole value is consumed to produce the bound parts.
        if let Expr::Ident(n, nspan) = &b.init {
            if let Some(info) = self.lookup(n) {
                if !info.ty.is_scalar() && info.param_conv.is_none() {
                    self.mark_moved(n.clone(), *nspan);
                }
            }
        }
    }

    /// Declare one name bound by a destructuring pattern (S74).
    pub(crate) fn declare_bound(&mut self, name: &str, span: Span, ty: Type, mutable: bool) {
        let sendable = self.sendability_problem(&ty, true).is_none();
        let task_lint_span = if is_task_type(&ty) { Some(span) } else { None };
        self.declare(
            name,
            span,
            LocalInfo {
                ty,
                mutable,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                sendable,
                task_lint_span,
            },
        );
    }

    // --- expressions ------------------------------------------------------

    pub(crate) fn require_bool(&mut self, e: &mut Expr, what: &str) {
        if let Some(t) = self.infer(e) {
            if t != Type::Bool {
                self.diags.push(Diagnostic::error(
                    "E0110",
                    format!(
                        "{} must be {}, but this is {}",
                        what,
                        Type::Bool.show(),
                        t.show()
                    ),
                    "the program needs a clear yes or no here".to_string(),
                    "compare the value to something, e.g. `x > 0` or `name == \"ok\"`".to_string(),
                    Some(e.span()),
                ));
            }
        }
    }

    pub(crate) fn unknown_name(&mut self, name: &str, span: Span) {
        let mut fix = format!("declare it first: `{} {} ...`", name, syntax::SIGIL_BIND_IMMUT);
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

}
