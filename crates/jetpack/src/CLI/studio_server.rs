use super::parse::Parsed;
use super::studio_transactions::{
    handle_studio_run, handle_studio_transaction, studio_live_projection, StudioAppliedChange,
    StudioChangeSet, StudioProvedSource,
};
use crate::Output::Theme;
use crate::Syntax;
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub(super) struct StudioContext {
    pub(super) config: PathBuf,
    pub(super) host: String,
    pub(super) offline: bool,
    pub(super) session_secret: String,
    pub(super) source_write: std::sync::Mutex<()>,
    pub(super) sessions: std::sync::Mutex<std::collections::BTreeSet<String>>,
    pub(super) changeset: std::sync::Mutex<Option<StudioChangeSet>>,
    pub(super) last_applied: std::sync::Mutex<Option<StudioAppliedChange>>,
    pub(super) live_projection: std::sync::Mutex<Option<String>>,
    pub(super) proved_source: std::sync::Mutex<Option<StudioProvedSource>>,
}

const STUDIO_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub(super) fn studio_host(parsed: &Parsed) -> Option<String> {
    parsed.flags.studio_host.clone()
}

pub(super) fn studio_context(parsed: &Parsed) -> Option<StudioContext> {
    let project = parsed
        .positional
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let config = if project.is_dir() {
        project.join(Syntax::CONFIG_FILE)
    } else {
        project
    };
    let host = studio_host(parsed).unwrap_or_else(|| "host".to_string());
    let offline = parsed.flags.offline;
    let session_secret = studio_session_secret()?;
    Some(StudioContext {
        config,
        host,
        offline,
        session_secret,
        source_write: std::sync::Mutex::new(()),
        sessions: std::sync::Mutex::new(std::collections::BTreeSet::new()),
        changeset: std::sync::Mutex::new(None),
        last_applied: std::sync::Mutex::new(None),
        live_projection: std::sync::Mutex::new(None),
        proved_source: std::sync::Mutex::new(None),
    })
}

pub(super) fn serve_studio(
    theme: &Theme,
    addr: &str,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: Option<&StudioContext>,
    open_browser: bool,
) -> i32 {
    let Some(context) = context else {
        theme.error(
            "jetos Studio could not start its secure service",
            "the operating system did not provide a secure session secret",
            "retry `jetos studio` after restoring the OS random source.",
        );
        return 2;
    };
    let listener = match std::net::TcpListener::bind(addr) {
        Ok(listener) => listener,
        Err(e) => {
            theme.error(
                "jetos Studio could not bind the local service",
                &format!("binding `{addr}` failed: {e}"),
                "choose a free loopback address, for example `--serve 127.0.0.1:7417`.",
            );
            return 2;
        }
    };
    let local_addr = match listener.local_addr() {
        Ok(addr) => addr,
        Err(error) => {
            theme.error(
                "jetos Studio could not start its secure service",
                &format!("reading the bound address failed: {error}"),
                "retry `jetos studio --serve`.",
            );
            return 2;
        }
    };
    let local = local_addr.to_string();
    let url = format!("http://{local}/studio/?session={}", context.session_secret);
    println!("{url}");
    theme.ok("Jetos Studio Service Listening");
    if open_browser {
        match std::process::Command::new("xdg-open").arg(&url).spawn() {
            Ok(_) => theme.ok("Opened Jetos Studio"),
            Err(_) => {
                theme.detail("open the printed Studio URL in a browser.");
            }
        }
    }
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let _ = configure_studio_stream(&stream);
                let _ = handle_studio_request(&mut stream, app, meta, data, context, local_addr);
            }
            Err(e) => {
                theme.error(
                    "jetos Studio service connection failed",
                    &format!("accepting a local connection failed: {e}"),
                    "restart `jetos studio --serve`.",
                );
                return 2;
            }
        }
    }
    0
}

fn configure_studio_stream(stream: &std::net::TcpStream) -> std::io::Result<()> {
    stream.set_read_timeout(Some(STUDIO_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(STUDIO_IO_TIMEOUT))
}

fn handle_studio_request(
    stream: &mut std::net::TcpStream,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: &StudioContext,
    local_addr: SocketAddr,
) -> std::io::Result<()> {
    let request_bytes = read_http_request(stream)?;
    let Some(request) = parse_studio_request(&request_bytes) else {
        return studio_write_response(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            b"malformed Studio request\n",
            None,
        );
    };
    if !studio_request_authorized(&request, context, local_addr) {
        return studio_write_response(
            stream,
            "401 Unauthorized",
            "text/plain; charset=utf-8",
            b"Studio session, host, or origin rejected\n",
            None,
        );
    }
    let method = request.method.as_str();
    let path = request.target.split('?').next().unwrap_or("/");
    if method == "POST" && path == "/studio/transaction" {
        let (status, body) = handle_studio_transaction(&request.body, Some(context));
        return studio_write_response(stream, status, "application/json", body.as_bytes(), None);
    }
    if method == "POST" && path == "/studio/run" {
        let (status, body) = handle_studio_run(&request.body, Some(context));
        return studio_write_response(stream, status, "application/json", body.as_bytes(), None);
    }
    let (status, content_type, body, set_cookie) = match path {
        "/" | "/studio" | "/studio/" | "/studio/index.html" => {
            (
                "200 OK",
                "text/html; charset=utf-8",
                fs_read_for_http(app),
                Some(context.session_secret.as_str()),
            )
        }
        "/studio/app.json" => (
            "200 OK",
            "application/json",
            fs_read_for_http(meta),
            None,
        ),
        "/studio/data.json" => match studio_live_projection(context, data) {
            Ok(body) => ("200 OK", "application/json", body.into_bytes(), None),
            Err(body) => (
                "500 Internal Server Error",
                "application/json",
                body.into_bytes(),
                None,
            ),
        },
        "/studio/source" => (
            "200 OK",
            "text/plain; charset=utf-8",
            fs_read_for_http(&context.config),
            None,
        ),
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found\n".to_vec(),
            None,
        ),
    };
    studio_write_response(stream, status, content_type, &body, set_cookie)
}

struct StudioRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: String,
}

fn studio_session_secret() -> Option<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = crate::TrustRoot::os_random_bytes::<32>().ok()?;
    let mut secret = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        secret.push(HEX[(byte >> 4) as usize] as char);
        secret.push(HEX[(byte & 0x0f) as usize] as char);
    }
    Some(secret)
}

fn parse_studio_request(bytes: &[u8]) -> Option<StudioRequest> {
    let header_end = bytes.windows(4).position(|window| window == b"\r\n\r\n")?;
    let header_end = header_end + 4;
    let headers = std::str::from_utf8(&bytes[..header_end]).ok()?;
    let mut lines = headers.split("\r\n");
    let mut request_line = lines.next()?.split_whitespace();
    let method = request_line.next()?;
    let target = request_line.next()?;
    if request_line.next()? != "HTTP/1.1" || request_line.next().is_some() {
        return None;
    }
    let mut parsed_headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty()
            || parsed_headers
                .insert(name, value.trim().to_string())
                .is_some()
        {
            return None;
        }
    }
    let body = String::from_utf8_lossy(&bytes[header_end..]).into_owned();
    Some(StudioRequest {
        method: method.to_string(),
        target: target.to_string(),
        headers: parsed_headers,
        body,
    })
}

fn studio_request_authorized(
    request: &StudioRequest,
    context: &StudioContext,
    local_addr: SocketAddr,
) -> bool {
    if !matches!(request.method.as_str(), "GET" | "POST") {
        return false;
    }
    let Some(host) = request.headers.get("host") else {
        return false;
    };
    if !studio_host_allowed(host, local_addr) {
        return false;
    }
    let origin = request.headers.get("origin");
    if let Some(origin) = origin {
        if !studio_origin_allowed(origin, host, local_addr) {
            return false;
        }
    }
    if request.method == "POST" {
        if origin.is_none()
            || !request.headers.get("content-type").is_some_and(|value| {
                value.split(';').next().is_some_and(|media_type| {
                    media_type.trim().eq_ignore_ascii_case("application/json")
                })
            })
            || !request.headers.contains_key("content-length")
        {
            return false;
        }
    }
    let query_session = match studio_query_session(&request.target) {
        Ok(session) => session,
        Err(()) => return false,
    };
    let cookie_session = match request.headers.get("cookie") {
        Some(cookie) => match studio_cookie_session(cookie) {
            Ok(session) => session,
            Err(()) => return false,
        },
        None => None,
    };
    let mut authenticated = false;
    if let Some(authorization) = request.headers.get("authorization") {
        let Some(token) = authorization.strip_prefix("Bearer ") else {
            return false;
        };
        if !constant_time_equal(token, &context.session_secret) {
            return false;
        }
        authenticated = true;
    }
    if let Some(session) = query_session.as_deref() {
        if !constant_time_equal(session, &context.session_secret) {
            return false;
        }
        authenticated = true;
    }
    if let Some(session) = cookie_session {
        if !constant_time_equal(session, &context.session_secret) {
            return false;
        }
        if request.method == "GET"
            && origin.is_none()
            && !request
                .headers
                .get("sec-fetch-site")
                .is_some_and(|site| site.eq_ignore_ascii_case("same-origin"))
        {
            return false;
        }
        authenticated = true;
    }
    authenticated
}

fn studio_query_session(target: &str) -> Result<Option<String>, ()> {
    let Some((_, query)) = target.split_once('?') else {
        return Ok(None);
    };
    let mut session = None;
    for part in query.split('&') {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        if name == "session" {
            if session.is_some() {
                return Err(());
            }
            session = Some(value.to_string());
        }
    }
    Ok(session)
}

fn studio_cookie_session(value: &str) -> Result<Option<&str>, ()> {
    let mut session = None;
    for part in value.split(';') {
        let (name, value) = part.trim().split_once('=').unwrap_or((part.trim(), ""));
        if name == "jetos_studio" {
            if session.is_some() {
                return Err(());
            }
            session = Some(value);
        }
    }
    Ok(session)
}

fn studio_host_allowed(value: &str, local_addr: SocketAddr) -> bool {
    let value = value.trim();
    let Some((host, port)) = studio_host_port(value) else {
        return false;
    };
    if port != local_addr.port() {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return local_addr.ip().is_loopback() || local_addr.ip().is_unspecified();
    }
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    ip == local_addr.ip()
        || local_addr.ip().is_unspecified()
        || (local_addr.ip().is_loopback() && ip.is_loopback())
}

fn studio_host_port(value: &str) -> Option<(&str, u16)> {
    if let Some(value) = value.strip_prefix('[') {
        let end = value.find(']')?;
        let host = &value[..end];
        let port = value.get(end + 1..)?.strip_prefix(':')?.parse().ok()?;
        return Some((host, port));
    }
    let (host, port) = value.rsplit_once(':')?;
    if host.contains(':') {
        return None;
    }
    Some((host, port.parse().ok()?))
}

fn studio_origin_allowed(value: &str, host: &str, local_addr: SocketAddr) -> bool {
    let Some(origin_host) = value.trim().strip_prefix("http://") else {
        return false;
    };
    !origin_host.contains('/')
        && studio_host_allowed(origin_host, local_addr)
        && origin_host.eq_ignore_ascii_case(host.trim())
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= left.get(index).copied().unwrap_or(0) as usize
            ^ right.get(index).copied().unwrap_or(0) as usize;
    }
    difference == 0
}

fn studio_write_response(
    stream: &mut std::net::TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    set_cookie: Option<&str>,
) -> std::io::Result<()> {
    use std::io::Write;
    let cookie = set_cookie
        .map(|session| {
            format!(
                "Set-Cookie: jetos_studio={session}; Path=/studio; HttpOnly; SameSite=Strict\r\n"
            )
        })
        .unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n{}Connection: close\r\n\r\n",
        body.len(),
        cookie
    )?;
    stream.write_all(body)
}

fn fs_read_for_http(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|_| b"missing\n".to_vec())
}

fn read_http_request(stream: &mut std::net::TcpStream) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if http_request_complete(&buf) || buf.len() > 64 * 1024 {
            break;
        }
    }
    Ok(buf)
}

fn http_request_complete(buf: &[u8]) -> bool {
    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4) else {
        return false;
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_len = headers
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    buf.len().saturating_sub(header_end) >= content_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_slowloris_connections_have_io_deadlines() {
        use std::io::Write;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();

        configure_studio_stream(&server).unwrap();
        assert_eq!(server.read_timeout().unwrap(), Some(STUDIO_IO_TIMEOUT));
        assert_eq!(server.write_timeout().unwrap(), Some(STUDIO_IO_TIMEOUT));
        server
            .set_read_timeout(Some(std::time::Duration::from_millis(20)))
            .unwrap();
        client.write_all(b"GET /studio/ HTTP/1.1\r\n").unwrap();
        let error = read_http_request(&mut server).unwrap_err();
        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ),
            "partial request should hit the read deadline: {error}"
        );
        drop(client);
    }
}
