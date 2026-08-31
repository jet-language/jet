//! `jet notebook` — one shared REPL session behind the first-party browser,
//! Canvas lens, Jupyter adapter, and bounded headless protocol.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{exit, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use jet::ExitCodes;
use jet::REPL::Notebook::{self, ClientKind, Kernel};

const MAX_REQUEST_HEADERS: usize = 64 * 1024;
const MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 64;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const BOOTSTRAP_ROUTE: &str = "/__jet_notebook_bootstrap";
const BOOTSTRAP_TTL: Duration = Duration::from_secs(30);

/// Dispatch `jet notebook [PATH] [--protocol] [--bind ADDR] [--token TOKEN]`.
pub(crate) fn run_notebook(raw: &[String]) {
    let path = notebook_path(raw);
    let protocol = raw
        .iter()
        .any(|arg| arg == "--protocol" || arg == "--headless");
    let bind = flag_value(raw, "--bind");
    let explicit_token = flag_value(raw, "--token").map(str::to_string);
    let environment = path
        .as_deref()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new("."));
    let env_hash = Kernel::environment_hash(environment);
    let mut kernel = match Kernel::open(path.as_deref(), env_hash) {
        Ok(kernel) => kernel,
        Err(error) => {
            crate::cli_error!("E2105", "notebook document failed to open: {error}");
            exit(ExitCodes::USER_ERROR);
        }
    };

    // An explicit bind is the production browser entry point. It must remain a
    // server even when launched by a non-TTY harness or a service manager.
    if protocol || (bind.is_none() && !io::stdin().is_terminal()) {
        run_headless(&mut kernel);
        exit(ExitCodes::OK);
    }

    let addr = bind.unwrap_or("127.0.0.1:0");
    if !is_loopback(addr) && bind.is_some() && explicit_token.as_deref().unwrap_or("").is_empty() {
        crate::cli_error!(
            @full
            "E2104",
            "non-loopback `jet notebook --bind` requires `--token <bearer>`",
            "notebook clients require explicit authentication outside loopback",
            "pass `--token <secret>` or bind `127.0.0.1`"
        );
        exit(ExitCodes::USER_ERROR);
    }
    let token = match explicit_token {
        Some(token) if !token.is_empty() => token,
        _ => match mint_token() {
            Ok(token) => token,
            Err(error) => {
                crate::cli_error!(
                    "E2105",
                    "notebook could not create an authentication token: {error}"
                );
                exit(ExitCodes::ICE);
            }
        },
    };
    let auto_open = io::stdin().is_terminal() && is_loopback(addr);
    match serve_loopback(kernel, addr, &token, path.as_deref(), auto_open) {
        Ok(code) => exit(code),
        Err(error) => {
            crate::cli_error!("E2105", "notebook server failed: {error}");
            exit(ExitCodes::ICE);
        }
    }
}

fn run_headless(kernel: &mut Kernel) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == ":quit" {
            break;
        }
        let response = Notebook::run_headless_script(kernel, &[&line]);
        if stdout.write_all(response.as_bytes()).is_err() {
            break;
        }
        if stdout.flush().is_err() {
            break;
        }
    }
}

fn flag_value<'a>(raw: &'a [String], name: &str) -> Option<&'a str> {
    raw.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{name}=")))
        .or_else(|| {
            raw.iter()
                .position(|arg| arg == name)
                .and_then(|index| raw.get(index + 1).map(String::as_str))
        })
}

fn notebook_path(raw: &[String]) -> Option<PathBuf> {
    let start = raw.iter().position(|arg| arg == "notebook")? + 1;
    let mut consumes_value = false;
    for arg in raw.iter().skip(start) {
        if consumes_value {
            consumes_value = false;
            continue;
        }
        if arg == "--bind" || arg == "--token" {
            consumes_value = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        return Some(PathBuf::from(arg));
    }
    None
}

fn mint_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut source = std::fs::File::open("/dev/urandom").map_err(|error| error.to_string())?;
    source
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_loopback(addr: &str) -> bool {
    addr.starts_with("127.")
        || addr.starts_with("localhost:")
        || addr == "localhost"
        || addr.starts_with("[::1]:")
        || addr == "[::1]"
        || addr.starts_with("::1:")
        || addr == "::1"
}

fn serve_loopback(
    kernel: Kernel,
    addr: &str,
    token: &str,
    path: Option<&Path>,
    auto_open: bool,
) -> Result<i32, String> {
    let listener = TcpListener::bind(addr).map_err(|error| error.to_string())?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    let bootstrap = if auto_open {
        let nonce = mint_token()?;
        let state = Arc::new(Mutex::new(Some(BootstrapGrant {
            nonce: nonce.clone(),
            expires_at: Instant::now() + BOOTSTRAP_TTL,
        })));
        open_notebook_browser(&bootstrap_url(bound, &nonce));
        Some(state)
    } else {
        None
    };
    eprintln!("{}", listener_notice(bound));
    if let Some(path) = path {
        eprintln!("document: {}", path.display());
    } else {
        eprintln!("document: untitled (save a `.jetnb` file from the notebook)");
    }
    eprintln!("clients: first-party / Canvas lens / Jupyter adapter share one session");
    eprintln!("Ctrl-C stops the server; `--protocol` accepts the same session headlessly");

    let shared = Arc::new(Mutex::new(kernel));
    let active = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        let Ok(stream) = connection else { continue };
        if active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_CONNECTIONS).then_some(count + 1)
            })
            .is_err()
        {
            continue;
        }
        let shared = Arc::clone(&shared);
        let active_for_thread = Arc::clone(&active);
        let token = token.to_string();
        let bootstrap = bootstrap.clone();
        let result = std::thread::Builder::new().spawn(move || {
            let stream = stream;
            let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(10)));
            let _ = handle_connection(stream, &shared, &token, bootstrap.as_deref());
            active_for_thread.fetch_sub(1, Ordering::AcqRel);
        });
        if result.is_err() {
            active.fetch_sub(1, Ordering::AcqRel);
        }
    }
    Ok(ExitCodes::OK)
}

struct BootstrapGrant {
    nonce: String,
    expires_at: Instant,
}

fn listener_notice(bound: SocketAddr) -> String {
    format!("jet notebook listening on http://{bound}/ (authentication token withheld)")
}

fn bootstrap_url(bound: SocketAddr, nonce: &str) -> String {
    format!(
        "http://{bound}{BOOTSTRAP_ROUTE}?nonce={}",
        fragment_component(nonce)
    )
}

fn redirect_location(token: &str) -> String {
    format!("/#token={}", fragment_component(token))
}

fn fragment_component(value: &str) -> String {
    value.bytes().fold(String::new(), |mut encoded, byte| {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
        encoded
    })
}

fn browser_launch_spec(explicit: Option<OsString>, url: &str) -> (OsString, Vec<OsString>) {
    if let Some(browser) = explicit {
        return (browser, vec![url.into()]);
    }
    #[cfg(target_os = "macos")]
    {
        ("open".into(), vec![url.into()])
    }
    #[cfg(target_os = "windows")]
    {
        // Pass the URL directly to Explorer. `cmd /C start` would make URL
        // metacharacters part of a command string.
        ("explorer.exe".into(), vec![url.into()])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("xdg-open".into(), vec![url.into()])
    }
}

fn open_notebook_browser(url: &str) {
    let explicit = std::env::var_os("JET_CANVAS_BROWSER")
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var_os("BROWSER").filter(|value| !value.is_empty()));
    let (program, args) = browser_launch_spec(explicit, url);
    let mut command = Command::new(program);
    command.args(args);
    if let Err(error) = command.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        eprintln!(
            "jet notebook browser handoff failed: {error}; set JET_CANVAS_BROWSER or BROWSER"
        );
    }
}

struct Request {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: String,
}

fn handle_connection(
    mut stream: TcpStream,
    kernel: &Arc<Mutex<Kernel>>,
    token: &str,
    bootstrap: Option<&Mutex<Option<BootstrapGrant>>>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| error.to_string())?;
    let request = read_request(&mut stream)?;
    let (route, query) = split_target(&request.target);
    if request.method == "GET" && route == BOOTSTRAP_ROUTE {
        let valid = bootstrap.is_some_and(|state| {
            form_value(query, "nonce").is_some_and(|nonce| consume_bootstrap(state, &nonce))
        });
        return if valid {
            write_redirect(&mut stream, token)
        } else {
            write_response(
                &mut stream,
                "401 Unauthorized",
                "application/json; charset=utf-8",
                &json_error("invalid or expired notebook bootstrap"),
            )
        };
    }
    let public = request.method == "GET" && (route == "/" || route == "/index.html");
    if !public && !authorized(&request, token) {
        return write_response(
            &mut stream,
            "401 Unauthorized",
            "application/json; charset=utf-8",
            &json_error("missing bearer token"),
        );
    }
    if public {
        return write_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            include_str!("../crates/jet-repl/src/Notebook/client.html"),
        );
    }
    if request.method == "GET" && route == "/health" {
        let kernel = kernel
            .lock()
            .map_err(|_| "notebook kernel lock poisoned".to_string())?;
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &format!(
                "{{\"ok\":true,\"cells\":{},\"turns\":{}}}",
                kernel.notebook.cells.len(),
                kernel.session.turns.len()
            ),
        );
    }
    if request.method != "POST" || !route.starts_with("/api/") {
        return write_response(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found",
        );
    }
    let interrupt_was_active =
        route == "/api/interrupt" && jet::Comptime::repl_interruptible_turn_active();
    if route == "/api/interrupt" {
        jet::Comptime::note_repl_interrupt();
    }
    let mut kernel = kernel
        .lock()
        .map_err(|_| "notebook kernel lock poisoned".to_string())?;
    let response = api_message(&mut kernel, route, &request.body, interrupt_was_active);
    let (status, body) = match response {
        ApiResponse::Ok(body) => ("200 OK", body),
        ApiResponse::Error(error) => ("400 Bad Request", json_error(&error)),
    };
    write_response(
        &mut stream,
        status,
        "application/json; charset=utf-8",
        &body,
    )
}

enum ApiResponse {
    Ok(String),
    Error(String),
}

fn api_message(
    kernel: &mut Kernel,
    route: &str,
    body: &str,
    interrupt_was_active: bool,
) -> ApiResponse {
    let value = |name: &str| form_value(body, name).unwrap_or_default();
    let client = || parse_client(&value("client"));
    let selected_client = client().unwrap_or(ClientKind::FirstParty);
    let message = match route {
        "/api/state" => return ApiResponse::Ok(kernel.state_json_for(selected_client)),
        "/api/add" => {
            let kind = match value("kind").as_str() {
                "markdown" => Notebook::CellKind::Markdown,
                _ => Notebook::CellKind::Jet,
            };
            let cell_id = kernel.notebook.add_cell(kind, value("source")).id.clone();
            return state_message_for(kernel, format!("added={cell_id}"), selected_client);
        }
        "/api/edit" => kernel
            .edit_cell(&value("cell_id"), value("source"))
            .map(|()| "edited".into()),
        "/api/run" => {
            let id = value("cell_id");
            match client() {
                Ok(client) => kernel.execute_cell(client, &id).map(|result| {
                    format!(
                        "ran={id};status={:?};elapsed_ms={}",
                        result.eval.status, result.elapsed_ms
                    )
                }),
                Err(error) => Err(error),
            }
        }
        "/api/profile" => {
            kernel.attach_perf();
            let id = value("cell_id");
            match client() {
                Ok(client) => kernel
                    .execute_cell(client, &id)
                    .map(|result| format!("profiled={id};elapsed_ms={}", result.elapsed_ms)),
                Err(error) => Err(error),
            }
        }
        "/api/debug" => {
            kernel.attach_debug();
            inspect_message(
                kernel,
                &value("cell_id"),
                client().unwrap_or(ClientKind::FirstParty),
            )
            .map(|message| format!("debug_attached;{message}"))
        }
        "/api/inspect" => inspect_message(
            kernel,
            &value("cell_id"),
            client().unwrap_or(ClientKind::FirstParty),
        ),
        "/api/complete" => {
            let prefix = value("prefix");
            let mut names: Vec<_> = kernel
                .session
                .scope
                .keys()
                .filter(|name| name.starts_with(&prefix))
                .cloned()
                .collect();
            names.sort();
            Ok(format!("completions={}", names.join(",")))
        }
        "/api/interrupt" => {
            if !interrupt_was_active {
                kernel.request_interrupt();
            }
            Ok("interrupt_requested".into())
        }
        "/api/stdin" => {
            kernel.push_stdin(value("line"));
            Ok(format!("stdin_queued={}", kernel.stdin_queue.len()))
        }
        "/api/open" => kernel
            .open_document(Path::new(&value("path")))
            .map(|()| "opened".into()),
        "/api/reopen" => kernel.reopen_document().map(|()| "reopened".into()),
        "/api/save" => kernel
            .save_document(nonempty_path(body, "path").as_deref())
            .map(|path| format!("saved={}", path.display())),
        "/api/merge" => merge_path(kernel, Path::new(&value("path"))),
        "/api/import" => match Notebook::import_ipynb(&value("text")) {
            Ok((notebook, loss)) => {
                kernel.replace_notebook(notebook);
                Ok(format!("imported;{}", loss.render().replace('\n', " | ")))
            }
            Err(error) => Err(error),
        },
        "/api/export/ipynb" => match Notebook::export_ipynb(&kernel.notebook) {
            Ok((content, loss)) => {
                return ApiResponse::Ok(export_message("notebook.ipynb", content, loss.render()))
            }
            Err(error) => Err(error),
        },
        "/api/export/jet" => {
            let (content, loss) = Notebook::export_jet(&kernel.notebook);
            return ApiResponse::Ok(export_message("notebook.jet", content, loss.render()));
        }
        "/api/grant" => kernel
            .grant_capability(&value("cell_id"), selected_client.renderer())
            .map(|()| "granted".into()),
        other => Err(format!("unknown notebook route `{other}`")),
    };
    match message {
        Ok(message) => state_message_for(kernel, message, selected_client),
        Err(error) => ApiResponse::Error(error),
    }
}

fn state_message_for(kernel: &Kernel, message: String, client: ClientKind) -> ApiResponse {
    ApiResponse::Ok(format!(
        "{{\"ok\":true,\"message\":{},\"state\":{}}}",
        json_str(&message),
        kernel.state_json_for(client)
    ))
}

fn inspect_message(kernel: &Kernel, cell_id: &str, client: ClientKind) -> Result<String, String> {
    let cell = kernel
        .notebook
        .cells
        .iter()
        .find(|cell| cell.id == cell_id)
        .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
    let output = (match client {
        ClientKind::FirstParty => kernel.first_party_visible_output(cell_id),
        ClientKind::CanvasLens => kernel.canvas_visible_output(cell_id),
        ClientKind::JupyterAdapter => kernel.jupyter_visible_output(cell_id),
    })
    .map(|out| out.text_plain.clone())
    .unwrap_or_else(|| "(no live output)".into());
    Ok(format!(
        "inspected={};source_len={};output={}",
        cell.id,
        cell.source.len(),
        output
    ))
}

fn nonempty_path(body: &str, key: &str) -> Option<PathBuf> {
    let path = form_value(body, key)?;
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn export_message(filename: &str, content: String, loss: String) -> String {
    format!(
        "{{\"ok\":true,\"filename\":{},\"content\":{},\"loss\":{}}}",
        json_str(filename),
        json_str(&content),
        json_str(&loss)
    )
}

fn parse_client(name: &str) -> Result<ClientKind, String> {
    match name {
        "" | "first" | "first-party" => Ok(ClientKind::FirstParty),
        "canvas" | "lens" => Ok(ClientKind::CanvasLens),
        "jupyter" | "jp" => Ok(ClientKind::JupyterAdapter),
        other => Err(format!("unknown client `{other}`")),
    }
}

fn merge_path(kernel: &mut Kernel, path: &Path) -> Result<String, String> {
    let notebook = Notebook::load_jetnb(path)?;
    kernel.merge_notebook(notebook);
    if kernel.notebook.merge_conflicts.is_empty() {
        Ok("merged by stable cell ID;conflicts=0".into())
    } else {
        Ok(format!(
            "merged by stable cell ID;conflicts={}",
            kernel.notebook.merge_conflicts.len()
        ))
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    read_request_until(stream, Instant::now() + REQUEST_TIMEOUT)
}

fn read_request_until(stream: &mut TcpStream, deadline: Instant) -> Result<Request, String> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("request deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before headers".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_REQUEST_HEADERS {
            return Err("request headers exceed 64 KiB".into());
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "headers are not UTF-8")?;
    let mut lines = header.lines();
    let mut request_line = lines
        .next()
        .ok_or("missing request line")?
        .split_whitespace();
    let method = request_line.next().unwrap_or_default().to_string();
    let target = request_line.next().unwrap_or_default().to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().map_err(|_| "invalid content length"))
        .transpose()?
        .unwrap_or(0);
    if length > MAX_REQUEST_BODY {
        return Err("request body exceeds 8 MiB".into());
    }
    while bytes.len() < header_end + length {
        let mut chunk = [0u8; 4096];
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("request deadline exceeded".into());
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("request ended before body".into());
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = String::from_utf8(bytes[header_end..header_end + length].to_vec())
        .map_err(|_| "request body is not UTF-8")?;
    Ok(Request {
        method,
        target,
        headers,
        body,
    })
}

fn split_target(target: &str) -> (&str, &str) {
    target.split_once('?').unwrap_or((target, ""))
}

fn authorized(request: &Request, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    request
        .headers
        .get("authorization")
        .is_some_and(|value| constant_time_eq(value.as_bytes(), expected.as_bytes()))
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn consume_bootstrap(state: &Mutex<Option<BootstrapGrant>>, nonce: &str) -> bool {
    consume_bootstrap_at(state, nonce, Instant::now())
}

fn consume_bootstrap_at(state: &Mutex<Option<BootstrapGrant>>, nonce: &str, now: Instant) -> bool {
    let Ok(mut grant) = state.lock() else {
        return false;
    };
    if grant.as_ref().is_some_and(|grant| now >= grant.expires_at) {
        *grant = None;
        return false;
    }
    let matches = grant
        .as_ref()
        .is_some_and(|grant| constant_time_eq(grant.nonce.as_bytes(), nonce.as_bytes()));
    if matches {
        *grant = None;
    }
    matches
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (decode_component(name).ok()? == key)
            .then(|| decode_component(value).ok())
            .flatten()
    })
}

fn decode_component(value: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut chars = value.bytes();
    while let Some(byte) = chars.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => {
                let high = chars.next().ok_or("incomplete percent escape")?;
                let low = chars.next().ok_or("incomplete percent escape")?;
                let hex = [high, low];
                let text = std::str::from_utf8(&hex).map_err(|_| "invalid percent escape")?;
                out.push(u8::from_str_radix(text, 16).map_err(|_| "invalid percent escape")?);
            }
            byte => out.push(byte),
        }
    }
    String::from_utf8(out).map_err(|_| "form value is not UTF-8".into())
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn write_redirect(stream: &mut TcpStream, token: &str) -> Result<(), String> {
    let location = redirect_location(token);
    let response = format!(
        "HTTP/1.1 303 See Other\r\nLocation: {location}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| error.to_string())
}

fn json_error(message: &str) -> String {
    format!("{{\"ok\":false,\"error\":{}}}", json_str(message))
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_notice_withholds_the_bearer_token() {
        let notice = listener_notice("127.0.0.1:43127".parse().unwrap());
        assert_eq!(
            notice,
            "jet notebook listening on http://127.0.0.1:43127/ (authentication token withheld)"
        );
        assert!(!notice.contains("#token="));
    }

    #[test]
    fn redirect_location_keeps_authentication_in_the_fragment() {
        let bound = "127.0.0.1:43127".parse().unwrap();
        assert_eq!(
            format!("http://{bound}{}", redirect_location("a&b")),
            "http://127.0.0.1:43127/#token=a%26b"
        );
        assert_eq!(redirect_location("a&b"), "/#token=a%26b");
        assert!(!listener_notice(bound).contains("a&b"));
    }

    #[test]
    fn browser_process_receives_bootstrap_nonce_without_bearer() {
        let bound = "127.0.0.1:43127".parse().unwrap();
        let bearer = "browser-bearer";
        let url = bootstrap_url(bound, "0123456789abcdef0123456789abcdef");
        let (_, args) = browser_launch_spec(Some("fake-browser".into()), &url);
        assert_eq!(args, vec![OsString::from(url.clone())]);
        assert!(!url.contains("#token="));
        assert!(!url.contains(bearer));
        assert!(args
            .iter()
            .all(|argument| !argument.to_string_lossy().contains(bearer)));
    }

    #[test]
    fn bootstrap_nonce_is_single_use_and_not_a_bearer() {
        let now = Instant::now();
        let nonce = "0123456789abcdef0123456789abcdef";
        let state = Mutex::new(Some(BootstrapGrant {
            nonce: nonce.into(),
            expires_at: now + BOOTSTRAP_TTL,
        }));
        assert!(!consume_bootstrap_at(&state, "wrong-nonce", now));
        assert!(consume_bootstrap_at(&state, nonce, now));
        assert!(!consume_bootstrap_at(&state, nonce, now));

        let request = Request {
            method: "POST".into(),
            target: "/api/state".into(),
            headers: HashMap::from([(String::from("authorization"), format!("Bearer {nonce}"))]),
            body: String::new(),
        };
        assert!(!authorized(&request, "actual-bearer"));
    }

    #[test]
    fn bootstrap_nonce_expires_and_is_removed() {
        let now = Instant::now();
        let state = Mutex::new(Some(BootstrapGrant {
            nonce: "expired-nonce".into(),
            expires_at: now,
        }));
        assert!(!consume_bootstrap_at(&state, "expired-nonce", now));
        assert!(state.lock().unwrap().is_none());
    }

    #[test]
    fn minted_bootstrap_nonce_is_fresh_and_hex_encoded() {
        let first = mint_token().unwrap();
        let second = mint_token().unwrap();
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, "0".repeat(32));
        assert_ne!(first, second);
    }

    #[test]
    fn notebook_auth_ignores_query_tokens_and_compares_the_bearer_header() {
        let request = Request {
            method: "POST".into(),
            target: "/api/state?token=attacker-query".into(),
            headers: HashMap::from([(
                String::from("authorization"),
                String::from("Bearer local-secret"),
            )]),
            body: String::new(),
        };
        assert!(authorized(&request, "local-secret"));

        let query_only = Request {
            headers: HashMap::new(),
            ..request
        };
        assert!(!authorized(&query_only, "local-secret"));

        let wrong = Request {
            headers: HashMap::from([(
                String::from("authorization"),
                String::from("Bearer local-secrex"),
            )]),
            ..query_only
        };
        assert!(!authorized(&wrong, "local-secret"));
    }

    #[test]
    fn partial_notebook_request_cannot_extend_the_absolute_deadline() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let writer = std::thread::spawn(move || {
            let mut client = client;
            for _ in 0..200 {
                if std::io::Write::write_all(&mut client, b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        let started = Instant::now();
        let result = read_request_until(&mut server, started + Duration::from_millis(80));
        assert!(result.is_err(), "incomplete request unexpectedly succeeded");
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "incomplete request outlived its absolute deadline"
        );
        drop(server);
        writer.join().unwrap();
    }
}
