//! D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): the native `.Oci`
//! image-layout builder. Builds a single-layer OCI image directly from an
//! already-realized package binary plus optional `expose`/`env_vars`/`files`
//! metadata (`ImagePlan`, `ModuleEval::Types`) — no Docker, no external OCI
//! tooling.
//!
//! **Deterministic by construction** (this is the hard, budgeted part): every
//! tar entry uses a fixed mtime (Unix epoch 0), uid/gid 0, and no uname/gname;
//! entries are written in a caller-independent sorted-by-path order; no
//! wall-clock timestamp is embedded anywhere in the layer, config, manifest,
//! or index. The same `BuildSpec` therefore always produces byte-identical
//! blobs and the same manifest digest — proven by the `tests` below (build
//! twice, compare; reorder the input files, compare again).
//!
//! **Layer media type is uncompressed** (`application/vnd.oci.image.layer.v1.tar`,
//! not the `+gzip` variant) — both are valid, spec-compliant OCI layer types.
//! `core.compress.gzip`'s flate2 bridge (D-CORE-COMPRESS1) was evaluated for
//! gzip compression here first, but that runtime is emitted only into generated
//! user-program bridge crates, never linked into `jetpack` itself. Linking flate2 into
//! `jet-driver` directly would violate I6 (zero external crates in the
//! compiler proper, which `jetpack`/`jet-driver` are part of). Uncompressed
//! tar sidesteps needing a native DEFLATE implementation to ship a real image
//! today; gzip layers are a follow-up once/if a native (I6-safe) deflate
//! exists.
//!
//! OCI Image Format Spec layout implemented (the minimal single-layer,
//! single-platform subset): `oci-layout`, `index.json`, and
//! `blobs/sha256/<digest>` for the layer tar, the image config, and the
//! manifest.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::JSON;
use crate::SHA256;

const ARCHITECTURE: &str = "amd64";
const OS: &str = "linux";
const MEDIA_TYPE_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
const MEDIA_TYPE_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
const MEDIA_TYPE_LAYER: &str = "application/vnd.oci.image.layer.v1.tar";

/// One file layered into the image, relative to the image root (no leading
/// `/`), e.g. `usr/local/bin/myapp`. `mode` is the tar entry's Unix
/// permission bits (e.g. `0o755` for the package binary, `0o644` for a
/// plain data file).
#[derive(Debug, Clone)]
pub struct LayerFile {
    pub path: String,
    pub data: Vec<u8>,
    pub mode: u32,
}

/// Everything needed to build one `.Oci` image (D-JPK-IMAGE1). Pure data —
/// no filesystem/network access happens until `build` runs. `files` need not
/// be pre-sorted; `build` sorts by path so declaration order never affects
/// the output.
#[derive(Debug, Clone, Default)]
pub struct BuildSpec {
    pub files: Vec<LayerFile>,
    /// The container's `Entrypoint` (D-JPK-IMAGE1: the package binary's path
    /// inside the image, e.g. `["/usr/local/bin/myapp"]`).
    pub entrypoint: Vec<String>,
    /// `env_vars:` — rendered as `KEY=value` strings in the config's `Env`.
    pub env: Vec<(String, String)>,
    /// `expose:` — TCP ports, rendered as `ExposedPorts` keys (`"<port>/tcp"`).
    pub expose: Vec<i64>,
}

/// The result of a successful build: where the OCI layout landed and the
/// top-level manifest digest that identifies this exact image.
#[derive(Debug, Clone)]
pub struct BuiltImage {
    pub out_dir: PathBuf,
    /// `sha256:<hex>` — the image manifest's own digest.
    pub manifest_digest: String,
    /// Total bytes across the layer + config + manifest blobs.
    pub total_bytes: u64,
}

/// Build the OCI layout into `out_dir` (created if absent). `ref_name` is a
/// human label recorded in `index.json`'s annotations (e.g. the `image.<name>`
/// contribution's name) — cosmetic only, never mixed into any digest.
pub fn build(spec: &BuildSpec, out_dir: &Path, ref_name: &str) -> io::Result<BuiltImage> {
    let mut files: Vec<&LayerFile> = spec.files.iter().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let layer_tar = build_tar(&files);
    let layer_digest = SHA256::sha256_hex(&layer_tar);

    let config_json = build_config_json(spec, &layer_digest);
    let config_digest = SHA256::sha256_hex(config_json.as_bytes());

    let manifest_json = build_manifest_json(
        &config_digest,
        config_json.len(),
        &layer_digest,
        layer_tar.len(),
    );
    let manifest_digest = SHA256::sha256_hex(manifest_json.as_bytes());

    fs::create_dir_all(out_dir.join("blobs").join("sha256"))?;
    write_blob(out_dir, &layer_digest, &layer_tar)?;
    write_blob(out_dir, &config_digest, config_json.as_bytes())?;
    write_blob(out_dir, &manifest_digest, manifest_json.as_bytes())?;
    write_index(out_dir, &manifest_digest, manifest_json.len(), ref_name)?;
    fs::write(
        out_dir.join("oci-layout"),
        "{\"imageLayoutVersion\":\"1.0.0\"}",
    )?;

    Ok(BuiltImage {
        out_dir: out_dir.to_path_buf(),
        manifest_digest: format!("sha256:{manifest_digest}"),
        total_bytes: (layer_tar.len() + config_json.len() + manifest_json.len()) as u64,
    })
}

fn write_blob(out_dir: &Path, digest: &str, data: &[u8]) -> io::Result<()> {
    fs::write(out_dir.join("blobs").join("sha256").join(digest), data)
}

fn write_index(
    out_dir: &Path,
    manifest_digest: &str,
    manifest_size: usize,
    ref_name: &str,
) -> io::Result<()> {
    let json = format!(
        "{{\"schemaVersion\":2,\"manifests\":[{{\"mediaType\":{mt},\"digest\":\"sha256:{d}\",\"size\":{s},\"annotations\":{{\"org.opencontainers.image.ref.name\":{name}}}}}]}}",
        mt = JSON::quote(MEDIA_TYPE_MANIFEST),
        d = manifest_digest,
        s = manifest_size,
        name = JSON::quote(ref_name),
    );
    fs::write(out_dir.join("index.json"), json)
}

/// The OCI image config (minimal subset: `architecture`/`os`/`config`/`rootfs`;
/// no `created`/`history` — an embedded build timestamp is exactly the kind of
/// nondeterminism this builder exists to avoid).
fn build_config_json(spec: &BuildSpec, layer_digest: &str) -> String {
    let env_arr = spec
        .env
        .iter()
        .map(|(k, v)| JSON::quote(&format!("{k}={v}")))
        .collect::<Vec<_>>()
        .join(",");
    let entrypoint_arr = spec
        .entrypoint
        .iter()
        .map(|s| JSON::quote(s))
        .collect::<Vec<_>>()
        .join(",");
    let mut ports = spec.expose.clone();
    ports.sort_unstable();
    ports.dedup();
    let exposed = ports
        .iter()
        .map(|p| format!("{}:{{}}", JSON::quote(&format!("{p}/tcp"))))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"architecture\":\"{arch}\",\"os\":\"{os}\",\"config\":{{\"Env\":[{env_arr}],\"ExposedPorts\":{{{exposed}}},\"Entrypoint\":[{entrypoint_arr}]}},\"rootfs\":{{\"type\":\"layers\",\"diff_ids\":[\"sha256:{layer_digest}\"]}}}}",
        arch = ARCHITECTURE,
        os = OS,
    )
}

fn build_manifest_json(
    config_digest: &str,
    config_size: usize,
    layer_digest: &str,
    layer_size: usize,
) -> String {
    format!(
        "{{\"schemaVersion\":2,\"mediaType\":{mt},\"config\":{{\"mediaType\":{cmt},\"digest\":\"sha256:{config_digest}\",\"size\":{config_size}}},\"layers\":[{{\"mediaType\":{lmt},\"digest\":\"sha256:{layer_digest}\",\"size\":{layer_size}}}]}}",
        mt = JSON::quote(MEDIA_TYPE_MANIFEST),
        cmt = JSON::quote(MEDIA_TYPE_CONFIG),
        lmt = JSON::quote(MEDIA_TYPE_LAYER),
    )
}

// ── tar (ustar, uncompressed, deterministic) ────────────────────────────────

/// Build one uncompressed ustar tar archive from `files` (assumed already
/// sorted by path). Every entry: mtime 0 (Unix epoch), uid/gid 0, no
/// uname/gname — nothing that varies run-to-run or machine-to-machine.
fn build_tar(files: &[&LayerFile]) -> Vec<u8> {
    let mut out = Vec::new();
    for f in files {
        write_tar_entry(&mut out, &f.path, &f.data, f.mode);
    }
    // End-of-archive marker: two 512-byte zero blocks.
    out.extend(std::iter::repeat(0u8).take(1024));
    out
}

fn write_tar_entry(out: &mut Vec<u8>, path: &str, data: &[u8], mode: u32) {
    let mut header = [0u8; 512];
    set_bytes(&mut header, 0, 100, path.as_bytes());
    set_octal(&mut header, 100, 8, mode as u64);
    set_octal(&mut header, 108, 8, 0); // uid
    set_octal(&mut header, 116, 8, 0); // gid
    set_octal(&mut header, 124, 12, data.len() as u64); // size
    set_octal(&mut header, 136, 12, 0); // mtime = epoch 0, always
    header[156] = b'0'; // typeflag '0' = regular file
    set_bytes(&mut header, 257, 6, b"ustar\0"); // magic
    set_bytes(&mut header, 263, 2, b"00"); // ustar version

    // Checksum: the field itself reads as ASCII spaces during the sum, then
    // is written as a 6-digit octal + NUL + space (the ustar convention).
    for b in &mut header[148..156] {
        *b = b' ';
    }
    let checksum: u32 = header.iter().map(|&b| b as u32).sum();
    let chksum_field = format!("{checksum:06o}\0 ");
    set_bytes(&mut header, 148, 8, chksum_field.as_bytes());

    out.extend_from_slice(&header);
    out.extend_from_slice(data);
    let pad = (512 - (data.len() % 512)) % 512;
    out.extend(std::iter::repeat(0u8).take(pad));
}

/// Copy `bytes` into `header[offset..offset+len]`, truncating to `len` and
/// zero-padding the rest (tar string fields are NUL-padded).
fn set_bytes(header: &mut [u8; 512], offset: usize, len: usize, bytes: &[u8]) {
    let n = bytes.len().min(len);
    header[offset..offset + n].copy_from_slice(&bytes[..n]);
}

/// Write `value` as a NUL-terminated octal ASCII string right-padded... no —
/// left-padded with `0`s to fill `len - 1` digits, then a trailing NUL, per
/// the classic (pre-GNU-base-256) tar numeric field encoding.
fn set_octal(header: &mut [u8; 512], offset: usize, len: usize, value: u64) {
    let digits = len - 1;
    let s = format!("{value:0>digits$o}\0", digits = digits);
    set_bytes(header, offset, len, s.as_bytes());
}

// ── digest-agnostic helper for callers: sort + dedupe env keys ─────────────

/// D-JPK-IMAGE1: fold `env_vars:` (already key-sorted — `ImagePlan::env_vars`
/// comes from a `CtValue::Map`/`BTreeMap`) into a plain `Vec`, defensively
/// re-sorted so a caller that hand-builds a `BuildSpec` some other way still
/// gets a deterministic `Env` order.
pub fn sorted_env(entries: &[(String, String)]) -> Vec<(String, String)> {
    let map: BTreeMap<&str, &str> = entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    map.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(path: &str, contents: &str) -> LayerFile {
        LayerFile {
            path: path.to_string(),
            data: contents.as_bytes().to_vec(),
            mode: 0o755,
        }
    }

    #[test]
    fn tar_layer_is_byte_identical_across_calls() {
        let files = vec![bin("usr/local/bin/app", "hello binary")];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let a = build_tar(&refs);
        let b = build_tar(&refs);
        assert_eq!(a, b);
    }

    #[test]
    fn tar_header_checksum_is_valid_ustar() {
        let files = vec![bin("usr/local/bin/app", "x")];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let tar = build_tar(&refs);
        // Recompute the checksum the way a real reader would: sum the header
        // bytes with the checksum field blanked to spaces, and it must match
        // what's encoded.
        let mut header = [0u8; 512];
        header.copy_from_slice(&tar[0..512]);
        let encoded = std::str::from_utf8(&header[148..154]).unwrap().to_string();
        let encoded_val = u32::from_str_radix(encoded.trim_end_matches('\0'), 8).unwrap();
        for b in &mut header[148..156] {
            *b = b' ';
        }
        let recomputed: u32 = header.iter().map(|&b| b as u32).sum();
        assert_eq!(encoded_val, recomputed);
    }

    /// I5/reproducibility: building the same `BuildSpec` twice, into two
    /// different output directories, yields byte-identical blobs and the same
    /// manifest digest — no timestamps, no nondeterministic ordering.
    #[test]
    fn build_is_reproducible_across_separate_builds() {
        let spec = BuildSpec {
            files: vec![bin("usr/local/bin/app", "the binary")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![("RUST_LOG".to_string(), "info".to_string())],
            expose: vec![8080],
        };
        let dir_a = std::env::temp_dir().join(format!("jet-oci-test-a-{}", std::process::id()));
        let dir_b = std::env::temp_dir().join(format!("jet-oci-test-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec, &dir_a, "myimage").unwrap();
        let b = build(&spec, &dir_b, "myimage").unwrap();
        assert_eq!(a.manifest_digest, b.manifest_digest);
        assert_eq!(a.total_bytes, b.total_bytes);

        // The actual blob bytes on disk must be identical too, not just the
        // digest string (a digest collision would be a much bigger bug, but
        // this proves the builder itself, not just SHA256, is deterministic).
        let blobs_a = dir_a.join("blobs").join("sha256");
        let blobs_b = dir_b.join("blobs").join("sha256");
        let digest_hex = a.manifest_digest.trim_start_matches("sha256:");
        assert_eq!(
            fs::read(blobs_a.join(digest_hex)).unwrap(),
            fs::read(blobs_b.join(digest_hex)).unwrap(),
        );

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    /// Declaration order of `files:` must not affect the digest — `build`
    /// sorts by path before laying out the tar.
    #[test]
    fn file_declaration_order_does_not_affect_digest() {
        let f1 = bin("usr/local/bin/app", "the binary");
        let f2 = LayerFile {
            path: "etc/app.conf".to_string(),
            data: b"config".to_vec(),
            mode: 0o644,
        };
        let spec_a = BuildSpec {
            files: vec![f1.clone(), f2.clone()],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
        };
        let spec_b = BuildSpec {
            files: vec![f2, f1],
            ..spec_a.clone()
        };
        let dir_a =
            std::env::temp_dir().join(format!("jet-oci-test-order-a-{}", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("jet-oci-test-order-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec_a, &dir_a, "myimage").unwrap();
        let b = build(&spec_b, &dir_b, "myimage").unwrap();
        assert_eq!(a.manifest_digest, b.manifest_digest);

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    /// A changed input (different file contents) must change the digest —
    /// the flip side of reproducibility: it isn't a constant.
    #[test]
    fn different_content_yields_different_digest() {
        let spec_a = BuildSpec {
            files: vec![bin("usr/local/bin/app", "version one")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
        };
        let spec_b = BuildSpec {
            files: vec![bin("usr/local/bin/app", "version two")],
            ..spec_a.clone()
        };
        let dir_a =
            std::env::temp_dir().join(format!("jet-oci-test-diff-a-{}", std::process::id()));
        let dir_b =
            std::env::temp_dir().join(format!("jet-oci-test-diff-b-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);

        let a = build(&spec_a, &dir_a, "myimage").unwrap();
        let b = build(&spec_b, &dir_b, "myimage").unwrap();
        assert_ne!(a.manifest_digest, b.manifest_digest);

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    #[ignore]
    fn manual_dump_for_tar_inspection() {
        let files = vec![
            bin("usr/local/bin/app", "hello binary contents"),
            LayerFile {
                path: "etc/app.conf".to_string(),
                data: b"key=value\n".to_vec(),
                mode: 0o644,
            },
        ];
        let refs: Vec<&LayerFile> = files.iter().collect();
        let tar = build_tar(&refs);
        let path = std::env::temp_dir().join("jet-oci-manual-check.tar");
        fs::write(path, &tar).unwrap();
    }

    #[test]
    fn oci_layout_has_the_required_files() {
        let spec = BuildSpec {
            files: vec![bin("usr/local/bin/app", "bin")],
            entrypoint: vec!["/usr/local/bin/app".to_string()],
            env: vec![],
            expose: vec![],
        };
        let dir = std::env::temp_dir().join(format!("jet-oci-test-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let built = build(&spec, &dir, "myimage").unwrap();
        assert!(dir.join("oci-layout").is_file());
        assert!(dir.join("index.json").is_file());
        let digest_hex = built.manifest_digest.trim_start_matches("sha256:");
        assert!(dir.join("blobs").join("sha256").join(digest_hex).is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
