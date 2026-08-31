//! `.jetlib` loadable-library artifact stamp and load-time trust boundary
//! (card #1421 plus the direct-loader completion in #1345).
//!
//! Two checks a `.jetlib` artifact must pass **before it is mapped**:
//!
//! - **c2 — compiler identity** (D-LIB-REUSE1=B, "pinned Jet dynamic
//!   libraries"): both sides of a load must share the exact compiler that
//!   built them. Jet makes no cross-version binary layout promise, so a
//!   mismatch is a checked refusal, never a crash. `E1338`.
//! - **c3 — declared-effect grant** (D-LIB-DYNTRUST1=A, "declared effects,
//!   granted at load"): a loadable library declares its effects like any
//!   Jet package; the host states what it grants at the load site. A
//!   library asking for more than the grant is refused. `E1339`.
//!
//! The ratified adversarial review answers "what stops a forged artifact
//! from claiming a narrow effect set": identity is checked first, so a
//! forged or foreign artifact fails the pin before anything about its
//! claimed effects is trusted (`check_before_map`).
//!
//! The package manifest owns the `Library.{ loadable: true }` field used to
//! request this artifact. The artifact payload is the native shared object
//! produced by the library build. The load site parses and checks the complete
//! header, including the target and the exact checked export table, before it
//! writes or maps the payload. The compiler pin is therefore a real pre-map
//! gate rather than a diagnostic-only helper.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Manifest::COMPILER_VERSION;
use crate::Sema::{effect_covers, EffectSet};

/// Fixed header magic for a `.jetlib` artifact stamp.
const MAGIC: &[u8] = b"jet-jetlib-v3\0";

/// The ABI table version is independent of the compiler version. A compiler
/// can keep its package identity while changing the native scalar contract.
pub const ABI_VERSION: u32 = 2;

/// Complete native boundary identity. This is deliberately a data value, not
/// a second policy table: the driver records it and the loader gates it.
pub const ABI_IDENTITY: &str =
    "jet.library.abi.v2;call=extern-c;scalar=homogeneous;access=read-write-move;text=jet-text-v1;ptr-len=checked-utf8";

const MAX_HEADER_ITEMS: usize = 4096;
const MAX_HEADER_FIELD_BYTES: usize = 1024 * 1024;
const MAX_EXPORT_PARAMS: usize = 4096;

/// The one scalar vocabulary admitted by D-EMBED1 for native Library rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JetLibScalar {
    Int,
    Float,
    Bool,
    Text,
}

impl JetLibScalar {
    fn tag(self) -> u8 {
        match self {
            Self::Int => 0,
            Self::Float => 1,
            Self::Bool => 2,
            Self::Text => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, String> {
        match tag {
            0 => Ok(Self::Int),
            1 => Ok(Self::Float),
            2 => Ok(Self::Bool),
            3 => Ok(Self::Text),
            _ => Err(format!("unknown .jetlib scalar tag {tag}")),
        }
    }
}

/// Access convention carried with each native parameter. The C representation
/// of scalar values stays homogeneous; this row preserves the Jet ownership
/// contract for the callee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JetLibAccess {
    Read,
    Write,
    Move,
}

impl JetLibAccess {
    fn tag(self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Write => 1,
            Self::Move => 2,
        }
    }

    fn from_tag(tag: u8) -> Result<Self, String> {
        match tag {
            0 => Ok(Self::Read),
            1 => Ok(Self::Write),
            2 => Ok(Self::Move),
            _ => Err(format!("unknown .jetlib access-convention tag {tag}")),
        }
    }
}

/// One exact C ABI row in a `.jetlib` header. D-EMBED1 makes the scalar and
/// parameter count sufficient for its scalar representation, while `symbol`
/// is the emitted foreign name. Access conventions remain explicit metadata;
/// they are not inferred from the generated Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetLibExport {
    pub name: String,
    pub symbol: String,
    pub scalar: JetLibScalar,
    pub params: u32,
    pub conventions: Vec<JetLibAccess>,
}

impl JetLibExport {
    pub fn new(name: impl Into<String>, scalar: JetLibScalar, params: usize) -> Self {
        Self::with_conventions(
            name,
            scalar,
            std::iter::repeat(JetLibAccess::Read).take(params).collect(),
        )
    }

    pub fn with_conventions(
        name: impl Into<String>,
        scalar: JetLibScalar,
        conventions: Vec<JetLibAccess>,
    ) -> Self {
        let name = name.into();
        Self {
            symbol: c_symbol(&name),
            name,
            scalar,
            params: u32::try_from(conventions.len())
                .expect("a function cannot have more than u32 params"),
            conventions,
        }
    }
}

/// The load-time identity a `.jetlib` artifact carries in its header: the
/// exact compiler/build/target/linker/ABI identity, Library name, checked
/// export table, payload digest, and effects its own code declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JetLibStamp {
    pub compiler_version: String,
    pub compiler_build: String,
    pub library_name: String,
    /// The selected manifest output entry, retained for build provenance.
    pub entry: Option<String>,
    pub target: String,
    pub target_triple: String,
    pub linker_identity: String,
    pub abi_identity: String,
    pub abi_version: u32,
    pub exports: Vec<JetLibExport>,
    pub declared_effects: EffectSet,
    pub payload_digest: String,
}

/// A complete `.jetlib`: the checked stamp followed by the native payload.
/// The payload is intentionally opaque here; the driver owns production and
/// the embedded Prelude owns the host mapping adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JetLibArtifact {
    pub stamp: JetLibStamp,
    pub payload: Vec<u8>,
}

impl JetLibArtifact {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.stamp.encode();
        out.extend_from_slice(&(self.payload.len() as u64).to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let (stamp, consumed) = JetLibStamp::decode_prefix(bytes)?;
        let rest = &bytes[consumed..];
        if rest.len() < 8 {
            return Err("truncated .jetlib artifact (payload length)".to_string());
        }
        let payload_len = u64::from_be_bytes(rest[..8].try_into().unwrap());
        let payload_len = usize::try_from(payload_len)
            .map_err(|_| "the .jetlib payload is too large for this host".to_string())?;
        let payload = &rest[8..];
        if payload.len() != payload_len {
            return Err(format!(
                "truncated .jetlib artifact (payload declares {payload_len} bytes, found {})",
                payload.len()
            ));
        }
        Ok(Self {
            stamp,
            payload: payload.to_vec(),
        })
    }
}

impl JetLibStamp {
    /// The stamp a build on this running compiler would write.
    pub fn for_this_compiler(declared_effects: EffectSet) -> Self {
        JetLibStamp {
            compiler_version: COMPILER_VERSION.to_string(),
            compiler_build: COMPILER_VERSION.to_string(),
            library_name: "anonymous".to_string(),
            entry: None,
            target: current_target(),
            target_triple: current_target(),
            linker_identity: "system".to_string(),
            abi_identity: ABI_IDENTITY.to_string(),
            abi_version: ABI_VERSION,
            exports: Vec::new(),
            declared_effects,
            payload_digest: String::new(),
        }
    }

    /// The stamp emitted for one checked native Library projection.
    pub fn for_library(
        name: impl Into<String>,
        exports: Vec<JetLibExport>,
        declared_effects: EffectSet,
    ) -> Self {
        Self::for_library_with_identity(
            name,
            exports,
            declared_effects,
            COMPILER_VERSION,
            current_target(),
            "system",
        )
    }

    /// The stamp emitted by the native driver with the exact identities that
    /// produced the payload. `linker_identity` is the complete toolchain
    /// identity, not only a display label.
    pub fn for_library_with_identity(
        name: impl Into<String>,
        exports: Vec<JetLibExport>,
        declared_effects: EffectSet,
        compiler_build: impl Into<String>,
        target_triple: impl Into<String>,
        linker_identity: impl Into<String>,
    ) -> Self {
        Self {
            compiler_version: COMPILER_VERSION.to_string(),
            compiler_build: compiler_build.into(),
            library_name: name.into(),
            entry: None,
            target: current_target(),
            target_triple: target_triple.into(),
            linker_identity: linker_identity.into(),
            abi_identity: ABI_IDENTITY.to_string(),
            abi_version: ABI_VERSION,
            exports,
            declared_effects,
            payload_digest: String::new(),
        }
    }

    /// Seal the native payload identity after the final shared object exists.
    pub fn seal_payload(&mut self, payload: &[u8]) {
        self.payload_digest = payload_digest(payload);
    }

    /// Serialize the header bytes a `.jetlib` artifact carries. Std-only
    /// (I6): a fixed magic, compiler/build/linker identity, ABI/target
    /// identity, the checked export table, payload digest, and a count of
    /// length-prefixed effect names
    /// (sorted — `EffectSet` is a `BTreeSet`, so iteration order is already
    /// canonical).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        push_bytes(&mut out, self.compiler_version.as_bytes());
        push_bytes(&mut out, self.compiler_build.as_bytes());
        push_bytes(&mut out, self.library_name.as_bytes());
        match &self.entry {
            Some(entry) => {
                out.push(1);
                push_bytes(&mut out, entry.as_bytes());
            }
            None => out.push(0),
        }
        push_bytes(&mut out, self.target.as_bytes());
        push_bytes(&mut out, self.target_triple.as_bytes());
        push_bytes(&mut out, self.linker_identity.as_bytes());
        push_bytes(&mut out, self.abi_identity.as_bytes());
        out.extend_from_slice(&self.abi_version.to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(self.exports.len())
                .expect("a .jetlib header cannot contain more than u32 exports")
                .to_be_bytes(),
        );
        for export in &self.exports {
            push_bytes(&mut out, export.name.as_bytes());
            push_bytes(&mut out, export.symbol.as_bytes());
            out.push(export.scalar.tag());
            out.extend_from_slice(&export.params.to_be_bytes());
            for convention in &export.conventions {
                out.push(convention.tag());
            }
        }
        out.extend_from_slice(&(self.declared_effects.len() as u32).to_be_bytes());
        for effect in &self.declared_effects {
            push_bytes(&mut out, effect.as_bytes());
        }
        push_bytes(&mut out, self.payload_digest.as_bytes());
        out
    }

    /// Parse header bytes written by [`encode`]. A bad magic, a truncated
    /// buffer, or non-UTF8 text fails closed — a malformed artifact is
    /// never partially trusted.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let (stamp, consumed) = Self::decode_prefix(bytes)?;
        if consumed != bytes.len() {
            return Err("unexpected payload after .jetlib stamp".to_string());
        }
        Ok(stamp)
    }

    fn decode_prefix(bytes: &[u8]) -> Result<(Self, usize), String> {
        let mut cur = bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| "not a .jetlib artifact (bad magic)".to_string())?;
        let (compiler_version, rest) = take_string(cur, "compiler version")?;
        cur = rest;
        let (compiler_build, rest) = take_string(cur, "compiler build")?;
        cur = rest;
        let (library_name, rest) = take_string(cur, "library name")?;
        cur = rest;
        let (entry, rest) = take_optional_string(cur, "output entry")?;
        cur = rest;
        let (target, rest) = take_string(cur, "target")?;
        cur = rest;
        let (target_triple, rest) = take_string(cur, "target triple")?;
        cur = rest;
        let (linker_identity, rest) = take_string(cur, "linker identity")?;
        cur = rest;
        let (abi_identity, rest) = take_string(cur, "ABI identity")?;
        cur = rest;
        let (abi_version, rest) = take_u32(cur, "ABI version")?;
        cur = rest;
        let (count, rest) = take_count(cur, "export")?;
        cur = rest;
        let mut exports = Vec::with_capacity(count);
        for _ in 0..count {
            let (name, rest) = take_string(cur, "export name")?;
            cur = rest;
            let (symbol, rest) = take_string(cur, "export symbol")?;
            cur = rest;
            let Some((&tag, rest)) = cur.split_first() else {
                return Err("truncated .jetlib header (export scalar)".to_string());
            };
            cur = rest;
            let scalar = JetLibScalar::from_tag(tag)?;
            let (params, rest) = take_u32(cur, "export parameter count")?;
            cur = rest;
            let params_usize = usize::try_from(params)
                .map_err(|_| ".jetlib export has too many parameters".to_string())?;
            if params_usize > MAX_EXPORT_PARAMS {
                return Err(".jetlib export has too many parameters".to_string());
            }
            let mut conventions = Vec::with_capacity(params_usize);
            for _ in 0..params_usize {
                let Some((&tag, rest)) = cur.split_first() else {
                    return Err("truncated .jetlib header (access convention)".to_string());
                };
                cur = rest;
                conventions.push(JetLibAccess::from_tag(tag)?);
            }
            exports.push(JetLibExport {
                name,
                symbol,
                scalar,
                params,
                conventions,
            });
        }
        let (count, rest) = take_count(cur, "effect")?;
        cur = rest;
        let mut declared_effects = EffectSet::new();
        for _ in 0..count {
            let (effect, rest) = take_bytes(cur)?;
            cur = rest;
            declared_effects.insert(
                String::from_utf8(effect).map_err(|_| "effect name is not UTF-8".to_string())?,
            );
        }
        let (payload_digest, rest) = take_string(cur, "payload digest")?;
        cur = rest;
        Ok((
            JetLibStamp {
                compiler_version,
                compiler_build,
                library_name,
                entry,
                target,
                target_triple,
                linker_identity,
                abi_identity,
                abi_version,
                exports,
                declared_effects,
                payload_digest,
            },
            bytes.len() - cur.len(),
        ))
    }
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(
        &u32::try_from(bytes.len())
            .expect("a .jetlib header field cannot exceed u32 bytes")
            .to_be_bytes(),
    );
    out.extend_from_slice(bytes);
}

fn take_bytes(cur: &[u8]) -> Result<(Vec<u8>, &[u8]), String> {
    if cur.len() < 4 {
        return Err("truncated .jetlib header (length prefix)".to_string());
    }
    let len = u32::from_be_bytes([cur[0], cur[1], cur[2], cur[3]]) as usize;
    let cur = &cur[4..];
    if len > MAX_HEADER_FIELD_BYTES {
        return Err(".jetlib header field is too large".to_string());
    }
    if cur.len() < len {
        return Err("truncated .jetlib header (field bytes)".to_string());
    }
    Ok((cur[..len].to_vec(), &cur[len..]))
}

fn take_string<'a>(cur: &'a [u8], what: &str) -> Result<(String, &'a [u8]), String> {
    let (bytes, rest) = take_bytes(cur).map_err(|error| format!("{error} ({what})"))?;
    String::from_utf8(bytes)
        .map(|value| (value, rest))
        .map_err(|_| format!("{what} is not UTF-8"))
}

fn take_optional_string<'a>(
    cur: &'a [u8],
    what: &str,
) -> Result<(Option<String>, &'a [u8]), String> {
    let Some((&present, rest)) = cur.split_first() else {
        return Err(format!("truncated .jetlib header ({what} presence)"));
    };
    match present {
        0 => Ok((None, rest)),
        1 => take_string(rest, what).map(|(value, rest)| (Some(value), rest)),
        _ => Err(format!("invalid .jetlib {what} presence tag {present}")),
    }
}

fn take_u32<'a>(cur: &'a [u8], what: &str) -> Result<(u32, &'a [u8]), String> {
    if cur.len() < 4 {
        return Err(format!("truncated .jetlib header ({what})"));
    }
    Ok((
        u32::from_be_bytes([cur[0], cur[1], cur[2], cur[3]]),
        &cur[4..],
    ))
}

fn take_count<'a>(cur: &'a [u8], what: &str) -> Result<(usize, &'a [u8]), String> {
    let (count, rest) = take_u32(cur, &format!("{what} count"))?;
    let count = usize::try_from(count)
        .map_err(|_| format!(".jetlib {what} count is too large"))?;
    if count > MAX_HEADER_ITEMS {
        return Err(format!(".jetlib header has too many {what} entries"));
    }
    Ok((count, rest))
}

/// The target identity used by both the artifact producer and the load-time
/// adapter. Library builds are host-native, so OS plus architecture is the
/// complete target identity for this surface.
pub fn current_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn payload_digest(payload: &[u8]) -> String {
    format!("sha256-{}", crate::SHA256::sha256_hex(payload))
}

pub fn validate_payload_digest(
    stamp: &JetLibStamp,
    payload: &[u8],
) -> Result<(), String> {
    if stamp.payload_digest.is_empty() {
        return Err(format!(
            "E1341: library `{}` has no native payload digest",
            if stamp.library_name.is_empty() {
                "<unnamed>"
            } else {
                &stamp.library_name
            }
        ));
    }
    let actual = payload_digest(payload);
    if stamp.payload_digest != actual {
        return Err(format!(
            "E1341: library `{}` payload digest is `{}`, but the artifact contains `{}`",
            stamp.library_name, stamp.payload_digest, actual
        ));
    }
    Ok(())
}

fn valid_payload_digest(digest: &str) -> bool {
    let Some(hex) = digest.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Validate metadata that is meaningful only at the native load boundary.
/// Compiler identity remains a separate first check so E1338 wins over any
/// untrusted effect or ABI claim.
pub fn validate_load_metadata(stamp: &JetLibStamp) -> Result<(), String> {
    if stamp.abi_version != ABI_VERSION {
        return Err(format!(
            "E1341: library `{}` uses unsupported .jetlib ABI version {}; this loader accepts ABI version {}",
            if stamp.library_name.is_empty() {
                "<unnamed>"
            } else {
                &stamp.library_name
            },
            stamp.abi_version,
            ABI_VERSION
        ));
    }
    if stamp.library_name.is_empty() || stamp.library_name.contains('\0') {
        return Err("E1341: .jetlib metadata has no valid Library name".to_string());
    }
    if stamp
        .entry
        .as_deref()
        .is_some_and(|entry| entry.is_empty() || entry.contains('\0'))
    {
        return Err("E1341: .jetlib metadata has an invalid output entry".to_string());
    }
    for (label, value) in [
        ("compiler build", &stamp.compiler_build),
        ("target triple", &stamp.target_triple),
        ("linker identity", &stamp.linker_identity),
    ] {
        if value.is_empty() || value.contains('\0') {
            return Err(format!("E1341: .jetlib metadata has no valid {label}"));
        }
    }
    if stamp.abi_identity != ABI_IDENTITY {
        return Err(format!(
            "E1341: library `{}` uses unsupported native ABI identity `{}`",
            stamp.library_name, stamp.abi_identity
        ));
    }
    if !valid_payload_digest(&stamp.payload_digest) {
        return Err(format!(
            "E1341: library `{}` has an invalid payload digest",
            stamp.library_name
        ));
    }
    if stamp.target != current_target() {
        return Err(format!(
            "E1341: library `{}` targets `{}`, but this loader targets `{}`",
            stamp.library_name,
            stamp.target,
            current_target()
        ));
    }
    if stamp.exports.is_empty() {
        return Err(format!(
            "E1341: library `{}` has no exported functions",
            stamp.library_name
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    let mut symbols = std::collections::BTreeSet::new();
    for export in &stamp.exports {
        if export.name.is_empty() || export.name.contains('\0') {
            return Err("E1341: .jetlib metadata has an invalid export name".to_string());
        }
        if export.symbol.is_empty()
            || export.symbol.contains('\0')
            || export.symbol != c_symbol(&export.name)
        {
            return Err(format!(
                "E1341: .jetlib export `{}` has an invalid native symbol `{}`",
                export.name, export.symbol
            ));
        }
        if !names.insert(export.name.clone()) {
            return Err(format!(
                "E1341: .jetlib metadata repeats export `{}`",
                export.name
            ));
        }
        if !symbols.insert(export.symbol.clone()) {
            return Err(format!(
                "E1341: .jetlib metadata repeats native symbol `{}`",
                export.symbol
            ));
        }
        let params = usize::try_from(export.params)
            .map_err(|_| "E1341: .jetlib export parameter count is invalid".to_string())?;
        if params > MAX_EXPORT_PARAMS || export.conventions.len() != params {
            return Err(format!(
                "E1341: .jetlib export `{}` has invalid access-convention metadata",
                export.name
            ));
        }
    }
    Ok(())
}

fn c_symbol(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        out.insert(0, '_');
    }
    out
}

/// E1338: an artifact's compiler-identity stamp doesn't match the running
/// compiler. D-LIB-REUSE1=B makes no cross-version binary layout promise, so
/// this is refused before the artifact is mapped, never a crash.
pub fn e1338(artifact_version: &str) -> Diagnostic {
    Diagnostic::error(
        "E1338",
        format!(
            "this loadable library was built by Jet `{artifact_version}`, but the loading program uses Jet `{COMPILER_VERSION}`"
        ),
        "a `.jetlib` artifact pins the exact compiler identity that built it (D-LIB-REUSE1=B) — Jet makes no cross-version binary layout promise, so a mismatched artifact is refused before it is mapped".to_string(),
        "rebuild the library with the loading program's Jet version, or install a matching Jet toolchain".to_string(),
        None::<Span>,
    )
}

/// E1339: a loaded library declares an effect the load site's grant doesn't
/// cover (D-LIB-DYNTRUST1=A). `library` names the artifact for the message.
pub fn e1339(library: &str, effect: &str) -> Diagnostic {
    Diagnostic::error(
        "E1339",
        format!(
            "library `{library}` declares the `{effect}` effect, which this load site doesn't grant"
        ),
        "a loadable Jet library declares its effects like any package (D-LIB-DYNTRUST1=A); the host states what it grants at the load site, and a library asking for more is refused before it is mapped".to_string(),
        format!(
            "widen the grant at the load site to include `{effect}`, or remove the effect from the library"
        ),
        None::<Span>,
    )
}

/// c2: refuse before mapping when the artifact's compiler identity doesn't
/// match this running compiler.
pub fn check_compiler_identity(stamp: &JetLibStamp) -> Result<(), Diagnostic> {
    if stamp.compiler_version != COMPILER_VERSION {
        return Err(e1338(&stamp.compiler_version));
    }
    Ok(())
}

/// c3: refuse before mapping when the artifact declares an effect the load
/// site's `grant` doesn't cover. Coverage (`effect_covers`), not exact
/// membership, matches an ancestor grant to a leaf effect (D-EFFTREE1) — the
/// same rule `EffectBudget::enforce` already uses for the whole-graph budget.
pub fn check_effect_grant(
    library: &str,
    stamp: &JetLibStamp,
    grant: &EffectSet,
) -> Result<(), Vec<Diagnostic>> {
    let mut diags = Vec::new();
    for effect in &stamp.declared_effects {
        if !grant.iter().any(|bound| effect_covers(bound, effect)) {
            diags.push(e1339(library, effect));
        }
    }
    if diags.is_empty() {
        Ok(())
    } else {
        Err(diags)
    }
}

/// The single "before mapping" gate: compiler identity first (c2), then the
/// effect grant (c3). Identity is checked first — a forged or foreign
/// artifact fails the pin before anything about its claimed effects is
/// trusted (D-LIB-DYNTRUST1's ratified tradeoff).
pub fn check_before_map(
    library: &str,
    stamp: &JetLibStamp,
    grant: &EffectSet,
) -> Result<(), Vec<Diagnostic>> {
    check_compiler_identity(stamp).map_err(|d| vec![d])?;
    check_effect_grant(library, stamp, grant)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effects(names: &[&str]) -> EffectSet {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn stamp_round_trips_through_encode_decode() {
        let stamp = JetLibStamp {
            compiler_version: "1.4.2".to_string(),
            declared_effects: effects(&["FS", "Net"]),
            ..Default::default()
        };
        let bytes = stamp.encode();
        assert_eq!(JetLibStamp::decode(&bytes).unwrap(), stamp);
    }

    #[test]
    fn library_stamp_carries_the_exact_native_export_table() {
        let mut stamp = JetLibStamp::for_library(
            "loadable",
            vec![
                JetLibExport::new("on_tick", JetLibScalar::Int, 1),
                JetLibExport::new("greet", JetLibScalar::Text, 1),
            ],
            effects(&["FS"]),
        );
        stamp.entry = Some("run".to_string());
        stamp.seal_payload(b"native-shared-object");
        let decoded = JetLibStamp::decode(&stamp.encode()).unwrap();
        assert_eq!(decoded, stamp);
        assert_eq!(decoded.entry.as_deref(), Some("run"));
        assert!(validate_load_metadata(&decoded).is_ok());
    }

    #[test]
    fn empty_effect_set_round_trips() {
        let stamp = JetLibStamp::for_this_compiler(EffectSet::new());
        let bytes = stamp.encode();
        assert_eq!(JetLibStamp::decode(&bytes).unwrap(), stamp);
    }

    #[test]
    fn artifact_round_trip_keeps_native_payload_opaque() {
        let artifact = JetLibArtifact {
            stamp: JetLibStamp::for_this_compiler(EffectSet::new()),
            payload: b"native-shared-object".to_vec(),
        };
        assert_eq!(
            JetLibArtifact::decode(&artifact.encode()).unwrap(),
            artifact
        );
    }

    #[test]
    fn artifact_rejects_a_truncated_payload() {
        let artifact = JetLibArtifact {
            stamp: JetLibStamp::for_this_compiler(EffectSet::new()),
            payload: b"native-shared-object".to_vec(),
        };
        let mut bytes = artifact.encode();
        bytes.pop();
        let error = JetLibArtifact::decode(&bytes).unwrap_err();
        assert!(error.contains("payload declares"), "{error}");
    }

    #[test]
    fn load_metadata_accepts_generic_export_names_and_shapes() {
        let mut stamp = JetLibStamp::for_library(
            "loadable",
            vec![JetLibExport::with_conventions(
                "advance",
                JetLibScalar::Bool,
                vec![JetLibAccess::Write],
            )],
            EffectSet::new(),
        );
        stamp.seal_payload(b"native-shared-object");
        assert!(validate_load_metadata(&stamp).is_ok());
    }

    #[test]
    fn load_metadata_rejects_a_wrong_abi_identity() {
        let mut stamp = JetLibStamp::for_library(
            "loadable",
            vec![JetLibExport::new("advance", JetLibScalar::Int, 1)],
            EffectSet::new(),
        );
        stamp.abi_identity = "foreign-abi".to_string();
        let error = validate_load_metadata(&stamp).unwrap_err();
        assert!(error.contains("native ABI identity"), "{error}");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let err = JetLibStamp::decode(b"not-a-jetlib").unwrap_err();
        assert!(err.contains("bad magic"), "{err}");
    }

    #[test]
    fn decode_rejects_truncated_bytes() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["FS"]));
        let mut bytes = stamp.encode();
        bytes.truncate(bytes.len() - 2);
        assert!(JetLibStamp::decode(&bytes).is_err());
    }

    #[test]
    fn matching_compiler_identity_passes() {
        let stamp = JetLibStamp::for_this_compiler(EffectSet::new());
        assert!(check_compiler_identity(&stamp).is_ok());
    }

    #[test]
    fn mismatched_compiler_identity_is_refused_before_mapping() {
        let stamp = JetLibStamp {
            compiler_version: "0.0.1-old".to_string(),
            declared_effects: EffectSet::new(),
            ..Default::default()
        };
        let err = check_compiler_identity(&stamp).unwrap_err();
        assert_eq!(err.code, "E1338");
        assert!(err.what.contains("0.0.1-old"));
        assert!(err.what.contains(COMPILER_VERSION));
    }

    #[test]
    fn declared_effect_within_grant_passes() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["FS"]));
        assert!(check_effect_grant("skyhawk", &stamp, &effects(&["FS"])).is_ok());
    }

    #[test]
    fn declared_effect_outside_grant_is_refused() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["Net"]));
        let errs = check_effect_grant("skyhawk", &stamp, &effects(&["FS"])).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "E1339");
        assert!(errs[0].what.contains("Net"));
        assert!(errs[0].what.contains("skyhawk"));
    }

    #[test]
    fn nested_effect_is_covered_by_ancestor_grant() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["FS.Read"]));
        assert!(check_effect_grant("skyhawk", &stamp, &effects(&["FS"])).is_ok());
    }

    #[test]
    fn sibling_leaf_is_not_covered_by_a_different_leaf_grant() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["FS.Write"]));
        let errs = check_effect_grant("skyhawk", &stamp, &effects(&["FS.Read"])).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "E1339");
    }

    #[test]
    fn check_before_map_reports_identity_before_effects() {
        // A mismatched-identity artifact is refused on the pin alone, even
        // when it also declares an effect outside the grant — a forged
        // artifact fails the identity check before its claims are trusted.
        let stamp = JetLibStamp {
            compiler_version: "0.0.1-old".to_string(),
            declared_effects: effects(&["Net"]),
            ..Default::default()
        };
        let errs = check_before_map("skyhawk", &stamp, &EffectSet::new()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "E1338");
    }

    #[test]
    fn check_before_map_passes_a_matching_in_grant_artifact() {
        let stamp = JetLibStamp::for_this_compiler(effects(&["FS"]));
        assert!(check_before_map("skyhawk", &stamp, &effects(&["FS"])).is_ok());
    }
}
