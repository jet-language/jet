use super::Closure::RECEIPTS_DIR;
use super::*;
use jet_codegen::development_receipt::{
    is_content_address, jet_development_receipt_render, JetDevelopmentReceipt,
    JetDevelopmentReceiptInput,
};
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

const RECEIPT_PARTIAL_SUFFIX: &str = ".partial";
static RECEIPT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Prepare the immutable receipt before the closure WAL becomes authoritative.
/// The caller keeps the returned digest in the package projection, so the
/// in-memory result, `meta.json`, closure record, and project lock all name the
/// same object.
pub(super) fn prepare_entry_receipt(roots: &Roots, entry: &mut StoreEntry) -> std::io::Result<()> {
    let (digest, _) = materialize_receipt(roots, entry)?;
    entry.receipt = digest;
    Ok(())
}

/// Prepare the package receipt and report whether this call published a new
/// receipt file. The admission transaction uses the status for rollback;
/// ordinary callers keep the simpler `prepare_entry_receipt` API.
pub(super) fn prepare_entry_receipt_with_status(
    roots: &Roots,
    entry: &mut StoreEntry,
) -> std::io::Result<bool> {
    let (digest, wrote) = materialize_receipt(roots, entry)?;
    entry.receipt = digest;
    Ok(wrote)
}

/// Returns the receipt digest and whether this call actually wrote it. The
/// recovery counter needs the second half: an already-present receipt is not a
/// recovery, and the digest alone cannot tell the two apart.
pub(super) fn materialize_receipt(
    roots: &Roots,
    entry: &StoreEntry,
) -> std::io::Result<(String, bool)> {
    let bytes = render_receipt(entry).into_bytes();
    let digest = format!("sha256-{}", SHA256::sha256_hex(&bytes));
    if !entry.receipt.is_empty() && entry.receipt != digest {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Hangar receipt projection for `{}` names `{}`, expected `{}`",
                entry.id, entry.receipt, digest
            ),
        ));
    }
    let receipts = roots.hangar_dir().join(RECEIPTS_DIR);
    super::Ingest::ensure_real_directory(&receipts, "Hangar receipt directory")?;
    let path = receipts.join(&digest);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt `{digest}` is not a regular file; repair the receipt object"
                ),
            ));
        }
        Ok(_) => {
            let actual = fs::read(&path)?;
            if actual != bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar receipt `{digest}` is corrupt; restore the exact object or remove the unreferenced receipt"
                    ),
                ));
            }
            return Ok((digest, false));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let sequence = RECEIPT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let partial = receipts.join(format!(
        ".{digest}-{}-{sequence}{RECEIPT_PARTIAL_SUFFIX}",
        std::process::id()
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    match fs::rename(&partial, &path) {
        Ok(()) => {
            super::sync_store_directory(&receipts)?;
            Ok((digest, true))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&partial);
            let actual = fs::read(&path)?;
            if actual != bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Hangar receipt `{digest}` changed during publication"),
                ));
            }
            Ok((digest, false))
        }
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

pub(super) fn recover_receipt_staging(roots: &Roots) -> std::io::Result<usize> {
    let receipts = roots.hangar_dir().join(RECEIPTS_DIR);
    let Ok(metadata) = fs::symlink_metadata(&receipts) else {
        return Ok(0);
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar receipt directory is not a real directory; repair the path before recovery",
        ));
    }
    let mut recovered = 0;
    for item in fs::read_dir(&receipts)? {
        let path = item?.path();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !name.starts_with('.') || !name.ends_with(RECEIPT_PARTIAL_SUFFIX) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(&path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt staging path `{}` is not removable",
                    path.display()
                ),
            ));
        }
        recovered += 1;
    }
    if recovered > 0 {
        super::sync_store_directory(&receipts)?;
    }
    Ok(recovered)
}

pub(super) fn render_receipt(entry: &StoreEntry) -> String {
    let mut inputs = vec![
        receipt_input("package-name", &entry.name),
        receipt_input("package-version", &entry.version),
        receipt_input("reference", &entry.reference),
        receipt_input(
            "source-fingerprint",
            &entry.cache_identity.source_fingerprint,
        ),
        receipt_input(
            "recipe-fingerprint",
            &entry.cache_identity.recipe_fingerprint,
        ),
        receipt_input(
            "policy-fingerprint",
            &entry.cache_identity.policy_fingerprint,
        ),
        receipt_input("platform", &entry.cache_identity.platform),
        receipt_input("platform-artifact-kind", &entry.platform_artifact_kind),
        receipt_input("producer-record", &entry.producer_record),
    ];
    if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
        for (name, key) in [
            ("catalog-tier", "nix.index.tier"),
            ("catalog-trust", "nix.index.trust"),
            ("signature-chain", "nix.index.signature-chain"),
            ("fallback-provenance", "nix.fallback.provenance"),
            ("fallback-request", "nix.fallback.request"),
            ("fallback-policy", "nix.fallback.policy.receipt"),
            ("fallback-graph", "nix.fallback.graph"),
            ("fallback-losses", "nix.fallback.losses"),
            ("fallback-proof", "nix.fallback.proof"),
        ] {
            if let Some(value) = producer.facts.get(key) {
                inputs.push(receipt_input(name, value));
            }
        }
    }
    let references = entry.references.iter().collect::<BTreeSet<_>>();
    for reference in references {
        inputs.push(receipt_input("closure", reference));
    }
    let action = entry_action_key(entry);
    let mut outputs = entry.named_outputs.clone();
    outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
    let outputs = outputs
        .into_iter()
        .map(|(name, digest)| receipt_input(&name, &digest))
        .collect();
    let receipt = JetDevelopmentReceipt {
        act: "package-realization".into(),
        locked_closure: action.clone(),
        inputs,
        planned_action: action,
        outputs,
        activation_proof: String::new(),
        parent_generation: String::new(),
        witness: receipt_witness(),
        outcome: "passed".into(),
        failure_path: None,
    };
    jet_development_receipt_render(&receipt)
}

fn receipt_input(name: &str, value: &str) -> JetDevelopmentReceiptInput {
    let digest = if is_content_address(value) {
        value.to_owned()
    } else {
        format!("sha256-{}", SHA256::sha256_hex(value.as_bytes()))
    };
    JetDevelopmentReceiptInput {
        name: name.to_owned(),
        digest,
    }
}

fn receipt_witness() -> String {
    std::env::var("JET_RECEIPT_WITNESS")
        .ok()
        .filter(|witness| !witness.is_empty())
        .unwrap_or_else(|| "jetpack".into())
}
