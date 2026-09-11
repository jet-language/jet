// D-FOUND-RECEIPT1: one typed receipt attachment kernel shared by generated
// native/Web programs. Hosts provide only the receipt-store environment and
// marshal the resulting append-only record; section meaning stays here.

const JET_RECEIPT_DIR_ENV: &str = "JET_RECEIPT_DIR";
const JET_RECEIPT_CLAIM_ENV: &str = "JET_RECEIPT_CLAIM";
const JET_RECEIPT_DIGEST_ENV: &str = "JET_RECEIPT_DIGEST";
const JET_RECEIPT_SECTION_RECORD_MAGIC: &[u8] = b"jet-receipt-section-record-v1\0";
const JET_RECEIPT_SECTION_RECORD_ID_MAGIC: &[u8] = b"jet-receipt-section-id-v1\0";
const JET_RECEIPT_SECTION_BYTES: usize = 64 * 1024 * 1024;
const JET_RECEIPT_SECTION_BYTES_TOTAL: u64 = 64 * 1024 * 1024;
const JET_RECEIPT_SECTION_COUNT: usize = 256;

/// Attach one checked `#Receipt` record to the current program receipt.
///
/// `section_name`, `type_name`, and `schema_digest` are compiler-supplied
/// facts. The public source call has one payload argument; these metadata
/// values are internal Core-call arguments and are never user-spellable.
pub fn jet_receipt_attach<T: __jet_Encode>(
    value: &T,
    section_name: &str,
    type_name: &str,
    schema_digest: &str,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (value, section_name, type_name, schema_digest);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        jet_receipt_attach_tree(
            value.jet_encode(),
            section_name,
            type_name,
            schema_digest,
        );
    }
}

/// Attach a value already rendered by an execution-tier adapter. The adapter
/// supplies no receipt policy: this function parses the canonical DataTree and
/// sends it through the same validation, digest, and publication path as the
/// generated `__jet_Encode` entry point.
pub fn jet_receipt_attach_encoded(
    payload: &str,
    section_name: &str,
    type_name: &str,
    schema_digest: &str,
) {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (payload, section_name, type_name, schema_digest);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let tree = jet_std::parse_json_datatree(payload).unwrap_or_else(|error| {
            let line = error.line.ok().unwrap_or(0);
            panic!(
                "receipt section `{section_name}` is not valid canonical JSON (line {}): {}",
                line, error.reason
            )
        });
        jet_receipt_attach_tree(tree, section_name, type_name, schema_digest);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_attach_tree(
    tree: jet_std::DataTree,
    section_name: &str,
    type_name: &str,
    schema_digest: &str,
) {
    let Ok(directory) = std::env::var(JET_RECEIPT_DIR_ENV) else {
        return;
    };
    let Ok(claim_key) = std::env::var(JET_RECEIPT_CLAIM_ENV) else {
        return;
    };
    let parent_digest = std::env::var(JET_RECEIPT_DIGEST_ENV).unwrap_or_default();
    jet_receipt_validate_digest(&claim_key, "claim");
    if !parent_digest.is_empty() {
        jet_receipt_validate_digest(&parent_digest, "parent");
    }
    jet_receipt_validate_name(section_name, 128, "section name");
    jet_receipt_validate_name(type_name, 256, "section type name");
    jet_receipt_validate_digest(schema_digest, "schema");

    let tree = jet_receipt_canonical_tree(&tree);
    let jet_std::DataTree::Object(_) = &tree else {
        panic!("receipt section `{section_name}` must encode as an object");
    };
    let mut payload = jet_std::render_datatree_json(&tree, false, 0).into_bytes();
    payload.push(b'\n');
    if payload.len() > JET_RECEIPT_SECTION_BYTES {
        panic!(
            "receipt section `{section_name}` is too large (maximum {} bytes)",
            JET_RECEIPT_SECTION_BYTES
        );
    }
    let payload_digest = jet_receipt_digest(&payload);
    let record = jet_receipt_record(
        &claim_key,
        &parent_digest,
        section_name,
        type_name,
        schema_digest,
        &payload_digest,
        &payload,
    );
    let record_id = jet_receipt_record_id(
        &claim_key,
        &parent_digest,
        section_name,
        type_name,
        schema_digest,
        &payload_digest,
    );
    let root = std::path::Path::new(&directory);
    let lane = if parent_digest.is_empty() {
        "staged"
    } else {
        "sections"
    };
    let target_dir = root.join(lane).join(&claim_key);
    jet_receipt_check_capacity(&target_dir, payload.len());
    if !ensure_receipt_directory(&target_dir) {
        panic!("receipt section directory is unsafe: {}", target_dir.display());
    }
    jet_receipt_publish(&target_dir.join(record_id), &record);
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_canonical_tree(tree: &jet_std::DataTree) -> jet_std::DataTree {
    match tree {
        jet_std::DataTree::Array(values) => jet_std::DataTree::Array(
            values
                .iter()
                .map(jet_receipt_canonical_tree)
                .collect(),
        ),
        jet_std::DataTree::Object(values) => {
            let mut values = values
                .iter()
                .map(|(name, value)| (name.clone(), jet_receipt_canonical_tree(value)))
                .collect::<Vec<_>>();
            values.sort_by(|left, right| left.0.cmp(&right.0));
            jet_std::DataTree::Object(values)
        }
        _ => tree.clone(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_validate_digest(value: &str, label: &str) {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        panic!("receipt section {label} digest is malformed");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_validate_name(value: &str, limit: usize, label: &str) {
    if value.is_empty()
        || value.len() > limit
        || !value
            .chars()
            .all(|character| !character.is_control() && character != '/' && character != '\\')
    {
        panic!("receipt {label} is invalid");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_digest(value: &[u8]) -> String {
    let digest = jet_sha256_raw(value);
    let mut text = String::with_capacity(64);
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_frame(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_record(
    claim_key: &str,
    parent_digest: &str,
    section_name: &str,
    type_name: &str,
    schema_digest: &str,
    payload_digest: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(JET_RECEIPT_SECTION_RECORD_MAGIC);
    for field in [
        claim_key.as_bytes(),
        parent_digest.as_bytes(),
        section_name.as_bytes(),
        type_name.as_bytes(),
        schema_digest.as_bytes(),
        payload_digest.as_bytes(),
        payload,
    ] {
        jet_receipt_frame(&mut record, field);
    }
    record
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_record_id(
    claim_key: &str,
    parent_digest: &str,
    section_name: &str,
    type_name: &str,
    schema_digest: &str,
    payload_digest: &str,
) -> String {
    let mut identity = Vec::new();
    identity.extend_from_slice(JET_RECEIPT_SECTION_RECORD_ID_MAGIC);
    for field in [
        claim_key.as_bytes(),
        parent_digest.as_bytes(),
        section_name.as_bytes(),
        type_name.as_bytes(),
        schema_digest.as_bytes(),
        payload_digest.as_bytes(),
    ] {
        jet_receipt_frame(&mut identity, field);
    }
    jet_receipt_digest(&identity)
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_check_capacity(directory: &std::path::Path, payload_bytes: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut count = 0usize;
    let mut total = 0u64;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            panic!("could not inspect receipt section");
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            panic!("receipt section record is not a regular file");
        }
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'))
        {
            continue;
        }
        count += 1;
        total = total.saturating_add(metadata.len());
    }
    if count >= JET_RECEIPT_SECTION_COUNT {
        panic!(
            "receipt has too many sections (maximum {})",
            JET_RECEIPT_SECTION_COUNT
        );
    }
    if total.saturating_add(payload_bytes as u64) > JET_RECEIPT_SECTION_BYTES_TOTAL {
        panic!(
            "receipt sections are too large (maximum {} bytes)",
            JET_RECEIPT_SECTION_BYTES_TOTAL
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn jet_receipt_publish(path: &std::path::Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else {
        panic!("receipt section path has no parent");
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let temp = parent.join(format!(".{}.{}.{}", path.display(), std::process::id(), stamp));
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
    {
        Ok(file) => file,
        Err(error) => panic!("could not stage receipt section: {error}"),
    };
    use std::io::Write;
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        let _ = std::fs::remove_file(&temp);
        panic!("could not flush receipt section");
    }
    match std::fs::hard_link(&temp, path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&temp);
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = std::fs::read(path).unwrap_or_default();
            let _ = std::fs::remove_file(&temp);
            if existing != bytes {
                panic!("receipt section identity collision with different content");
            }
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            panic!("could not publish receipt section: {error}");
        }
    }
}
