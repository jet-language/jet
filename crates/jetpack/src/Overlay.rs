//! Jetpack package overlays and overrides (D-JPK-OVERLAY1=A).
//!
//! Pure data types and parse/strip helpers live in `jet-pkg-model::Overlay`;
//! this module re-exports them and adds the engine-only operations (patch
//! application, semantic lock records, override drafting) that need
//! `SemanticLock` / filesystem write access.

pub use jet_pkg_model::Overlay::{
    balanced_with_len, find_workspace_body_start, top_level_commas, unquote,
    OverrideProvenance, OverlayError, OverlayPolicy, OverlaySet, PackageOverride,
    PatchApplication, ProviderOverride, ResolvedPackageOverride, parse_workspace_policy,
    strip_overlay_policy,
};

use super::SemanticLock::{LockIdentity, LockRationale, LockRecordKind, SemanticRecord};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

pub fn apply_overlay_patches(
    workspace_root: &Path,
    source_root: &Path,
    package: &PackageOverride,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let workspace_root = canonical_directory(workspace_root, "workspace root")?;
    let source_root = canonical_directory(source_root, "source root")?;
    let mut staged = BTreeMap::new();
    let mut applied = Vec::new();
    for patch in &package.patches {
        let patch_path = safe_existing_path(&workspace_root, patch, "patch")?;
        let text = std::fs::read_to_string(&patch_path).map_err(|e| {
            OverlayError::IO(format!(
                "could not read patch `{}`: {e}",
                patch_path.display()
            ))
        })?;
        applied.extend(apply_unified_patch_staged(
            &source_root,
            &text,
            &mut staged,
        )?);
    }
    commit_staged(&staged)?;
    Ok(applied)
}

#[cfg(test)]
fn apply_unified_patch(
    source_root: &Path,
    patch_text: &str,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let source_root = canonical_directory(source_root, "source root")?;
    let mut staged = BTreeMap::new();
    let applications = apply_unified_patch_staged(&source_root, patch_text, &mut staged)?;
    commit_staged(&staged)?;
    Ok(applications)
}

#[derive(Debug)]
struct StagedFile {
    path: std::path::PathBuf,
    original: Vec<u8>,
    output: Vec<u8>,
}

fn apply_unified_patch_staged(
    source_root: &Path,
    patch_text: &str,
    staged: &mut BTreeMap<std::path::PathBuf, StagedFile>,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let mut applications = Vec::new();
    let mut lines = patch_text.lines().peekable();
    while let Some(line) = lines.next() {
        if !line.starts_with("--- ") {
            continue;
        }
        let Some(next) = lines.next() else {
            return Err(OverlayError::Patch("patch missing `+++` line".to_string()));
        };
        if !next.starts_with("+++ ") {
            return Err(OverlayError::Patch("patch missing `+++` line".to_string()));
        }
        let target = normalize_patch_path(next.trim_start_matches("+++ ").trim());
        let relative = safe_relative_path(&target, "patched file")?;
        let path = source_root.join(&relative);
        let mut cursor = source_root.to_path_buf();
        for component in relative.components() {
            cursor.push(component.as_os_str());
            if let Ok(metadata) = std::fs::symlink_metadata(&cursor) {
                if metadata.file_type().is_symlink() {
                    return Err(OverlayError::Patch(format!(
                        "patched file `{target}` contains a symlink"
                    )));
                }
            }
        }
        let canonical_path = path.canonicalize().map_err(|e| {
            OverlayError::IO(format!(
                "could not resolve patched file `{}`: {e}",
                path.display()
            ))
        })?;
        if !canonical_path.starts_with(source_root) {
            return Err(OverlayError::Patch(format!(
                "patched file `{target}` escapes the source root"
            )));
        }
        if !staged.contains_key(&canonical_path) {
            let original = std::fs::read(&canonical_path).map_err(|error| {
                OverlayError::IO(format!(
                    "could not read patched file `{}`: {error}",
                    path.display()
                ))
            })?;
            staged.insert(
                canonical_path.clone(),
                StagedFile {
                    path: canonical_path.clone(),
                    output: original.clone(),
                    original,
                },
            );
        }
        let entry = staged
            .get_mut(&canonical_path)
            .expect("staged file was inserted or already present");
        let original = entry.output.clone();
        let mut file_lines: Vec<String> = String::from_utf8(original.clone())
            .map_err(|_| {
                OverlayError::Patch(format!(
                    "patched file `{target}` is not valid UTF-8"
                ))
            })?
            .lines()
            .map(|s| s.to_string())
            .collect();
        let trailing_newline = original.ends_with(b"\n");
        let mut added = 0usize;
        let mut removed = 0usize;
        let mut had_hunk = false;
        while matches!(lines.peek(), Some(l) if l.starts_with("@@ ")) {
            had_hunk = true;
            let header = lines.next().ok_or_else(|| {
                OverlayError::Patch("patch hunk header ended unexpectedly".to_string())
            })?;
            let (old_start, old_count, new_count) = parse_hunk_range(header)?;
            let mut idx = old_start.saturating_sub(1);
            let mut old_seen = 0usize;
            let mut new_seen = 0usize;
            loop {
                if old_seen == old_count && new_seen == new_count {
                    break;
                }
                let hline = lines.next().ok_or_else(|| {
                    OverlayError::Patch("patch hunk body ended unexpectedly".to_string())
                })?;
                if hline == r"\ No newline at end of file" {
                    continue;
                }
                let Some(tag) = hline.chars().next() else {
                    return Err(OverlayError::Patch(
                        "unsupported empty patch line".to_string(),
                    ));
                };
                let text = &hline[tag.len_utf8()..];
                match tag {
                    ' ' => {
                        if file_lines.get(idx).map(String::as_str) != Some(text) {
                            return Err(OverlayError::Patch(format!(
                                "patch context did not match `{target}`"
                            )));
                        }
                        idx += 1;
                        old_seen += 1;
                        new_seen += 1;
                    }
                    '-' => {
                        if file_lines.get(idx).map(String::as_str) != Some(text) {
                            return Err(OverlayError::Patch(format!(
                                "patch removal did not match `{target}`"
                            )));
                        }
                        file_lines.remove(idx);
                        removed += 1;
                        old_seen += 1;
                    }
                    '+' => {
                        if idx > file_lines.len() {
                            return Err(OverlayError::Patch(format!(
                                "patch insertion is outside `{target}`"
                            )));
                        }
                        file_lines.insert(idx, text.to_string());
                        idx += 1;
                        added += 1;
                        new_seen += 1;
                    }
                    _ => {
                        return Err(OverlayError::Patch(format!(
                            "unsupported patch line `{hline}`"
                        )));
                    }
                }
            }
        }
        if !had_hunk {
            return Err(OverlayError::Patch(format!(
                "patch for `{target}` has no hunks"
            )));
        }
        let mut output = file_lines.join("\n");
        if trailing_newline {
            output.push('\n');
        }
        entry.output = output.into_bytes();
        applications.push(PatchApplication {
            path: target,
            added,
            removed,
        });
    }
    if applications.is_empty() {
        return Err(OverlayError::Patch(
            "patch contains no file hunks".to_string(),
        ));
    }
    Ok(applications)
}

fn canonical_directory(path: &Path, label: &str) -> Result<std::path::PathBuf, OverlayError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        OverlayError::IO(format!("could not inspect {label} `{}`: {error}", path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(OverlayError::IO(format!(
            "{label} `{}` is not a real directory",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        OverlayError::IO(format!("could not resolve {label} `{}`: {error}", path.display()))
    })
}

fn safe_relative_path(raw: &str, label: &str) -> Result<std::path::PathBuf, OverlayError> {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::RootDir
            )
        })
    {
        return Err(OverlayError::Patch(format!(
            "{label} path `{raw}` must be a non-empty relative path without `..`"
        )));
    }
    Ok(path.to_path_buf())
}

fn safe_existing_path(
    root: &Path,
    raw: &str,
    label: &str,
) -> Result<std::path::PathBuf, OverlayError> {
    let relative = safe_relative_path(raw, label)?;
    let candidate = root.join(&relative);
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        if let Ok(metadata) = std::fs::symlink_metadata(&cursor) {
            if metadata.file_type().is_symlink() {
                return Err(OverlayError::Patch(format!(
                    "{label} `{raw}` contains a symlink"
                )));
            }
        }
    }
    let metadata = std::fs::symlink_metadata(&candidate).map_err(|error| {
        OverlayError::IO(format!("could not inspect {label} `{}`: {error}", candidate.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(OverlayError::Patch(format!(
            "{label} `{}` must be a regular file inside the workspace",
            candidate.display()
        )));
    }
    let canonical = candidate.canonicalize().map_err(|error| {
        OverlayError::IO(format!("could not resolve {label} `{}`: {error}", candidate.display()))
    })?;
    if !canonical.starts_with(root) {
        return Err(OverlayError::Patch(format!(
            "{label} `{raw}` escapes the workspace root"
        )));
    }
    Ok(canonical)
}

fn commit_staged(staged: &BTreeMap<std::path::PathBuf, StagedFile>) -> Result<(), OverlayError> {
    let mut committed: Vec<(&Path, &[u8])> = Vec::new();
    for entry in staged.values() {
        if entry.output == entry.original {
            continue;
        }
        if let Err(error) = write_staged_file(entry) {
            let mut rollback_errors = Vec::new();
            for (path, original) in committed.iter().rev() {
                if let Err(rollback) = write_bytes_atomically(path, original) {
                    rollback_errors.push(format!("{}: {rollback}", path.display()));
                }
            }
            let suffix = if rollback_errors.is_empty() {
                String::new()
            } else {
                format!("; rollback failed: {}", rollback_errors.join("; "))
            };
            return Err(OverlayError::IO(format!(
                "could not commit patched file `{}`: {error}{suffix}",
                entry.path.display()
            )));
        }
        committed.push((&entry.path, entry.original.as_slice()));
    }
    Ok(())
}

fn write_staged_file(entry: &StagedFile) -> std::io::Result<()> {
    write_bytes_atomically(&entry.path, &entry.output)
}

fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("patched file has no parent"))?;
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("file");
    let serial = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.jet-overlay-{}-{serial}", std::process::id()));
    let result = (|| {
        use std::io::Write as _;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if let Ok(metadata) = std::fs::metadata(path) {
            std::fs::set_permissions(&temporary, metadata.permissions())?;
        }
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn parse_hunk_range(header: &str) -> Result<(usize, usize, usize), OverlayError> {
    let old = header
        .split_whitespace()
        .find(|part| part.starts_with('-'))
        .ok_or_else(|| OverlayError::Patch("patch hunk missing old range".to_string()))?;
    let new = header
        .split_whitespace()
        .find(|part| part.starts_with('+'))
        .ok_or_else(|| OverlayError::Patch("patch hunk missing new range".to_string()))?;
    let (old_start, old_count) = parse_hunk_side(old, '-')?;
    let (_, new_count) = parse_hunk_side(new, '+')?;
    Ok((old_start, old_count, new_count))
}

fn parse_hunk_side(raw: &str, prefix: char) -> Result<(usize, usize), OverlayError> {
    let mut parts = raw.trim_start_matches(prefix).split(',');
    let start = parts
        .next()
        .unwrap_or_default()
        .parse::<usize>()
        .map_err(|_| OverlayError::Patch("patch hunk has bad range".to_string()))?;
    let count = parts
        .next()
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| OverlayError::Patch("patch hunk has bad range".to_string()))?
        .unwrap_or(1);
    if parts.next().is_some() {
        return Err(OverlayError::Patch("patch hunk has bad range".to_string()));
    }
    Ok((start, count))
}

fn normalize_patch_path(raw: &str) -> String {
    raw.split_whitespace()
        .next()
        .unwrap_or(raw)
        .trim_start_matches("a/")
        .trim_start_matches("b/")
        .to_string()
}

pub fn semantic_records(
    policy: &OverlayPolicy,
    owner: &str,
    platform: &str,
) -> Result<Vec<SemanticRecord>, OverlayError> {
    let mut records = Vec::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            let resolved = policy
                .resolve_package_override_checked(&package.package)?
                .ok_or_else(|| {
                    OverlayError::Malformed(format!(
                        "overlay `{}` package `{}` disappeared during resolution",
                        overlay.name, package.package
                    ))
                })?;
            let exact = resolved
                .version
                .clone()
                .or_else(|| resolved.source.clone())
                .unwrap_or_else(|| "source-policy".to_string());
            let policy_fingerprint = resolved_policy_fingerprint(&resolved, overlay);
            let mut rationales = resolved
                .provenance
                .iter()
                .map(|fact| {
                    let source_overlay = policy.overlay(&fact.overlay);
                    let provider = source_overlay
                        .and_then(|overlay| overlay.provider.as_ref())
                        .map(|provider| provider.provider.clone())
                        .unwrap_or_else(|| "unchanged".to_string());
                    let channel = source_overlay
                        .and_then(|overlay| overlay.provider.as_ref())
                        .and_then(|provider| provider.channel.clone())
                        .unwrap_or_default();
                    LockRationale {
                        owner_package: owner.to_string(),
                        reason: format!(
                            "overlay `{}` contributed `{}` = `{}` at priority {}",
                            fact.overlay, fact.field, fact.value, fact.priority
                        ),
                        source_ref: resolved
                            .source
                            .clone()
                            .unwrap_or_else(|| resolved.package.clone()),
                        provider,
                        channel_input: channel,
                        exact_output: exact.clone(),
                        policy_fingerprint: policy_fingerprint.clone(),
                        recipe_id: resolved.patches.join(","),
                        adapter_id: "workspace.overlay".to_string(),
                        signature: String::new(),
                        cache_provenance: "workspace.jet".to_string(),
                        update_command: "jetpack override draft".to_string(),
                    }
                })
                .collect::<Vec<_>>();
            if rationales.is_empty() {
                rationales.push(LockRationale {
                    owner_package: owner.to_string(),
                    reason: "workspace package overlay".to_string(),
                    source_ref: resolved
                        .source
                        .clone()
                        .unwrap_or_else(|| resolved.package.clone()),
                    provider: overlay
                        .provider
                        .as_ref()
                        .map(|provider| provider.provider.clone())
                        .unwrap_or_else(|| "unchanged".to_string()),
                    channel_input: overlay
                        .provider
                        .as_ref()
                        .and_then(|provider| provider.channel.clone())
                        .unwrap_or_default(),
                    exact_output: exact.clone(),
                    policy_fingerprint: policy_fingerprint.clone(),
                    recipe_id: resolved.patches.join(","),
                    adapter_id: "workspace.overlay".to_string(),
                    signature: String::new(),
                    cache_provenance: "workspace.jet".to_string(),
                    update_command: "jetpack override draft".to_string(),
                });
            }
            let record = SemanticRecord {
                identity: LockIdentity {
                    kind: LockRecordKind::PackageOverlay,
                    key: format!("{}:{}", overlay.name, package.package),
                    exact,
                    hash: policy_fingerprint,
                    platform: platform.to_string(),
                },
                rationales,
                future_fields: resolved_fact_fields(&resolved, &overlay.name),
            };
            records.push(record);
        }
    }
    Ok(records)
}

fn resolved_policy_fingerprint(
    resolved: &ResolvedPackageOverride,
    overlay: &OverlaySet,
) -> String {
    let provider = overlay
        .provider
        .as_ref()
        .map(|provider| {
            provider
                .channel
                .as_ref()
                .map(|channel| format!("{}#{channel}", provider.provider))
                .unwrap_or_else(|| provider.provider.clone())
        })
        .unwrap_or_else(|| "provider:unchanged".to_string());
    format!("{}:{}", resolved.policy_fingerprint(), provider)
}

fn resolved_fact_fields(
    resolved: &ResolvedPackageOverride,
    overlay: &str,
) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    fields.insert("overlay-fact-overlay".to_string(), overlay.to_string());
    fields.insert("overlay-fact-package".to_string(), resolved.package.clone());
    fields.insert(
        "overlay-fact-source".to_string(),
        resolved.source.clone().unwrap_or_default(),
    );
    fields.insert(
        "overlay-fact-version".to_string(),
        resolved.version.clone().unwrap_or_default(),
    );
    fields.insert("overlay-fact-flags".to_string(), resolved.flags.join(","));
    fields.insert(
        "overlay-fact-env".to_string(),
        resolved
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(","),
    );
    fields.insert(
        "overlay-fact-patches".to_string(),
        resolved.patches.join(","),
    );
    fields.insert(
        "overlay-fact-allow-unfree".to_string(),
        resolved.allow_unfree.to_string(),
    );
    fields.insert(
        "overlay-fact-priority".to_string(),
        resolved.priority.to_string(),
    );
    for (field, priority) in &resolved.field_priorities {
        fields.insert(
            format!("overlay-field-priority.{field}"),
            priority.to_string(),
        );
    }
    for (index, fact) in resolved.provenance.iter().enumerate() {
        let prefix = format!("overlay-provenance.{index}");
        fields.insert(format!("{prefix}.overlay"), fact.overlay.clone());
        fields.insert(format!("{prefix}.order"), fact.order.to_string());
        fields.insert(format!("{prefix}.field"), fact.field.clone());
        fields.insert(format!("{prefix}.value"), fact.value.clone());
        fields.insert(format!("{prefix}.priority"), fact.priority.to_string());
    }
    fields
}

/// Map `(overlay, package)` → policy fingerprint for exact invalidation diffs.
pub fn policy_fingerprints(
    policy: &OverlayPolicy,
) -> Result<BTreeMap<(String, String), String>, OverlayError> {
    let mut out = BTreeMap::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            let resolved = policy
                .resolve_package_override_checked(&package.package)?
                .ok_or_else(|| {
                    OverlayError::Malformed(format!(
                        "overlay `{}` package `{}` disappeared during resolution",
                        overlay.name, package.package
                    ))
                })?;
            out.insert(
                (overlay.name.clone(), package.package.clone()),
                resolved_policy_fingerprint(&resolved, overlay),
            );
        }
    }
    Ok(out)
}

/// Diff overlay policies and return exact action invalidations (E4-JP13).
pub fn invalidations_against(
    before: &OverlayPolicy,
    after: &OverlayPolicy,
    actions_by_package: &std::collections::BTreeMap<String, Vec<String>>,
) -> Result<Vec<super::SemanticLock::OverlayInvalidation>, OverlayError> {
    Ok(super::SemanticLock::overlay_invalidations(
        &policy_fingerprints(before)?,
        &policy_fingerprints(after)?,
        actions_by_package,
    ))
}

pub fn draft_overlay_source(
    existing: Option<&str>,
    overlay: &str,
    package: &str,
    patch: Option<&str>,
    provider: Option<&str>,
    channel: Option<&str>,
    allow_unfree: bool,
) -> String {
    let block = draft_block(overlay, package, patch, provider, channel, allow_unfree);
    match existing {
        Some(src) if find_workspace_body_start(src).is_some() => {
            let idx = src.rfind('}').unwrap_or(src.len());
            let mut out = String::new();
            out.push_str(src[..idx].trim_end());
            out.push_str("\n\n");
            out.push_str(&block);
            out.push('\n');
            out.push_str(&src[idx..]);
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out
        }
        _ => {
            let mut out = String::new();
            out.push_str("module workspace {\n");
            out.push_str("    members: []\n\n");
            out.push_str(&block);
            out.push_str("}\n");
            out
        }
    }
}

fn draft_block(
    overlay: &str,
    package: &str,
    patch: Option<&str>,
    provider: Option<&str>,
    channel: Option<&str>,
    allow_unfree: bool,
) -> String {
    let mut out = String::new();
    out.push_str(&format!("    overlay {overlay} {{\n"));
    if let Some(provider) = provider {
        match channel {
            Some(channel) => out.push_str(&format!(
                "        provider: Provider.{provider}(channel: \"{channel}\")\n"
            )),
            None => out.push_str(&format!("        provider: Provider.{provider}\n")),
        }
    }
    out.push_str("        overrides: {\n");
    out.push_str(&format!("            \"{package}\": .{{\n"));
    if let Some(patch) = patch {
        out.push_str(&format!(
            "                patches: [patch(\"{patch}\")],\n"
        ));
    }
    if allow_unfree {
        out.push_str(&format!(
            "                allowUnfree: true,\n"
        ));
    }
    out.push_str("            },\n        }\n");
    out.push_str("    }\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_records_capture_policy_reason() {
        let policy = parse_workspace_policy(
            r#"module workspace {
    overlay beta {
        provider: Provider.nixpkgs(channel: "unstable")
        package("foo").version: "1.2.3"
    }
}"#,
        )
        .unwrap();
        let records = semantic_records(&policy, "app", "x86_64-linux").unwrap();
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.identity.kind, LockRecordKind::PackageOverlay);
        assert_eq!(rec.identity.key, "beta:foo");
        assert_eq!(rec.rationales[0].provider, "nixpkgs");
        assert_eq!(rec.rationales[0].channel_input, "unstable");
        assert_eq!(rec.rationales[0].update_command, "jetpack override draft");
    }

    #[test]
    fn applies_unified_patch() {
        let root = std::env::temp_dir().join(format!("jet-overlay-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\ntwo\nthree\n").unwrap();
        let patch = "\
--- a/src/file.txt
+++ b/src/file.txt
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
";
        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied[0].path, "src/file.txt");
        assert_eq!(applied[0].added, 1);
        assert_eq!(applied[0].removed, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn malformed_hunk_header_returns_patch_error() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-bad-hunk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ malformed\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "patch hunk missing old range");
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn adjacent_file_boundary_preserves_both_valid_patches() {
        let root =
            std::env::temp_dir().join(format!("jet-overlay-boundary-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/one.txt"), "one\n").unwrap();
        std::fs::write(root.join("src/two.txt"), "two\n").unwrap();
        let patch = "--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/two.txt\n+++ b/src/two.txt\n@@ -1 +1 @@\n-two\n+TWO\n";

        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied.len(), 2);
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "ONE\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/two.txt")).unwrap(),
            "TWO\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_hunk_body_line_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-empty-hunk-line-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "unsupported empty patch line");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn unicode_hunk_body_tag_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-unicode-hunk-tag-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\néone\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "unsupported patch line `éone`");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removal_content_starting_with_file_header_marker_is_not_a_boundary() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-removal-marker-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "-- text\n").unwrap();
        let patch =
            "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n--- text\n+replacement\n";

        let applied = apply_unified_patch(&root, patch).unwrap();
        assert_eq!(applied[0].removed, 1);
        assert_eq!(applied[0].added, 1);
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "replacement\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn out_of_range_insertion_returns_patch_error() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-insertion-range-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch =
            "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -999,0 +999,1 @@\n+replacement\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert_eq!(err.message(), "patch insertion is outside `src/file.txt`");
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn patch_paths_cannot_escape_and_leave_source_unchanged() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-traversal-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/../outside.txt\n@@ -1 +1 @@\n-one\n+ONE\n";

        let err = apply_unified_patch(&root, patch).unwrap_err();
        assert!(err.message().contains("must be a non-empty relative path"));
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "one\n"
        );
        assert!(!root.join("outside.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_file_patch_failure_is_transactional() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-transaction-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/one.txt"), "one\n").unwrap();
        std::fs::write(root.join("src/two.txt"), "two\n").unwrap();
        let patch = "--- a/src/one.txt\n+++ b/src/one.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/two.txt\n+++ b/src/two.txt\n@@ -1 +1 @@\n-wrong\n+TWO\n";

        assert!(apply_unified_patch(&root, patch).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("src/one.txt")).unwrap(),
            "one\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/two.txt")).unwrap(),
            "two\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_file_patches_compose_in_staging_order() {
        let root = std::env::temp_dir().join(format!(
            "jet-overlay-repeat-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/file.txt"), "one\ntwo\n").unwrap();
        let patch = "--- a/src/file.txt\n+++ b/src/file.txt\n@@ -1 +1 @@\n-one\n+ONE\n--- a/src/file.txt\n+++ b/src/file.txt\n@@ -2 +2 @@\n-two\n+TWO\n";

        apply_unified_patch(&root, patch).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("src/file.txt")).unwrap(),
            "ONE\nTWO\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn draft_writes_workspace_source() {
        let src = draft_overlay_source(
            None,
            "plasma_beta",
            "kdePackages.plasma-desktop",
            Some("patches/focus.patch"),
            Some("nixpkgs"),
            Some("plasma-beta"),
            true,
        );
        assert!(src.contains("module workspace"));
        assert!(src.contains("overlay plasma_beta"));
        assert!(src.contains("Provider.nixpkgs(channel: \"plasma-beta\")"));
        assert!(src.contains("patch(\"patches/focus.patch\")"));
        assert!(src.contains("allowUnfree: true"));
    }
}
