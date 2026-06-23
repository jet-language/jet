use super::*;
use crate::AST::{
    AccessConvention, BinOp, BindPattern, Binding, CallArg, ElseBranch,
    Expr, ForKind, IfStmt, IndexKind, LValue, Pattern, Stmt, Type,
};
use crate::Collections::is_map_key_type;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Generics::{
    e0905, e0909, generic_depth_exceeded, COMPARABLE,
};
use crate::Syntax;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.lambda_mut_borrow_stack.push(HashSet::new());
        self.ct_scopes.push(HashMap::new());
    }

    pub(crate) fn pop_scope(&mut self) {
        self.lint_unjoined_tasks_in_current_scope();
        // D-ALLOC2: arena `view`s declared in the scope being popped leave their
        // region here — drop their bookkeeping so a same-named binding in an
        // outer scope isn't mistaken for the view. The arena binding itself
        // lives in `self.scopes`, so it pops alongside.
        let depth = self.scopes.len();
        self.arena_views.retain(|_, v| v.scope_len < depth);
        self.scopes.pop();
        self.lambda_mut_borrow_stack.pop();
        self.ct_scopes.pop();
    }

    pub(crate) fn lambda_mut_borrow_active(&self, name: &str) -> bool {
        self.lambda_mut_borrow_stack
            .iter()
            .any(|s| s.contains(name))
    }

    pub(crate) fn current_ct_globals(&self) -> HashMap<String, crate::Comptime::CtValue> {
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
                    task_has_view_capture: false,
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
                if core_type_known(n) {
                    return;
                }
                if self.type_param_scope.iter().any(|p| p.name == *n) {
                    return;
                }
                if self.trait_reg.is_trait_name(n) {
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
                        Syntax::TYPE_INT,
                        Syntax::TYPE_FLOAT,
                        Syntax::TYPE_BOOL,
                        Syntax::TYPE_STRING
                    ),
                    "check the spelling, or define the struct or enum first".to_string(),
                    Some(span),
                ));
            }
            Type::Apply { name, args } => {
                let is_core_generic =
                    matches!(name.as_str(), "Task" | "Channel" | "Sender" | "Ptr");
                if !is_core_generic && !self.registry.contains(name) {
                    self.diags.push(Diagnostic::error(
                        "E0119",
                        format!("there's no type called `{}`", name),
                        "generic types must name a struct or enum you defined".to_string(),
                        "check the spelling, or define the type first".to_string(),
                        Some(span),
                    ));
                }
                if !is_core_generic {
                    let expected = self
                        .trait_reg
                        .struct_params
                        .get(name)
                        .or_else(|| self.trait_reg.enum_params.get(name));
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
                if !self.trait_reg.is_trait_name(t) {
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


    /// Returns true when a diagnostic was emitted (the mismatch is already
    /// reported); callers may add a context-specific error otherwise.
    pub(crate) fn check_type_assignable(&mut self, want: &Type, got: &Type, span: Span) -> bool {
        if want == got {
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
                    Syntax::OP_TRY_SUFFIX,
                    Syntax::OP_FALLBACK,
                    Syntax::LIT_OK,
                    Syntax::LIT_ERR
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
                    Syntax::LIT_VALUE,
                    Syntax::LIT_NULL,
                    Syntax::LIT_VALUE
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
                format!("wrap it with `{}(...)`", Syntax::LIT_VALUE),
                Some(span),
            ));
            return true;
        }
        match (want, got) {
            (Type::TraitObject(trait_name), Type::Named(type_name)) => {
                if self.trait_reg.implements_trait(type_name, trait_name) {
                    return false;
                }
                let needs_derive = trait_name == COMPARABLE || trait_name == "Serialize";
                self.diags
                    .push(e0905(type_name, trait_name, span, needs_derive));
                return true;
            }
            (Type::TraitObject(trait_name), Type::Apply { name, .. }) => {
                if self.trait_reg.implements_trait(name, trait_name) {
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
        // D-L0201 liveness gate: before checking each statement, record the
        // tail of the current block (statements that follow it).  The lint
        // helper `is_name_live_after` reads this to decide whether the cloned
        // value is dead at this point.  We push the previous frame onto the
        // liveness_frames stack so `is_name_live_after` can walk enclosing
        // scopes; on exit we pop and restore.
        let saved_ptr = self.stmt_tail_ptr;
        let saved_len = self.stmt_tail_len;
        // Push the caller's frame as an enclosing scope (non-null only).
        let pushed_frame = !saved_ptr.is_null();
        if pushed_frame {
            self.liveness_frames.push((saved_ptr, saved_len));
        }
        for i in 0..stmts.len() {
            // tail = stmts[i+1..], i.e. the statements after index i.
            let tail = &stmts[i + 1..];
            self.stmt_tail_ptr = tail.as_ptr();
            self.stmt_tail_len = tail.len();
            self.check_stmt(&mut stmts[i]);
        }
        if pushed_frame {
            self.liveness_frames.pop();
        }
        self.stmt_tail_ptr = saved_ptr;
        self.stmt_tail_len = saved_len;
        if new_scope {
            self.pop_scope();
        }
    }

    /// D-L0201 liveness gate: returns `true` when `name` is referenced in any
    /// statement that follows the current statement in the innermost block.
    /// When true the implicit clone is *necessary* — suppressing L0201 is safe.
    /// When false we can't prove liveness from this frame, so the clone may be
    /// wasteful and L0201 fires.
    ///
    /// Checks the current block's tail AND all enclosing block tails pushed
    /// by `check_block`, so a clone inside a nested `if` body is not flagged
    /// when the value is used again in the enclosing block after the `if`.
    pub(crate) fn is_name_live_after(&self, name: &str) -> bool {
        // Check the innermost block's tail first.
        if !self.stmt_tail_ptr.is_null() && self.stmt_tail_len > 0 {
            // SAFETY: stmt_tail_ptr + stmt_tail_len describe a valid slice that was
            // set from `&stmts[i+1..]` just before the current check_stmt call.
            // The slice's data lives in the Program AST, which is `&mut Program`
            // at the call site and outlives the Checker.  We only read (no writes)
            // and only during `check_stmt`, so no aliasing issues.
            let tail = unsafe {
                std::slice::from_raw_parts(self.stmt_tail_ptr, self.stmt_tail_len)
            };
            if tail.iter().any(|s| stmt_refs_name(s, name)) {
                return true;
            }
        }
        // Walk enclosing frames (innermost pushed last) — if the name appears
        // in any enclosing block after the point this nested block was entered,
        // the clone is necessary.
        for &(ptr, len) in self.liveness_frames.iter().rev() {
            if !ptr.is_null() && len > 0 {
                // SAFETY: same as above — each frame was set from a block slice
                // in the Program AST that outlives the Checker.
                let frame = unsafe { std::slice::from_raw_parts(ptr, len) };
                if frame.iter().any(|s| stmt_refs_name(s, name)) {
                    return true;
                }
            }
        }
        false
    }

    /// Check two alternative branches with independent move states, then
    /// keep the union (a value moved in either branch counts as gone).
    pub(crate) fn check_stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Val(b) => self.check_binding(b),
            Stmt::Assign {
                target,
                op,
                op_span: _,
                value,
            } => {
                let is_compound = op.is_some();
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
                // D-UNINIT1: a plain `name = …` initializes a `#Uninit` binding; a
                // compound `name += …` reads it first, so it's a read-before-write.
                if let LValue::Local { name, name_span } = &*target {
                    if self.uninit.contains_key(name) {
                        if is_compound {
                            self.diags.push(Diagnostic::error(
                                "E0420",
                                format!("`{}` may be read before it is given a value", name),
                                format!(
                                    "`{}+=` reads `{}` first, but it was declared `#Uninit` and has no value yet",
                                    name, name
                                ),
                                format!("give `{}` a value with `{} = …` before updating it", name, name),
                                Some(*name_span),
                            ));
                        }
                        self.uninit.remove(name);
                    }
                }
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
                                        Syntax::SIGIL_BIND_MUT
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
                                    Syntax::SIGIL_BIND_IMMUT
                                )
                            };
                            let fix = if info.param_conv.is_some() {
                                format!(
                                    "mark the parameter `{} {}: {}` if the function should change it",
                                    Syntax::KW_MUTATE,
                                    name,
                                    info.ty.name()
                                )
                            } else {
                                format!(
                                    "declare it with `{} {} ...` instead",
                                    name,
                                    Syntax::SIGIL_BIND_MUT
                                )
                            };
                            self.diags.push(Diagnostic::error(
                                "E0111",
                                what,
                                format!(
                                    "only `{}` bindings (and `{}` parameters) can be changed",
                                    Syntax::SIGIL_BIND_MUT,
                                    Syntax::KW_MUTATE
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
                                                Syntax::SIGIL_BIND_MUT
                                            ),
                                            "assigning into a collection changes it".to_string(),
                                            format!("declare `{} {} ...`", root, Syntax::SIGIL_BIND_MUT),
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
                    // D-MUTSELF1: a field-assignment `place.field [op]= v`. The place
                    // must be a CHANGEABLE place: a `mut self` receiver, or a `:=`/`mut`
                    // local. A non-`mut` `self` (shared-read receiver) or a `@=`/shared
                    // binding is E0205, pointed at the assignment, with a "write the
                    // receiver as `mut self`" / "make it changeable" fix (owner Q1).
                    LValue::Field { base, field, span } => {
                        self.borrow_ctx = true;
                        let base_ty = self.infer(base);
                        // Validate the field exists and get its type (emits E0302 on a
                        // bad field). The value's type must match the field type (E0108).
                        if let Some(bt) = &base_ty {
                            if let Some(ft) = self.field_type(bt, field, *span) {
                                if let Some(vt) = &vt {
                                    if *vt != ft && ft != Type::Named(String::new()) {
                                        self.diags.push(Diagnostic::error(
                                            "E0108",
                                            format!(
                                                "field `{}` holds {}, but this value is {}",
                                                field,
                                                ft.show(),
                                                vt.show()
                                            ),
                                            "a field keeps one type for its whole life"
                                                .to_string(),
                                            type_fix_hint(&ft, vt),
                                            Some(value.span()),
                                        ));
                                    }
                                }
                            }
                        }
                        // The root place must be changeable. The headline is `self`:
                        // a `mut self` receiver (param_conv == Mutate) may be mutated;
                        // a shared-read `self`, or any non-`mut` local, may not.
                        if let Some(root) = expr_root_ident(base) {
                            let root = root.to_string();
                            if let Some(info) = self.lookup(&root) {
                                if !info.mutable {
                                    let is_self = root == Syntax::KW_SELF;
                                    let what = if is_self {
                                        format!(
                                            "can't change `{}` through a shared `{}` receiver",
                                            field,
                                            Syntax::KW_SELF
                                        )
                                    } else {
                                        format!("can't change `{}` — `{}` isn't changeable", field, root)
                                    };
                                    let fix = if is_self {
                                        format!(
                                            "write the receiver as `{} {}` so the method may change it",
                                            Syntax::KW_MUTATE,
                                            Syntax::KW_SELF
                                        )
                                    } else if info.param_conv.is_some() {
                                        format!(
                                            "mark the parameter `{} {}: {}` if the function should change it",
                                            Syntax::KW_MUTATE,
                                            root,
                                            info.ty.name()
                                        )
                                    } else {
                                        format!(
                                            "declare it with `{} {} ...` so it can change",
                                            root,
                                            Syntax::SIGIL_BIND_MUT
                                        )
                                    };
                                    self.diags.push(Diagnostic::error(
                                        "E0205",
                                        what,
                                        "while something is being changed in place, the place that owns it must be changeable".to_string(),
                                        fix,
                                        Some(*span),
                                    ));
                                }
                            }
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
                                Syntax::OP_TRY_SUFFIX,
                                Syntax::OP_FALLBACK,
                                Syntax::BUILTIN_PANIC
                            ),
                            Some(expr.span()),
                        ));
                    }
                    if is_task_type(&ty) {
                        self.diags.push(Diagnostic::lint(
                            "L1101",
                            "a spawned task is dropped without `.join()`".to_string(),
                            "the program may end before this task finishes".to_string(),
                            "store the task in a binding and call `.join()`, or chain `.detach()` for fire-and-forget".to_string(),
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
                        // D-ALLOC2: E0631 — returning an arena `view` would let
                        // it outlive the arena (the arena drops at scope end).
                        if let Expr::Ident(n, nspan) = &*e {
                            if self.is_arena_view(n) {
                                self.report_view_escape(n, "be returned", *nspan);
                            }
                        }
                        // Returning a borrowed parameter would move out of a
                        // borrow in the generated Rust (I2) — require a copy.
                        if let Expr::Ident(n, nspan) = &*e {
                            if let Some(info) = self.lookup(n) {
                                if !self.view_return
                                    && !info.ty.is_scalar()
                                    && matches!(
                                        info.param_conv,
                                        Some(AccessConvention::Read)
                                            | Some(AccessConvention::Write)
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
                                            Syntax::KW_MOVE,
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
            // D-WHEN1/D-WHEN2 (ratified 2026-06-19): compile-time conditional.
            Stmt::ComptimeIf { .. } => self.check_comptime_if(stmt),
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
                // D-UNINIT1: a loop body may run 0 times, so writes inside it don't
                // count as initializing after the loop.
                let saved_u = self.uninit.clone();
                self.check_block(body, true);
                self.uninit = saved_u;
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
                // D-UNINIT1: a loop body may run 0 times; writes inside it don't
                // count as initializing after the loop.
                let saved_u = self.uninit.clone();
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
                        if let Expr::Int(n, sp, _) = step {
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
                            task_has_view_capture: false,
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
                        // D-STDIN1=A: `loop line in io.stdin().lines()` — streaming stdin iterator.
                        Some(Type::Named(n)) if n == "StdinLines" => {
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
                self.uninit = saved_u;
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
                        .push(loop_control_outside(Syntax::KW_BREAK, *span));
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_CONTINUE, *span));
                }
            }
            // D-LABEL1: `break @name` / `continue @name`.
            Stmt::BreakLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_BREAK, *span));
                } else if !self.loop_labels.iter().any(|l| l == name) {
                    self.diags
                        .push(undefined_loop_label(name, &self.loop_labels, *span));
                }
            }
            Stmt::ContinueLabel(name, span) => {
                if self.loop_depth == 0 {
                    self.diags
                        .push(loop_control_outside(Syntax::KW_CONTINUE, *span));
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
                let saved_u = self.uninit.clone();
                self.check_block(inner, true);
                self.uninit = saved_u;
                self.loop_depth -= 1;
                if label.is_some() {
                    self.loop_labels.pop();
                }
            }
            Stmt::Unsafe { audit, body, span } => {
                // L3101 (D-LL2): every `#Unsafe` block needs a `#Audit("…")`
                // reason on the line above so the safety case is on record.
                if audit.is_none() {
                    self.diags.push(Diagnostic::lint(
                        "L3101",
                        "this `#Unsafe` block has no `#Audit` reason".to_string(),
                        "every gated region records, in one line, why it can't break memory safety"
                            .to_string(),
                        "add `#Audit(\"why this is safe\")` on the line above".to_string(),
                        Some(*span),
                    ));
                }
                let prev = self.in_unsafe;
                self.in_unsafe = true;
                self.check_block(body, true);
                self.in_unsafe = prev;
            }
            // D-REGION1 (opt B): an explicit `region r { … }`. A fresh lexical
            // scope: arena `view`s allocated inside cannot escape it (the
            // E0631 escape rule is enforced against the scope floor, identical
            // to the implicit scope-inferred region of opt A). The region name
            // is documentary in v1 — it labels the scope for the reader; the
            // bound is the scope itself.
            Stmt::Region { body, .. } => {
                self.check_block(body, true);
            }
            // D-EFF1 / D-QUAL1: a `#Caps(Net, Db) { … }` effect-restriction
            // region. Validate the cap names (E0119), open an accumulator so the
            // effects reached inside are tallied, check the body, then seal the
            // region for the post-pass E0741 subset check. A lexical scope.
            Stmt::Caps { caps, caps_span, body, .. } => {
                let mut cap_set = crate::Sema::EffectSet::new();
                let mut bad = false;
                for (name, span) in caps.iter() {
                    match crate::Sema::Effect::parse(name) {
                        Some(e) => {
                            cap_set.insert(e);
                        }
                        None => {
                            self.diags.push(unknown_effect(name, *span));
                            bad = true;
                        }
                    }
                }
                self.region_stack.push(crate::Sema::RegionAccum {
                    caps: cap_set,
                    caps_span: *caps_span,
                    direct: crate::Sema::EffectSet::new(),
                    edges: std::collections::BTreeSet::new(),
                    maximal: false,
                });
                self.check_block(body, true);
                let acc = self.region_stack.pop().expect("pushed above");
                // Skip the E0741 subset check when a cap name was invalid (the
                // cap set is incomplete) — E0119 is the real problem to fix.
                if !bad {
                    self.fx_regions.push(crate::Sema::RegionSummary {
                        caps: acc.caps,
                        direct: acc.direct,
                        edges: acc.edges,
                        maximal: acc.maximal,
                        caps_span: acc.caps_span,
                    });
                }
            }
            // D-CTX1 (ratified 2026-06-22, G2): `#Context(field: value) { … }`.
            // Type-check each field value: `allocator` must be an allocator
            // handle type; `logger` must be a logger value. E0762 on mismatch.
            // Q1 = A2: explicit allocator args at call sites override the
            // ambient — no static binding done here, only type validation and
            // block body checking. Q2 = Cβ: restore is per-block (RAII guard).
            // D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input
            // block. No type-checking beyond the body; the block is impure (IO
            // effect), so it is rejected inside `#Pure fn` (same rule as `io.input`).
            // `use core.term` is NOT required to write a `live` block — the block
            // is its own syntactic gate. `term.read_key()` does need the import.
            // E3301: freestanding builds have no terminal device.
            Stmt::Live { body, span } => {
                if self.in_pure {
                    self.diags.push(crate::Sema::e3401(
                        &self.fn_name.clone(),
                        "live { … }",
                        &[],
                        *span,
                    ));
                }
                if self.freestanding {
                    self.diags.push(crate::Sema::e3301(
                        "live { … }",
                        "Terminal I/O requires an OS terminal device. Build without `--freestanding`.",
                        *span,
                    ));
                }
                self.check_block(body, true);
            }
            Stmt::ContextBlock { fields, body, span } => {
                for (field_name, value_expr, field_span) in fields.iter_mut() {
                    let ty = self.infer(value_expr);
                    match field_name.as_str() {
                        crate::Syntax::CTX_FIELD_ALLOCATOR => {
                            // Must be one of the known allocator handle types.
                            let ok = match &ty {
                                Some(Type::Named(n)) => {
                                    crate::Codegen::alloc_handle_rust_type(n).is_some()
                                }
                                _ => false,
                            };
                            if !ok {
                                let got = ty.as_ref().map(|t| t.show()).unwrap_or_else(|| "unknown".to_string());
                                self.diags.push(Diagnostic::error(
                                    "E0762",
                                    format!("`allocator` needs an allocator, got {}", got),
                                    "the `allocator` field takes an `Arena`, `Bump`, `Pool`, or `Fixed` value".to_string(),
                                    "pass an allocator, e.g. `mem.Arena.new()`".to_string(),
                                    Some(*field_span),
                                ));
                            }
                        }
                        crate::Syntax::CTX_FIELD_LOGGER => {
                            // v1: any value accepted for logger; a future Logger
                            // type will narrow this. No E0762 for logger yet.
                            let _ = ty;
                        }
                        _ => {
                            // Parser already rejected unknown fields (E0761);
                            // this arm is unreachable in practice.
                        }
                    }
                }
                self.check_block(body, true);
            }
        }
    }

    pub(crate) fn check_if(&mut self, ifs: &mut IfStmt) {
        let before = self.moved.clone();
        let mut after = before.clone();
        // D-UNINIT1: definite-assignment merge. A `#Uninit` name is initialized
        // after the `if` only if it is written on *every* path; it stays uninit if
        // still-uninit in any branch (or, with no `else`, on the fall-through).
        let before_u = self.uninit.clone();
        let mut after_u: HashMap<String, Span> = HashMap::new();
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
                    task_has_view_capture: false,
                },
            );
        }
        self.check_block(&mut ifs.then_body, false);
        self.pop_scope();
        for (k, v) in self.moved.drain() {
            after.entry(k).or_insert(v);
        }
        for (k, v) in std::mem::take(&mut self.uninit) {
            after_u.entry(k).or_insert(v);
        }
        self.moved = before.clone();
        self.uninit = before_u.clone();
        match &mut ifs.else_branch {
            None => {
                // The cond-false path runs no branch, so everything stays uninit.
                for (k, v) in &before_u {
                    after_u.entry(k.clone()).or_insert(*v);
                }
            }
            Some(ElseBranch::Else(else_body)) => {
                self.check_block(else_body, true);
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
                for (k, v) in std::mem::take(&mut self.uninit) {
                    after_u.entry(k).or_insert(v);
                }
            }
            Some(ElseBranch::ElseIf(next)) => {
                self.check_if(next);
                for (k, v) in self.moved.drain() {
                    after.entry(k).or_insert(v);
                }
                for (k, v) in std::mem::take(&mut self.uninit) {
                    after_u.entry(k).or_insert(v);
                }
            }
        }
        self.moved = after;
        self.uninit = after_u;
    }

    /// D-WHEN1/D-WHEN2 (ratified 2026-06-19): check a `comptime if` statement.
    ///
    /// Steps:
    /// 1. Evaluate the condition with the comptime interpreter. The condition
    ///    must be Bool and comptime-evaluable (else E0989).
    /// 2. Select the arm whose condition is true.
    /// 3. Full type-check + lower the selected arm (`check_block`).
    /// 4. D-WHEN2: run a name-resolution-only pass over the dropped arm —
    ///    we enter `in_dropped_comptime_arm` mode, call `check_block`, but
    ///    then keep only `E0107` (unknown name) diagnostics from that pass.
    ///    This catches typos in dead code without requiring the arm to
    ///    type-check against the current context.
    pub(crate) fn check_comptime_if(&mut self, stmt: &mut Stmt) {
        let Stmt::ComptimeIf {
            cond,
            cond_span,
            then_body,
            else_body,
            selected_then,
            ..
        } = stmt
        else {
            return;
        };
        // Step 1: evaluate the condition at comptime.
        let globals = self.current_ct_globals();
        let selected = match crate::Comptime::evaluate_owned(
            cond,
            self.ct_funcs,
            self.ct_externs,
            self.ct_base_dir,
            &globals,
        ) {
            Ok(crate::Comptime::CtValue::Bool(b)) => b,
            Ok(_) => {
                // The condition evaluated but isn't Bool — fall back to E0989
                // (non-Bool condition is the same as non-comptime for users).
                self.diags.push(Diagnostic::error(
                    "E0989",
                    format!(
                        "a `comptime if` condition must be {}, not another type",
                        Type::Bool.show()
                    ),
                    "the condition selects a branch at compile time — it must be true or false"
                        .to_string(),
                    "write a Bool comptime expression, like `comptime if FLAG { … }`".to_string(),
                    Some(*cond_span),
                ));
                return;
            }
            Err(_d) => {
                // Condition failed to evaluate — not comptime-evaluable (E0989).
                self.diags.push(Diagnostic::error(
                    "E0989",
                    "this `comptime if` condition can't be known at compile time".to_string(),
                    "a `comptime if` condition must be a comptime expression — a `comptime` binding, a literal, or a pure function call with comptime arguments (D-WHEN1)".to_string(),
                    "use a `comptime` binding: `comptime FLAG = …; comptime if FLAG { … }`"
                        .to_string(),
                    Some(*cond_span),
                ));
                return;
            }
        };
        *selected_then = Some(selected);

        // Step 3: full type-check on the selected arm.
        if selected {
            self.check_block(then_body, true);
        } else if let Some(eb) = else_body {
            self.check_block(eb, true);
        }

        // Step 4: D-WHEN2 — name-resolution-only pass on the dropped arm.
        let dropped_arm: Option<&mut Vec<Stmt>> = if selected {
            else_body.as_mut()
        } else {
            Some(then_body)
        };
        if let Some(dropped) = dropped_arm {
            let diag_before = self.diags.len();
            let prev_flag = self.in_dropped_comptime_arm;
            self.in_dropped_comptime_arm = true;
            self.check_block(dropped, true);
            self.in_dropped_comptime_arm = prev_flag;
            // D-WHEN2: keep name-resolution diagnostics from the dropped arm
            // (unknown variable E0107, unknown function E0102) so typos still
            // teach, but drop all type-checking diagnostics.
            let dropped_diags: Vec<Diagnostic> = self.diags.drain(diag_before..).collect();
            for d in dropped_diags {
                if d.code == "E0107" || d.code == "E0102" {
                    self.diags.push(d);
                }
            }
        }
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
        arms: &mut [crate::AST::SwitchArm],
        else_body: &mut Option<Vec<Stmt>>,
        span: Span,
    ) {
        let subj_ty = self.infer(subject);
        let subj_name = match &*subject {
            Expr::Ident(n, _) => Some(n.clone()),
            _ if subj_ty.as_ref().is_some_and(|t| t.is_fallible()) => {
                Some(Syntax::KW_IT.to_string())
            }
            _ => None,
        };
        let it_scope = subj_name.as_deref() == Some(Syntax::KW_IT);
        if it_scope {
            self.push_scope();
            if let Some(st) = subj_ty.clone() {
                self.declare(
                    Syntax::KW_IT,
                    span,
                    LocalInfo {
                        ty: st,
                        mutable: false,
                        param_conv: None,
                        decl_loop_depth: self.loop_depth,
                        sendable: true,
                        task_lint_span: None,
                        task_has_view_capture: false,
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
                    // D-PATO: or-patterns cover multiple variants; insert all of them.
                    let covered_names: Vec<String> = if let Pattern::Or(alts, _) = &pattern {
                        alts.iter().filter_map(pattern_variant_name).collect()
                    } else if let Some(v) = pattern_variant_name(&pattern) {
                        vec![v]
                    } else {
                        Vec::new()
                    };
                    for variant in covered_names {
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
                                task_has_view_capture: false,
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
                    &format!("an `{}` arm's condition", Syntax::KW_IF),
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
                            task_has_view_capture: false,
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
                // D-PATR: Int/Char are open scalar types — range arms can never
                // prove totality, so an `else` (or wildcard) is always required.
                // `missing_pattern_coverage` returns None for Int/Char (infinite
                // domain), so we detect this case separately.
                let open_scalar_no_else = matches!(st, Type::Int | Type::Char)
                    && else_body.is_none();
                if open_scalar_no_else {
                    self.diags.push(Diagnostic::error(
                        "E0307",
                        format!(
                            "this `{}` over `{}` has no `{}` arm — range arms can't cover every value",
                            Syntax::KW_IF,
                            st.show(),
                            Syntax::KW_ELSE,
                        ),
                        format!(
                            "`{}` has infinitely many values; range arms only cover a subset (D-PATR)",
                            st.show()
                        ),
                        format!(
                            "add `{} {} {{ … }}` to handle values not matched by any range",
                            Syntax::KW_ELSE,
                            Syntax::OP_ARM_ARROW
                        ),
                        Some(span),
                    ));
                } else if let Some(missing) = missing_pattern_coverage(&st, &covered, self.registry) {
                    if else_body.is_none() {
                        let mut diag = Diagnostic::error(
                            "E0307",
                            format!(
                                "this `{}` doesn't cover every case — missing: {}",
                                Syntax::KW_IF,
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
                format!("this `{}` needs an `{}` arm", Syntax::KW_IF, Syntax::KW_ELSE),
                "mixed condition arms (or non-pattern arms) must always have a fallback (D-IF1)"
                    .to_string(),
                format!("add `{} {} {{ ... }}` after the last arm", Syntax::KW_ELSE, Syntax::OP_ARM_ARROW),
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
            Type::Named(n) if self.trait_reg.is_trait_name(&n) && !self.registry.contains(&n) => {
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
            .trait_reg
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

    /// D-UNINIT1 (opt C): `#Uninit name: Type` — gate on `use core.mem`, restrict
    /// to plain-data types (E0423), declare the binding, and record it as
    /// not-yet-written so the dataflow can prove write-before-read (E0420).
    pub(crate) fn check_uninit_binding(&mut self, b: &mut Binding) {
        let has_mem = self
            .core_imports
            .values()
            .any(|m| m == Syntax::CORE_MEM_MODULE);
        if !has_mem {
            self.diags.push(Diagnostic::error(
                "E0424",
                format!("`#{}` needs the low-level memory tier", Syntax::ATTR_UNINIT),
                format!(
                    "`#{}` skips the automatic zero-fill — an expert-tier operation",
                    Syntax::ATTR_UNINIT
                ),
                format!(
                    "add `use {}` at the top of this file to opt in",
                    Syntax::CORE_MEM_MODULE
                ),
                Some(b.name_span),
            ));
        }
        let (ty, ty_span) = match (&b.ty, b.ty_span) {
            (Some(t), Some(s)) => (self.resolve_type(t.clone()), s),
            _ => return,
        };
        if let Some(slot) = b.ty.as_mut() {
            *slot = ty.clone();
        }
        self.check_declared_type(&ty, ty_span);
        if !is_pod_uninit_type(&ty) {
            self.diags.push(Diagnostic::error(
                "E0423",
                format!("`#{}` needs a plain-data type", Syntax::ATTR_UNINIT),
                format!(
                    "`{}` may own heap memory or need cleanup, so leaving it uninitialized is unsafe",
                    ty.show()
                ),
                "use plain data — a number, `Bool`, `Char`, `U8`, or a fixed array of those (e.g. `[4096]U8`)".to_string(),
                Some(ty_span),
            ));
        }
        self.declare(
            &b.name,
            b.name_span,
            LocalInfo {
                ty,
                mutable: true,
                param_conv: None,
                decl_loop_depth: self.loop_depth,
                sendable: true,
                task_lint_span: None,
                task_has_view_capture: false,
            },
        );
        self.uninit.insert(b.name.clone(), b.name_span);
    }

    /// D-UNINIT1: clear an `#Uninit` binding's not-yet-written flag when it is
    /// passed as a `mut` argument (the fill case) — the callee writes it. Call
    /// before inferring the args so the read-hook doesn't flag the fill site.
    pub(crate) fn clear_uninit_mut_args(&mut self, args: &[CallArg]) {
        if self.uninit.is_empty() {
            return;
        }
        for arg in args {
            if arg.convention == AccessConvention::Write {
                if let Expr::Ident(n, _) = &arg.expr {
                    self.uninit.remove(n);
                }
            }
        }
    }

    pub(crate) fn check_binding(&mut self, b: &mut Binding) {
        // D-DETACH1: record the binding name so report_unsendable can flag view-capturing tasks.
        let prev_binding_name = self.current_binding_name.take();
        self.current_binding_name = Some(b.name.clone());
        if b.pattern.is_some() {
            self.check_destructuring_binding(b);
            self.current_binding_name = prev_binding_name;
            return;
        }
        if b.uninit {
            self.check_uninit_binding(b);
            self.current_binding_name = prev_binding_name;
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
                            resolved_ret: None,
                        };
                    } else if matches!(
                        info.param_conv,
                        Some(AccessConvention::Read) | Some(AccessConvention::Write)
                    ) {
                        self.diags.push(Diagnostic::error(
                            "E0120",
                            format!("`{}` is only borrowed here, so it can't be moved", n),
                            "this function reads the value but doesn't own it".to_string(),
                            format!(
                                "copy it instead: `{} {} {}.clone()`",
                                b.name,
                                if b.mutable {
                                    Syntax::SIGIL_BIND_MUT
                                } else {
                                    Syntax::SIGIL_BIND_IMMUT
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

        // E2502 (E2-M7): a line stream — `FileReader.lines()` / `StdinHandle
        // .lines()` — is a loop-source-only value. It may only be consumed
        // directly by `loop line in handle.lines()`; binding it to a name lets it
        // escape loop position, where there is no meaningful lowering. (Codegen
        // previously emitted a placeholder that rustc rejected — an I2 hole. This
        // moves the guarantee into sema, c109/I3.)
        if let Some(Type::Named(n)) = &it {
            if n == "FileLines" || n == "StdinLines" {
                self.diags.push(Diagnostic::error(
                    "E2502",
                    "a line stream can only be used directly in a loop".to_string(),
                    "`.lines()` hands back a lazy line reader meant to be iterated in place; storing it in a name would let it leave the loop, where it has no use".to_string(),
                    format!(
                        "iterate it directly: `loop {} in handle.lines() {{ … }}`",
                        if b.name.is_empty() { "line" } else { b.name.as_str() }
                    ),
                    Some(b.init.span()),
                ));
            }
        }

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
                // D-SG9: a fixed-width literal is range-checked and re-typed in
                // `infer` (E1003), so it arrives matching `annot`. A non-literal
                // width mismatch falls to E0108 below — no implicit narrowing or
                // widening between integer widths.
                if annot != actual {
                    // D-DIST1/D-DIST3 (E0128): distinct-type coercion is never implicit.
                    let distinct_name = if let Type::Named(n) = &annot {
                        if self.registry.is_distinct(n) { Some(n.clone()) } else { None }
                    } else { None };
                    if let Some(dt) = distinct_name {
                        self.diags.push(Diagnostic::error(
                            "E0128",
                            format!("a `{}` can't be used where a `{}` is expected", actual.name(), dt),
                            format!("`{}` and `{}` are different types — even though `{}` is built on `{}`, one is never accepted in place of the other", dt, actual.name(), dt, self.registry.distinct_base(&dt).map(|t| t.name()).unwrap_or_default()),
                            format!("construct a `{}`: `{}({})`", dt, dt, "expr"),
                            Some(b.init.span()),
                        ));
                    } else {
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
            match crate::Comptime::evaluate_owned(
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
        // D-ALLOC2: `x :: arena.alloc(v)` makes `x` a scope-bound view into
        // `arena`. Record it so E0631 (escape) / E0632 (use-after-reset) can
        // fire, and flag the binding for codegen (it lowers to a `&mut T`, read
        // through a deref). E0631: a binding whose *initializer is itself a view
        // name* (`y :: x`) would move the view to a new — possibly
        // longer-lived — binding; reject it (views are non-reassignable
        // non-escaping locals, I8).
        if let Some(arena) = self.arena_alloc_source(&b.init) {
            b.arena_view = true;
            self.record_arena_view(&b.name, arena);
        } else if let Expr::Ident(src, src_span) = &b.init {
            if self.is_arena_view(src) {
                self.report_view_escape(src, "be stored in another binding", *src_span);
            }
        }
        let task_has_view_capture = self.view_capture_tasks.contains(&b.name);
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
                task_has_view_capture,
            },
        );
        self.current_binding_name = prev_binding_name;
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
                task_has_view_capture: false,
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
        let mut fix = format!("declare it first: `{} {} ...`", name, Syntax::SIGIL_BIND_IMMUT);
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

/// D-UNINIT1: a `#Uninit` binding is restricted to plain-data ("POD") types —
/// no heap ownership, no Drop glue — so an uninitialized value can never expose
/// freed/owned state. v1 allows scalars, `Char`, `U8`, and fixed arrays of those.
pub(crate) fn is_pod_uninit_type(ty: &Type) -> bool {
    match ty {
        Type::Int | Type::Float | Type::Bool | Type::Char => true,
        Type::IntN { .. } | Type::Float32 => true,
        Type::FixedList { elem, .. } => is_pod_uninit_type(elem),
        _ => false,
    }
}
