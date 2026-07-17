use crate::AST::{AccessConvention, Expr, ForKind, IndexKind, LValue, Stmt, StrPart, Type};
use crate::Diagnostics::Diagnostic;
use crate::Sema::CheckerCoreLib::{is_swizzleable_math_type, parse_swizzle_member, swizzle_write_overlaps, SwizzleParse};
use crate::Sema::CheckerTaskGroup::TaskGroupCtx;
use crate::Sema::Diagnostics::{
    aliasing_while_mut, collection_changed_in_loop, collection_root_name,
    computed_field_not_settable, expr_root_ident, is_task_type, loop_control_outside,
    type_fix_hint, undefined_loop_label,
};
use crate::Sema::Effects::{grant_handle_escape, unknown_effect};
use crate::Sema::Registration::already_defined;
use crate::Sema::{type_is_copy, Checker, LocalInfo};
use crate::Syntax;
use std::collections::HashSet;
use super::helpers::layout_constraint_fingerprint;
impl<'a> Checker<'a> {
        /// Check two alternative branches with independent move states, then
        /// keep the union (a value moved in either branch counts as gone).
        pub(crate) fn check_stmt(&mut self, stmt: &mut Stmt) {
            if let Stmt::While { span, .. }
            | Stmt::For { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::CountedLoop { span, .. } = stmt
            {
                self.fx_memory_unbounded_control.push(*span);
                for region in &mut self.memory_policy_stack {
                    region.unbounded_control.push(*span);
                }
            }
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
                    self.check_lvalue_change(target, "be assigned");
                    let vt = self.infer(value);
                    if !is_compound {
                        self.reject_borrowed_param_subplace(
                            value,
                            vt.as_ref(),
                            "replace an owned value",
                        );
                        if let Expr::Ident(name, span) = value {
                            let borrowed = self.lookup(name).is_some_and(|info| {
                                !type_is_copy(&info.ty)
                                    && matches!(
                                        info.param_conv,
                                        Some(AccessConvention::Read)
                                            | Some(AccessConvention::Write)
                                    )
                            });
                            if borrowed {
                                self.diags.push(Diagnostic::error(
                                    "E0120",
                                    format!(
                                        "`{name}` was not moved here, so it cannot replace an owned value"
                                    ),
                                    "this function has read access only and does not own the value"
                                        .to_string(),
                                    format!("copy it explicitly with `{}{name}`", Syntax::SIGIL_COPY),
                                    Some(*span),
                                ));
                            }
                        }
                    }
                    self.note_move_if_direct_ident(value);
                    // D-UNINIT-SENTINEL1: a plain `name = …` initializes a `:= uninit`
                    // binding; a compound `name += …` reads it first, so it's a
                    // read-before-write.
                    if let LValue::Local { name, name_span } = &*target {
                        if self.uninit.contains_key(name) {
                            if is_compound {
                                self.diags.push(Diagnostic::error(
                                    "E0420",
                                    format!("`{}` may be read before it is given a value", name),
                                    format!(
                                        "`{}+=` reads `{}` first, but it was declared `:= uninit` and has no value yet",
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
                            if !info.mutable && !self.is_write_view(name) {
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
                                        "mark the parameter `{}: {}{}` if the function should change it",
                                        name,
                                        Syntax::SIGIL_WRITE,
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
                                        Syntax::SIGIL_WRITE
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
                            if let Expr::Ident(name, _) = base.as_ref() {
                                if self.uninit.contains_key(name) && !is_compound {
                                    self.uninit.remove(name);
                                }
                            }
                            self.borrow_ctx = true;
                            let base_ty = self.infer(base);
                            let idx_ty = self.infer(index);
                            match &base_ty {
                                Some(Type::Map { .. }) => *kind = IndexKind::Map,
                                // S76/D-FIXARR1: `buf[i] = v` on a fixed-size `[T#N]` indexes
                                // exactly like a growable `[T]` — same gate as `infer_index`'s
                                // read-side `Type::FixedList` arm. Missing this left `kind` at
                                // its `IndexKind::Unknown` default, which the TIR subset gate
                                // (`stmt_in_subset`) reads as "sema did not resolve it" and
                                // excludes the whole function — an I2 ICE since TIR is the
                                // only codegen path (R7).
                                Some(Type::List(_)) | Some(Type::FixedList { .. }) => {
                                    *kind = IndexKind::List
                                }
                                Some(Type::Apply { name, .. }) if name == "ViewMut" => {
                                    *kind = IndexKind::List
                                }
                                Some(Type::Named(n)) if self.trait_reg.index_types.contains_key(n) => {
                                    *kind = IndexKind::User(n.clone());
                                }
                                // D-MEM1 S6: `pool[id] = v` — generation-checked write.
                                Some(Type::Apply {
                                    name,
                                    args: pool_args,
                                }) if name == "Pool" => {
                                    *kind = IndexKind::Pool;
                                    let is_matching_id = matches!(
                                        &idx_ty,
                                        Some(Type::Apply { name, args: id_args })
                                            if name == "Id" && id_args.first() == pool_args.first()
                                    );
                                    if !is_matching_id {
                                        self.diags.push(Diagnostic::error(
                                            "E0112",
                                            format!(
                                                "`Pool` indexes need a matching `Id<T>`, not {}",
                                                idx_ty
                                                    .as_ref()
                                                    .map(|t| t.show())
                                                    .unwrap_or_else(|| "this".to_string())
                                            ),
                                            "a pool slot is only reached through the `Id<T>` its own `.add()` returned".to_string(),
                                            "index with the `Id<T>` handle from `.add(...)`".to_string(),
                                            Some(index.span()),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                            // D-SOA1: index-WRITE through a columnar list (`xs[i] = …`)
                            // is deferred — v1 supports index-READ, field-read, `len`,
                            // `is_empty`, `push`, and iteration. Reject rather than
                            // miscompile (the columns type has no `IndexMut`).
                            if let Some(Type::List(inner)) = &base_ty {
                                if let Type::Named(elem) = inner.as_ref() {
                                    if self.registry.is_columnar(elem) {
                                        self.diags.push(Diagnostic::error(
                                            "E1108",
                                            format!(
                                                "writing through `[ ]` isn't supported on a columnar list `{}` yet",
                                                Type::List(inner.clone()).show()
                                            ),
                                            "`@Layout(columnar)` lists support reading in v1 (indexing, field access, `len`, `is_empty`, `push`, iteration); index-write is deferred".to_string(),
                                            format!(
                                                "drop `@Layout(columnar)` from `{}` to assign through `[ ]`, or rebuild the list with `push`",
                                                elem
                                            ),
                                            Some(*span),
                                        ));
                                        return;
                                    }
                                }
                            }
                            // Writing through `[ ]` changes the owner: the root
                            // name must be changeable and not under a `for` borrow.
                            let base_is_pool =
                                matches!(&base_ty, Some(Type::Apply { name, .. }) if name == "Pool");
                            let base_is_view_mut =
                                matches!(&base_ty, Some(Type::Apply { name, .. }) if name == "ViewMut");
                            let base_has_write_view = expr_root_ident(base)
                                .is_some_and(|name| self.is_write_view(name));
                            if base_is_pool
                                || base_is_view_mut
                                || matches!(
                                    base_ty,
                                    Some(Type::Map { .. })
                                        | Some(Type::List(_))
                                        | Some(Type::FixedList { .. })
                                )
                            {
                                if let Some(root) = expr_root_ident(base) {
                                    let root = root.to_string();
                                    if self.iter_borrowed.contains(&root) {
                                        self.diags.push(collection_changed_in_loop(&root, *span));
                                    }
                                    if let Some(info) = self.lookup(&root) {
                                        if !base_is_view_mut
                                            && !base_has_write_view
                                            && !info.mutable
                                        {
                                            let (code, why, fix) = if matches!(
                                                info.param_conv,
                                                Some(AccessConvention::Read)
                                            ) {
                                                (
                                                    "E0205",
                                                    "an unmarked parameter gives read access only; assigning into it needs write access (`&`)".to_string(),
                                                    format!(
                                                        "change the parameter to `{}: {}{}`",
                                                        root,
                                                        Syntax::SIGIL_WRITE,
                                                        info.ty.name()
                                                    ),
                                                )
                                            } else {
                                                (
                                                    "E0202",
                                                    "assigning into a collection edits it; the binding must be declared mutable".to_string(),
                                                    format!(
                                                        "declare `{} {} ...`",
                                                        root,
                                                        Syntax::SIGIL_BIND_MUT
                                                    ),
                                                )
                                            };
                                            self.diags.push(Diagnostic::error(
                                                code,
                                                format!(
                                                    "cannot write to `{}` — it does not have edit access (`&`)",
                                                    root
                                                ),
                                                why,
                                                fix,
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
                                ..
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
                            } else if let Some(Type::Apply { name, args }) = base_ty {
                                if name == "ViewMut" {
                                    if let (Some(elem_ty), Some(vt)) = (args.first(), vt) {
                                        if vt != *elem_ty {
                                            self.diags.push(Diagnostic::error(
                                                "E0108",
                                                format!(
                                                    "this view holds {}, not {}",
                                                    elem_ty.show(),
                                                    vt.show()
                                                ),
                                                "every item written through a view must keep the owner's element type".to_string(),
                                                type_fix_hint(elem_ty, &vt),
                                                Some(value.span()),
                                            ));
                                        }
                                    }
                                }
                            } else if let Some(Type::Named(n)) = &base_ty {
                                if let Some((key_ty, value_ty)) = self.trait_reg.index_types.get(n) {
                                    if !self.trait_reg.index_mutable.contains(n) {
                                        self.diags.push(Diagnostic::error(
                                            "E0505",
                                            format!(
                                                "`{}` can be read with `[ ]` but not written — it has no `IndexMut` impl",
                                                n
                                            ),
                                            "bracket assignment needs `impl Type.IndexMut { fn set(&self, k, v) }`"
                                                .to_string(),
                                            format!(
                                                "use `.set(key, value)` instead, or add `impl {n}.IndexMut`"
                                            ),
                                            Some(*span),
                                        ));
                                    }
                                    if let Some(kt) = idx_ty {
                                        if kt != *key_ty {
                                            self.diags.push(Diagnostic::error(
                                                "E0505",
                                                format!(
                                                    "this value indexes with {}, not {}",
                                                    key_ty.show(),
                                                    kt.show()
                                                ),
                                                "the key must match the type's `Index` key".to_string(),
                                                format!("use a {} key here", key_ty.name()),
                                                Some(index.span()),
                                            ));
                                        }
                                    }
                                    if let Some(vt) = vt {
                                        if vt != *value_ty {
                                            self.diags.push(Diagnostic::error(
                                                "E0108",
                                                format!(
                                                    "this value holds {}, not {}",
                                                    value_ty.show(),
                                                    vt.show()
                                                ),
                                                "the stored value must match the type's `Index` value"
                                                    .to_string(),
                                                type_fix_hint(value_ty, &vt),
                                                Some(value.span()),
                                            ));
                                        }
                                    }
                                    if let Some(root) = expr_root_ident(base) {
                                        let root = root.to_string();
                                        if let Some(info) = self.lookup(&root) {
                                            if !info.mutable && !self.is_write_view(&root) {
                                                self.diags.push(Diagnostic::error(
                                                    "E0202",
                                                    format!(
                                                        "cannot write to `{}` — it does not have edit access (`&`)",
                                                        root
                                                    ),
                                                    "assigning through `[ ]` edits the value; the binding must be declared mutable"
                                                        .to_string(),
                                                    format!(
                                                        "declare `{} {} ...`",
                                                        root,
                                                        Syntax::SIGIL_BIND_MUT
                                                    ),
                                                    Some(*span),
                                                ));
                                            }
                                        }
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
                        // local. A non-`mut` `self` (shared-read receiver) or an immutable/shared
                        // binding is E0205, pointed at the assignment, with a "write the
                        // receiver as `mut self`" / "make it changeable" fix (owner Q1).
                        LValue::Field { base, field, span } => {
                            self.borrow_ctx = true;
                            // D-SOA1: `xs[i].field = …` where `xs` is a columnar list
                            // would write into a throwaway gathered value, not the
                            // column — reject (field-WRITE on a columnar element is
                            // deferred; reads are supported). Detected off the index
                            // base's root binding type (the common `local[i].f` form).
                            if let Expr::Index { base: ib, .. } = base.as_ref() {
                                let columnar_elem = expr_root_ident(ib)
                                    .and_then(|root| self.lookup(root))
                                    .and_then(|info| match &info.ty {
                                        Type::List(inner) => match inner.as_ref() {
                                            Type::Named(elem) if self.registry.is_columnar(elem) => {
                                                Some(elem.clone())
                                            }
                                            _ => None,
                                        },
                                        _ => None,
                                    });
                                if let Some(elem) = columnar_elem {
                                    self.diags.push(Diagnostic::error(
                                        "E1108",
                                        format!(
                                            "writing `{}[i].{}` isn't supported on a columnar list yet",
                                            expr_root_ident(ib).unwrap_or("xs"),
                                            field
                                        ),
                                        "`@Layout(columnar)` lists support reading a field (`xs[i].f`) in v1; writing one is deferred".to_string(),
                                        format!(
                                            "drop `@Layout(columnar)` from `{}` to write fields in place, or rebuild the element with `push`",
                                            elem
                                        ),
                                        Some(*span),
                                    ));
                                    return;
                                }
                            }
                            let base_ty = self.infer(base);
                            // D-FIELDPOL1: `s.computed_field = v` — a computed field is
                            // never stored, so a plain assignment has nothing to write.
                            if let Some(bt) = &base_ty {
                                if self.field_is_computed(bt, field) {
                                    self.diags.push(computed_field_not_settable(field, *span));
                                    return;
                                }
                            }
                            // D-SWIZZLE1: overlapping write swizzles (`v.xx = …`) are rejected.
                            if let Some(Type::Named(type_name)) = &base_ty {
                                if is_swizzleable_math_type(type_name)
                                    && !self.registry.contains(type_name)
                                {
                                    if let SwizzleParse::Ok(lanes) =
                                        parse_swizzle_member(field, type_name)
                                    {
                                        if swizzle_write_overlaps(&lanes) {
                                            self.diags.push(Diagnostic::error(
                                                "E3111",
                                                format!(
                                                    "write swizzle `{}` repeats a lane on `{}`",
                                                    field, type_name
                                                ),
                                                "each lane may be written at most once — overlapping patterns like `v.xx` have no single meaning"
                                                    .to_string(),
                                                format!(
                                                    "assign each lane once, e.g. `{}.xy = …` instead of `{}.{} = …`",
                                                    expr_root_ident(base).unwrap_or("v"),
                                                    expr_root_ident(base).unwrap_or("v"),
                                                    field
                                                ),
                                                Some(*span),
                                            ));
                                            return;
                                        }
                                    }
                                }
                            }
                            // Validate the field exists and get its type (emits E0302 on a
                            // bad field). The value's type must match the field type (E0108).
                            if let Some(bt) = &base_ty {
                                if let Some(ft) = self.field_type(bt, field, *span) {
                                    if self.type_contains_view_boundary(&ft) {
                                        self.diags.push(Diagnostic::error(
                                            "E2305",
                                            format!("view field `{field}` cannot be assigned a new source"),
                                            "a stored view field has one stabilized owner relationship; overwriting it would erase or change that public provenance"
                                                .to_string(),
                                            "construct a new value with the new view, or keep the original field source unchanged"
                                                .to_string(),
                                            Some(*span),
                                        ));
                                        return;
                                    }
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
                                                "a field keeps one type for its whole life".to_string(),
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
                                    if !info.mutable && !self.is_write_view(&root) {
                                        let is_self = root == Syntax::KW_SELF;
                                        let what = if is_self {
                                            format!(
                                                "cannot edit `{}` — `{}` has read access only; write access (`&`) is required",
                                                field,
                                                Syntax::KW_SELF
                                            )
                                        } else {
                                            format!("cannot edit `{}` — `{}` does not have write access (`&`)", field, root)
                                        };
                                        let fix = if is_self {
                                            format!(
                                                "write the receiver as `{}{}` to grant write access",
                                                Syntax::SIGIL_WRITE,
                                                Syntax::KW_SELF
                                            )
                                        } else if info.param_conv.is_some() {
                                            format!(
                                                "mark the parameter `{}: {}{}` to grant write access",
                                                root,
                                                Syntax::SIGIL_WRITE,
                                                info.ty.name()
                                            )
                                        } else {
                                            format!(
                                                "declare it with `{} {} ...` to give it write access",
                                                root,
                                                Syntax::SIGIL_BIND_MUT
                                            )
                                        };
                                        self.diags.push(Diagnostic::error(
                                            "E0205",
                                            what,
                                            "editing a field requires write access (`&`) on the owning place".to_string(),
                                            fix,
                                            Some(*span),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
                Stmt::Expr(_) => {
                    if self.rewrite_anonymous_taskgroup_spawn(stmt) {
                        if let Stmt::Val(b) = stmt {
                            self.check_binding(b);
                        }
                        return;
                    }
                    let Stmt::Expr(expr) = stmt else {
                        return;
                    };
                    if let Expr::Call(call) = expr {
                        if call.name == Syntax::INTERNAL_DEFER_CLOSE {
                            if let Some(arg) = call.args.first_mut() {
                                self.infer_fallible_stmt(&mut arg.expr);
                            }
                            return;
                        }
                    }
                    // D-IGNORERET2=A: `.drop("reason")` is the blessed explicit-discard
                    // terminal. When recognized, infer the *receiver* (for side effects),
                    // validate the reason is a non-empty string literal, and suppress E0402.
                    if let Expr::MethodCall {
                        receiver,
                        method,
                        method_span,
                        args,
                        ..
                    } = expr
                    {
                        if method == Syntax::METHOD_DROP {
                            let recv_ty = self.infer_fallible_stmt(receiver);
                            // Validate reason argument — must be a non-empty string literal.
                            match args.first() {
                                Some(a) => match &a.expr {
                                    Expr::Str(parts, _)
                                        if parts.len() == 1
                                            && matches!(&parts[0], StrPart::Lit(s) if s.is_empty()) =>
                                    {
                                        self.diags.push(Diagnostic::error(
                                            "E0407",
                                            "`.drop()` reason must not be empty".to_string(),
                                            "the reason documents why this result is intentionally discarded".to_string(),
                                            "write `.drop(\"why this is fine to ignore\")` with a real explanation".to_string(),
                                            Some(*method_span),
                                        ));
                                    }
                                    Expr::Str(parts, _)
                                        if parts.iter().all(|p| matches!(p, StrPart::Lit(_))) =>
                                    {
                                        // Non-empty plain string literal — valid.
                                    }
                                    _ => {
                                        self.diags.push(Diagnostic::error(
                                            "E0407",
                                            "`.drop()` requires a string literal reason".to_string(),
                                            "the reason must be a compile-time string, not a variable"
                                                .to_string(),
                                            "write `.drop(\"why this is fine to ignore\")`".to_string(),
                                            Some(*method_span),
                                        ));
                                    }
                                },
                                None => {
                                    self.diags.push(Diagnostic::error(
                                        "E0407",
                                        "`.drop()` requires a reason argument".to_string(),
                                        "the reason documents why this result is intentionally discarded".to_string(),
                                        "write `.drop(\"why this is fine to ignore\")`".to_string(),
                                        Some(*method_span),
                                    ));
                                }
                            }
                            // E0402 is suppressed — that is the entire point of `.drop()`.
                            // Task drop is still warned (L1101) because `.drop()` on a task
                            // doesn't actually join it; use `.detach()` for fire-and-forget.
                            if let Some(ty) = recv_ty {
                                if is_task_type(&ty) {
                                    self.diags.push(Diagnostic::lint(
                                        "L1101",
                                        "a spawned task is dropped without `.join()`".to_string(),
                                        "`.drop()` discards the task handle — the task may outlive the function".to_string(),
                                        "use `.detach()` for fire-and-forget, or `.join()` to wait".to_string(),
                                        Some(expr.span()),
                                    ));
                                }
                            }
                            // Short-circuit: don't fall through to the generic E0402 path.
                            return;
                        }
                    }
                    if let Some(ty) = self.infer_fallible_stmt(expr) {
                        if ty.is_fallible() && !self.suppress_must_use {
                            self.diags.push(Diagnostic::error(
                                "E0402",
                                "this call can fail and nothing checks it".to_string(),
                                "a fallible result can't be ignored — handle it or say failure is impossible"
                                    .to_string(),
                                format!(
                                    "use `{}`, `{}`, `{} ...`, or `.drop(\"reason\")` to intentionally discard",
                                    Syntax::OP_TRY_SUFFIX,
                                    Syntax::OP_FALLBACK,
                                    Syntax::BUILTIN_PANIC
                                ),
                                Some(expr.span()),
                            ));
                        } else if !self.suppress_must_use {
                            self.check_ignored_must_use(expr, &ty, expr.span());
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
                    // D-ENC-DYN1=A+: the declared return type may be a `Data` alias
                    // (`Json`/`Toml`/…); canonicalize it so it unifies with the returned value.
                    let resolved_ret = self.ret.clone().map(|t| self.resolve_type(t));
                    // D-STREAMYIELD1: a generator (`-> Stream<T>`) yields values; `return`
                    // only ever ends the stream early — bare `return;` is fine, `return
                    // value;` is E0806 (a generator body yields, it doesn't return a value).
                    if let Some(Type::Apply { name, args }) = &resolved_ret {
                        if name == Syntax::TYPE_STREAM && args.len() == 1 {
                            if let Some(e) = expr {
                                self.infer(e);
                                self.diags.push(Diagnostic::error(
                                    "E0806",
                                    format!("`{}` yields values, so `return` can't carry one", self.fn_name),
                                    "a generator body produces values with `yield`; `return` only ends the stream early".to_string(),
                                    "write `yield ...;` to hand back a value, or a bare `return;` to end the stream".to_string(),
                                    Some(e.span()),
                                ));
                            }
                            return;
                        }
                    }
                    match (&mut *expr, resolved_ret) {
                        (Some(e), Some(rt)) => {
                            let string_view_return = matches!(
                                &rt,
                                Type::Apply { name, args }
                                    if name == "View"
                                        && matches!(args.as_slice(), [Type::Named(inner)] if inner == "str")
                            );
                            // D-SHAPE-PLACE1=A: a bare maximal place is a read
                            // window. At a named `View<T>` return boundary, make
                            // that local acquisition explicit in the AST before
                            // inference; E2305 then checks today's provenance gate.
                            if matches!(&rt, Type::Apply { name, .. } if name == "View")
                                && !string_view_return
                                && !matches!(e, Expr::Copy(..) | Expr::Place(..))
                                && self.place_from_expr(e).is_some()
                            {
                                let span = e.span();
                                let inner = std::mem::replace(e, Expr::Absent(span));
                                *e = Expr::Place(
                                    Box::new(inner),
                                    crate::AST::PlaceAccess::Read,
                                    span,
                                );
                            }
                            let saved_expected = self.expected_type.clone();
                            self.expected_type = Some(rt.clone());
                            // Spawned task returns are checked separately by E1102.
                            self.borrow_ctx = self.is_task_spawn;
                            let saved_string_view_read = self.allow_string_view_read;
                            if string_view_return {
                                self.allow_string_view_read = true;
                            }
                            let et = self.infer(e);
                            self.allow_string_view_read = saved_string_view_read;
                            self.expected_type = saved_expected;
                            self.check_aggregate_view_return(e);
                            // D-ALLOC2: E0631 — returning an arena `view` would let
                            // it outlive the arena (the arena drops at scope end).
                            if let Expr::Ident(n, nspan) = &*e {
                                if self.is_arena_view(n) || self.is_fixed_backing_view(n) {
                                    self.report_view_escape(n, "be returned", *nspan);
                                }
                            }
                            // D-DYNARRAY1: E2305 — returning a `View<T>` whose owner
                            // list is local to this function would outlive it. Two
                            // shapes: an already-bound view name (`return window`),
                            // and a fresh range place made right in the
                            // `return` (`return incidents[0..2]`) — the latter
                            // needs `view_call_source` directly.
                            if string_view_return {
                                if let Expr::Ident(n, nspan) = &*e {
                                    if self.is_string_view(n) {
                                        self.check_named_string_view_binding_return(n, *nspan);
                                    } else {
                                        self.report_string_view_boundary(e.span());
                                    }
                                } else {
                                    self.report_string_view_boundary(e.span());
                                }
                            } else if matches!(&rt, Type::Apply { name, .. } if matches!(name.as_str(), "View" | "ViewMut")) {
                                if let Expr::Ident(n, nspan) = &*e {
                                    if self.is_list_view(n) {
                                        self.check_named_view_binding_return(n, *nspan);
                                    } else {
                                        self.report_view_return_boundary(e.span());
                                    }
                                } else if let Some((_, place, _, access)) =
                                    self.view_call_sources(e).into_iter().find(|(path, ..)| path.is_empty())
                                {
                                    self.check_named_view_return(&place, access, Vec::new(), e.span());
                                } else {
                                    self.report_view_return_boundary(e.span());
                                }
                            }
                            // D-MEM1 stage S5: no dedicated "returning a string
                            // view" check here — the general E2307 check on the
                            // `Expr::Ident` read (`self.infer(e)` above already
                            // ran it) already caught `return d` for a live view;
                            // a second check here would just double-report the
                            // same span (unlike `View<T>`, which needs its OWN
                            // check since its escape is otherwise silent — a
                            // string view's bare-`&str` read is never silent).
                            // Returning a borrowed parameter would move out of a
                            // borrow in the generated Rust (I2) — require a copy.
                            self.reject_borrowed_param_subplace(
                                e,
                                et.as_ref(),
                                "be returned as an owned value",
                            );
                            if let Expr::Ident(n, nspan) = &*e {
                                if let Some(info) = self.lookup(n) {
                                    if !info.ty.is_scalar()
                                        && matches!(
                                            info.param_conv,
                                            Some(AccessConvention::Read)
                                                | Some(AccessConvention::Write)
                                        )
                                    {
                                        self.diags.push(Diagnostic::error(
                                            "E0120",
                                            format!(
                                                "`{}` was not moved here, so it cannot be returned as-is",
                                                n
                                            ),
                                            "this function has read access only and does not own the value"
                                                .to_string(),
                                            format!(
                                                "return a copy: `return {}{};` — or take ownership with `{}: {}{}`. \
                                                 There's no borrow-return in v1 — to share the value without a full \
                                                 copy, store an owned field, or reach for `Shared<T>`/`Id<T>` \
                                                 once a real program needs shared ownership",
                                                Syntax::SIGIL_COPY,
                                                n,
                                                n,
                                                Syntax::SIGIL_MOVE,
                                                info.ty.name()
                                            ),
                                            Some(*nspan),
                                        ));
                                    }
                                }
                            }
                            if !string_view_return {
                                self.note_move_if_direct_ident(e);
                            }
                            if let Some(et) = et {
                                let http_handler_lambda = matches!(
                                    (&rt, &et),
                                    (Type::Named(name), Type::Fn { params, ret: Some(ret), .. })
                                        if name == "HttpHandler"
                                            && params == &vec![Type::Named("HttpSrvReq".to_string())]
                                            && ret.as_ref() == &Type::Named("HttpSrvResp".to_string())
                                );
                                let string_view_compatible = string_view_return && et == Type::String;
                                if et != rt && !http_handler_lambda && !string_view_compatible {
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
                // D-STREAMYIELD1: `yield expr` — legal only in a function whose return
                // type is `Stream<T>` (E0805 otherwise); `expr: T` (E0807 on mismatch).
                Stmt::Yield(e, span) => {
                    let resolved_ret = self.ret.clone().map(|t| self.resolve_type(t));
                    let elem_ty = match &resolved_ret {
                        Some(Type::Apply { name, args })
                            if name == Syntax::TYPE_STREAM && args.len() == 1 =>
                        {
                            Some(args[0].clone())
                        }
                        _ => None,
                    };
                    let Some(elem_ty) = elem_ty else {
                        self.diags.push(Diagnostic::error(
                            "E0805",
                            format!("`{}` outside a generator", Syntax::KW_YIELD),
                            "`yield` hands a value to a `Stream<T>` consumer — only a function declared `-> Stream<T>` may use it".to_string(),
                            format!("declare `-> {}<T>` on this function, or remove the `{}`", Syntax::TYPE_STREAM, Syntax::KW_YIELD),
                            Some(*span),
                        ));
                        self.infer(e);
                        return;
                    };
                    let saved_expected = self.expected_type.clone();
                    self.expected_type = Some(elem_ty.clone());
                    let got = self.infer(e);
                    self.expected_type = saved_expected;
                    if let Some(got) = got {
                        let got = self.resolve_type(got);
                        if got != elem_ty {
                            self.diags.push(Diagnostic::error(
                                "E0807",
                                format!(
                                    "this yields {}, but the stream is `{}<{}>`",
                                    got.show(),
                                    Syntax::TYPE_STREAM,
                                    elem_ty.show()
                                ),
                                "every `yield` in a generator must hand back the stream's element type"
                                    .to_string(),
                                type_fix_hint(&elem_ty, &got),
                                Some(e.span()),
                            ));
                        }
                    }
                }
                Stmt::If(ifs) => self.check_if(ifs),
                // D-CTMARKER1 (ratified 2026-06-25, piece 2): build-time execution block.
                Stmt::ComptimeBlock { .. } => self.check_comptime_block(stmt),
                // D-WHEN1/D-WHEN2 (ratified 2026-06-19): compile-time conditional.
                Stmt::ComptimeIf { .. } => self.check_comptime_if(stmt),
                Stmt::While {
                    cond,
                    body,
                    span: _,
                    label,
                } => {
                    let memory_multiplier = self.memory_control_multiplier;
                    self.memory_control_multiplier = None;
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
                    self.memory_control_multiplier = memory_multiplier;
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
                    let memory_multiplier = self.memory_control_multiplier;
                    let loop_multiplier = memory_multiplier.and_then(|outer| {
                        statically_bounded_for_iterations(kind)
                            .and_then(|iterations| outer.checked_mul(iterations))
                    });
                    self.memory_control_multiplier = memory_multiplier;
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
                                if let Expr::Int(n, sp, _, _) = step {
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
                                    def_span: vs,
                                    ty: Type::Int,
                                    mutable: false,
                                    param_conv: None,
                                    decl_loop_depth: self.loop_depth,
                                    sendable: true,
                                    task_lint_span: None,
                                    single_use_span: None,
                                },
                            );
                            self.memory_control_multiplier = loop_multiplier;
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
                                Some(Type::Map { key, value, .. }) => {
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
                                // D-PROCESS1=A: `loop line in child.stdout.lines()` /
                                // `child.stderr.lines()` — streaming subprocess output.
                                Some(Type::Named(n)) if n == "ProcessLines" => {
                                    self.declare_loop_var(var.clone(), *var_span, &Type::String);
                                }
                                Some(Type::Named(n))
                                    if self.trait_reg.iterable_items.contains_key(n) =>
                                {
                                    if var2.is_some() {
                                        self.diags.push(Diagnostic::error(
                                            "E0109",
                                            format!(
                                                "`for x in` on `{}` needs one loop name, not two",
                                                n
                                            ),
                                            "a custom iterable yields one item per step".to_string(),
                                            format!("write `for item in {n}`").to_string(),
                                            Some(collection.span()),
                                        ));
                                    } else {
                                        let item_ty = self
                                            .trait_reg
                                            .iterable_items
                                            .get(n)
                                            .cloned()
                                            .unwrap_or(Type::Int);
                                        self.declare_loop_var(var.clone(), *var_span, &item_ty);
                                    }
                                }
                                // D-STREAMYIELD1: `loop x in a_stream { }` — pull one value
                                // at a time from a generator's `Stream<T>`, blocking until
                                // the producer yields (or ends the stream by returning).
                                Some(Type::Apply { name, args })
                                    if name == crate::Syntax::TYPE_STREAM && args.len() == 1 =>
                                {
                                    self.declare_loop_var(var.clone(), *var_span, &args[0]);
                                }
                                // D-DYNARRAY1: `loop x in window` — a `View<T>` iterates its
                                // elements read-only, same shape as `loop x in a_list`.
                                Some(Type::Apply { name, args })
                                    if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 =>
                                {
                                    self.declare_loop_var(var.clone(), *var_span, &args[0]);
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
                            self.memory_control_multiplier = loop_multiplier;
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
                    self.memory_control_multiplier = memory_multiplier;
                }
                Stmt::Switch {
                    subject,
                    arms,
                    else_body,
                    span,
                }
                | Stmt::ComptimeSwitch {
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
                Stmt::CountedLoop {
                    init,
                    cond,
                    body,
                    step,
                    label,
                    ..
                } => {
                    let memory_multiplier = self.memory_control_multiplier;
                    self.memory_control_multiplier = None;
                    if let Some((n, _)) = label {
                        self.loop_labels.push(n.clone());
                    }
                    self.check_binding(init);
                    self.require_bool(cond, "a counted loop condition");
                    self.loop_depth += 1;
                    let saved_u = self.uninit.clone();
                    self.check_block(body, true);
                    self.check_stmt(step.as_mut());
                    self.uninit = saved_u;
                    self.loop_depth -= 1;
                    if label.is_some() {
                        self.loop_labels.pop();
                    }
                    self.memory_control_multiplier = memory_multiplier;
                }
                Stmt::Loop {
                    body: inner, label, ..
                } => {
                    let memory_multiplier = self.memory_control_multiplier;
                    self.memory_control_multiplier = None;
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
                    self.memory_control_multiplier = memory_multiplier;
                }
                Stmt::Unsafe { audit, body, span } => {
                    let _ = (audit, span); // L3101 is policy-aware in UnsafeObligations.
                    let prev = self.in_unsafe;
                    self.in_unsafe = true;
                    self.check_block(body, true);
                    self.in_unsafe = prev;
                }
                // D-CTEFFECT1: `@Impure("reason") { … }` — the Tier-2 comptime effect
                // gate. At runtime (which is what sema is checking here), this block is
                // semantically a plain block: it has no runtime significance. The gate is
                // enforced only inside the comptime interpreter. L3102 fires when no
                // reason was given (matches L3101 pattern for @Unsafe).
                Stmt::Impure { reason, body, span } => {
                    if reason.is_none() {
                        self.diags.push(Diagnostic::lint(
                            "L3102",
                            "this `@Impure` block has no reason".to_string(),
                            "every comptime effect gate records why ambient I/O is needed".to_string(),
                            "add the reason: `@Impure(\"reading build config\") { … }`".to_string(),
                            Some(*span),
                        ));
                    }
                    self.ct_impure_depth += 1;
                    self.check_block(body, true);
                    self.ct_impure_depth -= 1;
                }
                // D-SHIELDNAME1=A: `@Shield { … }` — a cancellation-shield region.
                // Legal anywhere ordinary statements are; a no-op outside a task.
                // Semantically a plain block: check the body, no effects, no gate.
                Stmt::Shield { body, .. } => {
                    self.check_block(body, true);
                }
                // D-REACTCORE1: `@Reactive { … }` — a reactive effect scope.
                Stmt::Reactive { body, span } => {
                    if self.in_comptime {
                        self.diags.push(Diagnostic::error(
                            "E2914",
                            "`@Reactive` can't run at comptime".to_string(),
                            "reactive effects subscribe to runtime signals and re-run when they change (D-REACTCORE1)"
                                .to_string(),
                            "move `@Reactive { … }` out of the `comptime` block".to_string(),
                            Some(*span),
                        ));
                    }
                    self.check_block(body, true);
                }
                Stmt::Off { body, .. } => {
                    let moved = self.moved.clone();
                    let uninit = self.uninit.clone();
                    let fx_direct = self.fx_direct.clone();
                    let fx_edges = self.fx_edges.clone();
                    let fx_maximal = self.fx_maximal;
                    let region_stack = self.region_stack.clone();
                    let fx_regions = self.fx_regions.clone();
                    let fx_callback_obligations = self.fx_callback_obligations.clone();
                    let fx_memory_events = self.fx_memory_events.clone();
                    let fx_memory_open = self.fx_memory_open.clone();
                    let memory_policy_stack = self.memory_policy_stack.clone();
                    let fx_memory_regions = self.fx_memory_regions.clone();
                    let fx_memory_unbounded_control = self.fx_memory_unbounded_control.clone();
                    let fx_memory_calls = self.fx_memory_calls.clone();
                    let memory_control_multiplier = self.memory_control_multiplier;
                    let prev_suppress = self.suppress_must_use;
                    self.suppress_must_use = true;
                    self.push_scope();
                    for stmt in body {
                        self.check_stmt(stmt);
                    }
                    self.drop_scope_no_obligation_checks();
                    self.suppress_must_use = prev_suppress;
                    self.moved = moved;
                    self.uninit = uninit;
                    self.fx_direct = fx_direct;
                    self.fx_edges = fx_edges;
                    self.fx_maximal = fx_maximal;
                    self.region_stack = region_stack;
                    self.fx_regions = fx_regions;
                    self.fx_callback_obligations = fx_callback_obligations;
                    self.fx_memory_events = fx_memory_events;
                    self.fx_memory_open = fx_memory_open;
                    self.memory_policy_stack = memory_policy_stack;
                    self.fx_memory_regions = fx_memory_regions;
                    self.fx_memory_unbounded_control = fx_memory_unbounded_control;
                    self.fx_memory_calls = fx_memory_calls;
                    self.memory_control_multiplier = memory_control_multiplier;
                }
                Stmt::DebugOnly { body, .. } => {
                    self.check_block(body, true);
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
                Stmt::Policy { declarations, body, span } => {
                    self.enter_memory_policy_region(declarations.clone(), *span);
                    self.check_block(body, true);
                    self.exit_memory_policy_region();
                }
                // D-TASKSCOPE1=A / D-NURSERY1=A: `taskgroup g { … }` — structured task scope.
                Stmt::TaskGroup {
                    name,
                    name_span,
                    body,
                    ..
                } => {
                    self.push_scope();
                    self.declare(
                        name,
                        *name_span,
                        LocalInfo {
                            def_span: *name_span,
                            ty: Type::Named(Syntax::TYPE_TASKGROUP.to_string()),
                            mutable: false,
                            param_conv: None,
                            decl_loop_depth: self.loop_depth,
                            sendable: true,
                            task_lint_span: None,
                            single_use_span: None,
                        },
                    );
                    self.taskgroup_stack.push(TaskGroupCtx::new(name.clone()));
                    self.check_block(body, false);
                    let join_start = body.len();
                    self.append_taskgroup_auto_joins(body);
                    for s in &mut body[join_start..] {
                        self.check_stmt(s);
                    }
                    self.taskgroup_stack.pop();
                    self.pop_scope();
                }
                // D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }` — a
                // Cassowary-style constraint block. Unlike `region`/`taskgroup`,
                // `name` is declared in the CURRENT scope (not pushed/popped
                // around it) so the handle outlives the block — later code reads
                // solved values (`NAME.value(v)`) or calls `NAME.suggest(...)`.
                // The parser already desugared every `box.anchor` read into a
                // `NAME.h(box, anchor)`/`NAME.v(box, anchor)` call, so every line
                // is an ordinary expression checked by the general GATE-1/GATE-2
                // machinery (`infer_binary`'s layout block, `layout_method_return`)
                // — the only layout-specific rule left to enforce here is that
                // each line's RESULT is a `Constraint` (E2933 otherwise).
                Stmt::Layout {
                    name,
                    name_span,
                    body,
                    ..
                } => {
                    self.declare(
                        name,
                        *name_span,
                        LocalInfo {
                            def_span: *name_span,
                            ty: Type::Named(Syntax::LAYOUT_HANDLE_TYPE.to_string()),
                            mutable: false,
                            param_conv: None,
                            decl_loop_depth: self.loop_depth,
                            sendable: true,
                            task_lint_span: None,
                            single_use_span: None,
                        },
                    );
                    self.push_scope();
                    // D-LAYOUT1: E2934 (lint) — a constraint line that is a
                    // byte-for-byte structural duplicate of an earlier one in the
                    // SAME block is almost always a copy-paste mistake (a real,
                    // if narrow, notion of "redundant": exact duplicates only —
                    // proving general LP redundancy, i.e. "implied by the
                    // others", is a much larger problem than this lint needs).
                    let mut seen_constraints: HashSet<String> = HashSet::new();
                    for stmt in body.iter_mut() {
                        if let Stmt::Expr(_) = stmt {
                            let Stmt::Expr(e) = stmt else { unreachable!() };
                            let fp = layout_constraint_fingerprint(e);
                            let t = self.infer(e);
                            let is_constraint = matches!(&t, Some(Type::Named(n)) if n == Syntax::LAYOUT_CONSTRAINT_TYPE);
                            if !is_constraint && t.is_some() {
                                self.diags.push(Diagnostic::error(
                                    "E2933",
                                    format!(
                                        "this line inside `{} {}` doesn't produce a constraint (found `{}`)",
                                        Syntax::KW_LAYOUT,
                                        name,
                                        t.as_ref().map(|ty| ty.name()).unwrap_or_default()
                                    ),
                                    "every line directly inside a `layout { … }` block must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`)".to_string(),
                                    "write a comparison, e.g. `label.width >= 80.0`, or capture it: `c :: label.width >= 80.0`".to_string(),
                                    Some(e.span()),
                                ));
                            } else if is_constraint && !seen_constraints.insert(fp) {
                                self.diags.push(Diagnostic::lint(
                                    "E2934",
                                    "this constraint repeats one already written in this `layout` block".to_string(),
                                    "an exact duplicate constraint doesn't tighten the layout — it's almost always a copy-paste leftover".to_string(),
                                    "remove the duplicate line, or change it if a different constraint was meant".to_string(),
                                    Some(e.span()),
                                ));
                            }
                        } else if let Stmt::Val(_) = stmt {
                            let fp = if let Stmt::Val(b) = stmt {
                                layout_constraint_fingerprint(&b.init)
                            } else {
                                unreachable!()
                            };
                            self.check_stmt(stmt);
                            if let Stmt::Val(b) = stmt {
                                let bname = b.name.clone();
                                let name_span = b.name_span;
                                let is_constraint = self
                                    .lookup(&bname)
                                    .map(|info| {
                                        matches!(&info.ty, Type::Named(n) if n == Syntax::LAYOUT_CONSTRAINT_TYPE)
                                    })
                                    .unwrap_or(false);
                                if !is_constraint {
                                    self.diags.push(Diagnostic::error(
                                        "E2933",
                                        format!(
                                            "this binding inside `{} {}` doesn't capture a constraint",
                                            Syntax::KW_LAYOUT,
                                            name
                                        ),
                                        "every line directly inside a `layout { … }` block must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`), optionally captured with `::`".to_string(),
                                        "bind a comparison: `c :: label.width >= 80.0`".to_string(),
                                        Some(name_span),
                                    ));
                                } else if !seen_constraints.insert(fp) {
                                    self.diags.push(Diagnostic::lint(
                                        "E2934",
                                        "this constraint repeats one already written in this `layout` block".to_string(),
                                        "an exact duplicate constraint doesn't tighten the layout — it's almost always a copy-paste leftover".to_string(),
                                        "remove the duplicate line, or change it if a different constraint was meant".to_string(),
                                        Some(name_span),
                                    ));
                                }
                            }
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E2933",
                                format!(
                                    "only constraint lines belong directly inside a `{} {}` block",
                                    Syntax::KW_LAYOUT,
                                    name
                                ),
                                "every line directly inside a `layout { … }` block must be a `>=`/`<=`/`==` comparison of layout values (a `Constraint`), optionally captured with `::`".to_string(),
                                "write a comparison, e.g. `label.width >= 80.0`".to_string(),
                                Some(stmt.span()),
                            ));
                        }
                    }
                    self.pop_scope();
                }
                // D-EFF1 / D-QUAL1: a `@Caps(Net, Db) { … }` effect-restriction
                // region. Validate the cap names (E0119), open an accumulator so the
                // effects reached inside are tallied, check the body, then seal the
                // region for the post-pass E0741 subset check. A lexical scope.
                Stmt::Caps {
                    caps,
                    caps_span,
                    body,
                    ..
                } => {
                    let mut cap_set = crate::Sema::EffectSet::new();
                    let mut bad = false;
                    for (name, span) in caps.iter() {
                        match crate::Sema::parse_effect_name(name) {
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
                        grant: false,
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
                            grant: acc.grant,
                        });
                    }
                }
                // D-SCAP1 (ratified 2026-06-21, opt A): a `#grant(Fs) { caps -> … }`
                // scoped-capability grant region — the dual of `@Caps`. Validate the
                // granted effect names (E0119), bind the first-class capability handle
                // `caps` in a fresh scope (revoked at scope end, RAII), open a grant
                // accumulator so effects reached inside are tallied against the grant
                // (an effect with no backing capability is E0712 in the post-pass),
                // check the body, then enforce that the handle does not escape (E0711).
                // A lexical scope; erased in codegen (I3).
                Stmt::Grant {
                    caps,
                    caps_span,
                    binding,
                    binding_span,
                    body,
                    ..
                } => {
                    let mut cap_set = crate::Sema::EffectSet::new();
                    let mut bad = false;
                    for (name, span) in caps.iter() {
                        match crate::Sema::parse_effect_name(name) {
                            Some(e) => {
                                cap_set.insert(e);
                            }
                            None => {
                                self.diags.push(unknown_effect(name, *span));
                                bad = true;
                            }
                        }
                    }
                    self.push_scope();
                    self.declare_loop_var(
                        binding.clone(),
                        *binding_span,
                        &Type::Named(crate::Syntax::CAP_HANDLE_TYPE.to_string()),
                    );
                    self.region_stack.push(crate::Sema::RegionAccum {
                        caps: cap_set,
                        caps_span: *caps_span,
                        direct: crate::Sema::EffectSet::new(),
                        edges: std::collections::BTreeSet::new(),
                        maximal: false,
                        grant: true,
                    });
                    self.check_block(body, true);
                    let acc = self.region_stack.pop().expect("pushed above");
                    // E0711: the capability handle may not outlive the grant — it is
                    // revoked at scope end. Flag a return/store/share that lets it
                    // escape. (Uses outside the block already fail as unknown-name,
                    // since the handle is scoped to this block.)
                    if let Some(escape_span) = grant_handle_escape(body, binding) {
                        self.diags.push(crate::Sema::e0711(binding, escape_span));
                    }
                    self.pop_scope();
                    // Skip the E0712 subset check when a grant name was invalid (the
                    // grant set is incomplete) — E0119 is the real problem to fix.
                    if !bad {
                        self.fx_regions.push(crate::Sema::RegionSummary {
                            caps: acc.caps,
                            direct: acc.direct,
                            edges: acc.edges,
                            maximal: acc.maximal,
                            caps_span: acc.caps_span,
                            grant: acc.grant,
                        });
                    }
                }
                // D-CTX1 (ratified 2026-06-22, G2): `@Context(field: value) { … }`.
                // Type-check each field value: `allocator` must be an allocator
                // handle type; `deadline` must be an Int epoch-ms instant; `logger`
                // is currently unconstrained. E0762 on mismatch.
                // Q1 = A2: explicit allocator args at call sites override the
                // ambient — no static binding done here, only type validation and
                // block body checking. Q2 = Cβ: restore is per-block (RAII guard).
                // D-TERM1 (ratified 2026-06-22): `live { … }` — terminal direct-input
                // block. No type-checking beyond the body; the block is impure (IO
                // effect), so it is rejected inside `@Pure fn` (same rule as `io.input`).
                // `use core.term` is NOT required to write a `live` block — the block
                // is its own syntactic gate. `term.read_key()` does need the import.
                // E3301: freestanding builds have no terminal device.
                Stmt::Live { body, span } => {
                    if self.in_pure {
                        self.diags.push(crate::Sema::e3401(
                            &self.fn_name.clone(),
                            "@Live { … }",
                            &[],
                            *span,
                        ));
                    }
                    if self.freestanding {
                        self.diags.push(crate::Sema::e3301(
                            "@Live { … }",
                            "Terminal I/O requires an OS terminal device. Build without `--freestanding`.",
                            *span,
                        ));
                    }
                    self.check_block(body, true);
                }
                // D-DOTSCOPE1: a scope-member statement (`.setup`/`.expect_fail`/
                // `.timeout`/`.skip` inside a `@Test` block). Member legality, args,
                // position, and nesting are validated by the `ScopeMembers` pass; here
                // the checker only type-checks the region body's ordinary statements.
                // The member args (`.timeout(500ms)`, `.skip("why")`) are intentionally
                // NOT inferred — a bare duration literal has no `@UnitFamily` in scope.
                // `.setup` is init sugar: its bindings leak into the test scope (no new
                // scope), so the rest of the body can use them. Every other member is
                // its own region (a closure / block / dead branch in codegen), so its
                // bindings are scoped — referencing them later is a normal unknown-name
                // error, never reaching codegen.
                Stmt::ScopeMember { name, body, .. } => {
                    let leak = name == crate::Syntax::SCOPE_TEST_SETUP;
                    self.check_block(body, !leak);
                }
                // D-DET1 (ratified 2026-06-22): `assume_deterministic { … }` — the
                // expert determinism-escape. Raise the suppression depth so the
                // determinism rejections inside a `@Pure fn` (E3403 non-deterministic
                // Core call / E3401 impure Core call) are suspended for the body. This
                // does NOT relax memory/type safety — only the determinism check. A
                // lexical scope; erased in codegen (I3 — a plain Rust block).
                Stmt::AssumeDet { body, .. } => {
                    self.det_suppress += 1;
                    self.check_block(body, true);
                    self.det_suppress -= 1;
                }
                // D-TXN1–D-TXN4 (ratified 2026-06-24): `@Transact(name) { … }`.
                // Bind the user-chosen handle `name` (typed `Transaction`) so
                // `name.on_commit(() => { … })` resolves inside the block, then check
                // the body with the transaction depth raised: an irreversible Core
                // effect (Net/Fs/Exec) reached directly in the block is E0746
                // (D-TXN2) at its call site. A lexical scope; erased in codegen (I3).
                Stmt::Transact {
                    name,
                    name_span,
                    body,
                    ..
                } => {
                    self.push_scope();
                    if let (Some(name), Some(name_span)) = (name, name_span) {
                        self.declare_loop_var(
                            name.clone(),
                            *name_span,
                            &Type::Named(crate::Syntax::TXN_HANDLE_TYPE.to_string()),
                        );
                    }
                    self.txn_depth += 1;
                    self.check_block(body, true);
                    self.txn_depth -= 1;
                    self.pop_scope();
                }
                Stmt::ContextBlock {
                    fields,
                    body,
                    span: _,
                } => {
                    for (field_name, value_expr, field_span) in fields.iter_mut() {
                        let ty = self.infer(value_expr);
                        match field_name.as_str() {
                            crate::Syntax::CTX_FIELD_ALLOCATOR => {
                                // Must be one of the known allocator handle types.
                                let ok = match &ty {
                                    Some(Type::Named(n)) => {
                                        crate::Syntax::alloc_handle_rust_type(n).is_some()
                                    }
                                    _ => false,
                                };
                                if !ok {
                                    let got = ty
                                        .as_ref()
                                        .map(|t| t.show())
                                        .unwrap_or_else(|| "unknown".to_string());
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
                            crate::Syntax::CTX_FIELD_DEADLINE => {
                                if !matches!(ty, Some(Type::Int)) {
                                    let got = ty
                                        .as_ref()
                                        .map(|t| t.show())
                                        .unwrap_or_else(|| "unknown".to_string());
                                    self.diags.push(Diagnostic::error(
                                        "E0762",
                                        format!("`deadline` needs an Int epoch-millis instant, got {}", got),
                                        "the `deadline` field carries an absolute time budget in milliseconds".to_string(),
                                        "pass an Int, e.g. `time.now() + 200`".to_string(),
                                        Some(*field_span),
                                    ));
                                }
                            }
                            _ => {
                                // Parser already rejected unknown fields (E0761);
                                // this arm is unreachable in practice.
                            }
                        }
                    }
                    let has_allocator = fields
                        .iter()
                        .any(|(n, _, _)| n == crate::Syntax::CTX_FIELD_ALLOCATOR);
                    let saved_depth = self.context_depth;
                    let saved_alloc = self.context_allocator_active;
                    self.context_depth += 1;
                    if has_allocator {
                        self.context_allocator_active = true;
                    }
                    self.check_block(body, true);
                    self.context_allocator_active = saved_alloc;
                    self.context_depth = saved_depth;
                }
            }
        }
    
}

fn statically_bounded_for_iterations(kind: &ForKind) -> Option<u64> {
    match kind {
        ForKind::Range { start, end, step } => {
            let Expr::Int(start, _, _, _) = start else { return None };
            let Expr::Int(end, _, _, _) = end else { return None };
            let step = match step {
                Some(Expr::Int(step, _, _, _)) if *step > 0 => *step as i128,
                None => 1,
                _ => return None,
            };
            if end < start {
                return Some(0);
            }
            let iterations = ((*end as i128 - *start as i128) / step) + 1;
            u64::try_from(iterations).ok()
        }
        ForKind::In { collection: Expr::ListLit(items, _) } => {
            u64::try_from(items.len()).ok()
        }
        ForKind::In { .. } => None,
    }
}
