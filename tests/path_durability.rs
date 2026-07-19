mod common;

fn jet_string(path: &std::path::Path) -> String {
    path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"")
}

fn unique_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "jet-path-durability-{name}-{}",
        std::process::id()
    ))
}

#[test]
fn normalize_is_lexical_rooted_and_platform_native() {
    if !common::have_rustc() {
        return;
    }
    let absolute = if cfg!(windows) {
        "C:/../../alpha/../beta"
    } else {
        "/../../alpha/../beta"
    };
    let root = if cfg!(windows) { "C:/" } else { "/" };
    let windows_cases = if cfg!(windows) {
        r#"
    print(path.normalize("C:../../leaf"))
    print(path.normalize("//server/share/../../leaf"))"#
    } else {
        ""
    };
    let src = format!(
        r#"
use core.path as path

fn run() {{
    print(path.normalize("../../alpha/./beta/../gamma"))
    print(path.normalize("{absolute}"))
    print(path.normalize(""))
    print(path.normalize("{root}")){windows_cases}
}}
"#
    );
    let (code, stdout, stderr) = common::build_and_run("jet_path_durability", "normalize", &src);
    assert_eq!(code, 0, "generated program failed: {stderr}");
    let sep = std::path::MAIN_SEPARATOR;
    let expected_relative = format!("..{sep}..{sep}alpha{sep}gamma");
    let expected_absolute = if cfg!(windows) {
        format!("C:{sep}beta")
    } else {
        format!("{sep}beta")
    };
    let expected_root = if cfg!(windows) {
        format!("C:{sep}")
    } else {
        sep.to_string()
    };
    let mut expected = vec![expected_relative, expected_absolute, String::new(), expected_root];
    if cfg!(windows) {
        expected.push(format!("C:..{sep}..{sep}leaf"));
        expected.push(format!(r"\\server{sep}share{sep}leaf"));
    }
    assert_eq!(stdout, format!("{}\n", expected.join("\n")));
}

#[test]
fn generated_normalizer_uses_platform_components() {
    let src = "use core.path as path\n\nfn run() { print(path.normalize(\"a/../b\")) }\n";
    let out = jet::compile(src).expect("path normalize fixture should compile");
    assert!(out.rust.contains("std::path::Component::Prefix"));
    assert!(out.rust.contains("std::path::Component::RootDir"));
    assert!(out.rust.contains("std::path::PathBuf::new()"));
    assert!(!out.rust.contains("s.split('/')"));
}

#[test]
fn generated_atomic_write_has_durable_replace_contract() {
    let src = r#"
use core.files as fs

fn run() {
    bytes: [U8] :: [1, 2, 3]
    fs.write_atomic("atomic.bin", bytes) ?? panic("atomic write failed")
}
"#;
    let out = jet::compile(src).expect("atomic-write fixture should compile");
    for required in [
        ".create_new(true)",
        ".write_all(content)",
        ".sync_all()",
        "struct JetAtomicTemp",
        "impl Drop for JetAtomicTemp",
        "jet_atomic_sync_parent",
        "ReplaceFileW",
        "MoveFileExW",
        "encode_wide()",
        "encoded.contains(&0)",
        "std::io::ErrorKind::InvalidInput",
    ] {
        assert!(out.rust.contains(required), "missing atomic-write contract: {required}");
    }
}

#[cfg(windows)]
#[test]
fn windows_atomic_replace_rejects_embedded_nul_without_touching_prefix_target() {
    use std::process::Command;

    if !common::have_rustc() {
        return;
    }
    let src = r#"
use core.files as fs

fn run() {
    bytes: [U8] :: [1]
    fs.write_atomic("atomic.bin", bytes) ?? panic("atomic write failed")
}
"#;
    let mut rust = jet::compile(src).expect("atomic-write fixture should compile").rust;
    rust = rust.replacen("fn main()", "fn jet_original_main()", 1);
    rust.push_str(
        r#"
fn main() {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let root = std::env::temp_dir().join(format!("jet-path-nul-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("prefix-target.bin");
    let temp = root.join("temp.bin");
    std::fs::write(&target, b"old").unwrap();
    std::fs::write(&temp, b"new").unwrap();

    let invalid = |path: &std::path::Path| {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        units.push(0);
        units.extend("suffix".encode_utf16());
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&units))
    };
    let target_error = jet_atomic_windows::replace(&temp, &invalid(&target)).unwrap_err();
    assert_eq!(target_error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    let temp_error = jet_atomic_windows::replace(&invalid(&temp), &target).unwrap_err();
    assert_eq!(temp_error.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(root);
}
"#,
    );
    let dir = unique_dir("windows-nul");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let rs = dir.join("main.rs");
    let bin = dir.join("main.exe");
    std::fs::write(&rs, rust).unwrap();
    let built = Command::new("rustc")
        .args(["--edition", "2021", rs.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(built.status.success(), "rustc failed: {}", String::from_utf8_lossy(&built.stderr));
    let ran = Command::new(&bin).output().unwrap();
    assert!(ran.status.success(), "NUL regression failed: {}", String::from_utf8_lossy(&ran.stderr));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn atomic_write_retries_collision_replaces_and_cleans_failed_temp() {
    if !common::have_rustc() {
        return;
    }
    let root = unique_dir("hostile");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("target.bin");
    let blocked = root.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(&target, b"old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let src = format!(
        r#"
use core.files as fs
use core.os as os
use core.path as path

fn run() {{
    root :: "{}"
    stale :: path.join(root, ".jet_tmp_{{os.pid()}}_0")
    fs.write(stale, "stale") ?? panic("stale setup failed")
    bytes: [U8] :: [110, 101, 119]
    fs.write_atomic("{}", bytes) ?? panic("replacement failed")
    fs.write_atomic("{}", bytes) ?? panic("expected replacement failure")
}}
"#,
        jet_string(&root),
        jet_string(&target),
        jet_string(&blocked),
    );
    let (code, _stdout, _stderr) =
        common::build_and_run("jet_path_durability", "atomic_hostile", &src);
    assert_eq!(code, 70, "directory replacement must fail through Jet panic");
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
    }
    let debris: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(".jet_tmp_"))
        .collect();
    assert_eq!(debris.len(), 1, "only the forced stale collision may remain");
    assert_eq!(std::fs::read(debris[0].path()).unwrap(), b"stale");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn concurrent_atomic_writers_leave_one_whole_payload() {
    if !common::have_rustc() {
        return;
    }
    let root = unique_dir("concurrent");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let target = root.join("target.bin");
    std::fs::write(&target, b"old").unwrap();
    let a = vec![65u8; 257];
    let b = vec![66u8; 263];
    let a_text = "A".repeat(a.len());
    let b_text = "B".repeat(b.len());
    let list = |bytes: &[u8]| bytes.iter().map(u8::to_string).collect::<Vec<_>>().join(", ");
    let src = format!(
        r#"
use core.files as fs

fn write_a() -> Int {{
    bytes: [U8] :: [{}]
    loop _; 1..25 {{
        fs.write_atomic("{}", bytes) ?? panic("writer a failed")
    }}
    return 1
}}
fn write_b() -> Int {{
    bytes: [U8] :: [{}]
    loop _; 1..25 {{
        fs.write_atomic("{}", bytes) ?? panic("writer b failed")
    }}
    return 2
}}
fn observe() -> Int {{
    loop _; 1..100 {{
        value :: fs.read("{}") ?? panic("observer read failed")
        if value != "old" && value != "{}" && value != "{}" {{
            panic("observer saw torn atomic-write payload")
        }}
    }}
    return 3
}}
fn run() {{
    taskgroup g {{
        a :: g.task(() => write_a())
        b :: g.task(() => write_b())
        observer :: g.task(() => observe())
        g.all([a, b, observer])
    }}
}}
"#,
        list(&a),
        jet_string(&target),
        list(&b),
        jet_string(&target),
        jet_string(&target),
        a_text,
        b_text,
    );
    let (code, _stdout, stderr) =
        common::build_and_run("jet_path_durability", "atomic_concurrent", &src);
    assert_eq!(code, 0, "concurrent atomic writers failed: {stderr}");
    let final_bytes = std::fs::read(&target).unwrap();
    assert!(final_bytes == a || final_bytes == b, "observed torn atomic-write payload");
    let debris = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with(".jet_tmp_"));
    assert!(!debris, "successful concurrent writes left temporary debris");
    let _ = std::fs::remove_dir_all(root);
}
