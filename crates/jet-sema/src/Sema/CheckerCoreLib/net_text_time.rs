use crate::AST::{AccessConvention, CallArg, ParamZone, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Checker;
use crate::Sema::Effects::Effect;
use super::alloc_ptrs::result_ty;
use super::core_types::{u8_ty, unit_ty};
use super::serde_diags::wrong_core_arity;

impl<'a> Checker<'a> {
    /// D-BROWSER-AUTO1=A: one argument checker for every Browser handle method.
    /// Return-shape lookup stays in `net_method_return`; this seam owns arity,
    /// argument types, and the ordinary read-only access convention.
    pub(crate) fn check_browser_method_args(
        &mut self,
        type_name: &str,
        method: &str,
        args: &mut [CallArg],
        span: Span,
    ) -> bool {
        let expected = match (type_name, method) {
            ("Browser", "capabilities" | "context" | "close" | "trace" | "privacy" | "receipt")
            | ("BrowserContext", "page" | "tab" | "close" | "isolated" | "user_hash")
            | ("BrowserPage", "close" | "main_frame" | "frames" | "screenshot" | "pdf"
                | "clear_cookies")
            | ("BrowserFrame", "close")
            | ("BrowserLocator", "click" | "hover")
            | ("BrowserIntercept", "remove")
            | ("BrowserEvent", "kind" | "request_id" | "request_method" | "url_hash"
                | "is_blocked" | "status_code" | "download_id" | "suggested_filename_hash")
            | ("BrowserCapabilities", "bidi" | "cdp" | "profile")
            | ("BrowserTrace", "entry_count" | "redacted" | "summary")
            | ("BrowserReceipt", "entry_count" | "redacted" | "summary" | "isolated" | "cleaned")
            | ("BrowserPrivacy", "isolated_profiles" | "redact_receipts" | "shared_profiles")
            | ("BrowserLocked", "engine" | "version" | "binary" | "protocol") => Vec::new(),
            ("BrowserLocked", "verify") => Vec::new(),
            ("Browser", "subscribe" | "protocol" | "add_intercept" | "continue_request"
                | "fail_request" | "allow_downloads")
            | ("BrowserPage", "goto" | "get_by_text" | "get_by_label" | "get_by_placeholder"
                | "get_by_test_id" | "get_by_css" | "cookie" | "storage_clear")
            | ("BrowserLocator", "fill" | "press" | "set_files") => {
                vec![Type::String]
            }
            ("Browser", "next_event") | ("BrowserLocator", "wait" | "wait_gone") => {
                vec![Type::Named("BrowserTimeout".to_string())]
            }
            ("Browser", "add_intercept_url")
            | ("BrowserPage", "get_by_role" | "storage_get")
            | ("BrowserProtocol", "send") => {
                vec![Type::String, Type::String]
            }
            ("Browser", "fulfill_request") => {
                vec![Type::String, Type::Int, Type::String]
            }
            ("BrowserPage", "set_cookie" | "storage_set") => {
                vec![Type::String, Type::String, Type::String]
            }
            _ => return false,
        };
        if matches!(
            (type_name, method),
            ("Browser", "context" | "subscribe" | "close" | "next_event" | "protocol"
                | "add_intercept" | "add_intercept_url" | "continue_request" | "fail_request"
                | "fulfill_request" | "allow_downloads")
                | ("BrowserContext", "page" | "tab" | "close")
                | ("BrowserPage", "goto" | "close" | "frames" | "screenshot" | "pdf"
                    | "set_cookie" | "cookie" | "clear_cookies" | "storage_get" | "storage_set"
                    | "storage_clear")
                | ("BrowserFrame", "close")
                | ("BrowserIntercept", "remove")
                | ("BrowserLocator", "wait" | "wait_gone" | "click" | "hover" | "fill" | "press"
                    | "set_files")
                | ("BrowserProtocol", "send")
        ) {
            self.record_effect(Effect::Net.name(), span);
        }
        if matches!((type_name, method), ("BrowserLocked", "verify")) {
            self.record_effect(Effect::FS.name(), span);
        }

        if args.len() != expected.len() {
            self.diags.push(wrong_core_arity(
                method,
                expected.len(),
                args.len(),
                span,
            ));
        }
        for (index, arg) in args.iter_mut().enumerate() {
            if let Some(param_ty) = expected.get(index) {
                self.expect_core_arg(method, index, param_ty, arg);
            } else {
                self.infer(&mut arg.expr);
            }
        }
        true
    }
}

/// E2-M10: type-check a method call on a networking opaque type.
/// Returns `Some(return_type)` when the method is valid.
pub fn net_method_return(
    type_name: &str,
    method: &str,
    n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    let str_ty = Type::String;
    let unit = unit_ty();
    let err = Type::Named("NetError".to_string());
    match (type_name, method) {
        // D-HTTP-CORE2=A: one request/response model for both HTTP roles.
        ("HTTPResponse", "status") => Some(Some(Type::Int)),
        ("HTTPResponse", "body") => Some(Some(Type::Named("HTTPBody".to_string()))),
        ("HTTPResponse", "header") if n_args == 1 => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        ("HTTPResponse", "cookies") => Some(Some(Type::List(Box::new(Type::String)))),
        ("HTTPResponse", "header") if n_args == 2 => Some(Some(Type::Named("HTTPResponse".to_string()))),
        ("HTTPResponse", "trailers") if n_args == 1 => Some(Some(Type::Result {
            ok: Box::new(Type::Named("HTTPResponse".to_string())),
            err: Box::new(Type::Named("HTTPError".to_string())),
        })),
        ("HTTPRequest", "method" | "path") => Some(Some(str_ty.clone())),
        ("HTTPRequest", "body") if n_args == 0 => Some(Some(Type::Named("HTTPBody".to_string()))),
        ("HTTPRequest", "trailers") if n_args == 0 => Some(Some(Type::Result {
            ok: Box::new(Type::Named("HTTPHeaders".to_string())),
            err: Box::new(Type::Named("HTTPError".to_string())),
        })),
        ("HTTPRequest", "header") if n_args == 1 => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        ("HTTPRequest", "header" | "body" | "timeout" | "connect_timeout" | "read_timeout"
            | "total_timeout" | "dns_timeout" | "tls_timeout" | "write_timeout"
            | "first_byte_timeout" | "redirects" | "proxy" | "cookie" | "form" | "multipart_text") => {
                Some(Some(Type::Named("HTTPRequest".to_string())))
            }
        ("HTTPRequest", "send") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("HTTPResponse".to_string())),
            err: Box::new(Type::Named("HTTPError".to_string())),
        })),
        ("HTTPRequest", "body_len") => Some(Some(Type::Int)),
        ("HTTPRequest", "under_limit") => Some(Some(Type::Bool)),
        ("HTTPHeaders", "first") => Some(Some(Type::Option(Box::new(Type::String)))),
        ("HTTPHeaders", "all") => Some(Some(Type::List(Box::new(Type::String)))),
        ("HTTPHeaders", "append" | "set") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("HTTPHeaders".to_string())),
            err: Box::new(Type::Named("HTTPError".to_string())),
        })),
        ("HTTPHeaders", "remove") => Some(Some(Type::Named("HTTPHeaders".to_string()))),
        // D-ROUTE1=A: req.param("name") → String? (none if not a param route or name absent).
        ("HTTPRequest", "param") => Some(Some(Type::Option(Box::new(str_ty.clone())))),
        // D-WS1=B: WebSocket connection and message methods.
        ("WsConn", "send_text") if n_args == 1 => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        ("WsConn", "send_bytes") if n_args == 1 => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        ("WsConn", "recv") if n_args == 0 => Some(Some(Type::Result {
            ok: Box::new(Type::Named("WsMessage".to_string())),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        ("WsConn", "close") if n_args == 2 => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        ("WsMessage", "is_text" | "is_binary" | "is_close") if n_args == 0 => {
            Some(Some(Type::Bool))
        }
        ("WsMessage", "text") if n_args == 0 => Some(Some(Type::Result {
            ok: Box::new(str_ty.clone()),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        ("WsMessage", "bytes") if n_args == 0 => Some(Some(Type::Result {
            ok: Box::new(Type::List(Box::new(u8_ty()))),
            err: Box::new(Type::Named("WsError".to_string())),
        })),
        // D-BROWSER-AUTO1=A: native BiDi handles.
        ("Browser", "capabilities") => {
            Some(Some(Type::Named("BrowserCapabilities".to_string())))
        }
        ("Browser", "context") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserContext".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "subscribe") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "close") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "next_event") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserEvent".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "add_intercept" | "add_intercept_url") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserIntercept".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "continue_request" | "fail_request" | "fulfill_request"
            | "allow_downloads") => {
            Some(Some(Type::Result {
                ok: Box::new(unit.clone()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            }))
        }
        ("Browser", "protocol") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserProtocol".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("Browser", "trace") => {
            Some(Some(Type::Named("BrowserTrace".to_string())))
        }
        ("Browser", "privacy") => {
            Some(Some(Type::Named("BrowserPrivacy".to_string())))
        }
        ("Browser", "receipt") => {
            Some(Some(Type::Named("BrowserReceipt".to_string())))
        }
        ("BrowserContext", "page" | "tab") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserPage".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserContext", "close") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserContext", "isolated") => Some(Some(Type::Bool)),
        ("BrowserContext", "user_hash") => Some(Some(Type::String)),
        ("BrowserPage", "goto") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "close") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "screenshot" | "pdf") => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "set_cookie" | "clear_cookies" | "storage_set" | "storage_clear") => {
            Some(Some(Type::Result {
                ok: Box::new(unit.clone()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            }))
        }
        ("BrowserPage", "cookie" | "storage_get") => Some(Some(Type::Result {
            ok: Box::new(Type::Option(Box::new(Type::String))),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "main_frame") => Some(Some(Type::Result {
            ok: Box::new(Type::Named("BrowserFrame".to_string())),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "frames") => Some(Some(Type::Result {
            ok: Box::new(Type::List(Box::new(Type::Named("BrowserFrame".to_string())))),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserPage", "get_by_role" | "get_by_text" | "get_by_label" | "get_by_placeholder"
            | "get_by_test_id" | "get_by_css") => {
            Some(Some(Type::Named("BrowserLocator".to_string())))
        }
        ("BrowserFrame", "close") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserLocator", "wait" | "wait_gone" | "click" | "hover" | "fill" | "press"
            | "set_files") => {
            Some(Some(Type::Result {
                ok: Box::new(unit.clone()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            }))
        }
        ("BrowserIntercept", "remove") => Some(Some(Type::Result {
            ok: Box::new(unit.clone()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserEvent", "kind" | "request_id" | "request_method" | "url_hash"
            | "download_id" | "suggested_filename_hash") => {
            Some(Some(Type::String))
        }
        ("BrowserEvent", "is_blocked") => Some(Some(Type::Bool)),
        ("BrowserEvent", "status_code") => Some(Some(Type::Int)),
        ("BrowserProtocol", "send") => Some(Some(Type::Result {
            ok: Box::new(Type::String),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        ("BrowserCapabilities", "bidi" | "cdp") => Some(Some(Type::Bool)),
        ("BrowserCapabilities", "profile") => Some(Some(Type::String)),
        ("BrowserTrace", "entry_count") => Some(Some(Type::Int)),
        ("BrowserTrace", "redacted") => Some(Some(Type::Bool)),
        ("BrowserTrace", "summary") => Some(Some(Type::String)),
        ("BrowserReceipt", "entry_count") => Some(Some(Type::Int)),
        ("BrowserReceipt", "redacted" | "isolated" | "cleaned") => Some(Some(Type::Bool)),
        ("BrowserReceipt", "summary") => Some(Some(Type::String)),
        ("BrowserPrivacy", "isolated_profiles" | "redact_receipts" | "shared_profiles") => {
            Some(Some(Type::Bool))
        }
        ("BrowserLocked", "engine" | "version" | "binary" | "protocol") => {
            Some(Some(Type::String))
        }
        ("BrowserLocked", "verify") => Some(Some(Type::Result {
            ok: Box::new(unit_ty()),
            err: Box::new(Type::Named("BrowserError".to_string())),
        })),
        // D-ROUTE1=A: HTTPRouter registration methods.
        ("HTTPRouter", "get" | "post" | "put" | "delete") => Some(Some(unit.clone())),
        // TcpListener methods.
        ("TcpListener", "accept") if n_args <= 1 => Some(Some(result_ty(
            Type::Named("TcpStream".to_string()),
            err.clone(),
        ))),
        ("TcpListener", "local_addr") => Some(Some(result_ty(str_ty.clone(), err.clone()))),
        // TcpStream methods.
        ("TcpStream", "read") if n_args == 0 => Some(Some(result_ty(str_ty.clone(), err.clone()))),
        ("TcpStream", "read") if n_args == 1 => Some(Some(result_ty(
            Type::List(Box::new(u8_ty())),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "read") if n_args == 2 => Some(Some(result_ty(
            Type::List(Box::new(u8_ty())),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "read_text") if n_args == 1 => Some(Some(result_ty(
            str_ty.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "read_text") if n_args == 2 => Some(Some(result_ty(
            str_ty.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "write") if n_args == 1 || n_args == 2 => Some(Some(result_ty(
            Type::Int,
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "write_all" | "write_text") if n_args == 1 || n_args == 2 => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "shutdown") if n_args == 1 => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "ready") if n_args == 2 => Some(Some(result_ty(
            Type::Named("NetReady".to_string()),
            Type::Named("NetError".to_string()),
        ))),
        ("TcpStream", "peer_addr") => Some(Some(result_ty(str_ty.clone(), err.clone()))),
        ("TcpStream", "local_addr") => Some(Some(result_ty(str_ty.clone(), err.clone()))),
        ("TcpStream", "close") => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("UdpSocket", "ready") if n_args == 2 => Some(Some(result_ty(
            Type::Named("NetReady".to_string()),
            Type::Named("NetError".to_string()),
        ))),
        ("UdpSocket", "close") if n_args == 0 => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("UdpSocket", "receive") if n_args == 2 => Some(Some(result_ty(
            Type::Named("UDPPacket".to_string()),
            Type::Named("NetError".to_string()),
        ))),
        ("UdpSocket", "send_to") if n_args == 3 => Some(Some(result_ty(
            Type::Int,
            Type::Named("NetError".to_string()),
        ))),
        ("UnixListener", "accept") if n_args == 1 => Some(Some(result_ty(
            Type::Named("UnixStream".to_string()),
            Type::Named("NetError".to_string()),
        ))),
        ("UnixStream", "read") if n_args == 2 => Some(Some(result_ty(
            Type::List(Box::new(u8_ty())),
            Type::Named("NetError".to_string()),
        ))),
        ("UnixStream", "write_all") if n_args == 2 => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("UnixStream", "ready") if n_args == 2 => Some(Some(result_ty(
            Type::Named("NetReady".to_string()),
            Type::Named("NetError".to_string()),
        ))),
        ("UnixStream", "close") if n_args == 0 => Some(Some(result_ty(
            unit.clone(),
            Type::Named("NetError".to_string()),
        ))),
        ("TLSStream", "read") if n_args == 2 => Some(Some(result_ty(
            Type::List(Box::new(u8_ty())),
            Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
        ))),
        ("TLSStream", "write_all") if n_args == 2 => Some(Some(result_ty(
            unit.clone(),
            Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
        ))),
        ("TLSStream", "close_write") if n_args == 1 => Some(Some(result_ty(
            unit.clone(),
            Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
        ))),
        ("TLSStream", "close") if n_args == 0 => Some(Some(result_ty(
            unit.clone(),
            Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
        ))),
        ("TLSStream", "ready") if n_args == 2 => Some(Some(result_ty(
            Type::Named("NetReady".to_string()),
            Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
        ))),
        ("TLSStream", "peer_identity") if n_args == 0 => {
            Some(Some(Type::Named("TLSPeerIdentity".to_string())))
        }
        ("UnixStream", "set_timeout") if n_args == 1 => Some(Some(result_ty(
            unit,
            Type::Named("NetError".to_string()),
        ))),
        ("TLSClientConfig", "with_alpn") if n_args == 1 => {
            Some(Some(result_ty(
                Type::Named("TLSClientConfig".to_string()),
                Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
            )))
        }
        ("TLSClientConfig", "with_trust" | "with_client_identity") if n_args == 1 => {
            Some(Some(result_ty(
                Type::Named("TLSClientConfig".to_string()),
                Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
            )))
        }
        ("TLSClientConfig", "with_version_bounds") if n_args == 2 => {
            Some(Some(result_ty(
                Type::Named("TLSClientConfig".to_string()),
                Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string()),
            )))
        }
        _ => None,
    }
}

pub fn require_net_method_labels(
    type_name: &str,
    method: &str,
    args: &mut Vec<crate::AST::CallArg>,
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let required = match (type_name, method, args.len()) {
        ("TcpListener", "accept", 1) | ("UnixListener", "accept", 1) => &[(0, "deadline")][..],
        ("TcpStream", "read" | "read_text" | "write" | "write_all" | "write_text", 2)
        | ("UnixStream" | "TLSStream", "read" | "write_all", 2)
        | ("TcpStream" | "UdpSocket" | "UnixStream" | "TLSStream", "ready", 2)
        | ("UdpSocket", "receive", 2) => &[(1, "deadline")][..],
        ("UdpSocket", "send_to", 3) => &[(2, "deadline")][..],
        ("TLSStream", "close_write", 1) => &[(0, "deadline")][..],
        ("TLSClientConfig", "with_version_bounds", 2) => &[(0, "min"), (1, "max")][..],
        _ => &[],
    };
    require_exact_labels(&format!("{type_name}.{method}"), args, required, span, diags);
}

/// Ratified named forms are syntax, not arity-only overloads. These parameters
/// are label-only and use the same D-APILABEL1 diagnostics as user calls.
pub fn require_exact_labels(
    api: &str,
    args: &mut Vec<crate::AST::CallArg>,
    required: &[(usize, &str)],
    span: Span,
    diags: &mut Vec<Diagnostic>,
) {
    let slot_count = required.iter().map(|(index, _)| *index).max().map_or(0, |index| index + 1);
    let params = (0..slot_count)
        .map(|index| {
            let label = required
                .iter()
                .find(|(required_index, _)| *required_index == index)
                .map_or("", |(_, label)| *label);
            crate::Sema::CallBinder::BindParam {
                label,
                name: label,
                zone: if label.is_empty() {
                    ParamZone::PositionalOnly
                } else {
                    ParamZone::LabelOnly
                },
                default: None,
                convention: AccessConvention::Read,
                ty: None,
                variadic: false,
                core_default: None,
            }
        })
        .collect::<Vec<_>>();
    let _ = crate::Sema::CallBinder::bind_call_args(api, &params, args, span, diags);
}

/// D-REGEXENGINE1=A: method return types for std-only regex values.
pub fn regex_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Regex" => match method {
            "pattern" | "source" | "flags" | "options" if argc == 0 => Some(Some(Type::String)),
            "names" if argc == 0 => Some(Some(Type::List(Box::new(Type::String)))),
            "count" if argc == 1 => Some(Some(Type::Int)),
            "is_match" if argc == 1 => Some(Some(Type::Bool)),
            "match" if argc == 1 => Some(Some(Type::Option(Box::new(Type::Named(
                "Match".to_string(),
            ))))),
            "find" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "find_all" | "split" if argc == 1 => Some(Some(Type::List(Box::new(Type::String)))),
            "matches" if argc == 1 => {
                Some(Some(Type::List(Box::new(Type::Named("Match".to_string())))))
            }
            "replace" | "replace_all" if argc == 2 => Some(Some(Type::String)),
            "split_limit" if argc == 2 => Some(Some(Type::List(Box::new(Type::String)))),
            "replace_all_with" if argc == 2 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Match" => match method {
            "group" | "name" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "start" | "end" if argc == 0 => Some(Some(Type::Int)),
            "named_captures" if argc == 0 => Some(Some(Type::List(Box::new(Type::List(
                Box::new(Type::String),
            ))))),
            "group_start" | "group_end" if argc == 1 => {
                Some(Some(Type::Option(Box::new(Type::Int))))
            }
            _ => None,
        },
        _ => None,
    }
}

/// D-PATHFS1: return type for `Path` instance method calls.
/// D-PENDING1=B: instance methods on `Loadable<T, E>`.
/// Returns `Some(Some(T))` for a valid method, `None` if not a Loadable method.
pub fn loadable_method_return(
    type_apply: &Type,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    let (val_ty, _err_ty) = match type_apply {
        Type::Apply { name, args } if name == "Loadable" && args.len() == 2 => {
            (args[0].clone(), args[1].clone())
        }
        _ => return None,
    };
    match (method, n_args) {
        ("is_loading" | "is_loaded" | "is_failed" | "is_idle", 0) => {
            Some(Some(Type::Bool))
        }
        // loaded() → T? — returns the value if in Loaded state, null otherwise.
        ("loaded", 0) => Some(Some(Type::Option(Box::new(val_ty)))),
        // or_else(default: T) → T
        ("or_else", 1) => Some(Some(val_ty)),
        _ => None,
    }
}

/// D-SHAPE-CTORVERB1=C: instance methods on generic `ExpiringValue<T>`.
pub fn expiring_method_return(
    type_apply: &Type,
    method: &str,
    _n_args: usize,
) -> Option<Option<Type>> {
    let val_ty = match type_apply {
        Type::Apply { name, args }
            if name == crate::Syntax::EXPIRING_VALUE_TYPE && args.len() == 1 =>
        {
            args[0].clone()
        }
        _ => return None,
    };
    match method {
        "get" => Some(Some(Type::Result {
            ok: Box::new(val_ty),
            err: Box::new(Type::Named("Expired".to_string())),
        })),
        "is_valid" => Some(Some(Type::Bool)),
        "force" => Some(Some(val_ty)),
        _ => None,
    }
}

/// D-NETDEP1=A / D-HTTPLIB1=A: method return types for HTTP types.
pub fn http_type_method_return(
    ty: &Type,
    method: &str,
    _args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let mk = |n: &str| Some(Some(Type::Named(n.to_string())));
    let mk_str = || Some(Some(Type::String));
    let mk_int = || Some(Some(Type::Int));
    let mk_opt_str = || Some(Some(Type::Option(Box::new(Type::String))));
    match ty {
        Type::Named(n) if n == "HTTPRequest" => match method {
            "method" | "path" => mk_str(),
            // D-HTTP-JSON1=A: typed JSON decode. The real return type comes
            // from the type argument in `CheckerInfer`.
            "json" if _args.is_empty() => Some(Some(Type::Result {
                ok: Box::new(Type::Named("Unknown".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "body" if _args.is_empty() => mk("HTTPBody"),
            "trailers" if _args.is_empty() => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPHeaders".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "param" | "header" if _args.len() == 1 => mk_opt_str(),
            "body" | "header" | "timeout" | "connect_timeout" | "read_timeout"
            | "total_timeout" | "dns_timeout" | "tls_timeout" | "write_timeout"
            | "first_byte_timeout" | "redirects" | "proxy" | "cookie" | "form" | "multipart_text" => {
                mk("HTTPRequest")
            }
            "body_len" => mk_int(),
            "under_limit" => Some(Some(Type::Bool)),
            "send" => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "HTTPClient" => match (method, _args.len()) {
            ("cookies" | "redirects" | "protocols" | "timeouts" | "raw_encoding" | "proxy" | "tls" | "allow_http_downgrade" | "retries", _) => mk("HTTPClient"),
            ("send", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "HTTPResponse" => match method {
            "status" => mk_int(),
            // D-HTTP-JSON1=A: typed JSON decode with an optional byte cap.
            "json" if _args.len() <= 1 => Some(Some(Type::Result {
                ok: Box::new(Type::Named("Unknown".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "body" => mk("HTTPBody"),
            "header" if _args.len() == 1 => mk_opt_str(),
            "header" => mk("HTTPResponse"),
            "trailers" if _args.len() == 1 => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "cookies" => Some(Some(Type::List(Box::new(Type::String)))),
            "protocol" | "remote_address" => mk_str(),
            "redirect_history" => Some(Some(Type::List(Box::new(Type::String)))),
            "timings" => Some(Some(Type::List(Box::new(Type::Int)))),
            "reused_connection" => Some(Some(Type::Bool)),
            "raw_content_encoding" => mk_opt_str(),
            _ => None,
        },
        Type::Named(n) if n == "HTTPMux" => match method {
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options" | "middleware" => Some(None),
            _ => None,
        },
        Type::Named(n) if n == "HTTPHandler" => match method {
            "handle" => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "WsConn" => match (method, _args.len()) {
            ("send_text" | "send_bytes", 1) | ("close", 2) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("WsError".to_string())),
            })),
            ("recv", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("WsMessage".to_string())),
                err: Box::new(Type::Named("WsError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "WsMessage" => match (method, _args.len()) {
            ("is_text" | "is_binary" | "is_close", 0) => Some(Some(Type::Bool)),
            ("text", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("WsError".to_string())),
            })),
            ("bytes", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::List(Box::new(u8_ty()))),
                err: Box::new(Type::Named("WsError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "Browser" => match (method, _args.len()) {
            ("capabilities", 0) => mk("BrowserCapabilities"),
            ("context", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserContext".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("subscribe", 1) | ("close", 0)
            | ("continue_request" | "fail_request" | "allow_downloads", 1)
            | ("fulfill_request", 3) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("next_event", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserEvent".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("add_intercept", 1) | ("add_intercept_url", 2) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserIntercept".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("protocol", 1) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserProtocol".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("trace", 0) => mk("BrowserTrace"),
            ("privacy", 0) => mk("BrowserPrivacy"),
            ("receipt", 0) => mk("BrowserReceipt"),
            _ => None,
        },
        Type::Named(n) if n == "BrowserContext" => match (method, _args.len()) {
            ("page" | "tab", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserPage".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("close", 0) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("isolated", 0) => Some(Some(Type::Bool)),
            ("user_hash", 0) => mk_str(),
            _ => None,
        },
        Type::Named(n) if n == "BrowserPage" => match (method, _args.len()) {
            ("goto", 1) | ("close", 0) | ("clear_cookies", 0)
            | ("set_cookie" | "storage_set", 3)
            | ("storage_clear", 1) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("screenshot" | "pdf", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("cookie", 1) | ("storage_get", 2) => Some(Some(Type::Result {
                ok: Box::new(Type::Option(Box::new(Type::String))),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("main_frame", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::Named("BrowserFrame".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("frames", 0) => Some(Some(Type::Result {
                ok: Box::new(Type::List(Box::new(Type::Named("BrowserFrame".to_string())))),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            ("get_by_role", 2)
            | ("get_by_text" | "get_by_label" | "get_by_placeholder" | "get_by_test_id"
                | "get_by_css", 1) => mk("BrowserLocator"),
            _ => None,
        },
        Type::Named(n) if n == "BrowserFrame" => match (method, _args.len()) {
            ("close", 0) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "BrowserLocator" => match (method, _args.len()) {
            ("wait" | "wait_gone", 1)
            | ("click" | "hover", 0)
            | ("fill" | "press" | "set_files", 1) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "BrowserIntercept" => match (method, _args.len()) {
            ("remove", 0) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "BrowserEvent" => match (method, _args.len()) {
            ("kind" | "request_id" | "request_method" | "url_hash" | "download_id"
                | "suggested_filename_hash", 0) => mk_str(),
            ("is_blocked", 0) => Some(Some(Type::Bool)),
            ("status_code", 0) => mk_int(),
            _ => None,
        },
        Type::Named(n) if n == "BrowserProtocol" => match (method, _args.len()) {
            ("send", 2) => Some(Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "BrowserCapabilities" => match (method, _args.len()) {
            ("bidi" | "cdp", 0) => Some(Some(Type::Bool)),
            ("profile", 0) => mk_str(),
            _ => None,
        },
        Type::Named(n) if n == "BrowserTrace" => match (method, _args.len()) {
            ("entry_count", 0) => mk_int(),
            ("redacted", 0) => Some(Some(Type::Bool)),
            ("summary", 0) => mk_str(),
            _ => None,
        },
        Type::Named(n) if n == "BrowserReceipt" => match (method, _args.len()) {
            ("entry_count", 0) => mk_int(),
            ("redacted" | "isolated" | "cleaned", 0) => Some(Some(Type::Bool)),
            ("summary", 0) => mk_str(),
            _ => None,
        },
        Type::Named(n) if n == "BrowserPrivacy" => match (method, _args.len()) {
            ("isolated_profiles" | "redact_receipts" | "shared_profiles", 0) => {
                Some(Some(Type::Bool))
            }
            _ => None,
        },
        Type::Named(n) if n == "BrowserLocked" => match (method, _args.len()) {
            ("engine" | "version" | "binary" | "protocol", 0) => mk_str(),
            ("verify", 0) => Some(Some(Type::Result {
                ok: Box::new(unit_ty()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "HTTPBody" => match method {
            "bytes" => Some(Some(Type::Result {
                ok: Box::new(Type::List(Box::new(u8_ty()))),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "text" => Some(Some(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "json" => Some(Some(Type::Result {
                ok: Box::new(Type::Named("Unknown".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            "chunks" => mk("HTTPBodyChunks"),
            "copy_to" => Some(Some(Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            _ => None,
        },
        Type::Named(n) if n == "HTTPServer" => match method {
            "local_addr" => Some(Some(Type::Result { ok: Box::new(Type::String), err: Box::new(Type::Named("HTTPError".to_string())) })),
            "serve" | "shutdown" => Some(Some(Type::Result {
                ok: Box::new(Type::Named("HTTPShutdownReport".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            })),
            _ => None,
        },
        _ => None,
    }
}

/// D-URL1=A: method return types for typed URL and MIME values.
pub fn url_mime_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Url" => match method {
            "scheme" | "path" | "query" | "to_string" | "username" | "password" | "userinfo"
            | "authority"
                if argc == 0 =>
            {
                Some(Some(Type::String))
            }
            "host" | "fragment" if argc == 0 => Some(Some(Type::Option(Box::new(Type::String)))),
            "port" | "default_port" if argc == 0 => {
                Some(Some(Type::Option(Box::new(Type::Int))))
            }
            "path_segments" if argc == 0 => Some(Some(Type::List(Box::new(Type::String)))),
            "query_pairs" if argc == 0 => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            "normalize" if argc == 0 => Some(Some(Type::Named("Url".to_string()))),
            "join" if argc == 1 => Some(Some(result_ty(
                Type::Named("Url".to_string()),
                Type::String,
            ))),
            "set_query" | "add_query" if argc == 2 => Some(Some(Type::Named("Url".to_string()))),
            _ => None,
        },
        Type::Named(n) if n == "Mime" => match method {
            "media_type" | "subtype" | "essence" | "to_string" if argc == 0 => {
                Some(Some(Type::String))
            }
            "param" if argc == 1 => Some(Some(Type::Option(Box::new(Type::String)))),
            "params" if argc == 0 => Some(Some(Type::List(Box::new(Type::List(Box::new(
                Type::String,
            )))))),
            _ => None,
        },
        _ => None,
    }
}

/// D-TIMEDEPTH1/D-TIME-CALENDAR1: method return types for civil-time values.
pub fn civil_time_method_return(
    ty: &Type,
    method: &str,
    args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let argc = args.len();
    match ty {
        Type::Named(n) if n == "Date" || n == "LocalDate" => match method {
            "year" | "month" | "day" | "weekday" | "iso_weekday" | "day_of_year" | "iso_week"
            | "quarter_of_year" | "days_in_month"
                if argc == 0 =>
            {
                Some(Some(Type::Int))
            }
            "is_leap_year" if argc == 0 => Some(Some(Type::Bool)),
            "diff_days" if argc == 1 => Some(Some(Type::Int)),
            "add_days" | "add_months" | "truncate" if argc == 1 => {
                Some(Some(Type::Named("LocalDate".to_string())))
            }
            "replace" if argc == 3 => Some(Some(Type::Named("LocalDate".to_string()))),
            "add_period" if argc == 1 => Some(Some(Type::Named("LocalDate".to_string()))),
            "to_string" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "LocalTime" => match method {
            "hour" | "minute" | "second" if argc == 0 => Some(Some(Type::Int)),
            "to_string" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "DateTime" => match method {
            "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond"
            | "to_timestamp" | "to_unix_ms"
                if argc == 0 =>
            {
                Some(Some(Type::Int))
            }
            "date" if argc == 0 => Some(Some(Type::Named("LocalDate".to_string()))),
            "time" if argc == 0 => Some(Some(Type::Named("LocalTime".to_string()))),
            "plus_duration" | "truncate" | "round" | "floor" | "ceil" if argc == 1 => {
                Some(Some(Type::Named("DateTime".to_string())))
            }
            "difference" if argc == 1 => Some(Some(Type::Named("Duration".to_string()))),
            "replace" if argc == 6 => Some(Some(Type::Named("DateTime".to_string()))),
            "in_zone" if argc == 1 => Some(Some(Type::Named("ZonedDateTime".to_string()))),
            "to_string" | "format_rfc3339" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Instant" => match method {
            "elapsed_millis" if argc == 0 => Some(Some(Type::Int)),
            "elapsed" if argc == 0 => Some(Some(Type::Named("Duration".to_string()))),
            _ => None,
        },
        Type::Named(n) if n == "Period" => match method {
            "to_string" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "Zone" => match method {
            "name" if argc == 0 => Some(Some(Type::String)),
            _ => None,
        },
        Type::Named(n) if n == "ZonedDateTime" => match method {
            "date" if argc == 0 => Some(Some(Type::Named("LocalDate".to_string()))),
            "time" if argc == 0 => Some(Some(Type::Named("LocalTime".to_string()))),
            "offset_seconds" if argc == 0 => Some(Some(Type::Int)),
            "is_dst" if argc == 0 => Some(Some(Type::Bool)),
            "to_datetime" if argc == 0 => Some(Some(Type::Named("DateTime".to_string()))),
            "zone" if argc == 0 => Some(Some(Type::Named("Zone".to_string()))),
            "add_duration" | "add_period" if argc == 1 => {
                Some(Some(Type::Named("ZonedDateTime".to_string())))
            }
            "to_string" if argc == 0 => Some(Some(Type::String)),
            "format" if argc == 1 => Some(Some(Type::String)),
            _ => None,
        },
        _ => None,
    }
}

/// D-APPROX1=A: return the type name string for a sketch receiver type.
pub fn sketch_type_name(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Named(n) => match n.as_str() {
            "HyperLogLog" => Some("HyperLogLog"),
            "TDigest" => Some("TDigest"),
            "CountMinSketch" => Some("CountMinSketch"),
            "ReservoirSampler" => Some("ReservoirSampler"),
            _ => None,
        },
        _ => None,
    }
}

/// D-APPROX1=A: method return types for sketch data structures.
pub fn sketch_method_return(
    ty: &Type,
    method: &str,
    _args: &[crate::AST::CallArg],
) -> Option<Option<Type>> {
    let name = sketch_type_name(ty)?;
    match (name, method) {
        ("HyperLogLog", "add") => Some(None), // void
        ("HyperLogLog", "count") => Some(Some(Type::Int)),
        ("TDigest", "add") => Some(None),
        ("TDigest", "quantile") => Some(Some(Type::Float)),
        ("CountMinSketch", "add") => Some(None),
        ("CountMinSketch", "count") => Some(Some(Type::Int)),
        ("ReservoirSampler", "add") => Some(None),
        ("ReservoirSampler", "sample") => Some(Some(Type::List(Box::new(Type::String)))),
        _ => None,
    }
}

pub fn path_method_return(
    type_name: &str,
    method: &str,
    _n_args: usize,
    _span: Span,
    _diags: &mut Vec<Diagnostic>,
) -> Option<Option<Type>> {
    if type_name != "Path" {
        return None;
    }
    let path = || Type::Named("Path".to_string());
    match method {
        "join" => Some(Some(path())),
        "parent" => Some(Some(Type::Option(Box::new(path())))),
        "extension" => Some(Some(Type::Option(Box::new(Type::String)))),
        "stem" => Some(Some(Type::Option(Box::new(Type::String)))),
        "to_string" => Some(Some(Type::String)),
        "write_atomic" => Some(Some(result_ty(unit_ty(), Type::String))),
        "walk" => Some(Some(Type::List(Box::new(path())))),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): a Jet-sized unsigned int type (`U8`/`U16`/`U32`/`U64`),
/// the return type of `Reader`'s width-specific reads.
fn uintn_ty(bits: u8) -> Type {
    Type::IntN {
        signed: false,
        bits,
    }
}

/// D-SHIFT1 (c7shift): method calls on `binary.Reader` (`Reader.over(bytes)`
/// static constructor is handled in `CheckerInfer/calls.rs`, mirroring
/// `Path.from`). Every read is fallible — a bounds miss is an ordinary `?`
/// error value (`Result<T, String>`, `path_method_return`'s exact error-type
/// convention above), never a panic or silent truncation (I1/L2).
pub fn binary_reader_method_return(
    type_name: &str,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    if type_name != "Reader" {
        return None;
    }
    let bytes = || Type::List(Box::new(u8_ty()));
    match (method, n_args) {
        ("read_u8", 0) => Some(Some(result_ty(uintn_ty(8), Type::String))),
        ("read_u16_le" | "read_u16_be", 0) => Some(Some(result_ty(uintn_ty(16), Type::String))),
        ("read_u32_le" | "read_u32_be", 0) => Some(Some(result_ty(uintn_ty(32), Type::String))),
        ("read_u64_le" | "read_u64_be", 0) => Some(Some(result_ty(uintn_ty(64), Type::String))),
        ("take", 1) => Some(Some(result_ty(bytes(), Type::String))),
        ("remaining", 0) => Some(Some(Type::Int)),
        ("is_at_end", 0) => Some(Some(Type::Bool)),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): method calls on `text.Cursor` (`Cursor.over(s)` static
/// constructor is handled in `CheckerInfer/calls.rs`). `take_pattern` is NOT
/// listed here — it needs its pattern-literal argument's hole types to
/// compute a return type, so it's dispatched directly at the call site
/// (`CheckerInfer/calls.rs`), for the same reason `Arena.alloc` is
/// resolved outside their generic method-return tables.
pub fn text_cursor_method_return(
    type_name: &str,
    method: &str,
    n_args: usize,
) -> Option<Option<Type>> {
    if type_name != "Cursor" {
        return None;
    }
    match (method, n_args) {
        ("take_until", 1) => Some(Some(result_ty(Type::String, Type::String))),
        ("skip_ws", 0) => Some(Some(unit_ty())),
        _ => None,
    }
}
