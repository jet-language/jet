use crate::Lock::LockFile;

// ──────────────────────────────────────────────
// SBOM generation (SPDX 2.3 tag-value)
// ──────────────────────────────────────────────

/// Generate an SPDX 2.3 tag-value SBOM from a lockfile.
///
/// Format: https://spdx.github.io/spdx-spec/v2.3/ (tag-value subset)
/// We emit the mandatory fields plus packages. The document namespace
/// is `https://jet-lang.org/spdx/<root-package>-<timestamp>`.
pub fn emit_spdx(lock: &LockFile, root_name: &str, root_version: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut out = String::new();

    // Document creation information.
    out.push_str("SPDXVersion: SPDX-2.3\n");
    out.push_str("DataLicense: CC0-1.0\n");
    out.push_str(&format!(
        "SPDXID: SPDXRef-DOCUMENT\n"
    ));
    out.push_str(&format!(
        "DocumentNamespace: https://jet-lang.org/spdx/{}-{}-{}\n",
        root_name, root_version, ts
    ));
    out.push_str(&format!(
        "DocumentName: {}-{}\n",
        root_name, root_version
    ));
    out.push_str("Creator: Tool: jet\n");
    out.push_str(&format!("Created: {}\n", spdx_timestamp(ts)));
    out.push_str("\n");

    // Root package.
    out.push_str("##### Root package\n\n");
    out.push_str(&format!("PackageName: {}\n", root_name));
    out.push_str("SPDXID: SPDXRef-root\n");
    out.push_str(&format!("PackageVersion: {}\n", root_version));
    out.push_str("FilesAnalyzed: false\n");
    out.push_str("PackageChecksum: NOASSERTION\n");
    out.push_str("PackageDownloadLocation: NOASSERTION\n");
    out.push_str("\n");

    // One package block per locked dependency.
    for (i, pkg) in lock.packages.iter().enumerate() {
        let spdx_id = format!("SPDXRef-pkg-{}", i);
        out.push_str(&format!("##### {}\n\n", pkg.name));
        out.push_str(&format!("PackageName: {}\n", pkg.name));
        out.push_str(&format!("SPDXID: {}\n", spdx_id));
        out.push_str(&format!("PackageVersion: {}\n", pkg.version));
        out.push_str("FilesAnalyzed: false\n");
        // The fingerprint is sha256-<hex>; SPDX uses SHA256: <hex>.
        let checksum = pkg
            .fingerprint
            .strip_prefix("sha256-")
            .map(|h| format!("SHA256: {}", h))
            .unwrap_or_else(|| "NOASSERTION".to_string());
        out.push_str(&format!("PackageChecksum: {}\n", checksum));
        out.push_str("PackageDownloadLocation: NOASSERTION\n");
        out.push_str("\n");

        // DESCRIBES relationship from root.
        out.push_str(&format!(
            "Relationship: SPDXRef-root DEPENDS_ON {}\n\n",
            spdx_id
        ));
    }

    out
}

/// Generate a CycloneDX 1.5 JSON SBOM from a lockfile.
pub fn emit_cyclonedx(lock: &LockFile, root_name: &str, root_version: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut components = Vec::new();
    for (i, pkg) in lock.packages.iter().enumerate() {
        let hash_val = pkg
            .fingerprint
            .strip_prefix("sha256-")
            .unwrap_or(&pkg.fingerprint);
        components.push(format!(
            r#"    {{
      "type": "library",
      "bom-ref": "pkg-{i}",
      "name": "{name}",
      "version": "{version}",
      "hashes": [{{ "alg": "SHA-256", "content": "{hash}" }}]
    }}"#,
            i = i,
            name = json_escape(&pkg.name),
            version = json_escape(&pkg.version),
            hash = json_escape(hash_val),
        ));
    }

    format!(
        r#"{{
  "bomFormat": "CycloneDX",
  "specVersion": "1.5",
  "serialNumber": "urn:uuid:jet-{ts}",
  "version": 1,
  "metadata": {{
    "timestamp": "{timestamp}",
    "tools": [{{ "name": "jet" }}],
    "component": {{
      "type": "library",
      "name": "{root_name}",
      "version": "{root_version}"
    }}
  }},
  "components": [
{components}
  ]
}}
"#,
        ts = ts,
        timestamp = iso8601(ts),
        root_name = json_escape(root_name),
        root_version = json_escape(root_version),
        components = components.join(",\n"),
    )
}

fn spdx_timestamp(secs: u64) -> String {
    // Simple ISO8601: 2026-01-01T00:00:00Z (we don't have chrono — I6)
    iso8601(secs)
}

pub(crate) fn iso8601(secs: u64) -> String {
    // Minimal ISO 8601 without chrono. We compute date parts from the epoch.
    // Accurate for years 1970–2100 (Gregorian, no leap seconds).
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let mut days = secs / 86400;

    let mut year = 1970u64;
    loop {
        let y_days = if is_leap(year) { 366 } else { 365 };
        if days < y_days {
            break;
        }
        days -= y_days;
        year += 1;
    }
    let months = if is_leap(year) {
        [31u64, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31u64, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in &months {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}
