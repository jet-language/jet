use super::*;

/// c109 Phase 6: is this `Expr::MethodCall` inside the subset? Two shapes only:
/// the synthetic `.clone()`, or a user-defined instance method on a covered type.
pub(crate) fn method_call_in_subset(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    // Shape (a): the sema-inserted `.clone()`. It takes no args; the receiver is an
    // owning field read / borrowed value, which must itself be in-subset. The AST
    // path emits `(recv).clone()` unconditionally (no `recv_type` needed) — match it.
    if method == "clone" {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    // D-TOOL4: `expect(x).snapshot()` — snapshot assertion. Receiver is a Call to
    // the ambient `expect` builtin (not a user fn / local). Zero method args; the
    // wrapped value must itself be in-subset. Lowers to `jet_expect(…).snapshot(…)?`.
    // Other `.snapshot()` shapes (user methods, Rollback) fall through below.
    if method == Syntax::BUILTIN_SNAPSHOT {
        if let Expr::Call(call) = receiver {
            if call.name == Syntax::BUILTIN_EXPECT
                && !cx.sigs.contains_key(&call.name)
                && !locals.contains(&call.name)
                && call.args.len() == 1
                && args.is_empty()
            {
                return expr_in_subset(&call.args[0].expr, cx, locals);
            }
        }
    }
    // c109 Phase 23: `.raw()` on a distinct type (D-DIST3). The AST `emit_method_call`
    // special-cases `method == "raw"` BEFORE any user dispatch, unconditionally emitting
    // `({recv}).0`. Sema (CheckerInfer ~L2039) admits `.raw()` ONLY on a distinct-type
    // value (E0311 otherwise), so any `.raw()` that survives to codegen is on a distinct
    // — covering an in-subset 0-arg `.raw()` is safe (and `recv_type` is `None` here,
    // since sema's `.raw()` arm returns the base type without the recv_type writeback).
    // D-TYPEDTEXT1=D: `Sql.raw("…")` / `Html.raw("…")` — `Sql`/`Html` name the
    // type (not a value), so `recv_type` is unset here. Checked BEFORE the
    // distinct-type `.raw()` below (same method name, disjoint receiver shape —
    // a distinct value's `.raw()` never takes an argument).
    if method == "raw" {
        if let Expr::Ident(n, _) = receiver {
            if n == "Sql" || n == "Html" {
                return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
            }
        }
    }
    // D-ERRCTX1=D: `<fallible>.context("…")` — sema marks the receiver via
    // `recv_type_out` (see `infer_method_call`).
    if method == "context" && recv_type.as_deref() == Some("__Fallible__") {
        return args.len() == 1
            && expr_in_subset(receiver, cx, locals)
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    if method == Syntax::METHOD_DISTINCT_RAW {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    // D-TYPEDTEXT1=D: `.template()`/`.params()` (Sql) and `.text()` (Html) —
    // 0-arg accessors on a checked typed-text value.
    if recv_type.as_deref() == Some("Sql") && matches!(method, "template" | "params") {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    if recv_type.as_deref() == Some("Html") && method == "text" {
        return args.is_empty() && expr_in_subset(receiver, cx, locals);
    }
    // D-OPTGC1: `handle.with` / `handle.with_mut` on a traced `Gc<T>` value.
    if matches!(method, "with" | "with_mut") {
        return args.len() == 1
            && matches!(&args[0].expr, Expr::Lambda(l) if lambda_in_subset(l, cx, locals))
            && expr_in_subset(receiver, cx, locals);
    }
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact` handle.
    // Sema types the handle `Transaction` (`recv_type == Some("Transaction")`).
    // It lowers to a Drop-backed commit guard (a `scope.guard` cousin), so the
    // single arg must be an in-subset literal zero-param lambda.
    if method == Syntax::TXN_ON_COMMIT && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` — the mirror of
    // `on_commit`, same in-subset shape (a literal zero-param lambda on a handle sema
    // typed `Transaction`).
    if method == Syntax::TXN_ON_ROLLBACK && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-TASKSCOPE1=A: `g.task { … }` on a taskgroup handle (scoped spawn).
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SPAWN_METHOD
    {
        return args.len() == 1
            && args[0].label.is_none()
            && matches!(&args[0].expr, Expr::Lambda(_))
            && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-NURSERY1=A: `g.all([…])` — join a list of task handles.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_ALL_METHOD
    {
        return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-CONCCOMB1=A: `g.race([…])` / `g.any([…])` — first completed task wins.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && (method == Syntax::TASKGROUP_RACE_METHOD || method == Syntax::TASKGROUP_ANY_METHOD)
    {
        return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
    }
    // D-CONCSELECT1=A: fluent scoped select on taskgroups.
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SELECT_METHOD
    {
        return args.is_empty();
    }
    if recv_type
        .as_deref()
        .is_some_and(|rt| rt == Syntax::TYPE_SELECT_BUILDER || rt.starts_with("SelectBuilder<"))
    {
        match method {
            Syntax::SELECT_RECV_METHOD | Syntax::SELECT_READ_METHOD => {
                return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
            }
            Syntax::SELECT_AFTER_METHOD => {
                return args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals);
            }
            Syntax::SELECT_WAIT_METHOD => return args.is_empty(),
            _ => {}
        }
    }
    // D-MEM1 S6 (D-POOLID-API1=A / D-SHARED-API1=A): `Pool<T>.add/remove/ids` and
    // `Shared<T>.read/edit`. Sema sets `recv_type` to `"Pool"`/`"Shared"`
    // explicitly for these (see `CheckerInfer/calls.rs`'s comment on why — the
    // method names collide with Set/List/Map's `add`/`remove`, unlike Task/
    // Sender's globally-unambiguous names), so gate on that instead of a name+
    // arity guess. `recv_type == Some("Pool")` is ALSO how D-ALLOC1's `mem.Pool`
    // slab allocator (a completely different, pre-existing type — see
    // `Type::Named` vs this stage's `Type::Apply` in the type's own doc
    // comment) marks its `.alloc`/`.reset`/`.free` methods, so only intercept
    // (and `return`) for the THREE names this stage actually owns — anything
    // else falls through to the allocator's existing `("Arena"|"Bump"|"Pool"|
    // "Fixed", …)` shape further down, unbroken.
    if recv_type.as_deref() == Some("Pool") && matches!(method, "add" | "remove" | "ids") {
        return match method {
            "add" => args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals),
            "remove" => args.len() == 1 && expr_in_subset(&args[0].expr, cx, locals),
            "ids" => args.is_empty(),
            _ => unreachable!("matches! above admitted only these"),
        };
    }
    if recv_type.as_deref() == Some("Shared") {
        return matches!(method, "read" | "edit")
            && args.len() == 1
            && matches!(&args[0].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals));
    }
    // Shape (m) [c109 Phase 27]: a CALL THROUGH a fn-typed struct field — `w.step(4)`
    // where `step: fn(Int) -> Int` is a field on a covered struct, NOT a user method.
    // Sema (CheckerInfer ~L2329) sets `recv_type == Some(<StructType>)` and re-routes the
    // node through `infer_call_value`, but registers NO `method_sigs` entry (it is a field,
    // not a method). The AST `emit_method_call` (Expression.rs ~L1573) detects this case
    // FIRST — a `struct_fields` entry whose type is `Type::Fn` — and emits
    // `(({recv}).{user_<field>})({args})` with PLAIN args (`emit_call_args(.., None, ..)`).
    // We mirror that order: tried before the user-method/static shapes so a fn-field whose
    // name happens to match a method name resolves to the field, exactly as the AST path.
    if fn_field_call_in_subset(receiver, method, args, recv_type, cx, locals) {
        return true;
    }
    // Shape (l) [c109 Phase 24]: a JSON construction `JSON.Boolean(b)` / `JSON.Number(n)`
    // / `JSON.Text(s)` / `JSON.Array(xs)` / `JSON.Object(map)`. The receiver is the
    // bare `Ident("JSON")` (a type name, NOT a local), and `method` is a JSON variant.
    // Sema (`check_core_json_lit`) types it as `JSON` WITHOUT setting `recv_type` (so
    // `recv_type == None`), and the AST `emit_method_call` routes it through
    // `emit_core_json_lit` (Expression.rs ~L1633) BEFORE the user enum-lit / instance
    // shapes. Tried here FIRST among the type-name receivers: `JSON` is not a core
    // import alias (so the core shape declines), not a local, not a user enum/struct
    // name. The single payload arg must be in-subset. `JSON.Null` is the no-arg Field
    // form, handled in `expr_in_subset`'s `Field` arm, not here.
    if let Expr::Ident(type_name, _) = receiver {
        if !locals.contains(type_name) && is_json_type_name(type_name) && is_json_variant(method) {
            return args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // D-DBDRIVER1: a `DbValue` construction `DbValue.Int(n)` / `.Float(f)` /
    // `.Text(s)` / `.Bool(b)` — same shape as the JSON construction just above.
    if let Expr::Ident(type_name, _) = receiver {
        if !locals.contains(type_name)
            && is_db_value_type_name(type_name)
            && is_db_value_variant(method)
        {
            return args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // Shape (e) [c109 Phase 10]: a core/stdlib module call `alias.method(args)` where
    // `alias` is a core import. Sema leaves `recv_type == None` for core calls
    // (`infer_core_call` returns without setting it). A core call is uniquely a
    // `MethodCall` whose receiver is an `Ident(alias)` with `alias ∈ cx.core_imports`
    // — disjoint from the builtin shape (which needs a *value* receiver) and the
    // static shape (a covered *type-name* receiver). Tried BEFORE the builtin shape:
    // a core method named `get`/`split`/… would otherwise be claimed (and rejected,
    // since a module alias is not a local) by the builtin shape's `return`. The
    // covered set is the type-monomorphic core calls (`core_call_covered`); the
    // polymorphic math/random/io specials + every closure-taking / handle-constructor
    // call stay on the AST path.
    if recv_type.is_none() {
        if matches!(receiver, Expr::Field(..)) {
            if let Some(submodule) =
                core_module_path_from_receiver(receiver, &cx.core_imports, locals)
            {
                return core_call_covered(&submodule, method)
                    && core_call_args_in_subset(&submodule, method, args, cx, locals);
            }
        }
        if let Expr::Ident(alias, _) = receiver {
            if !locals.contains(alias) {
                if let Some(module) = cx.core_imports.get(alias) {
                    // c109 Phase 13: the three closure-taking core calls (`tasks.spawn`/
                    // `http.serve`/`scope.guard`) — NOT in `core_fixed_sig`, each a
                    // bespoke emit shape with a literal-lambda closure arg.
                    if core_closure_call_in_subset(module, method, args, cx, locals) {
                        return true;
                    }
                    return core_call_covered(module, method)
                        && core_call_args_in_subset(module, method, args, cx, locals);
                }
                // Shape (i) [c109 Phase 14]: a qualified cross-module call
                // `alias.method(args)` — a `pub use` re-export (`reexport_calls`), a
                // file/dir-module import (`import_mods`), or an inline code module
                // (`code_modules`). The AST `emit_method_call` checks these in this
                // exact order (after `core_imports`, already handled above). Each
                // lowers to its resolved `{root}{mod}::{fn}` / `{root}user_{a}__{m}`
                // form. Args carry their import-signature conventions, reproduced via
                // `lower_one_call_arg`; the Arc form stays excluded.
                let is_module_alias = cx
                    .reexport_calls
                    .contains_key(&(alias.clone(), method.to_string()))
                    || cx.import_mods.contains_key(alias)
                    || cx.code_modules.contains(alias.as_str());
                if is_module_alias {
                    return args.iter().all(|a| {
                        a.label.is_none()
                            && !a.flags.shared_auto_clone
                            && arg_conv_in_subset(a)
                            && expr_in_subset(&a.expr, cx, locals)
                    });
                }
            }
        }
    }
    // Shape (k) [c109 Phase 19]: the arena allocator constructor `mem.Arena.new(…)`
    // (D-ALLOC1). The receiver is `Field(Ident(mem-alias), <AllocType>)`, method `new`.
    // Sema sets `recv_type == Some(<AllocType>)` (the receiver `mem.Arena` is typed
    // `Named(Arena)` via `infer_core_field`, then `.new()` dispatches through
    // `alloc_method_return`). The AST `emit_method_call` claims it via its FIRST branch
    // (the `mem.<Alloc>.new()` constructor, Expression.rs ~L1515) BEFORE any `rty`-keyed
    // arm — so we mirror that and try it FIRST, before the handle shape. The optional
    // `capacity:`/`slots:`/`size:` arg is admitted (a label is allowed HERE — the AST reads
    // `arg(0)` ignoring the label, choosing the ctor by allocator type, not label).
    if alloc_new_type(receiver, method, cx, locals).is_some() {
        return args.len() <= 1 && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` mirrors `mem.Arena.new()`:
    // receiver is a module-field sentinel, not a runtime value.
    if solve_new_type(receiver, method, cx, locals).is_some() {
        return args.len() == 1 && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    if let Some(static_type) = game_static_type(receiver, method, cx, locals) {
        let want = match (static_type, method) {
            ("Backend", "headless") => 0,
            ("Budgets", "new") => 4,
            _ => 1,
        };
        return args.len() == want && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    // D-OPTGC1: `gc.Gc.new<T>(value)` traced-handle constructor.
    if recv_type.as_deref() == Some(Syntax::GC_TYPE) && method == Syntax::GC_NEW {
        return args.len() == 1 && args.iter().all(|a| expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d) [c109 Phase 9]: a built-in collection/string method
    // (`emit_builtin_method`) — `len`/`push`/`get`/`keys`/`trim`/`split`/… on a
    // list/map/string receiver. Sema resolves these via `Collections::
    // builtin_method_return` and leaves `recv_type == None` (it sets `recv_type`
    // only for the numeric width conversions — Phase 12 — and for user instance /
    // handle methods). So `recv_type.is_none()` + a covered builtin name + an
    // in-subset *value* receiver uniquely identifies a builtin collection/string
    // call: the receiver must be a collection/string (the program type-checked, and
    // a struct/enum/handle/numeric receiver would have set `recv_type`). A bare
    // type-name ident (a static-call receiver) is NOT in `locals`, so it fails
    // `expr_in_subset` and is excluded here, falling through to the static shape.
    //
    // The Map-vs-List-vs-String emit branch (`rty = expr_jet_ty(receiver)`) is
    // resolved at LOWERING from the receiver's total type (reproducing the AST's
    // `expr_jet_ty`, incl. its `None` → default-branch partiality), never re-derived
    // in emit. Tried BEFORE the static/instance shapes (both keyed on the same
    // `recv_type`) to claim builtins first.
    if recv_type.is_none() && is_covered_builtin_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d-coll-ctor) [D-COLLBREADTH1=A]: a collection static constructor —
    // `Set.from([...])` or `Deque.new()`. The receiver is a bare type-name ident
    // (`"Set"` / `"Deque"`), NOT a local. Sema types the call and leaves
    // `recv_type == None`. The method is `"from"` (for Set) or `"new"` (for Deque).
    // Both are `is_intercepted_method_name` names, so they never reach the static
    // user-type shape (line ~2843). This shape claims them BEFORE that check. Every arg
    // must be in-subset (for `Set.from`, the list literal is always in-subset).
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !locals.contains(type_name.as_str()) {
                match (type_name.as_str(), method, args.len()) {
                    ("Set", "from", 1)
                    | ("SortedSet", "from", 1)
                    | ("PriorityQueue", "from", 1)
                    | ("ByteBuffer", "from", 1)
                    | ("Bag", "new", 0)
                    | ("Deque", "new", 0)
                    | ("SortedSet", "new", 0)
                    | ("PriorityQueue", "new", 0)
                    | ("Lru", "new", 1)
                    | ("BitSet", "new", 0)
                    | ("ByteBuffer", "new", 0) => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    // D-MEM1 S6: `Pool<T>.new()` / `Shared.new(x)` — same bare
                    // type-name static-constructor shape as `Deque.new()` above.
                    ("Pool", "new", 0) => return true,
                    ("Shared", "new", 1) => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    // D-PATHFS1: `Path.from(str)` — static constructor for typed paths.
                    // Like `Set.from`, admitted before `static_method_call_in_subset`
                    // blocks `from` (an intercepted name). Path is not a user type.
                    ("Path", "from", 1) if !cx.type_names.contains("Path") => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    // D-SHIFT1 (c7shift): `Reader.over(bytes)` / `Cursor.over(s)` —
                    // same static-constructor admission shape as `Path.from`.
                    ("Reader", "over", 1) if !cx.type_names.contains("Reader") => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    ("Cursor", "over", 1) if !cx.type_names.contains("Cursor") => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    // D-HOLE1: `Option.lift2(f, a, b)` — static combinator, not a
                    // user type. `f` is a plain in-subset lambda arg (no closure-arg
                    // gate needed here — `expr_in_subset`'s `Expr::Lambda` arm already
                    // routes through `lambda_in_subset`).
                    ("Option", "lift2", 3) if !cx.type_names.contains("Option") => {
                        return args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                    _ => {}
                }
            }
        }
    }
    // Shape (d2) [c109 Phase 19]: `Stopwatch.elapsed_millis()`. The AST
    // `emit_builtin_method` dispatches `elapsed_millis` on the method NAME alone (it
    // fires before any `rty` test, Expression.rs ~L1023), and sema types it via
    // `Collections::stopwatch_method_return` — leaving `recv_type == None` (NOT the
    // `Some(<handle>)` of the Phase-13 handle shape). So it is a Phase-9-style builtin
    // gap: a `MethodCall` with `recv_type == None`, a covered builtin name, an in-subset
    // value receiver (a `Stopwatch` `let`-bound from the covered `time.start` producer).
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`). Tried after the collection builtins so a list/map/string
    // `elapsed_millis` (impossible — no such method) can't be misclaimed.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        return expr_in_subset(receiver, cx, locals);
    }
    // Shape (d3) [c109 Phase 21 / D-TUPLE-DESTRUCT1]: a Task/Receiver/Sender
    // concurrency method. Like Stopwatch (d2), sema types these via
    // `Collections::builtin_method_return`'s `Type::Apply` arms
    // (`task_method_return`/`receiver_method_return`/`sender_method_return`,
    // Source/Collections.rs) and leaves `recv_type == None` (a Phase-9 builtin gap).
    // The AST `emit_builtin_method` dispatches them on the method NAME alone
    // (`join`/`detach`/`receive`/`send`). The names + arg counts are disjoint from
    // every other shape: `Task.join()` is the 0-arg `join` (the 1-arg list
    // `join(sep)` is claimed by shape d above); `detach`/`receive` (0 args) and
    // `send` (1 arg) are used by no other builtin. The receiver is a `Task`/
    // `Receiver`/`Sender` value `(tx, rx) := tasks.channel<T>()`-destructured or
    // `tasks.spawn(…)`-produced. Tried after the collection builtins so a
    // list/map/string method can't be misclaimed.
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d4): `Match.group(n)` (D-REGEXENGINE1). Sema sets `recv_type ==
    // Some("Match")` (the `Match` receiver type, CheckerInfer's user/handle-method
    // writeback), and the AST `emit_builtin_method` dispatches it on the method NAME
    // guarded by `rty == Some(Named("Match"))` (Expression.rs ~L1132). Keyed on
    // `recv_type == Some("Match")` + `group`/1 — disjoint from every user instance method
    // (whose `recv_type` is a covered struct/enum, never `Match`) and from the numeric
    // shape (a numeric `recv_type`). The receiver is a `Match` value (`if m == value(mat)`
    // binding). Lowered to `BuiltinMethod`/`TBuiltinOp::MatchGroup`. The result is `String?`.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d5) [D-REACT1=B]: a reactive `Signal`/`Derived` method (`.get()`/`.set(v)`).
    // Sema sets `recv_type == Some("Signal"|"Derived")` (CheckerInfer's reactive arm), so
    // these are keyed on recv_type — NOT the bare name (`get`/0 would alias a list `get`).
    // `Signal.get()`/`Derived.get()` → `(recv).get()`; `Signal.set(v)` → `(recv).set(v)`.
    if matches!(
        recv_type.as_deref(),
        Some("Signal") | Some("Derived") | Some("Computed")
    ) && is_reactive_method_name(method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d5b) [D-EVENT1=D]: Event/Hook handle methods. Handler arguments
    // must be literal lambdas so lowering can seed payload/result types and
    // render the stored callback for the event runtime.
    if is_event_handle_type(recv_type.as_deref()) && is_event_method_name(method, args.len()) {
        let handler_ok = match (method, args.len()) {
            ("on" | "once", 2) => args
                .get(1)
                .is_some_and(|a| a.label.is_none() && matches!(a.expr, Expr::Lambda(_))),
            ("on_priority", 3) => {
                args.get(1)
                    .is_some_and(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals))
                    && args
                        .get(2)
                        .is_some_and(|a| a.label.is_none() && matches!(a.expr, Expr::Lambda(_)))
            }
            _ => true,
        };
        return expr_in_subset(receiver, cx, locals)
            && handler_ok
            && args.iter().enumerate().all(|(i, a)| {
                a.label.is_none()
                    && match (method, args.len(), i) {
                        ("on" | "once", 2, 1) | ("on_priority", 3, 2) => true,
                        _ => expr_in_subset(&a.expr, cx, locals),
                    }
            });
    }
    // Shape (d5c) [D-WATCH-SCOPE1]: watcher handle/set methods. Callback
    // handlers are literal lambdas so lowering can seed `WatchEvent`.
    if is_watch_handle_type(recv_type.as_deref()) && is_watch_method_name(method, args.len()) {
        let handler_ok = match (recv_type.as_deref(), method, args.len()) {
            (Some("WatchHandle"), "on" | "once", 2) => args
                .get(1)
                .is_some_and(|a| a.label.is_none() && matches!(a.expr, Expr::Lambda(_))),
            _ => true,
        };
        return expr_in_subset(receiver, cx, locals)
            && handler_ok
            && args.iter().enumerate().all(|(i, a)| {
                a.label.is_none()
                    && match (recv_type.as_deref(), method, args.len(), i) {
                        (Some("WatchHandle"), "on" | "once", 2, 1) => true,
                        _ => expr_in_subset(&a.expr, cx, locals),
                    }
            });
    }
    // D-PROCESS1: ProcessSpec/ProcessChild subprocess handles. The lowerer routes
    // every admitted method through fixed prelude helpers, with sema-proved arity.
    if matches!(
        recv_type.as_deref(),
        Some("ProcessSpec" | "ProcessChild" | "ProcessStdin" | "ProcessStdoutStream" | "ProcessStderrStream")
    ) && is_process_handle_method_name(recv_type.as_deref(), method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d6) [D-HONESTNUM1=A]: a `Measurement<Float>` method.
    // Sema sets `recv_type == Some("Measurement")`.
    if recv_type.as_deref() == Some("Measurement") && is_measurement_method_name(method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d7) [D-PENDING1=B]: a `Loadable<T,E>` method.
    // Sema sets `recv_type == Some("Loadable")`.
    if recv_type.as_deref() == Some("Loadable") && is_loadable_method_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // D-TTLVAL1=A: Expiring<T> / Rotting<T> methods.
    if matches!(recv_type.as_deref(), Some("Expiring" | "Rotting"))
        && matches!(
            (method, args.len()),
            ("get", 1) | ("is_valid", 1) | ("force", 1)
        )
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d7b) [D-RENDERTGT2=A]: a UI backend method.
    if matches!(
        recv_type.as_deref(),
        Some("NullBackend" | "TuiBackend" | "GtkBackend")
    ) && is_ui_backend_method_name(recv_type.as_deref(), method, args.len())
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // c-devserver (owner-directed 2026-07-01): a DevServer builder method.
    if recv_type.as_deref() == Some("DevServer") && is_devserver_method_name(method, args.len()) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d8) [D-APPROX1=A]: a sketch method (HyperLogLog/TDigest/CMS/ReservoirSampler).
    if is_sketch_type(recv_type.as_deref()) && is_sketch_method_name(recv_type.as_deref(), method) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d10) [D-NETDEP1=A / D-HTTPLIB1=A]: an HTTP type method call.
    if is_http_type(recv_type.as_deref()) && is_http_method_name(recv_type.as_deref(), method) {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (d9) [D-TIMEDEPTH1=A]: a civil-time method (Date/DateTime).
    if matches!(
        recv_type.as_deref(),
        Some(
            "Date"
                | "LocalDate"
                | "LocalTime"
                | "DateTime"
                | "Instant"
                | "Period"
                | "Zone"
                | "ZonedDateTime"
        )
    ) && is_civil_time_method_name(recv_type.as_deref(), method)
    {
        return expr_in_subset(receiver, cx, locals)
            && args
                .iter()
                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // Shape (f) [c109 Phase 11]: a closure-taking collection method (`map`/`filter`/
    // `each`/`find`/`any`/`all`/`sort_by`/`reduce`). Like the Phase-9 builtin shape it
    // carries `recv_type == None` and an in-subset *value* receiver. The Fn-vs-FnMut
    // emit branch reads the lambda arg's `needs_fn_mut` meta, so the closure-arg
    // position MUST be a literal `Expr::Lambda` (a fn-value there defaults to the
    // non-mut form on the AST side, but covering that needs the deferred fn-value
    // emit — exclude). `reduce` takes (seed, lambda); the rest take (lambda).
    if recv_type.is_none() && closure_method_in_subset(method, args, cx, locals) {
        return expr_in_subset(receiver, cx, locals);
    }
    // Shape (g) [c109 Phase 12]: a numeric predicate / bit-population / width
    // conversion (`is_nan`/`count_ones`/`to_i32`/… — D-NUMOPS1). Sema sets
    // `recv_type == Some(<numeric name>)` for a numeric receiver (CheckerInfer
    // ~L2248), so a numeric method is uniquely a `MethodCall` whose `recv_type` parses
    // as a numeric type name (`Int`/`Float`/`F32`/`I8..U64`) and whose `method` is a
    // covered numeric op. All numeric ops are nullary (no args). The width source is
    // the total `recv_type`, so the widening/narrowing decision is total at lowering.
    if let Some(numeric_name) = recv_type {
        if crate::AST::numeric_type_from_name(numeric_name).is_some()
            && is_covered_numeric_method(method, args.len())
        {
            return expr_in_subset(receiver, cx, locals);
        }
    }
    // Shape (h2) [c109 Phase 25]: HttpRouter route registration `router.get(path, handler)`
    // / `.post`/`.put`/`.delete` (D-ROUTE1=A). Sema sets `recv_type == Some("HttpRouter")`.
    // The AST `emit_builtin_method` keys these on `rty == Some(HttpRouter)` BEFORE the
    // generic `get`/`post` collection arms, and emits the handler via `emit_router_handler`
    // (a boxed `Fn(HttpRequest)->HttpResponse` closure). We cover it when the receiver is
    // in-subset, the path arg is in-subset, and the handler arg is one `emit_router_handler`
    // reproduces byte-for-byte: a bare top-level-fn name (NOT a local → the `Box::new(move
    // |__req| user_<fn>(&__req)) as …` wrapper) or an in-subset literal lambda. Tried BEFORE
    // the numeric/handle/builtin shapes so the HttpRouter `get`/`post` is claimed here.
    if recv_type.as_deref() == Some("HttpRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        return router_register_in_subset(receiver, args, cx, locals);
    }
    // Shape (h) [c109 Phase 13]: a method ON a handle (FileReader/FileWriter/
    // StdinHandle/Stopwatch/TcpListener/TcpStream). Sema sets `recv_type ==
    // Some(<handle>)` (CheckerInfer, via the handle `*_method_return` tables). The AST
    // emit branch (`emit_builtin_method`) keys on `rty = expr_jet_ty(receiver)`; for
    // these handles the receiver is ALWAYS a `let`-bound local from a covered
    // handle-producing core call (`files.open`/`time.start`/`net.tcp_connect`/…) or
    // another covered handle method (`listener.accept()`), so its slot type is total
    // (`Some(<handle>)`) — `rty == recv_type` always, and the rty-keyed branch fires
    // identically. (c109 Phase 20: HttpRequest/HttpResponse accessors are NOW covered —
    // sema writes the `http.serve` lambda-param type back onto `p.ty`, so the slot type
    // is total even for an unannotated `(req)` param; the AST `rty`-keyed handle arm then
    // fires identically. They join `handle_method_op`.) Disjoint from
    // the numeric shape (a handle name isn't numeric) and the instance/static shapes
    // (a handle name isn't a covered struct/enum).
    // D-SIMD2 / D-LINALG1: a method on a built-in math value type (and NOT a user
    // type sharing the name). Admitted when the receiver + args are in-subset.
    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                return expr_in_subset(receiver, cx, locals)
                    && args
                        .iter()
                        .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
            }
        }
    }
    if let Some(handle) = recv_type {
        if (handle == "__SerdeEncode__" && method == "encode" && args.is_empty())
            || (handle == Syntax::TYPE_DATA
                && method == Syntax::METHOD_DATATREE_DECODE
                && args.is_empty())
        {
            return expr_in_subset(receiver, cx, locals);
        }
        if handle_method_op(handle, method, args.len()).is_some() {
            return expr_in_subset(receiver, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
        // D-SHIFT1 (c7shift): `cursor.take_pattern("…")` — not in
        // `handle_method_op` (its `THandleOp` carries the pattern parts, so
        // it's built at lowering, not from a `(handle, method, nargs)` key).
        // The sole argument is a parser-committed `Expr::StrMatchLit` leaf.
        if handle == "Cursor"
            && method == Syntax::METHOD_TAKE_PATTERN
            && args.len() == 1
            && matches!(args[0].expr, Expr::StrMatchLit(_, _))
        {
            return expr_in_subset(receiver, cx, locals);
        }
    }
    // D-LAYOUT1 / D-LAYOUT-GATES1: a method on `LayoutHandle`/`Constraint`
    // (mirrors the D-SIMD2 math-method carve-out immediately above). Admitted
    // when the receiver + args are in-subset.
    if let Some(handle) = recv_type {
        if crate::Sema::is_layout_type(handle)
            && crate::Sema::layout_method_return(handle, method, args.len()).is_some()
        {
            return expr_in_subset(receiver, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // Shape (j) [c109 Phase 16]: an enum-variant CONSTRUCTION `Enum.Variant(args)`.
    // The parser/sema never produce an `Expr::EnumLit` node for a payload variant —
    // a `Type.Variant(args)` stays a `MethodCall` (sema type-checks it via
    // `check_enum_lit` in place but does NOT rewrite the node). The AST `emit_method_call`
    // (Expression.rs ~L1635) routes such a call to `emit_enum_lit` when the receiver is
    // a known enum and `method` is a variant. This is THE shape that constructs
    // string/struct/collection-payload and recursive (boxed) enum values. We cover it
    // when the enum is covered and every (positional) arg is in-subset; the
    // borrowed-clone/`Box::new` decisions are resolved at lowering (`lower_enum_arg`),
    // reproducing `emit_boxed_enum_arg` byte-for-byte. Tried BEFORE the static shape
    // (which excludes variants), matching the AST dispatch order.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !locals.contains(type_name) {
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; handle it specially before the user-enum path.
                if type_name == crate::Syntax::TYPE_KEY {
                    return is_key_variant(method)
                        && args
                            .iter()
                            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                }
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        return enum_is_covered(type_name, cx)
                            && args
                                .iter()
                                .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
                    }
                }
            }
        }
    }
    // Shape (c): a STATIC (associated) method call `Type.make(x)`. Phase 6 deferred
    // this (its `recv_type` is `None`). The AST path emits `user_<T>::user_<method>(…)`
    // when the receiver is a type name in `cx.type_names` (Expression.rs ~L1644). We
    // reproduce exactly that, and only that: the receiver is a bare type-name ident
    // (not a local), the type is a covered struct/enum, the method is a registered
    // user method (in `method_sigs`) that is NOT an enum *variant* (those emit an enum
    // literal, a different lowering) and NOT a builtin/special intercept.
    if let Some(type_name) = static_call_type_name(receiver, locals) {
        return static_method_call_in_subset(&type_name, method, args, cx, locals);
    }
    if recv_type.is_none() {
        return false;
    }
    // Shape (n) [c109 Phase 30]: DYNAMIC dispatch on a TRAIT-OBJECT receiver
    // (`s.name()`/`s.area()` where `s: Shape` is a `Box<dyn user_Shape>`). Sema sets
    // `recv_type == Some(<trait>)` with the trait in `cx.trait_names`; the AST
    // `emit_method_call` (Expression.rs ~L1657) keys on `cx.trait_names.contains(rt)` and
    // emits `({recv}).{method}({args})` — the BARE method name (vtable dispatch), args
    // lowered PLAINLY (`emit_call_args(.., None, ..)`). Disjoint from the user-instance
    // shape below (a trait name is never a covered struct/enum) and from the
    // numeric/handle/builtin shapes (a trait name isn't any of those). Covered when the
    // receiver is in-subset and every arg is in-subset + unlabeled.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            return expr_in_subset(receiver, cx, locals)
                && args
                    .iter()
                    .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
        }
    }
    // Shape (b): a user-defined instance method. The `recv_type` is the TOTAL sema
    // fact; a `None` was handled above (static). Anything else (a fallback-inferred
    // path) the subset must NOT reproduce — but `recv_type == Some` is the total
    // instance-method signal.
    let Some(ty) = recv_type else {
        return false;
    };
    // The method must be a user-defined method on that type (in `method_sigs`). A real
    // `method_sigs` entry is the TOTAL "this is a user method on `ty`" signal: the AST
    // `emit_method_call` now dispatches to the user method (`user_<method>`) BEFORE
    // `emit_builtin_method` whenever `recv_type == Some(T)` and `(T, method) ∈ method_sigs`
    // (the builtin-name-collision fix), so a user method SHADOWING a builtin name
    // (`get`/`len`/…) routes here, not through the name-keyed builtin path.
    let Some(sig) = cx.method_sigs.get(&(ty.clone(), method.to_string())) else {
        // No user method: a name a core/stdlib/builtin/special lowering would intercept
        // *before* the user dispatch (`emit_builtin_method`, the `.raw()`/`.snapshot()`/
        // alloc special cases) has bespoke name-keyed lowering — exclude it (those are
        // covered by their own shapes, not the user-method TIR).
        return false;
    };
    // A builtin-name method with NO `method_sigs` entry was already excluded above. With a
    // real user method present, the intercepted-name check (`is_intercepted_method_name`,
    // still used by the static-call shape) no longer applies — the AST path dispatches to
    // the user method. The `clone`/`raw` special forms returned earlier in this function;
    // `snapshot`/`new` fire their AST special cases only for non-instance receivers (an
    // `expect(...)` call / a type-name ident), so an INSTANCE method of that name with a
    // `method_sigs` entry reaches here and routes to the user method on BOTH paths.
    // The receiver type must be a covered struct or enum (so the receiver place
    // emits exactly as the AST path does, and the method is a plain user method).
    let recv_ty = Type::Named(ty.clone());
    if !is_covered_struct_ty(&recv_ty, cx) && !is_covered_enum_ty(&recv_ty, cx) {
        return false;
    }
    // The receiver expression must itself be in-subset (a covered local/param/field).
    if !expr_in_subset(receiver, cx, locals) {
        return false;
    }
    // Arity must match the resolved signature (sema guaranteed it, but be defensive).
    if args.len() != sig.len() {
        return false;
    }
    // Every argument must be in-subset. Unlike a plain call, a method arg MAY use any
    // of `Read`/`Move`/`Mutate` with implicit/Arc clone — those are carried as total
    // flags and emitted verbatim (mirroring `emit_call_args`). c109 Phase 13: a Fn-typed
    // param routes through the `Box::new(…) as <fn-type>` coercion (`lower_one_call_arg`).
    // c109 Phase 23: a call-site LABEL (`r.scale(factor: 0.5)`, D-NARG1) is allowed —
    // labels are sema-validated documentation that never reorder (D-NARG-D4) and codegen
    // never reads `CallArg.label`, so a labeled arg emits identically.
    args.iter()
        .zip(sig.iter())
        .all(|(a, (_, _pty))| expr_in_subset(&a.expr, cx, locals))
}

fn core_module_path_from_receiver(
    receiver: &Expr,
    imports: &std::collections::HashMap<String, String>,
    locals: &std::collections::HashSet<String>,
) -> Option<String> {
    match receiver {
        Expr::Ident(alias, _) if !locals.contains(alias) => imports.get(alias).cloned(),
        Expr::Field(base, leaf, _) => {
            let module = core_module_path_from_receiver(base, imports, locals)?;
            let submodule = format!("{module}.{leaf}");
            crate::Syntax::is_known_core_module(&submodule).then_some(submodule)
        }
        _ => None,
    }
}

/// c109 Phase 27: is `recv.method(args)` a call THROUGH a fn-typed struct FIELD (not a
/// user method)? Returns the field's `Type::Fn` when so. `recv_type` is the total sema
/// fact (`Some(<StructType>)`, set by CheckerInfer's fn-field arm); the field must exist
/// on a COVERED struct with a `Type::Fn` type and the same name as `method`. Mirrors the
/// AST `emit_method_call` fn-field branch's guard (`struct_fields` lookup + `Type::Fn`).
pub(crate) fn fn_field_call_ty<'a>(
    method: &str,
    recv_type: &Option<String>,
    cx: &'a Cx,
) -> Option<&'a Type> {
    let ty_name = recv_type.as_ref()?;
    if !is_covered_struct_ty(&Type::Named(ty_name.clone()), cx) {
        return None;
    }
    let fields = cx.struct_fields.get(ty_name)?;
    let (_, fty) = fields.iter().find(|(n, _)| n == method)?;
    matches!(fty, Type::Fn { .. }).then_some(fty)
}

/// c109 Phase 27: is `w.step(4)` (a call through a fn-typed struct field) in-subset? The
/// receiver and every arg must be in-subset; args are emitted PLAINLY by the AST path
/// (`emit_call_args(.., None, ..)`), so no convention/Arc-clone fact applies — exclude any
/// labeled / Arc-clone arg defensively (sema never produces them here, but stay strict).
pub(crate) fn fn_field_call_in_subset(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if fn_field_call_ty(method, recv_type, cx).is_none() {
        return false;
    }
    expr_in_subset(receiver, cx, locals)
        && args.iter().all(|a| {
            a.label.is_none() && !a.flags.shared_auto_clone && expr_in_subset(&a.expr, cx, locals)
        })
}

/// c109 Phase 7: is a STATIC method call `Type.make(args)` inside the subset? The
/// AST path (Expression.rs ~L1644) emits `user_<Type>::user_<method>(args)` for a
/// `MethodCall` whose receiver is an ident in `cx.type_names`. We admit exactly that
/// case, conservatively:
///   - `type_name` is NOT a local (a local shadowing a type would be a field/method
///     access, not a static call);
///   - `type_name` is a covered struct or enum (so its `user_<T>` prefix is right);
///   - `method` is NOT an enum *variant* of `type_name` — a `Enum.Variant(args)`
///     receiver+method emits an enum literal (a different lowering, Expression.rs
///     ~L1635), so exclude it (Phase 4 covers enum literals via `Expr::EnumLit`/
///     unit `Expr::Field`, not this MethodCall shape);
///   - `method` is NOT a builtin/special intercept (`new`, etc.);
///   - `(type_name, method)` is a registered user method (`method_sigs`);
///   - every arg is in-subset, unlabeled, and not Fn-typed.
/// D-PROTO1/D-PROTO2: resolve a static-method receiver to a user type name.
/// `Payment.Client.client()` is `MethodCall(Field(Ident(Payment), Client), …)`.
pub(crate) fn static_call_type_name_unchecked(receiver: &Expr) -> Option<String> {
    match receiver {
        Expr::Ident(name, _) => Some(name.clone()),
        Expr::Field(base, leaf, _) => {
            if let Expr::Ident(prefix, _) = base.as_ref() {
                if prefix
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
                {
                    return Some(format!("{prefix}.{leaf}"));
                }
            }
            None
        }
        _ => None,
    }
}

pub(crate) fn static_call_type_name(receiver: &Expr, locals: &HashSet<String>) -> Option<String> {
    let name = static_call_type_name_unchecked(receiver)?;
    match receiver {
        Expr::Ident(n, _) if locals.contains(n) => None,
        Expr::Field(base, _, _) => {
            if let Expr::Ident(prefix, _) = base.as_ref() {
                if locals.contains(prefix) {
                    return None;
                }
            }
            Some(name)
        }
        _ => Some(name),
    }
}

pub(crate) fn static_method_call_in_subset(
    type_name: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if locals.contains(type_name) {
        return false;
    }
    // D-SIMD2 / D-LINALG1: a static method on a built-in math type (`F32x4.splat(x)`,
    // `Vec3.from_array([…])`). Admitted when the (type, method, nargs) names a covered
    // static and every arg is in-subset.
    if crate::Sema::is_math_type(type_name)
        && !cx.type_names.contains(type_name)
        && crate::Sema::math_static_return(type_name, method, args.len()).is_some()
    {
        return args
            .iter()
            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    if type_name == "Perf"
        && !cx.type_names.contains("Perf")
        && core_call_covered("core.perf", method)
    {
        return args
            .iter()
            .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals));
    }
    // c109 Phase 25: a STATIC constructor `Type.new(args)` is the Phase-7 static-call
    // shape (`recv_type == None`, receiver a covered type-name ident, `(Type, "new") ∈
    // method_sigs`) — NOT a builtin/instance intercept. `emit_builtin_method` has no
    // `new` arm, and the only `new` special-case (`MEM_ALLOC_NEW`, D-ALLOC1) fires ONLY
    // for a `Field(mem_alias, AllocType)` receiver, never an `Ident(Type)` receiver. So
    // the AST path falls through `emit_builtin_method` (returns None) to the type-name
    // static dispatch (Expression.rs ~L1644) → `user_<Type>::user_new(args)` — exactly
    // what the StaticCall lowering reproduces. We therefore admit `new` HERE (the
    // static shape) while `is_intercepted_method_name` keeps the INSTANCE-method intercept
    // (shape b) whole: a user instance method named `new`/`get`/… stays on the AST path.
    if method != Syntax::MEM_ALLOC_NEW && is_intercepted_method_name(method) {
        return false;
    }
    let ty = Type::Named(type_name.to_string());
    if !is_covered_struct_ty(&ty, cx) && !is_covered_enum_ty(&ty, cx) {
        return false;
    }
    // An enum-name receiver whose `method` names a variant emits an enum literal,
    // not a static call — exclude (it never reaches `method_sigs` on the AST path).
    if let Some(variants) = cx.enum_variants.get(type_name) {
        if variants.iter().any(|(v, _)| v == method) {
            return false;
        }
    }
    let Some(sig) = cx
        .method_sigs
        .get(&(type_name.to_string(), method.to_string()))
    else {
        return false;
    };
    if args.len() != sig.len() {
        return false;
    }
    // c109 Phase 13: a Fn-typed static-method param routes through the Box-coercion
    // (`lower_one_call_arg`). c109 Phase 23: a call-site LABEL (`Rect.new(width: 4.0)`,
    // D-NARG1) is allowed — labels never reorder (D-NARG-D4) and codegen ignores them.
    args.iter()
        .zip(sig.iter())
        .all(|(a, (_, _pty))| expr_in_subset(&a.expr, cx, locals))
}

/// Method names a core/stdlib/builtin/special lowering intercepts *before* the
/// user-method dispatch (`emit_method_call` → `emit_builtin_method` and the
/// `.raw()`/`.snapshot()`/`mem.*.new` special cases, Source/Codegen/Expression.rs).
/// A user method sharing one of these names is emitted by that bespoke lowering on
/// the AST path, not by `method_sigs`, so the TIR must NOT claim it — exclude.
/// The list is intentionally a superset (every name those paths mention, guarded
/// or not): an extra exclusion only keeps a function on the AST path (always safe).
pub(crate) fn is_intercepted_method_name(method: &str) -> bool {
    matches!(
        method,
        // Special-cased in `emit_method_call` / `emit_expr` (clone is the synthetic
        // path, handled separately above; raw/snapshot/new have bespoke lowering).
        "clone" | "raw" | "snapshot" | "new"
        // String / list / map / collection builtins (`emit_builtin_method`).
        | "parse" | "from_bytes" | "len" | "is_empty" | "push" | "pop" | "insert"
        | "remove" | "get" | "post" | "put" | "delete" | "first" | "last"
        | "contains" | "index_of" | "reverse" | "sort" | "join" | "detach"
        | "receive" | "sender" | "send" | "clear" | "chars" | "bytes" | "trim"
        | "split" | "starts_with" | "ends_with" | "replace" | "to_upper"
        | "to_lower" | "repeat" | "slice" | "keys" | "values" | "contains_key"
        | "to_string" | "map" | "filter" | "each" | "find" | "any" | "all"
        | "sort_by" | "reduce"
        // D-ITER1: lazy iterator adapters.
        | "take" | "skip" | "step_by" | "dedup" | "chunks" | "windows"
        | "enumerate" | "zip"
        | "take_while" | "skip_while" | "flat_map" | "scan"
        | "position" | "min_by" | "max_by" | "fold" | "group_by" | "count_by" | "partition"
        // Numeric predicates / bit ops / width conversions (D-NUMOPS1).
        | "is_nan" | "is_infinite" | "is_finite"
        | "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros"
        | "to_i8" | "to_i16" | "to_i32" | "to_i64" | "to_int" | "to_u8" | "to_u16"
        | "to_u32" | "to_u64" | "to_f32" | "to_f64" | "to_float"
        // Stopwatch / file / stdin / net / http / regex / alloc handle methods.
        | "elapsed_millis" | "write_line" | "flush" | "read_line" | "lines"
        | "alloc" | "reset" | "free" | "accept" | "local_addr" | "read" | "write"
        | "peer_addr" | "close" | "method" | "path" | "body" | "header" | "param"
        | "status" | "group"
        // D-COLLBREADTH1=A: Set<T> and Deque<T> methods.
        | "add" | "union" | "to_list" | "has" | "count"
        | "push_front" | "push_back" | "pop_front" | "pop_back" | "peek_front" | "peek_back"
        // `from` is the static constructor for Set — admitted here so the static-call
        // shape below can claim it before `is_intercepted_method_name` blocks it.
        | "from"
        // D-HOLE1: `zip` is already listed above (D-ITER1); `lift2` is `Option`'s
        // static combinator, admitted the same way `from`/`new` are.
        | "lift2"
    )
}

/// A call argument is in-subset only if its convention is one the emitter
/// reproduces: a `Read` borrow or a by-`Move` value (with an optional implicit
/// clone). `Mutate` args would need `&mut place` handling we don't yet emit.
pub(crate) fn arg_conv_in_subset(_a: &crate::AST::CallArg) -> bool {
    // c109 Phase 26: ALL three call-arg conventions are now in-subset. `Read` (`&(…)`
    // for a non-scalar) and `Move` (plain value — `take`-marked args, `08_ownership`'s
    // `archive(take "vault")`) were already admitted; `Mutate` (`&mut (…)`,
    // `bump(mut score)`) is the lift. `lower_one_call_arg` already resolves all three
    // borrow wrappers from the sig convention (`emit_call_args`' `match conv` —
    // `Read`/non-scalar → `&(…)`, `Mutate` → `&mut (…)`, else plain), reproduced
    // byte-for-byte in `emit_tir_call_args`. No convention is excluded.
    true
}

/// c109 Phase 11: is `method` a closure-taking collection method the TIR lowers,
/// with in-subset args? Covers `map`/`filter`/`each`/`find`/`any`/`all`/`sort_by`
/// (1 arg: a lambda) and `reduce` (2 args: a seed value + a lambda). The closure-arg
/// position MUST be a literal `Expr::Lambda` (the Fn-vs-FnMut emit branch reads its
/// `needs_fn_mut` meta; a fn-value there defaults to the non-mut form on the AST
/// side, but covering it needs the deferred fn-value emit). The seed (`reduce`) and
/// the lambda body must be in-subset. No labels.
pub(crate) fn closure_method_in_subset(
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if !crate::Collections::is_closure_method(method) {
        return false;
    }
    if args.iter().any(|a| a.label.is_some()) {
        return false;
    }
    match method {
        "reduce" | "scan" | "fold" | "par_fold" => {
            // (seed, lambda). The seed is any in-subset value; the lambda must be a
            // literal in-subset closure.
            args.len() == 2
                && expr_in_subset(&args[0].expr, cx, locals)
                && matches!(&args[1].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals))
        }
        // (lambda). map/filter/each/find/any/all/sort_by + D-ITER1 + D-AUTOPAR1 closure adapters.
        _ => {
            args.len() == 1
                && matches!(&args[0].expr, Expr::Lambda(lam) if lambda_in_subset(lam, cx, locals))
        }
    }
}
