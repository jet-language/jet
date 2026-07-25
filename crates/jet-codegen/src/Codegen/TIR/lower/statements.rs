use crate::AST::{BindPattern, Expr, ForKind, IndexKind, LValue, PlaceAccess, Stmt, Type, UnOp};
use crate::Codegen::Cx;
#[cfg(test)]
use crate::Codegen::build_cx;
#[cfg(test)]
use crate::Diagnostics::Span;
use crate::Codegen::mangle;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::label_name;
use crate::Codegen::TIR::lower::collect_txn_mut_roots;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_forin_collection;
use crate::Codegen::TIR::lower_if;
use crate::Codegen::TIR::lower::lower_string_view_init;
use crate::Codegen::TIR::lower::render_reactive_block_closure;
use crate::Codegen::TIR::lower_switch;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::lower::timeout_nanos;
use crate::Codegen::TIR::lower::tracked_float_origin;
use crate::Codegen::TIR::ScopeMemberKind;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TLetTy;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::TForInMethod;
use crate::Codegen::TIR::TIndexFieldAssign;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TPlace;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::lower::lower_comptime_scalar;
use crate::Codegen::TIR::unit_type;
use crate::Syntax;
use std::collections::HashMap;

pub(crate) fn lower_stmts(stmts: &[Stmt], cx: &Cx, env: &mut LowerEnv) -> Vec<TStmt> {
    let mut out = Vec::with_capacity(stmts.len() * if cx.debug_linemap { 2 } else { 1 });
    let mut split_views = split_view_plan(stmts, cx);
    let mut index = 0;
    while index < stmts.len() {
        if let Some(view) = split_views.remove(&index) {
            if cx.debug_linemap {
                out.push(TStmt::LineMarker(view.candidate.line));
            }
            let candidate = view.candidate;
            let slot = if candidate.single {
                TLocal::user(&candidate.name).through_ref()
            } else {
                TLocal::user(&candidate.name)
            };
            env.bind(&candidate.name, slot, candidate.ty.clone());
            out.push(TStmt::SplitViews {
                owner: view
                    .initialize
                    .then(|| lower_expr(&candidate.owner, cx, env)),
                root: view.root,
                len: view.len,
                source: view.source,
                source_start: view.source_start,
                before: view.before,
                split_tail: view.split_tail,
                segment: view.segment,
                after: view.after,
                name: candidate.name,
                start: candidate.start,
                end: candidate.end,
                single: candidate.single,
                write: candidate.write,
                line: candidate.line,
            });
            index += 1;
            continue;
        }
        let s = &stmts[index];
        if cx.debug_linemap {
            let line = crate::Diagnostics::span_line_col(&cx.src, s.span().start).0;
            out.push(TStmt::LineMarker(line));
        }
        out.push(lower_stmt(s, cx, env));
        index += 1;
    }
    out
}

#[derive(Clone)]
struct SplitViewCandidate {
    stmt_index: usize,
    owner: Expr,
    owner_key: String,
    name: String,
    ty: Option<Type>,
    start: i64,
    end: i64,
    single: bool,
    write: bool,
    line: usize,
    last_use: usize,
}

struct PlannedSplitView {
    candidate: SplitViewCandidate,
    initialize: bool,
    root: String,
    len: String,
    source: String,
    source_start: i64,
    before: String,
    split_tail: String,
    segment: String,
    after: String,
}

#[derive(Clone)]
struct SplitRegion {
    name: String,
    start: i64,
    end: Option<i64>,
}

fn const_place_bound(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Int(value, ..) => Some(*value),
        Expr::Unary(UnOp::Neg, inner, _) => const_place_bound(inner)?.checked_neg(),
        Expr::Paren(inner, _) => const_place_bound(inner),
        _ => None,
    }
}

fn split_owner_key(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(format!("name:{name}")),
        Expr::Field(base, field, _) => {
            Some(format!("{}.field:{field}", split_owner_key(base)?))
        }
        Expr::Index { base, index, .. } => Some(format!(
            "{}.index:{}",
            split_owner_key(base)?,
            const_place_bound(index)?
        )),
        Expr::Paren(inner, _) | Expr::Place(inner, _, _) => split_owner_key(inner),
        _ => None,
    }
}

fn split_view_candidate(stmt: &Stmt, stmt_index: usize, cx: &Cx) -> Option<SplitViewCandidate> {
    let Stmt::Val(binding) = stmt else {
        return None;
    };
    let Expr::Place(inner, access, _) = &binding.init else {
        return None;
    };
    let (base, start, end, single) = match inner.as_ref() {
        Expr::Slice {
            base, start, end, ..
        } => (base.as_ref(), const_place_bound(start)?, const_place_bound(end)?, false),
        Expr::Index { base, index, .. } => {
            let index = const_place_bound(index)?;
            (base.as_ref(), index, index, true)
        }
        _ => return None,
    };
    let owner_key = split_owner_key(base)?;
    Some(SplitViewCandidate {
        stmt_index,
        owner: base.clone(),
        owner_key,
        name: binding.name.clone(),
        ty: binding.ty.clone(),
        start,
        end,
        single,
        write: matches!(access, PlaceAccess::Write),
        line: crate::Diagnostics::span_line_col(&cx.src, binding.name_span.start).0,
        last_use: stmt_index,
    })
}

fn split_view_plan(stmts: &[Stmt], cx: &Cx) -> HashMap<usize, PlannedSplitView> {
    let mut candidates: Vec<_> = stmts
        .iter()
        .enumerate()
        .filter_map(|(index, stmt)| split_view_candidate(stmt, index, cx))
        .filter(|view| view.start >= 0 && view.end >= view.start)
        .collect();
    for candidate in &mut candidates {
        candidate.last_use = stmts[candidate.stmt_index + 1..]
            .iter()
            .enumerate()
            .filter(|(_, stmt)| crate::Sema::stmt_references_name_exact(stmt, &candidate.name))
            .map(|(offset, _)| candidate.stmt_index + 1 + offset)
            .next_back()
            .unwrap_or(candidate.stmt_index);
    }

    let mut parent: Vec<usize> = (0..candidates.len()).collect();
    fn find(parent: &mut [usize], mut index: usize) -> usize {
        while parent[index] != index {
            parent[index] = parent[parent[index]];
            index = parent[index];
        }
        index
    }
    fn union(parent: &mut [usize], a: usize, b: usize) {
        let a = find(parent, a);
        let b = find(parent, b);
        if a != b {
            parent[b] = a;
        }
    }
    for a in 0..candidates.len() {
        for b in a + 1..candidates.len() {
            let earlier = &candidates[a];
            let later = &candidates[b];
            if earlier.owner_key == later.owner_key
                && earlier.last_use >= later.stmt_index
                && (earlier.write || later.write)
                && (earlier.end < later.start || later.end < earlier.start)
            {
                union(&mut parent, a, b);
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for index in 0..candidates.len() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(index);
    }
    let mut groups: Vec<_> = groups.into_values().collect();
    groups.sort_by_key(|group| {
        group
            .iter()
            .map(|&index| candidates[index].stmt_index)
            .min()
            .unwrap_or(usize::MAX)
    });
    let mut planned = HashMap::new();
    for (plan_index, mut group) in groups.into_iter().enumerate() {
        if group.len() < 2
            || group.iter().enumerate().any(|(i, &a)| {
                group.iter().skip(i + 1).any(|&b| {
                    let a = &candidates[a];
                    let b = &candidates[b];
                    !(a.end < b.start || b.end < a.start)
                })
            })
        {
            continue;
        }
        group.sort_by_key(|&index| candidates[index].stmt_index);
        let root = format!("__jet_place_plan_{plan_index}_root");
        let len = format!("__jet_place_plan_{plan_index}_len");
        let mut regions = vec![SplitRegion {
            name: root.clone(),
            start: 0,
            end: None,
        }];
        for (step, candidate_index) in group.into_iter().enumerate() {
            let candidate = candidates[candidate_index].clone();
            let Some(region_index) = regions.iter().position(|region| {
                candidate.start >= region.start
                    && region.end.is_none_or(|end| candidate.end <= end)
            }) else {
                continue;
            };
            let region = regions.remove(region_index);
            let prefix = format!("__jet_place_plan_{plan_index}_{step}_before");
            let split_tail = format!("__jet_place_plan_{plan_index}_{step}_tail");
            let segment = format!("__jet_place_plan_{plan_index}_{step}_segment");
            let suffix = format!("__jet_place_plan_{plan_index}_{step}_after");
            if candidate.start > region.start {
                regions.push(SplitRegion {
                    name: prefix.clone(),
                    start: region.start,
                    end: Some(candidate.start - 1),
                });
            }
            if region.end.is_none_or(|end| candidate.end < end) {
                regions.push(SplitRegion {
                    name: suffix.clone(),
                    start: candidate.end + 1,
                    end: region.end,
                });
            }
            planned.insert(
                candidate.stmt_index,
                PlannedSplitView {
                    candidate,
                    initialize: step == 0,
                    root: root.clone(),
                    len: len.clone(),
                    source: region.name,
                    source_start: region.start,
                    before: prefix,
                    split_tail,
                    segment,
                    after: suffix,
                },
            );
        }
    }
    planned
}

pub(crate) fn lower_stmt(s: &Stmt, cx: &Cx, env: &mut LowerEnv) -> TStmt {
    if let Stmt::Assign { target, value, .. } = s {
        let root_name = match target {
            LValue::Local { name, .. } => Some(name.as_str()),
            LValue::Index { base, .. } | LValue::Field { base, .. } => {
                match base.as_ref() {
                    Expr::Ident(name, _) => Some(name.as_str()),
                    _ => None,
                }
            }
        };
        if let Some(name) = root_name.filter(|name| env.is_gc(name)) {
            let root = env.place_of(name);
            let edges = env.gc_edges_for_expr(value, Some(name));
            let slot = match target {
                LValue::Local { name, .. } => format!("local:{name}"),
                LValue::Field { field, .. } => format!("field:{field}"),
                LValue::Index { span, .. } => format!("index:{}", span.start),
            };
            let mut lowered_source = s.clone();
            let index_temp = if let (
                LValue::Index { index, span, .. },
                Stmt::Assign { target, .. },
            ) = (target, &mut lowered_source)
            {
                let lowered = lower_expr(index, cx, env);
                let source_name = format!("__jet_gc_index_{}", span.start);
                let rust_name = source_name.clone();
                let LValue::Index {
                    index: lowered_index,
                    ..
                } = target
                else {
                    unreachable!("matched index assignment")
                };
                *lowered_index = Box::new(Expr::Ident(source_name.clone(), *span));
                env.bind(
                    &source_name,
                    TLocal::generated(&rust_name),
                    Some(lowered.ty.clone()),
                );
                Some((rust_name, lowered))
            } else {
                None
            };
            let saved = env.locals.get(name).cloned();
            env.gc_locals.remove(name);
            env.bind(
                name,
                TLocal::generated("__jet_value").through_ref(),
                saved.as_ref().and_then(|(_, ty)| ty.clone()),
            );
            let stmt = lower_stmt(&lowered_source, cx, env);
            if let Some((slot, ty)) = saved {
                env.bind(name, slot, ty);
            }
            env.mark_gc(name);
            if let Some((temp, _)) = &index_temp {
                env.locals.remove(temp);
            }
            return TStmt::GcEdit {
                root,
                slot,
                edges,
                replace_all: matches!(target, LValue::Local { .. }),
                index_temp,
                stmt: Box::new(stmt),
            };
        }
    }
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
                binds.push((local_rust, field_rust));
                env.bind(
                    f.local_name(),
                    TLocal::user(f.local_name()),
                    field_tys.get(&f.name).cloned(),
                );
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
                binds.push((elem_rust, field_rust));
                env.bind(&e.name, TLocal::user(&e.name), Some(fty.clone()));
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
                elem_names.push(mangle(&e.name));
                env.bind(&e.name, TLocal::user(&e.name), elem_ty.clone());
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
            // D-UNINIT1 engine, reused unchanged by D-UNINIT-SENTINEL2: lower
            // `name := T.{ uninit }` to
            //   `let mut name: T = unsafe { std::mem::MaybeUninit::<T>::uninit().assume_init() };`
            // The source's `use core.mem` + `Type.{ uninit }` is the expert-tier opt-in (I1: no
            // `unsafe` in generated code without a source-level gate). Sema proved
            // write-before-read (E0420), so every subsequent read is post-write — the
            // `assume_init()` at declaration yields garbage bytes that are always
            // overwritten before any read. The `is_pod_uninit_type` guard in sema
            // (E0423) ensures T has no Drop glue, so no destructor ever reads the garbage.
            if b.uninit {
                let ty =
                    b.ty.as_ref()
                        .expect("E0421 ensures a `Type.{ uninit }` binding has a type");
                env.bind(&b.name, TLocal::user(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let mut",
                    let_ty: crate::Codegen::TIR::let_ty_for_opt(Some(ty), cx, false, false, false),
                    init: TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::Uninit,
                    },
                    track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
                };
            }
            // c109 Phase 19: an arena `view` binding (`x :: arena.alloc(v)`). The AST
            // `emit_let`'s `arena_view` branch emits `let <x> = <init>;` (NO type clause,
            // NEVER `let mut` — a view is a non-reassignable `&mut T`) and binds a DEREF'd
            // slot (reads go through `(*x)`). Reproduce it exactly: a `Let` with `kw: "let"`,
            // empty `ty_clause`, and a deref'd slot place `(*<x>)`.
            if b.arena_view {
                let init = lower_expr(&b.init, cx, env);
                env.bind(&b.name, TLocal::user(&b.name).through_ref(), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty: TLetTy::Inferred,
                    init,
                    track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
                };
            }
            // D-SHAPE-PLACE1=A: local place windows are references with no
            // written Rust type clause. Range windows already behave as slices;
            // whole/field/index windows bind a dereferenced transparent slot.
            if let Expr::Place(inner, _, _) = &b.init {
                let range = matches!(inner.as_ref(), Expr::Slice { .. });
                let init = lower_expr(&b.init, cx, env);
                let slot = if range {
                    TLocal::user(&b.name)
                } else {
                    TLocal::user(&b.name).through_ref()
                };
                env.bind(&b.name, slot, b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty: TLetTy::Inferred,
                    init,
                    track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
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
                env.bind(&b.name, TLocal::user(&b.name), Some(Type::String));
                env.mark_string_view(&b.name);
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty: TLetTy::StrView,
                    init,
                    track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
                };
            }
            // c109 (S57/M9.5): a comptime LOCAL `comptime name = expr`. The AST `emit_let`
            // builds `init` from `b.ct.serialize()` (the sema-evaluated value rendered to a
            // Rust literal) — the runtime `init` expr is never emitted. Reproduce it: a
            // verbatim `ConstInline` of the same serialized string, with `kw: "let"` (the
            // `(b.mutable && !b.is_comptime)` guard makes it `let`, never `let mut`) and the
            // type clause from `b.ty` (rendered exactly as the non-comptime path below). All
            // facts are pre-resolved (I3): no inference here.
            if b.is_comptime {
                let let_ty = crate::Codegen::TIR::let_ty_for_opt(b.ty.as_ref(), cx, false, false, false);
                let init = TExpr {
                    ty: b.ty.clone().unwrap_or(Type::Int),
                    kind: lower_comptime_scalar(b.ct.as_ref()).unwrap_or_else(|| {
                        b.ct
                            .as_ref()
                            .map(|v| TExprKind::CtLit(v.clone()))
                            .unwrap_or(TExprKind::DefaultLit)
                    }),
                };
                env.bind(&b.name, TLocal::user(&b.name), b.ty.clone());
                return TStmt::Let {
                    name: b.name.clone(),
                    kw: "let",
                    let_ty,
                    init,
                    track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
                };
            }
            let mut init = lower_owned_expr(&b.init, cx, env);
            // D-FIXARR1: if the binding annotation is `[T#N]` and the init lowered as a
            // growable list (e.g. a plain list literal), re-tag the TExpr type so the emit
            // produces a Rust array literal `[e1, …]` instead of `vec![…]`.
            if let Some(fl @ Type::FixedList { .. }) = &b.ty {
                if matches!(init.ty, Type::List(_)) && matches!(init.kind, TExprKind::ListLit(_)) {
                    init.ty = fl.clone();
                }
            }
            // D-UNIONTYPE1=A: member → union inject at the binding boundary.
            if let Some(want) = &b.ty {
                init = crate::Codegen::TIR::maybe_widen_expr_to_union(init, want);
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
            let is_resource = match &ty {
                Type::Named(name) | Type::Apply { name, .. } => cx.close_types.contains(name),
                _ => false,
            };
            if is_resource {
                init = TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::ResourceNew(Box::new(init)),
                };
            }
            // E2-M7/E2-M10/D-ALLOC1/D-ROUTE1: a handle binding forces `let mut` even
            // when bound immutably (its methods take `&mut self`). Mirror
            // `emit_let`'s `is_file_handle` set exactly.
            let is_file_handle = matches!(
                &ty,
                Type::Named(n) if n == "FileReader" || n == "FileWriter"
                    || n == "JSONReader" || n == "JSONWriter"
                    || n == "JSONLReader" || n == "JSONLWriter"
                    || n == "CSVReader" || n == "CSVWriter"
                    || n == "XMLReader" || n == "XMLWriter"
                    || n == "CBORReader" || n == "CBORWriter"
                    || n == "Stdout" || n == "Stderr"
                    || n == "TcpStream" || n == "UnixStream" || n == "HttpRouter"
                    || n == "Arena" || n == "Bump" || n == "Pool" || n == "Fixed"
            )
            // D-DATAFLOW1=A: DataStream.next / stream reducers take &mut.
            || matches!(
                &ty,
                Type::Apply { name, .. }
                    if name == "DataStream" && !cx.type_names.contains(name.as_str())
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
            let let_ty = crate::Codegen::TIR::let_ty_for_opt(
                b.ty.as_ref(),
                cx,
                mut_fn,
                is_resource,
                b.gc_promotion.is_some() || b.gc_transferred,
            );
            let track_origin = tracked_float_origin(b, &ty, cx);
            let binding_name = if is_resource {
                format!("__jet_resource_{}_{}", b.name, b.name_span.start)
            } else {
                b.name.clone()
            };
            let slot = if is_resource {
                TLocal::user(&binding_name).through_ref()
            } else {
                TLocal::user(&binding_name)
            };
            env.bind(&b.name, slot, Some(ty));
            if b.gc_promotion.is_some() || b.gc_transferred {
                env.mark_gc(&b.name);
            }
            if is_resource {
                env.mark_resource(&b.name);
            }
            TStmt::Let {
                name: binding_name,
                kw,
                let_ty,
                init,
                track_origin,
                gc_promotion: b.gc_promotion.clone(),
                gc_transferred: b.gc_transferred,
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
                    place: TPlace::Local(env.local_of(name)),
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
                // Sema-to-TIR handoff assert (ice_regressions b5 bug class): the
                // subset gate must have already excluded `IndexKind::Unknown` before
                // routing here — an `Unknown` default reaching lowering means sema
                // left an index kind unresolved and the gate missed it.
                let kind = if matches!(kind, IndexKind::Unknown) {
                    &IndexKind::List
                } else {
                    kind
                };
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
                    let elem_ty = value_t.ty.clone();
                    return TStmt::Assign {
                        place: TPlace::Expr(Box::new(TExpr {
                            ty: elem_ty,
                            kind: TExprKind::PoolSlot {
                                pool: Box::new(base_t),
                                id: Box::new(index_t),
                                mutable: true,
                                field: None,
                                line,
                            },
                        })),
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
                if let Expr::Index {
                    base: collection,
                    index,
                    kind,
                    span: index_span,
                } = base.as_ref()
                {
                    let is_map = matches!(kind, IndexKind::Map);
                    let index_proven = matches!(kind, IndexKind::FixedListProof);
                    if is_map
                        || index_proven
                        || matches!(kind, IndexKind::List)
                    {
                        let collection_t = lower_expr(collection, cx, env);
                        let elem_ty = match &collection_t.ty {
                            Type::List(elem) | Type::FixedList { elem, .. } => {
                                Some((**elem).clone())
                            }
                            Type::Map { value, .. } => Some((**value).clone()),
                            _ => None,
                        };
                        if let Some(elem_ty) = elem_ty {
                            let field_ty =
                                struct_field_type(cx, &elem_ty, field).unwrap_or(Type::Int);
                            let clone_value = if let Expr::Ident(vname, _) = value {
                                env.is_borrowed(vname)
                                    && env.ty_of(vname).is_some_and(|t| !t.is_scalar())
                            } else {
                                false
                            };
                            let line = crate::Diagnostics::span_line_col(
                                &cx.src,
                                index_span.start,
                            )
                            .0;
                            return TStmt::IndexFieldAssign(Box::new(TIndexFieldAssign {
                                base: collection_t,
                                index: lower_expr(index, cx, env),
                                is_map,
                                index_proven,
                                field: field.to_string(),
                                field_ty,
                                op: *op,
                                value: lower_expr(value, cx, env),
                                clone_value,
                                line,
                            }));
                        }
                    }
                }
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
                    let elem_ty = match &pool_t.ty {
                        Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                        _ => Type::Int,
                    };
                    let field_ty = struct_field_type(cx, &elem_ty, field).unwrap_or(Type::Int);
                    let place = TPlace::Expr(Box::new(TExpr {
                        ty: field_ty,
                        kind: TExprKind::PoolSlot {
                            pool: Box::new(pool_t),
                            id: Box::new(id_t),
                            mutable: true,
                            field: Some(field.to_string()),
                            line,
                        },
                    }));
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
                let place = TPlace::Expr(Box::new(lower_expr(&field_expr, cx, env)));
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
        Stmt::Return(Some(Expr::Ident(name, _)), _) if env.gc_return && env.is_gc(name) => {
            TStmt::Return(Some(TExpr {
                ty: env.ty_of(name).unwrap_or(Type::Int),
                kind: TExprKind::Local(env.local_of(name)),
            }))
        }
        Stmt::Return(Some(e), _) => {
            let mut value = lower_owned_expr(e, cx, env);
            if let Some(want) = &env.ret_ty {
                value = crate::Codegen::TIR::maybe_widen_expr_to_union(value, want);
            }
            TStmt::Return(Some(value))
        }
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
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::YieldSend {
                    value: Box::new(v),
                })),
            })
        }
        // D-IGNORERET2=A: `.drop("reason")` — lower only the receiver (for side effects).
        // The method call itself is erased; the "reason" string is audit-only.
        Stmt::Expr(Expr::Call(call)) if call.name == Syntax::INTERNAL_DEFER_CLOSE => {
            let close = call
                .args
                .first()
                .expect("parser creates one deferred close argument");
            let Expr::Call(close_call) = &close.expr else {
                unreachable!("parser creates a close call for deferred cleanup")
            };
            let Expr::Ident(resource, _) = &close_call.args[0].expr else {
                unreachable!("parser restricts deferred close to one resource binding")
            };
            TStmt::DeferClose {
                close: lower_expr(&close.expr, cx, env),
                resource: env.rust_name_of(resource),
                id: call.name_span.start,
            }
        }
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
            // The emitted outer Rust block owns the init binding and every loop-body
            // binding. Lower all of them in one child env so none survives the loop.
            let init_val = lower_expr(&init.init, cx, env);
            let init_ty = init.ty.clone();
            let mut scoped = clone_env(env);
            scoped.bind(&init.name, TLocal::user(&init.name), init_ty);
            let init_stmt = Box::new(TStmt::Let {
                name: init.name.clone(),
                kw: "let mut",
                let_ty: TLetTy::Inferred,
                init: init_val,
                track_origin: None,
                gc_promotion: None,
                gc_transferred: false,
            });
            let cond = lower_expr(cond, cx, &mut scoped);
            let step = step
                .as_ref()
                .map(|step| Box::new(lower_stmt(step.as_ref(), cx, &mut scoped)));
            TStmt::CountedLoop {
                label: label_name(label),
                init: init_stmt,
                cond,
                step,
                body: lower_stmts(body, cx, &mut scoped),
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
            ForKind::Range { start, end, step, exclusive } => {
                let start = lower_expr(start, cx, env);
                let end = lower_expr(end, cx, env);
                let step = step.as_ref().map(|s| lower_expr(s, cx, env));
                // The loop var is an `Int` local for the body's scope only. Panic
                // context inside the body sees it; a panic after the loop does not.
                let mut branch = clone_env(env);
                branch.bind(var, TLocal::user(var), Some(Type::Int));
                let lowered_body = lower_stmts(body, cx, &mut branch);
                TStmt::Range {
                    label: label_name(label),
                    var: var.clone(),
                    start,
                    end,
                    step,
                    exclusive: *exclusive,
                    body: lowered_body,
                }
            }
            // c109 Phase 5: collection iteration `loop x; coll` / `loop k, v; map`.
            // The collection string is resolved once. The loop var(s) bind in the body
            // scope with an *unresolved* type (`None`) — matching the AST slot's
            // `jet_ty: None`, so they never enable the overflow trap (parity).
            ForKind::In { collection, step } => {
                // c109 Phase 22: classify a method-call collection into the matching
                // `emit_for_in` branch (`chars`/`lines`/the `.iter().cloned()` default),
                // resolving the receiver/collection string off the SAME node shape the
                // AST path reads. `method_kind == None` is the plain `.iter()` form.
                let (iter_source, method_kind) = lower_forin_collection(collection, cx, env);
                // Infer the element type from the lowered collection so the loop
                // variable binds with its concrete type. This lets `core_struct_field_rust_name`
                // emit plain field names (not `user_<field>`) for core types like DirEntry.
                let lowered_coll = lower_expr(collection, cx, env);
                let mut method_kind = method_kind;
                let mut coll_elem_ty: Option<Type> = match &lowered_coll.ty {
                    Type::List(inner) => Some((**inner).clone()),
                    Type::FixedList { elem, .. } => Some((**elem).clone()),
                    Type::Map { key, value, .. } => Some(Type::Tuple(vec![
                        ("key".to_string(), Box::new((**key).clone())),
                        ("value".to_string(), Box::new((**value).clone())),
                    ])),
                    // D-STREAMYIELD1: a generator's `Stream<T>`.
                    Type::Apply { name, args } if name == "Stream" && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    // D-ITERTOOLS1=A: lazy `Iter<T>` view — element is T.
                    Type::Apply { name, args }
                        if name == crate::Syntax::TYPE_ITER && args.len() == 1 =>
                    {
                        Some(args[0].clone())
                    }
                    Type::Named(name) if name == "HttpBodyChunks" => Some(Type::Result {
                        ok: Box::new(Type::List(Box::new(Type::Named("U8".to_string())))),
                        err: Box::new(Type::Named("HttpError".to_string())),
                    }),
                    // D-DYNARRAY1: `loop x; window` — a `View<T>`'s element type.
                    Type::Apply { name, args }
                        if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 => {
                        Some(args[0].clone())
                    }
                    _ => None,
                };
                let by_value = matches!(&lowered_coll.ty,
                    Type::Apply { name, .. } if name == "Stream" || name == crate::Syntax::TYPE_ITER
                ) || matches!(&lowered_coll.ty, Type::Named(name) if name == "HttpBodyChunks");
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
                branch.bind(var, TLocal::user(var), coll_elem_ty.clone());
                if let Some((v2, _)) = var2 {
                    // Two-binding: map → key/value; sequence → index/item (D-RANGE-EXCL1=C).
                    match &lowered_coll.ty {
                        Type::Map { key, value, .. } => {
                            branch.bind(var, TLocal::user(var), Some((**key).clone()));
                            branch.bind(v2, TLocal::user(v2), Some((**value).clone()));
                        }
                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                            branch.bind(var, TLocal::user(var), Some(Type::Int));
                            branch.bind(v2, TLocal::user(v2), Some((**inner).clone()));
                        }
                        Type::Apply { name, args }
                            if matches!(name.as_str(), "View" | "ViewMut") && args.len() == 1 =>
                        {
                            branch.bind(var, TLocal::user(var), Some(Type::Int));
                            branch.bind(v2, TLocal::user(v2), Some(args[0].clone()));
                        }
                        _ => {
                            branch.bind(v2, TLocal::user(v2), None);
                        }
                    }
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
                    source: iter_source,
                    collection: lowered_coll,
                    step: step.as_ref().map(|step| lower_expr(step, cx, env)),
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
            span,
        } => lower_switch(subject, arms, else_body, *span, cx, env),
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
        // Lexical-scope rule: whenever `emit_tir_stmt` opens a Rust `{ ... }`, lower
        // declarations in a cloned env. Only three statement forms deliberately reuse
        // the parent env because emission is inline with no Rust block: selected
        // comptime-if, `layout`, and `.setup`.
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
        // c109 Phase 18: an audited `#Unsafe { … }` region (`Stmt::Unsafe`). Emission
        // adds a Rust lexical block, so lower its declarations in a child env. The `#Audit("…")`
        // annotation is dropped (codegen is dumb — it emits nothing, matching the AST).
        // I1: the source `#Unsafe` gate is 1:1 with this node, the only producer of a
        // Rust `unsafe` block.
        Stmt::Unsafe { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Unsafe(lower_stmts(body, cx, &mut scoped))
        }
        // D-CTEFFECT1: `#Impure` erases to a plain block at codegen (comptime-only gate, I3).
        Stmt::Impure { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // D-REACTCORE1: `#Reactive { … }` lowers to `jet_reactive_effect(closure)`.
        // Clone outer captures into the closure (same as a stored lambda).
        Stmt::Reactive { body, .. } => {
            let closure = render_reactive_block_closure(body, cx, env);
            TStmt::Reactive { closure }
        }
        // D-SHIELDNAME1=A: `#Shield { … }` lowers to a shield-guarded lexical block.
        Stmt::Shield { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Shield {
                body: lower_stmts(body, cx, &mut scoped),
            }
        }
        // c109 Phase 19: an explicit `region r { … }` (D-REGION1) emits a plain
        // Rust lexical block.
        Stmt::Region { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        Stmt::Policy { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // D-TASKSCOPE1=A: taskgroup erases to a plain block at codegen (I3).
        Stmt::TaskGroup { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // D-LAYOUT1 / D-LAYOUT-GATES1: `layout name { … }` needs a REAL
        // runtime object (unlike Region/TaskGroup, which erase) — bind `name`
        // to a fresh `jet_layout::Handle` BEFORE lowering the body, so the
        // desugared `name.h(box, anchor)` calls inside resolve to it, exactly
        // like an ordinary `name :: jet_layout::Handle::new(…)` binding would.
        Stmt::Layout { name, body, .. } => {
            let handle = TLocal::user(name);
            env.bind(
                name,
                handle.clone(),
                Some(Type::Named(Syntax::LAYOUT_HANDLE_TYPE.to_string())),
            );
            let lowered_body = lower_stmts(body, cx, env);
            TStmt::Layout {
                handle,
                label: name.clone(),
                body: lowered_body,
            }
        }
        // c109 Phase 26: a `#Caps(Io) { … }` effect-restriction region (D-EFF1). `emit_stmt`'s
        // `Stmt::Caps` arm is byte-for-byte `Stmt::Region`; effects erase at codegen (I3).
        Stmt::Caps { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // D-SCAP1: a `#grant(Fs) { caps -> … }` grant region. The capability handle
        // is a compile-time-only fact (authority to perform the granted effects),
        // erased here (I3); the body emits as a plain lexical `TStmt::Region`.
        // No runtime grant/revoke value, no `unsafe`.
        Stmt::Grant { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // c109 Phase 19: a `#Context(field: value) { … }` block (D-CTX1/D-DEADLINE1).
        // Resolve each field against the outer env, then lower the guarded Rust block
        // in a lexical child env.
        Stmt::ContextBlock { fields, body, .. } => {
            let guards = fields
                .iter()
                .map(|(name, v, _)| (name.clone(), lower_expr(v, cx, env)))
                .collect();
            let mut scoped = clone_env(env);
            TStmt::ContextBlock {
                guards,
                body: lower_stmts(body, cx, &mut scoped),
            }
        }
        // D-TERM1: `live { … }` emits an enter/guard/leave Rust lexical block.
        Stmt::Live { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Live {
                body: lower_stmts(body, cx, &mut scoped),
            }
        }
        // D-DOTSCOPE1: a `#Test` scope member (`.setup`/`.expect_fail`/`.timeout`/
        // `.skip`). Legality/args were checked in sema; here we pick the lowering
        // kind and fold `.timeout`'s duration literal to a nanosecond budget.
        // `.setup` emits inline, so its bindings are visible to the rest of the test;
        // the others open their own scope in
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
            let lowered_body = if matches!(&kind, ScopeMemberKind::Setup) {
                // `.setup` is emitted inline so its declarations intentionally remain
                // available to later statements in the test.
                lower_stmts(body, cx, env)
            } else {
                let mut scoped = clone_env(env);
                lower_stmts(body, cx, &mut scoped)
            };
            TStmt::ScopeMember {
                kind,
                body: lowered_body,
            }
        }
        // D-DET1: `assume_deterministic { … }` erases to a plain `TStmt::Region`
        // (byte-for-byte the `Stmt::Region`/`Stmt::Caps` shape). The determinism
        // suspension is a sema-only fact; nothing runtime, no `unsafe` (I3).
        Stmt::AssumeDet { body, .. } => {
            let mut scoped = clone_env(env);
            TStmt::Region(lower_stmts(body, cx, &mut scoped))
        }
        // D-TXN1–D-TXN4 (ratified 2026-06-24): `#Transact(name) { … }` block. Bind the
        // handle (typed `Transaction`) in a child env so `name.on_commit(…)` lowers
        // against it without escaping the emitted Rust block. The
        // `let mut <handle> = jet_transaction(); … <handle>.commit();` framing is
        // emitted in `emit_tir_stmt`; codegen is dumb (I3).
        Stmt::Transact { name, body, .. } => {
            let mut scoped = clone_env(env);
            let handle = name.as_ref().map(|name| {
                let slot = TLocal::user(name);
                scoped.bind(
                    name,
                    slot.clone(),
                    Some(Type::Named(Syntax::TXN_HANDLE_TYPE.to_string())),
                );
                slot
            });
            // D-TXN-ROLLBACK layer 1 (auto-snapshot): collect the root local names
            // assigned anywhere in the block (recursing into nested control flow, but
            // NOT into nested `#Transact` blocks or lambda bodies — those own their
            // own rollback scope / are deferred). Snapshot only roots ALREADY in scope
            // at block entry (params / outer locals): a local declared inside the block
            // needs no snapshot, since rollback discards it when the block scope ends.
            let mut roots: Vec<String> = Vec::new();
            collect_txn_mut_roots(body, &mut roots);
            let snapshots: Vec<(TLocal, Option<Type>)> = roots
                .iter()
                .filter(|r| env.locals.contains_key(*r))
                .map(|r| {
                    // D-TXN-ROLLBACK layer 2: if the root type implements Rollback,
                    // use snapshot_custom instead of the clone-based snapshot path.
                    let rollback_ty = env.ty_of(r).filter(|ty| {
                        matches!(ty, Type::Named(n) if cx.rollback_types.contains(n))
                    });
                    (env.local_of(r), rollback_ty)
                })
                .collect();
            // D-STM1=A (card #506): lower the body with `in_stm_transact` raised so a
            // `Shared<T>.edit` inside routes to the deferred `edit_txn`. `stm_touched`
            // is reset first and read after, so `uses_stm` reflects THIS block only
            // (save/restore isolates nested blocks); a Shared edit in a nested
            // `#Transact` attaches to that inner block's own transaction, not this one.
            let prev_in = cx.in_stm_transact.replace(true);
            let prev_touched = cx.stm_touched.replace(false);
            let lowered_body = lower_stmts(body, cx, &mut scoped);
            let uses_stm = cx.stm_touched.get();
            cx.in_stm_transact.set(prev_in);
            cx.stm_touched.set(prev_touched);
            TStmt::Transact {
                handle,
                snapshots,
                uses_stm,
                body: lowered_body,
            }
        }
        // Forward-safety default: a Stmt variant not in the subset never reaches
        // lowering (`stmt_in_subset` returns false for it). Kept as a guard against a
        // future variant; currently unreachable because every covered variant is matched.
        #[allow(unreachable_patterns)]
        _ => unreachable!("statement not in TIR subset"),
    }
}

/// W4 (durability): proves the sema-to-TIR handoff `debug_assert`s in
/// `lower_expr`'s `Expr::Index` arm and this file's `LValue::Index` arm
/// actually trip on a leaked `IndexKind::Unknown` — the exact ice_regressions
/// b5 bug class (sema left the index kind unresolved; the subset gate is
/// supposed to exclude it, but a gate bug could let one through). These are
/// `#[should_panic]` because the debug_assert is the thing under test, not a
/// normal lowering path — the subset gate itself still excludes `Unknown` in
/// every real compile.
#[cfg(test)]
mod handoff_assert_tests {
    use super::*;

    fn empty_cx() -> Cx {
        let src = "fn run() {}\n";
        let (toks, lex_diags) = crate::Lexer::lex(src);
        assert!(lex_diags.is_empty(), "lex errors: {lex_diags:?}");
        let prog = crate::Parser::parse(&toks).expect("parse failed");
        build_cx(&prog, src, "test.jet")
    }

    #[test]
    #[should_panic(expected = "sema-to-TIR handoff violated")]
    fn index_read_unknown_kind_trips_handoff_assert() {
        let cx = empty_cx();
        let mut env = LowerEnv::new("run".to_string());
        let idx_expr = Expr::Index {
            base: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
            index: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
            span: Span::new(0, 0),
            kind: IndexKind::Unknown, // seeded leak: sema never resolved this
        };
        let _ = lower_expr(&idx_expr, &cx, &mut env);
    }

    #[test]
    #[should_panic(expected = "sema-to-TIR handoff violated")]
    fn index_assign_unknown_kind_trips_handoff_assert() {
        let cx = empty_cx();
        let mut env = LowerEnv::new("run".to_string());
        let stmt = Stmt::Assign {
            target: LValue::Index {
                base: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
                index: Box::new(Expr::Int(0, Span::new(0, 0), None, None)),
                span: Span::new(0, 0),
                kind: IndexKind::Unknown, // seeded leak: sema never resolved this
            },
            op: None,
            op_span: Span::new(0, 0),
            value: Expr::Int(1, Span::new(0, 0), None, None),
        };
        let _ = lower_stmt(&stmt, &cx, &mut env);
    }
}
