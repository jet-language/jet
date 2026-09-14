// Host shims for TCP/UDP/Unix + HTTP mux/server + WS — same module as net_http_rt includes.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::sync::MutexGuard;

use crate::runtime_host::JitCallableSlot;
use crate::Concurrency;
use crate::JitResultValue;
use crate::Marshal::{alloc_string, clone_string, result_err_msg, result_ok};

enum NetHttpHandle {
    TcpListener(Arc<JetTCPListener>),
    TcpStream(Arc<Mutex<JetTCPStream>>),
    TLSClientConfig(JetTLSClientConfig),
    TLSRootCertificates(JetTLSRootCertificates),
    TLSClientIdentity(JetTLSClientIdentity),
    TLSStream(Arc<Mutex<JetTLSStream>>),
    SocketAddr(JetSocketAddr),
    IPAddr(JetIpAddr),
    DNSSrv(JetDNSSrv),
    UdpSocket(Arc<JetUDPSocket>),
    NetReady(Arc<JetNetReady>),
    UDPPacket(JetUDPPacket),
    #[cfg(unix)]
    UnixListener(Arc<JetUnixListener>),
    #[cfg(unix)]
    UnixStream(Arc<Mutex<JetUnixStream>>),
    HTTPMux(Arc<JetHTTPMux>),
    HTTPRouter(Arc<Mutex<JetHTTPRouter>>),
    HTTPRequest(JetHTTPRequest),
    HTTPResponse(JetHTTPResponse),
    HTTPBody(JetHTTPBody),
    HTTPBodyChunks(JetHTTPBodyChunks),
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
    let servers = {
        let handles = lock_handles();
        handles
            .iter()
            .filter_map(|handle| match handle.as_ref() {
                Some(NetHttpHandle::HTTPServer(server)) => Some(Arc::clone(server)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let grace = jet_std::Duration { ns: 0 };
    for server in servers {
        Concurrency::notify_http_test_shutdown_started();
        let _ = jet_http_server_shutdown(&server, &grace);
    }
    Concurrency::with_http_runtime_quiesced(|| {
        lock_handles().clear();
        Concurrency::clear_http_shared_runtime();
    });
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
fn with_handle_mut<R>(handle: i64, f: impl FnOnce(&mut NetHttpHandle) -> Option<R>) -> Option<R> {
    let mut v = lock_handles();
    let idx = handle.saturating_sub(1) as usize;
    v.get_mut(idx).and_then(|s| s.as_mut()).and_then(f)
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
fn ip_addr(handle: i64) -> Option<JetIpAddr> {
    with_handle(handle, |h| match h {
        NetHttpHandle::IPAddr(value) => Some(value.clone()),
        _ => None,
    })
}

fn tls_client_config(handle: i64) -> Option<JetTLSClientConfig> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TLSClientConfig(config) => Some(config.clone()),
        _ => None,
    })
}

fn tls_root_certificates(handle: i64) -> Option<JetTLSRootCertificates> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TLSRootCertificates(roots) => Some(roots.clone()),
        _ => None,
    })
}

pub(crate) fn tls_root_certificates_for_ambient(handle: i64) -> Option<JetTLSRootCertificates> {
    tls_root_certificates(handle)
}

fn tls_client_identity(handle: i64) -> Option<JetTLSClientIdentity> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TLSClientIdentity(identity) => Some(identity.clone()),
        _ => None,
    })
}

fn tls_stream(handle: i64) -> Option<Arc<Mutex<JetTLSStream>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TLSStream(stream) => Some(Arc::clone(stream)),
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
fn jet_jit_http_router_new() -> i64 {
    push_handle(NetHttpHandle::HTTPRouter(Arc::new(Mutex::new(
        jet_http_router_new(),
    ))))
}

fn jet_jit_http_router_register(
    router: i64,
    method: i64,
    pattern: i64,
    callable: i64,
    file: i64,
    line: i64,
    contract: i64,
) -> i64 {
    let method = clone_string(method);
    let pattern = clone_string(pattern);
    let file = clone_string(file);
    let contract = clone_string(contract);
    let handler = with_handle(callable, |handle| match handle {
        NetHttpHandle::HTTPHandler(handler) => Some(Arc::clone(handler)),
        _ => None,
    })
    .or_else(|| wrap_bound_http_handler(callable));
    let Some(handler) = handler else {
        Concurrency::with_runtime_mut(|runtime| runtime.set_trap("invalid resident HTTP handler"));
        return 0;
    };
    let Some(router) = http_router(router) else {
        Concurrency::with_runtime_mut(|runtime| runtime.set_trap("invalid HTTPRouter"));
        return 0;
    };
    let mut router = router.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    jet_http_router_register(
        &mut router,
        method,
        pattern,
        handler,
        &file,
        line as u32,
        contract,
    );
    0
}

fn http_router(handle: i64) -> Option<Arc<Mutex<JetHTTPRouter>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HTTPRouter(router) => Some(Arc::clone(router)),
        _ => None,
    })
}
fn jet_jit_http_openapi(router: i64) -> i64 {
    let Some(router) = http_router(router) else {
        Concurrency::with_runtime_mut(|runtime| runtime.set_trap("invalid HTTPRouter"));
        return 0;
    };
    let router = router.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    alloc_string(crate::Web::web_rt::jet_web_openapi(&router))
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
    result_err_bits(marshal_net_error(e))
}

fn io_err(e: jet_std::IOError) -> i64 {
    result_err_bits(marshal_io_error(e))
}

fn http_err(e: JetHTTPError) -> i64 {
    result_err_bits(marshal_http_error_packed(e))
}

fn option_string(s: Option<String>) -> i64 {
    match s {
        None => 0,
        Some(v) => alloc_string(v).wrapping_add(1),
    }
}
fn option_int(value: Option<i64>) -> i64 {
    value.map(|value| value.wrapping_add(1)).unwrap_or(0)
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

fn map_io_ok<T>(r: Result<T, jet_std::IOError>, f: impl FnOnce(T) -> i64) -> i64 {
    match r {
        Ok(v) => result_ok_handle(f(v)),
        Err(e) => io_err(e),
    }
}

fn map_io_unit(r: Result<(), jet_std::IOError>) -> i64 {
    match r {
        Ok(()) => result_ok_unit(),
        Err(e) => io_err(e),
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


fn marshal_net_error(error: JetNetError) -> i64 {
    let parts = jet_net_error_surface_parts(error);
    let payload_bits = match parts.payload {
        JetNetErrorSurfacePayload::Detail(detail) => net_error_detail_handle(detail),
        JetNetErrorSurfacePayload::DNS {
            ordinal,
            value,
            ..
        } => alloc_string(value).wrapping_shl(8) | ordinal,
    };
    payload_bits.wrapping_shl(8) | parts.ordinal
}
fn marshal_http_error_packed(error: JetHTTPError) -> i64 {
    let parts = jet_http_error_surface_parts(error);
    let payload = match parts.payload {
        JetHTTPErrorSurfacePayload::Unit => 0,
        JetHTTPErrorSurfacePayload::Int { value, .. } => value,
        JetHTTPErrorSurfacePayload::Text { value, .. } => alloc_string(value),
        JetHTTPErrorSurfacePayload::Operation { ordinal, .. } => ordinal,
    };
    payload.wrapping_shl(8) | parts.ordinal
}



fn net_io_operation_ordinal(operation: jet_std::IOOperation) -> i64 {
    match operation {
        jet_std::IOOperation::Read => 0,
        jet_std::IOOperation::Write => 1,
        jet_std::IOOperation::Flush => 2,
        jet_std::IOOperation::Connect => 3,
        jet_std::IOOperation::Accept => 4,
        jet_std::IOOperation::Close => 5,
        jet_std::IOOperation::Resolve => 6,
        jet_std::IOOperation::Codec => 7,
    }
}

fn net_io_context_handle(context: &jet_std::IOContext) -> i64 {
    let operation = net_io_operation_ordinal(context.operation);
    let resource = option_string(context.resource.clone());
    let os_code = context
        .os_code
        .map(|value| value.wrapping_add(1))
        .unwrap_or(0);
    let cause = option_string(context.cause.clone());
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(4);
        let _ = rt.heap.record_set_int(record, 0, operation);
        let _ = rt.heap.record_set_int(record, 1, resource);
        let _ = rt.heap.record_set_int(record, 2, os_code);
        let _ = rt.heap.record_set_int(record, 3, cause);
        record
    })
}

fn marshal_io_error(error: jet_std::IOError) -> i64 {
    let (_variant, ordinal, context) = match error {
        jet_std::IOError::InvalidInput(context) => ("InvalidInput", 0, context),
        jet_std::IOError::NotFound(context) => ("NotFound", 1, context),
        jet_std::IOError::PermissionDenied(context) => ("PermissionDenied", 2, context),
        jet_std::IOError::TimedOut(context) => ("TimedOut", 3, context),
        jet_std::IOError::Cancelled(context) => ("Cancelled", 4, context),
        jet_std::IOError::Closed(context) => ("Closed", 5, context),
        jet_std::IOError::Protocol(context) => ("Protocol", 6, context),
        jet_std::IOError::Other(context) => ("Other", 7, context),
    };
    let context_bits = net_io_context_handle(&context);
    context_bits.wrapping_shl(8) | ordinal
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
type HTTPHandlerWithEnvFn = unsafe extern "C" fn(i64, i64) -> i64;
type HTTPZeroHandlerFn = unsafe extern "C" fn() -> i64;
type HTTPZeroHandlerWithEnvFn = unsafe extern "C" fn(i64) -> i64;

fn decode_http_handler_result(res_h: i64) -> Result<JetHTTPResponse, JetHTTPError> {
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
}

fn resident_http_callable(handle: i64) -> Option<(usize, JitCallableSlot)> {
    Concurrency::http_callable_snapshot(handle)
}

fn invalid_http_handler() -> JetHTTPHandler {
    Arc::new(|_| {
        Err(JetHTTPError::IO {
            operation: "invalid resident HTTP handler".into(),
        })
    })
}
pub(crate) struct TestHttpHandler(JetHTTPHandler);

pub(crate) fn test_capture_http_handler(callable: i64) -> TestHttpHandler {
    TestHttpHandler(wrap_http_handler(callable))
}

pub(crate) fn test_invoke_captured_http_handler(handler: &TestHttpHandler) -> Result<(), String> {
    let request = JetHTTPRequest::server("GET", "/".to_string(), Vec::new(), JetHTTPHeaders::new());
    match (handler.0)(request) {
        Ok(_) => Ok(()),
        Err(JetHTTPError::IO { operation }) => Err(operation),
        Err(_) => Err("HTTP handler returned an error".to_string()),
    }
}

fn wrap_http_handler(callable: i64) -> JetHTTPHandler {
    let Some((epoch, slot)) = resident_http_callable(callable) else {
        return invalid_http_handler();
    };
    Arc::new(
        move |req: JetHTTPRequest| -> Result<JetHTTPResponse, JetHTTPError> {
            Concurrency::try_with_http_jet_runtime_at(epoch, || {
                let req_h = push_handle(NetHttpHandle::HTTPRequest(req));
                Concurrency::notify_http_test_handler_entry();
                let res_h = unsafe {
                    if slot.has_env {
                        let f: HTTPHandlerWithEnvFn = std::mem::transmute(slot.fn_ptr as usize);
                        f(slot.env, req_h)
                    } else {
                        let f: HTTPHandlerFn = std::mem::transmute(slot.fn_ptr as usize);
                        f(req_h)
                    }
                };
                decode_http_handler_result(res_h)
            })
            .unwrap_or_else(|| {
                Err(JetHTTPError::IO {
                    operation: "HTTP handler runtime unavailable".into(),
                })
            })
        },
    )
}

fn wrap_bound_http_handler(callable: i64) -> Option<JetHTTPHandler> {
    resident_http_callable(callable).map(|_| wrap_http_handler(callable))
}

fn wrap_http_zero_handler(callable: i64) -> JetHTTPHandler {
    let Some((epoch, slot)) = resident_http_callable(callable) else {
        return invalid_http_handler();
    };
    Arc::new(
        move |_req: JetHTTPRequest| -> Result<JetHTTPResponse, JetHTTPError> {
            Concurrency::try_with_http_jet_runtime_at(epoch, || {
                let res_h = unsafe {
                    if slot.has_env {
                        let f: HTTPZeroHandlerWithEnvFn = std::mem::transmute(slot.fn_ptr as usize);
                        f(slot.env)
                    } else {
                        let f: HTTPZeroHandlerFn = std::mem::transmute(slot.fn_ptr as usize);
                        f()
                    }
                };
                decode_http_handler_result(res_h)
            })
            .unwrap_or_else(|| {
                Err(JetHTTPError::IO {
                    operation: "HTTP handler runtime unavailable".into(),
                })
            })
        },
    )
}

// ── core.net ───────────────────────────────────────────────────────────────

fn jet_jit_net_socket_addr(host: i64, port: i64) -> i64 {
    let host = clone_string(host);
    map_net_ok(jet_net_socket_addr(&host, port), |a| {
        push_handle(NetHttpHandle::SocketAddr(a))
    })
}
fn jet_jit_net_ip_addr(text: i64) -> i64 {
    let text = clone_string(text);
    map_net_ok(jet_net_ip_addr(&text), |ip| {
        push_handle(NetHttpHandle::IPAddr(ip))
    })
}

fn jet_jit_net_ip_to_string(ip: i64) -> i64 {
    ip_addr(ip)
        .map(|ip| alloc_string(jet_net_ip_to_string(&ip)))
        .unwrap_or_else(|| alloc_string(String::new()))
}

fn jet_jit_net_ip_is_ipv4(ip: i64) -> i64 {
    i64::from(ip_addr(ip).is_some_and(|ip| jet_net_ip_is_ipv4(&ip)))
}

fn jet_jit_net_socket_addr_parse(text: i64) -> i64 {
    let text = clone_string(text);
    map_net_ok(jet_net_socket_addr_parse(&text), |addr| {
        push_handle(NetHttpHandle::SocketAddr(addr))
    })
}

fn jet_jit_net_tcp_connect_addr(addr: i64) -> i64 {
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(addr) => Some(addr.clone()),
        _ => None,
    }) else {
        return net_invalid("tcp connect", "SocketAddr");
    };
    map_net_ok(jet_net_tcp_connect_addr(&addr), |stream| {
        push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(stream))))
    })
}

fn jet_jit_net_tcp_connect_happy(host: i64, port: i64, timeout_ms: i64) -> i64 {
    let host = clone_string(host);
    map_net_ok(
        jet_net_tcp_connect_happy(&host, port, timeout_ms),
        |stream| push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(stream)))),
    )
}

fn jet_jit_net_socket_to_string(addr: i64) -> i64 {
    match with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_to_string(a)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

fn jet_jit_net_socket_host(addr: i64) -> i64 {
    match with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_host(a)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

fn jet_jit_net_socket_port_typed(addr: i64) -> i64 {
    with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(jet_net_socket_port(a)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_net_tcp_listen_str(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_tcp_listen(&addr), |l| {
        push_handle(NetHttpHandle::TcpListener(Arc::new(l)))
    })
}

fn jet_jit_net_tcp_listen_addr(addr: i64) -> i64 {
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

fn jet_jit_net_tcp_connect(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_tcp_connect(&addr), |s| {
        push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(s))))
    })
}
fn jet_jit_net_tcp_connect_timeout(addr: i64, timeout_ms: i64) -> i64 {
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return net_invalid("tcp connect timeout", "SocketAddr");
    };
    match jet_net_tcp_connect_timeout(&addr, timeout_ms) {
        Ok(stream) => result_ok_handle(push_handle(NetHttpHandle::TcpStream(Arc::new(
            Mutex::new(stream),
        )))),
        Err(error) => net_err(error),
    }
}
fn jet_jit_net_tcp_read(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp read", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_read(&mut guard) {
        Ok(value) => result_ok_handle(alloc_string(value)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_write(stream: i64, data: i64) -> i64 {
    let data = clone_string(data);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp write", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_write(&mut guard, &data))
}

fn jet_jit_net_tcp_read_bytes(stream: i64, limit: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp read", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_read_bytes(&mut guard, limit) {
        Ok(bytes) => result_ok_handle(alloc_bytes(&bytes)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_write_bytes(stream: i64, data: i64) -> i64 {
    let data = clone_bytes(data);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp write", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_write_bytes(&mut guard, &data) {
        Ok(count) => result_ok(count as u64),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_write_text(stream: i64, text: i64) -> i64 {
    let text = clone_string(text);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp write", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_write_text(&mut guard, &text))
}

fn jet_jit_net_listener_local_socket_addr(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("listener local address", "TcpListener");
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}
fn jet_jit_net_tcp_stream_local_addr(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp local address", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_local_addr(&guard) {
        Ok(address) => result_ok_handle(alloc_string(address)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_stream_peer_addr(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp peer address", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_peer_addr(&guard) {
        Ok(address) => result_ok_handle(alloc_string(address)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_stream_local_socket_addr(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp local address", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_local_socket_addr(&guard) {
        Ok(address) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(address))),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_tcp_stream_peer_socket_addr(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp peer address", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_peer_socket_addr(&guard) {
        Ok(address) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(address))),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_set_read_timeout(stream: i64, ms: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_read_timeout", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_read_timeout(&mut guard, ms))
}

fn jet_jit_net_set_write_timeout(stream: i64, ms: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_write_timeout", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_write_timeout(&mut guard, ms))
}

fn jet_jit_net_dns_aaaa(name: i64, ms: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_aaaa(&name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_handles(
            rows.into_iter().map(NetHttpHandle::IPAddr).collect(),
        )),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_dns_aaaa_at(server: i64, name: i64, ms: i64) -> i64 {
    let server = clone_string(server);
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_aaaa_at(&server, &name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_handles(
            rows.into_iter().map(NetHttpHandle::IPAddr).collect(),
        )),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_dns_srv(name: i64, ms: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_srv(&name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_handles(
            rows.into_iter().map(NetHttpHandle::DNSSrv).collect(),
        )),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_dns_srv_at(server: i64, name: i64, ms: i64) -> i64 {
    let server = clone_string(server);
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_srv_at(&server, &name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_handles(
            rows.into_iter().map(NetHttpHandle::DNSSrv).collect(),
        )),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_dns_srv_target(srv: i64) -> i64 {
    with_handle(srv, |handle| match handle {
        NetHttpHandle::DNSSrv(srv) => Some(jet_net_dns_srv_target(srv)),
        _ => None,
    })
    .map(alloc_string)
    .unwrap_or_else(|| alloc_string(String::new()))
}

fn jet_jit_net_dns_srv_port(srv: i64) -> i64 {
    with_handle(srv, |handle| match handle {
        NetHttpHandle::DNSSrv(srv) => Some(jet_net_dns_srv_port(srv)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_net_dns_srv_priority(srv: i64) -> i64 {
    with_handle(srv, |handle| match handle {
        NetHttpHandle::DNSSrv(srv) => Some(jet_net_dns_srv_priority(srv)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_net_dns_srv_weight(srv: i64) -> i64 {
    with_handle(srv, |handle| match handle {
        NetHttpHandle::DNSSrv(srv) => Some(jet_net_dns_srv_weight(srv)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_net_set_timeout(stream: i64, ms: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_timeout", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_timeout(&mut guard, ms))
}

fn jet_jit_net_nodelay(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("nodelay", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_nodelay(&guard) {
        Ok(v) => result_ok(if v { 1 } else { 0 }),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_set_nodelay(stream: i64, enabled: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_nodelay", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_nodelay(&guard, enabled != 0))
}

fn jet_jit_net_ttl(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("ttl", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_ttl(&guard) {
        Ok(v) => result_ok(v as u64),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_set_ttl(stream: i64, ttl: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("set_ttl", "TcpStream");
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_ttl(&guard, ttl))
}

fn jet_jit_net_socket_type(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return alloc_string(String::new());
    };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    alloc_string(jet_net_socket_type(&guard))
}

fn jet_jit_net_sendfile(stream: i64, path: i64) -> i64 {
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

fn jet_jit_net_dns_ptr(name: i64, ms: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_ptr(&name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_strings(rows)),
        Err(e) => net_err(e),
    }
}
fn jet_jit_net_dns_txt(name: i64, ms: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_txt(&name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_strings(rows)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_dns_txt_at(server: i64, name: i64, ms: i64) -> i64 {
    let server = clone_string(server);
    let name = clone_string(name);
    match jet_net_dns_result(jet_net_dns_txt_at(&server, &name, ms), &name) {
        Ok(rows) => result_ok_handle(list_of_strings(rows)),
        Err(error) => net_err(error),
    }
}

fn jet_jit_net_getservbyname(name: i64) -> i64 {
    let name = clone_string(name);
    match jet_net_getservbyname(&name) {
        Ok(port) => result_ok(port as u64),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_getservbyport(port: i64) -> i64 {
    match jet_net_getservbyport(port) {
        Ok(name) => result_ok_handle(alloc_string(name)),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_tcp_reply(stream: i64, status: i64, body: i64) -> i64 {
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

fn jet_jit_net_udp_bind(addr: i64) -> i64 {
    let addr = clone_string(addr);
    map_net_ok(jet_net_udp_bind(&addr), |s| {
        push_handle(NetHttpHandle::UdpSocket(Arc::new(s)))
    })
}
fn jet_jit_net_udp_bind_addr(addr: i64) -> i64 {
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(addr) => Some(addr.clone()),
        _ => None,
    }) else {
        return net_invalid("udp bind", "SocketAddr");
    };
    map_net_ok(jet_net_udp_bind_addr(&addr), |socket| {
        push_handle(NetHttpHandle::UdpSocket(Arc::new(socket)))
    })
}

fn jet_jit_net_udp_local_addr(socket: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_local_addr", "UdpSocket");
    };
    match jet_net_udp_local_addr(&socket) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_udp_set_timeout(socket: i64, ms: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_set_timeout", "UdpSocket");
    };
    map_net_unit(jet_net_udp_set_timeout(&socket, ms))
}

fn jet_jit_net_udp_send_bytes_to(socket: i64, data: i64, addr: i64) -> i64 {
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
fn jet_jit_net_udp_send_to(socket: i64, data: i64, addr: i64) -> i64 {
    let data = clone_string(data);
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return net_invalid("udp_send", "SocketAddr");
    };
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_send", "UdpSocket");
    };
    match jet_net_udp_send_to(&socket, &data, &addr) {
        Ok(n) => result_ok(n as u64),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_udp_receive(socket: i64, limit: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp_receive", "UdpSocket");
    };
    match jet_net_udp_receive(&socket, limit) {
        Ok(p) => result_ok_handle(push_handle(NetHttpHandle::UDPPacket(p))),
        Err(e) => net_err(e),
    }
}

fn jet_jit_net_udp_send_bytes_to_deadline(socket: i64, data: i64, addr: i64, deadline: i64) -> i64 {
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

fn jet_jit_net_udp_receive_deadline(socket: i64, limit: i64, deadline: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp receive", "UdpSocket");
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_udp_receive_deadline(&socket, limit, &deadline) {
        Ok(packet) => result_ok_handle(push_handle(NetHttpHandle::UDPPacket(packet))),
        Err(e) => net_err(e),
    }
}
fn jet_jit_net_udp_packet_data(packet: i64) -> i64 {
    let data = with_handle(packet, |h| match h {
        NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_data(p)),
        _ => None,
    })
    .unwrap_or_default();
    alloc_string(data)
}

fn jet_jit_net_udp_packet_bytes(packet: i64) -> i64 {
    match with_handle(packet, |h| match h {
        NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_bytes(p)),
        _ => None,
    }) {
        Some(b) => alloc_bytes(&b),
        None => alloc_bytes(&[]),
    }
}

fn jet_jit_net_udp_packet_original_len(packet: i64) -> i64 {
    with_handle(packet, |h| match h {
        NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_original_len(p)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_net_udp_packet_truncated(packet: i64) -> i64 {
    i64::from(
        with_handle(packet, |h| match h {
            NetHttpHandle::UDPPacket(p) => Some(jet_net_udp_packet_truncated(p)),
            _ => None,
        })
        .unwrap_or(false),
    )
}
fn jet_jit_net_udp_packet_addr(packet: i64) -> i64 {
    // Copy the address while holding the handle-table lock, then publish the
    // new address handle after releasing it. Calling `push_handle` inside the
    // `with_handle` callback would re-lock the same table and deadlock.
    let address = with_handle(packet, |handle| match handle {
        NetHttpHandle::UDPPacket(packet) => Some(jet_net_udp_packet_addr(packet)),
        _ => None,
    });
    address
        .map(|address| push_handle(NetHttpHandle::SocketAddr(address)))
        .unwrap_or(0)
}

#[cfg(unix)]
fn jet_jit_net_unix_listen(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_listen(&path), |l| {
        push_handle(NetHttpHandle::UnixListener(Arc::new(l)))
    })
}

#[cfg(unix)]
fn jet_jit_net_unix_accept(listener: i64) -> i64 {
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
fn jet_jit_net_unix_connect(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_connect(&path), |s| {
        push_handle(NetHttpHandle::UnixStream(Arc::new(Mutex::new(s))))
    })
}

#[cfg(unix)]
fn jet_jit_net_unix_read(stream: i64) -> i64 {
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
fn jet_jit_net_unix_write(stream: i64, data: i64) -> i64 {
    let data = clone_string(data);
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_write", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write(&mut guard, &data))
}

#[cfg(unix)]
fn jet_jit_net_unix_write_all_bytes(stream: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_write_all_bytes", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write_all_bytes(&mut guard, &bytes))
}

#[cfg(unix)]
fn jet_jit_net_unix_close(stream: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix_close", "UnixStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_close(&mut guard))
}

#[cfg(not(unix))]
fn jet_jit_net_unix_listen(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_listen(&path), |_| 0)
}
#[cfg(not(unix))]
fn jet_jit_net_unix_accept(_listener: i64) -> i64 {
    let listener = JetUnixListener;
    map_net_ok(jet_net_unix_accept(&listener), |_| 0)
}
#[cfg(not(unix))]
fn jet_jit_net_unix_connect(path: i64) -> i64 {
    let path = clone_string(path);
    map_net_ok(jet_net_unix_connect(&path), |_| 0)
}
#[cfg(not(unix))]
fn jet_jit_net_unix_read(_stream: i64) -> i64 {
    let mut stream = JetUnixStream;
    match jet_net_unix_read(&mut stream) {
        Ok(value) => result_ok_handle(alloc_string(value)),
        Err(error) => net_err(error),
    }
}
#[cfg(not(unix))]
fn jet_jit_net_unix_write(_stream: i64, data: i64) -> i64 {
    let mut stream = JetUnixStream;
    let data = clone_string(data);
    map_net_unit(jet_net_unix_write(&mut stream, &data))
}
#[cfg(not(unix))]
fn jet_jit_net_unix_write_all_bytes(_stream: i64, data: i64) -> i64 {
    let mut stream = JetUnixStream;
    let data = clone_bytes(data);
    map_net_unit(jet_net_unix_write_all_bytes(&mut stream, &data))
}
#[cfg(not(unix))]
fn jet_jit_net_unix_close(_stream: i64) -> i64 {
    let mut stream = JetUnixStream;
    map_net_unit(jet_net_unix_close(&mut stream))
}
#[cfg(unix)]
fn jet_jit_net_unix_accept_deadline(listener: i64, deadline: i64) -> i64 {
    let Some(listener) = unix_listener(listener) else {
        return net_invalid("unix accept", "UnixListener");
    };
    let deadline = jet_std::Duration { ns: deadline };
    match jet_net_unix_accept_deadline(&listener, &deadline) {
        Ok(stream) => result_ok_handle(push_handle(NetHttpHandle::UnixStream(Arc::new(
            Mutex::new(stream),
        )))),
        Err(error) => net_err(error),
    }
}

#[cfg(not(unix))]
fn jet_jit_net_unix_accept_deadline(_listener: i64, deadline: i64) -> i64 {
    let listener = JetUnixListener;
    let deadline = jet_std::Duration { ns: deadline };
    net_err(jet_net_unix_accept_deadline(&listener, &deadline).unwrap_err())
}

#[cfg(unix)]
fn jet_jit_net_unix_read_bytes_deadline(stream: i64, limit: i64, deadline: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix read", "UnixStream");
    };
    let deadline = jet_std::Duration { ns: deadline };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_unix_read_bytes_deadline(&mut guard, limit, &deadline) {
        Ok(bytes) => result_ok_handle(alloc_bytes(&bytes)),
        Err(error) => net_err(error),
    }
}

#[cfg(not(unix))]
fn jet_jit_net_unix_read_bytes_deadline(_stream: i64, _limit: i64, deadline: i64) -> i64 {
    let mut stream = JetUnixStream;
    let deadline = jet_std::Duration { ns: deadline };
    net_err(
        jet_net_unix_read_bytes_deadline(&mut stream, 0, &deadline)
            .unwrap_err(),
    )
}

#[cfg(unix)]
fn jet_jit_net_unix_write_all_bytes_deadline(stream: i64, data: i64, deadline: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix write", "UnixStream");
    };
    let deadline = jet_std::Duration { ns: deadline };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write_all_bytes_deadline(
        &mut guard,
        &bytes,
        &deadline,
    ))
}

#[cfg(not(unix))]
fn jet_jit_net_unix_write_all_bytes_deadline(
    _stream: i64,
    data: i64,
    deadline: i64,
) -> i64 {
    let mut stream = JetUnixStream;
    let bytes = clone_bytes(data);
    let deadline = jet_std::Duration { ns: deadline };
    net_err(
        jet_net_unix_write_all_bytes_deadline(&mut stream, &bytes, &deadline)
            .unwrap_err(),
    )
}

#[cfg(unix)]
fn jet_jit_net_unix_ready(stream: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return net_invalid("unix ready", "NetReadyInterest");
    };
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix ready", "UnixStream");
    };
    let deadline = jet_std::Duration { ns: deadline };
    let guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_ok(jet_net_unix_ready(&guard, interest, &deadline), |ready| {
        push_handle(NetHttpHandle::NetReady(Arc::new(ready)))
    })
}

#[cfg(not(unix))]
fn jet_jit_net_unix_ready(_stream: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return net_invalid("unix ready", "NetReadyInterest");
    };
    let stream = JetUnixStream;
    let deadline = jet_std::Duration { ns: deadline };
    net_err(jet_net_unix_ready(&stream, interest, &deadline).unwrap_err())
}

#[cfg(unix)]
fn jet_jit_net_unix_set_timeout(stream: i64, timeout: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_invalid("unix timeout", "UnixStream");
    };
    let timeout = jet_std::Duration { ns: timeout };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_set_timeout(&mut guard, &timeout))
}

#[cfg(not(unix))]
fn jet_jit_net_unix_set_timeout(_stream: i64, timeout: i64) -> i64 {
    let mut stream = JetUnixStream;
    let timeout = jet_std::Duration { ns: timeout };
    net_err(jet_net_unix_set_timeout(&mut stream, &timeout).unwrap_err())
}

// ── TcpListener / TcpStream handle methods ─────────────────────────────────

fn jet_jit_tcp_listener_accept(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("tcp accept", "TcpListener");
    };
    match jet_net_tcp_accept(&listener) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::TcpStream(Arc::new(Mutex::new(
            s,
        ))))),
        Err(e) => net_err(e),
    }
}

fn jet_jit_tcp_listener_local_addr(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return net_invalid("tcp local address", "TcpListener");
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(alloc_string(jet_net_socket_to_string(&a))),
        Err(e) => net_err(e),
    }
}

fn jet_jit_tcp_stream_read_text(stream: i64, limit: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp read", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    match jet_net_tcp_read_text(&mut guard, limit) {
        Ok(s) => result_ok_handle(alloc_string(s)),
        Err(e) => net_err(e),
    }
}

fn jet_jit_tcp_stream_write_all_bytes(stream: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("write_all", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_write_all_bytes(&mut guard, &bytes))
}
fn jet_jit_tcp_stream_shutdown(stream: i64, how: i64) -> i64 {
    let Some(how) = (match how {
        0 => Some(JetNetShutdown::Read),
        1 => Some(JetNetShutdown::Write),
        2 => Some(JetNetShutdown::Both),
        _ => None,
    }) else {
        return net_invalid("tcp shutdown", "NetShutdown");
    };
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp shutdown", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_shutdown(&mut guard, how))
}

fn jet_jit_tcp_stream_close(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_invalid("tcp close", "TcpStream");
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_close(&mut guard))
}

fn jet_jit_tcp_stream_ready(stream: i64, interest: i64, deadline: i64) -> i64 {
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

fn tls_version_bits(version: JetTLSVersion) -> i64 {
    match version {
        JetTLSVersion::Tls12 => 0,
        JetTLSVersion::Tls13 => 1,
    }
}

fn tls_trust_bits(value: i64) -> Option<JetTLSTrust> {
    let variant = value & 0xff;
    let payload = value >> 8;
    match variant {
        0 => Some(JetTLSTrust::System),
        1 => tls_root_certificates(payload).map(JetTLSTrust::SystemPlus),
        2 => tls_root_certificates(payload).map(JetTLSTrust::CustomOnly),
        _ => None,
    }
}

fn tls_version_from_bits(value: i64) -> Option<JetTLSVersion> {
    match value & 0xff {
        0 => Some(JetTLSVersion::Tls12),
        1 => Some(JetTLSVersion::Tls13),
        _ => None,
    }
}

fn tls_certificate_handle(certificate: JetTLSCertificate) -> i64 {
    let der = alloc_bytes(&certificate.der);
    let sha256 = alloc_bytes(&certificate.sha256);
    let spki_sha256 = alloc_bytes(&certificate.spki_sha256);
    let dns_names = list_of_strings(certificate.dns_names);
    let subject = alloc_string(certificate.subject);
    let issuer = alloc_string(certificate.issuer);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(8);
        let _ = rt.heap.record_set_int(record, 0, der);
        let _ = rt.heap.record_set_int(record, 1, sha256);
        let _ = rt.heap.record_set_int(record, 2, spki_sha256);
        let _ = rt.heap.record_set_int(record, 3, dns_names);
        let _ = rt
            .heap
            .record_set_int(record, 4, certificate.valid_from_unix_ms);
        let _ = rt
            .heap
            .record_set_int(record, 5, certificate.valid_until_unix_ms);
        let _ = rt.heap.record_set_string(record, 6, subject);
        let _ = rt.heap.record_set_string(record, 7, issuer);
        record
    })
}

fn tls_peer_identity_handle(identity: JetTLSPeerIdentity) -> i64 {
    let leaf = tls_certificate_handle(identity.leaf);
    let chain = Concurrency::with_runtime_mut(|rt| rt.heap.alloc_empty_list());
    for certificate in identity.certificate_chain {
        let certificate = tls_certificate_handle(certificate);
        Concurrency::with_runtime_mut(|rt| {
            let _ = rt.heap.list_push_int(chain, certificate);
        });
    }
    let server_name = alloc_string(identity.verified_server_name);
    let cipher_suite = alloc_string(identity.cipher_suite);
    let tls_version = tls_version_bits(identity.tls_version);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(5);
        let _ = rt.heap.record_set_string(record, 0, server_name);
        let _ = rt.heap.record_set_int(record, 1, leaf);
        let _ = rt.heap.record_set_int(record, 2, chain);
        let _ = rt.heap.record_set_string(record, 3, cipher_suite);
        let _ = rt.heap.record_set_int(record, 4, tls_version);
        record
    })
}

fn tls_take_tcp_stream(stream: i64) -> Result<JetTCPStream, JetNetError> {
    let Some(NetHttpHandle::TcpStream(stream)) = take_handle(stream) else {
        return Err(net_invalid_error("tls client", "TcpStream"));
    };
    Arc::try_unwrap(stream)
        .map(|mutex| {
            mutex
                .into_inner()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        })
        .map_err(|_| net_invalid_error("tls client", "shared TcpStream"))
}

fn tls_client_default(
    stream: JetTCPStream,
    server_name: &String,
    deadline: Option<&jet_std::Duration>,
) -> Result<JetTLSStream, JetNetError> {
    let callbacks = (
        crate::Net::runtime::tls::jet_net_tls_begin_impl,
        crate::Net::runtime::tls::jet_net_tls_handshake_step_impl,
        crate::Net::runtime::tls::jet_net_tls_abort_impl,
        crate::Net::runtime::tls::jet_net_tls_wants_impl,
        crate::Net::runtime::tls::jet_net_tls_read_ready_impl,
        crate::Net::runtime::tls::jet_net_tls_read_step_impl,
        crate::Net::runtime::tls::jet_net_tls_write_step_impl,
        crate::Net::runtime::tls::jet_net_tls_close_step_impl,
        crate::Net::runtime::tls::jet_net_tls_close_write_step_impl,
        crate::Net::runtime::tls::jet_net_tls_peer_identity_impl,
    );
    match deadline {
        Some(deadline) => jet_net_tls_client_scheduler_deadline(
            stream,
            server_name,
            deadline,
            callbacks.0,
            callbacks.1,
            callbacks.2,
            callbacks.3,
            callbacks.4,
            callbacks.5,
            callbacks.6,
            callbacks.7,
            callbacks.8,
            callbacks.9,
        ),
        None => jet_net_tls_client_scheduler(
            stream,
            server_name,
            callbacks.0,
            callbacks.1,
            callbacks.2,
            callbacks.3,
            callbacks.4,
            callbacks.5,
            callbacks.6,
            callbacks.7,
            callbacks.8,
            callbacks.9,
        ),
    }
}

fn tls_client_configured(
    stream: JetTCPStream,
    server_name: &String,
    config: &JetTLSClientConfig,
    deadline: &jet_std::Duration,
) -> Result<JetTLSStream, JetNetError> {
    jet_net_tls_client_scheduler_config_deadline(
        stream,
        server_name,
        config,
        deadline,
        crate::Net::runtime::tls::jet_net_tls_begin_config_impl,
        crate::Net::runtime::tls::jet_net_tls_handshake_step_impl,
        crate::Net::runtime::tls::jet_net_tls_abort_impl,
        crate::Net::runtime::tls::jet_net_tls_wants_impl,
        crate::Net::runtime::tls::jet_net_tls_read_ready_impl,
        crate::Net::runtime::tls::jet_net_tls_read_step_impl,
        crate::Net::runtime::tls::jet_net_tls_write_step_impl,
        crate::Net::runtime::tls::jet_net_tls_close_step_impl,
        crate::Net::runtime::tls::jet_net_tls_close_write_step_impl,
        crate::Net::runtime::tls::jet_net_tls_peer_identity_impl,
    )
}

fn jet_jit_tls_client_config_default() -> i64 {
    push_handle(NetHttpHandle::TLSClientConfig(
        jet_tls_client_config_default(),
    ))
}

fn jet_jit_tls_root_certificates_from_pem(pem: i64) -> i64 {
    let pem = clone_bytes(pem);
    match jet_tls_root_certificates_from_pem(
        &pem,
        crate::Net::runtime::tls::jet_net_tls_validate_roots_impl,
    ) {
        Ok(roots) => result_ok_handle(push_handle(NetHttpHandle::TLSRootCertificates(roots))),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_client_identity_from_pem(cert_chain: i64, private_key: i64) -> i64 {
    let cert_chain = clone_bytes(cert_chain);
    let private_key = clone_bytes(private_key);
    match jet_tls_client_identity_from_pem(
        &cert_chain,
        &private_key,
        crate::Net::runtime::tls::jet_net_tls_validate_identity_impl,
    ) {
        Ok(identity) => result_ok_handle(push_handle(NetHttpHandle::TLSClientIdentity(identity))),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_client_config_with_alpn(config: i64, protocols: i64) -> i64 {
    let Some(config) = tls_client_config(config) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_alpn",
            "invalid TLSClientConfig handle".to_string(),
        ));
    };
    let protocols = clone_string_list(protocols);
    match jet_tls_client_config_with_alpn(config, &protocols) {
        Ok(config) => result_ok_handle(push_handle(NetHttpHandle::TLSClientConfig(config))),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_client_config_with_trust(config: i64, trust: i64) -> i64 {
    let Some(config) = tls_client_config(config) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_trust",
            "invalid TLSClientConfig handle".to_string(),
        ));
    };
    let Some(trust) = tls_trust_bits(trust) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_trust",
            "invalid TLSClientTrust value".to_string(),
        ));
    };
    match jet_tls_client_config_with_trust(config, trust) {
        Ok(config) => result_ok_handle(push_handle(NetHttpHandle::TLSClientConfig(config))),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_client_config_with_identity(config: i64, identity: i64) -> i64 {
    let Some(config) = tls_client_config(config) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_client_identity",
            "invalid TLSClientConfig handle".to_string(),
        ));
    };
    let Some(identity) = tls_client_identity(identity) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_client_identity",
            "invalid TLSClientIdentity handle".to_string(),
        ));
    };
    match jet_tls_client_config_with_client_identity(config, &identity) {
        Ok(config) => result_ok_handle(push_handle(NetHttpHandle::TLSClientConfig(config))),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_client_config_with_version_bounds(config: i64, min: i64, max: i64) -> i64 {
    let Some(config) = tls_client_config(config) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_version_bounds",
            "invalid TLSClientConfig handle".to_string(),
        ));
    };
    let (Some(min), Some(max)) = (tls_version_from_bits(min), tls_version_from_bits(max)) else {
        return io_err(jet_tls_config_error(
            "ClientConfig.with_version_bounds",
            "invalid TLSVersion value".to_string(),
        ));
    };
    match jet_tls_client_config_with_version_bounds(config, min, max) {
        Ok(config) => result_ok_handle(push_handle(NetHttpHandle::TLSClientConfig(config))),
        Err(error) => io_err(error),
    }
}

fn tls_client_stream_result(
    stream: i64,
    server_name: String,
    config: Option<i64>,
    deadline: Option<i64>,
) -> Result<JetTLSStream, JetNetError> {
    let config = match config {
        Some(handle) => match tls_client_config(handle) {
            Some(config) => Some(config),
            None => return Err(net_invalid_error("tls client", "TLSClientConfig")),
        },
        None => None,
    };
    let stream = match tls_take_tcp_stream(stream) {
        Ok(stream) => stream,
        Err(error) => return Err(error),
    };
    match (config.as_ref(), deadline) {
        (Some(config), Some(ns)) => {
            tls_client_configured(stream, &server_name, config, &jet_std::Duration { ns })
        }
        (None, Some(ns)) => {
            tls_client_default(stream, &server_name, Some(&jet_std::Duration { ns }))
        }
        (None, None) => tls_client_default(stream, &server_name, None),
        (Some(_), None) => Err(net_invalid_error(
            "tls client",
            "missing configuration deadline",
        )),
    }
}

fn tls_client_result(
    stream: i64,
    server_name: i64,
    config: Option<i64>,
    deadline: Option<i64>,
) -> i64 {
    // I9: only the resident tier hands the name over as a JIT heap handle, so
    // the handle→String read happens at this raw-host boundary. The direct
    // adapter passes the owned String straight through; interpreter legs run
    // with no active resident runtime and never perform this heap round-trip.
    let result = tls_client_stream_result(stream, clone_string(server_name), config, deadline);
    map_net_ok(result, |stream| {
        push_handle(NetHttpHandle::TLSStream(Arc::new(Mutex::new(stream))))
    })
}

fn jet_jit_tls_client(stream: i64, server_name: i64) -> i64 {
    tls_client_result(stream, server_name, None, None)
}

fn jet_jit_tls_client_deadline(stream: i64, server_name: i64, deadline: i64) -> i64 {
    tls_client_result(stream, server_name, None, Some(deadline))
}

fn jet_jit_tls_client_config_deadline(
    stream: i64,
    server_name: i64,
    config: i64,
    deadline: i64,
) -> i64 {
    tls_client_result(stream, server_name, Some(config), Some(deadline))
}

fn jet_jit_tls_read_bytes(stream: i64, limit: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.read",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tls_read_bytes(&mut stream, limit) {
        Ok(bytes) => result_ok_handle(alloc_bytes(&bytes)),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_read_bytes_deadline(stream: i64, limit: i64, deadline: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.read",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tls_read_bytes_deadline(&mut stream, limit, &jet_std::Duration { ns: deadline }) {
        Ok(bytes) => result_ok_handle(alloc_bytes(&bytes)),
        Err(error) => io_err(error),
    }
}
fn jet_jit_net_tls_read(stream: i64) -> i64 {
    jet_jit_tls_read_text(stream, 8192)
}

fn jet_jit_tls_read_text(stream: i64, _limit: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.read_text",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tls_read_text(&mut stream) {
        Ok(text) => result_ok_handle(alloc_string(text)),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_write_bytes(stream: i64, data: i64) -> i64 {
    let data = clone_bytes(data);
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.write",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match jet_net_tls_write_bytes(&mut stream, &data) {
        Ok(count) => result_ok(count as u64),
        Err(error) => io_err(error),
    }
}

fn jet_jit_tls_write_all_bytes(stream: i64, data: i64) -> i64 {
    let data = clone_bytes(data);
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.write_all",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_unit(jet_net_tls_write_all_bytes(&mut stream, &data))
}

fn jet_jit_tls_write_all_bytes_deadline(stream: i64, data: i64, deadline: i64) -> i64 {
    let data = clone_bytes(data);
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.write_all",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_unit(jet_net_tls_write_all_bytes_deadline(
        &mut stream,
        &data,
        &jet_std::Duration { ns: deadline },
    ))
}

fn jet_jit_tls_write_text(stream: i64, text: i64) -> i64 {
    let text = clone_string(text);
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.write_text",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_unit(jet_net_tls_write_text(&mut stream, &text))
}

fn jet_jit_tls_close(stream: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.close",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_unit(jet_net_tls_close(&mut stream))
}

fn jet_jit_tls_close_write(stream: i64, deadline: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.close_write",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let mut stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_unit(jet_net_tls_close_write(
        &mut stream,
        &jet_std::Duration { ns: deadline },
    ))
}

fn jet_jit_tls_ready(stream: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return io_err(jet_tls_config_error(
            "TLSStream.ready",
            "invalid NetReadyInterest value".to_string(),
        ));
    };
    let Some(stream) = tls_stream(stream) else {
        return io_err(jet_tls_config_error(
            "TLSStream.ready",
            "invalid TLSStream handle".to_string(),
        ));
    };
    let stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    map_io_ok(
        jet_net_tls_ready(&stream, interest, &jet_std::Duration { ns: deadline }),
        |ready| push_handle(NetHttpHandle::NetReady(Arc::new(ready))),
    )
}

fn jet_jit_tls_peer_identity(stream: i64) -> i64 {
    let Some(stream) = tls_stream(stream) else {
        return 0;
    };
    let stream = stream
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    tls_peer_identity_handle(jet_net_tls_peer_identity(&stream))
}

fn jet_jit_udp_socket_ready(socket: i64, interest: i64, deadline: i64) -> i64 {
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

fn jet_jit_udp_socket_close(socket: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_invalid("udp close", "UdpSocket");
    };
    map_net_unit(jet_net_udp_close(&socket))
}

fn jet_jit_net_ready_readable(ready: i64) -> i64 {
    i64::from(
        net_ready(ready)
            .map(|ready| jet_net_ready_readable(&ready))
            .unwrap_or(false),
    )
}

fn jet_jit_net_ready_writable(ready: i64) -> i64 {
    i64::from(
        net_ready(ready)
            .map(|ready| jet_net_ready_writable(&ready))
            .unwrap_or(false),
    )
}

// ── core.http.server ───────────────────────────────────────────────────────

fn jet_jit_http_mux_new() -> i64 {
    push_handle(NetHttpHandle::HTTPMux(Arc::new(jet_http_mux_new())))
}

fn jet_jit_http_mux_add(mux: i64, method: i64, pattern: i64, callable: i64) -> i64 {
    let method = clone_string(method);
    let pattern = clone_string(pattern);
    let handler = with_handle(callable, |handle| match handle {
        NetHttpHandle::HTTPHandler(handler) => Some(Arc::clone(handler)),
        _ => None,
    })
    .or_else(|| wrap_bound_http_handler(callable));
    let Some(handler) = handler else {
        Concurrency::with_runtime_mut(|runtime| runtime.set_trap("invalid resident HTTP handler"));
        return 0;
    };
    if let Some(mux) = http_mux(mux) {
        jet_http_mux_add_handler(&mux, &method, &pattern, handler);
    }
    0
}

fn jet_jit_http_mux_add_zero(mux: i64, method: i64, pattern: i64, callable: i64) -> i64 {
    let method = clone_string(method);
    let pattern = clone_string(pattern);
    let handler = wrap_http_zero_handler(callable);
    if let Some(mux) = http_mux(mux) {
        jet_http_mux_add_handler(&mux, &method, &pattern, handler);
    }
    0
}

fn jet_jit_http_response(status: i64, body: i64) -> i64 {
    let body = clone_string(body);
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_response(
        status, &body,
    )))
}

fn jet_jit_http_server_response_header(response: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(response) = take_handle(response) else {
        return 0;
    };
    let NetHttpHandle::HTTPResponse(response) = response else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_response_header(
        response, &name, &value,
    )))
}

fn jet_jit_http_req_body(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

fn jet_jit_http_req_method(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_method(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

fn jet_jit_http_req_path(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_path(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

fn jet_jit_http_req_param(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(
        with_handle(req, |h| match h {
            NetHttpHandle::HTTPRequest(r) => Some(jet_http_request_param(r, &name)),
            _ => None,
        })
        .and_then(|value| value),
    )
}

fn jet_jit_http_req_header(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(
        with_handle(req, |h| match h {
            NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_header(r, &name)),
            _ => None,
        })
        .and_then(|r| r.ok()),
    )
}

fn jet_jit_http_req_text(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(request) => Some(jet_http_request_text(request)),
        _ => None,
    }) {
        Some(Ok(text)) => result_ok_handle(alloc_string(text)),
        Some(Err(error)) => http_err(error),
        None => result_err("invalid HTTPRequest".into()),
    }
}

fn jet_jit_http_req_text_with_limit(req: i64, limit: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(request) => {
            Some(jet_http_request_text_with_limit(request, limit))
        }
        _ => None,
    }) {
        Some(Ok(text)) => result_ok_handle(alloc_string(text)),
        Some(Err(error)) => http_err(error),
        None => result_err("invalid HTTPRequest".into()),
    }
}

fn jet_jit_http_body_text(body: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HTTPBody(b) => Some(jet_http_body_text(b, limit)),
        _ => None,
    }) {
        Some(Ok(s)) => result_ok_handle(alloc_string(s)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPBody".into()),
    }
}

fn jet_jit_http_body_bytes(body: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HTTPBody(b) => Some(jet_http_body_bytes(b, limit)),
        _ => None,
    }) {
        Some(Ok(bytes)) => result_ok_handle(alloc_bytes(&bytes)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPBody".into()),
    }
}
fn http_option_bits(value: Option<i64>) -> u64 {
    value
        .map(|value| (value as u64).wrapping_add(1))
        .unwrap_or(0)
}

fn jet_jit_http_body_chunks(body: i64, max_chunk: i64) -> i64 {
    match with_handle(body, |handle| match handle {
        NetHttpHandle::HTTPBody(body) => Some(jet_http_body_chunks(body, max_chunk)),
        _ => None,
    }) {
        Some(chunks) => push_handle(NetHttpHandle::HTTPBodyChunks(chunks)),
        None => 0,
    }
}

fn jet_jit_http_body_chunks_next(chunks: i64) -> i64 {
    match with_handle_mut(chunks, |handle| match handle {
        NetHttpHandle::HTTPBodyChunks(chunks) => Some(chunks.next()),
        _ => None,
    }) {
        Some(Some(Ok(bytes))) => result_ok(http_option_bits(Some(alloc_bytes(&bytes)))),
        Some(Some(Err(error))) => http_err(error),
        Some(None) => result_ok(0),
        None => result_err("invalid HTTPBodyChunks".into()),
    }
}

fn jet_jit_http_body_json_text(body: i64, has_limit: i64, limit: i64) -> i64 {
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

fn jet_jit_http_resp_text(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(response) => Some(jet_http_response_text(response)),
        _ => None,
    }) {
        Some(Ok(text)) => result_ok_handle(alloc_string(text)),
        Some(Err(error)) => http_err(error),
        None => result_err("invalid HTTPResponse".into()),
    }
}

fn jet_jit_http_resp_text_with_limit(resp: i64, limit: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(response) => {
            Some(jet_http_response_text_with_limit(response, limit))
        }
        _ => None,
    }) {
        Some(Ok(text)) => result_ok_handle(alloc_string(text)),
        Some(Err(error)) => http_err(error),
        None => result_err("invalid HTTPResponse".into()),
    }
}

fn http_file_reader_read(handle: i64, max: usize) -> Result<Option<Vec<u8>>, JetHTTPError> {
    Concurrency::with_runtime_mut(|rt| {
        let index = handle.saturating_sub(1) as usize;
        let Some(crate::enc_stream::FileReaderSlot::Live(reader)) = rt.file_readers.get_mut(index)
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
        let Some(crate::enc_stream::FileWriterSlot::Live(writer)) = rt.file_writers.get_mut(index)
        else {
            return Some(Err(JetHTTPError::IO {
                operation: "copy body".to_string(),
            }));
        };
        let result =
            std::io::Write::write_all(&mut writer.inner, bytes).map_err(|_| JetHTTPError::IO {
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

fn jet_jit_http_body_copy_to(body: i64, writer: i64, limit: i64) -> i64 {
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
fn jet_jit_http_nominal_static(
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

fn jet_jit_http_nominal_show(handle: i64) -> i64 {
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
/// Render the packed `HTTPError` carrier through the Prelude's `Display`
/// implementation. The JIT stores only the enum ordinal and one payload word;
/// rebuilding that Rust value here is marshalling, not a second error renderer.
fn jet_jit_http_error_show(bits: i64) -> i64 {
    alloc_string(
        net_http_error_from_packed(bits)
            .map(|error| error.to_string())
            .unwrap_or_default(),
    )
}
/// Decode the packed `NetError` carrier produced by `marshal_net_error`.
/// This reverses only the resident ABI; the shared Prelude `JetDisplay`
/// implementation remains the source of the failure text (I9).
fn net_error_detail_from_packed(payload: i64) -> Option<JetNetErrorDetail> {
    Concurrency::with_runtime_mut(|rt| {
        let operation = rt.heap.record_clone_string(payload, 0)?;
        let address = rt
            .heap
            .record_get_int(payload, 1)
            .filter(|encoded| *encoded > 0)
            .and_then(|encoded| encoded.checked_sub(1))
            .and_then(|handle| rt.heap.clone_string(handle));
        let name = rt
            .heap
            .record_get_int(payload, 2)
            .filter(|encoded| *encoded > 0)
            .and_then(|encoded| encoded.checked_sub(1))
            .and_then(|handle| rt.heap.clone_string(handle));
        let message = rt.heap.record_clone_string(payload, 3)?;
        let os_code = rt
            .heap
            .record_get_int(payload, 4)
            .filter(|encoded| *encoded > 0)
            .and_then(|encoded| encoded.checked_sub(1));
        Some(jet_net_detail(&operation, address, name, message, os_code))
    })
}
fn net_http_error_from_packed(bits: i64) -> Option<JetHTTPError> {
    let ordinal = (bits & 0xff) as u8;
    let payload = bits >> 8;
    match ordinal {
        0 => Some(JetHTTPError::InvalidMethod),
        1 => Some(JetHTTPError::InvalidUrl),
        2 => Some(JetHTTPError::InvalidHeader),
        3 => Some(JetHTTPError::InvalidStatus),
        4 => Some(JetHTTPError::BodyConsumed),
        5 => Some(JetHTTPError::InvalidFraming),
        6 => Some(JetHTTPError::UnsupportedEncoding),
        7 => Some(JetHTTPError::Cancelled),
        8 => Some(JetHTTPError::BodyTooLarge { limit: payload }),
        9 => Some(JetHTTPError::Resolve {
            host: clone_string(payload),
        }),
        10 => Some(JetHTTPError::Connect {
            address: clone_string(payload),
        }),
        11 => Some(JetHTTPError::TLS {
            stage: clone_string(payload),
        }),
        12 => Some(JetHTTPError::Timeout {
            phase: clone_string(payload),
        }),
        13 => Some(JetHTTPError::Proxy {
            stage: clone_string(payload),
        }),
        14 => Some(JetHTTPError::Redirect {
            reason: clone_string(payload),
        }),
        15 => Some(JetHTTPError::Protocol {
            version: clone_string(payload),
        }),
        16 => Some(JetHTTPError::IO {
            operation: clone_string(payload),
        }),
        17 => Some(JetHTTPError::Policy {
            reason: clone_string(payload),
        }),
        18 => Some(JetHTTPError::ResourceUnavailable {
            resource: clone_string(payload),
        }),
        19 => Some(JetHTTPError::Internal {
            incident_id: clone_string(payload),
        }),
        20 => match payload {
            0 => Some(JetHTTPError::UnsupportedTarget {
                operation: JetHTTPOperation::ClientConnect,
            }),
            1 => Some(JetHTTPError::UnsupportedTarget {
                operation: JetHTTPOperation::ServerBind,
            }),
            2 => Some(JetHTTPError::UnsupportedTarget {
                operation: JetHTTPOperation::ServeListener,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn jet_jit_net_error_operation(bits: i64) -> i64 {
    net_error_from_packed(bits)
        .map(|error| alloc_string(jet_net_error_operation(&error)))
        .unwrap_or_else(|| alloc_string(String::new()))
}

fn jet_jit_net_error_address(bits: i64) -> i64 {
    option_string(net_error_from_packed(bits).and_then(|error| jet_net_error_address(&error)))
}

fn jet_jit_net_error_name(bits: i64) -> i64 {
    option_string(net_error_from_packed(bits).and_then(|error| jet_net_error_name(&error)))
}

fn jet_jit_net_error_message(bits: i64) -> i64 {
    net_error_from_packed(bits)
        .map(|error| alloc_string(jet_net_error_message(&error)))
        .unwrap_or_else(|| alloc_string(String::new()))
}

fn jet_jit_net_error_os_code(bits: i64) -> i64 {
    option_int(net_error_from_packed(bits).and_then(|error| jet_net_error_os_code(&error)))
}

fn net_error_from_packed(bits: i64) -> Option<JetNetError> {
    let ordinal = (bits & 0xff) as u8;
    let payload = bits >> 8;
    match ordinal {
        0..=13 => {
            let detail = net_error_detail_from_packed(payload)?;
            Some(match ordinal {
                0 => JetNetError::InvalidInput(detail),
                1 => JetNetError::PermissionDenied(detail),
                2 => JetNetError::AddressInUse(detail),
                3 => JetNetError::AddressUnavailable(detail),
                4 => JetNetError::ConnectionRefused(detail),
                5 => JetNetError::ConnectionReset(detail),
                6 => JetNetError::NotConnected(detail),
                7 => JetNetError::Closed(detail),
                8 => JetNetError::Timeout(detail),
                9 => JetNetError::Cancelled(detail),
                10 => JetNetError::Unsupported(detail),
                11 => JetNetError::TLS(detail),
                12 => JetNetError::Protocol(detail),
                13 => JetNetError::Other(detail),
                _ => unreachable!(),
            })
        }
        14 => {
            let value = clone_string(payload >> 8);
            match (payload & 0xff) as u8 {
                0 => Some(JetNetError::DNS(JetNetDnsError::NotFound(value))),
                1 => Some(JetNetError::DNS(JetNetDnsError::Failure(value))),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Render the packed `NetError` carrier through the Prelude's `JetDisplay`
/// implementation. Rebuilding the enum from its shared surface ordinals is
/// marshalling only; this host does not duplicate network failure wording.
fn jet_jit_net_error_show(bits: i64) -> i64 {
    alloc_string(
        net_error_from_packed(bits)
            .map(|error| <JetNetError as JetDisplay>::jet_display(&error))
            .unwrap_or_default(),
    )
}

/// D-HTTP-JSON1=A: `server.json(status, body)` — body is already JSON text.
fn jet_jit_http_json_response(status: i64, body: i64) -> i64 {
    let body = clone_string(body);
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_json_text(
        status, &body,
    )))
}

/// D-HTTP-STATIC-FILES1=A: mount a directory under a prefix.
fn jet_jit_http_static_files(
    mux: i64,
    prefix: i64,
    root: i64,
    index: i64,
    dotfiles: i64,
    follow_links: i64,
) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return 0;
    };
    let prefix = clone_string(prefix);
    let root = clone_string(root);
    jet_http_srv_static_files_mount_defaulted(
        &mux,
        &prefix,
        &root,
        (index >= 0).then_some(index != 0),
        (dotfiles >= 0).then_some(dotfiles != 0),
        (follow_links >= 0).then_some(follow_links != 0),
    );
    0
}

/// D-HTTP-CORS1=A: build a CORS policy from a named-origin list or `.Any`.
/// `origins_mode`: 0 = `.Any`, 1 = string-list handle.
fn jet_jit_http_cors_policy(
    origins_mode: i64,
    origins: i64,
    methods: i64,
    headers: i64,
    credentials: i64,
    has_max_age: i64,
    max_age: i64,
) -> i64 {
    let origins = if origins_mode == 0 {
        JetHTTPCorsOrigins::Any
    } else {
        JetHTTPCorsOrigins::List(clone_string_list(origins))
    };
    let methods = (methods > 0).then(|| clone_string_list(methods));
    let headers = (headers > 0).then(|| clone_string_list(headers));
    match jet_http_cors_policy_defaulted(
        &origins,
        methods.as_ref(),
        headers.as_ref(),
        (credentials >= 0).then_some(credentials != 0),
        (has_max_age != 0).then_some(max_age),
    ) {
        Ok(policy) => result_ok_handle(push_handle(NetHttpHandle::HTTPCorsPolicy(policy))),
        Err(error) => http_err(error),
    }
}

/// Map JSON typed-decode `Result` errs to `HTTPError::InvalidFraming` (AOT parity).
fn jet_jit_http_project_json_decode_error(result: i64) -> i64 {
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
fn jet_jit_http_cors(mux: i64, policy: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return 0;
    };
    let Some(policy) = with_handle(policy, |h| match h {
        NetHttpHandle::HTTPCorsPolicy(policy) => Some(policy.clone()),
        _ => None,
    }) else {
        return 0;
    };
    jet_http_srv_install_cors(&mux, &policy);
    0
}

fn jet_jit_http_resp_status(resp: i64) -> i64 {
    with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_srv_response_status(r)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_http_resp_body(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_srv_response_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

fn jet_jit_http_client_resp_body(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_client_response_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HTTPBody(b)),
        None => 0,
    }
}

fn jet_jit_http_server_tls(cert: i64, key: i64) -> i64 {
    let tls = jet_http_srv_tls(&clone_string(cert), &clone_string(key));
    let cert = alloc_string(tls.cert_pem);
    let key = alloc_string(tls.key_pem);
    Concurrency::with_runtime_mut(|rt| {
        let record = rt.heap.alloc_record(2);
        let _ = rt.heap.record_set_string(record, 0, cert);
        let _ = rt.heap.record_set_string(record, 1, key);
        record
    })
}

fn decode_http_server_tls(raw: i64) -> Result<Option<JetHTTPServerTls>, String> {
    if raw == 0 {
        return Ok(None);
    }
    let record = raw
        .checked_sub(1)
        .ok_or_else(|| "invalid HTTPServerTls option".to_string())?;
    let fields = Concurrency::with_runtime_mut(|rt| {
        Some((
            rt.heap
                .record_get_string(record, 0)
                .and_then(|id| rt.heap.clone_string(id))?,
            rt.heap
                .record_get_string(record, 1)
                .and_then(|id| rt.heap.clone_string(id))?,
        ))
    })
    .ok_or_else(|| "invalid HTTPServerTls option".to_string())?;
    Ok(Some(jet_http_srv_tls(&fields.0, &fields.1)))
}

fn decode_http_server_deadline(raw: i64) -> Option<jet_std::Duration> {
    (raw != 0).then_some(jet_std::Duration {
        ns: raw.wrapping_sub(1),
    })
}

fn jet_jit_http_server_default(mux: i64, deadline_ns: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("invalid HTTPMux for default HTTP server");
        });
        return 0;
    };
    let deadline = jet_std::Duration { ns: deadline_ns };
    let server = jet_http_server_default(&mux, &deadline);
    push_handle(NetHttpHandle::HTTPServer(Arc::new(server)))
}

fn jet_jit_http_server_wait(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_wait(&server) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HTTPShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

fn jet_jit_http_server_bind(addr: i64, mux: i64, tls: i64, deadline: i64) -> i64 {
    let addr = clone_string(addr);
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    let tls = match decode_http_server_tls(tls) {
        Ok(tls) => tls,
        Err(error) => return result_err(error),
    };
    match jet_http_server_bind(
        &addr,
        (*mux).clone(),
        tls,
        decode_http_server_deadline(deadline),
    ) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::HTTPServer(Arc::new(s)))),
        Err(e) => result_err(e),
    }
}

fn jet_jit_http_server_local_addr(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_local_addr(&server) {
        Ok(a) => result_ok_handle(alloc_string(a)),
        Err(e) => result_err(e),
    }
}

fn jet_jit_http_server_serve(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HTTPServer".into());
    };
    match jet_http_server_serve(&server) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HTTPShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

fn jet_jit_http_server_shutdown(server: i64, grace_ms: i64) -> i64 {
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

fn jet_jit_http_shutdown_report_field(report: i64, field: i64) -> i64 {
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

jet_http_client_bridge!(native_http);

fn jet_jit_http_client_get(url: i64) -> i64 {
    let url = clone_string(url);
    match native_http_response(native_http::jet_http_client_get_impl(&url)) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => http_err(e),
    }
}

fn jet_jit_http_client_post(url: i64, body: i64) -> i64 {
    let url = clone_string(url);
    let body = clone_string(body);
    match native_http_response(native_http::jet_http_client_post_impl(&url, &body)) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => http_err(e),
    }
}

fn jet_jit_http_serve_once_listener(listener: i64, mux: i64) -> i64 {
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
fn jet_jit_http_serve_once(addr: i64, mux: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    let addr = clone_string(addr);
    match jet_http_mux_serve_once(&addr, (*mux).clone()) {
        Ok(()) => result_ok_unit(),
        Err(error) => result_err(error),
    }
}

fn jet_jit_http_mux_serve(addr: i64, mux: i64, tls: i64, deadline: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    let tls = match decode_http_server_tls(tls) {
        Ok(tls) => tls,
        Err(error) => return result_err(error),
    };
    let addr = clone_string(addr);
    match jet_http_mux_serve(
        &addr,
        (*mux).clone(),
        tls,
        decode_http_server_deadline(deadline),
    ) {
        Ok(()) => result_ok_unit(),
        Err(error) => result_err(error),
    }
}

fn jet_jit_http_serve(addr: i64, mux: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HTTPMux".into());
    };
    let addr = clone_string(addr);
    match jet_http_mux_serve(&addr, (*mux).clone(), None, None) {
        Ok(()) => result_ok_unit(),
        Err(error) => result_err(error),
    }
}

fn jet_jit_core_http_serve(addr: i64, callable: i64) -> i64 {
    let Some((epoch, slot)) = resident_http_callable(callable) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("MIR HTTP serve has an invalid callable handle");
        });
        return 0;
    };
    let addr = clone_string(addr);
    jet_http_serve(&addr, move |request| {
        Concurrency::try_with_http_jet_runtime_at(epoch, || {
            let request = push_handle(NetHttpHandle::HTTPRequest(request));
            Concurrency::notify_http_test_handler_entry();
            let response = unsafe {
                if slot.has_env {
                    let callback: HTTPHandlerWithEnvFn =
                        std::mem::transmute(slot.fn_ptr as usize);
                    callback(slot.env, request)
                } else {
                    let callback: HTTPHandlerFn = std::mem::transmute(slot.fn_ptr as usize);
                    callback(request)
                }
            };
            match take_handle(response) {
                Some(NetHttpHandle::HTTPResponse(response)) => response,
                Some(other) => {
                    let _ = push_handle(other);
                    Concurrency::with_runtime_mut(|rt| {
                        rt.set_host_fault("MIR HTTP serve callback did not return HTTPResponse");
                    });
                    jet_http_srv_internal_response()
                }
                None => {
                    Concurrency::with_runtime_mut(|rt| {
                        rt.set_host_fault("MIR HTTP serve callback returned an invalid handle");
                    });
                    jet_http_srv_internal_response()
                }
            }
        })
        .unwrap_or_else(jet_http_srv_internal_response)
    });
    // `jet_http_serve` only returns when the accept loop ends; the row's
    // carrier for a Unit result is the zero word.
    0
}

fn jet_jit_ws_upgrade(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_ws_upgrade(r)),
        _ => None,
    }) {
        Some(Ok(c)) => {
            result_ok_handle(push_handle(NetHttpHandle::WsConn(Arc::new(Mutex::new(c)))))
        }
        Some(Err(e)) => result_err(format!("{e:?}")),
        None => result_err("invalid HTTPRequest".into()),
    }
}

fn jet_jit_ws_connect(url: i64) -> i64 {
    let url = clone_string(url);
    match jet_ws_connect(&url) {
        Ok(c) => result_ok_handle(push_handle(NetHttpHandle::WsConn(Arc::new(Mutex::new(c))))),
        Err(e) => result_err(format!("{e:?}")),
    }
}

fn jet_jit_ws_send_text(conn: i64, text: i64) -> i64 {
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

fn jet_jit_ws_recv(conn: i64) -> i64 {
    let Some(conn) = ws_conn(conn) else {
        return result_err("invalid WsConn".into());
    };
    let guard = conn.lock().unwrap_or_else(|p| p.into_inner());
    match jet_ws_recv(&guard) {
        Ok(m) => result_ok_handle(push_handle(NetHttpHandle::WsMessage(m))),
        Err(e) => result_err(format!("{e:?}")),
    }
}

fn jet_jit_ws_close(conn: i64, code: i64, reason: i64) -> i64 {
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

fn jet_jit_ws_message_is_text(msg: i64) -> i64 {
    i64::from(
        with_handle(msg, |h| match h {
            NetHttpHandle::WsMessage(m) => Some(jet_ws_message_is_text(m)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

fn jet_jit_ws_message_text(msg: i64) -> i64 {
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
type HTTPMiddlewareWithEnvFn = unsafe extern "C" fn(i64, i64) -> i64;

fn list_of_handles(rows: Vec<NetHttpHandle>) -> i64 {
    let handles = rows.into_iter().map(push_handle).collect::<Vec<_>>();
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for handle in handles {
            let _ = rt.heap.list_push_int(list, handle);
        }
        list
    })
}

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
fn jet_jit_http_handler_bind(callable: i64, caps: i64) -> i64 {
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
    bind_http_closure(callable, env)
}

/// Single-capture bind: pack `cap0` into a fresh env list in the host.
fn jet_jit_http_handler_bind1(callable: i64, cap0: i64) -> i64 {
    let env = Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        let _ = rt.heap.list_push_int(list, cap0);
        list
    });
    bind_http_closure(callable, env)
}

fn bind_http_closure(callable: i64, env: i64) -> i64 {
    let Some((epoch, slot)) = resident_http_callable(callable).filter(|(_, slot)| slot.has_env)
    else {
        return push_handle(NetHttpHandle::HTTPHandler(invalid_http_handler()));
    };
    let f: HTTPClosureFn = unsafe { std::mem::transmute(slot.fn_ptr as usize) };
    let handler: JetHTTPHandler = Arc::new(move |req: JetHTTPRequest| {
        Concurrency::try_with_http_jet_runtime_at(epoch, || {
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
        .unwrap_or_else(|| {
            Err(JetHTTPError::IO {
                operation: "HTTP handler runtime unavailable".into(),
            })
        })
    });
    push_handle(NetHttpHandle::HTTPHandler(handler))
}

fn jet_jit_http_handler_handle(handler: i64, req: i64) -> i64 {
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

fn jet_jit_http_mux_middleware(mux: i64, mw_fn: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return 0;
    };
    let Some((epoch, slot)) = resident_http_callable(mw_fn) else {
        return 0;
    };
    jet_http_mux_middleware(
        &mux,
        Arc::new(move |next| {
            Concurrency::try_with_http_jet_runtime_at(epoch, || {
                let next_h = push_handle(NetHttpHandle::HTTPHandler(next));
                let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    if slot.has_env {
                        let f: HTTPMiddlewareWithEnvFn = std::mem::transmute(slot.fn_ptr as usize);
                        f(slot.env, next_h)
                    } else {
                        let f: HTTPMiddlewareFn = std::mem::transmute(slot.fn_ptr as usize);
                        f(next_h)
                    }
                }));
                let fail = |op: &'static str| -> JetHTTPHandler {
                    Arc::new(move |_| {
                        Err(JetHTTPError::IO {
                            operation: op.into(),
                        })
                    })
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
            .unwrap_or_else(|| {
                Arc::new(|_| {
                    Err(JetHTTPError::IO {
                        operation: "HTTP middleware runtime unavailable".into(),
                    })
                }) as JetHTTPHandler
            })
        }) as JetHTTPMiddleware,
    );
    0
}

fn jet_jit_http_request_id(mux: i64) -> i64 {
    if let Some(mux) = http_mux(mux) {
        jet_http_srv_install_request_id(&mux);
    }
    0
}

fn jet_jit_http_req_trailers(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_trailers(r)),
        _ => None,
    }) {
        Some(Ok(h)) => result_ok_handle(push_handle(NetHttpHandle::HTTPHeaders(h))),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HTTPRequest".into()),
    }
}

fn jet_jit_http_resp_trailers(resp: i64, trailers: i64) -> i64 {
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

fn jet_jit_http_req_body_len(req: i64) -> i64 {
    with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_body_len(r)),
        _ => None,
    })
    .unwrap_or(0)
}

fn jet_jit_http_req_under_limit(req: i64, max: i64) -> i64 {
    i64::from(
        with_handle(req, |h| match h {
            NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_req_under_limit(r, max)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

fn jet_jit_http_sse(data: i64) -> i64 {
    let data = clone_string(data);
    push_handle(NetHttpHandle::HTTPResponse(jet_http_srv_sse(&data)))
}

fn jet_jit_http_static_file_range(req: i64, path: i64, mime: i64) -> i64 {
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

fn jet_jit_http_static_file(path: i64, mime: i64) -> i64 {
    let path = clone_string(path);
    let mime = clone_string(mime);
    match jet_http_srv_static_file(&path, &mime) {
        Ok(response) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(response))),
        Err(error) => result_err(error),
    }
}

fn jet_jit_http_client_request_new(method: i64, url: i64) -> i64 {
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

fn jet_jit_http_client_request_body(req: i64, body: i64) -> i64 {
    let body = clone_string(body);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_body(
        req, &body,
    )))
}

fn jet_jit_http_client_request_json(req: i64, body: i64) -> i64 {
    let body = clone_string(body);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_json_text(req, &body),
    ))
}

fn jet_jit_http_client_request_form(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_form(
        req, &name, &value,
    )))
}

fn jet_jit_http_client_request_cookie(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_cookie(
        req, &name, &value,
    )))
}

fn jet_jit_http_client_request_header(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(jet_http_client_request_header(
        req, &name, &value,
    )))
}

fn jet_jit_http_client_request_redirects(req: i64, limit: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_redirects(req, limit),
    ))
}

fn jet_jit_http_client_request_connect_timeout(req: i64, ms: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_connect_timeout(req, ms),
    ))
}

fn jet_jit_http_client_request_read_timeout(req: i64, ms: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return 0;
    };
    push_handle(NetHttpHandle::HTTPRequest(
        jet_http_client_request_read_timeout(req, ms),
    ))
}

fn jet_jit_http_client_request_send(req: i64) -> i64 {
    let Some(req) = take_http_request(req) else {
        return result_err("invalid HTTPRequest".into());
    };
    match native_http_request(req) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HTTPResponse(resp))),
        Err(e) => http_err(e),
    }
}

fn jet_jit_http_resp_header(resp: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(
        with_handle(resp, |h| match h {
            NetHttpHandle::HTTPResponse(r) => Some(jet_http_client_response_header(r, &name)),
            _ => None,
        })
        .and_then(|r| r.ok()),
    )
}

fn jet_jit_http_resp_cookies(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HTTPResponse(r) => Some(jet_http_response_cookies(r)),
        _ => None,
    }) {
        Some(rows) => list_of_strings(rows),
        None => list_of_strings(Vec::new()),
    }
}
fn jet_jit_http_server_access_log(req: i64, status: i64) -> i64 {
    let text = with_handle(req, |h| match h {
        NetHttpHandle::HTTPRequest(r) => Some(jet_http_srv_access_log(r, status)),
        _ => None,
    })
    .unwrap_or_default();
    alloc_string(text)
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
    tcp_connect_timeout: "jet_jit_net_tcp_connect_timeout" => jet_jit_net_tcp_connect_timeout: sig2;
    tcp_stream_local_addr: "jet_jit_net_tcp_stream_local_addr" => jet_jit_net_tcp_stream_local_addr: sig1;
    tcp_stream_peer_addr: "jet_jit_net_tcp_stream_peer_addr" => jet_jit_net_tcp_stream_peer_addr: sig1;
    tcp_stream_local_socket_addr: "jet_jit_net_tcp_stream_local_socket_addr" => jet_jit_net_tcp_stream_local_socket_addr: sig1;
    tcp_stream_peer_socket_addr: "jet_jit_net_tcp_stream_peer_socket_addr" => jet_jit_net_tcp_stream_peer_socket_addr: sig1;
    set_read_timeout: "jet_jit_net_set_read_timeout" => jet_jit_net_set_read_timeout: sig2;
    set_write_timeout: "jet_jit_net_set_write_timeout" => jet_jit_net_set_write_timeout: sig2;
    dns_aaaa: "jet_jit_net_dns_aaaa" => jet_jit_net_dns_aaaa: sig2;
    dns_aaaa_at: "jet_jit_net_dns_aaaa_at" => jet_jit_net_dns_aaaa_at: sig3;
    dns_srv: "jet_jit_net_dns_srv" => jet_jit_net_dns_srv: sig2;
    dns_srv_at: "jet_jit_net_dns_srv_at" => jet_jit_net_dns_srv_at: sig3;
    dns_srv_target: "jet_jit_net_dns_srv_target" => jet_jit_net_dns_srv_target: sig1;
    dns_srv_port: "jet_jit_net_dns_srv_port" => jet_jit_net_dns_srv_port: sig1;
    dns_srv_priority: "jet_jit_net_dns_srv_priority" => jet_jit_net_dns_srv_priority: sig1;
    dns_srv_weight: "jet_jit_net_dns_srv_weight" => jet_jit_net_dns_srv_weight: sig1;
    listener_local_socket_addr: "jet_jit_net_listener_local_socket_addr2" => jet_jit_net_listener_local_socket_addr: sig1;
    set_timeout: "jet_jit_net_set_timeout" => jet_jit_net_set_timeout: sig2;
    nodelay: "jet_jit_net_nodelay" => jet_jit_net_nodelay: sig1;
    set_nodelay: "jet_jit_net_set_nodelay" => jet_jit_net_set_nodelay: sig2;
    ttl: "jet_jit_net_ttl" => jet_jit_net_ttl: sig1;
    set_ttl: "jet_jit_net_set_ttl" => jet_jit_net_set_ttl: sig2;
    socket_type: "jet_jit_net_socket_type" => jet_jit_net_socket_type: sig1;
    sendfile: "jet_jit_net_sendfile" => jet_jit_net_sendfile: sig2;
    dns_ptr: "jet_jit_net_dns_ptr" => jet_jit_net_dns_ptr: sig2;
    dns_txt: "jet_jit_net_dns_txt" => jet_jit_net_dns_txt: sig2;
    dns_txt_at: "jet_jit_net_dns_txt_at" => jet_jit_net_dns_txt_at: sig3;
    getservbyname: "jet_jit_net_getservbyname" => jet_jit_net_getservbyname: sig1;
    net_error_show: "jet_jit_net_error_show" => jet_jit_net_error_show: sig1;
    getservbyport: "jet_jit_net_getservbyport" => jet_jit_net_getservbyport: sig1;
    tcp_reply: "jet_jit_net_tcp_reply" => jet_jit_net_tcp_reply: sig3;
    udp_bind: "jet_jit_net_udp_bind" => jet_jit_net_udp_bind: sig1;
    udp_local_addr: "jet_jit_net_udp_local_addr" => jet_jit_net_udp_local_addr: sig1;
    udp_set_timeout: "jet_jit_net_udp_set_timeout" => jet_jit_net_udp_set_timeout: sig2;
    udp_send_bytes_to: "jet_jit_net_udp_send_bytes_to" => jet_jit_net_udp_send_bytes_to: sig3;
    udp_send_to: "jet_jit_net_udp_send_to" => jet_jit_net_udp_send_to: sig3;
    udp_send_bytes_to_deadline: "jet_jit_net_udp_send_bytes_to_deadline" => jet_jit_net_udp_send_bytes_to_deadline: sig4;
    udp_receive: "jet_jit_net_udp_receive" => jet_jit_net_udp_receive: sig2;
    udp_receive_deadline: "jet_jit_net_udp_receive_deadline" => jet_jit_net_udp_receive_deadline: sig3;
    udp_packet_data: "jet_jit_net_udp_packet_data" => jet_jit_net_udp_packet_data: sig1;
    udp_packet_bytes: "jet_jit_net_udp_packet_bytes" => jet_jit_net_udp_packet_bytes: sig1;
    udp_packet_original_len: "jet_jit_net_udp_packet_original_len" => jet_jit_net_udp_packet_original_len: sig1;
    udp_packet_truncated: "jet_jit_net_udp_packet_truncated" => jet_jit_net_udp_packet_truncated: sig1;
    udp_packet_addr: "jet_jit_net_udp_packet_addr" => jet_jit_net_udp_packet_addr: sig1;
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
    tcp_shutdown: "jet_jit_tcp_stream_shutdown" => jet_jit_tcp_stream_shutdown: sig2;
    tcp_close: "jet_jit_tcp_stream_close" => jet_jit_tcp_stream_close: sig1;
    tcp_ready: "jet_jit_tcp_stream_ready" => jet_jit_tcp_stream_ready: sig3;
    tls_client_config_default: "jet_jit_tls_client_config_default" => jet_jit_tls_client_config_default: sig0;
    tls_root_certificates_from_pem: "jet_jit_tls_root_certificates_from_pem" => jet_jit_tls_root_certificates_from_pem: sig1;
    tls_client_identity_from_pem: "jet_jit_tls_client_identity_from_pem" => jet_jit_tls_client_identity_from_pem: sig2;
    tls_client_config_with_alpn: "jet_jit_tls_client_config_with_alpn" => jet_jit_tls_client_config_with_alpn: sig2;
    tls_client_config_with_trust: "jet_jit_tls_client_config_with_trust" => jet_jit_tls_client_config_with_trust: sig2;
    tls_client_config_with_identity: "jet_jit_tls_client_config_with_identity" => jet_jit_tls_client_config_with_identity: sig2;
    tls_client_config_with_version_bounds: "jet_jit_tls_client_config_with_version_bounds" => jet_jit_tls_client_config_with_version_bounds: sig3;
    tls_client: "jet_jit_tls_client" => jet_jit_tls_client: sig2;
    tls_client_deadline: "jet_jit_tls_client_deadline" => jet_jit_tls_client_deadline: sig3;
    tls_client_config_deadline: "jet_jit_tls_client_config_deadline" => jet_jit_tls_client_config_deadline: sig4;
    tls_read_bytes: "jet_jit_tls_read_bytes" => jet_jit_tls_read_bytes: sig2;
    tls_read_bytes_deadline: "jet_jit_tls_read_bytes_deadline" => jet_jit_tls_read_bytes_deadline: sig3;
    tls_read_text: "jet_jit_tls_read_text" => jet_jit_tls_read_text: sig2;
    tls_read: "jet_jit_net_tls_read" => jet_jit_net_tls_read: sig1;
    tls_write_bytes: "jet_jit_tls_write_bytes" => jet_jit_tls_write_bytes: sig2;
    tls_write_all_bytes: "jet_jit_tls_write_all_bytes" => jet_jit_tls_write_all_bytes: sig2;
    tls_write_all_bytes_deadline: "jet_jit_tls_write_all_bytes_deadline" => jet_jit_tls_write_all_bytes_deadline: sig3;
    tls_write_text: "jet_jit_tls_write_text" => jet_jit_tls_write_text: sig2;
    tls_close: "jet_jit_tls_close" => jet_jit_tls_close: sig1;
    tls_close_write: "jet_jit_tls_close_write" => jet_jit_tls_close_write: sig2;
    tls_ready: "jet_jit_tls_ready" => jet_jit_tls_ready: sig3;
    tls_peer_identity: "jet_jit_tls_peer_identity" => jet_jit_tls_peer_identity: sig1;
    udp_ready: "jet_jit_udp_socket_ready" => jet_jit_udp_socket_ready: sig3;
    udp_close: "jet_jit_udp_socket_close" => jet_jit_udp_socket_close: sig1;
    http_openapi: "jet_web_openapi" => jet_jit_http_openapi: sig1;
    ready_readable: "jet_jit_net_ready_readable" => jet_jit_net_ready_readable: sig1;
    ready_writable: "jet_jit_net_ready_writable" => jet_jit_net_ready_writable: sig1;
    http_mux_new: "jet_jit_http_mux_new" => jet_jit_http_mux_new: sig0;
    http_router_new_prelude: "jet_http_router_new" => jet_jit_http_router_new: sig0;
    http_mux_add: "jet_http_mux_add_handler" => jet_jit_http_mux_add: sig4;
    http_mux_add_zero: "jet_http_mux_add_zero_handler" => jet_jit_http_mux_add_zero: sig4;
    http_router_new: "jet_jit_http_router_new" => jet_jit_http_router_new: sig0;
    http_router_register: "jet_jit_http_router_register" => jet_jit_http_router_register: sig7;
    http_router_register_prelude: "jet_http_router_register" => jet_jit_http_router_register: sig7;
    http_response: "jet_jit_http_response" => jet_jit_http_response: sig2;
    http_server_response_header: "jet_jit_http_server_response_header" => jet_jit_http_server_response_header: sig3;
    http_server_access_log: "jet_jit_http_server_access_log" => jet_jit_http_server_access_log: sig2;
    http_req_body: "jet_jit_http_req_body" => jet_jit_http_req_body: sig1;
    http_req_method: "jet_jit_http_req_method" => jet_jit_http_req_method: sig1;
    http_req_path: "jet_jit_http_req_path" => jet_jit_http_req_path: sig1;
    http_req_param: "jet_jit_http_req_param" => jet_jit_http_req_param: sig2;
    http_req_header: "jet_jit_http_req_header" => jet_jit_http_req_header: sig2;
    http_req_text: "jet_jit_http_req_text" => jet_jit_http_req_text: sig1;
    http_req_text_with_limit: "jet_jit_http_req_text_with_limit" => jet_jit_http_req_text_with_limit: sig2;
    http_req_text_prelude: "jet_http_request_text" => jet_jit_http_req_text: sig1;
    http_req_text_with_limit_prelude: "jet_http_request_text_with_limit" => jet_jit_http_req_text_with_limit: sig2;
    http_body_text: "jet_jit_http_body_text" => jet_jit_http_body_text: sig2;
    http_body_text_prelude: "jet_http_body_text" => jet_jit_http_body_text: sig2;
    http_body_bytes: "jet_jit_http_body_bytes" => jet_jit_http_body_bytes: sig2;
    http_body_chunks: "jet_jit_http_body_chunks" => jet_jit_http_body_chunks: sig2;
    http_body_chunks_next: "jet_jit_http_body_chunks_next" => jet_jit_http_body_chunks_next: sig1;
    http_body_json_text: "jet_jit_http_body_json_text" => jet_jit_http_body_json_text: sig3;
    http_body_copy_to: "jet_jit_http_body_copy_to" => jet_jit_http_body_copy_to: sig3;
    http_nominal_static: "jet_jit_http_nominal_static" => jet_jit_http_nominal_static: sig7;
    http_nominal_show: "jet_jit_http_nominal_show" => jet_jit_http_nominal_show: sig1;
    http_error_show: "jet_jit_http_error_show" => jet_jit_http_error_show: sig1;
    http_json_response: "jet_jit_http_json_response" => jet_jit_http_json_response: sig2;
    http_static_files: "jet_jit_http_static_files" => jet_jit_http_static_files: sig6;
    http_cors_policy: "jet_jit_http_cors_policy" => jet_jit_http_cors_policy: sig7;
    http_cors: "jet_jit_http_cors" => jet_jit_http_cors: sig2;
    http_project_json_decode_error: "jet_jit_http_project_json_decode_error" => jet_jit_http_project_json_decode_error: sig1;
    http_project_json_decode_error_prelude: "jet_http_project_json_decode_error" => jet_jit_http_project_json_decode_error: sig1;
    http_resp_status: "jet_jit_http_resp_status" => jet_jit_http_resp_status: sig1;
    http_resp_body: "jet_jit_http_resp_body" => jet_jit_http_resp_body: sig1;
    core_http_serve: "jet_http_serve" => jet_jit_core_http_serve: sig2;
    http_client_resp_body: "jet_jit_http_client_resp_body" => jet_jit_http_client_resp_body: sig1;
    http_mux_serve: "jet_http_mux_serve" => jet_jit_http_mux_serve: sig4;
    http_serve: "jet_jit_http_serve" => jet_jit_http_serve: sig2;
    http_resp_text: "jet_jit_http_resp_text" => jet_jit_http_resp_text: sig1;
    http_serve_once: "jet_jit_http_serve_once" => jet_jit_http_serve_once: sig2;
    http_resp_text_with_limit: "jet_jit_http_resp_text_with_limit" => jet_jit_http_resp_text_with_limit: sig2;
    http_resp_text_prelude: "jet_http_response_text" => jet_jit_http_resp_text: sig1;
    http_resp_text_with_limit_prelude: "jet_http_response_text_with_limit" => jet_jit_http_resp_text_with_limit: sig2;
    http_server_tls: "jet_jit_http_server_tls" => jet_jit_http_server_tls: sig2;
    http_server_default: "jet_http_server_default" => jet_jit_http_server_default: sig2;
    http_server_bind: "jet_http_server_bind" => jet_jit_http_server_bind: sig4;
    http_server_local_addr: "jet_jit_http_server_local_addr" => jet_jit_http_server_local_addr: sig1;
    http_server_local_addr_prelude: "jet_http_server_local_addr" => jet_jit_http_server_local_addr: sig1;
    http_server_serve: "jet_jit_http_server_serve" => jet_jit_http_server_serve: sig1;
    http_server_serve_prelude: "jet_http_server_serve" => jet_jit_http_server_serve: sig1;
    http_server_wait: "jet_http_server_wait" => jet_jit_http_server_wait: sig1;
    http_server_shutdown: "jet_jit_http_server_shutdown" => jet_jit_http_server_shutdown: sig2;
    http_server_shutdown_prelude: "jet_http_server_shutdown" => jet_jit_http_server_shutdown: sig2;
    http_shutdown_report_field: "jet_jit_http_shutdown_report_field" => jet_jit_http_shutdown_report_field: sig2;
    http_serve_once_listener: "jet_jit_http_serve_once_listener" => jet_jit_http_serve_once_listener: sig2;
    http_mux_serve_once_listener_prelude: "jet_http_mux_serve_once_listener" => jet_jit_http_serve_once_listener: sig2;
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
    http_static_file: "jet_jit_http_static_file" => jet_jit_http_static_file: sig2;
    http_static_file_range: "jet_jit_http_static_file_range" => jet_jit_http_static_file_range: sig3;
    http_client_request_new: "jet_jit_http_client_request_new" => jet_jit_http_client_request_new: sig2;
    http_client_request_body: "jet_jit_http_client_request_body" => jet_jit_http_client_request_body: sig2;
    http_client_request_json: "jet_jit_http_client_request_json" => jet_jit_http_client_request_json: sig2;
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

// ── Test-only direct Prelude adapters ──────────────────────────────────────
// The resident lifetime proof uses these adapters so it exercises the same
// shared mux and server kernels as the live JIT host functions.
pub(crate) fn test_http_mux() -> i64 {
    push_handle(NetHttpHandle::HTTPMux(Arc::new(jet_http_mux_new())))
}

pub(crate) fn test_http_mux_add_handler(
    mux: i64,
    method: &str,
    pattern: &str,
    handler: &TestHttpHandler,
) -> Result<(), String> {
    let mux = http_mux(mux).ok_or_else(|| "invalid HTTPMux".to_string())?;
    jet_http_mux_add_handler(&mux, method, pattern, handler.0.clone());
    Ok(())
}

pub(crate) fn test_http_server_bind(addr: String, mux: i64) -> Result<i64, String> {
    let mux = http_mux(mux).ok_or_else(|| "invalid HTTPMux".to_string())?;
    let server = jet_http_server_bind(&addr, (*mux).clone(), None, None)?;
    Ok(push_handle(NetHttpHandle::HTTPServer(Arc::new(server))))
}

pub(crate) fn test_http_server_local_addr(server: i64) -> Result<String, String> {
    let server = http_server(server).ok_or_else(|| "invalid HTTPServer".to_string())?;
    jet_http_server_local_addr(&server)
}

pub(crate) fn test_http_server_serve(server: i64) -> Result<(), String> {
    let server = http_server(server).ok_or_else(|| "invalid HTTPServer".to_string())?;
    jet_http_server_serve(&server).map(|_| ())
}

#[cfg(test)]
mod http_adapter_tests {
    use super::*;

    #[test]
    fn cors_defaulting_preserves_explicit_max_age() {
        let origins = JetHTTPCorsOrigins::List(vec!["https://app.example".to_string()]);
        let defaulted = jet_http_cors_policy_defaulted(&origins, None, None, None, None).unwrap();
        let explicit =
            jet_http_cors_policy_defaulted(&origins, None, None, None, Some(i64::MIN)).unwrap();
        assert_eq!(defaulted.max_age_secs, 86_400);
        assert_eq!(explicit.max_age_secs, i64::MIN);
    }
}
struct Http2DrainGate {
    cancelled: std::sync::mpsc::Sender<()>,
    release: Option<std::sync::mpsc::Receiver<()>>,
}

impl Drop for Http2DrainGate {
    fn drop(&mut self) {
        let _ = self.cancelled.send(());
        if let Some(release) = self.release.take() {
            let _ = release.recv();
        }
    }
}

/// Exercise the exact HTTP/2 dispatch ownership used by `jet_http2_serve`.
/// The queued scheduler task announces cancellation from its unwind cleanup,
/// then holds that cleanup until the caller releases it. Therefore a cleanup
/// completion observed before release is impossible while `drain()` still
/// owns every queued task.
pub(crate) fn test_http2_dispatch_drain() -> Result<(), String> {
    let (cancelled_tx, cancelled_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let control = JetTaskControl::new();
    let task_control = control.clone();
    let task = jet_scheduler_spawn_blocking_with_control(
        move || {
            let _gate = Http2DrainGate {
                cancelled: cancelled_tx,
                release: Some(release_rx),
            };
            let park = jet_codegen::scheduler::ParkSlot::new();
            loop {
                jet_codegen::scheduler::jet_scheduler_yield(
                    "HTTP/2 dispatch lifetime proof",
                    &park,
                    None,
                );
            }
        },
        task_control,
    );
    let mut dispatch_tasks = JetHTTP2DispatchTasks::default();
    dispatch_tasks.push(task, control);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let cleanup = std::thread::spawn(move || {
        let _ = started_tx.send(());
        Concurrency::with_http_runtime_quiesced(|| {
            dispatch_tasks.drain();
            clear_net_http_handles();
        });
        let _ = done_tx.send(());
    });
    started_rx
        .recv()
        .map_err(|_| "HTTP/2 cleanup did not start".to_string())?;
    cancelled_rx
        .recv()
        .map_err(|_| "HTTP/2 dispatch was not cancelled by drain".to_string())?;
    if done_rx.try_recv().is_ok() {
        let _ = release_tx.send(());
        let _ = cleanup.join();
        return Err("HTTP/2 cleanup completed before queued dispatch release".to_string());
    }
    release_tx
        .send(())
        .map_err(|_| "HTTP/2 dispatch release failed".to_string())?;
    done_rx
        .recv()
        .map_err(|_| "HTTP/2 cleanup did not complete after dispatch release".to_string())?;
    cleanup
        .join()
        .map_err(|_| "HTTP/2 cleanup thread panicked".to_string())?;
    Ok(())
}
