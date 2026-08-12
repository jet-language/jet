#[allow(unused_imports)]
use jet_foundation::Outcome::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

mod dns_resolver_policy {
    #[allow(unused_imports)]
    pub use jet_foundation::Outcome::*;
    include!("../../crates/jet-codegen/src/Prelude/CoreLib/Top/DNSResolverPolicy.rs");

    pub fn resolv_conf(text: &str) -> Vec<String> {
        jet_net_dns_parse_resolv_conf(text)
    }

    pub fn scutil(text: &str) -> Vec<String> {
        jet_net_dns_parse_scutil(text)
    }

    pub fn windows(text: &str) -> Vec<String> {
        jet_net_dns_parse_windows_addresses(text)
    }
}

fn compile_temp(name: &str, src: &str) -> jet::CompileOutput {
    let dir = std::env::temp_dir().join(format!("jet_corelib_test_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    })
}

fn standalone_tls_probe_source(mut rust: String) -> String {
    rust = rust.replacen("fn main()", "fn jet_generated_main()", 1);
    let mut isolated = String::with_capacity(rust.len());
    let mut cfg_test = false;
    let mut test_module = 0usize;
    for line in rust.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body == "#[cfg(test)]" {
            cfg_test = true;
            isolated.push_str(line);
            continue;
        }
        if body.starts_with("extern crate jet_ffi_") {
            continue;
        }
        if body.starts_with("fn jet_ffi_install_reporter() {") {
            isolated.push_str("fn jet_ffi_install_reporter() {}");
            if line.ends_with('\n') {
                isolated.push('\n');
            }
            continue;
        }
        if cfg_test {
            cfg_test = false;
            let trimmed = body.trim_start();
            if trimmed.starts_with("mod tests {") {
                let indent = &body[..body.len() - trimmed.len()];
                isolated.push_str(indent);
                isolated.push_str(&format!("mod jet_standalone_test_module_{test_module} {{"));
                if line.ends_with('\n') {
                    isolated.push('\n');
                }
                test_module += 1;
                continue;
            }
        }
        isolated.push_str(line);
    }
    isolated
}

fn jet_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_jet"))
}

fn build_and_run(
    dir: &PathBuf,
    name: &str,
    src: &str,
    env: &[(&str, &str)],
    stdin: Option<&str>,
) -> (i32, String, String) {
    let path = dir.join(name);
    fs::write(&path, src).unwrap();
    let shown = path.to_string_lossy();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected fixture:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    let mut rustc_cmd = Command::new("rustc");
    common::add_generated_rust(&mut rustc_cmd, &rs, &out.rust, out.ffi.is_some(), &[]);
    rustc_cmd.arg("-o").arg(&bin);
    if let Some(link) = &out.ffi {
        rustc_cmd
            .arg("--extern")
            .arg(format!("{}={}", link.crate_name, link.rlib_path.display()));
        for deps_dir in link.dependency_dirs().filter(|dir| dir.is_dir()) {
            rustc_cmd
                .arg("-L")
                .arg(format!("dependency={}", deps_dir.display()));
        }
    }
    let rustc = rustc_cmd.output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let mut cmd = Command::new(&bin);
    cmd.current_dir(dir);
    for (k, v) in env {
        cmd.env(k, v);
    }
    if let Some(text) = stdin {
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let Some(mut input) = child.stdin.take() {
            use std::io::Write;
            input.write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        return (
            out.status.code().unwrap_or(0),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        );
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(0),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn build_and_run_multi(
    dir: &PathBuf,
    name: &str,
    entry: &str,
    files: &[(&str, &str)],
) -> (i32, String, String) {
    for (rel, src) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, src).unwrap();
    }
    let entry_path = dir.join(entry);
    let src = fs::read_to_string(&entry_path).unwrap();
    let shown = entry_path.to_string_lossy();
    let out = jet::compile_with_path(&src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected multi-file fixture:\n{}",
            jet::render_diagnostics(&shown, &src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    let mut rustc_cmd = Command::new("rustc");
    common::add_generated_rust(&mut rustc_cmd, &rs, &out.rust, out.ffi.is_some(), &[]);
    let rustc = rustc_cmd.arg("-o").arg(&bin).output().unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated multi-file code:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).current_dir(dir).output().unwrap();
    (
        run.status.code().unwrap_or(0),
        String::from_utf8_lossy(&run.stdout).into_owned(),
        String::from_utf8_lossy(&run.stderr).into_owned(),
    )
}

#[cfg(unix)]
fn write_executable(path: &std::path::Path, body: &str) {
    fs::write(path, body).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn jet_string_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn dns_name_wire(name: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for label in name.trim_end_matches('.').split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out
}

fn dns_fixture_response(query: &[u8]) -> Vec<u8> {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos += 1;
    let qtype = u16::from_be_bytes([query[pos], query[pos + 1]]);
    let question_end = pos + 4;
    let mut resp = Vec::new();
    resp.extend_from_slice(&query[0..2]);
    resp.extend_from_slice(&0x8180u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&0u16.to_be_bytes());
    resp.extend_from_slice(&query[12..question_end]);
    resp.extend_from_slice(&[0xc0, 0x0c]);
    resp.extend_from_slice(&qtype.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes());
    resp.extend_from_slice(&0u32.to_be_bytes());
    let rdata = match qtype {
        16 => {
            let mut r = Vec::new();
            r.push(3);
            r.extend_from_slice(b"jet");
            r
        }
        33 => {
            let mut r = Vec::new();
            r.extend_from_slice(&10u16.to_be_bytes());
            r.extend_from_slice(&20u16.to_be_bytes());
            r.extend_from_slice(&443u16.to_be_bytes());
            r.extend_from_slice(&dns_name_wire("srv.example.test"));
            r
        }
        _ => Vec::new(),
    };
    resp.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    resp.extend_from_slice(&rdata);
    resp
}

fn dns_question_end(query: &[u8]) -> usize {
    let mut pos = 12usize;
    while pos < query.len() && query[pos] != 0 {
        pos += query[pos] as usize + 1;
    }
    pos + 5
}

fn dns_truncated_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8380u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    // A TC response may end inside a declared record. The client must validate
    // the authenticated header/question, then retry over TCP without parsing
    // an explicitly incomplete UDP record body.
    response.push(0xc0);
    response
}

fn dns_cname_additional_response(query: &[u8]) -> Vec<u8> {
    let end = dns_question_end(query);
    let alias = dns_name_wire("alias.example.test");
    let mut response = Vec::new();
    response.extend_from_slice(&query[0..2]);
    response.extend_from_slice(&0x8180u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&query[12..end]);
    response.extend_from_slice(&[0xc0, 0x0c]);
    response.extend_from_slice(&5u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&(alias.len() as u16).to_be_bytes());
    response.extend_from_slice(&alias);
    response.extend_from_slice(&alias);
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&1u16.to_be_bytes());
    response.extend_from_slice(&0u32.to_be_bytes());
    response.extend_from_slice(&4u16.to_be_bytes());
    response.extend_from_slice(&[192, 0, 2, 42]);
    response
}

fn bind_dns_dual_protocol_fixture() -> (
    std::net::TcpListener,
    std::net::UdpSocket,
    std::net::SocketAddr,
) {
    const MAX_ATTEMPTS: usize = 64;

    for attempt in 1..=MAX_ATTEMPTS {
        let tcp = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(tcp) => tcp,
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempt < MAX_ATTEMPTS =>
            {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => break,
            Err(error) => panic!("failed to bind DNS fixture TCP listener: {error}"),
        };
        let addr = tcp.local_addr().unwrap_or_else(|error| {
            panic!("failed to read DNS fixture TCP listener address: {error}")
        });
        match std::net::UdpSocket::bind(addr) {
            Ok(udp) => return (tcp, udp, addr),
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempt < MAX_ATTEMPTS =>
            {
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => break,
            Err(error) => panic!("failed to bind DNS fixture UDP socket at {addr}: {error}"),
        }
    }

    panic!("failed to reserve one TCP/UDP DNS fixture port after {MAX_ATTEMPTS} attempts")
}

