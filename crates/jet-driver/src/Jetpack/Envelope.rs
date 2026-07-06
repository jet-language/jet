//! D-JPK-CACHE1=A (U24) — the A4 hangar-object envelope.
//!
//! Every realized hangar object carries a small identity block that makes it a
//! cache-substitutable artifact: the content hash of its output tree, the
//! platform it was built for, a detached-signature slot (filled by package
//! signing, card #13), and provenance (how it was produced). These fields are
//! frozen into the hangar/lock schema now — the binary-cache protocol that
//! consumes them is a later card (D-JPK-CACHE1 protocol slice). A
//! build-from-source output is an envelope-carrying object exactly like a
//! substituted one, so the resolver never has to care which path produced it.

use crate::SHA256;
use std::path::{Path, PathBuf};

/// The A4 envelope. `Default` is the empty envelope (older records / providers
/// that predate the field), so reading a legacy `meta.json` never fails.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Envelope {
    /// Content hash of the realized output tree (`sha256-…`) — the cache key.
    pub output_hash: String,
    /// Target platform key (`<arch>-<os>`, e.g. `x86_64-linux`).
    pub platform: String,
    /// Detached-signature slot; empty until package signing (card #13) fills it.
    pub signature: String,
    /// How this output was produced: the resolved source ref + build recipe id.
    pub provenance: String,
}

impl Envelope {
    /// True when no envelope field is populated (a legacy or unrealized record).
    pub fn is_empty(&self) -> bool {
        self.output_hash.is_empty()
            && self.platform.is_empty()
            && self.signature.is_empty()
            && self.provenance.is_empty()
    }

    /// Build the envelope for a freshly realized output rooted at `out`.
    /// `reference` is the resolved source ref; `recipe_id` names the build path
    /// that produced it (`core-source`, `core-cargo-rlib`, `nix`, …).
    pub fn for_output(out: &str, reference: &str, recipe_id: &str) -> Envelope {
        Envelope {
            output_hash: output_hash_of(out),
            platform: host_platform(),
            signature: String::new(),
            provenance: format!("{reference} via {recipe_id}"),
        }
    }
}

/// The platform key for the host build target (`<arch>-<os>`). std-only (I6):
/// derived from the compile target, which is what the realized artifact runs on.
pub fn host_platform() -> String {
    super::Platform::host_key()
}

/// Content hash of a realized output root.
///
/// For a real local directory this is the full-tree content hash (every file's
/// relative path, length, and bytes) — not the compiler's `.jet`-only
/// `tree_hash`, since a realized output is `bin/`, `.rlib`, and arbitrary files.
/// For a non-directory (a `/nix/store/…` path in a fixture run, or a bare
/// output string) the store path is already content-addressed, so its text is
/// the identity.
pub fn output_hash_of(out: &str) -> String {
    let p = Path::new(out);
    if p.is_dir() {
        format!("sha256-{}", tree_content_hash(p))
    } else {
        format!("sha256-{}", SHA256::sha256_hex(out.as_bytes()))
    }
}

/// A content fingerprint over a whole directory tree: every file's relative
/// path, length, and bytes, in sorted order. Addresses *any* tree (unlike the
/// `.jet`-only `SHA256::tree_hash`).
pub fn tree_content_hash(root: &Path) -> String {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(root, &mut files);
    files.sort();
    let mut input: Vec<u8> = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        input.extend_from_slice(rel.as_bytes());
        input.push(0);
        if let Ok(bytes) = std::fs::read(path) {
            input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            input.extend_from_slice(&bytes);
        }
    }
    SHA256::sha256_hex(&input)
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_key_is_arch_os() {
        let p = host_platform();
        assert!(p.contains('-'), "platform key should be <arch>-<os>: {p}");
    }

    #[test]
    fn output_hash_of_dir_reflects_contents() {
        let base = std::env::temp_dir().join(format!(
            "env-hash-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("f"), "one").unwrap();
        std::fs::write(b.join("f"), "two").unwrap();
        let ha = output_hash_of(&a.to_string_lossy());
        let hb = output_hash_of(&b.to_string_lossy());
        assert!(ha.starts_with("sha256-"));
        assert_ne!(ha, hb, "different contents must hash differently");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn for_output_fills_all_fields_for_a_real_tree() {
        let dir = std::env::temp_dir().join(format!(
            "env-for-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x"), "hi").unwrap();
        let e = Envelope::for_output(&dir.to_string_lossy(), "mine:hello", "core-source");
        assert!(!e.is_empty());
        assert!(e.output_hash.starts_with("sha256-"));
        assert!(!e.platform.is_empty());
        assert!(e.provenance.contains("mine:hello"));
        assert!(e.provenance.contains("core-source"));
        assert!(
            e.signature.is_empty(),
            "signature slot stays empty until #13"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
