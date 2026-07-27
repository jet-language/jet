// Host shims for TCP/UDP/Unix + HTTP mux/server + WS — same module as net_http_rt includes.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::sync::{Mutex, MutexGuard};

use crate::Concurrency;
use crate::JitResultValue;

enum NetHttpHandle {
    TcpListener(Arc<JetTcpListener>),
    TcpStream(Arc<Mutex<JetTcpStream>>),
    SocketAddr(JetSocketAddr),
    UdpSocket(Arc<JetUdpSocket>),
    UdpPacket(JetUdpPacket),
    #[cfg(unix)]
    UnixListener(Arc<JetUnixListener>),
    #[cfg(unix)]
    UnixStream(Arc<Mutex<JetUnixStream>>),
    HttpMux(Arc<JetHttpMux>),
    HttpRequest(JetHttpRequest),
    HttpResponse(JetHttpResponse),
    HttpBody(JetHttpBody),
    HttpHeaders(JetHttpHeaders),
    HttpHandler(JetHttpHandler),
    HttpServer(Arc<JetHttpServer>),
    HttpShutdownReport(JetHttpShutdownReport),
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

fn tcp_listener(handle: i64) -> Option<Arc<JetTcpListener>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TcpListener(l) => Some(Arc::clone(l)),
        _ => None,
    })
}

fn tcp_stream(handle: i64) -> Option<Arc<Mutex<JetTcpStream>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TcpStream(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn udp_socket(handle: i64) -> Option<Arc<JetUdpSocket>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::UdpSocket(s) => Some(Arc::clone(s)),
        _ => None,
    })
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

fn http_mux(handle: i64) -> Option<Arc<JetHttpMux>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HttpMux(m) => Some(Arc::clone(m)),
        _ => None,
    })
}

fn http_server(handle: i64) -> Option<Arc<JetHttpServer>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HttpServer(s) => Some(Arc::clone(s)),
        _ => None,
    })
}

fn ws_conn(handle: i64) -> Option<Arc<Mutex<JetWsConn>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::WsConn(c) => Some(Arc::clone(c)),
        _ => None,
    })
}

fn clone_string(handle: i64) -> String {
    Concurrency::with_runtime_mut(|rt| rt.heap.clone_string(handle).unwrap_or_default())
}

fn alloc_string(s: String) -> i64 {
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_string(s))
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

fn alloc_bytes(bytes: &[u8]) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let list = rt.heap.alloc_empty_list();
        for b in bytes {
            let _ = rt.heap.list_push_int(list, i64::from(*b));
        }
        list
    })
}

fn result_ok(bits: u64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(JitResultValue { ok: true, bits });
        rt.results.len() as i64
    })
}

fn result_err(message: String) -> i64 {
    let handle = alloc_string(message);
    Concurrency::with_runtime_mut(|rt| {
        rt.results.push(JitResultValue {
            ok: false,
            bits: handle as u64,
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

fn net_err(e: JetNetError) -> i64 {
    result_err(format!("{e:?}"))
}

fn http_err(e: JetHttpError) -> i64 {
    result_err(format!("{e:?}"))
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

fn map_http_ok<T>(r: Result<T, JetHttpError>, f: impl FnOnce(T) -> i64) -> i64 {
    match r {
        Ok(v) => result_ok_handle(f(v)),
        Err(e) => http_err(e),
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

type HttpHandlerFn = unsafe extern "C" fn(i64) -> i64;

fn wrap_http_handler(fn_ptr: i64) -> JetHttpHandler {
    let f: HttpHandlerFn = unsafe { std::mem::transmute(fn_ptr as usize) };
    Arc::new(move |req: JetHttpRequest| -> Result<JetHttpResponse, JetHttpError> {
        Concurrency::with_http_jet_runtime(|| {
            let req_h = push_handle(NetHttpHandle::HttpRequest(req));
            let res_h = unsafe { f(req_h) };
            match decode_result(res_h) {
                Some((true, bits)) => {
                    let resp_h = bits as i64;
                    match take_handle(resp_h) {
                        Some(NetHttpHandle::HttpResponse(resp)) => Ok(resp),
                        other => {
                            if let Some(v) = other {
                                let _ = push_handle(v);
                            }
                            Err(JetHttpError::Io {
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
                    Err(JetHttpError::Io { operation: msg })
                }
                None => Err(JetHttpError::Io {
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
        return result_err("invalid SocketAddr".into());
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
        return result_err("invalid TcpListener".into());
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_set_timeout(stream: i64, ms: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "set_timeout",
            None,
            None,
            "invalid TcpStream".into(),
            None,
        )));
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_set_timeout(&mut guard, ms))
}

extern "C" fn jet_jit_net_tcp_reply(stream: i64, status: i64, body: i64) -> i64 {
    let status = clone_string(status);
    let body = clone_string(body);
    let Some(NetHttpHandle::TcpStream(s)) = take_handle(stream) else {
        return result_err("invalid TcpStream".into());
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
        return result_err("invalid UdpSocket".into());
    };
    match jet_net_udp_local_addr(&socket) {
        Ok(a) => result_ok_handle(push_handle(NetHttpHandle::SocketAddr(a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_set_timeout(socket: i64, ms: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "udp_set_timeout",
            None,
            None,
            "invalid UdpSocket".into(),
            None,
        )));
    };
    map_net_unit(jet_net_udp_set_timeout(&socket, ms))
}

extern "C" fn jet_jit_net_udp_send_bytes_to(socket: i64, data: i64, addr: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(addr) = with_handle(addr, |h| match h {
        NetHttpHandle::SocketAddr(a) => Some(a.clone()),
        _ => None,
    }) else {
        return result_err("invalid SocketAddr".into());
    };
    let Some(socket) = udp_socket(socket) else {
        return result_err("invalid UdpSocket".into());
    };
    match jet_net_udp_send_bytes_to(&socket, &bytes, &addr) {
        Ok(n) => result_ok(n as u64),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_receive(socket: i64, limit: i64) -> i64 {
    let Some(socket) = udp_socket(socket) else {
        return result_err("invalid UdpSocket".into());
    };
    match jet_net_udp_receive(&socket, limit) {
        Ok(p) => result_ok_handle(push_handle(NetHttpHandle::UdpPacket(p))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_net_udp_packet_bytes(packet: i64) -> i64 {
    match with_handle(packet, |h| match h {
        NetHttpHandle::UdpPacket(p) => Some(jet_net_udp_packet_bytes(p)),
        _ => None,
    }) {
        Some(b) => alloc_bytes(&b),
        None => alloc_bytes(&[]),
    }
}

extern "C" fn jet_jit_net_udp_packet_original_len(packet: i64) -> i64 {
    with_handle(packet, |h| match h {
        NetHttpHandle::UdpPacket(p) => Some(jet_net_udp_packet_original_len(p)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_net_udp_packet_truncated(packet: i64) -> i64 {
    i64::from(
        with_handle(packet, |h| match h {
            NetHttpHandle::UdpPacket(p) => Some(jet_net_udp_packet_truncated(p)),
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
        return result_err("invalid UnixListener".into());
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
        return result_err("invalid UnixStream".into());
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
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "unix_write",
            None,
            None,
            "invalid UnixStream".into(),
            None,
        )));
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write(&mut guard, &data))
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_write_all_bytes(stream: i64, data: i64) -> i64 {
    let bytes = clone_bytes(data);
    let Some(stream) = unix_stream(stream) else {
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "unix_write_all_bytes",
            None,
            None,
            "invalid UnixStream".into(),
            None,
        )));
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_write_all_bytes(&mut guard, &bytes))
}

#[cfg(unix)]
extern "C" fn jet_jit_net_unix_close(stream: i64) -> i64 {
    let Some(stream) = unix_stream(stream) else {
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "unix_close",
            None,
            None,
            "invalid UnixStream".into(),
            None,
        )));
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_unix_close(&mut guard))
}

#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_listen(_path: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_accept(_listener: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_connect(_path: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_read(_stream: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_write(_stream: i64, _data: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_write_all_bytes(_stream: i64, _data: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}
#[cfg(not(unix))]
extern "C" fn jet_jit_net_unix_close(_stream: i64) -> i64 {
    result_err("unix sockets unsupported".into())
}

// ── TcpListener / TcpStream handle methods ─────────────────────────────────

extern "C" fn jet_jit_tcp_listener_accept(listener: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return result_err("invalid TcpListener".into());
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
        return result_err("invalid TcpListener".into());
    };
    match jet_net_listener_local_socket_addr(&listener) {
        Ok(a) => result_ok_handle(alloc_string(jet_net_socket_to_string(&a))),
        Err(e) => net_err(e),
    }
}

extern "C" fn jet_jit_tcp_stream_read_text(stream: i64, limit: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return result_err("invalid TcpStream".into());
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
        return net_err(JetNetError::InvalidInput(jet_net_detail(
            "write_all",
            None,
            None,
            "invalid TcpStream".into(),
            None,
        )));
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_write_all_bytes(&mut guard, &bytes))
}

extern "C" fn jet_jit_tcp_stream_close(stream: i64) -> i64 {
    let Some(stream) = tcp_stream(stream) else {
        return result_ok_unit();
    };
    let mut guard = stream.lock().unwrap_or_else(|p| p.into_inner());
    map_net_unit(jet_net_tcp_close(&mut guard))
}

// ── core.http.server ───────────────────────────────────────────────────────

extern "C" fn jet_jit_http_mux_new() -> i64 {
    push_handle(NetHttpHandle::HttpMux(Arc::new(jet_http_mux_new())))
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
    push_handle(NetHttpHandle::HttpResponse(jet_http_srv_response(
        status, &body,
    )))
}

extern "C" fn jet_jit_http_req_body(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HttpBody(b)),
        None => 0,
    }
}

extern "C" fn jet_jit_http_req_method(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_method(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_http_req_path(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_path(r)),
        _ => None,
    }) {
        Some(s) => alloc_string(s),
        None => alloc_string(String::new()),
    }
}

extern "C" fn jet_jit_http_req_param(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_param(r, &name)),
        _ => None,
    })
    .flatten())
}

extern "C" fn jet_jit_http_req_header(req: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_header(r, &name)),
        _ => None,
    })
    .flatten())
}

extern "C" fn jet_jit_http_body_text(body: i64, limit: i64) -> i64 {
    match with_handle(body, |h| match h {
        NetHttpHandle::HttpBody(b) => Some(jet_http_body_text(b, limit)),
        _ => None,
    }) {
        Some(Ok(s)) => result_ok_handle(alloc_string(s)),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HttpBody".into()),
    }
}

extern "C" fn jet_jit_http_resp_status(resp: i64) -> i64 {
    with_handle(resp, |h| match h {
        NetHttpHandle::HttpResponse(r) => Some(jet_http_srv_response_status(r)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_http_resp_body(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HttpResponse(r) => Some(jet_http_srv_response_body(r)),
        _ => None,
    }) {
        Some(b) => push_handle(NetHttpHandle::HttpBody(b)),
        None => 0,
    }
}

extern "C" fn jet_jit_http_server_bind(addr: i64, mux: i64) -> i64 {
    let addr = clone_string(addr);
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HttpMux".into());
    };
    match jet_http_server_bind(&addr, (*mux).clone()) {
        Ok(s) => result_ok_handle(push_handle(NetHttpHandle::HttpServer(Arc::new(s)))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_local_addr(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HttpServer".into());
    };
    match jet_http_server_local_addr(&server) {
        Ok(a) => result_ok_handle(alloc_string(a)),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_serve(server: i64) -> i64 {
    let Some(server) = http_server(server) else {
        return result_err("invalid HttpServer".into());
    };
    match jet_http_server_serve(&server) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HttpShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_server_shutdown(server: i64, grace_ms: i64) -> i64 {
    let grace = jet_std::Duration { ms: grace_ms };
    let Some(server) = http_server(server) else {
        return result_err("invalid HttpServer".into());
    };
    match jet_http_server_shutdown(&server, &grace) {
        Ok(report) => result_ok_handle(push_handle(NetHttpHandle::HttpShutdownReport(report))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_shutdown_report_field(report: i64, field: i64) -> i64 {
    with_handle(report, |h| match h {
        NetHttpHandle::HttpShutdownReport(r) => Some(match field {
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
) -> Result<JetHttpResponse, String> {
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
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_client_post(url: i64, body: i64) -> i64 {
    let url = clone_string(url);
    let body = clone_string(body);
    match http_cleartext_exchange("POST", &url, Some(&body)) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_serve_once_listener(listener: i64, mux: i64) -> i64 {
    let Some(listener) = tcp_listener(listener) else {
        return result_err("invalid TcpListener".into());
    };
    let Some(mux) = http_mux(mux) else {
        return result_err("invalid HttpMux".into());
    };
    match jet_http_mux_serve_once_listener(&listener, &mux) {
        Ok(()) => result_ok_unit(),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_ws_upgrade(req: i64) -> i64 {
    match with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_ws_upgrade(r)),
        _ => None,
    }) {
        Some(Ok(c)) => result_ok_handle(push_handle(NetHttpHandle::WsConn(Arc::new(
            Mutex::new(c),
        )))),
        Some(Err(e)) => result_err(format!("{e:?}")),
        None => result_err("invalid HttpRequest".into()),
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

type HttpClosureFn = unsafe extern "C" fn(i64, i64) -> i64;
type HttpMiddlewareFn = unsafe extern "C" fn(i64) -> i64;

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
    let f: HttpClosureFn = unsafe { std::mem::transmute(fn_ptr as usize) };
    let handler: JetHttpHandler = Arc::new(move |req: JetHttpRequest| {
        Concurrency::with_http_jet_runtime(|| {
            let req_h = push_handle(NetHttpHandle::HttpRequest(req));
            let res_h = unsafe { f(env, req_h) };
            match decode_result(res_h) {
                Some((true, bits)) => match take_handle(bits as i64) {
                    Some(NetHttpHandle::HttpResponse(resp)) => Ok(resp),
                    other => {
                        if let Some(v) = other {
                            let _ = push_handle(v);
                        }
                        Err(JetHttpError::Io {
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
                    Err(JetHttpError::Io { operation: msg })
                }
                None => Err(JetHttpError::Io {
                    operation: "handler result".into(),
                }),
            }
        })
    });
    push_handle(NetHttpHandle::HttpHandler(handler))
}

extern "C" fn jet_jit_http_handler_handle(handler: i64, req: i64) -> i64 {
    let Some(handler) = with_handle(handler, |h| match h {
        NetHttpHandle::HttpHandler(h) => Some(Arc::clone(h)),
        _ => None,
    }) else {
        return result_err("invalid HttpHandler".into());
    };
    let Some(req) = take_handle(req).and_then(|h| match h {
        NetHttpHandle::HttpRequest(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HttpRequest".into());
    };
    match handler(req) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(resp))),
        Err(e) => http_err(e),
    }
}

extern "C" fn jet_jit_http_mux_middleware(mux: i64, mw_fn: i64) -> i64 {
    let Some(mux) = http_mux(mux) else {
        return 0;
    };
    let f: HttpMiddlewareFn = unsafe { std::mem::transmute(mw_fn as usize) };
    jet_http_mux_middleware(&mux, move |next| {
        Concurrency::with_http_jet_runtime(|| {
            let next_h = push_handle(NetHttpHandle::HttpHandler(next));
            let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { f(next_h) }));
            let fail = |op: &'static str| -> JetHttpHandler {
                Arc::new(move |_| Err(JetHttpError::Io { operation: op.into() }))
            };
            match out {
                Ok(out) => {
                    if let Some(h) = with_handle(out, |h| match h {
                        NetHttpHandle::HttpHandler(h) => Some(Arc::clone(h)),
                        _ => None,
                    }) {
                        let _ = take_handle(out);
                        return h;
                    }
                    if let Some((true, bits)) = decode_result(out) {
                        if let Some(NetHttpHandle::HttpHandler(h)) = take_handle(bits as i64) {
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
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_trailers(r)),
        _ => None,
    }) {
        Some(Ok(h)) => result_ok_handle(push_handle(NetHttpHandle::HttpHeaders(h))),
        Some(Err(e)) => http_err(e),
        None => result_err("invalid HttpRequest".into()),
    }
}

extern "C" fn jet_jit_http_resp_trailers(resp: i64, trailers: i64) -> i64 {
    let Some(resp) = take_handle(resp).and_then(|h| match h {
        NetHttpHandle::HttpResponse(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HttpResponse".into());
    };
    let Some(trailers) = take_handle(trailers).and_then(|h| match h {
        NetHttpHandle::HttpHeaders(t) => Some(t),
        other => {
            let _ = push_handle(other);
            None
        }
    }) else {
        return result_err("invalid HttpHeaders".into());
    };
    match jet_http_srv_response_trailers(resp, trailers) {
        Ok(r) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(r))),
        Err(e) => http_err(e),
    }
}

extern "C" fn jet_jit_http_req_body_len(req: i64) -> i64 {
    with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_body_len(r)),
        _ => None,
    })
    .unwrap_or(0)
}

extern "C" fn jet_jit_http_req_under_limit(req: i64, max: i64) -> i64 {
    i64::from(
        with_handle(req, |h| match h {
            NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_req_under_limit(r, max)),
            _ => None,
        })
        .unwrap_or(false),
    )
}

extern "C" fn jet_jit_http_sse(data: i64) -> i64 {
    let data = clone_string(data);
    push_handle(NetHttpHandle::HttpResponse(jet_http_srv_sse(&data)))
}

extern "C" fn jet_jit_http_static_file_range(req: i64, path: i64, mime: i64) -> i64 {
    let path = clone_string(path);
    let mime = clone_string(mime);
    match with_handle(req, |h| match h {
        NetHttpHandle::HttpRequest(r) => Some(jet_http_srv_static_file_range(r, &path, &mime)),
        _ => None,
    }) {
        Some(Ok(resp)) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(resp))),
        Some(Err(e)) => result_err(e),
        None => result_err("invalid HttpRequest".into()),
    }
}

extern "C" fn jet_jit_http_client_request_new(method: i64, url: i64) -> i64 {
    let method = clone_string(method);
    let url = clone_string(url);
    push_handle(NetHttpHandle::HttpRequest(JetHttpRequest {
        method,
        url,
        path: String::new(),
        version: "HTTP/1.1".to_string(),
        headers: JetHttpHeaders::new(),
        trailers: std::sync::Arc::new(std::sync::Mutex::new(JetHttpHeaders::new())),
        header_error: None,
        body: JetHttpBody::empty(),
        body_set: false,
        params: std::collections::BTreeMap::new(),
        route_template: None,
        timeout_ms: None,
        connect_timeout_ms: None,
        read_timeout_ms: None,
        total_timeout_ms: None,
        dns_timeout_ms: None,
        tls_timeout_ms: None,
        write_timeout_ms: None,
        first_byte_timeout_ms: None,
        redirects: None,
        proxy: None,
        cookies: Vec::new(),
        form: Vec::new(),
        multipart: Vec::new(),
    }))
}

fn take_http_request(handle: i64) -> Option<JetHttpRequest> {
    take_handle(handle).and_then(|h| match h {
        NetHttpHandle::HttpRequest(r) => Some(r),
        other => {
            let _ = push_handle(other);
            None
        }
    })
}

extern "C" fn jet_jit_http_client_request_form(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    req.form.push(name);
    req.form.push(value);
    push_handle(NetHttpHandle::HttpRequest(req))
}

extern "C" fn jet_jit_http_client_request_cookie(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    req.cookies.push(name);
    req.cookies.push(value);
    push_handle(NetHttpHandle::HttpRequest(req))
}

extern "C" fn jet_jit_http_client_request_header(req: i64, name: i64, value: i64) -> i64 {
    let name = clone_string(name);
    let value = clone_string(value);
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    if req.headers.append(&name, &value).is_err() {
        req.header_error = Some(JetHttpError::InvalidHeader);
    }
    push_handle(NetHttpHandle::HttpRequest(req))
}

extern "C" fn jet_jit_http_client_request_redirects(req: i64, limit: i64) -> i64 {
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    req.redirects = Some(limit);
    push_handle(NetHttpHandle::HttpRequest(req))
}

extern "C" fn jet_jit_http_client_request_connect_timeout(req: i64, ms: i64) -> i64 {
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    req.connect_timeout_ms = Some(ms);
    push_handle(NetHttpHandle::HttpRequest(req))
}

extern "C" fn jet_jit_http_client_request_read_timeout(req: i64, ms: i64) -> i64 {
    let Some(mut req) = take_http_request(req) else {
        return 0;
    };
    req.read_timeout_ms = Some(ms);
    push_handle(NetHttpHandle::HttpRequest(req))
}

fn http_cleartext_request(req: &JetHttpRequest) -> Result<JetHttpResponse, String> {
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
        return result_err("invalid HttpRequest".into());
    };
    match http_cleartext_request(&req) {
        Ok(resp) => result_ok_handle(push_handle(NetHttpHandle::HttpResponse(resp))),
        Err(e) => result_err(e),
    }
}

extern "C" fn jet_jit_http_resp_header(resp: i64, name: i64) -> i64 {
    let name = clone_string(name);
    option_string(with_handle(resp, |h| match h {
        NetHttpHandle::HttpResponse(r) => Some(r.headers.get(&name).cloned()),
        _ => None,
    })
    .flatten())
}

extern "C" fn jet_jit_http_resp_cookies(resp: i64) -> i64 {
    match with_handle(resp, |h| match h {
        NetHttpHandle::HttpResponse(r) => Some(
            r.headers
                .all("set-cookie")
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>(),
        ),
        _ => None,
    }) {
        Some(rows) => list_of_strings(rows),
        None => list_of_strings(Vec::new()),
    }
}

pub(crate) struct NetHttpHostFns {
    pub socket_addr: FuncId,
    pub socket_to_string: FuncId,
    pub socket_host: FuncId,
    pub socket_port_typed: FuncId,
    pub tcp_listen_str: FuncId,
    pub tcp_listen_addr: FuncId,
    pub tcp_connect: FuncId,
    pub listener_local_socket_addr: FuncId,
    pub set_timeout: FuncId,
    pub tcp_reply: FuncId,
    pub udp_bind: FuncId,
    pub udp_local_addr: FuncId,
    pub udp_set_timeout: FuncId,
    pub udp_send_bytes_to: FuncId,
    pub udp_receive: FuncId,
    pub udp_packet_bytes: FuncId,
    pub udp_packet_original_len: FuncId,
    pub udp_packet_truncated: FuncId,
    pub unix_listen: FuncId,
    pub unix_accept: FuncId,
    pub unix_connect: FuncId,
    pub unix_read: FuncId,
    pub unix_write: FuncId,
    pub unix_write_all_bytes: FuncId,
    pub unix_close: FuncId,
    pub tcp_accept: FuncId,
    pub tcp_local_addr: FuncId,
    pub tcp_read_text: FuncId,
    pub tcp_write_all_bytes: FuncId,
    pub tcp_close: FuncId,
    pub http_mux_new: FuncId,
    pub http_mux_add: FuncId,
    pub http_response: FuncId,
    pub http_req_body: FuncId,
    pub http_req_method: FuncId,
    pub http_req_path: FuncId,
    pub http_req_param: FuncId,
    pub http_req_header: FuncId,
    pub http_body_text: FuncId,
    pub http_resp_status: FuncId,
    pub http_resp_body: FuncId,
    pub http_server_bind: FuncId,
    pub http_server_local_addr: FuncId,
    pub http_server_serve: FuncId,
    pub http_server_shutdown: FuncId,
    pub http_shutdown_report_field: FuncId,
    pub http_serve_once_listener: FuncId,
    pub http_client_get: FuncId,
    pub http_client_post: FuncId,
    pub http_handler_bind: FuncId,
    pub http_handler_bind1: FuncId,
    pub http_handler_handle: FuncId,
    pub http_mux_middleware: FuncId,
    pub http_request_id: FuncId,
    pub http_req_trailers: FuncId,
    pub http_resp_trailers: FuncId,
    pub http_req_body_len: FuncId,
    pub http_req_under_limit: FuncId,
    pub http_sse: FuncId,
    pub http_static_file_range: FuncId,
    pub http_client_request_new: FuncId,
    pub http_client_request_form: FuncId,
    pub http_client_request_cookie: FuncId,
    pub http_client_request_header: FuncId,
    pub http_client_request_redirects: FuncId,
    pub http_client_request_connect_timeout: FuncId,
    pub http_client_request_read_timeout: FuncId,
    pub http_client_request_send: FuncId,
    pub http_resp_header: FuncId,
    pub http_resp_cookies: FuncId,
    pub ws_upgrade: FuncId,
    pub ws_connect: FuncId,
    pub ws_send_text: FuncId,
    pub ws_recv: FuncId,
    pub ws_close: FuncId,
    pub ws_message_is_text: FuncId,
    pub ws_message_text: FuncId,
}

pub(crate) fn register_net_http_symbols(builder: &mut JITBuilder) {
    builder.symbol("jet_jit_net_socket_addr", jet_jit_net_socket_addr as *const u8);
    builder.symbol(
        "jet_jit_net_socket_to_string",
        jet_jit_net_socket_to_string as *const u8,
    );
    builder.symbol("jet_jit_net_socket_host", jet_jit_net_socket_host as *const u8);
    builder.symbol(
        "jet_jit_net_socket_port_typed",
        jet_jit_net_socket_port_typed as *const u8,
    );
    builder.symbol(
        "jet_jit_net_tcp_listen_str",
        jet_jit_net_tcp_listen_str as *const u8,
    );
    builder.symbol(
        "jet_jit_net_tcp_listen_addr",
        jet_jit_net_tcp_listen_addr as *const u8,
    );
    builder.symbol("jet_jit_net_tcp_connect", jet_jit_net_tcp_connect as *const u8);
    builder.symbol(
        "jet_jit_net_listener_local_socket_addr2",
        jet_jit_net_listener_local_socket_addr as *const u8,
    );
    builder.symbol("jet_jit_net_set_timeout", jet_jit_net_set_timeout as *const u8);
    builder.symbol("jet_jit_net_tcp_reply", jet_jit_net_tcp_reply as *const u8);
    builder.symbol("jet_jit_net_udp_bind", jet_jit_net_udp_bind as *const u8);
    builder.symbol(
        "jet_jit_net_udp_local_addr",
        jet_jit_net_udp_local_addr as *const u8,
    );
    builder.symbol(
        "jet_jit_net_udp_set_timeout",
        jet_jit_net_udp_set_timeout as *const u8,
    );
    builder.symbol(
        "jet_jit_net_udp_send_bytes_to",
        jet_jit_net_udp_send_bytes_to as *const u8,
    );
    builder.symbol("jet_jit_net_udp_receive", jet_jit_net_udp_receive as *const u8);
    builder.symbol(
        "jet_jit_net_udp_packet_bytes",
        jet_jit_net_udp_packet_bytes as *const u8,
    );
    builder.symbol(
        "jet_jit_net_udp_packet_original_len",
        jet_jit_net_udp_packet_original_len as *const u8,
    );
    builder.symbol(
        "jet_jit_net_udp_packet_truncated",
        jet_jit_net_udp_packet_truncated as *const u8,
    );
    builder.symbol("jet_jit_net_unix_listen", jet_jit_net_unix_listen as *const u8);
    builder.symbol("jet_jit_net_unix_accept", jet_jit_net_unix_accept as *const u8);
    builder.symbol(
        "jet_jit_net_unix_connect",
        jet_jit_net_unix_connect as *const u8,
    );
    builder.symbol("jet_jit_net_unix_read", jet_jit_net_unix_read as *const u8);
    builder.symbol("jet_jit_net_unix_write", jet_jit_net_unix_write as *const u8);
    builder.symbol(
        "jet_jit_net_unix_write_all_bytes",
        jet_jit_net_unix_write_all_bytes as *const u8,
    );
    builder.symbol("jet_jit_net_unix_close", jet_jit_net_unix_close as *const u8);
    builder.symbol(
        "jet_jit_tcp_listener_accept",
        jet_jit_tcp_listener_accept as *const u8,
    );
    builder.symbol(
        "jet_jit_tcp_listener_local_addr",
        jet_jit_tcp_listener_local_addr as *const u8,
    );
    builder.symbol(
        "jet_jit_tcp_stream_read_text",
        jet_jit_tcp_stream_read_text as *const u8,
    );
    builder.symbol(
        "jet_jit_tcp_stream_write_all_bytes",
        jet_jit_tcp_stream_write_all_bytes as *const u8,
    );
    builder.symbol("jet_jit_tcp_stream_close", jet_jit_tcp_stream_close as *const u8);
    builder.symbol("jet_jit_http_mux_new", jet_jit_http_mux_new as *const u8);
    builder.symbol("jet_jit_http_mux_add", jet_jit_http_mux_add as *const u8);
    builder.symbol("jet_jit_http_response", jet_jit_http_response as *const u8);
    builder.symbol("jet_jit_http_req_body", jet_jit_http_req_body as *const u8);
    builder.symbol("jet_jit_http_req_method", jet_jit_http_req_method as *const u8);
    builder.symbol("jet_jit_http_req_path", jet_jit_http_req_path as *const u8);
    builder.symbol("jet_jit_http_req_param", jet_jit_http_req_param as *const u8);
    builder.symbol("jet_jit_http_req_header", jet_jit_http_req_header as *const u8);
    builder.symbol("jet_jit_http_body_text", jet_jit_http_body_text as *const u8);
    builder.symbol("jet_jit_http_resp_status", jet_jit_http_resp_status as *const u8);
    builder.symbol("jet_jit_http_resp_body", jet_jit_http_resp_body as *const u8);
    builder.symbol(
        "jet_jit_http_server_bind",
        jet_jit_http_server_bind as *const u8,
    );
    builder.symbol(
        "jet_jit_http_server_local_addr",
        jet_jit_http_server_local_addr as *const u8,
    );
    builder.symbol(
        "jet_jit_http_server_serve",
        jet_jit_http_server_serve as *const u8,
    );
    builder.symbol(
        "jet_jit_http_server_shutdown",
        jet_jit_http_server_shutdown as *const u8,
    );
    builder.symbol(
        "jet_jit_http_shutdown_report_field",
        jet_jit_http_shutdown_report_field as *const u8,
    );
    builder.symbol(
        "jet_jit_http_serve_once_listener",
        jet_jit_http_serve_once_listener as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_get",
        jet_jit_http_client_get as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_post",
        jet_jit_http_client_post as *const u8,
    );
    builder.symbol(
        "jet_jit_http_handler_bind",
        jet_jit_http_handler_bind as *const u8,
    );
    builder.symbol(
        "jet_jit_http_handler_bind1",
        jet_jit_http_handler_bind1 as *const u8,
    );
    builder.symbol(
        "jet_jit_http_handler_handle",
        jet_jit_http_handler_handle as *const u8,
    );
    builder.symbol(
        "jet_jit_http_mux_middleware",
        jet_jit_http_mux_middleware as *const u8,
    );
    builder.symbol(
        "jet_jit_http_request_id",
        jet_jit_http_request_id as *const u8,
    );
    builder.symbol(
        "jet_jit_http_req_trailers",
        jet_jit_http_req_trailers as *const u8,
    );
    builder.symbol(
        "jet_jit_http_resp_trailers",
        jet_jit_http_resp_trailers as *const u8,
    );
    builder.symbol(
        "jet_jit_http_req_body_len",
        jet_jit_http_req_body_len as *const u8,
    );
    builder.symbol(
        "jet_jit_http_req_under_limit",
        jet_jit_http_req_under_limit as *const u8,
    );
    builder.symbol("jet_jit_http_sse", jet_jit_http_sse as *const u8);
    builder.symbol(
        "jet_jit_http_static_file_range",
        jet_jit_http_static_file_range as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_new",
        jet_jit_http_client_request_new as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_form",
        jet_jit_http_client_request_form as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_cookie",
        jet_jit_http_client_request_cookie as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_header",
        jet_jit_http_client_request_header as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_redirects",
        jet_jit_http_client_request_redirects as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_connect_timeout",
        jet_jit_http_client_request_connect_timeout as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_read_timeout",
        jet_jit_http_client_request_read_timeout as *const u8,
    );
    builder.symbol(
        "jet_jit_http_client_request_send",
        jet_jit_http_client_request_send as *const u8,
    );
    builder.symbol(
        "jet_jit_http_resp_header",
        jet_jit_http_resp_header as *const u8,
    );
    builder.symbol(
        "jet_jit_http_resp_cookies",
        jet_jit_http_resp_cookies as *const u8,
    );
    builder.symbol("jet_jit_ws_upgrade", jet_jit_ws_upgrade as *const u8);
    builder.symbol("jet_jit_ws_connect", jet_jit_ws_connect as *const u8);
    builder.symbol("jet_jit_ws_send_text", jet_jit_ws_send_text as *const u8);
    builder.symbol("jet_jit_ws_recv", jet_jit_ws_recv as *const u8);
    builder.symbol("jet_jit_ws_close", jet_jit_ws_close as *const u8);
    builder.symbol(
        "jet_jit_ws_message_is_text",
        jet_jit_ws_message_is_text as *const u8,
    );
    builder.symbol("jet_jit_ws_message_text", jet_jit_ws_message_text as *const u8);
}

pub(crate) fn declare_net_http_host_fns(
    module: &mut JITModule,
) -> Result<NetHttpHostFns, String> {
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
    let import = |module: &mut JITModule, name: &str, sig: &Signature| {
        module
            .declare_function(name, Linkage::Import, sig)
            .map_err(|e| e.to_string())
    };
    Ok(NetHttpHostFns {
        socket_addr: import(module, "jet_jit_net_socket_addr", &sig2)?,
        socket_to_string: import(module, "jet_jit_net_socket_to_string", &sig1)?,
        socket_host: import(module, "jet_jit_net_socket_host", &sig1)?,
        socket_port_typed: import(module, "jet_jit_net_socket_port_typed", &sig1)?,
        tcp_listen_str: import(module, "jet_jit_net_tcp_listen_str", &sig1)?,
        tcp_listen_addr: import(module, "jet_jit_net_tcp_listen_addr", &sig1)?,
        tcp_connect: import(module, "jet_jit_net_tcp_connect", &sig1)?,
        listener_local_socket_addr: import(
            module,
            "jet_jit_net_listener_local_socket_addr2",
            &sig1,
        )?,
        set_timeout: import(module, "jet_jit_net_set_timeout", &sig2)?,
        tcp_reply: import(module, "jet_jit_net_tcp_reply", &sig3)?,
        udp_bind: import(module, "jet_jit_net_udp_bind", &sig1)?,
        udp_local_addr: import(module, "jet_jit_net_udp_local_addr", &sig1)?,
        udp_set_timeout: import(module, "jet_jit_net_udp_set_timeout", &sig2)?,
        udp_send_bytes_to: import(module, "jet_jit_net_udp_send_bytes_to", &sig3)?,
        udp_receive: import(module, "jet_jit_net_udp_receive", &sig2)?,
        udp_packet_bytes: import(module, "jet_jit_net_udp_packet_bytes", &sig1)?,
        udp_packet_original_len: import(module, "jet_jit_net_udp_packet_original_len", &sig1)?,
        udp_packet_truncated: import(module, "jet_jit_net_udp_packet_truncated", &sig1)?,
        unix_listen: import(module, "jet_jit_net_unix_listen", &sig1)?,
        unix_accept: import(module, "jet_jit_net_unix_accept", &sig1)?,
        unix_connect: import(module, "jet_jit_net_unix_connect", &sig1)?,
        unix_read: import(module, "jet_jit_net_unix_read", &sig1)?,
        unix_write: import(module, "jet_jit_net_unix_write", &sig2)?,
        unix_write_all_bytes: import(module, "jet_jit_net_unix_write_all_bytes", &sig2)?,
        unix_close: import(module, "jet_jit_net_unix_close", &sig1)?,
        tcp_accept: import(module, "jet_jit_tcp_listener_accept", &sig1)?,
        tcp_local_addr: import(module, "jet_jit_tcp_listener_local_addr", &sig1)?,
        tcp_read_text: import(module, "jet_jit_tcp_stream_read_text", &sig2)?,
        tcp_write_all_bytes: import(module, "jet_jit_tcp_stream_write_all_bytes", &sig2)?,
        tcp_close: import(module, "jet_jit_tcp_stream_close", &sig1)?,
        http_mux_new: import(module, "jet_jit_http_mux_new", &sig0)?,
        http_mux_add: import(module, "jet_jit_http_mux_add", &sig4)?,
        http_response: import(module, "jet_jit_http_response", &sig2)?,
        http_req_body: import(module, "jet_jit_http_req_body", &sig1)?,
        http_req_method: import(module, "jet_jit_http_req_method", &sig1)?,
        http_req_path: import(module, "jet_jit_http_req_path", &sig1)?,
        http_req_param: import(module, "jet_jit_http_req_param", &sig2)?,
        http_req_header: import(module, "jet_jit_http_req_header", &sig2)?,
        http_body_text: import(module, "jet_jit_http_body_text", &sig2)?,
        http_resp_status: import(module, "jet_jit_http_resp_status", &sig1)?,
        http_resp_body: import(module, "jet_jit_http_resp_body", &sig1)?,
        http_server_bind: import(module, "jet_jit_http_server_bind", &sig2)?,
        http_server_local_addr: import(module, "jet_jit_http_server_local_addr", &sig1)?,
        http_server_serve: import(module, "jet_jit_http_server_serve", &sig1)?,
        http_server_shutdown: import(module, "jet_jit_http_server_shutdown", &sig2)?,
        http_shutdown_report_field: import(module, "jet_jit_http_shutdown_report_field", &sig2)?,
        http_serve_once_listener: import(module, "jet_jit_http_serve_once_listener", &sig2)?,
        http_client_get: import(module, "jet_jit_http_client_get", &sig1)?,
        http_client_post: import(module, "jet_jit_http_client_post", &sig2)?,
        http_handler_bind: import(module, "jet_jit_http_handler_bind", &sig2)?,
        http_handler_bind1: import(module, "jet_jit_http_handler_bind1", &sig2)?,
        http_handler_handle: import(module, "jet_jit_http_handler_handle", &sig2)?,
        http_mux_middleware: import(module, "jet_jit_http_mux_middleware", &sig2)?,
        http_request_id: import(module, "jet_jit_http_request_id", &sig1)?,
        http_req_trailers: import(module, "jet_jit_http_req_trailers", &sig1)?,
        http_resp_trailers: import(module, "jet_jit_http_resp_trailers", &sig2)?,
        http_req_body_len: import(module, "jet_jit_http_req_body_len", &sig1)?,
        http_req_under_limit: import(module, "jet_jit_http_req_under_limit", &sig2)?,
        http_sse: import(module, "jet_jit_http_sse", &sig1)?,
        http_static_file_range: import(module, "jet_jit_http_static_file_range", &sig3)?,
        http_client_request_new: import(module, "jet_jit_http_client_request_new", &sig2)?,
        http_client_request_form: import(module, "jet_jit_http_client_request_form", &sig3)?,
        http_client_request_cookie: import(module, "jet_jit_http_client_request_cookie", &sig3)?,
        http_client_request_header: import(module, "jet_jit_http_client_request_header", &sig3)?,
        http_client_request_redirects: import(
            module,
            "jet_jit_http_client_request_redirects",
            &sig2,
        )?,
        http_client_request_connect_timeout: import(
            module,
            "jet_jit_http_client_request_connect_timeout",
            &sig2,
        )?,
        http_client_request_read_timeout: import(
            module,
            "jet_jit_http_client_request_read_timeout",
            &sig2,
        )?,
        http_client_request_send: import(module, "jet_jit_http_client_request_send", &sig1)?,
        http_resp_header: import(module, "jet_jit_http_resp_header", &sig2)?,
        http_resp_cookies: import(module, "jet_jit_http_resp_cookies", &sig1)?,
        ws_upgrade: import(module, "jet_jit_ws_upgrade", &sig1)?,
        ws_connect: import(module, "jet_jit_ws_connect", &sig1)?,
        ws_send_text: import(module, "jet_jit_ws_send_text", &sig2)?,
        ws_recv: import(module, "jet_jit_ws_recv", &sig1)?,
        ws_close: import(module, "jet_jit_ws_close", &sig3)?,
        ws_message_is_text: import(module, "jet_jit_ws_message_is_text", &sig1)?,
        ws_message_text: import(module, "jet_jit_ws_message_text", &sig1)?,
    })
}
