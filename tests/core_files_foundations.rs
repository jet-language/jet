//! Source-owned `core.files` behavior across the applicable execution tiers.

mod common;
mod tir_support;

#[test]
fn core_files_copy_dir_does_not_follow_symlinked_entries() {
    let scratch = common::Scratch::new("core_files_copy_dir_symlinks");
    let fixture = scratch.path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.files as files

fn run() {
    root :: "__FIXTURE__"
    source :: files.join(root, "source")
    destination :: files.join(root, "destination")
    outside :: files.join(root, "outside.txt")
    linked :: files.join(source, "linked.txt")
    copied_link :: files.join(destination, "linked.txt")
    regular :: files.join(source, "regular.txt")
    copied_regular :: files.join(destination, "regular.txt")

    files.create_dir(source) ?? panic("source")
    files.write(outside, "do not copy") ?? panic("outside")
    files.write(regular, "keep") ?? panic("regular")
    files.symlink(outside, linked) ?? panic("symlink")
    files.copy_dir(source, destination) ?? panic("copy_dir")

    print(files.read(copied_regular) ?? "missing")
    print(files.lexists(copied_link))
    print(files.read(copied_link) == .Err(_))
    files.remove_all(root) ?? panic("cleanup")
}
"#
    .replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree("core_files_foundations", &source, "keep\nfalse\ntrue\n");
}

#[test]
fn core_files_read_bytes_plain_iteration_preserves_unsigned_bytes_and_empty_input() {
    let scratch = common::Scratch::new("core_files_byte_iteration");
    let fixture_root = scratch.path.join("fixture");
    std::fs::create_dir(&fixture_root).expect("create byte iteration fixture root");
    std::fs::write(fixture_root.join("bytes.bin"), [0_u8, 1, 127, 128, 255])
        .expect("write byte iteration fixture");
    std::fs::write(fixture_root.join("empty.bin"), b"").expect("write empty byte fixture");
    let fixture = fixture_root
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = r#"
use core.files as files

fn run() {
    root :: "__FIXTURE__"
    bytes_path :: " {files.join(root, "bytes.bin")} "
    empty_path :: files.join(root, "empty.bin")
    bytes :: files.read_bytes(bytes_path.trim()) ?? panic("read bytes")
    loop byte in bytes {
        print(byte)
    }
    print(bytes.len())
    empty :: files.read_bytes(empty_path) ?? panic("read empty bytes")
    loop byte in empty {
        print(byte)
    }
    print(empty.len())
    missing_path :: " {files.join(root, "missing.bin")} "
    print(files.read_bytes(missing_path.trim()) == .Err(_))
    loop value in [7, 13] {
        print(value)
    }
}
"#
    .replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree(
        "core_files_byte_iteration",
        &source,
        "0\n1\n127\n128\n255\n5\n0\ntrue\n7\n13\n",
    );
}


#[test]
fn core_files_canonicalize_resolves_symlinks_and_rejects_missing_paths() {
    let scratch = common::Scratch::new("core_files_canonicalize");

    let fixture_path =
        std::fs::canonicalize(&scratch.path).expect("canonicalize test scratch path");
    let fixture = fixture_path
        .join("fixture")
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = r#"
use core.files as files

fn run() {
    root :: "__FIXTURE__"
    target_dir :: files.join(root, "target-dir")
    target :: files.join(target_dir, "target.txt")
    linked :: files.join(root, "linked.txt")
    missing :: files.join(target_dir, "missing.txt")

    files.create_dir_all(target_dir) ?? panic("target dir")
    files.write(target, "canonical target") ?? panic("target")
    files.symlink(target, linked) ?? panic("symlink")

    canonical_target :: files.canonicalize(target) ?? panic("target canonicalize")
    canonical_link :: files.canonicalize(linked) ?? panic("symlink canonicalize")
    print(files.is_abs(canonical_target))
    print(canonical_target == canonical_link)
    print(files.canonicalize(missing) == .Err(_))

    files.remove(linked) ?? panic("cleanup symlink")

    files.remove_all(root) ?? panic("cleanup")
}
"#
    .replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree("core_files_canonicalize", &source, "true\ntrue\ntrue\n");
}

#[cfg(unix)]
#[test]
fn core_files_directory_listing_preserves_invalid_utf8_basename_identity() {
    use std::os::unix::ffi::OsStrExt;
    let scratch = common::Scratch::new("core_files_raw_names");
    let fixture_root = scratch.path.join("names");
    std::fs::create_dir(&fixture_root).expect("create raw-name fixture");
    for name in [b"a\xff".as_slice(), b"a\x80".as_slice()] {
        std::fs::write(fixture_root.join(std::ffi::OsStr::from_bytes(name)), b"")
            .expect("create native non-Unicode basename");
    }
    let fixture = fixture_root.to_str().expect("Unicode fixture root")
        .replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.files as files

fn verify_names() {
    directory :: files.open_directory("__FIXTURE__")
    entries :: directory.list()
    assert(entries.len() == 2)
    first :: entries[0]
    second :: entries[1]
    assert(first.raw_name == [U8]{97, 128})
    assert(second.raw_name == [U8]{97, 255})
    assert(!first.name_is_unicode && !second.name_is_unicode)
    assert(first.name == "a�" && second.name == "a�")
    assert(first.path == second.path)
    assert(first.raw_name != second.raw_name)
    assert(!first.is_dir && !second.is_dir)
    assert(directory.list().len() == 2)
}

#Test("non-Unicode native basenames retain distinct raw identities") {
    verify_names()
}

fn run() {
    verify_names()
    print("distinct raw identities, equal display names: true")
}
"#.replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree(
        "core_files_raw_names",
        &source,
        "distinct raw identities, equal display names: true\n",
    );
    // The scratch root already owns a `package.jet`; the claim's authority
    // lives there rather than in a second, inline Package context (E1363).
    std::fs::write(
        scratch.join("package.jet"),
        "name: \"core_files_raw_names_claims\"\nversion: \"0.1.0\"\nauthority: { holds: { allow: [FS.Read, IO, Mem.Alloc] } }\n",
    ).expect("write native basename Jet claims manifest");
    let claim_source = scratch.join("run.jet");
    std::fs::write(&claim_source, &source).expect("write native basename Jet claims");
    check_jet_claims(&claim_source, 1);
}

fn check_core_files_golden(stem: &str) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Examples/features");
    let expected = std::fs::read_to_string(root.join("expected/io").join(format!("{stem}.out")))
        .expect("read core.files golden output");
    tir_support::assert_example_cli_tiers_agree(&format!("io/{stem}"), &expected);
}

#[test]
fn core_files_golden_files() { check_core_files_golden("files"); }
#[test]
fn core_files_golden_depth() { check_core_files_golden("files_depth"); }
#[test]
fn core_files_golden_set_mode() { check_core_files_golden("files_set_mode"); }
#[test]
fn core_files_golden_store_primitives() { check_core_files_golden("files_store_primitives"); }
#[test]
fn core_files_golden_handle_io() { check_core_files_golden("files_handle_io"); }
#[test]
fn core_files_golden_descriptor_transactions() { check_core_files_golden("files_descriptor_transactions"); }
#[test]
fn core_files_golden_walk_files() { check_core_files_golden("walk_files"); }

fn check_jet_claims(source: &std::path::Path, count: usize) {
    for release in [false, true] {
        let scratch = common::Scratch::new("core_files_jet_claims");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_jet"));
        command.args(["test", "--show-default", "--capture=all", "--serial", "--fresh"]);
        if release {
            command.arg("--release");
        }
        let output = command.arg(source)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("JET_STORE_DIR", scratch.join("cache"))
            .env("JETPACK_ROOT", scratch.join("jetpack"))
            .env("JETPACK_ENV", "1")
            .env("NO_COLOR", "1")
            .output().expect("execute Jet file descriptor claims");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "Jet claims failed (release={release}):\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr));
        assert!(stdout.contains(&format!("{count} passed, 0 failed")),
            "Jet claims were not all exercised (release={release}):\n{stdout}");
    }
}

#[test]
fn core_files_descriptor_jet_claims() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("Examples/features/io/files_descriptor_transactions.jet");
    check_jet_claims(&source, 4);
}

#[cfg(unix)]
#[test]
fn core_files_metadata_predicates_follow_links_and_keep_errors_false() {
    let scratch = common::Scratch::new("core_files_metadata_predicates");
    let root = scratch.path.join("fixture");
    std::fs::create_dir(&root).expect("create metadata fixture");
    std::fs::write(root.join("file"), b"contents").expect("create regular file");
    std::os::unix::fs::symlink(&root, root.join("directory-link")).expect("link directory");
    std::os::unix::fs::symlink(root.join("file"), root.join("file-link")).expect("link file");
    std::os::unix::fs::symlink(root.join("missing"), root.join("broken-link")).expect("create broken link");
    let fixture = root.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let source = r#"
use core.files as files

fn run() {
    root :: "__FIXTURE__"
    print(files.exists(root))
    print(files.is_dir(root))
    print(files.exists(files.join(root, "file")))
    print(files.is_dir(files.join(root, "file")))
    print(files.exists(files.join(root, "directory-link")))
    print(files.is_dir(files.join(root, "directory-link")))
    print(files.exists(files.join(root, "file-link")))
    print(files.is_dir(files.join(root, "file-link")))
    print(files.exists(files.join(root, "missing")))
    print(files.is_dir(files.join(root, "missing")))
    print(files.exists(files.join(root, "broken-link")))
    print(files.is_dir(files.join(root, "broken-link")))
    print(files.exists(files.join(root, "file/child")))
    print(files.is_dir(files.join(root, "file/child")))
}
"#.replace("__FIXTURE__", &fixture);
    tir_support::assert_tiers_agree(
        "core_files_metadata_predicates",
        &source,
        "true\ntrue\ntrue\nfalse\ntrue\ntrue\ntrue\nfalse\nfalse\nfalse\nfalse\nfalse\nfalse\nfalse\n",
    );
}

#[cfg(unix)]
#[test]
fn core_files_absolute_preserves_host_lexical_paths_without_collapsing() {
    let source = r#"
use core.files as files
use core.files.path as path
use core.sys as sys

fn run() {
    cwd :: sys.current_dir() ?? panic("current_dir")
    print((files.absolute("/absent//a/../b") ?? panic("absolute")) == "/absent//a/../b")
    print((files.absolute("./a//../b") ?? panic("relative")) == "{cwd}/./a//../b")
    print((files.absolute("C:\\absent") ?? panic("drive text")) == "{cwd}/C:\\absent")
    print((files.absolute("a\\b") ?? panic("backslash")) == "{cwd}/a\\b")
    print((files.absolute(path.of("/absent/./child")) ?? panic("Path")) == "/absent/./child")
}
"#;
    tir_support::assert_tiers_agree(
        "core_files_absolute",
        source,
        "true\ntrue\ntrue\ntrue\ntrue\n",
    );
}

#[test]
fn core_files_mktemp_composes_scalar_prefix_pid_and_clock_without_creating() {
    let source = r#"
use core.files as files
use core.sys as sys

fn run() {
    generated :: files.mktemp("Aé🌍-/")
    pid :: sys.getpid()
    print(files.basename(~generated).starts_with("A_____{pid}_"))
    print(files.is_absolute(~generated))
    print(!files.exists(generated))
}
"#;
    tir_support::assert_tiers_agree("core_files_mktemp", source, "true\ntrue\ntrue\n");
    tir_support::assert_release_tiers_agree("core_files_mktemp_release", source, "true\ntrue\ntrue\n");
}


