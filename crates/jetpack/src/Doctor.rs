//! Card #479 — read-only `jetpack doctor` health report.

use super::{Envelope, JSON, Store};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_dir() || !path.join("meta.json").exists() { continue; }
        objects += 1;
        let file_name = ent.file_name();
        let id = file_name.to_string_lossy();
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
    let (default_port, rest) = if let Some(v) = url.strip_prefix("https://") { (443, v) } else if let Some(v) = url.strip_prefix("http://") { (80, v) } else { return Err("unsupported URL scheme") };
    let authority = rest.split('/').next().unwrap_or("").rsplit('@').next().unwrap_or("");
    let (host, port) = authority.rsplit_once(':').and_then(|(h,p)| p.parse().ok().map(|p| (h,p))).unwrap_or((authority, default_port));
    if host.is_empty() { return Err("missing host"); }
    let addrs = (host, port).to_socket_addrs().map_err(|_| "name lookup failed")?;
    for addr in addrs { if TcpStream::connect_timeout(&addr, Duration::from_secs(2)).is_ok() { return Ok(true); } }
    Err("connection failed")
}

fn check_locks(project: &Path, roots: &Store::Roots) -> Check {
    let dirs = [roots.root.join(".locks"), Store::managed_dir(project).join(".locks")];
    let mut stale = BTreeSet::new();
    for dir in dirs { let Ok(rd) = fs::read_dir(dir) else { continue }; for ent in rd.flatten() {
        let path = ent.path(); if path.extension().and_then(|s| s.to_str()) != Some("lock") { continue; }
        if lock_is_stale(&path) { stale.insert(ent.file_name().to_string_lossy().into_owned()); }
    }}
    if stale.is_empty() { ok("locks", "no stale command locks") }
    else { degraded("locks", format!("{} stale lock(s): {}", stale.len(), stale.into_iter().collect::<Vec<_>>().join(", ")), "remove stale lock files after confirming no Jetpack process is running") }
}

fn lock_is_stale(path: &Path) -> bool {
    if let Ok(text) = fs::read_to_string(path) {
        if let Some(pid) = text.trim().strip_prefix("pid=").and_then(|s| s.parse::<u32>().ok()) {
            #[cfg(target_os="linux")] { return !Path::new("/proc").join(pid.to_string()).exists(); }
        }
    }
    fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|m| SystemTime::now().duration_since(m).ok()).is_some_and(|age| age.as_secs() > 24 * 60 * 60)
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
        (Ok(meta), Ok(key)) if meta.len() == 32 && key.trim().len() == 64 && key.trim().bytes().all(|b| b.is_ascii_hexdigit()) => ok("signing", format!("signing key for `{registry}` is present")),
        _ => broken("signing", format!("signing key for `{registry}` is malformed"), "restore the key pair from backup or rotate it deliberately with `jet registry keygen --force`"),
    }
}

fn ok(name: &'static str, detail: impl Into<String>) -> Check { Check { name, health: Health::Healthy, detail: detail.into(), fix: String::new() } }
fn degraded(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check { Check { name, health: Health::Degraded, detail: detail.into(), fix: fix.into() } }
fn broken(name: &'static str, detail: impl Into<String>, fix: impl Into<String>) -> Check { Check { name, health: Health::Broken, detail: detail.into(), fix: fix.into() } }
