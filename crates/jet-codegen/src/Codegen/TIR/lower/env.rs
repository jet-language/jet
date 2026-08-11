use crate::AST::{Expr, LValue, Stmt, Type};
use crate::Codegen::TIR::{TBindingOrigin, TLocal};
use crate::Syntax;
use std::collections::HashMap;
use std::collections::HashSet;
use std::cell::RefCell;
use std::rc::Rc;

/// Per-function lowering environment: a local name -> (structured slot, type).
/// Built from params, extended by `let` bindings. The slot already accounts for
/// parameter deref and binding provenance, so every TIR consumer sees the same
/// local facts.
///
/// The type is `Option<Type>`: a binding can carry a *resolved* type, or `None`
/// when the AST path's slot had `jet_ty: None` and we must reproduce that
/// partiality. The load-bearing case (c109 Phase 5) is a `loop x; coll`
/// iteration variable: `emit_for_in` binds its slot with `jet_ty: None`, so
/// `operand_is_integer`/`expr_jet_ty` resolve the var to `None` and it never
/// enables the overflow trap. Carrying `Some(elem_ty)` here would diverge —
/// `x + 1` would wrongly trap. So the iteration var is stored as `None`,
/// matching the AST path bit-for-bit (the Phase-3 "reproduce the AST's
/// partiality where it is load-bearing" lesson, again).

#[derive(Clone)]
pub(crate) struct LowerEnv {
    pub(super) locals: HashMap<String, (TLocal, Option<Type>)>,
    /// c109 Phase 8: the enclosing function's unmangled Jet name, used by a `?`
    /// (`TExprKind::Try`) to embed the trace-frame function name — exactly the value
    /// the AST path reads from `cx.current_fn` at emit time (set to `f.name`).
    pub(super) fn_name: String,
    /// D-UNIONTYPE1=A: enclosing function return type, for member→union inject
    /// at `return` / `Ok` / `Err` / `?` boundaries.
    pub(super) ret_ty: Option<Type>,
    /// D-FIELDPOL1: the owning struct name when lowering an inherent/trait
    /// method (`None` for a free function). `self`'s own env type is
    /// deliberately `None` (see `bind` above), so a `self.field` read can't
    /// resolve its receiver struct through `recv.ty` the way `x.field` does —
    /// this is the one place that struct name is available, used only to
    /// check `cx.computed_fields` (whether `self.field` needs a getter call).
    pub(super) self_owner: Option<String>,
    /// D-MEM1 stage S5: names bound by a `Binding.string_view` init — the Rust
    /// place is a plain `&str` (not the `String` its `Type::String` would
    /// ordinarily lower to). Consulted only by `Expr::Copy` lowering, which
    /// must materialize a view with `.to_string()` (an owned `String`), not
    /// the ordinary `.clone()` (which on a `&str` would just hand back
    /// another `&str` — the wrong Rust type for a `copy` result that needs to
    /// escape the view's scope).
    pub(super) string_view_locals: HashSet<String>,
    pub(super) borrowed_locals: HashSet<String>,
    pub(super) resource_locals: HashSet<String>,
    pub(super) gc_locals: HashSet<String>,
    /// Fixed-list locals created with `Type.{ uninit }`. TIR marks their
    /// index writes so AOT can use the vetted `jet_mem` storage wrapper.
    pub(super) uninit_fixed_locals: HashSet<String>,
    pub(super) gc_return: bool,
    /// D-TASKBORROW1=A: split-view locals and the handle type engines other
    /// than AOT hold them as. AOT keeps a real Rust reference; the JIT keeps a
    /// `ViewMut`/`View` window record, and a spawned task capturing one must be
    /// typed as that window or the child reads the wrong shape.
    pub(super) split_view_handles: HashMap<String, Type>,
    /// Operand types that lowering materializes with Rust `.clone()`. Generic
    /// function emission uses this to add `Clone` only where the body needs it.
    pub(super) cloned_types: Rc<RefCell<Vec<Type>>>,
    /// Function locals whose value crosses the native interrupt boundary.
    pub(super) send_fn_locals: HashSet<String>,
    /// Compiler-private default references resolve to the declaration-slot
    /// temporary made by source-order lowering.
    pub(super) binder_refs: HashMap<String, (String, Type)>,
}

impl LowerEnv {
    /// A fresh root env for a function/method body.
    pub(crate) fn new(fn_name: String) -> LowerEnv {
        LowerEnv {
            locals: HashMap::new(),
            fn_name,
            ret_ty: None,
            self_owner: None,
            string_view_locals: HashSet::new(),
            borrowed_locals: HashSet::new(),
            resource_locals: HashSet::new(),
            gc_locals: HashSet::new(),
            uninit_fixed_locals: HashSet::new(),
            gc_return: false,
            split_view_handles: HashMap::new(),
            cloned_types: Rc::new(RefCell::new(Vec::new())),
            send_fn_locals: HashSet::new(),
            binder_refs: HashMap::new(),
        }
    }
    /// Record the non-AOT handle type for a split-view local (see the field).
    pub(super) fn mark_split_view(&mut self, name: &str, handle: Type) {
        self.split_view_handles.insert(name.to_string(), handle);
    }
    pub(super) fn split_view_handle(&self, name: &str) -> Option<Type> {
        self.split_view_handles.get(name).cloned()
    }
    /// D-MEM1 stage S5: mark `name` as a string-view local (see `string_view_locals`).
    pub(super) fn mark_string_view(&mut self, name: &str) {
        self.string_view_locals.insert(name.to_string());
    }
    /// D-MEM1 stage S5: true if `name` is a live string-view local.
    pub(super) fn is_string_view_local(&self, name: &str) -> bool {
        self.string_view_locals.contains(name)
    }
    pub(super) fn mark_resource(&mut self, name: &str) {
        self.resource_locals.insert(name.to_string());
    }
    pub(super) fn mark_borrowed(&mut self, name: &str) {
        self.borrowed_locals.insert(name.to_string());
    }
    pub(super) fn is_resource(&self, name: &str) -> bool {
        self.resource_locals.contains(name)
    }
    pub(super) fn mark_gc(&mut self, name: &str) {
        self.gc_locals.insert(name.to_string());
    }
    pub(super) fn is_gc(&self, name: &str) -> bool {
        self.gc_locals.contains(name)
    }
    pub(super) fn mark_uninit_fixed(&mut self, name: &str) {
        self.uninit_fixed_locals.insert(name.to_string());
    }
    pub(super) fn is_uninit_fixed(&self, name: &str) -> bool {
        self.uninit_fixed_locals.contains(name)
    }
    pub(super) fn mark_send_fn(&mut self, name: &str) {
        self.send_fn_locals.insert(name.to_string());
    }
    pub(super) fn is_send_fn(&self, name: &str) -> bool {
        self.send_fn_locals.contains(name)
    }
    pub(super) fn binder_ref(&self, name: &str) -> Option<&(String, Type)> {
        self.binder_refs.get(name)
    }
    pub(super) fn gc_edges_for_expr(&self, expr: &Expr, exclude: Option<&str>) -> Vec<String> {
        let mut names = self.gc_locals.iter().collect::<Vec<_>>();
        names.sort();
        names
            .into_iter()
            .filter(|name| exclude != Some(name.as_str()))
            .filter(|name| gc_expr_references_ident(expr, name))
            .map(|name| format!("{}.id()", self.place_of(name)))
            .collect()
    }
    pub(super) fn note_clone(&mut self, ty: &Type) {
        self.cloned_types.borrow_mut().push(ty.clone());
    }
    /// Bind `name` to its resolved Rust place + type. The same lexical map drives
    /// expression resolution and rich-panic locals, so an out-of-scope branch binding
    /// can never be captured in generated Rust.
    pub(crate) fn bind(&mut self, name: &str, slot: TLocal, ty: Option<Type>) {
        self.locals.insert(name.to_string(), (slot, ty));
    }
    /// The structured slot for `name`. This is the single fact every engine
    /// resolves a local by; the Rust spellings below derive from it.
    pub(super) fn local_of(&self, name: &str) -> TLocal {
        match self.locals.get(name) {
            Some((slot, _)) => slot.clone(),
            None => TLocal::user(name),
        }
    }
    pub(super) fn origin_of(&self, name: &str) -> Option<TBindingOrigin> {
        self.locals
            .get(name)
            .and_then(|(slot, _)| slot.origin.clone())
    }
    pub(super) fn place_of(&self, name: &str) -> String {
        self.local_of(name).rust_place()
    }
    pub(super) fn ty_of(&self, name: &str) -> Option<Type> {
        self.locals.get(name).and_then(|(_, t)| t.clone())
    }
    /// c109 Phase 4: a name reads as a borrow when its slot is a deref — a
    /// by-reference parameter. The match lowering clones such a subject so the
    /// `match` owns the value, mirroring `emit_pattern_match_switch`.
    pub(super) fn is_borrowed(&self, name: &str) -> bool {
        self.borrowed_locals.contains(name)
            || matches!(self.locals.get(name), Some((slot, _)) if slot.deref)
    }
    /// The bare Rust binding name (without the deref wrapper), e.g. `user_light`
    /// for a by-reference slot. Used by the match-subject clone, which clones the
    /// borrow itself (`(user_light).clone()`), not `(*user_light)`.
    pub(super) fn rust_name_of(&self, name: &str) -> String {
        self.local_of(name).rust_name()
    }
}

fn gc_expr_references_ident(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Ident(candidate, _) => candidate == name,
        Expr::Call(call) => call
            .args
            .iter()
            .any(|arg| gc_expr_references_ident(&arg.expr, name)),
        Expr::MethodCall { receiver, args, .. }
        | Expr::CallValue {
            callee: receiver,
            args,
            ..
        } => {
            gc_expr_references_ident(receiver, name)
                || args
                    .iter()
                    .any(|arg| gc_expr_references_ident(&arg.expr, name))
        }
        Expr::Binary(_, left, right, _) => {
            gc_expr_references_ident(left, name) || gc_expr_references_ident(right, name)
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Field(inner, _, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _) => gc_expr_references_ident(inner, name),
        Expr::OptField { base, .. } => gc_expr_references_ident(base, name),
        Expr::Index { base, index, .. } => {
            gc_expr_references_ident(base, name) || gc_expr_references_ident(index, name)
        }
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            gc_expr_references_ident(base, name)
                || range.as_deref().map_or_else(
                    || {
                        gc_expr_references_ident(start, name)
                            || gc_expr_references_ident(end, name)
                    },
                    |range| gc_expr_references_ident(range, name),
                )
        }
        Expr::Range { start, end, .. } => {
            gc_expr_references_ident(start, name) || gc_expr_references_ident(end, name)
        }
        Expr::ListLit(items, _) => items
            .iter()
            .any(|item| gc_expr_references_ident(item, name)),
        Expr::MapLit(pairs, _) => pairs.iter().any(|(key, value)| {
            gc_expr_references_ident(key, name) || gc_expr_references_ident(value, name)
        }),
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, value)| gc_expr_references_ident(value, name)),
        Expr::TypedLit { body, .. } => {
            let mut hit = false;
            body.for_each_expr(|value| {
                if gc_expr_references_ident(value, name) {
                    hit = true;
                }
            });
            hit
        }
        Expr::TupleLit(fields, _, _) => fields
            .iter()
            .any(|(_, value)| gc_expr_references_ident(value, name)),
        Expr::EnumLit { args, .. } => args.iter().any(|arg| match arg {
            crate::AST::EnumLitArg::Positional(value)
            | crate::AST::EnumLitArg::Named { expr: value, .. } => {
                gc_expr_references_ident(value, name)
            }
        }),
        Expr::Str(parts, _) => parts.iter().any(|part| match part {
            crate::AST::StrPart::Interp(value, _) => gc_expr_references_ident(value, name),
            crate::AST::StrPart::Lit(_) => false,
        }),
        _ => false,
    }
}

/// D-DOTSCOPE1: fold a `.timeout(<dur>)` argument (a bare unit literal, sema-
/// validated) to a nanosecond budget. Falls back to 0 on the impossible shape.
pub(super) fn timeout_nanos(args: &[Expr]) -> u64 {
    if let Some(Expr::UnitLit {
        int, float, suffix, ..
    }) = args.first()
    {
        if let Some(mult) = Syntax::duration_suffix_nanos(suffix) {
            let nanos: u128 = if let Some(i) = int {
                (*i as u128).saturating_mul(mult)
            } else if let Some(f) = float {
                (*f * mult as f64) as u128
            } else {
                0
            };
            return nanos.min(u64::MAX as u128) as u64;
        }
    }
    0
}

/// D-TXN-ROLLBACK layer 1: collect the root local names that are *assigned* anywhere
/// in a `#Transact` body — `x = …`, `x += …`, `x.f = …`, `x[i] = …` — so each can be
/// auto-snapshotted at block entry and restored on a `?`-failure. Recurses through
/// nested control flow (if/while/for/switch/loop/region/etc.) but stops at:
///   • nested `#Transact` blocks — they establish their own rollback scope; and
///   • lambda bodies — a deferred execution context (the same reason `on_commit`
///     lambdas escape the enclosing transaction's effect check).
/// Each root is recorded once, in first-seen order. v1 covers assignment targets,
/// the clearly-analyzable, fully-correct case; mutation reached *only* through a
/// `&self` method call (no assignment) or a deep alias is the documented deferred
/// corner (D-TXN-ROLLBACK). This is a syntactic over-approximation filtered by the
/// caller to roots in scope at block entry.
pub(super) fn collect_txn_mut_roots(body: &[Stmt], out: &mut Vec<String>) {
    fn push(out: &mut Vec<String>, name: &str) {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    /// The root local ident of an assignable place, if any.
    fn lvalue_root(lv: &LValue) -> Option<&str> {
        match lv {
            LValue::Local { name, .. } => Some(name),
            LValue::Index { base, .. } | LValue::Field { base, .. } => expr_root(base),
        }
    }
    fn expr_root(e: &Expr) -> Option<&str> {
        match e {
            Expr::Ident(name, _) => Some(name),
            Expr::Field(base, _, _) => expr_root(base),
            Expr::Index { base, .. } => expr_root(base),
            _ => None,
        }
    }
    for s in body {
        match s {
            Stmt::Assign { target, .. } => {
                if let Some(root) = lvalue_root(target) {
                    push(out, root);
                }
            }
            // D-CANVASSTATE1=D: an `#Off` body emits nothing.
            Stmt::Switched { marker, .. } if crate::AST::switched_off(marker) => {}
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::CountedLoop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_txn_mut_roots(body, out),
            Stmt::Switch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    collect_txn_mut_roots(&arm.body, out);
                }
                if let Some(eb) = else_body {
                    collect_txn_mut_roots(eb, out);
                }
            }
            // D-META-STAGE1=B (formerly D-CTMARKER1): build-time block erases; no runtime mutations.
            Stmt::ComptimeBlock { .. } => {}
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                collect_txn_mut_roots(then_body, out);
                if let Some(eb) = else_body {
                    collect_txn_mut_roots(eb, out);
                }
            }
            // A nested `#Transact` owns its own rollback scope — don't pull its
            // mutations up into the enclosing block.
            Stmt::Transact { .. } => {}
            // Other statements (Expr/Val/Return/Break/…) introduce no assignment
            // targets we snapshot at block entry. (A `&self` mutating method call
            // hides inside `Stmt::Expr` — the documented deferred corner.)
            _ => {}
        }
    }
}
