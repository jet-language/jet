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
pub fn jet_http_server_tls_session_impl(
    cert_pem: &String,
    key_pem: &String,
    stream: TcpStream,
    mut on_request: Box<dyn FnMut(&[u8]) -> Result<(Vec<u8>, bool), String> + Send>,
    should_stop: Box<dyn Fn() -> bool + Send>,
) -> Result<(), String> {
    const MAX_REQUESTS: usize = 1000;
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

    for request_index in 0..MAX_REQUESTS {
        if request_index > 0 && should_stop() {
            break;
        }
        let between = request_index > 0;
        let idle = if between { IDLE_TIMEOUT } else { HEADER_TIMEOUT };
        let started = std::time::Instant::now();
        let mut pending = Vec::new();
        let mut reading_body = false;
        let request = loop {
            if between && pending.is_empty() && should_stop() {
                return Ok(());
            }
            if let Some(end) = jet_http_tls_message_end(&pending, MAX_BODY_BYTES)? {
                break pending.drain(..end).collect::<Vec<u8>>();
            }
            if pending.len() > MAX_HEADER_BYTES
                && jet_http_tls_header_end(&pending).is_none()
            {
                return Err("TLS request headers are too large".to_string());
            }
            let deadline = if reading_body {
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
                    if jet_http_tls_header_end(&pending).is_some() {
                        reading_body = true;
                    }
                    pending.extend_from_slice(&buf[..n]);
                    if jet_http_tls_header_end(&pending).is_some() {
                        reading_body = true;
                    }
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

        let (response, keep_alive) = on_request(&request)?;
        tls.sock
            .set_write_timeout(Some(BODY_TIMEOUT))
            .map_err(|_| "TLS write timeout setup failed".to_string())?;
        tls.write_all(&response)
            .map_err(|_| "TLS response write failed".to_string())?;
        let _ = tls.flush();
        if !keep_alive || should_stop() {
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
        Box::new(move |raw| {
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

fn jet_http_tls_header_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4).position(|window| window == b"\r\n\r\n")
}

fn jet_http_tls_message_end(raw: &[u8], max_body: usize) -> Result<Option<usize>, String> {
    let Some(header_end) = jet_http_tls_header_end(raw) else {
        return Ok(None);
    };
    let headers = std::str::from_utf8(&raw[..header_end])
        .map_err(|_| "TLS request headers are not valid UTF-8".to_string())?;
    let mut content_length = None;
    let mut chunked = false;
    for line in headers.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim_start_matches(|c| matches!(c, ' ' | '\t'));
        if name.eq_ignore_ascii_case("content-length") {
            let parsed = value
                .parse::<usize>()
                .map_err(|_| "TLS content-length is malformed".to_string())?;
            if content_length.is_some_and(|old| old != parsed) {
                return Err("TLS conflicting content-length headers".to_string());
            }
            content_length = Some(parsed);
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.split(',').any(|part| part.trim().eq_ignore_ascii_case("chunked"));
        }
    }
    let body_start = header_end + 4;
    if chunked {
        return jet_http_tls_chunked_end(&raw[body_start..], max_body)
            .map(|end| end.map(|body_end| body_start + body_end));
    }
    let length = content_length.unwrap_or(0);
    if length > max_body {
        return Err("TLS request body is too large".to_string());
    }
    let end = body_start + length;
    Ok((raw.len() >= end).then_some(end))
}

fn jet_http_tls_chunked_end(body: &[u8], max_body: usize) -> Result<Option<usize>, String> {
    let mut cursor = 0usize;
    let mut decoded = 0usize;
    loop {
        let Some(line_len) = body[cursor..]
            .windows(2)
            .position(|bytes| bytes == b"\r\n")
        else {
            return Ok(None);
        };
        let line_end = cursor + line_len;
        let size_text = std::str::from_utf8(&body[cursor..line_end])
            .map_err(|_| "TLS chunk size is malformed".to_string())?;
        let size_text = size_text.split(';').next().unwrap_or(size_text).trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| "TLS chunk size is malformed".to_string())?;
        cursor = line_end + 2;
        if size == 0 {
            if body.len().saturating_sub(cursor) < 2 {
                return Ok(None);
            }
            if &body[cursor..cursor + 2] != b"\r\n" {
                return Err("TLS request trailers are not supported".to_string());
            }
            return Ok(Some(cursor + 2));
        }
        decoded = decoded
            .checked_add(size)
            .ok_or_else(|| "TLS request body is too large".to_string())?;
        if decoded > max_body {
            return Err("TLS request body is too large".to_string());
        }
        if body.len().saturating_sub(cursor) < size + 2 {
            return Ok(None);
        }
        cursor += size;
        if &body[cursor..cursor + 2] != b"\r\n" {
            return Err("TLS chunk data is not followed by CRLF".to_string());
        }
        cursor += 2;
    }
}
