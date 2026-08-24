//! D-DEVR-TRY1=A: speculate over an edit overlay, then keep only explicitly.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::{Diagnostic, Severity, Span, TextEdit};
use jet::ExitCodes;
use jet_foundation::JSON::{json_escape, JSONValue};
use jet_semindex::{SemanticOp, SemanticOpTarget, SourceSpan};

#[derive(Clone)]
struct TryPlan {
    name: String,
    entry: PathBuf,
    action: Action,
}

#[derive(Clone)]
enum Action {
    Rename { from: String, to: String },
    Edits(Vec<EditPlan>),
}

#[derive(Clone)]
struct EditPlan {
    path: PathBuf,
    edit: TextEdit,
}

#[derive(Clone)]
struct Change {
    path: PathBuf,
    before: Vec<u8>,
    after: Vec<u8>,
}

struct Verdict {
    diagnostics: Vec<Diagnostic>,
    claims_rechecked: u64,
    claims_reused: u64,
}

pub(crate) fn run_try(raw: &str, keep: bool, json: bool) {
    let path = absolutize(raw);
    let plan = read_plan(&path);
    let staged = jet::run_compiler_work(|| stage(&plan));
    if staged.is_empty() {
        fail("try plan produced no edit")
    }

    let entry_source = staged
        .iter()
        .find(|change| same_path(&change.path, &plan.entry))
        .map(|change| String::from_utf8(change.after.clone()))
        .transpose()
        .unwrap_or_else(|_| fail("try overlay contains non-UTF-8 source"))
        .unwrap_or_else(|| {
            fs::read_to_string(&plan.entry).unwrap_or_else(|e| {
                fail(&format!(
                    "could not read try entry `{}`: {e}",
                    plan.entry.display()
                ))
            })
        });

    let verdict = jet::run_compiler_work(|| verdict(&plan.entry, &staged, &entry_source));
    let has_errors = verdict
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let receipt_id = receipt_id(&plan.name, &staged, &verdict);

    if has_errors {
        emit_failure(
            &plan,
            &entry_source,
            &verdict.diagnostics,
            &receipt_id,
            json,
        );
        exit(ExitCodes::USER_ERROR);
    }

    if keep {
        verify_current_bytes(&staged);
        let semantic_ops = semantic_ops_for_action(&plan);
        for change in &staged {
            // The existing fix transaction owns atomic replacement and its
            // replay checkpoint. A speculative keep uses that same writer;
            // the default path never enters it.
            crate::CmdCodemod::commit_fix_with_semantic_ops(
                &change.path,
                change.before.clone(),
                change.after.clone(),
                &semantic_ops,
            );
        }
    }

    emit_success(&plan, &verdict, &receipt_id, keep, json);
}

fn read_plan(path: &Path) -> TryPlan {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        fail(&format!(
            "could not read try plan `{}`: {e}",
            path.display()
        ))
    });
    let value = jet_foundation::JSON::parse(&text)
        .unwrap_or_else(|e| fail(&format!("invalid try plan JSON: {e}")));
    let mut object = match value {
        JSONValue::Object(object) => object,
        _ => fail("try plan must be a JSON object"),
    };

    if let Some(version) = object.remove("version") {
        if !matches!(version, JSONValue::Number(1)) {
            fail("unsupported try plan version; expected version 1")
        }
    }
    let name = take_string_default(&mut object, "name", "Try");
    let entry = resolve_against(path.parent(), &take_string(&mut object, "entry"));
    let action = match take_string_opt(&mut object, "operation").as_deref() {
        Some("rename") => {
            let from = take_string(&mut object, "from");
            let to = take_string(&mut object, "to");
            validate_ident(&from);
            validate_ident(&to);
            Action::Rename { from, to }
        }
        Some("text_edit") => Action::Edits(take_edits(&mut object, path)),
        Some(other) => fail(&format!("try plan operation `{other}` is not supported")),
        None if object.contains_key("edits") => Action::Edits(take_edits(&mut object, path)),
        None => fail("try plan needs `operation: \"rename\"` or `edits`"),
    };
    if !object.is_empty() {
        fail(&format!(
            "unknown field(s) in try plan: {}",
            object.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
    }
    TryPlan {
        name,
        entry,
        action,
    }
}

fn take_edits(object: &mut BTreeMap<String, JSONValue>, plan_path: &Path) -> Vec<EditPlan> {
    let values = match object.remove("edits") {
        Some(JSONValue::Array(values)) => values,
        Some(_) => fail("`edits` must be an array"),
        None => fail("try plan is missing `edits`"),
    };
    if values.is_empty() {
        fail("try plan needs at least one edit")
    }
    values
        .into_iter()
        .map(|value| {
            let mut object = match value {
                JSONValue::Object(object) => object,
                _ => fail("each try edit must be an object"),
            };
            let path = resolve_against(plan_path.parent(), &take_string(&mut object, "path"));
            let start = take_number(&mut object, "start");
            let end = take_number(&mut object, "end");
            if end < start {
                fail("try edit `end` must not be before `start`")
            }
            let new_text = take_string(&mut object, "new_text");
            if !object.is_empty() {
                fail(&format!(
                    "unknown field(s) in try edit: {}",
                    object.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
            }
            EditPlan {
                path,
                edit: TextEdit {
                    span: Span::new(start, end),
                    new_text,
                },
            }
        })
        .collect()
}

fn stage(plan: &TryPlan) -> Vec<Change> {
    match &plan.action {
        Action::Rename { from, to } => stage_rename(&plan.entry, from, to),
        Action::Edits(edits) => stage_edits(edits),
    }
}

fn semantic_ops_for_action(plan: &TryPlan) -> Vec<SemanticOp> {
    let Action::Rename { from, to } = &plan.action else {
        return Vec::new();
    };
    let index = jet_semindex::open(&plan.entry).unwrap_or_else(|_| fail("try rename could not rebuild its semantic index"));
    let targets = index
        .definition_facts()
        .iter()
        .filter(|fact| fact.name == *from)
        .map(|fact| SemanticOpTarget {
            stable_id: fact.stable_id.clone(),
            before: fact.human_identity.clone(),
            after: format!("{}::{to}", fact.module_path),
            kind: fact.kind.clone(),
            module_path: fact.module_path.clone(),
        })
        .collect();
    vec![SemanticOp {
        kind: "rename".to_string(),
        rule_id: Some(plan.name.clone()),
        from: Some(from.clone()),
        to: Some(to.clone()),
        node: None,
        match_template: None,
        replace_template: None,
        targets,
        files: Vec::new(),
    }]
}

fn stage_rename(entry: &Path, from: &str, to: &str) -> Vec<Change> {
    let index = jet_semindex::open(entry).unwrap_or_else(|error| {
        let source = fs::read_to_string(entry).unwrap_or_default();
        match error {
            jet_semindex::SemIndexError::Load(diagnostics) => eprint!(
                "{}",
                jet::render_diagnostics(&entry.display().to_string(), &source, &diagnostics)
            ),
        }
        exit(ExitCodes::USER_ERROR)
    });
    let mut edits = BTreeMap::<PathBuf, Vec<TextEdit>>::new();
    for definition in index
        .definitions()
        .iter()
        .filter(|definition| definition.name == from)
    {
        edits
            .entry(canonicalish(Path::new(&definition.module_path)))
            .or_default()
            .push(edit(definition.def_span, to));
    }
    for reference in index
        .references()
        .iter()
        .filter(|reference| reference.name == from)
    {
        edits
            .entry(canonicalish(Path::new(&reference.module_path)))
            .or_default()
            .push(edit(reference.span, to));
    }
    if edits.is_empty() {
        fail(&format!("try plan found no `{from}` symbols"))
    }
    let edits = edits
        .into_iter()
        .map(|(path, edits)| (path, edits))
        .collect::<Vec<_>>();
    stage_file_edits(&edits)
}

fn stage_edits(edits: &[EditPlan]) -> Vec<Change> {
    let mut grouped = BTreeMap::<PathBuf, Vec<TextEdit>>::new();
    for edit in edits {
        grouped
            .entry(canonicalish(&edit.path))
            .or_default()
            .push(edit.edit.clone());
    }
    let grouped = grouped.into_iter().collect::<Vec<_>>();
    stage_file_edits(&grouped)
}

fn stage_file_edits(edits: &[(PathBuf, Vec<TextEdit>)]) -> Vec<Change> {
    edits
        .iter()
        .map(|(path, edits)| {
            let before = fs::read(path)
                .unwrap_or_else(|e| fail(&format!("could not read `{}`: {e}", path.display())));
            let source = std::str::from_utf8(&before)
                .unwrap_or_else(|_| fail(&format!("try target `{}` is not UTF-8", path.display())));
            let after = jet::FixEngine::apply_edits(source, edits)
                .unwrap_or_else(|_| {
                    fail(&format!(
                        "try edits overlap or fall outside `{}`",
                        path.display()
                    ))
                })
                .into_bytes();
            if before == after {
                fail(&format!("try edit leaves `{}` unchanged", path.display()))
            }
            Change {
                path: path.clone(),
                before,
                after,
            }
        })
        .collect()
}

fn verdict(entry: &Path, changes: &[Change], entry_source: &str) -> Verdict {
    let entry_text = entry.to_string_lossy().into_owned();
    let mut queries = jet_driver::QueryService::CompilerQueries::new();
    let _base = queries.check_disk(&entry_text, false);
    let before = queries.stats();
    for change in changes {
        let source = std::str::from_utf8(&change.after)
            .unwrap_or_else(|_| fail("try overlay contains non-UTF-8 source"));
        let path = change.path.to_string_lossy().into_owned();
        queries.set_document(&path, source);
    }
    let checked = queries.check_text(&entry_text, entry_source, false);
    let after = queries.stats();
    Verdict {
        diagnostics: checked.diagnostics.as_ref().clone(),
        claims_rechecked: after.item_recomputes.saturating_sub(before.item_recomputes),
        claims_reused: after.item_hits.saturating_sub(before.item_hits),
    }
}

fn verify_current_bytes(changes: &[Change]) {
    for change in changes {
        let current = fs::read(&change.path).unwrap_or_else(|e| {
            fail(&format!(
                "could not re-read `{}`: {e}",
                change.path.display()
            ))
        });
        if current != change.before {
            fail(&format!(
                "try target `{}` changed while it was being checked; no files written",
                change.path.display()
            ))
        }
    }
}

fn emit_success(plan: &TryPlan, verdict: &Verdict, receipt: &str, keep: bool, json: bool) {
    if json {
        println!(
            "{{\"schema_version\":1,\"command\":\"try\",\"name\":\"{}\",\"status\":\"{}\",\"kept\":{},\"claims_rechecked\":{},\"claims_reused\":{},\"receipt_id\":\"{}\"}}",
            json_escape(&plan.name),
            if keep { "kept" } else { "rolled_back" },
            keep,
            verdict.claims_rechecked,
            verdict.claims_reused,
            receipt
        );
        return;
    }
    println!("try `{}`: verdict clean", plan.name);
    println!(
        "  claims re-checked: {} · claims reused: {}",
        verdict.claims_rechecked, verdict.claims_reused
    );
    println!("  receipt: {receipt}");
    if keep {
        println!("  kept: working tree now contains the staged bytes");
    } else {
        println!("  rolled back (default): working tree is byte-identical");
    }
}

fn emit_failure(
    plan: &TryPlan,
    entry_source: &str,
    diagnostics: &[Diagnostic],
    receipt: &str,
    json: bool,
) {
    if json {
        println!(
            "{{\"schema_version\":1,\"command\":\"try\",\"name\":\"{}\",\"status\":\"rolled_back\",\"kept\":false,\"verdict\":\"failed\",\"receipt_id\":\"{}\"}}",
            json_escape(&plan.name),
            receipt
        );
    } else {
        eprintln!("try `{}`: verdict failed; rolled back", plan.name);
        eprint!(
            "{}",
            jet::render_diagnostics(&plan.entry.display().to_string(), entry_source, diagnostics)
        );
    }
}

fn receipt_id(name: &str, changes: &[Change], verdict: &Verdict) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(&verdict.claims_rechecked.to_le_bytes());
    bytes.extend_from_slice(&verdict.claims_reused.to_le_bytes());
    for change in changes {
        bytes.extend_from_slice(change.path.to_string_lossy().as_bytes());
        bytes.extend_from_slice(&change.before);
        bytes.extend_from_slice(&change.after);
    }
    let digest = jet::SHA256::sha256_hex(&bytes);
    format!("try-{}", &digest[..12])
}

fn edit(span: SourceSpan, text: &str) -> TextEdit {
    TextEdit {
        span: Span::new(span.start, span.end),
        new_text: text.into(),
    }
}

fn take_string(object: &mut BTreeMap<String, JSONValue>, key: &str) -> String {
    match object.remove(key) {
        Some(JSONValue::String(value)) => value,
        Some(_) => fail(&format!("`{key}` must be text")),
        None => fail(&format!("try plan is missing `{key}`")),
    }
}

fn take_string_opt(object: &mut BTreeMap<String, JSONValue>, key: &str) -> Option<String> {
    match object.remove(key) {
        Some(JSONValue::String(value)) => Some(value),
        Some(_) => fail(&format!("`{key}` must be text")),
        None => None,
    }
}

fn take_string_default(
    object: &mut BTreeMap<String, JSONValue>,
    key: &str,
    default: &str,
) -> String {
    take_string_opt(object, key).unwrap_or_else(|| default.into())
}

fn take_number(object: &mut BTreeMap<String, JSONValue>, key: &str) -> usize {
    match object.remove(key) {
        Some(JSONValue::Number(value)) if value >= 0 => value as usize,
        Some(_) => fail(&format!("`{key}` must be a non-negative integer")),
        None => fail(&format!("try edit is missing `{key}`")),
    }
}

fn validate_ident(name: &str) {
    let mut chars = name.chars();
    if !chars
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        || !chars.all(|character| character == '_' || character.is_alphanumeric())
    {
        fail(&format!("`{name}` is not a Jet identifier"))
    }
}

fn absolutize(raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|e| fail(&format!("could not resolve current directory: {e}")))
            .join(path)
    }
}

fn resolve_against(parent: Option<&Path>, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        parent.unwrap_or_else(|| Path::new(".")).join(path)
    }
}

fn canonicalish(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn same_path(left: &Path, right: &Path) -> bool {
    canonicalish(left) == canonicalish(right)
}

fn fail(message: &str) -> ! {
    crate::cli_error!("E2105", "{message}");
    exit(ExitCodes::USER_ERROR)
}
