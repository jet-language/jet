//! Reversible source transitions for the unified Package surface.
//!
//! This module owns the file-moving part of the package.jet transition. It
//! does not invent a second package evaluator: every forward plan is checked
//! with PackageFacts and every reverse plan is checked against the exact bytes
//! recorded by its journal. A plan is safe to preview, apply, and fold without
//! relying on timestamps or last-writer-wins behavior.

use std::fmt;
use std::fs;
use std::collections::BTreeSet;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Package::{ConfigFacts, PackageFacts, PackageOutputKind, PackageParseError};
use crate::RuntimePolicy;

const PACKAGE_FILE: &str = "package.jet";
const JOURNAL_DIR: &str = ".jet/package-transition";
const JOURNAL_HEADER: &str = "jet-package-transition-v1";
const MAX_JOURNAL_BYTES: usize = 64 * 1024 * 1024;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SplitTarget {
    Environment,
    Package { name: String },
    Hosts { name: String },
}

impl SplitTarget {
    fn operation(&self) -> String {
        match self {
            Self::Environment => "split-env".to_string(),
            Self::Package { name } => format!("split-package-{name}"),
            Self::Hosts { name } => format!("split-hosts-{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeSummary {
    pub path: PathBuf,
    pub action: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionSummary {
    pub operation: String,
    pub changes: Vec<ChangeSummary>,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
    pub journal: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionResult {
    pub summary: TransitionSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionError(pub String);

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TransitionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileChange {
    relative: PathBuf,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransitionPlan {
    root: PathBuf,
    operation: String,
    changes: Vec<FileChange>,
    journal_path: PathBuf,
    journal: Vec<u8>,
    before_fingerprint: String,
    after_fingerprint: String,
    reverse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Journal {
    operation: String,
    before_fingerprint: String,
    after_fingerprint: String,
    changes: Vec<FileChange>,
}

/// Plan and apply one of the ratified jet split transitions.
pub fn split(root: &Path, target: SplitTarget, check_only: bool) -> Result<TransitionResult, TransitionError> {
    let plan = split_plan(root, target)?;
    let summary = plan.summary();
    if check_only {
        return Ok(TransitionResult { summary });
    }
    plan.apply()
}

/// Plan and apply the reverse of a recorded transition. path is the generated
/// file named by jet fold, relative to the Package root or an absolute path
/// inside it.
pub fn fold(root: &Path, path: &Path, check_only: bool) -> Result<TransitionResult, TransitionError> {
    let plan = fold_plan(root, path)?;
    let summary = plan.summary();
    if check_only {
        return Ok(TransitionResult { summary });
    }
    plan.apply()
}

/// Preview or apply the migration-era role-file fold used by jet init.
///
/// The converter accepts only closed, typed role files. An old file with an
/// open or unknown field is rejected before any file is changed; silently
/// copying such a file into package.jet would create a second interpretation
/// of the graph.
pub fn init(root: &Path, check_only: bool) -> Result<TransitionResult, TransitionError> {
    let plan = legacy_plan(root)?;
    let summary = plan.summary();
    if check_only {
        return Ok(TransitionResult { summary });
    }
    plan.apply()
}

/// Restore the last successful role-file migration. This is deliberately
/// separate from fold: it is the explicit migration-epoch escape hatch
/// promised by jet init --restore-role-files.
pub fn restore_role_files(root: &Path, check_only: bool) -> Result<TransitionResult, TransitionError> {
    let root = canonical_root(root)?;
    let journal_path = latest_legacy_journal(&root)?;
    let journal = read_journal(&journal_path)?;
    if !journal.operation.starts_with("legacy-role-fold") {
        return Err(TransitionError(format!(
            "journal {} is not a role-file migration",
            journal_path.display()
        )));
    }
    let plan = TransitionPlan {
        root,
        operation: format!("restore-{}", journal.operation),
        changes: journal.changes,
        journal_path,
        journal: Vec::new(),
        before_fingerprint: journal.after_fingerprint,
        after_fingerprint: journal.before_fingerprint,
        reverse: true,
    };
    let summary = plan.summary();
    if check_only {
        return Ok(TransitionResult { summary });
    }
    plan.apply()
}

impl TransitionPlan {
    fn summary(&self) -> TransitionSummary {
        let changes = self
            .changes
            .iter()
            .filter_map(|change| {
                let (old, new) = if self.reverse {
                    (&change.after, &change.before)
                } else {
                    (&change.before, &change.after)
                };
                if old == new {
                    return None;
                }
                let action = match (old.is_some(), new.is_some()) {
                    (false, true) => "create",
                    (true, false) => "remove",
                    (true, true) => "update",
                    (false, false) => return None,
                };
                Some(ChangeSummary {
                    path: change.relative.clone(),
                    action,
                })
            })
            .collect();
        TransitionSummary {
            operation: self.operation.clone(),
            changes,
            before_fingerprint: self.before_fingerprint.clone(),
            after_fingerprint: self.after_fingerprint.clone(),
            journal: self.journal_path.clone(),
        }
    }

    fn apply(self) -> Result<TransitionResult, TransitionError> {
        let root = self.root.clone();
        RuntimePolicy::with_lock(&root, "package-transition", || {
            self.apply_locked().map_err(|error| {
                io::Error::new(io::ErrorKind::Other, error.0)
            })
        })
        .map_err(|error| TransitionError(format!("couldn't lock package transition: {error}")))
    }

    fn apply_locked(self) -> Result<TransitionResult, TransitionError> {
        let target_is_forward = !self.reverse;
        for change in &self.changes {
            let path = safe_path(&self.root, &change.relative)?;
            let current = read_state(&path)?;
            let expected = if target_is_forward {
                &change.before
            } else {
                &change.after
            };
            if current != *expected {
                return Err(TransitionError(format!(
                    "stale transition: {} changed after the plan was created; rerun jet {} --check",
                    change.relative.display(),
                    if self.reverse { "fold" } else { "split" }
                )));
            }
        }

        let mut snapshots = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let path = safe_path(&self.root, &change.relative)?;
            snapshots.push((path.clone(), read_state(&path)?));
        }
        let result = (|| {
            for change in &self.changes {
                let path = safe_path(&self.root, &change.relative)?;
                let desired = if target_is_forward {
                    &change.after
                } else {
                    &change.before
                };
                apply_state(&self.root, &path, desired.as_deref())?;
            }
            if self.reverse {
                if self.journal_path.exists() {
                    remove_regular(&self.root, &self.journal_path)?;
                }
            } else {
                let journal_parent = self
                    .journal_path
                    .parent()
                    .ok_or_else(|| TransitionError("transition journal has no parent".to_string()))?;
                create_real_dirs(&self.root, journal_parent)?;
                write_atomic(&self.journal_path, &self.journal)?;
            }
            Ok::<(), TransitionError>(())
        })();
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for (change, (path, state)) in self.changes.iter().zip(snapshots.iter()).rev() {
                let expected_written = if target_is_forward {
                    change.after.as_deref()
                } else {
                    change.before.as_deref()
                };
                match read_state(path) {
                    Ok(current) if current.as_ref() == state.as_ref() => {}
                    Ok(current) if current.as_deref() == expected_written => {
                        if let Err(rollback) = apply_state(&self.root, path, state.as_deref()) {
                            rollback_errors.push(format!("{}: {rollback}", change.relative.display()));
                        }
                    }
                    Ok(_) => rollback_errors.push(format!(
                        "{} changed during rollback; its newer bytes were preserved",
                        change.relative.display()
                    )),
                    Err(read_error) => rollback_errors.push(format!(
                        "{} could not be inspected during rollback: {read_error}",
                        change.relative.display()
                    )),
                }
            }
            if rollback_errors.is_empty() {
                return Err(error);
            }
            return Err(TransitionError(format!(
                "{error}; rollback was incomplete: {}",
                rollback_errors.join("; ")
            )));
        }
        let summary = self.summary();
        Ok(TransitionResult { summary })
    }
}

fn split_plan(root: &Path, target: SplitTarget) -> Result<TransitionPlan, TransitionError> {
    let root = canonical_root(root)?;
    let package_path = root.join(PACKAGE_FILE);
    let package_bytes = read_regular(&package_path)?;
    let package_text = std::str::from_utf8(&package_bytes)
        .map_err(|_| TransitionError(format!("{PACKAGE_FILE} must be UTF-8")))?;
    let package = PackageFacts::load(&root)
        .ok_or_else(|| TransitionError(format!("no {PACKAGE_FILE} found in {}", root.display())))?
        .map_err(|error| package_error(&package_path, error))?;
    let entries = source_entries(package_text);
    let operation = target.operation();
    let mut changes = Vec::new();
    let mut root_after: String;
    let mut semantic_before = semantic_fingerprint(&package);
    let mut semantic_after = semantic_before.clone();

    match target {
        SplitTarget::Environment => {
            let (entry, value) = required_field(&entries, "environments", &package_path)?;
            let names = record_names(&value, "environments")?;
            if names.is_empty() {
                return Err(TransitionError(
                    "environments is empty; there is no closed fact to split".to_string(),
                ));
            }
            let config_name = if names.len() == 1 {
                names[0].clone()
            } else if names.iter().any(|name| name == "development") {
                "development".to_string()
            } else {
                return Err(TransitionError(
                    "jet split env found more than one Environment; name one closed Config first"
                        .to_string(),
                ));
            };
            let destination = PathBuf::from("package/env.jet");
            ensure_new_file(&root, &destination)?;
            root_after = remove_entry(package_text, &entry);
            root_after = add_config_reference(&root_after, &package.configs, &destination)?;
            let config_text = render_config(&config_name, "environments", &value);
            let config = ConfigFacts::parse(&config_text, destination.display().to_string())
                .map_err(|error| transition_parse_error(&destination, error))?;
            validate_composed_root(&root_after, &package, config)?;
            changes.push(FileChange {
                relative: destination,
                before: None,
                after: Some(config_text.into_bytes()),
            });
        }
        SplitTarget::Package { name } => {
            validate_name(&name, "Package")?;
            let (entry, body) = required_inline_config(&entries, &name, &package_path)?;
            let destination = PathBuf::from(format!("packages/{name}/package.jet"));
            ensure_new_file(&root, &destination)?;
            let member = render_member_package(&name, body);
            let _member_facts = PackageFacts::parse(&member, destination.display().to_string())
                .map_err(|error| transition_parse_error(&destination, error))?;
            let member_config = ConfigFacts::parse(body, destination.display().to_string())
                .map_err(|error| transition_parse_error(&destination, error))?;
            root_after = remove_entry(package_text, &entry);
            root_after = add_member_reference(&root_after, &package.members, &destination)?;
            let root_after_facts =
                PackageFacts::parse_uncomposed(&root_after, package_path.display().to_string())
                    .map_err(|error| package_error(&package_path, error))?;
            root_after_facts
                .validate_members()
                .map_err(|error| TransitionError(error.to_string()))?;
            let mut composed_after = root_after_facts.clone();
            composed_after
                .compose([member_config])
                .map_err(|error| TransitionError(error.to_string()))?;
            semantic_before = semantic_fingerprint(&package);
            semantic_after = semantic_fingerprint(&composed_after);
            changes.push(FileChange {
                relative: destination,
                before: None,
                after: Some(member.into_bytes()),
            });
        }
        SplitTarget::Hosts { name } => {
            validate_name(&name, "host")?;
            require_output_declaration(&entries, &name, &package_path)?;
            let output = package.outputs.get(&name).ok_or_else(|| {
                TransitionError(format!("outputs.{name} is not declared in {PACKAGE_FILE}"))
            })?;
            if output.kind != PackageOutputKind::System {
                return Err(TransitionError(format!(
                    "outputs.{name} is {:?}, but host extraction needs a System Output",
                    output.kind
                )));
            }
            let destination = PathBuf::from("package/fleet.jet");
            ensure_new_file(&root, &destination)?;
            let config_text = render_fleet_config(&name);
            let config_facts = ConfigFacts::parse(&config_text, destination.display().to_string())
                .map_err(|error| transition_parse_error(&destination, error))?;
            root_after = add_config_reference(package_text, &package.configs, &destination)?;
            validate_composed_root(&root_after, &package, config_facts)?;
            let system_fingerprint = output_fingerprint(output);
            semantic_before = system_fingerprint.clone();
            semantic_after = system_fingerprint;
            changes.push(FileChange {
                relative: destination,
                before: None,
                after: Some(config_text.into_bytes()),
            });
        }
    }

    if root_after.as_bytes() == package_bytes {
        return Err(TransitionError("the requested split would not change package.jet".to_string()));
    }
    changes.push(FileChange {
        relative: PathBuf::from(PACKAGE_FILE),
        before: Some(package_bytes),
        after: Some(root_after.into_bytes()),
    });
    forward_plan(&root, operation, changes, semantic_before, semantic_after)
}

fn fold_plan(root: &Path, path: &Path) -> Result<TransitionPlan, TransitionError> {
    let root = canonical_root(root)?;
    let relative = relative_inside(&root, path)?;
    if relative == Path::new(PACKAGE_FILE) {
        return Err(TransitionError(
            "jet fold needs the generated file named by a split, not package.jet".to_string(),
        ));
    }
    let journal_dir = root.join(JOURNAL_DIR);
    let entries = fs::read_dir(&journal_dir).map_err(|error| {
        TransitionError(format!(
            "no transition journal for {}: {error}",
            relative.display()
        ))
    })?;
    let mut matches = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| TransitionError(format!("couldn't read transition journal: {error}")))?;
        if !entry
            .file_type()
            .map_err(|error| TransitionError(error.to_string()))?
            .is_file()
        {
            continue;
        }
        let journal_path = entry.path();
        let journal = read_journal(&journal_path)?;
        if journal
            .changes
            .iter()
            .any(|change| change.relative == relative)
        {
            matches.push((journal_path, journal));
        }
    }
    if matches.len() != 1 {
        return Err(TransitionError(format!(
            "expected one transition journal for {}, found {}",
            relative.display(),
            matches.len()
        )));
    }
    let (journal_path, journal) = matches.pop().expect("one journal");
    Ok(TransitionPlan {
        root,
        operation: format!("fold-{}", journal.operation),
        changes: journal.changes,
        journal_path,
        journal: Vec::new(),
        before_fingerprint: journal.after_fingerprint,
        after_fingerprint: journal.before_fingerprint,
        reverse: true,
    })
}

fn legacy_plan(root: &Path) -> Result<TransitionPlan, TransitionError> {
    let root = canonical_root(root)?;
    let package_path = root.join(PACKAGE_FILE);
    let existing_package = if package_path.is_file() {
        Some(read_regular(&package_path)?)
    } else {
        None
    };
    let role_names = ["pkg.jet", "env.jet", "workspace.jet", "config.jet"];
    let mut role_files = Vec::new();
    for name in role_names {
        let path = root.join(name);
        if path.is_file() {
            role_files.push((name, read_regular(&path)?));
        }
    }
    if role_files.is_empty() {
        return Err(TransitionError(format!(
            "no migration-era role files found in {}",
            root.display()
        )));
    }
    if role_files.iter().any(|(name, _)| *name == "pkg.jet") && existing_package.is_some() {
        return Err(TransitionError(
            "both package.jet and pkg.jet exist; choose one Package root before migration".to_string(),
        ));
    }
    let root_bytes = existing_package
        .or_else(|| {
            role_files
                .iter()
                .find(|(name, _)| *name == "pkg.jet")
                .map(|(_, bytes)| bytes.clone())
        })
        .ok_or_else(|| {
            TransitionError(
                "role-file migration needs a typed pkg.jet or package.jet Package root".to_string(),
            )
        })?;
    let mut root_text = std::str::from_utf8(&root_bytes)
        .map_err(|_| TransitionError("the Package root must be UTF-8".to_string()))?
        .to_string();
    if role_files.iter().any(|(name, _)| *name == "pkg.jet")
        && PackageFacts::parse_uncomposed(&root_text, package_path.display().to_string()).is_err()
    {
        root_text = render_legacy_package_root(&root_text)?;
    }
    let before_text = root_text.clone();
    let root_facts = PackageFacts::parse_uncomposed(
        &before_text,
        package_path.display().to_string(),
    )
    .map_err(|error| package_error(&package_path, error))?;
    let before_facts = PackageFacts::load(&root)
        .ok_or_else(|| TransitionError(format!("no typed Package root in {}", root.display())))?
        .map_err(|error| package_error(&package_path, error))?;
    let mut configs = root_facts.configs.clone();
    let mut changes = Vec::new();

    for (name, bytes) in &role_files {
        if *name == "pkg.jet" {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| TransitionError(format!("{name} must be UTF-8")))?;
        if *name == "workspace.jet" {
            let body = module_body(text, "workspace")?.ok_or_else(|| {
                TransitionError("workspace.jet has no module workspace declaration".to_string())
            })?;
            let entries = source_entries(body);
            let (_, members) = required_field(&entries, "members", &PathBuf::from(name))?;
            for entry in entries {
                if entry.field.as_deref() != Some("members") {
                    let field = entry
                        .field
                        .unwrap_or_else(|| entry.raw.trim().to_string());
                    return Err(TransitionError(format!(
                        "workspace.jet contains unsupported field `{field}`; migration stops before writing"
                    )));
                }
            }
            root_text = set_or_add_field(&root_text, "members", &members)?;
            continue;
        }
        let (config_name, config_text) = if *name == "env.jet" {
            let (module_name, body) = env_module(text)?;
            (module_name.clone(), render_config(&module_name, "raw", body))
        } else {
            let config_name = "config".to_string();
            let config_text = if ConfigFacts::parse(text, name.to_string()).is_ok() {
                text.to_string()
            } else if let Some(body) = module_body(text, "config")? {
                render_config(&config_name, "raw", body)
            } else {
                return Err(TransitionError(format!(
                    "{name} contains open role fields; migration stops before writing. Close the facts in a Config first"
                )));
            };
            (config_name, config_text)
        };
        let destination = PathBuf::from(format!(
            "package/{}-{}.jet",
            name.trim_end_matches(".jet"),
            config_name
        ));
        ensure_new_file(&root, &destination)?;
        ConfigFacts::parse(&config_text, destination.display().to_string())
            .map_err(|error| transition_parse_error(&destination, error))?;
        root_text = add_config_reference(&root_text, &configs, &destination)?;
        configs.push(destination.to_string_lossy().into_owned());
        changes.push(FileChange {
            relative: destination,
            before: None,
            after: Some(config_text.into_bytes()),
        });
    }

    let root_after = root_text.clone().into_bytes();
    let migrated_facts = PackageFacts::parse_uncomposed(
        std::str::from_utf8(&root_after)
            .map_err(|_| TransitionError("migrated package root is not UTF-8".to_string()))?,
        package_path.display().to_string(),
    )
    .map_err(|error| package_error(&package_path, error))?;
    migrated_facts
        .validate_members_in(&root)
        .map_err(|error| TransitionError(error.to_string()))?;
    let mut composed = Vec::new();
    for change in &changes {
        if let Some(bytes) = &change.after {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| TransitionError("generated Config is not UTF-8".to_string()))?;
            composed.push(
                ConfigFacts::parse(text, change.relative.display().to_string())
                    .map_err(|error| transition_parse_error(&change.relative, error))?,
            );
        }
    }
    let mut before = before_facts.clone();
    before
        .compose(composed.clone())
        .map_err(|error| TransitionError(error.to_string()))?;
    let mut after = before_facts;
    after
        .compose(composed)
        .map_err(|error| TransitionError(error.to_string()))?;
    after
        .validate_defaults()
        .map_err(|error| TransitionError(error.to_string()))?;

    for (name, bytes) in role_files {
        changes.push(FileChange {
            relative: PathBuf::from(name),
            before: Some(bytes),
            after: None,
        });
    }
    changes.push(FileChange {
        relative: PathBuf::from(PACKAGE_FILE),
        before: if package_path.is_file() {
            Some(root_bytes)
        } else {
            None
        },
        after: Some(root_after),
    });
    forward_plan(
        &root,
        next_legacy_operation(&root)?,
        changes,
        semantic_fingerprint(&before),
        semantic_fingerprint(&after),
    )
}

fn forward_plan(
    root: &Path,
    operation: String,
    changes: Vec<FileChange>,
    before_fingerprint: String,
    after_fingerprint: String,
) -> Result<TransitionPlan, TransitionError> {
    let journal_path = journal_path(root, &operation, &changes)?;
    let journal = Journal {
        operation: operation.clone(),
        before_fingerprint: before_fingerprint.clone(),
        after_fingerprint: after_fingerprint.clone(),
        changes: changes.clone(),
    };
    let encoded = encode_journal(&journal);
    if encoded.len() > MAX_JOURNAL_BYTES {
        return Err(TransitionError(format!(
            "transition journal exceeds the {MAX_JOURNAL_BYTES}-byte limit"
        )));
    }
    Ok(TransitionPlan {
        root: root.to_path_buf(),
        operation,
        changes,
        journal_path,
        journal: encoded,
        before_fingerprint,
        after_fingerprint,
        reverse: false,
    })
}

fn validate_composed_root(
    root_text: &str,
    original: &PackageFacts,
    config: ConfigFacts,
) -> Result<(), TransitionError> {
    PackageFacts::parse_uncomposed(root_text, PACKAGE_FILE.to_string())
        .map_err(|error| TransitionError(error.to_string()))?;
    // `original` is already fully composed by PackageFacts::load. Use it as
    // the semantic baseline so file-backed Configs remain visible while the
    // generated contribution is checked for real conflicts/default errors.
    let mut candidate = original.clone();
    candidate
        .compose([config])
        .map_err(|error| TransitionError(error.to_string()))?;
    candidate
        .validate_defaults()
        .map_err(|error| TransitionError(error.to_string()))
}

fn package_error(path: &Path, error: PackageParseError) -> TransitionError {
    TransitionError(format!("typed Package {} is invalid: {error}", path.display()))
}

fn transition_parse_error(path: &Path, error: PackageParseError) -> TransitionError {
    TransitionError(format!(
        "Config {} is not a closed typed contribution: {error}",
        path.display()
    ))
}

fn canonical_root(root: &Path) -> Result<PathBuf, TransitionError> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        TransitionError(format!(
            "couldn't inspect Package root {}: {error}",
            root.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(TransitionError(format!(
            "Package root {} must be a real directory",
            root.display()
        )));
    }
    root.canonicalize().map_err(|error| {
        TransitionError(format!(
            "couldn't resolve Package root {}: {error}",
            root.display()
        ))
    })
}

fn read_regular(path: &Path) -> Result<Vec<u8>, TransitionError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| TransitionError(format!("couldn't read {}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransitionError(format!(
            "{} must be a regular file",
            path.display()
        )));
    }
    read_bounded(path)
}

fn read_state(path: &Path) -> Result<Option<Vec<u8>>, TransitionError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(TransitionError(format!(
            "{} is a symlink; refusing a path-ambiguous transition",
            path.display()
        ))),
        Ok(metadata) if metadata.is_file() => Ok(Some(read_bounded(path)?)),
        Ok(metadata) if metadata.is_dir() => Err(TransitionError(format!(
            "{} is a directory, not a transition file",
            path.display()
        ))),
        Ok(_) => Err(TransitionError(format!(
            "{} is not a regular file",
            path.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(TransitionError(format!(
            "couldn't inspect {}: {error}",
            path.display()
        ))),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, TransitionError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| TransitionError(format!("couldn't read {}: {error}", path.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(TransitionError(format!("{} must be a regular file", path.display())));
    }
    if metadata.len() > MAX_JOURNAL_BYTES as u64 {
        return Err(TransitionError(format!(
            "{} exceeds the {MAX_JOURNAL_BYTES}-byte transition limit",
            path.display()
        )));
    }
    let file = fs::File::open(path)
        .map_err(|error| TransitionError(format!("couldn't read {}: {error}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(MAX_JOURNAL_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| TransitionError(format!("couldn't read {}: {error}", path.display())))?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(TransitionError(format!(
            "{} exceeded the {MAX_JOURNAL_BYTES}-byte transition limit while being read",
            path.display()
        )));
    }
    Ok(bytes)
}

fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf, TransitionError> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err(TransitionError(format!(
            "transition path {} escapes the Package root",
            relative.display()
        )));
    }
    let path = root.join(relative);
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&cursor) {
            if metadata.file_type().is_symlink() {
                return Err(TransitionError(format!(
                    "transition path {} contains a symlink",
                    relative.display()
                )));
            }
        }
    }
    Ok(path)
}

fn relative_inside(root: &Path, path: &Path) -> Result<PathBuf, TransitionError> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let relative = candidate.strip_prefix(root).map_err(|_| {
        TransitionError(format!(
            "fold path {} escapes the Package root",
            path.display()
        ))
    })?;
    if relative.as_os_str().is_empty() {
        return Err(TransitionError("fold needs a generated transition file".to_string()));
    }
    let _ = safe_path(root, relative)?;
    Ok(relative.to_path_buf())
}

fn ensure_new_file(root: &Path, relative: &Path) -> Result<(), TransitionError> {
    let path = safe_path(root, relative)?;
    if fs::symlink_metadata(&path).is_ok() {
        return Err(TransitionError(format!(
            "transition destination {} already exists",
            relative.display()
        )));
    }
    let mut parent = path.parent().unwrap_or(root).to_path_buf();
    while parent != root {
        if let Ok(metadata) = fs::symlink_metadata(&parent) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(TransitionError(format!(
                    "transition parent {} is not a real directory",
                    parent.display()
                )));
            }
        }
        let Some(next) = parent.parent() else { break };
        parent = next.to_path_buf();
    }
    Ok(())
}

fn remove_regular(root: &Path, path: &Path) -> Result<(), TransitionError> {
    let relative = relative_inside(root, path)?;
    let _ = safe_path(root, &relative)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            TransitionError(format!(
                "{} is not a removable regular file",
                path.display()
            )),
        ),
        Ok(_) => fs::remove_file(path)
            .map_err(|error| TransitionError(format!("couldn't remove {}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(TransitionError(format!(
            "couldn't inspect {}: {error}",
            path.display()
        ))),
    }
}

fn apply_state(root: &Path, path: &Path, state: Option<&[u8]>) -> Result<(), TransitionError> {
    let _ = relative_inside(root, path)?;
    match state {
        Some(bytes) => {
            let parent = path.parent().unwrap_or(root);
            create_real_dirs(root, parent)?;
            write_atomic(path, bytes)
        }
        None => remove_regular(root, path),
    }
}

fn create_real_dirs(root: &Path, directory: &Path) -> Result<(), TransitionError> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| TransitionError("transition directory escapes the Package root".to_string()))?;
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        cursor.push(component.as_os_str());
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(TransitionError(format!(
                    "transition directory {} is not real",
                    cursor.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&cursor).map_err(|error| {
                    TransitionError(format!("couldn't create {}: {error}", cursor.display()))
                })?;
            }
            Err(error) => {
                return Err(TransitionError(format!(
                    "couldn't inspect {}: {error}",
                    cursor.display()
                )))
            }
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), TransitionError> {
    let parent = path
        .parent()
        .ok_or_else(|| TransitionError(format!("{} has no parent", path.display())))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| TransitionError("transition path is not UTF-8".to_string()))?;
    let temp = parent.join(format!(
        ".{name}.jet-transition-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    let mut file = options.open(&temp).map_err(|error| {
        TransitionError(format!("couldn't stage {}: {error}", path.display()))
    })?;
    use std::io::Write;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temp);
        return Err(TransitionError(format!(
            "couldn't stage {}: {error}",
            path.display()
        )));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(TransitionError(format!(
            "couldn't publish {}: {error}",
            path.display()
        )));
    }
    Ok(())
}

fn journal_path(
    root: &Path,
    operation: &str,
    changes: &[FileChange],
) -> Result<PathBuf, TransitionError> {
    let mut identity = operation.as_bytes().to_vec();
    for change in changes {
        identity.extend_from_slice(change.relative.to_string_lossy().as_bytes());
        if let Some(bytes) = &change.before {
            identity.extend_from_slice(bytes);
        }
    }
    let suffix = crate::SHA256::sha256_hex(&identity);
    Ok(root
        .join(JOURNAL_DIR)
        .join(format!("{operation}-{}.journal", &suffix[..16])))
}

fn encode_journal(journal: &Journal) -> Vec<u8> {
    let mut text = String::new();
    text.push_str(JOURNAL_HEADER);
    text.push('\n');
    text.push_str(&format!("operation={}\n", journal.operation));
    text.push_str(&format!("before={}\n", journal.before_fingerprint));
    text.push_str(&format!("after={}\n", journal.after_fingerprint));
    for change in &journal.changes {
        text.push_str("file=");
        text.push_str(&change.relative.to_string_lossy());
        text.push('|');
        encode_state(&mut text, change.before.as_deref());
        text.push('|');
        encode_state(&mut text, change.after.as_deref());
        text.push('\n');
    }
    text.into_bytes()
}

fn read_journal(path: &Path) -> Result<Journal, TransitionError> {
    let bytes = read_regular(path)?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(TransitionError(format!(
            "journal {} exceeds the {MAX_JOURNAL_BYTES}-byte limit",
            path.display()
        )));
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| TransitionError(format!("journal {} is not UTF-8", path.display())))?;
    let mut lines = text.lines();
    if lines.next() != Some(JOURNAL_HEADER) {
        return Err(TransitionError(format!(
            "journal {} has an unknown format",
            path.display()
        )));
    }
    let operation = journal_value(lines.next(), "operation", path)?;
    let before_fingerprint = journal_value(lines.next(), "before", path)?;
    let after_fingerprint = journal_value(lines.next(), "after", path)?;
    validate_journal_operation(&operation, path)?;
    validate_journal_fingerprint(&before_fingerprint, "before", path)?;
    validate_journal_fingerprint(&after_fingerprint, "after", path)?;
    let mut changes = Vec::new();
    let mut seen_paths = BTreeSet::new();
    for line in lines {
        let value = line.strip_prefix("file=").ok_or_else(|| {
            TransitionError(format!(
                "journal {} has a malformed file record",
                path.display()
            ))
        })?;
        let mut parts = value.split('|');
        let relative = PathBuf::from(parts.next().unwrap_or_default());
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::Prefix(_)
                )
            })
        {
            return Err(TransitionError(format!(
                "journal {} contains an escaping path",
                path.display()
            )));
        }
        if !seen_paths.insert(relative.clone()) {
            return Err(TransitionError(format!(
                "journal {} repeats file {}",
                path.display(),
                relative.display()
            )));
        }
        let before = decode_state(parts.next(), path)?;
        let after = decode_state(parts.next(), path)?;
        if parts.next().is_some() {
            return Err(TransitionError(format!(
                "journal {} has an extra file field",
                path.display()
            )));
        }
        changes.push(FileChange {
            relative,
            before,
            after,
        });
    }
    if changes.is_empty() {
        return Err(TransitionError(format!(
            "journal {} has no file records",
            path.display()
        )));
    }
    Ok(Journal {
        operation,
        before_fingerprint,
        after_fingerprint,
        changes,
    })
}

fn validate_journal_operation(operation: &str, path: &Path) -> Result<(), TransitionError> {
    if operation.is_empty()
        || operation.len() > 128
        || !operation
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(TransitionError(format!(
            "journal {} has an unsafe operation",
            path.display()
        )));
    }
    Ok(())
}

fn validate_journal_fingerprint(
    fingerprint: &str,
    field: &str,
    path: &Path,
) -> Result<(), TransitionError> {
    let Some(hex) = fingerprint.strip_prefix("sha256:") else {
        return Err(TransitionError(format!(
            "journal {} has an invalid {field} fingerprint",
            path.display()
        )));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TransitionError(format!(
            "journal {} has an invalid {field} fingerprint",
            path.display()
        )));
    }
    Ok(())
}

fn journal_value(
    line: Option<&str>,
    key: &str,
    path: &Path,
) -> Result<String, TransitionError> {
    line.and_then(|line| line.strip_prefix(&format!("{key}=")))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| TransitionError(format!("journal {} is missing {key}", path.display())))
}

fn encode_state(text: &mut String, state: Option<&[u8]>) {
    match state {
        None => text.push('-'),
        Some(bytes) => {
            text.push('f');
            for byte in bytes {
                use std::fmt::Write;
                let _ = write!(text, "{byte:02x}");
            }
        }
    }
}

fn decode_state(value: Option<&str>, path: &Path) -> Result<Option<Vec<u8>>, TransitionError> {
    let value = value.ok_or_else(|| {
        TransitionError(format!(
            "journal {} has an incomplete file record",
            path.display()
        ))
    })?;
    if value == "-" {
        return Ok(None);
    }
    let hex = value.strip_prefix('f').ok_or_else(|| {
        TransitionError(format!("journal {} has an invalid file state", path.display()))
    })?;
    if hex.len() % 2 != 0 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TransitionError(format!(
            "journal {} has invalid file bytes",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let high = (pair[0] as char).to_digit(16).expect("validated hex");
        let low = (pair[1] as char).to_digit(16).expect("validated hex");
        bytes.push(((high << 4) | low) as u8);
    }
    Ok(Some(bytes))
}

fn latest_legacy_journal(root: &Path) -> Result<PathBuf, TransitionError> {
    let directory = root.join(JOURNAL_DIR);
    let mut paths = fs::read_dir(&directory)
        .map_err(|error| TransitionError(format!("no role-file migration journal: {error}")))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            legacy_journal_sequence(&path).map(|sequence| (sequence, path))
        })
        .collect::<Vec<_>>();
    paths.sort_by_key(|(sequence, _)| *sequence);
    paths
        .pop()
        .map(|(_, path)| path)
        .ok_or_else(|| TransitionError("no role-file migration journal found".to_string()))
}

fn next_legacy_operation(root: &Path) -> Result<String, TransitionError> {
    let directory = root.join(JOURNAL_DIR);
    let mut next = 1_u64;
    if let Ok(entries) = fs::read_dir(&directory) {
        for entry in entries.flatten() {
            if let Some(sequence) = legacy_journal_sequence(&entry.path()) {
                next = next.max(sequence.saturating_add(1));
            }
        }
    }
    Ok(format!("legacy-role-fold-{next:020}"))
}

fn legacy_journal_sequence(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".journal")?;
    let sequence = stem.strip_prefix("legacy-role-fold-")?.split('-').next()?;
    sequence.parse().ok()
}

#[derive(Debug, Clone)]
struct SourceEntry {
    raw: String,
    field: Option<String>,
}

fn source_entries(source: &str) -> Vec<SourceEntry> {
    let mut entries = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    let mut line_comment = false;
    for (index, byte) in source.bytes().enumerate() {
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            }
            continue;
        }
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if byte == b'/' && source.as_bytes().get(index + 1) == Some(&b'/') {
            line_comment = true;
        } else if matches!(byte, b'{' | b'[' | b'(') {
            depth += 1;
        } else if matches!(byte, b'}' | b']' | b')') {
            depth -= 1;
        } else if depth == 0 && matches!(byte, b'\n' | b',') {
            let raw = source[start..index].to_string();
            if !raw.trim().is_empty() {
                entries.push(SourceEntry {
                    field: source_field(&raw),
                    raw,
                });
            }
            start = index + 1;
        }
    }
    let raw = source[start..].to_string();
    if !raw.trim().is_empty() {
        entries.push(SourceEntry {
            field: source_field(&raw),
            raw,
        });
    }
    entries
}

fn source_field(raw: &str) -> Option<String> {
    let raw = strip_comments(raw);
    let raw = raw.trim().trim_end_matches(';').trim();
    if let Some(separator) = raw.find("::") {
        let left = raw[..separator]
            .trim()
            .strip_prefix("pub ")
            .unwrap_or(raw[..separator].trim())
            .trim();
        if raw[separator + 2..].trim_start().starts_with("Config") {
            return Some(format!("::{left}"));
        }
    }
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in raw.bytes().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => quoted = true,
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b':' if depth == 0 => return Some(raw[..index].trim().to_string()),
            _ => {}
        }
    }
    None
}

fn required_field<'a>(
    entries: &'a [SourceEntry],
    field: &str,
    path: &Path,
) -> Result<(&'a SourceEntry, String), TransitionError> {
    let matches = entries
        .iter()
        .filter(|entry| entry.field.as_deref() == Some(field))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(TransitionError(format!(
            "{field} must be declared exactly once in {}",
            path.display()
        )));
    }
    let entry = matches[0];
    let value = field_value(&entry.raw, field).ok_or_else(|| {
        TransitionError(format!("{field} has no value in {}", path.display()))
    })?;
    Ok((entry, value))
}

fn required_record_entry<'a>(
    value: &'a str,
    name: &str,
    scope: &str,
    path: &Path,
) -> Result<(String, String), TransitionError> {
    let body = record_body(value).ok_or_else(|| {
        TransitionError(format!("{scope} in {} is not a record", path.display()))
    })?;
    let entries = source_entries(body);
    let matches = entries
        .iter()
        .filter(|entry| entry.field.as_deref() == Some(name))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(TransitionError(format!(
            "{scope}.{name} must be declared exactly once in {}",
            path.display()
        )));
    }
    let entry = matches[0];
    let field = entry.field.as_deref().unwrap_or_default();
    let value = field_value(&entry.raw, field).ok_or_else(|| {
        TransitionError(format!("{scope}.{name} has no value in {}", path.display()))
    })?;
    Ok((field.to_string(), value))
}

fn require_output_declaration(
    entries: &[SourceEntry],
    name: &str,
    path: &Path,
) -> Result<(), TransitionError> {
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.field.as_deref() == Some("outputs"))
    {
        let value = field_value(&entry.raw, "outputs")
            .ok_or_else(|| TransitionError(format!("outputs has no value in {}", path.display())))?;
        let _ = required_record_entry(&value, name, "outputs", path)?;
        return Ok(());
    }

    let matches = entries
        .iter()
        .filter(|entry| entry.field.as_deref() == Some(name))
        .filter_map(|entry| field_value(&entry.raw, name))
        .filter(|value| value.trim_start().starts_with("Output ::"))
        .count();
    if matches == 1 {
        Ok(())
    } else {
        Err(TransitionError(format!(
            "outputs.{name} must be declared exactly once in {}",
            path.display()
        )))
    }
}

fn required_inline_config<'a>(
    entries: &'a [SourceEntry],
    name: &str,
    path: &Path,
) -> Result<(&'a SourceEntry, &'a str), TransitionError> {
    let key = format!("::{name}");
    let matches = entries
        .iter()
        .filter(|entry| entry.field.as_deref() == Some(key.as_str()))
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(TransitionError(format!(
            "closed Config {name} was not found in {}",
            path.display()
        )));
    }
    let entry = matches[0];
    let value = config_body(&entry.raw).ok_or_else(|| {
        TransitionError(format!(
            "Config {name} is not a record in {}",
            path.display()
        ))
    })?;
    Ok((entry, value))
}

fn field_value(raw: &str, field: &str) -> Option<String> {
    let source = strip_comments(raw);
    let source = source.trim().trim_end_matches(';').trim();
    let index = source.find(':')?;
    if source[..index].trim() == field {
        Some(source[index + 1..].trim().to_string())
    } else {
        None
    }
}

fn config_body(raw: &str) -> Option<&str> {
    let clean = mask_comments(raw);
    let separator = clean.find("::")?;
    let value = clean[separator + 2..].trim();
    if !value.starts_with("Config") {
        return None;
    }
    record_body(&raw[separator + 2..])
}

fn record_body(value: &str) -> Option<&str> {
    let clean = mask_comments(value);
    let open = clean.find('{')?;
    let close = matching_delimiter(&clean, open, b'{', b'}')?;
    Some(&value[open + 1..close])
}

fn matching_delimiter(value: &str, open: usize, left: u8, right: u8) -> Option<usize> {
    let mut depth = 0i32;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in value.bytes().enumerate().skip(open) {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if byte == b'"' {
            quoted = true;
        } else if byte == left {
            depth += 1;
        } else if byte == right {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn record_names(value: &str, scope: &str) -> Result<Vec<String>, TransitionError> {
    let body = record_body(value)
        .ok_or_else(|| TransitionError(format!("{scope} is not a record")))?;
    Ok(source_entries(body)
        .into_iter()
        .filter_map(|entry| entry.field)
        .collect())
}

fn remove_entry(source: &str, target: &SourceEntry) -> String {
    let mut output = String::new();
    let target_raw = target.raw.trim();
    for entry in source_entries(source) {
        if entry.raw.trim() == target_raw {
            continue;
        }
        let raw = entry.raw.trim();
        if raw.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(raw);
    }
    if source.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn set_or_add_field(source: &str, field: &str, value: &str) -> Result<String, TransitionError> {
    let entries = source_entries(source);
    let replacement = format!("{field}: {}", value.trim());
    let matches = entries
        .iter()
        .filter(|entry| entry.field.as_deref() == Some(field))
        .collect::<Vec<_>>();
    if matches.len() > 1 {
        return Err(TransitionError(format!(
            "{field} is ambiguous; migration stopped before writing"
        )));
    }
    if matches.len() == 1 {
        let mut output = String::new();
        for entry in &entries {
            let raw = if entry.raw.trim() == matches[0].raw.trim() {
                replacement.as_str()
            } else {
                entry.raw.trim()
            };
            if raw.is_empty() {
                continue;
            }
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(raw);
        }
        if source.ends_with('\n') {
            output.push('\n');
        }
        return Ok(output);
    }
    let mut output = source.trim_end().to_string();
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(&replacement);
    output.push('\n');
    Ok(output)
}

fn add_config_reference(
    source: &str,
    existing: &[String],
    destination: &Path,
) -> Result<String, TransitionError> {
    let reference = destination.to_string_lossy().replace('\\', "/");
    if existing.iter().any(|value| value == &reference) {
        return Err(TransitionError(format!(
            "Config {reference} is already listed"
        )));
    }
    let entries = source_entries(source);
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.field.as_deref() == Some("configs"))
    {
        let value = field_value(&entry.raw, "configs").unwrap_or_default();
        if !value.trim_start().starts_with('[') {
            return Err(TransitionError(
                "configs is not a closed list; migration stopped before writing".to_string(),
            ));
        }
        let inner = value.trim().trim_start_matches('[').trim_end_matches(']').trim();
        let list = if inner.is_empty() {
            format!("[{}]", quote(&reference))
        } else {
            format!("[{inner}, {}]", quote(&reference))
        };
        return set_or_add_field(source, "configs", &list);
    }
    set_or_add_field(source, "configs", &format!("[{}]", quote(&reference)))
}

fn add_member_reference(
    source: &str,
    existing: &[crate::Package::MemberRef],
    destination: &Path,
) -> Result<String, TransitionError> {
    let reference = destination
        .parent()
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| destination.to_string_lossy().into_owned());
    if existing.iter().any(
        |member| matches!(member, crate::Package::MemberRef::Path(path) if path == &reference),
    ) {
        return Err(TransitionError(format!(
            "member {reference} is already listed"
        )));
    }
    let entries = source_entries(source);
    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.field.as_deref() == Some("members"))
    {
        let value = field_value(&entry.raw, "members").unwrap_or_default();
        if !value.trim_start().starts_with('[') {
            return Err(TransitionError(
                "members uses discovery; adding one extracted member would be ambiguous"
                    .to_string(),
            ));
        }
        let inner = value.trim().trim_start_matches('[').trim_end_matches(']').trim();
        let list = if inner.is_empty() {
            format!("[{}]", quote(&reference))
        } else {
            format!("[{inner}, {}]", quote(&reference))
        };
        return set_or_add_field(source, "members", &list);
    }
    set_or_add_field(source, "members", &format!("[{}]", quote(&reference)))
}

fn render_config(name: &str, field: &str, value: &str) -> String {
    if field == "raw" {
        format!("pub {name} :: Config.{{\n{}\n}}\n", value.trim())
    } else {
        format!("pub {name} :: Config.{{\n    {field}: {}\n}}\n", value.trim())
    }
}

fn render_member_package(name: &str, body: &str) -> String {
    format!("name: {}\n{}\n", quote(name), body.trim())
}

fn render_legacy_package_root(source: &str) -> Result<String, TransitionError> {
    let manifest = crate::PackageManifest::parse(source)
        .map_err(|error| TransitionError(format!("cannot migrate pkg.jet into package.jet: {error:?}")))?;
    if !manifest.packages.is_empty()
        || !manifest.build_profiles.is_empty()
        || !manifest.build_allow.is_empty()
        || manifest.effects_enabled
        || !manifest.grants.is_empty()
        || manifest.trust_policy.is_some()
        || !manifest.provider_policy.is_empty()
        || manifest.lints_deny.is_some()
        || !manifest.memory_policy.is_empty()
        || manifest.auto_derive.is_some()
    {
        return Err(TransitionError(
            "pkg.jet contains package, build, effects, policy, or target facts that need an explicit migration before jet init can fold it"
                .to_string(),
        ));
    }
    let mut output = format!(
        "name: {}\nversion: {}\n",
        quote(&manifest.package.name),
        quote(&manifest.package.version)
    );
    if let Some(jet) = manifest.package.jet_constraint {
        output.push_str(&format!("jet: {}\n", quote(&jet)));
    }
    if let Some(edition) = manifest.package.edition {
        output.push_str(&format!("edition: {}\n", quote(&edition)));
    }
    if let Some(license) = manifest.package.license {
        output.push_str(&format!("license: {}\n", quote(&license)));
    }
    if let Some(description) = manifest.package.description {
        output.push_str(&format!("description: {}\n", quote(&description)));
    }
    if let Some(repository) = manifest.package.repository {
        output.push_str(&format!("repository: {}\n", quote(&repository)));
    }
    if let Some(target) = manifest.package.target {
        output.push_str(&format!("target: {}\n", quote(&target)));
    }
    if let Some(layer) = manifest.package.layer {
        output.push_str(&format!("runtime: {}\n", quote(layer.as_str())));
    }
    output.push_str("deps: .{\n");
    for dependency in manifest.deps {
        output.push_str(&format!(
            "    {}: {}\n",
            dependency.name,
            legacy_dependency_value(&dependency.source)
        ));
    }
    output.push_str("}\n");
    Ok(output)
}

fn legacy_dependency_value(source: &crate::PackageManifest::DepSource) -> String {
    match source {
        crate::PackageManifest::DepSource::Version(value) => value.clone(),
        crate::PackageManifest::DepSource::Provider { provider, target } => {
            if matches!(provider, crate::RefSpec::Source::Path) {
                target.clone()
            } else {
                format!("{target}@{}", provider.label())
            }
        }
        crate::PackageManifest::DepSource::Git { url, selector } => {
            let (field, value) = match selector {
                crate::Manifest::GitSelector::Tag(value) => ("tag", value),
                crate::Manifest::GitSelector::Branch(value) => ("branch", value),
                crate::Manifest::GitSelector::Rev(value) => ("rev", value),
            };
            format!("{{ git: {url:?}, {field}: {value:?} }}")
        }
        crate::PackageManifest::DepSource::CLib { target } => format!("lib: {target}"),
    }
}

fn render_fleet_config(host: &str) -> String {
    format!(
        "pub home :: Config.{{\n    outputs: .{{\n        home: .Fleet.{{\n            name: \"home\"\n            hosts: .{{ {host}: systems.{host} }}\n        }}\n    }}\n}}\n"
    )
}

fn quote(value: &str) -> String {
    format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn validate_name(name: &str, role: &str) -> Result<(), TransitionError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        || name.starts_with('.')
        || name.contains("..")
    {
        return Err(TransitionError(format!("invalid {role} name {name}")));
    }
    Ok(())
}

fn strip_comments(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else if ch == '"' {
            quoted = true;
            output.push(ch);
        } else if ch == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for comment in chars.by_ref() {
                if comment == '\n' {
                    output.push('\n');
                    break;
                }
            }
        } else {
            output.push(ch);
        }
    }
    output
}

/// Replace comments and string literals with same-length ASCII spaces. The
/// byte positions stay stable, while role discovery and delimiter matching
/// can only see actual source tokens. Block comments may nest, matching Jet's
/// lexer boundary.
fn mask_comments(value: &str) -> String {
    let mut bytes = value.as_bytes().to_vec();
    let mut quoted = false;
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment_depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if line_comment {
            if byte == b'\n' {
                line_comment = false;
            } else {
                bytes[index] = b' ';
            }
            index += 1;
            continue;
        }
        if block_comment_depth > 0 {
            if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
                bytes[index] = b' ';
                bytes[index + 1] = b' ';
                block_comment_depth += 1;
                index += 2;
            } else if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
                bytes[index] = b' ';
                bytes[index + 1] = b' ';
                block_comment_depth -= 1;
                index += 2;
            } else {
                if byte != b'\n' {
                    bytes[index] = b' ';
                }
                index += 1;
            }
            continue;
        }
        if quoted {
            if byte != b'\n' {
                bytes[index] = b' ';
            }
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            bytes[index] = b' ';
            quoted = true;
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            bytes[index] = b' ';
            bytes[index + 1] = b' ';
            line_comment = true;
            index += 2;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            bytes[index] = b' ';
            bytes[index + 1] = b' ';
            block_comment_depth = 1;
            index += 2;
        } else {
            index += 1;
        }
    }
    String::from_utf8(bytes).expect("comment masking preserves UTF-8 outside comments")
}

fn module_body<'a>(
    source: &'a str,
    module: &str,
) -> Result<Option<&'a str>, TransitionError> {
    let marker = format!("module {module}");
    let clean = mask_comments(source);
    let Some(start) = clean.match_indices(&marker).find_map(|(start, _)| {
        let before_ok = start == 0
            || !clean[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
        let after = start + marker.len();
        let after_ok = clean[after..]
            .chars()
            .next()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'));
        (before_ok && after_ok).then_some(start)
    }) else {
        return Ok(None);
    };
    let after = &clean[start + marker.len()..];
    let open = after.find('{').ok_or_else(|| {
        TransitionError(format!("module {module} is missing its body"))
    })?;
    let absolute = start + marker.len() + open;
    let close = matching_delimiter(&clean, absolute, b'{', b'}').ok_or_else(|| {
        TransitionError(format!("module {module} has an unclosed body"))
    })?;
    Ok(Some(&source[absolute + 1..close]))
}

fn env_module(source: &str) -> Result<(String, &str), TransitionError> {
    let clean = mask_comments(source);
    let marker = "module env.";
    let start = clean.match_indices(marker).find_map(|(start, _)| {
        (start == 0
            || !clean[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_'))
            .then_some(start)
    }).ok_or_else(|| {
        TransitionError("env.jet has no module env.<name> declaration".to_string())
    })?;
    let name_start = start + marker.len();
    let name_end = clean[name_start..]
        .find(|ch: char| ch == '{' || ch.is_whitespace())
        .map(|offset| name_start + offset)
        .ok_or_else(|| TransitionError("env.jet has no environment name".to_string()))?;
    let name = clean[name_start..name_end].trim().to_string();
    validate_name(&name, "Environment")?;
    let open = clean[name_end..]
        .find('{')
        .map(|offset| name_end + offset)
        .ok_or_else(|| {
            TransitionError("env.jet is missing its environment body".to_string())
        })?;
    let close = matching_delimiter(&clean, open, b'{', b'}')
        .ok_or_else(|| TransitionError("env.jet has an unclosed environment body".to_string()))?;
    Ok((name, &source[open + 1..close]))
}

fn semantic_fingerprint(package: &PackageFacts) -> String {
    // The fingerprint is the checked fact graph, not its storage layout.
    // Config paths, inline bindings, and member references are transition
    // metadata; split/fold must preserve the facts they expose.
    let text = format!(
        "name={:?}\nversion={:?}\njet={:?}\nsource={:?}\ndeps={:?}\nservices={:?}\noutputs={:?}\nenvironments={:?}\ndefaults={:?}\n",
        package.name,
        package.version,
        package.jet,
        package.source,
        package.deps,
        package.services,
        package.outputs,
        package.environments,
        package.defaults
    );
    format!(
        "sha256:{}",
        crate::SHA256::sha256_hex(text.as_bytes())
    )
}

fn output_fingerprint(output: &crate::Package::OutputFact) -> String {
    format!(
        "sha256:{}",
        crate::SHA256::sha256_hex(format!("{output:?}").as_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "jet-transition-{label}-{}",
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn environment_split_and_fold_restore_exact_root() {
        let root = temp_root("env");
        let original =
            b"name: \"demo\"\nenvironments: .{ development: .Environment.{ tools: [\"git\"] } }\n";
        fs::write(root.join(PACKAGE_FILE), original).unwrap();
        let result = split(&root, SplitTarget::Environment, false).unwrap();
        assert_eq!(
            result.summary.before_fingerprint,
            result.summary.after_fingerprint
        );
        assert!(root.join("package/env.jet").is_file());
        fold(&root, Path::new("package/env.jet"), false).unwrap();
        assert_eq!(fs::read(root.join(PACKAGE_FILE)).unwrap(), original);
        assert!(!root.join("package/env.jet").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_split_refuses_without_mutation() {
        let root = temp_root("stale");
        fs::write(
            root.join(PACKAGE_FILE),
            "name: \"demo\"\nenvironments: .{ dev: .Environment.{ } }\n",
        )
        .unwrap();
        let plan = split_plan(&root, SplitTarget::Environment).unwrap();
        fs::write(root.join(PACKAGE_FILE), "name: \"changed\"\n").unwrap();
        let error = plan.apply().unwrap_err();
        assert!(error.0.contains("stale transition"));
        assert!(!root.join("package/env.jet").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_split_moves_named_config_and_records_member() {
        let root = temp_root("member");
        fs::write(
            root.join(PACKAGE_FILE),
            "name: \"workspace\"\napp :: Config.{ version: \"1\" }\n",
        )
        .unwrap();
        split(
            &root,
            SplitTarget::Package {
                name: "app".to_string(),
            },
            false,
        )
        .unwrap();
        let member = fs::read_to_string(root.join("packages/app/package.jet")).unwrap();
        assert!(member.contains("name: \"app\""));
        assert!(fs::read_to_string(root.join(PACKAGE_FILE))
            .unwrap()
            .contains("members:"));
        fold(&root, Path::new("packages/app/package.jet"), false).unwrap();
        assert!(fs::read_to_string(root.join(PACKAGE_FILE))
            .unwrap()
            .contains("app :: Config"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_open_role_file_is_rejected_before_writes() {
        let root = temp_root("legacy-open");
        fs::write(root.join("pkg.jet"), "name: \"demo\"\n").unwrap();
        fs::write(root.join("config.jet"), "languages: [python]\n").unwrap();
        let error = init(&root, false).unwrap_err();
        assert!(error.0.contains("open role fields"), "{}", error.0);
        assert!(!root.join(PACKAGE_FILE).exists());
        fs::remove_dir_all(root).unwrap();
    }
}
