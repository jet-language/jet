// D-WS1=B: native std-only WebSocket (RFC6455) for core.ws.
// Client connect and server upgrade share one frame codec. No external crates.

const JET_WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
const JET_WS_MAX_MESSAGE: usize = 1024 * 1024;
const JET_WS_MAX_CONTROL: usize = 125;
const JET_WS_DEFAULT_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

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

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetWsError {
    InvalidUrl,
    InvalidHandshake,
    Protocol,
    MessageTooLarge { limit: i64 },
    Io { operation: String },
    Timeout,
    Closed,
    Cancelled,
    UnsupportedTarget,
}

impl JetShow for JetWsError {
    fn jet_show(&self) -> String {
        match self {
            Self::InvalidUrl => "websocket URL is invalid".to_string(),
            Self::InvalidHandshake => "websocket handshake failed".to_string(),
            Self::Protocol => "websocket protocol error".to_string(),
            Self::MessageTooLarge { limit } => {
                format!("websocket message exceeds {limit} bytes")
            }
            Self::Io { operation } => format!("websocket I/O failed during {operation}"),
            Self::Timeout => "websocket timed out".to_string(),
            Self::Closed => "websocket is closed".to_string(),
            Self::Cancelled => "websocket cancelled".to_string(),
            Self::UnsupportedTarget => {
                "this build target lacks websocket client connect".to_string()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum JetWsMessage {
    Text(String),
    Binary(Vec<u8>),
    Close { code: i64, reason: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JetWsRole {
    Client,
    Server,
}

struct JetWsConn {
    stream: std::cell::RefCell<std::net::TcpStream>,
    role: JetWsRole,
    closed: std::cell::Cell<bool>,
    max_message: usize,
    pending: std::cell::RefCell<Option<JetWsMessage>>,
}

impl JetWsConn {
    fn from_stream(stream: std::net::TcpStream, role: JetWsRole) -> Result<Self, JetWsError> {
        stream
            .set_read_timeout(Some(JET_WS_DEFAULT_READ_TIMEOUT))
            .map_err(|_| JetWsError::Io {
                operation: "set read timeout".to_string(),
            })?;
        stream
            .set_write_timeout(Some(JET_WS_DEFAULT_READ_TIMEOUT))
            .map_err(|_| JetWsError::Io {
                operation: "set write timeout".to_string(),
            })?;
        Ok(Self {
            stream: std::cell::RefCell::new(stream),
            role,
            closed: std::cell::Cell::new(false),
            max_message: JET_WS_MAX_MESSAGE,
            pending: std::cell::RefCell::new(None),
        })
    }
}

fn jet_ws_sha1(data: &[u8]) -> [u8; 20] {
    // Minimal SHA-1 for the RFC6455 accept key only.
    fn rotl(value: u32, bits: u32) -> u32 {
        value.rotate_left(bits)
    }
    let mut h0: u32 = 0x6745_2301;
    let mut h1: u32 = 0xEFCD_AB89;
    let mut h2: u32 = 0x98BA_DCFE;
    let mut h3: u32 = 0x1032_5476;
    let mut h4: u32 = 0xC3D2_E1F0;
    let bit_len = (data.len() as u64).saturating_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..80 {
            w[i] = rotl(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let temp = rotl(a, 5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = rotl(b, 30);
            b = a;
            a = temp;
        }
        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (index, value) in [h0, h1, h2, h3, h4].into_iter().enumerate() {
        out[index * 4..(index + 1) * 4].copy_from_slice(&value.to_be_bytes());
    }
    out
}

fn jet_ws_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let n = ((bytes[index] as u32) << 16)
            | ((bytes[index + 1] as u32) << 8)
            | (bytes[index + 2] as u32);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    let rest = bytes.len() - index;
    if rest == 1 {
        let n = (bytes[index] as u32) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = ((bytes[index] as u32) << 16) | ((bytes[index + 1] as u32) << 8);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

fn jet_ws_accept_key(sec_websocket_key: &str) -> String {
    let mut material = Vec::with_capacity(sec_websocket_key.len() + JET_WS_GUID.len());
    material.extend_from_slice(sec_websocket_key.as_bytes());
    material.extend_from_slice(JET_WS_GUID);
    jet_ws_base64(&jet_ws_sha1(&material))
}

fn jet_ws_random_key() -> String {
    let mut bytes = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    for (index, slot) in bytes.iter_mut().enumerate() {
        *slot = ((nanos >> (index * 8)) as u8).wrapping_add(index as u8).wrapping_mul(37);
    }
    jet_ws_base64(&bytes)
}

fn jet_ws_mask_key() -> [u8; 4] {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    [
        (nanos as u8).wrapping_mul(17),
        ((nanos >> 8) as u8).wrapping_mul(19),
        ((nanos >> 16) as u8).wrapping_mul(23),
        ((nanos >> 24) as u8).wrapping_mul(29),
    ]
}

fn jet_ws_apply_mask(payload: &mut [u8], mask: [u8; 4]) {
    for (index, byte) in payload.iter_mut().enumerate() {
        *byte ^= mask[index % 4];
    }
}

fn jet_ws_header_get<'a>(headers: &'a JetHttpHeaders, name: &str) -> Option<&'a str> {
    headers.get(name).map(String::as_str)
}

fn jet_ws_header_has_token(headers: &JetHttpHeaders, name: &str, token: &str) -> bool {
    headers.all(name).iter().any(|value| {
        value
            .split(',')
            .map(str::trim)
            .any(|part| part.eq_ignore_ascii_case(token))
    })
}

fn jet_ws_parse_url(url: &str) -> Result<(String, u16, String), JetWsError> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or(JetWsError::InvalidUrl)?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err(JetWsError::InvalidUrl);
    }
    let (host, port) = if let Some(host) = authority.strip_prefix('[') {
        let (host, rest) = host.split_once(']').ok_or(JetWsError::InvalidUrl)?;
        let port = if rest.is_empty() {
            80
        } else {
            rest.strip_prefix(':')
                .ok_or(JetWsError::InvalidUrl)?
                .parse::<u16>()
                .map_err(|_| JetWsError::InvalidUrl)?
        };
        (host.to_string(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') {
            return Err(JetWsError::InvalidUrl);
        }
        (
            host.to_string(),
            port.parse::<u16>().map_err(|_| JetWsError::InvalidUrl)?,
        )
    } else {
        (authority.to_string(), 80)
    };
    if host.is_empty() || !path.starts_with('/') {
        return Err(JetWsError::InvalidUrl);
    }
    Ok((host, port, path))
}

fn jet_ws_write_frame(
    stream: &mut std::net::TcpStream,
    opcode: u8,
    payload: &[u8],
    masked: bool,
) -> Result<(), JetWsError> {
    use std::io::Write;
    if payload.len() > JET_WS_MAX_MESSAGE {
        return Err(JetWsError::MessageTooLarge {
            limit: JET_WS_MAX_MESSAGE as i64,
        });
    }
    if matches!(opcode, 0x8 | 0x9 | 0xA) && payload.len() > JET_WS_MAX_CONTROL {
        return Err(JetWsError::Protocol);
    }
    let mut header = Vec::with_capacity(14);
    header.push(0x80 | (opcode & 0x0F));
    let mask_bit = if masked { 0x80 } else { 0 };
    if payload.len() < 126 {
        header.push(mask_bit | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        header.push(mask_bit | 126);
        header.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        header.push(mask_bit | 127);
        header.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    let mut body = payload.to_vec();
    if masked {
        let mask = jet_ws_mask_key();
        header.extend_from_slice(&mask);
        jet_ws_apply_mask(&mut body, mask);
    }
    stream.write_all(&header).map_err(|_| JetWsError::Io {
        operation: "write frame header".to_string(),
    })?;
    stream.write_all(&body).map_err(|_| JetWsError::Io {
        operation: "write frame payload".to_string(),
    })?;
    stream.flush().map_err(|_| JetWsError::Io {
        operation: "flush frame".to_string(),
    })?;
    Ok(())
}

fn jet_ws_io_kind(error: &std::io::Error) -> JetWsError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => JetWsError::Timeout,
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset => {
            JetWsError::Closed
        }
        _ => JetWsError::Io {
            operation: "read".to_string(),
        },
    }
}

fn jet_ws_read_exact(
    stream: &mut std::net::TcpStream,
    buf: &mut [u8],
) -> Result<(), JetWsError> {
    use std::io::Read;
    let mut filled = 0;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => return Err(JetWsError::Closed),
            Ok(count) => filled += count,
            Err(error) => return Err(jet_ws_io_kind(&error)),
        }
    }
    Ok(())
}

fn jet_ws_read_frame(
    stream: &mut std::net::TcpStream,
    expect_masked: bool,
    max_message: usize,
) -> Result<(u8, bool, Vec<u8>), JetWsError> {
    let mut header = [0u8; 2];
    jet_ws_read_exact(stream, &mut header)?;
    let fin = header[0] & 0x80 != 0;
    let rsv = header[0] & 0x70;
    let opcode = header[0] & 0x0F;
    if rsv != 0 {
        return Err(JetWsError::Protocol);
    }
    let masked = header[1] & 0x80 != 0;
    if masked != expect_masked {
        return Err(JetWsError::Protocol);
    }
    let mut len = (header[1] & 0x7F) as usize;
    if len == 126 {
        let mut ext = [0u8; 2];
        jet_ws_read_exact(stream, &mut ext)?;
        len = u16::from_be_bytes(ext) as usize;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        jet_ws_read_exact(stream, &mut ext)?;
        let value = u64::from_be_bytes(ext);
        if value > usize::MAX as u64 {
            return Err(JetWsError::MessageTooLarge {
                limit: max_message as i64,
            });
        }
        len = value as usize;
    }
    if matches!(opcode, 0x8 | 0x9 | 0xA) {
        if !fin || len > JET_WS_MAX_CONTROL {
            return Err(JetWsError::Protocol);
        }
    } else if len > max_message {
        return Err(JetWsError::MessageTooLarge {
            limit: max_message as i64,
        });
    }
    let mask = if masked {
        let mut key = [0u8; 4];
        jet_ws_read_exact(stream, &mut key)?;
        Some(key)
    } else {
        None
    };
    let mut payload = vec![0u8; len];
    if len > 0 {
        jet_ws_read_exact(stream, &mut payload)?;
    }
    if let Some(mask) = mask {
        jet_ws_apply_mask(&mut payload, mask);
    }
    Ok((opcode, fin, payload))
}

fn jet_ws_send_message(conn: &JetWsConn, opcode: u8, payload: &[u8]) -> Result<(), JetWsError> {
    if conn.closed.get() {
        return Err(JetWsError::Closed);
    }
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            return Err(JetWsError::Cancelled);
        }
    }
    let masked = matches!(conn.role, JetWsRole::Client);
    jet_ws_write_frame(&mut *conn.stream.borrow_mut(), opcode, payload, masked)
}

fn jet_ws_send_text(conn: &JetWsConn, text: &String) -> Result<(), JetWsError> {
    jet_ws_send_message(conn, 0x1, text.as_bytes())
}

fn jet_ws_send_binary(conn: &JetWsConn, bytes: &Vec<u8>) -> Result<(), JetWsError> {
    jet_ws_send_message(conn, 0x2, bytes)
}

fn jet_ws_close(conn: &JetWsConn, code: i64, reason: &String) -> Result<(), JetWsError> {
    if conn.closed.get() {
        return Ok(());
    }
    let code = if (1000..=4999).contains(&code) {
        code as u16
    } else {
        1000
    };
    let reason_bytes = reason.as_bytes();
    let reason_bytes = if reason_bytes.len() > 123 {
        &reason_bytes[..123]
    } else {
        reason_bytes
    };
    let mut payload = Vec::with_capacity(2 + reason_bytes.len());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(reason_bytes);
    let result = jet_ws_send_message(conn, 0x8, &payload);
    conn.closed.set(true);
    let _ = conn.stream.borrow_mut().shutdown(std::net::Shutdown::Both);
    result
}

fn jet_ws_recv(conn: &JetWsConn) -> Result<JetWsMessage, JetWsError> {
    if let Some(message) = conn.pending.borrow_mut().take() {
        return Ok(message);
    }
    if conn.closed.get() {
        return Err(JetWsError::Closed);
    }
    let expect_masked = matches!(conn.role, JetWsRole::Server);
    let mut assembled = Vec::new();
    let mut data_opcode = None;
    loop {
        if let Some(remaining) = jet_deadline_remaining_ms() {
            if remaining <= 0 {
                return Err(JetWsError::Cancelled);
            }
        }
        let (opcode, fin, payload) = jet_ws_read_frame(
            &mut *conn.stream.borrow_mut(),
            expect_masked,
            conn.max_message,
        )?;
        match opcode {
            0x0 => {
                let Some(kind) = data_opcode else {
                    return Err(JetWsError::Protocol);
                };
                if assembled.len().saturating_add(payload.len()) > conn.max_message {
                    return Err(JetWsError::MessageTooLarge {
                        limit: conn.max_message as i64,
                    });
                }
                assembled.extend_from_slice(&payload);
                if fin {
                    return jet_ws_finish_data(kind, assembled);
                }
            }
            0x1 | 0x2 => {
                if data_opcode.is_some() {
                    return Err(JetWsError::Protocol);
                }
                if fin {
                    return jet_ws_finish_data(opcode, payload);
                }
                data_opcode = Some(opcode);
                assembled = payload;
            }
            0x8 => {
                let (code, reason) = if payload.len() >= 2 {
                    let code = u16::from_be_bytes([payload[0], payload[1]]) as i64;
                    let reason = String::from_utf8_lossy(&payload[2..]).into_owned();
                    (code, reason)
                } else {
                    (1000, String::new())
                };
                conn.closed.set(true);
                let _ = jet_ws_write_frame(
                    &mut *conn.stream.borrow_mut(),
                    0x8,
                    &payload,
                    matches!(conn.role, JetWsRole::Client),
                );
                let _ = conn.stream.borrow_mut().shutdown(std::net::Shutdown::Both);
                return Ok(JetWsMessage::Close { code, reason });
            }
            0x9 => {
                jet_ws_write_frame(
                    &mut *conn.stream.borrow_mut(),
                    0xA,
                    &payload,
                    matches!(conn.role, JetWsRole::Client),
                )?;
            }
            0xA => {}
            _ => return Err(JetWsError::Protocol),
        }
    }
}

fn jet_ws_finish_data(opcode: u8, payload: Vec<u8>) -> Result<JetWsMessage, JetWsError> {
    match opcode {
        0x1 => {
            let text = String::from_utf8(payload).map_err(|_| JetWsError::Protocol)?;
            Ok(JetWsMessage::Text(text))
        }
        0x2 => Ok(JetWsMessage::Binary(payload)),
        _ => Err(JetWsError::Protocol),
    }
}

fn jet_ws_validate_upgrade_request(req: &JetHttpRequest) -> Result<String, JetWsError> {
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
        .map_err(|_| JetWsError::Io {
            operation: "write upgrade response".to_string(),
        })?;
    stream.flush().map_err(|_| JetWsError::Io {
        operation: "flush upgrade response".to_string(),
    })?;
    Ok(())
}

fn jet_ws_upgrade(req: &JetHttpRequest) -> Result<JetWsConn, JetWsError> {
    let key = jet_ws_validate_upgrade_request(req)?;
    let accept = jet_ws_accept_key(&key);
    let mut stream = JET_WS_ACTIVE_STREAM.with(|slot| -> Result<std::net::TcpStream, JetWsError> {
        let ptr = slot.borrow().ok_or(JetWsError::InvalidHandshake)?;
        // SAFETY: pointer is set only while JetWsStreamGuard wraps mux dispatch.
        // JET_VETTED_UNSAFE_BEGIN: jet_ws_upgrade
        let stream = unsafe { &mut *ptr };
        // JET_VETTED_UNSAFE_END: jet_ws_upgrade
        stream.try_clone().map_err(|_| JetWsError::Io {
            operation: "clone upgrade stream".to_string(),
        })
    })?;
    jet_ws_write_upgrade_response(&mut stream, &accept)?;
    JET_WS_UPGRADED.with(|flag| flag.set(true));
    JetWsConn::from_stream(stream, JetWsRole::Server)
}

fn jet_ws_connect(url: &String) -> Result<JetWsConn, JetWsError> {
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "windows"
    )))]
    {
        let _ = url;
        return Err(JetWsError::UnsupportedTarget);
    }
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "windows"
    ))]
    {
        use std::io::{Read, Write};
        let (host, port, path) = jet_ws_parse_url(url)?;
        let addr = format!("{host}:{port}");
        let mut stream = std::net::TcpStream::connect(addr).map_err(|_| JetWsError::Io {
            operation: "connect".to_string(),
        })?;
        stream
            .set_read_timeout(Some(JET_WS_DEFAULT_READ_TIMEOUT))
            .map_err(|_| JetWsError::Io {
                operation: "set read timeout".to_string(),
            })?;
        stream
            .set_write_timeout(Some(JET_WS_DEFAULT_READ_TIMEOUT))
            .map_err(|_| JetWsError::Io {
                operation: "set write timeout".to_string(),
            })?;
        let key = jet_ws_random_key();
        let host_header = if port == 80 {
            host.clone()
        } else {
            format!("{host}:{port}")
        };
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {host_header}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|_| JetWsError::Io {
                operation: "write handshake".to_string(),
            })?;
        stream.flush().map_err(|_| JetWsError::Io {
            operation: "flush handshake".to_string(),
        })?;
        let mut response = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let read = stream.read(&mut chunk).map_err(|error| jet_ws_io_kind(&error))?;
            if read == 0 {
                return Err(JetWsError::InvalidHandshake);
            }
            response.extend_from_slice(&chunk[..read]);
            if response.len() > 16 * 1024 {
                return Err(JetWsError::InvalidHandshake);
            }
            if let Some(end) = response.windows(4).position(|window| window == b"\r\n\r\n") {
                let header = std::str::from_utf8(&response[..end]).map_err(|_| JetWsError::InvalidHandshake)?;
                let mut lines = header.split("\r\n");
                let status = lines.next().ok_or(JetWsError::InvalidHandshake)?;
                if !status.starts_with("HTTP/1.1 101") && !status.starts_with("HTTP/1.0 101") {
                    return Err(JetWsError::InvalidHandshake);
                }
                let mut upgrade_ok = false;
                let mut connection_ok = false;
                let mut accept_ok = false;
                let expected = jet_ws_accept_key(&key);
                for line in lines {
                    let Some((name, value)) = line.split_once(':') else {
                        continue;
                    };
                    let value = value.trim();
                    if name.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case("websocket")
                    {
                        upgrade_ok = true;
                    } else if name.eq_ignore_ascii_case("connection")
                        && value
                            .split(',')
                            .map(str::trim)
                            .any(|part| part.eq_ignore_ascii_case("Upgrade"))
                    {
                        connection_ok = true;
                    } else if name.eq_ignore_ascii_case("sec-websocket-accept") && value == expected
                    {
                        accept_ok = true;
                    }
                }
                if !(upgrade_ok && connection_ok && accept_ok) {
                    return Err(JetWsError::InvalidHandshake);
                }
                break;
            }
        }
        JetWsConn::from_stream(stream, JetWsRole::Client)
    }
}

fn jet_ws_message_is_text(message: &JetWsMessage) -> bool {
    matches!(message, JetWsMessage::Text(_))
}

fn jet_ws_message_is_binary(message: &JetWsMessage) -> bool {
    matches!(message, JetWsMessage::Binary(_))
}

fn jet_ws_message_is_close(message: &JetWsMessage) -> bool {
    matches!(message, JetWsMessage::Close { .. })
}

fn jet_ws_message_text(message: &JetWsMessage) -> Result<String, JetWsError> {
    match message {
        JetWsMessage::Text(text) => Ok(text.clone()),
        _ => Err(JetWsError::Protocol),
    }
}

fn jet_ws_message_bytes(message: &JetWsMessage) -> Result<Vec<u8>, JetWsError> {
    match message {
        JetWsMessage::Binary(bytes) => Ok(bytes.clone()),
        _ => Err(JetWsError::Protocol),
    }
}

fn jet_ws_accept_key_public(key: &String) -> String {
    jet_ws_accept_key(key)
}
