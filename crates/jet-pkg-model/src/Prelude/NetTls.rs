use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type TLSStream = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

struct JetTLSState {
    stream: TLSStream,
    server_name: String,
    pending_read: Vec<u8>,
    read_eof: bool,
    pending_write: Option<usize>,
    write_closing: bool,
    write_closed: bool,
}

pub type JetTLSPeerSnapshot = (
    String,
    Vec<Vec<u8>>,
    Vec<Vec<u8>>,
    Vec<Vec<String>>,
    Vec<i64>,
    Vec<i64>,
    Vec<String>,
    Vec<String>,
    String,
    i64,
);

static JET_NET_TLS_NEXT: AtomicI64 = AtomicI64::new(1);
static JET_NET_TLS_STREAMS: OnceLock<Mutex<BTreeMap<i64, JetTLSState>>> = OnceLock::new();
static JET_NET_TLS_CLOSED: OnceLock<Mutex<std::collections::BTreeSet<i64>>> = OnceLock::new();

fn jet_net_tls_streams() -> &'static Mutex<BTreeMap<i64, JetTLSState>> {
    JET_NET_TLS_STREAMS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn jet_net_tls_closed() -> &'static Mutex<std::collections::BTreeSet<i64>> {
    JET_NET_TLS_CLOSED.get_or_init(|| Mutex::new(std::collections::BTreeSet::new()))
}

fn jet_net_tls_config(
    trust_mode: i64,
    custom_ca_pem: Option<&[u8]>,
    identity: Option<(&[u8], &[u8])>,
    min_version: i64,
    max_version: i64,
    alpn: &[String],
) -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    if matches!(trust_mode, 0 | 1) {
        let certs = rustls_native_certs::load_native_certs()
            .map_err(|e| format!("TLS could not load system certificate roots: {}", e))?;
        if certs.is_empty() && trust_mode == 0 {
            return Err("TLS could not find system certificate roots".to_string());
        }
        for cert in certs {
            roots
                .add(cert)
                .map_err(|e| format!("TLS could not use a system certificate root: {}", e))?;
        }
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
    if !(0..=2).contains(&trust_mode) {
        return Err("TLS trust mode is invalid".to_string());
    }
    if min_version > max_version || !matches!(min_version, 12 | 13) || !matches!(max_version, 12 | 13) {
        return Err("TLS version bounds must be between Tls12 and Tls13 with min <= max".to_string());
    }
    let versions: Vec<&'static rustls::SupportedProtocolVersion> = match (min_version, max_version) {
        (12, 12) => vec![&rustls::version::TLS12],
        (12, 13) => vec![&rustls::version::TLS13, &rustls::version::TLS12],
        (13, 13) => vec![&rustls::version::TLS13],
        _ => unreachable!(),
    };
    let builder = rustls::ClientConfig::builder_with_protocol_versions(&versions)
        .with_root_certificates(roots);
    let mut config = if let Some((cert_pem, key_pem)) = identity {
        let certs = jet_net_tls_pem_certificates(cert_pem)?;
        let key = jet_net_tls_private_key(key_pem)?;
        builder.with_client_auth_cert(certs, key)
            .map_err(|e| format!("TLS client identity is invalid: {}", e))?
    } else {
        builder.with_no_client_auth()
    };
    config.alpn_protocols = alpn
        .iter()
        .map(|protocol| {
            if protocol.is_empty() || protocol.len() > u8::MAX as usize {
                return Err("TLS ALPN protocols must contain 1 to 255 bytes".to_string());
            }
            Ok(protocol.as_bytes().to_vec())
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Arc::new(config))
}

pub fn jet_net_tls_validate_roots_impl(pem: &Vec<u8>) -> Result<(), String> {
    let certs = jet_net_tls_pem_certificates(pem)?;
    let mut roots = rustls::RootCertStore::empty();
    for cert in certs {
        roots.add(cert).map_err(|e| format!("TLS could not use a custom certificate root: {}", e))?;
    }
    Ok(())
}

fn jet_net_tls_private_key(pem: &[u8]) -> Result<rustls::pki_types::PrivateKeyDer<'static>, String> {
    let text = std::str::from_utf8(pem)
        .map_err(|_| "TLS private key PEM must be UTF-8 text".to_string())?
        .trim();
    for (label, kind) in [("PRIVATE KEY", 8), ("RSA PRIVATE KEY", 1), ("EC PRIVATE KEY", 2)] {
        let begin = format!("-----BEGIN {label}-----");
        let end = format!("-----END {label}-----");
        if let Some(body) = text.strip_prefix(&begin).and_then(|rest| rest.strip_suffix(&end)) {
            let der = jet_net_tls_pem_base64(body)?;
            return Ok(match kind {
                8 => rustls::pki_types::PrivateKeyDer::Pkcs8(
                    rustls::pki_types::PrivatePkcs8KeyDer::from(der),
                ),
                1 => rustls::pki_types::PrivateKeyDer::Pkcs1(
                    rustls::pki_types::PrivatePkcs1KeyDer::from(der),
                ),
                _ => rustls::pki_types::PrivateKeyDer::Sec1(
                    rustls::pki_types::PrivateSec1KeyDer::from(der),
                ),
            });
        }
    }
    Err("TLS private key PEM must contain exactly one PKCS#8, PKCS#1, or SEC1 key".to_string())
}

pub fn jet_net_tls_validate_identity_impl(cert_pem: &Vec<u8>, key_pem: &Vec<u8>) -> Result<(), String> {
    let certs = jet_net_tls_pem_certificates(cert_pem)?;
    let leaf = certs.first().ok_or_else(|| "TLS client certificate chain is empty".to_string())?;
    let key = jet_net_tls_private_key(key_pem)?;
    let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|e| format!("TLS private key is unusable: {}", e))?;
    let key_spki = signing_key.public_key()
        .ok_or_else(|| "TLS private key cannot expose its public key for matching".to_string())?;
    let cert_spki = jet_net_tls_certificate_parts(leaf.as_ref())?.0;
    if key_spki.as_ref() != cert_spki.as_slice() {
        return Err("TLS client certificate does not match the private key".to_string());
    }
    rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_client_auth_cert(certs, key)
        .map_err(|e| format!("TLS client identity is invalid: {}", e))?;
    Ok(())
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
    trust_mode: i64,
    custom_ca_pem: Option<&[u8]>,
    identity: Option<(&[u8], &[u8])>,
    min_version: i64,
    max_version: i64,
    alpn: &[String],
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
    let conn = rustls::ClientConnection::new(
        jet_net_tls_config(trust_mode, custom_ca_pem, identity, min_version, max_version, alpn)?, name,
    )
        .map_err(|e| format!("TLS handshake with `{}` failed: {}", server_name, e))?;
    let tls = rustls::StreamOwned::new(conn, stream);
    let id = JET_NET_TLS_NEXT.fetch_add(1, Ordering::Relaxed);
    jet_net_tls_streams().lock().unwrap().insert(
        id,
        JetTLSState {
            stream: tls,
            server_name: server_name.clone(),
            pending_read: Vec::new(),
            read_eof: false,
            pending_write: None,
            write_closing: false,
            write_closed: false,
        },
    );
    Ok(id)
}

/// Email runtime handshake seam: caller polls this between ambient cancellation
/// and deadline checks. No bridge worker or hidden retry exists.
pub fn jet_net_tls_begin_impl(stream: TcpStream, server_name: &String) -> Result<i64, String> {
    jet_net_tls_begin_inner(stream, server_name, 0, None, None, 12, 13, &[])
}

pub fn jet_net_tls_begin_config_impl(
    stream: TcpStream,
    server_name: &String,
    trust_mode: i64,
    roots: &Vec<u8>,
    cert_chain: &Vec<u8>,
    private_key: &Vec<u8>,
    min_version: i64,
    max_version: i64,
    alpn: &Vec<String>,
) -> Result<i64, String> {
    let custom = (trust_mode != 0).then_some(roots.as_slice());
    let identity = (!cert_chain.is_empty()).then_some((cert_chain.as_slice(), private_key.as_slice()));
    jet_net_tls_begin_inner(stream, server_name, trust_mode, custom, identity, min_version, max_version, alpn)
}

pub fn jet_net_tls_begin_with_ca_impl(
    stream: TcpStream,
    server_name: &String,
    custom_ca_pem: &Vec<u8>,
) -> Result<i64, String> {
    jet_net_tls_begin_inner(stream, server_name, 1, Some(custom_ca_pem), None, 12, 13, &[])
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

pub fn jet_net_tls_read_ready_impl(id: i64) -> Result<bool, String> {
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let state = streams
        .get_mut(&id)
        .ok_or_else(|| "TLS stream is closed".to_string())?;
    if !state.pending_read.is_empty() || state.read_eof {
        return Ok(true);
    }
    let mut byte = [0u8; 1];
    match state.stream.read(&mut byte) {
        Ok(0) => {
            state.read_eof = true;
            Ok(true)
        }
        Ok(n) => {
            state.pending_read.extend_from_slice(&byte[..n]);
            Ok(true)
        }
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            Err(format!("TLS protocol truncation: {}", error))
        }
        Err(error) => Err(format!("TLS read readiness failed: {}", error)),
    }
}

pub fn jet_net_tls_peer_identity_impl(id: i64) -> Result<JetTLSPeerSnapshot, String> {
    let streams = jet_net_tls_streams().lock().unwrap();
    let state = streams.get(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
    if state.stream.conn.is_handshaking() {
        return Err("TLS peer identity is unavailable before verification".to_string());
    }
    let chain = state.stream.conn.peer_certificates()
        .ok_or_else(|| "TLS verified peer did not provide a certificate chain".to_string())?;
    let mut ders = Vec::with_capacity(chain.len());
    let mut spkis = Vec::with_capacity(chain.len());
    let mut dns_names = Vec::with_capacity(chain.len());
    let mut valid_from = Vec::with_capacity(chain.len());
    let mut valid_until = Vec::with_capacity(chain.len());
    let mut subjects = Vec::with_capacity(chain.len());
    let mut issuers = Vec::with_capacity(chain.len());
    for cert in chain {
        let (spki, names, from, until, subject, issuer) = jet_net_tls_certificate_parts(cert.as_ref())?;
        ders.push(cert.as_ref().to_vec());
        spkis.push(spki);
        dns_names.push(names);
        valid_from.push(from);
        valid_until.push(until);
        subjects.push(subject);
        issuers.push(issuer);
    }
    let cipher_suite = state
        .stream
        .conn
        .negotiated_cipher_suite()
        .ok_or_else(|| "TLS verified peer did not expose a negotiated cipher suite".to_string())?;
    let cipher_suite = format!("{:?}", cipher_suite.suite());
    let tls_version = match state.stream.conn.protocol_version() {
        Some(rustls::ProtocolVersion::TLSv1_2) => 12,
        Some(rustls::ProtocolVersion::TLSv1_3) => 13,
        Some(version) => {
            return Err(format!(
                "TLS verified peer negotiated unsupported protocol version: {:?}",
                version
            ));
        }
        None => return Err("TLS verified peer did not expose a negotiated protocol version".to_string()),
    };
    Ok((
        state.server_name.clone(),
        ders,
        spkis,
        dns_names,
        valid_from,
        valid_until,
        subjects,
        issuers,
        cipher_suite,
        tls_version,
    ))
}

fn jet_net_tls_der_element(input: &[u8], pos: &mut usize) -> Result<(u8, usize, usize, usize, usize), String> {
    let start = *pos;
    let tag = *input.get(*pos).ok_or_else(|| "TLS certificate DER is truncated".to_string())?;
    *pos += 1;
    let first = *input.get(*pos).ok_or_else(|| "TLS certificate DER length is truncated".to_string())?;
    *pos += 1;
    let len = if first & 0x80 == 0 {
        first as usize
    } else {
        let count = (first & 0x7f) as usize;
        if count == 0 || count > std::mem::size_of::<usize>() {
            return Err("TLS certificate DER has an invalid length".to_string());
        }
        let mut len = 0usize;
        for _ in 0..count {
            len = len.checked_mul(256)
                .and_then(|n| input.get(*pos).map(|byte| n + *byte as usize))
                .ok_or_else(|| "TLS certificate DER length overflow".to_string())?;
            *pos += 1;
        }
        len
    };
    let content = *pos;
    let end = content.checked_add(len).filter(|end| *end <= input.len())
        .ok_or_else(|| "TLS certificate DER value is truncated".to_string())?;
    *pos = end;
    Ok((tag, content, end, start, end))
}

fn jet_net_tls_der_expect(input: &[u8], pos: &mut usize, expected: u8) -> Result<(usize, usize, usize, usize), String> {
    let (tag, content, end, start, total_end) = jet_net_tls_der_element(input, pos)?;
    if tag != expected {
        return Err(format!("TLS certificate DER expected tag {expected:#x}, found {tag:#x}"));
    }
    Ok((content, end, start, total_end))
}

fn jet_net_tls_certificate_parts(der: &[u8]) -> Result<(Vec<u8>, Vec<String>, i64, i64, String, String), String> {
    let mut pos = 0;
    let (outer_start, outer_end, _, _) = jet_net_tls_der_expect(der, &mut pos, 0x30)?;
    if outer_end != der.len() { return Err("TLS certificate DER has trailing data".to_string()); }
    let mut outer = outer_start;
    let (tbs_start, tbs_end, _, _) = jet_net_tls_der_expect(der, &mut outer, 0x30)?;
    let mut p = tbs_start;
    if der.get(p) == Some(&0xa0) { let _ = jet_net_tls_der_element(der, &mut p)?; }
    let _serial = jet_net_tls_der_element(der, &mut p)?;
    let _signature = jet_net_tls_der_element(der, &mut p)?;
    let (issuer_start, issuer_end, _, _) = jet_net_tls_der_expect(der, &mut p, 0x30)?;
    let issuer = jet_net_tls_name(der, issuer_start, issuer_end)?;
    let (validity_start, validity_end, _, _) = jet_net_tls_der_expect(der, &mut p, 0x30)?;
    let mut validity = validity_start;
    let (from_tag, from_start, from_end, _, _) = jet_net_tls_der_element(der, &mut validity)?;
    let (until_tag, until_start, until_end, _, _) = jet_net_tls_der_element(der, &mut validity)?;
    if validity != validity_end { return Err("TLS certificate validity has extra fields".to_string()); }
    let valid_from = jet_net_tls_time(from_tag, &der[from_start..from_end])?;
    let valid_until = jet_net_tls_time(until_tag, &der[until_start..until_end])?;
    let (subject_start, subject_end, _, _) = jet_net_tls_der_expect(der, &mut p, 0x30)?;
    let subject = jet_net_tls_name(der, subject_start, subject_end)?;
    let (_, _, spki_start, spki_end) = jet_net_tls_der_expect(der, &mut p, 0x30)?;
    let spki = der[spki_start..spki_end].to_vec();
    let mut dns_names = Vec::new();
    while p < tbs_end {
        let (tag, content, end, _, _) = jet_net_tls_der_element(der, &mut p)?;
        if tag == 0xa3 {
            let mut ex = content;
            let (seq_start, seq_end, _, _) = jet_net_tls_der_expect(der, &mut ex, 0x30)?;
            let mut ext = seq_start;
            while ext < seq_end {
                let (one_start, one_end, _, _) = jet_net_tls_der_expect(der, &mut ext, 0x30)?;
                let mut one = one_start;
                let (oid_start, oid_end, _, _) = jet_net_tls_der_expect(der, &mut one, 0x06)?;
                if der.get(one) == Some(&0x01) { let _ = jet_net_tls_der_element(der, &mut one)?; }
                let (value_start, value_end, _, _) = jet_net_tls_der_expect(der, &mut one, 0x04)?;
                if &der[oid_start..oid_end] == [0x55, 0x1d, 0x11] {
                    dns_names = jet_net_tls_dns_names(&der[value_start..value_end])?;
                }
                if one != one_end { return Err("TLS certificate extension has trailing fields".to_string()); }
            }
            if ex != end { return Err("TLS certificate extensions have trailing data".to_string()); }
        }
    }
    Ok((spki, dns_names, valid_from, valid_until, subject, issuer))
}

fn jet_net_tls_dns_names(value: &[u8]) -> Result<Vec<String>, String> {
    let mut pos = 0;
    let (start, end, _, _) = jet_net_tls_der_expect(value, &mut pos, 0x30)?;
    if pos != value.len() { return Err("TLS subjectAltName has trailing data".to_string()); }
    let mut names = Vec::new();
    let mut p = start;
    while p < end {
        let (tag, content, item_end, _, _) = jet_net_tls_der_element(value, &mut p)?;
        if tag == 0x82 {
            names.push(std::str::from_utf8(&value[content..item_end])
                .map_err(|_| "TLS DNS name is not valid ASCII".to_string())?.to_string());
        }
    }
    Ok(names)
}

fn jet_net_tls_name(der: &[u8], start: usize, end: usize) -> Result<String, String> {
    fn escaped(bytes: &[u8]) -> String {
        let mut text = String::new();
        for &byte in bytes {
            match byte {
                b'\\' => text.push_str("\\\\"),
                b',' => text.push_str("\\,"),
                b'=' => text.push_str("\\="),
                0x20..=0x7e => text.push(byte as char),
                _ => text.push_str(&format!("\\x{byte:02x}")),
            }
        }
        text
    }

    let mut rdns = Vec::new();
    let mut p = start;
    while p < end {
        let (set_start, set_end, _, _) = jet_net_tls_der_expect(der, &mut p, 0x31)?;
        let mut set = set_start;
        while set < set_end {
            let (attr_start, attr_end, _, _) = jet_net_tls_der_expect(der, &mut set, 0x30)?;
            let mut attr = attr_start;
            let (oid_start, oid_end, _, _) = jet_net_tls_der_expect(der, &mut attr, 0x06)?;
            let (tag, value_start, value_end, _, _) = jet_net_tls_der_element(der, &mut attr)?;
            if attr != attr_end { return Err("TLS certificate name has trailing fields".to_string()); }
            let label = match &der[oid_start..oid_end] {
                [0x55, 0x04, 0x03] => "CN",
                [0x55, 0x04, 0x06] => "C",
                [0x55, 0x04, 0x07] => "L",
                [0x55, 0x04, 0x08] => "ST",
                [0x55, 0x04, 0x0a] => "O",
                [0x55, 0x04, 0x0b] => "OU",
                _ => "OID",
            };
            let bytes = &der[value_start..value_end];
            // Non-UTF8 DirectoryStrings, including Teletex and BMP/Universal,
            // stay as escaped source bytes: audit text is lossless and cannot
            // fail an already-verified handshake.
            let value = if tag == 0x0c {
                std::str::from_utf8(bytes).map(str::to_string).unwrap_or_else(|_| escaped(bytes))
            } else {
                escaped(bytes)
            };
            rdns.push(format!("{label}={value}"));
        }
    }
    Ok(rdns.join(","))
}

fn jet_net_tls_time(tag: u8, bytes: &[u8]) -> Result<i64, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "TLS certificate time is not ASCII".to_string())?;
    let (year, rest) = match tag {
        0x17 if text.len() == 13 && text.ends_with('Z') => {
            let yy: i64 = text[0..2].parse().map_err(|_| "TLS certificate UTC time is invalid".to_string())?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &text[2..12])
        }
        0x18 if text.len() == 15 && text.ends_with('Z') => (
            text[0..4].parse().map_err(|_| "TLS certificate generalized time is invalid".to_string())?,
            &text[4..14],
        ),
        _ => return Err("TLS certificate time uses an unsupported encoding".to_string()),
    };
    let parse = |range: std::ops::Range<usize>| -> Result<i64, String> {
        rest[range].parse().map_err(|_| "TLS certificate time contains invalid digits".to_string())
    };
    let month = parse(0..2)?;
    let day = parse(2..4)?;
    let hour = parse(4..6)?;
    let minute = parse(6..8)?;
    let second = parse(8..10)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return Err("TLS certificate time is out of range".to_string());
    }
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Ok((days * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
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
    let state = streams
        .get_mut(&id)
        .ok_or_else(|| "TLS stream is closed".to_string())?;
    if !state.pending_read.is_empty() {
        let take = std::cmp::min(limit as usize, state.pending_read.len());
        return Ok(Some(state.pending_read.drain(..take).collect()));
    }
    if state.read_eof {
        return Ok(Some(Vec::new()));
    }
    let mut bytes = vec![0u8; std::cmp::min(limit as usize, 16 * 1024 * 1024)];
    match state.stream.read(&mut bytes) {
        Ok(n) => {
            bytes.truncate(n);
            state.read_eof = n == 0;
            Ok(Some(bytes))
        }
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            Err(format!("TLS protocol truncation: {}", error))
        }
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
    if state.write_closing || state.write_closed {
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
    if state.write_closed {
        streams.remove(&id);
        jet_net_tls_closed().lock().unwrap().insert(id);
        return Ok(true);
    }
    if !state.write_closing {
        state.stream.conn.send_close_notify();
        state.write_closing = true;
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

pub fn jet_net_tls_close_write_step_impl(id: i64) -> Result<bool, String> {
    let mut streams = jet_net_tls_streams().lock().unwrap();
    let state = streams.get_mut(&id).ok_or_else(|| "TLS stream is closed".to_string())?;
    if state.write_closed { return Ok(true); }
    if !state.write_closing {
        state.stream.conn.send_close_notify();
        state.write_closing = true;
    }
    match state.stream.flush() {
        Ok(()) => {
            match state.stream.sock.shutdown(std::net::Shutdown::Write) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
                Err(error) => return Err(format!("TLS socket write shutdown failed: {}", error)),
            }
            state.write_closed = true;
            state.write_closing = false;
            Ok(true)
        }
        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted | std::io::ErrorKind::TimedOut) => Ok(false),
        Err(error) => Err(format!("TLS close-notify flush failed: {}", error)),
    }
}
