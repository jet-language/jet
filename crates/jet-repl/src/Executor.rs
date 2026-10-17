//! The one build-and-execute session executor (#4720, D-EXEC1/D-EXEC2).
//!
//! `jet repl` (cooked, interactive, Console, Sql, Notebook) and `jet eval`
//! never evaluate Jet in-process. A checked turn is materialized into one
//! ordinary Jet program, which a private child `jet __jet_repl_run_stdin`
//! builds and executes through the normal `jet run` pipeline. Session state is
//! rebuilt by replaying the accepted statements of earlier turns before the
//! current one (so earlier side effects repeat until native in-memory O0
//! lands, the cost D-EXEC2 ratified).
//!
//! Protocol (shared with the Jet-compiled CLI):
//! - stdin: `<byte length>\n`, the materialized program, then the program's
//!   own stdin (lines queued for `input()`; EOF after them).
//! - argv: `--allow=R,…` / `--deny=R,…` invocation rights. The child never
//!   prompts; an undecided right is refused with the same E1803 `jet run`
//!   gives, before anything executes.
//! - stdout/stderr: replayed output, then `@@jet-repl-<nonce>@@turn` on both
//!   streams, then the turn's own output, then length-prefixed value records
//!   on stdout: `@@jet-repl-<nonce>@@value <i> <chars>\n<Display text>\n` and,
//!   when JSON was requested, `@@jet-repl-<nonce>@@json <i> <chars>\n<json>\n`.
//!   Lengths count Unicode scalars (`String.len()`), so user text can neither
//!   forge nor end a record.
//! - a refusal before execution (check, build, authority) is one
//!   `jet.status/v1` diagnostics envelope on stdout and no turn marker.

use crate::Diagnostics::Diagnostic;
use crate::AST::{EnumDef, StructDef, Type, VariantPayload};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::Path;

/// Identifier-safe per-session token for compiler-owned names and markers.
pub fn session_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos
        ^ (u64::from(std::process::id()) << 32)
        ^ COUNTER.fetch_add(1, Ordering::Relaxed).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    format!("{mixed:016x}")
}

/// The compiler-owned binding an echoed expression is stored in.
pub fn echo_name(nonce: &str, step: usize) -> String {
    format!("jet_repl_echo_{nonce}_{step}")
}

/// Session declarations the type-directed JSON renderer can see into.
#[derive(Clone, Copy)]
pub struct TypeCatalog<'a> {
    pub structs: &'a HashMap<String, StructDef>,
    pub enums: &'a HashMap<String, EnumDef>,
}

/// One value the turn exports after its statements ran.
pub struct ReportedValue<'a> {
    pub name: &'a str,
    pub ty: Option<&'a Type>,
}

pub struct TurnProgram<'a> {
    pub nonce: &'a str,
    pub imports: &'a str,
    pub items: &'a str,
    /// Accepted statements of earlier turns, already in materialized form.
    pub replay: &'a [String],
    /// This turn's statements, already in materialized form.
    pub statements: &'a str,
    pub reports: &'a [ReportedValue<'a>],
    /// Also export each value as JSON (the `jet eval --json` contract).
    pub json: bool,
    pub types: TypeCatalog<'a>,
    /// The program's own stdin (queued `input()` lines), after the source.
    pub input: &'a str,
}

/// Invocation rights forwarded to the child.
#[derive(Clone, Debug, Default)]
pub struct Policy {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct TurnValue {
    pub name: String,
    /// Checked Jet type spelling, when sema recorded one.
    pub ty: Option<String>,
    /// The built program's own Display rendering.
    pub shown: String,
    /// Type-directed JSON rendering (only when requested).
    pub json: Option<String>,
}

#[derive(Clone, Debug)]
pub enum TurnFailure {
    /// The child refused before the turn ran: check, build, or authority.
    /// `undecided` lists rights an E1803 refusal left undecided.
    Refused {
        diagnostics: Vec<Diagnostic>,
        undecided: Vec<String>,
    },
    /// The turn started and the program stopped with a failure report.
    Stopped { report: String },
    /// The child was killed by an interrupt.
    Interrupted,
}

#[derive(Clone, Debug, Default)]
pub struct TurnOutcome {
    pub stdout: String,
    pub stderr: String,
    pub values: Vec<TurnValue>,
    pub failure: Option<TurnFailure>,
}

impl TurnOutcome {
    pub fn value(&self, name: &str) -> Option<&TurnValue> {
        self.values.iter().find(|value| value.name == name)
    }
}

fn marker(nonce: &str) -> String {
    format!("@@jet-repl-{nonce}@@")
}

/// True when the accumulated items declare a top-level `fn run`.
fn items_declare_run(items: &str) -> bool {
    let (tokens, _) = crate::Lexer::lex(items);
    tokens.windows(2).any(|pair| {
        matches!(&pair[0].kind, crate::Lexer::TokKind::KwFn)
            && matches!(&pair[1].kind, crate::Lexer::TokKind::Ident(name) if name == "run")
    })
}

/// Rename the free identifier `run` (never a `.run` member) so a session
/// that loaded a program's own `fn run` can still host the turn entry.
fn rename_run(source: &str, replacement: &str) -> String {
    let (tokens, _) = crate::Lexer::lex(source);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (index, token) in tokens.iter().enumerate() {
        let crate::Lexer::TokKind::Ident(name) = &token.kind else {
            continue;
        };
        if name != "run" || token.span.end > source.len() || token.span.start < cursor {
            continue;
        }
        let member = index > 0 && matches!(tokens[index - 1].kind, crate::Lexer::TokKind::Dot);
        if member || &source[token.span.start..token.span.end] != "run" {
            continue;
        }
        out.push_str(&source[cursor..token.span.start]);
        out.push_str(replacement);
        cursor = token.span.end;
    }
    out.push_str(&source[cursor..]);
    out
}

/// The one program text a turn executes as.
pub fn materialize(program: &TurnProgram<'_>) -> String {
    let nonce = program.nonce;
    let mark = marker(nonce);
    let rename = items_declare_run(program.items).then(|| format!("jet_repl_run_{nonce}"));
    let fix = |text: &str| match &rename {
        Some(name) => rename_run(text, name),
        None => text.to_string(),
    };
    let mut out = String::new();
    out.push_str(program.imports);
    out.push_str(&fix(program.items));
    out.push_str("\nfn run() {\n");
    for statement in program.replay {
        out.push_str(&fix(statement));
        out.push('\n');
    }
    out.push_str(&format!("print(\"{mark}turn\")\neprint(\"{mark}turn\")\n"));
    out.push_str(&fix(program.statements));
    out.push('\n');
    for (index, report) in program.reports.iter().enumerate() {
        let shown = format!("jet_repl_s_{nonce}_{index}");
        let count = format!("jet_repl_n_{nonce}_{index}");
        out.push_str(&format!("{shown} :: \"{{{}}}\"\n", report.name));
        out.push_str(&format!("{count} :: {shown}.len()\n"));
        out.push_str(&format!("print(\"{mark}value {index} {{{count}}}\")\nprint({shown})\n"));
        if program.json {
            let json = format!("jet_repl_j_{nonce}_{index}");
            let json_count = format!("jet_repl_m_{nonce}_{index}");
            out.push_str(&format!("{json} := \"\"\n"));
            let mut renderer = JsonRenderer {
                out: &mut out,
                nonce,
                next: 0,
                types: program.types,
            };
            renderer.value(report.ty, report.name, &json, 0);
            out.push_str(&format!("{json_count} :: {json}.len()\n"));
            out.push_str(&format!("print(\"{mark}json {index} {{{json_count}}}\")\nprint({json})\n"));
        }
    }
    out.push_str("}\n");
    out
}

/// Generates Jet statements that append the JSON form of a value to a String
/// binding, mirroring the canonical `CtValue::to_json` shapes: numbers and
/// booleans bare, strings/chars quoted, `()`/absent as `null`, lists as
/// arrays, maps and structs as objects, unit variants as `"Name"`, payload
/// variants as `{"Name":[…]}` / `{"Name":{…}}`, a told failure as
/// `{"err":…}`. Types the session cannot see into (Core opaque types) render
/// their Display text as a JSON string.
struct JsonRenderer<'a, 'b> {
    out: &'a mut String,
    nonce: &'a str,
    next: usize,
    types: TypeCatalog<'b>,
}

const JSON_DEPTH_LIMIT: usize = 12;

impl JsonRenderer<'_, '_> {
    fn fresh(&mut self) -> String {
        self.next += 1;
        format!("jet_repl_t_{}_{}", self.nonce, self.next)
    }

    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn literal(&mut self, target: &str, text: &str) {
        // Jet string literal: escape `\`, `"`, and double the braces.
        let mut escaped = String::new();
        for ch in text.chars() {
            match ch {
                '\\' => escaped.push_str("\\\\"),
                '"' => escaped.push_str("\\\""),
                '{' => escaped.push_str("{{"),
                '}' => escaped.push_str("}}"),
                ch => escaped.push(ch),
            }
        }
        self.line(&format!("{target} += \"{escaped}\""));
    }

    fn value(&mut self, ty: Option<&Type>, expr: &str, target: &str, depth: usize) {
        let Some(ty) = ty else {
            return self.display_string(expr, target);
        };
        if depth > JSON_DEPTH_LIMIT {
            return self.display_string(expr, target);
        }
        match ty {
            Type::Int
            | Type::IntN { .. }
            | Type::InlineRange { .. }
            | Type::Float
            | Type::Float32
            | Type::Bool => self.line(&format!("{target} += \"{{{expr}}}\"")),
            Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE => self.literal(target, "null"),
            Type::Char => {
                self.literal(target, "\"");
                self.line(&format!("{target} += \"{{{expr}}}\""));
                self.literal(target, "\"");
            }
            Type::String => self.string(expr, target),
            Type::Shared(inner) | Type::Tagged { inner, .. } => {
                self.value(Some(inner), expr, target, depth)
            }
            Type::Option(inner) => {
                let held = self.fresh();
                self.line(&format!("if {expr} == .None {{"));
                self.literal(target, "null");
                self.line("} else {");
                self.line(&format!("{held} :: {expr} ?? panic(\"absent\")"));
                self.value(Some(inner), &held, target, depth + 1);
                self.line("}");
            }
            Type::Result { ok, err } => {
                let good = self.fresh();
                let bad = self.fresh();
                self.line(&format!("if {expr} == .Ok({good}) {{"));
                self.value(Some(ok), &good, target, depth + 1);
                self.line(&format!("}} else if {expr} == .Err({bad}) {{"));
                self.literal(target, "{\"err\":");
                self.value(Some(err), &bad, target, depth + 1);
                self.literal(target, "}");
                self.line("}");
            }
            Type::List(elem) | Type::FixedList { elem, .. } => {
                let first = self.fresh();
                let item = self.fresh();
                self.literal(target, "[");
                self.line(&format!("{first} := true"));
                self.line(&format!("loop {item} in {expr} {{"));
                self.line(&format!("if !{first} {{"));
                self.literal(target, ",");
                self.line("}");
                self.line(&format!("{first} = false"));
                self.value(Some(elem), &item, target, depth + 1);
                self.line("}");
                self.literal(target, "]");
            }
            Type::Map { key, value, .. } => {
                let first = self.fresh();
                let k = self.fresh();
                let v = self.fresh();
                self.literal(target, "{");
                self.line(&format!("{first} := true"));
                self.line(&format!("loop ({k}, {v}) in {expr} {{"));
                self.line(&format!("if !{first} {{"));
                self.literal(target, ",");
                self.line("}");
                self.line(&format!("{first} = false"));
                self.value(Some(key), &k, target, depth + 1);
                self.literal(target, ":");
                self.value(Some(value), &v, target, depth + 1);
                self.line("}");
                self.literal(target, "}");
            }
            Type::Tuple(fields) => {
                self.literal(target, "{");
                for (index, (name, field_ty)) in fields.iter().enumerate() {
                    if index > 0 {
                        self.literal(target, ",");
                    }
                    self.literal(target, &format!("\"{name}\":"));
                    self.value(Some(field_ty), &format!("{expr}.{name}"), target, depth + 1);
                }
                self.literal(target, "}");
            }
            Type::Named(name) => {
                if let Some(def) = self
                    .types
                    .structs
                    .get(name)
                    .filter(|def| def.type_params.is_empty())
                {
                    let fields: Vec<(String, Type)> = def
                        .fields
                        .iter()
                        .filter(|field| {
                            field.computed.is_none()
                                && !crate::Syntax::is_memo_storage_name(&field.name)
                        })
                        .map(|field| (field.name.clone(), field.ty.clone()))
                        .collect();
                    self.literal(target, "{");
                    for (index, (field, field_ty)) in fields.iter().enumerate() {
                        if index > 0 {
                            self.literal(target, ",");
                        }
                        self.literal(target, &format!("\"{field}\":"));
                        self.value(Some(field_ty), &format!("{expr}.{field}"), target, depth + 1);
                    }
                    self.literal(target, "}");
                    return;
                }
                if let Some(def) = self
                    .types
                    .enums
                    .get(name)
                    .filter(|def| def.type_params.is_empty())
                {
                    let variants: Vec<(String, VariantPayload)> = def
                        .variants
                        .iter()
                        .map(|variant| (variant.name.clone(), variant.payload.clone()))
                        .collect();
                    for (index, (variant, payload)) in variants.iter().enumerate() {
                        let keyword = if index == 0 { "if" } else { "} else if" };
                        match payload {
                            VariantPayload::Unit => {
                                self.line(&format!("{keyword} {expr} == .{variant} {{"));
                                self.literal(target, &format!("\"{variant}\""));
                            }
                            VariantPayload::Single(inner, _) => {
                                let held = self.fresh();
                                self.line(&format!("{keyword} {expr} == .{variant}({held}) {{"));
                                self.literal(target, &format!("{{\"{variant}\":["));
                                self.value(Some(inner), &held, target, depth + 1);
                                self.literal(target, "]}");
                            }
                            VariantPayload::Named(fields) => {
                                let held: Vec<String> =
                                    fields.iter().map(|_| self.fresh()).collect();
                                self.line(&format!(
                                    "{keyword} {expr} == .{variant}({}) {{",
                                    held.join(", ")
                                ));
                                self.literal(target, &format!("{{\"{variant}\":{{"));
                                for (position, field) in fields.iter().enumerate() {
                                    if position > 0 {
                                        self.literal(target, ",");
                                    }
                                    self.literal(target, &format!("\"{}\":", field.name));
                                    self.value(Some(&field.ty), &held[position], target, depth + 1);
                                }
                                self.literal(target, "}}");
                            }
                        }
                    }
                    if !variants.is_empty() {
                        self.line("}");
                    }
                    return;
                }
                self.display_string(expr, target)
            }
            _ => self.display_string(expr, target),
        }
    }

    fn display_string(&mut self, expr: &str, target: &str) {
        let shown = self.fresh();
        self.line(&format!("{shown} :: \"{{{expr}}}\""));
        self.string(&shown, target);
    }

    fn string(&mut self, expr: &str, target: &str) {
        let ch = self.fresh();
        self.literal(target, "\"");
        self.line(&format!("loop {ch} in {expr}.chars() {{"));
        self.line(&format!("if {ch} == '\"' {{"));
        self.literal(target, "\\\"");
        self.line(&format!("}} else if {ch} == '\\\\' {{"));
        self.literal(target, "\\\\");
        self.line(&format!("}} else if {ch} == '\\n' {{"));
        self.literal(target, "\\n");
        self.line(&format!("}} else if {ch} == '\\t' {{"));
        self.literal(target, "\\t");
        self.line("} else {");
        self.line(&format!("{target} += \"{{{ch}}}\""));
        self.line("}");
        self.line("}");
        self.literal(target, "\"");
    }
}

/// Build and execute one materialized turn in a private child `jet`.
pub fn run_turn(
    program: &TurnProgram<'_>,
    policy: &Policy,
    base_dir: &Path,
) -> io::Result<TurnOutcome> {
    let source = materialize(program);
    let jet = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("jet"));
    let mut command = std::process::Command::new(jet);
    command.arg("__jet_repl_run_stdin");
    if !policy.allow.is_empty() {
        command.arg(format!("--allow={}", policy.allow.join(",")));
    }
    if !policy.deny.is_empty() {
        command.arg(format!("--deny={}", policy.deny.join(",")));
    }
    if base_dir.is_dir() {
        command.current_dir(base_dir);
    }
    let mut child = command
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| {
        io::Error::new(io::ErrorKind::BrokenPipe, "REPL child did not expose its source pipe")
    })?;
    let framed = format!("{}\n{source}", source.len());
    if let Err(error) = stdin.write_all(framed.as_bytes()) {
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    // The program may read none of its queued input; a closed pipe there is
    // not a failure. A writer thread keeps a large queue from deadlocking
    // against the child's own output.
    let input = program.input.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let output = child.wait_with_output();
    let _ = writer.join();
    let output = output?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok(parse_outcome(program, &output.status, &stdout, &stderr))
}

fn interrupted(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        matches!(status.signal(), Some(2 | 9 | 15))
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        false
    }
}

fn after_marker<'s>(text: &'s str, mark: &str) -> Option<&'s str> {
    let needle = format!("{mark}turn\n");
    text.find(&needle).map(|at| &text[at + needle.len()..])
}

/// Read `count` Unicode scalars after `rest`, returning them and the rest
/// past the terminating newline.
fn take_chars(rest: &str, count: usize) -> Option<(&str, &str)> {
    let end = rest
        .char_indices()
        .nth(count)
        .map(|(at, _)| at)
        .unwrap_or(rest.len());
    if rest.chars().count() < count {
        return None;
    }
    let (body, tail) = rest.split_at(end);
    Some((body, tail.strip_prefix('\n').unwrap_or(tail)))
}

fn parse_outcome(
    program: &TurnProgram<'_>,
    status: &std::process::ExitStatus,
    stdout: &str,
    stderr: &str,
) -> TurnOutcome {
    let mark = marker(program.nonce);
    let Some(turn_out) = after_marker(stdout, &mark) else {
        if interrupted(status) {
            return TurnOutcome {
                failure: Some(TurnFailure::Interrupted),
                ..TurnOutcome::default()
            };
        }
        return TurnOutcome {
            failure: Some(refusal(stdout, stderr)),
            ..TurnOutcome::default()
        };
    };
    let header = |kind: &str| format!("{mark}{kind} ");
    let value_header = header("value");
    let json_header = header("json");
    let records_at = if turn_out.starts_with(&value_header) {
        Some(0)
    } else {
        turn_out.find(&format!("\n{value_header}")).map(|at| at + 1)
    };
    let (user_out, mut records) = match records_at {
        Some(at) => (&turn_out[..at], &turn_out[at..]),
        None => (turn_out, ""),
    };
    let mut values: Vec<TurnValue> = Vec::new();
    while !records.is_empty() {
        let (kind, rest) = if let Some(rest) = records.strip_prefix(&value_header) {
            ("value", rest)
        } else if let Some(rest) = records.strip_prefix(&json_header) {
            ("json", rest)
        } else {
            break;
        };
        let Some((head, rest)) = rest.split_once('\n') else {
            break;
        };
        let mut fields = head.split(' ');
        let (Some(Ok(index)), Some(Ok(count))) = (
            fields.next().map(str::parse::<usize>),
            fields.next().map(str::parse::<usize>),
        ) else {
            break;
        };
        let Some((body, tail)) = take_chars(rest, count) else {
            break;
        };
        let Some(report) = program.reports.get(index) else {
            break;
        };
        if kind == "value" {
            values.push(TurnValue {
                name: report.name.to_string(),
                ty: report.ty.map(Type::name),
                shown: body.to_string(),
                json: None,
            });
        } else if let Some(value) = values.iter_mut().find(|value| value.name == report.name) {
            value.json = Some(body.to_string());
        }
        records = tail;
    }
    let turn_err = after_marker(stderr, &mark).unwrap_or(stderr);
    let failure = if status.success() {
        None
    } else if interrupted(status) {
        Some(TurnFailure::Interrupted)
    } else {
        Some(TurnFailure::Stopped {
            report: turn_err.to_string(),
        })
    };
    let (stderr, failure) = match failure {
        Some(TurnFailure::Stopped { report }) => (
            String::new(),
            Some(TurnFailure::Stopped { report }),
        ),
        other => (turn_err.to_string(), other),
    };
    TurnOutcome {
        stdout: user_out.to_string(),
        stderr,
        values,
        failure,
    }
}

/// Read the child's `jet.status/v1` diagnostics envelope.
fn refusal(stdout: &str, stderr: &str) -> TurnFailure {
    let mut diagnostics = Vec::new();
    let mut undecided = Vec::new();
    for line in stdout.lines().filter(|line| line.starts_with('{')) {
        let Ok(tree) = jet_foundation::JSON::parse(line) else {
            continue;
        };
        let Some(reports) = field(&tree, "reports").and_then(array) else {
            continue;
        };
        for report in reports {
            let text = |name: &str| field(report, name).and_then(text).unwrap_or_default();
            if text("severity") != "error" {
                continue;
            }
            let code = text("code");
            if code == "E1803" {
                undecided.extend(undecided_rights(&text("detail")));
            }
            diagnostics.push(Diagnostic::error(
                &code,
                text("what"),
                text("why"),
                text("fix"),
                None,
            ));
        }
    }
    if diagnostics.is_empty() {
        return TurnFailure::Stopped {
            report: format!("{stdout}{stderr}"),
        };
    }
    TurnFailure::Refused {
        diagnostics,
        undecided,
    }
}

fn field<'t>(
    tree: &'t jet_foundation::Dyn::Dyn,
    name: &str,
) -> Option<&'t jet_foundation::Dyn::Dyn> {
    match tree {
        jet_foundation::Dyn::Dyn::Object(fields) => fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value),
        _ => None,
    }
}

fn array(tree: &jet_foundation::Dyn::Dyn) -> Option<&[jet_foundation::Dyn::Dyn]> {
    match tree {
        jet_foundation::Dyn::Dyn::Array(items) => Some(items),
        _ => None,
    }
}

fn text(tree: &jet_foundation::Dyn::Dyn) -> Option<String> {
    match tree {
        jet_foundation::Dyn::Dyn::Text(value) => Some(value.clone()),
        _ => None,
    }
}

/// `authority_facts:required_effects=A, B; granted_effects=…; denied_effects=…`
/// → required rights neither granted nor denied.
fn undecided_rights(detail: &str) -> Vec<String> {
    let list = |key: &str| -> Vec<String> {
        detail
            .split(';')
            .find_map(|part| {
                let part = part.trim();
                let part = part.strip_prefix("authority_facts:").unwrap_or(part);
                part.strip_prefix(key)
            })
            .map(|names| {
                names
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && *name != "none" && *name != "…")
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let granted = list("granted_effects=");
    let denied = list("denied_effects=");
    list("required_effects=")
        .into_iter()
        .filter(|right| {
            let covered = |held: &String| {
                right == held || right.starts_with(&format!("{held}."))
            };
            !granted.iter().any(covered) && !denied.iter().any(covered)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_run_skips_members_and_other_names() {
        assert_eq!(
            rename_run("fn run() { x.run(); running(); run() }", "r9"),
            "fn r9() { x.run(); running(); r9() }"
        );
    }

    #[test]
    fn undecided_rights_drop_granted_and_denied_rows() {
        let detail = "authority_facts:required_effects=FS.Write, IO, Net; granted_effects=Exec.Args, IO, Mem.Alloc; denied_effects=Net";
        assert_eq!(undecided_rights(detail), vec!["FS.Write".to_string()]);
    }
}
