// Host shims for TCP/UDP/Unix + HTTP mux/server + WS — same module as net_http_rt includes.

use std::sync::MutexGuard;

use crate::Concurrency;

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
    WsMessage(JetWsMessage)
}

// Process-wide: spawn workers share handles (thread_local was empty on workers).
// Never hold this lock across blocking accept/read/write — clone Arc first.
static HANDLES: Mutex<Vec<Option<NetHttpHandle>>> = Mutex::new(Vec::new());

fn lock_handles() -> MutexGuard<'static, Vec<Option<NetHttpHandle>>> {
    HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn with_handle<R>(handle: i64, f: impl FnOnce(&NetHttpHandle) -> Option<R>) -> Option<R> {
    let v = lock_handles();
    let idx = handle.saturating_sub(1) as usize;
    v.get(idx).and_then(|s| s.as_ref()).and_then(f)
}

fn tls_stream(handle: i64) -> Option<Arc<Mutex<JetTLSStream>>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::TLSStream(stream) => Some(Arc::clone(stream)),
        _ => None
    })
}

fn http_mux(handle: i64) -> Option<Arc<JetHTTPMux>> {
    with_handle(handle, |h| match h {
        NetHttpHandle::HTTPMux(m) => Some(Arc::clone(m)),
        _ => None
    })
}

// ── core.net ───────────────────────────────────────────────────────────────

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
        Err(error) => net_err(error)
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

#[cfg(not(unix))]
fn jet_jit_net_unix_accept_deadline(_listener: i64, deadline: i64) -> i64 {
    let listener = JetUnixListener;
    let deadline = jet_std::Duration { ns: deadline };
    net_err(jet_net_unix_accept_deadline(&listener, &deadline).unwrap_err())
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

#[cfg(not(unix))]
fn jet_jit_net_unix_ready(_stream: i64, interest: i64, deadline: i64) -> i64 {
    let Some(interest) = net_ready_interest(interest) else {
        return net_invalid("unix ready", "NetReadyInterest");
    };
    let stream = JetUnixStream;
    let deadline = jet_std::Duration { ns: deadline };
    net_err(jet_net_unix_ready(&stream, interest, &deadline).unwrap_err())
}

#[cfg(not(unix))]
fn jet_jit_net_unix_set_timeout(_stream: i64, timeout: i64) -> i64 {
    let mut stream = JetUnixStream;
    let timeout = jet_std::Duration { ns: timeout };
    net_err(jet_net_unix_set_timeout(&mut stream, &timeout).unwrap_err())
}

// ── TcpListener / TcpStream handle methods ─────────────────────────────────

// ── core.http.server ───────────────────────────────────────────────────────

jet_http_client_bridge!(native_http);


