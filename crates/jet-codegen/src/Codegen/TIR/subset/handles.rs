use crate::AST::{Expr, Type};
use crate::Codegen::Cx;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::TIR::expr_in_subset;
use crate::Codegen::TIR::lambda_in_subset;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::unit_type;
use crate::Syntax;
use std::collections::HashSet;

/// c109 Phase 13: resolve a handle method `(handle, method, nargs)` into a total
/// `THandleOp`, reproducing the handle arms of `emit_builtin_method`
/// (Source/Codegen/Expression.rs). Returns `None` for anything not covered (so the
/// caller falls through to other shapes). Excluded (with reason): `lines` on
/// FileReader/StdinHandle (dead — E2502, loop-source-only); all HttpRouter `get`/
/// `post`/`put`/`delete` (closure handler → `emit_router_handler`); HttpRequest/
/// HttpResponse accessors (serve-lambda-param slot may be unresolved → AST handle arm
/// wouldn't fire); Arena/Bump/Pool/Fixed (`alloc`/`reset` — the producer
/// `mem.*.new` isn't a covered call, so an allocator never binds in a covered fn);
/// Channel/Sender/Task (`receive`/`send`/`sender`/`detach` — producers not covered);
/// `Match.group` (the `Option<Match>` unwrap chain isn't cleanly reachable).
/// c109 Phase 19: is this MethodCall the arena allocator constructor `mem.Arena.new(…)`
/// (D-ALLOC1)? Reproduces `emit_method_call`'s constructor branch (Expression.rs ~L1515):
/// the receiver is `Field(Ident(alias), <AllocType>)` where `alias ∈ core_imports` maps to
/// `core.mem` and `<AllocType> ∈ {Arena,Bump,Pool,Fixed}`, and `method == "new"`. Returns
/// the resolved allocator type-name (so the gate can admit it) or `None`.
pub(crate) fn alloc_new_type<'a>(
    receiver: &'a Expr,
    method: &str,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<&'a str> {
    let Expr::Field(inner, alloc_type, _) = receiver else {
        return None;
    };
    let Expr::Ident(alias, _) = &**inner else {
        return None;
    };
    if locals.contains(alias) {
        return None;
    }
    if cx.core_imports.get(alias).map(String::as_str) != Some(Syntax::CORE_MEM_MODULE) {
        return None;
    }
    match alloc_type.as_str() {
        "Fixed" if method == "over" || method == Syntax::MEM_ALLOC_NEW => {
            Some(alloc_type.as_str())
        }
        "Arena" | "Bump" | "Pool" if method == Syntax::MEM_ALLOC_NEW => {
            Some(alloc_type.as_str())
        }
        _ => None,
    }
}

/// D-SOLVER-LIB1=A: is this `solve.Solver.new(seed)`?
pub(crate) fn solve_new_type<'a>(
    receiver: &'a Expr,
    method: &str,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<&'a str> {
    if method != "new" {
        return None;
    }
    let Expr::Field(inner, solver_type, _) = receiver else {
        return None;
    };
    let Expr::Ident(alias, _) = &**inner else {
        return None;
    };
    if locals.contains(alias) {
        return None;
    }
    if cx.core_imports.get(alias).map(String::as_str) != Some(Syntax::CORE_SOLVE_MODULE) {
        return None;
    }
    (solver_type == Syntax::SOLVER_TYPE).then_some(solver_type.as_str())
}

/// D-SHAPE-DURATION1=A: resolve a bare type-owned runtime constructor.
pub(crate) fn duration_new_unit(
    receiver: &Expr,
    method: &str,
    locals: &HashSet<String>,
) -> Option<&'static str> {
    let Expr::Ident(type_name, _) = receiver else {
        return None;
    };
    if type_name != Syntax::DURATION_TYPE || locals.contains(type_name) {
        return None;
    }
    Syntax::duration_unit_for_constructor(method)
}

pub(crate) fn game_static_type<'a>(
    receiver: &'a Expr,
    method: &str,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<&'a str> {
    let Expr::Field(inner, static_type, _) = receiver else {
        return None;
    };
    let Expr::Ident(alias, _) = &**inner else {
        return None;
    };
    if locals.contains(alias) || cx.core_imports.get(alias).map(String::as_str) != Some("core.game")
    {
        return None;
    }
    match (static_type.as_str(), method) {
        ("Scene", "new") | ("Replay", "record") | ("Backend", "headless") => {
            Some(static_type.as_str())
        }
        _ => None,
    }
}

pub(crate) fn tls_static_op(
    receiver: &Expr,
    method: &str,
    cx: &Cx,
    locals: &HashSet<String>,
) -> Option<THandleOp> {
    let Expr::Field(inner, static_type, _) = receiver else {
        return None;
    };
    let Expr::Ident(alias, _) = &**inner else {
        return None;
    };
    if locals.contains(alias) || cx.core_imports.get(alias).map(String::as_str) != Some("core.tls") {
        return None;
    }
    match (static_type.as_str(), method) {
        ("ClientConfig", "default") => Some(THandleOp::TlsClientConfigDefault),
        ("RootCertificates", "from_pem") => Some(THandleOp::TlsRootCertificatesFromPem),
        ("ClientIdentity", "from_pem") => Some(THandleOp::TlsClientIdentityFromPem),
        _ => None,
    }
}

/// c109 Phase 25: is `router.get(path, handler)` (and `.post`/`.put`/`.delete`) inside
/// the subset? Reproduces `emit_router_handler` (Source/Codegen/Expression.rs): the
/// handler (arg 1) must be either a BARE TOP-LEVEL FN name (an `Ident` not in locals —
/// the `env.get(name).is_none()` branch → the `move |__req| user_<fn>(&__req)` wrapper)
/// or an in-subset literal LAMBDA (the `Box::new(<lambda>)` branch). The path (arg 0) is
/// any in-subset value. No labels.
pub(crate) fn router_register_in_subset(
    receiver: &Expr,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &HashSet<String>,
) -> bool {
    if args.iter().any(|a| a.label.is_some()) {
        return false;
    }
    if !expr_in_subset(receiver, cx, locals) {
        return false;
    }
    if !expr_in_subset(&args[0].expr, cx, locals) {
        return false;
    }
    match &args[1].expr {
        // A bare top-level fn name (the `env.get(name).is_none()` named-fn branch). It
        // must NOT be a local (a local handler would take the `Box::new(emit_expr(…))`
        // path, which for a fn-typed local emits its own `Box::new` — still covered, but
        // we keep to the simple named-fn + lambda shapes the live suite uses).
        Expr::Ident(name, _) => !locals.contains(name),
        // A literal in-subset lambda (the `Box::new(<lambda>)` branch).
        Expr::Lambda(lam) => lambda_in_subset(lam, cx, locals),
        _ => false,
    }
}

pub(crate) fn handle_method_op(handle: &str, method: &str, nargs: usize) -> Option<THandleOp> {
    Some(match (handle, method, nargs) {
        ("FileReader", "read_line", 0) => THandleOp::FileReaderReadLine,
        ("FileWriter", "write_line", 1) => THandleOp::FileWriterWriteLine,
        ("FileWriter", "flush", 0) => THandleOp::FileWriterFlush,
        ("JSONReader", "next", 0) => THandleOp::JSONReaderNext,
        ("JSONWriter", "write", 1) => THandleOp::JSONWriterWrite,
        ("JSONWriter", "flush", 0) => THandleOp::JSONWriterFlush,
        ("JSONWriter", "finish", 0) => THandleOp::JSONWriterFinish,
        ("JSONLReader", "next", 0) => THandleOp::JSONLReaderNext,
        ("JSONLWriter", "write", 1) => THandleOp::JSONLWriterWrite,
        ("JSONLWriter", "flush", 0) => THandleOp::JSONLWriterFlush,
        ("JSONLWriter", "finish", 0) => THandleOp::JSONLWriterFinish,
        ("CSVReader", "next", 0) => THandleOp::CSVReaderNext,
        ("CSVWriter", "write", 1) => THandleOp::CSVWriterWrite,
        ("CSVWriter", "flush", 0) => THandleOp::CSVWriterFlush,
        ("CSVWriter", "finish", 0) => THandleOp::CSVWriterFinish,
        ("XMLReader", "next", 0) => THandleOp::XMLReaderNext,
        ("XMLWriter", "write", 1) => THandleOp::XMLWriterWrite,
        ("XMLWriter", "flush", 0) => THandleOp::XMLWriterFlush,
        ("XMLWriter", "finish", 0) => THandleOp::XMLWriterFinish,
        ("CBORReader", "next", 0) => THandleOp::CBORReaderNext,
        ("CBORWriter", "write", 1) => THandleOp::CBORWriterWrite,
        ("CBORWriter", "flush", 0) => THandleOp::CBORWriterFlush,
        ("CBORWriter", "finish", 0) => THandleOp::CBORWriterFinish,
        ("StdinHandle", "read_line", 0) => THandleOp::StdinReadLine,
        ("Stdout", "write", 1) => THandleOp::StdoutWrite,
        ("Stdout", "write_line", 1) => THandleOp::StdoutWriteLine,
        ("Stdout", "write_bytes", 1) => THandleOp::StdoutWriteBytes,
        ("Stdout", "flush", 0) => THandleOp::StdoutFlush,
        ("Stdout", "is_tty", 0) => THandleOp::StdoutIsTty,
        ("Stderr", "write", 1) => THandleOp::StderrWrite,
        ("Stderr", "write_line", 1) => THandleOp::StderrWriteLine,
        ("Stderr", "write_bytes", 1) => THandleOp::StderrWriteBytes,
        ("Stderr", "flush", 0) => THandleOp::StderrFlush,
        ("Stderr", "is_tty", 0) => THandleOp::StderrIsTty,
        ("Stopwatch", "elapsed_millis", 0) => THandleOp::StopwatchElapsedMillis,
        // D-DET1: deterministic injected Clock/Rng capability methods.
        ("Clock", "now", 0) => THandleOp::ClockNow,
        ("Clock", "tick", 1) => THandleOp::ClockTick,
        // D-DET-CAPAPI: absolute set + Duration advance; the widened Rng draws; Duration read.
        ("Clock", "advance", 1) => THandleOp::ClockAdvance,
        ("Clock", "wait", 1) => THandleOp::ClockWait,
        ("Rng", "int", 2) => THandleOp::RngInt,
        ("Rng", "float", 0) => THandleOp::RngFloat,
        ("Rng", "float_range", 2) => THandleOp::RngFloatRange,
        ("Rng", "bool", 0) => THandleOp::RngBool,
        ("Rng", "bool", 1) => THandleOp::RngBoolP,
        ("Rng", "normal", 2) => THandleOp::RngNormal,
        ("Rng", "exponential", 1) => THandleOp::RngExponential,
        ("Rng", "bytes", 1) => THandleOp::RngBytes,
        ("Rng", "split", 0) => THandleOp::RngSplit,
        ("Rng", "pick", 1) => THandleOp::RngPick,
        ("Rng", "weighted_pick", 2) => THandleOp::RngWeightedPick,
        ("Rng", "sample", 2) => THandleOp::RngSample,
        ("Rng", "shuffle", 1) => THandleOp::RngShuffle,
        ("Solver", "require", 1) => THandleOp::SolverRequire,
        ("Solver", "failure_count", 0) => THandleOp::SolverFailureCount,
        ("Solver", "status", 0) => THandleOp::SolverStatus,
        ("GameScene", "on_frame", 1) => THandleOp::GameSceneOnFrame,
        ("GameScene", "component", 1) => THandleOp::GameSceneComponent,
        ("GameScene", "query", 1) => THandleOp::GameSceneQuery,
        ("GameAssets", "image", 1) => THandleOp::GameAssetsImage,
        ("GameAssets", "sound", 1) => THandleOp::GameAssetsSound,
        ("GameInputMap", "bind", 2) => THandleOp::GameInputBind,
        ("GameInputSnapshot", "pressed", 1) => THandleOp::GameInputPressed,
        ("Duration", "in", 1) => THandleOp::DurationIn { unit: None },
        ("BigInt", "add" | "sub" | "mul", 1) => THandleOp::PreciseMethod {
            type_name: "BigInt".to_string(),
            method: method.to_string(),
        },
        ("BigInt", "neg" | "to_string", 0) => THandleOp::PreciseMethod {
            type_name: "BigInt".to_string(),
            method: method.to_string(),
        },
        ("Decimal", "add" | "sub" | "mul", 1) => THandleOp::PreciseMethod {
            type_name: "Decimal".to_string(),
            method: method.to_string(),
        },
        ("Decimal", "to_string", 0) => THandleOp::PreciseMethod {
            type_name: "Decimal".to_string(),
            method: method.to_string(),
        },
        ("TcpListener", "accept", 0 | 1) => THandleOp::TcpListenerAccept,
        ("TcpListener", "local_addr", 0) => THandleOp::TcpListenerLocalAddr,
        ("TcpStream", "read", 0) => THandleOp::TcpStreamRead,
        ("TcpStream", "read", 1 | 2) => THandleOp::TcpStreamReadBytes,
        ("TcpStream", "read_text", 1 | 2) => THandleOp::TcpStreamReadText,
        ("TcpStream", "write", 1 | 2) => THandleOp::TcpStreamWriteBytes,
        ("TcpStream", "write_all", 1 | 2) => THandleOp::TcpStreamWriteAllBytes,
        ("TcpStream", "write_text", 1 | 2) => THandleOp::TcpStreamWriteText,
        ("TcpStream", "shutdown", 1) => THandleOp::TcpStreamShutdown,
        ("TcpStream", "ready", 2) => THandleOp::TcpStreamReady,
        ("TcpStream", "peer_addr", 0) => THandleOp::TcpStreamPeerAddr,
        ("TcpStream", "local_addr", 0) => THandleOp::TcpStreamLocalAddr,
        ("TcpStream", "close", 0) => THandleOp::TcpStreamClose,
        ("UdpSocket", "ready", 2) => THandleOp::UdpSocketReady,
        ("UdpSocket", "close", 0) => THandleOp::UdpSocketClose,
        ("UdpSocket", "receive", 2) => THandleOp::UdpSocketReceiveDeadline,
        ("UdpSocket", "send_to", 3) => THandleOp::UdpSocketSendToDeadline,
        ("UnixListener", "accept", 1) => THandleOp::UnixListenerAcceptDeadline,
        ("UnixStream", "read", 2) => THandleOp::UnixStreamReadDeadline,
        ("UnixStream", "write_all", 2) => THandleOp::UnixStreamWriteAllDeadline,
        ("UnixStream", "ready", 2) => THandleOp::UnixStreamReady,
        ("UnixStream", "close", 0) => THandleOp::UnixStreamClose,
        ("UnixStream", "set_timeout", 1) => THandleOp::UnixStreamSetTimeout,
        ("TlsStream", "read", 2) => THandleOp::TlsStreamReadDeadline,
        ("TlsStream", "write_all", 2) => THandleOp::TlsStreamWriteAllDeadline,
        ("TlsStream", "ready", 2) => THandleOp::TlsStreamReady,
        ("TlsStream", "close", 0) => THandleOp::TlsStreamClose,
        ("TlsStream", "close_write", 1) => THandleOp::TlsStreamCloseWrite,
        ("TlsStream", "peer_identity", 0) => THandleOp::TlsStreamPeerIdentity,
        ("TlsClientConfig", "with_alpn", 1) => THandleOp::TlsClientConfigWithAlpn,
        ("TlsClientConfig", "with_trust", 1) => THandleOp::TlsClientConfigWithTrust,
        ("TlsClientConfig", "with_client_identity", 1) => THandleOp::TlsClientConfigWithIdentity,
        ("TlsClientConfig", "with_version_bounds", 2) => THandleOp::TlsClientConfigWithVersionBounds,
        // c109 Phase 19: the four arena allocators (`alloc`/`reset`). Sema sets
        // `recv_type == Some(<allocator>)` via `alloc_method_return`; the AST
        // `emit_builtin_method` arms key on the same `rty`. `Arena`/`Bump`/`Pool`/`Fixed`
        // share identical Rust method names (the engines differ; the surface doesn't).
        // D-ARGS1: ArgsSpec builder methods.
        ("ArgsSpec", "flag", 2) => THandleOp::ArgsSpecFlag,
        ("ArgsSpec", "flag_short", 3) => THandleOp::ArgsSpecFlagShort,
        ("ArgsSpec", "option", 3) => THandleOp::ArgsSpecOption,
        ("ArgsSpec", "option_short", 4) => THandleOp::ArgsSpecOptionShort,
        ("ArgsSpec", "option_default", 4) => THandleOp::ArgsSpecOptionDefault,
        ("ArgsSpec", "option_env", 4) => THandleOp::ArgsSpecOptionEnv,
        ("ArgsSpec", "option_int", 3) => THandleOp::ArgsSpecOptionInt,
        ("ArgsSpec", "option_float", 3) => THandleOp::ArgsSpecOptionFloat,
        ("ArgsSpec", "option_choice", 4) => THandleOp::ArgsSpecOptionChoice,
        ("ArgsSpec", "repeat", 3) => THandleOp::ArgsSpecRepeat,
        ("ArgsSpec", "required_option", 3) => THandleOp::ArgsSpecRequiredOption,
        ("ArgsSpec", "positional", 2) => THandleOp::ArgsSpecPositional,
        ("ArgsSpec", "subcommand", 3) => THandleOp::ArgsSpecSubcommand,
        ("ArgsSpec", "version", 1) => THandleOp::ArgsSpecVersion,
        ("ArgsSpec", "completion", 1) => THandleOp::ArgsSpecCompletion,
        ("ArgsSpec", "help", 0) => THandleOp::ArgsSpecHelp,
        ("ArgsSpec", "parse", 1) => THandleOp::ArgsSpecParse,
        // D-ANY-JAI1 (c7jaiany §6): reflect.of(x)'s Value/Field handle methods.
        ("Value", "type_name", 0) => THandleOp::ReflectValueTypeName,
        ("Value", "display", 0) => THandleOp::ReflectValueDisplay,
        ("Value", "fields", 0) => THandleOp::ReflectValueFields,
        ("Field", "name", 0) => THandleOp::ReflectFieldName,
        ("Field", "value", 0) => THandleOp::ReflectFieldValue,
        // D-ARGS1: ParsedArgs query methods.
        ("ParsedArgs", "flag", 1) => THandleOp::ParsedArgsFlag,
        ("ParsedArgs", "option", 1) => THandleOp::ParsedArgsOption,
        ("ParsedArgs", "option_int", 1) => THandleOp::ParsedArgsOptionInt,
        ("ParsedArgs", "option_float", 1) => THandleOp::ParsedArgsOptionFloat,
        ("ParsedArgs", "options", 1) => THandleOp::ParsedArgsOptions,
        ("ParsedArgs", "positional", 1) => THandleOp::ParsedArgsPositional,
        ("ParsedArgs", "subcommand", 0) => THandleOp::ParsedArgsSubcommand,
        ("Arena" | "Bump" | "Pool" | "Fixed", "alloc", 1) => THandleOp::AllocAlloc,
        ("Arena" | "Bump" | "Pool" | "Fixed", "reset", 0) => THandleOp::AllocReset,
        // c109 Phase 20: HttpRequest/HttpResponse accessors (E2-M10, D-ROUTE1=A).
        // Now reachable because the `http.serve` lambda param type is written back
        // onto `p.ty` (sema), so the slot type is total. The AST `emit_builtin_method`
        // arms key on the same `rty == Some(HttpRequest|HttpResponse)`. Reproduced
        // byte-for-byte in `emit_tir_handle_method`.
        ("HttpRequest", "method", 0) => THandleOp::HttpReqField("method"),
        ("HttpRequest", "path", 0) => THandleOp::HttpReqField("path"),
        ("HttpRequest", "body", 0) => THandleOp::HttpReqField("body"),
        ("HttpRequest", "header", 1) => THandleOp::HttpReqHeader,
        ("HttpRequest", "param", 1) => THandleOp::HttpReqParam,
        ("HttpResponse", "status", 0) => THandleOp::HttpRespField("status"),
        ("HttpResponse", "body", 0) => THandleOp::HttpRespField("body"),
        ("HttpResponse", "header", 1) => THandleOp::HttpRespHeader,
        // D-SERDE-ACCESS=B: DataTree accessor methods.
        ("DataTree", "field", 1) => THandleOp::DataTreeField,
        ("DataTree", "at", 1) => THandleOp::DataTreeAt,
        ("DataTree", "int", 0) => THandleOp::DataTreeInt,
        ("DataTree", "text", 0) => THandleOp::DataTreeText,
        ("DataTree", "bool", 0) => THandleOp::DataTreeBool,
        ("DataTree", "float", 0) => THandleOp::DataTreeFloat,
        // D-SERDE-ACCESS=B: same accessors on Json/Data (the dynamic parse result).
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "field", 1) => THandleOp::JsonField,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "at", 1) => THandleOp::JsonAt,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "int", 0) => THandleOp::JsonInt,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "text", 0) => THandleOp::JsonText,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "bool", 0) => THandleOp::JsonBool,
        ("Data" | "Json" | "Toml" | "Yaml" | "Csv", "float", 0) => THandleOp::JsonFloat,
        (
            "Url",
            "scheme" | "host" | "port" | "path" | "path_segments" | "query" | "query_pairs"
            | "fragment" | "normalize" | "to_string",
            0,
        ) => THandleOp::UrlMimeMethod {
            kind: "Url".to_string(),
            method: method.to_string(),
        },
        ("Url", "join", 1) => THandleOp::UrlMimeMethod {
            kind: "Url".to_string(),
            method: method.to_string(),
        },
        ("Url", "set_query" | "add_query", 2) => THandleOp::UrlMimeMethod {
            kind: "Url".to_string(),
            method: method.to_string(),
        },
        ("Mime", "media_type" | "subtype" | "essence" | "params" | "to_string", 0) => {
            THandleOp::UrlMimeMethod {
                kind: "Mime".to_string(),
                method: method.to_string(),
            }
        }
        ("Mime", "param", 1) => THandleOp::UrlMimeMethod {
            kind: "Mime".to_string(),
            method: method.to_string(),
        },
        ("Message", "envelope", 0) | ("Message", "with_envelope", 1) | ("Mailer", "send", 1) => THandleOp::EmailMethod {
            method: method.to_string(),
        },
        ("Regex", "is_match" | "match" | "find" | "find_all" | "matches" | "split", 1) => {
            THandleOp::RegexMethod {
                kind: "Regex".to_string(),
                method: method.to_string(),
            }
        }
        ("Regex", "replace" | "replace_all" | "replace_all_with" | "split_limit", 2) => {
            THandleOp::RegexMethod {
                kind: "Regex".to_string(),
                method: method.to_string(),
            }
        }
        ("Match", "start" | "end", 0) => THandleOp::RegexMethod {
            kind: "Match".to_string(),
            method: method.to_string(),
        },
        ("Match", "group" | "name" | "group_start" | "group_end", 1) => THandleOp::RegexMethod {
            kind: "Match".to_string(),
            method: method.to_string(),
        },
        // D-PATHFS1: typed Path instance methods.
        ("Path", "join", 1) => THandleOp::PathJoin,
        ("Path", "parent", 0) => THandleOp::PathParent,
        ("Path", "extension", 0) => THandleOp::PathExtension,
        ("Path", "stem", 0) => THandleOp::PathStem,
        ("Path", "to_string", 0) => THandleOp::PathToString,
        ("Path", "write_atomic", 1) => THandleOp::PathWriteAtomic,
        ("Path", "walk", 0) => THandleOp::PathWalk,
        // D-DBDRIVER1: `DbConnection` instance methods.
        ("DbConnection", "query", 2) => THandleOp::DbQuery,
        ("DbConnection", "query_one", 2) => THandleOp::DbQueryOne,
        ("DbConnection", "execute", 2) => THandleOp::DbExecute,
        ("DbConnection", "begin", 0) => THandleOp::DbBegin,
        ("DbConnection", "commit", 0) => THandleOp::DbCommit,
        ("DbConnection", "rollback", 0) => THandleOp::DbRollback,
        ("DbConnection", "close", 0) => THandleOp::DbClose,
        // D-DBDRIVER1: `DbValue` accessor methods.
        ("DbValue", "int", 0) => THandleOp::DbValueInt,
        ("DbValue", "float", 0) => THandleOp::DbValueFloat,
        ("DbValue", "text", 0) => THandleOp::DbValueText,
        ("DbValue", "bool", 0) => THandleOp::DbValueBool,
        ("DbValue", "is_null", 0) => THandleOp::DbValueIsNull,
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): `Plugin` instance methods.
        ("Plugin", "call", 2) => THandleOp::PluginCall,
        ("Plugin", "call_int", 2) => THandleOp::PluginCallInt,
        // D-SHIFT1 (c7shift) / D-BINPAT1 (card #506 follow-up): `Reader`
        // instance methods. `take_pattern` isn't here — an argument-dependent
        // method (like Cursor's), resolved at its call site.
        ("Reader", "read_u8", 0) => THandleOp::ReaderReadU8,
        ("Reader", "read_u16_le", 0) => THandleOp::ReaderReadU16Le,
        ("Reader", "read_u16_be", 0) => THandleOp::ReaderReadU16Be,
        ("Reader", "read_u32_le", 0) => THandleOp::ReaderReadU32Le,
        ("Reader", "read_u32_be", 0) => THandleOp::ReaderReadU32Be,
        ("Reader", "read_u64_le", 0) => THandleOp::ReaderReadU64Le,
        ("Reader", "read_u64_be", 0) => THandleOp::ReaderReadU64Be,
        ("Reader", "take", 1) => THandleOp::ReaderTake,
        ("Reader", "remaining", 0) => THandleOp::ReaderRemaining,
        ("Reader", "is_at_end", 0) => THandleOp::ReaderAtEnd,
        // D-SHIFT1: `Cursor` instance methods (excluding `take_pattern`).
        ("Cursor", "take_until", 1) => THandleOp::CursorTakeUntil,
        ("Cursor", "skip_ws", 0) => THandleOp::CursorSkipWs,
        // D-SIMD2 / D-LINALG1 math methods are handled by a dedicated gate + lowering
        // block (user-type-aware via `cx.type_names`), NOT here — `handle_method_op`
        // has no `cx`, and a user struct may share a math name.
        _ => return None,
    })
}

/// c109 Phase 13: the resolved return type of a covered handle method, read from the
/// authoritative sema handle tables (`file_handle_method_return`/`net_method_return`,
/// Source/Sema/CheckerCoreLib.rs) — a pure `(handle, method)` dispatch, no inference.
/// The return type is rarely load-bearing in emit (a binding carries sema's `b.ty`),
/// but kept total per the design principle. A throwaway diags vec absorbs the table's
/// diagnostic side-channel (sema already validated, so none fire here).
pub(crate) fn handle_method_return_ty(handle: &str, method: &str, nargs: usize) -> Type {
    let span = crate::Diagnostics::Span { start: 0, end: 0 };
    let mut sink = Vec::new();
    let ret = crate::Sema::file_handle_method_return(handle, method, nargs, span, &mut sink)
        .or_else(|| crate::Sema::encoding_handle_method_return(handle, method, nargs))
        .or_else(|| crate::Sema::net_method_return(handle, method, nargs, span, &mut sink))
        .or_else(|| crate::Sema::path_method_return(handle, method, nargs, span, &mut sink))
        .or_else(|| {
            if handle == "DbConnection" {
                Some(crate::Sema::db_connection_method_return_ty(method))
            } else {
                None
            }
        })
        .or_else(|| {
            if handle == "Plugin" {
                Some(crate::Sema::plugin_method_return_ty(method))
            } else {
                None
            }
        })
        .or_else(|| {
            if is_db_value_type_name(handle) {
                Some(crate::Sema::db_value_method_return(method, nargs))
            } else {
                None
            }
        })
        .or_else(|| {
            // D-ANY-JAI1 (c7jaiany §6): `Value`/`Field` (`reflect.of(x)`) —
            // needed so `.fields()`'s element type (`[Field]`) is total for a
            // chained/looped access (`loop f; v.fields() { f.name() }`).
            if crate::Sema::is_reflect_type_name(handle) {
                Some(crate::Sema::reflect_method_return(handle, method, nargs))
            } else {
                None
            }
        })
        .or_else(|| {
            if handle == crate::Syntax::TYPE_BIGINT
                || handle == crate::Syntax::TYPE_DECIMAL
                || handle == crate::Syntax::DURATION_TYPE
            {
                crate::Collections::builtin_method_return(
                    &Type::Named(handle.to_string()),
                    method,
                    nargs,
                    false,
                )
            } else {
                None
            }
        })
        .or_else(|| match (handle, method, nargs) {
            ("Url", "scheme" | "path" | "query" | "to_string", 0) => Some(Some(Type::String)),
            ("Url", "host" | "fragment", 0) => Some(Some(Type::Option(Box::new(Type::String)))),
            ("Url", "port", 0) => Some(Some(Type::Option(Box::new(Type::Int)))),
            ("Url", "path_segments", 0) => Some(Some(Type::List(Box::new(Type::String)))),
            ("Url", "query_pairs", 0) => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            ("Url", "normalize" | "set_query" | "add_query", _) => {
                Some(Some(Type::Named("Url".to_string())))
            }
            ("Url", "join", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("Url".to_string())),
                err: Box::new(Type::String),
            })),
            ("Mime", "media_type" | "subtype" | "essence" | "to_string", 0) => {
                Some(Some(Type::String))
            }
            ("Mime", "param", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
            ("Mime", "params", 0) => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            ("Message", "envelope", 0) => Some(Some(Type::Named("Envelope".to_string()))),
            ("Message", "with_envelope", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("Message".to_string())),
                err: Box::new(Type::Named("EmailError".to_string())),
            })),
            ("Mailer", "send", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("SendReport".to_string())),
                err: Box::new(Type::Named("EmailError".to_string())),
            })),
            ("Regex", "is_match", 1) => Some(Some(Type::Bool)),
            ("Regex", "match", 1) => Some(Some(Type::Option(Box::new(Type::Named(
                "Match".to_string(),
            ))))),
            ("Regex", "find", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
            ("Regex", "find_all" | "split", 1) => Some(Some(Type::List(Box::new(Type::String)))),
            ("Regex", "matches", 1) => {
                Some(Some(Type::List(Box::new(Type::Named("Match".to_string())))))
            }
            ("Regex", "replace" | "replace_all" | "replace_all_with", 2) => {
                Some(Some(Type::String))
            }
            ("Regex", "split_limit", 2) => Some(Some(Type::List(Box::new(Type::String)))),
            ("Match", "group" | "name", 1) => Some(Some(Type::Option(Box::new(Type::String)))),
            ("Match", "start" | "end", 0) => Some(Some(Type::Int)),
            ("Match", "group_start" | "group_end", 1) => {
                Some(Some(Type::Option(Box::new(Type::Int))))
            }
            _ => None,
        })
        // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — `take_pattern`
        // is excluded (like `Arena.alloc` above), resolved
        // directly at its call site since its return type depends on the
        // pattern literal's holes.
        .or_else(|| crate::Sema::binary_reader_method_return(handle, method, nargs))
        .or_else(|| crate::Sema::text_cursor_method_return(handle, method, nargs))
        .or_else(|| {
            if handle == crate::Syntax::SOLVER_TYPE {
                crate::Collections::builtin_method_return(
                    &Type::Named(crate::Syntax::SOLVER_TYPE.to_string()),
                    method,
                    nargs,
                    false,
                )
            } else {
                None
            }
        })
        .or_else(|| match (handle, method, nargs) {
            ("GameAssets", "image", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("GameImage".to_string())),
                err: Box::new(Type::String),
            })),
            ("GameAssets", "sound", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("GameSound".to_string())),
                err: Box::new(Type::String),
            })),
            ("GameScene", "query", 1) => Some(Some(Type::List(Box::new(Type::String)))),
            ("GameInputSnapshot", "pressed", 1) => Some(Some(Type::Bool)),
            ("GameScene", "on_frame" | "component", 1)
            | ("GameInputMap", "bind", 2) => Some(None),
            _ => None,
        });
    match ret {
        Some(Some(t)) => t,
        _ => unit_type(),
    }
}

/// c109 Phase 13: the resolved return type of a closure-taking core call, matching
/// `infer_core_call` (Source/Sema/CheckerCoreLib.rs). `spawn` → `Task<elem>` (the
/// closure's body type — total from the lowered lambda's return); `serve` → Unit (runs
/// forever); `guard` → `ScopeGuard`. These types are rarely load-bearing in emit (a
/// binding carries sema's `b.ty`), but kept total per the design principle.
pub(crate) fn core_closure_call_return_ty(module: &str, method: &str, body_ty: Type) -> Type {
    match (module, method) {
        ("core.tasks", "spawn") => Type::Apply {
            name: "Task".to_string(),
            args: vec![body_ty],
        },
        ("core.scope", "guard") => Type::Named("ScopeGuard".to_string()),
        _ => unit_type(),
    }
}

/// c109 Phase 10: the resolved return type of a covered core call, read from the
/// authoritative `Sema::core_fixed_sig` table (totality). A `None` return (a
/// void-effect call like `fs.write`/`env.set`/`process.exit`) lowers to `Unit`.
pub(crate) fn core_call_return_ty(module: &str, method: &str) -> Type {
    // c109 Phase 25: the http producer/parse/dispatch calls aren't in `core_fixed_sig`;
    // their return types are fixed (sema's `infer_core_call`). Carried total per the
    // design principle (the binding's annotation/inference is the load-bearing fact, but
    // this keeps the node's `ty` honest — `dispatch` → HttpResponse composes with the
    // `.status()`/`.body()` accessors that read it).
    match (module, method) {
        ("jet.http", "router") => return Type::Named("HttpRouter".to_string()),
        ("jet.http", "parse") => return Type::Named("HttpRequest".to_string()),
        ("jet.http", "dispatch") => return Type::Named("HttpResponse".to_string()),
        // c109 Phase 29: qualified `io.input(prompt)`. NOT in `core_fixed_sig` — its return
        // type is fixed (`Result<String, IOError>`) but lives in sema's bespoke
        // `infer_core_call` arm (CheckerCoreLib.rs `("core.io", "input")`), NOT the table.
        // Same type the ambient bare `input(...)` (Phase 25 `AmbientInput`) carries, so it
        // composes with the Phase-8 `??`/`?? return <value>` fallback.
        ("core.io", "input") => {
            return Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
            }
        }
        // D-HONESTNUM1=A: `M.from(value, uncertainty)` → `Measurement<Float>`.
        ("core.science.measurement", "from") => {
            return Type::Apply {
                name: Syntax::TYPE_MEASUREMENT.to_string(),
                args: vec![Type::Float],
            }
        }
        // D-PENDING1=B: Loadable constructors — type carries T from the loaded(val) arg.
        ("core.reactive.loadable", "idle") | ("core.reactive.loadable", "loading") => {
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![
                    Type::Named("Unknown".to_string()),
                    Type::Named("Unknown".to_string()),
                ],
            }
        }
        ("core.reactive.loadable", "loaded") => {
            // Type is Loadable<T, Unknown> — T comes from the arg; Unknown for E.
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![Type::Int, Type::Named("Unknown".to_string())], // sema refines T
            };
        }
        ("core.reactive.loadable", "failed") => {
            return Type::Apply {
                name: "Loadable".to_string(),
                args: vec![Type::Named("Unknown".to_string()), Type::String], // sema refines E
            };
        }
        // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` always returns `Value`,
        // regardless of the arg's type.
        ("core.reflect", "of") => return Type::Named("Value".to_string()),
        // D-APPROX1=A: sketch constructors → opaque named types.
        ("core.sketch.hll", "new") => return Type::Named("HyperLogLog".to_string()),
        ("core.sketch.tdigest", "new") => return Type::Named("TDigest".to_string()),
        ("core.sketch.cms", "new") => return Type::Named("CountMinSketch".to_string()),
        ("core.sketch.reservoir", "new") => return Type::Named("ReservoirSampler".to_string()),
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") | ("core.time.date", "today") => {
            return Type::Named("Date".to_string())
        }
        ("core.time.date", "parse") => {
            return Type::Result {
                ok: Box::new(Type::Named("Date".to_string())),
                err: Box::new(Type::String),
            }
        }
        ("core.time.datetime", "from_timestamp") | ("core.time.datetime", "now") => {
            return Type::Named("DateTime".to_string())
        }
        // D-TTLVAL1=A: Expiring<T> / Rotting<T> constructors — T from arg 0.
        ("core.time.expiring", "new") => {
            return Type::Apply {
                name: "Expiring".to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }
        }
        ("core.vault", "rotting_new") => {
            return Type::Apply {
                name: "Rotting".to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }
        }
        // D-EVENT1=D: generic constructors; sema normally writes the precise
        // resolved return type and lowering reads that. Placeholders keep node
        // totality for defensive/fallback paths.
        ("core.event", "new" | "with_policy") => {
            return Type::Apply {
                name: "Event".to_string(),
                args: vec![Type::Named("Unknown".to_string())],
            }
        }
        ("core.event", "async_result") => {
            return Type::Result {
                ok: Box::new(Type::Apply {
                    name: "AsyncEvent".to_string(),
                    args: vec![Type::Named("Unknown".to_string()), Type::Named("Unknown".to_string())],
                }),
                err: Box::new(Type::Named("EventConfigError".to_string())),
            }
        }
        ("core.event", "hook") => {
            return Type::Apply {
                name: "Hook".to_string(),
                args: vec![
                    Type::Named("Unknown".to_string()),
                    Type::Named("Unknown".to_string()),
                ],
            }
        }
        ("core.event", "scope") => return Type::Named("EventScope".to_string()),
        ("core.event", "policy_sync" | "policy_async") => {
            return Type::Named("EventPolicy".to_string())
        }
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP constructors.
        ("core.http.client", "get") | ("core.http.client", "post") => {
            return Type::Result {
                ok: Box::new(Type::Named("HttpClientResp".to_string())),
                err: Box::new(Type::String),
            }
        }
        ("core.http.client", "request") => return Type::Named("HttpClientReq".to_string()),
        ("core.http.server", "mux") => return Type::Named("HttpMux".to_string()),
        ("core.http.server", "bind") => return Type::Result {
            ok: Box::new(Type::Named("HttpServer".to_string())),
            err: Box::new(Type::String),
        },
        ("core.http.server", "tls") => return Type::Named("HttpServerTls".to_string()),
        ("core.http.server", "serve" | "serve_once" | "serve_once_listener") => {
            return Type::Result {
                ok: Box::new(Type::Tuple(vec![])),
                err: Box::new(Type::String),
            }
        }
        ("core.http.server", "static_file" | "static_file_range") => {
            return Type::Result {
                ok: Box::new(Type::Named("HttpSrvResp".to_string())),
                err: Box::new(Type::String),
            }
        }
        ("core.http.server", "response") => return Type::Named("HttpSrvResp".to_string()),
        ("core.http.server", "sse") => return Type::Named("HttpSrvResp".to_string()),
        ("core.http.server", "access_log") => return Type::String,
        _ => {}
    }
    crate::Sema::core_fixed_sig(module, method)
        .and_then(|(_, ret)| ret)
        .unwrap_or_else(unit_type)
}

// ---------------------------------------------------------------------------
// Lowering: AST -> TIR. This is where every fact is resolved ONCE.
// ---------------------------------------------------------------------------
