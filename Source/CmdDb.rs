//! `jet db` — a bounded local typed-table SQL console.
//!
//! The command is a host adapter only. It reads an explicitly authorized local
//! source, derives one temporary Codable row type, and sends the generated
//! decode/query expression through `jet::REPL::Sql`. SQL parsing and row
//! selection remain in the shared Prelude kernel included by both sides.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Instant;

use jet::AST::{CtKey, CtReport, CtValue};
use jet::Diagnostics::{json_str, ColorChoice};
use jet::ExitCodes;
use jet::REPL::Sql::{SqlEval, SqlSession};
use jet_foundation::DataTree::DataTree as Value;
use jet_foundation::EncodingJson::parse_json;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use crate::OutputAdapter::{
    write_mode_diagnostic, write_mode_machine, write_mode_renderable, write_mode_status,
};
use crate::OutputAdapter::OutputMode;
use jet_foundation::CsvKernel::{CsvOptions, CsvParser};


mod sql_kernel {
    include!("../crates/jet-codegen/src/Prelude/Core/SqlQuery.rs");
}

const DEFAULT_MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
const DEFAULT_MAX_ROWS: usize = 100_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const ROW_TYPE_NAME: &str = "DbConsoleRow";

const DOSSIER_DATA_ROWS: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "schema",
        "jet db .schema",
        "typed columns, nullability, and source row bound",
        "source-scoped",
        "derived from the loaded source",
        "available only in a live local query session",
    ),
    (
        "table",
        "jet db .tables",
        "source identity and typed row carrier",
        "source-scoped",
        "loaded source identity",
        "available only in a live local query session",
    ),
    (
        "timing",
        "jet db .stats",
        "elapsed milliseconds for the latest query",
        "query-scoped",
        "not measured by inspect",
        "available after a query in a live local query session",
    ),
    (
        "plan",
        "jet db .plan",
        "shared SQL parser plan: scan, filter, group, project, sort, limit",
        "query-scoped",
        "not measured by inspect",
        "available only in a live local query session",
    ),
    (
        "output",
        "jet db --json|--jsonl|--csv",
        "deterministic output mode and structured receipt",
        "invocation-scoped",
        "selected by the invocation",
        "available when the console invocation runs",
    ),
    (
        "result-size",
        "jet db --max-output-bytes",
        "final rendered result bytes and explicit output bound",
        "query-scoped",
        "not measured by inspect",
        "available after a query in a live local query session",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum DbOutput {
    Human,
    Json,
    Jsonl,
    Csv,
}

struct DbOptions {
    source_path: Option<PathBuf>,
    source_name: Option<String>,
    query: Option<String>,
    script: Option<PathBuf>,
    output: DbOutput,
    machine_override: Option<bool>,
    color_override: Option<ColorChoice>,
    max_input_bytes: usize,
    max_rows: usize,
    max_output_bytes: usize,
    allow_rights: Vec<String>,
    deny_rights: Vec<String>,
    allow_fs: bool,
    deny_fs: bool,
    allow_write: bool,
    deny_write: bool,
    allow_db: bool,
    deny_db: bool,
    browser: bool,
    help: bool,
}

struct DbError {
    code: String,
    operation: String,
    source: String,
    message: String,
    hint: String,
}

impl DbError {
    fn new(
        code: impl Into<String>,
        operation: impl Into<String>,
        source: impl Into<String>,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            operation: operation.into(),
            source: source.into(),
            message: message.into(),
            hint: hint.into(),
        }
    }

    fn diagnostics(operation: &str, source: &str, diagnostics: &[jet::Diagnostics::Diagnostic]) -> Self {
        let Some(diagnostic) = diagnostics.first() else {
            return Self::new(
                "E2600",
                operation,
                source,
                "the SQL evaluator returned no diagnostic details",
                "retry the statement after correcting its source or SQL",
            );
        };
        Self::new(
            diagnostic.code.clone(),
            operation,
            source,
            diagnostic.what.clone(),
            diagnostic.fix.clone(),
        )
    }
}

struct ColumnAccum {
    kind: Option<ColumnKind>,
    seen: usize,
    nullable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnKind {
    Int,
    Float,
    Bool,
    String,
}

impl ColumnKind {
    fn jet_name(self) -> &'static str {
        match self {
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Bool => "Bool",
            Self::String => "String",
        }
    }
}

struct Column {
    name: String,
    kind: ColumnKind,
    nullable: bool,
}

struct LoadedSource {
    name: String,
    path: String,
    json: String,
    columns: Vec<Column>,
    rows: usize,
    input_bytes: usize,
}

struct DbState {
    last_sql: Option<String>,
    last_plan: Option<String>,
    last_result: Option<String>,
    last_rows: usize,
    last_elapsed_ms: u128,
    last_output_bytes: usize,
}

impl DbState {
    fn new() -> Self {
        Self {
            last_sql: None,
            last_plan: None,
            last_result: None,
            last_rows: 0,
            last_elapsed_ms: 0,
            last_output_bytes: 0,
        }
    }
}

fn db_status(operation: &str, ok: bool, fields: StatusFields) -> StatusEnvelope {
    StatusEnvelope::new("db", ok)
        .with_field("operation", operation)
        .with_fields(fields)
}

fn write_db_status(mode: OutputMode, operation: &str, ok: bool, fields: StatusFields) {
    let status = db_status(operation, ok, fields).json_line();
    write_mode_machine(mode, &status);
}

pub(crate) fn run_db(args: &[String], mode: OutputMode) -> i32 {
    let mut migration_start = usize::from(args.first().is_some_and(|argument| argument == "db"));
    while let Some(argument) = args.get(migration_start).map(String::as_str) {
        let global = matches!(
            argument,
            "--json"
                | "--jsonl"
                | "--csv"
                | "--machine"
                | "--human"
                | "--no-color"
                | "--force-color"
                | "--color"
                | "--format"
                | "--quiet"
                | "--allow"
                | "--deny"
                | "--browser"
        ) || argument.starts_with("--format=")
            || argument.starts_with("--color=")
            || argument.starts_with("--allow=")
            || argument.starts_with("--deny=");
        if !global {
            break;
        }
        migration_start += 1;
        if matches!(argument, "--format" | "--allow" | "--deny") {
            migration_start += 1;
        }
    }
    if args
        .get(migration_start)
        .is_some_and(|argument| argument == "migrate")
    {
        return crate::CmdMigration::run_migration(&args[migration_start..], mode);
    }
    let options = match parse_options(args) {
        Ok(options) => options,
        Err(error) => {
            emit_error(mode, DbOutput::Human, &error);
            return ExitCodes::USAGE;
        }
    };
    let output_mode = resolved_mode(mode, &options);
    if options.help {
        if output_mode.json {
            write_db_status(
                output_mode,
                "help",
                true,
                StatusFields::new().with("usage", usage()),
            );
        } else {
            write_mode_renderable(output_mode, usage());
        }
        return 0;
    }
    if options.deny_db && options.allow_db {
        let error = DbError::new(
            "E1803",
            "authority",
            "",
            "DB.Read authority was both granted and denied",
            "remove the overlapping DB right from `--allow` or `--deny`",
        );
        emit_error(output_mode, options.output, &error);
        return ExitCodes::USER_ERROR;
    }

    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let source = match options.source_path.as_ref() {
        Some(path) => match load_source(
            path,
            options.source_name.as_deref(),
            &current_dir,
            options.allow_fs,
            options.deny_fs,
            options.allow_db,
            options.deny_db,
            options.max_input_bytes,
            options.max_rows,
        ) {
            Ok(source) => Some(source),
            Err(error) => {
                emit_error(output_mode, options.output, &error);
                return ExitCodes::USER_ERROR;
            }
        },
        None => None,
    };
    if options.browser && source.is_none() {
        let error = DbError::new(
            "E2600",
            "browser",
            "",
            "the browser projection needs a loaded local source",
            "pass a local JSON, JSONL, or CSV source with `--allow=FS.Read`",
        );
        emit_error(output_mode, options.output, &error);
        return ExitCodes::USER_ERROR;
    }

    if options.query.is_some() && options.script.is_some() {
        let error = DbError::new(
            "E2104",
            "arguments",
            "",
            "`--query` and `--script` cannot be used together",
            "choose one SQL input source",
        );
        emit_error(output_mode, options.output, &error);
        return ExitCodes::USAGE;
    }

    let script = match options.script.as_ref() {
        Some(path) => match read_script(
            path,
            &current_dir,
            options.allow_fs,
            options.deny_fs,
            options.allow_db,
            options.deny_db,
            options.max_input_bytes,
        ) {
            Ok(script) => Some(script),
            Err(error) => {
                emit_error(output_mode, options.output, &error);
                return ExitCodes::USER_ERROR;
            }
        },
        None => None,
    };

    let mut session = SqlSession::new(
        &current_dir,
        jet::REPL::ReplFlags::new(&options.allow_rights, &options.deny_rights)
            .with_color(output_mode.color),
    );
    if let Some(source) = source.as_ref() {
        if let Err(error) = prepare_session(&mut session, source) {
            emit_error(output_mode, options.output, &error);
            return ExitCodes::USER_ERROR;
        }
    }

    let mut state = DbState::new();
    if let Some(query) = options.query.as_deref() {
        let inputs = vec![query.to_string()];
        let status = process_inputs(
            &inputs,
            false,
            source.as_ref(),
            &mut session,
            &mut state,
            &options,
            output_mode,
        );
        return finish_browser(status, options.browser, source.as_ref(), &session, &state, output_mode);
    }
    if let Some(script) = script.as_deref() {
        let inputs = match split_console_inputs(script) {
            Ok(inputs) => inputs,
            Err(message) => {
                let error = DbError::new("E2702", "script", "", message, "close the SQL quote or parenthesized expression");
                emit_error(output_mode, options.output, &error);
                return ExitCodes::USER_ERROR;
            }
        };
        let status = process_inputs(
            &inputs,
            false,
            source.as_ref(),
            &mut session,
            &mut state,
            &options,
            output_mode,
        );
        return finish_browser(status, options.browser, source.as_ref(), &session, &state, output_mode);
    }

    let stdin_is_terminal = io::stdin().is_terminal();
    if !stdin_is_terminal {
        let input = match read_stdin_bounded(options.max_input_bytes) {
            Ok(input) => input,
            Err(error) => {
                emit_error(output_mode, options.output, &error);
                return ExitCodes::USER_ERROR;
            }
        };
        let inputs = match split_console_inputs(&input) {
            Ok(inputs) => inputs,
            Err(message) => {
                let error = DbError::new("E2702", "stdin", "stdin", message, "close the SQL quote or parenthesized expression");
                emit_error(output_mode, options.output, &error);
                return ExitCodes::USER_ERROR;
            }
        };
        let status = process_inputs(
            &inputs,
            false,
            source.as_ref(),
            &mut session,
            &mut state,
            &options,
            output_mode,
        );
        return finish_browser(status, options.browser, source.as_ref(), &session, &state, output_mode);
    }

    if !output_mode.json && !output_mode.quiet {
        write_mode_status(output_mode, "jet db> ");
    }
    let mut line = String::new();
    let mut pending = String::new();
    let mut had_error = false;
    loop {
        line.clear();
        let read = match io::stdin().read_line(&mut line) {
            Ok(read) => read,
            Err(error) => {
                let db_error = DbError::new(
                    "E2105",
                    "stdin",
                    "stdin",
                    format!("could not read console input: {error}"),
                    "provide a readable SQL script or exit the console",
                );
                emit_error(output_mode, options.output, &db_error);
                had_error = true;
                break;
            }
        };
        if read == 0 {
            if !pending.trim().is_empty() {
                let inputs = match split_sql_statements(&pending) {
                    Ok(inputs) => inputs,
                    Err(message) => {
                        let db_error = DbError::new("E2702", "stdin", "stdin", message, "close the SQL quote or parenthesized expression");
                        emit_error(output_mode, options.output, &db_error);
                        had_error = true;
                        Vec::new()
                    }
                };
                if process_inputs(
                    &inputs,
                    true,
                    source.as_ref(),
                    &mut session,
                    &mut state,
                    &options,
                    output_mode,
                ) != 0 {
                    had_error = true;
                }
            }
            break;
        }
        if pending.len().saturating_add(line.len()) > options.max_input_bytes {
            let error = DbError::new(
                "E2603",
                "stdin",
                "stdin",
                format!("SQL statement exceeds the {}-byte limit (not truncated)", options.max_input_bytes),
                "raise `--max-bytes` explicitly or submit a smaller statement",
            );
            emit_error(output_mode, options.output, &error);
            pending.clear();
            had_error = true;
            if !output_mode.json && !output_mode.quiet {
                write_mode_status(output_mode, "jet db> ");
            }
            continue;
        }
        let trimmed = line.trim();
        if pending.trim().is_empty() && trimmed.starts_with('.') {
            let inputs = vec![trimmed.to_string()];
            if process_inputs(
                &inputs,
                true,
                source.as_ref(),
                &mut session,
                &mut state,
                &options,
                output_mode,
            ) != 0 {
                had_error = true;
            }
            if trimmed == ".quit" || trimmed == ".exit" {
                break;
            }
            if !output_mode.json && !output_mode.quiet {
                write_mode_status(output_mode, "jet db> ");
            }
            continue;
        }
        pending.push_str(&line);
        if has_complete_sql_statement(&pending) {
            let inputs = match split_sql_statements(&pending) {
                Ok(inputs) => inputs,
                Err(message) => {
                    let db_error = DbError::new("E2702", "stdin", "stdin", message, "close the SQL quote or parenthesized expression");
                    emit_error(output_mode, options.output, &db_error);
                    had_error = true;
                    Vec::new()
                }
            };
            pending.clear();
            if process_inputs(
                &inputs,
                true,
                source.as_ref(),
                &mut session,
                &mut state,
                &options,
                output_mode,
            ) != 0 {
                had_error = true;
            }
            if !output_mode.json && !output_mode.quiet {
                write_mode_status(output_mode, "jet db> ");
            }
        } else if !output_mode.json && !output_mode.quiet {
            write_mode_status(output_mode, "...> ");
        }
    }
    let status = if had_error {
        ExitCodes::USER_ERROR
    } else {
        0
    };
    finish_browser(status, options.browser, source.as_ref(), &session, &state, output_mode)
}
fn finish_browser(
    status: i32,
    enabled: bool,
    source: Option<&LoadedSource>,
    session: &SqlSession,
    state: &DbState,
    mode: OutputMode,
) -> i32 {
    if !enabled {
        return status;
    }
    match serve_browser(source, session, state, mode) {
        Ok(()) => status,
        Err(error) => {
            emit_error(mode, DbOutput::Human, &error);
            if status == 0 {
                ExitCodes::USER_ERROR
            } else {
                status
            }
        }
    }
}

fn serve_browser(
    source: Option<&LoadedSource>,
    session: &SqlSession,
    state: &DbState,
    mode: OutputMode,
) -> Result<(), DbError> {
    let source = source.ok_or_else(|| {
        DbError::new(
            "E2600",
            "browser",
            "",
            "the browser projection needs a loaded local source",
            "load a local JSON, JSONL, or CSV source before serving the inspector",
        )
    })?;
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        DbError::new(
            "E2105",
            "browser",
            &source.path,
            format!("could not bind the loopback browser inspector: {error}"),
            "check local loopback availability and retry",
        )
    })?;
    let address = listener.local_addr().map_err(|error| {
        DbError::new(
            "E2105",
            "browser",
            &source.path,
            format!("could not determine the loopback inspector address: {error}"),
            "retry the local browser projection",
        )
    })?;
    let snapshot = browser_snapshot(source, session, state);
    let page = browser_page(&snapshot);
    let url = format!("http://{address}/");
    if mode.json {
        write_db_status(
            mode,
            "browser",
            true,
            StatusFields::new()
                .with("url", url.as_str())
                .with("source", source.name.as_str())
                .with("plan", state.last_plan.as_deref().unwrap_or("not-run"))
                .with(
                    "status",
                    if state.last_result.is_some() {
                        "ready"
                    } else {
                        "schema-only"
                    },
                )
                .with(
                    "trust",
                    "loopback-only; same authorized SqlSession; no remote provider",
                ),
        );
    } else {
        write_mode_status(mode, &format!("browser inspector: {url}\n"));
    }
    let (mut stream, peer) = listener.accept().map_err(|error| {
        DbError::new(
            "E2105",
            "browser",
            &source.path,
            format!("browser inspector did not receive a loopback request: {error}"),
            "open the printed loopback URL while `jet db --browser` is running",
        )
    })?;
    if !peer.ip().is_loopback() {
        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Err(DbError::new(
            "E1803",
            "browser",
            &source.path,
            "browser inspector rejected a non-loopback peer",
            "connect only through the printed 127.0.0.1 URL",
        ));
    }
    let mut request = [0_u8; 16 * 1024];
    let read = stream.read(&mut request).map_err(|error| {
        DbError::new(
            "E2105",
            "browser",
            &source.path,
            format!("could not read the browser request: {error}"),
            "retry the loopback inspector request",
        )
    })?;
    let request_line = String::from_utf8_lossy(&request[..read]);
    let request_path = request_line.split_whitespace().nth(1).unwrap_or("");
    let (status_line, content_type, body) = match request_path {
        "/" => ("200 OK", "text/html; charset=utf-8", page),
        "/state.json" => ("200 OK", "application/json", snapshot),
        _ => ("404 Not Found", "text/plain; charset=utf-8", "not found\n".to_string()),
    };
    let response = format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(response.as_bytes()).map_err(|error| {
        DbError::new(
            "E2105",
            "browser",
            &source.path,
            format!("could not write the browser response: {error}"),
            "retry the loopback inspector request",
        )
    })?;
    Ok(())
}

fn browser_snapshot(source: &LoadedSource, session: &SqlSession, state: &DbState) -> String {
    let result = state.last_result.as_deref().unwrap_or("null");
    let status = if state.last_result.is_some() {
        "ready"
    } else {
        "schema-only"
    };
    format!(
        "{{\"schema\":\"jet.db/v1\",\"source\":{},\"source_path\":{},\"status\":{},\"trust\":{{\"transport\":\"loopback\",\"authority\":\"same authorized SqlSession\",\"base_dir\":{},\"remote\":false}},\"table\":{{\"rows\":{},\"columns\":{}}},\"plan\":{},\"timing\":{{\"elapsed_ms\":{}}},\"result_size\":{{\"bytes\":{},\"rows\":{}}},\"result\":{}}}\\n",
        json_str(&source.name),
        json_str(&source.path),
        json_str(status),
        json_str(&session.base_dir().to_string_lossy()),
        source.rows,
        columns_json(&source.columns),
        json_str(state.last_plan.as_deref().unwrap_or("not-run")),
        state.last_elapsed_ms,
        state.last_output_bytes,
        state.last_rows,
        result,
    )
}

fn browser_page(snapshot: &str) -> String {
    let escaped = html_escape(snapshot);
    format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Jet DB inspector</title><style>body{{font:15px system-ui,sans-serif;max-width:1000px;margin:2rem auto;padding:0 1rem;background:#101419;color:#e7edf4}}pre{{white-space:pre-wrap;overflow:auto;background:#18212b;border:1px solid #334454;border-radius:8px;padding:1rem}}</style><h1>Jet DB inspector</h1><p>Loopback-only projection of the same authorized SQL session.</p><pre>{escaped}</pre>"
    )
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}


/// Stable metadata rows used by `jet inspect dossier data`. A dossier is a
/// separate process from a live console, so session values are deliberately
/// reported as scoped facts rather than invented measurements.
pub(crate) fn dossier_data_json() -> String {
    let rows = DOSSIER_DATA_ROWS;
    format!(
        "[{}]",
        rows.iter()
            .map(
                |(kind, surface, meaning, scope, value, status)| format!(
                    "{{\"kind\":{},\"surface\":{},\"meaning\":{},\"scope\":{},\"value\":{},\"status\":{},\"authority\":\"same local typed query session\"}}",
                    json_str(kind),
                    json_str(surface),
                    json_str(meaning),
                    json_str(scope),
                    json_str(value),
                    json_str(status),
                )
            )
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(crate) fn dossier_data_value() -> StatusValue {
    StatusValue::array(DOSSIER_DATA_ROWS.iter().map(
        |(kind, surface, meaning, scope, value, status)| {
            StatusValue::object(
                StatusFields::new()
                    .with("kind", *kind)
                    .with("surface", *surface)
                    .with("meaning", *meaning)
                    .with("scope", *scope)
                    .with("value", *value)
                    .with("status", *status)
                    .with("authority", "same local typed query session"),
            )
        },
    ))
}

pub(crate) fn dossier_data_limits_value() -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("max_input_bytes", DEFAULT_MAX_INPUT_BYTES)
            .with("max_rows", DEFAULT_MAX_ROWS)
            .with("max_output_bytes", DEFAULT_MAX_OUTPUT_BYTES),
    )
}

pub(crate) fn dossier_data_text() -> String {
    [
        "sql console data facts (inspect is not a live query session)",
        "  schema: typed columns, nullability, and source row bound (.schema)",
        "  table: source identity and typed row carrier (.tables)",
        "  timing: latest-query elapsed milliseconds (.stats; not measured here)",
        "  plan: shared scan/filter/group/project/sort/limit plan (.plan; not measured here)",
        "  output: deterministic human/json/jsonl/csv modes",
        "  result-size: final rendered bytes and --max-output-bytes bound (.stats; not measured here)",
    ]
    .join("\n")
        + "\n"
}

pub(crate) fn dossier_data_limits_json() -> String {
    format!(
        "{{\"max_input_bytes\":{},\"max_rows\":{},\"max_output_bytes\":{}}}",
        DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_ROWS, DEFAULT_MAX_OUTPUT_BYTES
    )
}

fn parse_output_format(value: &str) -> Result<DbOutput, DbError> {
    match value {
        "human" | "text" => Ok(DbOutput::Human),
        "json" => Ok(DbOutput::Json),
        "jsonl" => Ok(DbOutput::Jsonl),
        "csv" => Ok(DbOutput::Csv),
        _ => Err(DbError::new(
            "E2104",
            "arguments",
            "",
            format!("unknown db output format `{value}`"),
            "choose human, json, jsonl, or csv",
        )),
    }
}

fn canonical_cli_right(right: &str) -> Option<String> {
    let canonical = jet_foundation::Authority::parse_right(right)?;
    let operation = canonical.split_once(':').map_or(canonical.as_str(), |(head, _)| head);
    if !operation.contains('.') {
        return Some(canonical);
    }
    let builtin = jet_foundation::Syntax::BUILTIN_EFFECT_LEAVES
        .iter()
        .any(|leaf| leaf.eq_ignore_ascii_case(operation));
    let declared = jet_foundation::Authority::effect_declarations()
        .iter()
        .filter(|declaration| declaration.name.contains('.'))
        .any(|declaration| declaration.name.eq_ignore_ascii_case(operation));
    (builtin || declared).then_some(canonical)
}

fn parse_rights_value(value: &str, option: &str, rights: &mut Vec<String>) -> Result<(), DbError> {
    let mut parsed: Vec<String> = Vec::new();
    for raw_right in value.split(',') {
        let right = raw_right.trim();
        if right.is_empty() {
            return Err(DbError::new(
                "E2104",
                "arguments",
                "",
                format!("{option} contains an empty right"),
                format!("pass `{option}=FS.Read,DB.Read`"),
            ));
        }
        let canonical = canonical_cli_right(right).ok_or_else(|| {
            DbError::new(
                "E2104",
                "arguments",
                "",
                format!("unknown authority right `{right}`"),
                "use a canonical effect root or leaf such as FS.Read, FS.Write, or DB.Read",
            )
        })?;
        if !rights.iter().any(|existing| existing.eq_ignore_ascii_case(&canonical))
            && !parsed.iter().any(|existing| existing.eq_ignore_ascii_case(&canonical))
        {
            parsed.push(canonical);
        }
    }
    rights.extend(parsed);
    Ok(())
}

fn has_exact_right(rights: &[String], expected: &str) -> bool {
    rights.iter().any(|right| right.eq_ignore_ascii_case(expected))
}

fn parse_options(args: &[String]) -> Result<DbOptions, DbError> {
    let mut options = DbOptions {
        source_path: None,
        source_name: None,
        query: None,
        script: None,
        output: DbOutput::Human,
        machine_override: None,
        color_override: None,
        max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
        max_rows: DEFAULT_MAX_ROWS,
        max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
        allow_rights: Vec::new(),
        deny_rights: Vec::new(),
        allow_fs: false,
        deny_fs: false,
        allow_write: false,
        deny_write: false,
        allow_db: false,
        deny_db: false,
        browser: false,
        help: false,
    };
    let mut index = if args.first().is_some_and(|arg| arg == "db") {
        1
    } else {
        0
    };
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "--help" || argument == "-h" {
            options.help = true;
            index += 1;
            continue;
        }
        if argument == "--json" {
            options.output = DbOutput::Json;
            options.machine_override = Some(true);
            index += 1;
            continue;
        }
        if argument == "--jsonl" {
            options.output = DbOutput::Jsonl;
            options.machine_override = Some(true);
            index += 1;
            continue;
        }
        if argument == "--csv" {
            options.output = DbOutput::Csv;
            options.machine_override = Some(false);
            index += 1;
            continue;
        }
        if argument == "--format" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new(
                    "E2104",
                    "arguments",
                    "",
                    "`--format` needs an output format",
                    "pass `--format json` or `--format=json`",
                )
            })?;
            options.output = parse_output_format(value)?;
            options.machine_override = Some(matches!(options.output, DbOutput::Json | DbOutput::Jsonl));
            index += 2;
            continue;
        }
        if let Some(format) = argument.strip_prefix("--format=") {
            options.output = parse_output_format(format)?;
            options.machine_override = Some(matches!(options.output, DbOutput::Json | DbOutput::Jsonl));
            index += 1;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--allow=") {
            parse_rights_value(value, "--allow", &mut options.allow_rights)?;
            index += 1;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--deny=") {
            parse_rights_value(value, "--deny", &mut options.deny_rights)?;
            index += 1;
            continue;
        }
        if argument == "--allow" || argument == "--deny" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new(
                    "E2104",
                    "arguments",
                    "",
                    format!("`{argument}` needs a comma-separated rights value"),
                    format!("pass `{argument}=FS.Read,DB.Read`"),
                )
            })?;
            if argument == "--allow" {
                parse_rights_value(value, argument, &mut options.allow_rights)?;
            } else {
                parse_rights_value(value, argument, &mut options.deny_rights)?;
            }
            index += 2;
            continue;
        }
        if argument == "--browser" {
            options.browser = true;
            index += 1;
            continue;
        }
        if argument == "--machine" {
            options.machine_override = Some(true);
            index += 1;
            continue;
        }
        if argument == "--human" {
            options.machine_override = Some(false);
            index += 1;
            continue;
        }
        if argument == "--no-color" {
            options.color_override = Some(ColorChoice::Never);
            index += 1;
            continue;
        }
        if argument == "--force-color" || argument == "--color" {
            options.color_override = Some(ColorChoice::Always);
            index += 1;
            continue;
        }
        if let Some(color) = argument.strip_prefix("--color=") {
            options.color_override = match color {
                "always" | "on" => Some(ColorChoice::Always),
                "never" | "off" => Some(ColorChoice::Never),
                "auto" => None,
                _ => {
                    return Err(DbError::new(
                        "E2104",
                        "arguments",
                        "",
                        format!("unknown color mode `{color}`"),
                        "choose `--color=always`, `--color=never`, or `--color=auto`",
                    ))
                }
            };
            index += 1;
            continue;
        }
        if argument == "--quiet" {
            index += 1;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--query=") {
            options.query = Some(value.to_string());
            index += 1;
            continue;
        }
        if argument == "--query" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--query` needs SQL text", "pass `--query 'SELECT * FROM source'" )
            })?;
            options.query = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--script=") {
            options.script = Some(PathBuf::from(value));
            index += 1;
            continue;
        }
        if argument == "--script" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--script` needs a path", "pass `--script report.sql`" )
            })?;
            options.script = Some(PathBuf::from(value));
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--table=") {
            let (name, path) = value.split_once('=').ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--table` needs NAME=PATH", "pass `--table orders=orders.json`" )
            })?;
            options.source_name = Some(name.to_string());
            options.source_path = Some(PathBuf::from(path));
            index += 1;
            continue;
        }
        if argument == "--table" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--table` needs NAME=PATH", "pass `--table orders=orders.json`" )
            })?;
            let (name, path) = value.split_once('=').ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--table` needs NAME=PATH", "pass `--table orders=orders.json`" )
            })?;
            options.source_name = Some(name.to_string());
            options.source_path = Some(PathBuf::from(path));
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--max-rows=") {
            options.max_rows = parse_positive_limit(value, "--max-rows")?;
            index += 1;
            continue;
        }
        if argument == "--max-rows" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--max-rows` needs an integer", "pass `--max-rows 10000`" )
            })?;
            options.max_rows = parse_positive_limit(value, "--max-rows")?;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--max-bytes=") {
            options.max_input_bytes = parse_positive_limit(value, "--max-bytes")?;
            index += 1;
            continue;
        }
        if argument == "--max-bytes" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--max-bytes` needs an integer", "pass `--max-bytes 1048576`" )
            })?;
            options.max_input_bytes = parse_positive_limit(value, "--max-bytes")?;
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--max-output-bytes=") {
            options.max_output_bytes = parse_positive_limit(value, "--max-output-bytes")?;
            index += 1;
            continue;
        }
        if argument == "--max-output-bytes" {
            let value = args.get(index + 1).ok_or_else(|| {
                DbError::new("E2104", "arguments", "", "`--max-output-bytes` needs an integer", "pass `--max-output-bytes 1048576`" )
            })?;
            options.max_output_bytes = parse_positive_limit(value, "--max-output-bytes")?;
            index += 2;
            continue;
        }
        if argument.starts_with("--allow-") || argument.starts_with("--deny-") {
            let replacement = match argument {
                "--allow-fs" => "`--allow=FS`",
                "--allow-read" => "`--allow=FS.Read`",
                "--deny-fs" => "`--deny=FS`",
                "--deny-read" => "`--deny=FS.Read`",
                "--allow-write" => "`--allow=FS.Write`",
                "--allow-db" => "`--allow=DB`",
                "--deny-db" => "`--deny=DB`",
                _ => "`--allow=RIGHTS` or `--deny=RIGHTS`",
            };
            return Err(DbError::new(
                "E2104",
                "arguments",
                "",
                format!("retired db authority option `{argument}`"),
                format!("replace it with {replacement}"),
            ));
        }
        if argument.starts_with('-') {
            return Err(DbError::new(
                "E2104",
                "arguments",
                "",
                format!("unknown db option `{argument}`"),
                "run `jet db --help` for the bounded console options",
            ));
        }
        if options.source_path.is_some() {
            return Err(DbError::new(
                "E2104",
                "arguments",
                "",
                "the db console accepts one source path",
                "use `--table NAME=PATH` for an explicit table identity",
            ));
        }
        options.source_path = Some(PathBuf::from(argument));
        index += 1;
    }
    if let Some(conflict) = options.allow_rights.iter().find(|allow| {
        options
            .deny_rights
            .iter()
            .any(|deny| deny.eq_ignore_ascii_case(allow))
    }) {
        return Err(DbError::new(
            "E2102",
            "authority",
            "",
            format!("authority right `{conflict}` was both granted and denied"),
            "remove the duplicate right from either `--allow` or `--deny`",
        ));
    }
    options.allow_fs =
        has_exact_right(&options.allow_rights, "FS")
            || has_exact_right(&options.allow_rights, "FS.Read");
    options.allow_write =
        has_exact_right(&options.allow_rights, "FS")
            || has_exact_right(&options.allow_rights, "FS.Write");
    options.allow_db =
        has_exact_right(&options.allow_rights, "DB")
            || has_exact_right(&options.allow_rights, "DB.Read");
    options.deny_fs =
        has_exact_right(&options.deny_rights, "FS")
            || has_exact_right(&options.deny_rights, "FS.Read");
    options.deny_write =
        has_exact_right(&options.deny_rights, "FS")
            || has_exact_right(&options.deny_rights, "FS.Write");
    options.deny_db =
        has_exact_right(&options.deny_rights, "DB")
            || has_exact_right(&options.deny_rights, "DB.Read");
    Ok(options)
}

fn parse_positive_limit(value: &str, option: &str) -> Result<usize, DbError> {
    let parsed = value.parse::<usize>().map_err(|_| {
        DbError::new(
            "E2104",
            "arguments",
            "",
            format!("{option} needs a non-negative integer, got `{value}`"),
            format!("pass `{option}=10000` or another bounded integer"),
        )
    })?;
    if parsed == 0 {
        return Err(DbError::new(
            "E2104",
            "arguments",
            "",
            format!("{option} must be greater than zero"),
            "choose a positive bound so output cannot become an accidental unbounded operation",
        ));
    }
    Ok(parsed)
}

fn effective_mode(mode: OutputMode, output: DbOutput) -> OutputMode {
    if matches!(output, DbOutput::Json | DbOutput::Jsonl) && !mode.json {
        OutputMode {
            json: true,
            color: mode.color,
            quiet: mode.quiet,
        }
    } else {
        mode
    }
}
fn resolved_mode(mode: OutputMode, options: &DbOptions) -> OutputMode {
    let mut resolved = effective_mode(mode, options.output);
    if let Some(machine) = options.machine_override {
        resolved.json = machine;
    }
    if let Some(color) = options.color_override {
        resolved.color = color;
    }
    resolved
}

fn emit_error(mode: OutputMode, output: DbOutput, error: &DbError) {
    let mode = effective_mode(mode, output);
    if mode.json {
        write_db_status(
            mode,
            error.operation.as_str(),
            false,
            StatusFields::new()
                .with(
                    "source",
                    if error.source.is_empty() {
                        StatusValue::Null
                    } else {
                        StatusValue::from(error.source.as_str())
                    },
                )
                .with("code", error.code.as_str())
                .with("message", error.message.as_str())
                .with("hint", error.hint.as_str()),
        );
    } else {
        write_mode_diagnostic(
            mode,
            &format!(
                "error [{}] {}: {}\nfix: {}\n",
                error.code, error.operation, error.message, error.hint
            ),
        );
    }
}

fn usage() -> &'static str {
    "jet db [PATH] [--query SQL | --script PATH]\n\n  --table NAME=PATH       preserve an explicit typed source identity\n  --allow=RIGHTS          authorize comma-separated canonical rights\n  --deny=RIGHTS           deny comma-separated canonical rights\n  --machine | --human     override the output channel\n  --color=MODE            choose auto, always, or never color\n  --browser               serve a loopback-only inspector for this session\n  --max-rows N            bound decoded input rows\n  --max-bytes N           bound source/script bytes\n  --max-output-bytes N    bound one rendered result\n  --json | --jsonl | --csv deterministic output modes\n  .tables .schema .plan .stats .export PATH [json|csv] .help .quit\n"
}

fn enforce_json_row_bound(text: &str, max_rows: usize, path: &str) -> Result<(), DbError> {
    let mut started = false;
    let mut quote = false;
    let mut escaped = false;
    let mut nested = 0_usize;
    let mut item = false;
    let mut items = 0_usize;
    for character in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote {
            if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quote = false;
            }
            continue;
        }
        if character == '"' {
            quote = true;
            if started && nested == 0 {
                item = true;
            }
            continue;
        }
        if !started {
            if character.is_whitespace() {
                continue;
            }
            if character != '[' {
                return Ok(());
            }
            started = true;
            continue;
        }
        if nested == 0 {
            match character {
                ']' => break,
                ',' => {
                    if item {
                        items = items.saturating_add(1);
                        item = false;
                    }
                }
                '{' | '[' => {
                    nested = 1;
                    item = true;
                }
                character if !character.is_whitespace() => item = true,
                _ => {}
            }
        } else {
            match character {
                '"' => {}
                '{' | '[' => nested = nested.saturating_add(1),
                '}' | ']' => nested = nested.saturating_sub(1),
                _ => {}
            }
        }
    }
    if item {
        items = items.saturating_add(1);
    }
    if items > max_rows {
        return Err(DbError::new(
            "E2603",
            "source",
            path,
            format!("JSON source has {items} rows, over the {max_rows}-row limit"),
            "raise `--max-rows` explicitly or split the source",
        ));
    }
    Ok(())
}

fn load_source(
    path: &Path,
    explicit_name: Option<&str>,
    base_dir: &Path,
    allow_fs: bool,
    deny_fs: bool,
    allow_db: bool,
    deny_db: bool,
    max_bytes: usize,
    max_rows: usize,
) -> Result<LoadedSource, DbError> {
    let path_text = path.to_string_lossy().into_owned();
    if is_remote(&path_text) {
        if deny_db || !allow_db {
            return Err(DbError::new(
                "E1803",
                "source",
                &path_text,
                "remote database or URL sources require explicit DB authority",
                "pass `--allow=DB.Read` only when a configured provider is authorized; this console never fetches a remote URL itself",
            ));
        }
        return Err(DbError::new(
            "E2601",
            "source",
            &path_text,
            "no remote provider is configured for `jet db`",
            "use a local JSON, JSONL, or CSV source, or run the authorized provider command that owns the remote connection",
        ));
    }
    if deny_fs || !allow_fs {
        return Err(DbError::new(
            "E1803",
            "source",
            &path_text,
            "reading a db source needs explicit FS authority",
            "pass `--allow=FS.Read` and keep the source path local",
        ));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    let text = read_bounded_file(&resolved, max_bytes, "source")?;
    let name = explicit_name
        .map(str::to_string)
        .unwrap_or_else(|| source_name_from_path(&resolved));
    if !valid_table_name(&name) {
        return Err(DbError::new(
            "E2602",
            "source",
            &path_text,
            format!("table identity `{name}` is not a SQL identifier"),
            "pass `--table NAME=PATH` with letters, digits, `_`, or `.` in NAME",
        ));
    }
    let extension = resolved
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "parquet" | "sqlite" | "db" | "duckdb") {
        return Err(DbError::new(
            "E2601",
            "source",
            &path_text,
            format!("the local typed loader does not support `.{extension}`"),
            "use JSON, JSONL, or CSV for this console; do not route an unsupported file through a different engine",
        ));
    }
    if extension == "csv" {
        return load_csv_source(&name, &path_text, &text, max_rows);
    }
    if matches!(extension.as_str(), "jsonl" | "ndjson") {
        return load_jsonl_source(&name, &path_text, &text, max_rows);
    }
    enforce_json_row_bound(&text, max_rows, &path_text)?;
    let value = parse_json(&text, false).map_err(|error| {
        DbError::new(
            "E2701",
            "source",
            &path_text,
            format!("invalid JSON source: {}", error.message),
            "provide a top-level array of objects or use a CSV/JSONL source",
        )
    })?;
    normalize_json_source(&name, &path_text, value, text.len(), max_rows)
}

fn read_script(
    path: &Path,
    base_dir: &Path,
    allow_fs: bool,
    deny_fs: bool,
    allow_db: bool,
    deny_db: bool,
    max_bytes: usize,
) -> Result<String, DbError> {
    let path_text = path.to_string_lossy().into_owned();
    if is_remote(&path_text) {
        if deny_db || !allow_db {
            return Err(DbError::new(
                "E1803",
                "script",
                &path_text,
                "remote SQL scripts require explicit DB authority",
                "pass a local script path and authorize FS reads",
            ));
        }
        return Err(DbError::new(
            "E2601",
            "script",
            &path_text,
            "no remote script provider is configured",
            "pass a local SQL script path",
        ));
    }
    if deny_fs || !allow_fs {
        return Err(DbError::new(
            "E1803",
            "script",
            &path_text,
            "reading a SQL script needs explicit FS authority",
            "pass `--allow=FS.Read`",
        ));
    }
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    read_bounded_file(&resolved, max_bytes, "script")
}

fn read_stdin_bounded(max_bytes: usize) -> Result<String, DbError> {
    let mut stdin = io::stdin().lock();
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = stdin.read(&mut chunk).map_err(|error| {
            DbError::new(
                "E2105",
                "stdin",
                "stdin",
                format!("could not read SQL from stdin: {error}"),
                "provide a readable SQL script or pass `--query`",
            )
        })?;
        if read == 0 {
            break;
        }
        if bytes.len().saturating_add(read) > max_bytes {
            return Err(DbError::new(
                "E2603",
                "stdin",
                "stdin",
                format!("SQL input exceeds the {max_bytes}-byte limit (not truncated)"),
                "raise `--max-bytes` explicitly or split the script",
            ));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8(bytes).map_err(|_| {
        DbError::new(
            "E2701",
            "stdin",
            "stdin",
            "SQL input is not valid UTF-8",
            "encode the SQL script as UTF-8",
        )
    })
}

fn read_bounded_file(path: &Path, max_bytes: usize, operation: &str) -> Result<String, DbError> {
    let display = path.to_string_lossy().into_owned();
    let metadata = fs::metadata(path).map_err(|error| {
        DbError::new(
            "E2105",
            operation,
            &display,
            format!("could not inspect `{display}`: {error}"),
            "check that the local path exists and is readable",
        )
    })?;
    let size = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if size > max_bytes {
        return Err(DbError::new(
            "E2603",
            operation,
            &display,
            format!("input is {size} bytes, over the {max_bytes}-byte limit"),
            "raise the explicit bound or split the source before querying it",
        ));
    }
    let bytes = fs::read(path).map_err(|error| {
        DbError::new(
            "E2105",
            operation,
            &display,
            format!("could not read `{display}`: {error}"),
            "check the path and FS authority",
        )
    })?;
    if bytes.len() > max_bytes {
        return Err(DbError::new(
            "E2603",
            operation,
            &display,
            format!("input exceeded the {max_bytes}-byte limit while it was read"),
            "use a smaller source or raise the explicit bound",
        ));
    }
    String::from_utf8(bytes).map_err(|_| {
        DbError::new(
            "E2701",
            operation,
            &display,
            "input is not valid UTF-8",
            "encode the local source or script as UTF-8",
        )
    })
}

fn parse_csv_bounded(
    text: &str,
    max_rows: usize,
    path: &str,
) -> Result<Vec<jet_foundation::CsvKernel::CsvRecord>, DbError> {
    let mut parser = CsvParser::new(CsvOptions {
        delimiter: ',',
        header: false,
        skip_blank: true,
    })
    .map_err(|error| DbError::new("E2701", "source", path, error, "correct the malformed or truncated CSV record"))?;
    let mut records = Vec::new();
    for character in text.chars() {
        if let Some(record) = parser
            .push(character)
            .map_err(|error| DbError::new("E2701", "source", path, error.message(), "correct the malformed or truncated CSV record"))?
        {
            records.push(record);
            if records.len() > max_rows.saturating_add(1) {
                return Err(DbError::new(
                    "E2603",
                    "source",
                    path,
                    format!("CSV source has more than the {max_rows}-row limit"),
                    "raise `--max-rows` explicitly or split the source",
                ));
            }
        }
    }
    if let Some(record) = parser
        .finish()
        .map_err(|error| DbError::new("E2701", "source", path, error.message(), "correct the malformed or truncated CSV record"))?
    {
        records.push(record);
    }
    if records.len().saturating_sub(1) > max_rows {
        return Err(DbError::new(
            "E2603",
            "source",
            path,
            format!("CSV source has more than the {max_rows}-row limit"),
            "raise `--max-rows` explicitly or split the source",
        ));
    }
    Ok(records)
}

fn load_csv_source(
    name: &str,
    path: &str,
    text: &str,
    max_rows: usize,
) -> Result<LoadedSource, DbError> {
    let records = parse_csv_bounded(text, max_rows, path)?;
    let Some(header) = records.first() else {
        return Err(DbError::new(
            "E2701",
            "source",
            path,
            "CSV source has no header row",
            "put one non-empty column name on the first CSV record",
        ));
    };
    let mut seen = HashSet::new();
    for column in &header.fields {
        if !valid_column_name(column) || !seen.insert(column.clone()) {
            return Err(DbError::new(
                "E2701",
                "source",
                path,
                format!("CSV header has an invalid or duplicate column `{column}`"),
                "use unique ASCII identifier column names",
            ));
        }
    }
    let rows = records.len().saturating_sub(1);
    if rows > max_rows {
        return Err(DbError::new(
            "E2603",
            "source",
            path,
            format!("CSV source has {rows} rows, over the {max_rows}-row limit"),
            "raise `--max-rows` explicitly or split the source",
        ));
    }
    let mut values = Vec::with_capacity(rows);
    for (row_index, record) in records.iter().skip(1).enumerate() {
        if record.fields.len() != header.fields.len() {
            return Err(DbError::new(
                "E2701",
                "source",
                path,
                format!("CSV row {} has {} fields, expected {}", row_index + 2, record.fields.len(), header.fields.len()),
                "make every row match the header width",
            ));
        }
        let entries = header
            .fields
            .iter()
            .zip(&record.fields)
            .map(|(name, value)| (name.clone(), Value::Text(value.clone())))
            .collect::<Vec<_>>();
        values.push(Value::Object(entries));
    }
    normalize_json_source(name, path, Value::Array(values), text.len(), max_rows)
}

fn load_jsonl_source(
    name: &str,
    path: &str,
    text: &str,
    max_rows: usize,
) -> Result<LoadedSource, DbError> {
    let mut values = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if values.len() == max_rows {
            return Err(DbError::new(
                "E2603",
                "source",
                path,
                format!("JSONL source has more than the {max_rows}-row limit"),
                "raise `--max-rows` explicitly or split the source",
            ));
        }
        let value = parse_json(line, false).map_err(|error| {
            DbError::new(
                "E2701",
                "source",
                path,
                format!("invalid JSONL value on line {}: {}", line_index + 1, error.message),
                "repair the line or use a valid JSONL object",
            )
        })?;
        values.push(value);
    }
    normalize_json_source(name, path, Value::Array(values), text.len(), max_rows)
}

fn normalize_json_source(
    name: &str,
    path: &str,
    value: Value,
    input_bytes: usize,
    max_rows: usize,
) -> Result<LoadedSource, DbError> {
    let Value::Array(rows) = value else {
        return Err(DbError::new(
            "E2701",
            "source",
            path,
            "source root must be an array of objects",
            "use `[ { \"column\": value } ]` or convert the source to CSV/JSONL",
        ));
    };
    if rows.len() > max_rows {
        return Err(DbError::new(
            "E2603",
            "source",
            path,
            format!("source has {} rows, over the {max_rows}-row limit", rows.len()),
            "raise `--max-rows` explicitly or split the source",
        ));
    }
    let mut accumulators = BTreeMap::<String, ColumnAccum>::new();
    for (row_index, row) in rows.iter().enumerate() {
        let Value::Object(entries) = row else {
            return Err(DbError::new(
                "E2701",
                "source",
                path,
                format!("row {} is not an object", row_index + 1),
                "make every row an object with named columns",
            ));
        };
        let mut row_names = HashSet::new();
        for (column, cell) in entries {
            if !valid_column_name(column) {
                return Err(DbError::new(
                    "E2701",
                    "source",
                    path,
                    format!("row {} has invalid column `{column}`", row_index + 1),
                    "use unique ASCII identifier column names",
                ));
            }
            if !row_names.insert(column.clone()) {
                return Err(DbError::new(
                    "E2701",
                    "source",
                    path,
                    format!("row {} repeats column `{column}`", row_index + 1),
                    "remove duplicate object keys before querying",
                ));
            }
            let accumulator = accumulators.entry(column.clone()).or_insert(ColumnAccum {
                kind: None,
                seen: 0,
                nullable: false,
            });
            accumulator.seen += 1;
            match value_kind(cell) {
                Some(kind) => {
                    accumulator.kind = Some(match accumulator.kind {
                        None => kind,
                        Some(existing) => merge_kind(existing, kind),
                    });
                }
                None => accumulator.nullable = true,
            }
        }
    }
    let columns = accumulators
        .into_iter()
        .map(|(name, accumulator)| Column {
            name,
            kind: accumulator.kind.unwrap_or(ColumnKind::String),
            nullable: accumulator.nullable || accumulator.seen < rows.len(),
        })
        .collect::<Vec<_>>();
    let json_rows = rows
        .iter()
        .map(|row| normalized_row_json(row, &columns))
        .collect::<Result<Vec<_>, _>>()?;
    let json = format!("[{}]", json_rows.join(","));
    Ok(LoadedSource {
        name: name.to_string(),
        path: path.to_string(),
        json,
        columns,
        rows: rows.len(),
        input_bytes,
    })
}

fn value_kind(value: &Value) -> Option<ColumnKind> {
    match value {
        Value::Null => None,
        Value::Bool(_) => Some(ColumnKind::Bool),
        Value::Int(_) => Some(ColumnKind::Int),
        Value::Float(_) => Some(ColumnKind::Float),
        Value::Number(number) => number
            .parse::<i64>()
            .map(|_| ColumnKind::Int)
            .or_else(|_| number.parse::<f64>().map(|_| ColumnKind::Float))
            .ok()
            .or(Some(ColumnKind::String)),
        Value::TypedText(_)
        | Value::Text(_)
        | Value::Bytes(_)
        | Value::Array(_)
        | Value::Object(_) => Some(ColumnKind::String),
    }
}

fn merge_kind(left: ColumnKind, right: ColumnKind) -> ColumnKind {
    if left == right || (matches!(left, ColumnKind::Int) && matches!(right, ColumnKind::Int)) {
        left
    } else if matches!(left, ColumnKind::Int | ColumnKind::Float)
        && matches!(right, ColumnKind::Int | ColumnKind::Float)
    {
        ColumnKind::Float
    } else {
        ColumnKind::String
    }
}

fn normalized_row_json(row: &Value, columns: &[Column]) -> Result<String, DbError> {
    let Value::Object(entries) = row else {
        return Err(DbError::new("E2701", "source", "", "row is not an object", "use an object row"));
    };
    let fields = columns
        .iter()
        .map(|column| {
            let value = entries
                .iter()
                .find(|(name, _)| name == &column.name)
                .map(|(_, value)| value);
            format!("{}:{}", json_str(&column.name), normalized_cell(value, column))
        })
        .collect::<Vec<_>>();
    Ok(format!("{{{}}}", fields.join(",")))
}

fn normalized_cell(value: Option<&Value>, column: &Column) -> String {
    let Some(value) = value else {
        return if column.nullable {
            "null".to_string()
        } else {
            default_cell(column.kind)
        };
    };
    if matches!(value, Value::Null) {
        return if column.nullable {
            "null".to_string()
        } else {
            default_cell(column.kind)
        };
    }
    match column.kind {
        ColumnKind::Int => match value {
            Value::Int(value) => value.to_string(),
            Value::Number(value) => value.parse::<i64>().unwrap_or(0).to_string(),
            _ => "0".to_string(),
        },
        ColumnKind::Float => match value {
            Value::Int(value) => format!("{value}.0"),
            Value::Float(value) => render_float(*value),
            Value::Number(value) => value.clone(),
            _ => "0.0".to_string(),
        },
        ColumnKind::Bool => match value {
            Value::Bool(value) => value.to_string(),
            _ => "false".to_string(),
        },
        ColumnKind::String => json_str(&value_text(value)),
    }
}

fn default_cell(kind: ColumnKind) -> String {
    match kind {
        ColumnKind::Int => "0".to_string(),
        ColumnKind::Float => "0.0".to_string(),
        ColumnKind::Bool => "false".to_string(),
        ColumnKind::String => json_str(""),
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => render_float(*value),
        Value::Number(value) => value.clone(),
        Value::TypedText(value) | Value::Text(value) => value.clone(),
        Value::Bytes(values) => format!(
            "[{}]",
            values
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(render_json_value).collect::<Vec<_>>().join(",")
        ),
        Value::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(name, value)| format!("{}:{}", json_str(name), render_json_value(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn render_json_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => render_float(*value),
        Value::Number(value) => value.clone(),
        Value::TypedText(value) | Value::Text(value) => json_str(value),
        Value::Bytes(values) => format!(
            "[{}]",
            values.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
        ),
        Value::Array(values) => format!(
            "[{}]",
            values.iter().map(render_json_value).collect::<Vec<_>>().join(",")
        ),
        Value::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(name, value)| format!("{}:{}", json_str(name), render_json_value(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn render_float(value: f64) -> String {
    let rendered = value.to_string();
    if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
        rendered
    } else {
        format!("{rendered}.0")
    }
}

fn prepare_session(session: &mut SqlSession, source: &LoadedSource) -> Result<(), DbError> {
    for import in ["use core.data as data", "use core.encoding.json as json"] {
        session
            .execute(import)
            .map_err(|diagnostics| DbError::diagnostics("import", &source.path, &diagnostics))?;
    }
    session
        .define_item(&generated_row_item(&source.columns))
        .map_err(|diagnostics| DbError::diagnostics("schema", &source.path, &diagnostics))
}

fn generated_row_item(columns: &[Column]) -> String {
    let mut source = format!("#Codable\nstruct {ROW_TYPE_NAME} {{\n");
    for column in columns {
        source.push_str("    ");
        source.push_str(&column.name);
        source.push_str(": ");
        source.push_str(column.kind.jet_name());
        if column.nullable {
            source.push('?');
        }
        source.push('\n');
    }
    source.push_str("}\n");
    source
}

fn process_inputs(
    inputs: &[String],
    interactive: bool,
    source: Option<&LoadedSource>,
    session: &mut SqlSession,
    state: &mut DbState,
    options: &DbOptions,
    mode: OutputMode,
) -> i32 {
    let mut status = 0;
    for input in inputs {
        if input.trim().is_empty() {
            continue;
        }
        let result = if input.trim_start().starts_with('.') {
            handle_dot(input.trim(), source, state, options, mode)
        } else {
            run_query(input.trim(), source, session, state, options, mode)
                .map(|()| true)
        };
        match result {
            Ok(true) => {}
            Ok(false) => return 0,
            Err(error) => {
                emit_error(mode, options.output, &error);
                status = ExitCodes::USER_ERROR;
                if !interactive {
                    return status;
                }
            }
        }
    }
    status
}
fn db_sql_value(value: &Value) -> sql_kernel::JetSqlValue {
    match value {
        Value::Null => sql_kernel::JetSqlValue::Null,
        Value::Bool(value) => sql_kernel::JetSqlValue::Bool(*value),
        Value::Int(value) => sql_kernel::JetSqlValue::Int(*value),
        Value::Float(value) => sql_kernel::JetSqlValue::Float(*value),
        Value::Number(value) => sql_kernel::JetSqlValue::literal(value),
        Value::Text(value) => sql_kernel::JetSqlValue::Text(value.clone()),
        other => sql_kernel::JetSqlValue::Text(value_text(other)),
    }
}

fn db_sql_json_value(value: sql_kernel::JetSqlValue) -> Value {
    match value {
        sql_kernel::JetSqlValue::Null => Value::Null,
        sql_kernel::JetSqlValue::Bool(value) => Value::Bool(value),
        sql_kernel::JetSqlValue::Int(value) => Value::Int(value),
        sql_kernel::JetSqlValue::Float(value) => Value::Float(value),
        sql_kernel::JetSqlValue::Text(value) => Value::Text(value),
    }
}

fn run_dynamic_query(
    source: &LoadedSource,
    query: &sql_kernel::JetSqlQuery,
) -> Result<(SqlEval, Value, String, usize), DbError> {
    let parsed = parse_json(&source.json, false).map_err(|error| {
        DbError::new(
            "E2600",
            "query",
            &source.path,
            format!("normalized source could not be decoded: {}", error.message),
            "reload the local source before querying it",
        )
    })?;
    let Value::Array(values) = parsed else {
        return Err(DbError::new(
            "E2600",
            "query",
            &source.path,
            "normalized source is not a row array",
            "reload a local JSON, JSONL, or CSV source",
        ));
    };
    let fields = source
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let result = sql_kernel::execute_sql_rows(&fields, values.len(), query, |index, field| {
        values.get(index).and_then(|row| match row {
            Value::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == field)
                .map(|(_, value)| db_sql_value(value)),
            _ => None,
        })
    })
    .map_err(|message| {
        DbError::new(
            "E2702",
            "query",
            &source.path,
            message,
            "use declared source columns and the supported SELECT query surface",
        )
    })?;
    let result_values = result
        .into_iter()
        .map(|row| {
            Value::Object(
                row.fields
                    .into_iter()
                    .map(|(name, value)| (name, db_sql_json_value(value)))
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let result_tree = Value::Array(result_values);
    let rows = match &result_tree {
        Value::Array(values) => values.len(),
        _ => unreachable!("query result tree is always an array"),
    };
    let rendered = render_json_value(&result_tree);
    Ok((
        SqlEval {
            value: None,
            stdout: String::new(),
            stderr: String::new(),
        },
        result_tree,
        rendered,
        rows,
    ))
}
fn query_result_columns(source: &LoadedSource, query: &sql_kernel::JetSqlQuery) -> Vec<Column> {
    query
        .projection
        .iter()
        .flat_map(|item| match item {
            sql_kernel::JetSqlSelectItem::Wildcard => source
                .columns
                .iter()
                .map(|column| Column {
                    name: column.name.clone(),
                    kind: column.kind,
                    nullable: column.nullable,
                })
                .collect::<Vec<_>>(),
            sql_kernel::JetSqlSelectItem::Field { field, alias } => {
                let source_column = source.columns.iter().find(|column| column.name == *field);
                vec![Column {
                    name: alias.clone().unwrap_or_else(|| field.clone()),
                    kind: source_column.map(|column| column.kind).unwrap_or(ColumnKind::String),
                    nullable: source_column.map(|column| column.nullable).unwrap_or(true),
                }]
            }
            sql_kernel::JetSqlSelectItem::Aggregate {
                function,
                field,
                alias,
            } => {
                let source_column = field
                    .as_deref()
                    .and_then(|field| source.columns.iter().find(|column| column.name == field));
                let default_name = field
                    .as_ref()
                    .map(|field| format!("{}_{}", function.as_str(), field))
                    .unwrap_or_else(|| function.as_str().to_string());
                let (kind, nullable) = match function {
                    sql_kernel::JetSqlAggregate::Count => (ColumnKind::Int, false),
                    sql_kernel::JetSqlAggregate::Sum => (
                        source_column.map(|column| column.kind).unwrap_or(ColumnKind::Float),
                        true,
                    ),
                    sql_kernel::JetSqlAggregate::Avg => (ColumnKind::Float, true),
                    sql_kernel::JetSqlAggregate::Min => (
                        source_column.map(|column| column.kind).unwrap_or(ColumnKind::String),
                        true,
                    ),
                    sql_kernel::JetSqlAggregate::Max => (
                        source_column.map(|column| column.kind).unwrap_or(ColumnKind::String),
                        true,
                    ),
                };
                vec![Column {
                    name: alias.clone().unwrap_or(default_name),
                    kind,
                    nullable,
                }]
            }
        })
        .collect()
}



fn result_row_count(value: &CtValue) -> Option<usize> {
    match value {
        CtValue::Present(value) => result_row_count(value),
        CtValue::List(values) => Some(values.len()),
        _ => None,
    }
}

fn run_query(
    sql: &str,
    source: Option<&LoadedSource>,
    session: &mut SqlSession,
    state: &mut DbState,
    options: &DbOptions,
    mode: OutputMode,
) -> Result<(), DbError> {
    let source = source.ok_or_else(|| {
        DbError::new(
            "E2600",
            "query",
            "",
            "a SQL query needs a local typed source",
            "pass `jet db source.json --allow=FS.Read --query 'SELECT * FROM source'`",
        )
    })?;
    let query = sql_kernel::parse_sql_query(sql).map_err(|message| {
        DbError::new(
            "E2702",
            "query",
            &source.path,
            message,
            "use SELECT ... FROM source with optional WHERE, GROUP BY, ORDER BY, and LIMIT clauses",
        )
    })?;
    if query.source != source.name {
        return Err(DbError::new(
            "E2702",
            "query",
            &source.path,
            format!("query source `{}` does not match loaded source `{}`", query.source, source.name),
            format!("use `FROM {}` or pass `--table {}` with the desired identity", source.name, source.name),
        ));
    }
    sql_kernel::validate_sql_query_schema(
        &query,
        |field| source.columns.iter().any(|column| column.name == field),
        "SQL query",
    )
    .map_err(|message| {
        DbError::new(
            "E2702",
            "query",
            &source.path,
            message,
            "use only columns declared by the loaded source",
        )
    })?;
    let plan = render_plan(&source.name, &query);
    let result_columns = query_result_columns(source, &query);
    let started = Instant::now();
    let (evaluated, result_value, rendered, rows) = if query.needs_projection_execution() {
        let (evaluated, result_tree, rendered, rows) = run_dynamic_query(source, &query)?;
        (
            evaluated,
            status_data_value(&result_tree),
            rendered,
            rows,
        )
    } else {
        let expression = format!(
            "data.query(json.decode<[{ROW_TYPE_NAME}]>({}) ?? panic(\"input\"), SQL{{{}}})",
            json_str(&source.json),
            json_str(sql),
        );
        let evaluated = session
            .execute(&expression)
            .map_err(|diagnostics| DbError::diagnostics("query", &source.path, &diagnostics))?;
        let value = evaluated.value.as_ref().ok_or_else(|| {
            DbError::new(
                "E2600",
                "query",
                &source.path,
                "the SQL query produced no result value",
                "keep the query as a SELECT expression",
            )
        })?;
        let rendered = jet::Comptime::render_datatree_for_tir(value);
        let rows = result_row_count(value).ok_or_else(|| {
            DbError::new(
                "E2600",
                "query",
                &source.path,
                "query result was not a typed row list",
                "use the local typed-table query surface rather than a scalar expression",
            )
        })?;
        let result_value = status_ct_value(value);
        (evaluated, result_value, rendered, rows)
    };
    let elapsed_ms = started.elapsed().as_millis();
    let final_output_bytes = final_output_bytes(
        sql,
        source,
        &evaluated,
        &result_value,
        &rendered,
        rows,
        elapsed_ms,
        &plan,
        &result_columns,
        options,
        mode,
    )?;
    state.last_sql = Some(sql.to_string());
    state.last_plan = Some(plan.clone());
    state.last_result = Some(rendered.to_string());
    state.last_rows = rows;
    state.last_elapsed_ms = elapsed_ms;
    state.last_output_bytes = final_output_bytes;
    emit_result(
        sql,
        source,
        &evaluated,
        &result_value,
        &rendered,
        rows,
        elapsed_ms,
        &plan,
        &result_columns,
        options,
        mode,
    )
}

fn db_output_name(output: DbOutput, mode: OutputMode) -> &'static str {
    match output {
        DbOutput::Csv => "csv",
        DbOutput::Jsonl => "jsonl",
        DbOutput::Json => "json",
        DbOutput::Human if mode.json => "json",
        DbOutput::Human => "human",
    }
}

fn status_trust(transport: &str, authority: &str, remote: bool) -> StatusValue {
    StatusValue::object(
        StatusFields::new()
            .with("transport", transport)
            .with("authority", authority)
            .with("remote", remote),
    )
}

fn status_value_json_len(value: &StatusValue) -> usize {
    match value {
        StatusValue::Null => 4,
        StatusValue::Bool(value) => value.to_string().len(),
        StatusValue::Integer(value) => value.to_string().len(),
        StatusValue::Float(value) => value.len(),
        StatusValue::String(value) => json_str(value).len(),
        StatusValue::Array(values) => {
            2 + values.iter().map(status_value_json_len).sum::<usize>() + values.len().saturating_sub(1)
        }
        StatusValue::Object(fields) => {
            2 + fields
                .iter()
                .map(|(name, value)| json_str(name).len() + 1 + status_value_json_len(value))
                .sum::<usize>()
                + fields.iter().count().saturating_sub(1)
        }
    }
}

fn db_row_status(
    source: &LoadedSource,
    index: usize,
    output_mode: &str,
    plan: &str,
    rows: usize,
    row: &StatusValue,
) -> StatusEnvelope {
    db_status(
        "row",
        true,
        StatusFields::new()
            .with("source", source.name.as_str())
            .with("index", index)
            .with("output_mode", output_mode)
            .with("plan", plan)
            .with(
                "result_size",
                StatusValue::object(
                    StatusFields::new()
                        .with("bytes", status_value_json_len(row))
                        .with("rows", rows),
                ),
            )
            .with(
                "trust",
                status_trust(
                    "local",
                    "same authorized local session",
                    false,
                ),
            )
            .with("row", row.clone()),
    )
}

fn db_query_status(
    sql: &str,
    source: &LoadedSource,
    evaluated: &SqlEval,
    result: &StatusValue,
    rows: usize,
    elapsed_ms: u128,
    plan: &str,
    columns: &[Column],
    output_mode: &str,
) -> StatusEnvelope {
    db_status(
        "query",
        true,
        StatusFields::new()
            .with("source", source.name.as_str())
            .with("sql", sql)
            .with("rows", rows)
            .with("elapsed_ms", elapsed_ms as i128)
            .with("input_bytes", source.input_bytes)
            .with("output_mode", output_mode)
            .with("plan", plan)
            .with(
                "result_size",
                StatusValue::object(
                    StatusFields::new()
                        .with("bytes", status_value_json_len(result))
                        .with("rows", rows),
                ),
            )
            .with(
                "trust",
                status_trust(
                    "local",
                    "same authorized local session",
                    false,
                ),
            )
            .with("columns", columns_value(columns))
            .with("result", result.clone())
            .with("stdout", evaluated.stdout.as_str())
            .with("stderr", evaluated.stderr.as_str()),
    )
}

fn final_output_bytes(
    sql: &str,
    source: &LoadedSource,
    evaluated: &SqlEval,
    result: &StatusValue,
    rendered: &str,
    rows: usize,
    elapsed_ms: u128,
    plan: &str,
    columns: &[Column],
    options: &DbOptions,
    mode: OutputMode,
) -> Result<usize, DbError> {
    let output = if mode.json && options.output == DbOutput::Jsonl {
        let StatusValue::Array(values) = result else {
            return Err(DbError::new(
                "E2600",
                "query",
                &source.path,
                "result is not an array for JSONL output",
                "use `--json` for the bounded envelope",
            ));
        };
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                db_row_status(
                    source,
                    index,
                    db_output_name(options.output, mode),
                    plan,
                    rows,
                    value,
                )
                .json_line()
                .len()
            })
            .sum::<usize>()
    } else if mode.json {
        db_query_status(
            sql,
            source,
            evaluated,
            result,
            rows,
            elapsed_ms,
            plan,
            columns,
            db_output_name(options.output, mode),
        )
        .json_line()
        .len()
    } else if options.output == DbOutput::Csv {
        result_csv(rendered, columns, &source.path)?.len()
    } else {
        rendered.len()
            + evaluated.stdout.len()
            + evaluated.stderr.len()
            + 1
            + if mode.quiet {
                0
            } else {
                format!("rows: {rows} time: {elapsed_ms}ms source: {}\n", source.name).len()
            }
    };
    if output > options.max_output_bytes {
        return Err(DbError::new(
            "E2603",
            "query",
            &source.path,
            format!(
                "final rendered output is {output} bytes, over the {}-byte output limit",
                options.max_output_bytes
            ),
            "raise `--max-output-bytes` or add a smaller SQL LIMIT",
        ));
    }
    Ok(output)
}

fn emit_result(
    sql: &str,
    source: &LoadedSource,
    evaluated: &SqlEval,
    result: &StatusValue,
    rendered: &str,
    rows: usize,
    elapsed_ms: u128,
    plan: &str,
    columns: &[Column],
    options: &DbOptions,
    mode: OutputMode,
) -> Result<(), DbError> {
    if mode.json {
        if options.output == DbOutput::Jsonl {
            let StatusValue::Array(values) = result else {
                return Err(DbError::new(
                    "E2600",
                    "query",
                    &source.path,
                    "result is not an array for JSONL output",
                    "use `--json` for the bounded envelope",
                ));
            };
            for (index, value) in values.iter().enumerate() {
                let status = db_row_status(
                    source,
                    index,
                    db_output_name(options.output, mode),
                    plan,
                    rows,
                    value,
                )
                .json_line();
                write_mode_machine(mode, &status);
            }
            return Ok(());
        }
        let status = db_query_status(
            sql,
            source,
            evaluated,
            result,
            rows,
            elapsed_ms,
            plan,
            columns,
            db_output_name(options.output, mode),
        )
        .json_line();
        write_mode_machine(mode, &status);
        return Ok(());
    }
    if options.output == DbOutput::Csv {
        let csv = result_csv(rendered, columns, &source.path)?;
        write_mode_renderable(mode, &csv);
    } else {
        write_mode_renderable(mode, &format!("{rendered}\n"));
    }
    if !evaluated.stdout.is_empty() {
        write_mode_renderable(mode, &evaluated.stdout);
    }
    if !evaluated.stderr.is_empty() {
        write_mode_diagnostic(mode, &evaluated.stderr);
    }
    write_mode_status(mode, &format!("rows: {rows} time: {elapsed_ms}ms source: {}\n", source.name));
    Ok(())
}

fn columns_json(columns: &[Column]) -> String {
    format!(
        "[{}]",
        columns
            .iter()
            .map(|column| {
                format!(
                    "{{\"name\":{},\"type\":{},\"nullable\":{}}}",
                    json_str(&column.name),
                    json_str(column.kind.jet_name()),
                    column.nullable
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn columns_value(columns: &[Column]) -> StatusValue {
    StatusValue::array(columns.iter().map(|column| {
        StatusValue::object(
            StatusFields::new()
                .with("name", column.name.as_str())
                .with("type", column.kind.jet_name())
                .with("nullable", column.nullable),
        )
    }))
}
fn status_data_value(value: &Value) -> StatusValue {
    match value {
        Value::Null => StatusValue::Null,
        Value::Bool(value) => StatusValue::from(*value),
        Value::Int(value) => StatusValue::from(*value),
        Value::Float(value) => StatusValue::Float(value.to_string()),
        Value::Number(value) => value
            .parse::<i128>()
            .map(StatusValue::Integer)
            .unwrap_or_else(|_| StatusValue::Float(value.clone())),
        Value::TypedText(value) | Value::Text(value) => StatusValue::from(value.as_str()),
        Value::Bytes(values) => {
            StatusValue::array(values.iter().map(|value| StatusValue::from(*value as i128)))
        }
        Value::Array(values) => StatusValue::array(values.iter().map(status_data_value)),
        Value::Object(fields) => StatusValue::object(
            fields
                .iter()
                .fold(StatusFields::new(), |fields, (name, value)| {
                    fields.with(name.as_str(), status_data_value(value))
                }),
        ),
    }
}

fn status_map_key(key: &CtKey) -> String {
    match key {
        CtKey::Int(value) => value.to_string(),
        CtKey::Str(value) => value.clone(),
        CtKey::Bool(value) => value.to_string(),
        CtKey::Char(value) => value.to_string(),
        CtKey::Tuple(_) | CtKey::Struct { .. } | CtKey::Enum { .. } => {
            key.to_value().to_json()
        }
    }
}

fn status_ct_value(value: &CtValue) -> StatusValue {
    if let CtValue::Enum {
        type_name,
        variant,
        args,
    } = value
    {
        if matches!(
            type_name.as_str(),
            "DataTree" | "JSON" | "TOML" | "YAML" | "CSV"
        ) {
            return match variant.as_str() {
                "Null" => StatusValue::Null,
                "Number" => match args.first().map(|(_, value)| value) {
                    Some(CtValue::Str(value)) => value
                        .parse::<i128>()
                        .map(StatusValue::Integer)
                        .unwrap_or_else(|_| StatusValue::Float(value.clone())),
                    _ => StatusValue::Null,
                },
                _ => args
                    .first()
                    .map(|(_, value)| status_ct_value(value))
                    .unwrap_or(StatusValue::Null),
            };
        }
    }
    match value {
        CtValue::Int(value) => StatusValue::from(*value),
        CtValue::Float(value) => StatusValue::Float(value.to_json()),
        CtValue::Bool(value) => StatusValue::from(*value),
        CtValue::Char(value) => StatusValue::from(value.to_string()),
        CtValue::Str(value) => StatusValue::from(value.as_str()),
        CtValue::BigInt(value) => StatusValue::from(value.to_string_rep().as_str()),
        CtValue::Bytes(values) => {
            StatusValue::array(values.iter().map(|value| StatusValue::from(*value as i128)))
        }
        CtValue::List(values) => StatusValue::array(values.iter().map(status_ct_value)),
        CtValue::Map(values) => StatusValue::object(
            values.iter().fold(StatusFields::new(), |fields, (key, value)| {
                fields.with(status_map_key(key), status_ct_value(value))
            }),
        ),
        CtValue::Struct { fields, .. } => StatusValue::object(
            fields
                .iter()
                .filter(|(name, _)| !jet::Syntax::is_memo_storage_name(name))
                .fold(StatusFields::new(), |fields, (name, value)| {
                    fields.with(name.as_str(), status_ct_value(value))
                }),
        ),
        CtValue::Enum {
            variant, args, ..
        } if args.is_empty() => StatusValue::from(variant.as_str()),
        CtValue::Enum {
            variant, args, ..
        } if args.iter().all(|(label, _)| label.is_some()) => {
            let fields = args.iter().fold(StatusFields::new(), |fields, (label, value)| {
                fields.with(
                    label.as_deref().expect("checked enum label"),
                    status_ct_value(value),
                )
            });
            StatusValue::object(
                StatusFields::new().with(variant.as_str(), StatusValue::object(fields)),
            )
        }
        CtValue::Enum { variant, args, .. } => StatusValue::object(
            StatusFields::new().with(
                variant.as_str(),
                StatusValue::array(args.iter().map(|(_, value)| status_ct_value(value))),
            ),
        ),
        CtValue::Present(value) => status_ct_value(value),
        CtValue::Failed(CtReport::Clean(_)) | CtValue::Unit | CtValue::Closure(_) => {
            StatusValue::Null
        }
        CtValue::Failed(CtReport::Told(value)) => {
            StatusValue::object(StatusFields::new().with("err", status_ct_value(value)))
        }
    }
}
fn result_csv(rendered: &str, columns: &[Column], path: &str) -> Result<String, DbError> {
    let parsed = parse_json(rendered, false).map_err(|error| {
        DbError::new(
            "E2600",
            "csv",
            path,
            format!("result is not valid JSON: {}", error.message),
            "use `--json` for the canonical result envelope",
        )
    })?;
    let Value::Array(rows) = parsed else {
        return Err(DbError::new(
            "E2600",
            "csv",
            path,
            "result is not an array",
            "use `--json` for the canonical result envelope",
        ));
    };

    let inferred = rows.first().and_then(|row| match row {
        Value::Object(fields) => Some(fields),
        _ => None,
    });
    let inferred_columns = inferred
        .filter(|fields| {
            fields.len() != columns.len()
                || fields
                    .iter()
                    .zip(columns)
                    .any(|((name, _), column)| name != &column.name)
        })
        .map(|fields| {
            fields
                .iter()
                .map(|(name, _)| Column {
                    name: name.clone(),
                    kind: ColumnKind::String,
                    nullable: true,
                })
                .collect::<Vec<_>>()
        });
    let columns = inferred_columns.as_deref().unwrap_or(columns);
    let mut lines = Vec::new();
    lines.push(
        columns
            .iter()
            .map(|column| csv_escape(&column.name))
            .collect::<Vec<_>>()
            .join(","),
    );
    for row in rows {
        let Value::Object(fields) = row else {
            return Err(DbError::new(
                "E2600",
                "csv",
                path,
                "a result row is not an object",
                "use `--json` for nested result values",
            ));
        };
        let line = columns
            .iter()
            .map(|column| {
                let value = fields
                    .iter()
                    .find(|(name, _)| name == &column.name)
                    .map(|(_, value)| value);
                csv_escape(value.map(value_text).unwrap_or_default().as_str())
            })
            .collect::<Vec<_>>()
            .join(",");
        lines.push(line);
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn render_plan(source: &str, query: &sql_kernel::JetSqlQuery) -> String {
    let mut steps = vec![format!("scan {source}")];
    if let Some((field, operator, value)) = &query.filter {
        steps.push(format!("filter {field} {operator} {value}"));
    }
    if !query.group_by.is_empty() {
        let keys = query
            .group_by
            .iter()
            .map(|key| match key {
                sql_kernel::JetSqlGroupKey::Field(field) => field.clone(),
                sql_kernel::JetSqlGroupKey::Ordinal(ordinal) => format!("#{ordinal}"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        steps.push(format!("group {keys}"));
    }
    let projection = query
        .projection
        .iter()
        .map(|item| match item {
            sql_kernel::JetSqlSelectItem::Wildcard => "*".to_string(),
            sql_kernel::JetSqlSelectItem::Field { field, alias } => alias
                .as_ref()
                .map(|alias| format!("{field} AS {alias}"))
                .unwrap_or_else(|| field.clone()),
            sql_kernel::JetSqlSelectItem::Aggregate {
                function,
                field,
                alias,
            } => {
                let function = match function {
                    sql_kernel::JetSqlAggregate::Count => "COUNT",
                    sql_kernel::JetSqlAggregate::Sum => "SUM",
                    sql_kernel::JetSqlAggregate::Avg => "AVG",
                    sql_kernel::JetSqlAggregate::Min => "MIN",
                    sql_kernel::JetSqlAggregate::Max => "MAX",
                };
                let argument = field.as_deref().unwrap_or("*");
                let expression = format!("{function}({argument})");
                alias
                    .as_ref()
                    .map(|alias| format!("{expression} AS {alias}"))
                    .unwrap_or(expression)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    steps.push(format!("project {projection}"));
    if let Some((field, descending)) = &query.order {
        steps.push(format!("sort {field} {}", if *descending { "desc" } else { "asc" }));
    }
    if let Some(limit) = query.limit {
        steps.push(format!("limit {limit}"));
    }
    steps.join(" -> ")
}

fn handle_dot(
    input: &str,
    source: Option<&LoadedSource>,
    state: &DbState,
    options: &DbOptions,
    mode: OutputMode,
) -> Result<bool, DbError> {
    let mut parts = input.splitn(2, char::is_whitespace);
    let command = parts.next().unwrap_or(input);
    let argument = parts.next().map(str::trim).filter(|value| !value.is_empty());
    match command {
        ".quit" | ".exit" => Ok(false),
        ".help" => {
            if mode.json {
                write_db_status(
                    mode,
                    "help",
                    true,
                    StatusFields::new().with("usage", usage()),
                );
            } else {
                write_mode_renderable(mode, usage());
            }
            Ok(true)
        }
        ".tables" => {
            let source = source.ok_or_else(|| {
                DbError::new(
                    "E2600",
                    "tables",
                    "",
                    "no source is loaded",
                    "pass a local source path first",
                )
            })?;
            if mode.json {
                write_db_status(
                    mode,
                    "tables",
                    true,
                    StatusFields::new().with(
                        "tables",
                        StatusValue::array([StatusValue::from(source.name.as_str())]),
                    ),
                );
            } else {
                write_mode_renderable(mode, &format!("{}\n", source.name));
            }
            Ok(true)
        }
        ".schema" => {
            let source = source.ok_or_else(|| {
                DbError::new(
                    "E2600",
                    "schema",
                    "",
                    "no source is loaded",
                    "pass a local source path first",
                )
            })?;
            if mode.json {
                write_db_status(
                    mode,
                    "schema",
                    true,
                    StatusFields::new()
                        .with("source", source.name.as_str())
                        .with("rows", source.rows)
                        .with("columns", columns_value(&source.columns)),
                );
            } else {
                for column in &source.columns {
                    write_mode_renderable(
                        mode,
                        &format!(
                            "{}: {}{}\n",
                            column.name,
                            column.kind.jet_name(),
                            if column.nullable { "?" } else { "" }
                        ),
                    );
                }
            }
            Ok(true)
        }
        ".plan" => {
            let plan = state.last_plan.as_deref().ok_or_else(|| {
                DbError::new(
                    "E2600",
                    "plan",
                    "",
                    "no query plan is available",
                    "run a SELECT query before `.plan`",
                )
            })?;
            if mode.json {
                write_db_status(
                    mode,
                    "plan",
                    true,
                    StatusFields::new().with("plan", plan),
                );
            } else {
                write_mode_renderable(mode, &format!("{plan}\n"));
            }
            Ok(true)
        }
        ".stats" => {
            let statement = state.last_sql.as_deref().unwrap_or("");
            if mode.json {
                write_db_status(
                    mode,
                    "stats",
                    true,
                    StatusFields::new()
                        .with("statement", statement)
                        .with("rows", state.last_rows)
                        .with(
                            "elapsed_ms",
                            StatusValue::from(
                                i128::try_from(state.last_elapsed_ms).unwrap_or(i128::MAX),
                            ),
                        )
                        .with("output_bytes", state.last_output_bytes),
                );
            } else {
                write_mode_renderable(
                    mode,
                    &format!(
                        "statement: {statement}\nrows: {} time: {}ms output: {} bytes\n",
                        state.last_rows, state.last_elapsed_ms, state.last_output_bytes
                    ),
                );
            }
            Ok(true)
        }
        ".export" => {
            let argument = argument.ok_or_else(|| {
                DbError::new(
                    "E2104",
                    "export",
                    "",
                    "`.export` needs a destination path",
                    "use `.export report.json json` or `.export report.csv csv`",
                )
            })?;
            export_result(argument, source, state, options, mode)
        }
        _ => Err(DbError::new(
            "E2104",
            "dot-command",
            "",
            format!("unknown db console command `{command}`"),
            "use `.help`, `.tables`, `.schema`, `.plan`, `.stats`, `.export`, or `.quit`",
        )),
    }
}

fn export_result(
    argument: &str,
    source: Option<&LoadedSource>,
    state: &DbState,
    options: &DbOptions,
    mode: OutputMode,
) -> Result<bool, DbError> {
    if !options.allow_write || options.deny_write {
        return Err(DbError::new(
            "E1803",
            "export",
            argument,
            "export writes need explicit FS.Write authority",
            "start `jet db --allow=FS.Write` (and `--allow=FS.Read` to load a source); denied writes never fall back to stdout",
        ));
    }
    let mut parts = argument.split_whitespace();
    let path = parts.next().unwrap_or_default();
    if path.is_empty() {
        return Err(DbError::new("E2104", "export", "", "`.export` needs a destination path", "use `.export report.json json`"));
    }
    let format = parts.next().unwrap_or("json");
    let result = state.last_result.as_deref().ok_or_else(|| DbError::new("E2600", "export", path, "no query result is available", "run a SELECT query before exporting"))?;
    let bytes = match format {
        "json" => result.to_string(),
        "csv" => {
            let source = source.ok_or_else(|| DbError::new("E2600", "export", path, "CSV export needs a loaded source schema", "load a typed source before exporting"))?;
            result_csv(result, &source.columns, path)?
        }
        _ => return Err(DbError::new("E2104", "export", path, format!("unknown export format `{format}`"), "choose json or csv")),
    };
    if bytes.len() > options.max_output_bytes {
        return Err(DbError::new(
            "E2603",
            "export",
            path,
            format!(
                "export output is {} bytes, over the {}-byte output limit",
                bytes.len(),
                options.max_output_bytes
            ),
            "raise `--max-output-bytes` or export a smaller query result",
        ));
    }
    let destination = PathBuf::from(path);
    fs::write(&destination, bytes.as_bytes()).map_err(|error| DbError::new("E2105", "export", path, format!("could not write `{path}`: {error}"), "check the destination and explicit FS authority"))?;
    if mode.json {
        write_db_status(
            mode,
            "export",
            true,
            StatusFields::new()
                .with("path", path)
                .with("bytes", bytes.len()),
        );
    } else {
        write_mode_status(mode, &format!("exported {} bytes to {path}\n", bytes.len()));
    }
    Ok(true)
}

fn split_console_inputs(text: &str) -> Result<Vec<String>, String> {
    let mut inputs = Vec::new();
    let mut pending = String::new();
    for line in text.lines() {
        if pending.trim().is_empty() && line.trim_start().starts_with('.') {
            inputs.push(line.trim().to_string());
            continue;
        }
        pending.push_str(line);
        pending.push('\n');
        if has_complete_sql_statement(&pending) {
            inputs.extend(split_sql_statements(&pending)?);
            pending.clear();
        }
    }
    if !pending.trim().is_empty() {
        inputs.extend(split_sql_statements(&pending)?);
    }
    Ok(inputs)
}

fn has_complete_sql_statement(text: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_i32;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            escaped = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                if characters.peek() == Some(&delimiter) {
                    characters.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' => depth -= 1,
            ';' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn split_sql_statements(text: &str) -> Result<Vec<String>, String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut depth = 0_i32;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if quote.is_some() && character == '\\' {
            current.push(character);
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            current.push(character);
            if character == delimiter {
                if characters.peek() == Some(&delimiter) {
                    current.push(characters.next().unwrap_or(delimiter));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                current.push(character);
            }
            '(' => {
                depth += 1;
                current.push(character);
            }
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("SQL statement closes a parenthesis that it did not open".to_string());
                }
                current.push(character);
            }
            ';' if depth == 0 => {
                if !current.trim().is_empty() {
                    statements.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(character),
        }
    }
    if quote.is_some() {
        return Err("SQL string literal is missing its closing quote".to_string());
    }
    if depth != 0 {
        return Err("SQL statement has unbalanced parentheses".to_string());
    }
    if !current.trim().is_empty() {
        statements.push(current.trim().to_string());
    }
    Ok(statements)
}

fn is_remote(value: &str) -> bool {
    value.contains("://") || value.starts_with("postgres:") || value.starts_with("mysql:")
}

fn source_name_from_path(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("source");
    let mut name = raw
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() || character == '_' { character } else { '_' })
        .collect::<String>();
    if name.is_empty() {
        name.push_str("source");
    }
    if name.chars().next().is_some_and(|character| character.is_ascii_digit()) {
        name.insert_str(0, "table_");
    }
    name
}

fn valid_table_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '$'))
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}

fn valid_column_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}
