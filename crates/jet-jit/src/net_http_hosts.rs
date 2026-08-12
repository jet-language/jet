// Host shims for TCP/UDP/Unix + HTTP mux/server + WS — same module as net_http_rt includes.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use jet_codegen::AST::{CtKey, CtValue, Type};
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use crate::Concurrency;
use crate::JitResultValue;
use crate::Marshal::{alloc_string, clone_string, result_err_msg, result_ok};

enum NetHttpHandle {
    TcpListener(Arc<JetTCPListener>),
    TcpStream(Arc<Mutex<JetTCPStream>>),
    SocketAddr(JetSocketAddr),
    UdpSocket(Arc<JetUDPSocket>),
    NetReady(Arc<JetNetReady>),
    UDPPacket(JetUDPPacket),
    #[cfg(unix)]
    UnixListener(Arc<JetUnixListener>),
    #[cfg(unix)]
    UnixStream(Arc<Mutex<JetUnixStream>>),
    HTTPMux(Arc<JetHTTPMux>),
    HTTPRequest(JetHTTPRequest),
    HTTPResponse(JetHTTPResponse),
    HTTPBody(JetHTTPBody),
    HTTPHeaders(JetHTTPHeaders),
    HTTPMethod(JetHTTPMethod),
    HTTPStatus(JetHTTPStatus),
    HTTPVersion(JetHTTPVersion),
    HTTPHeaderName(JetHTTPHeaderName),
    HTTPHeaderValue(JetHTTPHeaderValue),
    HTTPHandler(JetHTTPHandler),
    HTTPServer(Arc<JetHTTPServer>),
    HTTPShutdownReport(JetHTTPShutdownReport),
    HTTPCorsPolicy(JetHTTPCorsPolicy),
    WsConn(Arc<Mutex<JetWsConn>>),
    WsMessage(JetWsMessage),
}

// Process-wide: spawn workers share handles (thread_local was empty on workers).
// Never hold this lock across blocking accept/read/write — clone Arc first.
static HANDLES: Mutex<Vec<Option<NetHttpHandle>>> = Mutex::new(Vec::new());

fn lock_handles() -> MutexGuard<'static, Vec<Option<NetHttpHandle>>> {
    HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn clear_net_http_handles() {
    lock_handles().clear();
}

fn push_handle(value: NetHttpHandle) -> i64 {
    let mut v = lock_handles();
    v.push(Some(value));
    v.len() as i64
}

fn with_handle<R>(handle: i64, f: impl FnOnce(&NetHttpHandle) -> Option<R>) -> Option<R> {
    let v = lock_handles();
    let idx = handle.saturating_sub(1) as usize;
    v.get(idx).and_then(|s| s.as_ref()).and_then(f)
}

fn take_handle(handle: i64) -> Option<NetHttpHandle> {
    let mut v = lock_handles();
    let idx = handle.saturating_sub(1) as usize;
    v.get_mut(idx).and_then(|s| s.take())
}

fn tcp_listener(handle: i64) -> Option<Arc<JetTCPListener>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TcpListener(l) => Some(Arc::clone(l)),
        _ => None,
    })
}

fn tcp_stream(handle: i64) -> Option<Arc<Mutex<JetTCPStream>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TcpStream(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn udp_socket(handle: i64) -> Option<Arc<JetUDPSocket>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::UdpSocket(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn net_ready(handle: i64) -> Option<Arc<JetNetReady>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::NetReady(ready) => Some(Arc::clone(ready)),
        _ => None,
    })
}

fn net_ready_interest(value: i64) -> Option<JetNetReadyInterest> {
    match value {
        0 => Some(JetNetReadyInterest::Read),
        1 => Some(JetNetReadyInterest::Write),
        2 => Some(JetNetReadyInterest::ReadWrite),
        _ => None,
    }
}

#[cfg(unix)]
fn unix_listener(handle: i64) -> Option<Arc<JetUnixListener>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::UnixListener(l) => Some(Arc::clone(l)),
        _ => None,
    })
}

#[cfg(unix)]
fn unix_stream(handle: i64) -> Option<Arc<Mutex<JetUnixStream>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::UnixStream(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn http_mux(handle: i64) -> Option<Arc<JetHTTPMux>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HTTPMux(m) => Some(Arc::clone(m)),
        _ => None,
    })
}

fn http_server(handle: i64) -> Option<Arc<JetHTTPServer>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HTTPServer(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn ws_conn(handle: i64) -> Option<Arc<Mutex<JetWsConn>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::WsConn(c) => Some(Arc::clone(c)),
        _ => None,
    })
}

fn clone_string_list(handle: i64) -> Vec<String> {
    if handle <= 0 {
        return Vec::new();
    }
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            let sid = rt.heap.list_get_int(handle, i).unwrap_or(0);
            out.push(rt.heap.clone_string(sid).unwrap_or_default());
        }
        out
    })
}

fn clone_bytes(handle: i64) -> Vec<u8> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.list_len(handle).unwrap_or(0);
        let mut out = Vec::with_capacity(len as usize);
        for i in 0..len {
            out.push(rt.heap.list_get_int(handle, i).unwrap_or(0) as u8);
        }
        out
    })
}

fn clone_string_map(handle: i64) -> Option<BTreeMap<String, String>> {
    Concurrency::with_runtime_mut(|rt| {
        let len = rt.heap.map_len(handle)?;
        let mut out = BTreeMap::new();
        for index in 0..len {
            let key = rt.heap.map_key_at(handle, index)?;
            let value = rt.heap.map_value_at(handle, index)?;
            out.insert(rt.heap.clone_string(key)?, rt.heap.clone_string(value)?);
        }
        Some(out)
    })
}

fn alloc_bytes(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for b in bytes {
            let _ = rt.heap.list_push_int(list, i64::from(*b));
        }
        list
    })
}

fn result_err(msg: String) -> i64 {
    result_err_msg(&msg)
}

fn result_err_bits(bits: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(JitResultValue {
            ok: false,
            bits: bits as u64,
        });
        rt.results.len() as i64
    })
}

fn result_ok_unit() -> i64 {
    result_ok(0)
}

fn result_ok_handle(h: i64) -> i64 {
    result_ok(h as u64)
}

fn net_invalid_error(operation: &str, resource: &str) -> JetNetError {
    jet_net_invalid_input(operation, resource)
}

fn net_invalid(operation: &str, resource: &str) -> i64 {
    net_err(net_invalid_error(operation, resource))
}

fn net_err(e: JetNetError) -> i64 {
    result_err_bits(marshal_net_error(e).0)
}

fn http_err(e: JetHTTPError) -> i64 {
    result_err_bits(marshal_http_error(e).0)
}

fn option_string(s: Option<String>) -> i64 {
    match s {
        None => 0,
        Some(v) => alloc_string(v).wrapping_add(1),
    }
}

fn map_net_ok<T>(r: Result<T, JetNetError>, f: impl FnOnce(T) -> i64) -> i64 {
    match r {
        Ok(v) => result_ok_handle(f(v)),
        Err(e) => net_err(e),
    }
}

fn map_net_unit(r: Result<(), JetNetError>) -> i64 {
    match r {
        Ok(()) => result_ok_unit(),
        Err(e) => net_err(e),
    }
}

fn map_http_ok<T>(r: Result<T, JetHTTPError>, f: impl FnOnce(T) -> i64) -> i64 {
    match r {
        Ok(v) => result_ok_handle(f(v)),
        Err(e) => http_err(e),
    }
}

fn net_error_detail_handle(detail: JetNetErrorDetail) -> i64 {
    let operation = alloc_string(detail.operation);
    let address = option_string(detail.address);
    let name = option_string(detail.name);
    let message = alloc_string(detail.message);
    let os_code = detail
        .os_code
        .map(|value| value.wrapping_add(1))
        .unwrap_or(0);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(5);
        let _ = rt.heap.record_set_string(record, 0, operation);
        let _ = rt.heap.record_set_int(record, 1, address);
        let _ = rt.heap.record_set_int(record, 2, name);
        let _ = rt.heap.record_set_string(record, 3, message);
        let _ = rt.heap.record_set_int(record, 4, os_code);
        record
    })
}

fn net_ct_optional_string(value: Option<String>) -> CtValue {
    match value {
        Some(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        None => CtValue::absent(Type::String),
    }
}

fn net_ct_optional_int(value: Option<i64>) -> CtValue {
    match value {
        Some(value) => CtValue::Present(Box::new(CtValue::Int(value))),
        None => CtValue::absent(Type::Int),
    }
}

fn net_error_detail_value(detail: JetNetErrorDetail) -> CtValue {
    CtValue::Struct {
        type_name: "NetErrorDetail".to_string(),
        fields: vec![
            ("operation".to_string(), CtValue::Str(detail.operation)),
            ("address".to_string(), net_ct_optional_string(detail.address)),
            ("name".to_string(), net_ct_optional_string(detail.name)),
            ("message".to_string(), CtValue::Str(detail.message)),
            ("os_code".to_string(), net_ct_optional_int(detail.os_code)),
        ],
    }
}

fn marshal_net_error(error: JetNetError) -> (i64, CtValue) {
    let parts = jet_net_error_surface_parts(error);
    let (payload_bits, args) = match parts.payload {
        JetNetErrorSurfacePayload::Detail(detail) => (
            net_error_detail_handle(detail.clone()),
            vec![(None, net_error_detail_value(detail))],
        ),
        JetNetErrorSurfacePayload::DNS {
            variant,
            ordinal,
            value,
        } => {
            let value_handle = alloc_string(value.clone());
            let packed = value_handle.wrapping_shl(8) | ordinal;
            (
                packed,
                vec![
                    (
                        None,
                        CtValue::Enum {
                            type_name: "NetDnsError".to_string(),
                            variant: variant.to_string(),
                            args: vec![(None, CtValue::Str(value))],
                        },
                    ),
                ],
            )
        }
    };
    let packed = payload_bits.wrapping_shl(8) | parts.ordinal;
    let value = CtValue::Enum {
        type_name: "NetError".to_string(),
        variant: parts.variant.to_string(),
        args,
    };
    (packed, value)
}

fn net_error_value(error: JetNetError) -> CtValue {
    marshal_net_error(error).1
}

fn net_io_operation_value(operation: jet_std::IOOperation) -> CtValue {
    let variant = match operation {
        jet_std::IOOperation::Read => "Read",
        jet_std::IOOperation::Write => "Write",
        jet_std::IOOperation::Flush => "Flush",
        jet_std::IOOperation::Connect => "Connect",
        jet_std::IOOperation::Accept => "Accept",
        jet_std::IOOperation::Close => "Close",
        jet_std::IOOperation::Resolve => "Resolve",
        jet_std::IOOperation::Codec => "Codec",
    };
    CtValue::Enum {
        type_name: "IOOperation".to_string(),
        variant: variant.to_string(),
        args: vec![],
    }
}

fn net_io_context_value(context: jet_std::IOContext) -> CtValue {
    let optional_string = |value: Option<String>| match value {
        Some(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        None => CtValue::absent(Type::String),
    };
    let optional_int = |value: Option<i64>| match value {
        Some(value) => CtValue::Present(Box::new(CtValue::Int(value))),
        None => CtValue::absent(Type::Int),
    };
    CtValue::Struct {
        type_name: "IOContext".to_string(),
        fields: vec![
            ("operation".to_string(), net_io_operation_value(context.operation)),
            ("resource".to_string(), optional_string(context.resource)),
            ("os_code".to_string(), optional_int(context.os_code)),
            ("cause".to_string(), optional_string(context.cause)),
        ],
    }
}

fn net_io_error_value(error: jet_std::IOError) -> CtValue {
    let (variant, context) = match error {
        jet_std::IOError::InvalidInput(context) => ("InvalidInput", context),
        jet_std::IOError::NotFound(context) => ("NotFound", context),
        jet_std::IOError::PermissionDenied(context) => ("PermissionDenied", context),
        jet_std::IOError::TimedOut(context) => ("TimedOut", context),
        jet_std::IOError::Cancelled(context) => ("Cancelled", context),
        jet_std::IOError::Closed(context) => ("Closed", context),
        jet_std::IOError::Protocol(context) => ("Protocol", context),
        jet_std::IOError::Other(context) => ("Other", context),
    };
    CtValue::Enum {
        type_name: "IOError".to_string(),
        variant: variant.to_string(),
        args: vec![(None, net_io_context_value(context))],
    }
}

fn decode_result(handle: i64) -> Option<(bool, u64)> {
    if handle <= 0 {
        return None;
    }
    Concurrency::with_runtime_mut(|rt| {
        let idx = handle.saturating_sub(1) as usize;
        rt.results.get(idx).map(|r| (r.ok, r.bits))
    })
}

type HTTPHandlerFn = unsafe extern "C" fn(i64) -> i64;

fn wrap_http_handler(fn_ptr: i64) -> JetHTTPHandler {
    let f: HTTPHandlerFn = unsafe { std::mem::transmute(fn_ptr as usize) };
    Arc::new(move |req: JetHTTPRequest| -> Result<JetHTTPResponse, JetHTTPError> {
        Concurrency::with_http_jet_runtime(|| {
            let req_h = push_handle(NetHttpHandle::HTTPRequest(req));
            let res_h = unsafe { f(req_h) };
            match decode_result(res_h) {
                Some((true, bits)) => {
                    let resp_h = bits as i64;
                    match take_handle(resp_h) {
                        Some(NetHttpHandle::HTTPResponse(resp)) => Ok(resp),
                        other => {
                            if let Some(v) = other {
                                let _ = push_handle(v);
                            }
                            Err(JetHTTPError::IO {
                                operation: "handler response".into(),
                            })
                        }
                    }
                }
                Some((false, bits)) => {
                    let msg = Concurrency::with_runtime_mut(|rt| {
                        rt.heap
                            .clone_string(bits as i64)
                            .unwrap_or_else(|| "handler error".into())
                    });
                    Err(JetHTTPError::IO { operation: msg })
                }
                None => Err(JetHTTPError::IO {
                    operation: "handler result".into(),
                }),
            }
        })
    })
}

// ── core.net ───────────────────────────────────────────────────────────────

extern "C" fn jet_jit_net_socket_addr(host: i64, port: i64) -> i64 {
    let host = clone_string(host);
    map_net_ok(jet_net_socket_addr(&host, port), |a| {
        push_handle(NetHttpHandle::SocketAddr(a))
    })
}

extern "C" fn jet_jit_net_socket_to_string(addr: i64) -> i64 {
    match with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_to_string(a)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_net_socket_host(addr: i64) -> i64 {
    match with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_host(a)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_net_socket_port_typed(addr: i64) -> i64 {
    with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_port(a)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_net_tcp_listen_str(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_tcp_listen(&addr), |l| {
        push_handle(NetHttpHandle::TcpListener(Arc::new(l)))
    })
}

extern "C" fn jet_jit_net_tcp_listen_addr(addr: i64) -> i64 {
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return net_invalid("tcp listen", "SocketAddr");
    };
    map_net_ok(jet_net_tcp_listen_addr(&addr), |l| {
        push_handle(NetHttpHandle::TcpListener(Arc::new(l)))
    })
}

extern "C" fn jet_jit_net_tcp_connect(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_tcp_connect(&addr), |s| {
        push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(s))))
    })
}

extern "C" fn jet_jit_net_listener_local_socket_addr(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("listener local address", "TcpListener");
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_set_timeout(stream: i64, ms: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_timeout", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_timeout(&mut guard, ms))
}

extern "C" fn jet_jit_net_nodelay(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("nodelay", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_nodelay(&guard) {
        Ok(v) => result_ok(if v { 1 } else { 0 }),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_set_nodelay(stream: i64, enabled: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_nodelay", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_nodelay(&guard, enabled != 0))
}

extern "C" fn jet_jit_net_ttl(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("ttl", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_ttl(&guard) {
        Ok(v) => result_ok(v as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_set_ttl(stream: i64, ttl: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_ttl", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_ttl(&guard, ttl))
}

extern "C" fn jet_jit_net_socket_type(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return alloc_string(String::new());
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    alloc_string(jet_net_socket_type(&guard))
}

extern "C" fn jet_jit_net_sendfile(stream: i64, path: i64) -> i64 {
    let path = clone_string(path);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("sendfile", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_sendfile(&mut guard, &path) {
        Ok(n) => result_ok(n as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_dns_ptr(name: i64, ms: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_ptr(&name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_strings(rows)),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_getservbyname(name: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_getservbyname(&name) {
        Ok(port) => result_ok(port as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_getservbyport(port: i64) -> i64 {
    match jet_net_getservbyport(port) {
        Ok(name) => result_ok_handle(alloc_string(name)),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_tcp_reply(stream: i64, status: i64, body: i64) -> i64 {
    let status = clone_string(status);
    let body = clone_string(body);
    let Some(NetHttpHandle::TcpStream(s)) = take_handle(stream) else {
        return net_invalid("tcp reply", "TcpStream");
    };
    let Ok(stream) = Arc::try_unwrap(s).map(|m| m.into_inner().unwrap_or_else(|p| p.into_inner()))
    else {
        return result_err("TcpStream still shared".into());
    };
    map_net_unit(jet_net_tcp_reply(stream, &status, &body))
}

extern "C" fn jet_jit_net_udp_bind(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_udp_bind(&addr), |s| {
        push_handle(NetHttpHandle::UdpSocket(Arc::new(s)))
    })
}

extern "C" fn jet_jit_net_udp_local_addr(socket: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_local_addr", "UdpSocket");
    };
    match jet_net_udp_local_addr(&socket) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_set_timeout(socket: i64, ms: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_set_timeout", "UdpSocket");
    };
    map_net_unit(jet_net_udp_set_timeout(&socket, ms))
}

extern "C" fn jet_jit_net_udp_send_bytes_to(socket: i64, data: i64, addr: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return net_invalid("udp_send", "SocketAddr");
    };
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_send", "UdpSocket");
    };
    match jet_net_udp_send_bytes_to(&socket, &bytes, &addr) {
        Ok(n) => result_ok(n as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_receive(socket: i64, limit: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_receive", "UdpSocket");
    };
    match jet_net_udp_receive(&socket, limit) {
        Ok(p) => result_ok_handle(push_handle(NetHttpHandle::UDPPacket(p))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_send_bytes_to_deadline(
    socket: i64,
    data: i64,
    addr: i64,
    deadline: i64,
) -> i64 {
    let bytes = clone_bytes(data);
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return net_invalid("udp send", "SocketAddr");
    };
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp send", "UdpSocket");
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_send_bytes_to_deadline(&socket, &bytes, &addr, &deadline) {
        Ok(n) => result_ok(n as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_receive_deadline(
    socket: i64,
    limit: i64,
    deadline: i64,
) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp receive", "UdpSocket");
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_receive_deadline(&socket, limit, &deadline) {
        Ok(packet) => result_ok_handle(push_handle(NetHttpHandle::UDPPacket(packet))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_packet_bytes(packet: i64) -> i64 {
    match with_handle(packet, |h| match h {
        NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_bytes(p)),
        _ => None,
    }) {
        Some(b) => alloc_bytes(&b),
        None => alloc_bytes(&[]),
    }
}

extern "C" fn jet_jit_net_udp_packet_original_len(packet: i64) -> i64 {
    with_handle(packet, |h| match h {
        NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_original_len(p)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_net_udp_packet_truncated(packet: i64) -> i64 {
    i64::from(
        with_handle(packet, |h| match h {
            NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_truncated(p)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_listen(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_listen(&path), |l| {
        push_handle(NetHttpHandle::UnixListener(Arc::new(l)))
    })
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_accept(listener: i64) -> i64 {
    let Some(listener) = unix_listener(listener) else {
        return net_invalid("unix accept", "UnixListener");
    };
    match jet_net_unix_accept(&listener) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::UnixStream(Arc::new(
            Mutex::new(s),
        )))),
        Err(e) => net_err(e),
    }
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_connect(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_connect(&path), |s| {
        push_handle(NetHttpHandle::UnixStream(Arc::new(Mutex::new(s))))
    })
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_read(stream: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix read", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_unix_read(&mut guard) {
        Ok(s) => result_ok_handle(alloc_string(s)),
        Err(e) => net_err(e),
    }
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_write(stream: i64, data: i64) -> i64 {
    let data = clone_string(data);
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_write", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write(&mut guard, &data))
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_write_all_bytes(stream: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_write_all_bytes", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write_all_bytes(&mut guard, &bytes))
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_close(stream: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_close", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_close(&mut guard))
}

#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_listen(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_listen(&path), |_| 0)
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_accept(_listener: i64) -> i64 {
    let listener = JetUnixListener;
    map_net_ok(jet_net_unix_accept(&listener), |_| 0)
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_connect(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_connect(&path), |_| 0)
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_read(_stream: i64) -> i64 {
    let mut stream = JetUnixStream;
    match jet_net_unix_read(&mut stream) {
        Ok(value) => result_ok_handle(alloc_string(value)),
        Err(error) => net_err(error),
    }
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_write(_stream: i64, data: i64) -> i64 {
    let mut stream = JetUnixStream;
    let data = clone_string(data);
    map_net_unit(jet_net_unix_write(&mut stream, &data))
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_write_all_bytes(_stream: i64, data: i64) -> i64 {
    let mut stream = JetUnixStream;
    let data = clone_bytes(data);
    map_net_unit(jet_net_unix_write_all_bytes(&mut stream, &data))
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_close(_stream: i64) -> i64 {
    let mut stream = JetUnixStream;
    map_net_unit(jet_net_unix_close(&mut stream))
}

// ── TcpListener / TcpStream handle methods ─────────────────────────────────

extern "C" fn jet_jit_tcp_listener_accept(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("tcp accept", "TcpListener");
    };
    match jet_net_tcp_accept(&listener) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::TcpStream(Arc::new(
            Mutex::new(s),
        )))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_tcp_listener_local_addr(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("tcp local address", "TcpListener");
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(alloc_string(jet_net_socket_to_string(&a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_tcp_stream_read_text(stream: i64, limit: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp read", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_read_text(&mut guard, limit) {
        Ok(s) => result_ok_handle(alloc_string(s)),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_tcp_stream_write_all_bytes(stream: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("write_all", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_write_all_bytes(&mut guard, &bytes))
}

extern "C" fn jet_jit_tcp_stream_close(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp close", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_close(&mut guard))
}

extern "C" fn jet_jit_tcp_stream_ready(stream: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return net_invalid("tcp ready", "NetReadyInterest");
    };
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp ready", "TcpStream");
    };
    let deadline = jet_std::Duration { ns: deadline };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_ok(
        jet_net_tcp_ready_deadline(&mut guard, interest, &deadline),
        |ready| push_handle(NetHttpHandle::NetReady(Arc::new(ready))),
    )
}

extern "C" fn jet_jit_udp_socket_ready(socket: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return net_invalid("udp ready", "NetReadyInterest");
    };
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp ready", "UdpSocket");
    };
    let deadline = jet_std::Duration { ns: deadline };
    map_net_ok(jet_net_udp_ready(&socket, interest, &deadline), |ready| {
        push_handle(NetHttpHandle::NetReady(Arc::new(ready)))
    })
}

extern "C" fn jet_jit_udp_socket_close(socket: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp close", "UdpSocket");
    };
    map_net_unit(jet_net_udp_close(&socket))
}

extern "C" fn jet_jit_net_ready_readable(ready: i64) -> i64 {
    i64::from(
        net_ready(ready)
            .map(|ready| jet_net_ready_readable(&ready))
            .unwrap_or(false),
    )
}

extern "C" fn jet_jit_net_ready_writable(ready: i64) -> i64 {
    i64::from(
        net_ready(ready)
            .map(|ready| jet_net_ready_writable(&ready))
            .unwrap_or(false),
    )
}

// ── core.http.server ───────────────────────────────────────────────────────

extern "C" fn jet_jit_http_mux_new() -> i64 {
    push_handle(NetHttpHandle::HTTPMux(Arc::new(jet_http_mux_new())))
}

extern "C" fn jet_jit_http_mux_add(mux: i64, method: i64, pattern: i64, fn_ptr: i64) -> i64 {
    let method = clone_string(method);
    let pattern = clone_string(pattern);
    let handler = wrap_http_handler(fn_ptr);
    if let Some(mux) = http_mux(mux) {
        jet_http_mux_add_handler(&mux, &method, &pattern, handler);
    }
    0
}

extern "C" fn jet_jit_http_response(status: i64, body: i64) -> i64 {
    let body = clone_string(body);
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_response(
        status, &body,
    )))
}

extern "C" fn jet_jit_http_req_body(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

extern "C" fn jet_jit_http_req_method(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_method(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_http_req_path(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_path(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_http_req_param(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_param(r, &name)),
        _ => None,
    })
    .and_then(|r| r.ok()))
}

extern "C" fn jet_jit_http_req_header(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_header(r, &name)),
        _ => None,
    })
    .and_then(|r| r.ok()))
}

extern "C" fn jet_jit_http_body_text(body: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HTTPBody(b) => Some(jet_http_body_text(b, limit)),
        _ => None,
    }) {
        Some(Ok(s)) => result_ok_handle(alloc_string(s)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPBody".into()),
    }
}

extern "C" fn jet_jit_http_body_bytes(body: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HTTPBody(b) => Some(jet_http_body_bytes(b, limit)),
        _ => None,
    }) {
        Some(Ok(bytes)) => result_ok_handle(alloc_bytes(&bytes)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPBody".into()),
    }
}

extern "C" fn jet_jit_http_body_json_text(body: i64, has_limit: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HTTPBody(b) => Some(jet_http_body_json_text_defaulted(
            b,
            (has_limit != 0).then_some(limit),
        )),
        _ => None,
    }) {
        Some(Ok(s)) => result_ok_handle(alloc_string(s)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPBody".into()),
    }
}

fn http_file_reader_read(handle: i64, max: usize) -> Result<Option<Vec<u8>>, JetHTTPError> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        let Some(crate::enc_stream::FileReaderSlot::Live(reader)) =
            rt.file_readers.get_mut(index)
        else {
            return Some(Err(JetHTTPError::IO {
                operation: "read body".to_string(),
            }));
        };
        let mut bytes = vec![0; max];
        let result = match std::io::Read::read(&mut reader.inner, &mut bytes) {
            Ok(0) => Ok(None),
            Ok(read) => {
                bytes.truncate(read);
                Ok(Some(bytes))
            }
            Err(_) => Err(JetHTTPError::IO {
                operation: "read body".to_string(),
            }),
        };
        Some(result)
    })
    .unwrap_or_else(|| {
        Err(JetHTTPError::IO {
            operation: "read body".to_string(),
        })
    })
}

fn http_file_reader_close(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        if let Some(slot) = rt.file_readers.get_mut(index) {
            *slot = crate::enc_stream::FileReaderSlot::Taken;
        }
    });
}

fn http_file_writer_write(handle: i64, bytes: &[u8]) -> Result<(), JetHTTPError> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        let Some(crate::enc_stream::FileWriterSlot::Live(writer)) =
            rt.file_writers.get_mut(index)
        else {
            return Some(Err(JetHTTPError::IO {
                operation: "copy body".to_string(),
            }));
        };
        let result = std::io::Write::write_all(&mut writer.inner, bytes).map_err(|_| JetHTTPError::IO {
            operation: "copy body".to_string(),
        });
        Some(result)
    })
    .unwrap_or_else(|| {
        Err(JetHTTPError::IO {
            operation: "copy body".to_string(),
        })
    })
}

extern "C" fn jet_jit_http_body_copy_to(body: i64, writer: i64, limit: i64) -> i64 {
    let result = with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => Some(jet_http_body_bytes(body, limit)),
        _ => None,
    });
    match result {
        Some(Ok(bytes)) => match http_file_writer_write(writer, &bytes) {
            Ok(()) => result_ok_handle(bytes.len() as i64),
            Err(error) => http_err(error),
        },
        Some(Err(error)) => http_err(error),
        None => result_err("invalid HTTPBody".into()),
    }
}

/// D-HTTP-NOMINAL1: resident JIT marshals nominal HTTP constructors through
/// the same Prelude functions that AOT emits. The op is a closed compiler
/// mapping; arguments are already carrier handles and unused slots are zero.
extern "C" fn jet_jit_http_nominal_static(
    op: i64,
    arg0: i64,
    arg1: i64,
    _arg2: i64,
    _arg3: i64,
    _arg4: i64,
    _arg5: i64,
) -> i64 {
    match op {
        1 => map_http_ok(JetHTTPMethod::custom(clone_string(arg0)), |value| {
            push_handle(NetHttpHandle::HTTPMethod(value))
        }),
        2 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::get())),
        3 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::head())),
        4 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::post())),
        5 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::put())),
        6 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::delete())),
        7 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::connect())),
        8 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::options())),
        9 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::trace())),
        10 => push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::patch())),
        11 => map_http_ok(JetHTTPStatus::new(arg0), |value| {
            push_handle(NetHttpHandle::HTTPStatus(value))
        }),
        12 => push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_1_0())),
        13 => push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_1_1())),
        14 => push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_2())),
        15 => map_http_ok(JetHTTPHeaderName::new(clone_string(arg0)), |value| {
            push_handle(NetHttpHandle::HTTPHeaderName(value))
        }),
        16 => map_http_ok(JetHTTPHeaderValue::new(clone_string(arg0)), |value| {
            push_handle(NetHttpHandle::HTTPHeaderValue(value))
        }),
        17 => push_handle(NetHttpHandle::HTTPHeaders(JetHTTPHeaders::new())),
        18 => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::empty())),
        19 => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_bytes(
            clone_bytes(arg0),
        ))),
        20 => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_text(
            clone_string(arg0),
        ))),
        21 => {
            let Some((top, sub, params)) = crate::Net::mime_parts(arg1) else {
                return result_err("invalid MIME".into());
            };
            push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_text_with_mime(
                clone_string(arg0),
                jet_std::JetMIME { top, sub, params },
            )))
        }
        22 => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_json(arg0))),
        23 => {
            let Some(values) = clone_string_map(arg0) else {
                return result_err("invalid HTTP form map".into());
            };
            push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_form(values)))
        }
        24 => {
            let Some(values) = clone_string_map(arg0) else {
                return result_err("invalid HTTP multipart map".into());
            };
            push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_multipart(values)))
        }
        25 => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::bridge(
            arg0,
            None,
            http_file_reader_read,
            http_file_reader_close,
        ))),
        26 => match jet_http_consume_limit(arg1) {
            Ok(length) => push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::bridge(
                arg0,
                Some(length),
                http_file_reader_read,
                http_file_reader_close,
            ))),
            Err(error) => http_err(error),
        },
        _ => result_err(format!("unknown HTTP nominal operation {op}")),
    }
}

extern "C" fn jet_jit_http_nominal_show(handle: i64) -> i64 {
    let shown = with_handle(handle, |value| match value {
        NetHttpHandle::HTTPMethod(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPStatus(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPVersion(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPHeaderName(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPHeaderValue(value) => Some(value.jet_show()),
        _ => None,
    })
    .unwrap_or_default();
    alloc_string(shown)
}

/// D-HTTP-JSON1=A: `server.json(status, body)` — body is already JSON text.
extern "C" fn jet_jit_http_json_response(status: i64, body: i64) -> i64 {
    runtime_json_response(status, clone_string(body))
}

/// D-HTTP-STATIC-FILES1=A: mount a directory under a prefix.
extern "C" fn jet_jit_http_static_files(
    mux: i64,
    prefix: i64,
    root: i64,
    index: i64,
    dotfiles: i64,
    follow_links: i64,
) -> i64 {
    let _ = runtime_static_files(
        mux,
        clone_string(prefix),
        clone_string(root),
        (index >= 0).then_some(index != 0),
        (dotfiles >= 0).then_some(dotfiles != 0),
        (follow_links >= 0).then_some(follow_links != 0),
    );
    0
}

/// D-HTTP-CORS1=A: build a CORS policy from a named-origin list or `.Any`.
/// `origins_mode`: 0 = `.Any`, 1 = string-list handle.
extern "C" fn jet_jit_http_cors_policy(
    origins_mode: i64,
    origins: i64,
    methods: i64,
    headers: i64,
    credentials: i64,
    has_max_age: i64,
    max_age: i64,
) -> i64 {
    let origins_any = origins_mode == 0;
    let origin_list = if origins_any {
        Vec::new()
    } else {
        clone_string_list(origins)
    };
    match runtime_cors_policy(
        origins_any,
        origin_list,
        (methods > 0).then(|| clone_string_list(methods)),
        (headers > 0).then(|| clone_string_list(headers)),
        (credentials >= 0).then_some(credentials != 0),
        (has_max_age != 0).then_some(max_age),
    ) {
        Ok(h) => result_ok_handle(h),
        Err(error) => result_err_bits(error.packed),
    }
}

/// Map JSON typed-decode `Result` errs to `HTTPError::InvalidFraming` (AOT parity).
extern "C" fn jet_jit_http_project_json_decode_error(result: i64) -> i64 {
    let is_error = Concurrency::with_runtime_mut(|rt| {
        result
            .checked_sub(1)
            .and_then(|index| rt.results.get(index as usize))
            .is_some_and(|value| !value.ok)
    });
    if is_error {
        http_err(jet_http_json_decode_error())
    } else {
        result
    }
}

/// D-HTTP-CORS1=A: install a policy on a mux.
extern "C" fn jet_jit_http_cors(mux: i64, policy: i64) -> i64 {
    let _ = runtime_cors(mux, policy);
    0
}

extern "C" fn jet_jit_http_resp_status(resp: i64) -> i64 {
    with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_srv_response_status(r)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_http_resp_body(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_srv_response_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

extern "C" fn jet_jit_http_client_resp_body(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_client_response_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

extern "C" fn jet_jit_http_server_bind(addr: i64, mux: i64) -> i64 {
    let addr = clone_string(addr);
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    match jet_http_server_bind(&addr, (*mux).clone()) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::HTTPServer(Arc::new(s)))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_local_addr(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_local_addr(&server) {
        Ok(a) => result_ok_handle(alloc_string(a)),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_serve(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_serve(&server) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HTTPShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_shutdown(server: i64, grace_ms: i64) -> i64 {
    let grace = jet_std::Duration {
        ns: grace_ms.saturating_mul(1_000_000),
    };
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_shutdown(&server, &grace) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HTTPShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_shutdown_report_field(report: i64, field: i64) -> i64 {
    with_handle(report, |h| match h {
        NetHttpHandle::HTTPShutdownReport(r) => Some(match field {
            0 => r.user_accepted,
            1 => r.user_overloaded,
            2 => r.user_completed,
            3 => r.user_cancelled,
            _ => 0,
        }),
        _ => None,
    })
    .unwrap_or(0)
}

/// Cleartext `http://` GET/POST for hermetic loopback stems (no TLS bridge).
fn http_cleartext_exchange(
    method: &str,
    url: &str,
    body: Option<&str>,
) -> Result<JetHTTPResponse, String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported url scheme: {url}"))?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let host_port = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let mut stream = std::net::TcpStream::connect(&host_port)
        .map_err(|e| format!("http connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let body_bytes = body.unwrap_or("").as_bytes();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n"
    );
    if body.is_some() {
        req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    req.push_str("\r\n");
    use std::io::{Read, Write};
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("http write failed: {e}"))?;
    if !body_bytes.is_empty() {
        stream
            .write_all(body_bytes)
            .map_err(|e| format!("http body write failed: {e}"))?;
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header_bytes = header_end + 4;
                    let head = String::from_utf8_lossy(&buf[..header_end]);
                    let mut content_length = None;
                    for line in head.lines().skip(1) {
                        if let Some(v) = line
                            .split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        {
                            content_length = Some(v);
                            break;
                        }
                    }
                    if let Some(len) = content_length {
                        while buf.len() < header_bytes + len {
                            match stream.read(&mut tmp) {
                                Ok(0) => break,
                                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                                Err(_) => break,
                            }
                        }
                        buf.truncate(header_bytes + len);
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let (head, body_text) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut resp = jet_http_srv_response(status, &body_text.to_string());
    for line in head.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let _ = resp
                .headers
                .append(&k.trim().to_string(), &v.trim().to_string());
        }
    }
    Ok(resp)
}

extern "C" fn jet_jit_http_client_get(url: i64) -> i64 {
    let url = clone_string(url);
    match http_cleartext_exchange("GET", &url, None) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_client_post(url: i64, body: i64) -> i64 {
    let url = clone_string(url);
    let body = clone_string(body);
    match http_cleartext_exchange("POST", &url, Some(&body)) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_serve_once_listener(listener: i64, mux: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return result_err("invalid TcpListener".into());
    };
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    match jet_http_mux_serve_once_listener(&listener, &mux) {
        Ok(()) => result_ok_unit(),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_ws_upgrade(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_ws_upgrade(r)),
        _ => None,
    }) {
        Some(Ok(c)) => result_ok_handle(push_handle(NetHttpHandle::WsConn(Arc::new(
            Mutex::new(c),
        )))),
        Some(Err(e)) => result_err(format!("{e:?}")),
        None => result_err("invalid HTTPRequest".into()),
    }
}

extern "C" fn jet_jit_ws_connect(url: i64) -> i64 {
    let url = clone_string(url);
    match jet_ws_connect(&url) {
        Ok(c) => result_ok_handle(push_handle(NetHttpHandle::WsConn(Arc::new(Mutex::new(
            c,
        ))))),
        Err(e) => result_err(format!("{e:?}")),
    }
}

extern "C" fn jet_jit_ws_send_text(conn: i64, text: i64) -> i64 {
    let text = clone_string(text);
    let Some(conn) = ws_conn(conn) else {
        return result_err("invalid WsConn".into());
    };
    let guard = conn.lock().unwrap_or_else(|p| p.into_inner());
    match jet_ws_send_text(&guard, &text) {
        Ok(()) => result_ok_unit(),
        Err(e) => result_err(format!("{e:?}")),
    }
}

extern "C" fn jet_jit_ws_recv(conn: i64) -> i64 {
    let Some(conn) = ws_conn(conn) else {
        return result_err("invalid WsConn".into());
    };
    let guard = conn.lock().unwrap_or_else(|p| p.into_inner());
    match jet_ws_recv(&guard) {
        Ok(m) => result_ok_handle(push_handle(NetHttpHandle::WsMessage(m))),
        Err(e) => result_err(format!("{e:?}")),
    }
}

extern "C" fn jet_jit_ws_close(conn: i64, code: i64, reason: i64) -> i64 {
    let reason = clone_string(reason);
    let Some(conn) = ws_conn(conn) else {
        return result_err("invalid WsConn".into());
    };
    let guard = conn.lock().unwrap_or_else(|p| p.into_inner());
    match jet_ws_close(&guard, code, &reason) {
        Ok(()) => result_ok_unit(),
        Err(e) => result_err(format!("{e:?}")),
    }
}

extern "C" fn jet_jit_ws_message_is_text(msg: i64) -> i64 {
    i64::from(
        with_handle(msg, |h| match h {
            NetHttpHandle::WsMessage(m) => Some(jet_ws_message_is_text(m)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

extern "C" fn jet_jit_ws_message_text(msg: i64) -> i64 {
    match with_handle(msg, |h| match h {
        NetHttpHandle::WsMessage(m) => Some(jet_ws_message_text(m)),
        _ => None,
    }) {
        Some(Ok(s)) => result_ok_handle(alloc_string(s)),
        Some(Err(e)) => result_err(format!("{e:?}")),
        None => result_err("invalid WsMessage".into()),
    }
}

type HTTPClosureFn = unsafe extern "C" fn(i64, i64) -> i64;
type HTTPMiddlewareFn = unsafe extern "C" fn(i64) -> i64;

fn list_of_strings(rows: Vec<String>) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for row in rows {
            let sid = rt.heap.alloc_string(row);
            let _ = rt.heap.list_push_int(list, sid);
        }
        list
    })
}

/// Bind a capturing Jet HTTP handler. `caps` is a heap list of capture handles.
extern "C" fn jet_jit_http_handler_bind(fn_ptr: i64, caps: i64) -> i64 {
    let env = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        let len = rt.heap.list_len(caps).unwrap_or(0);
        for i in 0..len {
            if let Some(v) = rt.heap.list_get_int(caps, i) {
                let _ = rt.heap.list_push_int(list, v);
            }
        }
        list
    });
    bind_http_closure(fn_ptr, env)
}

/// Single-capture bind: pack `cap0` into a fresh env list in the host.
extern "C" fn jet_jit_http_handler_bind1(fn_ptr: i64, cap0: i64) -> i64 {
    let env = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        let _ = rt.heap.list_push_int(list, cap0);
        list
    });
    bind_http_closure(fn_ptr, env)
}

fn bind_http_closure(fn_ptr: i64, env: i64) -> i64 {
    let f: HTTPClosureFn = unsafe { std::mem::transmute(fn_ptr as usize) };
    let handler: JetHTTPHandler = Arc::new(move |req: JetHTTPRequest| {
        Concurrency::with_http_jet_runtime(|| {
            let req_h = push_handle(NetHttpHandle::HTTPRequest(req));
            let res_h = unsafe { f(env, req_h) };
            match decode_result(res_h) {
                Some((true, bits)) => match take_handle(bits as i64) {
                    Some(NetHttpHandle::HTTPResponse(resp)) => Ok(resp),
                    other => {
                        if let Some(v) = other {
                            let _ = push_handle(v);
                        }
                        Err(JetHTTPError::IO {
                            operation: "handler response".into(),
                        })
                    }
                },
                Some((false, bits)) => {
                    let msg = Concurrency::with_runtime_mut(|rt| {
                        rt.heap
                            .clone_string(bits as i64)
                            .unwrap_or_else(|| "handler error".into())
                    });
                    Err(JetHTTPError::IO { operation: msg })
                }
                None => Err(JetHTTPError::IO {
                    operation: "handler result".into(),
                }),
            }
        })
    });
    push_handle(NetHttpHandle::HTTPHandler(handler))
}

extern "C" fn jet_jit_http_handler_handle(handler: i64, req: i64) -> i64 {
    let Some(handler) = with_handle(handler, |h| match h {
        NetHttpHandle::HTTPHandler(h) => Some(Arc::clone(h)),
        _ => None,
    }) else {
        return result_err("invalid HTTPHandler".into());
    };
    let Some(req) = take_handle(req).and_then(|h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HTTPRequest".into());
    };
    match handler(req) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => http_err(e),
    }
}

extern "C" fn jet_jit_http_mux_middleware(mux: i64, mw_fn: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return 0;
    };
    let f: HTTPMiddlewareFn = unsafe { std::mem::transmute(mw_fn as usize) };
    jet_http_mux_middleware(&mux, move |next| {
        Concurrency::with_http_jet_runtime(|| {
            let next_h = push_handle(NetHttpHandle::HTTPHandler(next));
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { f(next_h) }));
            let fail = |op: &'static str| -> JetHTTPHandler {
                Arc::new(move |_| Err(JetHTTPError::IO { operation: op.into() }))
            };
            match out {
                Ok(out) => {
                    if let Some(h) = with_handle(out, |h| match h {
                        NetHttpHandle::HTTPHandler(h) => Some(Arc::clone(h)),
                        _ => None,
                    }) {
                        let _ = take_handle(out);
                        return h;
                    }
                    if let Some((true, bits)) = decode_result(out) {
                        if let Some(NetHttpHandle::HTTPHandler(h)) = take_handle(bits as i64) {
                            return h;
                        }
                    }
                    fail("middleware returned non-handler")
                }
                Err(_) => fail("middleware panic"),
            }
        })
    });
    0
}

extern "C" fn jet_jit_http_request_id(mux: i64) -> i64 {
    if let Some(mux) = http_mux(mux) {
        jet_http_srv_install_request_id(&mux);
    }
    0
}

extern "C" fn jet_jit_http_req_trailers(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_trailers(r)),
        _ => None,
    }) {
        Some(Ok(h)) => result_ok_handle(push_handle(NetHttpHandle::HTTPHeaders(h))),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPRequest".into()),
    }
}

extern "C" fn jet_jit_http_resp_trailers(resp: i64, trailers: i64) -> i64 {
    let Some(resp) = take_handle(resp).and_then(|h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HTTPResponse".into());
    };
    let Some(trailers) = take_handle(trailers).and_then(|h| match h {
        NetHttpHandle::HTTPHeaders(t) => Some(t),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HTTPHeaders".into());
    };
    match jet_http_srv_response_trailers(resp, trailers) {
        Ok(r) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(r))),
        Err(e) => http_err(e),
    }
}

extern "C" fn jet_jit_http_req_body_len(req: i64) -> i64 {
    with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_body_len(r)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_http_req_under_limit(req: i64, max: i64) -> i64 {
    i64::from(
        with_handle(req, |h| match h {
            NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_under_limit(r, max)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

extern "C" fn jet_jit_http_sse(data: i64) -> i64 {
    let data = clone_string(data);
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_sse(&data)))
}

extern "C" fn jet_jit_http_static_file_range(req: i64, path: i64, mime: i64) -> i64 {
    let path = clone_string(path);
    let mime = clone_string(mime);
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_static_file_range(r, &path, &mime)),
        _ => None,
    }) {
        Some(Ok(resp)) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Some(Err(e)) => result_err(e),
        None => result_err("invalid HTTPRequest".into()),
    }
}

extern "C" fn jet_jit_http_client_request_new(method: i64, url: i64) -> i64 {
    let method = clone_string(method);
    let url = clone_string(url);
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_new(
        &method, &url,
    )))
}

fn take_http_request(handle: i64) -> Option<JetHTTPRequest> {
    take_handle(handle).and_then(|h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    })
}

extern "C" fn jet_jit_http_client_request_body(req: i64, body: i64) -> i64 {
    let body = clone_string(body);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_body(
        req, &body,
    )))
}

extern "C" fn jet_jit_http_client_request_form(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_form(
        req, &name, &value,
    )))
}

extern "C" fn jet_jit_http_client_request_cookie(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_cookie(
        req, &name, &value,
    )))
}

extern "C" fn jet_jit_http_client_request_header(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_header(
        req, &name, &value,
    )))
}

extern "C" fn jet_jit_http_client_request_redirects(req: i64, limit: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_redirects(req, limit),
    ))
}

extern "C" fn jet_jit_http_client_request_connect_timeout(req: i64, ms: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_connect_timeout(req, ms),
    ))
}

extern "C" fn jet_jit_http_client_request_read_timeout(req: i64, ms: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_read_timeout(req, ms),
    ))
}

fn http_cleartext_request(req: &JetHTTPRequest) -> Result<JetHTTPResponse, String> {
    let mut cookie = String::new();
    for chunk in req.cookies.chunks(2) {
        if chunk.len() == 2 {
            if !cookie.is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(&chunk[0]);
            cookie.push('=');
            cookie.push_str(&chunk[1]);
        }
    }
    let mut form_body = String::new();
    for chunk in req.form.chunks(2) {
        if chunk.len() == 2 {
            if !form_body.is_empty() {
                form_body.push('&');
            }
            form_body.push_str(&urlencoding_encode(&chunk[0]));
            form_body.push('=');
            form_body.push_str(&urlencoding_encode(&chunk[1]));
        }
    }
    let form_set = !form_body.is_empty();
    let body_text = if form_set {
        Some(form_body)
    } else if req.body_set {
        Some(
            req.body
                .text(8 * 1024 * 1024)
                .map_err(|e| format!("{e:?}"))?,
        )
    } else {
        None
    };
    let rest = req
        .url
        .strip_prefix("http://")
        .ok_or_else(|| format!("unsupported url scheme: {}", req.url))?;
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, format!("/{p}")),
        None => (rest, "/".to_string()),
    };
    let host_port = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let mut stream = std::net::TcpStream::connect(&host_port)
        .map_err(|e| format!("http connect failed: {e}"))?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    use std::io::{Read, Write};
    let body_bytes = body_text.as_deref().unwrap_or("").as_bytes();
    let mut msg = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        req.method, path, authority
    );
    if !cookie.is_empty() {
        msg.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if form_set {
        msg.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
    }
    for (name, value) in &req.headers {
        msg.push_str(&format!("{name}: {value}\r\n"));
    }
    if body_text.is_some() {
        msg.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    msg.push_str("\r\n");
    stream
        .write_all(msg.as_bytes())
        .map_err(|e| format!("http write failed: {e}"))?;
    if !body_bytes.is_empty() {
        stream
            .write_all(body_bytes)
            .map_err(|e| format!("http body write failed: {e}"))?;
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header_bytes = header_end + 4;
                    let head = String::from_utf8_lossy(&buf[..header_end]);
                    let mut content_length = None;
                    for line in head.lines().skip(1) {
                        if let Some(v) = line
                            .split_once(':')
                            .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        {
                            content_length = Some(v);
                            break;
                        }
                    }
                    if let Some(len) = content_length {
                        while buf.len() < header_bytes + len {
                            match stream.read(&mut tmp) {
                                Ok(0) => break,
                                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                                Err(_) => break,
                            }
                        }
                        buf.truncate(header_bytes + len);
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let (head, body_text) = text.split_once("\r\n\r\n").unwrap_or((text.as_ref(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut resp = jet_http_srv_response(status, &body_text.to_string());
    for line in head.lines().skip(1) {
        if let Some((k, v)) = line.split_once(':') {
            let _ = resp.headers.append(&k.trim().to_string(), &v.trim().to_string());
        }
    }
    Ok(resp)
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

extern "C" fn jet_jit_http_client_request_send(req: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return result_err("invalid HTTPRequest".into());
    };
    match http_cleartext_request(&req) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_resp_header(resp: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_client_response_header(r, &name)),
        _ => None,
    })
    .and_then(|r| r.ok()))
}

extern "C" fn jet_jit_http_resp_cookies(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_response_cookies(r)),
        _ => None,
    }) {
        Some(rows) => list_of_strings(rows),
        None => list_of_strings(Vec::new()),
    }
}

host_fns! {
    struct NetHttpHostFns;
    register: register_net_http_symbols;
    declare: declare_net_http_host_fns(module) {
        let cc = module.target_config().default_call_conv;
        let mut sig0 = Signature::new(cc);
        sig0.returns.push(AbiParam::new(types::I64));
        let mut sig1 = Signature::new(cc);
        sig1.params.push(AbiParam::new(types::I64));
        sig1.returns.push(AbiParam::new(types::I64));
        let mut sig2 = Signature::new(cc);
        sig2.params.push(AbiParam::new(types::I64));
        sig2.params.push(AbiParam::new(types::I64));
        sig2.returns.push(AbiParam::new(types::I64));
        let mut sig3 = Signature::new(cc);
        for _ in 0..3 {
            sig3.params.push(AbiParam::new(types::I64));
        }
        sig3.returns.push(AbiParam::new(types::I64));
        let mut sig4 = Signature::new(cc);
        for _ in 0..4 {
            sig4.params.push(AbiParam::new(types::I64));
        }
        sig4.returns.push(AbiParam::new(types::I64));

        let mut sig5 = Signature::new(cc);
        for _ in 0..5 {
            sig5.params.push(AbiParam::new(types::I64));
        }
        sig5.returns.push(AbiParam::new(types::I64));
        let mut sig6 = Signature::new(cc);
        for _ in 0..6 {
            sig6.params.push(AbiParam::new(types::I64));
        }
        sig6.returns.push(AbiParam::new(types::I64));
        let mut sig7 = Signature::new(cc);
        for _ in 0..7 {
            sig7.params.push(AbiParam::new(types::I64));
        }
        sig7.returns.push(AbiParam::new(types::I64));

    }
    socket_addr: "jet_jit_net_socket_addr" => jet_jit_net_socket_addr: sig2;
    socket_to_string: "jet_jit_net_socket_to_string" => jet_jit_net_socket_to_string: sig1;
    socket_host: "jet_jit_net_socket_host" => jet_jit_net_socket_host: sig1;
    socket_port_typed: "jet_jit_net_socket_port_typed" => jet_jit_net_socket_port_typed: sig1;
    tcp_listen_str: "jet_jit_net_tcp_listen_str" => jet_jit_net_tcp_listen_str: sig1;
    tcp_listen_addr: "jet_jit_net_tcp_listen_addr" => jet_jit_net_tcp_listen_addr: sig1;
    tcp_connect: "jet_jit_net_tcp_connect" => jet_jit_net_tcp_connect: sig1;
    listener_local_socket_addr: "jet_jit_net_listener_local_socket_addr2" => jet_jit_net_listener_local_socket_addr: sig1;
    set_timeout: "jet_jit_net_set_timeout" => jet_jit_net_set_timeout: sig2;
    nodelay: "jet_jit_net_nodelay" => jet_jit_net_nodelay: sig1;
    set_nodelay: "jet_jit_net_set_nodelay" => jet_jit_net_set_nodelay: sig2;
    ttl: "jet_jit_net_ttl" => jet_jit_net_ttl: sig1;
    set_ttl: "jet_jit_net_set_ttl" => jet_jit_net_set_ttl: sig2;
    socket_type: "jet_jit_net_socket_type" => jet_jit_net_socket_type: sig1;
    sendfile: "jet_jit_net_sendfile" => jet_jit_net_sendfile: sig2;
    dns_ptr: "jet_jit_net_dns_ptr" => jet_jit_net_dns_ptr: sig2;
    getservbyname: "jet_jit_net_getservbyname" => jet_jit_net_getservbyname: sig1;
    getservbyport: "jet_jit_net_getservbyport" => jet_jit_net_getservbyport: sig1;
    tcp_reply: "jet_jit_net_tcp_reply" => jet_jit_net_tcp_reply: sig3;
    udp_bind: "jet_jit_net_udp_bind" => jet_jit_net_udp_bind: sig1;
    udp_local_addr: "jet_jit_net_udp_local_addr" => jet_jit_net_udp_local_addr: sig1;
    udp_set_timeout: "jet_jit_net_udp_set_timeout" => jet_jit_net_udp_set_timeout: sig2;
    udp_send_bytes_to: "jet_jit_net_udp_send_bytes_to" => jet_jit_net_udp_send_bytes_to: sig3;
    udp_send_bytes_to_deadline: "jet_jit_net_udp_send_bytes_to_deadline" => jet_jit_net_udp_send_bytes_to_deadline: sig4;
    udp_receive: "jet_jit_net_udp_receive" => jet_jit_net_udp_receive: sig2;
    udp_receive_deadline: "jet_jit_net_udp_receive_deadline" => jet_jit_net_udp_receive_deadline: sig3;
    udp_packet_bytes: "jet_jit_net_udp_packet_bytes" => jet_jit_net_udp_packet_bytes: sig1;
    udp_packet_original_len: "jet_jit_net_udp_packet_original_len" => jet_jit_net_udp_packet_original_len: sig1;
    udp_packet_truncated: "jet_jit_net_udp_packet_truncated" => jet_jit_net_udp_packet_truncated: sig1;
    unix_listen: "jet_jit_net_unix_listen" => jet_jit_net_unix_listen: sig1;
    unix_accept: "jet_jit_net_unix_accept" => jet_jit_net_unix_accept: sig1;
    unix_connect: "jet_jit_net_unix_connect" => jet_jit_net_unix_connect: sig1;
    unix_read: "jet_jit_net_unix_read" => jet_jit_net_unix_read: sig1;
    unix_write: "jet_jit_net_unix_write" => jet_jit_net_unix_write: sig2;
    unix_write_all_bytes: "jet_jit_net_unix_write_all_bytes" => jet_jit_net_unix_write_all_bytes: sig2;
    unix_close: "jet_jit_net_unix_close" => jet_jit_net_unix_close: sig1;
    tcp_accept: "jet_jit_tcp_listener_accept" => jet_jit_tcp_listener_accept: sig1;
    tcp_local_addr: "jet_jit_tcp_listener_local_addr" => jet_jit_tcp_listener_local_addr: sig1;
    tcp_read_text: "jet_jit_tcp_stream_read_text" => jet_jit_tcp_stream_read_text: sig2;
    tcp_write_all_bytes: "jet_jit_tcp_stream_write_all_bytes" => jet_jit_tcp_stream_write_all_bytes: sig2;
    tcp_close: "jet_jit_tcp_stream_close" => jet_jit_tcp_stream_close: sig1;
    tcp_ready: "jet_jit_tcp_stream_ready" => jet_jit_tcp_stream_ready: sig3;
    udp_ready: "jet_jit_udp_socket_ready" => jet_jit_udp_socket_ready: sig3;
    udp_close: "jet_jit_udp_socket_close" => jet_jit_udp_socket_close: sig1;
    ready_readable: "jet_jit_net_ready_readable" => jet_jit_net_ready_readable: sig1;
    ready_writable: "jet_jit_net_ready_writable" => jet_jit_net_ready_writable: sig1;
    http_mux_new: "jet_jit_http_mux_new" => jet_jit_http_mux_new: sig0;
    http_mux_add: "jet_jit_http_mux_add" => jet_jit_http_mux_add: sig4;
    http_response: "jet_jit_http_response" => jet_jit_http_response: sig2;
    http_req_body: "jet_jit_http_req_body" => jet_jit_http_req_body: sig1;
    http_req_method: "jet_jit_http_req_method" => jet_jit_http_req_method: sig1;
    http_req_path: "jet_jit_http_req_path" => jet_jit_http_req_path: sig1;
    http_req_param: "jet_jit_http_req_param" => jet_jit_http_req_param: sig2;
    http_req_header: "jet_jit_http_req_header" => jet_jit_http_req_header: sig2;
    http_body_text: "jet_jit_http_body_text" => jet_jit_http_body_text: sig2;
    http_body_bytes: "jet_jit_http_body_bytes" => jet_jit_http_body_bytes: sig2;
    http_body_json_text: "jet_jit_http_body_json_text" => jet_jit_http_body_json_text: sig3;
    http_body_copy_to: "jet_jit_http_body_copy_to" => jet_jit_http_body_copy_to: sig3;
    http_nominal_static: "jet_jit_http_nominal_static" => jet_jit_http_nominal_static: sig7;
    http_nominal_show: "jet_jit_http_nominal_show" => jet_jit_http_nominal_show: sig1;
    http_json_response: "jet_jit_http_json_response" => jet_jit_http_json_response: sig2;
    http_static_files: "jet_jit_http_static_files" => jet_jit_http_static_files: sig6;
    http_cors_policy: "jet_jit_http_cors_policy" => jet_jit_http_cors_policy: sig7;
    http_cors: "jet_jit_http_cors" => jet_jit_http_cors: sig2;
    http_project_json_decode_error: "jet_jit_http_project_json_decode_error" => jet_jit_http_project_json_decode_error: sig1;
    http_resp_status: "jet_jit_http_resp_status" => jet_jit_http_resp_status: sig1;
    http_resp_body: "jet_jit_http_resp_body" => jet_jit_http_resp_body: sig1;
    http_client_resp_body: "jet_jit_http_client_resp_body" => jet_jit_http_client_resp_body: sig1;
    http_server_bind: "jet_jit_http_server_bind" => jet_jit_http_server_bind: sig2;
    http_server_local_addr: "jet_jit_http_server_local_addr" => jet_jit_http_server_local_addr: sig1;
    http_server_serve: "jet_jit_http_server_serve" => jet_jit_http_server_serve: sig1;
    http_server_shutdown: "jet_jit_http_server_shutdown" => jet_jit_http_server_shutdown: sig2;
    http_shutdown_report_field: "jet_jit_http_shutdown_report_field" => jet_jit_http_shutdown_report_field: sig2;
    http_serve_once_listener: "jet_jit_http_serve_once_listener" => jet_jit_http_serve_once_listener: sig2;
    http_client_get: "jet_jit_http_client_get" => jet_jit_http_client_get: sig1;
    http_client_post: "jet_jit_http_client_post" => jet_jit_http_client_post: sig2;
    http_handler_bind: "jet_jit_http_handler_bind" => jet_jit_http_handler_bind: sig2;
    http_handler_bind1: "jet_jit_http_handler_bind1" => jet_jit_http_handler_bind1: sig2;
    http_handler_handle: "jet_jit_http_handler_handle" => jet_jit_http_handler_handle: sig2;
    http_mux_middleware: "jet_jit_http_mux_middleware" => jet_jit_http_mux_middleware: sig2;
    http_request_id: "jet_jit_http_request_id" => jet_jit_http_request_id: sig1;
    http_req_trailers: "jet_jit_http_req_trailers" => jet_jit_http_req_trailers: sig1;
    http_resp_trailers: "jet_jit_http_resp_trailers" => jet_jit_http_resp_trailers: sig2;
    http_req_body_len: "jet_jit_http_req_body_len" => jet_jit_http_req_body_len: sig1;
    http_req_under_limit: "jet_jit_http_req_under_limit" => jet_jit_http_req_under_limit: sig2;
    http_sse: "jet_jit_http_sse" => jet_jit_http_sse: sig1;
    http_static_file_range: "jet_jit_http_static_file_range" => jet_jit_http_static_file_range: sig3;
    http_client_request_new: "jet_jit_http_client_request_new" => jet_jit_http_client_request_new: sig2;
    http_client_request_body: "jet_jit_http_client_request_body" => jet_jit_http_client_request_body: sig2;
    http_client_request_form: "jet_jit_http_client_request_form" => jet_jit_http_client_request_form: sig3;
    http_client_request_cookie: "jet_jit_http_client_request_cookie" => jet_jit_http_client_request_cookie: sig3;
    http_client_request_header: "jet_jit_http_client_request_header" => jet_jit_http_client_request_header: sig3;
    http_client_request_redirects: "jet_jit_http_client_request_redirects" => jet_jit_http_client_request_redirects: sig2;
    http_client_request_connect_timeout: "jet_jit_http_client_request_connect_timeout" => jet_jit_http_client_request_connect_timeout: sig2;
    http_client_request_read_timeout: "jet_jit_http_client_request_read_timeout" => jet_jit_http_client_request_read_timeout: sig2;
    http_client_request_send: "jet_jit_http_client_request_send" => jet_jit_http_client_request_send: sig1;
    http_resp_header: "jet_jit_http_resp_header" => jet_jit_http_resp_header: sig2;
    http_resp_cookies: "jet_jit_http_resp_cookies" => jet_jit_http_resp_cookies: sig1;
    ws_upgrade: "jet_jit_ws_upgrade" => jet_jit_ws_upgrade: sig1;
    ws_connect: "jet_jit_ws_connect" => jet_jit_ws_connect: sig1;
    ws_send_text: "jet_jit_ws_send_text" => jet_jit_ws_send_text: sig2;
    ws_recv: "jet_jit_ws_recv" => jet_jit_ws_recv: sig1;
    ws_close: "jet_jit_ws_close" => jet_jit_ws_close: sig3;
    ws_message_is_text: "jet_jit_ws_message_is_text" => jet_jit_ws_message_is_text: sig1;
    ws_message_text: "jet_jit_ws_message_text" => jet_jit_ws_message_text: sig1;
}






// ── I9 shared Prelude adapters (C hosts + ambient call these; no forked logic) ─

/// Build a CORS policy via `jet_http_cors_policy` only. Returns an opaque handle.
/// `origins_any`: true → `.Any`; false → `List(origins)`.
pub(crate) struct RuntimeHttpError {
    pub(crate) value: CtValue,
    packed: i64,
}

pub(crate) fn runtime_cors_policy(
    origins_any: bool,
    origins: Vec<String>,
    methods: Option<Vec<String>>,
    headers: Option<Vec<String>>,
    credentials: Option<bool>,
    max_age: Option<i64>,
) -> Result<i64, RuntimeHttpError> {
    let origins = if origins_any {
        JetHTTPCorsOrigins::Any
    } else {
        JetHTTPCorsOrigins::List(origins)
    };
    match jet_http_cors_policy_defaulted(
        &origins,
        methods.as_ref(),
        headers.as_ref(),
        credentials,
        max_age,
    ) {
        Ok(policy) => Ok(push_handle(NetHttpHandle::HTTPCorsPolicy(policy))),
        Err(error) => {
            let (packed, value) = marshal_http_error(error);
            Err(RuntimeHttpError { value, packed })
        }
    }
}

/// Install CORS via `jet_http_srv_install_cors` only.
pub(crate) fn runtime_cors(mux: i64, policy: i64) -> Result<(), String> {
    let mux = http_mux(mux).ok_or_else(|| "invalid HTTPMux".to_string())?;
    let policy = with_handle(policy, |h| match h {
        NetHttpHandle::HTTPCorsPolicy(p) => Some(p.clone()),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPCorsPolicy".to_string())?;
    jet_http_srv_install_cors(&mux, &policy);
    Ok(())
}

/// Mount static files via `jet_http_srv_static_files_mount` only.
pub(crate) fn runtime_static_files(
    mux: i64,
    prefix: String,
    root: String,
    index: Option<bool>,
    dotfiles: Option<bool>,
    follow_links: Option<bool>,
) -> Result<(), String> {
    let mux = http_mux(mux).ok_or_else(|| "invalid HTTPMux".to_string())?;
    jet_http_srv_static_files_mount_defaulted(
        &mux,
        &prefix,
        &root,
        index,
        dotfiles,
        follow_links,
    );
    Ok(())
}

/// JSON response with AOT content-type — body is already JSON text (same as AOT
/// after `jet_enc_json_to_string`).
pub(crate) fn runtime_json_response(status: i64, body: String) -> i64 {
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_json_text(
        status, &body,
    )))
}

pub(crate) fn runtime_http_mux() -> i64 {
    push_handle(NetHttpHandle::HTTPMux(Arc::new(jet_http_mux_new())))
}

// ── I9 UDP ambient adapters ───────────────────────────────────────────────

fn net_ct_handle(type_name: &str, handle: i64) -> CtValue {
    let fields = if type_name == "SocketAddr" {
        with_handle(handle, |value| match value {
            NetHttpHandle::SocketAddr(address) => Some(vec![
                ("handle".to_string(), CtValue::Int(handle)),
                ("host".to_string(), CtValue::Str(jet_net_socket_host(address))),
                ("port".to_string(), CtValue::Int(jet_net_socket_port(address))),
                (
                    "text".to_string(),
                    CtValue::Str(jet_net_socket_to_string(address)),
                ),
            ]),
            _ => None,
        })
    } else {
        None
    };
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: fields.unwrap_or_else(|| vec![("handle".to_string(), CtValue::Int(handle))]),
    }
}

pub(crate) fn runtime_net_socket_addr(host: String, port: i64) -> CtValue {
    match jet_net_socket_addr(&host, port) {
        Ok(addr) => CtValue::Present(Box::new(net_ct_handle(
            "SocketAddr",
            push_handle(NetHttpHandle::SocketAddr(addr)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_net_socket_to_string(address: i64) -> CtValue {
    CtValue::Str(
        with_handle(address, |handle| match handle {
            NetHttpHandle::SocketAddr(address) => Some(jet_net_socket_to_string(address)),
            _ => None,
        })
        .unwrap_or_default(),
    )
}

pub(crate) fn runtime_net_socket_host(address: i64) -> CtValue {
    CtValue::Str(
        with_handle(address, |handle| match handle {
            NetHttpHandle::SocketAddr(address) => Some(jet_net_socket_host(address)),
            _ => None,
        })
        .unwrap_or_default(),
    )
}

pub(crate) fn runtime_net_socket_port(address: i64) -> CtValue {
    CtValue::Int(
        with_handle(address, |handle| match handle {
            NetHttpHandle::SocketAddr(address) => Some(jet_net_socket_port(address)),
            _ => None,
        })
        .unwrap_or(0),
    )
}

fn tcp_listener_result(listener: JetTCPListener) -> CtValue {
    CtValue::Present(Box::new(net_ct_handle(
        "TcpListener",
        push_handle(NetHttpHandle::TcpListener(Arc::new(listener))),
    )))
}

fn tcp_stream_result(stream: JetTCPStream) -> CtValue {
    CtValue::Present(Box::new(net_ct_handle(
        "TcpStream",
        push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(stream)))),
    )))
}

pub(crate) fn runtime_tcp_listen(address: String) -> CtValue {
    match jet_net_tcp_listen(&address) {
        Ok(listener) => tcp_listener_result(listener),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_listen_addr(address: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp listen",
            "SocketAddr",
        ))));
    };
    match jet_net_tcp_listen_addr(&address) {
        Ok(listener) => tcp_listener_result(listener),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_listener_accept(listener: i64, deadline: Option<i64>) -> CtValue {
    let Some(listener) = tcp_listener(listener) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp accept",
            "TcpListener",
        ))));
    };
    let result = match deadline {
        Some(ns) => {
            let deadline = jet_std::Duration { ns };
            jet_net_tcp_accept_deadline(&listener, &deadline)
        }
        None => jet_net_tcp_accept(&listener),
    };
    match result {
        Ok(stream) => tcp_stream_result(stream),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_connect(address: String) -> CtValue {
    match jet_net_tcp_connect(&address) {
        Ok(stream) => tcp_stream_result(stream),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_connect_addr(address: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp connect",
            "SocketAddr",
        ))));
    };
    match jet_net_tcp_connect_addr(&address) {
        Ok(stream) => tcp_stream_result(stream),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_connect_timeout(address: i64, timeout_ms: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp connect",
            "SocketAddr",
        ))));
    };
    match jet_net_tcp_connect_timeout(&address, timeout_ms) {
        Ok(stream) => tcp_stream_result(stream),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_connect_happy(host: String, port: i64, timeout_ms: i64) -> CtValue {
    match jet_net_tcp_connect_happy(&host, port, timeout_ms) {
        Ok(stream) => tcp_stream_result(stream),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_listener_local_addr(listener: i64) -> CtValue {
    let Some(listener) = tcp_listener(listener) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp listener local address",
            "TcpListener",
        ))));
    };
    match jet_net_listener_local_addr(&listener) {
        Ok(address) => CtValue::Present(Box::new(CtValue::Str(address))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_listener_local_socket_addr(listener: i64) -> CtValue {
    let Some(listener) = tcp_listener(listener) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp listener local address",
            "TcpListener",
        ))));
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(address) => CtValue::Present(Box::new(net_ct_handle(
            "SocketAddr",
            push_handle(NetHttpHandle::SocketAddr(address)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_read(stream: i64) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp read",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_read(&mut stream) {
        Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_read_bytes(
    stream: i64,
    limit: i64,
    deadline: Option<i64>,
) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp read",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match deadline {
        Some(ns) => {
            let deadline = jet_std::Duration { ns };
            jet_net_tcp_read_bytes_deadline(&mut stream, limit, &deadline)
        }
        None => jet_net_tcp_read_bytes(&mut stream, limit),
    };
    match result {
        Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_read_io(stream: i64, limit: i64) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_io_error_value(jet_std::IOError::Other(
            jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                Some("TcpStream".to_string()),
                None,
                Some("invalid TcpStream handle".to_string()),
            ),
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match JetIOReader::read(&mut *stream, limit) {
        Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
        Err(error) => CtValue::failed(Box::new(net_io_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_read_text(
    stream: i64,
    limit: i64,
    deadline: Option<i64>,
) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp read text",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match deadline {
        Some(ns) => {
            let deadline = jet_std::Duration { ns };
            jet_net_tcp_read_text_deadline(&mut stream, limit, &deadline)
        }
        None => jet_net_tcp_read_text(&mut stream, limit),
    };
    match result {
        Ok(value) => CtValue::Present(Box::new(CtValue::Str(value))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write(stream: i64, data: String) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp write",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_write(&mut stream, &data) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write_bytes(
    stream: i64,
    data: Vec<u8>,
    deadline: Option<i64>,
) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp write",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match deadline {
        Some(ns) => {
            let deadline = jet_std::Duration { ns };
            jet_net_tcp_write_bytes_deadline(&mut stream, &data, &deadline)
        }
        None => jet_net_tcp_write_bytes(&mut stream, &data),
    };
    match result {
        Ok(written) => CtValue::Present(Box::new(CtValue::Int(written))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write_io(stream: i64, data: Vec<u8>) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_io_error_value(jet_std::IOError::Other(
            jet_std::IOContext::new(
                jet_std::IOOperation::Write,
                Some("TcpStream".to_string()),
                None,
                Some("invalid TcpStream handle".to_string()),
            ),
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match JetIOWriter::write(&mut *stream, &data) {
        Ok(written) => CtValue::Present(Box::new(CtValue::Int(written))),
        Err(error) => CtValue::failed(Box::new(net_io_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write_all_bytes(
    stream: i64,
    data: Vec<u8>,
    deadline: Option<i64>,
) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp write all",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let result = match deadline {
        Some(ns) => {
            let deadline = jet_std::Duration { ns };
            jet_net_tcp_write_all_bytes_deadline(&mut stream, &data, &deadline)
        }
        None => jet_net_tcp_write_all_bytes(&mut stream, &data),
    };
    match result {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write_all_io(stream: i64, data: Vec<u8>) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_io_error_value(jet_std::IOError::Other(
            jet_std::IOContext::new(
                jet_std::IOOperation::Write,
                Some("TcpStream".to_string()),
                None,
                Some("invalid TcpStream handle".to_string()),
            ),
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match JetIOWriter::write_all(&mut *stream, &data) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_io_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_write_text(
    stream: i64,
    data: String,
    deadline: Option<i64>,
) -> CtValue {
    runtime_tcp_stream_write_all_bytes(stream, data.into_bytes(), deadline)
}

pub(crate) fn runtime_tcp_stream_shutdown(stream: i64, how: i64) -> CtValue {
    let Some(how) = (match how {
        0 => Some(JetNetShutdown::Read),
        1 => Some(JetNetShutdown::Write),
        2 => Some(JetNetShutdown::Both),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp shutdown",
            "NetShutdown",
        ))));
    };
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp shutdown",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_shutdown(&mut stream, how) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_close(stream: i64) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp close",
            "TcpStream",
        ))));
    };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_close(&mut stream) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_ready(stream: i64, interest: i64, deadline: i64) -> CtValue {
    let Some(interest) = net_ready_interest(interest) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp ready",
            "NetReadyInterest",
        ))));
    };
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp ready",
            "TcpStream",
        ))));
    };
    let deadline = jet_std::Duration { ns: deadline };
    let mut stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_ready_deadline(&mut stream, interest, &deadline) {
        Ok(ready) => CtValue::Present(Box::new(net_ct_handle(
            "NetReady",
            push_handle(NetHttpHandle::NetReady(Arc::new(ready))),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_local_addr(stream: i64) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp local address",
            "TcpStream",
        ))));
    };
    let stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_local_addr(&stream) {
        Ok(address) => CtValue::Present(Box::new(CtValue::Str(address))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_tcp_stream_peer_addr(stream: i64) -> CtValue {
    let Some(stream) = tcp_stream(stream) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "tcp peer address",
            "TcpStream",
        ))));
    };
    let stream = stream.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tcp_peer_addr(&stream) {
        Ok(address) => CtValue::Present(Box::new(CtValue::Str(address))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_bind(address: String) -> CtValue {
    match jet_net_udp_bind(&address) {
        Ok(socket) => CtValue::Present(Box::new(net_ct_handle(
            "UdpSocket",
            push_handle(NetHttpHandle::UdpSocket(Arc::new(socket))),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_bind_addr(address: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp bind",
            "SocketAddr",
        ))));
    };
    match jet_net_udp_bind_addr(&address) {
        Ok(socket) => CtValue::Present(Box::new(net_ct_handle(
            "UdpSocket",
            push_handle(NetHttpHandle::UdpSocket(Arc::new(socket))),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_local_addr(socket: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp local address",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_local_addr(&socket) {
        Ok(address) => CtValue::Present(Box::new(net_ct_handle(
            "SocketAddr",
            push_handle(NetHttpHandle::SocketAddr(address)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_set_timeout(socket: i64, timeout_ms: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "set udp timeout",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_set_timeout(&socket, timeout_ms) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_send_to(socket: i64, data: String, address: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "SocketAddr",
        ))));
    };
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_send_to(&socket, &data, &address) {
        Ok(bytes) => CtValue::Present(Box::new(CtValue::Int(bytes))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_recv_from(socket: i64, limit: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp receive",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_recv_from(&socket, limit) {
        Ok(packet) => CtValue::Present(Box::new(net_ct_handle(
            "UDPPacket",
            push_handle(NetHttpHandle::UDPPacket(packet)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_send_bytes_to(socket: i64, data: Vec<u8>, address: i64) -> CtValue {
    let Some(address) = with_handle(address, |handle| match handle {
        NetHttpHandle::SocketAddr(address) => Some(address.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "SocketAddr",
        ))));
    };
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_send_bytes_to(&socket, &data, &address) {
        Ok(bytes) => CtValue::Present(Box::new(CtValue::Int(bytes))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_receive(socket: i64, limit: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp receive",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_receive(&socket, limit) {
        Ok(packet) => CtValue::Present(Box::new(net_ct_handle(
            "UDPPacket",
            push_handle(NetHttpHandle::UDPPacket(packet)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_ready(socket: i64, interest: i64, deadline: i64) -> CtValue {
    let Some(interest) = net_ready_interest(interest) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp ready",
            "NetReadyInterest",
        ))));
    };
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp ready",
            "UdpSocket",
        ))));
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_ready(&socket, interest, &deadline) {
        Ok(ready) => CtValue::Present(Box::new(net_ct_handle(
            "NetReady",
            push_handle(NetHttpHandle::NetReady(Arc::new(ready))),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_close(socket: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp close",
            "UdpSocket",
        ))));
    };
    match jet_net_udp_close(&socket) {
        Ok(()) => CtValue::Present(Box::new(CtValue::Unit)),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_receive_deadline(socket: i64, limit: i64, deadline: i64) -> CtValue {
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp receive",
            "UdpSocket",
        ))));
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_receive_deadline(&socket, limit, &deadline) {
        Ok(packet) => CtValue::Present(Box::new(net_ct_handle(
            "UDPPacket",
            push_handle(NetHttpHandle::UDPPacket(packet)),
        ))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_send_to_deadline(
    socket: i64,
    data: Vec<u8>,
    addr: i64,
    deadline: i64,
) -> CtValue {
    let Some(addr) = with_handle(addr, |handle| match handle {
        NetHttpHandle::SocketAddr(addr) => Some(addr.clone()),
        _ => None,
    }) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "SocketAddr",
        ))));
    };
    let Some(socket) = udp_socket(socket) else {
        return CtValue::failed(Box::new(net_error_value(net_invalid_error(
            "udp send",
            "UdpSocket",
        ))));
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_send_bytes_to_deadline(&socket, &data, &addr, &deadline) {
        Ok(bytes) => CtValue::Present(Box::new(CtValue::Int(bytes))),
        Err(error) => CtValue::failed(Box::new(net_error_value(error))),
    }
}

pub(crate) fn runtime_udp_packet_data(packet: i64) -> CtValue {
    CtValue::Str(
        with_handle(packet, |handle| match handle {
            NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_data(packet)),
            _ => None,
        })
        .unwrap_or_default(),
    )
}

pub(crate) fn runtime_udp_packet_addr(packet: i64) -> CtValue {
    let address = with_handle(packet, |handle| match handle {
        NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_addr(packet)),
        _ => None,
    });
    match address {
        Some(address) => net_ct_handle(
            "SocketAddr",
            push_handle(NetHttpHandle::SocketAddr(address)),
        ),
        None => net_ct_handle("SocketAddr", 0),
    }
}

pub(crate) fn runtime_udp_packet_bytes(packet: i64) -> CtValue {
    CtValue::Bytes(
        with_handle(packet, |handle| match handle {
            NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_bytes(packet)),
            _ => None,
        })
        .unwrap_or_default(),
    )
}

pub(crate) fn runtime_udp_packet_original_len(packet: i64) -> CtValue {
    CtValue::Int(
        with_handle(packet, |handle| match handle {
            NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_original_len(packet)),
            _ => None,
        })
        .unwrap_or(0),
    )
}

pub(crate) fn runtime_udp_packet_truncated(packet: i64) -> CtValue {
    CtValue::Bool(
        with_handle(packet, |handle| match handle {
            NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_truncated(packet)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

pub(crate) fn runtime_net_ready_readable(ready: i64) -> CtValue {
    CtValue::Bool(
        net_ready(ready)
            .map(|ready| jet_net_ready_readable(&ready))
            .unwrap_or(false),
    )
}

pub(crate) fn runtime_net_ready_writable(ready: i64) -> CtValue {
    CtValue::Bool(
        net_ready(ready)
            .map(|ready| jet_net_ready_writable(&ready))
            .unwrap_or(false),
    )
}

fn marshal_http_error(error: JetHTTPError) -> (i64, CtValue) {
    let parts = jet_http_error_surface_parts(error);
    let (payload_bits, args) = match parts.payload {
        JetHTTPErrorSurfacePayload::Unit => (0, vec![]),
        JetHTTPErrorSurfacePayload::Int { field, value } => (
            value,
            vec![(Some(field.to_string()), CtValue::Int(value))],
        ),
        JetHTTPErrorSurfacePayload::Text { field, value } => (
            alloc_string(value.clone()),
            vec![(Some(field.to_string()), CtValue::Str(value))],
        ),
        JetHTTPErrorSurfacePayload::Operation {
            field,
            variant,
            ordinal,
        } => (
            ordinal,
            vec![(
                Some(field.to_string()),
                CtValue::Enum {
                    type_name: "HTTPOperation".into(),
                    variant: variant.to_string(),
                    args: vec![],
                },
            )],
        ),
    };
    let packed = payload_bits.wrapping_shl(8) | parts.ordinal;
    let value = CtValue::Enum {
        type_name: "HTTPError".into(),
        variant: parts.variant.to_string(),
        args,
    };
    (packed, value)
}

fn http_error_value(error: JetHTTPError) -> CtValue {
    marshal_http_error(error).1
}

fn http_ct_handle(type_name: &str, handle: i64) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("handle".to_string(), CtValue::Int(handle))],
    }
}

fn http_ct_outcome<T>(
    type_name: &str,
    result: Result<T, JetHTTPError>,
    store: impl FnOnce(T) -> i64,
) -> CtValue {
    match result {
        Ok(value) => CtValue::Present(Box::new(http_ct_handle(type_name, store(value)))),
        Err(error) => CtValue::failed(Box::new(http_error_value(error))),
    }
}

fn http_ct_string(value: &CtValue) -> Option<String> {
    match value {
        CtValue::Str(value) => Some(value.clone()),
        _ => None,
    }
}

fn http_ct_bytes(value: &CtValue) -> Option<Vec<u8>> {
    match value {
        CtValue::Bytes(value) => Some(value.clone()),
        CtValue::List(values) => values
            .iter()
            .map(|value| match value {
                CtValue::Int(value) if (0..=255).contains(value) => Some(*value as u8),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn http_ct_string_map(value: &CtValue) -> Option<BTreeMap<String, String>> {
    let CtValue::Map(values) = value else {
        return None;
    };
    values
        .iter()
        .map(|(key, value)| match (key, value) {
            (CtKey::Str(key), CtValue::Str(value)) => Some((key.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

fn http_ct_mime(value: &CtValue) -> Option<jet_std::JetMIME> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "Mime" {
        return None;
    }
    let field = |wanted: &str| fields.iter().find_map(|(name, value)| {
        (name == wanted).then_some(value)
    });
    let CtValue::Str(top) = field("top")? else {
        return None;
    };
    let CtValue::Str(sub) = field("sub")? else {
        return None;
    };
    let CtValue::List(params) = field("params")? else {
        return None;
    };
    let params = params
        .iter()
        .map(|param| match param {
            CtValue::List(pair) => match pair.as_slice() {
                [CtValue::Str(key), CtValue::Str(value)] => Some((key.clone(), value.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(jet_std::JetMIME {
        top: top.clone(),
        sub: sub.clone(),
        params,
    })
}

fn http_ct_reader(reader: crate::enc_stream::runtime::JetFileReader) -> JetFileReader {
    JetFileReader {
        inner: reader.inner,
        path: reader.path,
    }
}

fn http_ct_writer(writer: crate::enc_stream::runtime::JetFileWriter) -> JetFileWriter {
    JetFileWriter {
        inner: writer.inner,
        path: writer.path,
    }
}

/// Whole-program interpreter adapter for the nominal HTTP constructors. The
/// constructors and their validation remain in the included HTTP Prelude;
/// this function only turns CtValue arguments into the Prelude's carriers.
pub(crate) fn runtime_http_nominal_static(
    path: &str,
    method: &str,
    args: &[CtValue],
) -> Result<CtValue, String> {
    let type_name = path.rsplit("::").next().unwrap_or(path);
    let value = match (type_name, method, args.len()) {
        ("JetHTTPMethod", "custom", 1) => {
            let token = http_ct_string(&args[0]).ok_or_else(|| "HTTPMethod.custom text".to_string())?;
            http_ct_outcome("HTTPMethod", JetHTTPMethod::custom(token), |value| {
                push_handle(NetHttpHandle::HTTPMethod(value))
            })
        }
        ("JetHTTPMethod", "get", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::get())))
        }
        ("JetHTTPMethod", "head", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::head())))
        }
        ("JetHTTPMethod", "post", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::post())))
        }
        ("JetHTTPMethod", "put", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::put())))
        }
        ("JetHTTPMethod", "delete", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::delete())))
        }
        ("JetHTTPMethod", "connect", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::connect())))
        }
        ("JetHTTPMethod", "options", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::options())))
        }
        ("JetHTTPMethod", "trace", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::trace())))
        }
        ("JetHTTPMethod", "patch", 0) => {
            http_ct_handle("HTTPMethod", push_handle(NetHttpHandle::HTTPMethod(JetHTTPMethod::patch())))
        }
        ("JetHTTPStatus", "new", 1) => {
            let CtValue::Int(code) = args[0] else {
                return Err("HTTPStatus.new integer".to_string());
            };
            http_ct_outcome("HTTPStatus", JetHTTPStatus::new(code), |value| {
                push_handle(NetHttpHandle::HTTPStatus(value))
            })
        }
        ("JetHTTPVersion", "http_1_0", 0) => {
            http_ct_handle("HTTPVersion", push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_1_0())))
        }
        ("JetHTTPVersion", "http_1_1", 0) => {
            http_ct_handle("HTTPVersion", push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_1_1())))
        }
        ("JetHTTPVersion", "http_2", 0) => {
            http_ct_handle("HTTPVersion", push_handle(NetHttpHandle::HTTPVersion(JetHTTPVersion::http_2())))
        }
        ("JetHTTPHeaderName", "new", 1) => {
            let name = http_ct_string(&args[0]).ok_or_else(|| "HTTPHeaderName.new text".to_string())?;
            http_ct_outcome("HTTPHeaderName", JetHTTPHeaderName::new(name), |value| {
                push_handle(NetHttpHandle::HTTPHeaderName(value))
            })
        }
        ("JetHTTPHeaderValue", "new", 1) => {
            let value = http_ct_string(&args[0]).ok_or_else(|| "HTTPHeaderValue.new text".to_string())?;
            http_ct_outcome("HTTPHeaderValue", JetHTTPHeaderValue::new(value), |value| {
                push_handle(NetHttpHandle::HTTPHeaderValue(value))
            })
        }
        ("JetHTTPHeaders", "new", 0) => {
            http_ct_handle("HTTPHeaders", push_handle(NetHttpHandle::HTTPHeaders(JetHTTPHeaders::new())))
        }
        ("JetHTTPBody", "empty", 0) => {
            http_ct_handle("HTTPBody", push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::empty())))
        }
        ("JetHTTPBody", "bytes", 1) => {
            let bytes = http_ct_bytes(&args[0]).ok_or_else(|| "HTTPBody.bytes bytes".to_string())?;
            http_ct_handle("HTTPBody", push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_bytes(bytes))))
        }
        ("JetHTTPBody", "text", 1) => {
            let text = http_ct_string(&args[0]).ok_or_else(|| "HTTPBody.text text".to_string())?;
            http_ct_handle("HTTPBody", push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_text(text))))
        }
        ("JetHTTPBody", "text", 2) => {
            let text = http_ct_string(&args[0]).ok_or_else(|| "HTTPBody.text text".to_string())?;
            let mime = http_ct_mime(&args[1]).ok_or_else(|| "HTTPBody.text MIME".to_string())?;
            http_ct_handle(
                "HTTPBody",
                push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_text_with_mime(text, mime))),
            )
        }
        ("JetHTTPBody", "json", 1) => {
            let text = jet_codegen::Comptime::render_datatree_for_tir(&args[0]);
            http_ct_handle(
                "HTTPBody",
                push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_bytes_with_content_type(
                    text.into_bytes(),
                    Some("application/json".to_string()),
                ))),
            )
        }
        ("JetHTTPBody", "form", 1) => {
            let values = http_ct_string_map(&args[0]).ok_or_else(|| "HTTPBody.form map".to_string())?;
            http_ct_handle("HTTPBody", push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_form(values))))
        }
        ("JetHTTPBody", "multipart", 1) => {
            let values = http_ct_string_map(&args[0]).ok_or_else(|| "HTTPBody.multipart map".to_string())?;
            http_ct_handle(
                "HTTPBody",
                push_handle(NetHttpHandle::HTTPBody(JetHTTPBody::from_multipart(values))),
            )
        }
        ("JetHTTPBody", "reader", 1 | 2) => {
            let CtValue::Int(file) = args[0] else {
                return Err("HTTPBody.reader FileReader".to_string());
            };
            let reader = crate::enc_stream::take_file_reader_for_http(file)
                .map_err(|error| format!("HTTPBody.reader: {error}"))?;
            let reader = http_ct_reader(reader);
            let result = if args.len() == 1 {
                JetHTTPBody::from_reader(reader)
            } else {
                let CtValue::Int(limit) = args[1] else {
                    return Err("HTTPBody.reader length".to_string());
                };
                JetHTTPBody::from_reader_with_length(reader, limit)
            };
            http_ct_outcome("HTTPBody", result, |value| {
                push_handle(NetHttpHandle::HTTPBody(value))
            })
        }
        _ => return Err(format!("unsupported HTTP nominal static {type_name}.{method}")),
    };
    Ok(value)
}

pub(crate) fn runtime_http_nominal_show(handle: i64) -> Result<String, String> {
    with_handle(handle, |value| match value {
        NetHttpHandle::HTTPMethod(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPStatus(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPVersion(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPHeaderName(value) => Some(value.jet_show()),
        NetHttpHandle::HTTPHeaderValue(value) => Some(value.jet_show()),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTP nominal handle".to_string())
}

pub(crate) fn runtime_http_body_bytes(
    body: i64,
    limit: i64,
) -> Result<Result<Vec<u8>, CtValue>, String> {
    with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => Some(jet_http_body_bytes(body, limit).map_err(http_error_value)),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPBody".to_string())
}

pub(crate) fn runtime_http_body_text(
    body: i64,
    limit: i64,
) -> Result<Result<String, CtValue>, String> {
    with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => Some(jet_http_body_text(body, limit).map_err(http_error_value)),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPBody".to_string())
}

pub(crate) fn runtime_http_body_copy_to(
    body: i64,
    writer: crate::enc_stream::runtime::JetFileWriter,
    limit: i64,
) -> Result<Result<i64, CtValue>, String> {
    let mut writer = http_ct_writer(writer);
    with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => {
            Some(jet_http_body_copy_to(body, &mut writer, limit).map_err(http_error_value))
        }
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPBody".to_string())
}

pub(crate) fn runtime_http_req_body(request: i64) -> Result<i64, String> {
    let body = with_handle(request, |handle| match handle {
        NetHttpHandle::HTTPRequest(request) => Some(jet_http_srv_req_body(request)),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPRequest".to_string())?;
    Ok(push_handle(NetHttpHandle::HTTPBody(body)))
}

pub(crate) fn runtime_http_resp_body(response: i64) -> Result<i64, String> {
    let body = with_handle(response, |handle| match handle {
        NetHttpHandle::HTTPResponse(response) => Some(jet_http_client_response_body(response)),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPResponse".to_string())?;
    Ok(push_handle(NetHttpHandle::HTTPBody(body)))
}

pub(crate) fn runtime_http_body_json_text(
    body: i64,
    limit: Option<i64>,
) -> Result<Result<String, CtValue>, String> {
    with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => Some(
            jet_http_body_json_text_defaulted(body, limit).map_err(http_error_value),
        ),
        _ => None,
    })
    .ok_or_else(|| "invalid HTTPBody".to_string())
}

pub(crate) fn runtime_http_json_decode_error() -> CtValue {
    http_error_value(jet_http_json_decode_error())
}

pub(crate) fn runtime_http_request_body(
    request: i64,
    body: String,
) -> Result<i64, String> {
    let request =
        take_http_request(request).ok_or_else(|| "invalid HTTPRequest".to_string())?;
    Ok(push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_body(request, &body),
    )))
}

pub(crate) fn runtime_http_request_new(method: String, url: String) -> i64 {
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_new(
        &method, &url,
    )))
}

#[cfg(test)]
mod http_i9_adapter_tests {
    use super::*;

    #[test]
    fn http_error_marshalling_uses_canonical_surface_shape() {
        let (invalid, invalid_value) = marshal_http_error(JetHTTPError::InvalidFraming);
        assert_eq!(invalid, 5);
        assert!(matches!(
            invalid_value,
            CtValue::Enum { type_name, variant, args }
                if type_name == "HTTPError" && variant == "InvalidFraming" && args.is_empty()
        ));

        let reason = "named origins required".to_string();
        let (policy, policy_value) =
            marshal_http_error(JetHTTPError::Policy { reason: reason.clone() });
        assert_eq!(policy & 0xff, 17);
        assert!(matches!(
            policy_value,
            CtValue::Enum { type_name, variant, args }
                if type_name == "HTTPError"
                    && variant == "Policy"
                    && matches!(
                        args.as_slice(),
                        [(Some(field), CtValue::Str(value))]
                            if field == "reason" && value == "named origins required"
                    )
        ));
    }

    #[test]
    fn net_error_marshalling_uses_canonical_surface_shape() {
        let detail = jet_net_detail(
            "udp receive",
            Some("127.0.0.1:9".to_string()),
            None,
            "timed out".to_string(),
            Some(110),
        );
        let (timeout, timeout_value) = marshal_net_error(JetNetError::Timeout(detail));
        assert_eq!(timeout & 0xff, 8);
        assert!(matches!(
            timeout_value,
            CtValue::Enum { type_name, variant, args }
                if type_name == "NetError"
                    && variant == "Timeout"
                    && matches!(
                        args.as_slice(),
                        [(None, CtValue::Struct { type_name: detail_type, .. })]
                            if detail_type == "NetErrorDetail"
                    )
        ));

        let (dns, dns_value) = marshal_net_error(JetNetError::DNS(
            JetNetDnsError::NotFound("missing.example".to_string()),
        ));
        assert_eq!(dns & 0xff, 14);
        assert!(matches!(
            dns_value,
            CtValue::Enum { type_name, variant, args }
                if type_name == "NetError"
                    && variant == "DNS"
                    && matches!(
                        args.as_slice(),
                        [(None, CtValue::Enum { type_name: dns_type, variant: dns_variant, .. })]
                            if dns_type == "NetDnsError" && dns_variant == "NotFound"
                    )
        ));
    }

    #[test]
    fn cors_defaulting_preserves_explicit_max_age() {
        let origins = JetHTTPCorsOrigins::List(vec!["https://app.example".to_string()]);
        let defaulted =
            jet_http_cors_policy_defaulted(&origins, None, None, None, None).unwrap();
        let explicit =
            jet_http_cors_policy_defaulted(&origins, None, None, None, Some(i64::MIN)).unwrap();
        assert_eq!(defaulted.max_age_secs, 86_400);
        assert_eq!(explicit.max_age_secs, i64::MIN);
    }
}
