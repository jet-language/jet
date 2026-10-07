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
    for name in [b"a\x80".as_slice(), b"a\xff".as_slice()] {
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
}

#[test]
fn core_files_goldens_match_across_tiers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Examples/features");
    for stem in [
        "files", "files_depth", "files_set_mode", "files_store_primitives",
        "files_handle_io", "files_descriptor_transactions", "walk_files",
    ] {
        let source = std::fs::read_to_string(root.join("io").join(format!("{stem}.jet")))
            .expect("read core.files golden source");
        let expected = std::fs::read_to_string(root.join("expected/io").join(format!("{stem}.out")))
            .expect("read core.files golden output");
        tir_support::assert_tiers_agree(stem, &source, &expected);
    }
}
