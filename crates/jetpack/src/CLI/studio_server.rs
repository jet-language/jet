struct StudioContext {
    config: PathBuf,
    host: String,
    offline: bool,
    changeset: std::sync::Mutex<Option<StudioChangeSet>>,
    last_applied: std::sync::Mutex<Option<StudioAppliedChange>>,
    live_projection: std::sync::Mutex<Option<String>>,
    proved_source: std::sync::Mutex<Option<String>>,
}

fn studio_host(parsed: &Parsed) -> Option<String> {
    parsed.flags.studio_host.clone()
}

fn studio_context(parsed: &Parsed) -> Option<StudioContext> {
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
    let offline = parsed.flags.offline || !Provider::nix_on_path();
    Some(StudioContext {
        config,
        host,
        offline,
        changeset: std::sync::Mutex::new(None),
        last_applied: std::sync::Mutex::new(None),
        live_projection: std::sync::Mutex::new(None),
        proved_source: std::sync::Mutex::new(None),
    })
}

fn serve_studio(
    theme: &Theme,
    addr: &str,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: Option<&StudioContext>,
    open_browser: bool,
) -> i32 {
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
    let local = listener
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| addr.to_string());
    let url = format!("http://{local}/studio/");
    println!("{url}");
    theme.ok("jetos Studio service listening");
    if open_browser {
        match std::process::Command::new("xdg-open").arg(&url).spawn() {
            Ok(_) => theme.ok("opened jetos Studio"),
            Err(_) => {
                theme.detail("open the printed Studio URL in a browser.");
            }
        }
    }
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let _ = handle_studio_request(&mut stream, app, meta, data, context);
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

fn handle_studio_request(
    stream: &mut std::net::TcpStream,
    app: &Path,
    meta: &Path,
    data: &Path,
    context: Option<&StudioContext>,
) -> std::io::Result<()> {
    use std::io::Write;
    let request_bytes = read_http_request(stream)?;
    let request = String::from_utf8_lossy(&request_bytes);
    let method = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("GET");
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    if method == "POST" && path == "/studio/transaction" {
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
        let (status, body) = handle_studio_transaction(body, context);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        return stream.write_all(body.as_bytes());
    }
    if method == "POST" && path == "/studio/run" {
        let body = request.split("\r\n\r\n").nth(1).unwrap_or("");
        let (status, body) = handle_studio_run(body, context);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes())?;
        return stream.write_all(body.as_bytes());
    }
    let (status, content_type, body) = match path {
        "/" | "/studio" | "/studio/" | "/studio/index.html" => {
            ("200 OK", "text/html; charset=utf-8", fs_read_for_http(app))
        }
        "/studio/app.json" => ("200 OK", "application/json", fs_read_for_http(meta)),
        "/studio/data.json" => match context {
            Some(context) => match studio_live_projection(context, data) {
                Ok(body) => ("200 OK", "application/json", body.into_bytes()),
                Err(body) => ("500 Internal Server Error", "application/json", body.into_bytes()),
            },
            None => ("200 OK", "application/json", fs_read_for_http(data)),
        },
        "/studio/source" => match context {
            Some(context) => (
                "200 OK",
                "text/plain; charset=utf-8",
                fs_read_for_http(&context.config),
            ),
            None => (
                "400 Bad Request",
                "text/plain; charset=utf-8",
                b"missing Studio project context\n".to_vec(),
            ),
        },
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            b"not found\n".to_vec(),
        ),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.write_all(&body)
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
