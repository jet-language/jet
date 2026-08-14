//! `jet notebook` — one shared REPL session behind the first-party browser,
//! Canvas lens, Jupyter adapter, and bounded headless protocol.

use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::sync::{Arc, Mutex};

use jet::ExitCodes;
use jet::REPL::Notebook::{self, ClientKind, Kernel};

const MAX_REQUEST_HEADERS: usize = 64 * 1024;
const MAX_REQUEST_BODY: usize = 8 * 1024 * 1024;

/// Dispatch `jet notebook [PATH] [--protocol] [--bind ADDR] [--token TOKEN]`.
pub(crate) fn run_notebook(raw: &[String]) {
    let path = raw
        .iter()
        .skip_while(|arg| arg.as_str() != "notebook")
        .nth(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(PathBuf::from);
    let protocol = raw.iter().any(|arg| arg == "--protocol" || arg == "--headless");
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
    match serve_loopback(kernel, addr, &token, path.as_deref()) {
        Ok(code) => exit(code),
        Err(error) => {
            crate::cli_error!("E2105", "notebook server failed: {error}");
            exit(ExitCodes::ICE);
        }
    }
}

fn run_headless(kernel: &mut Kernel) {
    let stdin = io::stdin();
    let mut out = String::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "quit" || line == ":quit" {
            break;
        }
        out.push_str(&Notebook::run_headless_script(kernel, &[&line]));
    }
    print!("{out}");
    let _ = io::stdout().flush();
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

fn mint_token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    let mut source = std::fs::File::open("/dev/urandom").map_err(|error| error.to_string())?;
    source
        .read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_loopback(addr: &str) -> bool {
    addr.starts_with("127.") || addr.starts_with("localhost:") || addr == "localhost"
}

fn serve_loopback(
    kernel: Kernel,
    addr: &str,
    token: &str,
    path: Option<&Path>,
) -> Result<i32, String> {
    let listener = TcpListener::bind(addr).map_err(|error| error.to_string())?;
    let bound = listener.local_addr().map_err(|error| error.to_string())?;
    eprintln!("jet notebook listening on http://{bound}/#token={token}");
    if let Some(path) = path {
        eprintln!("document: {}", path.display());
    } else {
        eprintln!("document: untitled (save a `.jetnb` file from the notebook)");
    }
    eprintln!("clients: first-party / Canvas lens / Jupyter adapter share one session");
    eprintln!("Ctrl-C stops the server; `--protocol` accepts the same session headlessly");

    let shared = Arc::new(Mutex::new(kernel));
    for connection in listener.incoming() {
        let Ok(stream) = connection else { continue };
        let shared = Arc::clone(&shared);
        let token = token.to_string();
        std::thread::spawn(move || {
            let _ = handle_connection(stream, &shared, &token);
        });
    }
    Ok(ExitCodes::OK)
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
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .map_err(|error| error.to_string())?;
    let request = read_request(&mut stream)?;
    let (route, query) = split_target(&request.target);
    let public = request.method == "GET" && (route == "/" || route == "/index.html");
    if !public && !authorized(&request, query, token) {
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
        let kernel = kernel.lock().map_err(|_| "notebook kernel lock poisoned".to_string())?;
        return write_response(
            &mut stream,
            "200 OK",
            "application/json; charset=utf-8",
            &format!("{{\"ok\":true,\"cells\":{},\"turns\":{}}}", kernel.notebook.cells.len(), kernel.session.turns.len()),
        );
    }
    if request.method != "POST" || !route.starts_with("/api/") {
        return write_response(&mut stream, "404 Not Found", "text/plain; charset=utf-8", "not found");
    }
    let interrupt_was_active = route == "/api/interrupt"
        && jet::Comptime::repl_interruptible_turn_active();
    if route == "/api/interrupt" {
        jet::Comptime::note_repl_interrupt();
    }
    let mut kernel = kernel.lock().map_err(|_| "notebook kernel lock poisoned".to_string())?;
    let response = api_message(&mut kernel, route, &request.body, interrupt_was_active);
    let (status, body) = match response {
        ApiResponse::Ok(body) => ("200 OK", body),
        ApiResponse::Error(error) => ("400 Bad Request", json_error(&error)),
    };
    write_response(&mut stream, status, "application/json; charset=utf-8", &body)
}

enum ApiResponse {
    Ok(String),
    Error(String),
}

fn api_message(kernel: &mut Kernel, route: &str, body: &str, interrupt_was_active: bool) -> ApiResponse {
    let value = |name: &str| form_value(body, name).unwrap_or_default();
    let client = || parse_client(&value("client"));
    let message = match route {
        "/api/state" => return ApiResponse::Ok(kernel.state_json()),
        "/api/add" => {
            let kind = match value("kind").as_str() {
                "markdown" => Notebook::CellKind::Markdown,
                _ => Notebook::CellKind::Jet,
            };
            let cell_id = kernel.notebook.add_cell(kind, value("source")).id.clone();
            return state_message(kernel, format!("added={cell_id}"));
        }
        "/api/edit" => kernel.edit_cell(&value("cell_id"), value("source")).map(|()| "edited".into()),
        "/api/run" => {
            let id = value("cell_id");
            match client() {
                Ok(client) => kernel.execute_cell(client, &id).map(|result| {
                    format!("ran={id};status={:?};elapsed_ms={}", result.eval.status, result.elapsed_ms)
                }),
                Err(error) => Err(error),
            }
        }
        "/api/profile" => {
            kernel.attach_perf();
            let id = value("cell_id");
            match client() {
                Ok(client) => kernel.execute_cell(client, &id).map(|result| {
                    format!("profiled={id};elapsed_ms={}", result.elapsed_ms)
                }),
                Err(error) => Err(error),
            }
        }
        "/api/debug" => {
            kernel.attach_debug();
            inspect_message(kernel, &value("cell_id"))
        }
        "/api/inspect" => inspect_message(kernel, &value("cell_id")),
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
        "/api/open" => kernel.open_document(Path::new(&value("path"))).map(|()| "opened".into()),
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
            Ok((content, loss)) => return ApiResponse::Ok(export_message("notebook.ipynb", content, loss.render())),
            Err(error) => Err(error),
        },
        "/api/export/jet" => {
            let (content, loss) = Notebook::export_jet(&kernel.notebook);
            return ApiResponse::Ok(export_message("notebook.jet", content, loss.render()));
        }
        "/api/grant" => kernel
            .grant_capability(&value("cell_id"), &value("renderer"))
            .map(|()| "granted".into()),
        other => Err(format!("unknown notebook route `{other}`")),
    };
    match message {
        Ok(message) => state_message(kernel, message),
        Err(error) => ApiResponse::Error(error),
    }
}

fn state_message(kernel: &Kernel, message: String) -> ApiResponse {
    ApiResponse::Ok(format!("{{\"ok\":true,\"message\":{},\"state\":{}}}", json_str(&message), kernel.state_json()))
}

fn inspect_message(kernel: &Kernel, cell_id: &str) -> Result<String, String> {
    let cell = kernel
        .notebook
        .cells
        .iter()
        .find(|cell| cell.id == cell_id)
        .ok_or_else(|| format!("unknown cell `{cell_id}`"))?;
    let output = kernel
        .notebook
        .visible_output(cell_id)
        .map(|out| out.bundle.text_plain.clone())
        .unwrap_or_else(|| "(no live output)".into());
    Ok(format!("inspected={};source_len={};output={}", cell.id, cell.source.len(), output))
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
    Ok(format!("merged={}", path.display()))
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0u8; 4096];
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
    let mut request_line = lines.next().ok_or("missing request line")?.split_whitespace();
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

fn authorized(request: &Request, query: &str, token: &str) -> bool {
    request
        .headers
        .get("authorization")
        .is_some_and(|value| value == &format!("Bearer {token}"))
        || form_value(query, "token").is_some_and(|value| value == token)
}

fn form_value(body: &str, key: &str) -> Option<String> {
    body.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (decode_component(name).ok()? == key).then(|| decode_component(value).ok()).flatten()
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

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) -> Result<(), String> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).map_err(|error| error.to_string())
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
