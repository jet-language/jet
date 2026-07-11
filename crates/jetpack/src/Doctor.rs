//! Card #479 — read-only `jetpack doctor` health report.

use super::{Envelope, FFI, JSON, Store};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const STALE_AFTER: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Health { Healthy, Degraded, Broken }

impl Health {
    fn word(self) -> &'static str { match self { Self::Healthy => "healthy", Self::Degraded => "degraded", Self::Broken => "broken" } }
    fn mark(self) -> &'static str { match self { Self::Healthy => "ok", Self::Degraded => "warn", Self::Broken => "fail" } }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check { pub name: &'static str, pub health: Health, pub detail: String, pub fix: String }

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report { pub checks: Vec<Check> }

impl Report {
    pub fn health(&self) -> Health { self.checks.iter().map(|c| c.health).max().unwrap_or(Health::Healthy) }
    pub fn exit_code(&self) -> i32 { match self.health() { Health::Healthy => 0, Health::Degraded => 1, Health::Broken => 2 } }
    pub fn to_human(&self) -> String {
        let mut out = String::from("jetpack doctor — checking package-manager health\n");
        for c in &self.checks {
            out.push_str(&format!("  [{:>4}] {:<12} — {}\n", c.health.mark(), c.name, c.detail));
            if !c.fix.is_empty() { out.push_str(&format!("         fix: {}\n", c.fix)); }
        }
        out.push_str(&format!("result: {}\n", self.health().word()));
        out
    }
    pub fn to_json(&self) -> String {
        let checks = self.checks.iter().map(|c| format!(
            "{{\"name\":{},\"status\":{},\"detail\":{},\"fix\":{}}}",
            JSON::quote(c.name), JSON::quote(c.health.word()), JSON::quote(&c.detail), JSON::quote(&c.fix)
        )).collect::<Vec<_>>().join(",");
        format!("{{\"kind\":\"jetpack.doctor\",\"status\":{},\"checks\":[{}]}}", JSON::quote(self.health().word()), checks)
    }
}

pub fn run(project: &Path, online: bool) -> Report {
    let roots = Store::resolve();
    Report { checks: vec![check_hangar(&roots), check_registry(online), check_locks(project, &roots), check_cache(&roots), check_signing_key()] }
}

fn check_hangar(roots: &Store::Roots) -> Check {
    let hangar = roots.hangar_dir();
    if !hangar.exists() { return ok("hangar", "empty; created on first realization"); }
    let rd = match fs::read_dir(&hangar) { Ok(rd) => rd, Err(e) => return broken("hangar", format!("unreadable ({})", e.kind()), "restore read access to the Jetpack root") };
    let entries = Store::list(roots);
    let known: BTreeMap<_, _> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut objects = 0usize;
    for next in rd {
        let ent = match next {
            Ok(ent) => ent,
            Err(e) => return broken("hangar", format!("directory enumeration failed ({})", e.kind()), "restore read access to every Hangar object"),
        };
        let path = ent.path();
        if !path.is_dir() { continue; }
        if ent.file_name() == "build-scratch" { continue; }
        objects += 1;
        let file_name = ent.file_name();
        let id = file_name.to_string_lossy();
        if !path.join("meta.json").is_file() { return broken("hangar", format!("object `{id}` has no metadata"), "remove the incomplete object with `jetpack clean`, then realize it again") }
        let Some(entry) = known.get(id.as_ref()) else { return broken("hangar", format!("object `{id}` has malformed metadata"), "run `jetpack clean`, then realize the package again") };
        if !Path::new(&entry.out).exists() { return broken("hangar", format!("object `{id}` points to a missing output"), "realize the package again") }
        match Envelope::try_output_hash_of(&entry.out) {
            Ok(actual) if !entry.envelope.output_hash.is_empty() && actual == entry.envelope.output_hash => {}
            Ok(_) => return broken("hangar", format!("object `{id}` failed its content digest"), "remove the corrupt object with `jetpack clean`, then realize it again"),
            Err(_) => return broken("hangar", format!("object `{id}` cannot be hashed safely"), "remove the corrupt object with `jetpack clean`, then realize it again"),
        }
    }
    ok("hangar", format!("{objects} object(s) readable and content-verified"))
}

fn registry_endpoints() -> Vec<(String, String)> {
    let mut found = BTreeMap::new();
    if let Ok(url) = std::env::var("JET_REGISTRY_URL") { if !url.is_empty() { found.insert("jet".to_string(), url); } }
    for (key, url) in std::env::vars() {
        if let Some(name) = key.strip_prefix("JET_REGISTRY_").and_then(|s| s.strip_suffix("_URL")) {
            if !name.is_empty() && !url.is_empty() { found.insert(name.to_ascii_lowercase(), url); }
        }
    }
    if found.is_empty() { found.insert("jet".into(), "https://github.com/jet-lang/registry".into()); }
    found.into_iter().collect()
}

fn check_registry(online: bool) -> Check {
    let endpoints = registry_endpoints();
    let mut skipped = 0;
    for (name, url) in &endpoints {
        match endpoint_reachable(url, online) {
            Ok(true) => {}
            Ok(false) => skipped += 1,
            Err(reason) => return broken("registry", format!("`{name}` is unreachable ({reason})"), "check registry or mirror configuration and credentials, then rerun with `--online`"),
        }
    }
    if skipped > 0 { ok("registry", format!("{} configured; network probe skipped (offline-safe default)", endpoints.len())) }
    else { ok("registry", format!("{} configured endpoint(s) reachable", endpoints.len())) }
}

fn endpoint_reachable(url: &str, online: bool) -> Result<bool, &'static str> {
    if let Some(path) = url.strip_prefix("file://") { return if Path::new(path).exists() { Ok(true) } else { Err("local index missing") }; }
    if !url.contains("://") { return if Path::new(url).exists() { Ok(true) } else { Err("local index missing") }; }
    if !online { return Ok(false); }
    if url.starts_with("https://") { return git_registry_probe(url).map(|_| true); }
    let (default_port, rest) = if let Some(v) = url.strip_prefix("http://") { (80, v) } else { return Err("unsupported URL scheme") };
    let raw_authority = rest.split('/').next().unwrap_or("");
    let (credentials, authority) = raw_authority.rsplit_once('@')
        .map(|(credentials, authority)| (Some(credentials), authority))
        .unwrap_or((None, raw_authority));
    let (host, port) = authority.rsplit_once(':').and_then(|(h,p)| p.parse().ok().map(|p| (h,p))).unwrap_or((authority, default_port));
    if host.is_empty() { return Err("missing host"); }
    let addrs = (host, port).to_socket_addrs().map_err(|_| "name lookup failed")?;
    let path = format!("/{}", rest.split_once('/').map(|(_, p)| p).unwrap_or(""));
    for addr in addrs {
        let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else { continue };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let authorization = credentials.map(|value| format!("Authorization: Basic {}\r\n", base64(value.as_bytes()))).unwrap_or_default();
        let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{authorization}Connection: close\r\n\r\n");
        if stream.write_all(request.as_bytes()).is_err() { continue; }
        let mut response = [0u8; 256];
        let Ok(n) = stream.read(&mut response) else { continue };
        let status = std::str::from_utf8(&response[..n]).ok().and_then(|s| s.split_whitespace().nth(1)).and_then(|s| s.parse::<u16>().ok());
        return match status { Some(200..=399) => Ok(true), Some(_) => Err("HTTP status rejected"), None => Err("malformed HTTP response") };
    }
    Err("connection failed")
}

fn git_registry_probe(url: &str) -> Result<(), &'static str> {
    let mut child = Command::new("git").args(["ls-remote", url, "HEAD"])
        .env("GIT_TERMINAL_PROMPT", "0").stdout(Stdio::null()).stderr(Stdio::null())
        .spawn().map_err(|_| "git registry client unavailable")?;
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return if status.success() { Ok(()) } else { Err("HTTPS registry rejected probe") },
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => { let _ = child.kill(); let _ = child.wait(); return Err("HTTPS registry probe timed out") }
            Err(_) => return Err("HTTPS registry probe failed"),
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = ((chunk[0] as u32) << 16) | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8) | chunk.get(2).copied().unwrap_or(0) as u32;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 { TABLE[((n >> 6) & 63) as usize] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[(n & 63) as usize] as char } else { '=' });
    }
    out
}

fn check_locks(project: &Path, roots: &Store::Roots) -> Check {
    let dirs = [roots.root.join(".locks"), Store::managed_dir(project).join(".locks")];
    let mut stale = BTreeSet::new();
    let mut unknown = BTreeSet::new();
    for dir in dirs {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => { unknown.insert(format!("{} (directory unreadable)", dir.display())); continue; }
        };
        for next in rd {
        let ent = match next { Ok(ent) => ent, Err(_) => { unknown.insert("directory entry unreadable".to_string()); continue; } };
        let path = ent.path(); if path.extension().and_then(|s| s.to_str()) != Some("lock") { continue; }
        match lock_state(&path) {
            LockState::Stale => { stale.insert(ent.file_name().to_string_lossy().into_owned()); }
            LockState::Unknown => { unknown.insert(ent.file_name().to_string_lossy().into_owned()); }
            LockState::Live => {}
        }
    }}
    if !stale.is_empty() { degraded("locks", format!("{} stale lock(s): {}", stale.len(), stale.into_iter().collect::<Vec<_>>().join(", ")), "remove stale lock files after confirming no Jetpack process is running") }
    else if !unknown.is_empty() { degraded("locks", format!("{} lock(s) have unknown liveness: {}", unknown.len(), unknown.into_iter().collect::<Vec<_>>().join(", ")), "inspect the lock owner; never delete a lock whose process identity is unknown") }
    else { ok("locks", "no stale command locks") }
}

enum LockState { Live, Stale, Unknown }

fn lock_state(path: &Path) -> LockState {
    let Ok(text) = fs::read_to_string(path) else { return LockState::Unknown };
    let pid = text.lines().find_map(|line| line.strip_prefix("pid=")?.parse::<u32>().ok());
    let Some(pid) = pid else { return LockState::Unknown };
    #[cfg(target_os="linux")]
    {
        let proc = Path::new("/proc").join(pid.to_string());
        if !proc.exists() { return LockState::Stale; }
        let recorded = text.lines().find_map(|line| line.strip_prefix("process_start="));
        let current = process_start_identity(pid);
        return match (recorded, current.as_deref()) { (Some(a), Some(b)) if a == b => LockState::Live, (Some(_), Some(_)) => LockState::Stale, _ => LockState::Unknown };
    }
    #[cfg(not(target_os="linux"))]
    { let _ = pid; LockState::Unknown }
}

#[cfg(target_os="linux")]
fn process_start_identity(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(Path::new("/proc").join(pid.to_string()).join("stat")).ok()?;
    let after = stat.rsplit_once(") ")?.1;
    after.split_whitespace().nth(19).map(str::to_string)
}

fn check_cache(roots: &Store::Roots) -> Check {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let stale = Store::list(roots).into_iter().filter(|e| e.last_used_at == 0 || now.saturating_sub(e.last_used_at) > STALE_AFTER).count();
    if stale == 0 { ok("cache", "no objects unused for more than 30 days") }
    else { degraded("cache", format!("{stale} object(s) unused for more than 30 days"), "run `jetpack clean` to review and collect stale objects") }
}

fn keys_dir() -> PathBuf {
    std::env::var_os("JET_KEYS_DIR").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join(".jet/keys")
    })
}

fn check_signing_key() -> Check {
    let registry = std::env::var("JET_REGISTRY_NAME").unwrap_or_else(|_| "jet".into());
    let secret = keys_dir().join(format!("{registry}.ed25519"));
    let public = keys_dir().join(format!("{registry}.ed25519.pub"));
    if !secret.is_file() || !public.is_file() { return degraded("signing", format!("signing key for `{registry}` is missing or incomplete"), "run `jet registry keygen` before publishing") }
    match (fs::metadata(&secret), fs::read_to_string(&public)) {
        (Ok(meta), Ok(key)) if meta.len() == 32 && key.trim().len() == 64 && key.trim().bytes().all(|b| b.is_ascii_hexdigit()) => match key_pair_matches(&secret, key.trim()) {
            Ok(true) => ok("signing", format!("signing key for `{registry}` is present and matches its public key")),
            Ok(false) => broken("signing", format!("signing key for `{registry}` does not match its public key"), "restore the matching public key from backup or rotate the pair deliberately"),
            Err(_) => degraded("signing", format!("signing key for `{registry}` could not be cryptographically checked"), "check the signing bridge, then rerun `jetpack doctor`"),
        },
        _ => broken("signing", format!("signing key for `{registry}` is malformed"), "restore the key pair from backup or rotate it deliberately with `jet registry keygen --force`"),
    }
}

fn key_pair_matches(secret: &Path, public_hex: &str) -> Result<bool, ()> {
    let seed = fs::read(secret).map_err(|_| ())?;
    let helper = FFI::cached_crypto_helper_path();
    if !helper.is_file() { return Err(()); }
    let challenge = b"jetpack-doctor-keypair-v1";
    let command = format!("sign {} {}", hex(&seed), hex(challenge));
    let signature = run_crypto_helper(&helper, &command)?;
    let verify = format!("verify {} {} {}", public_hex, hex(challenge), signature);
    Ok(run_crypto_helper_status(&helper, &verify)? == 0)
}

fn run_crypto_helper(helper: &Path, command: &str) -> Result<String, ()> {
    let mut child = Command::new(helper).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|_| ())?;
    child.stdin.take().ok_or(())?.write_all(command.as_bytes()).map_err(|_| ())?;
    let out = child.wait_with_output().map_err(|_| ())?;
    if !out.status.success() { return Err(()); }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn run_crypto_helper_status(helper: &Path, command: &str) -> Result<i32, ()> {
    let mut child = Command::new(helper).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_| ())?;
    child.stdin.take().ok_or(())?.write_all(command.as_bytes()).map_err(|_| ())?;
    Ok(child.wait().map_err(|_| ())?.code().unwrap_or(-1))
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn ok(name: &'static str, detail: impl Into<String>) -> Check { Check { name, health: Health::Healthy, detail: detail.into(), fix: String::new() } }
fn degraded(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check { Check { name, health: Health::Degraded, detail: detail.into(), fix: fix.into() } }
fn broken(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check { Check { name, health: Health::Broken, detail: detail.into(), fix: fix.into() } }

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("doctor-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn hangar_enumeration_rejects_missing_and_malformed_metadata() {
        let root = scratch("enumeration");
        let roots = Store::Roots { root: root.clone(), dev_mode: false };
        fs::create_dir_all(roots.hangar_dir().join("missing-meta")).unwrap();
        assert!(check_hangar(&roots).detail.contains("has no metadata"));
        fs::remove_dir_all(roots.hangar_dir().join("missing-meta")).unwrap();
        fs::create_dir_all(roots.hangar_dir().join("bad-meta")).unwrap();
        fs::write(roots.hangar_dir().join("bad-meta/meta.json"), "not json").unwrap();
        assert!(check_hangar(&roots).detail.contains("malformed metadata"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn lock_liveness_is_live_dead_or_unknown_without_age_guessing() {
        let root = scratch("locks");
        let live = root.join("live.lock");
        let malformed = root.join("malformed.lock");
        let dead = root.join("dead.lock");
        #[cfg(target_os = "linux")]
        {
            let start = process_start_identity(std::process::id()).unwrap();
            fs::write(&live, format!("pid={}\nprocess_start={start}\n", std::process::id())).unwrap();
            assert!(matches!(lock_state(&live), LockState::Live));
            fs::write(&dead, "pid=4294967294\nprocess_start=1\n").unwrap();
            assert!(matches!(lock_state(&dead), LockState::Stale));
        }
        fs::write(&malformed, "old but no process identity\n").unwrap();
        assert!(matches!(lock_state(&malformed), LockState::Unknown));
        let policy_root = scratch("lock-read-errors");
        fs::write(policy_root.join(".locks"), "not a directory").unwrap();
        let roots = Store::Roots { root: policy_root.clone(), dev_mode: false };
        assert!(check_locks(&root, &roots).detail.contains("directory unreadable"));
        fs::remove_file(policy_root.join(".locks")).unwrap();
        fs::create_dir_all(policy_root.join(".locks")).unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("loop.lock", policy_root.join(".locks/loop.lock")).unwrap();
            assert!(check_locks(&root, &roots).detail.contains("unknown liveness"));
        }
        fs::remove_dir_all(policy_root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
