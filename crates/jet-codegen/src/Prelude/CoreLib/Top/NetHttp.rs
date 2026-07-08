// ── E2-M10: networking (core.net + jet.http) ─────────────────────────────────
// All networking uses std::net only — zero external crates in the prelude (I6).
// TLS (D-NET1) is delivered as the `jet.tls` FFI package and is not included here.

pub struct JetTcpListener {
    inner: std::net::TcpListener,
}

pub struct JetTcpStream {
    inner: std::net::TcpStream,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetIpAddr {
    inner: std::net::IpAddr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JetSocketAddr {
    inner: std::net::SocketAddr,
}

pub struct JetUdpSocket {
    inner: std::net::UdpSocket,
}

#[derive(Clone, Debug)]
pub struct JetUdpPacket {
    data: String,
    addr: JetSocketAddr,
}

#[derive(Clone, Debug)]
pub struct JetDnsSrv {
    priority: i64,
    weight: i64,
    port: i64,
    target: String,
}

#[cfg(unix)]
pub struct JetUnixListener {
    inner: std::os::unix::net::UnixListener,
}

#[cfg(unix)]
pub struct JetUnixStream {
    inner: std::os::unix::net::UnixStream,
}

#[cfg(not(unix))]
pub struct JetUnixListener;

#[cfg(not(unix))]
pub struct JetUnixStream;

pub struct JetTlsStream {
    id: i64,
}

#[derive(Clone)]
pub struct JetHttpRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub params: std::collections::BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct JetHttpResponse {
    pub status: String,
    pub body: String,
    pub headers: std::collections::BTreeMap<String, String>,
}

// D-ROUTE1=A: HTTP router — registration + :param dispatch.
#[derive(Clone)]
enum RouteSegment {
    Static(String),
    Param(String),
}

type JetHttpHandler = Box<dyn Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync>;

struct JetHttpRoute {
    method: String,
    segments: Vec<RouteSegment>,
    handler: JetHttpHandler,
}

pub struct JetHttpRouter {
    routes: Vec<JetHttpRoute>,
}

impl JetShow for JetTcpListener {
    fn jet_show(&self) -> String {
        format!(
            "TcpListener({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
    }
}
impl JetShow for JetTcpStream {
    fn jet_show(&self) -> String {
        format!(
            "TcpStream({})",
            self.inner
                .peer_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
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
impl JetShow for JetUdpSocket {
    fn jet_show(&self) -> String {
        format!(
            "UdpSocket({})",
            self.inner
                .local_addr()
                .map(|a| a.to_string())
                .unwrap_or_default()
        )
    }
}
impl JetShow for JetUdpPacket {
    fn jet_show(&self) -> String {
        format!("UdpPacket({} bytes from {})", self.data.len(), self.addr.inner)
    }
}
impl JetShow for JetDnsSrv {
    fn jet_show(&self) -> String {
        format!(
            "DnsSrv(priority={}, weight={}, port={}, target={})",
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
impl JetShow for JetTlsStream {
    fn jet_show(&self) -> String {
        format!("TlsStream({})", self.id)
    }
}

fn jet_net_timeout(ms: i64) -> Result<std::time::Duration, String> {
    if ms < 0 {
        return Err("network timeout must be non-negative".to_string());
    }
    Ok(std::time::Duration::from_millis(ms as u64))
}

fn jet_net_apply_tcp_deadlines(stream: &std::net::TcpStream, op: &str) {
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded(op);
        }
        let dur = Some(std::time::Duration::from_millis(remaining as u64));
        let _ = stream.set_read_timeout(dur);
        let _ = stream.set_write_timeout(dur);
    }
}

fn jet_net_apply_udp_deadline(socket: &std::net::UdpSocket, op: &str) {
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded(op);
        }
        let dur = Some(std::time::Duration::from_millis(remaining as u64));
        let _ = socket.set_read_timeout(dur);
        let _ = socket.set_write_timeout(dur);
    }
}

fn jet_net_ip_addr(text: &String) -> Result<JetIpAddr, String> {
    text.parse::<std::net::IpAddr>()
        .map(|inner| JetIpAddr { inner })
        .map_err(|e| format!("invalid IP address `{}`: {}", text, e))
}

fn jet_net_ip_to_string(ip: &JetIpAddr) -> String {
    ip.inner.to_string()
}

fn jet_net_ip_is_ipv4(ip: &JetIpAddr) -> bool {
    ip.inner.is_ipv4()
}

fn jet_net_socket_addr(host: &String, port: i64) -> Result<JetSocketAddr, String> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(format!("invalid port `{}`: expected 0..65535", port));
    }
    let text = format!("{}:{}", host, port);
    text.parse::<std::net::SocketAddr>()
        .or_else(|_| {
            use std::net::ToSocketAddrs;
            text.to_socket_addrs()?
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))
        })
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| format!("resolve `{}` failed: {}", text, e))
}

fn jet_net_socket_addr_parse(text: &String) -> Result<JetSocketAddr, String> {
    text.parse::<std::net::SocketAddr>()
        .map(|inner| JetSocketAddr { inner })
        .map_err(|e| format!("invalid socket address `{}`: {}", text, e))
}

fn jet_net_socket_host(addr: &JetSocketAddr) -> String {
    addr.inner.ip().to_string()
}

fn jet_net_socket_port(addr: &JetSocketAddr) -> i64 {
    addr.inner.port() as i64
}

fn jet_net_socket_to_string(addr: &JetSocketAddr) -> String {
    addr.inner.to_string()
}

fn jet_net_tcp_listen_addr(addr: &JetSocketAddr) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.inner)
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_addr(addr: &JetSocketAddr) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.inner)
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_timeout(addr: &JetSocketAddr, ms: i64) -> Result<JetTcpStream, String> {
    let timeout = jet_net_timeout(ms)?;
    std::net::TcpStream::connect_timeout(&addr.inner, timeout)
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr.inner, e))
}

fn jet_net_tcp_connect_happy(host: &String, port: i64, ms: i64) -> Result<JetTcpStream, String> {
    if port < 0 || port > u16::MAX as i64 {
        return Err(format!("invalid port `{}`: expected 0..65535", port));
    }
    let timeout = jet_net_timeout(ms)?;
    let deadline = std::time::Instant::now() + timeout;
    let mut addrs: Vec<std::net::SocketAddr> = {
        use std::net::ToSocketAddrs;
        (host.as_str(), port as u16)
            .to_socket_addrs()
            .map_err(|e| format!("resolve `{}` failed: {}", host, e))?
            .collect()
    };
    addrs.sort_by_key(|a| if a.is_ipv6() { 0 } else { 1 });
    let mut last = "no address".to_string();
    for addr in addrs {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        match std::net::TcpStream::connect_timeout(&addr, deadline.saturating_duration_since(now)) {
            Ok(s) => return Ok(JetTcpStream { inner: s }),
            Err(e) => last = format!("{}: {}", addr, e),
        }
    }
    Err(format!("connect to `{}` failed: {}", host, last))
}

fn jet_net_listener_local_socket_addr(listener: &JetTcpListener) -> JetSocketAddr {
    JetSocketAddr {
        inner: listener
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_tcp_local_socket_addr(stream: &JetTcpStream) -> JetSocketAddr {
    JetSocketAddr {
        inner: stream
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_tcp_peer_socket_addr(stream: &JetTcpStream) -> JetSocketAddr {
    JetSocketAddr {
        inner: stream
            .inner
            .peer_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}
impl JetShow for JetHttpRequest {
    fn jet_show(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}
impl JetShow for JetHttpResponse {
    fn jet_show(&self) -> String {
        format!("HTTP {}", self.status)
    }
}
impl JetShow for JetHttpRouter {
    fn jet_show(&self) -> String {
        format!("HttpRouter({} routes)", self.routes.len())
    }
}

fn jet_net_tcp_listen(addr: &String) -> Result<JetTcpListener, String> {
    std::net::TcpListener::bind(addr.as_str())
        .map(|l| JetTcpListener { inner: l })
        .map_err(|e| format!("bind on `{}` failed: {}", addr, e))
}

fn jet_net_tcp_accept(listener: &JetTcpListener) -> Result<JetTcpStream, String> {
    listener
        .inner
        .accept()
        .map(|(s, _)| JetTcpStream { inner: s })
        .map_err(|e| format!("accept failed: {}", e))
}

fn jet_net_tcp_connect(addr: &String) -> Result<JetTcpStream, String> {
    std::net::TcpStream::connect(addr.as_str())
        .map(|s| JetTcpStream { inner: s })
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))
}

fn jet_net_tcp_read(stream: &mut JetTcpStream) -> Result<String, String> {
    use std::io::Read;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp read");
        }
        let _ = stream
            .inner
            .set_read_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let mut buf = [0u8; 8192];
    loop {
        match stream.inner.read(&mut buf) {
            Ok(0) => return Ok(String::new()),
            Ok(n) => {
                jet_deadline_check("tcp read");
                return String::from_utf8(buf[..n].to_vec())
                    .map_err(|e| format!("tcp read: invalid UTF-8: {}", e));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, true, false, "tcp read");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp read");
                }
                return Err(format!("tcp read failed: {}", e));
            }
        }
    }
}

fn jet_net_tcp_write(stream: &mut JetTcpStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    if let Some(remaining) = jet_deadline_remaining_ms() {
        if remaining <= 0 {
            jet_deadline_exceeded("tcp write");
        }
        let _ = stream
            .inner
            .set_write_timeout(Some(std::time::Duration::from_millis(remaining as u64)));
    }
    let bytes = data.as_bytes();
    let mut off = 0usize;
    while off < bytes.len() {
        match stream.inner.write(&bytes[off..]) {
            Ok(0) => return Err("tcp write failed: zero bytes written".to_string()),
            Ok(n) => off += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                jet_scheduler_io_wait(&stream.inner, false, true, "tcp write");
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut {
                    jet_deadline_exceeded("tcp write");
                }
                return Err(format!("tcp write failed: {}", e));
            }
        }
    }
    jet_deadline_check("tcp write");
    Ok(())
}

fn jet_net_tcp_local_addr(stream: &JetTcpStream) -> String {
    stream
        .inner
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_tcp_peer_addr(stream: &JetTcpStream) -> String {
    stream
        .inner
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_listener_local_addr(listener: &JetTcpListener) -> String {
    listener
        .inner
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn jet_net_set_timeout(stream: &mut JetTcpStream, ms: i64) {
    if let Ok(dur) = jet_net_timeout(ms) {
        let _ = stream.inner.set_read_timeout(Some(dur));
        let _ = stream.inner.set_write_timeout(Some(dur));
    }
}

fn jet_net_set_read_timeout(stream: &mut JetTcpStream, ms: i64) -> Result<(), String> {
    stream
        .inner
        .set_read_timeout(Some(jet_net_timeout(ms)?))
        .map_err(|e| format!("set tcp read timeout failed: {}", e))
}

fn jet_net_set_write_timeout(stream: &mut JetTcpStream, ms: i64) -> Result<(), String> {
    stream
        .inner
        .set_write_timeout(Some(jet_net_timeout(ms)?))
        .map_err(|e| format!("set tcp write timeout failed: {}", e))
}

fn jet_net_udp_bind(addr: &String) -> Result<JetUdpSocket, String> {
    std::net::UdpSocket::bind(addr.as_str())
        .map(|inner| JetUdpSocket { inner })
        .map_err(|e| format!("udp bind on `{}` failed: {}", addr, e))
}

fn jet_net_udp_bind_addr(addr: &JetSocketAddr) -> Result<JetUdpSocket, String> {
    std::net::UdpSocket::bind(addr.inner)
        .map(|inner| JetUdpSocket { inner })
        .map_err(|e| format!("udp bind on `{}` failed: {}", addr.inner, e))
}

fn jet_net_udp_local_addr(socket: &JetUdpSocket) -> JetSocketAddr {
    JetSocketAddr {
        inner: socket
            .inner
            .local_addr()
            .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap()),
    }
}

fn jet_net_udp_set_timeout(socket: &JetUdpSocket, ms: i64) -> Result<(), String> {
    let dur = jet_net_timeout(ms)?;
    socket
        .inner
        .set_read_timeout(Some(dur))
        .map_err(|e| format!("set udp read timeout failed: {}", e))?;
    socket
        .inner
        .set_write_timeout(Some(dur))
        .map_err(|e| format!("set udp write timeout failed: {}", e))
}

fn jet_net_udp_send_to(
    socket: &JetUdpSocket,
    data: &String,
    addr: &JetSocketAddr,
) -> Result<i64, String> {
    jet_net_apply_udp_deadline(&socket.inner, "udp send");
    socket
        .inner
        .send_to(data.as_bytes(), addr.inner)
        .map(|n| n as i64)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                jet_deadline_exceeded("udp send");
            }
            format!("udp send to `{}` failed: {}", addr.inner, e)
        })
}

fn jet_net_udp_recv_from(socket: &JetUdpSocket, limit: i64) -> Result<JetUdpPacket, String> {
    if limit <= 0 {
        return Err("udp receive limit must be positive".to_string());
    }
    jet_net_apply_udp_deadline(&socket.inner, "udp receive");
    let cap = std::cmp::min(limit as usize, 1 << 20);
    let mut buf = vec![0u8; cap];
    socket
        .inner
        .recv_from(&mut buf)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::TimedOut {
                jet_deadline_exceeded("udp receive");
            }
            format!("udp receive failed: {}", e)
        })
        .and_then(|(n, addr)| {
            String::from_utf8(buf[..n].to_vec())
                .map(|data| JetUdpPacket {
                    data,
                    addr: JetSocketAddr { inner: addr },
                })
                .map_err(|e| format!("udp receive: invalid UTF-8: {}", e))
        })
}

fn jet_net_udp_packet_data(packet: &JetUdpPacket) -> String {
    packet.data.clone()
}

fn jet_net_udp_packet_addr(packet: &JetUdpPacket) -> JetSocketAddr {
    packet.addr.clone()
}

#[cfg(unix)]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, String> {
    let _ = std::fs::remove_file(path);
    std::os::unix::net::UnixListener::bind(path)
        .map(|inner| JetUnixListener { inner })
        .map_err(|e| format!("unix listen on `{}` failed: {}", path, e))
}

#[cfg(not(unix))]
fn jet_net_unix_listen(path: &String) -> Result<JetUnixListener, String> {
    Err(format!("unix sockets are not supported on this platform: {}", path))
}

#[cfg(unix)]
fn jet_net_unix_accept(listener: &JetUnixListener) -> Result<JetUnixStream, String> {
    listener
        .inner
        .accept()
        .map(|(inner, _)| JetUnixStream { inner })
        .map_err(|e| format!("unix accept failed: {}", e))
}

#[cfg(not(unix))]
fn jet_net_unix_accept(_listener: &JetUnixListener) -> Result<JetUnixStream, String> {
    Err("unix sockets are not supported on this platform".to_string())
}

#[cfg(unix)]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, String> {
    std::os::unix::net::UnixStream::connect(path)
        .map(|inner| JetUnixStream { inner })
        .map_err(|e| format!("unix connect to `{}` failed: {}", path, e))
}

#[cfg(not(unix))]
fn jet_net_unix_connect(path: &String) -> Result<JetUnixStream, String> {
    Err(format!("unix sockets are not supported on this platform: {}", path))
}

#[cfg(unix)]
fn jet_net_unix_read(stream: &mut JetUnixStream) -> Result<String, String> {
    use std::io::Read;
    let mut buf = [0u8; 8192];
    stream
        .inner
        .read(&mut buf)
        .map_err(|e| format!("unix read failed: {}", e))
        .and_then(|n| {
            String::from_utf8(buf[..n].to_vec())
                .map_err(|e| format!("unix read: invalid UTF-8: {}", e))
        })
}

#[cfg(not(unix))]
fn jet_net_unix_read(_stream: &mut JetUnixStream) -> Result<String, String> {
    Err("unix sockets are not supported on this platform".to_string())
}

#[cfg(unix)]
fn jet_net_unix_write(stream: &mut JetUnixStream, data: &String) -> Result<(), String> {
    use std::io::Write;
    stream
        .inner
        .write_all(data.as_bytes())
        .map_err(|e| format!("unix write failed: {}", e))
}

#[cfg(not(unix))]
fn jet_net_unix_write(_stream: &mut JetUnixStream, _data: &String) -> Result<(), String> {
    Err("unix sockets are not supported on this platform".to_string())
}

fn jet_net_dns_system_servers() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") {
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if parts.next() == Some("nameserver") {
                if let Some(host) = parts.next() {
                    out.push(format!("{}:53", host));
                }
            }
        }
    }
    if out.is_empty() {
        out.push("1.1.1.1:53".to_string());
    }
    out
}

fn jet_net_dns_encode_name(out: &mut Vec<u8>, name: &str) -> Result<(), String> {
    let trimmed = name.trim_end_matches('.');
    if trimmed.is_empty() {
        out.push(0);
        return Ok(());
    }
    for label in trimmed.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(format!("invalid DNS name `{}`", name));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

fn jet_net_dns_read_name(packet: &[u8], pos: &mut usize) -> Result<String, String> {
    let mut labels = Vec::new();
    let mut p = *pos;
    let mut jumped = false;
    let mut seen = 0usize;
    loop {
        if p >= packet.len() {
            return Err("truncated DNS name".to_string());
        }
        let len = packet[p];
        if len & 0xc0 == 0xc0 {
            if p + 1 >= packet.len() {
                return Err("truncated DNS compression pointer".to_string());
            }
            let ptr = (((len & 0x3f) as usize) << 8) | packet[p + 1] as usize;
            if !jumped {
                *pos = p + 2;
            }
            p = ptr;
            jumped = true;
            seen += 1;
            if seen > packet.len() {
                return Err("cyclic DNS compression pointer".to_string());
            }
            continue;
        }
        p += 1;
        if len == 0 {
            if !jumped {
                *pos = p;
            }
            break;
        }
        let end = p + len as usize;
        if end > packet.len() {
            return Err("truncated DNS label".to_string());
        }
        labels.push(String::from_utf8_lossy(&packet[p..end]).to_string());
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

fn jet_net_dns_query(server: &String, name: &String, qtype: u16, ms: i64) -> Result<Vec<Vec<u8>>, String> {
    let timeout = jet_net_timeout(ms)?;
    let server_addr = server
        .parse::<std::net::SocketAddr>()
        .map_err(|e| format!("invalid DNS server `{}`: {}", server, e))?;
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")
        .map_err(|e| format!("dns socket bind failed: {}", e))?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("dns timeout setup failed: {}", e))?;
    let mut req = Vec::new();
    req.extend_from_slice(&0x4a57u16.to_be_bytes());
    req.extend_from_slice(&0x0100u16.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    req.extend_from_slice(&0u16.to_be_bytes());
    jet_net_dns_encode_name(&mut req, name)?;
    req.extend_from_slice(&qtype.to_be_bytes());
    req.extend_from_slice(&1u16.to_be_bytes());
    socket
        .send_to(&req, server_addr)
        .map_err(|e| format!("dns query send failed: {}", e))?;
    let mut packet = vec![0u8; 4096];
    let (n, _) = socket
        .recv_from(&mut packet)
        .map_err(|e| format!("dns query for `{}` failed: {}", name, e))?;
    packet.truncate(n);
    if packet.len() < 12 {
        return Err("truncated DNS response".to_string());
    }
    let an = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let mut pos = 12usize;
    let _ = jet_net_dns_read_name(&packet, &mut pos)?;
    pos += 4;
    let mut out = Vec::new();
    for _ in 0..an {
        let _ = jet_net_dns_read_name(&packet, &mut pos)?;
        if pos + 10 > packet.len() {
            return Err("truncated DNS answer".to_string());
        }
        let ty = u16::from_be_bytes([packet[pos], packet[pos + 1]]);
        let class = u16::from_be_bytes([packet[pos + 2], packet[pos + 3]]);
        let rdlen = u16::from_be_bytes([packet[pos + 8], packet[pos + 9]]) as usize;
        pos += 10;
        if pos + rdlen > packet.len() {
            return Err("truncated DNS rdata".to_string());
        }
        if ty == qtype && class == 1 {
            out.push(packet[pos..pos + rdlen].to_vec());
        }
        pos += rdlen;
    }
    Ok(out)
}

fn jet_net_dns_a(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_a_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS A lookup for `{}` failed", name))
}

fn jet_net_dns_aaaa(name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_aaaa_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS AAAA lookup for `{}` failed", name))
}

fn jet_net_dns_a_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_query(server, name, 1, ms)?
        .into_iter()
        .filter(|r| r.len() == 4)
        .map(|r| JetIpAddr {
            inner: std::net::IpAddr::V4(std::net::Ipv4Addr::new(r[0], r[1], r[2], r[3])),
        })
        .collect())
}

fn jet_net_dns_aaaa_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetIpAddr>, String> {
    Ok(jet_net_dns_query(server, name, 28, ms)?
        .into_iter()
        .filter(|r| r.len() == 16)
        .map(|r| {
            let mut b = [0u8; 16];
            b.copy_from_slice(&r);
            JetIpAddr {
                inner: std::net::IpAddr::V6(std::net::Ipv6Addr::from(b)),
            }
        })
        .collect())
}

fn jet_net_dns_txt(name: &String, ms: i64) -> Result<Vec<String>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_txt_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS TXT lookup for `{}` failed", name))
}

fn jet_net_dns_txt_at(server: &String, name: &String, ms: i64) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for r in jet_net_dns_query(server, name, 16, ms)? {
        let mut p = 0usize;
        let mut s = String::new();
        while p < r.len() {
            let len = r[p] as usize;
            p += 1;
            if p + len > r.len() {
                return Err("truncated DNS TXT record".to_string());
            }
            s.push_str(&String::from_utf8_lossy(&r[p..p + len]));
            p += len;
        }
        out.push(s);
    }
    Ok(out)
}

fn jet_net_dns_srv(name: &String, ms: i64) -> Result<Vec<JetDnsSrv>, String> {
    for server in jet_net_dns_system_servers() {
        if let Ok(v) = jet_net_dns_srv_at(&server, name, ms) {
            return Ok(v);
        }
    }
    Err(format!("DNS SRV lookup for `{}` failed", name))
}

fn jet_net_dns_srv_at(server: &String, name: &String, ms: i64) -> Result<Vec<JetDnsSrv>, String> {
    let packets = jet_net_dns_query(server, name, 33, ms)?;
    let mut out = Vec::new();
    for r in packets {
        if r.len() < 7 {
            return Err("truncated DNS SRV record".to_string());
        }
        let priority = u16::from_be_bytes([r[0], r[1]]) as i64;
        let weight = u16::from_be_bytes([r[2], r[3]]) as i64;
        let port = u16::from_be_bytes([r[4], r[5]]) as i64;
        let mut pos = 6usize;
        let target = jet_net_dns_read_name(&r, &mut pos)?;
        out.push(JetDnsSrv {
            priority,
            weight,
            port,
            target,
        });
    }
    Ok(out)
}

fn jet_net_dns_srv_target(srv: &JetDnsSrv) -> String {
    srv.target.clone()
}

fn jet_net_dns_srv_port(srv: &JetDnsSrv) -> i64 {
    srv.port
}

fn jet_net_dns_srv_priority(srv: &JetDnsSrv) -> i64 {
    srv.priority
}

fn jet_net_dns_srv_weight(srv: &JetDnsSrv) -> i64 {
    srv.weight
}

/// Send a well-formed HTTP/1.1 response on a TcpStream and close it.
/// Handles CRLF line endings internally so Jet code doesn't need `\r`.
fn jet_net_tcp_reply(mut stream: JetTcpStream, status: &String, body: &String) {
    use std::io::Write;
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    let _ = stream.inner.write_all(response.as_bytes());
}

// ── HTTP/1.1 client (minimal, over std::net::TcpStream) ──────────────────────

fn jet_http_get(url: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "GET", &[], "")
}

fn jet_http_post(url: &String, body: &String) -> Result<JetHttpResponse, String> {
    jet_http_request(url, "POST", &[], body.as_str())
}

fn jet_http_request(
    url: &str,
    method: &str,
    extra_headers: &[(&str, &str)],
    body: &str,
) -> Result<JetHttpResponse, String> {
    use std::io::{Read, Write};
    // Parse URL: http://host[:port]/path
    let url_str = url;
    let (host_port, path) = if let Some(rest) = url_str.strip_prefix("http://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let hp = &rest[..slash];
        let p = if slash < rest.len() {
            &rest[slash..]
        } else {
            "/"
        };
        (hp.to_string(), p.to_string())
    } else if let Some(rest) = url_str.strip_prefix("https://") {
        return Err("HTTPS requires the `jet.tls` package; this is plain HTTP. Add `jet.tls` to your pkg.jet to enable HTTPS.".to_string());
        // Keep the variable to silence unused warning in case we extend later.
        #[allow(unreachable_code)]
        {
            (rest.to_string(), "/".to_string())
        }
    } else {
        return Err(format!("URL must start with http:// — got `{}`", url));
    };
    // Default port 80 if not specified.
    let addr = if host_port.contains(':') {
        host_port.clone()
    } else {
        format!("{}:80", host_port)
    };
    let host = host_port.split(':').next().unwrap_or(&host_port);
    let mut stream = std::net::TcpStream::connect(&addr)
        .map_err(|e| format!("connect to `{}` failed: {}", addr, e))?;
    // Build HTTP/1.1 request.
    let content_len = body.len();
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Jet/1.0\r\nConnection: close\r\n",
        method, path, host
    );
    if !body.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", content_len));
    }
    for (k, v) in extra_headers {
        req.push_str(&format!("{}: {}\r\n", k, v));
    }
    req.push_str("\r\n");
    if !body.is_empty() {
        req.push_str(body);
    }
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("http write failed: {}", e))?;
    // Read response.
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("http read failed: {}", e))?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    // Parse status line + headers + body.
    let sep = text.find("\r\n\r\n").unwrap_or(text.len());
    let header_part = &text[..sep];
    let body_part = if sep + 4 <= text.len() {
        text[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let status_line = lines.next().unwrap_or("HTTP/1.1 200 OK");
    let status = status_line
        .splitn(2, ' ')
        .nth(1)
        .unwrap_or("200 OK")
        .to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    Ok(JetHttpResponse {
        status,
        body: body_part,
        headers,
    })
}

// ── HTTP/1.1 server (blocking, one thread per connection) ────────────────────
// note: `jet serve` uses one task per connection. This is excellent for internal
//       services and tools at hundreds of concurrent connections. For very high
//       connection counts, Jet is not the right tool yet — see docs/services.md.

fn jet_http_serve<F>(addr: &String, handler: F)
where
    F: Fn(JetHttpRequest) -> JetHttpResponse + Send + Sync + 'static,
{
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str()).unwrap_or_else(|e| {
        eprintln!("E2801: bind on `{}` failed: {}", addr, e);
        std::process::exit(1);
    });
    let handler = std::sync::Arc::new(handler);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("E2801: accept failed: {}", e);
                continue;
            }
        };
        let h = handler.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = h(req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_parse_request(raw: &str) -> JetHttpRequest {
    let sep = raw.find("\r\n\r\n").unwrap_or(raw.len());
    let header_part = &raw[..sep];
    let body = if sep + 4 <= raw.len() {
        raw[sep + 4..].to_string()
    } else {
        String::new()
    };
    let mut lines = header_part.lines();
    let request_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts = request_line.splitn(3, ' ');
    let method = parts.next().unwrap_or("GET").to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut headers = std::collections::BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(": ") {
            headers.insert(k.to_lowercase(), v.to_string());
        }
    }
    JetHttpRequest {
        method,
        path,
        body,
        headers,
        params: std::collections::BTreeMap::new(),
    }
}

fn jet_http_format_response(resp: &JetHttpResponse) -> String {
    let mut out = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        resp.status,
        resp.body.len()
    );
    for (k, v) in &resp.headers {
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    out.push_str("\r\n");
    out.push_str(&resp.body);
    out
}

// D-ROUTE1=A: router runtime ──────────────────────────────────────────────────

fn jet_http_router_new() -> JetHttpRouter {
    JetHttpRouter { routes: Vec::new() }
}

fn jet_http_router_parse_pattern(pattern: &str) -> Vec<RouteSegment> {
    pattern
        .split('/')
        .filter_map(|seg| {
            if seg.is_empty() {
                return None;
            }
            if let Some(name) = seg.strip_prefix(':') {
                Some(RouteSegment::Param(name.to_string()))
            } else {
                Some(RouteSegment::Static(seg.to_string()))
            }
        })
        .collect()
}

fn jet_http_router_register(
    router: &mut JetHttpRouter,
    method: String,
    pattern: String,
    handler: JetHttpHandler,
    file: &str,
    line: u32,
) {
    // E2804 (runtime): duplicate method+pattern fails at registration time in
    // Jet-owned runtime voice, not a raw Rust panic banner.
    let segs = jet_http_router_parse_pattern(&pattern);
    let is_dup = router.routes.iter().any(|r| {
        r.method == method
            && r.segments.len() == segs.len()
            && r.segments
                .iter()
                .zip(segs.iter())
                .all(|(a, b)| match (a, b) {
                    (RouteSegment::Static(x), RouteSegment::Static(y)) => x == y,
                    (RouteSegment::Param(_), RouteSegment::Param(_)) => true,
                    _ => false,
                })
    });
    if is_dup {
        jet_panic(
            file,
            line,
            &format!("E2804: duplicate route `{} {}`", method, pattern),
        );
    }
    router.routes.push(JetHttpRoute {
        method,
        segments: segs,
        handler,
    });
}

/// Count static segments in a route (for precedence: more statics win).
fn route_static_count(segs: &[RouteSegment]) -> usize {
    segs.iter()
        .filter(|s| matches!(s, RouteSegment::Static(_)))
        .count()
}

fn jet_http_router_dispatch(router: &JetHttpRouter, req: JetHttpRequest) -> JetHttpResponse {
    let path_segs: Vec<&str> = req.path.split('/').filter(|s| !s.is_empty()).collect();
    // Collect matching routes with their static count (for precedence).
    let mut candidates: Vec<(usize, usize)> = Vec::new(); // (route_idx, static_count)
    for (i, route) in router.routes.iter().enumerate() {
        if route.segments.len() != path_segs.len() {
            continue;
        }
        let mut ok = true;
        for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
            if let RouteSegment::Static(s) = rseg {
                if s != pseg {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            candidates.push((i, route_static_count(&route.segments)));
        }
    }
    if candidates.is_empty() {
        return JetHttpResponse {
            status: "404 Not Found".to_string(),
            body: "404 not found".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    }
    // Pick highest static-count match with the right method; otherwise 405.
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    let method_match = candidates
        .iter()
        .find(|(i, _)| router.routes[*i].method == req.method);
    let Some((route_idx, _)) = method_match.copied() else {
        return JetHttpResponse {
            status: "405 Method Not Allowed".to_string(),
            body: "405 method not allowed".to_string(),
            headers: std::collections::BTreeMap::new(),
        };
    };
    let route = &router.routes[route_idx];
    let mut params = std::collections::BTreeMap::new();
    for (rseg, pseg) in route.segments.iter().zip(path_segs.iter()) {
        if let RouteSegment::Param(name) = rseg {
            params.insert(name.clone(), pseg.to_string());
        }
    }
    let mut req2 = req;
    req2.params = params;
    (route.handler)(req2)
}

fn jet_http_serve_router(addr: &String, router: JetHttpRouter) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(addr.as_str()).unwrap_or_else(|e| {
        eprintln!("E2801: bind on `{}` failed: {}", addr, e);
        std::process::exit(1);
    });
    let router = std::sync::Arc::new(router);
    loop {
        let (mut stream, _peer) = match listener.accept() {
            Ok(x) => x,
            Err(e) => {
                eprintln!("E2801: accept failed: {}", e);
                continue;
            }
        };
        let r = router.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let req = jet_http_parse_request(&raw);
            let resp = jet_http_router_dispatch(&r, req);
            let response_text = jet_http_format_response(&resp);
            let _ = stream.write_all(response_text.as_bytes());
        });
    }
}

fn jet_http_request_param(req: &JetHttpRequest, name: &String) -> Option<String> {
    req.params.get(name.as_str()).cloned()
}

