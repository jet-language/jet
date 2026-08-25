//! Exact identity gate for the one-shot local Nix compatibility fallback.
//!
//! This module does not resolve an ambient `nixpkgs` name. It binds the
//! executable bytes, reported Nix version, locked nixpkgs revision, target
//! system, attribute path, and canonical project-lock digest before a caller
//! may construct a fallback evaluation.

use crate::{JSON, Lock, SHA256};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROVENANCE_SCHEMA: &str = "jetpack-nix-fallback-v1";
const LOCKED_NIXPKGS_PREFIX: &str = "github:NixOS/nixpkgs#";
const SUPPORTED_SYSTEMS: &[&str] = &[
    "x86_64-linux",
    "aarch64-linux",
    "x86_64-darwin",
    "aarch64-darwin",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NixFallbackError(String);

impl NixFallbackError {
    fn new(detail: impl Into<String>) -> Self {
        Self(detail.into())
    }
}

impl fmt::Display for NixFallbackError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}

impl std::error::Error for NixFallbackError {}

/// All inputs that identify one permitted local Nix fallback evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NixFallbackIdentity {
    pub(crate) executable: PathBuf,
    pub(crate) executable_sha256: String,
    pub(crate) version: String,
    pub(crate) nixpkgs_revision: String,
    pub(crate) system: String,
    pub(crate) attr: Vec<String>,
    pub(crate) lock_sha256: String,
}

impl NixFallbackIdentity {
    /// Inspect one explicit executable and bind it to the exact project lock.
    /// `source_name` is the lock's declared source name, never a live channel
    /// lookup. The executable is invoked only for `--version` here.
    pub(crate) fn from_project(
        project: &Path,
        source_name: &str,
        executable: &Path,
        system: &str,
        attr: &[String],
    ) -> Result<Self, NixFallbackError> {
        let (lock_sha256, nixpkgs_revision) = exact_project_binding(project, source_name)?;
        let (executable, executable_sha256, version) = inspect_executable(executable)?;
        Self::from_observed(
            executable,
            executable_sha256,
            version,
            nixpkgs_revision,
            system,
            attr,
            lock_sha256,
        )
    }

    /// Construct an identity from already observed executable facts. The
    /// caller must obtain the facts from the exact executable and exact lock;
    /// all fields are still validated here before use.
    pub(crate) fn from_observed(
        executable: PathBuf,
        executable_sha256: String,
        version: String,
        nixpkgs_revision: String,
        system: &str,
        attr: &[String],
        lock_sha256: String,
    ) -> Result<Self, NixFallbackError> {
        let executable = canonical_executable_path(&executable)?;
        validate_sha256(&executable_sha256, "Nix executable")?;
        let actual_executable_sha256 = SHA256::sha256_hex(
            &fs::read(&executable).map_err(|error| {
                NixFallbackError::new(format!(
                    "could not read Nix executable `{}`: {error}",
                    executable.display()
                ))
            })?,
        );
        if executable_sha256 != actual_executable_sha256 {
            return Err(NixFallbackError::new(
                "Nix executable bytes differ from the bound executable identity",
            ));
        }
        validate_version(&version)?;
        validate_revision(&nixpkgs_revision)?;
        validate_system(system)?;
        validate_attr(attr)?;
        validate_sha256(&lock_sha256, "project lock")?;
        Ok(Self {
            executable,
            executable_sha256,
            version,
            nixpkgs_revision,
            system: system.to_string(),
            attr: attr.to_vec(),
            lock_sha256,
        })
    }

    /// Exact source input allowed for a fallback invocation. A bare
    /// `nixpkgs`, channel name, or floating flake ref has no representation in
    /// this API and therefore cannot reach the evaluator.
    pub(crate) fn locked_nixpkgs_input(&self) -> String {
        format!("{LOCKED_NIXPKGS_PREFIX}{}", self.nixpkgs_revision)
    }

    pub(crate) fn project_lock_sha256(
        project: &Path,
        source_name: &str,
    ) -> Result<String, NixFallbackError> {
        exact_project_lock(project, source_name)
    }

    pub(crate) fn attrpath(&self) -> String {
        self.attr.join(".")
    }

    /// Check the complete request immediately before the local Nix call.
    /// This keeps a caller from binding an identity and then silently
    /// substituting an ambient source, system, attr, or changed lock.
    pub(crate) fn validate_request(
        &self,
        nixpkgs_input: &str,
        system: &str,
        attr: &[String],
        lock_sha256: &str,
    ) -> Result<(), NixFallbackError> {
        if nixpkgs_input != self.locked_nixpkgs_input() {
            return Err(NixFallbackError::new(
                "local Nix fallback requires the exact locked nixpkgs revision",
            ));
        }
        if system != self.system {
            return Err(NixFallbackError::new(
                "local Nix fallback system differs from its bound identity",
            ));
        }
        if attr != self.attr {
            return Err(NixFallbackError::new(
                "local Nix fallback attr differs from its bound identity",
            ));
        }
        if lock_sha256 != self.lock_sha256 {
            return Err(NixFallbackError::new(
                "project lock changed before local Nix fallback evaluation",
            ));
        }
        Ok(())
    }

    /// Machine-readable provenance stored with a Jetpack import and its lock.
    pub(crate) fn provenance(&self) -> String {
        let attrs = self
            .attr
            .iter()
            .map(|value| JSON::quote(value))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"schema\":{},\"executable\":{},\"executable_sha256\":{},\"version\":{},\"nixpkgs_revision\":{},\"system\":{},\"attr\":[{}],\"lock_sha256\":{}}}",
            JSON::quote(PROVENANCE_SCHEMA),
            JSON::quote(&self.executable.to_string_lossy()),
            JSON::quote(&self.executable_sha256),
            JSON::quote(&self.version),
            JSON::quote(&self.nixpkgs_revision),
            JSON::quote(&self.system),
            attrs,
            JSON::quote(&self.lock_sha256),
        )
    }

    /// Decode and revalidate the exact identity persisted in a lock/receipt.
    pub(crate) fn from_provenance(value: &str) -> Result<Self, NixFallbackError> {
        let JSON::JSONValue::Object(fields) = JSON::parse(value)
            .map_err(|error| NixFallbackError::new(format!("invalid Nix fallback provenance: {error}")))?
        else {
            return Err(NixFallbackError::new(
                "Nix fallback provenance must be a JSON object",
            ));
        };
        const PROVENANCE_FIELDS: &[&str] = &[
            "schema",
            "executable",
            "executable_sha256",
            "version",
            "nixpkgs_revision",
            "system",
            "attr",
            "lock_sha256",
        ];
        if fields.len() != PROVENANCE_FIELDS.len()
            || PROVENANCE_FIELDS
                .iter()
                .any(|field| !fields.contains_key(*field))
        {
            return Err(NixFallbackError::new(
                "Nix fallback provenance has an unknown or missing identity field",
            ));
        }
        let schema = string_field(&fields, "schema")?;
        if schema != PROVENANCE_SCHEMA {
            return Err(NixFallbackError::new(
                "unsupported Nix fallback provenance schema",
            ));
        }
        let executable = PathBuf::from(string_field(&fields, "executable")?);
        let executable_sha256 = string_field(&fields, "executable_sha256")?;
        let version = string_field(&fields, "version")?;
        let nixpkgs_revision = string_field(&fields, "nixpkgs_revision")?;
        let system = string_field(&fields, "system")?;
        let attr = fields
            .get("attr")
            .ok_or_else(|| NixFallbackError::new("Nix fallback provenance is missing `attr`"))?
            .as_array()
            .map_err(NixFallbackError::new)?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .map_err(NixFallbackError::new)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let lock_sha256 = string_field(&fields, "lock_sha256")?;
        Self::from_observed(
            executable,
            executable_sha256,
            version,
            nixpkgs_revision,
            &system,
            &attr,
            lock_sha256,
        )
    }

    /// Re-hash and re-read the executable recorded in imported state before a
    /// later Jetpack-only run. A changed binary or version cannot reuse it.
    pub(crate) fn verify_executable(&self) -> Result<(), NixFallbackError> {
        let (path, digest, version) = inspect_executable(&self.executable)?;
        if path != self.executable || digest != self.executable_sha256 || version != self.version {
            return Err(NixFallbackError::new(
                "recorded Nix executable identity no longer matches the installed executable",
            ));
        }
        Ok(())
    }
}

fn inspect_executable(path: &Path) -> Result<(PathBuf, String, String), NixFallbackError> {
    let path = canonical_executable_path(path)?;
    let bytes = fs::read(&path).map_err(|error| {
        NixFallbackError::new(format!("could not read Nix executable `{}`: {error}", path.display()))
    })?;
    let digest = SHA256::sha256_hex(&bytes);
    let output = Command::new(&path)
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|error| {
            NixFallbackError::new(format!("could not inspect Nix executable `{}`: {error}", path.display()))
        })?;
    if !output.status.success() {
        return Err(NixFallbackError::new(format!(
            "Nix executable `{}` rejected `--version`",
            path.display()
        )));
    }
    let text = String::from_utf8(output.stdout)
        .map_err(|_| NixFallbackError::new("Nix `--version` output is not UTF-8"))?;
    let version = text
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("nix (Nix) "))
        .ok_or_else(|| NixFallbackError::new("Nix `--version` output has no exact Nix version"))?
        .trim()
        .to_string();
    validate_version(&version)?;
    Ok((path, digest, version))
}

fn canonical_executable_path(path: &Path) -> Result<PathBuf, NixFallbackError> {
    if path.as_os_str().is_empty() {
        return Err(NixFallbackError::new("Nix executable path is empty"));
    }
    let path = fs::canonicalize(path).map_err(|error| {
        NixFallbackError::new(format!("could not resolve Nix executable `{}`: {error}", path.display()))
    })?;
    let metadata = fs::metadata(&path).map_err(|error| {
        NixFallbackError::new(format!("could not stat Nix executable `{}`: {error}", path.display()))
    })?;
    if !metadata.is_file() {
        return Err(NixFallbackError::new(format!(
            "Nix executable `{}` is not a regular file",
            path.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(NixFallbackError::new(format!(
                "Nix executable `{}` is not executable",
                path.display()
            )));
        }
    }
    Ok(path)
}

fn exact_project_binding(
    project: &Path,
    source_name: &str,
) -> Result<(String, String), NixFallbackError> {
    let lock_path = crate::Store::lock_path(project);
    let metadata = fs::symlink_metadata(&lock_path).map_err(|error| {
        NixFallbackError::new(format!("could not inspect project lock `{}`: {error}", lock_path.display()))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(NixFallbackError::new(format!(
            "project lock `{}` is not a regular file",
            lock_path.display()
        )));
    }
    let raw = fs::read_to_string(&lock_path).map_err(|error| {
        NixFallbackError::new(format!("could not read project lock `{}`: {error}", lock_path.display()))
    })?;
    let lock = Lock::parse(&raw)
        .map_err(|error| NixFallbackError::new(format!("could not parse project lock: {error}")))?;
    let channel = lock
        .source_channels
        .iter()
        .find(|channel| {
            channel.name == source_name
                || (source_name == crate::Syntax::REF_SOURCE_JETPACK
                    && channel.name == crate::Syntax::REF_SOURCE_NIXPKGS)
        })
        .ok_or_else(|| {
            NixFallbackError::new(format!(
                "project lock has no exact source channel `{source_name}`"
            ))
        })?;
    let revision = revision_from_locked_input(&channel.exact)?;
    let mut canonical = lock;
    for package in &mut canonical.packages {
        package.receipt = None;
    }
    let digest = SHA256::sha256_hex(Lock::write(&canonical).as_bytes());
    validate_sha256(&digest, "project lock")?;
    Ok((digest, revision))
}

fn exact_project_lock(project: &Path, source_name: &str) -> Result<String, NixFallbackError> {
    exact_project_binding(project, source_name).map(|(digest, _)| digest)
}

fn revision_from_locked_input(input: &str) -> Result<String, NixFallbackError> {
    let revision = input
        .strip_prefix(LOCKED_NIXPKGS_PREFIX)
        .ok_or_else(|| {
            NixFallbackError::new(
                "local Nix fallback rejects ambient or unpinned nixpkgs input",
            )
        })?;
    validate_revision(revision)?;
    Ok(revision.to_string())
}

fn string_field(
    fields: &std::collections::BTreeMap<String, JSON::JSONValue>,
    name: &str,
) -> Result<String, NixFallbackError> {
    fields
        .get(name)
        .ok_or_else(|| NixFallbackError::new(format!("Nix fallback provenance is missing `{name}`")))?
        .as_str()
        .map(str::to_string)
        .map_err(NixFallbackError::new)
}

fn validate_version(version: &str) -> Result<(), NixFallbackError> {
    if version.is_empty()
        || !version
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        || version.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || !(character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '-' | '+' | '_'))
        })
    {
        return Err(NixFallbackError::new("Nix version is not exact"));
    }
    Ok(())
}

fn validate_revision(revision: &str) -> Result<(), NixFallbackError> {
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(NixFallbackError::new(
            "nixpkgs revision must be exactly 40 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn validate_system(system: &str) -> Result<(), NixFallbackError> {
    if !SUPPORTED_SYSTEMS.contains(&system) {
        return Err(NixFallbackError::new(format!(
            "unsupported Nix fallback system `{system}`"
        )));
    }
    Ok(())
}

fn validate_attr(attr: &[String]) -> Result<(), NixFallbackError> {
    if attr.is_empty()
        || attr.iter().any(|segment| {
            segment.is_empty()
                || segment.chars().any(|character| {
                    character.is_control() || character.is_whitespace() || character == '#'
                })
        })
    {
        return Err(NixFallbackError::new(
            "Nix fallback attr must be a non-empty exact attribute path",
        ));
    }
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), NixFallbackError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(NixFallbackError::new(format!(
            "{label} identity must be a 64-character lowercase SHA-256 digest"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const REVISION: &str = "b5aa0fbd538984f6e3d201be0005b4463d8b09f8";

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("jetpack-nix-fallback-{label}-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_project(root: &Path, exact: &str) {
        let managed = root.join(crate::Syntax::SOURCE_ROOT_DIR);
        fs::create_dir_all(&managed).unwrap();
        fs::write(
            managed.join("lock"),
            format!(
                "version = 1\n\n[[source_channel]]\nname = \"nixpkgs\"\nchannel = \"nixpkgs-unstable\"\nexact = \"{exact}\"\n\n[root]\ndependencies = []\n"
            ),
        )
        .unwrap();
    }

    fn observed(root: &Path) -> NixFallbackIdentity {
        let executable = root.join("nix");
        fs::write(&executable, b"nix-test-binary").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        NixFallbackIdentity::from_observed(
            executable.canonicalize().unwrap(),
            SHA256::sha256_hex(b"nix-test-binary"),
            "2.35.2".into(),
            REVISION.into(),
            "x86_64-linux",
            &["ripgrep".into()],
            "a".repeat(64),
        )
        .unwrap()
    }

    #[test]
    fn fallback_identity_contains_every_exact_input() {
        let root = temp_root("identity");
        let identity = observed(&root);
        assert_eq!(identity.locked_nixpkgs_input(), format!("{LOCKED_NIXPKGS_PREFIX}{REVISION}"));
        assert_eq!(identity.attrpath(), "ripgrep");
        let provenance = identity.provenance();
        let round_trip = NixFallbackIdentity::from_provenance(&provenance).unwrap();
        assert_eq!(round_trip, identity);
        assert!(provenance.contains("\"executable_sha256\""));
        assert!(provenance.contains("\"nixpkgs_revision\""));
        assert!(provenance.contains("\"lock_sha256\""));
        let with_unknown = provenance.replacen(
            ",\"lock_sha256\"",
            ",\"unexpected\":\"value\",\"lock_sha256\"",
            1,
        );
        assert!(NixFallbackIdentity::from_provenance(&with_unknown).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fallback_identity_rejects_ambient_nixpkgs_and_drift() {
        let root = temp_root("request");
        let identity = observed(&root);
        assert!(identity
            .validate_request("nixpkgs#ripgrep", "x86_64-linux", &["ripgrep".into()], &"a".repeat(64))
            .is_err());
        assert!(identity
            .validate_request(
                &identity.locked_nixpkgs_input(),
                "aarch64-linux",
                &["ripgrep".into()],
                &"a".repeat(64),
            )
            .is_err());
        assert!(identity
            .validate_request(
                &identity.locked_nixpkgs_input(),
                "x86_64-linux",
                &["jq".into()],
                &"a".repeat(64),
            )
            .is_err());
        assert!(identity
            .validate_request(
                &identity.locked_nixpkgs_input(),
                "x86_64-linux",
                &["ripgrep".into()],
                &"b".repeat(64),
            )
            .is_err());
        assert!(identity
            .validate_request(
                &identity.locked_nixpkgs_input(),
                "x86_64-linux",
                &["ripgrep".into()],
                &"a".repeat(64),
            )
            .is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_binding_rejects_unpinned_lock_input() {
        let root = temp_root("lock");
        write_project(&root, "nixpkgs#nixos-unstable");
        let executable = root.join("missing-nix");
        let error = exact_project_lock(&root, "nixpkgs").unwrap_err();
        assert!(error.to_string().contains("ambient or unpinned"));
        assert!(!executable.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn project_binding_reads_version_from_the_bound_executable() {
        let root = temp_root("bound");
        write_project(&root, &format!("{LOCKED_NIXPKGS_PREFIX}{REVISION}"));
        let executable = root.join("nix");
        fs::write(&executable, b"#!/bin/sh\nprintf 'nix (Nix) 2.35.2\\n'").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let identity = NixFallbackIdentity::from_project(
            &root,
            "nixpkgs",
            &executable,
            "x86_64-linux",
            &["ripgrep".into()],
        )
        .unwrap();
        assert_eq!(identity.version, "2.35.2");
        assert_eq!(identity.nixpkgs_revision, REVISION);
        assert_eq!(identity.attrpath(), "ripgrep");
        assert_eq!(identity.locked_nixpkgs_input(), format!("{LOCKED_NIXPKGS_PREFIX}{REVISION}"));
        fs::remove_dir_all(root).unwrap();
    }
}
