use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let batteries_dir = manifest_dir.join("batteries");
    println!("cargo:rerun-if-changed={}", batteries_dir.display());

    let mut packages = Vec::new();
    if batteries_dir.is_dir() {
        let mut entries = fs::read_dir(&batteries_dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", batteries_dir.display()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", batteries_dir.display()));
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
            if metadata.file_type().is_symlink() {
                panic!("battery registry cannot include symlink {}", path.display());
            }
            if !metadata.is_dir() {
                panic!("battery registry entry is not a directory: {}", path.display());
            }
            let slug = entry
                .file_name()
                .into_string()
                .unwrap_or_else(|_| panic!("battery directory name is not UTF-8: {}", path.display()));
            if slug.is_empty() || slug == "." || slug == ".." || !is_safe_component(&slug) {
                panic!("battery directory name is unsafe: {slug:?}");
            }
            println!("cargo:rerun-if-changed={}", path.display());
            let files = collect_files(&path, &path);
            if !files.iter().any(|(relative, _, _)| relative == "package.jet") {
                panic!("battery {slug} is missing package.jet");
            }
            let canonical = format!("{slug}.jet");
            if !files.iter().any(|(relative, _, _)| relative == &canonical) {
                panic!("battery {slug} is missing canonical {canonical}");
            }
            packages.push((slug, files));
        }
    }

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let generated = out_dir.join("batteries_registry.rs");
    let mut source = String::from("pub(crate) static PACKAGES: &[EmbeddedPackage] = &[\n");
    for (slug, files) in &packages {
        source.push_str("    EmbeddedPackage { slug: ");
        source.push_str(&rust_string(slug));
        source.push_str(", files: &[\n");
        for (relative, _, mode) in files {
            source.push_str("        EmbeddedFile { path: ");
            source.push_str(&rust_string(relative));
            source.push_str(", mode: ");
            source.push_str(&format!("0o{mode:o}"));
            source.push_str(", bytes: include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), ");
            source.push_str(&rust_string(&format!("/batteries/{slug}/{relative}")));
            source.push_str(")), },\n");
        }
        source.push_str("    ] },\n");
    }
    source.push_str("];\n");
    fs::write(&generated, source)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", generated.display()));
}

fn collect_files(root: &Path, current: &Path) -> Vec<(String, Vec<u8>, u32)> {
    let mut entries = fs::read_dir(current)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", current.display()))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", current.display()));
    entries.sort_by_key(|entry| entry.file_name());
    let mut files = Vec::new();
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
        if metadata.file_type().is_symlink() {
            panic!("battery registry cannot include symlink {}", path.display());
        }
        if metadata.is_dir() {
            files.extend(collect_files(root, &path));
            continue;
        }
        if !metadata.is_file() {
            panic!("battery registry cannot include non-file {}", path.display());
        }
        let relative = path
            .strip_prefix(root)
            .expect("collected path must be below package root")
            .components()
            .map(|component| match component {
                std::path::Component::Normal(name) => name
                    .to_str()
                    .unwrap_or_else(|| panic!("battery path is not UTF-8: {}", path.display()))
                    .to_string(),
                _ => panic!("battery path is not a normal relative path: {}", path.display()),
            })
            .collect::<Vec<_>>()
            .join("/");
        println!("cargo:rerun-if-changed={}", path.display());
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o7777
        };
        #[cfg(not(unix))]
        let mode = 0o644;
        let bytes = fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        files.push((relative, bytes, mode));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn is_safe_component(value: &str) -> bool {
    !value.chars().any(|character| character.is_control())
        && !value.contains(['/', '\\'])
}

fn rust_string(value: &str) -> String {
    format!("{:?}", value)
}
