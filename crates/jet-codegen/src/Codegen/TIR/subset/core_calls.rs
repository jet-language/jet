use crate::AST::{Expr};
use crate::Codegen::Cx;
use crate::Codegen::TIR::expr_in_subset;
use crate::Codegen::TIR::lambda_in_subset;
use std::collections::HashSet;

/// c109 Phase 10: is a core/stdlib call `(module, method)` one the TIR lowers? The
/// covered set is exactly the **type-monomorphic** core calls — those whose full
/// signature (param conventions + return type) is fixed by `Sema::core_fixed_sig`.
/// That table is the authoritative total source: its return type gives the node's
/// total `ty` (for `?`-unwrap and binding inference), and `emit_core_call`
/// (Source/Codegen/Expression.rs) has a matching emit arm for every one of these.
///
/// Gating on `core_fixed_sig(...).is_some()` cleanly EXCLUDES the deferred calls:
///   - **closure-taking** (`tasks.spawn`, `http.serve`, `scope.guard`) — not in the
///     table / typed `None` → Phase 11 lambdas;
///   - **polymorphic** math/random/io specials (`math.abs`/`min`/`max`/`clamp`,
///     `random.pick`/`shuffle`, `io.input`/`io.eprint`) — return type depends on the
///     arg type, resolved by bespoke `check_core_call` logic, not the fixed table, so
///     a total `ty` would need re-inference (I3) → deferred;
///   - **handle-constructor** specials NOT in the table (`tasks.channel`,
///     `http.router`/`parse`/`dispatch`) and `core.mem` ptr/alloc (`#Unsafe`).
/// A handle-PRODUCING call that IS in the table (`files.open` → `FileReader`,
/// `net.tcp_connect` → `TcpStream`, `time.start` → `Stopwatch`, …) is covered: the
/// CALL emits a plain helper call (parity-exact), and any later METHOD on the
/// returned handle is itself out of subset → excludes the enclosing function.
pub(crate) fn core_call_covered(module: &str, method: &str) -> bool {
    // c109 Phase 18: the low-level `core.mem` pointer ops (`address_of`/`volatile_read`,
    // S58). NOT in `core_fixed_sig` (their types come from bespoke sema logic), but both
    // are deterministic and reproducible from total facts: `address_of(x) -> Int` is an
    // inert address cast (no `unsafe`); `volatile_read(p) -> ptr_elem(p)` reads through a
    // typed pointer, and `volatile_write(p, value) -> Unit` writes through one (the
    // volatile ops are valid because they are only reachable inside an
    // `#Unsafe` region/fn — sema E3101 — already lowered to a Rust `unsafe` context). The
    // return type is resolved at lowering (see `lower_method_call`), so it is total.
    if module == "core.mem" && matches!(method, "address_of" | "volatile_read" | "volatile_write") {
        return true;
    }
    // c109 Phase 20: the polymorphic core specials (`math.abs/min/max/clamp`,
    // `random.pick/shuffle`, `io.eprint`). NOT in `core_fixed_sig` — their return
    // type is arg-type dependent, resolved by sema's bespoke `infer_core_call` and
    // written onto the node's `resolved_ret` field (read at lowering, so it's total —
    // I3). The EMITTED form is a fixed per-`(module, method)` string (reproduced in
    // `emit_tir_core_call`), args emitted plainly, byte-for-byte `emit_core_call`.
    // (`io.input` is NOT here — it IS in `core_fixed_sig`, covered by Phase 10.)
    // D-TUPLE-DESTRUCT1: `tasks.channel<T>()` is now a polymorphic core special
    // (registered in `is_polymorphic_core_special` above) — its `(Sender<T>,
    // Receiver<T>)` return type is arg-type dependent (the call-site turbofish
    // `<T>`, sema E0904 requires it), so it's covered by the check just above, not
    // a dedicated block here. Sema writes the resolved `Type::Tuple` onto the
    // node's `resolved_ret`; lowering reads it totally (I3), and the emit reads `T`
    // off that same type to build `{root}jet_std::channel::<{T}>()`
    // (Source/Codegen/TIR/emit.rs `emit_tir_core_call`). The `Receiver`/`Sender`/
    // `Task` METHODS route via their own shape (d3 below).
    if crate::Sema::is_polymorphic_core_special(module, method) {
        return true;
    }
    // c109 Phase 25: the HttpRouter producer + the parse/dispatch core calls (D-ROUTE1=A).
    // NOT in `core_fixed_sig` — their return types are fixed per `(module, method)` but
    // live in sema's bespoke `infer_core_call` (`router` → HttpRouter, `parse` →
    // HttpRequest, `dispatch` → HttpResponse). Each emits a fixed-string `CoreCall`
    // (`{root}jet_http_router_new()` / `{root}jet_http_parse_request(&(raw))` /
    // `{root}jet_http_router_dispatch(&(router), req)`), reproduced in `emit_tir_core_call`.
    // `http.serve` stays out (closure-taking, covered by `CoreClosureCall`); `http.router`
    // is arg-free so it can't collide. The producer's `HttpRouter` value type is covered
    // (`is_covered_handle_ty`) and its binding is forced to `let mut` (D-ROUTE1=A).
    if module == "jet.http" && matches!(method, "router" | "parse" | "dispatch") {
        return true;
    }
    // D-TEXTWIDTH1=B: `text.display_width` — NOT in `core_fixed_sig` (its return
    // type varies with arg count: `Int` for 1 arg, `Int ? TextError` for the
    // `policy:` 2-arg form). Sema's bespoke `core_call.rs` dispatch resolves
    // it totally per call-site arity, mirroring `core.game.run` above.
    if module == "core.text" && method == "display_width" {
        return true;
    }
    // c109 Phase 29: qualified `io.input(prompt)`. NOT in `core_fixed_sig` — its return
    // type (`Result<String, IOError>`) lives in sema's bespoke `infer_core_call` arm
    // (CheckerCoreLib.rs), carried total by `core_call_return_ty`. It is the DISTINCT
    // qualified MethodCall on a `core.io` alias (the ambient bare `input()`, Phase 25, is
    // a separate `Expr::Call` node → `AmbientInput`). Emits the same fixed-string CoreCall
    // `{root}jet_std_io_input(None|Some(&(prompt)))` (reproduced in `emit_tir_core_call`),
    // byte-for-byte the AST `emit_core_call` arm. Composes with the Phase-8 `?? return`.
    if module == "core.io" && method == "input" {
        return true;
    }
    // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`. NOT in `core_fixed_sig` (the
    // param type is "any printable value", not a fixed `Type`), but the
    // return type IS fixed regardless of the arg (always `Value`) — total,
    // not arg-dependent, so it's covered like the other fixed-but-not-in-
    // the-table specials above.
    if module == "core.reflect" && method == "of" {
        return true;
    }
    // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `JetMeasurement<f64>`. NOT in
    // `core_fixed_sig` — the return type is `Measurement<Float>` (generic Apply).
    if module == "core.science.measurement" && method == "from" {
        return true;
    }
    // D-PENDING1=B: `L.idle/loading/loaded/failed` → `JetLoadable`. NOT in `core_fixed_sig`.
    if module == "core.reactive.loadable" && matches!(method, "idle" | "loading" | "loaded" | "failed")
    {
        return true;
    }
    // D-APPROX1=A: `HLL.new()`, `TD.new()`, `CMS.new()`, `RS.new(capacity)`. NOT in `core_fixed_sig`.
    if matches!(
        module,
        "core.sketch.hll" | "core.sketch.tdigest" | "core.sketch.cms" | "core.sketch.reservoir"
    ) && method == "new"
    {
        return true;
    }
    // D-EVENT1=D: Event/Hook constructors are generic over the payload/result
    // types, so their real return type comes from sema's resolved call.
    if module == "core.event"
        && matches!(
            method,
            "new" | "with_policy" | "hook" | "scope" | "policy_sync" | "policy_async"
        )
    {
        return true;
    }
    // D-TIMEDEPTH1=A: civil-time constructors. NOT in `core_fixed_sig`.
    if matches!(module, "core.time.date" | "core.time.datetime")
        && matches!(method, "new" | "today" | "parse" | "from_timestamp" | "now")
    {
        return true;
    }
    // D-TTLVAL1=A: Expiring<T> / Rotting<T> constructors. NOT in `core_fixed_sig` (generic T).
    if module == "core.time.expiring" && method == "new" {
        return true;
    }
    if module == "core.vault" && method == "rotting_new" {
        return true;
    }
    // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors. NOT in `core_fixed_sig`.
    if matches!(module, "core.http.client" | "core.http.server")
        && matches!(
            method,
            "get"
                | "post"
                | "request"
                | "bind"
                | "mux"
                | "serve"
                | "serve_once"
                | "serve_once_listener"
                | "response"
                | "tls"
                | "sse"
                | "static_file"
                | "static_file_range"
                | "access_log"
        )
    {
        return true;
    }
    crate::Sema::core_fixed_sig(module, method).is_some()
}

pub(super) fn core_call_args_in_subset(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &std::collections::HashSet<String>,
) -> bool {
    if module == "core.http.server" && method == "serve" && args.len() == 3 {
        return args.iter().enumerate().all(|(idx, a)| {
            let label_ok = if idx == 2 {
                matches!(
                    a.label.as_ref().map(|(label, _)| label.as_str()),
                    Some("tls")
                )
            } else {
                a.label.is_none()
            };
            label_ok && expr_in_subset(&a.expr, cx, locals)
        });
    }
    if module == "core.game" && method == "run" {
        return args.iter().enumerate().all(|(idx, a)| {
            let label_ok = match idx {
                0 => a.label.is_none(),
                1 => matches!(
                    a.label.as_ref().map(|(label, _)| label.as_str()),
                    None | Some("replay") | Some("backend")
                ),
                2 => matches!(
                    a.label.as_ref().map(|(label, _)| label.as_str()),
                    None | Some("backend")
                ),
                _ => false,
            };
            label_ok && expr_in_subset(&a.expr, cx, locals)
        });
    }
    args.iter()
        .all(|a| a.label.is_none() && expr_in_subset(&a.expr, cx, locals))
}

/// c109 Phase 13: is a closure-taking core call (`tasks.spawn`/`http.serve`/
/// `scope.guard`) inside the subset? These are NOT in `core_fixed_sig` — each has a
/// bespoke emit shape (`emit_core_call`, Source/Codegen/Expression.rs). We cover only
/// the cleanest, byte-reproducible case for each, where the closure arg is a LITERAL
/// in-subset lambda:
///   - `tasks.spawn(<lambda>)` — 1 arg, a literal lambda (the `emit_spawn_lambda`
///     `move |…|` form). A non-lambda spawn arg (a fn-value) takes the AST `arg(0)`
///     path — excluded (its byte shape differs).
///   - `http.serve(addr, <lambda>)` — 2 args; arg0 (addr) any in-subset value, arg1 a
///     literal lambda (the `jet_http_serve(&(addr), <lambda>)` branch). The
///     router-handler branch needs an HttpRouter value, which can only come from
///     `http.router()` (not in `core_fixed_sig`) — so it can't arise in a covered fn.
///   - `scope.guard(<lambda>)` — 1 arg, a literal zero-param lambda.
pub(crate) fn core_closure_call_in_subset(
    module: &str,
    method: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    let lambda_arg = |i: usize| matches!(args.get(i).map(|a| &a.expr), Some(Expr::Lambda(lam)) if lambda_in_subset(lam, cx, locals));
    let no_labels = args.iter().all(|a| a.label.is_none());
    match (module, method) {
        ("core.tasks", "spawn") => args.len() == 1 && no_labels && lambda_arg(0),
        ("jet.http", "serve") => {
            args.len() == 2
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
        }
        ("core.scope", "guard") => args.len() == 1 && no_labels && lambda_arg(0),
        // D-DATA-SURFACE1=A: typed table selectors. Rows arg is in subset; selector
        // args must be literal lambdas so lowering can seed row param types.
        ("core.data", "filter" | "sort_by") => {
            args.len() == 2
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
        }
        ("core.data", "group_count") => {
            args.len() == 2
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
        }
        ("core.data", "group_sum" | "group_mean") => {
            args.len() == 3
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
                && lambda_arg(2)
        }
        ("core.data", "inner_join" | "left_join") => {
            args.len() == 4
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && expr_in_subset(&args[1].expr, cx, locals)
                && lambda_arg(2)
                && lambda_arg(3)
        }
        ("core.data", "pivot_sum") => {
            args.len() == 4
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
                && lambda_arg(2)
                && lambda_arg(3)
        }
        ("core.data", "lazy_filter" | "lazy_sort_by") => {
            args.len() == 2
                && no_labels
                && expr_in_subset(&args[0].expr, cx, locals)
                && lambda_arg(1)
        }
        // D-REACT1=B / D-SIGNAL1: `reactive.derived/computed/effect(<lambda>)` —
        // 1 arg, a literal zero-param in-subset lambda (rendered by `render_lambda_str`).
        ("jet.reactive", "derived" | "computed" | "effect") => {
            args.len() == 1 && no_labels && lambda_arg(0)
        }
        // D-RENDERTGT2=A (c133 M2): `ui.reactive_render(<lambda>)`.
        ("core.ui", "reactive_render") => args.len() == 1 && no_labels && lambda_arg(0),
        _ => false,
    }
}
