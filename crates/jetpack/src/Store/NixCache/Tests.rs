use self::TestCache::{base64_encode, remove_dir, signed_narinfo, unique_dir, TestCacheServer};
use super::*;
use ed25519_dalek::SigningKey;
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

#[path = "TestCache.rs"]
mod TestCache;

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
fn standard_narinfo_defaults_to_bzip2_and_keeps_optional_hashes_optional() {
    let text = concat!(
        "StorePath: /nix/store/0123456789abcdfghijklmnpqrsvwxyz-tool\n",
        "URL: nar/tool.nar.xz?sha256=example\n",
        "NarHash: sha256:0000000000000000000000000000000000000000000000000000000000000000\n",
        "NarSize: 1\n",
    );
    let info = NixNarInfo::parse(text).unwrap();
    assert_eq!(info.compression, NixCompression::Bzip2);
    assert_eq!(info.file_hash, None);
    assert_eq!(info.file_size, None);
    assert!(NixNarInfo::parse(&text.replace("tool.nar.xz", "../tool.nar.xz")).is_err());
    assert!(NixNarInfo::parse(
        &text.replace(
            "NarSize: 1",
            "References: 0123456789abcdfghijklmnpqrsvwxyz-ref 0123456789abcdfghijklmnpqrsvwxyz-ref\nNarSize: 1",
        )
    )
    .is_err());
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
    assert_eq!(
        super::bytes_to_hex(&streaming.finalize()),
        crate::SHA256::sha256_hex(&nar)
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
    let root = unique_dir("closure");
    let source_root = root.join("source");
    fs::create_dir_all(source_root.join("root")).unwrap();
    fs::create_dir_all(source_root.join("leaf")).unwrap();
    fs::write(source_root.join("root/payload"), b"root").unwrap();
    fs::write(source_root.join("leaf/payload"), b"leaf").unwrap();
    let (root_nar, _) = super::super::write_nar(&source_root.join("root")).unwrap();
    let (leaf_nar, _) = super::super::write_nar(&source_root.join("leaf")).unwrap();
    let root_path = "/nix/store/00000000000000000000000000000000-root";
    let leaf_path = "/nix/store/11111111111111111111111111111111-leaf";
    let signing_key = SigningKey::from_bytes(&[7; 32]);
    let key_id = "test-cache-1";
    let root_info = signed_narinfo(
        root_path,
        "root.nar",
        &root_nar,
        &[leaf_path],
        key_id,
        &signing_key,
    );
    let leaf_info = signed_narinfo(leaf_path, "leaf.nar", &leaf_nar, &[], key_id, &signing_key);
    let root_key = format!(
        "{key_id}:{}\n",
        base64_encode(&signing_key.verifying_key().to_bytes())
    );
    let cache_info = b"StoreDir: /nix/store\nWantMassQuery: 1\n".to_vec();
    let routes = BTreeMap::from([
        ("/nix-cache-info".to_string(), cache_info),
        (
            "/00000000000000000000000000000000.narinfo".to_string(),
            root_info,
        ),
        ("/nar/root.nar".to_string(), root_nar),
    ]);
    let server = TestCacheServer::start(routes.clone());
    let jet_root = root.join("jetpack");
    fs::create_dir_all(jet_root.join("config")).unwrap();
    fs::create_dir_all(jet_root.join("trust")).unwrap();
    fs::write(
        jet_root.join("config/nix-cache-v1.endpoint"),
        &server.endpoint,
    )
    .unwrap();
    fs::write(jet_root.join("trust/nix-cache-v1.ed25519.pub"), root_key).unwrap();
    let roots = Roots::at(jet_root);
    let request = NixOutputRequest {
        name: "out".to_string(),
        store_path: root_path.to_string(),
    };

    let error = admit_nix_closure(&roots, &[request.clone()], false).unwrap_err();
    assert_eq!(error.kind(), NixCacheErrorKind::MissingReference);
    assert!(crate::Store::list(&roots).is_empty());

    let mut routes = routes;
    routes.insert(
        "/11111111111111111111111111111111.narinfo".to_string(),
        leaf_info,
    );
    routes.insert("/nar/leaf.nar".to_string(), leaf_nar);
    server.replace_routes(routes);
    let admitted = admit_nix_closure(&roots, &[request], false).unwrap();
    assert_eq!(admitted.objects.len(), 2);
    assert_eq!(admitted.outputs.len(), 1);
    assert_eq!(
        admitted.objects[root_path].direct_reference_digests.len(),
        1
    );
    assert!(admitted
        .objects
        .values()
        .all(|object| object.hangar_path.is_dir()));
    assert_eq!(crate::Store::list(&roots).len(), 2);
    let offline = admit_nix_closure(
        &roots,
        &[NixOutputRequest {
            name: "out".to_string(),
            store_path: root_path.to_string(),
        }],
        true,
    )
    .unwrap();
    assert_eq!(offline.objects.len(), 2);
    remove_dir(&root);
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
