//! Jetpack package overlays and overrides (D-JPK-OVERLAY1=A).
//!
//! Pure data types and parse/strip helpers live in `jet-pkg-model::Overlay`;
//! this module re-exports them and adds the engine-only operations (patch
//! application, semantic lock records, override drafting) that need
//! `SemanticLock` / filesystem write access.

pub use jet_pkg_model::Overlay::{
    balanced_with_len, find_workspace_body_start, top_level_commas, unquote,
    OverlayError, OverlayPolicy, OverlaySet, PackageOverride, PatchApplication,
    ProviderOverride, parse_workspace_policy, strip_overlay_policy,
};

use super::SemanticLock::{LockIdentity, LockRationale, LockRecordKind, SemanticRecord};
use std::path::Path;

pub fn apply_overlay_patches(
    workspace_root: &Path,
    source_root: &Path,
    package: &PackageOverride,
) -> Result<Vec<PatchApplication>, OverlayError> {
    let mut applied = Vec::new();
    for patch in &package.patches {
        let patch_path = workspace_root.join(patch);
        let text = std::fs::read_to_string(&patch_path).map_err(|e| {
            OverlayError::IO(format!(
                "could not read patch `{}`: {e}",
                patch_path.display()
            ))
        })?;
        applied.extend(apply_unified_patch(source_root, &text)?);
    }
    Ok(applied)
}

fn apply_unified_patch(
    source_root: &Path,
    patch_text: &str,
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
        let path = source_root.join(&target);
        let original = std::fs::read_to_string(&path).map_err(|e| {
            OverlayError::IO(format!(
                "could not read patched file `{}`: {e}",
                path.display()
            ))
        })?;
        let mut file_lines: Vec<String> = original.lines().map(|s| s.to_string()).collect();
        let trailing_newline = original.ends_with('\n');
        let mut added = 0usize;
        let mut removed = 0usize;
        while matches!(lines.peek(), Some(l) if l.starts_with("@@ ")) {
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
        let mut output = file_lines.join("\n");
        if trailing_newline {
            output.push('\n');
        }
        std::fs::write(&path, output).map_err(|e| {
            OverlayError::IO(format!(
                "could not write patched file `{}`: {e}",
                path.display()
            ))
        })?;
        applications.push(PatchApplication {
            path: target,
            added,
            removed,
        });
    }
    Ok(applications)
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
) -> Vec<SemanticRecord> {
    let mut records = Vec::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            let provider = overlay
                .provider
                .as_ref()
                .map(|p| p.provider.clone())
                .unwrap_or_else(|| "unchanged".to_string());
            let channel = overlay
                .provider
                .as_ref()
                .and_then(|p| p.channel.clone())
                .unwrap_or_default();
            let exact = package
                .version
                .clone()
                .or_else(|| package.source.clone())
                .unwrap_or_else(|| "source-policy".to_string());
            records.push(SemanticRecord::new(
                LockIdentity {
                    kind: LockRecordKind::PackageOverlay,
                    key: format!("{}:{}", overlay.name, package.package),
                    exact,
                    hash: package.policy_fingerprint(overlay),
                    platform: platform.to_string(),
                },
                LockRationale {
                    owner_package: owner.to_string(),
                    reason: "workspace package overlay".to_string(),
                    source_ref: package
                        .source
                        .clone()
                        .unwrap_or_else(|| package.package.clone()),
                    provider,
                    channel_input: channel,
                    exact_output: package.package.clone(),
                    policy_fingerprint: package.policy_fingerprint(overlay),
                    recipe_id: package.patches.join(","),
                    adapter_id: "workspace.overlay".to_string(),
                    signature: String::new(),
                    cache_provenance: "workspace.jet".to_string(),
                    update_command: "jetpack override draft".to_string(),
                },
            ));
        }
    }
    records
}

/// Map `(overlay, package)` → policy fingerprint for exact invalidation diffs.
pub fn policy_fingerprints(
    policy: &OverlayPolicy,
) -> std::collections::BTreeMap<(String, String), String> {
    let mut out = std::collections::BTreeMap::new();
    for overlay in &policy.overlays {
        for package in &overlay.packages {
            out.insert(
                (overlay.name.clone(), package.package.clone()),
                package.policy_fingerprint(overlay),
            );
        }
    }
    out
}

/// Diff overlay policies and return exact action invalidations (E4-JP13).
pub fn invalidations_against(
    before: &OverlayPolicy,
    after: &OverlayPolicy,
    actions_by_package: &std::collections::BTreeMap<String, Vec<String>>,
) -> Vec<super::SemanticLock::OverlayInvalidation> {
    super::SemanticLock::overlay_invalidations(
        &policy_fingerprints(before),
        &policy_fingerprints(after),
        actions_by_package,
    )
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
    if let Some(patch) = patch {
        out.push_str(&format!(
            "        package(\"{package}\").patches += [patch(\"{patch}\")]\n"
        ));
    }
    if allow_unfree {
        out.push_str(&format!(
            "        package(\"{package}\").allowUnfree: true\n"
        ));
    }
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
        let records = semantic_records(&policy, "app", "x86_64-linux");
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
