pub(crate) fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    if !cx.debug_linemap {
        return stmts.iter().map(|s| lower_stmt(s, cx, env)).collect();
    }
    // D-DBG3 step 2 (dap-debugger): native `jet debug` build — interleave a
    // `LineMarker` ahead of every statement so the generated Rust carries a
    // rust-line -> jet-line table (`TStmt::LineMarker`'s doc). Off by default, so
    // every other build (incl. the JIT tier, which never sets `debug_linemap`)
    // is unaffected — `Vec<TStmt>` doubles in length ONLY on this path.
    let mut out = Vec::with_capacity(stmts.len() * 2);
    for s in stmts {
        let line = crate::Diagnostics::span_line_col(&cx.src, s.span().start).0;
        out.push(TStmt::LineMarker(line));
        out.push(lower_stmt(s, cx, env));
    }
    out
}

pub(crate) fn lower_stmt(s: &Stmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    match s {
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Struct { .. })) => {
            // c109: a struct-destructuring binding `Type { x, y } :: <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Named`/`Apply` naming a struct
            // (sema guarantees it). The per-field type comes from `cx.struct_fields`,
            // reproducing `emit_stmt`'s `BindPattern::Struct` arm. Each field binds with
            // its resolved type and a non-deref'd slot (the clone owns the value); the
            // pattern's field name is BOTH the bound local and the `.field` read.
            let Some(BindPattern::Struct { fields, span, .. }) = &b.pattern else {
                unreachable!("guard matched a struct pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let field_tys: HashMap<String, Type> = match &init.ty {
                Type::Named(n) | Type::Apply { name: n, .. } => cx
                    .struct_fields
                    .get(n)
                    .map(|fs| fs.iter().cloned().collect())
                    .unwrap_or_default(),
                _ => HashMap::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for f in fields {
                let field_rust = mangle(&f.name).to_string();
                let local_rust = mangle(f.local_name()).to_string();
                binds.push((local_rust.clone(), field_rust));
                env.bind(f.local_name(), local_rust, field_tys.get(&f.name).cloned());
            }
            return TStmt::StructDestructure {
                tmp,
                init,
                kw,
                binds,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::Tuple { .. })) => {
            // c109 Phase 23: a tuple-destructuring binding `(a, b) :: <init>`. Lower the
            // init ONCE; its total `.ty` is a `Type::Tuple` (sema guarantees it). Pair the
            // pattern elements to the tuple's CANONICAL fields by position, reproducing
            // `emit_stmt`'s `BindPattern::Tuple` arm. Each element binds with its resolved
            // field type and a non-deref'd slot (the clone owns the value).
            let Some(BindPattern::Tuple { elems, span }) = &b.pattern else {
                unreachable!("guard matched a tuple pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let canonical: Vec<(String, Type)> = match &init.ty {
                Type::Tuple(fs) => fs.iter().map(|(n, t)| (n.clone(), (**t).clone())).collect(),
                _ => Vec::new(),
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let mut binds = Vec::new();
            for (e, (fname, fty)) in elems.iter().zip(canonical.iter()) {
                let elem_rust = mangle(&e.name).to_string();
                let field_rust = mangle(fname).to_string();
                binds.push((elem_rust.clone(), field_rust));
                env.bind(&e.name, elem_rust, Some(fty.clone()));
            }
            return TStmt::TupleDestructure {
                tmp,
                init,
                kw,
                binds,
            };
        }
        Stmt::Val(b) if matches!(&b.pattern, Some(BindPattern::List { .. })) => {
            // c109 Phase 26: a list-destructuring binding `[a, b, c] :: <init>`. Lower
            // the init ONCE, then bind each element via `jet_unpack_vec(tmp, want, i,
            // file, line)`, reproducing `emit_stmt`'s `BindPattern::List` arm. The
            // element slot type reproduces `expr_jet_ty(init)`'s `Some(List(inner))`-only
            // match: the LOWERED init's `.ty` is exactly what `expr_jet_ty(&b.init)`
            // resolves (an Ident → its slot type), so a non-`List` init (e.g. a `[T#N]`
            // fan-out result) yields a `None` element type — byte-identical partiality.
            let Some(BindPattern::List { elems, span }) = &b.pattern else {
                unreachable!("guard matched a list pattern")
            };
            let init = lower_expr(&b.init, cx, env);
            let elem_ty = match &init.ty {
                Type::List(inner) => Some((**inner).clone()),
                _ => None,
            };
            let tmp = format!("__jet_d{}", span.start);
            let kw = if b.mutable { "let mut" } else { "let" };
            let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
            let mut elem_names = Vec::new();
            for e in elems {
                let m = mangle(&e.name).to_string();
                elem_names.push(m.clone());
                env.bind(&e.name, m, elem_ty.clone());
            }
            return TStmt::ListDestructure {
                tmp,
                init,
                kw,
                want: elems.len(),
                file: cx.file.clone(),
                line,
                elems: elem_names,
            };
        }
        Stmt::Val(b) => {
            // D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL1: lower
            // `name: T := uninit` to
            //   `let mut name: T = unsafe { std::mem::MaybeUninit::<T>::uninit().assume_init() };`
            // The source's `use core.mem` + `:= uninit` is the expert-tier opt-in (I1: no
            // `unsafe` in generated code without a source-level gate). Sema proved
            // write-before-read (E0420), so every subsequent read is post-write — the
            // `assume_init()` at declaration yields garbage bytes that are always
            // overwritten before any read. The `is_pod_uninit_type` guard in sema
            // (E0423) ensures T has no Drop glue, so no destructor ever reads the garbage.
            if b.uninit {
                let ty =
                    b.ty.as_ref()
                        .expect("E0421 ensures a `:= uninit` binding has a type");
                let rust_ty = cx.rust_type(ty);
                let init_str = format!(
                    "unsafe {{ std::mem::MaybeUninit::<{}>::uninit().assume_init() }}",
                    rust_ty
                );
                env.bind(&b.name, mangle(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let mut",
                    ty_clause: format!(": {}", rust_ty),
                    init: TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::ConstInline(init_str),
                    },
                    track_origin: None,
                };
            }
            // c109 Phase 19: an arena `view` binding (`x :: arena.alloc(v)`). The AST
            // `emit_let`'s `arena_view` branch emits `let <x> = <init>;` (NO type clause,
            // NEVER `let mut` — a view is a non-reassignable `&mut T`) and binds a DEREF'd
            // slot (reads go through `(*x)`). Reproduce it exactly: a `Let` with `kw: "let"`,
            // empty `ty_clause`, and a deref'd slot place `(*<x>)`.
            if b.arena_view {
                let init = lower_expr(&b.init, cx, env);
                env.bind(&b.name, format!("(*{})", mangle(&b.name)), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause: String::new(),
                    init,
                    track_origin: None,
                };
            }
            // D-MEM1 stage S5 (2026-07-04): a string-view binding (`x :: s.trim()` /
            // `x :: s.after(sep)` / `x :: s.before(sep)`; sema set `string_view`
            // after proving E2307-safety — see `CheckerCore.rs`'s binding check).
            // Unlike `arena_view` this binds a plain `&str` (no deref needed to
            // read it): `ty_clause: ": &str"`, `kw: "let"` (non-reassignable,
            // non-escaping local, I8, same as arena/list views), and the init
            // goes through the borrowed `_view` builtin op instead of
            // `resolve_builtin_op`'s owned default.
            if b.string_view {
                let init = lower_string_view_init(&b.init, cx, env);
                env.bind(&b.name, mangle(&b.name), Some(Type::String));
                env.mark_string_view(&b.name);
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause: ": &str".to_string(),
                    init,
                    track_origin: None,
                };
            }
            // c109 (S57/M9.5): a comptime LOCAL `comptime NAME = expr`. The AST `emit_let`
            // builds `init` from `b.ct.serialize()` (the sema-evaluated value rendered to a
            // Rust literal) — the runtime `init` expr is never emitted. Reproduce it: a
            // verbatim `ConstInline` of the same serialized string, with `kw: "let"` (the
            // `(b.mutable && !b.is_comptime)` guard makes it `let`, never `let mut`) and the
            // type clause from `b.ty` (rendered exactly as the non-comptime path below). All
            // facts are pre-resolved (I3): no inference here.
            if b.is_comptime {
                let serialized =
                    b.ct.as_ref()
                        .map(|v| v.serialize())
                        .unwrap_or_else(|| "Default::default()".to_string());
                // Mirror `emit_let`'s type clause exactly (a Fn type via `rust_fn_trait`,
                // others via `rust_type`). A comptime value is never fn-typed, but match
                // the AST shape verbatim for total byte-parity.
                let ty_clause =
                    b.ty.as_ref()
                        .map(|t| {
                            if let Type::Fn { params, ret, .. } = t {
                                format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), false))
                            } else {
                                format!(": {}", cx.rust_type(t))
                            }
                        })
                        .unwrap_or_default();
                let init = TExpr {
                    ty: b.ty.clone().unwrap_or(Type::Int),
                    kind: TExprKind::ConstInline(serialized),
                };
                env.bind(&b.name, mangle(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    ty_clause,
                    init,
                    track_origin: None,
                };
            }
            let mut init = lower_expr(&b.init, cx, env);
            // D-FIXARR1: if the binding annotation is `[T#N]` and the init lowered as a
            // growable list (e.g. a plain list literal), re-tag the TExpr type so the emit
            // produces a Rust array literal `[e1, …]` instead of `vec![…]`.
            if let Some(fl @ Type::FixedList { .. }) = &b.ty {
                if matches!(init.ty, Type::List(_)) && matches!(init.kind, TExprKind::ListLit(_)) {
                    init.ty = fl.clone();
                }
            }
            // D-SOA1: an EMPTY list literal `[]` for a declared columnar `[S]` lowers
            // with an Int placeholder element type (no element to infer from), so it
            // came through as a plain `ListLit([])`/`vec![]`. Rewrite it to the
            // columnar empty constructor `user_<S>_columns::from_aos(vec![])` using
            // the binding's declared type.
            if let Some(decl @ Type::List(inner)) = &b.ty {
                if let Some(columns_ty) = cx.columnar_list_type(inner) {
                    if matches!(&init.kind, TExprKind::ListLit(es) if es.is_empty()) {
                        init = TExpr {
                            ty: decl.clone(),
                            kind: TExprKind::ColumnarListLit {
                                columns_ty,
                                elems: Vec::new(),
                            },
                        };
                    }
                }
            }
            // c109 Phase 13: reproduce `emit_let`'s `mut_fn` form — an escaping FnMut
            // lambda binding gets `let mut` AND an `as <fn-trait(mut)>` init coercion +
            // a `: <fn-trait(mut)>` annotation. Decided here from `Lambda.meta`.
            let mut_fn = matches!(
                &b.init,
                Expr::Lambda(l) if l.meta.escapes && l.meta.needs_fn_mut
            );
            if mut_fn {
                if let Some(Type::Fn { params, ret, .. }) = &b.ty {
                    let coerced = format!(
                        "{} as {}",
                        emit_tir_expr(&init, cx),
                        cx.rust_fn_trait(params, ret.as_deref(), true)
                    );
                    init = TExpr {
                        ty: init.ty.clone(),
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::NamedFn { wrapper: coerced },
                        },
                    };
                }
            }
            // Totality: if the source omitted the type, infer it ONCE here from
            // the init's already-resolved type. Codegen never infers.
            let ty = b.ty.clone().unwrap_or_else(|| init.ty.clone());
            // E2-M7/E2-M10/D-ALLOC1/D-ROUTE1: a handle binding forces `let mut` even
            // when bound immutably (its methods take `&mut self`). Mirror
            // `emit_let`'s `is_file_handle` set exactly.
            let is_file_handle = matches!(
                &ty,
                Type::Named(n) if n == "FileReader" || n == "FileWriter"
                    || n == "Stdout" || n == "Stderr"
                    || n == "TcpStream" || n == "UnixStream" || n == "HttpRouter"
                    || n == "Arena" || n == "Bump" || n == "Pool" || n == "Fixed"
            )
            // D-SHIFT1 (c7shift): `Reader`/`Cursor` bindings are usually
            // written without an annotation (`r :: Reader.over(bytes)`), so
            // test the resolved type; every read advances `pos` (`&mut self`).
            // User-type-wins guard as everywhere else for these two names.
            || matches!(
                &ty,
                Type::Named(n) if (n == "Reader" || n == "Cursor")
                    && !cx.type_names.contains(n.as_str())
            );
            let kw = if (b.mutable && !b.is_comptime) || mut_fn || is_file_handle {
                "let mut"
            } else {
                "let"
            };
            // The type annotation clause, rendered exactly as `emit_let`: a Fn type via
            // `rust_fn_trait(params, ret, mut_fn)`, others via `rust_type`. Empty for an
            // inferred binding.
            let ty_clause =
                b.ty.as_ref()
                    .map(|t| {
                        if let Type::Fn { params, ret, .. } = t {
                            format!(": {}", cx.rust_fn_trait(params, ret.as_deref(), mut_fn))
                        } else {
                            format!(": {}", cx.rust_type(t))
                        }
                    })
                    .unwrap_or_default();
            let track_origin = tracked_float_origin(b, &ty, cx);
            env.bind(&b.name, mangle(&b.name), Some(ty));
            TStmt::Let {
                name: b.name.clone(),
                kw,
                ty_clause,
                init,
                track_origin,
            }
        }
        Stmt::Assign {
            target, op, value, ..
        } => match target {
            LValue::Local { name, .. } => {
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                TStmt::Assign {
                    place: env.place_of(name),
                    op: *op,
                    value: lower_expr(value, cx, env),
                    clone_value,
                }
            }
            // c109 Phase 5: `coll[i] = v`. The `IndexKind` is resolved by sema; carry
            // it as the total `is_map` fact (the gate excluded `Unknown`). No compound
            // op on an index lvalue (parser admits only `=`).
            LValue::Index {
                base,
                index,
                kind,
                span,
            } => {
                let base_t = lower_expr(base, cx, env);
                let index_t = lower_expr(index, cx, env);
                let value_t = lower_expr(value, cx, env);
                if let IndexKind::User(type_name) = kind {
                    return TStmt::IndexHookAssign {
                        type_name: type_name.clone(),
                        base: base_t,
                        index: index_t,
                        value: value_t,
                    };
                }
                // D-MEM1 S6: `pool[id] = v` — a genuine mutable place through
                // `jet_pool_get_mut` (generation-checked, panics on a stale `id`),
                // not a value round-trip. Reuses the plain `TStmt::Assign` (a raw
                // Rust place string) rather than `IndexAssign`'s bool-keyed
                // List/Map dispatch, since Pool needs its own helper + panic text.
                if matches!(kind, IndexKind::Pool) {
                    let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0;
                    let b = emit_tir_expr(&base_t, cx);
                    let i = emit_tir_expr(&index_t, cx);
                    return TStmt::Assign {
                        place: format!(
                            "(*{root}jet_std::jet_pool_get_mut(&mut ({b}), {i}, {file:?}, {line}))",
                            root = cx.root_prefix,
                            file = cx.file,
                        ),
                        op: *op,
                        value: value_t,
                        clone_value: false,
                    };
                }
                TStmt::IndexAssign {
                    base: base_t,
                    index: index_t,
                    is_map: matches!(kind, IndexKind::Map),
                    value: value_t,
                }
            }
            // D-MUTSELF1: a field-assignment `place.field [op]= v`. The place is the
            // field READ lowered to its resolved Rust string (`((*self)).field` once
            // the `mut self` slot derefs), reusing the same `Expr::Field` lowering the
            // read path uses — byte-for-byte the AST `LValue::Field` form. Carried as a
            // plain `TStmt::Assign` so the `op` compound form rides the shared emit.
            LValue::Field { base, field, span } => {
                let base_t = lower_expr(base, cx, env);
                let swizzle_write = match &base_t.ty {
                    Type::Named(type_name)
                        if crate::Sema::is_swizzleable_math_type(type_name)
                            && !cx.struct_fields.contains_key(type_name) =>
                    {
                        match crate::Sema::parse_swizzle_member(field, type_name) {
                            crate::Sema::SwizzleParse::Ok(lanes) => {
                                let lanes_u8: Vec<u8> = lanes.iter().map(|&i| i as u8).collect();
                                Some((type_name.clone(), lanes_u8))
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some((type_name, lanes_u8)) = swizzle_write {
                    let clone_value = if let Expr::Ident(vname, _) = value {
                        env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                    } else {
                        false
                    };
                    return TStmt::MathSwizzleAssign {
                        base: base_t,
                        type_name,
                        lanes: lanes_u8,
                        value: lower_expr(value, cx, env),
                        clone_value,
                    };
                }
                // D-MEM1 S6: `pool[id].field = v` — the general fallback below
                // resolves `place` by re-emitting the FIELD-READ expression (fine
                // for an owning local/`self`, but a `Pool` index-read is a value
                // clone via `jet_pool_get` — writing `.field` on that would edit a
                // throwaway copy and silently drop the change). Build a genuine
                // mutable place through `jet_pool_get_mut` instead.
                if let Expr::Index {
                    base: pool_expr,
                    index: id_expr,
                    kind: IndexKind::Pool,
                    span: idx_span,
                } = base.as_ref()
                {
                    let line = crate::Diagnostics::span_line_col(&cx.src, idx_span.start).0;
                    let pool_t = lower_expr(pool_expr, cx, env);
                    let id_t = lower_expr(id_expr, cx, env);
                    let p = emit_tir_expr(&pool_t, cx);
                    let i = emit_tir_expr(&id_t, cx);
                    let place = format!(
                        "(*{root}jet_std::jet_pool_get_mut(&mut ({p}), {i}, {file:?}, {line})).{field_rust}",
                        root = cx.root_prefix,
                        file = cx.file,
                        field_rust = mangle(field),
                    );
                    let clone_value = if let Expr::Ident(vname, _) = value {
                        env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                    } else {
                        false
                    };
                    return TStmt::Assign {
                        place,
                        op: *op,
                        value: lower_expr(value, cx, env),
                        clone_value,
                    };
                }
                let field_expr = Expr::Field(base.clone(), field.clone(), *span);
                let place = emit_tir_expr(&lower_expr(&field_expr, cx, env), cx);
                // c150: mirror the lower_enum_arg clone predicate — a borrowed non-scalar
                // ident on the RHS would move out of a shared reference (E0507, I2).
                let clone_value = if let Expr::Ident(vname, _) = value {
                    env.is_borrowed(vname) && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                } else {
                    false
                };
                TStmt::Assign {
                    place,
                    op: *op,
                    value: lower_expr(value, cx, env),
                    clone_value,
                }
            }
        },
        Stmt::Return(Some(e), _) => TStmt::Return(Some(lower_expr(e, cx, env))),
        Stmt::Return(None, _) => TStmt::Return(None),
        // D-STREAMYIELD1: `yield e` inside a generator's spawned thread — send on
        // the channel the wrapping `Stream<T>` body opened (see `emit_generator_body`),
        // blocking (rendezvous, bound 0) until the consumer pulls. A closed receiver
        // (consumer stopped early) makes `send` fail; ignored — the thread just runs
        // to completion doing nothing further useful, rather than panicking.
        Stmt::Yield(e, _) => {
            let v = lower_expr(e, cx, env);
            TStmt::ExprStmt(TExpr {
                ty: unit_type(),
                kind: TExprKind::ConstInline(format!(
                    "let _ = __jet_yield_tx.send({});",
                    emit_tir_expr(&v, cx)
                )),
            })
        }
        // D-IGNORERET2=A: `.drop("reason")` — lower only the receiver (for side effects).
        // The method call itself is erased; the "reason" string is audit-only.
        Stmt::Expr(Expr::MethodCall {
            receiver, method, ..
        }) if method == Syntax::METHOD_DROP => TStmt::ExprStmt(lower_expr(receiver, cx, env)),
        Stmt::Expr(e) => TStmt::ExprStmt(lower_expr(e, cx, env)),
        Stmt::If(ifs) => lower_if(ifs, cx, env),
        // c109 Phase 2: control-flow loops. Loop bodies are their own scope —
        // lower on a cloned env so bindings inside don't leak out.
        Stmt::Loop { body, label, .. } => {
            let mut branch = clone_env(env);
            TStmt::Loop {
                label: label_name(label),
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::While {
            cond, body, label, ..
        } => {
            let cond = lower_expr(cond, cx, env);
            let mut branch = clone_env(env);
            TStmt::While {
                label: label_name(label),
                cond,
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        // D-LOOP-SEMICOLON1=A: `loop init; cond; step { body }` three-part counted loop.
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            label,
            ..
        } => {
            // Lower the init binding as a `let mut` local.
            let init_val = lower_expr(&init.init, cx, env);
            let init_ty = init.ty.clone();
            env.bind(&init.name, mangle(&init.name), init_ty);
            let init_stmt = Box::new(TStmt::Let {
                name: init.name.clone(),
                kw: "let mut",
                ty_clause: String::new(),
                init: init_val,
                track_origin: None,
            });
            let cond = lower_expr(cond, cx, env);
            let mut branch = clone_env(env);
            let step = Box::new(lower_stmt(step.as_ref(), cx, &mut branch));
            TStmt::CountedLoop {
                label: label_name(label),
                init: init_stmt,
                cond,
                step,
                body: lower_stmts(body, cx, &mut branch),
            }
        }
        Stmt::For {
            var,
            var2,
            kind,
            body,
            label,
            ..
        } => match kind {
            ForKind::Range { start, end, step } => {
                let start = lower_expr(start, cx, env);
                let end = lower_expr(end, cx, env);
                let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                // The loop var is an `Int` local for the body's scope only. The AST
                // (`Statement.rs`) inserts it into the shared env, emits the body, then
                // RESTORES the prior binding — so a scalar `??` panic dump INSIDE the body
                // sees the var, but one after the loop does not. Reproduce that exactly:
                // bind it on the shared `panic_locals`, lower the body, then restore.
                let mut branch = clone_env(env);
                let prev = branch.panic_locals.borrow().get(var).cloned();
                branch.bind(var, mangle(var), Some(Type::Int));
                let lowered_body = lower_stmts(body, cx, &mut branch);
                match prev {
                    Some(p) => {
                        branch.panic_locals.borrow_mut().insert(var.clone(), p);
                    }
                    None => {
                        branch.panic_locals.borrow_mut().remove(var);
                    }
                }
                TStmt::Range {
                    label: label_name(label),
                    var: var.clone(),
                    start,
                    end,
                    step,
                    body: lowered_body,
                }
            }
            // c109 Phase 5: collection iteration `loop x in coll` / `loop k, v in map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection } => {
                // c109 Phase 22: classify a method-call collection into the matching
                // `emit_for_in` branch (`chars`/`lines`/the `.iter().cloned()` default),
                // resolving the receiver/collection string off the SAME node shape the
                // AST path reads. `method_kind == None` is the plain `.iter()` form.
                let (collection_str, method_kind) = lower_forin_collection(collection, cx, env);
                // Infer the element type from the lowered collection so the loop
                // variable binds with its concrete type. This lets `core_struct_field_rust_name`
                // emit plain field names (not `user_<field>`) for core types like DirEntry.
                let lowered_coll = lower_expr(collection, cx, env);
                let mut method_kind = method_kind;
                let mut coll_elem_ty: Option<Type> = match &lowered_coll.ty {
                    Type::List(inner) => Some((**inner).clone()),
                    Type::FixedList { elem, .. } => Some((**elem).clone()),
                    // Map iteration: key type for single-binding form.
                    Type::Map { key, .. } => Some((**key).clone()),
                    // D-STREAMYIELD1: a generator's `Stream<T>`.
                    Type::Apply { name, args } if name == "Stream" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    // D-DYNARRAY1: `loop x in window` — a `View<T>`'s element type.
                    Type::Apply { name, args } if name == "View" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    _ => None,
                };
                let by_value =
                    matches!(&lowered_coll.ty, Type::Apply { name, .. } if name == "Stream");
                if method_kind.is_none() {
                    if let Type::Named(n) = &lowered_coll.ty {
                        if let Some(hook) = cx.iterable_hooks.get(n) {
                            method_kind = Some(TForInMethod::Iterable {
                                coll_type: n.clone(),
                                iter_type: hook.iter_type.clone(),
                            });
                            coll_elem_ty = Some(hook.item_type.clone());
                        }
                    }
                }
                let mut branch = clone_env(env);
                branch
                    .locals
                    .insert(var.clone(), (mangle(var), coll_elem_ty.clone()));
                if let Some((v2, _)) = var2 {
                    // Two-binding map form: v2 gets the value type.
                    let v2_ty = match &coll_elem_ty {
                        _ => None, // map value type is not tracked here; keep None for v2
                    };
                    branch.locals.insert(v2.clone(), (mangle(v2), v2_ty));
                }
                // D-SOA1: a single-binding loop over a columnar list iterates the
                // gathered AoS view (`iter_aos`), not `Vec::iter` (which the columns
                // type doesn't expose).
                let columnar = var2.is_none()
                    && method_kind.is_none()
                    && coll_elem_ty
                        .as_ref()
                        .map(|t| cx.columnar_list_type(t).is_some())
                        .unwrap_or(false);
                TStmt::ForIn {
                    label: label_name(label),
                    var: var.clone(),
                    var2: var2.as_ref().map(|(n, _)| n.clone()),
                    collection_str,
                    method_kind,
                    columnar,
                    by_value,
                    body: lower_stmts(body, cx, &mut branch),
                }
            }
        },
        Stmt::Break(_) => TStmt::Break(None),
        Stmt::Continue(_) => TStmt::Continue(None),
        Stmt::BreakLabel(name, _) => TStmt::Break(Some(name.clone())),
        Stmt::ContinueLabel(name, _) => TStmt::Continue(Some(name.clone())),
        // c109 Phase 4: a `when`/match. The gate already classified it as either an
        // exhaustive enum match (shape A) or an all-range scalar switch (shape B).
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => lower_switch(subject, arms, else_body, cx, env),
        // D-CTMARKER1 (ratified 2026-06-25, piece 2): `comptime { … }` runs at
        // build time and erases entirely — no runtime Rust is emitted (I3).
        Stmt::ComptimeBlock { .. } => TStmt::Inline(vec![]),
        // D-CANVASSTATE1=D: `#Off` type-checks in sema but emits no runtime TIR.
        Stmt::Off { .. } => TStmt::Inline(vec![]),
        // D-CANVASSTATE1=D: `#DebugOnly` is a lexical debug-only region. Lower
        // on a cloned env so declarations cannot be required by release code.
        Stmt::DebugOnly { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::DebugOnly(lower_stmts(body, cx, &mut scoped))
        }
        // c109 Phase 15: a resolved comptime-if (`Stmt::ComptimeIf`). Sema chose the
        // branch (`selected_then`); the AST `emit_stmts` emits ONLY that branch's
        // statements INLINE on the SAME `&mut env` at the SAME indent (no `if`, no
        // block — its `let`s leak into the outer scope). Reproduce both: lower the
        // selected branch's statements on the SAME `env` (so their bindings leak, like
        // the AST shared env) and wrap them in a flat `Inline` node.
        Stmt::ComptimeIf {
            then_body,
            else_body,
            selected_then,
            ..
        } => {
            let chosen: &[Stmt] = match selected_then {
                Some(true) => then_body,
                Some(false) => else_body.as_deref().unwrap_or(&[]),
                // Sema didn't resolve (earlier error) — emit nothing (I3), like the AST.
                None => &[],
            };
            TStmt::Inline(lower_stmts(chosen, cx, env))
        }
        // c109 Phase 18: an audited `#Unsafe { … }` region (`Stmt::Unsafe`). The AST
        // `emit_stmts` emits `unsafe { … }` and lowers the body on the SAME `&mut env`
        // (the body's `let`s leak into the outer scope). Reproduce: lower the body on the
        // SAME `env` (so bindings leak) and wrap in `TStmt::Unsafe`. The `#Audit("…")`
        // annotation is dropped (codegen is dumb — it emits nothing, matching the AST).
        // I1: the source `#Unsafe` gate is 1:1 with this node, the only producer of a
        // Rust `unsafe` block.
        Stmt::Unsafe { body, .. } => TStmt::Unsafe(lower_stmts(body, cx, env)),
        // D-CTEFFECT1: `#Impure` erases to a plain block at codegen (comptime-only gate, I3).
        Stmt::Impure { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-IGNORERET2=A: `#Suppress(MustUse)` erases to a plain block at codegen.
        // The sema suppression is a compile-time-only fact (I3).
        Stmt::SuppressMustUse { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-REACTCORE1: `#Reactive { … }` lowers to `jet_reactive_effect(closure)`.
        // Clone outer captures into the closure (same as a stored lambda).
        Stmt::Reactive { body, .. } => {
            let closure = render_reactive_block_closure(body, cx, env);
            TStmt::Reactive { closure }
        }
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1). The AST emits a plain
        // block and lowers the body on the SAME `&mut env` (its `let`s leak into the outer
        // scope). Reproduce: lower the body on the SAME `env`, wrap in `TStmt::Region`.
        Stmt::Region { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-TASKSCOPE1=A: taskgroup erases to a plain block at codegen (I3).
        Stmt::TaskGroup { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout NAME { … }` needs a REAL
        // runtime object (unlike Region/TaskGroup, which erase) — bind `name`
        // to a fresh `jet_layout::Handle` BEFORE lowering the body, so the
        // desugared `NAME.h(box, anchor)` calls inside resolve to it, exactly
        // like an ordinary `NAME :: jet_layout::Handle::new(…)` binding would.
        Stmt::Layout { name, body, .. } => {
            let place = mangle(name);
            env.bind(
                name,
                place.clone(),
                Some(Type::Named(Syntax::LAYOUT_HANDLE_TYPE.to_string())),
            );
            let lowered_body = lower_stmts(body, cx, env);
            TStmt::Layout {
                rust_place: place,
                label: name.clone(),
                body: lowered_body,
            }
        }
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1). `emit_stmt`'s
        // `Stmt::Caps` arm is byte-for-byte `Stmt::Region` — a plain block with the body lowered
        // on the SAME `&mut env` (its `let`s leak). Effects erase at codegen (I3); reuse the
        // `TStmt::Region` shape.
        Stmt::Caps { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-SCAP1: a `#grant(Fs) { caps -> … }` grant region. The capability handle
        // is a compile-time-only fact (authority to perform the granted effects),
        // erased here (I3); the body lowers on the SAME `&mut env` (its `let`s leak)
        // into a plain `TStmt::Region` — byte-for-byte the `Stmt::Region`/`Stmt::Caps`
        // shape. No runtime grant/revoke value, no `unsafe`.
        Stmt::Grant { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1/D-DEADLINE1).
        // Resolve each field into a `(field_name, value)` guard at lowering, then lower the body on
        // the SAME `env` (it leaks like a region). Emit reproduces `emit_stmts`'s
        // `Stmt::ContextBlock` arm byte-for-byte.
        Stmt::ContextBlock { fields, body, .. } => {
            let guards = fields
                .iter()
                .map(|(name, v, _)| (name.clone(), lower_expr(v, cx, env)))
                .collect();
            TStmt::ContextBlock {
                guards,
                body: lower_stmts(body, cx, env),
            }
        }
        // D-TERM1 (ratified 2026-06-22): `live { … }` block. The body leaks into the
        // enclosing `env` (same as `Stmt::Region`) so let-bindings inside are visible
        // after the block (consistent with all other Jet lexical blocks). Lowering only
        // records the body; the enter/guard/leave preamble is emitted in `emit_tir_stmt`.
        Stmt::Live { body, .. } => TStmt::Live {
            body: lower_stmts(body, cx, env),
        },
        // D-DOTSCOPE1: a `#Test` scope member (`.setup`/`.expect_fail`/`.timeout`/
        // `.skip`). Legality/args were checked in sema; here we pick the lowering
        // kind and fold `.timeout`'s duration literal to a nanosecond budget.
        // `.setup`'s body leaks into `env` (like a region) so its bindings are
        // visible to the rest of the test; the others open their own scope in
        // `emit_tir_stmt`.
        Stmt::ScopeMember {
            name, args, body, ..
        } => {
            let kind = if name == Syntax::SCOPE_TEST_SETUP {
                ScopeMemberKind::Setup
            } else if name == Syntax::SCOPE_TEST_EXPECT_FAIL {
                ScopeMemberKind::ExpectFail
            } else if name == Syntax::SCOPE_TEST_TIMEOUT {
                ScopeMemberKind::Timeout(timeout_nanos(args))
            } else {
                ScopeMemberKind::Skip
            };
            TStmt::ScopeMember {
                kind,
                body: lower_stmts(body, cx, env),
            }
        }
        // D-DET1: `assume_deterministic { … }` erases to a plain `TStmt::Region`
        // (byte-for-byte the `Stmt::Region`/`Stmt::Caps` shape). The determinism
        // suspension is a sema-only fact; nothing runtime, no `unsafe` (I3).
        Stmt::AssumeDet { body, .. } => TStmt::Region(lower_stmts(body, cx, env)),
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block. Bind the
        // handle (typed `Transaction`) in `env` so `name.on_commit(…)` lowers against
        // it, then lower the body on the SAME `env` (it leaks like a region). The
        // `let mut <handle> = jet_transaction(); … <handle>.commit();` framing is
        // emitted in `emit_tir_stmt`; codegen is dumb (I3).
        Stmt::Transact { name, body, .. } => {
            let handle = name.as_ref().map(|name| {
                let h = mangle(name);
                env.bind(
                    name,
                    h.clone(),
                    Some(Type::Named(Syntax::TXN_HANDLE_TYPE.to_string())),
                );
                h
            });
            // D-TXN-ROLLBACK layer 1 (auto-snapshot): collect the root local names
            // assigned anywhere in the block (recursing into nested control flow, but
            // NOT into nested `#Transact` blocks or lambda bodies — those own their
            // own rollback scope / are deferred). Snapshot only roots ALREADY in scope
            // at block entry (params / outer locals): a local declared inside the block
            // needs no snapshot, since rollback discards it when the block scope ends.
            // Each becomes `&mut <place>` so the prelude can clone+restore it.
            let mut roots: Vec<String> = Vec::new();
            collect_txn_mut_roots(body, &mut roots);
            let snapshots: Vec<(String, Option<String>)> = roots
                .iter()
                .filter(|r| env.locals.contains_key(*r))
                .map(|r| {
                    let place_ref = format!("&mut {}", env.place_of(r));
                    // D-TXN-ROLLBACK layer 2: if the root type implements Rollback,
                    // use snapshot_custom instead of the clone-based snapshot path.
                    let rollback_ty = env.ty_of(r).and_then(|ty| {
                        if let crate::AST::Type::Named(n) = ty {
                            if cx.rollback_types.contains(&n) {
                                return Some(format!("user_{n}"));
                            }
                        }
                        None
                    });
                    (place_ref, rollback_ty)
                })
                .collect();
            TStmt::Transact {
                handle,
                snapshots,
                body: lower_stmts(body, cx, env),
            }
        }
        // Forward-safety default: a Stmt variant not in the subset never reaches
        // lowering (`stmt_in_subset` returns false for it). Kept as a guard against a
        // future variant; currently unreachable because every covered variant is matched.
        #[allow(unreachable_patterns)]
        _ => unreachable!("statement not in TIR subset"),
    }
}
