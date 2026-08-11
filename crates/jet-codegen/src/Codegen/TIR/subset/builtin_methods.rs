use crate::Codegen::TIR::TNumericOp;

/// c109 Phase 9: is `method` (with `nargs` arguments) a built-in collection/string
/// method the TIR lowers? This is the NON-closure, non-numeric, non-handle slice of
/// built-in method lowering, restricted to the list/map/
/// string surface (`Source/Collections.rs`). The closure-taking methods (`map`/
/// `filter`/`each`/`find`/`any`/`all`/`sort_by`/`reduce` — `Collections::
/// is_closure_method`) are deferred to the lambda phase; the numeric width/predicate/
/// numeric queries (`is_nan`/`count_ones`/… — D-NUMOPS1) and the handle methods
/// (FileWriter/TcpStream/HTTPRequest/… — Phase 10) carry a `Some(recv_type)`, so the
/// gate's `recv_type.is_none()` guard already excludes them; this name list is the
/// final filter. The arg count disambiguates `join()` (no separator) vs `join(sep)`.
pub(crate) fn is_covered_builtin_name(method: &str, nargs: usize) -> bool {
    // Closure-taking methods are NEVER covered here (Phase 11), even by name.
    // Exception: 0-arg forms that share a name with a closure adapter
    // (ByteBuffer.position — cursor read, not Iter.position(pred)).
    if crate::Collections::is_closure_method(method) && nargs > 0 {
        return false;
    }
    // D-ZIPPAD1: zip-family arity is variadic. Sema has already validated the
    // receiver and every input/fill label; lowering carries the resolved row
    // shape, so this gate must not impose an artificial arity ceiling.
    if matches!(method, "zip" | "zip_short" | "zip_pad") {
        return true;
    }
    matches!(
        (method, nargs),
        // D-FAIL-CARRIER1=A: `.or_err("why")` lifts a clean absence into a
        // failure; `.partial` and `.notes` read the carrier's middle states.
        ("or_err", 1) | ("partial", 0) | ("notes", 0)
        // List + map shared.
        |         ("len", 0) | ("is_empty", 0) | ("clear", 0)
        // List-only.
        | ("push", 1) | ("pop", 0) | ("pop", 1) | ("first", 0) | ("last", 0)
        | ("index_of", 1) | ("reverse", 0) | ("sort", 0) | ("join", 1)
        // List + map: insert/remove/get (the Map vs List branch resolves at lowering).
        | ("insert", 2) | ("add", 2) | ("add_new", 2) | ("remove", 1 | 2) | ("get", 1)
        // List + string: contains.
        | ("contains", 1)
        // Map-only.
        | ("keys", 0) | ("values", 0) | ("has_key", 1) | ("merge", 1) | ("merge", 2)
        // String-only.
        | ("chars", 0) | ("bytes", 0) | ("trim", 0) | ("split", 1)
        | ("starts_with", 1) | ("ends_with", 1) | ("replace", 2)
        | ("to_upper", 0) | ("to_lower", 0) | ("repeat", 1) | ("slice", 2)
        | ("trim_start", 0) | ("trim_end", 0)
        | ("pad_start", 2) | ("pad_end", 2)
        | ("is_alphabetic", 0) | ("is_numeric", 0)
        | ("is_whitespace", 0) | ("is_ascii", 0)
        | ("to_title", 0) | ("split_once", 1)
        | ("is_lower", 0) | ("is_upper", 0)
        | ("capitalize", 0) | ("swapcase", 0) | ("normalize", 0)
        | ("remove_prefix", 1) | ("remove_suffix", 1)
        | ("rsplit", 1)
        | ("count", 1) | ("extend", 1) | ("concat", 1)
        // D-STR-AFTER1: first-occurrence substring split.
        | ("after", 1) | ("before", 1)
        // c97/D-STRPARSE1: parsing stays `Type.parse`.
        | ("lines", 0)
        // D-STR-DECLINE1=C: `to_int`/`to_float` — same `Int.parse`/`Float.parse`
        // builtin, string is the receiver either way. `matches`/`match` — the
        // one core.regex engine, composed for a String receiver.
        | ("to_int", 0) | ("to_float", 0)
        | ("matches", 1) | ("match", 1)
        // `to_string` (String/Bool/Char receiver — those carry `recv_type == None`;
        // a numeric `to_string` sets `recv_type` and so is excluded by the guard).
        | ("to_string", 0)
        // D-ITER1: non-closure lazy adapters.
        | ("take", 1) | ("skip", 1) | ("step_by", 1)
        | ("dedup", 0) | ("chunks", 1) | ("windows", 1)
        | ("indexed", 0) | ("indexes", 0) | ("zip", 1)
        | ("sum", 0) | ("product", 0) | ("min", 0) | ("max", 0)
        | ("flatten", 0) | ("intersperse", 1) | ("unzip", 0)
        // #1479 Iter ledger surface (non-closure).
        | ("cycle", 1) | ("drop_last", 1) | ("shuffle", 0)
        | ("is_sorted", 0) | ("average", 0)
        // D-LOOPMAP1=B: enter the lazy pipeline plane from an in-memory list.
        | ("lazy", 0)
        // D-COLLBREADTH1=A: Set<T> instance methods.
        | ("add", 1) | ("union", 1) | ("to_list", 0)
        | ("intersection", 1) | ("difference", 1)
        | ("symmetric_difference", 1)
        | ("is_subset", 1) | ("is_superset", 1) | ("is_disjoint", 1)
        | ("to_set", 0)
        // #1478: Set-only single-arg `replace`/`take` (native swap-in /
        // remove-and-return) — distinct arity from String/List's 2-arg
        // `replace` above and the Iter-adapter `take` (nargs 1 either way,
        // no collision: both already resolve by receiver type at lowering).
        | ("replace", 1)
        | ("peek", 0) | ("to_sorted_list", 0)
        | ("capacity", 0) | ("count", 0) | ("to_bytes", 0)
        | ("write_u8", 1) | ("write_byte", 1) | ("write_u16_le", 1) | ("write_u16_be", 1)
        | ("write_u32_le", 1) | ("write_u32_be", 1)
        | ("write_u64_le", 1) | ("write_u64_be", 1) | ("write_bytes", 1) | ("write", 1)
        | ("position", 0) | ("eof", 0) | ("rewind", 0) | ("flush", 0) | ("close", 0)
        | ("shutdown", 0) | ("get_buffer", 0) | ("buffer", 0) | ("string", 0)
        | ("title", 0) | ("clone", 0) | ("copy", 0) | ("read", 0)
        | ("read_byte", 0) | ("next", 0) | ("parse", 0)
        | ("seek", 1) | ("read_bytes", 1) | ("read_string", 1)
        | ("last_index_of", 1) | ("equal", 1) | ("compare", 1) | ("copy_to", 1)
        | ("binary_search", 1) | ("random", 0) | ("min_max", 0) | ("slice", 1)
        | ("contains_value", 1) | ("pop_first", 0)
        | ("write_to", 1)
        // D-TAG1: Bag<T> instance methods (add/remove share list/set arms above).
        | ("has", 1)
        // D-COLLBREADTH1=A: Deque<T> instance methods.
        | ("push_front", 1) | ("push_back", 1)
        | ("pop_front", 0) | ("pop_back", 0)
        | ("peek_front", 0) | ("peek_back", 0)
        | ("delete", 1)
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
        | ("split_write", 1)
        | ("get_disjoint_write", 1)
    )
    // NOTE: `is_empty` (now Bool-typed in `Collections::*_method_return` after the
    // c109 fix; lowered to `TBuiltinOp::IsEmpty`) is covered above. `join()` (no
    // separator) stays excluded: sema requires `join(sep)` (E0311 on no-arg), so the
    // no-arg form never reaches codegen — its AST arm is dead.
}

/// c109 Phase 21 + D-COROUTINE1=A / D-TUPLE-DESTRUCT1: is `(method, nargs)` a
/// Task/Receiver/Sender concurrency method (`emit_builtin_method`'s `Type::Apply`-
/// receiver arms)? `Task.join()/detach()/pause()/resume()/cancel()`,
/// `Receiver.receive()`, `Sender.send(v)`. The arg count disambiguates
/// `Task.join()` (0 args) from the list `join(sep)` (1 arg, shape d) and `Sender.send(v)`
/// (1 arg) — every name+arity here is disjoint from every other covered shape.
pub(crate) fn is_concurrency_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("join", 0)
            
            | ("detach", 0)
            | ("pause", 0)
            | ("pause", 1)
            | ("resume", 0)
            | ("cancel", 0)
            | ("receive", 0)
            | ("send", 1)
            | ("close", 0)
    )
}

/// D-REACT1=B: is `(method, nargs)` a reactive `Signal`/`Derived` method?
/// `Signal.get()`/`Derived.get()` (0 args), `Signal.set(v)` (1 arg). Always keyed
/// together with `recv_type == Some("Signal"|"Derived")`, never on the name alone.
pub(crate) fn is_reactive_method_name(method: &str, nargs: usize) -> bool {
    matches!((method, nargs), ("get", 0) | ("set", 1))
}

pub(crate) fn is_reactive_effect_method_name(method: &str, nargs: usize) -> bool {
    matches!((method, nargs), ("unsubscribe" | "is_active", 0))
}

pub(crate) fn is_event_handle_type(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("Event" | "AsyncEvent" | "Hook" | "DecisionHook" | "Subscription" | "EventScope" | "EventTrace" | "DispatchReport")
    )
}

pub(crate) fn is_event_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("on" | "once", 2)
            | ("on_priority", 3)
            | ("emit" | "emit_async", 1)
            | ("run", 1 | 2)
            | ("unsubscribe" | "is_active" | "cancel" | "active_count", 0)
            | ("trace" | "listener_count" | "queued_count", 0)
            | ("summary" | "delivered" | "queued" | "dropped", 0)
            | ("close" | "running_count" | "blocked_count" | "accepted" | "delivered_handlers" | "state" | "failures", 0)
    )
}

pub(crate) fn is_watch_handle_type(name: Option<&str>) -> bool {
    matches!(name, Some("WatchHandle" | "WatchSet"))
}

pub(crate) fn is_watch_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("poll" | "events" | "summary", 0)
            | ("is_active" | "cancel", 0)
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
                // D-PROCESS-SESSION1=A / D-PROCESS-SESSION2=D: beginner and
                // expert terminal opt-in plus the keyed host report.
                | ("terminal", 0 | 1)
                | ("env_clear" | "detached" | "capabilities" | "run" | "run_checked" | "spawn", 0)
                | ("timeout" | "output_limit", 1)
        ),
        Some("ProcessChild") => matches!(
            (method, nargs),
            ("id" | "wait" | "exited" | "kill" | "terminate" | "interrupt", 0)
        ),
        Some("TerminalSession") => matches!((method, nargs), ("resize", 1)),
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
        // D-UI-MOUNT1=A: measure→layout→paint; 1-arg uses the backend default viewport.
        (_, "mount", 1 | 2) => true,
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

/// D-WEBAPP1=D: is `(method, nargs)` a `WebApp` builder method?
pub(crate) fn is_webapp_method_name(method: &str, nargs: usize) -> bool {
    matches!(
        (method, nargs),
        ("route" | "page" | "layout" | "action" | "form" | "data", 2)
            | ("mount", 2 | 3 | 4)
            | ("routes" | "security" | "assets" | "split" | "code_split" | "cache" | "a11y" | "adapter", 1)
            | (
                "csr" | "ssr" | "ssg" | "stream" | "streaming" | "island" | "hydration_dev"
                    | "hydration_release" | "facts_json",
                0
            )
            | ("serve", 0 | 1)
    )
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is this an HTTP type?
pub(crate) fn is_http_type(recv_type: Option<&str>) -> bool {
    matches!(
        recv_type,
        Some(
            "HTTPRequest"
                | "HTTPClient"
                | "HTTPResponse"
                | "HTTPHeaders"
                | "HTTPBody"
                | "HTTPMux"
                | "HTTPHandler"
                | "HTTPServer"
                | "HTTPServerTls"
                | "WsConn"
                | "WsMessage"
                | "Browser"
                | "BrowserContext"
                | "BrowserPage"
                | "BrowserFrame"
                | "BrowserLocator"
                | "BrowserIntercept"
                | "BrowserEvent"
                | "BrowserTrace"
                | "BrowserReceipt"
                | "BrowserPrivacy"
                | "BrowserCapabilities"
                | "BrowserProtocol"
                | "BrowserLocked",
        )
    )
}

/// D-NETDEP1=A / D-HTTPLIB1=A: is `method` valid for this HTTP type?
pub(crate) fn is_http_method_name(recv_type: Option<&str>, method: &str) -> bool {
    match recv_type {
        Some("HTTPRequest") => matches!(
            method,
            "method" | "path" | "param" | "body_len" | "under_limit" | "header" | "body"
                | "timeout" | "connect_timeout" | "read_timeout" | "total_timeout"
                | "dns_timeout" | "tls_timeout" | "write_timeout" | "first_byte_timeout"
                | "redirects" | "proxy" | "cookie" | "form" | "multipart_text" | "send"
                | "trailers" | "json"
        ),
        Some("HTTPResponse") => matches!(method, "status" | "json" | "body" | "header" | "cookies" | "trailers" | "protocol" | "remote_address" | "redirect_history" | "timings" | "reused_connection" | "raw_content_encoding"),
        Some("HTTPClient") => matches!(method, "cookies" | "redirects" | "protocols" | "timeouts" | "raw_encoding" | "proxy" | "tls" | "allow_http_downgrade" | "retries" | "send"),
        Some("HTTPHeaders") => matches!(method, "first" | "all" | "append" | "set" | "remove"),
        Some("HTTPBody") => matches!(method, "bytes" | "text" | "json" | "chunks" | "copy_to"),
        Some("HTTPMux") => matches!(method, "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "middleware"),
        Some("HTTPHandler") => method == "handle",
        Some("HTTPServer") => matches!(method, "local_addr" | "serve" | "shutdown"),
        Some("WsConn") => matches!(method, "send_text" | "send_bytes" | "recv" | "close"),
        Some("WsMessage") => matches!(method, "is_text" | "is_binary" | "is_close" | "text" | "bytes"),
        Some("Browser") => matches!(
            method,
            "capabilities"
                | "context"
                | "subscribe"
                | "next_event"
                | "add_intercept"
                | "add_intercept_url"
                | "continue_request"
                | "fail_request"
                | "fulfill_request"
                | "allow_downloads"
                | "protocol"
                | "trace"
                | "privacy"
                | "receipt"
                | "close"
        ),
        Some("BrowserContext") => {
            matches!(method, "page" | "tab" | "close" | "isolated" | "user_hash")
        }
        Some("BrowserPage") => matches!(
            method,
            "goto"
                | "get_by_role"
                | "get_by_text"
                | "get_by_label"
                | "get_by_placeholder"
                | "get_by_test_id"
                | "get_by_css"
                | "close"
                | "main_frame"
                | "frames"
                | "screenshot"
                | "pdf"
                | "set_cookie"
                | "cookie"
                | "clear_cookies"
                | "storage_get"
                | "storage_set"
                | "storage_clear"
        ),
        Some("BrowserFrame") => method == "close",
        Some("BrowserLocator") => {
            matches!(
                method,
                "wait" | "wait_gone" | "click" | "hover" | "fill" | "press" | "set_files"
            )
        },
        Some("BrowserIntercept") => method == "remove",
        Some("BrowserEvent") => matches!(
            method,
            "kind"
                | "request_id"
                | "request_method"
                | "url_hash"
                | "is_blocked"
                | "status_code"
                | "download_id"
                | "suggested_filename_hash"
        ),
        Some("BrowserProtocol") => method == "send",
        Some("BrowserCapabilities") => matches!(method, "bidi" | "cdp" | "profile"),
        Some("BrowserTrace") => matches!(method, "entry_count" | "redacted" | "summary"),
        Some("BrowserReceipt") => {
            matches!(method, "entry_count" | "redacted" | "summary" | "isolated" | "cleaned")
        }
        Some("BrowserPrivacy") => {
            matches!(method, "isolated_profiles" | "redact_receipts" | "shared_profiles")
        }
        Some("BrowserLocked") => {
            matches!(method, "engine" | "version" | "binary" | "protocol" | "verify")
        }
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
                | "quarter_of_year"
                | "days_in_month"
                | "is_leap_year"
                | "truncate"
                | "replace"
                | "format"
                | "to_string"
        ),
        Some("LocalTime") => matches!(method, "hour" | "minute" | "second" | "to_string"),
        Some("DateTime") => matches!(
            method,
            "hour"
                | "minute"
                | "second"
                | "millisecond"
                | "microsecond"
                | "nanosecond"
                | "to_timestamp"
                | "to_unix_ms"
                | "date"
                | "time"
                | "plus_duration"
                | "difference"
                | "truncate"
                | "round"
                | "floor"
                | "ceil"
                | "replace"
                | "in_zone"
                | "format_rfc3339"
                | "format"
                | "to_string"
        ),
        Some("Instant") => matches!(method, "elapsed_millis" | "elapsed"),
        Some("Period") => matches!(method, "to_string"),
        Some("Zone") => matches!(method, "name"),
        Some("ZonedDateTime") => matches!(
            method,
            "date"
                | "time"
                | "offset_seconds"
                | "is_dst"
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

/// Resolve a numeric receiver query into a total TIR operation.
pub(crate) fn resolve_numeric_op(method: &str, src_name: &str) -> Option<TNumericOp> {
    // Float predicates → `(recv).{method}()`.
    if let "is_nan" | "is_infinite" | "is_finite" = method {
        return Some(TNumericOp::Predicate(method.to_string()));
    }
    // Integer bit-population queries → `((recv).{method}() as i64)`.
    if let "count_ones" | "count_zeros" | "leading_zeros" | "trailing_zeros" = method {
        let width = match src_name {
            "I8" | "U8" => 8,
            "I16" | "U16" => 16,
            "I32" | "U32" => 32,
            _ => 64,
        };
        return Some(TNumericOp::BitCount {
            method: method.to_string(),
            width,
        });
    }
    // `to_string` on a numeric receiver → `(recv).jet_show()` (the AST `to_string` arm).
    if method == "to_string" {
        return Some(TNumericOp::ToShow);
    }
    None
}

/// D-SHAPE-CONVERT1=A: resolve `Target.from_source(value)` to the existing
/// numeric conversion TIR operation. The call direction changes; the checked
/// widening/narrowing law does not.
pub(crate) fn resolve_numeric_conversion_op(
    target_name: &str,
    source_name: &str,
) -> Option<TNumericOp> {
    let (dst_rust, dst_int) = numeric_rust_type_tir(target_name)?;
    let Some((dsigned, dbits)) = dst_int else {
        if dst_rust == "f32" && matches!(source_name, "Float" | "F64") {
            return Some(TNumericOp::FloatNarrow {
                dst_spelling: target_name.to_string(),
            });
        }
        return Some(TNumericOp::CastAs {
            dst_rust: dst_rust.to_string(),
        });
    };
    match parse_int_name_tir(source_name) {
        Some((ssigned, sbits)) => {
            let (slo, shi) = crate::AST::int_range(ssigned, sbits);
            let (dlo, dhi) = crate::AST::int_range(dsigned, dbits);
            if dlo <= slo && shi <= dhi {
                Some(TNumericOp::CastAs {
                    dst_rust: dst_rust.to_string(),
                })
            } else {
                Some(TNumericOp::TryFrom {
                    host_kind: numeric_host_kind(dsigned, dbits)?,
                    dst_rust: dst_rust.to_string(),
                    dst_spelling: target_name.to_string(),
                })
            }
        }
        None => {
            let (lo, hi) = crate::AST::int_range(dsigned, dbits);
            Some(TNumericOp::FloatToInt {
                host_kind: numeric_host_kind(dsigned, dbits)?,
                dst_rust: dst_rust.to_string(),
                dst_spelling: target_name.to_string(),
                lower: format!("{lo}.0"),
                upper_exclusive: format!("{}.0", hi + 1),
            })
        }
    }
}

fn numeric_host_kind(signed: bool, bits: u8) -> Option<i64> {
    Some(match (signed, bits) {
        (true, 8) => 0,
        (true, 16) => 1,
        (true, 32) => 2,
        (true, 64) => 3,
        (false, 8) => 4,
        (false, 16) => 5,
        (false, 32) => 6,
        (false, 64) => 7,
        _ => return None,
    })
}

fn numeric_rust_type_tir(name: &str) -> Option<(&'static str, Option<(bool, u8)>)> {
    Some(match name {
        "I8" => ("i8", Some((true, 8))),
        "I16" => ("i16", Some((true, 16))),
        "I32" => ("i32", Some((true, 32))),
        "I64" | "Int" => ("i64", Some((true, 64))),
        "U8" => ("u8", Some((false, 8))),
        "U16" => ("u16", Some((false, 16))),
        "U32" => ("u32", Some((false, 32))),
        "U64" => ("u64", Some((false, 64))),
        "F32" => ("f32", None),
        "F64" | "Float" => ("f64", None),
        _ => return None,
    })
}

/// c109 Phase 12: TIR-local copy of `parse_int_name` —
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

/// Is `method` (with `nargs` args) a numeric predicate / bit-op the TIR lowers?
/// This is the D-NUMOPS1 slice of
/// `emit_builtin_method` keyed on a numeric receiver (`recv_type == Some(numeric)`):
/// the float predicates (`is_nan`/`is_infinite`/`is_finite`), the integer bit-pop
/// queries (`count_ones`/`count_zeros`/`leading_zeros`/`trailing_zeros`). All are
/// nullary. `to_string` on a numeric receiver is NOT here — it sets
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
                | "to_string"
        )
}

/// c109 Phase 28: is `member` a per-type numeric bounds constant (`U8.MAX`,
/// `I32.MIN`, `Float.INFINITY`, …)? Mirrors the AST `emit_expr` Field arm's filter
/// exactly.
pub(crate) fn is_numeric_bounds_const(member: &str) -> bool {
    matches!(
        member,
        "MIN" | "MAX" | "INFINITY" | "NEG_INFINITY" | "NAN" | "EPSILON"
    )
}
