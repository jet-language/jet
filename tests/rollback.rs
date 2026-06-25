//! D-TXN-ROLLBACK layer 2: the `Rollback` trait — custom snapshot/restore path.

use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn unique_tmp(tag: &str) -> std::path::PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("jet_rollback_{tag}_{}_{}", std::process::id(), n))
}

fn have_rustc() -> bool {
    Command::new("rustc").arg("--version").output().is_ok()
}

fn build_and_run(name: &str, src: &str) -> (i32, String) {
    let dir = unique_tmp(name);
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join(format!("{name}.jet"));
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown).unwrap_or_else(|diags| {
        panic!(
            "front end rejected:\n{}",
            jet::render_diagnostics(&shown, src, &diags)
        )
    });
    let rs = dir.join(format!("{name}.rs"));
    let bin = dir.join(name);
    fs::write(&rs, &out.rust).unwrap();
    let rustc = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc rejected generated code (I2 violation):\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let run = Command::new(&bin).output().unwrap();
    (run.status.code().unwrap_or(0), String::from_utf8_lossy(&run.stdout).into_owned())
}

/// On `?`-failure both fields are restored to their pre-block values via the
/// custom `restore` method (not a full clone).
#[test]
fn rollback_restores_on_failure() {
    if !have_rustc() { return; }
    let src = r#"
struct Counter {
    value: Int
    ops: Int
}
struct CounterSnap {
    value: Int
    ops: Int
}
impl Counter: Rollback {
    type Snapshot = CounterSnap
    fn snapshot(self) -> CounterSnap {
        return CounterSnap { value: self.value, ops: self.ops }
    }
    fn restore(~self, snap: ^CounterSnap) {
        self.value = snap.value
        self.ops = snap.ops
    }
}
enum Fail { Bad }
fn bump(c: ~Counter) -> Int ? Fail {
    #Transact {
        c.value += 1
        c.ops += 1
        return err(Fail.Bad)
    }
    return ok(c.value)
}
fn main() {
    c := Counter { value: 10, ops: 0 }
    _ @= bump(~c) ?? 0
    print(c.value)
    print(c.ops)
}
"#;
    let (code, stdout) = build_and_run("rollback_failure", src);
    assert_eq!(code, 0);
    // Both fields restored to 10 and 0.
    assert_eq!(stdout, "10\n0\n");
}

/// On success the changes are committed: rollback must NOT run.
#[test]
fn rollback_not_triggered_on_success() {
    if !have_rustc() { return; }
    let src = r#"
struct Counter {
    value: Int
    ops: Int
}
struct CounterSnap {
    value: Int
    ops: Int
}
impl Counter: Rollback {
    type Snapshot = CounterSnap
    fn snapshot(self) -> CounterSnap {
        return CounterSnap { value: self.value, ops: self.ops }
    }
    fn restore(~self, snap: ^CounterSnap) {
        self.value = snap.value
        self.ops = snap.ops
    }
}
enum Fail { Bad }
fn bump(c: ~Counter) -> Int ? Fail {
    #Transact {
        c.value += 1
        c.ops += 1
    }
    return ok(c.value)
}
fn main() {
    c := Counter { value: 10, ops: 0 }
    _ @= bump(~c) ?? 0
    print(c.value)
    print(c.ops)
}
"#;
    let (code, stdout) = build_and_run("rollback_success", src);
    assert_eq!(code, 0);
    // Changes committed: 11 and 1.
    assert_eq!(stdout, "11\n1\n");
}

/// Generated Rust contains `snapshot_custom` and `trait user_Rollback`.
/// `unsafe` must not leak outside `mod jet_txn`.
#[test]
fn snapshot_custom_in_codegen() {
    let src = r#"
struct Counter {
    value: Int
}
struct CounterSnap { value: Int }
impl Counter: Rollback {
    type Snapshot = CounterSnap
    fn snapshot(self) -> CounterSnap {
        return CounterSnap { value: self.value }
    }
    fn restore(~self, snap: ^CounterSnap) {
        self.value = snap.value
    }
}
enum Fail { Bad }
fn bump(c: ~Counter) -> Int ? Fail {
    #Transact {
        c.value += 1
        return err(Fail.Bad)
    }
    return ok(c.value)
}
fn main() {
    c := Counter { value: 0 }
    _ @= bump(~c) ?? 0
    print(c.value)
}
"#;
    let dir = unique_tmp("codegen");
    fs::create_dir_all(&dir).unwrap();
    let jet_path = dir.join("snap_codegen.jet");
    fs::write(&jet_path, src).unwrap();
    let shown = jet_path.to_string_lossy().into_owned();
    let out = jet::compile_with_path(src, &shown)
        .unwrap_or_else(|diags| panic!("front end rejected: {:?}", diags));
    let rust = &out.rust;

    assert!(
        rust.contains("snapshot_custom"),
        "expected snapshot_custom in generated Rust:\n{rust}"
    );
    assert!(
        rust.contains("trait user_Rollback"),
        "expected trait user_Rollback in generated Rust:\n{rust}"
    );

    // Strip `mod jet_txn { … }` and `mod jet_mem { … }` then verify no `unsafe` remains.
    fn strip_mod(src: &str, name: &str) -> String {
        let marker = format!("mod {name}");
        let Some(start) = src.find(&marker) else { return src.to_string(); };
        let bytes = src.as_bytes();
        let (mut depth, mut i, mut end, mut seen) = (0usize, start, src.len(), false);
        while i < bytes.len() {
            match bytes[i] {
                b'{' => { depth += 1; seen = true; }
                b'}' => {
                    if seen { depth -= 1; }
                    if seen && depth == 0 { end = i + 1; break; }
                }
                _ => {}
            }
            i += 1;
        }
        let mut out = src[..start].to_string();
        out.push_str(&src[end..]);
        out
    }

    let user_code = strip_mod(rust, "jet_txn");
    let user_code = strip_mod(&user_code, "jet_mem");
    assert!(
        !user_code.contains("unsafe"),
        "unsafe leaked outside jet_txn/jet_mem:\n{user_code}"
    );
}
