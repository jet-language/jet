//! U13: encrypted-repo-secrets engine (card c9jetpackgates, D-JPK-SECRETCRYPTO1).
//!
//! Covers:
//!   * `jetpack secrets keygen/recipients/set/get` full round trip through the
//!     real age-style crypto FFI bridge;
//!   * `jetpack secrets get <missing>` is a clean E1263, not a hang/panic;
//!   * no plaintext ever lands on disk anywhere under the project or the
//!     helper's own cache — a real filesystem-scan assertion, not just a unit
//!     test of the crypto function.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::jetpack_bin;

struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "jpk-secrets-it-{tag}-{nanos}-{:?}",
            std::thread::current().id()
        ));
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn jetpack() -> Command {
    Command::new(jetpack_bin())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // Consume `\x1b[...letter` (CSI sequence).
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Full round trip: `keygen` mints an identity, `recipients add` registers
/// it, `set` encrypts+stores an entry, `get` decrypts it back — and the
/// plaintext value never appears anywhere on disk under the project dir.
#[test]
fn secrets_keygen_set_get_roundtrip_no_plaintext_on_disk() {
    let proj = Scratch::new("roundtrip-proj");
    let keys = Scratch::new("roundtrip-keys");

    let keygen_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "keygen"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets keygen");
    assert!(
        keygen_out.status.success(),
        "keygen failed: {}",
        String::from_utf8_lossy(&keygen_out.stderr)
    );
    let keygen_text = strip_ansi(&String::from_utf8_lossy(&keygen_out.stderr));
    let recipient = keygen_text
        .lines()
        .find_map(|l| l.split_once("recipient: ").map(|(_, v)| v.trim()))
        .expect("keygen must print a recipient line")
        .to_string();
    assert!(
        recipient.starts_with("age1"),
        "recipient should be an age1... string: {recipient}"
    );

    let add_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "recipients", "add", &recipient])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets recipients add");
    assert!(add_out.status.success());

    let list_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "recipients", "list"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets recipients list");
    assert_eq!(String::from_utf8_lossy(&list_out.stdout).trim(), recipient);

    const PLAINTEXT: &str = "hunter2-super-secret-value";
    let set_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "set", "db_password", PLAINTEXT])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets set");
    assert!(
        set_out.status.success(),
        "set failed: {}",
        String::from_utf8_lossy(&set_out.stderr)
    );

    // The no-plaintext invariant: scan every file under the project dir
    // (the encrypted store, the recipients file, anything else jetpack might
    // have written) for the raw plaintext bytes. None may contain it.
    for entry in walk(&proj.path) {
        let bytes = fs::read(&entry).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(PLAINTEXT),
            "plaintext leaked into `{}` — secrets must only ever touch disk as ciphertext",
            entry.display()
        );
    }
    // No stray temp file (`.tmp`) survives a completed `set`.
    for entry in walk(&proj.path) {
        assert!(
            entry.extension().and_then(|e| e.to_str()) != Some("tmp"),
            "a stray temp file `{}` survived `secrets set` — the write must be atomic",
            entry.display()
        );
    }
    // The store itself must exist and hold ciphertext (not literally empty).
    let store = proj.path.join(".jet").join("secrets.age");
    assert!(store.is_file(), "store must exist after `set`");
    assert!(!fs::read(&store).unwrap().is_empty());

    let get_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "db_password"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets get");
    assert!(
        get_out.status.success(),
        "get failed: {}",
        String::from_utf8_lossy(&get_out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&get_out.stdout).trim(),
        PLAINTEXT,
        "decrypted value must round-trip exactly"
    );

    // Updating an existing store exercises Windows replacement semantics:
    // rename-on-top is not supported there without an explicit atomic
    // replacement operation.
    const UPDATED: &str = "second-super-secret-value";
    let update_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "set", "db_password", UPDATED])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("update existing secret");
    assert!(
        update_out.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&update_out.stderr)
    );
    let updated_get = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "db_password"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("read updated secret");
    assert!(updated_get.status.success());
    assert_eq!(String::from_utf8_lossy(&updated_get.stdout).trim(), UPDATED);
    for entry in walk(&proj.path) {
        let bytes = fs::read(&entry).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(PLAINTEXT), "old plaintext leaked into `{}`", entry.display());
        assert!(!text.contains(UPDATED), "updated plaintext leaked into `{}`", entry.display());
    }
}

/// `jetpack secrets get <name>` on a name that isn't in the store is a clean
/// E1263, not a hang or a panic.
#[test]
fn secrets_get_missing_entry_is_e1263() {
    let proj = Scratch::new("missing-proj");
    let keys = Scratch::new("missing-keys");

    // Set up a store with one entry so the "missing" case is a real lookup
    // miss, not just an absent store.
    let keygen_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "keygen"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets keygen");
    let keygen_text = strip_ansi(&String::from_utf8_lossy(&keygen_out.stderr));
    let recipient = keygen_text
        .lines()
        .find_map(|l| l.split_once("recipient: ").map(|(_, v)| v.trim()))
        .unwrap()
        .to_string();
    jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "recipients", "add", &recipient])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .unwrap();
    jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "set", "known_key", "known_value"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .unwrap();

    let get_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "no_such_key"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets get");
    assert!(
        !get_out.status.success(),
        "get on a missing entry must fail (nonzero exit)"
    );
    let stderr = strip_ansi(&String::from_utf8_lossy(&get_out.stderr));
    assert!(
        stderr.contains("E1263"),
        "expected E1263 in stderr, got:\n{stderr}"
    );
}

/// `jetpack secrets get <name>` with no store at all (never `set` anything)
/// is also E1263, not a different error — an absent store is just an empty
/// one, so "no entry" is the same failure mode either way.
#[test]
fn secrets_get_with_no_store_at_all_is_e1263() {
    let proj = Scratch::new("nostore-proj");
    let keys = Scratch::new("nostore-keys");
    let get_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "anything"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets get");
    assert!(!get_out.status.success());
    let stderr = strip_ansi(&String::from_utf8_lossy(&get_out.stderr));
    assert!(
        stderr.contains("E1263"),
        "expected E1263 in stderr, got:\n{stderr}"
    );
}

fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
