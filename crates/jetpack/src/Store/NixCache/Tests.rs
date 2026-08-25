use self::TestCache::{
    base64_encode, remove_dir, signed_narinfo, signed_zstd_narinfo, unique_dir, TestCacheServer,
};
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
    let compressed = encode_zstd_deterministic(&nar).unwrap();
    assert_eq!(
        zstd::stream::decode_all(Cursor::new(&compressed)).unwrap(),
        nar
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
fn debug_real_ripgrep_staged_hash() {
    let nar = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target-nixfeed/ripgrep-15.2.0.nar");
    let destination = unique_dir("real-ripgrep-debug");
    remove_dir(&destination);
    super::super::read_nar_stream(
        fs::File::open(nar).unwrap(),
        &destination,
        7_088_584,
    )
    .unwrap();
    super::super::seal_node(&destination).unwrap();
    let result = crate::Envelope::try_output_hash_of(&destination.to_string_lossy());
    remove_dir(&destination);
    panic!("real ripgrep staged hash: {result:?}");
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
    let root_info = signed_zstd_narinfo(
        root_path,
        "root.nar.zst",
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
        (
            "/nar/root.nar.zst".to_string(),
            encode_zstd_deterministic(&root_nar).unwrap(),
        ),
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
    routes.insert("/nar/leaf.nar".to_string(), leaf_nar.clone());
    server.replace_routes(routes);
    let progress = crate::Output::ByteProgress::new();
    let admitted = admit_nix_closure_with_progress(
        &roots,
        &[request],
        false,
        Some(progress.clone()),
    )
    .unwrap();
    let root_compressed = encode_zstd_deterministic(&root_nar).unwrap();
    assert_eq!(
        progress.snapshot().total,
        Some(root_compressed.len() as u64 + leaf_nar.len() as u64)
    );
    assert_eq!(
        progress.snapshot().transferred,
        Some(root_compressed.len() as u64 + leaf_nar.len() as u64)
    );
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
fn native_nix_cache_shared_transitive_object_admits_twice_and_stores_once() {
    let root = unique_dir("shared-closure");
    let source_root = root.join("source");
    for name in ["first", "second", "shared"] {
        fs::create_dir_all(source_root.join(name)).unwrap();
    }
    fs::write(source_root.join("first/payload"), b"first").unwrap();
    fs::write(source_root.join("second/payload"), b"second").unwrap();
    fs::write(source_root.join("shared/payload"), b"shared").unwrap();
    let (first_nar, _) = super::super::write_nar(&source_root.join("first")).unwrap();
    let (second_nar, _) = super::super::write_nar(&source_root.join("second")).unwrap();
    let (shared_nar, _) = super::super::write_nar(&source_root.join("shared")).unwrap();
    let first_path = "/nix/store/00000000000000000000000000000000-first";
    let second_path = "/nix/store/11111111111111111111111111111111-second";
    let shared_path = "/nix/store/22222222222222222222222222222222-shared";
    let signing_key = SigningKey::from_bytes(&[8; 32]);
    let key_id = "shared-cache-1";
    let routes = BTreeMap::from([
        (
            "/nix-cache-info".to_string(),
            b"StoreDir: /nix/store\nWantMassQuery: 1\n".to_vec(),
        ),
        (
            "/00000000000000000000000000000000.narinfo".to_string(),
            signed_narinfo(
                first_path,
                "first.nar",
                &first_nar,
                &[shared_path],
                key_id,
                &signing_key,
            ),
        ),
        (
            "/11111111111111111111111111111111.narinfo".to_string(),
            signed_narinfo(
                second_path,
                "second.nar",
                &second_nar,
                &[shared_path],
                key_id,
                &signing_key,
            ),
        ),
        (
            "/22222222222222222222222222222222.narinfo".to_string(),
            signed_narinfo(
                shared_path,
                "shared.nar",
                &shared_nar,
                &[],
                key_id,
                &signing_key,
            ),
        ),
        ("/nar/first.nar".to_string(), first_nar),
        ("/nar/second.nar".to_string(), second_nar),
        ("/nar/shared.nar".to_string(), shared_nar),
    ]);
    let server = TestCacheServer::start(routes);
    let jet_root = root.join("jetpack");
    fs::create_dir_all(jet_root.join("config")).unwrap();
    fs::create_dir_all(jet_root.join("trust")).unwrap();
    fs::write(
        jet_root.join("config/nix-cache-v1.endpoint"),
        &server.endpoint,
    )
    .unwrap();
    fs::write(
        jet_root.join("trust/nix-cache-v1.ed25519.pub"),
        format!(
            "{key_id}:{}\n",
            base64_encode(&signing_key.verifying_key().to_bytes())
        ),
    )
    .unwrap();
    let roots = Roots::at(jet_root);

    let first = admit_nix_closure(
        &roots,
        &[NixOutputRequest {
            name: "first".into(),
            store_path: first_path.into(),
        }],
        false,
    )
    .unwrap();
    let shared_digest = first.objects[shared_path].hangar_digest.clone();
    let second = admit_nix_closure(
        &roots,
        &[NixOutputRequest {
            name: "second".into(),
            store_path: second_path.into(),
        }],
        false,
    )
    .unwrap();
    assert_eq!(first.objects.len(), 2);
    assert_eq!(second.objects.len(), 2);
    assert_eq!(second.objects[shared_path].hangar_digest, shared_digest);

    let mut second_entry = crate::Store::list_checked(&roots)
        .unwrap()
        .into_iter()
        .find(|entry| {
            ProducerRecord::decode(&entry.producer_record)
                .ok()
                .and_then(|producer| producer.facts.get("nix.store-path").cloned())
                .as_deref()
                == Some(second_path)
        })
        .unwrap();
    let mut producer = ProducerRecord::decode(&second_entry.producer_record).unwrap();
    producer
        .facts
        .insert("nix.output.bin".into(), shared_path.into());
    second_entry.producer_record = producer.encode();
    second_entry
        .named_outputs
        .insert("bin".into(), shared_digest.clone());
    second_entry.id.push_str("-bin");
    second_entry.receipt.clear();
    RuntimePolicy::with_lock(&roots.root, "hangar", || {
        crate::Store::register_entry_unlocked(&roots, &second_entry)
    })
    .unwrap();

    let graph = crate::Store::closure_graph(&roots).unwrap();
    assert_eq!(graph.records.len(), 4);
    assert_eq!(graph.objects.len(), 3);
    assert_eq!(
        graph.objects[&shared_digest].path,
        roots
            .hangar_dir()
            .join("objects")
            .join(&shared_digest)
            .to_string_lossy()
    );
    assert_eq!(
        fs::read_dir(roots.hangar_dir().join("objects"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy() == shared_digest)
            .count(),
        1
    );
    remove_dir(&root);
}

fn nar_with_directory_entries(names: &[&[u8]]) -> Vec<u8> {
    fn put(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&(value.len() as u64).to_le_bytes());
        output.extend_from_slice(value);
        let padding = (8 - value.len() % 8) % 8;
        output.resize(output.len() + padding, 0);
    }

    let mut output = Vec::new();
    put(&mut output, b"nix-archive-1");
    put(&mut output, b"(");
    put(&mut output, b"type");
    put(&mut output, b"directory");
    for name in names {
        put(&mut output, b"entry");
        put(&mut output, b"(");
        put(&mut output, b"name");
        put(&mut output, name);
        put(&mut output, b"node");
        put(&mut output, b"(");
        put(&mut output, b"type");
        put(&mut output, b"regular");
        put(&mut output, b"contents");
        put(&mut output, b"");
        put(&mut output, b")");
        put(&mut output, b")");
    }
    put(&mut output, b")");
    output
}

#[cfg(unix)]
#[test]
fn native_nix_cache_reads_canonical_directories_symlinks_and_executables() {
    fn put(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&(value.len() as u64).to_le_bytes());
        output.extend_from_slice(value);
        let padding = (8 - value.len() % 8) % 8;
        output.resize(output.len() + padding, 0);
    }

    fn regular(output: &mut Vec<u8>, contents: &[u8], executable: bool) {
        put(output, b"(");
        put(output, b"type");
        put(output, b"regular");
        if executable {
            put(output, b"executable");
            put(output, b"");
        }
        put(output, b"contents");
        put(output, contents);
        put(output, b")");
    }

    fn entry(output: &mut Vec<u8>, name: &[u8], node: impl FnOnce(&mut Vec<u8>)) {
        put(output, b"entry");
        put(output, b"(");
        put(output, b"name");
        put(output, name);
        put(output, b"node");
        node(output);
        put(output, b")");
    }

    let mut nar = Vec::new();
    put(&mut nar, b"nix-archive-1");
    put(&mut nar, b"(");
    put(&mut nar, b"type");
    put(&mut nar, b"directory");
    entry(&mut nar, b"bin", |output| regular(output, b"tool", true));
    entry(&mut nar, b"lib", |output| {
        put(output, b"(");
        put(output, b"type");
        put(output, b"directory");
        entry(output, b"target", |output| regular(output, b"target", false));
        put(output, b")");
    });
    entry(&mut nar, b"link", |output| {
        put(output, b"(");
        put(output, b"type");
        put(output, b"symlink");
        put(output, b"target");
        put(output, b"lib/target");
        put(output, b")");
    });
    entry(&mut nar, b"nix-link", |output| {
        put(output, b"(");
        put(output, b"type");
        put(output, b"symlink");
        put(output, b"target");
        put(
            output,
            b"/nix/store/11111111111111111111111111111111-target/bin/tool",
        );
        put(output, b")");
    });
    put(&mut nar, b")");

    let destination = unique_dir("canonical-nar");
    remove_dir(&destination);
    let stats = super::super::read_nar_stream(
        Cursor::new(&nar),
        &destination,
        nar.len() as u64,
    )
    .unwrap();
    assert_eq!(stats.nodes, 6);
    assert_eq!(fs::read(destination.join("bin")).unwrap(), b"tool");
    assert_eq!(fs::read(destination.join("lib/target")).unwrap(), b"target");
    assert_eq!(
        fs::read_link(destination.join("link")).unwrap(),
        PathBuf::from("lib/target")
    );
    assert_eq!(
        fs::read_link(destination.join("nix-link")).unwrap(),
        PathBuf::from("/nix/store/11111111111111111111111111111111-target/bin/tool")
    );
    crate::Envelope::try_output_hash_of_in_hangar(
        &destination.to_string_lossy(),
        &destination,
        false,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_ne!(
        fs::metadata(destination.join("bin"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
    remove_dir(&destination);
}

fn assert_single_nix_cache_failure(
    tag: &str,
    store_path: &str,
    narinfo: Vec<u8>,
    object: Vec<u8>,
    trusted_key: &SigningKey,
    expected: NixCacheErrorKind,
) {
    let root = unique_dir(tag);
    let narinfo_text = String::from_utf8(narinfo.clone()).unwrap();
    let mut routes = BTreeMap::from([
        (
            "/nix-cache-info".to_string(),
            b"StoreDir: /nix/store\nWantMassQuery: 1\n".to_vec(),
        ),
        (
            format!(
                "/{}.narinfo",
                store_path.rsplit('/').next().unwrap().get(..32).unwrap()
            ),
            narinfo,
        ),
    ]);
    if let Ok(info) = NixNarInfo::parse(&narinfo_text) {
        routes.insert(
            format!(
                "/{}",
                info.url.split('?').next().unwrap_or(info.url.as_str())
            ),
            object,
        );
    }
    let server = TestCacheServer::start(routes);
    let jet_root = root.join("jetpack");
    fs::create_dir_all(jet_root.join("config")).unwrap();
    fs::create_dir_all(jet_root.join("trust")).unwrap();
    fs::write(
        jet_root.join("config/nix-cache-v1.endpoint"),
        &server.endpoint,
    )
    .unwrap();
    fs::write(
        jet_root.join("trust/nix-cache-v1.ed25519.pub"),
        format!(
            "test-cache-1:{}\n",
            base64_encode(&trusted_key.verifying_key().to_bytes())
        ),
    )
    .unwrap();
    let roots = Roots::at(jet_root);
    let error = admit_nix_closure(
        &roots,
        &[NixOutputRequest {
            name: "out".to_string(),
            store_path: store_path.to_string(),
        }],
        false,
    )
    .unwrap_err();
    assert_eq!(error.kind(), expected);
    assert_eq!(error.code(), "E1350");
    assert!(crate::Store::list(&roots).is_empty());
    for path in [
        roots.hangar_dir().join("objects"),
        roots.hangar_dir().join("receipts"),
    ] {
        if let Ok(entries) = fs::read_dir(path) {
            assert!(entries.into_iter().next().is_none());
        }
    }
    drop(server);
    remove_dir(&root);
}

#[test]
fn native_nix_cache_negative_inputs_fail_closed() {
    let store_path = "/nix/store/22222222222222222222222222222222-negative";
    let signing_key = SigningKey::from_bytes(&[9; 32]);
    let wrong_key = SigningKey::from_bytes(&[10; 32]);
    let key_id = "test-cache-1";
    let valid_nar = nar_with_directory_entries(&[]);

    assert_single_nix_cache_failure(
        "wrong-key",
        store_path,
        signed_narinfo(
            store_path,
            "negative.nar",
            &valid_nar,
            &[],
            key_id,
            &signing_key,
        ),
        valid_nar.clone(),
        &wrong_key,
        NixCacheErrorKind::Signature,
    );

    let zstd_info = signed_zstd_narinfo(
        store_path,
        "negative.nar.zst",
        &valid_nar,
        &[],
        key_id,
        &signing_key,
    );
    let compressed = crate::Store::encode_zstd_deterministic(&valid_nar).unwrap();
    let mut corrupted = compressed.clone();
    let corruption_index = corrupted.len() - 1;
    corrupted[corruption_index] ^= 1;
    assert_single_nix_cache_failure(
        "compressed-corruption",
        store_path,
        zstd_info,
        corrupted,
        &signing_key,
        NixCacheErrorKind::CompressedCorruption,
    );

    assert_single_nix_cache_failure(
        "nar-corruption",
        store_path,
        signed_narinfo(
            store_path,
            "negative.nar",
            b"not-a-nar",
            &[],
            key_id,
            &signing_key,
        ),
        b"not-a-nar".to_vec(),
        &signing_key,
        NixCacheErrorKind::NarCorruption,
    );

    let duplicate_nar = nar_with_directory_entries(&[b"A", b"a"]);
    assert_single_nix_cache_failure(
        "duplicate-entry",
        store_path,
        signed_narinfo(
            store_path,
            "duplicate.nar",
            &duplicate_nar,
            &[],
            key_id,
            &signing_key,
        ),
        duplicate_nar,
        &signing_key,
        NixCacheErrorKind::DuplicateEntry,
    );

    let traversal_nar = nar_with_directory_entries(&[b".."]);
    assert_single_nix_cache_failure(
        "path-traversal",
        store_path,
        signed_narinfo(
            store_path,
            "traversal.nar",
            &traversal_nar,
            &[],
            key_id,
            &signing_key,
        ),
        traversal_nar,
        &signing_key,
        NixCacheErrorKind::PathTraversal,
    );

    let url_traversal = String::from_utf8(signed_narinfo(
        store_path,
        "negative.nar",
        &valid_nar,
        &[],
        key_id,
        &signing_key,
    ))
    .unwrap()
    .replace("URL: nar/negative.nar\n", "URL: ../negative.nar\n");
    assert_single_nix_cache_failure(
        "url-traversal",
        store_path,
        url_traversal.into_bytes(),
        valid_nar.clone(),
        &signing_key,
        NixCacheErrorKind::PathTraversal,
    );

    let reference = "/nix/store/33333333333333333333333333333333-reference";
    let duplicate_references = String::from_utf8(signed_narinfo(
        store_path,
        "negative.nar",
        &valid_nar,
        &[reference],
        key_id,
        &signing_key,
    ))
    .unwrap()
    .replace(
        "References: 33333333333333333333333333333333-reference\n",
        "References: 33333333333333333333333333333333-reference 33333333333333333333333333333333-reference\n",
    );
    assert_single_nix_cache_failure(
        "duplicate-reference",
        store_path,
        duplicate_references.into_bytes(),
        valid_nar,
        &signing_key,
        NixCacheErrorKind::DuplicateEntry,
    );
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
    for (detail, kind) in [
        (
            "NAR stream directory contains duplicate names",
            NixCacheErrorKind::DuplicateEntry,
        ),
        (
            "NAR name is not one safe path component",
            NixCacheErrorKind::PathTraversal,
        ),
        (
            "zstd data corruption detected",
            NixCacheErrorKind::CompressedCorruption,
        ),
    ] {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, detail);
        assert_eq!(super::classify_nar_error(&error), kind);
        assert_eq!(NixCacheError::new(kind, detail).code(), "E1350");
    }
}
