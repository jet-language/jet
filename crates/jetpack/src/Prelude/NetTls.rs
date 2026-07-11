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

fn jet_net_tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs()
        .map_err(|e| format!("TLS could not load system certificate roots: {}", e))?;
    if certs.is_empty() {
        return Err("TLS could not find system certificate roots".to_string());
    }
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| format!("TLS could not use a system certificate root: {}", e))?;
    }
    Ok(Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    ))
}

pub fn jet_net_tls_connect_impl(stream: TcpStream, server_name: &String) -> Result<i64, String> {
    static RUSTLS_PROVIDER: std::sync::Once = std::sync::Once::new();
    RUSTLS_PROVIDER.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    let name = rustls::pki_types::ServerName::try_from(server_name.clone())
        .map_err(|_| format!("invalid TLS server name `{}`", server_name))?;
    let conn = rustls::ClientConnection::new(jet_net_tls_config()?, name)
        .map_err(|e| format!("TLS handshake with `{}` failed: {}", server_name, e))?;
    let mut tls = rustls::StreamOwned::new(conn, stream);
    tls.flush()
        .map_err(|e| format!("TLS handshake with `{}` failed: {}", server_name, e))?;
    let id = JET_NET_TLS_NEXT.fetch_add(1, Ordering::Relaxed);
    JET_NET_TLS_STREAMS.with(|cell| {
        cell.borrow_mut().insert(id, tls);
    });
    Ok(id)
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

pub fn jet_net_tls_close_impl(id: i64) -> Result<(), String> {
    JET_NET_TLS_STREAMS.with(|cell| {
        if let Some(mut stream) = cell.borrow_mut().remove(&id) {
            stream.conn.send_close_notify();
            let _ = stream.flush();
            JET_NET_TLS_CLOSED.with(|closed| { closed.borrow_mut().insert(id); });
            return Ok(());
        }
        if JET_NET_TLS_CLOSED.with(|closed| closed.borrow().contains(&id)) { Ok(()) }
        else { Err("TLS stream is closed".to_string()) }
    })
}
