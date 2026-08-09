//! `jet notebook` — D-NOTEBOOK-SURFACE1=D first-party entry + headless protocol.

use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::exit;

use jet::ExitCodes;
use jet::REPL::Notebook::{self, ClientKind, Kernel};

/// Dispatch `jet notebook [PATH] [--protocol] [--bind ADDR] [--token TOKEN]`.
pub(crate) fn run_notebook(raw: &[String]) {
    let path = raw
        .iter()
        .skip_while(|a| a.as_str() != "notebook")
        .nth(1)
        .filter(|a| !a.starts_with('-'))
        .cloned();
    let protocol = raw.iter().any(|a| a == "--protocol" || a == "--headless");
    let bind = flag_value(raw, "--bind");
    let token = flag_value(raw, "--token").map(str::to_string);

    let env = match &path {
        Some(p) => Kernel::environment_hash(Path::new(p).parent().unwrap_or(Path::new("."))),
        None => Kernel::environment_hash(Path::new(".")),
    };
    let mut kernel = Kernel::open(path.as_deref().map(Path::new), env);

    if protocol || !io::stdin().is_terminal() {
        // Headless JSONL / script protocol — used by tests and CI.
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
            out.push_str(&Notebook::run_headless_script(&mut kernel, &[&line]));
        }
        print!("{out}");
        let _ = io::stdout().flush();
        exit(ExitCodes::OK);
    }

    // Interactive: loopback-only by default (D-NOTEBOOK-SURFACE1=D).
    let addr = bind.unwrap_or("127.0.0.1:0");
    if !addr.starts_with("127.0.0.1") && !addr.starts_with("localhost") && bind.is_some() {
        if token.as_deref().unwrap_or("").is_empty() {
            crate::cli_error!(@full "E2104", "non-loopback `jet notebook --bind` requires `--token <bearer>`", "D-NOTEBOOK-SURFACE1=D keeps the notebook client loopback-only unless auth is explicit", "pass `--token <secret>` or bind `127.0.0.1`");
            exit(ExitCodes::USER_ERROR);
        }
    }
    let token = token.unwrap_or_else(mint_token);
    match serve_loopback(&mut kernel, addr, &token, path.as_deref()) {
        Ok(code) => exit(code),
        Err(e) => {
            crate::cli_error!("E2105", "notebook server failed: {e}");
            exit(ExitCodes::ICE);
        }
    }
}

fn flag_value<'a>(raw: &'a [String], name: &str) -> Option<&'a str> {
    raw.iter()
        .find_map(|a| a.strip_prefix(&format!("{name}=")))
        .or_else(|| {
            raw.iter()
                .position(|a| a == name)
                .and_then(|i| raw.get(i + 1).map(|s| s.as_str()))
        })
}

fn mint_token() -> String {
    let mut bytes = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut bytes);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Minimal loopback HTTP surface: health + execute-last cell via query API.
/// Full Studio/Canvas UI is a lens over the same [`Kernel`].
fn serve_loopback(
    kernel: &mut Kernel,
    addr: &str,
    token: &str,
    path: Option<&str>,
) -> Result<i32, String> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(addr).map_err(|e| e.to_string())?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?;
    let fragment = format!("http://{bound}/#token={token}");
    eprintln!("jet notebook listening on {fragment}");
    if let Some(path) = path {
        eprintln!("document: {path}");
    }
    eprintln!("clients: first-party / Canvas lens / Jupyter adapter share one session");
    eprintln!("press Ctrl-C to stop; pipe `--protocol` for headless tests");

    // Seed an empty Jet cell so the shared session is observable immediately.
    if kernel.notebook.cells.is_empty() {
        let _ = kernel
            .notebook
            .add_cell(Notebook::CellKind::Jet, "1 + 1");
    }
    let seed_id = kernel.notebook.cells[0].id.clone();
    let _ = kernel.execute_cell(ClientKind::FirstParty, &seed_id);

    listener.set_nonblocking(false).ok();
    for conn in listener.incoming() {
        let mut stream = match conn {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let authorized = req.contains(&format!("Authorization: Bearer {token}"))
            || req.contains(&format!("token={token}"))
            || req.contains(&format!("#token={token}"));
        let (status, body, ctype) = if !authorized && !req.starts_with("GET / ") {
            (
                "401 Unauthorized",
                "missing bearer token".to_string(),
                "text/plain; charset=utf-8",
            )
        } else if req.starts_with("GET /health") {
            (
                "200 OK",
                format!(
                    "{{\"ok\":true,\"turns\":{},\"cells\":{}}}",
                    kernel.session.turns.len(),
                    kernel.notebook.cells.len()
                ),
                "application/json; charset=utf-8",
            )
        } else if req.starts_with("GET / ") || req.starts_with("GET /index") {
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>Jet notebook</title>\
                 <h1>Jet notebook</h1>\
                 <p>Shared REPL session · {} turns · {} cells</p>\
                 <p>Open with fragment token. Canvas lens and Jupyter adapter use the same kernel.</p>",
                kernel.session.turns.len(),
                kernel.notebook.cells.len()
            );
            ("200 OK", html, "text/html; charset=utf-8")
        } else {
            (
                "404 Not Found",
                "not found".into(),
                "text/plain; charset=utf-8",
            )
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    }
    Ok(ExitCodes::OK)
}
