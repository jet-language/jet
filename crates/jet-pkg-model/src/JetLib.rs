//! `.jetlib` loadable-library artifact stamp and load-time trust boundary
//! (card #1421, criteria 2 and 3 only).
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
//! request this artifact. Deliberately out of scope here (card #1421 criteria
//! 5-6, later slices): the `Mod.load` Jet-level call surface and native export
//! (D-LIB-EXPORT1). This module is the artifact-format and check machinery
//! those slices wire up — nothing here assumes their user-facing spelling.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Manifest::COMPILER_VERSION;
use crate::Sema::{effect_covers, EffectSet};

/// Fixed header magic for a `.jetlib` artifact stamp.
const MAGIC: &[u8] = b"jet-jetlib-v1\0";

/// The load-time identity a `.jetlib` artifact carries in its header: the
/// exact compiler version that built it, and the effects its own code
/// declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JetLibStamp {
    pub compiler_version: String,
    pub declared_effects: EffectSet,
}

impl JetLibStamp {
    /// The stamp a build on this running compiler would write.
    pub fn for_this_compiler(declared_effects: EffectSet) -> Self {
        JetLibStamp {
            compiler_version: COMPILER_VERSION.to_string(),
            declared_effects,
        }
    }

    /// Serialize the header bytes a `.jetlib` artifact carries. Std-only
    /// (I6): a fixed magic, then a length-prefixed version string, then a
    /// count of length-prefixed effect names (sorted — `EffectSet` is a
    /// `BTreeSet`, so iteration order is already canonical).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        push_bytes(&mut out, self.compiler_version.as_bytes());
        out.extend_from_slice(&(self.declared_effects.len() as u32).to_be_bytes());
        for effect in &self.declared_effects {
            push_bytes(&mut out, effect.as_bytes());
        }
        out
    }

    /// Parse header bytes written by [`encode`]. A bad magic, a truncated
    /// buffer, or non-UTF8 text fails closed — a malformed artifact is
    /// never partially trusted.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let cur = bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| "not a .jetlib artifact (bad magic)".to_string())?;
        let (version, cur) = take_bytes(cur)?;
        let compiler_version = String::from_utf8(version)
            .map_err(|_| "compiler version is not UTF-8".to_string())?;
        if cur.len() < 4 {
            return Err("truncated .jetlib header (effect count)".to_string());
        }
        let count = u32::from_be_bytes([cur[0], cur[1], cur[2], cur[3]]) as usize;
        let mut cur = &cur[4..];
        let mut declared_effects = EffectSet::new();
        for _ in 0..count {
            let (effect, rest) = take_bytes(cur)?;
            cur = rest;
            declared_effects.insert(
                String::from_utf8(effect).map_err(|_| "effect name is not UTF-8".to_string())?,
            );
        }
        Ok(JetLibStamp {
            compiler_version,
            declared_effects,
        })
    }
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn take_bytes(cur: &[u8]) -> Result<(Vec<u8>, &[u8]), String> {
    if cur.len() < 4 {
        return Err("truncated .jetlib header (length prefix)".to_string());
    }
    let len = u32::from_be_bytes([cur[0], cur[1], cur[2], cur[3]]) as usize;
    let cur = &cur[4..];
    if cur.len() < len {
        return Err("truncated .jetlib header (field bytes)".to_string());
    }
    Ok((cur[..len].to_vec(), &cur[len..]))
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
        };
        let bytes = stamp.encode();
        assert_eq!(JetLibStamp::decode(&bytes).unwrap(), stamp);
    }

    #[test]
    fn empty_effect_set_round_trips() {
        let stamp = JetLibStamp::for_this_compiler(EffectSet::new());
        let bytes = stamp.encode();
        assert_eq!(JetLibStamp::decode(&bytes).unwrap(), stamp);
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
