// ── E2-M10: networking (core.net + core.http) ─────────────────────────────────
// All networking uses std::net only — zero external crates in the prelude (I6).
// TLS (D-NET1) is delivered as the `jet.tls` FFI package and is not included here.

pub struct JetTCPListener {
    inner: std::net::TcpListener,
}

impl Clone for JetTCPListener {
    fn clone(&self) -> Self {
        Self {
            inner: self
                .inner
                .try_clone()
                .expect("TcpListener try_clone for spawn capture"),
        }
    }
}

pub struct JetTCPStream {
    inner: std::net::TcpStream,
    closed: bool,
    read_shutdown: bool,
    write_shutdown: bool,
    read_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
}

// D-NETIO-CONTRACT2=B: one nominal byte-stream contract. Network-specific
// state stays on JetTCPStream; generic consumers see only IOError.
trait JetIOReader {
    fn read(&mut self, limit: i64) -> Result<Vec<u8>, jet_std::IOError>;
}

trait JetIOWriter {
    fn write(&mut self, bytes: &Vec<u8>) -> Result<i64, jet_std::IOError>;
    fn write_all(&mut self, bytes: &Vec<u8>) -> Result<(), jet_std::IOError>;
}

fn jet_net_to_io_error(error: JetNetError) -> jet_std::IOError {
    let operation = match jet_net_error_operation(&error).as_str() {
        operation if operation.contains("read") => jet_std::IOOperation::Read,
        operation if operation.contains("write") || operation.contains("send") => jet_std::IOOperation::Write,
        operation if operation.contains("connect") => jet_std::IOOperation::Connect,
        operation if operation.contains("accept") => jet_std::IOOperation::Accept,
        operation if operation.contains("close") || operation.contains("shutdown") => jet_std::IOOperation::Close,
        operation if operation.contains("resolve") || operation.contains("dns") => jet_std::IOOperation::Resolve,
        _ => jet_std::IOOperation::Codec,
    };
    let resource = jet_net_error_address(&error).or_else(|| jet_net_error_name(&error));
    let context = jet_std::IOContext::new(operation, resource, jet_net_error_os_code(&error), Some(error.jet_show()));
    match error {
        JetNetError::InvalidInput(_) => jet_std::IOError::InvalidInput(context),
        JetNetError::PermissionDenied(_) => jet_std::IOError::PermissionDenied(context),
        JetNetError::Timeout(_) => jet_std::IOError::TimedOut(context),
        JetNetError::Cancelled(_) => jet_std::IOError::Cancelled(context),
        JetNetError::Closed(_) | JetNetError::NotConnected(_) => jet_std::IOError::Closed(context),
        JetNetError::Protocol(_) => jet_std::IOError::Protocol(context),
        _ => jet_std::IOError::Other(context),
    }
}

impl JetIOReader for JetTCPStream {
    fn read(&mut self, limit: i64) -> Result<Vec<u8>, jet_std::IOError> {
        if limit <= 0 {
            return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                None,
                None,
                Some("tcp read limit must be positive".to_string()),
            )));
        }
        jet_net_tcp_read_bytes(self, limit).map_err(jet_net_to_io_error)
    }
}

impl JetIOWriter for JetTCPStream {
    fn write(&mut self, bytes: &Vec<u8>) -> Result<i64, jet_std::IOError> {
        jet_net_tcp_write_bytes(self, bytes).map_err(jet_net_to_io_error)
    }

    fn write_all(&mut self, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
        jet_net_tcp_write_all_bytes(self, bytes).map_err(jet_net_to_io_error)
    }
}

impl JetIOReader for JetUnixStream {
    fn read(&mut self, limit: i64) -> Result<Vec<u8>, jet_std::IOError> {
        if limit <= 0 {
            return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                None,
                None,
                Some("unix read limit must be positive".to_string()),
            )));
        }
        jet_net_unix_read_bytes(self, limit).map_err(jet_net_to_io_error)
    }
}

impl JetIOWriter for JetUnixStream {
    fn write(&mut self, bytes: &Vec<u8>) -> Result<i64, jet_std::IOError> {
        jet_net_unix_write_bytes(self, bytes).map_err(jet_net_to_io_error)
    }

    fn write_all(&mut self, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
        jet_net_unix_write_all_bytes(self, bytes).map_err(jet_net_to_io_error)
    }
}

impl JetIOReader for JetTLSStream {
    fn read(&mut self, limit: i64) -> Result<Vec<u8>, jet_std::IOError> {
        if limit <= 0 {
            return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
                jet_std::IOOperation::Read,
                None,
                None,
                Some("tls read limit must be positive".to_string()),
            )));
        }
        jet_net_tls_read_bytes(self, limit)
    }
}

impl JetIOWriter for JetTLSStream {
    fn write(&mut self, bytes: &Vec<u8>) -> Result<i64, jet_std::IOError> {
        jet_net_tls_write_bytes(self, bytes)
    }

    fn write_all(&mut self, bytes: &Vec<u8>) -> Result<(), jet_std::IOError> {
        jet_net_tls_write_all_bytes(self, bytes)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetNetErrorDetail {
    operation: String,
    address: Option<String>,
    name: Option<String>,
    message: String,
    os_code: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetNetDnsError {
    NotFound(String),
    Failure(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum JetNetError {
    InvalidInput(JetNetErrorDetail),
    PermissionDenied(JetNetErrorDetail),
    AddressInUse(JetNetErrorDetail),
    AddressUnavailable(JetNetErrorDetail),
    ConnectionRefused(JetNetErrorDetail),
    ConnectionReset(JetNetErrorDetail),
    NotConnected(JetNetErrorDetail),
    Closed(JetNetErrorDetail),
    Timeout(JetNetErrorDetail),
    Cancelled(JetNetErrorDetail),
    Unsupported(JetNetErrorDetail),
    DNS(JetNetDnsError),
    TLS(JetNetErrorDetail),
    Protocol(JetNetErrorDetail),
    Other(JetNetErrorDetail),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetNetShutdown {
    Read,
    Write,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JetNetReadyInterest {
    Read,
    Write,
    ReadWrite,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JetNetReady {
    readable: bool,
    writable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetIpAddr {
    inner: std::net::IpAddr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetSocketAddr {
    inner: std::net::SocketAddr,
}

#[derive(Clone)]
pub struct JetUDPSocket {
    inner: std::sync::Arc<std::net::UdpSocket>,
    timeout_ms: std::sync::Arc<std::sync::Mutex<Option<i64>>>,
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(unix)]
    scheduler: std::sync::Arc<JetSchedulerRawIoHandle>,
}

#[derive(Clone, Debug)]
pub struct JetUDPPacket {
    data: Vec<u8>,
    addr: JetSocketAddr,
    original_len: i64,
    truncated: bool,
}

#[derive(Clone, Debug)]
pub struct JetDNSSrv {
    priority: i64,
    weight: i64,
    port: i64,
    target: String,
}

#[cfg(unix)]
pub struct JetUnixListener {
    inner: std::sync::Arc<std::os::unix::net::UnixListener>,
    scheduler: std::sync::Arc<JetSchedulerRawIoHandle>,
}

#[cfg(unix)]
impl Clone for JetUnixListener {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            scheduler: self.scheduler.clone(),
        }
    }
}

#[cfg(unix)]
pub struct JetUnixStream {
    inner: std::os::unix::net::UnixStream,
    scheduler: std::sync::Arc<JetSchedulerRawIoHandle>,
    closed: bool,
    read_shutdown: bool,
    write_shutdown: bool,
    read_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
}

#[cfg(unix)]
fn jet_net_scheduler_handle<T: std::os::fd::AsRawFd>(handle: &T) -> std::sync::Arc<JetSchedulerRawIoHandle> {
    jet_scheduler_raw_io_handle(handle.as_raw_fd())
}

#[cfg(not(unix))]
pub struct JetUnixListener;

#[cfg(not(unix))]
pub struct JetUnixStream;

pub struct JetTLSStream {
    id: i64,
    socket: std::net::TcpStream,
    read_timeout_ms: Option<i64>,
    write_timeout_ms: Option<i64>,
    wants: fn(i64) -> Result<(bool, bool), String>,
    read_ready: fn(i64) -> Result<bool, String>,
    read_step: fn(i64, i64) -> Result<Option<Vec<u8>>, String>,
    write_step: fn(i64, &Vec<u8>) -> Result<Option<i64>, String>,
    close_step: fn(i64) -> Result<bool, String>,
    close_write_step: fn(i64) -> Result<bool, String>,
    peer_snapshot: fn(i64) -> Result<JetTLSPeerSnapshot, String>,
    peer_identity: Option<JetTLSPeerIdentity>,
}

type JetTLSPeerSnapshot = (
    String,
    Vec<Vec<u8>>,
    Vec<Vec<u8>>,
    Vec<Vec<String>>,
    Vec<i64>,
    Vec<i64>,
    Vec<String>,
    Vec<String>,
);

#[derive(Clone)]
pub struct JetTLSRootCertificates {
    pem: Vec<u8>,
}

#[derive(Clone)]
pub struct JetTLSClientIdentity {
    cert_chain: Vec<u8>,
    private_key: JetCryptoSecretBytes,
}

#[derive(Clone)]
pub enum JetTLSTrust {
    System,
    SystemPlus(JetTLSRootCertificates),
    CustomOnly(JetTLSRootCertificates),
}

#[derive(Clone, Copy)]
pub enum JetTLSVersion {
    Tls12,
    Tls13,
}

#[derive(Clone)]
pub struct JetTLSClientConfig {
    trust: JetTLSTrust,
    identity: Option<JetTLSClientIdentity>,
    min_version: JetTLSVersion,
    max_version: JetTLSVersion,
    alpn: Vec<String>,
}

fn jet_tls_client_config_default() -> JetTLSClientConfig {
    JetTLSClientConfig {
        trust: JetTLSTrust::System,
        identity: None,
        min_version: JetTLSVersion::Tls12,
        max_version: JetTLSVersion::Tls13,
        alpn: Vec::new(),
    }
}

fn jet_tls_client_config_with_alpn(
    mut config: JetTLSClientConfig,
    protocols: &Vec<String>,
) -> Result<JetTLSClientConfig, jet_std::IOError> {
    if protocols.iter().any(|protocol| protocol.is_empty() || protocol.len() > u8::MAX as usize) {
        return Err(jet_tls_config_error(
            "ClientConfig.with_alpn",
            "TLS ALPN protocols must contain 1 to 255 bytes".to_string(),
        ));
    }
    config.alpn = protocols.clone();
    Ok(config)
}

fn jet_tls_config_error(operation: &str, message: String) -> jet_std::IOError {
    jet_std::IOError::InvalidInput(jet_std::IOContext::new(
        jet_std::IOOperation::Connect,
        Some("tls client configuration".to_string()),
        None,
        Some(format!("{} failed: {}", operation, message)),
    ))
}

fn jet_tls_root_certificates_from_pem(
    pem: &Vec<u8>,
    validate: fn(&Vec<u8>) -> Result<(), String>,
) -> Result<JetTLSRootCertificates, jet_std::IOError> {
    validate(pem).map_err(|message| jet_tls_config_error("RootCertificates.from_pem", message))?;
    Ok(JetTLSRootCertificates { pem: pem.clone() })
}

fn jet_tls_client_identity_from_pem(
    cert_chain: &Vec<u8>,
    private_key: &Vec<u8>,
    validate: fn(&Vec<u8>, &Vec<u8>) -> Result<(), String>,
) -> Result<JetTLSClientIdentity, jet_std::IOError> {
    validate(cert_chain, private_key)
        .map_err(|message| jet_tls_config_error("ClientIdentity.from_pem", message))?;
    Ok(JetTLSClientIdentity {
        cert_chain: cert_chain.clone(),
        private_key: JetCryptoSecretBytes::new(private_key.clone()),
    })
}

fn jet_tls_client_config_with_trust(
    mut config: JetTLSClientConfig,
    trust: JetTLSTrust,
) -> Result<JetTLSClientConfig, jet_std::IOError> {
    config.trust = trust;
    Ok(config)
}

fn jet_tls_client_config_with_client_identity(
    mut config: JetTLSClientConfig,
    identity: &JetTLSClientIdentity,
) -> Result<JetTLSClientConfig, jet_std::IOError> {
    config.identity = Some(identity.clone());
    Ok(config)
}

fn jet_tls_client_config_with_version_bounds(
    mut config: JetTLSClientConfig,
    min: JetTLSVersion,
    max: JetTLSVersion,
) -> Result<JetTLSClientConfig, jet_std::IOError> {
    let value = |version| match version { JetTLSVersion::Tls12 => 12, JetTLSVersion::Tls13 => 13 };
    if value(min) > value(max) {
        return Err(jet_tls_config_error(
            "ClientConfig.with_version_bounds",
            "minimum TLS version exceeds maximum".to_string(),
        ));
    }
    config.min_version = min;
    config.max_version = max;
    Ok(config)
}

fn jet_tls_client_config_http_parts(
    config: &JetTLSClientConfig,
) -> (i64, Vec<u8>, Vec<u8>, Vec<u8>, i64, i64) {
    let (trust, roots) = match &config.trust {
        JetTLSTrust::System => (0i64, Vec::new()),
        JetTLSTrust::SystemPlus(roots) => (1i64, roots.pem.clone()),
        JetTLSTrust::CustomOnly(roots) => (2i64, roots.pem.clone()),
    };
    let empty = Vec::new();
    let (cert, key) = config
        .identity
        .as_ref()
        .map(|identity| (identity.cert_chain.clone(), identity.private_key.as_vec().clone()))
        .unwrap_or((empty.clone(), empty));
    let version = |version| match version {
        JetTLSVersion::Tls12 => 12i64,
        JetTLSVersion::Tls13 => 13i64,
    };
    (
        trust,
        roots,
        cert,
        key,
        version(config.min_version),
        version(config.max_version),
    )
}

#[derive(Clone)]
pub struct JetTLSCertificate {
    pub der: Vec<u8>,
    pub sha256: Vec<u8>,
    pub spki_sha256: Vec<u8>,
    pub dns_names: Vec<String>,
    pub valid_from_unix_ms: i64,
    pub valid_until_unix_ms: i64,
    pub subject: String,
    pub issuer: String,
}

#[derive(Clone)]
pub struct JetTLSPeerIdentity {
    pub verified_server_name: String,
    pub leaf: JetTLSCertificate,
    pub certificate_chain: Vec<JetTLSCertificate>,
}


impl JetShow for JetTCPListener {
    fn jet_show(&self) -> String {
        format!(
            "TcpListener({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|error| format!("address unavailable: {}", error))
        )
    }
}
impl JetShow for JetTCPStream {
    fn jet_show(&self) -> String {
        format!(
            "TcpStream({})",
            self.inner
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|error| format!("peer unavailable: {}", error))
        )
    }
}
impl JetShow for JetNetError {
    fn jet_show(&self) -> String {
        match self {
            JetNetError::DNS(JetNetDnsError::NotFound(name)) => {
                format!("DNS name not found: `{}`", name)
            }
            JetNetError::DNS(JetNetDnsError::Failure(message)) => message.clone(),
            JetNetError::InvalidInput(d)
            | JetNetError::PermissionDenied(d)
            | JetNetError::AddressInUse(d)
            | JetNetError::AddressUnavailable(d)
            | JetNetError::ConnectionRefused(d)
            | JetNetError::ConnectionReset(d)
            | JetNetError::NotConnected(d)
            | JetNetError::Closed(d)
            | JetNetError::Timeout(d)
            | JetNetError::Cancelled(d)
            | JetNetError::Unsupported(d)
            | JetNetError::TLS(d)
            | JetNetError::Protocol(d)
            | JetNetError::Other(d) => d.message.clone(),
        }
    }
}
impl JetShow for JetIpAddr {
    fn jet_show(&self) -> String {
        self.inner.to_string()
    }
}
impl JetShow for JetSocketAddr {
    fn jet_show(&self) -> String {
        self.inner.to_string()
    }
}
impl JetShow for JetUDPSocket {
    fn jet_show(&self) -> String {
        format!(
            "UdpSocket({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_else(|error| format!("address unavailable: {}", error))
        )
    }
}
impl JetShow for JetUDPPacket {
    fn jet_show(&self) -> String {
        format!("UDPPacket({} bytes from {})", self.data.len(), self.addr.inner)
    }
}
impl JetShow for JetDNSSrv {
    fn jet_show(&self) -> String {
        format!(
            "DNSSrv(priority={}, weight={}, port={}, target={})",
            self.priority, self.weight, self.port, self.target
        )
    }
}
impl JetShow for JetUnixListener {
    fn jet_show(&self) -> String {
        "UnixListener".to_string()
    }
}
impl JetShow for JetUnixStream {
    fn jet_show(&self) -> String {
        "UnixStream".to_string()
    }
}
impl JetShow for JetTLSStream {
    fn jet_show(&self) -> String {
        format!("TLSStream({})", self.id)
    }
}

fn jet_net_timeout(ms: i64) -> Result<std::time::Duration, String> {
    if ms < 0 {
        return Err("network timeout must be non-negative".to_string());
    }
    Ok(std::time::Duration::from_millis(ms as u64))
}

fn jet_net_detail(
    operation: &str,
    address: Option<String>,
    name: Option<String>,
    message: String,
    os_code: Option<i64>,
) -> JetNetErrorDetail {
    JetNetErrorDetail {
        operation: operation.to_string(),
        address,
        name,
        message,
        os_code,
    }
}

fn jet_net_invalid_input(operation: &str, resource: &str) -> JetNetError {
    JetNetError::InvalidInput(jet_net_detail(
        operation,
        None,
        None,
        format!("invalid {resource}"),
        None,
    ))
}

fn jet_net_io_error(operation: &str, address: Option<String>, error: std::io::Error) -> JetNetError {
    let detail = jet_net_detail(
        operation,
        address,
        None,
        format!("{} failed: {}", operation, error),
        error.raw_os_error().map(|code| code as i64),
    );
    match error.kind() {
        std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => JetNetError::InvalidInput(detail),
        std::io::ErrorKind::PermissionDenied => JetNetError::PermissionDenied(detail),
        std::io::ErrorKind::AddrInUse => JetNetError::AddressInUse(detail),
        std::io::ErrorKind::AddrNotAvailable => JetNetError::AddressUnavailable(detail),
        std::io::ErrorKind::ConnectionRefused => JetNetError::ConnectionRefused(detail),
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe => JetNetError::ConnectionReset(detail),
        std::io::ErrorKind::NotConnected => JetNetError::NotConnected(detail),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => JetNetError::Timeout(detail),
        std::io::ErrorKind::Unsupported => JetNetError::Unsupported(detail),
        _ => JetNetError::Other(detail),
    }
}

fn jet_net_closed(operation: &str) -> JetNetError {
    JetNetError::Closed(jet_net_detail(
        operation,
        None,
        None,
        format!("{} failed: socket is closed", operation),
        None,
    ))
}

fn jet_net_tls_result<T>(result: Result<T, String>, operation: &str) -> Result<T, JetNetError> {
    result.map_err(|message| {
        let detail = jet_net_detail(operation, None, None, message.clone(), None);
        if message.starts_with("TLS protocol truncation:") {
            JetNetError::Protocol(detail)
        } else if message.contains("closed") {
            JetNetError::Closed(detail)
        } else if message.contains("timed out") || message.contains("deadline") {
            JetNetError::Timeout(detail)
        } else {
            JetNetError::TLS(detail)
        }
    })
}

fn jet_net_tls_io_result<T>(
    result: Result<T, String>,
    operation: jet_std::IOOperation,
) -> Result<T, jet_std::IOError> {
    result.map_err(|message| {
        let context = jet_std::IOContext::new(operation, Some("TLS stream".to_string()), None, Some(message.clone()));
        if message.starts_with("TLS protocol truncation:") || message.starts_with("TLS protocol error:") {
            jet_std::IOError::Protocol(context)
        } else if message.contains("closed") {
            jet_std::IOError::Closed(context)
        } else if message.contains("timed out") || message.contains("deadline") {
            jet_std::IOError::TimedOut(context)
        } else if message.contains("cancelled") {
            jet_std::IOError::Cancelled(context)
        } else {
            jet_std::IOError::Other(context)
        }
    })
}

fn jet_net_dns_result<T>(result: Result<T, String>, name: &str) -> Result<T, JetNetError> {
    result.map_err(|message| {
        if message.starts_with("DNS name not found") {
            JetNetError::DNS(JetNetDnsError::NotFound(name.to_string()))
        } else if message.starts_with("network operation cancelled") {
            JetNetError::Cancelled(jet_net_detail("dns", None, Some(name.to_string()), message, None))
        } else if message.starts_with("network protocol error") {
            JetNetError::Protocol(jet_net_detail("dns", None, Some(name.to_string()), message, None))
        } else if message.contains("timed out") || message.contains("timeout") {
            JetNetError::Timeout(jet_net_detail("dns", None, Some(name.to_string()), message, None))
        } else {
            JetNetError::DNS(JetNetDnsError::Failure(message))
        }
    })
}

fn jet_net_error_detail(error: &JetNetError) -> Option<&JetNetErrorDetail> {
    match error {
        JetNetError::InvalidInput(d)
        | JetNetError::PermissionDenied(d)
        | JetNetError::AddressInUse(d)
        | JetNetError::AddressUnavailable(d)
        | JetNetError::ConnectionRefused(d)
        | JetNetError::ConnectionReset(d)
        | JetNetError::NotConnected(d)
        | JetNetError::Closed(d)
        | JetNetError::Timeout(d)
        | JetNetError::Cancelled(d)
        | JetNetError::Unsupported(d)
        | JetNetError::TLS(d)
        | JetNetError::Protocol(d)
        | JetNetError::Other(d) => Some(d),
        JetNetError::DNS(_) => None,
    }
}

fn jet_net_error_operation(error: &JetNetError) -> String {
    jet_net_error_detail(error).map(|d| d.operation.clone()).unwrap_or_else(|| "dns".to_string())
}
fn jet_net_error_address(error: &JetNetError) -> Option<String> {
    jet_net_error_detail(error).and_then(|d| d.address.clone())
}
fn jet_net_error_name(error: &JetNetError) -> Option<String> {
    match error {
        JetNetError::DNS(JetNetDnsError::NotFound(name)) => Some(name.clone()),
        _ => jet_net_error_detail(error).and_then(|d| d.name.clone()),
    }
}
fn jet_net_error_message(error: &JetNetError) -> String { error.jet_show() }
fn jet_net_error_os_code(error: &JetNetError) -> Option<i64> {
    jet_net_error_detail(error).and_then(|d| d.os_code)
}

// Canonical CoreLib shape used by every engine adapter to marshal `NetError`.
// Ordinals follow the surface enum order, not the Rust declaration order.
enum JetNetErrorSurfacePayload {
    Detail(JetNetErrorDetail),
    DNS {
        variant: &'static str,
        ordinal: i64,
        value: String,
    },
}

struct JetNetErrorSurfaceParts {
    variant: &'static str,
    ordinal: i64,
    payload: JetNetErrorSurfacePayload,
}

fn jet_net_error_surface_parts(error: JetNetError) -> JetNetErrorSurfaceParts {
    let detail = |variant, ordinal, value| JetNetErrorSurfaceParts {
        variant,
        ordinal,
        payload: JetNetErrorSurfacePayload::Detail(value),
    };
    match error {
        JetNetError::InvalidInput(value) => detail("InvalidInput", 0, value),
        JetNetError::PermissionDenied(value) => detail("PermissionDenied", 1, value),
        JetNetError::AddressInUse(value) => detail("AddressInUse", 2, value),
        JetNetError::AddressUnavailable(value) => detail("AddressUnavailable", 3, value),
        JetNetError::ConnectionRefused(value) => detail("ConnectionRefused", 4, value),
        JetNetError::ConnectionReset(value) => detail("ConnectionReset", 5, value),
        JetNetError::NotConnected(value) => detail("NotConnected", 6, value),
        JetNetError::Closed(value) => detail("Closed", 7, value),
        JetNetError::Timeout(value) => detail("Timeout", 8, value),
        JetNetError::Cancelled(value) => detail("Cancelled", 9, value),
        JetNetError::Unsupported(value) => detail("Unsupported", 10, value),
        JetNetError::TLS(value) => detail("TLS", 11, value),
        JetNetError::Protocol(value) => detail("Protocol", 12, value),
        JetNetError::Other(value) => detail("Other", 13, value),
        JetNetError::DNS(JetNetDnsError::NotFound(value)) => JetNetErrorSurfaceParts {
            variant: "DNS",
            ordinal: 14,
            payload: JetNetErrorSurfacePayload::DNS {
                variant: "NotFound",
                ordinal: 0,
                value,
            },
        },
        JetNetError::DNS(JetNetDnsError::Failure(value)) => JetNetErrorSurfaceParts {
            variant: "DNS",
            ordinal: 14,
            payload: JetNetErrorSurfacePayload::DNS {
                variant: "Failure",
                ordinal: 1,
                value,
            },
        },
    }
}

fn jet_net_tcp_stream(inner: std::net::TcpStream) -> Result<JetTCPStream, JetNetError> {
    inner
        .set_nonblocking(true)
        .map_err(|error| jet_net_io_error("tcp scheduler registration", None, error))?;
    Ok(JetTCPStream {
        inner,
        closed: false,
        read_shutdown: false,
        write_shutdown: false,
        read_timeout_ms: None,
        write_timeout_ms: None,
    })
}

fn jet_net_operation_deadline(timeout_ms: Option<i64>) -> Option<JetDeadlineGuard> {
    let timeout_ms = timeout_ms?;
    let configured = jet_std_time_now().saturating_add(timeout_ms);
    let deadline = jet_ctx_deadline_ms().map_or(configured, |ambient| ambient.min(configured));
    Some(jet_ctx_push_deadline(deadline))
}

fn jet_net_explicit_deadline(deadline: &jet_std::Duration, operation: &str) -> Result<Option<JetDeadlineGuard>, JetNetError> {
    jet_net_timeout(deadline.as_millis()).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail(operation, None, None, message, None))
    })?;
    Ok(jet_net_operation_deadline(Some(deadline.as_millis())))
}

fn jet_net_deadline_timeout(operation: &str) -> JetNetError {
    JetNetError::Timeout(jet_net_detail(
        operation,
        None,
        None,
        format!("deadline exceeded while waiting in {}", operation),
        None,
    ))
}

fn jet_net_scheduler_park(operation: &str, millis: u64) -> Result<(), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| jet_scheduler_park_ms("network wait", millis)) {
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

fn jet_net_wait_for_connect(
    receiver: std::sync::mpsc::Receiver<Result<std::net::TcpStream, std::io::Error>>,
    address: Option<String>,
) -> Result<JetTCPStream, JetNetError> {
    loop {
        match receiver.try_recv() {
            Ok(Ok(stream)) => return jet_net_tcp_stream(stream),
            Ok(Err(error)) => return Err(jet_net_io_error("tcp connect", address, error)),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                jet_net_scheduler_park("tcp connect", 5)?;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(JetNetError::Other(jet_net_detail(
                    "tcp connect", address, None,
                    "tcp connect worker stopped without a result".to_string(), None,
                )));
            }
        }
    }
}

fn jet_net_connect_addr_worker(addr: std::net::SocketAddr) -> Result<JetTCPStream, JetNetError> {
    let address = addr.to_string();
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout("tcp connect"));
    }
    let timeout = jet_deadline_remaining_ms().map(|ms| {
        std::time::Duration::from_millis(ms.max(1) as u64)
    });
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = match timeout {
            Some(timeout) => std::net::TcpStream::connect_timeout(&addr, timeout),
            None => std::net::TcpStream::connect(addr),
        };
        let _ = sender.send(result);
    });
    jet_net_wait_for_connect(receiver, Some(address))
}

fn jet_net_scheduler_wait(
    stream: &std::net::TcpStream,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_io_wait(stream, read, write, operation)
    }) {
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation,
            None,
            None,
            format!("{} cancelled", operation),
            None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation,
            None,
            None,
            format!("{} scheduler wait failed: {}", operation, message),
            None,
        ))),
    }
}

#[cfg(unix)]
fn jet_net_tcp_scheduler_ready_wait(
    stream: &std::net::TcpStream,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(bool, bool), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_tcp_stream_ready_wait(stream, read, write, operation)
    }) {
        JetSchedulerWait::Ready(observed) => Ok(observed),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

#[cfg(not(unix))]
fn jet_net_tcp_scheduler_ready_wait(
    _stream: &std::net::TcpStream,
    _read: bool,
    _write: bool,
    operation: &str,
) -> Result<(bool, bool), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail(
        operation,
        None,
        None,
        "observed TCP readiness is not available on this platform".to_string(),
        None,
    )))
}

#[cfg(unix)]
fn jet_net_unix_scheduler_wait(
    stream: &JetUnixStream,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_unix_stream_io_wait(&stream.scheduler, read, write, operation)
    }) {
        JetSchedulerWait::Ready(()) if stream.closed => Err(jet_net_closed(operation)),
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

#[cfg(unix)]
fn jet_net_unix_scheduler_ready_wait(
    stream: &JetUnixStream,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(bool, bool), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_unix_stream_ready_wait(&stream.scheduler, read, write, operation)
    }) {
        JetSchedulerWait::Ready(_) if stream.closed => Err(jet_net_closed(operation)),
        JetSchedulerWait::Ready(observed) => Ok(observed),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

fn jet_net_tcp_listener_scheduler_wait(
    listener: &std::net::TcpListener,
    operation: &str,
) -> Result<(), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_tcp_listener_io_wait(listener, operation)
    }) {
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

#[cfg(unix)]
fn jet_net_unix_listener_scheduler_wait(
    listener: &JetUnixListener,
    operation: &str,
) -> Result<(), JetNetError> {
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_unix_listener_io_wait(&listener.scheduler, operation)
    }) {
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

fn jet_net_udp_scheduler_wait(
    socket: &JetUDPSocket,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(), JetNetError> {
    if jet_scheduler_wait_point_cancelled() {
        return Err(JetNetError::Cancelled(jet_net_detail(
            operation,
            None,
            None,
            format!("{} cancelled", operation),
            None,
        )));
    }
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout(operation));
    }
    let waited = jet_scheduler_wait_without_unwind(|| {
        #[cfg(unix)]
        jet_scheduler_udp_io_wait(&socket.scheduler, read, write, operation);
        #[cfg(not(unix))]
        {
            let _ = (socket, read, write);
            jet_scheduler_park_ms("udp readiness", 5);
        }
    });
    match waited {
        JetSchedulerWait::Ready(()) if socket.closed.load(std::sync::atomic::Ordering::Acquire) => {
            Err(jet_net_closed(operation))
        }
        JetSchedulerWait::Ready(()) if jet_scheduler_wait_point_cancelled() => {
            Err(JetNetError::Cancelled(jet_net_detail(
                operation,
                None,
                None,
                format!("{} cancelled", operation),
                None,
            )))
        }
        JetSchedulerWait::Ready(())
            if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) =>
        {
            Err(jet_net_deadline_timeout(operation))
        }
        JetSchedulerWait::Ready(()) => Ok(()),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation, None, None, format!("{} cancelled", operation), None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation, None, None, format!("{} scheduler wait failed: {}", operation, message), None,
        ))),
    }
}

#[cfg(unix)]
fn jet_net_udp_scheduler_ready_wait(
    socket: &JetUDPSocket,
    read: bool,
    write: bool,
    operation: &str,
) -> Result<(bool, bool), JetNetError> {
    if jet_scheduler_wait_point_cancelled() {
        return Err(JetNetError::Cancelled(jet_net_detail(
            operation,
            None,
            None,
            format!("{} cancelled", operation),
            None,
        )));
    }
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout(operation));
    }
    match jet_scheduler_wait_without_unwind(|| {
        jet_scheduler_udp_ready_wait(&socket.scheduler, read, write, operation)
    }) {
        JetSchedulerWait::Ready(_) if socket.closed.load(std::sync::atomic::Ordering::Acquire) => {
            Err(jet_net_closed(operation))
        }
        JetSchedulerWait::Ready(_observed) if jet_scheduler_wait_point_cancelled() => {
            Err(JetNetError::Cancelled(jet_net_detail(
                operation,
                None,
                None,
                format!("{} cancelled", operation),
                None,
            )))
        }
        JetSchedulerWait::Ready(_) if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) => {
            Err(jet_net_deadline_timeout(operation))
        }
        JetSchedulerWait::Ready(observed) => Ok(observed),
        JetSchedulerWait::Cancelled => Err(JetNetError::Cancelled(jet_net_detail(
            operation,
            None,
            None,
            format!("{} cancelled", operation),
            None,
        ))),
        JetSchedulerWait::Deadline(_) => Err(jet_net_deadline_timeout(operation)),
        JetSchedulerWait::Panicked(message) => Err(JetNetError::Other(jet_net_detail(
            operation,
            None,
            None,
            format!("{} scheduler wait failed: {}", operation, message),
            None,
        ))),
    }
}

#[cfg(not(unix))]
fn jet_net_udp_scheduler_ready_wait(
    _socket: &JetUDPSocket,
    _read: bool,
    _write: bool,
    operation: &str,
) -> Result<(bool, bool), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail(
        operation,
        None,
        None,
        "observed UDP readiness is not available on this platform".to_string(),
        None,
    )))
}

fn jet_net_tls_scheduler_wait(
    stream: &JetTLSStream,
    fallback_read: bool,
    fallback_write: bool,
    operation: &str,
) -> Result<(), JetNetError> {
    let (mut read, mut write) = jet_net_tls_result((stream.wants)(stream.id), operation)?;
    if !read && !write {
        read = fallback_read;
        write = fallback_write;
    }
    jet_net_scheduler_wait(&stream.socket, read, write, operation)
}

fn jet_net_tls_io_scheduler_wait(
    stream: &JetTLSStream,
    fallback_read: bool,
    fallback_write: bool,
    operation: jet_std::IOOperation,
) -> Result<(), jet_std::IOError> {
    let (mut read, mut write) = jet_net_tls_io_result((stream.wants)(stream.id), operation)?;
    if !read && !write {
        read = fallback_read;
        write = fallback_write;
    }
    let label = match operation {
        jet_std::IOOperation::Read => "tls read",
        jet_std::IOOperation::Write => "tls write",
        jet_std::IOOperation::Close => "tls close",
        _ => "tls I/O",
    };
    jet_net_scheduler_wait(&stream.socket, read, write, label).map_err(jet_net_to_io_error)
}

fn jet_net_tls_client_scheduler(
    stream: JetTCPStream,
    server_name: &String,
    begin: fn(std::net::TcpStream, &String) -> Result<i64, String>,
    handshake_step: fn(i64) -> Result<bool, String>,
    abort: fn(i64),
    wants: fn(i64) -> Result<(bool, bool), String>,
    read_ready: fn(i64) -> Result<bool, String>,
    read_step: fn(i64, i64) -> Result<Option<Vec<u8>>, String>,
    write_step: fn(i64, &Vec<u8>) -> Result<Option<i64>, String>,
    close_step: fn(i64) -> Result<bool, String>,
    close_write_step: fn(i64) -> Result<bool, String>,
    peer_snapshot: fn(i64) -> Result<JetTLSPeerSnapshot, String>,
) -> Result<JetTLSStream, JetNetError> {
    jet_net_tls_client_scheduler_with_begin(
        stream,
        |inner| begin(inner, server_name),
        handshake_step,
        abort,
        wants,
        read_ready,
        read_step,
        write_step,
        close_step,
        close_write_step,
        peer_snapshot,
    )
}

fn jet_net_tls_client_scheduler_with_begin<F>(
    stream: JetTCPStream,
    begin: F,
    handshake_step: fn(i64) -> Result<bool, String>,
    abort: fn(i64),
    wants: fn(i64) -> Result<(bool, bool), String>,
    read_ready: fn(i64) -> Result<bool, String>,
    read_step: fn(i64, i64) -> Result<Option<Vec<u8>>, String>,
    write_step: fn(i64, &Vec<u8>) -> Result<Option<i64>, String>,
    close_step: fn(i64) -> Result<bool, String>,
    close_write_step: fn(i64) -> Result<bool, String>,
    peer_snapshot: fn(i64) -> Result<JetTLSPeerSnapshot, String>,
) -> Result<JetTLSStream, JetNetError>
where
    F: FnOnce(std::net::TcpStream) -> Result<i64, String>,
{
    let socket = stream
        .inner
        .try_clone()
        .map_err(|error| jet_net_io_error("tls scheduler registration", None, error))?;
    let read_timeout_ms = stream.read_timeout_ms;
    let write_timeout_ms = stream.write_timeout_ms;
    let handshake_timeout_ms = match (read_timeout_ms, write_timeout_ms) {
        (Some(read), Some(write)) => Some(read.min(write)),
        (read, write) => read.or(write),
    };
    let id = jet_net_tls_result(begin(stream.inner), "tls handshake")?;
    let mut tls = JetTLSStream {
        id,
        socket,
        read_timeout_ms,
        write_timeout_ms,
        wants,
        read_ready,
        read_step,
        write_step,
        close_step,
        close_write_step,
        peer_snapshot,
        peer_identity: None,
    };
    let result = (|| {
        let _deadline = jet_net_operation_deadline(handshake_timeout_ms);
        loop {
            if jet_net_tls_result(handshake_step(id), "tls handshake")? {
                let peer = jet_net_tls_peer_identity_from_snapshot(jet_net_tls_result(
                    (tls.peer_snapshot)(id), "tls peer identity",
                )?)?;
                tls.peer_identity = Some(peer);
                return Ok(tls);
            }
            jet_net_tls_scheduler_wait(&tls, true, true, "tls handshake")?;
        }
    })();
    if result.is_err() {
        abort(id);
    }
    result
}

fn jet_net_tls_client_scheduler_deadline(
    stream: JetTCPStream,
    server_name: &String,
    deadline: &jet_std::Duration,
    begin: fn(std::net::TcpStream, &String) -> Result<i64, String>,
    handshake_step: fn(i64) -> Result<bool, String>,
    abort: fn(i64),
    wants: fn(i64) -> Result<(bool, bool), String>,
    read_ready: fn(i64) -> Result<bool, String>,
    read_step: fn(i64, i64) -> Result<Option<Vec<u8>>, String>,
    write_step: fn(i64, &Vec<u8>) -> Result<Option<i64>, String>,
    close_step: fn(i64) -> Result<bool, String>,
    close_write_step: fn(i64) -> Result<bool, String>,
    peer_snapshot: fn(i64) -> Result<JetTLSPeerSnapshot, String>,
) -> Result<JetTLSStream, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "tls handshake")?;
    jet_net_tls_client_scheduler(
        stream, server_name, begin, handshake_step, abort, wants, read_ready, read_step, write_step,
        close_step, close_write_step, peer_snapshot,
    )
}

fn jet_net_tls_client_scheduler_config_deadline(
    stream: JetTCPStream,
    server_name: &String,
    config: &JetTLSClientConfig,
    deadline: &jet_std::Duration,
    begin: fn(
        std::net::TcpStream, &String, i64, &Vec<u8>, &Vec<u8>, &Vec<u8>, i64, i64, &Vec<String>,
    ) -> Result<i64, String>,
    handshake_step: fn(i64) -> Result<bool, String>,
    abort: fn(i64),
    wants: fn(i64) -> Result<(bool, bool), String>,
    read_ready: fn(i64) -> Result<bool, String>,
    read_step: fn(i64, i64) -> Result<Option<Vec<u8>>, String>,
    write_step: fn(i64, &Vec<u8>) -> Result<Option<i64>, String>,
    close_step: fn(i64) -> Result<bool, String>,
    close_write_step: fn(i64) -> Result<bool, String>,
    peer_snapshot: fn(i64) -> Result<JetTLSPeerSnapshot, String>,
) -> Result<JetTLSStream, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "tls handshake")?;
    let (trust_mode, roots) = match &config.trust {
        JetTLSTrust::System => (0, Vec::new()),
        JetTLSTrust::SystemPlus(roots) => (1, roots.pem.clone()),
        JetTLSTrust::CustomOnly(roots) => (2, roots.pem.clone()),
    };
    let empty = Vec::new();
    let (cert_chain, private_key) = config.identity.as_ref()
        .map(|identity| (&identity.cert_chain, identity.private_key.as_vec()))
        .unwrap_or((&empty, &empty));
    let version = |version| match version { JetTLSVersion::Tls12 => 12, JetTLSVersion::Tls13 => 13 };
    jet_net_tls_client_scheduler_with_begin(
        stream,
        |inner| begin(
            inner, server_name, trust_mode, &roots, cert_chain, private_key,
            version(config.min_version), version(config.max_version), &config.alpn,
        ),
        handshake_step,
        abort,
        wants,
        read_ready,
        read_step,
        write_step,
        close_step,
        close_write_step,
        peer_snapshot,
    )
}

fn jet_net_tls_read_bytes(stream: &mut JetTLSStream, limit: i64) -> Result<Vec<u8>, jet_std::IOError> {
    if limit <= 0 {
        return Err(jet_std::IOError::InvalidInput(jet_std::IOContext::new(
            jet_std::IOOperation::Read, Some("TLS stream".to_string()), None,
            Some("tls read limit must be positive".to_string()),
        )));
    }
    let _deadline = jet_net_operation_deadline(stream.read_timeout_ms);
    loop {
        match jet_net_tls_io_result((stream.read_step)(stream.id, limit), jet_std::IOOperation::Read)? {
            Some(bytes) => return Ok(bytes),
            None => jet_net_tls_io_scheduler_wait(stream, true, false, jet_std::IOOperation::Read)?,
        }
    }
}

fn jet_net_tls_read_bytes_deadline(stream: &mut JetTLSStream, limit: i64, deadline: &jet_std::Duration) -> Result<Vec<u8>, jet_std::IOError> {
    let _deadline = jet_net_explicit_deadline(deadline, "tls read").map_err(jet_net_to_io_error)?;
    jet_net_tls_read_bytes(stream, limit)
}

fn jet_net_tls_read_text(stream: &mut JetTLSStream) -> Result<String, jet_std::IOError> {
    let bytes = jet_net_tls_read_bytes(stream, 8192)?;
    String::from_utf8(bytes).map_err(|error| jet_std::IOError::InvalidInput(jet_std::IOContext::new(
        jet_std::IOOperation::Read, Some("TLS stream".to_string()), None,
        Some(format!("tls read text failed: invalid UTF-8: {}", error)),
    )))
}

fn jet_net_tls_write_bytes_with_current_deadline(
    stream: &mut JetTLSStream,
    data: &Vec<u8>,
) -> Result<i64, jet_std::IOError> {
    loop {
        match jet_net_tls_io_result((stream.write_step)(stream.id, data), jet_std::IOOperation::Write)? {
            Some(count) => return Ok(count),
            None => jet_net_tls_io_scheduler_wait(stream, false, true, jet_std::IOOperation::Write)?,
        }
    }
}

fn jet_net_tls_write_bytes(stream: &mut JetTLSStream, data: &Vec<u8>) -> Result<i64, jet_std::IOError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    jet_net_tls_write_bytes_with_current_deadline(stream, data)
}

fn jet_net_tls_write_all_bytes(stream: &mut JetTLSStream, data: &Vec<u8>) -> Result<(), jet_std::IOError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    let mut offset = 0usize;
    while offset < data.len() {
        let chunk = data[offset..].to_vec();
        let count = jet_net_tls_write_bytes_with_current_deadline(stream, &chunk)? as usize;
        if count == 0 {
            return Err(jet_std::IOError::Other(jet_std::IOContext::new(
                jet_std::IOOperation::Write, Some("TLS stream".to_string()), None,
                Some("tls write all failed: zero bytes written".to_string()),
            )));
        }
        offset += count;
    }
    Ok(())
}

fn jet_net_tls_write_all_bytes_deadline(stream: &mut JetTLSStream, data: &Vec<u8>, deadline: &jet_std::Duration) -> Result<(), jet_std::IOError> {
    let _deadline = jet_net_explicit_deadline(deadline, "tls write all").map_err(jet_net_to_io_error)?;
    jet_net_tls_write_all_bytes(stream, data)
}

fn jet_net_tls_write_text(stream: &mut JetTLSStream, text: &String) -> Result<(), jet_std::IOError> {
    jet_net_tls_write_all_bytes(stream, &text.as_bytes().to_vec())
}

fn jet_net_tls_close(stream: &mut JetTLSStream) -> Result<(), jet_std::IOError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    loop {
        if jet_net_tls_io_result((stream.close_step)(stream.id), jet_std::IOOperation::Close)? {
            return Ok(());
        }
        jet_net_tls_io_scheduler_wait(stream, false, true, jet_std::IOOperation::Close)?;
    }
}

fn jet_net_tls_close_write(
    stream: &mut JetTLSStream,
    deadline: &jet_std::Duration,
) -> Result<(), jet_std::IOError> {
    let _deadline = jet_net_explicit_deadline(deadline, "tls close write").map_err(jet_net_to_io_error)?;
    loop {
        if jet_net_tls_io_result((stream.close_write_step)(stream.id), jet_std::IOOperation::Close)? {
            return Ok(());
        }
        jet_net_tls_io_scheduler_wait(stream, false, true, jet_std::IOOperation::Close)?;
    }
}

fn jet_net_tls_peer_identity_from_snapshot(
    snapshot: JetTLSPeerSnapshot,
) -> Result<JetTLSPeerIdentity, JetNetError> {
    let (verified_server_name, ders, spkis, dns_names, valid_from, valid_until, subjects, issuers) = snapshot;
    let mut certificate_chain = Vec::with_capacity(ders.len());
    for index in 0..ders.len() {
        let der = ders[index].clone();
        certificate_chain.push(JetTLSCertificate {
            sha256: jet_sha256_raw(&der).to_vec(),
            spki_sha256: jet_sha256_raw(&spkis[index]).to_vec(),
            der,
            dns_names: dns_names[index].clone(),
            valid_from_unix_ms: valid_from[index],
            valid_until_unix_ms: valid_until[index],
            subject: subjects[index].clone(),
            issuer: issuers[index].clone(),
        });
    }
    let leaf = certificate_chain.first().cloned().ok_or_else(|| JetNetError::TLS(jet_net_detail(
        "tls peer identity", None, None, "verified TLS peer chain is empty".to_string(), None,
    )))?;
    Ok(JetTLSPeerIdentity { verified_server_name, leaf, certificate_chain })
}

fn jet_net_tls_peer_identity(stream: &JetTLSStream) -> JetTLSPeerIdentity {
    stream.peer_identity.clone().expect("verified TLS stream always retains peer identity")
}

fn jet_net_tls_ready(
    stream: &JetTLSStream,
    interest: JetNetReadyInterest,
    deadline: &jet_std::Duration,
) -> Result<JetNetReady, jet_std::IOError> {
    let operation = if matches!(interest, JetNetReadyInterest::Write) {
        jet_std::IOOperation::Write
    } else {
        jet_std::IOOperation::Read
    };
    let _deadline = jet_net_explicit_deadline(deadline, "tls ready").map_err(jet_net_to_io_error)?;
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_to_io_error(jet_net_deadline_timeout("tls ready")));
    }
    let want_read = matches!(interest, JetNetReadyInterest::Read | JetNetReadyInterest::ReadWrite);
    let want_write = matches!(interest, JetNetReadyInterest::Write | JetNetReadyInterest::ReadWrite);
    loop {
        let readable = want_read && jet_net_tls_io_result((stream.read_ready)(stream.id), operation)?;
        if readable {
            return Ok(JetNetReady { readable: true, writable: false });
        }
        let (tls_read, tls_write) = jet_net_tls_io_result((stream.wants)(stream.id), operation)?;
        let (raw_readable, raw_writable) = jet_net_tcp_scheduler_ready_wait(
            &stream.socket,
            want_read || tls_read,
            want_write || tls_write,
            "tls ready",
        ).map_err(jet_net_to_io_error)?;
        let readable = want_read && raw_readable
            && jet_net_tls_io_result((stream.read_ready)(stream.id), operation)?;
        let writable = want_write && raw_writable;
        if readable || writable {
            return Ok(JetNetReady { readable, writable });
        }
    }
}

fn jet_net_apply_tcp_deadlines(stream: &std::net::TcpStream, op: &str) -> Result<(), JetNetError> {
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            return Err(jet_net_deadline_timeout(op));
        }
        let dur = Some(std::time::Duration::from_millis(remaining as u64));
        stream.set_read_timeout(dur)
            .map_err(|error| jet_net_io_error(&format!("{} read timeout", op), None, error))?;
        stream.set_write_timeout(dur)
            .map_err(|error| jet_net_io_error(&format!("{} write timeout", op), None, error))?;
    }
    Ok(())
}

fn jet_net_ip_addr(text: &String) -> Result<JetIpAddr, JetNetError> {
    jet_net_pure_parse_ip(text)
        .map(|inner| JetIpAddr { inner })
        .map_err(|e| JetNetError::InvalidInput(jet_net_detail("parse IP address", Some(text.clone()), None, format!("invalid IP address `{}`: {}", text, e), None)))
}

fn jet_net_ip_to_string(ip: &JetIpAddr) -> String {
    ip.inner.to_string()
}

fn jet_net_ip_is_ipv4(ip: &JetIpAddr) -> bool {
    jet_net_pure_ip_is_ipv4(&ip.inner)
}

fn jet_net_socket_addr(host: &String, port: i64) -> Result<JetSocketAddr, JetNetError> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(JetNetError::InvalidInput(jet_net_detail("resolve socket address", Some(host.clone()), None, format!("invalid port `{}`: expected 0..65535", port), None)));
    }
    let text = format!("{}:{}", host, port);
    let resolved = match jet_net_pure_parse_socket_addr(&text) {
        Ok(address) => Ok(address),
        Err(_) => {
            use std::net::ToSocketAddrs;
            match text.to_socket_addrs() { Ok(iter) => iter, Err(e) => return Err(jet_net_io_error("resolve socket address", Some(text), e)) }
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))
        }
    };
    resolved
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| jet_net_io_error("resolve socket address", Some(text), e))
}

fn jet_net_socket_addr_parse(text: &String) -> Result<JetSocketAddr, JetNetError> {
    jet_net_pure_parse_socket_addr(text)
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| JetNetError::InvalidInput(jet_net_detail("parse socket address", Some(text.clone()), None, format!("invalid socket address `{}`: {}", text, e), None)))
}

fn jet_net_socket_host(addr: &JetSocketAddr) -> String {
    jet_net_pure_socket_host(&addr.inner)
}

fn jet_net_socket_port(addr: &JetSocketAddr) -> i64 {
    jet_net_pure_socket_port(&addr.inner)
}

fn jet_net_socket_to_string(addr: &JetSocketAddr) -> String {
    jet_net_pure_socket_to_string(&addr.inner)
}

fn jet_net_tcp_listen_addr(addr: &JetSocketAddr) -> Result<JetTCPListener, JetNetError> {
    let inner = std::net::TcpListener::bind(addr.inner)
        .map_err(|e| jet_net_io_error("tcp listen", Some(addr.inner.to_string()), e))?;
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("tcp listen", Some(addr.inner.to_string()), e))?;
    Ok(JetTCPListener { inner })
}

fn jet_net_tcp_connect_addr(addr: &JetSocketAddr) -> Result<JetTCPStream, JetNetError> {
    jet_net_connect_addr_worker(addr.inner)
}

fn jet_net_tcp_connect_timeout(addr: &JetSocketAddr, ms: i64) -> Result<JetTCPStream, JetNetError> {
    jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("tcp connect", Some(addr.inner.to_string()), None, m, None)))?;
    let _deadline = jet_net_operation_deadline(Some(ms));
    jet_net_connect_addr_worker(addr.inner)
}

fn jet_net_tcp_connect_happy(host: &String, port: i64, ms: i64) -> Result<JetTCPStream, JetNetError> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(JetNetError::InvalidInput(jet_net_detail("tcp connect", Some(host.clone()), None, format!("invalid port `{}`: expected 0..65535", port), None)));
    }
    jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("tcp connect", Some(host.clone()), None, m, None)))?;
    let _deadline = jet_net_operation_deadline(Some(ms));
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout("tcp connect"));
    }
    let query = host.clone();
    let (resolve_sender, resolve_receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        use std::net::ToSocketAddrs;
        let result = (query.as_str(), port as u16)
            .to_socket_addrs()
            .map(|addrs| addrs.collect::<Vec<_>>());
        let _ = resolve_sender.send(result);
    });
    let addrs = loop {
        match resolve_receiver.try_recv() {
            Ok(Ok(addrs)) => break addrs,
            Ok(Err(error)) => return Err(jet_net_io_error("resolve socket address", Some(host.clone()), error)),
            Err(std::sync::mpsc::TryRecvError::Empty) => jet_net_scheduler_park("tcp connect", 5)?,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(JetNetError::Other(jet_net_detail(
                    "tcp connect", Some(host.clone()), None,
                    "socket address resolver stopped without a result".to_string(), None,
                )));
            }
        }
    };
    if addrs.is_empty() {
        return Err(JetNetError::AddressUnavailable(jet_net_detail("tcp connect", Some(host.clone()), None, format!("tcp connect failed: no address for `{}`", host), None)));
    }

    let first_is_v6 = addrs[0].is_ipv6();
    let mut v6 = addrs.iter().copied().filter(std::net::SocketAddr::is_ipv6);
    let mut v4 = addrs.iter().copied().filter(std::net::SocketAddr::is_ipv4);
    let mut ordered = Vec::with_capacity(addrs.len());
    loop {
        let next = if ordered.len() % 2 == 0 {
            if first_is_v6 { v6.next().or_else(|| v4.next()) } else { v4.next().or_else(|| v6.next()) }
        } else if first_is_v6 {
            v4.next().or_else(|| v6.next())
        } else {
            v6.next().or_else(|| v4.next())
        };
        match next {
            Some(addr) => ordered.push(addr),
            None => break,
        }
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    let mut next = 0usize;
    let mut in_flight = 0usize;
    let mut last = None;
    let mut next_launch = std::time::Instant::now();
    loop {
        if next < ordered.len() && std::time::Instant::now() >= next_launch {
            let addr = ordered[next];
            let remaining = jet_deadline_remaining_ms().unwrap_or(0);
            if remaining <= 0 {
                return Err(jet_net_deadline_timeout("tcp connect"));
            }
            let attempt_sender = sender.clone();
            std::thread::spawn(move || {
                let timeout = std::time::Duration::from_millis(remaining as u64);
                let _ = attempt_sender.send((addr, std::net::TcpStream::connect_timeout(&addr, timeout)));
            });
            next += 1;
            in_flight += 1;
            next_launch = std::time::Instant::now() + std::time::Duration::from_millis(250);
        }
        match receiver.try_recv() {
            Ok((_addr, Ok(stream))) => return jet_net_tcp_stream(stream),
            Ok((addr, Err(error))) => {
                in_flight -= 1;
                last = Some((addr, error));
                next_launch = std::time::Instant::now();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => unreachable!("connect sender retained"),
        }
        if next == ordered.len() && in_flight == 0 {
            let (addr, error) = last.expect("at least one connection attempt completed");
            return Err(jet_net_io_error("tcp connect", Some(addr.to_string()), error));
        }
        jet_net_scheduler_park("tcp connect", 5)?;
    }
}

fn jet_net_listener_local_socket_addr(listener: &JetTCPListener) -> Result<JetSocketAddr, JetNetError> {
    listener.inner.local_addr().map(|inner| JetSocketAddr { inner })
        .map_err(|e| jet_net_io_error("tcp listener local address", None, e))
}

fn jet_net_tcp_local_socket_addr(stream: &JetTCPStream) -> Result<JetSocketAddr, JetNetError> {
    stream.inner.local_addr().map(|inner| JetSocketAddr { inner })
        .map_err(|e| jet_net_io_error("tcp local address", None, e))
}

fn jet_net_tcp_peer_socket_addr(stream: &JetTCPStream) -> Result<JetSocketAddr, JetNetError> {
    stream.inner.peer_addr().map(|inner| JetSocketAddr { inner })
        .map_err(|e| jet_net_io_error("tcp peer address", None, e))
}

fn jet_net_tcp_listen(addr: &String) -> Result<JetTCPListener, JetNetError> {
    let inner = std::net::TcpListener::bind(addr.as_str())
        .map_err(|e| jet_net_io_error("tcp listen", Some(addr.clone()), e))?;
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("tcp listen", Some(addr.clone()), e))?;
    Ok(JetTCPListener { inner })
}

fn jet_net_tcp_accept(listener: &JetTCPListener) -> Result<JetTCPStream, JetNetError> {
    loop {
        match listener.inner.accept() {
            Ok((stream, _)) => return jet_net_tcp_stream(stream),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_tcp_listener_scheduler_wait(&listener.inner, "tcp accept")?;
            }
            Err(error) => return Err(jet_net_io_error("tcp accept", None, error)),
        }
    }
}

fn jet_net_tcp_accept_deadline(listener: &JetTCPListener, deadline: &jet_std::Duration) -> Result<JetTCPStream, JetNetError> {
    let deadline_ms = deadline.as_millis();
    jet_net_timeout(deadline_ms).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("tcp accept", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline_ms));
    jet_net_tcp_accept(listener)
}

fn jet_net_tcp_connect(addr: &String) -> Result<JetTCPStream, JetNetError> {
    let address = addr.clone();
    let worker_address = address.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(std::net::TcpStream::connect(worker_address.as_str()));
    });
    jet_net_wait_for_connect(receiver, Some(address))
}

fn jet_net_tcp_read(stream: &mut JetTCPStream) -> Result<String, JetNetError> {
    jet_net_tcp_read_text(stream, 8192)
}

fn jet_net_tcp_read_bytes(stream: &mut JetTCPStream, limit: i64) -> Result<Vec<u8>, JetNetError> {
    use std::io::Read;
    if stream.closed || stream.read_shutdown {
        return Err(jet_net_closed("tcp read"));
    }
    if limit < 0 {
        return Err(JetNetError::InvalidInput(jet_net_detail(
            "tcp read",
            None,
            None,
            "tcp read limit must be non-negative".to_string(),
            None,
        )));
    }
    let cap = std::cmp::min(limit as usize, 16 * 1024 * 1024);
    if cap == 0 {
        return Ok(Vec::new());
    }
    let _deadline = jet_net_operation_deadline(stream.read_timeout_ms);
    jet_net_apply_tcp_deadlines(&stream.inner, "tcp read")?;
    let mut bytes = vec![0u8; cap];
    loop {
        match stream.inner.read(&mut bytes) {
            Ok(n) => {
                bytes.truncate(n);
                return Ok(bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_net_scheduler_wait(&stream.inner, true, false, "tcp read")?;
            }
            Err(e) => return Err(jet_net_io_error("tcp read", None, e)),
        }
    }
}

fn jet_net_tcp_read_bytes_deadline(stream: &mut JetTCPStream, limit: i64, deadline: &jet_std::Duration) -> Result<Vec<u8>, JetNetError> {
    let deadline_ms = deadline.as_millis();
    jet_net_timeout(deadline_ms).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("tcp read", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline_ms));
    jet_net_tcp_read_bytes(stream, limit)
}

fn jet_net_tcp_read_text(stream: &mut JetTCPStream, limit: i64) -> Result<String, JetNetError> {
    let bytes = jet_net_tcp_read_bytes(stream, limit)?;
    String::from_utf8(bytes).map_err(|error| {
        JetNetError::InvalidInput(jet_net_detail(
            "tcp read text",
            None,
            None,
            format!("tcp read text failed: invalid UTF-8: {}", error),
            None,
        ))
    })
}

fn jet_net_tcp_read_text_deadline(stream: &mut JetTCPStream, limit: i64, deadline: &jet_std::Duration) -> Result<String, JetNetError> {
    let bytes = jet_net_tcp_read_bytes_deadline(stream, limit, deadline)?;
    String::from_utf8(bytes).map_err(|error| JetNetError::InvalidInput(jet_net_detail(
        "tcp read text", None, None, format!("tcp read text failed: {}", error), None,
    )))
}

fn jet_net_tcp_write_bytes_with_current_deadline(
    stream: &mut JetTCPStream,
    data: &[u8],
) -> Result<i64, JetNetError> {
    use std::io::Write;
    if stream.closed || stream.write_shutdown {
        return Err(jet_net_closed("tcp write"));
    }
    jet_net_apply_tcp_deadlines(&stream.inner, "tcp write")?;
    loop {
        match stream.inner.write(data) {
            Ok(n) => return Ok(n as i64),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_net_scheduler_wait(&stream.inner, false, true, "tcp write")?;
            }
            Err(e) => return Err(jet_net_io_error("tcp write", None, e)),
        }
    }
}

fn jet_net_tcp_write_bytes(stream: &mut JetTCPStream, data: &Vec<u8>) -> Result<i64, JetNetError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    jet_net_tcp_write_bytes_with_current_deadline(stream, data)
}

fn jet_net_tcp_write_bytes_deadline(stream: &mut JetTCPStream, data: &Vec<u8>, deadline: &jet_std::Duration) -> Result<i64, JetNetError> {
    let deadline_ms = deadline.as_millis();
    jet_net_timeout(deadline_ms).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("tcp write", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline_ms));
    jet_net_tcp_write_bytes(stream, data)
}

fn jet_net_tcp_write_all_bytes(stream: &mut JetTCPStream, data: &Vec<u8>) -> Result<(), JetNetError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    let mut offset = 0usize;
    while offset < data.len() {
        let wrote = jet_net_tcp_write_bytes_with_current_deadline(stream, &data[offset..])? as usize;
        if wrote == 0 {
            return Err(JetNetError::ConnectionReset(jet_net_detail(
                "tcp write all",
                None,
                None,
                "tcp write all failed: zero bytes written".to_string(),
                None,
            )));
        }
        offset += wrote;
    }
    Ok(())
}

fn jet_net_tcp_write_all_bytes_deadline(stream: &mut JetTCPStream, data: &Vec<u8>, deadline: &jet_std::Duration) -> Result<(), JetNetError> {
    let deadline_ms = deadline.as_millis();
    jet_net_timeout(deadline_ms).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("tcp write all", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline_ms));
    jet_net_tcp_write_all_bytes(stream, data)
}

fn jet_net_tcp_write_text(stream: &mut JetTCPStream, text: &String) -> Result<(), JetNetError> {
    jet_net_tcp_write_all_bytes(stream, &text.as_bytes().to_vec())
}

fn jet_net_tcp_write_text_deadline(stream: &mut JetTCPStream, text: &String, deadline: &jet_std::Duration) -> Result<(), JetNetError> {
    jet_net_tcp_write_all_bytes_deadline(stream, &text.as_bytes().to_vec(), deadline)
}

fn jet_net_tcp_shutdown(stream: &mut JetTCPStream, how: JetNetShutdown) -> Result<(), JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("tcp shutdown"));
    }
    let shutdown = match how {
        JetNetShutdown::Read => std::net::Shutdown::Read,
        JetNetShutdown::Write => std::net::Shutdown::Write,
        JetNetShutdown::Both => std::net::Shutdown::Both,
    };
    stream.inner.shutdown(shutdown).map_err(|e| jet_net_io_error("tcp shutdown", None, e))?;
    match how {
        JetNetShutdown::Read => stream.read_shutdown = true,
        JetNetShutdown::Write => stream.write_shutdown = true,
        JetNetShutdown::Both => {
            stream.read_shutdown = true;
            stream.write_shutdown = true;
        }
    }
    Ok(())
}

fn jet_net_tcp_close(stream: &mut JetTCPStream) -> Result<(), JetNetError> {
    if stream.closed {
        return Ok(());
    }
    match stream.inner.shutdown(std::net::Shutdown::Both) {
        Ok(()) => {}
        Err(error) if matches!(error.kind(), std::io::ErrorKind::NotConnected) => {}
        Err(error) => return Err(jet_net_io_error("tcp close", None, error)),
    }
    stream.closed = true;
    stream.read_shutdown = true;
    stream.write_shutdown = true;
    Ok(())
}

fn jet_net_tcp_ready(
    stream: &mut JetTCPStream,
    interest: JetNetReadyInterest,
    deadline_ms: i64,
) -> Result<JetNetReady, JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("tcp ready"));
    }
    let _ = jet_net_timeout(deadline_ms).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("tcp ready", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline_ms));
    let read = matches!(interest, JetNetReadyInterest::Read | JetNetReadyInterest::ReadWrite);
    let write = matches!(interest, JetNetReadyInterest::Write | JetNetReadyInterest::ReadWrite);
    let (readable, writable) =
        jet_net_tcp_scheduler_ready_wait(&stream.inner, read, write, "tcp ready")?;
    Ok(JetNetReady { readable, writable })
}

fn jet_net_tcp_ready_deadline(
    stream: &mut JetTCPStream,
    interest: JetNetReadyInterest,
    deadline: &jet_std::Duration,
) -> Result<JetNetReady, JetNetError> {
    jet_net_tcp_ready(stream, interest, deadline.as_millis())
}

fn jet_net_ready_readable(ready: &JetNetReady) -> bool {
    ready.readable
}

fn jet_net_ready_writable(ready: &JetNetReady) -> bool {
    ready.writable
}

fn jet_net_tcp_write(stream: &mut JetTCPStream, data: &String) -> Result<(), JetNetError> {
    jet_net_tcp_write_text(stream, data)
}

fn jet_net_tcp_local_addr(stream: &JetTCPStream) -> Result<String, JetNetError> {
    stream.inner.local_addr().map(|a| a.to_string())
        .map_err(|e| jet_net_io_error("tcp local address", None, e))
}

fn jet_net_tcp_peer_addr(stream: &JetTCPStream) -> Result<String, JetNetError> {
    stream.inner.peer_addr().map(|a| a.to_string())
        .map_err(|e| jet_net_io_error("tcp peer address", None, e))
}

fn jet_net_listener_local_addr(listener: &JetTCPListener) -> Result<String, JetNetError> {
    listener.inner.local_addr().map(|a| a.to_string())
        .map_err(|e| jet_net_io_error("tcp listener local address", None, e))
}

fn jet_net_set_timeout(stream: &mut JetTCPStream, ms: i64) -> Result<(), JetNetError> {
    let _ = jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("set tcp timeout", None, None, m, None)))?;
    stream.read_timeout_ms = Some(ms);
    stream.write_timeout_ms = Some(ms);
    Ok(())
}

fn jet_net_set_read_timeout(stream: &mut JetTCPStream, ms: i64) -> Result<(), JetNetError> {
    let _ = jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("set tcp read timeout", None, None, m, None)))?;
    stream.read_timeout_ms = Some(ms);
    Ok(())
}

fn jet_net_set_write_timeout(stream: &mut JetTCPStream, ms: i64) -> Result<(), JetNetError> {
    let _ = jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("set tcp write timeout", None, None, m, None)))?;
    stream.write_timeout_ms = Some(ms);
    Ok(())
}

fn jet_net_nodelay(stream: &JetTCPStream) -> Result<bool, JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("tcp nodelay"));
    }
    stream
        .inner
        .nodelay()
        .map_err(|e| jet_net_io_error("tcp nodelay", None, e))
}

fn jet_net_set_nodelay(stream: &JetTCPStream, enabled: bool) -> Result<(), JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("set tcp nodelay"));
    }
    stream
        .inner
        .set_nodelay(enabled)
        .map_err(|e| jet_net_io_error("set tcp nodelay", None, e))
}

fn jet_net_ttl(stream: &JetTCPStream) -> Result<i64, JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("tcp ttl"));
    }
    stream
        .inner
        .ttl()
        .map(|v| v as i64)
        .map_err(|e| jet_net_io_error("tcp ttl", None, e))
}

fn jet_net_set_ttl(stream: &JetTCPStream, ttl: i64) -> Result<(), JetNetError> {
    if stream.closed {
        return Err(jet_net_closed("set tcp ttl"));
    }
    if !(0..=u32::MAX as i64).contains(&ttl) {
        return Err(JetNetError::InvalidInput(jet_net_detail(
            "set tcp ttl",
            None,
            None,
            "ttl must fit u32".to_string(),
            None,
        )));
    }
    stream
        .inner
        .set_ttl(ttl as u32)
        .map_err(|e| jet_net_io_error("set tcp ttl", None, e))
}

/// TCP streams are always SOCK_STREAM; returned for ledger socket_type parity.
fn jet_net_socket_type(_stream: &JetTCPStream) -> String {
    "stream".to_string()
}

/// Copy file bytes onto an open TCP stream (observable sendfile; not the syscall).
fn jet_net_sendfile(stream: &mut JetTCPStream, path: &String) -> Result<i64, JetNetError> {
    if stream.closed || stream.write_shutdown {
        return Err(jet_net_closed("tcp sendfile"));
    }
    let mut file = std::fs::File::open(path.as_str()).map_err(|e| {
        jet_net_io_error("tcp sendfile open", Some(path.clone()), e)
    })?;
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    let mut written: u64 = 0;
    let mut buf = [0u8; 8192];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf).map_err(|e| {
            jet_net_io_error("tcp sendfile read", Some(path.clone()), e)
        })?;
        if n == 0 {
            break;
        }
        let chunk = buf[..n].to_vec();
        jet_net_tcp_write_all_bytes(stream, &chunk)?;
        written += n as u64;
    }
    Ok(written.min(i64::MAX as u64) as i64)
}

const JET_NET_SERVICES: &[(&str, i64)] = &[
    ("ftp", 21),
    ("ssh", 22),
    ("telnet", 23),
    ("smtp", 25),
    ("dns", 53),
    ("domain", 53),
    ("http", 80),
    ("pop3", 110),
    ("imap", 143),
    ("https", 443),
    ("smtps", 465),
    ("submission", 587),
    ("imaps", 993),
    ("pop3s", 995),
];

fn jet_net_getservbyname(name: &String) -> Result<i64, JetNetError> {
    let key = name.to_ascii_lowercase();
    JET_NET_SERVICES
        .iter()
        .find(|(n, _)| *n == key)
        .map(|(_, port)| *port)
        .ok_or_else(|| {
            JetNetError::InvalidInput(jet_net_detail(
                "getservbyname",
                None,
                None,
                format!("unknown service `{name}`"),
                None,
            ))
        })
}

fn jet_net_getservbyport(port: i64) -> Result<String, JetNetError> {
    JET_NET_SERVICES
        .iter()
        .find(|(_, p)| *p == port)
        .map(|(n, _)| (*n).to_string())
        .ok_or_else(|| {
            JetNetError::InvalidInput(jet_net_detail(
                "getservbyport",
                None,
                None,
                format!("unknown port `{port}`"),
                None,
            ))
        })
}

fn jet_net_udp_bind(addr: &String) -> Result<JetUDPSocket, JetNetError> {
    let inner = std::net::UdpSocket::bind(addr.as_str())
        .map_err(|e| jet_net_io_error("udp bind", Some(addr.clone()), e))?;
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("udp bind", Some(addr.clone()), e))?;
    let inner = std::sync::Arc::new(inner);
    Ok(JetUDPSocket {
        inner: inner.clone(),
        timeout_ms: std::sync::Arc::new(std::sync::Mutex::new(None)),
        closed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        #[cfg(unix)]
        scheduler: jet_net_scheduler_handle(inner.as_ref()),
    })
}

fn jet_net_udp_bind_addr(addr: &JetSocketAddr) -> Result<JetUDPSocket, JetNetError> {
    let inner = std::net::UdpSocket::bind(addr.inner)
        .map_err(|e| jet_net_io_error("udp bind", Some(addr.inner.to_string()), e))?;
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("udp bind", Some(addr.inner.to_string()), e))?;
    let inner = std::sync::Arc::new(inner);
    Ok(JetUDPSocket {
        inner: inner.clone(),
        timeout_ms: std::sync::Arc::new(std::sync::Mutex::new(None)),
        closed: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        #[cfg(unix)]
        scheduler: jet_net_scheduler_handle(inner.as_ref()),
    })
}

fn jet_net_udp_open(socket: &JetUDPSocket, operation: &str) -> Result<(), JetNetError> {
    if socket.closed.load(std::sync::atomic::Ordering::Acquire) {
        Err(jet_net_closed(operation))
    } else {
        Ok(())
    }
}

fn jet_net_udp_local_addr(socket: &JetUDPSocket) -> Result<JetSocketAddr, JetNetError> {
    jet_net_udp_open(socket, "udp local address")?;
    socket.inner.local_addr().map(|inner| JetSocketAddr { inner })
        .map_err(|e| jet_net_io_error("udp local address", None, e))
}

fn jet_net_udp_set_timeout(socket: &JetUDPSocket, ms: i64) -> Result<(), JetNetError> {
    jet_net_udp_open(socket, "set udp timeout")?;
    let _ = jet_net_timeout(ms).map_err(|m| JetNetError::InvalidInput(jet_net_detail("set udp timeout", None, None, m, None)))?;
    *socket.timeout_ms.lock().unwrap() = Some(ms);
    Ok(())
}

fn jet_net_udp_send_to(
    socket: &JetUDPSocket,
    data: &String,
    addr: &JetSocketAddr,
) -> Result<i64, JetNetError> {
    jet_net_udp_send_slice(socket, data.as_bytes(), addr)
}

fn jet_net_udp_recv_from(socket: &JetUDPSocket, limit: i64) -> Result<JetUDPPacket, JetNetError> {
    jet_net_udp_open(socket, "udp receive")?;
    if limit <= 0 {
        return Err(JetNetError::InvalidInput(jet_net_detail("udp receive", None, None, "udp receive limit must be positive".to_string(), None)));
    }
    let _deadline = jet_net_operation_deadline(*socket.timeout_ms.lock().unwrap());
    let cap = std::cmp::min(limit as usize, 1 << 20);
    let mut buf = vec![0u8; 65535];
    loop {
        match socket.inner.recv_from(&mut buf) {
            Ok((n, addr)) => return Ok({
            let original_len = n;
            buf.truncate(std::cmp::min(n, cap));
            JetUDPPacket {
                data: buf,
                addr: JetSocketAddr { inner: addr },
                original_len: original_len as i64,
                truncated: original_len > cap,
            }
            }),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_udp_scheduler_wait(socket, true, false, "udp receive")?;
            }
            Err(error) => return Err(jet_net_io_error("udp receive", None, error)),
        }
    }
}

fn jet_net_udp_packet_data(packet: &JetUDPPacket) -> String {
    String::from_utf8_lossy(&packet.data).to_string()
}

fn jet_net_udp_packet_addr(packet: &JetUDPPacket) -> JetSocketAddr {
    packet.addr.clone()
}

fn jet_net_udp_send_bytes_to(
    socket: &JetUDPSocket,
    data: &Vec<u8>,
    addr: &JetSocketAddr,
) -> Result<i64, JetNetError> {
    jet_net_udp_send_slice(socket, data, addr)
}

fn jet_net_udp_send_slice(
    socket: &JetUDPSocket,
    data: &[u8],
    addr: &JetSocketAddr,
) -> Result<i64, JetNetError> {
    jet_net_udp_open(socket, "udp send")?;
    let _deadline = jet_net_operation_deadline(*socket.timeout_ms.lock().unwrap());
    loop {
        match socket.inner.send_to(data, addr.inner) {
            Ok(n) if n == data.len() => return Ok(n as i64),
            Ok(n) => return Err(JetNetError::Protocol(jet_net_detail(
                "udp send", Some(addr.inner.to_string()), None,
                format!("udp send wrote {} of {} datagram bytes", n, data.len()), None,
            ))),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_udp_scheduler_wait(socket, false, true, "udp send")?;
            }
            Err(error) => return Err(jet_net_io_error("udp send", Some(addr.inner.to_string()), error)),
        }
    }
}

fn jet_net_udp_receive(socket: &JetUDPSocket, limit: i64) -> Result<JetUDPPacket, JetNetError> {
    jet_net_udp_open(socket, "udp receive")?;
    if limit < 0 {
        return Err(JetNetError::InvalidInput(jet_net_detail(
            "udp receive", None, None, "udp receive limit must be non-negative".to_string(), None,
        )));
    }
    let _deadline = jet_net_operation_deadline(*socket.timeout_ms.lock().unwrap());
    let cap = std::cmp::min(limit as usize, 65535);
    let mut bytes = vec![0u8; 65535];
    let (original_len, addr) = loop {
        match socket.inner.recv_from(&mut bytes) {
            Ok(packet) => break packet,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_udp_scheduler_wait(socket, true, false, "udp receive")?;
            }
            Err(error) => return Err(jet_net_io_error("udp receive", None, error)),
        }
    };
    bytes.truncate(std::cmp::min(original_len, cap));
    Ok(JetUDPPacket {
        data: bytes,
        addr: JetSocketAddr { inner: addr },
        original_len: original_len as i64,
        truncated: original_len > cap,
    })
}

fn jet_net_udp_receive_deadline(
    socket: &JetUDPSocket,
    limit: i64,
    deadline: &jet_std::Duration,
) -> Result<JetUDPPacket, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "udp receive")?;
    jet_net_udp_receive(socket, limit)
}

fn jet_net_udp_send_bytes_to_deadline(
    socket: &JetUDPSocket,
    data: &Vec<u8>,
    addr: &JetSocketAddr,
    deadline: &jet_std::Duration,
) -> Result<i64, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "udp send")?;
    jet_net_udp_send_bytes_to(socket, data, addr)
}

fn jet_net_udp_close(socket: &JetUDPSocket) -> Result<(), JetNetError> {
    if !socket
        .closed
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        #[cfg(unix)]
        socket.scheduler.close();
    }
    Ok(())
}

fn jet_net_udp_ready(
    socket: &JetUDPSocket,
    interest: JetNetReadyInterest,
    deadline: &jet_std::Duration,
) -> Result<JetNetReady, JetNetError> {
    jet_net_udp_open(socket, "udp ready")?;
    jet_net_timeout(deadline.as_millis()).map_err(|message| {
        JetNetError::InvalidInput(jet_net_detail("udp ready", None, None, message, None))
    })?;
    let _deadline = jet_net_operation_deadline(Some(deadline.as_millis()));
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout("udp ready"));
    }
    #[cfg(not(unix))]
    return Err(JetNetError::Unsupported(jet_net_detail(
        "udp ready", None, None, "udp readiness backend is unavailable on this target".to_string(), None,
    )));
    #[cfg(unix)]
    {
        let want_read = matches!(interest, JetNetReadyInterest::Read | JetNetReadyInterest::ReadWrite);
        let want_write = matches!(interest, JetNetReadyInterest::Write | JetNetReadyInterest::ReadWrite);
        let (readable, writable) =
            jet_net_udp_scheduler_ready_wait(socket, want_read, want_write, "udp ready")?;
        Ok(JetNetReady { readable, writable })
    }
}

fn jet_net_udp_packet_bytes(packet: &JetUDPPacket) -> Vec<u8> {
    packet.data.clone()
}

fn jet_net_udp_packet_original_len(packet: &JetUDPPacket) -> i64 {
    packet.original_len
}

fn jet_net_udp_packet_truncated(packet: &JetUDPPacket) -> bool {
    packet.truncated
}

#[cfg(unix)]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, JetNetError> {
    let _ = std::fs::remove_file(path);
    let inner = std::os::unix::net::UnixListener::bind(path)
        .map_err(|e| jet_net_io_error("unix listen", Some(path.clone()), e))?;
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("unix listen", Some(path.clone()), e))?;
    let inner = std::sync::Arc::new(inner);
    Ok(JetUnixListener {
        scheduler: jet_net_scheduler_handle(inner.as_ref()),
        inner,
    })
}

#[cfg(not(unix))]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix listen", Some(path.clone()), None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_accept(listener: &JetUnixListener) -> Result<JetUnixStream, JetNetError> {
    loop {
        match listener.inner.accept() {
            Ok((inner, _)) => {
                inner.set_nonblocking(true)
                    .map_err(|error| jet_net_io_error("unix accept", None, error))?;
                return Ok(JetUnixStream {
                    scheduler: jet_net_scheduler_handle(&inner),
                    inner,
                    closed: false,
                    read_shutdown: false,
                    write_shutdown: false,
                    read_timeout_ms: None,
                    write_timeout_ms: None,
                });
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_unix_listener_scheduler_wait(listener, "unix accept")?;
            }
            Err(error) => return Err(jet_net_io_error("unix accept", None, error)),
        }
    }
}

#[cfg(unix)]
fn jet_net_unix_accept_deadline(listener: &JetUnixListener, deadline: &jet_std::Duration) -> Result<JetUnixStream, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "unix accept")?;
    jet_net_unix_accept(listener)
}

#[cfg(not(unix))]
fn jet_net_unix_accept(_listener: &JetUnixListener) -> Result<JetUnixStream, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix accept", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(not(unix))]
fn jet_net_unix_accept_deadline(_listener: &JetUnixListener, _deadline: &jet_std::Duration) -> Result<JetUnixStream, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix accept", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, JetNetError> {
    let address = path.clone();
    let worker_address = address.clone();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = sender.send(std::os::unix::net::UnixStream::connect(worker_address));
    });
    let inner = loop {
        match receiver.try_recv() {
            Ok(Ok(stream)) => break stream,
            Ok(Err(error)) => return Err(jet_net_io_error("unix connect", Some(address), error)),
            Err(std::sync::mpsc::TryRecvError::Empty) => jet_net_scheduler_park("unix connect", 5)?,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err(JetNetError::Other(jet_net_detail(
                "unix connect", Some(address), None, "unix connect worker stopped without a result".to_string(), None,
            ))),
        }
    };
    inner.set_nonblocking(true)
        .map_err(|e| jet_net_io_error("unix connect", Some(path.clone()), e))?;
    Ok(JetUnixStream {
            scheduler: jet_net_scheduler_handle(&inner),
            inner,
            closed: false,
            read_shutdown: false,
            write_shutdown: false,
            read_timeout_ms: None,
            write_timeout_ms: None,
        })
}

#[cfg(not(unix))]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix connect", Some(path.clone()), None, "unix sockets are not supported on this platform".to_string(), None)))
}

fn jet_net_unix_connect_deadline(path: &String, deadline: &jet_std::Duration) -> Result<JetUnixStream, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "unix connect")?;
    jet_net_unix_connect(path)
}

#[cfg(unix)]
fn jet_net_unix_read(stream: &mut JetUnixStream) -> Result<String, JetNetError> {
    let bytes = jet_net_unix_read_bytes(stream, 8192)?;
    String::from_utf8(bytes).map_err(|e| JetNetError::InvalidInput(jet_net_detail("unix read text", None, None, format!("unix read text failed: invalid UTF-8: {}", e), None)))
}

#[cfg(not(unix))]
fn jet_net_unix_read(_stream: &mut JetUnixStream) -> Result<String, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix read", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_write(stream: &mut JetUnixStream, data: &String) -> Result<(), JetNetError> {
    jet_net_unix_write_all_bytes(stream, &data.as_bytes().to_vec())
}

#[cfg(not(unix))]
fn jet_net_unix_write(_stream: &mut JetUnixStream, _data: &String) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix write", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_read_bytes(stream: &mut JetUnixStream, limit: i64) -> Result<Vec<u8>, JetNetError> {
    use std::io::Read;
    if stream.closed || stream.read_shutdown {
        return Err(jet_net_closed("unix read"));
    }
    if limit <= 0 {
        return Err(JetNetError::InvalidInput(jet_net_detail(
            "unix read", None, None, "unix read limit must be positive".to_string(), None,
        )));
    }
    let _deadline = jet_net_operation_deadline(stream.read_timeout_ms);
    let mut bytes = vec![0u8; std::cmp::min(limit as usize, 16 * 1024 * 1024)];
    loop {
        match stream.inner.read(&mut bytes) {
            Ok(n) => {
                bytes.truncate(n);
                return Ok(bytes);
            }
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_unix_scheduler_wait(stream, true, false, "unix read")?;
            }
            Err(error) => return Err(jet_net_io_error("unix read", None, error)),
        }
    }
}

#[cfg(unix)]
fn jet_net_unix_read_bytes_deadline(stream: &mut JetUnixStream, limit: i64, deadline: &jet_std::Duration) -> Result<Vec<u8>, JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "unix read")?;
    jet_net_unix_read_bytes(stream, limit)
}

#[cfg(not(unix))]
fn jet_net_unix_read_bytes_deadline(_stream: &mut JetUnixStream, _limit: i64, _deadline: &jet_std::Duration) -> Result<Vec<u8>, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix read", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(not(unix))]
fn jet_net_unix_read_bytes(_stream: &mut JetUnixStream, _limit: i64) -> Result<Vec<u8>, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail(
        "unix read", None, None, "unix sockets are not supported on this platform".to_string(), None,
    )))
}

#[cfg(unix)]
fn jet_net_unix_write_all_bytes(stream: &mut JetUnixStream, data: &Vec<u8>) -> Result<(), JetNetError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    let mut offset = 0usize;
    while offset < data.len() {
        let wrote = jet_net_unix_write_slice(stream, &data[offset..])? as usize;
        if wrote == 0 {
            return Err(JetNetError::ConnectionReset(jet_net_detail(
                "unix write all", None, None, "unix write all failed: zero bytes written".to_string(), None,
            )));
        }
        offset += wrote;
    }
    Ok(())
}

#[cfg(unix)]
fn jet_net_unix_write_all_bytes_deadline(stream: &mut JetUnixStream, data: &Vec<u8>, deadline: &jet_std::Duration) -> Result<(), JetNetError> {
    let _deadline = jet_net_explicit_deadline(deadline, "unix write all")?;
    jet_net_unix_write_all_bytes(stream, data)
}

#[cfg(not(unix))]
fn jet_net_unix_write_all_bytes_deadline(_stream: &mut JetUnixStream, _data: &Vec<u8>, _deadline: &jet_std::Duration) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix write", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_write_bytes(stream: &mut JetUnixStream, data: &Vec<u8>) -> Result<i64, JetNetError> {
    let _deadline = jet_net_operation_deadline(stream.write_timeout_ms);
    jet_net_unix_write_slice(stream, data)
}

#[cfg(unix)]
fn jet_net_unix_write_slice(stream: &mut JetUnixStream, data: &[u8]) -> Result<i64, JetNetError> {
    use std::io::Write;
    if stream.closed || stream.write_shutdown { return Err(jet_net_closed("unix write")); }
    loop {
        match stream.inner.write(data) {
            Ok(count) => return Ok(count as i64),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                jet_net_unix_scheduler_wait(stream, false, true, "unix write")?;
            }
            Err(error) => return Err(jet_net_io_error("unix write", None, error)),
        }
    }
}

#[cfg(not(unix))]
fn jet_net_unix_write_all_bytes(_stream: &mut JetUnixStream, _data: &Vec<u8>) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail(
        "unix write", None, None, "unix sockets are not supported on this platform".to_string(), None,
    )))
}

#[cfg(not(unix))]
fn jet_net_unix_write_bytes(_stream: &mut JetUnixStream, _data: &Vec<u8>) -> Result<i64, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail(
        "unix write", None, None, "unix sockets are not supported on this platform".to_string(), None,
    )))
}

#[cfg(unix)]
fn jet_net_unix_shutdown(stream: &mut JetUnixStream, how: JetNetShutdown) -> Result<(), JetNetError> {
    if stream.closed { return Err(jet_net_closed("unix shutdown")); }
    let os_how = match how {
        JetNetShutdown::Read => std::net::Shutdown::Read,
        JetNetShutdown::Write => std::net::Shutdown::Write,
        JetNetShutdown::Both => std::net::Shutdown::Both,
    };
    stream.inner.shutdown(os_how).map_err(|error| jet_net_io_error("unix shutdown", None, error))?;
    match how {
        JetNetShutdown::Read => stream.read_shutdown = true,
        JetNetShutdown::Write => stream.write_shutdown = true,
        JetNetShutdown::Both => { stream.read_shutdown = true; stream.write_shutdown = true; }
    }
    Ok(())
}

#[cfg(not(unix))]
fn jet_net_unix_shutdown(_stream: &mut JetUnixStream, _how: JetNetShutdown) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix shutdown", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_close(stream: &mut JetUnixStream) -> Result<(), JetNetError> {
    if stream.closed { return Ok(()); }
    stream.closed = true;
    stream.read_shutdown = true;
    stream.write_shutdown = true;
    stream.scheduler.close();
    match stream.inner.shutdown(std::net::Shutdown::Both) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
        Err(error) => return Err(jet_net_io_error("unix close", None, error)),
    }
    Ok(())
}

#[cfg(not(unix))]
fn jet_net_unix_close(_stream: &mut JetUnixStream) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix close", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_set_timeout(stream: &mut JetUnixStream, timeout: &jet_std::Duration) -> Result<(), JetNetError> {
    jet_net_timeout(timeout.as_millis()).map_err(|message| JetNetError::InvalidInput(jet_net_detail(
        "set unix timeout", None, None, message, None,
    )))?;
    stream.read_timeout_ms = Some(timeout.as_millis());
    stream.write_timeout_ms = Some(timeout.as_millis());
    Ok(())
}

#[cfg(not(unix))]
fn jet_net_unix_set_timeout(_stream: &mut JetUnixStream, _timeout: &jet_std::Duration) -> Result<(), JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("set unix timeout", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

#[cfg(unix)]
fn jet_net_unix_ready(
    stream: &JetUnixStream,
    interest: JetNetReadyInterest,
    deadline: &jet_std::Duration,
) -> Result<JetNetReady, JetNetError> {
    if stream.closed { return Err(jet_net_closed("unix ready")); }
    let _deadline = jet_net_explicit_deadline(deadline, "unix ready")?;
    if matches!(jet_deadline_remaining_ms(), Some(ms) if ms <= 0) {
        return Err(jet_net_deadline_timeout("unix ready"));
    }
    let want_read = matches!(interest, JetNetReadyInterest::Read | JetNetReadyInterest::ReadWrite);
    let want_write = matches!(interest, JetNetReadyInterest::Write | JetNetReadyInterest::ReadWrite);
    let (readable, writable) =
        jet_net_unix_scheduler_ready_wait(stream, want_read, want_write, "unix ready")?;
    Ok(JetNetReady { readable, writable })
}

#[cfg(not(unix))]
fn jet_net_unix_ready(_stream: &JetUnixStream, _interest: JetNetReadyInterest, _deadline: &jet_std::Duration) -> Result<JetNetReady, JetNetError> {
    Err(JetNetError::Unsupported(jet_net_detail("unix ready", None, None, "unix sockets are not supported on this platform".to_string(), None)))
}

fn jet_net_dns_system_servers() -> Vec<String> {
    let mut out = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        out.extend(jet_net_dns_parse_resolv_conf(&text));
    }

    // macOS resolver policy is scoped per interface/VPN. `scutil --dns` is
    // the stable system view; /etc/resolv.conf explicitly does not carry all
    // scoped resolvers.
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("scutil").arg("--dns").output() {
        out.extend(jet_net_dns_parse_scutil(&String::from_utf8_lossy(&output.stdout)));
    }

    // PowerShell returns address values, not localized ipconfig labels, and
    // preserves IPv4/IPv6 plus interface-scoped resolver policy.
    #[cfg(windows)]
    if let Ok(output) = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "Get-DNSClientServerAddress | ForEach-Object { $_.ServerAddresses }"])
        .output()
    {
        out.extend(jet_net_dns_parse_windows_addresses(&String::from_utf8_lossy(&output.stdout)));
    }
    out.sort();
    out.dedup();
    out
}

fn jet_net_dns_encode_name(out: &mut Vec<u8>, name: &str) -> Result<(), String> {
    let trimmed = name.trim_end_matches('.');
    if trimmed.is_empty() {
        out.push(0);
        return Ok(());
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 || !label.is_ascii() {
            return Err(format!("invalid DNS name `{}`", name));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    if out.len() > 255 {
        return Err(format!("invalid DNS name `{}`: encoded name exceeds 255 bytes", name));
    }
    Ok(())
}

fn jet_net_dns_read_name(packet: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut labels = Vec::new();
    let mut p = *pos;
    let mut jumped = false;
    let mut seen = vec![false; packet.len()];
    let mut wire_len = 1usize;
    loop {
        if p >= packet.len() {
            return Err("network protocol error: truncated DNS name".to_string());
        }
        if seen[p] {
            return Err("network protocol error: cyclic DNS compression pointer".to_string());
        }
        seen[p] = true;
        let len = packet[p];
        if len & 0xc0 == 0xc0 {
            if p + 1 >= packet.len() {
                return Err("network protocol error: truncated DNS compression pointer".to_string());
            }
            let ptr = (((len & 0x3f) as usize) << 8) | packet[p + 1] as usize;
            if ptr >= packet.len() {
                return Err("network protocol error: DNS compression pointer is out of bounds".to_string());
            }
            if ptr >= p {
                return Err("network protocol error: DNS compression pointer does not point backward".to_string());
            }
            if !jumped {
                *pos = p + 2;
            }
            p = ptr;
            jumped = true;
            continue;
        }
        if len & 0xc0 != 0 {
            return Err("network protocol error: reserved DNS label encoding".to_string());
        }
        p += 1;
        if len == 0 {
            if !jumped {
                *pos = p;
            }
            break;
        }
        if len > 63 {
            return Err("network protocol error: DNS label exceeds 63 bytes".to_string());
        }
        let end = p + len as usize;
        if end > packet.len() {
            return Err("network protocol error: truncated DNS label".to_string());
        }
        wire_len += len as usize + 1;
        if wire_len > 255 {
            return Err("network protocol error: DNS name exceeds 255 bytes".to_string());
        }
        let label = std::str::from_utf8(&packet[p..end])
            .map_err(|_| "network protocol error: DNS label is not valid UTF-8".to_string())?;
        labels.push(label.to_string());
        p = end;
        if !jumped {
            *pos = p;
        }
    }
    Ok(if labels.is_empty() {
        ".".to_string()
    } else {
        labels.join(".")
    })
}

#[derive(Clone)]
struct JetDNSWireRecord {
    owner: String,
    ty: u16,
    class: u16,
    packet: std::sync::Arc<Vec<u8>>,
    rdata_start: usize,
    rdata_len: usize,
}

fn jet_net_dns_txid() -> Result<u16, String> {
    let mut bytes = [0u8; 2];
    jet_crypto_entropy_fill(&mut bytes).map_err(|_| {
        "DNS transaction ID needs operating-system cryptographic randomness".to_string()
    })?;
    Ok(u16::from_be_bytes(bytes))
}

fn jet_net_dns_read_u16(packet: &[u8], at: usize, what: &str) -> Result<u16, String> {
    let bytes = packet
        .get(at..at + 2)
        .ok_or_else(|| format!("network protocol error: truncated DNS {}", what))?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn jet_net_dns_parse_response(
    packet: Vec<u8>,
    txid: u16,
    name: &str,
    qtype: u16,
) -> Result<(bool, Vec<JetDNSWireRecord>), String> {
    if packet.len() < 12 {
        return Err("network protocol error: truncated DNS header".to_string());
    }
    if jet_net_dns_read_u16(&packet, 0, "transaction ID")? != txid {
        return Err("network protocol error: DNS transaction ID mismatch".to_string());
    }
    let flags = jet_net_dns_read_u16(&packet, 2, "flags")?;
    if flags & 0x8000 == 0 {
        return Err("network protocol error: DNS packet is not a response".to_string());
    }
    if flags & 0x7800 != 0 {
        return Err("network protocol error: unsupported DNS opcode".to_string());
    }
    if flags & 0x0040 != 0 {
        return Err("network protocol error: reserved DNS header bit is set".to_string());
    }
    let rcode = flags & 0x000f;
    if rcode == 3 {
        return Err(format!("DNS name not found: `{}`", name));
    }
    if rcode != 0 {
        return Err(format!("DNS server failure for `{}`: RCODE {}", name, rcode));
    }
    let qd = jet_net_dns_read_u16(&packet, 4, "question count")? as usize;
    if qd != 1 {
        return Err("network protocol error: DNS response must echo one question".to_string());
    }
    let total_records = jet_net_dns_read_u16(&packet, 6, "answer count")? as usize
        + jet_net_dns_read_u16(&packet, 8, "authority count")? as usize
        + jet_net_dns_read_u16(&packet, 10, "additional count")? as usize;
    let mut pos = 12usize;
    let echoed_name = jet_net_dns_read_name(&packet, &mut pos)?;
    let echoed_type = jet_net_dns_read_u16(&packet, pos, "question type")?;
    let echoed_class = jet_net_dns_read_u16(&packet, pos + 2, "question class")?;
    pos += 4;
    if !echoed_name.trim_end_matches('.').eq_ignore_ascii_case(name.trim_end_matches('.'))
        || echoed_type != qtype
        || echoed_class != 1
    {
        return Err("network protocol error: DNS question echo mismatch".to_string());
    }
    // TC explicitly means the UDP body is incomplete. The header and echoed
    // question above must still authenticate; declared records cannot be parsed
    // safely until the bounded TCP retry supplies the complete message.
    if flags & 0x0200 != 0 {
        return Ok((true, Vec::new()));
    }
    let minimum_record_bytes = total_records.checked_mul(11)
        .ok_or_else(|| "network protocol error: DNS record count overflows packet bounds".to_string())?;
    if minimum_record_bytes > packet.len().saturating_sub(pos) {
        return Err("network protocol error: DNS record counts exceed packet bounds".to_string());
    }
    let packet = std::sync::Arc::new(packet);
    let mut records = Vec::with_capacity(total_records);
    for _ in 0..total_records {
        let owner = jet_net_dns_read_name(packet.as_slice(), &mut pos)?;
        let ty = jet_net_dns_read_u16(packet.as_slice(), pos, "record type")?;
        let class = jet_net_dns_read_u16(packet.as_slice(), pos + 2, "record class")?;
        let rdata_len = jet_net_dns_read_u16(packet.as_slice(), pos + 8, "record length")? as usize;
        pos += 10;
        let end = pos.checked_add(rdata_len)
            .filter(|end| *end <= packet.len())
            .ok_or_else(|| "network protocol error: truncated DNS record data".to_string())?;
        records.push(JetDNSWireRecord {
            owner,
            ty,
            class,
            packet: packet.clone(),
            rdata_start: pos,
            rdata_len,
        });
        pos = end;
    }
    if pos != packet.len() {
        return Err("network protocol error: trailing bytes after DNS records".to_string());
    }
    Ok((false, records))
}

fn jet_net_dns_cancelled() -> bool {
    jet_scheduler_task_cancelled() && !jet_scheduler_shielded()
}

fn jet_net_dns_remaining(deadline: std::time::Instant, name: &str) -> Result<std::time::Duration, String> {
    if jet_net_dns_cancelled() {
        return Err(format!("network operation cancelled during DNS lookup for `{}`", name));
    }
    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    if remaining.is_zero() {
        return Err(format!("network timeout during DNS lookup for `{}`", name));
    }
    Ok(remaining)
}

fn jet_net_dns_io_slice(deadline: std::time::Instant, name: &str) -> Result<std::time::Duration, String> {
    Ok(jet_net_dns_remaining(deadline, name)?.min(std::time::Duration::from_millis(10)))
}

fn jet_net_dns_tcp_exchange(
    server_addr: std::net::SocketAddr,
    request: &[u8],
    deadline: std::time::Instant,
    name: &str,
) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(
        &server_addr,
        jet_net_dns_remaining(deadline, name)?,
    )
        .map_err(|e| format!("DNS TCP connect to `{}` failed: {}", server_addr, e))?;
    let len = u16::try_from(request.len()).map_err(|_| "network protocol error: DNS request exceeds TCP frame".to_string())?;
    let mut framed = Vec::with_capacity(request.len() + 2);
    framed.extend_from_slice(&len.to_be_bytes());
    framed.extend_from_slice(request);
    let mut written = 0usize;
    while written < framed.len() {
        stream.set_write_timeout(Some(jet_net_dns_io_slice(deadline, name)?))
            .map_err(|e| format!("DNS TCP timeout setup failed: {}", e))?;
        match stream.write(&framed[written..]) {
            Ok(0) => return Err("network protocol error: DNS TCP connection closed while sending query".to_string()),
            Ok(n) => written += n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => continue,
            Err(error) => return Err(format!("DNS TCP query send failed: {}", error)),
        }
    }
    let mut prefix = [0u8; 2];
    let mut prefix_read = 0usize;
    while prefix_read < prefix.len() {
        stream.set_read_timeout(Some(jet_net_dns_io_slice(deadline, name)?))
            .map_err(|e| format!("DNS TCP timeout setup failed: {}", e))?;
        match stream.read(&mut prefix[prefix_read..]) {
            Ok(0) => return Err("network protocol error: DNS TCP connection closed before response length".to_string()),
            Ok(n) => prefix_read += n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => continue,
            Err(error) => return Err(format!("DNS TCP response length failed: {}", error)),
        }
    }
    let response_len = u16::from_be_bytes(prefix) as usize;
    if response_len < 12 || response_len > 65535 {
        return Err("network protocol error: invalid DNS TCP response length".to_string());
    }
    let mut packet = vec![0u8; response_len];
    let mut read = 0usize;
    while read < packet.len() {
        stream.set_read_timeout(Some(jet_net_dns_io_slice(deadline, name)?))
            .map_err(|e| format!("DNS TCP timeout setup failed: {}", e))?;
        match stream.read(&mut packet[read..]) {
            Ok(0) => return Err("network protocol error: DNS TCP response is truncated".to_string()),
            Ok(n) => read += n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => continue,
            Err(error) => return Err(format!("DNS TCP response truncated: {}", error)),
        }
    }
    Ok(packet)
}

fn jet_net_dns_query(server: &String, name: &String, qtype: u16, ms: i64) -> Result<Vec<JetDNSWireRecord>, String> {
    let configured = jet_net_timeout(ms)?;
    let timeout = match jet_deadline_remaining_ms() {
        Some(remaining) if remaining <= 0 => return Err(format!("network timeout during DNS lookup for `{}`", name)),
        Some(remaining) => configured.min(std::time::Duration::from_millis(remaining as u64)),
        None => configured,
    };
    let deadline = std::time::Instant::now().checked_add(timeout)
        .ok_or_else(|| "network timeout exceeds platform range".to_string())?;
    jet_net_dns_remaining(deadline, name)?;
    let server_addr = server
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("invalid DNS server `{}`: {}", server, e))?;
    let bind_addr = if server_addr.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
    let socket = std::net::UdpSocket::bind(bind_addr)
        .map_err(|e| format!("dns socket bind failed: {}", e))?;
    socket.connect(server_addr).map_err(|e| format!("DNS server `{}` is unreachable: {}", server, e))?;
    let mut req = Vec::new();
    let txid = jet_net_dns_txid()?;
    req.extend_from_slice(&txid.to_be_bytes());
    req.extend_from_slice(&0x0100u16.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    jet_net_dns_encode_name(&mut req, name)?;
    req.extend_from_slice(&qtype.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    socket
        .send(&req)
        .map_err(|e| format!("dns query send failed: {}", e))?;
    let mut packet = vec![0u8; 65535];
    let n = loop {
        socket.set_read_timeout(Some(jet_net_dns_io_slice(deadline, name)?))
            .map_err(|e| format!("dns timeout setup failed: {}", e))?;
        match socket.recv(&mut packet) {
            Ok(n) => break n,
            Err(error) if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => continue,
            Err(error) => return Err(format!("dns query for `{}` failed: {}", name, error)),
        }
    };
    packet.truncate(n);
    let (truncated, records) = jet_net_dns_parse_response(packet, txid, name, qtype)?;
    if truncated {
        let packet = jet_net_dns_tcp_exchange(server_addr, &req, deadline, name)?;
        let (still_truncated, records) = jet_net_dns_parse_response(packet, txid, name, qtype)?;
        if still_truncated {
            return Err("network protocol error: DNS TCP response is truncated".to_string());
        }
        return Ok(records);
    }
    Ok(records)
}

fn jet_net_dns_record_name(record: &JetDNSWireRecord) -> Result<String, String> {
    let mut pos = record.rdata_start;
    let name = jet_net_dns_read_name(record.packet.as_slice(), &mut pos)?;
    if pos != record.rdata_start + record.rdata_len {
        return Err("network protocol error: DNS name record length mismatch".to_string());
    }
    Ok(name)
}

fn jet_net_dns_matching_records(records: &[JetDNSWireRecord], name: &str, qtype: u16) -> Result<Vec<JetDNSWireRecord>, String> {
    let mut current = name.trim_end_matches('.').to_string();
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..16 {
        if !visited.insert(current.to_ascii_lowercase()) {
            return Err("network protocol error: cyclic DNS CNAME chain".to_string());
        }
        let found: Vec<_> = records.iter().filter(|r| r.class == 1 && r.ty == qtype && r.owner.eq_ignore_ascii_case(&current)).cloned().collect();
        if !found.is_empty() {
            return Ok(found);
        }
        let alias = records.iter().find(|r| r.class == 1 && r.ty == 5 && r.owner.eq_ignore_ascii_case(&current));
        match alias {
            Some(record) => current = jet_net_dns_record_name(record)?.trim_end_matches('.').to_string(),
            None => return Ok(Vec::new()),
        }
    }
    Err("network protocol error: DNS CNAME chain exceeds 16 records".to_string())
}

fn jet_net_dns_system_lookup(name: &String, ms: i64) -> Result<Vec<std::net::SocketAddr>, String> {
    let configured = jet_net_timeout(ms)?;
    let timeout = match jet_deadline_remaining_ms() {
        Some(remaining) if remaining <= 0 => {
            return Err(format!("network timeout during DNS lookup for `{}`", name));
        }
        Some(remaining) => configured.min(std::time::Duration::from_millis(remaining as u64)),
        None => configured,
    };
    let owned_name = name.clone();
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        use std::net::ToSocketAddrs;
        let result = (owned_name.as_str(), 0)
            .to_socket_addrs()
            .map(|iter| iter.collect::<Vec<_>>())
            .map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    let deadline = std::time::Instant::now().checked_add(timeout)
        .ok_or_else(|| "network timeout exceeds platform range".to_string())?;
    loop {
        let slice = jet_net_dns_io_slice(deadline, name)?;
        match rx.recv_timeout(slice) {
            Ok(result) => return result.map_err(|e| format!("DNS lookup for `{}` failed: {}", name, e)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(format!("DNS lookup worker for `{}` failed", name));
            }
        }
    }
}

fn jet_net_dns_a(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    let mut out: Vec<_> = jet_net_dns_system_lookup(name, ms)?
        .into_iter().filter(|a| a.is_ipv4()).map(|a| JetIpAddr { inner: a.ip() }).collect();
    out.sort_by_key(|a| a.inner);
    out.dedup_by_key(|a| a.inner);
    Ok(out)
}

fn jet_net_dns_aaaa(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    let mut out: Vec<_> = jet_net_dns_system_lookup(name, ms)?
        .into_iter().filter(|a| a.is_ipv6()).map(|a| JetIpAddr { inner: a.ip() }).collect();
    out.sort_by_key(|a| a.inner);
    out.dedup_by_key(|a| a.inner);
    Ok(out)
}

fn jet_net_dns_a_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_matching_records(&jet_net_dns_query(server, name, 1, ms)?, name, 1)?
        .into_iter()
        .map(|r| {
            let data = &r.packet[r.rdata_start..r.rdata_start + r.rdata_len];
            if data.len() != 4 { return Err("network protocol error: DNS A record is not 4 bytes".to_string()); }
            Ok(JetIpAddr {
            inner: std::net::IpAddr::V4(std::net::Ipv4Addr::new(data[0], data[1], data[2], data[3])),
        })
        })
        .collect::<Result<Vec<_>, _>>()?)
}

fn jet_net_dns_aaaa_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_matching_records(&jet_net_dns_query(server, name, 28, ms)?, name, 28)?
        .into_iter()
        .map(|r| {
            let data = &r.packet[r.rdata_start..r.rdata_start + r.rdata_len];
            if data.len() != 16 { return Err("network protocol error: DNS AAAA record is not 16 bytes".to_string()); }
            let mut b = [0u8; 16];
            b.copy_from_slice(data);
            Ok(JetIpAddr {
                inner: std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)),
            })
        })
        .collect::<Result<Vec<_>, _>>()?)
}

fn jet_net_dns_txt(name: &String, ms: i64) -> Result<Vec<String>, String> {
    let deadline = std::time::Instant::now() + jet_net_timeout(ms)?;
    let servers = jet_net_dns_system_servers();
    if servers.is_empty() {
        return Err("host DNS configuration has no resolver for TXT lookup".to_string());
    }
    let mut last = String::new();
    for server in servers {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() { return Err(format!("network timeout during DNS lookup for `{}`", name)); }
        match jet_net_dns_txt_at(&server, name, remaining.as_millis().min(i64::MAX as u128) as i64) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn jet_net_dns_txt_at(server: &String, name: &String, ms: i64) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let records = jet_net_dns_query(server, name, 16, ms)?;
    for r in jet_net_dns_matching_records(&records, name, 16)? {
        let data = &r.packet[r.rdata_start..r.rdata_start + r.rdata_len];
        let mut p = 0usize;
        let mut s = String::new();
        while p < data.len() {
            let len = data[p] as usize;
            p += 1;
            if p + len > data.len() {
                return Err("network protocol error: truncated DNS TXT record".to_string());
            }
            let part = std::str::from_utf8(&data[p..p + len])
                .map_err(|_| "network protocol error: DNS TXT record is not valid UTF-8".to_string())?;
            s.push_str(part);
            p += len;
        }
        out.push(s);
    }
    Ok(out)
}

fn jet_net_dns_ptr(name: &String, ms: i64) -> Result<Vec<String>, String> {
    let deadline = std::time::Instant::now() + jet_net_timeout(ms)?;
    let servers = jet_net_dns_system_servers();
    if servers.is_empty() {
        return Err("host DNS configuration has no resolver for PTR lookup".to_string());
    }
    let mut last = String::new();
    for server in servers {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("network timeout during DNS lookup for `{name}`"));
        }
        match jet_net_dns_ptr_at(
            &server,
            name,
            remaining.as_millis().min(i64::MAX as u128) as i64,
        ) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn jet_net_dns_ptr_at(server: &String, name: &String, ms: i64) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let records = jet_net_dns_query(server, name, 12, ms)?;
    for r in jet_net_dns_matching_records(&records, name, 12)? {
        out.push(jet_net_dns_record_name(&r)?);
    }
    Ok(out)
}

fn jet_net_dns_srv(name: &String, ms: i64) -> Result<Vec<JetDNSSrv>, String> {
    let deadline = std::time::Instant::now() + jet_net_timeout(ms)?;
    let servers = jet_net_dns_system_servers();
    if servers.is_empty() {
        return Err("host DNS configuration has no resolver for SRV lookup".to_string());
    }
    let mut last = String::new();
    for server in servers {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() { return Err(format!("network timeout during DNS lookup for `{}`", name)); }
        match jet_net_dns_srv_at(&server, name, remaining.as_millis().min(i64::MAX as u128) as i64) {
            Ok(v) => return Ok(v),
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn jet_net_dns_srv_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetDNSSrv>, String> {
    let records = jet_net_dns_query(server, name, 33, ms)?;
    let mut out = Vec::new();
    for r in jet_net_dns_matching_records(&records, name, 33)? {
        if r.rdata_len < 7 {
            return Err("network protocol error: truncated DNS SRV record".to_string());
        }
        let data = &r.packet[r.rdata_start..r.rdata_start + r.rdata_len];
        let priority = u16::from_be_bytes([data[0], data[1]]) as i64;
        let weight = u16::from_be_bytes([data[2], data[3]]) as i64;
        let port = u16::from_be_bytes([data[4], data[5]]) as i64;
        let mut pos = r.rdata_start + 6;
        let target = jet_net_dns_read_name(r.packet.as_slice(), &mut pos)?;
        if pos != r.rdata_start + r.rdata_len {
            return Err("network protocol error: DNS SRV record length mismatch".to_string());
        }
        out.push(JetDNSSrv {
            priority,
            weight,
            port,
            target,
        });
    }
    Ok(out)
}

fn jet_net_dns_srv_target(srv: &JetDNSSrv) -> String {
    srv.target.clone()
}

fn jet_net_dns_srv_port(srv: &JetDNSSrv) -> i64 {
    srv.port
}

fn jet_net_dns_srv_priority(srv: &JetDNSSrv) -> i64 {
    srv.priority
}

fn jet_net_dns_srv_weight(srv: &JetDNSSrv) -> i64 {
    srv.weight
}

/// Send a well-formed HTTP/1.1 response on a TcpStream and close it.
/// Handles CRLF line endings internally so Jet code doesn't need `\r`.
fn jet_net_tcp_reply(mut stream: JetTCPStream, status: &String, body: &String) -> Result<(), JetNetError> {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    stream.inner.write_all(response.as_bytes())
        .map_err(|e| jet_net_io_error("tcp reply", None, e))?;
    stream.inner.shutdown(std::net::Shutdown::Write)
        .map_err(|e| jet_net_io_error("tcp reply close", None, e))
}
