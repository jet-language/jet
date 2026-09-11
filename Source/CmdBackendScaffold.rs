//! `jet new service|route|job|migration` — additive backend scaffolds.
//!
//! Scaffolds are source, not compiler state.  Preview is the default; only
//! `--apply` publishes a deterministic plan through the existing codemod
//! transaction.  `--remove` consumes the scaffold receipt and refuses edited
//! or unowned files.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::exit;

use jet::ExitCodes;
use jet::Migrations::MigrationLock;
use jet_foundation::DataTree::DataTree;
use jet_foundation::EncodingJson::parse_json;
use jet_foundation::JSON::json_escape as json_quote;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use crate::CmdCodemod::Transaction::{self, Change};
use crate::OutputMode;

const RECEIPT_SCHEMA: &str = "jet-scaffold-v1";
const MAX_RECEIPT_BYTES: usize = 1024 * 1024;
const ROUTE_REGISTRATION: &str = ".routes(from: \"routes\")";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Preview,
    Apply,
    Remove,
}

impl Default for Operation {
    fn default() -> Self {
        Self::Preview
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScaffoldKind {
    Service,
    Route,
    Job,
    Migration,
}

impl ScaffoldKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "service" => Some(Self::Service),
            "route" => Some(Self::Route),
            "job" => Some(Self::Job),
            "migration" => Some(Self::Migration),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Route => "route",
            Self::Job => "job",
            Self::Migration => "migration",
        }
    }
}

#[derive(Clone, Default)]
struct Options {
    operation: Operation,
    path: Option<String>,
    model: Option<String>,
    up: Option<String>,
    down: Option<String>,
    risk: Option<String>,
    lock: Option<MigrationLock>,
    version: Option<u64>,
}

struct PlannedFile {
    relative: String,
    absolute: PathBuf,
    bytes: Vec<u8>,
    before: Option<Vec<u8>>,
}


struct ScaffoldPlan {
    files: Vec<PlannedFile>,
    model: Option<String>,
    route: Option<String>,
    migration: Option<String>,
}

/// Dispatch one backend scaffold after `main` has resolved `new <kind> <name>`.
pub(crate) fn run_new_backend(kind: &str, name: &str, args: &[String], mode: OutputMode) {
    let kind = ScaffoldKind::parse(kind).unwrap_or_else(|| {
        fail(
            "E2101",
            format!("unknown backend scaffold `{kind}`"),
            "choose service, route, job, or migration",
            mode,
        )
    });
    validate_name(name, kind, mode);
    let args = backend_option_args(args, kind, name, mode);
    let options = parse_options(args, kind, name, mode);
    if options.operation == Operation::Remove {
        run_remove(kind, name, mode);
        return;
    }
    let root = std::env::current_dir().unwrap_or_else(|error| {
        fail(
            "E2105",
            format!("couldn't read the current project directory: {error}"),
            "run the scaffold from a readable project directory",
            mode,
        )
    });
    let receipt_path = receipt_path(&root, kind, name);
    let existing_receipt = read_regular(&receipt_path, mode);
    let existing_files = existing_receipt
        .as_deref()
        .and_then(|bytes| parse_receipt(&root, kind, name, &receipt_path, bytes));
    let mut options = reuse_receipt_migration_version(&options, existing_files.as_deref(), name);
    let plan = render_files(&root, kind, name, &options, existing_files.as_deref(), mode);
    if options.version.is_none() {
        options.version = plan
            .migration
            .as_deref()
            .and_then(|version| version.parse::<u64>().ok());
    }
    let receipt = render_receipt(&root, kind, name, &options, &plan.files, &receipt_path);
    if options.operation == Operation::Preview {
        render_preview(kind, name, &plan, &receipt_path, mode);
        return;
    }
    apply_files(kind, name, &plan.files, &receipt_path, &receipt, mode);
}

fn backend_option_args<'a>(
    args: &'a [String],
    kind: ScaffoldKind,
    name: &str,
    mode: OutputMode,
) -> &'a [String] {
    if args.first().map(String::as_str) != Some("new") {
        return args;
    }
    if args.get(1).map(String::as_str) != Some(kind.as_str())
        || args.get(2).map(String::as_str) != Some(name)
    {
        fail(
            "E2104",
            "backend scaffold arguments do not match the selected kind and name",
            "pass one kind and one name after `new`",
            mode,
        );
    }
    &args[3..]
}
fn parse_options(args: &[String], kind: ScaffoldKind, name: &str, mode: OutputMode) -> Options {
    let mut options = Options::default();
    let mut operation = None;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "new" {
            if args.get(index + 1).map(String::as_str) != Some(kind.as_str())
                || args.get(index + 2).map(String::as_str) != Some(name)
            {
                fail(
                    "E2104",
                    "backend scaffold arguments do not match the selected kind and name",
                    "pass one kind and one name after `new`",
                    mode,
                );
            }
            index += 3;
            continue;
        }
        if !arg.starts_with('-') {
            fail(
                "E2104",
                format!("`jet new {} ` received unexpected positional `{arg}`", kind.as_str()),
                "put scaffold options after the one required name",
                mode,
            );
        }
        if matches!(arg.as_str(), "--preview" | "--apply" | "--remove") {
            let next = match arg.as_str() {
                "--preview" => Operation::Preview,
                "--apply" => Operation::Apply,
                "--remove" => Operation::Remove,
                _ => unreachable!(),
            };
            if operation.is_some_and(|current| current != next) {
                fail(
                    "E2104",
                    "scaffold operation flags are mutually exclusive",
                    "choose exactly one of `--preview`, `--apply`, or `--remove`",
                    mode,
                );
            }
            operation = Some(next);
            index += 1;
            continue;
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(flag, value)| (flag, Some(value)));
        if matches!(flag, "--json" | "--quiet" | "--no-color" | "--color")
            || flag.starts_with("--color=")
        {
            index += 1;
            if flag == "--color" && inline.is_none() && args.get(index).is_some_and(|value| {
                matches!(value.as_str(), "auto" | "always" | "never")
            }) {
                index += 1;
            }
            continue;
        }
        let takes_value = matches!(
            flag,
            "--path"
                | "--route"
                | "--model"
                | "--up"
                | "--sql"
                | "--down"
                | "--risk"
                | "--lock"
                | "--version"
        );
        if !takes_value {
            fail(
                "E2102",
                format!("unknown `jet new {} ` flag `{arg}`", kind.as_str()),
                "use `jet new --help` to see scaffold options",
                mode,
            );
        }
        let value = if let Some(value) = inline {
            value.to_string()
        } else {
            index += 1;
            args.get(index).cloned().unwrap_or_else(|| {
                fail(
                    "E2104",
                    format!("`{flag}` needs a value"),
                    format!("write `{flag} <value>` for `jet new {}`", kind.as_str()),
                    mode,
                )
            })
        };
        match flag {
            "--path" | "--route" => set_once(&mut options.path, value, flag, mode),
            "--model" => set_once(&mut options.model, value, flag, mode),
            "--up" | "--sql" => set_once(&mut options.up, value, "--up", mode),
            "--down" => set_once(&mut options.down, value, flag, mode),
            "--risk" => set_once(&mut options.risk, value, flag, mode),
            "--lock" => {
                if options.lock.is_some() {
                    fail("E2104", "`--lock` was supplied twice", "choose one lock policy", mode);
                }
                options.lock =
                    Some(MigrationLock::parse(&value).unwrap_or_else(|error| {
                        fail("E2456", error.message, error.fix, mode)
                    }));
            }
            "--version" => {
                if options.version.is_some() {
                    fail(
                        "E2104",
                        "`--version` was supplied twice",
                        "choose one migration version",
                        mode,
                    );
                }
                let version = value.parse::<u64>().unwrap_or_else(|_| {
                    fail(
                        "E2104",
                        format!("`{value}` is not a migration version"),
                        "use a positive decimal version",
                        mode,
                    )
                });
                if version == 0 {
                    fail(
                        "E2104",
                        "migration versions start at 1",
                        "use a positive migration version",
                        mode,
                    );
                }
                options.version = Some(version);
            }
            _ => unreachable!(),
        }
        index += 1;
    }
    options.operation = operation.unwrap_or_default();
    if options.operation == Operation::Remove
        && [
            options.path.is_some(),
            options.model.is_some(),
            options.up.is_some(),
            options.down.is_some(),
            options.risk.is_some(),
            options.lock.is_some(),
            options.version.is_some(),
        ]
        .into_iter()
        .any(|set| set)
    {
        fail(
            "E2104",
            "`--remove` cannot be combined with generation options",
            "remove by the original kind and name only",
            mode,
        );
    }
    let invalid = match kind {
        ScaffoldKind::Service => false,
        ScaffoldKind::Route => {
            options.model.is_some()
                || options.up.is_some()
                || options.down.is_some()
                || options.risk.is_some()
                || options.lock.is_some()
                || options.version.is_some()
        }
        ScaffoldKind::Job => [
            options.path.is_some(),
            options.model.is_some(),
            options.up.is_some(),
            options.down.is_some(),
            options.risk.is_some(),
            options.lock.is_some(),
            options.version.is_some(),
        ]
        .into_iter()
        .any(|set| set),
        ScaffoldKind::Migration => options.path.is_some() || options.model.is_some(),
    };
    if invalid {
        fail(
            "E2104",
            format!("one or more scaffold options do not apply to `{}`", kind.as_str()),
            "use only the flags documented for this scaffold kind",
            mode,
        );
    }
    if options
        .risk
        .as_deref()
        .is_some_and(|risk| risk.len() > 256 || risk.chars().any(char::is_control))
    {
        fail(
            "E2104",
            "migration risk text contains unsupported control characters or is too long",
            "use a short single-line risk summary",
            mode,
        );
    }
    options
}

fn render_files(
    root: &Path,
    kind: ScaffoldKind,
    name: &str,
    options: &Options,
    existing_files: Option<&[ReceiptFile]>,
    mode: OutputMode,
) -> ScaffoldPlan {
    let slug = slugify(name);
    let (rendered, model, route) = match kind {
        ScaffoldKind::Service => {
            let (rendered, model, route) = service_slice(root, name, &slug, options, mode);
            (rendered, Some(model), Some(route))
        }
        ScaffoldKind::Route => {
            let route = options.path.as_deref().unwrap_or("");
            let route = if route.is_empty() {
                format!("/{slug}")
            } else {
                route.to_string()
            };
            validate_route(&route, mode);
            let relative = route_file_relative(&route, mode);
            (
                vec![(relative, route_source(name, &route).into_bytes())],
                None,
                Some(route),
            )
        }
        ScaffoldKind::Job => (
            vec![(
                format!("jobs/job_{slug}.jet"),
                job_source(name, &slug).into_bytes(),
            )],
            None,
            None,
        ),
        ScaffoldKind::Migration => {
            let Some(up) = options.up.as_deref() else {
                fail(
                    "E2104",
                    "migration scaffold needs forward SQL",
                    "pass `--up <SQL>` so the generated change is reviewable",
                    mode,
                );
            };
            (
                vec![migration_file(
                    root,
                    name,
                    up,
                    options.down.as_deref(),
                    options,
                    mode,
                )],
                None,
                None,
            )
        }
    };
    let migration = rendered.iter().find_map(|(relative, _)| {
        relative
            .strip_prefix("migrations/")
            .and_then(|path| path.split_once('_'))
            .map(|(version, _)| version.to_owned())
    });
    let mut files = rendered
        .into_iter()
        .map(|(relative, bytes)| {
            let absolute = root.join(&relative);
            validate_destination(root, &absolute, mode);
            PlannedFile {
                relative,
                absolute,
                bytes,
                before: None,
            }
        })
        .collect::<Vec<_>>();
    if matches!(kind, ScaffoldKind::Service | ScaffoldKind::Route) {
        if let Some(app) = app_routes_registration(root, mode, existing_files) {
            files.push(app);
        }
    }
    ScaffoldPlan {
        files,
        model,
        route,
        migration,
    }
}


fn service_slice(
    root: &Path,
    name: &str,
    slug: &str,
    options: &Options,
    mode: OutputMode,
) -> (Vec<(String, Vec<u8>)>, String, String) {
    let model = options
        .model
        .as_deref()
        .map(model_name)
        .unwrap_or_else(|| model_name(name));
    if !valid_identifier(&model) {
        fail(
            "E2104",
            format!("service model name `{model}` is not a valid identifier"),
            "use `--model <Name>` beginning with a letter or underscore",
            mode,
        );
    }
    let model_slug = slugify(&model);
    let route = options.path.as_deref().unwrap_or("");
    let route = if route.is_empty() {
        format!("/{slug}")
    } else {
        route.to_string()
    };
    validate_route(&route, mode);
    let migration_up = options
        .up
        .clone()
        .unwrap_or_else(|| format!("CREATE TABLE {slug} (id INTEGER PRIMARY KEY);"));
    let migration_down = options
        .down
        .clone()
        .or_else(|| Some(format!("DROP TABLE {slug};")));
    let migration = migration_file(
        root,
        name,
        &migration_up,
        migration_down.as_deref(),
        options,
        mode,
    );
    let route_file = route_file_relative(&route, mode);

    (
        vec![
            (
                format!("services/{slug}.jet"),
                service_source(name, slug).into_bytes(),
            ),
            (
                route_file,
                route_source(name, &route).into_bytes(),
            ),
            (
                format!("models/model_{model_slug}.jet"),
                model_source(&model).into_bytes(),
            ),
            (
                format!("jobs/job_{slug}.jet"),
                job_source(name, slug).into_bytes(),
            ),
            migration,
            (
                format!("tests/{slug}.jet"),
                test_source(name, &model).into_bytes(),
            ),
        ],
        model,
        route,
    )
}
fn app_routes_registration(
    root: &Path,
    mode: OutputMode,
    existing_files: Option<&[ReceiptFile]>,
) -> Option<PlannedFile> {
    let entry = crate::find_project_entry(root);
    validate_destination(root, &entry, mode);
    let before = read_regular(&entry, mode).unwrap_or_else(|| {
        fail(
            "E2105",
            format!("couldn't read selected App entry `{}`", entry.display()),
            "check project read authority and retry",
            mode,
        )
    });
    let source = std::str::from_utf8(&before).unwrap_or_else(|_| {
        fail(
            "E2456",
            format!("selected App entry `{}` is not valid UTF-8", entry.display()),
            "keep the checked App entry in UTF-8 source",
            mode,
        )
    });
    let relative = entry
        .strip_prefix(root)
        .unwrap_or_else(|_| {
            fail(
                "E2456",
                format!("selected App entry `{}` escapes the project", entry.display()),
                "run scaffolding from the selected project root",
                mode,
            )
        })
        .to_string_lossy()
        .replace('\\', "/");
    let entry_text = entry.to_string_lossy().into_owned();
    let bundle = jet::Loader::load_entry(&entry_text).unwrap_or_else(|diagnostics| {
        let detail = diagnostics
            .first()
            .map(|diagnostic| diagnostic.what.as_str())
            .unwrap_or("the loader returned no diagnostic");
        fail(
            "E2456",
            format!(
                "selected project entry `{}` is not a checked App root: {detail}",
                entry.display()
            ),
            "define one checked `fn run() App` entry before generating routes",
            mode,
        )
    });
    let module = bundle.modules.get(bundle.entry).unwrap_or_else(|| {
        fail(
            "E2456",
            format!("selected project entry `{}` has no loaded source module", entry.display()),
            "repair the checked project entry before generating routes",
            mode,
        )
    });
    let run = jet::AST::app_entry_run_fn(&module.items).unwrap_or_else(|| {
        fail(
            "E2456",
            format!(
                "selected project entry `{}` has no checked `fn run() App`",
                entry.display()
            ),
            "define one checked `fn run() App` entry before generating routes",
            mode,
        )
    });
    let (start, end) = (run.span.start, run.span.end);
    let function_source = source.get(start..end).unwrap_or_else(|| {
        fail(
            "E2456",
            format!(
                "checked App span for `{}` does not match its source",
                entry.display()
            ),
            "restore the selected App entry and retry scaffolding",
            mode,
        )
    });
    let recorded_app = existing_files.and_then(|files| {
        files
            .iter()
            .find(|file| file.relative.as_str() == relative.as_str() && file.before.is_some())
    });
    if has_routes_registration(function_source) {
        let Some(receipt_file) = recorded_app else {
            return None;
        };
        let Some(receipt_before) = receipt_file.before.as_deref() else {
            return None;
        };
        if receipt_file.digest != jet::SHA256::sha256_hex(&before)
            || !app_registration_inverse(&entry, &before, receipt_before)
        {
            fail(
                "E2456",
                format!("selected App entry `{}` drifted after scaffold apply", entry.display()),
                "restore the recorded App route registration or remove the scaffold after review",
                mode,
            );
        }
        return Some(PlannedFile {
            relative,
            absolute: entry,
            bytes: before.clone(),
            before: Some(receipt_before.to_vec()),
        });
    }
    if recorded_app.is_some() {
        fail(
            "E2456",
            format!("recorded App route registration `{}` is missing", entry.display()),
            "restore the recorded App route registration before rerunning the scaffold",
            mode,
        );
    }
    let constructor_end = app_constructor_end(function_source).unwrap_or_else(|reason| {
        fail(
            "E2456",
            format!(
                "selected App entry `{}` cannot register route discovery: {reason}",
                entry.display()
            ),
            "return one canonical `web.app()` builder from `fn run() App`",
            mode,
        )
    });
    let mut after = source.to_string();
    after.insert_str(start + constructor_end, ROUTE_REGISTRATION);
    Some(PlannedFile {
        relative,
        absolute: entry,
        bytes: after.into_bytes(),
        before: Some(before),
    })
}

fn app_registration_inverse(path: &Path, current: &[u8], before: &[u8]) -> bool {
    let source = match std::str::from_utf8(current) {
        Ok(source) => source,
        Err(_) => return false,
    };
    let entry_text = path.to_string_lossy().into_owned();
    let bundle = match jet::Loader::load_entry(&entry_text) {
        Ok(bundle) => bundle,
        Err(_) => return false,
    };
    let Some(module) = bundle.modules.get(bundle.entry) else {
        return false;
    };
    let Some(run) = jet::AST::app_entry_run_fn(&module.items) else {
        return false;
    };
    let (start, end) = (run.span.start, run.span.end);
    let Some(function_source) = source.get(start..end) else {
        return false;
    };
    let Ok(constructor_end) = app_constructor_end(function_source) else {
        return false;
    };
    let insert_at = start + constructor_end;
    let Some(registration_end) = insert_at.checked_add(ROUTE_REGISTRATION.len()) else {
        return false;
    };
    if source.as_bytes().get(insert_at..registration_end)
        != Some(ROUTE_REGISTRATION.as_bytes())
    {
        return false;
    }
    let mut restored = Vec::with_capacity(current.len() - ROUTE_REGISTRATION.len());
    restored.extend_from_slice(&current[..insert_at]);
    restored.extend_from_slice(&current[registration_end..]);
    restored == before
}

fn syntax_tokens(source: &str) -> Vec<(String, usize, usize)> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len()
                && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
            {
                index += 1;
            }
            index = (index + 2).min(bytes.len());
            continue;
        }
        if matches!(bytes[index], b'"' | b'\'') {
            let start = index;
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = (index + 2).min(bytes.len());
                } else if bytes[index] == quote {
                    index += 1;
                    break;
                } else {
                    index += 1;
                }
            }
            tokens.push((
                String::from_utf8_lossy(&bytes[start..index]).into_owned(),
                start,
                index,
            ));
            continue;
        }
        if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            tokens.push((
                String::from_utf8_lossy(&bytes[start..index]).into_owned(),
                start,
                index,
            ));
            continue;
        }
        let start = index;
        index += 1;
        tokens.push((
            String::from_utf8_lossy(&bytes[start..index]).into_owned(),
            start,
            index,
        ));
    }
    tokens
}

fn has_routes_registration(source: &str) -> bool {
    let tokens = syntax_tokens(source);
    let is_routes_literal = |index: usize| {
        matches!(
            tokens.get(index).map(|token| token.0.as_str()),
            Some("\"routes\"") | Some("'routes'")
        )
    };
    for index in 0..tokens.len() {
        if tokens[index].0 != "."
            || tokens.get(index + 1).map(|token| token.0.as_str()) != Some("routes")
            || tokens.get(index + 2).map(|token| token.0.as_str()) != Some("(")
        {
            continue;
        }
        if is_routes_literal(index + 3)
            || (tokens.get(index + 3).map(|token| token.0.as_str()) == Some("from")
                && tokens.get(index + 4).map(|token| token.0.as_str()) == Some(":")
                && is_routes_literal(index + 5))
        {
            return true;
        }
    }
    false
}

fn app_constructor_end(source: &str) -> Result<usize, &'static str> {
    let tokens = syntax_tokens(source);
    let mut ends = Vec::new();
    for index in 0..tokens.len() {
        if tokens[index].0 != "app"
            || tokens.get(index + 1).map(|token| token.0.as_str()) != Some("(")
            || tokens.get(index + 2).map(|token| token.0.as_str()) != Some(")")
        {
            continue;
        }
        let web_qualified = index >= 2
            && tokens[index - 1].0 == "."
            && tokens[index - 2].0 == "web"
            && (index < 3 || tokens[index - 3].0 != ".");
        let core_qualified = index >= 4
            && tokens[index - 1].0 == "."
            && tokens[index - 2].0 == "web"
            && tokens[index - 3].0 == "."
            && tokens[index - 4].0 == "core";
        let qualified = web_qualified || core_qualified;
        let bare = (index == 0 || tokens[index - 1].0 != ".")
            && tokens.get(index.wrapping_sub(1)).map(|token| token.0.as_str()) != Some("fn");
        if (qualified || bare) && !ends.contains(&tokens[index + 2].2) {
            ends.push(tokens[index + 2].2);
        }
    }
    match ends.as_slice() {
        [end] => Ok(*end),
        [] => Err("the checked run function has no canonical App builder"),
        _ => Err("the checked run function has multiple App builders"),
    }
}


fn migration_file(
    root: &Path,
    name: &str,
    up: &str,
    down: Option<&str>,
    options: &Options,
    mode: OutputMode,
) -> (String, Vec<u8>) {
    let (_, relative, text) = crate::CmdMigration::plan_new_migration(
        root,
        name,
        up,
        down,
        options.lock,
        options.risk.as_deref(),
        options.version,
    )
    .unwrap_or_else(|error| fail("E2456", error.message, error.fix, mode));
    (relative, text.into_bytes())
}

fn apply_files(
    kind: ScaffoldKind,
    name: &str,
    files: &[PlannedFile],
    receipt_path: &Path,
    receipt: &str,
    mode: OutputMode,
) {
    let root = std::env::current_dir().unwrap_or_else(|error| {
        fail(
            "E2105",
            format!("couldn't read project directory: {error}"),
            "run the scaffold from the project root",
            mode,
        )
    });
    let log_path = receipt_path.with_extension("log.json");
    let log = render_log(kind, name, files, receipt_path);
    let receipt_before = read_regular(receipt_path, mode);
    let receipt_matches = receipt_before.as_deref() == Some(receipt.as_bytes());
    let log_before = read_regular(&log_path, mode);
    if receipt_matches {
        if log_before.as_deref() != Some(log.as_bytes()) {
            fail(
                "E2456",
                format!("scaffold transaction log `{}` is missing or edited", log_path.display()),
                "restore the recorded log or remove the scaffold after review",
                mode,
            );
        }
    } else if log_before.is_some() {
        fail(
            "E2456",
            format!("scaffold transaction log `{}` already exists", log_path.display()),
            "choose a new scaffold name or remove the stale transaction after review",
            mode,
        );
    }
    let mut changes = Vec::new();
    let mut absent = Vec::new();
    for file in files {
        let current = read_regular(&file.absolute, mode);
        if let Some(expected_before) = file.before.as_deref() {
            let Some(current) = current.as_deref() else {
                fail(
                    "E2456",
                    format!("selected App entry `{}` is missing", file.relative),
                    "restore the checked App entry before applying the scaffold",
                    mode,
                );
            };
            if receipt_matches {
                if current == file.bytes.as_slice() {
                    continue;
                }
                fail(
                    "E2456",
                    format!("selected App entry `{}` drifted after scaffold apply", file.relative),
                    "restore the recorded App registration or remove the scaffold after review",
                    mode,
                );
            }
            if current != expected_before {
                fail(
                    "E2456",
                    format!("selected App entry `{}` changed before commit", file.relative),
                    "review the App edit, then rerun scaffolding from the unchanged entry",
                    mode,
                );
            }
            changes.push(Change {
                path: file.absolute.clone(),
                before: expected_before.to_vec(),
                after: file.bytes.clone(),
            });
            continue;
        }
        if let Some(current) = current.as_deref() {
            if receipt_matches && current == file.bytes.as_slice() {
                continue;
            }
            fail(
                "E2456",
                format!("scaffold destination `{}` already exists", file.relative),
                "choose a new name; scaffolds never overwrite source",
                mode,
            );
        }
        if receipt_matches {
            fail(
                "E2456",
                format!("recorded scaffold output `{}` is missing", file.relative),
                "restore the complete scaffold or remove its receipt after review",
                mode,
            );
        }
        changes.push(Change {
            path: file.absolute.clone(),
            before: Vec::new(),
            after: file.bytes.clone(),
        });
        absent.push(file.absolute.clone());
    }
    if receipt_matches {
        render_unchanged(kind, name, receipt_path, mode);
        return;
    }
    if receipt_before.is_some() {
        fail(
            "E2456",
            format!("scaffold receipt `{}` already exists", receipt_path.display()),
            "use `--remove` with the original scaffold name or choose another name",
            mode,
        );
    }
    ensure_parent_directories(
        &root,
        files.iter().map(|file| file.absolute.as_path()),
        mode,
    );
    changes.push(Change {
        path: receipt_path.to_path_buf(),
        before: Vec::new(),
        after: receipt.as_bytes().to_vec(),
    });
    absent.push(receipt_path.to_path_buf());
    let lock = Transaction::lock(&root);
    Transaction::recover(&lock);
    Transaction::commit_generate(&lock, &changes, &absent, &log_path, log.as_bytes());
    if mode.json {
        let fields = StatusFields::new()
            .with("kind", kind.as_str())
            .with("name", name)
            .with(
                "receipt",
                receipt_path.display().to_string(),
            );
        println!(
            "{}",
            StatusEnvelope::new("new.backend", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "scaffold: applied {} {}\nreceipt: {}\n",
                kind.as_str(),
                name,
                receipt_path.display()
            ),
        );
    }
}

fn run_remove(kind: ScaffoldKind, name: &str, mode: OutputMode) {
    let root = std::env::current_dir().unwrap_or_else(|error| {
        fail(
            "E2105",
            format!("couldn't read project directory: {error}"),
            "run the scaffold from the project root",
            mode,
        )
    });
    let receipt = receipt_path(&root, kind, name);
    let lock = Transaction::lock(&root);
    Transaction::recover(&lock);
    let bytes = read_regular(&receipt, mode).unwrap_or_else(|| {
        fail(
            "E2456",
            format!("scaffold receipt `{}` was not found", receipt.display()),
            "remove only a scaffold that was applied with this exact name",
            mode,
        )
    });
    let files = parse_receipt(&root, kind, name, &receipt, &bytes).unwrap_or_else(|| {
        fail(
            "E2456",
            format!("scaffold receipt `{}` is invalid", receipt.display()),
            "restore a valid scaffold receipt before removing outputs",
            mode,
        )
    });
    let log_path = receipt.with_extension("log.json");
    let log_files = files
        .iter()
        .map(|file| (file.relative.clone(), file.digest.clone()))
        .collect::<Vec<_>>();
    let expected_log = render_log_paths(kind, name, &log_files, &receipt);
    let log_before = read_regular(&log_path, mode).unwrap_or_else(|| {
        fail(
            "E2456",
            format!("scaffold transaction log `{}` is missing", log_path.display()),
            "restore the transaction log before removing outputs",
            mode,
        )
    });
    if log_before != expected_log.as_bytes() {
        fail(
            "E2456",
            format!("scaffold transaction log `{}` was edited", log_path.display()),
            "restore the recorded transaction log before removing outputs",
            mode,
        );
    }
    let mut seen = BTreeSet::new();
    let mut changes = Vec::with_capacity(files.len() + 2);
    for file in &files {
        if !seen.insert(file.relative.clone()) {
            fail(
                "E2456",
                format!(
                    "scaffold receipt `{}` repeats `{}`",
                    receipt.display(),
                    file.relative
                ),
                "restore the original receipt before removing outputs",
                mode,
            );
        }
        let path = root.join(&file.relative);
        validate_destination(&root, &path, mode);
        let current = read_regular(&path, mode).unwrap_or_else(|| {
            fail(
                "E2456",
                format!("recorded scaffold output `{}` is missing", file.relative),
                "restore the recorded output or remove the scaffold receipt after review",
                mode,
            )
        });
        if jet::SHA256::sha256_hex(&current) != file.digest {
            fail(
                "E2456",
                format!("recorded scaffold output `{}` was edited", file.relative),
                "revert or preserve the edit, then rerun `jet new` with an explicit removal decision",
                mode,
            );
        }
        if let Some(before) = file.before.as_deref() {
            if !app_registration_inverse(&path, &current, before) {
                fail(
                    "E2456",
                    format!(
                        "recorded App edit `{}` is not the exact route-registration inverse",
                        file.relative
                    ),
                    "restore the original App source before removing the scaffold",
                    mode,
                );
            }
        }
        changes.push(Change {
            path,
            before: current,
            after: file.before.clone().unwrap_or_default(),
        });
    }
    changes.push(Change {
        path: receipt.clone(),
        before: bytes,
        after: Vec::new(),
    });
    changes.push(Change {
        path: log_path,
        before: log_before,
        after: Vec::new(),
    });
    Transaction::commit_delete(&lock, &changes);
    if mode.json {
        let fields = StatusFields::new()
            .with("kind", kind.as_str())
            .with("name", name)
            .with("removed", files.len());
        println!(
            "{}",
            StatusEnvelope::new("new.backend.remove", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!("removed only recorded outputs for {} {}\n", kind.as_str(), name),
        );
    }
}

fn render_preview(
    kind: ScaffoldKind,
    name: &str,
    plan: &ScaffoldPlan,
    receipt: &Path,
    mode: OutputMode,
) {
    if mode.json {
        let outputs = StatusValue::array(plan.files.iter().map(|file| {
            StatusValue::object(
                StatusFields::new()
                    .with("path", file.relative.as_str())
                    .with("sha256", jet::SHA256::sha256_hex(&file.bytes))
                    .with("state", planned_state(file)),
            )
        }));
        let fields = StatusFields::new()
            .with("kind", kind.as_str())
            .with("name", name)
            .with("outputs", outputs)
            .with("receipt", receipt.display().to_string());
        println!(
            "{}",
            StatusEnvelope::new("new.backend.preview", true)
                .with_fields(fields)
                .json()
        );
        return;
    }
    crate::OutputAdapter::write_mode_status(
        mode,
        &format!(
            "scaffold: {} {}\npreview: planned source writes (nothing applied)\n",
            kind.as_str(),
            name
        ),
    );
    if let (Some(route), Some(model)) = (plan.route.as_deref(), plan.model.as_deref()) {
        let migration = plan.migration.as_deref().unwrap_or("next");
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!(
                "route {route}  model {model}\njob {name}  migration {migration}\n"
            ),
        );
    }
    for file in &plan.files {
        crate::OutputAdapter::write_mode_status(
            mode,
            &format!("  {} {}\n", planned_state(file), file.relative),
        );
    }
    crate::OutputAdapter::write_mode_status(
        mode,
        &format!(
            "receipt: {}\napply: jet new {} {} --apply\n",
            receipt.display(),
            kind.as_str(),
            name
        ),
    );
}

fn planned_state(file: &PlannedFile) -> &'static str {
    if file.before.is_none() {
        return destination_state(&file.absolute);
    }
    match fs::symlink_metadata(&file.absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => "conflict(link)",
        Ok(metadata) if metadata.is_dir() => "conflict(directory)",
        Ok(metadata) if metadata.is_file() => match fs::read(&file.absolute) {
            Ok(current) if current == file.bytes => "unchanged",
            Ok(current) if file.before.as_deref() == Some(current.as_slice()) => "edit",
            Ok(_) => "conflict",
            Err(_) => "conflict(unreadable)",
        },
        Ok(_) => "conflict(special)",
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "conflict(missing)",
        Err(_) => "conflict(unreadable)",
    }
}

fn destination_state(path: &Path) -> &'static str {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => "conflict(link)",
        Ok(metadata) if metadata.is_dir() => "conflict(directory)",
        Ok(metadata) if metadata.is_file() => "conflict",
        Ok(_) => "conflict(special)",
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "create",
        Err(_) => "conflict(unreadable)",
    }
}

fn render_unchanged(kind: ScaffoldKind, name: &str, receipt: &Path, mode: OutputMode) {
    if mode.json {
        let fields = StatusFields::new()
            .with("kind", kind.as_str())
            .with("name", name)
            .with("status", "unchanged")
            .with("receipt", receipt.display().to_string());
        println!(
            "{}",
            StatusEnvelope::new("new.backend", true)
                .with_fields(fields)
                .json()
        );
    } else {
        crate::OutputAdapter::write_mode_status(mode, &format!("scaffold: {} {} unchanged\nreceipt: {}\n", kind.as_str(), name, receipt.display()));
    }
}

fn render_receipt(
    root: &Path,
    kind: ScaffoldKind,
    name: &str,
    options: &Options,
    files: &[PlannedFile],
    receipt: &Path,
) -> String {
    let files = files
        .iter()
        .map(|file| {
            (
                file.relative.clone(),
                jet::SHA256::sha256_hex(&file.bytes),
                file.before.clone(),
            )
        })
        .collect::<Vec<_>>();
    let authority = receipt_authority(root, receipt);
    render_receipt_text(kind, name, authority.as_str(), options, &files)
}

fn receipt_authority(root: &Path, receipt: &Path) -> String {
    receipt
        .strip_prefix(root)
        .unwrap_or(receipt)
        .to_string_lossy()
        .replace('\\', "/")
}

fn render_receipt_body(
    kind: ScaffoldKind,
    name: &str,
    authority: &str,
    options: &Options,
    files: &[(String, String, Option<Vec<u8>>)],
) -> String {
    let outputs = files
        .iter()
        .map(|(path, digest, before)| {
            let before = before
                .as_deref()
                .map(hex_bytes)
                .map(|value| format!(",\"before\":\"{value}\""))
                .unwrap_or_default();
            format!(
                "{{\"path\":\"{}\",\"digest\":\"{}\"{before}}}",
                json_quote(path),
                digest
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let inputs = render_receipt_inputs(options);
    format!(
        "{{\"schema\":\"{RECEIPT_SCHEMA}\",\"status\":\"applied\",\"kind\":\"{}\",\"name\":\"{}\",\"slug\":\"{}\",\"toolchain\":\"jet/{}\",\"authority\":\"{}\",\"inputs\":{inputs},\"files\":[{}]}}",
        kind.as_str(),
        json_quote(name),
        json_quote(&slugify(name)),
        env!("CARGO_PKG_VERSION"),
        json_quote(authority),
        outputs
    )
}

fn render_receipt_text(
    kind: ScaffoldKind,
    name: &str,
    authority: &str,
    options: &Options,
    files: &[(String, String, Option<Vec<u8>>)],
) -> String {
    let body = render_receipt_body(kind, name, authority, options, files);
    let body_digest = jet::SHA256::sha256_hex(body.as_bytes());
    let identity = jet::SHA256::sha256_hex(
        format!(
            "{}\0{}\0{}\0{}",
            kind.as_str(),
            name,
            authority,
            body_digest
        )
        .as_bytes(),
    );
    format!(
        "{},\"body_sha256\":\"{}\",\"identity\":\"{}\"}}\n",
        body.trim_end_matches('}'),
        body_digest,
        identity
    )
}
fn render_receipt_inputs(options: &Options) -> String {
    let optional_text = |value: Option<&str>| {
        value
            .map(|value| format!("\"{}\"", json_quote(value)))
            .unwrap_or_else(|| "null".to_string())
    };
    let lock = options
        .lock
        .map(MigrationLock::as_str)
        .map(|value| format!("\"{value}\""))
        .unwrap_or_else(|| "null".to_string());
    let version = options
        .version
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"path\":{},\"model\":{},\"up\":{},\"down\":{},\"risk\":{},\"lock\":{lock},\"version\":{version}}}",
        optional_text(options.path.as_deref()),
        optional_text(options.model.as_deref()),
        optional_text(options.up.as_deref()),
        optional_text(options.down.as_deref()),
        optional_text(options.risk.as_deref()),
    )
}
fn receipt_input_text(
    fields: &[(String, DataTree)],
    key: &str,
) -> Option<Option<String>> {
    match json_value(fields, key)? {
        DataTree::Null => Some(None),
        DataTree::Text(value) => Some(Some(value.clone())),
        _ => None,
    }
}

fn parse_receipt_inputs(value: &DataTree) -> Option<Options> {
    let DataTree::Object(fields) = value else {
        return None;
    };
    const KEYS: &[&str] = &["path", "model", "up", "down", "risk", "lock", "version"];
    let mut seen_keys = BTreeSet::new();
    if fields
        .iter()
        .any(|(key, _)| !KEYS.iter().any(|expected| *expected == key) || !seen_keys.insert(key))
        || seen_keys.len() != KEYS.len()
    {
        return None;
    }
    let path = receipt_input_text(fields, "path")?;
    let model = receipt_input_text(fields, "model")?;
    let up = receipt_input_text(fields, "up")?;
    let down = receipt_input_text(fields, "down")?;
    let risk = receipt_input_text(fields, "risk")?;
    let lock = match receipt_input_text(fields, "lock")? {
        Some(value) => Some(MigrationLock::parse(&value).ok()?),
        None => None,
    };
    let version = match json_value(fields, "version")? {
        DataTree::Null => None,
        DataTree::Int(value) if *value > 0 => Some(*value as u64),
        DataTree::Number(value) => {
            let value = value.parse::<u64>().ok()?;
            (value > 0).then_some(value)
        }
        _ => return None,
    };
    Some(Options {
        operation: Operation::Apply,
        path,
        model,
        up,
        down,
        risk,
        lock,
        version,
    })
}


fn render_log(
    kind: ScaffoldKind,
    name: &str,
    files: &[PlannedFile],
    receipt: &Path,
) -> String {
    let files = files
        .iter()
        .map(|file| {
            (
                file.relative.clone(),
                jet::SHA256::sha256_hex(&file.bytes),
            )
        })
        .collect::<Vec<_>>();
    render_log_paths(kind, name, &files, receipt)
}

fn render_log_paths(
    kind: ScaffoldKind,
    name: &str,
    files: &[(String, String)],
    receipt: &Path,
) -> String {
    let paths = files
        .iter()
        .map(|(path, _)| format!("\"{}\"", json_quote(path)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":\"jet-scaffold-transaction-v1\",\"kind\":\"{}\",\"name\":\"{}\",\"receipt\":\"{}\",\"files\":[{}]}}\n",
        kind.as_str(),
        json_quote(name),
        json_quote(&receipt.display().to_string()),
        paths
    )
}
struct ReceiptFile {
    relative: String,
    digest: String,
    before: Option<Vec<u8>>,
}

fn parse_receipt(
    root: &Path,
    kind: ScaffoldKind,
    name: &str,
    receipt: &Path,
    bytes: &[u8],
) -> Option<Vec<ReceiptFile>> {
    if bytes.len() > MAX_RECEIPT_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let DataTree::Object(fields) = parse_json(text, true).ok()? else {
        return None;
    };
    const KEYS: &[&str] = &[
        "schema",
        "status",
        "kind",
        "name",
        "slug",
        "toolchain",
        "authority",
        "inputs",
        "files",
        "body_sha256",
        "identity",
    ];
    let mut seen_keys = BTreeSet::new();
    if fields
        .iter()
        .any(|(key, _)| !KEYS.iter().any(|expected| *expected == key) || !seen_keys.insert(key))
        || seen_keys.len() != KEYS.len()
    {
        return None;
    }
    let expected_slug = slugify(name);
    let expected_toolchain = format!("jet/{}", env!("CARGO_PKG_VERSION"));
    let expected_authority = receipt_authority(root, receipt);
    if json_text(&fields, "schema")? != RECEIPT_SCHEMA
        || json_text(&fields, "status")? != "applied"
        || json_text(&fields, "kind")? != kind.as_str()
        || json_text(&fields, "name")? != name
        || json_text(&fields, "slug")? != expected_slug.as_str()
        || json_text(&fields, "toolchain")? != expected_toolchain.as_str()
        || json_text(&fields, "authority")? != expected_authority.as_str()
    {
        return None;
    }
    let receipt_options = parse_receipt_inputs(json_value(&fields, "inputs")?)?;
    let app_relative = if matches!(kind, ScaffoldKind::Service | ScaffoldKind::Route) {
        let entry = crate::find_project_entry(root);
        Some(
            entry
                .strip_prefix(root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/"),
        )
    } else {
        None
    };
    let DataTree::Array(entries) = json_value(&fields, "files")? else {
        return None;
    };
    if entries.is_empty() || entries.len() > 1024 {
        return None;
    }
    let mut files = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();
    for entry in entries {
        let DataTree::Object(entry_fields) = entry else {
            return None;
        };
        if (entry_fields.len() != 2 && entry_fields.len() != 3)
            || entry_fields
                .iter()
                .any(|(key, _)| key != "path" && key != "digest" && key != "before")
        {
            return None;
        }
        let path = json_text(entry_fields, "path")?.to_string();
        let digest = json_text(entry_fields, "digest")?.to_string();
        let before = match json_value(entry_fields, "before") {
            None => None,
            Some(DataTree::Text(value)) => Some(decode_hex(value)?),
            Some(_) => return None,
        };
        let is_app = app_relative.as_deref() == Some(path.as_str());
        let valid_path = if is_app {
            before.is_some()
                && Path::new(&path).extension().and_then(|extension| extension.to_str())
                    == Some("jet")
        } else {
            before.is_none()
                && valid_receipt_relative(&path)
                && valid_receipt_file(kind, name, &path, &receipt_options)
        };
        if !valid_path || !valid_digest(&digest) || !seen.insert(path.clone()) {
            return None;
        }
        files.push(ReceiptFile {
            relative: path,
            digest,
            before,
        });
    }
    let authority = json_text(&fields, "authority")?;
    let receipt_files = files
        .iter()
        .map(|file| (file.relative.clone(), file.digest.clone(), file.before.clone()))
        .collect::<Vec<_>>();
    let body = render_receipt_body(kind, name, authority, &receipt_options, &receipt_files);
    let body_digest = json_text(&fields, "body_sha256")?;
    if body_digest != jet::SHA256::sha256_hex(body.as_bytes()) {
        return None;
    }
    let identity = jet::SHA256::sha256_hex(
        format!(
            "{}\0{}\0{}\0{}",
            kind.as_str(),
            name,
            authority,
            body_digest
        )
        .as_bytes(),
    );
    if json_text(&fields, "identity")? != identity
        || text
            != render_receipt_text(kind, name, authority, &receipt_options, &receipt_files)
    {
        return None;
    }
    Some(files)
}


fn json_value<'a>(
    fields: &'a [(String, DataTree)],
    key: &str,
) -> Option<&'a DataTree> {
    fields
        .iter()
        .find_map(|(field, value)| (field == key).then_some(value))
}

fn json_text<'a>(fields: &'a [(String, DataTree)], key: &str) -> Option<&'a str> {
    match json_value(fields, key)? {
        DataTree::Text(value) => Some(value),
        _ => None,
    }
}

fn valid_receipt_relative(path: &str) -> bool {
    let relative = Path::new(path);
    path != ""
        && !path.contains('\\')
        && !relative.is_absolute()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && relative.components().count() >= 2
        && relative
            .components()
            .next()
            .is_some_and(|component| {
                matches!(
                    component,
                    Component::Normal(name)
                        if matches!(
                            name.to_str(),
                            Some("services")
                                | Some("routes")
                                | Some("models")
                                | Some("jobs")
                                | Some("migrations")
                                | Some("tests")
                        )
                )
            })
        && relative.extension().and_then(|extension| extension.to_str()) == Some("jet")
}
fn valid_receipt_file(
    kind: ScaffoldKind,
    name: &str,
    path: &str,
    options: &Options,
) -> bool {
    let slug = slugify(name);
    let expected = |directory: &str| path == format!("{directory}/{slug}.jet");
    let expected_job = path == format!("jobs/job_{slug}.jet");
    let model = options
        .model
        .as_deref()
        .map(model_name)
        .unwrap_or_else(|| model_name(name));
    let expected_model = path == format!("models/model_{}.jet", slugify(&model));
    let route_output = path.starts_with("routes/") && path.ends_with(".jet");
    match kind {
        ScaffoldKind::Route => route_output,
        ScaffoldKind::Job => expected_job,
        ScaffoldKind::Migration => migration_receipt_file(path, &slug),
        ScaffoldKind::Service => {
            expected("services")
                || route_output
                || expected_job
                || expected("tests")
                || expected_model
                || migration_receipt_file(path, &slug)
        }
    }
}


fn migration_receipt_file(path: &str, slug: &str) -> bool {
    let relative = Path::new(path);
    relative.parent().and_then(Path::to_str) == Some("migrations")
        && relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.split_once('_'))
            .is_some_and(|(version, migration_slug)| {
                version != "0"
                    && !version.is_empty()
                    && version.chars().all(|character| character.is_ascii_digit())
                    && migration_slug == slug
            })
}
fn reuse_receipt_migration_version(
    options: &Options,
    existing_files: Option<&[ReceiptFile]>,
    name: &str,
) -> Options {
    let mut effective = options.clone();
    if effective.version.is_none() {
        let slug = slugify(name);
        effective.version = existing_files.and_then(|files| receipt_migration_version(files, &slug));
    }
    effective
}

fn receipt_migration_version(files: &[ReceiptFile], slug: &str) -> Option<u64> {
    let mut version = None;
    for file in files {
        if !migration_receipt_file(&file.relative, slug) {
            continue;
        }
        let parsed = Path::new(&file.relative)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.split_once('_'))
            .and_then(|(version, _)| version.parse::<u64>().ok())?;
        if version.replace(parsed).is_some() {
            return None;
        }
    }
    version
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 0x0f) as usize] as char);
    }
    text
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    fn nibble(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        decoded.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(decoded)
}


fn read_regular(path: &Path, mode: OutputMode) -> Option<Vec<u8>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            fail("E2456", format!("scaffold path `{}` is not a regular file", path.display()), "move the link or special file aside before scaffolding", mode)
        }
        Ok(_) => Some(fs::read(path).unwrap_or_else(|error| fail("E2105", format!("couldn't read scaffold path `{}`: {error}", path.display()), "check project read authority and retry", mode))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => fail("E2105", format!("couldn't inspect scaffold path `{}`: {error}", path.display()), "check project permissions and retry", mode),
    }
}

fn validate_destination(root: &Path, path: &Path, mode: OutputMode) {
    let relative = path.strip_prefix(root).unwrap_or_else(|_| fail("E2456", format!("scaffold destination `{}` escapes the project", path.display()), "use a project-relative backend scaffold", mode));
    if relative.components().any(|component| !matches!(component, Component::Normal(_))) || relative.starts_with(".git") || relative.starts_with("target") || relative.starts_with(".jet") {
        fail("E2456", format!("scaffold destination `{}` is not a source path", relative.display()), "keep generated backend source outside compiler state", mode);
    }
    let mut parent = root.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        parent.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&parent) {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                fail("E2456", format!("scaffold parent `{}` is not a real directory", parent.display()), "replace the link or special file with a normal source directory", mode);
            }
        }
    }
}

fn valid_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(character) if character == '_' || character.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn validate_name(name: &str, kind: ScaffoldKind, mode: OutputMode) {
    let slug = slugify(name);
    if name.is_empty()
        || slug.is_empty()
        || name.len() > 96
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
        })
        || !valid_identifier(&slug)
    {
        fail(
            "E2104",
            format!("{} name `{name}` is not safe", kind.as_str()),
            "use ASCII letters, digits, `_`, or `-` and begin with a letter or underscore",
            mode,
        );
    }
}

fn ensure_parent_directories<'a>(
    root: &Path,
    paths: impl Iterator<Item = &'a Path>,
    mode: OutputMode,
) {
    for path in paths {
        let relative = path.strip_prefix(root).unwrap_or_else(|_| fail("E2456", format!("scaffold path `{}` escapes the project", path.display()), "use a project-relative backend scaffold", mode));
        let mut current = root.to_path_buf();
        for component in relative.parent().into_iter().flat_map(Path::components) {
            let Component::Normal(name) = component else {
                fail("E2456", format!("scaffold parent `{}` is not a normal directory", current.display()), "use a project-relative backend scaffold", mode);
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    fail("E2456", format!("scaffold parent `{}` is not a real directory", current.display()), "replace the link or special file with a normal source directory", mode);
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).unwrap_or_else(|error| fail("E2105", format!("couldn't create scaffold directory `{}`: {error}", current.display()), "check project write authority and retry", mode));
                }
                Err(error) => fail("E2105", format!("couldn't inspect scaffold directory `{}`: {error}", current.display()), "check project permissions and retry", mode),
            }
        }
    }
}
fn validate_route(route: &str, mode: OutputMode) {
    if route.chars().any(char::is_control) {
        fail(
            "E2104",
            format!("route path `{route}` is not safe"),
            "use a single project-relative web path such as `/orders`",
            mode,
        );
    }
    if let Err(reason) = jet::Syntax::validate_http_route_pattern(route) {
        fail(
            "E2104",
            format!("route path `{route}` is not safe: {reason}"),
            "use a canonical route such as `/orders`, `/orders/:id`, or `/assets/*path`",
            mode,
        );
    }
}
fn route_file_relative(route: &str, mode: OutputMode) -> String {
    if route == "/" {
        return "routes/index.jet".to_string();
    }
    let segments = route.split('/').skip(1).collect::<Vec<_>>();
    let mut file_segments = Vec::with_capacity(segments.len() + 1);
    let mut append_index = false;
    for (index, segment) in segments.iter().enumerate() {
        let file_segment = if let Some(name) = segment.strip_prefix(':') {
            format!("[{name}]")
        } else if segment.starts_with('*') {
            fail(
                "E2456",
                format!("route `{route}` cannot map to the file-routing scanner"),
                "use a named `:param` route or add catch-all support to the canonical scanner first",
                mode,
            )
        } else {
            let final_segment = index + 1 == segments.len();
            if segment.starts_with('[') && segment.ends_with(']')
                || segment.contains('\\')
                || segment.contains(':')
            {
                fail(
                    "E2456",
                    format!("route `{route}` cannot map to the file-routing scanner"),
                    "use route segments representable by `routes/**/*.jet` convention paths",
                    mode,
                )
            }
            if final_segment
                && (segment.starts_with('_') || *segment == "index" || *segment == "page")
            {
                append_index = true;
            }
            (*segment).to_string()
        };
        file_segments.push(file_segment);
    }
    if append_index {
        file_segments.push("index".to_string());
    }
    format!("routes/{}.jet", file_segments.join("/"))
}


fn set_once(slot: &mut Option<String>, value: String, flag: &str, mode: OutputMode) {
    if slot.replace(value).is_some() {
        fail("E2104", format!("`{flag}` was supplied twice"), "provide one value per scaffold option", mode);
    }
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous = None;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if character.is_ascii_uppercase() && previous.is_some_and(|previous: char| previous.is_ascii_lowercase()) && !slug.ends_with('_') {
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

fn service_source(name: &str, slug: &str) -> String {
    format!(
        r#"use core.service as service
use job_{slug} as job

fn {slug}_worker() {{
    job.enqueue("{name} job")
}}

pub fn run() {{
    tree := service.tree("{slug}")
    tree.set_delivery(service.delivery_durable()) ?? panic("delivery")
    endpoint :: tree.worker("{slug}", {slug}_worker, capacity: 1) ?? panic("worker")
    tree.start() ?? panic("start")
    print(tree.show())
    tree.stop() ?? panic("stop")
}}
"#
    )
}

fn route_source(name: &str, route: &str) -> String {
    format!(
        "use core.web as web\n\nfn page() WebPage -> web.page(\"{name}\", \"<main><h1>{name}</h1></main>\")\n\n// route: {route}\n"
    )
}


fn job_source(name: &str, slug: &str) -> String {
    let payload = format!("{}JobPayload", model_name(name));
    format!(
        r#"use core.jobs as jobs

#Codable
pub struct {payload} {{
    message: String
}}

#Job
pub fn {slug}_job(payload: {payload}) {{
    print(payload.message)
}}

pub fn enqueue(message: String) {{
    queue := jobs.queue() ?? panic("queue")
    payload :: {payload}{{message: message}}
    receipt :: queue.enqueue({slug}_job, payload) ?? panic("enqueue")
    print(receipt)
}}
"#
    )
}
fn model_name(name: &str) -> String {
    let mut model = String::new();
    let mut upper = true;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if upper {
                model.push(character.to_ascii_uppercase());
                upper = false;
            } else {
                model.push(character);
            }
        } else {
            upper = true;
        }
    }
    if model.ends_with("ies") && model.len() > 3 {
        model.truncate(model.len() - 3);
        model.push('y');
    } else if model.ends_with('s') && !model.ends_with("ss") && model.len() > 1 {
        model.pop();
    }
    model
}

fn model_source(model: &str) -> String {
    format!("#Codable\npub struct {model} {{\n    pub id: Int\n}}\n")
}

fn test_source(name: &str, model: &str) -> String {
    let model_module = format!("model_{}", slugify(model));
    format!("use {model_module} as model\n\n#Test(\"{name} model\") {{\n    value := model.{model}{{id: 1}}\n    assert_eq(value.id, 1)\n}}\n")
}

fn receipt_path(root: &Path, kind: ScaffoldKind, name: &str) -> PathBuf {
    root.join(".jet").join("codemods").join(format!("scaffold-{}-{}.receipt.json", kind.as_str(), slugify(name)))
}

fn fail(code: &'static str, message: impl Into<String>, fix: impl Into<String>, mode: OutputMode) -> ! {
    crate::emit_cli_report(code, message.into(), "backend scaffolds use one deterministic source transaction".to_string(), fix.into(), mode.json);
    exit(ExitCodes::USER_ERROR);
}
