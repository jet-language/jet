//! U13: encrypted-repo-secrets engine (card c9jetpackgates, D-JPK-SECRETCRYPTO1).
//!
//! Covers:
//!   * `jetpack secrets keygen/recipients/set/get/unset/list/import` lifecycle
//!     through the real age-style crypto FFI bridge;
//!   * `jetpack secrets get <missing>` is a clean E1263, not a hang/panic;
//!   * no plaintext ever lands on disk anywhere under the project or the
//!     helper's own cache — a real filesystem-scan assertion, not just a unit
//!     test of the crypto function.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

mod common;
use common::{jetpack_bin, Scratch};

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

fn setup_vault(proj: &Scratch, keys: &Scratch) {
    let keygen_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "keygen"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets keygen");
    assert!(
        keygen_out.status.success(),
        "keygen failed without exposing secret data"
    );
    let keygen_text = strip_ansi(&String::from_utf8_lossy(&keygen_out.stderr));
    let recipient = keygen_text
        .lines()
        .find_map(|line| {
            line.split_once("recipient: ")
                .map(|(_, value)| value.trim())
        })
        .expect("keygen must print a recipient line")
        .to_string();
    assert!(
        recipient.starts_with("age1"),
        "recipient should be age-formatted"
    );
    let add_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "recipients", "add", &recipient])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets recipients add");
    assert!(add_out.status.success(), "recipient registration failed");
}

/// Full round trip: `keygen` mints an identity, `recipients add` registers
/// it, `set` encrypts+stores an entry, `get` decrypts it back — and the
/// plaintext value never appears anywhere on disk under the project dir.
#[test]
fn secrets_keygen_set_get_roundtrip_no_plaintext_on_disk() {
    let proj = Scratch::new("roundtrip-proj");
    let keys = Scratch::new("roundtrip-keys");
    setup_vault(&proj, &keys);

    let list_out = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "recipients", "list"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("run jetpack secrets recipients list");
    assert!(String::from_utf8_lossy(&list_out.stdout)
        .trim()
        .starts_with("age1"));

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
    assert!(
        String::from_utf8_lossy(&get_out.stdout).trim() == PLAINTEXT,
        "decrypted value mismatch"
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
    assert!(
        String::from_utf8_lossy(&updated_get.stdout).trim() == UPDATED,
        "updated secret mismatch"
    );
    for entry in walk(&proj.path) {
        let bytes = fs::read(&entry).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(PLAINTEXT),
            "old plaintext leaked into `{}`",
            entry.display()
        );
        assert!(
            !text.contains(UPDATED),
            "updated plaintext leaked into `{}`",
            entry.display()
        );
    }
}

#[test]
fn secrets_unset_reencrypts_store_and_removes_value() {
    let proj = Scratch::new("unset-proj");
    let keys = Scratch::new("unset-keys");
    setup_vault(&proj, &keys);
    const REMOVED: &str = "unset-value-never-printed";
    let set_removed = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "set", "removed_name", REMOVED])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("set removed secret");
    assert!(set_removed.status.success(), "initial secret setup failed");
    let set_kept = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "set", "kept_name", "kept-value"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("set kept secret");
    assert!(set_kept.status.success(), "second secret setup failed");

    let store = proj.path.join(".jet").join("secrets.age");
    let before = fs::read(&store).unwrap();
    let unset = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "unset", "removed_name"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("unset secret");
    assert!(unset.status.success(), "unset failed");
    assert!(
        !String::from_utf8_lossy(&unset.stdout).contains(REMOVED)
            && !String::from_utf8_lossy(&unset.stderr).contains(REMOVED),
        "unset output leaked a secret value"
    );
    assert!(
        before != fs::read(&store).unwrap(),
        "unset must re-encrypt the store"
    );
    assert!(!String::from_utf8_lossy(&fs::read(&store).unwrap()).contains(REMOVED));

    let missing = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "removed_name"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("read removed secret");
    assert!(
        !missing.status.success(),
        "removed secret was still readable"
    );
    assert!(
        strip_ansi(&String::from_utf8_lossy(&missing.stderr)).contains("E1263"),
        "removed secret must return not-found"
    );
}

#[test]
fn secrets_list_prints_names_without_values() {
    let proj = Scratch::new("list-proj");
    let keys = Scratch::new("list-keys");
    setup_vault(&proj, &keys);
    const FIRST_VALUE: &str = "list-value-one-never-printed";
    const SECOND_VALUE: &str = "list-value-two-never-printed";
    for (name, value) in [("first_name", FIRST_VALUE), ("second_name", SECOND_VALUE)] {
        let set = jetpack()
            .current_dir(&proj.path)
            .args(["secrets", "set", name, value])
            .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
            .output()
            .expect("set listed secret");
        assert!(set.status.success(), "listed secret setup failed");
    }

    let list = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "list"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("list secrets");
    assert!(list.status.success(), "list failed");
    assert!(
        String::from_utf8_lossy(&list.stdout)
            .lines()
            .collect::<Vec<_>>()
            == ["first_name", "second_name"],
        "list must print declared names only"
    );
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );
    assert!(
        !output.contains(FIRST_VALUE),
        "list output leaked a secret value"
    );
    assert!(
        !output.contains(SECOND_VALUE),
        "list output leaked a secret value"
    );
}

#[test]
fn secrets_import_dotenv_preserves_source_and_reports_names() {
    let proj = Scratch::new("import-proj");
    let keys = Scratch::new("import-keys");
    setup_vault(&proj, &keys);
    const API_VALUE: &str = "import-api-value-never-printed";
    const TOKEN_VALUE: &str = "import-token-value-never-printed";
    const SESSION_VALUE: &str = "import-session-value-never-printed";
    let source = format!(
        "# source remains unchanged\nAPI_URL={API_VALUE}\nIMPORTED_TOKEN=\"{TOKEN_VALUE}\"\nexport SESSION_KEY={SESSION_VALUE}\n"
    );
    let env_file = proj.path.join(".env");
    fs::write(&env_file, &source).unwrap();
    let before = fs::read(&env_file).unwrap();

    let import = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "import"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("import .env secrets");
    assert!(import.status.success(), "dotenv import failed");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&import.stdout),
        strip_ansi(&String::from_utf8_lossy(&import.stderr))
    );
    for name in ["API_URL", "IMPORTED_TOKEN", "SESSION_KEY"] {
        assert!(output.contains(name), "import output omitted a secret name");
    }
    assert!(
        output.contains("left `.env` untouched"),
        "import omitted source advice"
    );
    assert!(
        output.contains("remove it"),
        "import omitted cleanup advice"
    );
    assert!(
        !output.contains(API_VALUE),
        "import output leaked a secret value"
    );
    assert!(
        !output.contains(TOKEN_VALUE),
        "import output leaked a secret value"
    );
    assert!(
        !output.contains(SESSION_VALUE),
        "import output leaked a secret value"
    );
    assert!(
        fs::read(&env_file).unwrap() == before,
        "import changed the source file"
    );

    let get = jetpack()
        .current_dir(&proj.path)
        .args(["secrets", "get", "IMPORTED_TOKEN"])
        .env("JET_KEYS_DIR", keys.path.to_str().unwrap())
        .output()
        .expect("read imported secret");
    assert!(get.status.success(), "imported secret was not stored");
    assert!(
        String::from_utf8_lossy(&get.stdout).trim() == TOKEN_VALUE,
        "imported secret value mismatch"
    );
}

/// `jetpack secrets get <name>` on a name that isn't in the store is a clean
/// E1263, not a hang or a panic.
#[test]
fn secrets_get_missing_entry_is_e1263() {
    let proj = Scratch::new("missing-proj");
    let keys = Scratch::new("missing-keys");

    // Set up a store with one entry so the "missing" case is a real lookup
    // miss, not just an absent store.
    setup_vault(&proj, &keys);
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
