use std::io;
use std::path::{Path, PathBuf};

/// The one domain used by the resident and private compiler artifacts.
pub const COMPILER_DOMAIN: &str = "jet.compiler.v2";
/// Canonical compiler inputs. Build.rs and private emitted-artifact harnesses
/// use this same root set; adding a root is an identity-scope change.
pub const COMPILER_SOURCES: &[&str] = &[
    "Cargo.toml",
    "Compiler",
    "build.rs",
    // jet-sema embeds the Core module bodies from here.
    "Core",
    "Source",
    "crates/jet-foundation",
    "crates/jet-unicode",
    "crates/jet-lexer",
    "crates/jet-parser",
    "crates/jet-net",
    "crates/jet-comptime",
    "crates/jet-sema",
    "crates/jet-codegen",
    "crates/jet-driver",
    "crates/jet-pkg-model",
    "crates/jet-env-model",
    "crates/jet-nix-eval",
    "crates/jetpack",
    "crates/jet-queries",
    "crates/jet-semindex",
    "crates/jet-impact",
    "crates/jet-jit",
    "crates/jet-rt",
    "crates/jet-repl",
    "crates/jet-debug",
    "crates/jet-cli",
    "crates/jet-canvas",
    "crates/jet-devserver",
    "Docs/spec/reference/core-library.md",
    "Docs/spec/diagnostics.md",
    "Examples/features/collections/wordcount.jet",
    "tests/fixtures/nix-compat/oracle.json",
];

/// Profile/toolchain overrides that alter compiler semantics and therefore feed
/// the same identity framing as source bytes and build facts.
pub fn profile_override_keys() -> Vec<String> {
    let mut keys = vec![
        "RUSTC_WRAPPER".to_string(),
        "RUSTC_WORKSPACE_WRAPPER".to_string(),
    ];
    for profile in ["RELEASE", "DEV", "TEST", "BENCH"] {
        for setting in [
            "PANIC",
            "OVERFLOW_CHECKS",
            "DEBUG_ASSERTIONS",
            "LTO",
            "CODEGEN_UNITS",
            "OPT_LEVEL",
            "DEBUG",
            "STRIP",
            "INCREMENTAL",
        ] {
            keys.push(format!("CARGO_PROFILE_{profile}_{setting}"));
        }
    }
    keys
}

/// A TARGET naming a custom `.json` target spec carries semantics in the file
/// body, not the name.
pub fn target_spec_path() -> Option<PathBuf> {
    let target = std::env::var("TARGET").ok()?;
    let path = PathBuf::from(&target);
    (target.ends_with(".json") && path.is_file()).then_some(path)
}

/// Collect the exact build facts used by the canonical semantic identity.
pub fn build_facts() -> io::Result<Vec<(String, String)>> {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let output = std::process::Command::new(rustc)
        .args(["--version", "--verbose"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "rustc --version --verbose failed",
        ));
    }
    let mut facts = vec![(
        "rustc".into(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )];
    for key in [
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
    ] {
        facts.push((key.into(), std::env::var(key).unwrap_or_default()));
    }
    facts.extend(std::env::vars().filter(|(key, _)| key.starts_with("CARGO_FEATURE_")));
    for key in profile_override_keys() {
        facts.push((key.clone(), std::env::var(&key).unwrap_or_default()));
    }
    if let Some(spec) = target_spec_path() {
        facts.push(("TARGET_SPEC_JSON".into(), std::fs::read_to_string(spec)?));
    }
    facts.sort();
    Ok(facts)
}

/// Compute the canonical identity over files below `root` using the existing
/// domain/fact/path/length framing. This is shared by build.rs and private
/// bootstrap harnesses; callers do not invent a second hash policy.
pub fn semantic_id(
    root: &Path,
    domain: &str,
    roots: &[&str],
    facts: &[(String, String)],
) -> io::Result<String> {
    semantic_id_with_extra(root, domain, roots, facts, &[])
}

/// Identity of what the compiler sources mean, independent of how a host
/// binary was built. Build/cache receipts still use `COMPILER_DOMAIN` with
/// the complete build facts; only compiler-authored MIR uses this identity.
pub fn compiler_source_id(root: &Path) -> io::Result<String> {
    semantic_id(root, "jet.compiler-source.v1", COMPILER_SOURCES, &[])
}

/// Compute one canonical identity over rooted files plus explicit generated
/// inputs. The extra rows use the same sorted path/length framing rather than
/// a second artifact-specific hash convention.
pub fn semantic_id_with_extra(
    root: &Path,
    domain: &str,
    roots: &[&str],
    facts: &[(String, String)],
    extra: &[(String, Vec<u8>)],
) -> io::Result<String> {
    let mut paths = Vec::new();
    for path in roots {
        collect_files(root.join(path), &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    let mut inputs = Vec::with_capacity(paths.len() + extra.len());
    for path in paths {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        inputs.push((relative, std::fs::read(path)?));
    }
    inputs.extend(extra.iter().cloned());
    Ok(semantic_id_from_inputs(domain, facts, &inputs))
}

/// Apply the same canonical framing to explicit in-memory inputs. Private
/// bootstrap stages use this for their generated source plus path-linked
/// Host/Runner inputs before handing the identity to their backend harness.
pub fn semantic_id_from_inputs(
    domain: &str,
    facts: &[(String, String)],
    inputs: &[(String, Vec<u8>)],
) -> String {
    let mut files = inputs.to_vec();
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files.dedup_by(|left, right| left.0 == right.0);

    let mut hash = crate::SHA256::StreamingSha256::new();
    hash.update(domain.as_bytes());
    hash.update(&[0]);
    for (key, value) in facts {
        hash.update(&(key.len() as u64).to_be_bytes());
        hash.update(key.as_bytes());
        hash.update(&(value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    for (path, bytes) in files {
        hash.update(&(path.len() as u64).to_be_bytes());
        hash.update(path.as_bytes());
        hash.update(&(bytes.len() as u64).to_be_bytes());
        hash.update(&bytes);
    }
    hash.finalize()
        .iter()
        .fold(String::with_capacity(64), |mut text, byte| {
            use std::fmt::Write;
            let _ = write!(text, "{byte:02x}");
            text
        })
}

pub fn collect_files(path: PathBuf, files: &mut Vec<PathBuf>) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(&path)?;
    if metadata.is_file() {
        files.push(path);
    } else if metadata.is_dir() {
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                matches!(
                    name,
                    ".git"
                        | "target"
                        | "tests"
                        | "test"
                        | "testdata"
                        | "fixtures"
                        | "examples"
                        | "benches"
                        | "expected"
                )
            })
        {
            return Ok(());
        }
        let mut children = std::fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            collect_files(child.path(), files)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_identity_is_relocatable_and_build_identity_keeps_profiles() {
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let fixture = Fixture(std::env::temp_dir().join(format!(
            "jet-compiler-source-id-{}-{unique}",
            std::process::id()
        )));
        let left = fixture.0.join("left");
        let right = fixture.0.join("right");
        for root in [&left, &right] {
            for input in COMPILER_SOURCES {
                let path = root.join(input);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, input).unwrap();
            }
        }
        let source = compiler_source_id(&left).unwrap();
        assert_eq!(source, compiler_source_id(&right).unwrap());
        assert_eq!(
            source,
            semantic_id(&left, "jet.compiler-source.v1", COMPILER_SOURCES, &[]).unwrap()
        );
        let dev = vec![("PROFILE".to_string(), "debug".to_string())];
        let release = vec![("PROFILE".to_string(), "release".to_string())];
        assert_ne!(
            semantic_id(&left, COMPILER_DOMAIN, COMPILER_SOURCES, &dev).unwrap(),
            semantic_id(&left, COMPILER_DOMAIN, COMPILER_SOURCES, &release).unwrap()
        );
        std::fs::write(right.join("Compiler"), "changed compiler source").unwrap();
        assert_ne!(source, compiler_source_id(&right).unwrap());
        let extra = vec![("generated.rs".to_string(), b"generated compiler".to_vec())];
        assert_ne!(
            semantic_id(&left, COMPILER_DOMAIN, COMPILER_SOURCES, &dev).unwrap(),
            semantic_id_with_extra(&left, COMPILER_DOMAIN, COMPILER_SOURCES, &dev, &extra)
                .unwrap()
        );
    }
}
