//! Pure authority seam for Jetpack's native Nix evaluator.
//!
//! The dependency-free `no_std` baseline is reinforced by mandatory
//! resolved-symbol lints against process and network authority. Forbidden
//! unsafe code prevents native FFI and dynamic loading.

#![no_std]
#![forbid(unsafe_code)]
#![deny(warnings)]
#![deny(clippy::disallowed_methods)]
#![deny(clippy::disallowed_types)]
#![deny(clippy::std_instead_of_alloc)]
#![deny(clippy::std_instead_of_core)]
#![deny(unused_extern_crates)]
#![allow(non_snake_case)]
#![allow(clippy::upper_case_acronyms)] // Jet S66 requires canonical `JSON`.

extern crate alloc;

mod JSON;

mod Authority {
    #![allow(dead_code)] // Ordered slices B-F consume this authority.

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum InternalStage {
        Syntax,
        Values,
        Evaluation,
        Authority,
        Derivation,
        Flakes,
    }

    #[derive(Debug)]
    pub(crate) struct InternalTestHarness {
        private: (),
    }

    #[derive(Debug)]
    pub(crate) struct InternalStagePermit {
        stage: InternalStage,
    }

    impl InternalStagePermit {
        pub(crate) fn stage(&self) -> InternalStage {
            self.stage
        }
    }

    #[cfg(test)]
    pub(crate) fn test_harness() -> InternalTestHarness {
        InternalTestHarness { private: () }
    }

    pub(crate) fn authorize_internal(
        _harness: &InternalTestHarness,
        stage: InternalStage,
    ) -> InternalStagePermit {
        InternalStagePermit { stage }
    }
}

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use JSON::JSON as JSONValue;

const ORACLE_JSON: &str = include_str!("../../../tests/fixtures/nix-compat/oracle.json");
const NIX_VERSION: &str = "2.34.8";
const NIX_TAG_OBJECT: &str = "b6769c588f60b3e762f73d3a8cf60294df078ccd";
const NIX_SOURCE_COMMIT: &str = "f3f1c3c5b8ad91850e0f7c590cf177f7ab022024";
const NIXPKGS_REVISION: &str = "b5aa0fbd538984f6e3d201be0005b4463d8b09f8";
const NIXPKGS_LAST_MODIFIED: u64 = 1_782_723_713;
const NIXPKGS_NAR_HASH: &str = "sha256-oPXCU/SSUokcGaJREHibG1CBX3+s/W7orDWQOZDsEeQ=";
const REQUIRED_SYSTEMS: [&str; 4] = [
    "aarch64-darwin",
    "aarch64-linux",
    "x86_64-darwin",
    "x86_64-linux",
];
const MAX_EVALUATOR_INPUT_BYTES: usize = 1 << 20;
const MAX_DEV_SHELL_PACKAGES: usize = 256;
const MAX_PACKAGE_NAME_BYTES: usize = 128;

/// A typed projection of the supported, non-executing devShell surface.
///
/// This is deliberately smaller than the Nix language. It evaluates literal
/// package lists and records fields that have no Jet environment meaning. No
/// derivation, hook, import, or arbitrary function is executed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevShellEvaluation {
    system: String,
    packages: Vec<String>,
    unsupported: Vec<String>,
}

impl DevShellEvaluation {
    pub fn system(&self) -> &str {
        &self.system
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn unsupported(&self) -> &[String] {
        &self.unsupported
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationError {
    InputTooLarge,
    UnsupportedSystem(String),
    Unsupported(String),
    Invalid(String),
    ResourceLimit(String),
}

impl fmt::Display for EvaluationError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge => write!(output, "foreign flake exceeds the 1 MiB evaluator limit"),
            Self::UnsupportedSystem(system) => {
                write!(output, "native flake evaluation does not support system `{system}`")
            }
            Self::Unsupported(reason) => write!(output, "unsupported foreign flake expression: {reason}"),
            Self::Invalid(reason) => write!(output, "invalid foreign flake expression: {reason}"),
            Self::ResourceLimit(reason) => write!(output, "foreign flake evaluator limit exceeded: {reason}"),
        }
    }
}

impl core::error::Error for EvaluationError {}

/// Evaluate the bounded native devShell surface.
pub fn evaluate_devshell(
    source: &str,
    system: &str,
) -> core::result::Result<DevShellEvaluation, EvaluationError> {
    if source.len() > MAX_EVALUATOR_INPUT_BYTES {
        return Err(EvaluationError::InputTooLarge);
    }
    if !REQUIRED_SYSTEMS.contains(&system) {
        return Err(EvaluationError::UnsupportedSystem(system.to_string()));
    }

    let source = strip_comments(source)?;
    let has_shell = source.contains("devShell") || source.contains("mkShell");
    if !has_shell {
        return Err(EvaluationError::Unsupported(
            "no supported `devShell` or `mkShell` output was found".into(),
        ));
    }

    let mut packages = Vec::new();
    for field in ["packages", "buildInputs", "nativeBuildInputs"] {
        for assignment in assignments(&source, field) {
            if !in_shell_context(&source, assignment) {
                continue;
            }
            let (items, _) = list_after_assignment(&source, assignment, field)?;
            for package in items {
                if packages.len() >= MAX_DEV_SHELL_PACKAGES {
                    return Err(EvaluationError::ResourceLimit(format!(
                        "devShell has more than {MAX_DEV_SHELL_PACKAGES} packages"
                    )));
                }
                packages.push(package);
            }
        }
    }

    packages.sort();
    packages.dedup();
    let mut unsupported = Vec::new();
    for assignment in assignments(&source, "shellHook") {
        if !in_shell_context(&source, assignment) {
            continue;
        }
        let value = skip_space(&source, assignment + "shellHook".len());
        if source.as_bytes().get(value) != Some(&b'=') {
            continue;
        }
        let value = skip_space(&source, value + 1);
        match quoted_string(&source, value) {
            Some((hook, _)) if !hook.trim().is_empty() => unsupported.push("shellHook".into()),
            Some(_) => {}
            None => unsupported.push("shellHook".into()),
        }
    }
    unsupported.sort();
    unsupported.dedup();

    Ok(DevShellEvaluation {
        system: system.to_string(),
        packages,
        unsupported,
    })
}

fn strip_comments(source: &str) -> core::result::Result<String, EvaluationError> {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut i = 0;
    let mut in_string = false;
    let mut escaped = false;
    while i < bytes.len() {
        if in_string {
            let byte = bytes[i];
            if escaped {
                let character = source[i..]
                    .chars()
                    .next()
                    .ok_or_else(|| EvaluationError::Invalid("invalid UTF-8 boundary".into()))?;
                output.push(character);
                escaped = false;
                i += character.len_utf8();
                continue;
            } else if byte == b'\\' {
                output.push('\\');
                escaped = true;
            } else if byte == b'"' {
                output.push('"');
                in_string = false;
            } else {
                let character = source[i..]
                    .chars()
                    .next()
                    .ok_or_else(|| EvaluationError::Invalid("invalid UTF-8 boundary".into()))?;
                output.push(character);
                i += character.len_utf8();
                continue;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            output.push('"');
            i += 1;
            continue;
        }
        if bytes[i] == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
            output.push_str("''");
            i += 2;
            while i + 1 < bytes.len()
                && !(bytes[i] == b'\'' && bytes[i + 1] == b'\'')
            {
                if bytes[i] == b'\n' {
                    output.push('\n');
                } else {
                    output.push(' ');
                }
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(EvaluationError::Invalid(
                    "unterminated indented string".into(),
                ));
            }
            output.push_str("''");
            i += 2;
            continue;
        }
        if bytes[i] == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            if i < bytes.len() {
                output.push('\n');
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            let start = i;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 >= bytes.len() {
                return Err(EvaluationError::Invalid("unterminated comment".into()));
            }
            if source[start..i].contains('\n') {
                output.push('\n');
            } else {
                output.push(' ');
            }
            i += 2;
            continue;
        }
        let character = source[i..]
            .chars()
            .next()
            .ok_or_else(|| EvaluationError::Invalid("invalid UTF-8 boundary".into()))?;
        output.push(character);
        i += character.len_utf8();
    }
    if in_string {
        return Err(EvaluationError::Invalid("unterminated string".into()));
    }
    Ok(output)
}

fn assignments(source: &str, field: &str) -> Vec<usize> {
    let mut result = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(field) {
        let start = offset + found;
        let before = source.as_bytes().get(start.wrapping_sub(1)).copied();
        let after = source.as_bytes().get(start + field.len()).copied();
        if !before.is_some_and(is_identifier_byte)
            && !after.is_some_and(is_identifier_byte)
            && skip_space(source, start + field.len()) < source.len()
            && source.as_bytes()[skip_space(source, start + field.len())] == b'='
        {
            result.push(start);
        }
        offset = start + field.len();
    }
    result
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn in_shell_context(source: &str, assignment: usize) -> bool {
    let prefix = &source[..assignment];
    let boundary = prefix
        .rfind("};")
        .map_or(0, |index| index + 2);
    prefix[boundary..].contains("mkShell")
        || prefix[boundary..].contains("devShells")
        || prefix[boundary..].contains("devShell")
}

fn list_after_assignment(
    source: &str,
    field: usize,
    name: &str,
) -> core::result::Result<(Vec<String>, usize), EvaluationError> {
    let mut cursor = skip_space(source, field + name.len());
    if source.as_bytes().get(cursor) != Some(&b'=') {
        return Err(EvaluationError::Invalid(format!("`{name}` is not an assignment")));
    }
    cursor = skip_space(source, cursor + 1);
    if source[cursor..].starts_with("with ") {
        let Some(separator) = source[cursor..].find(';') else {
            return Err(EvaluationError::Invalid(format!("`{name}` has an incomplete `with`")));
        };
        let scope = source[cursor + 5..cursor + separator].trim();
        if scope != "pkgs" && scope != "legacyPackages" {
            return Err(EvaluationError::Unsupported(format!(
                "`{name}` imports package scope `{scope}`"
            )));
        }
        cursor = skip_space(source, cursor + separator + 1);
    }
    if source.as_bytes().get(cursor) != Some(&b'[') {
        return Err(EvaluationError::Unsupported(format!(
            "`{name}` must be a literal package list"
        )));
    }
    let end = matching(source, cursor, b'[', b']')?;
    Ok((parse_package_list(&source[cursor + 1..end])?, end + 1))
}

fn matching(
    source: &str,
    start: usize,
    open: u8,
    close: u8,
) -> core::result::Result<usize, EvaluationError> {
    let mut depth = 0;
    let mut cursor = start;
    while cursor < source.len() {
        match source.as_bytes()[cursor] {
            byte if byte == open => depth += 1,
            byte if byte == close => {
                depth -= 1;
                if depth == 0 {
                    return Ok(cursor);
                }
            }
            b'"' => {
                let Some((_, end)) = quoted_string(source, cursor) else {
                    return Err(EvaluationError::Invalid("unterminated string".into()));
                };
                cursor = end;
                continue;
            }
            _ => {}
        }
        cursor += 1;
    }
    Err(EvaluationError::Invalid("unterminated package list".into()))
}

fn parse_package_list(list: &str) -> core::result::Result<Vec<String>, EvaluationError> {
    let mut packages = Vec::new();
    let mut cursor = 0;
    while cursor < list.len() {
        cursor = skip_list_space(list, cursor);
        if cursor == list.len() {
            break;
        }
        let start = cursor;
        let mut parentheses = 0;
        while list.as_bytes().get(cursor) == Some(&b'(') {
            parentheses += 1;
            cursor = skip_list_space(list, cursor + 1);
        }
        let package_start = cursor;
        if list.as_bytes().get(cursor) == Some(&b'"') {
            let (_, end) = quoted_string(list, cursor)
                .ok_or_else(|| EvaluationError::Invalid("unterminated package name".into()))?;
            cursor = end;
        } else {
            while list.as_bytes().get(cursor).is_some_and(package_byte) {
                cursor += 1;
            }
        }
        if cursor == package_start {
            return Err(EvaluationError::Unsupported(format!(
                "package expression near `{}`",
                &list[start..list.len().min(start + 24)]
            )));
        }
        let raw = list[package_start..cursor].trim_matches('"');
        let package = package_name(raw)?;
        for _ in 0..parentheses {
            cursor = skip_list_space(list, cursor);
            if list.as_bytes().get(cursor) != Some(&b')') {
                return Err(EvaluationError::Unsupported(format!(
                    "package `{package}` is not a literal value"
                )));
            }
            cursor += 1;
        }
        cursor = skip_list_space(list, cursor);
        if list.as_bytes().get(cursor).is_some_and(|byte| *byte == b'(') {
            return Err(EvaluationError::Unsupported(format!(
                "package `{package}` uses a function call"
            )));
        }
        packages.push(package);
    }
    Ok(packages)
}

fn package_byte(byte: &u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'+' | b'.' | b'/')
}

fn package_name(raw: &str) -> core::result::Result<String, EvaluationError> {
    let raw = raw
        .strip_prefix("pkgs.")
        .or_else(|| raw.strip_prefix("legacyPackages."))
        .unwrap_or(raw);
    let raw = raw.rsplit('.').next().unwrap_or(raw);
    if raw.is_empty() || raw.len() > MAX_PACKAGE_NAME_BYTES || raw.contains('$') {
        return Err(EvaluationError::Unsupported(format!(
            "invalid package name `{raw}`"
        )));
    }
    Ok(raw.to_string())
}

fn skip_space(source: &str, mut cursor: usize) -> usize {
    while source.as_bytes().get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    cursor
}

fn skip_list_space(source: &str, mut cursor: usize) -> usize {
    cursor = skip_space(source, cursor);
    while source.as_bytes().get(cursor) == Some(&b',') {
        cursor = skip_space(source, cursor + 1);
    }
    cursor
}

fn quoted_string(source: &str, start: usize) -> Option<(String, usize)> {
    if source.as_bytes().get(start) != Some(&b'"') {
        return None;
    }
    let mut output = String::new();
    let mut cursor = start + 1;
    let mut escaped = false;
    while cursor < source.len() {
        let byte = source.as_bytes()[cursor];
        if escaped {
            output.push(byte as char);
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Some((output, cursor + 1));
        } else {
            output.push(byte as char);
        }
        cursor += 1;
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundaryError {
    Manifest(String),
    Evaluation(String),
    UnsupportedOracleSystem(String),
    MissingOracleIdentity {
        system: String,
        field: &'static str,
    },
    MalformedSRI {
        field: &'static str,
    },
    OracleIdentityMismatch {
        system: String,
        field: &'static str,
        expected: String,
        actual: String,
    },
    OracleBuildBlocked {
        system: String,
        status: String,
    },
}

impl fmt::Display for BoundaryError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Manifest(reason) => write!(output, "invalid pinned Nix oracle manifest: {reason}"),
            Self::Evaluation(reason) => write!(output, "native Nix evaluation failed: {reason}"),
            Self::UnsupportedOracleSystem(system) => {
                write!(output, "unsupported pinned Nix oracle system `{system}`")
            }
            Self::MissingOracleIdentity { system, field } => {
                write!(output, "pinned Nix oracle `{system}` is missing `{field}`")
            }
            Self::MalformedSRI { field } => {
                write!(output, "observed Nix oracle `{field}` is not canonical SHA-256 SRI")
            }
            Self::OracleIdentityMismatch {
                system,
                field,
                expected,
                actual,
            } => write!(
                output,
                "pinned Nix oracle `{system}` `{field}` mismatch: expected `{expected}`, got `{actual}`"
            ),
            Self::OracleBuildBlocked { system, status } => {
                write!(output, "pinned Nix oracle `{system}` is `{status}`")
            }
        }
    }
}

impl core::error::Error for BoundaryError {}

type Result<T> = core::result::Result<T, BoundaryError>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleBuild {
    build_nar_hash: Option<String>,
    executable_nar_hash: Option<String>,
    status: String,
}

/// Exact, fully validated oracle authority. Raw manifests cannot construct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedOracleManifest {
    nix_version: String,
    nix_tag_object: String,
    nix_source_commit: String,
    builds: BTreeMap<String, OracleBuild>,
    nixpkgs_revision: String,
    nixpkgs_last_modified: u64,
    nixpkgs_nar_hash: String,
    corpus_status: String,
}

impl ValidatedOracleManifest {
    pub fn embedded() -> Result<Self> {
        Self::parse_and_validate(ORACLE_JSON)
    }

    fn parse_and_validate(text: &str) -> Result<Self> {
        let manifest = Self::parse_raw(text)?;
        manifest.validate_pin()?;
        Ok(manifest)
    }

    fn parse_raw(text: &str) -> Result<Self> {
        let root = JSON::parse(text).map_err(BoundaryError::Manifest)?;
        let root = object(&root, "root")?;
        exact_keys(root, &["schema", "nix", "nixpkgs", "corpus_status"], "root")?;
        let schema = root.get("schema").ok_or_else(|| missing("root.schema"))?;
        if !matches!(schema, JSONValue::Num(value) if *value == 1.0) {
            return Err(BoundaryError::Manifest("`schema` must be 1".into()));
        }

        let nix = child_object(root, "nix")?;
        exact_keys(nix, &["version", "tag_object", "source_commit", "builds"], "nix")?;
        let builds_object = child_object(nix, "builds")?;
        let mut builds = BTreeMap::new();
        for (system, value) in builds_object {
            let build = object(value, &format!("nix.builds.{system}"))?;
            exact_keys(
                build,
                &["build_nar_hash", "executable_nar_hash", "status"],
                &format!("nix.builds.{system}"),
            )?;
            builds.insert(
                system.clone(),
                OracleBuild {
                    build_nar_hash: optional_sri(build, "build_nar_hash")?,
                    executable_nar_hash: optional_sri(build, "executable_nar_hash")?,
                    status: required_string(build, "status")?,
                },
            );
        }

        let nixpkgs = child_object(root, "nixpkgs")?;
        exact_keys(nixpkgs, &["rev", "last_modified", "nar_hash"], "nixpkgs")?;
        let nixpkgs_nar_hash = required_string(nixpkgs, "nar_hash")?;
        canonical_sha256_sri(&nixpkgs_nar_hash).map_err(|_| {
            BoundaryError::Manifest("`nixpkgs.nar_hash` must be canonical SHA-256 SRI".into())
        })?;

        Ok(Self {
            nix_version: required_string(nix, "version")?,
            nix_tag_object: required_string(nix, "tag_object")?,
            nix_source_commit: required_string(nix, "source_commit")?,
            builds,
            nixpkgs_revision: required_string(nixpkgs, "rev")?,
            nixpkgs_last_modified: required_u64(nixpkgs, "last_modified")?,
            nixpkgs_nar_hash,
            corpus_status: required_string(root, "corpus_status")?,
        })
    }

    fn validate_pin(&self) -> Result<()> {
        exact("nix.version", &self.nix_version, NIX_VERSION)?;
        exact("nix.tag_object", &self.nix_tag_object, NIX_TAG_OBJECT)?;
        exact("nix.source_commit", &self.nix_source_commit, NIX_SOURCE_COMMIT)?;
        exact("nixpkgs.rev", &self.nixpkgs_revision, NIXPKGS_REVISION)?;
        if self.nixpkgs_last_modified != NIXPKGS_LAST_MODIFIED {
            return Err(BoundaryError::Manifest(format!(
                "`nixpkgs.last_modified` must be `{NIXPKGS_LAST_MODIFIED}`, got `{}`",
                self.nixpkgs_last_modified
            )));
        }
        exact("nixpkgs.nar_hash", &self.nixpkgs_nar_hash, NIXPKGS_NAR_HASH)?;
        let systems: Vec<&str> = self.builds.keys().map(String::as_str).collect();
        if systems != REQUIRED_SYSTEMS {
            return Err(BoundaryError::Manifest(format!(
                "oracle systems must be exactly {REQUIRED_SYSTEMS:?}, got {systems:?}"
            )));
        }
        for (system, build) in &self.builds {
            if build.status != "blocked" && build.status != "ready" {
                return Err(BoundaryError::Manifest(format!(
                    "nix.builds.{system}.status must be `blocked` or `ready`"
                )));
            }
        }
        if self.corpus_status != "blocked_on_oracle_build_hashes"
            && self.corpus_status != "bit_exact"
        {
            return Err(BoundaryError::Manifest(
                "`corpus_status` must be `blocked_on_oracle_build_hashes` or `bit_exact`".into(),
            ));
        }
        Ok(())
    }

    pub fn verify_oracle(&self, observed: &OracleBuildIdentity) -> Result<VerifiedOracle> {
        let expected = self
            .builds
            .get(&observed.system)
            .ok_or_else(|| BoundaryError::UnsupportedOracleSystem(observed.system.clone()))?;
        let build_hash = expected.build_nar_hash.as_deref().ok_or_else(|| {
            BoundaryError::MissingOracleIdentity {
                system: observed.system.clone(),
                field: "build_nar_hash",
            }
        })?;
        let executable_hash = expected.executable_nar_hash.as_deref().ok_or_else(|| {
            BoundaryError::MissingOracleIdentity {
                system: observed.system.clone(),
                field: "executable_nar_hash",
            }
        })?;
        compare_identity(
            &observed.system,
            "build_nar_hash",
            build_hash,
            &observed.build_nar_hash,
        )?;
        compare_identity(
            &observed.system,
            "executable_nar_hash",
            executable_hash,
            &observed.executable_nar_hash,
        )?;
        if expected.status != "ready" {
            return Err(BoundaryError::OracleBuildBlocked {
                system: observed.system.clone(),
                status: expected.status.clone(),
            });
        }
        Ok(VerifiedOracle {
            system: observed.system.clone(),
        })
    }

    pub fn nix_version(&self) -> &str {
        &self.nix_version
    }

    pub fn nix_tag_object(&self) -> &str {
        &self.nix_tag_object
    }

    pub fn nix_source_commit(&self) -> &str {
        &self.nix_source_commit
    }

    pub fn nixpkgs_revision(&self) -> &str {
        &self.nixpkgs_revision
    }

    pub fn nixpkgs_last_modified(&self) -> u64 {
        self.nixpkgs_last_modified
    }

    pub fn nixpkgs_nar_hash(&self) -> &str {
        &self.nixpkgs_nar_hash
    }

    pub fn evaluator_identity(&self, system: &str) -> Result<String> {
        if !REQUIRED_SYSTEMS.contains(&system) {
            return Err(BoundaryError::UnsupportedOracleSystem(system.to_string()));
        }
        Ok(format!(
            "native-nix:{}:{}:{}",
            self.nix_version, self.nixpkgs_revision, system
        ))
    }

    pub fn systems(&self) -> Vec<&str> {
        self.builds.keys().map(String::as_str).collect()
    }

    pub fn product_ready(&self) -> bool {
        self.corpus_status == "bit_exact"
            && self.builds.values().all(|build| {
                build.status == "ready"
                    && build.build_nar_hash.is_some()
                    && build.executable_nar_hash.is_some()
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleBuildIdentity {
    system: String,
    build_nar_hash: String,
    executable_nar_hash: String,
}

impl OracleBuildIdentity {
    pub fn new(system: &str, build_nar_hash: &str, executable_nar_hash: &str) -> Result<Self> {
        canonical_sha256_sri(build_nar_hash).map_err(|_| BoundaryError::MalformedSRI {
            field: "build_nar_hash",
        })?;
        canonical_sha256_sri(executable_nar_hash).map_err(|_| BoundaryError::MalformedSRI {
            field: "executable_nar_hash",
        })?;
        Ok(Self {
            system: system.to_string(),
            build_nar_hash: build_nar_hash.to_string(),
            executable_nar_hash: executable_nar_hash.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOracle {
    system: String,
}

impl VerifiedOracle {
    pub fn system(&self) -> &str {
        &self.system
    }
}

fn object<'a>(value: &'a JSONValue, path: &str) -> Result<&'a BTreeMap<String, JSONValue>> {
    value
        .as_object()
        .map_err(|_| BoundaryError::Manifest(format!("`{path}` must be an object")))
}

fn child_object<'a>(
    parent: &'a BTreeMap<String, JSONValue>,
    key: &str,
) -> Result<&'a BTreeMap<String, JSONValue>> {
    let value = parent.get(key).ok_or_else(|| missing(key))?;
    object(value, key)
}

fn required_string(parent: &BTreeMap<String, JSONValue>, key: &str) -> Result<String> {
    parent
        .get(key)
        .ok_or_else(|| missing(key))?
        .as_str()
        .map(ToString::to_string)
        .map_err(|_| BoundaryError::Manifest(format!("`{key}` must be a string")))
}

fn required_u64(parent: &BTreeMap<String, JSONValue>, key: &str) -> Result<u64> {
    match parent.get(key).ok_or_else(|| missing(key))? {
        JSONValue::Num(value)
            if value.is_finite()
                && *value >= 0.0
                && *value <= u64::MAX as f64
                && (*value as u64) as f64 == *value =>
        {
            Ok(*value as u64)
        }
        _ => Err(BoundaryError::Manifest(format!(
            "`{key}` must be an unsigned integer"
        ))),
    }
}

fn optional_sri(parent: &BTreeMap<String, JSONValue>, key: &'static str) -> Result<Option<String>> {
    match parent.get(key).ok_or_else(|| missing(key))? {
        JSONValue::Null => Ok(None),
        JSONValue::Str(value) => {
            canonical_sha256_sri(value).map_err(|_| {
                BoundaryError::Manifest(format!(
                    "`{key}` must be canonical SHA-256 SRI"
                ))
            })?;
            Ok(Some(value.clone()))
        }
        _ => Err(BoundaryError::Manifest(format!(
            "`{key}` must be a string or null"
        ))),
    }
}

fn exact_keys(
    parent: &BTreeMap<String, JSONValue>,
    expected: &[&str],
    path: &str,
) -> Result<()> {
    let actual: Vec<&str> = parent.keys().map(String::as_str).collect();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    if actual != expected {
        return Err(BoundaryError::Manifest(format!(
            "`{path}` keys must be exactly {expected:?}, got {actual:?}"
        )));
    }
    Ok(())
}

fn missing(path: &str) -> BoundaryError {
    BoundaryError::Manifest(format!("missing `{path}`"))
}

fn exact(field: &str, actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        return Err(BoundaryError::Manifest(format!(
            "`{field}` must be `{expected}`, got `{actual}`"
        )));
    }
    Ok(())
}

fn compare_identity(
    system: &str,
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<()> {
    if expected != actual {
        return Err(BoundaryError::OracleIdentityMismatch {
            system: system.to_string(),
            field,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn canonical_sha256_sri(value: &str) -> core::result::Result<[u8; 32], ()> {
    let encoded = value.strip_prefix("sha256-").ok_or(())?;
    let bytes = encoded.as_bytes();
    if bytes.len() != 44 || bytes[43] != b'=' {
        return Err(());
    }
    let mut decoded = [0_u8; 32];
    let mut output = 0;
    for quartet in 0..11 {
        let offset = quartet * 4;
        let a = base64_value(bytes[offset]).ok_or(())?;
        let b = base64_value(bytes[offset + 1]).ok_or(())?;
        let c = base64_value(bytes[offset + 2]).ok_or(())?;
        if quartet == 10 {
            if c & 0b11 != 0 {
                return Err(());
            }
            decoded[output] = (a << 2) | (b >> 4);
            decoded[output + 1] = (b << 4) | (c >> 2);
            output += 2;
        } else {
            let d = base64_value(bytes[offset + 3]).ok_or(())?;
            decoded[output] = (a << 2) | (b >> 4);
            decoded[output + 1] = (b << 4) | (c >> 2);
            decoded[output + 2] = (c << 6) | d;
            output += 3;
        }
    }
    if output == decoded.len() {
        Ok(decoded)
    } else {
        Err(())
    }
}

fn base64_value(value: u8) -> Option<u8> {
    match value {
        b'A'..=b'Z' => Some(value - b'A'),
        b'a'..=b'z' => Some(value - b'a' + 26),
        b'0'..=b'9' => Some(value - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
