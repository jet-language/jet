//! `jet db migrate` — checked migration files, plans, and database receipts.
//!
//! The command never interprets SQL itself. Preview renders the catalog's
//! exact statements; apply/rollback pass those statements to the shared
//! typed database migration kernel, which owns the ledger and transaction.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet::Migrations::{
    read_lock_info, render_migration, MigrationCatalog, MigrationDefinition, MigrationDirection,
    MigrationError, MigrationLock, MigrationLockInfo, MigrationPlan, MigrationStep, MigrationTarget,
    MIGRATION_DIRECTORY,
};

use crate::OutputMode;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};

/// Dispatch `jet db migrate <verb> …`.
pub(crate) fn run_migration(args: &[String], mode: OutputMode) -> i32 {
    let Some(first) = args.first().map(String::as_str) else {
        print_help(mode);
        return 0;
    };
    if matches!(first, "help" | "--help" | "-h") {
        print_help(mode);
        return 0;
    }
    if first != "migrate" {
        fail(
            MigrationError::usage(
                "E2101",
                format!("`jet db {first}` is not a database migration command"),
                "run `jet db migrate --help` to list the migration verbs",
            ),
            mode,
        );
    }
    let rest = &args[1..];
    let Some(verb) = rest.first().map(String::as_str) else {
        print_help(mode);
        return 0;
    };
    if matches!(verb, "help" | "--help" | "-h") {
        print_help(mode);
        return 0;
    }
    let options = parse_options(&rest[1..], verb).unwrap_or_else(|error| fail(error, mode));
    validate_options(&options, verb).unwrap_or_else(|error| fail(error, mode));
    let root = std::env::current_dir().unwrap_or_else(|error| {
        fail(MigrationError::io("couldn't read the current project directory", error), mode)
    });
    match verb {
        "new" => run_new(&root, &options, mode),
        "preview" => run_preview(&root, &options, mode),
        "plan" => run_plan_command(&root, &options, mode),
        "status" => run_status(&root, &options, mode),
        "drift" => run_drift(&root, &options, mode),
        "checksum" => run_checksum(&root, &options, mode),
        "locks" => run_locks(&root, &options, mode),
        "target" => run_target(&root, &options, mode),
        "apply" => run_apply(&root, &options, mode, false),
        "step" => run_apply(&root, &options, mode, true),
        "rollback" => run_rollback(&root, &options, mode),
        "resume" => run_resume(&root, &options, mode),
        other => fail(
            MigrationError::usage(
                "E2101",
                format!("unknown `jet db migrate` verb `{other}`"),
                "use new, preview, plan, status, drift, checksum, locks, target, apply, step, rollback, or resume",
            ),
            mode,
        ),
    };
    0
}

#[derive(Default)]
struct MigrationOptions {
    positionals: Vec<String>,
    target: Option<String>,
    database: Option<PathBuf>,
    lock: Option<MigrationLock>,
    to: Option<u64>,
    step: Option<u64>,
    rollback: bool,
    up: Option<String>,
    down: Option<String>,
    risk: Option<String>,
}

fn parse_options(args: &[String], verb: &str) -> Result<MigrationOptions, MigrationError> {
    let mut options = MigrationOptions::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        let takes_value = matches!(
            flag,
            "--target"
                | "--database"
                | "--lock"
                | "--to"
                | "--step"
                | "--up"
                | "--sql"
                | "--down"
                | "--risk"
        );
        if flag == "--help" || flag == "-h" {
            print_help(OutputMode {
                json: false,
                color: jet::Diagnostics::ColorChoice::Never,
                quiet: false,
            });
            exit(ExitCodes::OK);
        }
        if flag == "--rollback" {
            options.rollback = true;
            index += 1;
            continue;
        }
        if matches!(flag, "--json" | "--quiet" | "--no-color") || flag.starts_with("--color") {
            index += 1;
            continue;
        }
        if !arg.starts_with('-') {
            options.positionals.push(arg.clone());
            index += 1;
            continue;
        }
        if !takes_value {
            return Err(MigrationError::usage(
                "E2102",
                format!("unknown `jet db migrate` flag `{arg}`"),
                "use `jet db migrate --help` to see migration flags",
            ));
        }
        let value = if let Some(value) = inline {
            value.to_string()
        } else {
            index += 1;
            args.get(index).cloned().ok_or_else(|| {
                MigrationError::usage(
                    "E2104",
                    format!("`{flag}` needs a value"),
                    format!("write `{flag} <value>` for `jet db migrate {verb}`"),
                )
            })?
        };
        match flag {
            "--target" => set_once(&mut options.target, value, "--target")?,
            "--database" => set_once(&mut options.database, PathBuf::from(value), "--database")?,
            "--lock" => {
                if options.lock.is_some() {
                    return Err(MigrationError::usage("E2104", "`--lock` was supplied twice", "choose one migration lock policy"));
                }
                options.lock = Some(MigrationLock::parse(&value)?);
            }
            "--to" => {
                if options.to.is_some() {
                    return Err(MigrationError::usage("E2104", "migration target version was supplied twice", "choose one `--to` value"));
                }
                options.to = Some(parse_version(&value)?);
            }
            "--step" => {
                if options.step.is_some() {
                    return Err(MigrationError::usage("E2104", "migration step count was supplied twice", "choose one `--step` value"));
                }
                options.step = Some(parse_count(&value)?);
            }
            "--up" | "--sql" => set_once(&mut options.up, value, "--up")?,
            "--down" => set_once(&mut options.down, value, "--down")?,
            "--risk" => set_once(&mut options.risk, value, "--risk")?,
            _ => unreachable!("value flag was checked above"),
        }
        index += 1;
    }
    if verb == "step" && options.step.is_none() && options.positionals.len() == 1 {
        options.step = Some(parse_count(&options.positionals.remove(0))?);
    }
    if verb == "rollback" && options.step.is_none() && options.to.is_none() && options.positionals.len() == 1 {
        options.to = Some(parse_version(&options.positionals.remove(0))?);
    }
    Ok(options)
}
fn validate_options(options: &MigrationOptions, verb: &str) -> Result<(), MigrationError> {
    if !matches!(verb, "new" | "preview" | "plan" | "status" | "drift" | "checksum" | "locks" | "target" | "apply" | "step" | "rollback" | "resume") {
        return Ok(());
    }
    if options.to.is_some() && options.step.is_some() {
        return Err(MigrationError::usage(
            "E2104",
            "`--to` and `--step` cannot be combined",
            "choose an absolute target version or a relative step count",
        ));
    }
    let target_required = !matches!(verb, "new" | "locks");
    if target_required && options.target.is_none() {
        return Err(MigrationError::usage(
            "E2104",
            format!("`jet db migrate {verb}` needs `--target <name>`"),
            "name the exact deployment target before inspecting or changing state",
        ));
    }
    let positional_count = options.positionals.len();
    if options.rollback && !matches!(verb, "preview" | "plan" | "checksum") {
        return Err(MigrationError::usage(
            "E2104",
            format!("`--rollback` is not valid for `jet db migrate {verb}`"),
            "use `rollback` to execute a reverse plan, or use `preview --rollback` to inspect one",
        ));
    }
    let expected_positionals = match verb {
        "new" => Some(1),
        "step" | "rollback" => Some(0),
        "preview" | "plan" | "status" | "drift" | "checksum" | "locks" | "target" | "apply" | "resume" => Some(0),
        _ => None,
    };
    if expected_positionals != Some(positional_count) {
        return Err(MigrationError::usage(
            "E2104",
            format!("`jet db migrate {verb}` received unexpected positional arguments"),
            "use flags for target, version, and step selection",
        ));
    }
    let sql_options = options.up.is_some() || options.down.is_some() || options.risk.is_some();
    if sql_options && verb != "new" {
        return Err(MigrationError::usage(
            "E2104",
            format!("SQL generation flags are only valid for `jet db migrate new`, not `{verb}`"),
            "move `--up`, `--down`, and `--risk` to the `new` command",
        ));
    }
    if matches!(verb, "new" | "status" | "drift" | "checksum" | "locks" | "target" | "resume")
        && (options.to.is_some() || options.step.is_some())
    {
        return Err(MigrationError::usage(
            "E2104",
            format!("version selection is not valid for `jet db migrate {verb}`"),
            "use `preview`, `plan`, `apply`, `step`, or `rollback` to select a version",
        ));
    }
    if verb == "new" && (options.target.is_some() || options.database.is_some()) {
        return Err(MigrationError::usage(
            "E2104",
            "`new` does not accept a deployment target or database path",
            "use `--target` and `--database` only when inspecting or applying a migration",
        ));
    }
    if matches!(verb, "apply" | "step" | "rollback" | "resume")
        && matches!(options.lock, Some(MigrationLock::Shared))
    {
        return Err(MigrationError::usage(
            "E2104",
            format!("`--lock shared` is not valid for migration writer command `{verb}`"),
            "use the default exclusive lock or pass `--lock exclusive` before changing database state",
        ));
    }
    if matches!(verb, "status" | "drift" | "checksum" | "locks") && options.lock.is_some() {
        return Err(MigrationError::usage(
            "E2104",
            format!("`--lock` is not valid for read-only `{verb}`"),
            "remove `--lock`; inspection does not acquire a migration writer lock",
        ));
    }
    if verb == "locks" && options.database.is_some() {
        return Err(MigrationError::usage(
            "E2104",
            "`locks` does not select a database path",
            "use `--target <name>` to filter lock records",
        ));
    }
    Ok(())
}


fn run_new(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let Some(name) = options.positionals.first() else {
        fail(
            MigrationError::usage(
                "E2104",
                "`jet db migrate new` needs a migration name",
                "run `jet db migrate new AddIndex --up \"CREATE INDEX ...\"`",
            ),
            mode,
        );
    };
    if options.positionals.len() != 1 {
        fail(MigrationError::usage("E2104", "`jet db migrate new` takes one migration name", "put SQL in `--up`, not another positional argument"), mode);
    }
    let Some(up) = options.up.as_deref() else {
        fail(
            MigrationError::usage(
                "E2104",
                "`jet db migrate new` needs forward SQL",
                "pass `--up <SQL>` (or `--sql <SQL>`) so the generated change is reviewable, not a placeholder",
            ),
            mode,
        );
    };
    let (version, relative, text) = plan_new_migration(
        root,
        name,
        up,
        options.down.as_deref(),
        options.lock,
        options.risk.as_deref(),
        None,
    )
    .unwrap_or_else(|error| fail(error, mode));
    let path = root.join(&relative);
    if path.exists() {
        fail(
            MigrationError::conflict(
                format!("migration `{}` already exists", path.display()),
                "rename the migration or remove only the uncommitted duplicate after review",
            ),
            mode,
        );
    }
    fs::create_dir_all(path.parent().expect("migration path has a parent"))
        .unwrap_or_else(|error| fail(MigrationError::io("couldn't create migrations directory", error), mode));
    write_new_file(&path, &text).unwrap_or_else(|error| fail(error, mode));
    let relative = path.strip_prefix(root).unwrap_or(&path).display().to_string();
    if mode.json {
        let fields = StatusFields::new()
            .with("version", version)
            .with("name", name.as_str())
            .with("path", relative.as_str())
            .with(
                "checksum",
                format!("sha256:{}", jet::SHA256::sha256_hex(text.as_bytes())),
            );
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.new", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(mode, &format!("created {relative}\n"));
    }
}

fn run_preview(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    run_plan_command_with_operation(root, options, mode, "db.migrate.preview");
}
/// Plan the canonical migration filename and source once for every creator.
pub(crate) fn plan_new_migration(
    root: &Path,
    name: &str,
    up: &str,
    down: Option<&str>,
    lock: Option<MigrationLock>,
    risk: Option<&str>,
    version: Option<u64>,
) -> Result<(u64, String, String), MigrationError> {
    let catalog = MigrationCatalog::load(root)?;
    let version = version.unwrap_or_else(|| catalog.next_version());
    let lock = lock.unwrap_or(MigrationLock::Exclusive);
    let risk = risk.unwrap_or("schema change");
    let text = render_migration(version, name, up, down, lock, risk)?;
    let slug = slugify(name);
    Ok((
        version,
        format!("{MIGRATION_DIRECTORY}/{version:03}_{slug}.jet"),
        text,
    ))
}


fn run_plan_command(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    run_plan_command_with_operation(root, options, mode, "db.migrate.plan");
}

fn run_plan_command_with_operation(root: &Path, options: &MigrationOptions, mode: OutputMode, operation: &str) {
    let direction = if options.rollback {
        MigrationDirection::Rollback
    } else {
        MigrationDirection::Apply
    };
    let (catalog, target, plan) = load_plan(root, options, direction, false, mode);
    render_plan(&catalog, &target, &plan, mode, operation);
}

fn load_state(
    root: &Path,
    options: &MigrationOptions,
    mode: OutputMode,
) -> (MigrationCatalog, MigrationTarget, jet_jit::MigrationState) {
    let catalog = MigrationCatalog::load(root).unwrap_or_else(|error| fail(error, mode));
    let name = options.target.as_deref().unwrap_or("default");
    let target = MigrationTarget::new(
        root,
        name,
        options.database.as_deref(),
        options.lock.unwrap_or(MigrationLock::Shared),
    )
    .unwrap_or_else(|error| fail(error, mode));
    let state = inspect_database_state(&catalog, &target, mode);
    (catalog, target, state)
}

fn inspect_database_state(
    catalog: &MigrationCatalog,
    target: &MigrationTarget,
    mode: OutputMode,
) -> jet_jit::MigrationState {
    let query = jet_jit::MigrationStateRequest {
        target: target.name.clone(),
        target_identity: target.identity.clone(),
        database_identity: target.database_identity.clone(),
        source_identity: catalog.source_identity.clone(),
        schema_identity: jet_jit::MIGRATION_SCHEMA_IDENTITY.to_string(),
    };
    jet_jit::inspect_migrations(&target.database, query).unwrap_or_else(|error| {
        fail(
            MigrationError::authority(
                format!("couldn't inspect the database migration ledger for `{}`", target.name),
                error,
            ),
            mode,
        )
    })
}
fn run_status(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let (catalog, target, state) = load_state(root, options, mode);
    let locks = read_lock_info(root, Some(&target.name)).unwrap_or_else(|error| fail(error, mode));
    if mode.json {
        let migrations = StatusValue::array(catalog.migrations.iter().map(migration_value));
        let receipts = StatusValue::array(state.receipts.iter().map(outcome_value));
        let drift = StatusValue::array(
            state
                .drift
                .iter()
                .map(|value| StatusValue::from(value.as_str())),
        );
        let locks = StatusValue::array(locks.iter().map(lock_value));
        let fields = StatusFields::new()
            .with("target", target.name.as_str())
            .with("target_identity", target.identity.as_str())
            .with("database_identity", target.database_identity.as_str())
            .with("source_identity", state.source_identity.as_str())
            .with("state_checksum", state.checksum.as_str())
            .with("current_version", state.current_version)
            .with("drift", drift)
            .with("migrations", migrations)
            .with("receipts", receipts)
            .with("staged", StatusValue::array(Vec::<StatusValue>::new()))
            .with("locks", locks);
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.status", !state.is_drifted())
                .with_fields(fields)
                .json()
        );
        return;
    }
    crate::OutputAdapter::write_mode_status(
        mode,
        &format!(
            "target: {}\ncurrent: {}\nstate checksum: {}\ntarget identity: {}\ndatabase identity: {}\nsource identity: {}\nschema identity: {}\n",
            target.name,
            state.current_version,
            state.checksum,
            target.identity,
            target.database_identity,
            state.source_identity,
            state.schema_identity
        ),
    );
    for migration in &catalog.migrations {
        let status = if migration.version <= state.current_version { "applied" } else { "pending" };
        crate::OutputAdapter::write_mode_status(mode, &format!("{} {:03} {}\n", status, migration.version, migration.name));
    }
    for receipt in state.receipts {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!("receipt: database ledger:{} ({})\n", receipt.receipt_id, receipt.status),
        );
    }
    for lock in locks {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "lock: {} {} {} pid={} {}\n",
                lock.lock.as_str(),
                lock.target,
                lock.database,
                lock.pid,
                lock.path.display()
            ),
        );
    }
    for message in state.drift {
        crate::OutputAdapter::write_mode_status(mode, &format!("drift: {message}\n"));
    }
}

fn run_drift(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let (_catalog, target, state) = load_state(root, options, mode);
    if mode.json {
        let drift = StatusValue::array(
            state
                .drift
                .iter()
                .map(|value| StatusValue::from(value.as_str())),
        );
        let fields = StatusFields::new()
            .with("target", target.name.as_str())
            .with("target_identity", target.identity.as_str())
            .with("database_identity", target.database_identity.as_str())
            .with("state_checksum", state.checksum.as_str())
            .with("drift", drift);
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.drift", !state.is_drifted())
                .with_fields(fields)
                .json()
        );
    } else if state.is_drifted() {
        crate::OutputAdapter::write_mode_status(mode, &format!("target: {}\ndrift detected:\n{}\n", target.name, state.drift.iter().map(|message| format!("- {message}")).collect::<Vec<_>>().join("\n")));
    } else {
        crate::OutputAdapter::write_mode_status(mode, &format!("target: {}\nclean; state checksum: {}\n", target.name, state.checksum));
    }
}

fn run_checksum(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let (catalog, target, state) = load_state(root, options, mode);
    let plan_checksum = if options.to.is_some() || options.step.is_some() {
        let direction = if options.rollback {
            MigrationDirection::Rollback
        } else {
            MigrationDirection::Apply
        };
        let (plan_catalog, plan_target, plan) = load_plan(root, options, direction, false, mode);
        Some(migration_request_checksum(
            &plan_catalog,
            &plan_target,
            &plan,
        ))
    } else {
        None
    };
    if mode.json {
        let mut fields = StatusFields::new()
            .with("target", target.name.as_str())
            .with("target_identity", target.identity.as_str())
            .with("database_identity", target.database_identity.as_str())
            .with("catalog_checksum", catalog.source_identity.as_str())
            .with("schema_identity", state.schema_identity.as_str())
            .with("state_checksum", state.checksum.as_str());
        if let Some(plan_checksum) = plan_checksum.as_deref() {
            fields = fields.with("plan_checksum", plan_checksum);
        }
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.checksum", !state.is_drifted())
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "target: {}\ncatalog checksum: {}\nstate checksum: {}\ntarget identity: {}\ndatabase identity: {}\n{}",
                target.name,
                catalog.source_identity,
                state.checksum,
                target.identity,
                target.database_identity,
                plan_checksum.map(|value| format!("plan checksum: {value}\n")).unwrap_or_default()
            ),
        );
    }
}

fn run_locks(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let locks = read_lock_info(root, options.target.as_deref()).unwrap_or_else(|error| fail(error, mode));
    if mode.json {
        let locks = StatusValue::array(locks.iter().map(lock_value));
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.locks", true)
                .with_field("locks", locks)
                .json()
        );
    } else if locks.is_empty() {
        crate::OutputAdapter::write_mode_status(mode, "no active migration locks\n");
    } else {
        for lock in locks {
            crate::OutputAdapter::write_mode_status(
                mode,
                &format!(
                    "{} {} target_identity={} pid={} database={} database_identity={} ({})\n",
                    lock.lock.as_str(),
                    lock.target,
                    lock.target_identity,
                    lock.pid,
                    lock.database,
                    lock.database_identity,
                    lock.path.display()
                ),
            );
        }
    }
}

fn run_target(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let Some(name) = options.target.as_deref() else {
        fail(
            MigrationError::usage("E2104", "`jet db migrate target` needs `--target <name>`", "name the deployment target explicitly, for example `--target prod`"),
            mode,
        );
    };
    let target = MigrationTarget::new(root, name, options.database.as_deref(), options.lock.unwrap_or(MigrationLock::Exclusive))
        .unwrap_or_else(|error| fail(error, mode));
    let relative = target.database.strip_prefix(root).unwrap_or(&target.database).display().to_string();
    if mode.json {
        let fields = StatusFields::new()
            .with("target", target.name.as_str())
            .with("database", relative.as_str())
            .with("identity", target.identity.as_str())
            .with("database_identity", target.database_identity.as_str())
            .with("lock", target.lock.as_str());
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.target", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(mode, &format!("target: {}\ndatabase: {}\nlock: {}\nidentity: {}\ndatabase identity: {}\n", target.name, relative, target.lock.as_str(), target.identity, target.database_identity));
    }
}

fn run_apply(root: &Path, options: &MigrationOptions, mode: OutputMode, step_command: bool) {
    let (catalog, target, plan) = load_plan(root, options, MigrationDirection::Apply, step_command, mode);
    run_plan(&catalog, &target, &plan, mode);
}

fn run_rollback(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let (catalog, target, plan) = load_plan(root, options, MigrationDirection::Rollback, false, mode);
    run_plan(&catalog, &target, &plan, mode);
}
fn run_resume(root: &Path, options: &MigrationOptions, mode: OutputMode) {
    let Some(name) = options.target.as_deref() else {
        fail(MigrationError::usage("E2104", "`jet db migrate resume` needs `--target <name>`", "resume only after naming the exact target"), mode);
    };
    let catalog = MigrationCatalog::load(root).unwrap_or_else(|error| fail(error, mode));
    let target = MigrationTarget::new(
        root,
        name,
        options.database.as_deref(),
        options.lock.unwrap_or(MigrationLock::Shared),
    )
    .unwrap_or_else(|error| fail(error, mode));
    let state = inspect_database_state(&catalog, &target, mode);
    if state.is_drifted() {
        fail(
            MigrationError::authority(
                format!("target `{name}` still has migration drift"),
                state.drift.join("; "),
            ),
            mode,
        );
    }
    if mode.json {
        let receipts = StatusValue::array(state.receipts.iter().map(outcome_value));
        let fields = StatusFields::new()
            .with("target", name)
            .with("receipts", receipts)
            .with("state_checksum", state.checksum.as_str())
            .with("status", "no_reconciliation_needed");
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.resume", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "target: {name}\nno database migration reconciliation is pending; state checksum: {}\n",
                state.checksum
            ),
        );
    }
}

fn load_plan(
    root: &Path,
    options: &MigrationOptions,
    direction: MigrationDirection,
    step_command: bool,
    mode: OutputMode,
) -> (MigrationCatalog, MigrationTarget, MigrationPlan) {
    let Some(name) = options.target.as_deref() else {
        fail(
            MigrationError::usage("E2104", "migration target is required", "pass `--target <name>`; writes never choose an ambient database"),
            mode,
        );
    };
    let lock = options.lock.unwrap_or(MigrationLock::Exclusive);
    let target = MigrationTarget::new(root, name, options.database.as_deref(), lock)
        .unwrap_or_else(|error| fail(error, mode));
    let catalog = MigrationCatalog::load(root).unwrap_or_else(|error| fail(error, mode));
    let state = inspect_database_state(&catalog, &target, mode);
    if state.is_drifted() {
        fail(
            MigrationError::authority(
                format!("migration target `{name}` has unreconciled state"),
                state.drift.join("; "),
            ),
            mode,
        );
    }
    let current = state.current_version;
    let requested = if step_command || options.step.is_some() {
        let count = options.step.unwrap_or(1);
        if count == 0 {
            fail(MigrationError::usage("E2104", "migration step count must be positive", "use `--step 1` or a larger count"), mode);
        }
        Some(match direction {
            MigrationDirection::Apply => version_after(&catalog, current, count).unwrap_or_else(|| fail(MigrationError::usage("E2456", "step count exceeds pending migrations", "choose a smaller step count or inspect status"), mode)),
            MigrationDirection::Rollback => version_before(&catalog, current, count).unwrap_or_else(|| fail(MigrationError::usage("E2456", "step count exceeds applied migrations", "choose a smaller step count or inspect status"), mode)),
        })
    } else {
        options.to
    };
    let plan = match direction {
        MigrationDirection::Apply => catalog.plan_apply(target.clone(), current, requested),
        MigrationDirection::Rollback => catalog.plan_rollback(target.clone(), current, requested),
    }
    .unwrap_or_else(|error| fail(error, mode));
    (catalog, target, plan)
}

fn migration_request(
    catalog: &MigrationCatalog,
    target: &MigrationTarget,
    plan: &MigrationPlan,
) -> jet_jit::MigrationRequest {
    let operation = match plan.direction {
        MigrationDirection::Apply => jet_jit::MigrationOperation::Apply,
        MigrationDirection::Rollback => jet_jit::MigrationOperation::Rollback,
    };
    jet_jit::MigrationRequest {
        operation,
        migration_key: format!(
            "migration:{}:{}",
            plan.from_version.min(plan.to_version),
            plan.from_version.max(plan.to_version)
        ),
        target: target.name.clone(),
        target_identity: target.identity.clone(),
        database_identity: target.database_identity.clone(),
        source_identity: catalog.source_identity.clone(),
        schema_identity: jet_jit::MIGRATION_SCHEMA_IDENTITY.to_string(),
        tool_identity: plan.tool_identity.clone(),
        from_version: plan.from_version,
        to_version: plan.to_version,
        lock: match target.lock {
            MigrationLock::Shared => jet_jit::MigrationLock::Shared,
            MigrationLock::Exclusive => jet_jit::MigrationLock::Exclusive,
        },
        risk: plan.risk.clone(),
        steps: plan
            .steps
            .iter()
            .map(|step| jet_jit::MigrationSql {
                ordinal: step.ordinal,
                sql: step.sql.clone(),
                inverse_sql: step.inverse_sql.clone(),
                risk: step.risk.clone(),
                lock: match step.lock {
                    MigrationLock::Shared => jet_jit::MigrationLock::Shared,
                    MigrationLock::Exclusive => jet_jit::MigrationLock::Exclusive,
                },
            })
            .collect(),
    }
}

fn migration_request_checksum(
    catalog: &MigrationCatalog,
    target: &MigrationTarget,
    plan: &MigrationPlan,
) -> String {
    jet_jit::migration_request_checksum(&migration_request(catalog, target, plan))
}

fn run_plan(catalog: &MigrationCatalog, target: &MigrationTarget, plan: &MigrationPlan, mode: OutputMode) {
    if plan.steps.is_empty() {
        if mode.json {
            println!(
                "{}",
                StatusEnvelope::new(
                    format!("db.migrate.{}", plan.direction.as_str()),
                    true,
                )
                .with_field("status", "up_to_date")
                .with_field("target", target.name.as_str())
                .with_field("direction", plan.direction.as_str())
                .with_field("version", plan.to_version)
                .json()
            );
        } else {
            crate::OutputAdapter::write_mode_status(mode, &format!("target {} is already at migration {}\n", target.name, plan.to_version));
        }
        return;
    }
    let _lock_guard = target
        .acquire_lock()
        .unwrap_or_else(|error| fail(error, mode));
    let request = migration_request(catalog, target, plan);
    let outcome = jet_jit::run_migration(&target.database, request).unwrap_or_else(|error| {
        fail(
            MigrationError::authority(
                format!("database migration `{}` failed", plan.direction.as_str()),
                error,
            ),
            mode,
        )
    });
    if mode.json {
        let receipt = outcome_value(&outcome);
        let fields = StatusFields::new()
            .with("receipt", receipt)
            .with("receipt_store", "database")
            .with("ledger", "__jet_migrations");
        println!(
            "{}",
            StatusEnvelope::new(
                format!("db.migrate.{}", plan.direction.as_str()),
                outcome.error.is_none(),
            )
            .with_fields(fields)
            .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "receipt: database ledger:{}\nstatus: {}\ntarget: {}\nversion: {}\nlock: {}\nrisk: {}\n",
                outcome.receipt_id,
                outcome.status,
                target.name,
                outcome.to_version,
                migration_lock_name(outcome.lock),
                outcome.risk
            ),
        );
    }
}

fn render_plan(catalog: &MigrationCatalog, target: &MigrationTarget, plan: &MigrationPlan, mode: OutputMode, operation: &str) {
    let checksum = migration_request_checksum(catalog, target, plan);
    if mode.json {
        let steps = StatusValue::array(plan.steps.iter().map(step_value));
        let fields = StatusFields::new()
            .with("target", target.name.as_str())
            .with("target_identity", target.identity.as_str())
            .with("database_identity", target.database_identity.as_str())
            .with("source_identity", plan.source_identity.as_str())
            .with("schema_identity", jet_jit::MIGRATION_SCHEMA_IDENTITY)
            .with("direction", plan.direction.as_str())
            .with("from", plan.from_version)
            .with("to", plan.to_version)
            .with("checksum", checksum.as_str())
            .with("risk", plan.risk.as_str())
            .with("lock", plan.target.lock.as_str())
            .with("steps", steps);
        println!(
            "{}",
            StatusEnvelope::new(operation, true)
                .with_fields(fields)
                .json()
        );
        return;
    }
    crate::OutputAdapter::write_mode_status(
        mode,
        &format!(
            "target: {}\nfrom: {}\nto: {}\ndirection: {}\nchecksum: {}\ntarget identity: {}\ndatabase identity: {}\nlock: {}\nrisk: {}\n",
            target.name,
            plan.from_version,
            plan.to_version,
            plan.direction.as_str(),
            checksum,
            target.identity,
            target.database_identity,
            plan.target.lock.as_str(),
            plan.risk
        ),
    );
    for step in &plan.steps {
        crate::OutputAdapter::write_mode_status(mode, &format!("SQL: {}\nrisk: {}\nlock: {}\n", step.sql, step.risk, step.lock.as_str()));
    }
}


fn write_new_file(path: &Path, text: &str) -> Result<(), MigrationError> {
    let temp = path.with_extension("jet.tmp");
    if temp.exists() {
        return Err(MigrationError::conflict(format!("temporary migration path `{}` already exists", temp.display()), "remove the stale temporary file after checking its contents"));
    }
    fs::write(&temp, text).map_err(|error| MigrationError::io("couldn't stage migration file", error))?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(MigrationError::io("couldn't publish migration file", error));
    }
    Ok(())
}

fn version_after(catalog: &MigrationCatalog, current: u64, count: u64) -> Option<u64> {
    catalog
        .migrations
        .iter()
        .filter(|migration| migration.version > current)
        .nth(count.checked_sub(1)? as usize)
        .map(|migration| migration.version)
}

fn version_before(catalog: &MigrationCatalog, current: u64, count: u64) -> Option<u64> {
    let previous = catalog
        .migrations
        .iter()
        .filter(|migration| migration.version < current)
        .map(|migration| migration.version)
        .collect::<Vec<_>>();
    if let Some(version) = previous.iter().rev().nth(count.checked_sub(1)? as usize) {
        Some(*version)
    } else if count == previous.len() as u64 + 1 {
        Some(0)
    } else {
        None
    }
}

fn parse_version(value: &str) -> Result<u64, MigrationError> {
    let version = value.parse::<u64>().map_err(|_| MigrationError::usage("E2104", format!("`{value}` is not a migration version"), "use a positive decimal version"))?;
    if version == 0 {
        return Err(MigrationError::usage("E2104", "migration versions start at 1", "use a positive version"));
    }
    Ok(version)
}

fn parse_count(value: &str) -> Result<u64, MigrationError> {
    let count = value.parse::<u64>().map_err(|_| MigrationError::usage("E2104", format!("`{value}` is not a migration step count"), "use a positive decimal count"))?;
    if count == 0 {
        return Err(MigrationError::usage("E2104", "migration step count must be positive", "use `--step 1` or a larger count"));
    }
    Ok(count)
}

fn set_once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), MigrationError> {
    if slot.is_some() {
        return Err(MigrationError::usage("E2104", format!("`{flag}` was supplied twice"), "provide one value per migration option"));
    }
    *slot = Some(value);
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

fn migration_lock_name(lock: jet_jit::MigrationLock) -> &'static str {
    match lock {
        jet_jit::MigrationLock::Shared => "shared",
        jet_jit::MigrationLock::Exclusive => "exclusive",
    }
}

fn migration_value(migration: &MigrationDefinition) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("version", migration.version)
            .with("name", migration.name.as_str())
            .with("rollback", migration.has_rollback()),
    )
}

fn lock_value(lock: &MigrationLockInfo) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("target", lock.target.as_str())
            .with("target_identity", lock.target_identity.as_str())
            .with("database", lock.database.as_str())
            .with("database_identity", lock.database_identity.as_str())
            .with("lock", lock.lock.as_str())
            .with("pid", lock.pid)
            .with("path", lock.path.display().to_string()),
    )
}

fn step_value(step: &MigrationStep) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("ordinal", step.ordinal)
            .with("sql", step.sql.as_str())
            .with("risk", step.risk.as_str())
            .with("lock", step.lock.as_str()),
    )
}

fn outcome_value(outcome: &jet_jit::MigrationOutcome) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("receipt_id", outcome.receipt_id.as_str())
            .with("operation", outcome.operation.as_str())
            .with("status", outcome.status.as_str())
            .with("target", outcome.target.as_str())
            .with("target_identity", outcome.target_identity.as_str())
            .with("source_identity", outcome.source_identity.as_str())
            .with("schema_identity", outcome.schema_identity.as_str())
            .with("database_identity", outcome.database_identity.as_str())
            .with("tool_identity", outcome.tool_identity.as_str())
            .with("from_version", outcome.from_version)
            .with("to_version", outcome.to_version)
            .with("step_count", outcome.step_count)
            .with(
                "step_ids",
                StatusValue::array(
                    outcome
                        .step_ids
                        .iter()
                        .map(|step| StatusValue::from(step.as_str())),
                ),
            )
            .with("checksum", outcome.checksum.as_str())
            .with("lock", migration_lock_name(outcome.lock))
            .with("risk", outcome.risk.as_str())
            .with(
                "started_unix_ms",
                StatusValue::Integer(outcome.started_unix_ms as i128),
            )
            .with(
                "finished_unix_ms",
                StatusValue::Integer(outcome.finished_unix_ms as i128),
            )
            .with(
                "error",
                outcome
                    .error
                    .as_deref()
                    .map(StatusValue::from)
                    .unwrap_or(StatusValue::Null),
            ),
    )
}

fn fail(error: MigrationError, mode: OutputMode) -> ! {
    crate::emit_cli_report(
        error.code,
        error.message,
        "migration commands keep versioned SQL, target identity, and receipts together".to_string(),
        error.fix,
        mode.json,
    );
    exit(ExitCodes::USER_ERROR)
}

fn print_help(mode: OutputMode) {
    let text = "jet db migrate — versioned, reviewable database changes\n\nUsage:\n  jet db migrate new <Name> --up <SQL> [--down <SQL>] [--lock shared|exclusive] [--risk <note>]\n  jet db migrate preview --target <name> [--to <version>|--step <count>]\n  jet db migrate plan --target <name> [--to <version>|--step <count>]\n  jet db migrate status --target <name>\n  jet db migrate drift --target <name>\n  jet db migrate checksum --target <name> [--to <version>|--step <count>]\n  jet db migrate locks [--target <name>]\n  jet db migrate target --target <name> [--database <relative-path>] [--lock shared|exclusive]\n  jet db migrate apply --target <name> [--to <version>|--step <count>] [--lock exclusive]\n  jet db migrate step --target <name> <count> [--lock exclusive]\n  jet db migrate rollback --target <name> [<version>|--step <count>] [--lock exclusive]\n  jet db migrate resume --target <name> [--database <relative-path>] [--lock exclusive]\n\npreview and plan never write. status, drift, checksum, and locks inspect one canonical target state. apply stages a receipt, acquires the selected target lock, and commits the checked SQL transaction before publishing the receipt. rollback requires checked inverse SQL; unsupported dual-write metadata is rejected. Target names and lock policy are explicit for writes; receipt identities do not contain database contents or secrets.\n";
    if mode.json {
        println!(
            "{}",
            StatusEnvelope::new("db.migrate.help", true)
                .with_field("help", text)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(mode, text);
    }
}
