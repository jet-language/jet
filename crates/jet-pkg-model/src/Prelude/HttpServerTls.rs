use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn jet_http_server_tls_config(
    cert_pem: &String,
    key_pem: &String,
) -> Result<Arc<rustls::ServerConfig>, String> {
    static RUSTLS_PROVIDER: std::sync::Once = std::sync::Once::new();
    RUSTLS_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });

    let mut cert_reader = BufReader::new(cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "TLS certificate PEM could not be read".to_string())?;
    if certs.is_empty() {
        return Err("TLS certificate PEM did not contain a certificate".to_string());
    }

    let mut key_reader = BufReader::new(key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|_| "TLS private key PEM could not be read".to_string())?
        .ok_or_else(|| "TLS private key PEM did not contain a private key".to_string())?;

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| "TLS certificate and private key do not match".to_string())?;
    Ok(Arc::new(config))
}

pub fn jet_http_server_tls_validate_impl(
    cert_pem: &String,
    key_pem: &String,
) -> Result<(), String> {
    jet_http_server_tls_config(cert_pem, key_pem).map(|_| ())
}

/// One TLS connection session: handshake, then repeatedly read one buffered HTTP/1
/// request, dispatch, and write the response until the dispatcher asks to close,
/// idle timeout hits, or shutdown stops keep-alive reuse.
///
/// `on_request` receives `(raw_request, force_close)`. `force_close` is set on the
/// final request of the 1000-request cap so the response carries `Connection: close`
/// like the plain HTTP path.
pub fn jet_http_server_tls_session_impl(
    cert_pem: &String,
    key_pem: &String,
    stream: TcpStream,
    on_request: Box<dyn FnMut(&[u8], bool) -> Result<(Vec<u8>, bool), String> + Send>,
    should_stop: Box<dyn Fn() -> bool + Send>,
) -> Result<(), String> {
    jet_http_server_tls_session_limited(cert_pem, key_pem, stream, on_request, should_stop, 1000)
}

fn jet_http_server_tls_session_limited(
    cert_pem: &String,
    key_pem: &String,
    stream: TcpStream,
    mut on_request: Box<dyn FnMut(&[u8], bool) -> Result<(Vec<u8>, bool), String> + Send>,
    should_stop: Box<dyn Fn() -> bool + Send>,
    max_requests: usize,
) -> Result<(), String> {
    const MAX_HEADER_BYTES: usize = 32 * 1024;
    const MAX_BODY_BYTES: usize = 1024 * 1024;
    const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
    const BODY_TIMEOUT: Duration = Duration::from_secs(30);
    const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
    const SHUTDOWN_POLL: Duration = Duration::from_millis(20);

    let config = jet_http_server_tls_config(cert_pem, key_pem)?;
    let conn = rustls::ServerConnection::new(config)
        .map_err(|_| "TLS server could not start the handshake".to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, stream);

    // Carry pipelined leftovers across requests — same law as plain HTTP/1.
    let mut pending = Vec::new();
    for request_index in 0..max_requests {
        if request_index > 0 && should_stop() {
            break;
        }
        let between = request_index > 0;
        let idle = if between { IDLE_TIMEOUT } else { HEADER_TIMEOUT };
        let started = std::time::Instant::now();
        let mut framing = JetHttpTlsMessageState::new();
        let (request, rejected) = loop {
            if between && pending.is_empty() && should_stop() {
                return Ok(());
            }
            match framing.advance(&pending, MAX_HEADER_BYTES, MAX_BODY_BYTES) {
                JetHttpTlsMessageStatus::Complete(end) => {
                    break (pending.drain(..end).collect::<Vec<u8>>(), false);
                }
                JetHttpTlsMessageStatus::Reject => {
                    break (std::mem::take(&mut pending), true);
                }
                JetHttpTlsMessageStatus::Pending => {}
            }
            let deadline = if framing.reading_body() {
                started + BODY_TIMEOUT
            } else if pending.is_empty() && between {
                started + idle
            } else {
                started + HEADER_TIMEOUT.min(idle)
            };
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                if between && pending.is_empty() {
                    return Ok(());
                }
                return Err("TLS request timed out".to_string());
            }
            let timeout = if should_stop() {
                remaining.min(SHUTDOWN_POLL)
            } else {
                remaining
            };
            tls.sock
                .set_read_timeout(Some(timeout.max(Duration::from_millis(1))))
                .map_err(|_| "TLS read timeout setup failed".to_string())?;
            let mut buf = [0u8; 8192];
            match tls.read(&mut buf) {
                Ok(0) if pending.is_empty() => return Ok(()),
                Ok(0) => {
                    return Err(
                        "TLS request ended before its declared framing was complete".to_string(),
                    )
                }
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if between && pending.is_empty() && should_stop() {
                        return Ok(());
                    }
                    continue;
                }
                Err(_) => return Err("TLS request read failed".to_string()),
            }
        };

        let force_close = rejected || request_index + 1 == max_requests;
        // Match plaintext: once shutdown is set, do not start another keep-alive
        // request even if bytes already arrived on the socket.
        if request_index > 0 && should_stop() {
            break;
        }
        let (response, keep_alive) = on_request(&request, force_close)?;
        tls.sock
            .set_write_timeout(Some(BODY_TIMEOUT))
            .map_err(|_| "TLS write timeout setup failed".to_string())?;
        tls.write_all(&response)
            .map_err(|_| "TLS response write failed".to_string())?;
        let _ = tls.flush();
        if !keep_alive || force_close || should_stop() {
            break;
        }
    }
    let _ = tls.sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}

/// Backward-compatible one-shot entry used by older emit shapes.
pub fn jet_http_server_tls_handle_impl(
    cert_pem: &String,
    key_pem: &String,
    stream: TcpStream,
    handler: Box<dyn FnOnce(String) -> String + Send>,
) -> Result<(), String> {
    let mut handler = Some(handler);
    jet_http_server_tls_session_impl(
        cert_pem,
        key_pem,
        stream,
        Box::new(move |raw, _force_close| {
            let response = handler
                .take()
                .ok_or_else(|| "TLS one-shot handler already consumed".to_string())?(
                String::from_utf8_lossy(raw).into_owned(),
            );
            Ok((response.into_bytes(), false))
        }),
        Box::new(|| false),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JetHttpTlsMessageStatus {
    Pending,
    Complete(usize),
    Reject,
}

enum JetHttpTlsBodyState {
    Headers,
    ContentLength(usize),
    Chunked {
        body_start: usize,
        state: JetHttpTlsChunkState,
    },
}

struct JetHttpTlsMessageState {
    header_scan: usize,
    #[cfg(test)]
    inspected: usize,
    body: JetHttpTlsBodyState,
}

impl JetHttpTlsMessageState {
    fn new() -> Self {
        Self {
            header_scan: 0,
            #[cfg(test)]
            inspected: 0,
            body: JetHttpTlsBodyState::Headers,
        }
    }

    fn reading_body(&self) -> bool {
        !matches!(self.body, JetHttpTlsBodyState::Headers)
    }

    #[cfg(test)]
    fn inspected(&self) -> usize {
        self.inspected
            + match &self.body {
                JetHttpTlsBodyState::Chunked { state, .. } => state.inspected,
                _ => 0,
            }
    }

    fn advance(
        &mut self,
        raw: &[u8],
        max_header: usize,
        max_body: usize,
    ) -> JetHttpTlsMessageStatus {
        if matches!(self.body, JetHttpTlsBodyState::Headers) {
            let start = self.header_scan.saturating_sub(3);
            let found = raw.get(start..).and_then(|tail| {
                tail.windows(4)
                    .position(|window| window == b"\r\n\r\n")
            });
            #[cfg(test)]
            {
                self.inspected = self.inspected.saturating_add(match found {
                    Some(position) => position + 1,
                    None => raw.len().saturating_sub(start).saturating_sub(3),
                });
            }
            let Some(position) = found else {
                self.header_scan = raw.len();
                return if raw.len() > max_header.saturating_add(4) {
                    JetHttpTlsMessageStatus::Reject
                } else {
                    JetHttpTlsMessageStatus::Pending
                };
            };
            let header_end = start + position;
            if header_end > max_header {
                return JetHttpTlsMessageStatus::Reject;
            }
            self.body = match jet_http_tls_body_state(&raw[..header_end], header_end + 4, max_body) {
                Some(body) => body,
                None => return JetHttpTlsMessageStatus::Reject,
            };
        }
        match &mut self.body {
            JetHttpTlsBodyState::Headers => JetHttpTlsMessageStatus::Pending,
            JetHttpTlsBodyState::ContentLength(end) => {
                if raw.len() >= *end {
                    JetHttpTlsMessageStatus::Complete(*end)
                } else {
                    JetHttpTlsMessageStatus::Pending
                }
            }
            JetHttpTlsBodyState::Chunked { body_start, state } => {
                match state.advance(&raw[*body_start..], max_body) {
                    JetHttpTlsMessageStatus::Complete(end) => {
                        JetHttpTlsMessageStatus::Complete(*body_start + end)
                    }
                    status => status,
                }
            }
        }
    }
}

fn jet_http_tls_body_state(
    header: &[u8],
    body_start: usize,
    max_body: usize,
) -> Option<JetHttpTlsBodyState> {
    let header = std::str::from_utf8(header).ok()?;
    let mut lines = header.split("\r\n");
    lines.next()?;
    let mut content_length = None;
    let mut transfer_encoding = None;
    for line in lines {
        let (name, value) = line.split_once(':')?;
        let value = value.trim_matches(|character| matches!(character, ' ' | '\t'));
        if name.eq_ignore_ascii_case("content-length") {
            for value in value.split(',') {
                let value = value.trim_matches(|character| matches!(character, ' ' | '\t'));
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                let parsed = value.bytes().try_fold(0usize, |length, byte| {
                    length.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
                })?;
                if content_length.is_some_and(|old| old != parsed) {
                    return None;
                }
                content_length = Some(parsed);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            if transfer_encoding.replace(value).is_some() {
                return None;
            }
        }
    }
    if transfer_encoding.is_some() && content_length.is_some() {
        return None;
    }
    if let Some(encoding) = transfer_encoding {
        if !encoding.eq_ignore_ascii_case("chunked") {
            return None;
        }
        return Some(JetHttpTlsBodyState::Chunked {
            body_start,
            state: JetHttpTlsChunkState::new(),
        });
    }
    let length = content_length.unwrap_or(0);
    if length > max_body {
        return None;
    }
    Some(JetHttpTlsBodyState::ContentLength(body_start.checked_add(length)?))
}

#[derive(Clone, Copy)]
enum JetHttpTlsChunkPhase {
    Size,
    Data(usize),
    FinalCrlf,
}

struct JetHttpTlsChunkState {
    cursor: usize,
    search: usize,
    decoded: usize,
    framing: usize,
    #[cfg(test)]
    inspected: usize,
    phase: JetHttpTlsChunkPhase,
}

impl JetHttpTlsChunkState {
    const MAX_FRAMING: usize = 32 * 1024;

    fn new() -> Self {
        Self {
            cursor: 0,
            search: 0,
            decoded: 0,
            framing: 0,
            #[cfg(test)]
            inspected: 0,
            phase: JetHttpTlsChunkPhase::Size,
        }
    }

    fn add_framing(&mut self, amount: usize) -> bool {
        self.framing = self.framing.saturating_add(amount);
        self.framing <= Self::MAX_FRAMING
    }

    fn advance(&mut self, body: &[u8], max_body: usize) -> JetHttpTlsMessageStatus {
        loop {
            match self.phase {
                JetHttpTlsChunkPhase::Size => {
                    let start = self.search.saturating_sub(1).max(self.cursor);
                    let found = body.get(start..).and_then(|tail| {
                        tail.windows(2).position(|window| window == b"\r\n")
                    });
                    #[cfg(test)]
                    {
                        self.inspected = self.inspected.saturating_add(match found {
                            Some(position) => position + 1,
                            None => body.len().saturating_sub(start).saturating_sub(1),
                        });
                    }
                    let Some(position) = found else {
                        self.search = body.len();
                        return if self
                            .framing
                            .saturating_add(body.len().saturating_sub(self.cursor))
                            > Self::MAX_FRAMING
                        {
                            JetHttpTlsMessageStatus::Reject
                        } else {
                            JetHttpTlsMessageStatus::Pending
                        };
                    };
                    let line_end = start + position;
                    if !self.add_framing(line_end - self.cursor + 2) {
                        return JetHttpTlsMessageStatus::Reject;
                    }
                    let Some(size) = jet_http_tls_chunk_size(&body[self.cursor..line_end]) else {
                        return JetHttpTlsMessageStatus::Reject;
                    };
                    self.cursor = line_end + 2;
                    if size == 0 {
                        self.phase = JetHttpTlsChunkPhase::FinalCrlf;
                    } else {
                        let Some(decoded) = self.decoded.checked_add(size) else {
                            return JetHttpTlsMessageStatus::Reject;
                        };
                        if decoded > max_body {
                            return JetHttpTlsMessageStatus::Reject;
                        }
                        self.decoded = decoded;
                        let Some(end) = self.cursor.checked_add(size) else {
                            return JetHttpTlsMessageStatus::Reject;
                        };
                        self.phase = JetHttpTlsChunkPhase::Data(end);
                    }
                }
                JetHttpTlsChunkPhase::Data(end) => {
                    let Some(crlf) = body.get(end..end.saturating_add(2)) else {
                        return JetHttpTlsMessageStatus::Pending;
                    };
                    if crlf != b"\r\n" || !self.add_framing(2) {
                        return JetHttpTlsMessageStatus::Reject;
                    }
                    self.cursor = end + 2;
                    self.search = self.cursor;
                    self.phase = JetHttpTlsChunkPhase::Size;
                }
                JetHttpTlsChunkPhase::FinalCrlf => {
                    let Some(crlf) = body.get(self.cursor..self.cursor.saturating_add(2)) else {
                        return JetHttpTlsMessageStatus::Pending;
                    };
                    if crlf != b"\r\n" || !self.add_framing(2) {
                        return JetHttpTlsMessageStatus::Reject;
                    }
                    return JetHttpTlsMessageStatus::Complete(self.cursor + 2);
                }
            }
        }
    }
}

fn jet_http_tls_chunk_size(line: &[u8]) -> Option<usize> {
    let digits = line.iter().take_while(|byte| byte.is_ascii_hexdigit()).count();
    let rest = line[digits..]
        .iter()
        .find(|byte| !matches!(byte, b' ' | b'\t'));
    if digits == 0 || rest.is_some_and(|byte| *byte != b';') {
        return None;
    }
    line[..digits].iter().try_fold(0usize, |size, byte| {
        let digit = (*byte as char).to_digit(16)? as usize;
        size.checked_mul(16)?.checked_add(digit)
    })
}
