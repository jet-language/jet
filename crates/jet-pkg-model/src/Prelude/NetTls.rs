use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

struct JetTlsState {
    stream: TlsStream,
    server_name: String,
    pending_write: Option<usize>,
    closing: bool,
}

static JET_NET_TLS_NEXT: AtomicI64 = AtomicI64::new(1);
static JET_NET_TLS_STREAMS: OnceLock<Mutex<BTreeMap<i64, JetTlsState>>> = OnceLock::new();
static JET_NET_TLS_CLOSED: OnceLock<Mutex<std::collections::BTreeSet<i64>>> = OnceLock::new();

fn jet_net_tls_streams() -> &'static Mutex<BTreeMap<i64, JetTlsState>> {
    JET_NET_TLS_STREAMS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn jet_net_tls_closed() -> &'static Mutex<std::collections::BTreeSet<i64>> {
    JET_NET_TLS_CLOSED.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()))
}

fn jet_net_tls_config(custom_ca_pem: Option<&[u8]>) -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs()
        .map_err(|e| format!("TLS could not load system certificate roots: {}", e))?;
    if certs.is_empty() && custom_ca_pem.is_none() {
        return Err("TLS could not find system certificate roots".to_string());
    }
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| format!("TLS could not use a system certificate root: {}", e))?;
    }
    if let Some(pem) = custom_ca_pem {
        let certs = jet_net_tls_pem_certificates(pem)?;
        let mut added = 0usize;
        for cert in certs {
            roots
                .add(cert)
                .map_err(|e| format!("TLS could not use a custom certificate root: {}", e))?;
            added = added
                .checked_add(1)
                .ok_or_else(|| "TLS custom CA certificate count overflow".to_string())?;
        }
        if added == 0 {
            return Err("TLS custom CA PEM did not contain a certificate".to_string());
        }
    }
    Ok(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

fn jet_net_tls_pem_certificates(
    pem: &[u8],
) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let text = std::str::from_utf8(pem)
        .map_err(|_| "TLS custom CA PEM must be UTF-8 text".to_string())?;
    let mut rest = text;
    let mut out = Vec::new();
    while let Some(start) = rest.find(BEGIN) {
        if !rest[..start].trim().is_empty() {
            return Err("TLS custom CA PEM contains data outside certificate blocks".to_string());
        }
        rest = &rest[start + BEGIN.len()..];
        let stop = rest.find(END)
            .ok_or_else(|| "TLS custom CA PEM has an unterminated certificate".to_string())?;
        let der = jet_net_tls_pem_base64(&rest[..stop])?;
        out.push(rustls::pki_types::CertificateDer::from(der));
        rest = &rest[stop + END.len()..];
    }
    if out.is_empty() || !rest.trim().is_empty() {
        return Err("TLS custom CA PEM must contain only certificate blocks".to_string());
    }
    Ok(out)
}

fn jet_net_tls_pem_base64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut quartet = [0u8; 4];
    let mut used = 0usize;
    let mut padded = false;
    for byte in text.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if padded { return Err("TLS custom CA PEM has data after base64 padding".to_string()); }
        quartet[used] = byte;
        used += 1;
        if used == 4 {
            let mut values = [0u8; 4];
            let mut pads = 0usize;
            for index in 0..4 {
                if quartet[index] == b'=' { pads += 1; }
                else {
                    if pads != 0 { return Err("TLS custom CA PEM has invalid base64 padding".to_string()); }
                    values[index] = match quartet[index] {
                        b'A'..=b'Z' => quartet[index] - b'A',
                        b'a'..=b'z' => quartet[index] - b'a' + 26,
                        b'0'..=b'9' => quartet[index] - b'0' + 52,
                        b'+' => 62,
                        b'/' => 63,
                        _ => return Err("TLS custom CA PEM contains invalid base64".to_string()),
                    };
                }
            }
            if pads > 2 { return Err("TLS custom CA PEM has invalid base64 padding".to_string()); }
            out.push(values[0] << 2 | values[1] >> 4);
            if pads < 2 { out.push(values[1] << 4 | values[2] >> 2); }
            if pads == 0 { out.push(values[2] << 6 | values[3]); }
            padded = pads != 0;
            used = 0;
        }
    }
    if used != 0 || out.is_empty() { return Err("TLS custom CA PEM contains incomplete base64".to_string()); }
    Ok(out)
}

fn jet_net_tls_begin_inner(
    stream: TcpStream,
    server_name: &String,
    custom_ca_pem: Option<&[u8]>,
) -> Result<i64, String> {
    static RUSTLS_PROVIDER: std::sync::Once = std::sync::Once::new();
    RUSTLS_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let name = rustls::pki_types::ServerName::try_from(server_name.clone())
        .map_err(|_| format!("invalid TLS server name `{}`", server_name))?;
    stream
        .set_nonblocking(true)
        .map_err(|e| format!("TLS could not configure the TCP stream: {}", e))?;
    let conn = rustls::ClientConnection::new(jet_net_tls_config(custom_ca_pem)?, name)
        .map_err(|e| format!("TLS handshake with `{}` failed: {}", server_name, e))?;
    let tls = rustls::StreamOwned::new(conn, stream);
    let id = JET_NET_TLS_NEXT.fetch_add(1, Ordering::Relaxed);
    jet_net_tls_streams().lock().unwrap().insert(
        id,
        JetTlsState {
            stream: tls,
            server_name: server_name.clone(),
            pending_write: None,
            closing: false,
        },
    );
    Ok(id)
}

/// Email runtime handshake seam: caller polls this between ambient cancellation
/// and deadline checks. No bridge worker or hidden retry exists.
pub fn jet_net_tls_begin_impl(stream: TcpStream, server_name: &String) -> Result<i64, String> {
    jet_net_tls_begin_inner(stream, server_name, None)
}

pub fn jet_net_tls_begin_with_ca_impl(
    stream: TcpStream,
    server_name: &String,
    custom_ca_pem: &Vec<u8>,
) -> Result<i64, String> {
    jet_net_tls_begin_inner(stream, server_name, Some(custom_ca_pem))
}

pub fn jet_net_tls_handshake_step_impl(id: i64) -> Result<bool, String> {
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let state = streams
        .get_mut(&id)
        .ok_or_else(|| "TLS stream is closed".to_string())?;
    let tls = &mut state.stream;
        if !tls.conn.is_handshaking() {
            return Ok(true);
        }
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) if tls.conn.is_handshaking() => Ok(false),
            Ok(_) => Ok(true),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(false),
            Err(error) => Err(format!(
                "TLS handshake with `{}` failed: {}",
                state.server_name, error
            )),
        }
}

pub fn jet_net_tls_wants_impl(id: i64) -> Result<(bool, bool), String> {
    let streams = jet_net_tls_streams().lock().unwrap();
    let tls = &streams
        .get(&id)
        .ok_or_else(|| "TLS stream is closed".to_string())?
        .stream;
    Ok((tls.conn.wants_read(), tls.conn.wants_write()))
}

pub fn jet_net_tls_abort_impl(id: i64) {
    jet_net_tls_streams().lock().unwrap().remove(&id);
    jet_net_tls_closed().lock().unwrap().insert(id);
}

pub fn jet_net_tls_set_poll_timeout_impl(id: i64, millis: i64) -> Result<(), String> {
    if !(1..=1000).contains(&millis) {
        return Err("TLS poll timeout must be between 1 and 1000 milliseconds".to_string());
    }
    {
        let mut streams = jet_net_tls_streams().lock().unwrap();
        let tls = &mut streams
            .get_mut(&id)
            .ok_or_else(|| "TLS stream is closed".to_string())?
            .stream;
        let timeout = Some(std::time::Duration::from_millis(millis as u64));
        tls.sock.set_read_timeout(timeout).map_err(|e| format!("TLS could not set read timeout: {}", e))?;
        tls.sock.set_write_timeout(timeout).map_err(|e| format!("TLS could not set write timeout: {}", e))
    }
}

pub fn jet_net_tls_read_impl(id: i64) -> Result<String, String> {
    let bytes = jet_net_tls_read_bytes_impl(id, 8192)?;
    String::from_utf8(bytes).map_err(|e| format!("TLS read: invalid UTF-8: {}", e))
}

pub fn jet_net_tls_read_bytes_impl(id: i64, limit: i64) -> Result<Vec<u8>, String> {
    match jet_net_tls_read_step_impl(id, limit)? {
        Some(bytes) => Ok(bytes),
        None => Err("TLS read would block".to_string()),
    }
}

pub fn jet_net_tls_read_step_impl(id: i64, limit: i64) -> Result<Option<Vec<u8>>, String> {
    if limit <= 0 {
        return Err("TLS read limit must be positive".to_string());
    }
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let stream = &mut streams
        .get_mut(&id)
        .ok_or_else(|| "TLS stream is closed".to_string())?
        .stream;
    let mut bytes = vec![0u8; std::cmp::min(limit as usize, 16 * 1024 * 1024)];
    match stream.read(&mut bytes) {
        Ok(n) => {
            bytes.truncate(n);
            Ok(Some(bytes))
        }
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(None),
        Err(error) => Err(format!("TLS read failed: {}", error)),
    }
}

pub fn jet_net_tls_write_impl(id: i64, data: &String) -> Result<(), String> {
    jet_net_tls_write_all_bytes_impl(id, &data.as_bytes().to_vec())
}

pub fn jet_net_tls_write_bytes_impl(id: i64, data: &Vec<u8>) -> Result<i64, String> {
    match jet_net_tls_write_step_impl(id, data)? {
        Some(count) => Ok(count),
        None => Err("TLS write would block".to_string()),
    }
}

pub fn jet_net_tls_write_all_bytes_impl(id: i64, data: &Vec<u8>) -> Result<(), String> {
    match jet_net_tls_write_step_impl(id, data)? {
        Some(count) if count as usize == data.len() => Ok(()),
        Some(count) => Err(format!("TLS write accepted {} of {} bytes", count, data.len())),
        None => Err("TLS write would block".to_string()),
    }
}

pub fn jet_net_tls_write_step_impl(id: i64, data: &Vec<u8>) -> Result<Option<i64>, String> {
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let state = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
    if state.closing {
        return Err("TLS stream is closed".to_string());
    }
    if state.pending_write.is_none() {
        match state.stream.write(data) {
            Ok(count) => state.pending_write = Some(count),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => return Ok(None),
            Err(error) => return Err(format!("TLS write failed: {}", error)),
        }
    }
    match state.stream.flush() {
        Ok(()) => Ok(state.pending_write.take().map(|count| count as i64)),
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(None),
        Err(error) => {
            state.pending_write = None;
            Err(format!("TLS write failed: {}", error))
        }
    }
}

fn jet_net_tls_flush_close_notify<S: Read + Write>(
    stream: &mut rustls::StreamOwned<rustls::ClientConnection, S>,
) -> Result<(), String> {
    stream.conn.send_close_notify();
    stream
        .flush()
        .map_err(|e| format!("TLS close-notify flush failed: {}", e))
}

pub fn jet_net_tls_close_impl(id: i64) -> Result<(), String> {
    match jet_net_tls_close_step_impl(id)? {
        true => Ok(()),
        false => Err("TLS close would block".to_string()),
    }
}

pub fn jet_net_tls_close_step_impl(id: i64) -> Result<bool, String> {
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let Some(state) = streams.get_mut(&id) else {
        return if jet_net_tls_closed().lock().unwrap().contains(&id) {
            Ok(true)
        } else {
            Err("TLS stream is closed".to_string())
        };
    };
    if !state.closing {
        state.stream.conn.send_close_notify();
        state.closing = true;
    }
    match state.stream.flush() {
        Ok(()) => {
            match state.stream.sock.shutdown(std::net::Shutdown::Write) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
                Err(error) => return Err(format!("TLS socket write shutdown failed: {}", error)),
            }
            streams.remove(&id);
            jet_net_tls_closed().lock().unwrap().insert(id);
            Ok(true)
        }
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(false),
        Err(error) => Err(format!("TLS close-notify flush failed: {}", error)),
    }
}
