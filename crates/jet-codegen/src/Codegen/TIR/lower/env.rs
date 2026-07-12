/// Per-function lowering environment: a local name -> (Rust place string, type).
/// Built from params, extended by `let` bindings. The "place" already accounts
/// for parameter deref, so `Local` emission needs no further resolution.
///
/// The type is `Option<Type>`: a binding can carry a *resolved* type, or `None`
/// when the AST path's slot had `jet_ty: None` and we must reproduce that
/// partiality. The load-bearing case (c109 Phase 5) is a `loop x in coll`
/// iteration variable: `emit_for_in` binds its slot with `jet_ty: None`, so
/// `operand_is_integer`/`expr_jet_ty` resolve the var to `None` and it never
/// enables the overflow trap. Carrying `Some(elem_ty)` here would diverge —
/// `x + 1` would wrongly trap. So the iteration var is stored as `None`,
/// matching the AST path bit-for-bit (the Phase-3 "reproduce the AST's
/// partiality where it is load-bearing" lesson, again).

pub(crate) struct LowerEnv {
    locals: HashMap<String, (String, Option<Type>)>,
    /// c109 Phase 8: the enclosing function's unmangled Jet name, used by a `?`
    /// (`TExprKind::Try`) to embed the trace-frame function name — exactly the value
    /// the AST path reads from `cx.current_fn` at emit time (set to `f.name`).
    fn_name: String,
    /// D-FIELDPOL1: the owning struct name when lowering an inherent/trait
    /// method (`None` for a free function). `self`'s own env type is
    /// deliberately `None` (see `bind` above), so a `self.field` read can't
    /// resolve its receiver struct through `recv.ty` the way `x.field` does —
    /// this is the one place that struct name is available, used only to
    /// check `cx.computed_fields` (whether `self.field` needs a getter call).
    self_owner: Option<String>,
    /// D-MEM1 stage S5: names bound by a `Binding.string_view` init — the Rust
    /// place is a plain `&str` (not the `String` its `Type::String` would
    /// ordinarily lower to). Consulted only by `Expr::Copy` lowering, which
    /// must materialize a view with `.to_string()` (an owned `String`), not
    /// the ordinary `.clone()` (which on a `&str` would just hand back
    /// another `&str` — the wrong Rust type for a `copy` result that needs to
    /// escape the view's scope).
    string_view_locals: HashSet<String>,
}

impl LowerEnv {
    /// A fresh root env for a function/method body.
    fn new(fn_name: String) -> LowerEnv {
        LowerEnv {
            locals: HashMap::new(),
            fn_name,
            self_owner: None,
            string_view_locals: HashSet::new(),
        }
    }
    /// D-MEM1 stage S5: mark `name` as a string-view local (see `string_view_locals`).
    fn mark_string_view(&mut self, name: &str) {
        self.string_view_locals.insert(name.to_string());
    }
    /// D-MEM1 stage S5: true if `name` is a live string-view local.
    fn is_string_view_local(&self, name: &str) -> bool {
        self.string_view_locals.contains(name)
    }
    /// Bind `name` to its resolved Rust place + type. The same lexical map drives
    /// expression resolution and rich-panic locals, so an out-of-scope branch binding
    /// can never be captured in generated Rust.
    fn bind(&mut self, name: &str, place: String, ty: Option<Type>) {
        self.locals.insert(name.to_string(), (place, ty));
    }
    fn place_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
    fn ty_of(&self, name: &str) -> Option<Type> {
        self.locals.get(name).and_then(|(_, t)| t.clone())
    }
    /// c109 Phase 4: a name reads as a borrow when its resolved place is a deref
    /// (`(*name)`) — a by-reference parameter slot. The match lowering clones such
    /// a subject so the `match` owns the value, mirroring `emit_pattern_match_switch`.
    fn is_borrowed(&self, name: &str) -> bool {
        matches!(self.locals.get(name), Some((place, _)) if place.starts_with("(*"))
    }
    /// The bare Rust binding name (without the deref wrapper), e.g. `user_light`
    /// for a slot whose place is `(*user_light)`. Used by the match-subject clone,
    /// which clones the borrow itself (`(user_light).clone()`), not `(*user_light)`.
    fn rust_name_of(&self, name: &str) -> String {
        match self.locals.get(name) {
            Some((place, _)) if place.starts_with("(*") && place.ends_with(')') => {
                place[2..place.len() - 1].to_string()
            }
            Some((place, _)) => place.clone(),
            None => mangle(name),
        }
    }
}

/// D-DOTSCOPE1: fold a `.timeout(<dur>)` argument (a bare unit literal, sema-
/// validated) to a nanosecond budget. Falls back to 0 on the impossible shape.
fn timeout_nanos(args: &[Expr]) -> u64 {
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
fn collect_txn_mut_roots(body: &[Stmt], out: &mut Vec<String>) {
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
            Stmt::If(ifs) => walk_if(ifs, out),
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::CountedLoop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::SuppressMustUse { body, .. }
            | Stmt::DebugOnly { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::Caps { body, .. }
            | Stmt::Grant { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. } => collect_txn_mut_roots(body, out),
            Stmt::Off { .. } => {}
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
            // D-CTMARKER1: build-time block erases; no runtime mutations.
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
    fn walk_if(ifs: &crate::AST::IfStmt, out: &mut Vec<String>) {
        collect_txn_mut_roots(&ifs.then_body, out);
        match &ifs.else_branch {
            Some(crate::AST::ElseBranch::ElseIf(inner)) => walk_if(inner, out),
            Some(crate::AST::ElseBranch::Else(body)) => collect_txn_mut_roots(body, out),
            None => {}
        }
    }
}
