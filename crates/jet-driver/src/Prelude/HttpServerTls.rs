use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

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

pub fn jet_http_server_tls_handle_impl(
    cert_pem: &String,
    key_pem: &String,
    stream: TcpStream,
    handler: Box<dyn FnOnce(String) -> String + Send>,
) -> Result<(), String> {
    let config = jet_http_server_tls_config(cert_pem, key_pem)?;
    let conn = rustls::ServerConnection::new(config)
        .map_err(|_| "TLS server could not start the handshake".to_string())?;
    let mut tls = rustls::StreamOwned::new(conn, stream);

    let mut buf = vec![0u8; 65536];
    let n = tls
        .read(&mut buf)
        .map_err(|_| "TLS handshake failed before Jet could read the request".to_string())?;
    let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
    let response = handler(raw);
    tls.write_all(response.as_bytes())
        .map_err(|_| "TLS response write failed".to_string())?;
    let _ = tls.flush();
    Ok(())
}
