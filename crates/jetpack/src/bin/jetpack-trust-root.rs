//! Prepare the public trust-root files for the Jet publication site.
//!
//! This tool accepts public Ed25519 trust files only.  It never generates,
//! reads, or copies a signing secret.  The production key ceremony remains an
//! operator action outside this repository.

use jetpack::TrustRoot::{public_trust_manifest, PublicTrustKey, RootBootstrap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_PUBLIC_KEY_BYTES: u64 = 4096;

fn main() {
    if let Err(error) = run(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("jetpack-trust-root: {error}");
        std::process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("export") {
        return Err(usage());
    }
    let mut output = None;
    let mut bootstrap = None;
    let mut key_paths = Vec::new();
    let mut index_key = None;
    let mut cache_key = None;
    let mut toolchain_key = None;
    let mut i = 1;
    while i < args.len() {
        let (name, slot) = match args[i].as_str() {
            "--output" => ("--output", &mut output),
            "--bootstrap" => ("--bootstrap", &mut bootstrap),
            "--index-key" => ("--index-key", &mut index_key),
            "--cache-key" => ("--cache-key", &mut cache_key),
            "--toolchain-key" => ("--toolchain-key", &mut toolchain_key),
            other => return Err(format!("unknown option `{other}`\n\n{}", usage())),
        };
        let value = args
            .get(i + 1)
            .filter(|value| !value.starts_with('-'))
            .ok_or_else(|| format!("{name} needs a path\n\n{}", usage()))?;
        *slot = Some(PathBuf::from(value));
        i += 2;
    }
    let output = output.ok_or_else(|| format!("--output is required\n\n{}", usage()))?;
    for (role, path) in [
        ("index", index_key),
        ("cache", cache_key),
        ("toolchain", toolchain_key),
    ] {
        if let Some(path) = path {
            key_paths.push((role, path));
        }
    }

    let mut keys = Vec::new();
    for (role, path) in key_paths {
        let value = read_public_key(&path).map_err(|error| {
            format!("read {role} public trust key `{}`: {error}", path.display())
        })?;
        let key = PublicTrustKey::from_nix_line(role, &value)
            .map_err(|error| format!("{role} public trust key `{}`: {error}", path.display()))?;
        keys.push(key);
    }
    let bootstrap = bootstrap
        .as_deref()
        .map(RootBootstrap::load)
        .transpose()
        .map_err(|error| format!("load trusted-root bootstrap: {error}"))?;
    let manifest = public_trust_manifest("jet-lang.dev", bootstrap.as_ref(), &keys)
        .map_err(|error| format!("render trust manifest: {error}"))?;

    write_immutable(&output.join("trust-manifest.json"), manifest.as_bytes())?;
    for key in &keys {
        write_immutable(
            &output.join(key.path_for_publication()),
            format!("{}:{}\n", key.key_id, key.public_key).as_bytes(),
        )?;
    }
    write_immutable(&output.join("README.md"), readme(&keys).as_bytes())?;
    println!(
        "prepared public trust root: {} ({} key(s)); production hosting remains pending",
        output.display(),
        keys.len()
    );
    Ok(())
}

fn usage() -> String {
    "usage: jetpack-trust-root export --output <dir> [--bootstrap <jetpack-root>] [--index-key <file>] [--cache-key <file>] [--toolchain-key <file>]".to_string()
}

fn read_public_key(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "public trust key is not a regular file",
        ));
    }
    if metadata.len() > MAX_PUBLIC_KEY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "public trust key is too large",
        ));
    }
    let value = String::from_utf8(fs::read(path)?).map_err(|_| {
        io::Error::new(io::ErrorKind::InvalidInput, "public trust key is not UTF-8")
    })?;
    Ok(value.trim().to_string())
}

fn write_immutable(path: &Path, body: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(format!("publication path is not a regular file: {}", path.display()))
        }
        Ok(_) => {
            let existing = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
            if existing == body {
                Ok(())
            } else {
                Err(format!("immutable publication file changed: {}", path.display()))
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|error| format!("create {}: {error}", path.display()))?;
            use std::io::Write as _;
            file.write_all(body)
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("write {}: {error}", path.display()))
        }
        Err(error) => Err(format!("inspect {}: {error}", path.display())),
    }
}

fn readme(keys: &[PublicTrustKey]) -> String {
    let mut out = String::from(
        "# Jet public trust root\n\n\
This directory contains publication-safe trust metadata. It contains public\
keys only. `TrustKey` HMAC secrets and signing keys stay in the offline\
publisher or host trust stores and must never be copied here.\n\n\
## Files\n\n\
- `trust-manifest.json` records the domain, root pin, role, algorithm, key id,\
  and exact client trust-file path.\n",
    );
    for key in keys {
        out.push_str(&format!(
            "- `{}` is the `{}` role's public key.\n",
            key.path_for_publication(),
            key.role
        ));
    }
    if keys.is_empty() {
        out.push_str(
            "- No production key is installed. The manifest is deliberately\
              marked `awaiting-key-ceremony`.\n",
        );
    }
    out.push_str(
        "\n## Rotation\n\n\
Generate a new offline threshold-root decision. Verify the replacement public\
keys and root metadata with the old root, then run the exporter with the new\
public-key files and bootstrap pin. Publish the new immutable key files and\
manifest together only after review. Revoke the old key at the root's agreed\
expiry; do not put private material in this tree.\n\n\
The real DNS, TLS, signer, and hosting publication are still pending owner\
approval.\n",
    );
    out
}

