//! `jet:` self-toolchain pin — pinning **which `jet` compiler** a project runs
//! (D-JPK-TOOLCHAIN1=A, card #179, U30).
//!
//! DISTINCT from [`crate::Jetpack::Toolchain`] (D-JPK-BUILDTOOL1): that module
//! is the Rust/native **build** toolchain that compiles a user's `extern rust`
//! bridge crates. THIS module pins the Jet compiler itself. A `pkg.jet` gains a
//! `jet:` channel ref (`jet: 0.4`); a running `jet` whose version differs from
//! the locked pin **realizes the pinned compiler as a prebuilt hangar object**
//! (D-JPK-CACHE1 substitution path) and **re-execs into it** (D-JPK-DISPATCH1
//! process seam). It never source-builds the compiler — a platform cache miss
//! is an honest `E1251`, never a from-scratch compile and never a silent use of
//! the wrong `jet`.
//!
//! Slices (see `docs/plans/epoch-4/toolchain-as-dependency.md`):
//!   T1  frozen-forward identity pre-parse ([`identity_preparse`]) + `E1249`.
//!   T2  channel resolution + `[[toolchain]]` lock record + `E1250`.
//!   T3  version-mismatch detect → realize → re-exec ([`decide`]) + `E1251`.
//!   T4  verbs: [`report_pin`], [`move_pin`], [`write_init`].

use std::path::{Path, PathBuf};

use crate::Diagnostics::Diagnostic;
use crate::Lock::{self, LockEnvelope, LockedToolchain};
use crate::Syntax;

// ──────────────────────────────────────────────
// T1 — frozen-forward identity block
// ──────────────────────────────────────────────

/// The contract-frozen identity of a `pkg.jet`: the `payload:` block's `name`
/// and `version`, plus the `jet:` pin (a channel ref). Extracted by a reader
/// whose grammar never narrows, so version dispatch can never be wedged by
/// later manifest evolution (the Go `go.mod` contract).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdentityBlock {
    pub payload_name: String,
    pub payload_version: String,
    /// The raw `jet:` value (a channel ref). `None` = unpinned — the running
    /// `jet` is used with no fetch (rung-0/1 stays frictionless).
    pub jet: Option<String>,
}

/// Frozen-forward identity pre-parse (T1). Reads only `payload:`'s `name`,
/// `version`, and `jet` fields, tolerating any other top-level key, any unknown
/// nested block, and any surrounding syntax. Independent of the full manifest
/// parser on purpose: this grammar is CONTRACT-FROZEN (documented in
/// `docs/spec/spec.md`) and must never be narrowed.
pub fn identity_preparse(text: &str) -> IdentityBlock {
    let text = strip_line_comments(text);
    let mut id = IdentityBlock::default();
    if let Some(body) = top_block_body(&text, "payload") {
        id.payload_name = simple_field(body, "name").unwrap_or_default();
        id.payload_version = simple_field(body, "version").unwrap_or_default();
        id.jet = simple_field(body, "jet");
    }
    id
}

/// Strip `//` line comments, leaving string literals intact. Local + minimal so
/// the frozen reader carries no dependency on the evolving manifest helpers.
fn strip_line_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut in_str = false;
        let mut cut = line.len();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => in_str = !in_str,
                b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    cut = i;
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// Body of a top-level `key: { … }` block (brace-matched, strings skipped), or
/// `None`. Only depth-0 occurrences count, so an unknown nested block never
/// hijacks the read.
fn top_block_body<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let bytes = text.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ if depth == 0 && is_ident_start(c) => {
                let start = i;
                while i < bytes.len() && is_ident_char(bytes[i]) {
                    i += 1;
                }
                let word = &text[start..i];
                // skip ws, expect ':', ws, '{'
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if word == key && j < bytes.len() && bytes[j] == b':' {
                    j += 1;
                    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'{' {
                        // brace-match from j
                        let body_start = j + 1;
                        let mut d = 0i32;
                        let mut k = j;
                        let mut s = false;
                        while k < bytes.len() {
                            let ck = bytes[k];
                            if s {
                                if ck == b'"' {
                                    s = false;
                                }
                            } else if ck == b'"' {
                                s = true;
                            } else if ck == b'{' {
                                d += 1;
                            } else if ck == b'}' {
                                d -= 1;
                                if d == 0 {
                                    return Some(&text[body_start..k]);
                                }
                            }
                            k += 1;
                        }
                        return None;
                    }
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Value of a `key: value` entry at depth 0 within `body` (nested blocks and
/// strings skipped), unquoted + trimmed. `None` if absent.
fn simple_field(body: &str, key: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            _ if depth == 0 && is_ident_start(c) => {
                let start = i;
                while i < bytes.len() && is_ident_char(bytes[i]) {
                    i += 1;
                }
                let word = &body[start..i];
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if word == key && j < bytes.len() && bytes[j] == b':' {
                    j += 1;
                    // capture to end of a top-level value: stop at ',' or '\n'
                    let vstart = j;
                    let mut s = false;
                    while j < bytes.len() {
                        let cj = bytes[j];
                        if s {
                            if cj == b'"' {
                                s = false;
                            }
                        } else if cj == b'"' {
                            s = true;
                        } else if cj == b',' || cj == b'\n' {
                            break;
                        }
                        j += 1;
                    }
                    return Some(unquote(&body[vstart..j]));
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_suffix(',').unwrap_or(t).trim();
    t.trim_matches('"').trim().to_string()
}

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}
fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

// ──────────────────────────────────────────────
// T1 — channel classification (E1249)
// ──────────────────────────────────────────────

/// Classify a `jet:` pin value as a channel ref (D-JPK-CHANNEL1). Accepted
/// forms: a `MAJOR.MINOR` / `MAJOR.MINOR.PATCH` version (`0.4`, `0.4.2`) or a
/// bare named channel (`main`, `stable`, `nightly`). Anything else — a range
/// operator (`>=1.0.0`), an empty value, a bare major (`1`) — is `E1249`.
pub fn classify_channel(raw: &str) -> Result<String, Diagnostic> {
    let c = raw.trim().trim_matches('"').trim();
    if is_version_channel(c) || is_named_channel(c) {
        Ok(c.to_string())
    } else {
        Err(e1249(raw))
    }
}

fn is_version_channel(c: &str) -> bool {
    let parts: Vec<&str> = c.split('.').collect();
    (parts.len() == 2 || parts.len() == 3)
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A `jet:` value is a legacy range/compat constraint (owned by E1208), not a
/// channel pin, when it leads with a comparison/wildcard operator. Version
/// dispatch skips these; only channel-form values drive the pin.
fn is_range_pin(value: &str) -> bool {
    matches!(
        value.trim().trim_matches('"').trim().bytes().next(),
        Some(b'>') | Some(b'<') | Some(b'=') | Some(b'^') | Some(b'~') | Some(b'*')
    )
}

fn is_named_channel(c: &str) -> bool {
    let mut chars = c.bytes();
    match chars.next() {
        Some(b) if b.is_ascii_lowercase() => {}
        _ => return false,
    }
    c.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The channel a running toolchain version belongs to: its `MAJOR.MINOR`
/// (`0.4.2` → `0.4`). `jet init` pins this by default.
pub fn channel_of(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[0], parts[1])
    } else {
        version.to_string()
    }
}

// ──────────────────────────────────────────────
// T2 — channel resolution + lock record
// ──────────────────────────────────────────────

/// Resolve a channel ref to an exact toolchain version. Channels resolve ONLY
/// on `jet update jet` / first realize (mirroring D-JPK-CHANNEL1); every other
/// run reads the locked exact version. A `MAJOR.MINOR` channel resolves to the
/// series head `MAJOR.MINOR.0`; a `MAJOR.MINOR.PATCH` channel to itself. A
/// named channel (`main`) has no offline-derivable exact version — resolving it
/// requires a realizable object, so it is `E1251` here.
pub fn resolve_channel(channel: &str) -> Result<String, Diagnostic> {
    let parts: Vec<&str> = channel.split('.').collect();
    if is_version_channel(channel) {
        return Ok(match parts.len() {
            2 => format!("{}.{}.0", parts[0], parts[1]),
            _ => channel.to_string(),
        });
    }
    Err(e1251(channel, channel, &current_platform()))
}

/// The locked `jet` self-toolchain pin for this project, if the lock records
/// one. Selected by its `jet-` object-id prefix so a co-located bridge
/// build-toolchain entry (`toolchain-<version>`, D-JPK-BUILDTOOL1) is never
/// mistaken for the compiler pin.
pub fn locked_toolchain(root: &Path) -> Option<LockedToolchain> {
    Lock::load(root)?
        .toolchains
        .into_iter()
        .find(|t| t.id.starts_with("jet-"))
}

/// Object id for a resolved toolchain version (D-PM1): `jet-<version>-<fp>`,
/// where `fp` is a short platform-qualified fingerprint.
pub fn object_id(version: &str) -> String {
    let fp = crate::SHA256::sha256_hex(format!("{version}:{}", current_platform()).as_bytes());
    format!("jet-{version}-{}", &fp[..8])
}

/// Build the `[[toolchain]]` lock record for a resolved pin.
pub fn toolchain_record(channel: &str, version: &str) -> LockedToolchain {
    LockedToolchain {
        id: object_id(version),
        channel: channel.to_string(),
        version: version.to_string(),
        envelope: LockEnvelope {
            output_hash: String::new(),
            platform: current_platform(),
            signature: String::new(),
            provenance: format!("jet-{version} via toolchain"),
        },
    }
}

/// The platform triple string used in the toolchain envelope / object id.
pub fn current_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        other => other,
    };
    format!("{}-{}", std::env::consts::ARCH, os)
}

// ──────────────────────────────────────────────
// T3 — decide: realize + re-exec the pinned toolchain
// ──────────────────────────────────────────────

/// The dispatch decision for a manifest-driven verb in a project directory.
#[derive(Debug)]
pub enum PinDecision {
    /// No pin, the running `jet` already satisfies it, or this process is
    /// already the exec'd pinned child — run natively.
    RunNative,
    /// Re-exec into the realized pinned toolchain `binary`, tagging the child
    /// with the exec marker = `version` so it runs natively (no loop).
    ReExec {
        binary: PathBuf,
        channel: String,
        version: String,
    },
    /// A blocking diagnostic (bad pin / channel-in-CI / platform miss).
    Report(Diagnostic),
}

/// Decide whether the running `jet` must hand off to a pinned toolchain (T3).
/// `running_version` is this binary's version; `offline` is `--offline`/CI.
///
/// Native-first: a running `jet` in the pinned channel satisfies the pin and
/// runs the build directly — no fetch, no lock write, no exec. The channel
/// resolves to an exact version and the lock is written ONLY when a genuine
/// mismatch forces first realization (or via `jet update jet`), so a plain
/// `jet run` on a pinned project never mutates the source tree.
pub fn decide(root: &Path, running_version: &str, offline: bool) -> PinDecision {
    // Re-exec guard: if this process was itself exec'd by a pin, never
    // re-dispatch — break the loop and run the pinned toolchain natively.
    if std::env::var_os(Syntax::TOOLCHAIN_EXEC_MARKER_ENV).is_some() {
        return PinDecision::RunNative;
    }

    let Some(manifest) = read_manifest(root) else {
        return PinDecision::RunNative;
    };
    let id = identity_preparse(&manifest);
    let Some(raw_pin) = id.jet else {
        return PinDecision::RunNative; // unpinned
    };
    // A range/operator form (`>=1.0.0`, `^1.2`, `*`) is the legacy toolchain
    // compatibility constraint (E1208 in the normal compile path), NOT a
    // channel pin — version dispatch leaves it alone.
    if is_range_pin(&raw_pin) {
        return PinDecision::RunNative;
    }
    let channel = match classify_channel(&raw_pin) {
        Ok(c) => c,
        Err(d) => return PinDecision::Report(d),
    };
    let running_channel = channel_of(running_version);

    // Locked exact version wins when present; otherwise the pin is unresolved.
    let locked = locked_toolchain(root);
    let target = locked.as_ref().map(|tc| tc.version.clone());

    // Native satisfaction: the running jet already is the pinned toolchain, or
    // is a patch of the same channel — the channel model accepts an in-channel
    // running jet without realizing a specific patch.
    let satisfied = match &target {
        Some(v) => running_version == v || running_channel == channel_of(v),
        None => is_version_channel(&channel) && running_channel == channel,
    };
    if satisfied {
        return PinDecision::RunNative;
    }

    // A genuine mismatch: resolve the channel to an exact version (first
    // realization) and hand off to the pinned prebuilt.
    let target_version = match target {
        Some(v) => v,
        None => {
            if offline {
                return PinDecision::Report(e1250(&channel));
            }
            match resolve_channel(&channel) {
                Ok(v) => v,
                Err(d) => return PinDecision::Report(d),
            }
        }
    };
    // Re-check native against the just-resolved version (covers a lock-absent
    // in-channel exact pin).
    if running_version == target_version || running_channel == channel_of(&target_version) {
        return PinDecision::RunNative;
    }

    match realize_jet_binary(&channel, &target_version) {
        Ok(binary) => {
            // Record the pin now that we've resolved for realization, so
            // re-runs read the lock instead of re-resolving the channel.
            if locked.is_none() {
                Lock::record_toolchain(root, toolchain_record(&channel, &target_version));
            }
            PinDecision::ReExec {
                binary,
                channel,
                version: target_version,
            }
        }
        Err(d) => PinDecision::Report(d),
    }
}

/// The honest one-liner printed before handing off to a pinned toolchain.
pub fn handoff_line(channel: &str, pinned_version: &str, installed_version: &str) -> String {
    format!(
        "jet: project pins toolchain {channel} ({pinned_version}); installed {installed_version} — realizing and exec"
    )
}

fn read_manifest(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(Syntax::PAYLOAD_FILE)).ok()
}

/// Realize the pinned toolchain object and return the path to its `jet`
/// binary (T3). D-JPK-CACHE1 substitution boundary: the object is materialized
/// into the hangar (a fixture stands in via `JET_TOOLCHAIN_FIXTURE` in tests;
/// #179's downloader points it at the fetched prebuilt in production). Never
/// source-builds the compiler; a platform cache miss is `E1251`.
pub fn realize_jet_binary(channel: &str, version: &str) -> Result<PathBuf, Diagnostic> {
    let dir = std::env::var_os(Syntax::TOOLCHAIN_OBJECT_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| e1251(channel, version, &current_platform()))?;
    let bin = dir.join(Syntax::BINARY_NAME);
    if bin.is_file() && is_executable(&bin) {
        Ok(bin)
    } else {
        Err(e1251(channel, version, &current_platform()))
    }
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

// ──────────────────────────────────────────────
// T4 — verbs
// ──────────────────────────────────────────────

/// `jet toolchain` — read-only pin/version/status report (T4).
pub fn report_pin(root: &Path) -> String {
    let manifest = read_manifest(root);
    let id = manifest
        .as_deref()
        .map(identity_preparse)
        .unwrap_or_default();
    let mut out = String::new();
    match &id.jet {
        Some(pin) => out.push_str(&format!("pin:      jet {pin}\n")),
        None => {
            out.push_str("pin:      (none — unpinned, uses the running jet)\n");
            return out;
        }
    }
    match locked_toolchain(root) {
        Some(tc) => {
            out.push_str(&format!("locked:   {}\n", tc.version));
            out.push_str(&format!("object:   {}\n", tc.id));
            let realized = realize_jet_binary(&tc.channel, &tc.version).is_ok();
            out.push_str(&format!(
                "state:    {}\n",
                if realized { "realized" } else { "not realized" }
            ));
        }
        None => {
            out.push_str("locked:   (unresolved — run `jet update jet`)\n");
            out.push_str("state:    not realized\n");
        }
    }
    out
}

/// `jet update jet [<channel>]` — move the pin deliberately (T4). Re-resolves
/// the channel, updates the lock's `[[toolchain]]` record, and returns a
/// summary line. This is the ONLY place the pin moves.
pub fn move_pin(
    root: &Path,
    channel_arg: Option<&str>,
    running_version: &str,
) -> Result<String, Diagnostic> {
    let id = read_manifest(root).as_deref().map(identity_preparse);
    let channel = match channel_arg {
        Some(c) => classify_channel(c)?,
        None => match id.and_then(|i| i.jet) {
            Some(pin) => classify_channel(&pin)?,
            None => channel_of(running_version),
        },
    };
    let version = resolve_channel(&channel)?;
    Lock::record_toolchain(root, toolchain_record(&channel, &version));
    Ok(format!(
        "jet: pinned toolchain {channel} → {version} (locked in {})",
        Syntax::UNIFIED_LOCK_FILE
    ))
}

/// `jet init` — write a `pkg.jet` pinning the running toolchain's channel by
/// default, so a lifted project is reproducible from birth (T4, U11 lift).
/// Refuses to clobber an existing manifest.
pub fn write_init(dir: &Path, name: &str, running_version: &str) -> Result<String, Diagnostic> {
    let path = dir.join(Syntax::PAYLOAD_FILE);
    if path.exists() {
        return Err(Diagnostic::error(
            "E1252",
            format!("a `{}` already exists here", Syntax::PAYLOAD_FILE),
            "`jet init` writes a fresh package manifest; overwriting one would discard its \
             dependencies, pins, and identity."
                .to_string(),
            "edit the existing manifest, or run `jet init` in an empty directory.".to_string(),
            None,
        ));
    }
    let channel = channel_of(running_version);
    let body = init_manifest(name, &channel);
    std::fs::write(&path, &body).map_err(|e| {
        Diagnostic::error(
            "E1252",
            format!("couldn't write `{}`", Syntax::PAYLOAD_FILE),
            format!("the manifest could not be created: {e}"),
            "check directory permissions and try again.".to_string(),
            None,
        )
    })?;
    Ok(format!(
        "created {} (pinned jet {channel})",
        Syntax::PAYLOAD_FILE
    ))
}

/// The `pkg.jet` body `jet init` writes: a payload identity with a `jet:`
/// channel pin (the running toolchain's channel).
pub fn init_manifest(name: &str, channel: &str) -> String {
    format!(
        "payload: {{\n    name:    \"{name}\",\n    version: \"0.1.0\",\n    jet:     {channel},\n}}\n\ndeps: {{\n}}\n"
    )
}

// ──────────────────────────────────────────────
// Diagnostics (E1249 / E1250 / E1251) — docs/spec/diagnostics.md
// ──────────────────────────────────────────────

/// E1249 — a `jet:` pin value is not a valid version/channel ref.
pub fn e1249(value: &str) -> Diagnostic {
    Diagnostic::error(
        "E1249",
        format!("`{}` is not a valid toolchain pin", value.trim()),
        "the `jet:` field pins which Jet toolchain builds this project — its value is a \
         channel ref, not a version range."
            .to_string(),
        "write a channel: `jet: 0.4` (track the 0.4 series), `jet: 0.4.2` (exact), or a named \
         channel like `jet: main`."
            .to_string(),
        None,
    )
}

/// E1250 — an unlocked channel pin under `--offline`/CI with no lock entry.
pub fn e1250(channel: &str) -> Diagnostic {
    Diagnostic::error(
        "E1250",
        format!("toolchain channel `{channel}` is pinned but not locked"),
        format!(
            "an `--offline`/CI build won't resolve a channel — it needs the exact toolchain \
             version recorded in `{}`, and none is present.",
            Syntax::UNIFIED_LOCK_FILE
        ),
        format!(
            "run `jet update jet` to resolve `{channel}` to an exact version and commit `{}`.",
            Syntax::UNIFIED_LOCK_FILE
        ),
        None,
    )
}

/// E1251 — the pinned toolchain isn't available for this platform (cache miss).
/// Never a source build of the compiler, never a silent wrong `jet`.
pub fn e1251(channel: &str, version: &str, platform: &str) -> Diagnostic {
    Diagnostic::error(
        "E1251",
        format!("toolchain {channel} ({version}) isn't available for {platform}"),
        "this project pins a Jet toolchain, but no prebuilt object for it was found for this \
         platform. Jet realizes the pinned compiler as a prebuilt — it never builds the \
         compiler from source and never silently falls back to a different `jet`."
            .to_string(),
        "move the pin with `jet update jet <channel>` to a toolchain your platform has, or \
         install the pinned toolchain from the release page."
            .to_string(),
        None,
    )
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Serialises the tests that mutate the process-global toolchain object env
    /// var, so parallel runners never see each other's setting.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn scratch(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "jetpin-{tag}-{}-{:?}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(p.join(".jet")).unwrap();
        p
    }

    // ── T1 ──

    #[test]
    fn identity_block_parses_across_unknown_fields() {
        // Unknown top-level keys, an unknown nested block, and unknown fields
        // inside payload must not stop the identity read.
        let src = r#"
// leading comment
future_top_key: { nested: { deep: [1, 2, 3] }, more: "syntax we don't know yet" }
payload: {
    name:    "wordstats",
    version: "0.3.1",
    jet:     0.4,
    someday: { a: 1, b: { c: 2 } },
    license: "MIT",
}
tomorrow: [ a, b, c ]
deps: { textkit: "1.2.0" }
"#;
        let id = identity_preparse(src);
        assert_eq!(id.payload_name, "wordstats");
        assert_eq!(id.payload_version, "0.3.1");
        assert_eq!(id.jet.as_deref(), Some("0.4"));
    }

    #[test]
    fn identity_block_unpinned_when_no_jet_field() {
        let src = "payload: { name: \"x\", version: \"1.0.0\" }\n";
        let id = identity_preparse(src);
        assert_eq!(id.payload_name, "x");
        assert_eq!(id.jet, None);
    }

    #[test]
    fn identity_block_accepts_quoted_pin() {
        let src = "payload: { name: \"x\", version: \"1\", jet: \"main\" }\n";
        assert_eq!(identity_preparse(src).jet.as_deref(), Some("main"));
    }

    #[test]
    fn classify_channel_accepts_versions_and_named() {
        assert_eq!(classify_channel("0.4").unwrap(), "0.4");
        assert_eq!(classify_channel("0.4.2").unwrap(), "0.4.2");
        assert_eq!(classify_channel("main").unwrap(), "main");
        assert_eq!(classify_channel("\"0.4\"").unwrap(), "0.4");
    }

    #[test]
    fn classify_channel_rejects_ranges_reports_e1249() {
        for bad in [">=1.0.0", "^1.2", "1", "", "0.", "1.x", "Main"] {
            let d = classify_channel(bad).unwrap_err();
            assert_eq!(d.code, "E1249", "should reject {bad:?}");
        }
    }

    // ── T2 ──

    #[test]
    fn toolchain_pin_locks_exact() {
        // Channel `0.4` resolves to an exact `0.4.0`, recorded in the lock;
        // re-reads take the locked version, not the channel.
        let root = scratch("locks-exact");
        let resolved = resolve_channel("0.4").unwrap();
        assert_eq!(resolved, "0.4.0");
        Lock::record_toolchain(&root, toolchain_record("0.4", &resolved));

        let tc = locked_toolchain(&root).expect("locked");
        assert_eq!(tc.channel, "0.4");
        assert_eq!(tc.version, "0.4.0");
        assert!(tc.id.starts_with("jet-0.4.0-"));
        // The lock is authoritative on re-read: a second load sees the exact
        // version without re-resolving the channel.
        assert_eq!(locked_toolchain(&root).unwrap().version, "0.4.0");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn offline_unlocked_channel_reports_e1250() {
        let root = scratch("offline-e1250");
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\", jet: 0.9 }\n",
        )
        .unwrap();
        match decide(&root, "0.4.0", true) {
            PinDecision::Report(d) => assert_eq!(d.code, "E1250"),
            other => panic!("expected E1250, got {other:?}"),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    // ── T3 ──

    #[test]
    fn mismatched_jet_realizes_and_execs_pin() {
        // A fixture toolchain object (a `jet` script printing its version)
        // stands in for a downloaded prebuilt. A running `jet` with a different
        // self-version realizes it and re-execs; the child reports the pinned
        // version and honours the exec marker + env passthrough.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let root = scratch("reexec");
        let obj = scratch("obj");
        // fixture prebuilt jet: prints its version + confirms marker/env
        let script = format!(
            "#!/bin/sh\necho pinned-jet-0.4.0\necho marker=${}\necho passthru=$JETPIN_TEST_ENV\n",
            Syntax::TOOLCHAIN_EXEC_MARKER_ENV
        );
        let jetbin = obj.join(Syntax::BINARY_NAME);
        std::fs::write(&jetbin, script).unwrap();
        std::fs::write(obj.join("version"), "0.4.0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&jetbin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\", jet: 0.4 }\n",
        )
        .unwrap();
        // lock already resolved to 0.4.0
        Lock::record_toolchain(&root, toolchain_record("0.4", "0.4.0"));

        std::env::set_var(Syntax::TOOLCHAIN_OBJECT_ENV, &obj);
        std::env::remove_var(Syntax::TOOLCHAIN_EXEC_MARKER_ENV);

        let decision = decide(&root, "9.9.9", false);
        let (binary, channel, version) = match decision {
            PinDecision::ReExec {
                binary,
                channel,
                version,
            } => (binary, channel, version),
            other => panic!("expected ReExec, got {other:?}"),
        };
        assert_eq!(channel, "0.4");
        assert_eq!(version, "0.4.0");
        assert_eq!(binary, jetbin);

        // Perform the exec the main-loop shim would: set the marker + inherit env.
        #[cfg(unix)]
        {
            let out = std::process::Command::new(&binary)
                .env(Syntax::TOOLCHAIN_EXEC_MARKER_ENV, &version)
                .env("JETPIN_TEST_ENV", "carried")
                .output()
                .unwrap();
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains("pinned-jet-0.4.0"), "child version: {text}");
            assert!(text.contains("marker=0.4.0"), "exec marker: {text}");
            assert!(text.contains("passthru=carried"), "env passthrough: {text}");
        }

        // Re-exec guard: with the marker set, the pinned child runs natively.
        std::env::set_var(Syntax::TOOLCHAIN_EXEC_MARKER_ENV, "0.4.0");
        assert!(matches!(
            decide(&root, "0.4.0", false),
            PinDecision::RunNative
        ));
        std::env::remove_var(Syntax::TOOLCHAIN_EXEC_MARKER_ENV);
        std::env::remove_var(Syntax::TOOLCHAIN_OBJECT_ENV);
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&obj).ok();
    }

    #[test]
    fn matched_version_runs_native() {
        let root = scratch("native");
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\", jet: 0.4 }\n",
        )
        .unwrap();
        Lock::record_toolchain(&root, toolchain_record("0.4", "0.4.0"));
        std::env::remove_var(Syntax::TOOLCHAIN_EXEC_MARKER_ENV);
        assert!(matches!(
            decide(&root, "0.4.0", false),
            PinDecision::RunNative
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn platform_cache_miss_reports_e1251() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var(Syntax::TOOLCHAIN_OBJECT_ENV);
        let d = realize_jet_binary("0.4", "0.4.0").unwrap_err();
        assert_eq!(d.code, "E1251");
        assert!(d.what.contains("0.4.0"));
    }

    // ── T4 ──

    #[test]
    fn jet_toolchain_reports_pin() {
        let root = scratch("report");
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\", jet: 0.4 }\n",
        )
        .unwrap();
        Lock::record_toolchain(&root, toolchain_record("0.4", "0.4.0"));
        let report = report_pin(&root);
        assert!(report.contains("pin:      jet 0.4"), "{report}");
        assert!(report.contains("locked:   0.4.0"), "{report}");
        assert!(report.contains("object:   jet-0.4.0-"), "{report}");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn jet_toolchain_reports_unpinned() {
        let root = scratch("report-unpinned");
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\" }\n",
        )
        .unwrap();
        assert!(report_pin(&root).contains("unpinned"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn jet_update_jet_moves_lock() {
        let root = scratch("update");
        std::fs::write(
            root.join(Syntax::PAYLOAD_FILE),
            "payload: { name: \"x\", version: \"1\", jet: 0.4 }\n",
        )
        .unwrap();
        // move to an explicit new channel
        let msg = move_pin(&root, Some("0.7"), "9.9.9").unwrap();
        assert!(msg.contains("0.7"), "{msg}");
        assert_eq!(locked_toolchain(&root).unwrap().version, "0.7.0");
        assert_eq!(locked_toolchain(&root).unwrap().channel, "0.7");
        // with no arg, uses the manifest pin
        move_pin(&root, None, "9.9.9").unwrap();
        assert_eq!(locked_toolchain(&root).unwrap().channel, "0.4");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn jet_init_pins_running_channel() {
        let root = scratch("init");
        // running toolchain 0.6.1 → pins channel 0.6
        let msg = write_init(&root, "myapp", "0.6.1").unwrap();
        assert!(msg.contains("0.6"), "{msg}");
        let text = std::fs::read_to_string(root.join(Syntax::PAYLOAD_FILE)).unwrap();
        let id = identity_preparse(&text);
        assert_eq!(id.payload_name, "myapp");
        assert_eq!(id.jet.as_deref(), Some("0.6"));
        // a second init refuses to clobber
        let err = write_init(&root, "other", "0.6.1").unwrap_err();
        assert_eq!(err.code, "E1252");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn channel_of_takes_major_minor() {
        assert_eq!(channel_of("0.4.2"), "0.4");
        assert_eq!(channel_of("1.10.0"), "1.10");
    }
}
