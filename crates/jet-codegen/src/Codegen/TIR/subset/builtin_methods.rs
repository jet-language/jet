use crate::Codegen::TIR::TNumericOp;

/// c109 Phase 9: is `method` (with `nargs` arguments) a built-in collection/string
/// method the TIR lowers? This is the NON-closure, non-numeric, non-handle slice of
/// `emit_builtin_method` (Source/Codegen/Expression.rs), restricted to the list/map/
/// string surface (`Source/Collections.rs`). The closure-taking methods (`map`/
/// `filter`/`each`/`find`/`any`/`all`/`sort_by`/`reduce` — `Collections::
/// is_closure_method`) are deferred to the lambda phase; the numeric width/predicate/
/// bit methods (`to_i32`/`is_nan`/`count_ones`/… — D-NUMOPS1) and the handle methods
/// (FileWriter/TcpStream/HttpRequest/… — Phase 10) carry a `Some(recv_type)`, so the
/// gate's `recv_type.is_none()` guard already excludes them; this name list is the
/// final filter. The arg count disambiguates `join()` (no separator) vs `join(sep)`.
pub(crate) fn is_covered_builtin_name(method: &str, nargs: usize) -> bool {
    // Closure-taking methods are NEVER covered here (Phase 11), even by name.
    if crate::Collections::is_closure_method(method) {
        return false;
    }
    matches!(
        (method, nargs),
        // List + map shared.
        ("len", 0) | ("is_empty", 0) | ("clear", 0)
        // List-only.
        | ("push", 1) | ("pop", 0) | ("first", 0) | ("last", 0)
        | ("index_of", 1) | ("reverse", 0) | ("sort", 0) | ("join", 1)
        // List + map: insert/remove/get (the Map vs List branch resolves at lowering).
        | ("insert", 2) | ("remove", 1) | ("get", 1)
        // List + string: contains.
        | ("contains", 1)
        // Map-only.
        | ("keys", 0) | ("values", 0) | ("contains_key", 1)
        // String-only.
        | ("chars", 0) | ("bytes", 0) | ("trim", 0) | ("split", 1)
        | ("starts_with", 1) | ("ends_with", 1) | ("replace", 2)
        | ("to_upper", 0) | ("to_lower", 0) | ("repeat", 1) | ("slice", 2)
        // D-STR-AFTER1: first-occurrence substring split.
        | ("after", 1) | ("before", 1)
        // c97/D-STRPARSE1. `to_int`/0 on a String reaches here only with `recv_type ==
        // None` (the numeric `to_int` sets a numeric `recv_type`, so it never does);
        // `resolve_builtin_op`'s `is_string` guard is the final filter.
        | ("lines", 0) | ("to_int", 0)
        // `to_string` (String/Bool/Char receiver — those carry `recv_type == None`;
        // a numeric `to_string` sets `recv_type` and so is excluded by the guard).
        | ("to_string", 0)
        // D-ITER1: non-closure lazy adapters.
        | ("take", 1) | ("skip", 1) | ("step_by", 1)
        | ("dedup", 0) | ("chunks", 1) | ("windows", 1)
        | ("enumerate", 0) | ("zip", 1)
        | ("sum", 0) | ("product", 0) | ("min", 0) | ("max", 0)
        | ("flatten", 0) | ("intersperse", 1) | ("unzip", 0)
        // D-COLLBREADTH1=A: Set<T> instance methods.
        | ("add", 1) | ("union", 1) | ("to_list", 0)
        | ("peek", 0) | ("to_sorted_list", 0) | ("put", 2)
        | ("capacity", 0) | ("count", 0) | ("to_bytes", 0)
        | ("write_u8", 1) | ("write_u16_le", 1) | ("write_u16_be", 1)
        | ("write_u32_le", 1) | ("write_u32_be", 1)
        | ("write_u64_le", 1) | ("write_u64_be", 1) | ("write_bytes", 1)
        // D-TAG1: Bag<T> instance methods (add/remove share list/set arms above).
        | ("has", 1) | ("count", 1)
        // D-COLLBREADTH1=A: Deque<T> instance methods.
        | ("push_front", 1) | ("push_back", 1)
        | ("pop_front", 0) | ("pop_back", 0)
        | ("peek_front", 0) | ("peek_back", 0)
        // D-FAILCOMP1: failure-aware list adapter.
        | ("try_collect", 0)
        // D-DYNARRAY1: `list.view(a..b)` — zero-copy window constructor. The
        // View<T> read-accessor methods (`get`/`first`/`last`/`index_of`/
        // `len`/`is_empty`/`contains`) need no separate entry — they share
        // the exact same (name, argcount) pairs as the list arms above, and
        // `resolve_builtin_op`/`method_call_in_subset` don't branch on
        // receiver type for those (a `&[T]` receiver satisfies them exactly
        // as a `Vec<T>` does — see `Context::rust_type`'s `View` arm).
        | ("view", 2)
    )
    // NOTE: `is_empty` (now Bool-typed in `Collections::*_method_return` after the
    // c109 fix; lowered to `TBuiltinOp::IsEmpty`) is covered above. `join()` (no
    // separator) stays excluded: sema requires `join(sep)` (E0311 on no-arg), so the
    // no-arg form never reaches codegen — its AST arm is dead.
}

/// c109 Phase 21 + D-COROUTINE1=A / D-TUPLE-DESTRUCT1: is `(method, nargs)` a
/// Task/Receiver/Sender concurrency method (`emit_builtin_method`'s `Type::Apply`-
/// receiver arms)? `Task.join()/wait()/detach()/pause()/resume()/cancel()/trace()`,
/// `Receiver.receive()`, `Sender.send(v)`. The arg count disambiguates
/// `Task.join()` (0 args) from the list `join(sep)` (1 arg, shape d) and `Sender.send(v)`
/// (1 arg) — every name+arity here is disjoint from every other covered shape.
pub(crate) fn is_concurrency_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("join", 0)
            | ("wait", 0)
            | ("detach", 0)
            | ("pause", 0)
            | ("resume", 0)
            | ("cancel", 0)
            | ("trace", 0)
            | ("receive", 0)
            | ("send", 1)
    )
}

/// D-REACT1=B: is `(method, nargs)` a reactive `Signal`/`Derived` method?
/// `Signal.get()`/`Derived.get()` (0 args), `Signal.set(v)` (1 arg). Always keyed
/// together with `recv_type == Some("Signal"|"Derived")`, never on the name alone.
pub(crate) fn is_reactive_method_name(method: &str, nargs: usize) -> bool {
    matches!((method, nargs), ("get", 0) | ("set", 1))
}

pub(crate) fn is_event_handle_type(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("Event" | "Hook" | "Subscription" | "EventScope" | "EventTrace")
    )
}

pub(crate) fn is_event_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("on" | "once", 2)
            | ("on_priority", 3)
            | ("emit" | "emit_async", 1)
            | ("run", 2)
            | ("unsubscribe" | "active" | "cancel" | "active_count", 0)
            | ("trace" | "listener_count" | "queued_count", 0)
            | ("summary" | "delivered" | "queued" | "dropped", 0)
    )
}

pub(crate) fn is_watch_handle_type(name: Option<&str>) -> bool {
    matches!(name, Some("WatchHandle" | "WatchSet"))
}

pub(crate) fn is_watch_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("poll" | "events" | "summary", 0)
            | ("active" | "cancel", 0)
            | ("on" | "once", 2)
            | ("add", 1)
    )
}

pub(crate) fn is_process_handle_method_name(
    recv_type: Option<&str>,
    method: &str,
    nargs: usize,
) -> bool {
    match recv_type {
        Some("ProcessSpec") => matches!(
            (method, nargs),
            ("cwd" | "env_remove" | "stdin" | "stdout" | "stderr", 1)
                | ("env", 2)
                | ("env_clear" | "detached" | "run" | "spawn", 0)
                | ("timeout" | "output_limit", 1)
        ),
        Some("ProcessChild") => matches!(
            (method, nargs),
            ("id" | "wait" | "kill" | "terminate" | "interrupt", 0)
        ),
        // D-PROCESS1=A: `.write(text)` on `child.stdin`.
        Some("ProcessStdin") => matches!((method, nargs), ("write", 1)),
        // D-PROCESS1=A: `.lines()` on `child.stdout`/`child.stderr`.
        Some("ProcessStdoutStream") | Some("ProcessStderrStream") => {
            matches!((method, nargs), ("lines", 0))
        }
        _ => false,
    }
}

/// D-HONESTNUM1=A: is `(method, nargs)` a `Measurement<Float>` method?
/// `.add/sub/mul/div(m)` (1 arg), `.value()/.uncertainty()` (0 args).
/// Always keyed with `recv_type == Some("Measurement")`.
pub(crate) fn is_measurement_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("add" | "sub" | "mul" | "div", 1) | ("value" | "uncertainty", 0)
    )
}

/// D-PENDING1=B: is `(method, nargs)` a `Loadable<T,E>` method?
/// `.is_loading()/.is_loaded()/.is_failed()/.is_idle()/.loaded()` (0 args),
/// `.or_else(default)` (1 arg).
/// Always keyed with `recv_type == Some("Loadable")`.
pub(crate) fn is_loadable_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        (
            "is_loading" | "is_loaded" | "is_failed" | "is_idle" | "loaded",
            0
        ) | ("or_else", 1)
    )
}

/// D-RENDERTGT2=A (c133 M1/M2): is `(backend, method, nargs)` a UI backend method?
pub(crate) fn is_ui_backend_method_name(backend: Option<&str>, method: &str, nargs: usize) -> bool {
    match (backend, method, nargs) {
        (_, "measure", 2) | (_, "layout", 2) | (_, "paint", 1) | (_, "on_event", 1) => true,
        (Some("NullBackend"), "commands", 0) => true,
        (Some("TuiBackend"), "frame_lines" | "render_count", 0) => true,
        // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
        (_, "set_focus_group", 1) | (_, "focused_label", 0) => true,
        // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 retained-widget surface.
        (Some("GtkBackend"), "label" | "button", 1) => true,
        (Some("GtkBackend"), "set_text" | "set_color" | "on_click", 2) => true,
        (Some("GtkBackend"), "set_size", 3) => true,
        (Some("GtkBackend"), "present", 1) => true,
        _ => false,
    }
}

/// c-devserver (owner-directed 2026-07-01): is `(method, nargs)` a `DevServer`
/// builder method? `.html(path)` / `.port(n)` (1 arg each), `.serve()` (0 args).
pub(crate) fn is_devserver_method_name(method: &str, nargs: usize) -> bool {
    matches!((method, nargs), ("html", 1) | ("port", 1) | ("serve", 0))
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is this an HTTP type?
pub(crate) fn is_http_type(recv_type: Option<&str>) -> bool {
    matches!(
        recv_type,
        Some(
            "HttpClientReq"
                | "HttpClientResp"
                | "HttpMux"
                | "HttpHandler"
                | "HttpSrvReq"
                | "HttpSrvResp"
                | "HttpServer"
                | "HttpServerTls",
        )
    )
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is `method` valid for this HTTP type?
pub(crate) fn is_http_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("HttpClientReq") => matches!(
            method,
            "header"
                | "body"
                | "timeout"
                | "connect_timeout"
                | "read_timeout"
                | "total_timeout"
                | "redirects"
                | "proxy"
                | "cookie"
                | "form"
                | "multipart_text"
                | "send"
        ),
        Some("HttpClientResp") => matches!(method, "status" | "body" | "header" | "cookies"),
        Some("HttpMux") => matches!(method, "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "middleware"),
        Some("HttpHandler") => method == "handle",
        Some("HttpSrvReq") => matches!(
            method,
            "method" | "path" | "body" | "param" | "header" | "body_len" | "under_limit"
        ),
        Some("HttpSrvResp") => matches!(method, "header" | "status" | "body"),
        Some("HttpServer") => matches!(method, "local_addr" | "serve" | "shutdown"),
        _ => false,
    }
}

/// D-TIMEDEPTH1=A: is `method` valid for this civil-time type?
pub(crate) fn is_civil_time_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("Date" | "LocalDate") => matches!(
            method,
            "year"
                | "month"
                | "day"
                | "add_days"
                | "add_months"
                | "add_period"
                | "diff_days"
                | "weekday"
                | "iso_weekday"
                | "day_of_year"
                | "iso_week"
                | "truncate"
                | "format"
                | "to_string"
        ),
        Some("LocalTime") => matches!(method, "hour" | "minute" | "second" | "to_string"),
        Some("DateTime") => matches!(
            method,
            "hour"
                | "minute"
                | "second"
                | "to_timestamp"
                | "to_unix_ms"
                | "date"
                | "time"
                | "plus_duration"
                | "truncate"
                | "round"
                | "in_zone"
                | "format_rfc3339"
                | "format"
                | "to_string"
        ),
        Some("Instant") => matches!(method, "elapsed_millis"),
        Some("Period") => matches!(method, "to_string"),
        Some("Zone") => matches!(method, "name"),
        Some("ZonedDateTime") => matches!(
            method,
            "date"
                | "time"
                | "offset_seconds"
                | "to_datetime"
                | "zone"
                | "add_duration"
                | "add_period"
                | "format"
                | "to_string"
        ),
        _ => false,
    }
}

/// D-APPROX1=A: is this a sketch receiver type?
pub(crate) fn is_sketch_type(recv_type: Option<&str>) -> bool {
    matches!(
        recv_type,
        Some("HyperLogLog" | "TDigest" | "CountMinSketch" | "ReservoirSampler")
    )
}

/// D-APPROX1=A: is `method` a valid method for this sketch type?
pub(crate) fn is_sketch_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("HyperLogLog") => matches!(method, "add" | "count"),
        Some("TDigest") => matches!(method, "add" | "quantile"),
        Some("CountMinSketch") => matches!(method, "add" | "count"),
        Some("ReservoirSampler") => matches!(method, "add" | "sample"),
        _ => false,
    }
}

/// c109 Phase 12: resolve a numeric method (`is_nan`/`count_ones`/`to_i32`/…) into a
/// total `TNumericOp`, reproducing `emit_builtin_method`'s numeric arms +
/// `numeric_conversion`/`conv_rust_target` (Source/Codegen/Expression.rs) EXACTLY.
/// `src_name` is the receiver's numeric type name (the AST path's `src =
/// recv_type.or_else(rty.name())`, where `recv_type` is always `Some` for a numeric
/// method — so the source width is total here). The widening-vs-narrowing decision
/// (which `numeric_conversion` makes from the source/target int ranges) is decided
/// HERE, never in emit. Returns `None` for a name this doesn't own (defensive — the
/// gate already restricted to the covered set).
pub(crate) fn resolve_numeric_op(method: &str, src_name: &str) -> Option<TNumericOp> {
    // Float predicates → `(recv).{method}()`.
    if let "is_nan" | "is_infinite" | "is_finite" = method {
        return Some(TNumericOp::Predicate(method.to_string()));
    }
    if method == "origin" {
        return Some(TNumericOp::Origin);
    }
    // Integer bit-population queries → `((recv).{method}() as i64)`.
    if let "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" = method {
        return Some(TNumericOp::BitCount(method.to_string()));
    }
    // `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string` arm).
    if method == "to_string" {
        return Some(TNumericOp::ToShow);
    }
    // Width conversion. Mirror `conv_rust_target` + `numeric_conversion`.
    let (dst_rust, dst_spelling, dst_int) = conv_rust_target_tir(method)?;
    let Some((dsigned, dbits)) = dst_int else {
        // Float target (int→float / float→float): always representable — `as`.
        return Some(TNumericOp::CastAs {
            dst_rust: dst_rust.to_string(),
        });
    };
    // The AST path's `src = recv_type.or_else(rty.name())`; here `recv_type` is the
    // total numeric name, so `parse_int_name(src_name)` is the source int width.
    match parse_int_name_tir(src_name) {
        Some((ssigned, sbits)) => {
            let (slo, shi) = crate::AST::int_range(ssigned, sbits);
            let (dlo, dhi) = crate::AST::int_range(dsigned, dbits);
            if dlo <= slo && shi <= dhi {
                // Widening — infallible `as`.
                Some(TNumericOp::CastAs {
                    dst_rust: dst_rust.to_string(),
                })
            } else {
                // Narrowing — checked `try_from` returning `Result<T, String>`.
                Some(TNumericOp::TryFrom {
                    dst_rust: dst_rust.to_string(),
                    dst_spelling: dst_spelling.to_string(),
                })
            }
        }
        // Float (or unknown) source → integer target: a saturating `as` cast.
        None => Some(TNumericOp::CastAs {
            dst_rust: dst_rust.to_string(),
        }),
    }
}

/// c109 Phase 12: TIR-local copy of `conv_rust_target` (Source/Codegen/Expression.rs)
/// — the Rust type, spelling, and integer `(signed, bits)` (or `None` for a float) a
/// `to_*` width-conversion method targets. Kept in sync with the AST path.
pub(crate) fn conv_rust_target_tir(
    method: &str,
) -> Option<(&'static str, &'static str, Option<(bool, u8)>)> {
    Some(match method {
        "to_i8" => ("i8", "I8", Some((true, 8))),
        "to_i16" => ("i16", "I16", Some((true, 16))),
        "to_i32" => ("i32", "I32", Some((true, 32))),
        "to_i64" | "to_int" => ("i64", "Int", Some((true, 64))),
        "to_u8" => ("u8", "U8", Some((false, 8))),
        "to_u16" => ("u16", "U16", Some((false, 16))),
        "to_u32" => ("u32", "U32", Some((false, 32))),
        "to_u64" => ("u64", "U64", Some((false, 64))),
        "to_f32" => ("f32", "F32", None),
        "to_f64" | "to_float" => ("f64", "Float", None),
        _ => return None,
    })
}

/// c109 Phase 12: TIR-local copy of `parse_int_name` (Source/Codegen/Expression.rs) —
/// parse a numeric type name to `(signed, bits)`, `None` for floats/non-numeric.
pub(crate) fn parse_int_name_tir(name: &str) -> Option<(bool, u8)> {
    match name {
        "Int" => Some((true, 64)),
        "Float" | "F32" | "F64" => None,
        _ => {
            let signed = name.starts_with('I');
            if (signed || name.starts_with('U')) && name.len() > 1 {
                name[1..].parse::<u8>().ok().map(|b| (signed, b))
            } else {
                None
            }
        }
    }
}

/// c109 Phase 12: is `method` (with `nargs` args) a numeric predicate / bit-op /
/// width-conversion method the TIR lowers? This is the D-NUMOPS1 slice of
/// `emit_builtin_method` keyed on a numeric receiver (`recv_type == Some(numeric)`):
/// the float predicates (`is_nan`/`is_infinite`/`is_finite`), the integer bit-pop
/// queries (`count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros`), and the
/// width conversions (`to_i8`…`to_u64`/`to_int`/`to_f32`/`to_f64`/`to_float`). All
/// are nullary. `to_string` on a numeric receiver is NOT here — it sets
/// `recv_type == Some(numeric)` too, but the AST routes it through the plain
/// `to_string` arm (`(recv).jet_show()`), which is the Phase-9 `BuiltinMethod` shape;
/// a numeric `to_string` carries `recv_type == Some`, so it never reaches the Phase-9
/// `recv_type.is_none()` gate — it must be covered here as a distinct op.
pub(crate) fn is_covered_numeric_method(method: &str, nargs: usize) -> bool {
    nargs == 0
        && matches!(
            method,
            "is_nan"
                | "is_infinite"
                | "is_finite"
                | "origin"
                | "count_ones"
                | "count_zeros"
                | "leading_zeros"
                | "trailing_zeros"
                | "to_i8"
                | "to_i16"
                | "to_i32"
                | "to_i64"
                | "to_int"
                | "to_u8"
                | "to_u16"
                | "to_u32"
                | "to_u64"
                | "to_f32"
                | "to_f64"
                | "to_float"
                | "to_string"
        )
}

/// c109 Phase 28: is `member` a per-type numeric bounds constant (`U8.MAX`,
/// `I32.MIN`, `Float.INFINITY`, …)? Mirrors the AST `emit_expr` Field arm's filter
/// (Source/Codegen/Expression.rs ~L226) exactly.
pub(crate) fn is_numeric_bounds_const(member: &str) -> bool {
    matches!(
        member,
        "MIN" | "MAX" | "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON"
    )
}
