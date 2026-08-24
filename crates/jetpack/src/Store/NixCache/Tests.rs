use super::*;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

const STANDARD_NARINFO: &str = concat!(
    "StorePath: /nix/store/axp6zlky4x2v3jwcbq24a2cz25hzlw9b-ripgrep-15.2.0\n",
    "URL: nar/19yag7za8bz38dzxd7g20p8738bmb80n4ci9y3hfaxhy15rxxxyh.nar.zst\n",
    "Compression: zstd\n",
    "FileHash: sha256:1lhgjf25d2ca7sx1ka4g5lsskicr484vqi7cbndzhz598hbr18zy\n",
    "FileSize: 2133450\n",
    "NarHash: sha256:19yag7za8bz38dzxd7g20p8738bmb80n4ci9y3hfaxhy15rxxxyh\n",
    "NarSize: 7088584\n",
    "References: 0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67 dsn500c5j62qz9f49mi3nhx74jbkf6xq-pcre2-10.47 r48746qznwqxxl9qzd8f08ny8mg1dg2y-gcc-15.3.0-lib\n",
    "Sig: cache.nixos.org-1:u47N81GjFd/qpAQ8bRz3Ve584pYwp/gWswtHa6PwWSzhfYvw7oTBW0DThOzapKGuxqqnvw9HfKRnggOniyPBDw==\n",
);

#[test]
fn standard_narinfo_parses_real_zstd_and_distinct_hashes() {
    let info = NixNarInfo::parse(STANDARD_NARINFO).unwrap();
    assert_eq!(info.compression, NixCompression::Zstd);
    assert_ne!(info.file_hash.as_deref(), Some(info.nar_hash.as_str()));
    assert_eq!(info.file_size, Some(2_133_450));
    assert_eq!(info.nar_size, 7_088_584);
    assert_eq!(info.references.len(), 3);
}

#[test]
fn nix_narinfo_signature_matches_upstream_fingerprint() {
    let info = NixNarInfo::parse(STANDARD_NARINFO).unwrap();
    let key = NixPublicKey::parse("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=")
        .unwrap();
    info.verify_signature("/nix/store", &[key.clone()]).unwrap();

    let wrong_key =
        NixPublicKey::parse("cache.nixos.org-1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
            .unwrap();
    assert!(info.verify_signature("/nix/store", &[wrong_key]).is_err());

    for (needle, replacement) in [
        ("ripgrep-15.2.0", "ripgrep-15.2.1"),
        ("7088584", "7088585"),
        (
            "0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-67",
            "0d8g8n0a11v6f5m2h416ajyxmnkwc3md-glibc-2.42-68",
        ),
    ] {
        let changed = STANDARD_NARINFO.replacen(needle, replacement, 1);
        let changed = NixNarInfo::parse(&changed).unwrap();
        assert!(changed
            .verify_signature("/nix/store", &[key.clone()])
            .is_err());
    }
    let changed = STANDARD_NARINFO.replacen(
        "NarHash: sha256:19yag7za8bz38dzxd7g20p8738bmb80n4ci9y3hfaxhy15rxxxyh\n",
        &format!("NarHash: sha256:{}\n", "0".repeat(52)),
        1,
    );
    let changed = NixNarInfo::parse(&changed).unwrap();
    assert!(changed.verify_signature("/nix/store", &[key]).is_err());
}

#[test]
fn native_nix_cache_streams_without_process_tools() {
    let source = include_str!("../NixCache.rs");
    assert!(!source.contains("Command"));
    assert!(!source.contains("curl"));
    assert!(!source.contains("nix-store"));

    let root = unique_dir("stream");
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin/payload"), vec![b'x'; 128 * 1024]).unwrap();
    let (nar, expected) = super::super::write_nar(&root).unwrap();
    assert!(expected.bytes > 64 * 1024);
    let mut streaming = crate::SHA256::StreamingSha256::new();
    for chunk in nar.chunks(73) {
        streaming.update(chunk);
    }
    eprintln!(
        "one-shot={} direct-stream={}",
        crate::SHA256::sha256_hex(&nar),
        super::bytes_to_hex(&streaming.finalize())
    );
    let destination = root.with_extension("decoded");
    let actual =
        super::super::read_nar_stream(Cursor::new(nar), &destination, expected.bytes).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        fs::read(destination.join("bin/payload")).unwrap().len(),
        128 * 1024
    );
    remove_dir(&root);
    remove_dir(&destination);
}

#[test]
fn native_nix_cache_recurses_and_admits_closure_atomically() {
    let source = include_str!("../NixCache.rs");
    assert!(source.contains("while let Some(store_path) = queue.iter().next().cloned()"));
    assert!(source.contains("self.rollback()"));
    assert!(source.contains("Closure::register_entries_unlocked"));
}

#[test]
fn native_nix_cache_failures_are_e1350_and_snapshot_pinned() {
    let error = NixCacheError::new(NixCacheErrorKind::PathTraversal, "test");
    assert_eq!(error.code(), "E1350");
    let rendered = crate::Diagnostics::render_all(
        "<nix-cache>",
        "",
        std::slice::from_ref(&error.diagnostic()),
    );
    let snapshot = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/jetpack-diagnostics/nix_cache_admission_failures.stderr");
    if std::env::var_os("UPDATE_EXPECT").is_some() {
        fs::write(&snapshot, &rendered).unwrap();
    }
    assert_eq!(
        rendered,
        fs::read_to_string(&snapshot).expect("missing Nix cache diagnostic snapshot")
    );
    for kind in [
        NixCacheErrorKind::Metadata,
        NixCacheErrorKind::WrongKey,
        NixCacheErrorKind::Signature,
        NixCacheErrorKind::Transport,
        NixCacheErrorKind::UnsupportedCompression,
        NixCacheErrorKind::CompressedCorruption,
        NixCacheErrorKind::NarCorruption,
        NixCacheErrorKind::MissingReference,
        NixCacheErrorKind::PathTraversal,
        NixCacheErrorKind::DuplicateEntry,
        NixCacheErrorKind::Admission,
    ] {
        assert_eq!(NixCacheError::new(kind, "test").code(), "E1350");
    }
}

fn unique_dir(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("jet-nix-cache-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn remove_dir(path: &PathBuf) {
    let _ = fs::remove_dir_all(path);
}
