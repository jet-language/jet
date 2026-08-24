//! Card #1895: cross-run runtime-witness ledger and conservative repairs.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet_foundation::Report::render_status_json;
use jet_foundation::JSON::{json_escape, parse_json, JSONValue};

use crate::OutputMode;

const LEDGER_SCHEMA: &str = "jet.memory.ledger";
const LEDGER_VERSION: u64 = 1;
const MAX_LEDGER_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LEDGER_ROWS: usize = 65_536;
const MAX_FIELD_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Row {
    kind: String,
    code: String,
    source: String,
    span_start: u64,
    span_end: u64,
    byte_spans: bool,
    scope: String,
    provenance: String,
    detail: String,
    expected: Option<String>,
    repairs: Vec<String>,
}

pub(crate) fn configure_ledger() {
    if std::env::var_os("JET_MEMORY_LEDGER").is_none() {
        let root = project_root();
        std::env::set_var("JET_MEMORY_LEDGER", ledger_path(&root));
    }
}

pub(crate) fn audit(args: &[String], mode: OutputMode) {
    reject_arguments(args, "audit memory", mode);
    let rows = load(mode);
    if mode.json {
        println!("{}", render_audit_json(&rows));
    } else {
        let color = mode.color_stderr_for(std::io::stdout().is_terminal());
        print!("{}", render_audit_text(&rows, color));
    }
}

pub(crate) fn fix(args: &[String], mode: OutputMode) {
    reject_arguments(args, "fix memory", mode);
    let dry_run = args.iter().any(|argument| argument == "--dry-run");
    let root = project_root();
    let rows = load(mode);
    let mut candidates: BTreeMap<PathBuf, Vec<&Row>> = BTreeMap::new();
    let mut options = Vec::new();
    for row in &rows {
        if row.byte_spans && row.expected.is_some() && row.repairs.len() == 1 {
            if let Some(path) = repair_path(&root, &row.source) {
                candidates.entry(path).or_default().push(row);
            } else {
                options.push(row);
            }
        } else {
            options.push(row);
        }
    }

    let mut applied = Vec::new();
    for (path, mut edits) in candidates {
        let mut source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(_) => {
                options.extend(edits);
                continue;
            }
        };
        edits.sort_by_key(|row| std::cmp::Reverse((row.span_start, row.span_end)));
        let mut last_start = u64::MAX;
        let mut changed = false;
        for row in edits {
            let Ok(start) = usize::try_from(row.span_start) else {
                options.push(row);
                continue;
            };
            let Ok(end) = usize::try_from(row.span_end) else {
                options.push(row);
                continue;
            };
            let expected = row
                .expected
                .as_deref()
                .expect("candidate has expected text");
            let replacement = &row.repairs[0];
            if row.span_end > last_start {
                options.push(row);
                continue;
            }
            let replacement_end = start.checked_add(replacement.len());
            if start <= source.len()
                && source.is_char_boundary(start)
                && replacement_end.is_some_and(|end| {
                    end <= source.len()
                        && source.is_char_boundary(end)
                        && &source[start..end] == replacement
                })
            {
                applied.push((row, false));
                last_start = row.span_start;
                continue;
            }
            if start > end
                || end > source.len()
                || !source.is_char_boundary(start)
                || !source.is_char_boundary(end)
                || &source[start..end] != expected
            {
                options.push(row);
                continue;
            }
            source.replace_range(start..end, replacement);
            changed = true;
            applied.push((row, true));
            last_start = row.span_start;
        }
        if changed && !dry_run {
            write_atomic(&path, &source).unwrap_or_else(|error| {
                fail(
                    format!(
                        "cannot apply memory repair to `{}`: {error}",
                        path.display()
                    ),
                    mode,
                )
            });
        }
    }

    if mode.json {
        let applied_json = applied
            .iter()
            .map(|(row, changed)| {
                format!(
                    "{{\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"repair\":\"{}\",\"changed\":{}}}",
                    json_escape(&row.source),
                    row.span_start,
                    row.span_end,
                    json_escape(&row.repairs[0]),
                    *changed && !dry_run,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let payload = format!(
            "{{\"dry_run\":{},\"applied\":[{}],\"options\":{}}}",
            dry_run,
            applied_json,
            options.len(),
        );
        println!(
            "{}",
            render_status_json("ok", true, "fix.memory", &format!(",\"memory\":{payload}"))
        );
        return;
    }

    println!("jet fix memory");
    println!("coverage: exercised runs only");
    if applied.is_empty() {
        println!("applied: none");
    } else {
        for (row, changed) in &applied {
            let action = if dry_run && *changed {
                "would apply"
            } else if *changed {
                "applied"
            } else {
                "already applied"
            };
            println!(
                "{action}: {}:{}..{} -> {}",
                row.source, row.span_start, row.span_end, row.repairs[0]
            );
        }
    }
    for row in options {
        println!(
            "options: {}:{}..{} {}",
            row.source, row.span_start, row.span_end, row.detail
        );
        if row.repairs.is_empty() {
            println!("  - no safe automatic repair is known");
        } else {
            for repair in &row.repairs {
                println!("  - {repair}");
            }
        }
    }
}

fn load(mode: OutputMode) -> Vec<Row> {
    let root = project_root();
    let path = std::env::var_os("JET_MEMORY_LEDGER")
        .map(PathBuf::from)
        .unwrap_or_else(|| ledger_path(&root));
    read_rows(&path).unwrap_or_else(|error| fail(error, mode))
}

fn reject_arguments(args: &[String], command: &str, mode: OutputMode) {
    if let Some(argument) = args.iter().find(|argument| !argument.starts_with('-')) {
        fail(format!("unexpected {command} argument `{argument}`"), mode);
    }
}

fn project_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    jet::Loader::find_manifest_root(&cwd).unwrap_or(cwd)
}

fn ledger_path(root: &Path) -> PathBuf {
    root.join(".jet").join("memory").join("ledger-v1.jsonl")
}
fn repair_path(root: &Path, source: &str) -> Option<PathBuf> {
    let source = PathBuf::from(source);
    let joined = if source.is_absolute() {
        source
    } else {
        root.join(source)
    };
    let metadata = std::fs::symlink_metadata(&joined).ok()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let canonical_root = std::fs::canonicalize(root).ok()?;
    let canonical_source = std::fs::canonicalize(joined).ok()?;
    canonical_source
        .starts_with(&canonical_root)
        .then_some(canonical_source)
}

fn read_rows(path: &Path) -> Result<Vec<Row>, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("no memory ledger exists at `{}`", path.display())
        } else {
            format!("cannot inspect memory ledger `{}`: {error}", path.display())
        }
    })?;
    if !metadata.file_type().is_file() {
        return Err(format!(
            "memory ledger `{}` is not a regular file",
            path.display()
        ));
    }
    if metadata.len() > MAX_LEDGER_BYTES {
        return Err("memory ledger exceeds its 4 MiB safety limit".to_string());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read memory ledger `{}`: {error}", path.display()))?;
    let mut rows = Vec::new();
    for (index, line) in raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
    {
        if rows.len() >= MAX_LEDGER_ROWS {
            return Err(format!(
                "memory ledger has more than {MAX_LEDGER_ROWS} rows"
            ));
        }
        let value = parse_json(line)
            .map_err(|_| format!("memory ledger row {} is malformed", index + 1))?;
        rows.push(parse_row(&value, index + 1)?);
    }
    rows.sort_by(|left, right| {
        (
            &left.source,
            left.span_start,
            left.span_end,
            &left.kind,
            &left.code,
            &left.provenance,
            &left.detail,
            &left.repairs,
        )
            .cmp(&(
                &right.source,
                right.span_start,
                right.span_end,
                &right.kind,
                &right.code,
                &right.provenance,
                &right.detail,
                &right.repairs,
            ))
    });
    Ok(rows)
}

fn parse_row(value: &JSONValue, line: usize) -> Result<Row, String> {
    let object = match value {
        JSONValue::Object(object) => object,
        _ => return Err(format!("memory ledger row {line} is not an object")),
    };
    if string(object, "schema", line)? != LEDGER_SCHEMA
        || uint(object, "version", line)? != LEDGER_VERSION
    {
        return Err(format!(
            "memory ledger row {line} has an incompatible schema"
        ));
    }
    let span_start = uint(object, "span_start", line)?;
    let span_end = uint(object, "span_end", line)?;
    if span_start > span_end {
        return Err(format!("memory ledger row {line} has a reversed span"));
    }
    let expected = match object.get("expected") {
        Some(JSONValue::Null) | None => None,
        Some(JSONValue::String(value)) if value.len() <= MAX_FIELD_BYTES => Some(value.clone()),
        _ => {
            return Err(format!(
                "memory ledger row {line} has invalid expected text"
            ))
        }
    };
    let repairs = match object.get("repairs") {
        Some(JSONValue::Array(values)) if values.len() <= 16 => values
            .iter()
            .map(|value| match value {
                JSONValue::String(value) if !value.is_empty() && value.len() <= MAX_FIELD_BYTES => {
                    Ok(value.clone())
                }
                _ => Err(format!("memory ledger row {line} has an invalid repair")),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(format!("memory ledger row {line} has invalid repairs")),
    };
    Ok(Row {
        kind: safe_string(object, "kind", line)?,
        code: safe_string(object, "code", line)?,
        source: safe_string(object, "source", line)?,
        span_start,
        span_end,
        byte_spans: boolean(object, "byte_spans", line)?,
        scope: safe_string(object, "scope", line)?,
        provenance: safe_string(object, "provenance", line)?,
        detail: safe_string(object, "detail", line)?,
        expected,
        repairs,
    })
}

fn field<'a>(
    object: &'a std::collections::BTreeMap<String, JSONValue>,
    name: &str,
    line: usize,
) -> Result<&'a JSONValue, String> {
    object
        .get(name)
        .ok_or_else(|| format!("memory ledger row {line} is missing `{name}`"))
}

fn string<'a>(
    object: &'a std::collections::BTreeMap<String, JSONValue>,
    name: &str,
    line: usize,
) -> Result<&'a str, String> {
    match field(object, name, line)? {
        JSONValue::String(value) => Ok(value),
        _ => Err(format!("memory ledger row {line} `{name}` is not text")),
    }
}

fn safe_string(
    object: &std::collections::BTreeMap<String, JSONValue>,
    name: &str,
    line: usize,
) -> Result<String, String> {
    let value = string(object, name, line)?;
    if value.is_empty() || value.len() > MAX_FIELD_BYTES || value.chars().any(char::is_control) {
        return Err(format!(
            "memory ledger row {line} `{name}` is empty, unsafe, or too long"
        ));
    }
    Ok(value.to_string())
}

fn uint(
    object: &std::collections::BTreeMap<String, JSONValue>,
    name: &str,
    line: usize,
) -> Result<u64, String> {
    match field(object, name, line)? {
        JSONValue::Number(value) if *value >= 0 => Ok(*value as u64),
        _ => Err(format!(
            "memory ledger row {line} `{name}` is not a non-negative integer"
        )),
    }
}

fn boolean(
    object: &std::collections::BTreeMap<String, JSONValue>,
    name: &str,
    line: usize,
) -> Result<bool, String> {
    match field(object, name, line)? {
        JSONValue::Bool(value) => Ok(*value),
        _ => Err(format!("memory ledger row {line} `{name}` is not Bool")),
    }
}

fn render_audit_text(rows: &[Row], color: bool) -> String {
    let title = if color {
        "\x1b[1;36mjet audit memory\x1b[0m"
    } else {
        "jet audit memory"
    };
    let mut out = format!(
        "{title}\ncoverage: exercised runs only\nwitnesses: {}\n",
        rows.len()
    );
    if rows.is_empty() {
        out.push_str("\nNo runtime memory witnesses recorded.\n");
        return out;
    }
    for row in rows {
        out.push_str(&format!(
            "\n{}:{}..{}  {} {}\n  scope: {}\n  provenance: {}\n  witness: {}\n",
            row.source,
            row.span_start,
            row.span_end,
            row.kind,
            row.code,
            row.scope,
            row.provenance,
            row.detail
        ));
        if row.repairs.len() == 1 && row.byte_spans && row.expected.is_some() {
            out.push_str(&format!("  static repair: {}\n", row.repairs[0]));
        } else if row.repairs.is_empty() {
            out.push_str("  static repair: none known\n");
        } else {
            out.push_str("  static repair options:\n");
            for repair in &row.repairs {
                out.push_str(&format!("    - {repair}\n"));
            }
        }
    }
    out
}

fn render_audit_json(rows: &[Row]) -> String {
    let count = rows.len();
    let rows = rows
        .iter()
        .map(|row| {
            let repairs = row
                .repairs
                .iter()
                .map(|repair| format!("\"{}\"", json_escape(repair)))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"kind\":\"{}\",\"code\":\"{}\",\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"byte_spans\":{},\"scope\":\"{}\",\"provenance\":\"{}\",\"detail\":\"{}\",\"repairs\":[{}]}}",
                json_escape(&row.kind),
                json_escape(&row.code),
                json_escape(&row.source),
                row.span_start,
                row.span_end,
                row.byte_spans,
                json_escape(&row.scope),
                json_escape(&row.provenance),
                json_escape(&row.detail),
                repairs,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let payload = format!(
        "{{\"coverage\":\"exercised runs only\",\"witnesses\":{},\"rows\":[{}]}}",
        count, rows,
    );
    render_status_json(
        "ok",
        true,
        "audit.memory",
        &format!(",\"memory\":{payload}"),
    )
}

fn write_atomic(path: &Path, source: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("source");
    let temporary = parent.join(format!(".{name}.memory-fix-{}", std::process::id()));
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("cannot inspect source permissions: {error}"))?;
    std::fs::write(&temporary, source)
        .map_err(|error| format!("cannot write temporary repair: {error}"))?;
    std::fs::set_permissions(&temporary, metadata.permissions())
        .map_err(|error| format!("cannot preserve source permissions: {error}"))?;
    std::fs::rename(&temporary, path)
        .map_err(|error| format!("cannot publish repaired source: {error}"))
}

fn fail(detail: String, mode: OutputMode) -> ! {
    let diagnostic =
        jet::Diagnostics::Diagnostic::from_row("E2112", &[("detail", detail.as_str())], None);
    if mode.json {
        print!(
            "{}",
            jet::render_all_json(
                &jet::Diagnostics::ReportPath::from_process(""),
                "",
                &[diagnostic],
            )
        );
    } else {
        eprint!(
            "{}",
            jet::render_all_colored("", "", &[diagnostic], mode.color_stderr())
        );
    }
    exit(jet::ExitCodes::USER_ERROR)
}
