use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

type TlsStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

thread_local! {
    static JET_NET_TLS_STREAMS: RefCell<BTreeMap<i64, TlsStream>> = RefCell::new(BTreeMap::new());
    static JET_NET_TLS_CLOSED: RefCell<std::collections::BTreeSet<i64>> = RefCell::new(std::collections::BTreeSet::new());
}

static JET_NET_TLS_NEXT: AtomicI64 = AtomicI64::new(1);

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

fn jet_net_tls_connect_inner(
    stream: TcpStream,
    server_name: &String,
    custom_ca_pem: Option<&[u8]>,
) -> Result<i64, String> {
    let id = jet_net_tls_begin_inner(stream, server_name, custom_ca_pem)?;
    loop {
        if jet_net_tls_handshake_step_impl(id)? { return Ok(id); }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
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
    JET_NET_TLS_STREAMS.with(|cell| {
        cell.borrow_mut().insert(id, tls);
    });
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
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let tls = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
        if !tls.conn.is_handshaking() {
            tls.sock.set_nonblocking(false)
                .map_err(|e| format!("TLS could not configure the TCP stream: {}", e))?;
            return Ok(true);
        }
        match tls.conn.complete_io(&mut tls.sock) {
            Ok(_) if tls.conn.is_handshaking() => Ok(false),
            Ok(_) => {
                tls.sock.set_nonblocking(false)
                    .map_err(|e| format!("TLS could not configure the TCP stream: {}", e))?;
                Ok(true)
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => Ok(false),
            Err(error) => Err(format!("TLS handshake failed: {}", error)),
        }
    })
}

pub fn jet_net_tls_set_poll_timeout_impl(id: i64, millis: i64) -> Result<(), String> {
    if !(1..=1000).contains(&millis) {
        return Err("TLS poll timeout must be between 1 and 1000 milliseconds".to_string());
    }
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let tls = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
        let timeout = Some(std::time::Duration::from_millis(millis as u64));
        tls.sock.set_read_timeout(timeout).map_err(|e| format!("TLS could not set read timeout: {}", e))?;
        tls.sock.set_write_timeout(timeout).map_err(|e| format!("TLS could not set write timeout: {}", e))
    })
}

pub fn jet_net_tls_connect_impl(stream: TcpStream, server_name: &String) -> Result<i64, String> {
    jet_net_tls_connect_inner(stream, server_name, None)
}

/// D-EMAIL-SMTP-CONFIG1=A: verified SystemPlusCa connection. Custom roots are
/// appended to system roots; rustls' normal DNS-name verifier remains active.
pub fn jet_net_tls_connect_with_ca_impl(
    stream: TcpStream,
    server_name: &String,
    custom_ca_pem: &Vec<u8>,
) -> Result<i64, String> {
    jet_net_tls_connect_inner(stream, server_name, Some(custom_ca_pem))
}

pub fn jet_net_tls_read_impl(id: i64) -> Result<String, String> {
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let stream = streams
            .get_mut(&id)
            .ok_or_else(|| "TLS stream is closed".to_string())?;
        let mut buf = [0u8; 8192];
        stream
            .read(&mut buf)
            .map_err(|e| format!("TLS read failed: {}", e))
            .and_then(|n| {
                String::from_utf8(buf[..n].to_vec())
                    .map_err(|e| format!("TLS read: invalid UTF-8: {}", e))
            })
    })
}

pub fn jet_net_tls_read_bytes_impl(id: i64, limit: i64) -> Result<Vec<u8>, String> {
    if limit < 0 {
        return Err("TLS read limit must be non-negative".to_string());
    }
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let stream = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
        let mut bytes = vec![0u8; std::cmp::min(limit as usize, 16 * 1024 * 1024)];
        if bytes.is_empty() { return Ok(bytes); }
        let n = stream.read(&mut bytes).map_err(|e| format!("TLS read failed: {}", e))?;
        bytes.truncate(n);
        Ok(bytes)
    })
}

pub fn jet_net_tls_write_impl(id: i64, data: &String) -> Result<(), String> {
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let stream = streams
            .get_mut(&id)
            .ok_or_else(|| "TLS stream is closed".to_string())?;
        stream
            .write_all(data.as_bytes())
            .map_err(|e| format!("TLS write failed: {}", e))
    })
}

pub fn jet_net_tls_write_bytes_impl(id: i64, data: &Vec<u8>) -> Result<i64, String> {
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let stream = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
        stream.write(data).map(|n| n as i64).map_err(|e| format!("TLS write failed: {}", e))
    })
}

pub fn jet_net_tls_write_all_bytes_impl(id: i64, data: &Vec<u8>) -> Result<(), String> {
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        let stream = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
        stream.write_all(data).map_err(|e| format!("TLS write failed: {}", e))
    })
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
    JET_NET_TLS_STREAMS.with(|cell| {
        let mut streams = cell.borrow_mut();
        if let Some(stream) = streams.get_mut(&id) {
            jet_net_tls_flush_close_notify(stream)?;
            streams.remove(&id);
            JET_NET_TLS_CLOSED.with(|closed| { closed.borrow_mut().insert(id); });
            return Ok(());
        }
        if JET_NET_TLS_CLOSED.with(|closed| closed.borrow().contains(&id)) { Ok(()) }
        else { Err("TLS stream is closed".to_string()) }
    })
}
