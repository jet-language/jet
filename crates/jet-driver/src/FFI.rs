//! M7 hidden cargo FFI bridge (S50).
//!
//! When a program declares `extern rust` blocks with crate dependencies, the
//! driver materializes a cached cargo project under `~/.cache/jet/ffi/` and
//! links the built rlib into the user's generated program.

use crate::Diagnostics::Diagnostic;
use crate::AST::{AccessConvention, ExternFn, ExternRustBlock, Item, ProgramBundle, Type};
use std::collections::{hash_map::DefaultHasher, BTreeMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

// FfiLink struct lives in AST for cross-seam sharing; re-export here.
pub use crate::AST::FfiLink;

/// One foreign function collected from the import graph.
#[derive(Debug, Clone)]
pub struct ExternEntry {
    pub jet_name: String,
    pub rust_path: String,
    pub wrapper_name: String,
    pub params: Vec<(AccessConvention, Type)>,
    pub return_type: Option<Type>,
    pub crate_spec: String,
    /// Human-facing hint for E0705 (`extern` line context).
    pub line_hint: String,
}

/// Gather every `extern rust` function across all modules.
pub fn collect_externs(bundle: &ProgramBundle) -> Vec<ExternEntry> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        for item in &module.items {
            let Item::ExternRust(block) = item else {
                continue;
            };
            for ef in &block.functions {
                out.push(extern_entry(ef, block, &module.display));
            }
        }
    }
    out
}

fn extern_entry(ef: &ExternFn, block: &ExternRustBlock, _file: &str) -> ExternEntry {
    ExternEntry {
        jet_name: ef.name.clone(),
        rust_path: ef.rust_path.clone(),
        wrapper_name: format!("jet_ffi_{}", ef.name),
        params: ef
            .params
            .iter()
            .map(|p| (p.convention, p.ty.clone()))
            .collect(),
        return_type: ef.return_type.clone(),
        crate_spec: block.crate_spec.clone(),
        line_hint: format!("`{}` in `extern rust \"{}\"`", ef.name, block.crate_spec),
    }
}

/// Build (or reuse) the hidden wrapper crate. Returns `Ok(None)` when the
/// program has no `extern rust` declarations and does not use `jet.regex`,
/// `core.archive`, `jet.db`, or `core.compress.{gzip,zstd}`.
///
/// `jet.regex` (D-REGEX1), `core.archive` (D-DEP-ARCHIVE1), `jet.db`
/// (D-DEP-DB1), and `core.compress` (D-CODECS1) are delivered through this
/// same hidden-cargo bridge: when a program imports any of them, the bridge
/// crate gains the matching dependency and a hand-written runtime
/// (`Source/Prelude/Regex.rs`, `Source/Prelude/Archive.rs`,
/// `Source/Prelude/Db.rs`, `Source/Prelude/Compress.rs`). The compiler crate
/// (`Source/`) stays zero-dependency (I6). These are the owner-approved I6
/// bootstrap exceptions, to be native-ized before the end of Epoch 3.
pub fn prepare(bundle: &ProgramBundle) -> Result<Option<FfiLink>, Vec<Diagnostic>> {
    let entries = collect_externs(bundle);
    // `used_core` entries are `"{module}::{method}"` (e.g. `jet.regex::is_match`),
    // so match on the module prefix, not a bare module name.
    let needs_regex = bundle
        .used_core
        .iter()
        .any(|u| u == "jet.regex" || u.starts_with("jet.regex::"));
    let needs_archive = bundle
        .used_core
        .iter()
        .any(|u| u == "core.archive" || u.starts_with("core.archive::"));
    let needs_db = bundle
        .used_core
        .iter()
        .any(|u| u == "jet.db" || u.starts_with("jet.db::"));
    // D-CODECS1: standalone `core.compress.gzip` / `core.compress.zstd` codecs.
    let needs_compress = bundle.used_core.iter().any(|u| {
        u == "core.compress.gzip"
            || u.starts_with("core.compress.gzip::")
            || u == "core.compress.zstd"
            || u.starts_with("core.compress.zstd::")
    });
    // D-NETDEP1=A / D-HTTPLIB4=B: ureq + rustls for core.http.client TLS support.
    let needs_http_client = bundle
        .used_core
        .iter()
        .any(|u| u == "core.http.client" || u.starts_with("core.http.client::"));
    // D-DEP-CRYPTO1=A: RustCrypto AEAD + Ed25519 for core.crypto envelope APIs.
    let needs_crypto = bundle.used_core.iter().any(|u| {
        u == "jet.crypto"
            || u.starts_with("jet.crypto::")
            || u == "core.crypto.expert"
            || u.starts_with("core.crypto.expert::")
    });
    // D-DEP-WASM1=A (c81): `core.plugin` — the sandboxed WASM Component Model
    // plugin loader (`Plugin.load`/`.call`).
    let needs_plugin = bundle
        .used_core
        .iter()
        .any(|u| u == "jet.plugin" || u.starts_with("jet.plugin::"));
    if entries.is_empty()
        && !needs_regex
        && !needs_archive
        && !needs_db
        && !needs_http_client
        && !needs_crypto
        && !needs_compress
        && !needs_plugin
    {
        return Ok(None);
    }

    build_bridge(
        &entries,
        needs_regex,
        needs_archive,
        needs_db,
        needs_http_client,
        needs_crypto,
        needs_compress,
        needs_plugin,
    )
    .map(Some)
}

/// The `regex` crate version that backs `jet.regex` (D-REGEX1). Lives only here,
/// never in the compiler's Cargo.toml (I6).
pub const REGEX_CRATE_SPEC: (&str, &str) = ("regex", "1");

/// The `flate2` crate version that backs `core.archive` gzip (D-DEP-ARCHIVE1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const ARCHIVE_CRATE_SPEC: (&str, &str) = ("flate2", "1");

/// The `zip` crate version that backs `core.archive` zip (D-DEP-ARCHIVE1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const ZIP_CRATE_SPEC: (&str, &str) = ("zip", "2");

/// The `tar` crate version that backs `core.archive` tar (D-DEP-ARCHIVE1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const TAR_CRATE_SPEC: (&str, &str) = ("tar", "0");

/// The `rusqlite` crate version that backs `jet.db` (D-DEP-DB1).
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const DB_CRATE_SPEC: (&str, &str) = ("rusqlite", "0.31");

/// The `ureq` crate version that backs `core.http.client` (D-NETDEP1=A, D-HTTPLIB4=B).
/// rustls is pulled in automatically via `ureq`'s `tls` feature flag.
/// Lives only here — never in the compiler's Cargo.toml (I6).
pub const HTTP_CLIENT_CRATE_SPEC: (&str, &str) = ("ureq", "2");

/// Hand-written HTTP client runtime emitted into the bridge crate when `core.http.client` is used.
const HTTP_CLIENT_RUNTIME: &str = include_str!("Prelude/Http.rs");

/// Crate dependency specs that require non-trivial TOML values (e.g. feature flags).
/// These are emitted verbatim as the right-hand side of the `name = …` line.
const FEATURED_DEPS: &[(&str, &str)] = &[
    (
        "rusqlite",
        "{ version = \"0.31\", features = [\"bundled\"] }",
    ),
    ("ureq", "{ version = \"2\", features = [\"tls\"] }"),
    (
        "wasmtime",
        "{ version = \"26\", features = [\"component-model\"] }",
    ),
];

/// Hand-written regex runtime emitted into the bridge crate when `jet.regex` is
/// used. This is the only code that touches the `regex` crate.
const REGEX_RUNTIME: &str = include_str!("Prelude/Regex.rs");

/// Hand-written archive runtime emitted into the bridge crate when `core.archive`
/// is used. This is the only code that touches the `flate2`, `zip`, and `tar` crates.
const ARCHIVE_RUNTIME: &str = include_str!("Prelude/Archive.rs");

/// Hand-written database runtime emitted into the bridge crate when `jet.db`
/// is used. This is the only code that touches the `rusqlite` crate.
const DB_RUNTIME: &str = include_str!("Prelude/Db.rs");

/// The `aes-gcm` crate version backing `core.crypto` envelope (D-DEP-CRYPTO1).
pub const AES_GCM_CRATE_SPEC: (&str, &str) = ("aes-gcm", "0.10");

/// The `chacha20poly1305` crate version backing `core.crypto` envelope (D-DEP-CRYPTO1).
pub const CHACHA_POLY_CRATE_SPEC: (&str, &str) = ("chacha20poly1305", "0.10");

/// The `ed25519-dalek` crate version backing `core.crypto.sign/verify` (D-DEP-CRYPTO1).
pub const ED25519_CRATE_SPEC: (&str, &str) = ("ed25519-dalek", "2");

/// Hand-written crypto runtime emitted into the bridge crate when `core.crypto`
/// seal/open/sign/verify is used (D-CRYPTOENV1, D-DEP-CRYPTO1).
const CRYPTO_RUNTIME: &str = include_str!("Prelude/Crypto.rs");

/// The `wasmtime` crate version that backs `core.plugin` (D-DEP-WASM1=A, c81).
/// Lives only here — never in the compiler's Cargo.toml (I6). Reuses the
/// already-approved Cranelift backend internally (D-JITDEP1).
pub const WASMTIME_CRATE_SPEC: (&str, &str) = ("wasmtime", "26");

/// Hand-written plugin-loader runtime emitted into the bridge crate when
/// `core.plugin` is used. This is the only place the `wasmtime` crate is
/// touched.
const PLUGIN_RUNTIME: &str = include_str!("Prelude/Plugin.rs");

/// The `flate2` crate version that backs `core.compress.gzip` (D-CODECS1).
/// Same crate as `core.archive`'s gzip; `core.compress` must also work standalone,
/// independent of `core.archive`, so it is inserted whenever `core.compress.gzip`
/// is used even if `core.archive` isn't. Lives only here — never in the
/// compiler's Cargo.toml (I6).
pub const COMPRESS_GZIP_CRATE_SPEC: (&str, &str) = ARCHIVE_CRATE_SPEC;

/// The `zstd` crate version that backs `core.compress.zstd` (D-CODECS1). Pure
/// bootstrap dep: the `zstd` crate is a Rust binding that vendors/builds the C
/// zstd source via `zstd-sys` at compile time (same I6 bootstrap-exception
/// posture as `rusqlite`'s bundled SQLite, `DB_CRATE_SPEC`). Lives only here —
/// never in the compiler's Cargo.toml (I6).
pub const COMPRESS_ZSTD_CRATE_SPEC: (&str, &str) = ("zstd", "0.13");

/// Hand-written compression runtime emitted into the bridge crate when
/// `core.compress.gzip` or `core.compress.zstd` is used (D-CODECS1). This is
/// the only place the standalone codec paths touch `flate2` / `zstd`.
const COMPRESS_RUNTIME: &str = include_str!("Prelude/Compress.rs");

pub fn build_bridge(
    entries: &[ExternEntry],
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
) -> Result<FfiLink, Vec<Diagnostic>> {
    let mut deps = collect_crate_deps(entries);
    if needs_regex {
        deps.insert(
            REGEX_CRATE_SPEC.0.to_string(),
            REGEX_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_archive {
        deps.insert(
            ARCHIVE_CRATE_SPEC.0.to_string(),
            ARCHIVE_CRATE_SPEC.1.to_string(),
        );
        deps.insert(ZIP_CRATE_SPEC.0.to_string(), ZIP_CRATE_SPEC.1.to_string());
        deps.insert(TAR_CRATE_SPEC.0.to_string(), TAR_CRATE_SPEC.1.to_string());
    }
    if needs_db {
        deps.insert(DB_CRATE_SPEC.0.to_string(), DB_CRATE_SPEC.1.to_string());
    }
    if needs_http_client {
        deps.insert(
            HTTP_CLIENT_CRATE_SPEC.0.to_string(),
            HTTP_CLIENT_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_crypto {
        deps.insert(
            AES_GCM_CRATE_SPEC.0.to_string(),
            AES_GCM_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            CHACHA_POLY_CRATE_SPEC.0.to_string(),
            CHACHA_POLY_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            ED25519_CRATE_SPEC.0.to_string(),
            ED25519_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_compress {
        // core.compress must work standalone, independent of core.archive, so
        // flate2 is inserted here too; BTreeMap::insert with the same key/value
        // when core.archive also pulled it in is a harmless no-op overwrite.
        deps.insert(
            COMPRESS_GZIP_CRATE_SPEC.0.to_string(),
            COMPRESS_GZIP_CRATE_SPEC.1.to_string(),
        );
        deps.insert(
            COMPRESS_ZSTD_CRATE_SPEC.0.to_string(),
            COMPRESS_ZSTD_CRATE_SPEC.1.to_string(),
        );
    }
    if needs_plugin {
        deps.insert(
            WASMTIME_CRATE_SPEC.0.to_string(),
            WASMTIME_CRATE_SPEC.1.to_string(),
        );
    }
    let key = cache_key_full(
        entries,
        &deps,
        needs_regex,
        needs_archive,
        needs_db,
        needs_http_client,
        needs_crypto,
        needs_compress,
        needs_plugin,
    );
    let cache_root = cache_dir().join(format!("{:016x}", key));
    let crate_name = format!("jet_ffi_{:016x}", key);
    let target_dir = cache_root.join("target");
    let target = target_dir.join("release");
    let rlib = target.join(format!("lib{}.rlib", crate_name));
    let deps_dir = target.join("deps");

    // c146: when the bridge carries crypto, it also emits a `jet-crypto-helper`
    // binary (a thin stdin wrapper around `jet_crypto_*_impl`) that `jet`'s own
    // publish/keygen path shells out to. Its path is fixed by the bin target name.
    let helper_bin = if needs_crypto {
        Some(target.join("jet-crypto-helper"))
    } else {
        None
    };
    // The build is only "cached and ready" when the rlib exists AND — if a helper
    // is expected — the helper binary exists too (a bridge cached before c146
    // landed would have the rlib but no helper, so fall through to rebuild).
    let helper_ready = helper_bin.as_ref().map(|p| p.is_file()).unwrap_or(true);

    // Fast path (cache hit): the key is content-derived from the exact
    // extern-rust signature / dep set, so an rlib already sitting at this
    // path is a valid build for this call — reuse it without touching
    // `cargo` or rewriting the cached sources. No lock needed: we don't
    // write anything.
    if rlib.is_file() && helper_ready {
        return Ok(FfiLink {
            crate_name,
            rlib_path: rlib,
            deps_dir,
            helper_bin_path: helper_bin,
        });
    }

    if !command_exists("cargo") {
        return Err(vec![Diagnostic::error(
            "E0703",
            "can't call foreign Rust crates without `cargo`".to_string(),
            "Jet builds a small helper crate for each `extern rust` dependency set".to_string(),
            "install Rust from https://rustup.rs (this includes `cargo`), then try again"
                .to_string(),
            None,
        )]);
    }

    fs::create_dir_all(&cache_root)
        .map_err(|e| tool_error(&format!("couldn't create the FFI cache folder: {}", e)))?;

    // Slow path (cache miss): another `jet` process may be building this same
    // key right now. Cargo's `CARGO_TARGET_DIR` lock protects `target/`, not
    // the `Cargo.toml`/`src/lib.rs` sources this function is about to
    // (re)write, so guard the write+build with our own cross-process lock,
    // scoped to this cache key — two processes on *different* keys never
    // block each other.
    let _lock = BuildLock::acquire(&cache_root)?;

    // Re-check under the lock: whoever held it may have just finished
    // building this exact key while we were waiting.
    if rlib.is_file() && helper_ready {
        return Ok(FfiLink {
            crate_name,
            rlib_path: rlib,
            deps_dir,
            helper_bin_path: helper_bin,
        });
    }

    let src_dir = cache_root.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| tool_error(&format!("couldn't create the FFI build folder: {}", e)))?;

    let manifest = cache_root.join("Cargo.toml");
    let lib_rs = src_dir.join("lib.rs");
    fs::write(&manifest, emit_cargo_toml(&crate_name, &deps))
        .map_err(|e| tool_error(&format!("couldn't write the FFI manifest: {}", e)))?;
    fs::write(
        &lib_rs,
        emit_wrapper_lib(
            entries,
            needs_regex,
            needs_archive,
            needs_db,
            needs_http_client,
            needs_crypto,
            needs_compress,
            needs_plugin,
        ),
    )
    .map_err(|e| tool_error(&format!("couldn't write the FFI wrappers: {}", e)))?;

    // c146: emit the crypto helper binary alongside the lib when crypto is in play.
    if needs_crypto {
        let bin_dir = src_dir.join("bin");
        fs::create_dir_all(&bin_dir)
            .map_err(|e| tool_error(&format!("couldn't create the FFI bin folder: {}", e)))?;
        fs::write(
            bin_dir.join("jet-crypto-helper.rs"),
            emit_crypto_helper_bin(&crate_name),
        )
        .map_err(|e| tool_error(&format!("couldn't write the crypto helper: {}", e)))?;
    }

    let out = Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .map_err(|e| {
            vec![Diagnostic::error(
                "E0703",
                format!("couldn't run `cargo`: {}", e),
                "Jet needs `cargo` to build foreign crate wrappers".to_string(),
                "install Rust from https://rustup.rs, then try again".to_string(),
                None,
            )]
        })?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if looks_like_signature_mismatch(&stderr) {
            let hint = entries
                .first()
                .map(|e| e.line_hint.clone())
                .unwrap_or_else(|| "an `extern rust` declaration".to_string());
            return Err(vec![Diagnostic::error(
                "E0705",
                format!(
                    "the Rust item doesn't match the Jet signature at {}",
                    hint
                ),
                "the `= \"rust::path\"` on an `extern rust` line must name a safe Rust function whose parameters and return type match what you wrote"
                    .to_string(),
                format!(
                    "check the path and types on {}, or pick a different Rust function",
                    hint
                ),
                None,
            )
            .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
        }
        let dep = deps
            .keys()
            .next()
            .map(|k| format!("{}@{}", k, deps[k]))
            .unwrap_or_else(|| "a foreign crate".to_string());
        return Err(vec![Diagnostic::error(
            "E0704",
            format!("couldn't fetch or build `{}`", dep),
            "pure-Rust crates only — crates that need system libraries or a build script aren't supported yet"
                .to_string(),
            "try a different crate version, check your network, or pick another crate"
                .to_string(),
            None,
        )
        .with_detail(format!("  cargo said:\n{}", stable_cargo_detail(&stderr)))]);
    }

    if !rlib.is_file() {
        return Err(tool_error(&format!(
            "FFI build finished but `{}` is missing",
            rlib.display()
        )));
    }
    if let Some(bin) = &helper_bin {
        if !bin.is_file() {
            return Err(tool_error(&format!(
                "FFI build finished but the crypto helper `{}` is missing",
                bin.display()
            )));
        }
    }
    Ok(FfiLink {
        crate_name,
        rlib_path: rlib,
        deps_dir,
        helper_bin_path: helper_bin,
    })
}

/// The crypto helper binary source (c146). A thin stdin-protocol wrapper around
/// the crate's own `jet_crypto_*_impl` functions (which is the *only* code that
/// touches `ed25519-dalek`, D-DEP-CRYPTO1). `jet` shells out to this exactly as
/// it already shells out to `cargo`/`rustc`, keeping the compiler crate itself
/// zero-dependency (I6). Protocol (one command line on stdin, hex-encoded args):
///   `keygen`                          → stdout `<seed_hex> <pub_hex>`
///   `sign <seed_hex> <msg_hex>`       → stdout `<sig_hex>`         (exit 0)
///   `verify <pub_hex> <msg_hex> <sig_hex>` → exit 0 valid / 2 invalid / 1 error
fn emit_crypto_helper_bin(crate_name: &str) -> String {
    format!(
        r#"// Auto-generated Ed25519 signing helper (card c146) — do not edit.
#![allow(warnings)]
use std::io::Read;
use std::process::exit;

fn hex_encode(bytes: &[u8]) -> String {{
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {{
        s.push_str(&format!("{{:02x}}", b));
    }}
    s
}}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {{
    let s = s.trim();
    if s.len() % 2 != 0 {{
        return Err("odd-length hex".to_string());
    }}
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {{
        let hi = (bytes[i] as char).to_digit(16).ok_or("bad hex digit")?;
        let lo = (bytes[i + 1] as char).to_digit(16).ok_or("bad hex digit")?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }}
    Ok(out)
}}

fn fail(msg: &str) -> ! {{
    eprintln!("error: {{}}", msg);
    exit(1);
}}

fn main() {{
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {{
        fail("couldn't read stdin");
    }}
    let mut parts = input.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    match cmd {{
        "keygen" => {{
            let (seed, public) = {crate_name}::jet_crypto_keygen_impl();
            println!("{{}} {{}}", hex_encode(&seed), hex_encode(&public));
        }}
        "sign" => {{
            let seed = parts.next().unwrap_or_else(|| fail("sign: missing key"));
            let msg = parts.next().unwrap_or_else(|| fail("sign: missing message"));
            let seed = hex_decode(seed).unwrap_or_else(|e| fail(&e));
            let msg = hex_decode(msg).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_crypto_sign_impl(&seed, &msg) {{
                Ok(sig) => println!("{{}}", hex_encode(&sig)),
                Err(e) => fail(&e),
            }}
        }}
        "verify" => {{
            let pk = parts.next().unwrap_or_else(|| fail("verify: missing public key"));
            let msg = parts.next().unwrap_or_else(|| fail("verify: missing message"));
            let sig = parts.next().unwrap_or_else(|| fail("verify: missing signature"));
            let pk = hex_decode(pk).unwrap_or_else(|e| fail(&e));
            let msg = hex_decode(msg).unwrap_or_else(|e| fail(&e));
            let sig = hex_decode(sig).unwrap_or_else(|e| fail(&e));
            match {crate_name}::jet_crypto_verify_impl(&pk, &msg, &sig) {{
                Ok(()) => exit(0),
                Err(_) => exit(2),
            }}
        }}
        other => fail(&format!("unknown command `{{}}`", other)),
    }}
}}
"#,
        crate_name = crate_name
    )
}

/// Cross-process lock guarding the slow path (rewrite + `cargo build`) of the
/// FFI bridge cache for one cache key. Same atomic `create_dir` + stale-steal
/// shape as `tests/common/mod.rs` `FfiBridgeLock`, kept as a *separate* lock
/// (different failure domain: this one guards real concurrent `jet`
/// processes; the test lock also serializes different test *binaries* in the
/// same suite run) — scoped per cache key rather than global, and
/// error-returning instead of panicking, since this runs in the compiler
/// itself and must never crash a build (I2: no path here may surface as an
/// internal panic in place of a diagnostic).
struct BuildLock {
    dir: PathBuf,
}

impl BuildLock {
    /// Blocks until the lock is held. Steals a stale lock (mtime older than 2
    /// minutes — far longer than any single FFI bridge `cargo build` takes)
    /// so a killed/timed-out `jet` process can't wedge every later build on
    /// this key.
    fn acquire(cache_root: &std::path::Path) -> Result<BuildLock, Vec<Diagnostic>> {
        let dir = cache_root.join(".build-lock");
        loop {
            match fs::create_dir(&dir) {
                Ok(()) => return Ok(BuildLock { dir }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Ok(meta) = fs::metadata(&dir) {
                        if let Ok(age) = meta.modified().and_then(|m| {
                            m.elapsed()
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                        }) {
                            if age > std::time::Duration::from_secs(120) {
                                let _ = fs::remove_dir(&dir);
                                continue;
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(tool_error(&format!(
                        "couldn't lock the FFI cache folder: {}",
                        e
                    )));
                }
            }
        }
    }
}

impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.dir);
    }
}

fn collect_crate_deps(entries: &[ExternEntry]) -> BTreeMap<String, String> {
    let mut deps = BTreeMap::new();
    for e in entries {
        if e.crate_spec == "std" {
            continue;
        }
        if let Some((name, ver)) = parse_crate_spec(&e.crate_spec) {
            deps.insert(name, ver);
        }
    }
    deps
}

/// `"std"` or `Some((name, version))` for `"name@version"`.
pub fn parse_crate_spec(spec: &str) -> Option<(String, String)> {
    if spec == "std" {
        return None;
    }
    let (name, ver) = spec.split_once('@')?;
    if name.is_empty() || ver.is_empty() {
        return None;
    }
    Some((name.to_string(), ver.to_string()))
}

fn cache_key_full(
    entries: &[ExternEntry],
    deps: &BTreeMap<String, String>,
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
) -> u64 {
    let mut h = DefaultHasher::new();
    // Only perturb the key when a ring module is actually needed, so programs
    // without those modules keep their historical cache key. The dep is already
    // in `deps`; the flag guards the (currently impossible) empty-deps case.
    if needs_regex {
        needs_regex.hash(&mut h);
    }
    if needs_archive {
        needs_archive.hash(&mut h);
    }
    if needs_db {
        needs_db.hash(&mut h);
    }
    if needs_http_client {
        needs_http_client.hash(&mut h);
    }
    if needs_crypto {
        needs_crypto.hash(&mut h);
    }
    if needs_compress {
        needs_compress.hash(&mut h);
    }
    if needs_plugin {
        needs_plugin.hash(&mut h);
    }
    for (k, v) in deps {
        k.hash(&mut h);
        v.hash(&mut h);
    }
    for e in entries {
        e.wrapper_name.hash(&mut h);
        e.rust_path.hash(&mut h);
        e.crate_spec.hash(&mut h);
        for (c, t) in &e.params {
            format!("{:?}", c).hash(&mut h);
            type_key(t).hash(&mut h);
        }
        if let Some(rt) = &e.return_type {
            type_key(rt).hash(&mut h);
        }
    }
    h.finish()
}

fn type_key(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".into(),
        Type::Float => "Float".into(),
        Type::IntN { signed, bits } => crate::AST::int_spelling(*signed, *bits),
        Type::Float32 => "F32".into(),
        Type::Bool => "Bool".into(),
        Type::String => "String".into(),
        Type::Char => "Char".into(),
        Type::List(inner) => format!("List<{}>", type_key(inner)),
        Type::Map { key, value } => format!("Map<{},{}>", type_key(key), type_key(value)),
        Type::Shared(inner) => format!("Shared<{}>", type_key(inner)),
        Type::Option(inner) => format!("{}?", type_key(inner)),
        Type::Result { ok, err } => format!("Result<{},{}>", type_key(ok), type_key(err)),
        Type::Fn { params, ret, .. } => {
            let ps = params.iter().map(type_key).collect::<Vec<_>>().join(",");
            let r = ret.as_ref().map(|t| type_key(t)).unwrap_or_default();
            format!("fn({ps})->{r}")
        }
        Type::Named(n) => n.clone(),
        Type::Apply { name, args } => format!(
            "{name}<{}>",
            args.iter().map(type_key).collect::<Vec<_>>().join(",")
        ),
        Type::TraitObject(t) => format!("dyn {}", t.join(" + ")),
        Type::Tuple(fields) => format!(
            "({})",
            fields
                .iter()
                .map(|(n, t)| format!("{n}:{}", type_key(t)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::FixedList { elem, len } => format!("List<{}#{}>", type_key(elem), len),
        Type::Tagged { marker, inner } => format!("#{marker}:{}", type_key(inner)),
    }
}

fn cache_dir() -> PathBuf {
    dirs_home().join(".cache").join("jet").join("ffi")
}

fn dirs_home() -> PathBuf {
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h);
    }
    if let Ok(h) = std::env::var("USERPROFILE") {
        return PathBuf::from(h);
    }
    PathBuf::from("/tmp")
}

fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

fn emit_cargo_toml(crate_name: &str, deps: &BTreeMap<String, String>) -> String {
    let mut s = format!(
        "[package]\nname = \"{crate_name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"rlib\"]\n\n"
    );
    if !deps.is_empty() {
        s.push_str("[dependencies]\n");
        for (name, ver) in deps {
            // Some crates need feature flags or other TOML table syntax — check
            // the allowlist first; fall back to the plain `name = "version"` form.
            if let Some((_, toml_val)) = FEATURED_DEPS.iter().find(|(n, _)| *n == name) {
                s.push_str(&format!("{name} = {toml_val}\n"));
            } else {
                s.push_str(&format!("{name} = \"{ver}\"\n"));
            }
        }
    }
    s
}

fn emit_wrapper_lib(
    entries: &[ExternEntry],
    needs_regex: bool,
    needs_archive: bool,
    needs_db: bool,
    needs_http_client: bool,
    needs_crypto: bool,
    needs_compress: bool,
    needs_plugin: bool,
) -> String {
    let mut out = String::from(
        "// Auto-generated FFI wrappers — do not edit.\n#![allow(warnings)]\n\nfn ffi_panic() -> ! {\n    eprintln!(\"panic: a foreign function panicked\");\n    std::process::exit(70);\n}\n\n",
    );
    if needs_regex {
        // D-REGEX1: the regex runtime is the only place the `regex` crate is touched.
        out.push_str(REGEX_RUNTIME);
        out.push('\n');
    }
    if needs_archive {
        // D-DEP-ARCHIVE1: the archive runtime is the only place `flate2`, `zip`,
        // and `tar` crates are touched.
        out.push_str(ARCHIVE_RUNTIME);
        out.push('\n');
    }
    if needs_db {
        // D-DEP-DB1: the database runtime is the only place `rusqlite` is touched.
        out.push_str(DB_RUNTIME);
        out.push('\n');
    }
    if needs_http_client {
        // D-NETDEP1=A: the HTTP client runtime is the only place `ureq` is touched.
        out.push_str(HTTP_CLIENT_RUNTIME);
        out.push('\n');
    }
    if needs_crypto {
        // D-DEP-CRYPTO1=A: the crypto runtime is the only place RustCrypto is touched.
        out.push_str(CRYPTO_RUNTIME);
        out.push('\n');
    }
    if needs_compress {
        // D-CODECS1: the compress runtime is the only place the standalone
        // `core.compress.gzip` / `core.compress.zstd` codec paths are touched.
        out.push_str(COMPRESS_RUNTIME);
        out.push('\n');
    }
    if needs_plugin {
        // D-DEP-WASM1=A: the plugin runtime is the only place `wasmtime` is touched.
        out.push_str(PLUGIN_RUNTIME);
        out.push('\n');
    }
    let mut names: HashSet<String> = HashSet::new();
    for e in entries {
        names.insert(e.jet_name.clone());
    }
    for e in entries {
        out.push_str(&emit_wrapper_fn(e, &names));
        out.push('\n');
    }
    out
}

fn emit_wrapper_fn(entry: &ExternEntry, user_types: &HashSet<String>) -> String {
    let params: Vec<String> = entry
        .params
        .iter()
        .enumerate()
        .map(|(i, (_, ty))| format!("p{i}: {}", rust_type(ty, user_types)))
        .collect();
    let ret = entry
        .return_type
        .as_ref()
        .map(|t| rust_type(t, user_types))
        .unwrap_or_else(|| "()".to_string());
    let call_args: Vec<String> = (0..entry.params.len()).map(|i| format!("p{i}")).collect();
    let rust_call = if ret == "()" {
        format!("{}({});", entry.rust_path, call_args.join(", "))
    } else {
        format!("{}({})", entry.rust_path, call_args.join(", "))
    };
    let body = if ret == "()" {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {{\n        {rust_call}\n    }})) {{\n        Ok(()) => (),\n        Err(_) => ffi_panic(),\n    }}"
        )
    } else {
        format!(
            "match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {rust_call})) {{\n        Ok(v) => v,\n        Err(_) => ffi_panic(),\n    }}"
        )
    };
    format!(
        "pub fn {}({}){} {{\n    {body}\n}}\n",
        entry.wrapper_name,
        params.join(", "),
        if ret == "()" {
            String::new()
        } else {
            format!(" -> {ret}")
        }
    )
}

fn rust_type(ty: &Type, user_types: &HashSet<String>) -> String {
    match ty {
        Type::Int => "i64".to_string(),
        Type::Float => "f64".to_string(),
        Type::IntN { signed, bits } => format!("{}{}", if *signed { 'i' } else { 'u' }, bits),
        Type::Float32 => "f32".to_string(),
        Type::Bool => "bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "char".to_string(),
        Type::List(inner) => format!("Vec<{}>", rust_type(inner, user_types)),
        Type::Map { key, value } => format!(
            "std::collections::BTreeMap<{}, {}>",
            rust_type(key, user_types),
            rust_type(value, user_types)
        ),
        // D-MEM1 S6: the main codegen path (Codegen/Context.rs) now renders
        // `Type::Shared` as `jet_std::JetShared<T>` (`Arc<RwLock<T>>`), not a
        // bare `Arc<T>` — this C-FFI bridge type table is untested for
        // `Shared<T>` crossing the boundary (no test exercises it; a
        // concurrency handle in a C-ABI signature is not a realistic shape),
        // left as the pre-S6 mapping rather than guessed at.
        Type::Shared(inner) => format!("std::sync::Arc<{}>", rust_type(inner, user_types)),
        Type::Option(inner) => format!("Option<{}>", rust_type(inner, user_types)),
        Type::Result { ok, err } => format!(
            "Result<{}, {}>",
            rust_type(ok, user_types),
            rust_type(err, user_types)
        ),
        Type::Fn { .. } => "Box<dyn std::any::Any>".to_string(),
        Type::Named(name) if name == "Error" => "String".to_string(),
        Type::Named(name) if user_types.contains(name) => format!("user_{name}"),
        Type::Named(name) => name.clone(),
        Type::Apply { name, args } if user_types.contains(name) => format!(
            "user_{name}<{}>",
            args.iter()
                .map(|a| rust_type(a, user_types))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Apply { .. } | Type::TraitObject(_) | Type::Tuple(_) => {
            "Box<dyn std::any::Any>".to_string()
        }
        // D-FIXARR1: [T#N] lowers to a real Rust array [T; N] in FFI too.
        Type::FixedList { elem, len } => format!("[{}; {}]", rust_type(elem, user_types), len),
        Type::Tagged { inner, .. } => rust_type(inner, user_types),
    }
}

fn looks_like_signature_mismatch(stderr: &str) -> bool {
    stderr.contains("E0308")
        || stderr.contains("E0277")
        || stderr.contains("E0061")
        || stderr.contains("E0425")
        || stderr.contains("cannot find")
        || stderr.contains("mismatched types")
        || stderr.contains("arguments to this function")
}

fn indent_block(s: &str) -> String {
    s.lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keep cargo output stable for ui snapshots — drop fetch noise and parallel-build chatter.
fn stable_cargo_detail(stderr: &str) -> String {
    let kept: Vec<String> = stderr
        .lines()
        .filter(|line| {
            let t = line.trim();
            !t.is_empty()
                && !t.starts_with("Updating crates.io")
                && !t.starts_with("Locking ")
                && !t.contains("waiting for other jobs")
                && !(t.starts_with("Compiling ") && !t.contains("jet_ffi_"))
        })
        .map(|line| normalize_ffi_cache_path(line))
        .collect();
    indent_block(&kept.join("\n"))
}

/// Keep ui snapshots stable across machines (`/home/…/.cache/jet/ffi/…` → `~/.cache/jet/ffi/…`).
fn normalize_ffi_cache_path(line: &str) -> String {
    let marker = ".cache/jet/ffi/";
    let Some(idx) = line.find(marker) else {
        return line.to_string();
    };
    let path_start = line[..idx]
        .rfind('/')
        .and_then(|slash| {
            let pre = &line[..slash];
            pre.rfind(|c| c == '(' || c == ' ')
                .map(|j| j + 1)
                .or(Some(0))
        })
        .unwrap_or(idx);
    let rest = &line[idx + marker.len()..];
    let hash_len = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
    if hash_len != 16 {
        return line.to_string();
    }
    let hash = &rest[..hash_len];
    let suffix = &rest[hash_len..];
    format!("{}~/.cache/jet/ffi/{}{}", &line[..path_start], hash, suffix)
}

fn tool_error(msg: &str) -> Vec<Diagnostic> {
    vec![Diagnostic::error(
        "E0704",
        msg.to_string(),
        "building the foreign crate bridge failed".to_string(),
        "check disk permissions and try again".to_string(),
        None,
    )]
}

// c43: U32/IntN FFI type-mapping tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::AST::Type;
    use std::collections::HashSet;

    #[test]
    fn intn_u32_maps_to_rust_u32() {
        // c43: `Type::IntN { signed: false, bits: 32 }` (Jet `U32`) must lower
        // to Rust `u32` in the FFI bridge — not `i64` or any other type.
        let empty = HashSet::new();
        assert_eq!(
            rust_type(
                &Type::IntN {
                    signed: false,
                    bits: 32
                },
                &empty
            ),
            "u32",
            "U32 must map to Rust u32 in FFI"
        );
    }

    #[test]
    fn intn_i32_maps_to_rust_i32() {
        // c43: signed 32-bit maps to Rust i32 (S44 signed-integer subset).
        let empty = HashSet::new();
        assert_eq!(
            rust_type(
                &Type::IntN {
                    signed: true,
                    bits: 32
                },
                &empty
            ),
            "i32",
            "I32 must map to Rust i32 in FFI"
        );
    }

    #[test]
    fn intn_width_family_round_trip() {
        // c43: verify all supported fixed-width integer types map correctly.
        let empty = HashSet::new();
        for &(signed, bits, expected) in &[
            (false, 8_u8, "u8"),
            (true, 8, "i8"),
            (false, 16, "u16"),
            (true, 16, "i16"),
            (false, 32, "u32"),
            (true, 32, "i32"),
            (false, 64, "u64"),
            (true, 64, "i64"),
        ] {
            assert_eq!(
                rust_type(&Type::IntN { signed, bits }, &empty),
                expected,
                "IntN {{ signed:{}, bits:{} }} should map to {}",
                signed,
                bits,
                expected
            );
        }
    }

    #[test]
    fn int_maps_to_i64_and_float_maps_to_f64() {
        // Regression guard: base types haven't drifted.
        let empty = HashSet::new();
        assert_eq!(rust_type(&Type::Int, &empty), "i64");
        assert_eq!(rust_type(&Type::Float, &empty), "f64");
        assert_eq!(rust_type(&Type::Float32, &empty), "f32");
    }
}
