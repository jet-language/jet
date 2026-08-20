use super::*;

#[cfg(unix)]
#[test]
fn cas_pool_hardlink_preserves_cache_verification_and_rejects_outside_peers() {
    use std::os::unix::fs::MetadataExt as _;

    let (roots, _g) = temp_roots();
    // Two distinct object digests that share an identical file payload.
    let src_c = roots.root.join("cas-c");
    fs::create_dir_all(&src_c).unwrap();
    fs::write(src_c.join("payload"), "shared-cas-bytes").unwrap();
    fs::write(src_c.join("unique"), "c-only").unwrap();
    let src_d = roots.root.join("cas-d");
    fs::create_dir_all(&src_d).unwrap();
    fs::write(src_d.join("payload"), "shared-cas-bytes").unwrap();
    fs::write(src_d.join("unique"), "d-only").unwrap();

    let mut outs_c = BTreeMap::new();
    outs_c.insert("out".to_string(), src_c);
    let third = ingest_tree(
        &roots,
        &IngestRequest {
            name: "cas-c".into(),
            version: "1".into(),
            reference: "path:cas-c".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: outs_c,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    let mut outs_d = BTreeMap::new();
    outs_d.insert("out".to_string(), src_d);
    let fourth = ingest_tree(
        &roots,
        &IngestRequest {
            name: "cas-d".into(),
            version: "1".into(),
            reference: "path:cas-d".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: outs_d,
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: String::new(),
        },
    )
    .unwrap();
    assert_ne!(
        third.entry.envelope.output_hash,
        fourth.entry.envelope.output_hash
    );

    // Ingest leaves nlink=1 (no cas peers yet).
    let pay_c = Path::new(&third.entry.out).join("payload");
    assert_eq!(fs::metadata(&pay_c).unwrap().nlink(), 1);

    let report = optimize_cas_pool(&roots).unwrap();
    assert!(report.optimized_files >= 2, "{report:?}");
    assert!(roots.hangar_dir().join("cas").is_dir());
    assert!(fs::metadata(&pay_c).unwrap().nlink() >= 2);

    // Hangar-internal cas peers: verify still green; digest stable.
    verify_hangar_object(&roots, &third.entry).unwrap();
    verify_hangar_object(&roots, &fourth.entry).unwrap();
    let expectation = test_expectation(Path::new(&third.entry.out));
    let proof = verify_cache_entry(&roots, &third.entry, &third.entry.reference, &expectation);
    assert!(proof.output_digest, "{proof:?}");
    assert!(proof.trusted(), "{proof:?}");
    find_verified_by_reference(&roots, &third.entry.reference, &expectation)
        .unwrap()
        .unwrap()
        .lease
        .validate()
        .unwrap();

    // Outside-hangar peer still rejected.
    let outside = roots.root.join("outside-peer");
    fs::hard_link(&pay_c, &outside).unwrap();
    let bare = super::super::super::super::Envelope::try_output_hash_of(&third.entry.out);
    assert!(bare.is_err(), "{bare:?}");
    let in_hangar = super::super::super::super::Envelope::try_output_hash_of_in_hangar(
        &third.entry.out,
        &roots.hangar_dir(),
        false,
    );
    assert!(in_hangar.is_err(), "{in_hangar:?}");
    let proof = verify_cache_entry(&roots, &third.entry, &third.entry.reference, &expectation);
    assert!(!proof.output_digest, "{proof:?}");
    assert!(!proof.trusted(), "{proof:?}");
    fs::remove_file(outside).ok();
}
    #[cfg(target_os = "linux")]
    #[test]
    fn ingest_rejects_semantic_xattr_without_platform_artifact_kind() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("xattr-src");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("payload");
        fs::write(&file, "xattr-bytes").unwrap();
        set_user_xattr(&file, "user.jet.test", b"keep");
        let mut outputs = BTreeMap::new();
        outputs.insert("out".to_string(), src);
        let err = ingest_tree(
            &roots,
            &IngestRequest {
                name: "xattr".into(),
                version: "1".into(),
                reference: "path:xattr".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs,
                signature: String::new(),
                provenance: String::new(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code(), "E1315");
        assert!(
            err.what().contains("semantic xattr") || err.why().contains("semantic xattr"),
            "{err:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ingest_keeps_semantic_xattr_with_platform_artifact_kind() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("xattr-ok");
        fs::create_dir_all(&src).unwrap();
        let file = src.join("payload");
        fs::write(&file, "xattr-bytes").unwrap();
        set_user_xattr(&src, "user.jet.directory", b"directory");
        set_user_xattr(&file, "user.jet.test", b"keep");
        let mut outputs = BTreeMap::new();
        outputs.insert("out".to_string(), src.clone());
        let ingested = ingest_tree(
            &roots,
            &IngestRequest {
                name: "xattr-ok".into(),
                version: "1".into(),
                reference: "path:xattr-ok".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs,
                signature: String::new(),
                provenance: String::new(),
                platform_artifact_kind: "macos-app".into(),
            },
        )
        .unwrap();
        let first_hash = ingested.entry.envelope.output_hash.clone();
        assert_eq!(ingested.entry.platform_artifact_kind, "macos-app");
        verify_hangar_object(&roots, &ingested.entry).unwrap();
        let sealed = Path::new(&ingested.entry.out).join("payload");
        let names = super::super::super::super::Envelope::list_xattr_names(&sealed).unwrap();
        assert!(
            names.iter().any(|n| n == "user.jet.test"),
            "semantic xattr must be preserved on sealed object: {names:?}"
        );
        let root_names = super::super::super::super::Envelope::list_xattr_names(
            Path::new(&ingested.entry.out),
        )
        .unwrap();
        assert!(root_names.iter().any(|name| name == "user.jet.directory"));
        set_user_xattr(&src, "user.jet.directory", b"changed");
        let changed_hash = super::super::super::super::Envelope::try_output_hash_of_with_policy(
            &src.to_string_lossy(),
            true,
            &mut |_, _| {},
        )
        .unwrap();
        assert_ne!(first_hash, changed_hash);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ingest_rejects_semantic_directory_xattr_without_platform_kind() {
        let (roots, _g) = temp_roots();
        let src = roots.root.join("xattr-directory-reject");
        fs::create_dir_all(&src).unwrap();
        set_user_xattr(&src, "user.jet.directory", b"reject");
        let error = ingest_tree(
            &roots,
            &IngestRequest {
                name: "xattr-directory".into(),
                version: "1".into(),
                reference: "path:xattr-directory".into(),
                cache_identity: test_identity(),
                references: Vec::new(),
                outputs: BTreeMap::from([("out".into(), src)]),
                signature: String::new(),
                provenance: String::new(),
                platform_artifact_kind: String::new(),
            },
        )
        .unwrap_err();
        assert!(error.what().contains("semantic xattr") || error.why().contains("semantic xattr"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ingest_symlink_xattr_is_nofollow_rejected_digested_and_copied() {
        use std::os::unix::fs::symlink;
        let (roots, _g) = temp_roots();
        let src = roots.root.join("xattr-symlink");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("target"), "bytes").unwrap();
        symlink("target", src.join("link")).unwrap();
        set_apple_xattr(&src.join("link"), "user.jet.symlink", b"first");
        let request = |kind: &str| IngestRequest {
            name: "xattr-symlink".into(),
            version: "1".into(),
            reference: "path:xattr-symlink".into(),
            cache_identity: test_identity(),
            references: Vec::new(),
            outputs: BTreeMap::from([("out".into(), src.clone())]),
            signature: String::new(),
            provenance: String::new(),
            platform_artifact_kind: kind.into(),
        };
        assert!(ingest_tree(&roots, &request("")).is_err());
        let ingested = ingest_tree(&roots, &request("macos-tree")).unwrap();
        let sealed = Path::new(&ingested.entry.out).join("link");
        assert!(super::super::super::super::Envelope::list_xattr_names(&sealed)
            .unwrap()
            .iter()
            .any(|name| name == "user.jet.symlink"));
        let first = ingested.entry.envelope.output_hash;
        set_apple_xattr(&src.join("link"), "user.jet.symlink", b"second");
        let second = super::super::super::super::Envelope::try_output_hash_of_with_policy(
            &src.to_string_lossy(),
            true,
            &mut |_, _| {},
        )
        .unwrap();
        assert_ne!(first, second);
    }

    #[cfg(target_os = "linux")]
    fn set_user_xattr(path: &Path, name: &str, value: &[u8]) {
        use std::os::unix::ffi::OsStrExt as _;
        type LibcChar = i8;
        #[link(name = "c")]
        extern "C" {
            fn lsetxattr(
                path: *const LibcChar,
                name: *const LibcChar,
                value: *const u8,
                size: usize,
                flags: i32,
            ) -> i32;
        }
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let c_name = std::ffi::CString::new(name).unwrap();
        let rc = unsafe {
            lsetxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_ptr(),
                value.len(),
                0,
            )
        };
        assert_eq!(rc, 0, "lsetxattr failed: {}", std::io::Error::last_os_error());
    }

    #[cfg(target_os = "macos")]
    fn set_apple_xattr(path: &Path, name: &str, value: &[u8]) {
        use std::os::unix::ffi::OsStrExt as _;
        unsafe extern "C" {
            fn setxattr(
                path: *const i8,
                name: *const i8,
                value: *const u8,
                size: usize,
                position: u32,
                options: i32,
            ) -> i32;
        }
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let name = std::ffi::CString::new(name).unwrap();
        let rc = unsafe {
            setxattr(
                path.as_ptr(),
                name.as_ptr(),
                value.as_ptr(),
                value.len(),
                0,
                0x0001,
            )
        };
        assert_eq!(rc, 0, "setxattr failed: {}", std::io::Error::last_os_error());
    }
}
