//! Script-speed warm run cache at the tier boundary (#741).
//!
//! Keys an unchanged `jet run` by source + WatchService dependency stamps +
//! compiler-build identity + configuration. A hit reloads a captured tier-1
//! Cranelift module (see `jet_jit::run_cached_module`) and skips load/parse/
//! check/TIR lowering/codegen. Does not touch AOT [`BuildCache`].

use crate::SHA256::sha256_hex;
use jet_devserver::WatchService::{PathStamp, RootKind, WatchGraph};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

static PARSE: AtomicU64 = AtomicU64::new(0);
static CHECK: AtomicU64 = AtomicU64::new(0);
static LOWER: AtomicU64 = AtomicU64::new(0);
static CODEGEN: AtomicU64 = AtomicU64::new(0);
static LINK: AtomicU64 = AtomicU64::new(0);
static CACHE_HIT: AtomicU64 = AtomicU64::new(0);
static CACHE_MISS: AtomicU64 = AtomicU64::new(0);
static SIGNPOST_SHOWN: AtomicBool = AtomicBool::new(false);

/// Phase counters for trace proof (tests + `JET_RUN_TRACE=1`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunPhases {
    pub parse: u64,
    pub check: u64,
    pub lower: u64,
    pub codegen: u64,
    pub link: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

pub fn reset_phases() {
    PARSE.store(0, Ordering::Relaxed);
    CHECK.store(0, Ordering::Relaxed);
    LOWER.store(0, Ordering::Relaxed);
    CODEGEN.store(0, Ordering::Relaxed);
    LINK.store(0, Ordering::Relaxed);
    CACHE_HIT.store(0, Ordering::Relaxed);
    CACHE_MISS.store(0, Ordering::Relaxed);
}

pub fn note_parse() {
    PARSE.fetch_add(1, Ordering::Relaxed);
}
pub fn note_check() {
    CHECK.fetch_add(1, Ordering::Relaxed);
}
pub fn note_lower() {
    LOWER.fetch_add(1, Ordering::Relaxed);
}
pub fn note_codegen() {
    CODEGEN.fetch_add(1, Ordering::Relaxed);
}
pub fn note_link() {
    LINK.fetch_add(1, Ordering::Relaxed);
}

pub fn phases() -> RunPhases {
    RunPhases {
        parse: PARSE.load(Ordering::Relaxed),
        check: CHECK.load(Ordering::Relaxed),
        lower: LOWER.load(Ordering::Relaxed),
        codegen: CODEGEN.load(Ordering::Relaxed),
        link: LINK.load(Ordering::Relaxed),
        cache_hits: CACHE_HIT.load(Ordering::Relaxed),
        cache_misses: CACHE_MISS.load(Ordering::Relaxed),
    }
}

fn cache_root() -> PathBuf {
    if let Ok(dir) = std::env::var("JET_RUN_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("JET_CACHE_DIR") {
        return PathBuf::from(dir).join("run");
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".cache").join("jet").join("run")
}

fn compiler_identity() -> String {
    use std::sync::OnceLock;
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY
        .get_or_init(|| {
            #[cfg(target_os = "linux")]
            let meta = fs::metadata("/proc/self/exe");
            #[cfg(not(target_os = "linux"))]
            let meta = std::env::current_exe().and_then(fs::metadata);
            let build = match meta {
                Ok(m) => format!(
                    "{}:{}:{:?}",
                    m.len(),
                    m.modified().ok().map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    }).unwrap_or(0),
                    std::env::current_exe().ok()
                ),
                Err(_) => "unavailable".into(),
            };
            // Fold a short content prefix so rebuilds that keep mtime still miss.
            #[cfg(target_os = "linux")]
            let tip = fs::File::open("/proc/self/exe")
                .ok()
                .and_then(|mut f| {
                    use std::io::Read;
                    let mut buf = [0u8; 4096];
                    let n = f.read(&mut buf).ok()?;
                    Some(sha256_hex(&buf[..n]))
                })
                .unwrap_or_else(|| "notip".into());
            #[cfg(not(target_os = "linux"))]
            let tip = "notip".to_string();
            format!(
                "abi=1\u{1}build={build}\u{1}tip={tip}\u{1}version={}",
                crate::Manifest::COMPILER_VERSION
            )
        })
        .clone()
}

fn path_digest(path: &Path) -> String {
    match fs::read(path) {
        Ok(bytes) => sha256_hex(&bytes),
        Err(_) => {
            let stamp = PathStamp::capture(path);
            format!(
                "missing:{}:{:?}",
                stamp.exists,
                stamp.len.unwrap_or(0)
            )
        }
    }
}

/// Content + dependency + compiler + config key for a script run.
pub fn run_cache_key(entry: &Path, program_args: &[&str]) -> String {
    let mut graph = match WatchGraph::discover(entry) {
        Ok(graph) => graph,
        Err(diagnostic) => {
            // Fail closed: an authority error yields a key no healthy run can
            // produce, so the cache misses and the real run reports it.
            return format!("jet-run-cache-v1:discover-error:{}:{}", entry.display(), diagnostic.code);
        }
    };
    graph.refresh_stamps();
    let mut chunks = Vec::new();
    chunks.push(b"jet-run-cache-v1".to_vec());
    chunks.push(compiler_identity().into_bytes());
    chunks.push(format!("args={}", program_args.join("\u{1}")).into_bytes());
    let mut paths = graph.watched_paths();
    paths.sort();
    for path in paths {
        let kind = graph
            .nodes()
            .find(|n| n.path == path)
            .map(|n| n.kind)
            .unwrap_or(RootKind::Import);
        chunks.push(format!("{}:{}", kind.as_str(), path.display()).into_bytes());
        chunks.push(path_digest(&path).into_bytes());
        let stamp = PathStamp::capture(&path);
        chunks.push(format!("{:?}:{:?}", stamp.mtime, stamp.len).into_bytes());
    }
    let mut flat = Vec::new();
    for c in chunks {
        flat.extend_from_slice(&(c.len() as u64).to_be_bytes());
        flat.extend_from_slice(&c);
    }
    sha256_hex(&flat)
}

fn entry_dir(key: &str) -> PathBuf {
    cache_root().join(key)
}

/// Try a warm tier-1 module hit. On success returns the run outcome.
pub fn try_warm_run(entry: &Path, program_args: &[&str]) -> Option<jet_foundation::JitBackend::RunOutcome> {
    let key = run_cache_key(entry, program_args);
    let dir = entry_dir(&key);
    let artifact_path = dir.join("module.bin");
    let bytes = fs::read(&artifact_path).ok()?;
    // Install program argv exactly like the cold path (#1254): without it,
    // `core.io.args()` in a warm run falls back to the raw CLI argv.
    let mut argv = Vec::with_capacity(program_args.len() + 1);
    argv.push(entry.to_string_lossy().into_owned());
    argv.extend(program_args.iter().map(|arg| (*arg).to_string()));
    match jet_jit::with_program_args(&argv, || jet_jit::run_cached_module(&bytes)) {
        Ok(outcome) => {
            CACHE_HIT.fetch_add(1, Ordering::Relaxed);
            if std::env::var_os("JET_RUN_TRACE").is_some() {
                eprintln!("[run-cache] hit key={key}");
            }
            Some(outcome)
        }
        Err(err) => {
            if std::env::var_os("JET_RUN_TRACE").is_some() {
                eprintln!("[run-cache] load-failed key={key}: {err}");
            }
            let _ = fs::remove_dir_all(&dir);
            None
        }
    }
}

/// Store the tier-1 artifact from the just-finished native compile, if any.
pub fn store_after_miss(entry: &Path, program_args: &[&str]) {
    CACHE_MISS.fetch_add(1, Ordering::Relaxed);
    let Some(artifact) = jet_jit::take_last_tier_artifact() else {
        if std::env::var_os("JET_RUN_TRACE").is_some() {
            eprintln!("[run-cache] miss (no tier-1 artifact)");
        }
        return;
    };
    let key = run_cache_key(entry, program_args);
    let dir = entry_dir(&key);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let dest = dir.join("module.bin");
    let tmp = dir.join(format!("module.bin.tmp.{}", std::process::id()));
    if fs::write(&tmp, &artifact).is_err() {
        let _ = fs::remove_file(&tmp);
        return;
    }
    if fs::rename(&tmp, &dest).is_err() {
        let _ = fs::copy(&tmp, &dest);
        let _ = fs::remove_file(&tmp);
    }
    if std::env::var_os("JET_RUN_TRACE").is_some() {
        eprintln!("[run-cache] store key={key} bytes={}", artifact.len());
    }
}

/// Clear the once-per-workflow signpost latch (tests).
pub fn reset_signpost_for_test() {
    SIGNPOST_SHOWN.store(false, Ordering::Relaxed);
}

/// Exact signpost line (stderr only; never program stdout).
pub fn signpost_line() -> &'static str {
    "tip: for a faster edit loop, use `jet dev` (watches and reuses the resident JIT)"
}

/// Whether a slow-run tip would print (does not consume the once-guard).
pub fn signpost_eligible(started: Instant, is_tty: bool) -> bool {
    if SIGNPOST_SHOWN.load(Ordering::Relaxed) {
        return false;
    }
    if !is_tty {
        return false;
    }
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("JET_JSON").is_some() {
        return false;
    }
    started.elapsed().as_millis() >= 200
}

/// One-line `jet dev` tip when a cold compile was slow. Once per process.
///
/// Conditions are checked before the once-latch so a silent non-TTY / NO_COLOR
/// probe does not burn the tip for a later useful slow run.
pub fn maybe_signpost(started: Instant, is_tty: bool) {
    if !signpost_eligible(started, is_tty) {
        return;
    }
    if SIGNPOST_SHOWN.swap(true, Ordering::Relaxed) {
        return;
    }
    let _ = writeln!(std::io::stderr(), "{}", signpost_line());
}

pub fn stderr_is_tty() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}
