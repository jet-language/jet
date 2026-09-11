//! Versioned database migration planning and target selection.
//!
//! The command layer owns target selection and process wiring. This module
//! owns the durable migration graph, deterministic source format, and
//! advisory process lock files. The selected database owns migration state,
//! checksums, transition identity, and receipts.

use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use jet_foundation::SHA256::sha256_hex;

static NEXT_SHARED_LOCK_ID: AtomicU64 = AtomicU64::new(0);

pub const MIGRATION_SCHEMA: &str = "jet.db.migration/v1";
pub const MIGRATION_DIRECTORY: &str = "migrations";
pub const MIGRATION_LOCK_DIRECTORY: &str = ".jet/migrations/locks";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationLock {
    Shared,
    Exclusive,
}

impl MigrationLock {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Exclusive => "exclusive",
        }
    }

    pub fn parse(value: &str) -> Result<Self, MigrationError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "shared" | "read" => Ok(Self::Shared),
            "exclusive" | "write" => Ok(Self::Exclusive),
            _ => Err(MigrationError::usage(
                "E2456",
                format!("unknown migration lock policy `{value}`"),
                "use `--lock shared` for inspection or `--lock exclusive` before a write",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationDirection {
    Apply,
    Rollback,
}

impl MigrationDirection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Apply => "apply",
            Self::Rollback => "rollback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RollbackMode {
    Inverse,
    Irreversible,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationError {
    pub code: &'static str,
    pub message: String,
    pub fix: String,
}

impl MigrationError {
    pub fn usage(code: &'static str, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            fix: fix.into(),
        }
    }

    pub fn io(context: impl Into<String>, error: impl fmt::Display) -> Self {
        Self {
            code: "E2105",
            message: format!("{}: {}", context.into(), safe_error(&error.to_string())),
            fix: "check the project permissions and rerun the migration command".to_string(),
        }
    }

    pub fn authority(message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: "E2456",
            message: message.into(),
            fix: fix.into(),
        }
    }

    pub fn conflict(message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            code: "E2456",
            message: message.into(),
            fix: fix.into(),
        }
    }
}

impl fmt::Display for MigrationError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}: {} ({})", self.code, self.message, self.fix)
    }
}

impl std::error::Error for MigrationError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationTarget {
    pub name: String,
    pub database: PathBuf,
    pub identity: String,
    pub database_identity: String,
    pub lock: MigrationLock,
    root: PathBuf,
}

impl MigrationTarget {
    pub fn new(
        root: &Path,
        name: &str,
        database: Option<&Path>,
        lock: MigrationLock,
    ) -> Result<Self, MigrationError> {
        validate_name(name, "target")?;
        let root = root.to_path_buf();
        let database_path = database
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(format!(".jet/db/{name}.sqlite")));
        let database = safe_database_path(&root, &database_path)?;
        let relative = database
            .strip_prefix(&root)
            .map_err(|_| {
                MigrationError::authority(
                    "database target escapes the project root",
                    "pass a relative `--database` path inside this project",
                )
            })?
            .to_string_lossy()
            .to_string();
        let database_identity = format!(
            "sha256:{}",
            sha256_hex(format!("{MIGRATION_SCHEMA}\0database\0{relative}").as_bytes())
        );
        let identity = format!(
            "sha256:{}",
            sha256_hex(format!("{MIGRATION_SCHEMA}\0target\0{name}\0{relative}").as_bytes())
        );
        Ok(Self {
            name: name.to_string(),
            database,
            identity,
            database_identity,
            lock,
            root,
        })
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root
            .join(MIGRATION_LOCK_DIRECTORY)
            .join(format!("{}.lock", self.name))
    }

    fn shared_lock_path(&self) -> PathBuf {
        let sequence = NEXT_SHARED_LOCK_ID.fetch_add(1, Ordering::Relaxed);
        self.root
            .join(MIGRATION_LOCK_DIRECTORY)
            .join(format!("{}.shared.{}.{}.{}.lock", self.name, std::process::id(), now_ms(), sequence))
    }

    pub fn acquire_lock(&self) -> Result<MigrationLockGuard, MigrationError> {
        let directory = self.root.join(MIGRATION_LOCK_DIRECTORY);
        fs::create_dir_all(&directory)
            .map_err(|error| MigrationError::io("couldn't create migration lock directory", error))?;
        let directory_metadata = fs::symlink_metadata(&directory)
            .map_err(|error| MigrationError::io("couldn't inspect migration lock directory", error))?;
        if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
            return Err(MigrationError::authority(
                "migration lock directory is not project-owned",
                "replace the lock path with a normal directory before migrating",
            ));
        }
        let exclusive_path = self.lock_path();
        let shared_paths = lock_paths(&directory, &self.name)?;
        let blocked = match self.lock {
            MigrationLock::Shared => exclusive_path.exists(),
            MigrationLock::Exclusive => exclusive_path.exists() || !shared_paths.is_empty(),
        };
        if blocked {
            return Err(MigrationError::conflict(
                format!("migration target `{}` is already locked", self.name),
                "wait for the other migration process or remove the lock only after checking its owner",
            ));
        }
        let shared_path = matches!(self.lock, MigrationLock::Shared)
            .then(|| self.shared_lock_path());
        let lock_path = shared_path.as_ref().unwrap_or(&exclusive_path);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    MigrationError::conflict(
                        format!("migration target `{}` is already locked", self.name),
                        "wait for the other migration process or remove the lock only after checking its owner",
                    )
                } else {
                    MigrationError::io("couldn't acquire migration lock", error)
                }
            })?;
        let owner = format!(
            "schema={MIGRATION_SCHEMA}\ntarget={}\ntarget_identity={}\ndatabase={}\ndatabase_identity={}\nlock={}\npid={}\n",
            self.name,
            self.identity,
            self.database.strip_prefix(&self.root).unwrap_or(&self.database).display(),
            self.database_identity,
            self.lock.as_str(),
            std::process::id()
        );
        if let Err(error) = file.write_all(owner.as_bytes()).and_then(|_| file.sync_all()) {
            let _ = fs::remove_file(&lock_path);
            return Err(MigrationError::io("couldn't write migration lock", error));
        }
        let blocked_after = match self.lock {
            MigrationLock::Shared => exclusive_path.exists(),
            MigrationLock::Exclusive => match lock_paths(&directory, &self.name) {
                Ok(paths) => paths.iter().any(|path| path.as_path() != lock_path.as_path()),
                Err(error) => {
                    let _ = fs::remove_file(&lock_path);
                    return Err(error);
                }
            },
        };
        if blocked_after {
            let _ = fs::remove_file(&lock_path);
            return Err(MigrationError::conflict(
                format!("migration target `{}` became locked while acquiring its lock", self.name),
                "retry after the other migration process releases its lock",
            ));
        }
        Ok(MigrationLockGuard { path: shared_path.unwrap_or(exclusive_path) })
    }

    pub fn list_locks(&self) -> Result<Vec<MigrationLockInfo>, MigrationError> {
        read_lock_info(&self.root, Some(&self.name))
    }
}

fn lock_paths(directory: &Path, target: &str) -> Result<Vec<PathBuf>, MigrationError> {
    let exact = format!("{target}.lock");
    let shared_prefix = format!("{target}.shared.");
    let entries = fs::read_dir(directory)
        .map_err(|error| MigrationError::io("couldn't inspect migration locks", error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| MigrationError::io("couldn't inspect migration locks", error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == exact || (name.starts_with(&shared_prefix) && name.ends_with(".lock")) {
            paths.push(entry.path());
        }
    }
    Ok(paths)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationLockInfo {
    pub path: PathBuf,
    pub target: String,
    pub target_identity: String,
    pub database: String,
    pub database_identity: String,
    pub lock: MigrationLock,
    pub pid: u64,
}

pub fn read_lock_info(root: &Path, target: Option<&str>) -> Result<Vec<MigrationLockInfo>, MigrationError> {
    if let Some(target) = target {
        validate_name(target, "target")?;
    }
    let directory = root.join(MIGRATION_LOCK_DIRECTORY);
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(MigrationError::io("couldn't inspect migration lock directory", error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(MigrationError::authority(
            "migration lock directory is not project-owned",
            "replace the lock path with a normal directory before listing locks",
        ));
    }
    let entries = fs::read_dir(&directory)
        .map_err(|error| MigrationError::io("couldn't inspect migration locks", error))?;
    let mut locks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| MigrationError::io("couldn't inspect migration locks", error))?;
        let path = entry.path();
        if !entry.file_type().map_err(|error| MigrationError::io("couldn't inspect migration lock", error))?.is_file() {
            continue;
        }
        let text = fs::read_to_string(&path)
            .map_err(|error| MigrationError::io(format!("couldn't read migration lock `{}`", path.display()), error))?;
        let values = text.lines().filter_map(|line| line.split_once('=')).collect::<std::collections::HashMap<_, _>>();
        let name = values.get("target").copied().unwrap_or_default();
        if target.is_some_and(|expected| expected != name) {
            continue;
        }
        validate_name(name, "target")?;
        if values.get("schema").copied() != Some(MIGRATION_SCHEMA)
            || values.get("target_identity").copied().unwrap_or_default().is_empty()
            || values.get("database").copied().unwrap_or_default().is_empty()
            || values.get("database_identity").copied().unwrap_or_default().is_empty()
        {
            return Err(MigrationError::authority(
                format!("migration lock `{}` is missing canonical target metadata", path.display()),
                "release the lock only after checking its owner, then reacquire it with the current Jet tool",
            ));
        }
        let database = values.get("database").copied().unwrap_or_default();
        let target_identity = values.get("target_identity").copied().unwrap_or_default();
        let database_identity = values.get("database_identity").copied().unwrap_or_default();
        let expected_target = MigrationTarget::new(
            root,
            name,
            Some(Path::new(database)),
            MigrationLock::Shared,
        )
        .map_err(|error| {
            MigrationError::authority(
                format!("migration lock `{}` has an invalid database target: {}", path.display(), error.message),
                "release the lock only after checking its owner, then reacquire it with the current Jet tool",
            )
        })?;
        if target_identity != expected_target.identity || database_identity != expected_target.database_identity {
            return Err(MigrationError::authority(
                format!("migration lock `{}` has stale target or database identity", path.display()),
                "release the lock only after checking its owner, then reacquire it with the current Jet tool",
            ));
        }
        let lock = MigrationLock::parse(values.get("lock").copied().unwrap_or_default())?;
        let pid = values
            .get("pid")
            .copied()
            .unwrap_or_default()
            .parse::<u64>()
            .map_err(|_| MigrationError::conflict(format!("migration lock `{}` has an invalid owner pid", path.display()), "remove the malformed lock only after checking its owner"))?;
        locks.push(MigrationLockInfo {
            path,
            target: name.to_string(),
            target_identity: values.get("target_identity").copied().unwrap_or_default().to_string(),
            database: values.get("database").copied().unwrap_or_default().to_string(),
            database_identity: values.get("database_identity").copied().unwrap_or_default().to_string(),
            lock,
            pid,
        });
    }
    locks.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(locks)
}

pub struct MigrationLockGuard {
    path: PathBuf,
}

impl Drop for MigrationLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationStep {
    pub ordinal: u64,
    pub sql: String,
    pub inverse_sql: Option<String>,
    pub risk: String,
    pub lock: MigrationLock,
}

impl MigrationStep {
    fn new(
        ordinal: u64,
        sql: String,
        inverse_sql: Option<String>,
        risk: String,
        lock: MigrationLock,
    ) -> Result<Self, MigrationError> {
        if sql.trim().is_empty() {
            return Err(MigrationError::usage(
                "E2456",
                "migration contains an empty forward SQL step",
                "write at least one non-empty `// up:` statement",
            ));
        }
        if sql.contains('\0') || sql.chars().any(|character| character.is_control() && character != '\n' && character != '\r' && character != '\t') {
            return Err(MigrationError::usage(
                "E2456",
                "migration SQL contains an invalid control character",
                "write UTF-8 SQL without NUL or terminal control bytes",
            ));
        }
        Ok(Self {
            ordinal,
            sql,
            inverse_sql,
            risk,
            lock,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationDefinition {
    pub version: u64,
    pub name: String,
    pub path: PathBuf,
    pub source_digest: String,
    pub steps: Vec<MigrationStep>,
    rollback: RollbackMode,
}

impl MigrationDefinition {
    pub fn has_rollback(&self) -> bool {
        !matches!(self.rollback, RollbackMode::Irreversible)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationCatalog {
    pub root: PathBuf,
    pub migrations: Vec<MigrationDefinition>,
    pub source_identity: String,
}

impl MigrationCatalog {
    pub fn load(root: &Path) -> Result<Self, MigrationError> {
        let directory = root.join(MIGRATION_DIRECTORY);
        let mut entries = Vec::new();
        if directory.exists() {
            let metadata = fs::symlink_metadata(&directory)
                .map_err(|error| MigrationError::io("couldn't inspect the migrations directory", error))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(MigrationError::authority(
                    "the migrations path is not a normal directory",
                    "replace `migrations` with a project-owned directory",
                ));
            }
            for entry in fs::read_dir(&directory)
                .map_err(|error| MigrationError::io("couldn't read the migrations directory", error))?
            {
                let entry = entry.map_err(|error| MigrationError::io("couldn't read a migration entry", error))?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path)
                    .map_err(|error| MigrationError::io("couldn't inspect a migration entry", error))?;
                if metadata.file_type().is_symlink() {
                    return Err(MigrationError::authority(
                        format!("migration entry `{}` is a symlink", path.display()),
                        "copy the migration into the project-owned migrations directory",
                    ));
                }
                if metadata.is_dir() {
                    continue;
                }
                if path.extension().and_then(|extension| extension.to_str()) != Some("jet") {
                    continue;
                }
                entries.push(path);
            }
        }
        entries.sort();
        let mut migrations = Vec::with_capacity(entries.len());
        let mut versions = BTreeSet::new();
        let mut names = BTreeSet::new();
        for path in entries {
            let migration = parse_migration(&path)?;
            if !versions.insert(migration.version) {
                return Err(MigrationError::conflict(
                    format!("duplicate migration version {}", migration.version),
                    "rename one migration so every version is unique",
                ));
            }
            if !names.insert(migration.name.clone()) {
                return Err(MigrationError::conflict(
                    format!("duplicate migration name `{}`", migration.name),
                    "rename one migration so every migration name is unique",
                ));
            }
            migrations.push(migration);
        }
        migrations.sort_by_key(|migration| migration.version);
        let mut identity = String::new();
        for migration in &migrations {
            identity.push_str(&format!(
                "{}\0{}\0{}\0",
                migration.version, migration.name, migration.source_digest
            ));
            for step in &migration.steps {
                identity.push_str(&step.sql);
                identity.push('\0');
                if let Some(inverse) = &step.inverse_sql {
                    identity.push_str(inverse);
                }
                identity.push('\0');
            }
        }
        Ok(Self {
            root: root.to_path_buf(),
            migrations,
            source_identity: format!("sha256:{}", sha256_hex(identity.as_bytes())),
        })
    }

    pub fn next_version(&self) -> u64 {
        self.migrations
            .iter()
            .map(|migration| migration.version)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn definition(&self, version: u64) -> Option<&MigrationDefinition> {
        self.migrations.iter().find(|migration| migration.version == version)
    }

    pub fn plan_apply(
        &self,
        target: MigrationTarget,
        from_version: u64,
        target_version: Option<u64>,
    ) -> Result<MigrationPlan, MigrationError> {
        if from_version != 0 && self.definition(from_version).is_none() {
            return Err(MigrationError::authority(
                format!("migration state starts at undeclared version {from_version}"),
                "reconcile the target receipt history before planning another migration",
            ));
        }
        let to_version = target_version.unwrap_or_else(|| {
            self.migrations
                .iter()
                .map(|migration| migration.version)
                .max()
                .unwrap_or(from_version)
        });
        if to_version < from_version {
            return Err(MigrationError::usage(
                "E2456",
                format!("migration target {} is behind current version {}", to_version, from_version),
                "use `rollback --target <name> --step <count>` for a reverse plan",
            ));
        }
        if self.definition(to_version).is_none() && to_version != from_version {
            return Err(MigrationError::usage(
                "E2456",
                format!("migration target version {} is not declared", to_version),
                "choose a declared version from `jet db migrate status`",
            ));
        }
        let mut steps = Vec::new();
        for migration in &self.migrations {
            if migration.version > from_version && migration.version <= to_version {
                steps.extend(migration.steps.iter().cloned());
            }
        }
        Ok(MigrationPlan::new(
            target,
            self.source_identity.clone(),
            from_version,
            to_version,
            MigrationDirection::Apply,
            steps,
        ))
    }

    pub fn plan_rollback(
        &self,
        target: MigrationTarget,
        from_version: u64,
        target_version: Option<u64>,
    ) -> Result<MigrationPlan, MigrationError> {
        if from_version != 0 && self.definition(from_version).is_none() {
            return Err(MigrationError::authority(
                format!("migration state starts at undeclared version {from_version}"),
                "reconcile the target receipt history before planning another migration",
            ));
        }
        let to_version = target_version.unwrap_or_else(|| {
            self.migrations
                .iter()
                .map(|migration| migration.version)
                .filter(|version| *version < from_version)
                .max()
                .unwrap_or(0)
        });
        if to_version >= from_version {
            return Err(MigrationError::usage(
                "E2456",
                format!("rollback target {} is not below current version {}", to_version, from_version),
                "choose an older declared version or pass `--step <count>`",
            ));
        }
        let mut steps = Vec::new();
        for migration in self.migrations.iter().rev() {
            if migration.version > to_version && migration.version <= from_version {
                if !migration.has_rollback() {
                    return Err(MigrationError::authority(
                        format!("migration {} `{}` has no checked rollback path", migration.version, migration.name),
                        "add checked `// down:` inverse SQL before requesting rollback",
                    ));
                }
                for step in migration.steps.iter().rev() {
                    let Some(sql) = step.inverse_sql.clone() else {
                        return Err(MigrationError::authority(
                            format!("migration {} `{}` has no inverse SQL", migration.version, migration.name),
                            "add `// down:` SQL before requesting rollback",
                        ));
                    };
                    steps.push(MigrationStep::new(
                        step.ordinal,
                        sql,
                        None,
                        step.risk.clone(),
                        step.lock,
                    )?);
                }
            }
        }
        Ok(MigrationPlan::new(
            target,
            self.source_identity.clone(),
            from_version,
            to_version,
            MigrationDirection::Rollback,
            steps,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationPlan {
    pub target: MigrationTarget,
    pub source_identity: String,
    pub schema_identity: String,
    pub database_identity: String,
    pub tool_identity: String,
    pub from_version: u64,
    pub to_version: u64,
    pub direction: MigrationDirection,
    pub steps: Vec<MigrationStep>,
    pub risk: String,
}

impl MigrationPlan {
    fn new(
        target: MigrationTarget,
        source_identity: String,
        from_version: u64,
        to_version: u64,
        direction: MigrationDirection,
        steps: Vec<MigrationStep>,
    ) -> Self {
        let mut risks = BTreeSet::new();
        for step in &steps {
            risks.insert(step.risk.clone());
        }
        let risk = if risks.is_empty() {
            "none".to_string()
        } else {
            risks.into_iter().collect::<Vec<_>>().join(", ")
        };
        Self {
            database_identity: target.database_identity.clone(),
            schema_identity: format!("sha256:{}", sha256_hex(MIGRATION_SCHEMA.as_bytes())),
            tool_identity: format!("jet/{}", env!("CARGO_PKG_VERSION")),
            target,
            source_identity,
            from_version,
            to_version,
            direction,
            steps,
            risk,
        }
    }
}

pub fn render_migration(
    version: u64,
    name: &str,
    up: &str,
    down: Option<&str>,
    lock: MigrationLock,
    risk: &str,
) -> Result<String, MigrationError> {
    validate_name(name, "migration")?;
    if up.trim().is_empty() {
        return Err(MigrationError::usage(
            "E2456",
            "a migration needs forward SQL",
            "pass `--up <SQL>` (or `--sql <SQL>`) to `jet db migrate new`",
        ));
    }
    if version == 0 {
        return Err(MigrationError::usage(
            "E2456",
            "migration versions start at 1",
            "let `jet db migrate new` allocate the next version",
        ));
    }
    let slug = slugify(name);
    let risk = risk.trim();
    let mut output = format!(
        "// jet db migration/v1\n// name: {}\n// lock: {}\n// risk: {}\n// up: SQL{{{}}}\n",
        name,
        lock.as_str(),
        if risk.is_empty() { "schema change" } else { risk },
        escape_sql_literal(up.trim())
    );
    if let Some(down) = down.filter(|value| !value.trim().is_empty()) {
        output.push_str(&format!("// down: SQL{{{}}}\n", escape_sql_literal(down.trim())));
    }
    output.push_str(&format!("// slug: {slug}\n"));
    Ok(output)
}


fn parse_migration(path: &Path) -> Result<MigrationDefinition, MigrationError> {
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        MigrationError::usage("E2456", "migration filename is not valid UTF-8", "rename the migration using `<version>_<name>.jet")
    })?;
    let stem = file_name.strip_suffix(".jet").ok_or_else(|| {
        MigrationError::usage("E2456", format!("migration `{file_name}` needs the `.jet` extension"), "use a checked `.jet` migration file")
    })?;
    let (version_text, slug) = stem.split_once('_').ok_or_else(|| {
        MigrationError::usage("E2456", format!("migration `{file_name}` needs `<version>_<name>.jet`"), "rename it to a versioned migration filename")
    })?;
    if version_text.is_empty() || !version_text.chars().all(|character| character.is_ascii_digit()) {
        return Err(MigrationError::usage("E2456", format!("migration `{file_name}` has an invalid version"), "use a positive decimal version prefix"));
    }
    let version = version_text.parse::<u64>().map_err(|_| MigrationError::usage("E2456", format!("migration `{file_name}` has an oversized version"), "use a version that fits in an unsigned 64-bit integer"))?;
    if version == 0 {
        return Err(MigrationError::usage("E2456", format!("migration `{file_name}` starts at version zero"), "version migrations from 1 upward"));
    }
    let source = fs::read(path).map_err(|error| MigrationError::io(format!("couldn't read migration `{}`", path.display()), error))?;
    let source_digest = format!("sha256:{}", sha256_hex(&source));
    let text = String::from_utf8(source).map_err(|_| MigrationError::usage("E2456", format!("migration `{file_name}` is not UTF-8"), "save migration files as UTF-8"))?;
    if !text.lines().any(|line| line.trim() == "// jet db migration/v1") {
        return Err(MigrationError::usage(
            "E2456",
            format!("migration `{file_name}` is missing the `{MIGRATION_SCHEMA}` marker"),
            "start a file with `// jet db migration/v1` or recreate it with `jet db migrate new`",
        ));
    }
    let mut name = None;
    let mut lock = MigrationLock::Exclusive;
    let mut risk = None;
    let mut up = Vec::new();
    let mut down = Vec::new();
    let mut rollback = None;
    for line in text.lines() {
        let Some((key, value)) = metadata(line) else { continue };
        match key {
            "name" => name = Some(value.trim().to_string()),
            "lock" => lock = MigrationLock::parse(value)?,
            "risk" => risk = Some(value.trim().to_string()),
            "up" | "up_sql" => up.push(parse_sql(value)?),
            "down" | "down_sql" => down.push(parse_sql(value)?),
            "rollback" => rollback = Some(value.trim().to_ascii_lowercase()),
            _ => {}
        }
    }
    let name = name.unwrap_or_else(|| slug.replace('-', " "));
    validate_name(&name, "migration")?;
    if slugify(&name) != slug {
        return Err(MigrationError::conflict(
            format!("migration `{file_name}` name does not match its filename slug"),
            "rename the file or make its `// name:` metadata match",
        ));
    }
    if up.is_empty() {
        return Err(MigrationError::usage("E2456", format!("migration `{file_name}` has no forward SQL"), "add one or more `// up: SQL{...}` lines"));
    }
    let risk = risk.filter(|value| !value.is_empty()).unwrap_or_else(|| infer_risk(&up));
    let explicit_mode = rollback.as_deref();
    let rollback_mode = match explicit_mode {
        Some("inverse") if !down.is_empty() => RollbackMode::Inverse,
        Some("irreversible") if down.is_empty() => RollbackMode::Irreversible,
        Some(other) => {
            return Err(MigrationError::usage(
                "E2456",
                format!("migration `{file_name}` has invalid rollback mode `{other}`"),
                "use `inverse` or `irreversible` with matching `down` SQL",
            ));
        }
        None if !down.is_empty() => RollbackMode::Inverse,
        None => RollbackMode::Irreversible,
    };
    let mut steps = Vec::with_capacity(up.len());
    for (index, sql) in up.into_iter().enumerate() {
        let inverse_sql = down.get(index).cloned();
        steps.push(MigrationStep::new(index as u64 + 1, sql, inverse_sql, risk.clone(), lock)?);
    }
    if down.len() > steps.len() {
        return Err(MigrationError::usage("E2456", format!("migration `{file_name}` has more down steps than up steps"), "keep one inverse statement per forward statement"));
    }
    Ok(MigrationDefinition {
        version,
        name,
        path: path.to_path_buf(),
        source_digest,
        steps,
        rollback: rollback_mode,
    })
}

fn metadata(line: &str) -> Option<(&str, &str)> {
    let line = line.trim().strip_prefix("//").unwrap_or(line.trim()).trim();
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn parse_sql(value: &str) -> Result<String, MigrationError> {
    let value = value.trim();
    let value = if let Some(inner) = value.strip_prefix("SQL{").and_then(|value| value.strip_suffix('}')) {
        unescape_sql_literal(inner)?
    } else if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        unescape_sql_literal(&value[1..value.len() - 1])?
    } else {
        value.to_string()
    };
    if value.trim().is_empty() {
        return Err(MigrationError::usage("E2456", "migration SQL cannot be empty", "write a non-empty SQL statement"));
    }
    Ok(value)
}

fn infer_risk(steps: &[String]) -> String {
    let sql = steps.iter().map(|step| step.to_ascii_lowercase()).collect::<Vec<_>>().join(" ");
    if sql.contains("drop") || sql.contains("alter") {
        "table lock".to_string()
    } else if sql.contains("update") || sql.contains("delete") || sql.contains("insert") {
        "data rewrite".to_string()
    } else if sql.contains("create index") {
        "index build".to_string()
    } else {
        "schema change".to_string()
    }
}

fn validate_name(value: &str, kind: &str) -> Result<(), MigrationError> {
    if value.is_empty() || value.len() > 96 || !value.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')) {
        return Err(MigrationError::usage(
            "E2456",
            format!("{kind} name `{value}` is not safe"),
            format!("use only ASCII letters, digits, `_`, `-`, or `.` in the {kind} name"),
        ));
    }
    Ok(())
}


fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous = None;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase()
                && previous.is_some_and(|previous: char| previous.is_ascii_lowercase())
                && !slug.ends_with('_')
            {
                slug.push('_');
            }
            slug.push(character.to_ascii_lowercase());
            previous = Some(character);
        } else if !slug.ends_with('_') {
            slug.push('_');
            previous = Some(character);
        }
    }
    while slug.ends_with('_') {
        slug.pop();
    }
    slug
}

fn safe_database_path(root: &Path, path: &Path) -> Result<PathBuf, MigrationError> {
    let mut clean = PathBuf::new();
    if path.is_absolute() {
        return Err(MigrationError::authority("absolute database paths are not accepted", "pass a database path relative to the project root"));
    }
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(value) => clean.push(value),
            Component::ParentDir => return Err(MigrationError::authority("database path contains `..`", "choose a path below the project root")),
            Component::RootDir | Component::Prefix(_) => return Err(MigrationError::authority("database path is outside the project root", "pass a relative database path")),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(MigrationError::authority("database path is empty", "pass `--database <relative-path>`"));
    }
    let database = root.join(clean);
    if database.exists() && fs::symlink_metadata(&database).map(|metadata| metadata.file_type().is_symlink()).unwrap_or(true) {
        return Err(MigrationError::authority("database target is a symlink", "use a project-owned regular database path"));
    }
    Ok(database)
}

fn escape_sql_literal(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "{{")
        .replace('}', "}}")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn unescape_sql_literal(value: &str) -> Result<String, MigrationError> {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            let Some(escaped) = chars.next() else {
                return Err(MigrationError::usage(
                    "E2456",
                    "migration SQL ends with an escape",
                    "finish the SQL literal with a complete escape",
                ));
            };
            match escaped {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                't' => output.push('\t'),
                '\\' => output.push('\\'),
                '"' => output.push('"'),
                '{' => output.push('{'),
                '}' => output.push('}'),
                other => {
                    return Err(MigrationError::usage(
                        "E2456",
                        format!("unknown SQL escape `\\{other}`"),
                        "use `\\n`, `\\r`, `\\t`, `\\\\`, `\\\"`, `\\{`, or `\\}`",
                    ));
                }
            }
        } else if character == '{' && chars.peek() == Some(&'{') {
            let _ = chars.next();
            output.push('{');
        } else if character == '}' && chars.peek() == Some(&'}') {
            let _ = chars.next();
            output.push('}');
        } else {
            output.push(character);
        }
    }
    Ok(output)
}



fn safe_error(error: &str) -> String {
    let mut result = error
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\r' | '\t'))
        .collect::<String>();
    for secret in ["password", "passwd", "token", "secret", "api_key", "apikey"] {
        if result.to_ascii_lowercase().contains(secret) {
            result = "migration operation failed without a safe public detail".to_string();
            break;
        }
    }
    if result.len() > 240 {
        result.truncate(240);
        result.push('…');
    }
    result
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

