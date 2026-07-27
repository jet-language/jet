// D-WS1=B: native std-only WebSocket (RFC6455) for core.ws.
// Client connect and server upgrade share one frame codec. No external crates.
thread_local! {
    static JET_WS_ACTIVE_STREAM: std::cell::RefCell<Option<*mut std::net::TcpStream>> =
        const { std::cell::RefCell::new(None) };
    static JET_WS_UPGRADED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct JetWsStreamGuard;

impl JetWsStreamGuard {
    fn install(stream: &mut std::net::TcpStream) -> Self {
        JET_WS_UPGRADED.with(|flag| flag.set(false));
        JET_WS_ACTIVE_STREAM.with(|slot| {
            *slot.borrow_mut() = Some(stream as *mut std::net::TcpStream);
        });
        Self
    }
}

impl Drop for JetWsStreamGuard {
    fn drop(&mut self) {
        JET_WS_ACTIVE_STREAM.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }
}

fn jet_ws_take_upgraded() -> bool {
    JET_WS_UPGRADED.with(|flag| {
        let upgraded = flag.get();
        flag.set(false);
        upgraded
    })
}

fn jet_ws_header_get<'a>(headers: &'a JetHTTPHeaders, name: &str) -> Option<&'a str> {
    headers.get(name).map(String::as_str)
}

fn jet_ws_header_has_token(headers: &JetHTTPHeaders, name: &str, token: &str) -> bool {
    headers.all(name).iter().any(|value| {
        value
            .split(',')
            .map(str::trim)
            .any(|part| part.eq_ignore_ascii_case(token))
    })
}

fn jet_ws_validate_upgrade_request(req: &JetHTTPRequest) -> Result<String, JetWsError> {
    if req.method != "GET" {
        return Err(JetWsError::InvalidHandshake);
    }
    if !jet_ws_header_has_token(&req.headers, "upgrade", "websocket") {
        return Err(JetWsError::InvalidHandshake);
    }
    if !jet_ws_header_has_token(&req.headers, "connection", "Upgrade") {
        return Err(JetWsError::InvalidHandshake);
    }
    let version = jet_ws_header_get(&req.headers, "sec-websocket-version")
        .ok_or(JetWsError::InvalidHandshake)?;
    if version != "13" {
        return Err(JetWsError::InvalidHandshake);
    }
    let key = jet_ws_header_get(&req.headers, "sec-websocket-key")
        .ok_or(JetWsError::InvalidHandshake)?;
    if key.is_empty() || key.len() > 256 {
        return Err(JetWsError::InvalidHandshake);
    }
    Ok(key.to_string())
}

fn jet_ws_write_upgrade_response(
    stream: &mut std::net::TcpStream,
    accept: &str,
) -> Result<(), JetWsError> {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|_| JetWsError::IO {
            operation: "write upgrade response".to_string(),
        })?;
    stream.flush().map_err(|_| JetWsError::IO {
        operation: "flush upgrade response".to_string(),
    })?;
    Ok(())
}

fn jet_ws_upgrade(req: &JetHTTPRequest) -> Result<JetWsConn, JetWsError> {
    let key = jet_ws_validate_upgrade_request(req)?;
    let accept = jet_ws_accept_key(&key);
    let mut stream = JET_WS_ACTIVE_STREAM.with(|slot| -> Result<std::net::TcpStream, JetWsError> {
        let ptr = slot.borrow().ok_or(JetWsError::InvalidHandshake)?;
        // SAFETY: pointer is set only while JetWsStreamGuard wraps mux dispatch.
        // JET_VETTED_UNSAFE_BEGIN: jet_ws_upgrade
        let stream = unsafe { &mut *ptr };
        // JET_VETTED_UNSAFE_END: jet_ws_upgrade
        stream.try_clone().map_err(|_| JetWsError::IO {
            operation: "clone upgrade stream".to_string(),
        })
    })?;
    jet_ws_write_upgrade_response(&mut stream, &accept)?;
    JET_WS_UPGRADED.with(|flag| flag.set(true));
    JetWsConn::from_stream(stream, JetWsRole::Server)
}
