//! D-CODEMOD1 / D-CODEMOD-BATCH1=A: one replayable semantic codemod engine.

#[path = "CmdCodemod/Json.rs"]
mod Json;
#[path = "CmdCodemod/Transaction.rs"]
mod Transaction;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::{Severity, Span, TextEdit};
use jet::ExitCodes;
use jet_semindex::{
    open, open_with_overlays_and_diagnostics, open_with_overlays_diagnostics_and_inputs, SemIndex,
    SemIndexError, SymbolKind,
};
use Json::Value;
use Transaction::Change;

#[derive(Clone)]
struct V1 {
    name: String,
    entry: PathBuf,
    from: String,
    to: String,
}
#[derive(Clone)]
struct Batch {
    name: String,
    project: PathBuf,
    roots: Vec<Root>,
    rules: Vec<Rule>,
    snapshots: BTreeMap<PathBuf, PathBuf>,
}
#[derive(Clone)]
struct Root {
    path: PathBuf,
    validate: Validation,
}
#[derive(Clone, Copy, PartialEq)]
enum Validation {
    Clean,
    Fixture,
}
#[derive(Clone)]
enum Rule {
    Rename {
        id: String,
        name: String,
        kind: String,
        defined_in: Option<PathBuf>,
        to: String,
        matches: usize,
        allow_zero: bool,
    },
    Ast {
        id: String,
        node: String,
        pattern: Template,
        replacement: Template,
        matches: usize,
        allow_zero: bool,
    },
}
#[derive(Clone)]
struct Template {
    atoms: Vec<Atom>,
}
#[derive(Clone, Debug)]
enum Atom {
    Literal(String),
    Capture(String, bool),
}
#[derive(Clone)]
struct Unit {
    entry: PathBuf,
    validation: Validation,
}
#[derive(Clone)]
struct FileState {
    before: Vec<u8>,
    staged: Vec<u8>,
}
struct BatchPlan {
    files: BTreeMap<PathBuf, FileState>,
    counts: Vec<(String, usize)>,
    validations: Vec<String>,
    inputs: BTreeMap<PathBuf, String>,
}

pub(crate) fn run_codemod(args: &[String]) {
    let positional: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .map(String::as_str)
        .collect();
    let yes = args.iter().any(|a| a == "--yes");
    match positional.as_slice() {
        ["dry-run", object] => run_object(object, false, yes),
        ["apply", object] => run_object(object, true, yes),
        ["undo", log] => undo(&absolutize(log)),
        _ => fail("`jet inspect codemod` needs `dry-run`, `apply`, or `undo`\n Fix: jet inspect codemod dry-run migration.codemod.json"),
    }
}

fn run_object(raw: &str, apply: bool, yes: bool) {
    let path = absolutize(raw);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| fail(&format!("could not read codemod `{}`: {e}", path.display())));
    let value = Json::parse(&text).unwrap_or_else(|e| fail(&format!("invalid codemod JSON: {e}")));
    let version = match &value {
        Value::Object(o) => number_default(o, "version", 1),
        _ => fail("codemod object must be a JSON object"),
    };
    if version == 1 {
        run_v1(parse_v1(value, &path), apply);
    } else if version == 2 {
        run_v2(parse_batch(value, &path), apply, yes);
    } else {
        fail(&format!(
            "unsupported codemod version {version}; expected 1 or 2"
        ));
    }
}

fn parse_v1(value: Value, path: &Path) -> V1 {
    let mut o = value.object().unwrap_or_else(|e| fail(&e));
    let _ = take_number_opt(&mut o, "version");
    let name = take_string_default(&mut o, "name", "Codemod");
    let entry = resolve_against(path.parent(), &take_string(&mut o, "entry"));
    let operation = take_string(&mut o, "operation");
    if operation != "rename" {
        fail("version 1 supports only `rename`");
    }
    let from = take_string(&mut o, "from");
    let to = take_string(&mut o, "to");
    reject_unknown(o, "version 1 codemod");
    validate_ident(&from);
    validate_ident(&to);
    V1 {
        name,
        entry,
        from,
        to,
    }
}

fn parse_batch(value: Value, object_path: &Path) -> Batch {
    let mut o = value.object().unwrap_or_else(|e| fail(&e));
    if take_number(&mut o, "version") != 2 {
        fail("batch codemod requires `version: 2`");
    }
    let name = take_string(&mut o, "name");
    if name.trim().is_empty() {
        fail("codemod name cannot be empty")
    }
    let project_raw = take_string(&mut o, "project");
    if Path::new(&project_raw).is_absolute() {
        fail("project must be resolved relative to the codemod object")
    }
    let project = fs::canonicalize(resolve_against(object_path.parent(), &project_raw))
        .unwrap_or_else(|e| fail(&format!("could not canonicalize project: {e}")));
    if !project.is_dir() {
        fail("codemod project must be a directory")
    }
    let root_values = take_array(&mut o, "roots");
    if root_values.is_empty() {
        fail("batch codemod needs at least one root")
    }
    let mut roots = Vec::new();
    for value in root_values {
        let mut r = value
            .object()
            .unwrap_or_else(|_| fail("root must be an object"));
        let raw = take_string(&mut r, "path");
        let path = secure_existing(&project, &raw, true);
        let validation = match take_string(&mut r, "validate").as_str() {
            "clean" => Validation::Clean,
            "fixture" => Validation::Fixture,
            other => fail(&format!("unknown root validation `{other}`")),
        };
        reject_unknown(r, "root");
        roots.push(Root {
            path,
            validate: validation,
        });
    }
    let values = take_array(&mut o, "rules");
    if values.is_empty() {
        fail("batch codemod needs at least one rule")
    }
    let mut ids = BTreeSet::new();
    let mut rules = Vec::new();
    for value in values {
        let mut r = value
            .object()
            .unwrap_or_else(|_| fail("rule must be an object"));
        let id = take_string(&mut r, "id");
        if !ids.insert(id.clone()) {
            fail(&format!("duplicate rule id `{id}`"))
        }
        let kind = take_string(&mut r, "kind");
        let matches = take_number(&mut r, "matches") as usize;
        let allow_zero = take_bool_default(&mut r, "allow_zero", false);
        if matches == 0 && !allow_zero {
            fail(&format!(
                "rule `{id}` declares zero matches without `allow_zero: true`"
            ))
        }
        match kind.as_str() {
            "symbol_rename" => {
                let mut from = take_object(&mut r, "from");
                let name = take_string(&mut from, "name");
                let symbol_kind = take_string(&mut from, "symbol_kind");
                let defined_in = take_string_opt(&mut from, "defined_in")
                    .map(|p| secure_existing(&project, &p, false));
                reject_unknown(from, "symbol selector");
                let to = take_string(&mut r, "to");
                validate_ident(&name);
                validate_ident(&to);
                reject_unknown(r, "symbol_rename rule");
                rules.push(Rule::Rename {
                    id,
                    name,
                    kind: symbol_kind,
                    defined_in,
                    to,
                    matches,
                    allow_zero,
                });
            }
            "ast_rewrite" => {
                let node = take_string(&mut r, "node");
                if !matches!(node.as_str(), "expr" | "stmt" | "item" | "type") {
                    fail(&format!("rule `{id}` has unknown node class `{node}`"))
                }
                let pattern = parse_template(&take_string(&mut r, "match"));
                let replacement = parse_template(&take_string(&mut r, "replace"));
                validate_templates(&id, &pattern, &replacement);
                validate_typed_template(&id, &node, &pattern, "match");
                validate_typed_template(&id, &node, &replacement, "replace");
                reject_unknown(r, "ast_rewrite rule");
                rules.push(Rule::Ast {
                    id,
                    node,
                    pattern,
                    replacement,
                    matches,
                    allow_zero,
                });
            }
            other => fail(&format!("unknown codemod rule kind `{other}`")),
        }
    }
    let mut snapshots = BTreeMap::new();
    if let Some(map) = take_object_opt(&mut o, "snapshot_after") {
        for (raw, input) in map {
            let Value::String(input) = input else {
                fail("snapshot_after values must be paths")
            };
            let dest = secure_destination(&project, &raw);
            let input = secure_existing_project_file(&project, &input);
            if dest == input {
                fail("snapshot_after input may not alias its destination")
            };
            if snapshots.insert(dest, input).is_some() {
                fail("duplicate snapshot_after destination")
            }
        }
    }
    reject_unknown(o, "batch codemod");
    Batch {
        name,
        project,
        roots,
        rules,
        snapshots,
    }
}

fn run_v2(batch: Batch, apply: bool, yes: bool) {
    let _lock = Transaction::lock(&batch.project);
    Transaction::recover(&batch.project);
    let plan = plan_batch(&batch);
    verify_inputs(&plan.inputs);
    print_batch(&batch, &plan);
    if !apply {
        println!("ready: no files written");
        return;
    }
    if !yes {
        eprintln!("warning: an editor that does not honor the codemod lock can still race the final rename\n Fix: review the dry run, then repeat with `--yes`");
        fail("apply requires `--yes`");
    }
    let changes = plan
        .files
        .iter()
        .filter(|(_, s)| s.before != s.staged)
        .map(|(path, s)| Change {
            path: path.clone(),
            before: s.before.clone(),
            after: s.staged.clone(),
        })
        .collect::<Vec<_>>();
    let log_path = log_path(&batch.project, &batch.name);
    let log = render_v2_log(&batch, &changes);
    Transaction::commit(&batch.project, &changes, &log_path, log.as_bytes());
    println!(
        "codemod `{}` applied\n  files: {}\n  log: {}",
        batch.name,
        changes.len(),
        log_path.display()
    );
}

fn plan_batch(batch: &Batch) -> BatchPlan {
    let units = discover_units(batch);
    let mut files = BTreeMap::new();
    let mut inputs = BTreeMap::new();
    for u in &units {
        let bytes = read_fingerprint(&u.entry, &mut inputs);
        files.entry(u.entry.clone()).or_insert(FileState {
            before: bytes.clone(),
            staged: bytes,
        });
        if u.validation == Validation::Fixture {
            let stderr = u.entry.with_extension("stderr");
            let bytes = read_fingerprint(&stderr, &mut inputs);
            files.entry(stderr).or_insert(FileState {
                before: bytes.clone(),
                staged: bytes,
            });
        }
    }
    for input in batch.snapshots.values() {
        read_fingerprint(input, &mut inputs);
    }
    let mut counts = Vec::new();
    for rule in &batch.rules {
        let count = apply_rule(rule, &units, &mut files, &mut inputs);
        let (id, expected, allow) = match rule {
            Rule::Rename {
                id,
                matches,
                allow_zero,
                ..
            }
            | Rule::Ast {
                id,
                matches,
                allow_zero,
                ..
            } => (id, *matches, *allow_zero),
        };
        if count != expected {
            fail(&format!(
                "rule `{id}` matched {count}, expected {expected}; no files written"
            ))
        }
        if count == 0 && !allow {
            fail(&format!("rule `{id}` matched zero nodes; no files written"))
        }
        counts.push((id.clone(), count));
    }
    let validations = validate_units(batch, &units, &mut files, &mut inputs);
    verify_inputs(&inputs);
    BatchPlan {
        files,
        counts,
        validations,
        inputs,
    }
}

fn discover_units(batch: &Batch) -> Vec<Unit> {
    let mut map: BTreeMap<PathBuf, Validation> = BTreeMap::new();
    for root in &batch.roots {
        if root.path.is_file() {
            insert_unit(&mut map, root.path.clone(), root.validate)
        } else {
            walk_jet(&root.path, root.validate, &mut map)
        }
    }
    map.into_iter()
        .map(|(entry, validation)| Unit { entry, validation })
        .collect()
}
fn insert_unit(map: &mut BTreeMap<PathBuf, Validation>, path: PathBuf, v: Validation) {
    if let Some(old) = map.insert(path.clone(), v) {
        if old != v {
            fail(&format!(
                "root `{}` has conflicting validation modes",
                path.display()
            ))
        }
    }
}
fn walk_jet(dir: &Path, v: Validation, out: &mut BTreeMap<PathBuf, Validation>) {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|e| fail(&format!("could not read root `{}`: {e}", dir.display())))
        .map(|e| {
            e.unwrap_or_else(|x| fail(&format!("could not read root entry: {x}")))
                .path()
        })
        .collect::<Vec<_>>();
    entries.sort();
    for p in entries {
        let meta = fs::symlink_metadata(&p)
            .unwrap_or_else(|e| fail(&format!("could not inspect `{}`: {e}", p.display())));
        if meta.file_type().is_symlink() {
            fail(&format!(
                "symlink inside codemod root is not allowed: `{}`",
                p.display()
            ))
        }
        if meta.is_dir() {
            walk_jet(&p, v, out)
        } else if meta.is_file() && p.extension().and_then(|s| s.to_str()) == Some("jet") {
            insert_unit(out, fs::canonicalize(&p).unwrap(), v)
        }
    }
}

fn apply_rule(
    rule: &Rule,
    units: &[Unit],
    files: &mut BTreeMap<PathBuf, FileState>,
    inputs: &mut BTreeMap<PathBuf, String>,
) -> usize {
    match rule {
        Rule::Rename {
            id,
            name,
            kind,
            defined_in,
            to,
            ..
        } => {
            let mut edits: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
            let mut anchors = BTreeSet::new();
            for u in units {
                let idx = index_unit(u, files, inputs);
                if idx.definitions().iter().any(|d| d.name == *to) {
                    fail(&format!(
                        "rule `{id}` rename destination `{to}` already resolves in unit `{}`",
                        u.entry.display()
                    ));
                }
                for d in idx.definitions().iter().filter(|d| {
                    d.name == *name
                        && kind_matches(&d.kind, kind)
                        && defined_in
                            .as_ref()
                            .is_none_or(|p| same_path(Path::new(&d.module_path), p))
                }) {
                    anchors.insert((canonicalish(Path::new(&d.module_path)), d.identity.clone()));
                }
            }
            if anchors.is_empty() {
                return 0;
            }
            for u in units {
                let idx = index_unit(u, files, inputs);
                for d in idx.definitions().iter().filter(|d| {
                    d.name == *name
                        && kind_matches(&d.kind, kind)
                        && anchors
                            .iter()
                            .any(|(p, identity)| {
                                same_path(Path::new(&d.module_path), p)
                                    && d.identity == *identity
                            })
                }) {
                    edits
                        .entry(canonicalish(Path::new(&d.module_path)))
                        .or_default()
                        .push(edit(d.def_span, to));
                }
                for r in idx.references().iter().filter(|r| {
                    r.name == *name
                        && r.target_identity.as_ref().is_some_and(|target| {
                            anchors.iter().any(|(_, identity)| identity == target)
                        })
                }) {
                    let path = canonicalish(Path::new(&r.module_path));
                    if files.contains_key(&path) {
                        edits.entry(path).or_default().push(edit(r.span, to));
                    }
                }
            }
            dedup_apply_edits(id, edits, files)
        }
        Rule::Ast {
            id,
            node,
            pattern,
            replacement,
            ..
        } => {
            let mut edits: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
            for u in units {
                let idx = index_unit(u, files, inputs);
                validate_pattern_bindings(id, pattern, replacement, &idx, &u.entry);
                let state = &files[&u.entry];
                let source = String::from_utf8(state.staged.clone())
                    .unwrap_or_else(|_| fail(&format!("`{}` is not UTF-8", u.entry.display())));
                for found in find_template_matches(pattern, &source, node, &idx, &u.entry) {
                    if !semantic_candidate(pattern, &found, &idx, &u.entry) {
                        continue;
                    }
                    let replacement_text = render_replacement(replacement, &found.captures);
                    edits.entry(u.entry.clone()).or_default().push(TextEdit {
                        span: Span::new(found.start, found.end),
                        new_text: replacement_text,
                    });
                }
            }
            dedup_apply_edits(id, edits, files)
        }
    }
}

fn index_unit(
    unit: &Unit,
    files: &BTreeMap<PathBuf, FileState>,
    inputs: &mut BTreeMap<PathBuf, String>,
) -> SemIndex {
    let texts = files
        .iter()
        .filter(|(p, _)| p.extension().and_then(|s| s.to_str()) == Some("jet"))
        .map(|(p, s)| {
            (
                p.as_path(),
                std::str::from_utf8(&s.staged).unwrap_or_else(|_| fail("Jet source is not UTF-8")),
            )
        })
        .collect::<Vec<_>>();
    let (idx, _diags, module_inputs) =
        open_with_overlays_diagnostics_and_inputs(&unit.entry, &texts)
            .unwrap_or_else(|e| render_index_error(&unit.entry, e));
    for module in module_inputs {
        let p = canonicalish(&module);
        if !files.contains_key(&p) && p.exists() {
            read_fingerprint(&p, inputs);
        }
    }
    idx
}
fn validate_units(
    batch: &Batch,
    units: &[Unit],
    files: &mut BTreeMap<PathBuf, FileState>,
    inputs: &mut BTreeMap<PathBuf, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for u in units {
        let texts = files
            .iter()
            .filter(|(p, _)| p.extension().and_then(|s| s.to_str()) == Some("jet"))
            .map(|(p, s)| (p.as_path(), std::str::from_utf8(&s.staged).unwrap()))
            .collect::<Vec<_>>();
        let (_idx, diags) = open_with_overlays_and_diagnostics(&u.entry, &texts)
            .unwrap_or_else(|e| render_index_error(&u.entry, e));
        let errors = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .cloned()
            .collect::<Vec<_>>();
        match u.validation {
            Validation::Clean => {
                if !errors.is_empty() {
                    fail(&format!(
                        "clean root `{}` has {} diagnostic(s) after rewrite",
                        u.entry.display(),
                        errors.len()
                    ))
                }
                out.push(format!("{} clean", project_rel(&batch.project, &u.entry)));
            }
            Validation::Fixture => {
                if errors.is_empty() {
                    fail(&format!(
                        "fixture `{}` no longer produces a diagnostic",
                        u.entry.display()
                    ))
                }
                let src = std::str::from_utf8(&files[&u.entry].staged).unwrap();
                let rendered =
                    jet::render_diagnostics(&u.entry.display().to_string(), src, &errors);
                let bytes = rendered.into_bytes();
                let stderr = u.entry.with_extension("stderr");
                if let Some(input) = batch.snapshots.get(&stderr) {
                    let expected = fs::read(input)
                        .unwrap_or_else(|e| fail(&format!("could not read snapshot input: {e}")));
                    if bytes != expected {
                        let at = bytes
                            .iter()
                            .zip(&expected)
                            .position(|(a, b)| a != b)
                            .unwrap_or(bytes.len().min(expected.len()));
                        fail(&format!("generated diagnostics for `{}` do not equal snapshot_after `{}` (generated {} bytes {}, expected {} bytes {}, first difference byte {})",u.entry.display(),input.display(),bytes.len(),hash_bytes(&bytes),expected.len(),hash_bytes(&expected),at))
                    }
                    files.get_mut(&stderr).unwrap().staged = expected;
                } else if files[&stderr].before != bytes {
                    fail(&format!("generated diagnostics for `{}` differ from paired snapshot; declare snapshot_after",u.entry.display()))
                }
                out.push(format!(
                    "{} fixture snapshot exact",
                    project_rel(&batch.project, &u.entry)
                ));
            }
        }
        read_fingerprint(&u.entry, inputs);
    }
    out
}

fn dedup_apply_edits(
    id: &str,
    mut by_file: BTreeMap<PathBuf, Vec<TextEdit>>,
    files: &mut BTreeMap<PathBuf, FileState>,
) -> usize {
    let mut count = 0;
    for (path, edits) in &mut by_file {
        edits.sort_by_key(|e| (e.span.start, e.span.end));
        edits.dedup_by(|a, b| a.span == b.span && a.new_text == b.new_text);
        for pair in edits.windows(2) {
            if pair[0].span.end > pair[1].span.start {
                fail(&format!(
                    "rule `{id}` has overlapping edits in `{}`",
                    path.display()
                ))
            }
        }
        let state = files.get_mut(path).unwrap_or_else(|| {
            fail(&format!(
                "rule `{id}` selects read-only import `{}`",
                path.display()
            ))
        });
        let source = std::str::from_utf8(&state.staged).unwrap();
        let after = jet::FixEngine::apply_edits(source, edits).unwrap_or_else(|_| {
            fail(&format!(
                "rule `{id}` has overlapping edits in `{}`",
                path.display()
            ))
        });
        state.staged = after.into_bytes();
        count += edits.len();
    }
    count
}

#[derive(Clone)]
struct LexToken {
    start: usize,
    end: usize,
    text: String,
}
struct Found {
    start: usize,
    end: usize,
    captures: BTreeMap<String, String>,
}
fn lex_template_source(src: &str) -> Vec<LexToken> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b.get(i..i + 2) == Some(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1
            }
            continue;
        }
        if b.get(i..i + 2) == Some(b"/*") {
            let start = i;
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if b.get(i..i + 2) == Some(b"/*") {
                    depth += 1;
                    i += 2
                } else if b.get(i..i + 2) == Some(b"*/") {
                    depth -= 1;
                    i += 2
                } else {
                    i += 1
                }
            }
            if depth > 0 {
                fail(&format!("unterminated comment at byte {start}"))
            }
            continue;
        }
        let start = i;
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2
                } else if b[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1
                }
            }
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' || b[i] >= 128 {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] >= 128) {
                i += 1
            }
        } else if b[i].is_ascii_digit() {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1
            }
        } else {
            let two = b.get(i..i + 2);
            if matches!(
                two,
                Some(b"::")
                    | Some(b"->")
                    | Some(b"=>")
                    | Some(b"==")
                    | Some(b"!=")
                    | Some(b"<=")
                    | Some(b">=")
                    | Some(b"?.")
                    | Some(b"??")
                    | Some(b":=")
                    | Some(b"..")
            ) {
                i += 2
            } else {
                i += 1
            }
        }
        out.push(LexToken {
            start,
            end: i,
            text: src[start..i].to_string(),
        });
    }
    out
}
fn parse_template(src: &str) -> Template {
    let raw = lex_template_source(src);
    let mut atoms = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i].text == "$" {
            let name = raw
                .get(i + 1)
                .unwrap_or_else(|| fail("capture needs a name"))
                .text
                .clone();
            validate_ident(&name);
            let variadic = raw.get(i + 2).is_some_and(|t| t.text == "..")
                && raw.get(i + 3).is_some_and(|t| t.text == ".");
            atoms.push(Atom::Capture(name, variadic));
            i += if variadic { 4 } else { 2 };
        } else {
            atoms.push(Atom::Literal(raw[i].text.clone()));
            i += 1
        }
    }
    if atoms.is_empty() {
        fail("AST template cannot be empty")
    }
    Template { atoms }
}
fn validate_templates(id: &str, m: &Template, r: &Template) {
    let mut declared = BTreeSet::new();
    for a in &m.atoms {
        if let Atom::Capture(n, _) = a {
            declared.insert(n.clone());
        }
    }
    let mut used = BTreeSet::new();
    for a in &r.atoms {
        if let Atom::Capture(n, _) = a {
            if !declared.contains(n) {
                fail(&format!(
                    "rule `{id}` replacement uses undeclared capture `${n}`"
                ))
            }
            used.insert(n.clone());
        }
    }
    for n in declared {
        if !used.contains(&n) {
            fail(&format!("rule `{id}` capture `${n}` is unused"))
        }
    }
}
fn find_template_matches(
    t: &Template,
    src: &str,
    node: &str,
    idx: &SemIndex,
    path: &Path,
) -> Vec<Found> {
    let toks = lex_jet_source(src);
    let boundaries = idx
        .structural_nodes()
        .iter()
        .filter(|n| n.class == node && same_path(Path::new(&n.module_path), path))
        .map(|n| (n.span.start, n.span.end))
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    for start in 0..toks.len() {
        let mut captures = BTreeMap::new();
        if let Some(end) = match_atoms(&t.atoms, 0, &toks, start, src, &mut captures) {
            if end > start {
                let found_start = toks[start].start;
                let found_end = toks[end - 1].end;
                if !boundaries.contains(&(found_start, found_end)) {
                    continue;
                }
                let found = Found {
                    start: found_start,
                    end: found_end,
                    captures,
                };
                if captures_are_structural(t, &found, src, idx, path) {
                    out.push(found)
                }
            }
        }
    }
    out
}

fn captures_are_structural(
    template: &Template,
    found: &Found,
    source: &str,
    idx: &SemIndex,
    path: &Path,
) -> bool {
    let nodes = idx
        .structural_nodes()
        .iter()
        .filter(|node| {
            matches!(node.class.as_str(), "expr" | "stmt" | "item" | "type")
                && same_path(Path::new(&node.module_path), path)
                && node.span.start >= found.start
                && node.span.end <= found.end
        })
        .collect::<Vec<_>>();
    template.atoms.iter().all(|atom| {
        let Atom::Capture(name, variadic) = atom else {
            return true;
        };
        let captured = &found.captures[name];
        if *variadic && captured.is_empty() {
            return true;
        }
        let pieces = if *variadic {
            split_top_level_capture(captured)
        } else {
            vec![captured.as_str()]
        };
        pieces.into_iter().all(|piece| {
            let expected = normalized(piece);
            nodes.iter().any(|node| {
                source
                    .get(node.span.start..node.span.end)
                    .is_some_and(|candidate| normalized(candidate) == expected)
            })
        })
    })
}

fn split_top_level_capture(source: &str) -> Vec<&str> {
    let tokens = lex_jet_source(source);
    let mut depth = 0isize;
    let mut start = 0usize;
    let mut pieces = Vec::new();
    for token in &tokens {
        match token.text.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "," if depth == 0 => {
                pieces.push(source[start..token.start].trim());
                start = token.end;
            }
            _ => {}
        }
    }
    pieces.push(source[start..].trim());
    pieces
}
fn semantic_candidate(t: &Template, f: &Found, idx: &SemIndex, path: &Path) -> bool {
    t.atoms.iter().enumerate().all(|(i, atom)| {
        let Atom::Literal(name) = atom else {
            return true;
        };
        if !is_identifier_literal(name)
            || is_builtin(name)
            || t.atoms
                .get(i + 1)
                .is_some_and(|next| matches!(next, Atom::Literal(x) if x == ":"))
        {
            return true;
        }
        idx.references().iter().any(|reference| {
            reference.name == *name
                && same_path(Path::new(&reference.module_path), path)
                && reference.span.start >= f.start
                && reference.span.end <= f.end
                && reference.target_identity.is_some()
        }) || idx.definitions().iter().any(|definition| {
            definition.name == *name
                && same_path(Path::new(&definition.module_path), path)
                && definition.def_span.start >= f.start
                && definition.def_span.end <= f.end
        })
    })
}
fn match_atoms(
    atoms: &[Atom],
    ai: usize,
    toks: &[LexToken],
    ti: usize,
    src: &str,
    caps: &mut BTreeMap<String, String>,
) -> Option<usize> {
    if ai == atoms.len() {
        return Some(ti);
    }
    match &atoms[ai] {
        Atom::Literal(l) => {
            if toks.get(ti)?.text == *l {
                match_atoms(atoms, ai + 1, toks, ti + 1, src, caps)
            } else {
                None
            }
        }
        Atom::Capture(name, variadic) => {
            let min = if *variadic { 0 } else { 1 };
            for end in (ti + min..=toks.len()).rev() {
                if !balanced(&toks[ti..end]) {
                    continue;
                }
                let text = if end == ti {
                    String::new()
                } else {
                    src[toks[ti].start..toks[end - 1].end].to_string()
                };
                if let Some(old) = caps.get(name) {
                    if normalized(old) != normalized(&text) {
                        continue;
                    }
                } else {
                    caps.insert(name.clone(), text.clone());
                }
                if let Some(done) = match_atoms(atoms, ai + 1, toks, end, src, caps) {
                    return Some(done);
                }
                if caps.get(name) == Some(&text) {
                    caps.remove(name);
                }
            }
            None
        }
    }
}
fn balanced(t: &[LexToken]) -> bool {
    let mut stack = Vec::new();
    for x in t {
        match x.text.as_str() {
            "(" | "[" | "{" => stack.push(x.text.as_str()),
            ")" => {
                if stack.pop() != Some("(") {
                    return false;
                }
            }
            "]" => {
                if stack.pop() != Some("[") {
                    return false;
                }
            }
            "}" => {
                if stack.pop() != Some("{") {
                    return false;
                }
            }
            _ => {}
        }
    }
    stack.is_empty()
}
fn normalized(src: &str) -> Vec<String> {
    lex_jet_source(src).into_iter().map(|t| t.text).collect()
}

fn lex_jet_source(src: &str) -> Vec<LexToken> {
    let (tokens, diagnostics) = jet::Lexer::lex(src);
    if !diagnostics.is_empty() {
        fail("staged Jet source no longer lexes while matching an AST template");
    }
    tokens
        .into_iter()
        .filter_map(|token| {
            let text = src.get(token.span.start..token.span.end)?.to_string();
            (!text.is_empty()).then_some(LexToken {
                start: token.span.start,
                end: token.span.end,
                text,
            })
        })
        .collect()
}

fn validate_typed_template(id: &str, node: &str, template: &Template, side: &str) {
    let captures = template
        .atoms
        .iter()
        .filter_map(|atom| match atom {
            Atom::Capture(name, variadic) => Some((
                name.clone(),
                if *variadic {
                    "__codemod_a, __codemod_b".to_string()
                } else {
                    "__codemod_value".to_string()
                },
            )),
            Atom::Literal(_) => None,
        })
        .collect();
    let source = render_replacement(template, &captures);
    let wrapped = match node {
        "expr" => format!("fn run() {{ print({source}) }}\n"),
        "stmt" => format!("fn run() {{ {source} }}\n"),
        "item" => format!("{source}\n"),
        "type" => format!("fn __codemod(value: {source}) {{}}\nfn run() {{}}\n"),
        _ => unreachable!(),
    };
    if !jet::Compiler::parse_source(&wrapped).diagnostics.is_empty() {
        fail(&format!(
            "rule `{id}` {side} is not a valid Jet {node} template"
        ));
    }
}
fn render_replacement(t: &Template, c: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    for (a_i, a) in t.atoms.iter().enumerate() {
        let part = match a {
            Atom::Literal(s) => s,
            Atom::Capture(n, _) => &c[n],
        };
        if needs_space(out.chars().last(), part.chars().next()) {
            out.push(' ')
        }
        out.push_str(part);
        if a_i + 1 < t.atoms.len() && matches!(a,Atom::Literal(s)if s==","||s==":" ) {
            out.push(' ')
        }
    }
    out
}
fn needs_space(a: Option<char>, b: Option<char>) -> bool {
    a.zip(b).is_some_and(|(x, y)| {
        (x.is_alphanumeric() || x == '_') && (y.is_alphanumeric() || y == '_')
    })
}
fn validate_pattern_bindings(id: &str, m: &Template, r: &Template, idx: &SemIndex, unit: &Path) {
    let captures = m
        .atoms
        .iter()
        .filter_map(|a| {
            if let Atom::Capture(n, _) = a {
                Some(n.as_str())
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>();
    for (template, replacement) in [(m, false), (r, true)] {
        for (i, atom) in template.atoms.iter().enumerate() {
            let Atom::Literal(name) = atom else { continue };
            if !is_identifier_literal(name)
                || captures.contains(name.as_str())
                || template
                    .atoms
                    .get(i + 1)
                    .is_some_and(|n| matches!(n,Atom::Literal(x)if x==":"))
            {
                continue;
            }
            let defs = idx.definitions().iter().filter(|d| d.name == *name).count();
            if defs > 1 {
                fail(&format!(
                    "rule `{id}` identifier `{name}` is ambiguous in unit `{}`",
                    unit.display()
                ))
            }
            if replacement && defs == 0 && !is_builtin(name) {
                fail(&format!(
                    "rule `{id}` replacement identifier `{name}` does not resolve in unit `{}`",
                    unit.display()
                ))
            }
        }
    }
}

fn print_batch(batch: &Batch, plan: &BatchPlan) {
    let changed = plan
        .files
        .iter()
        .filter(|(_, s)| s.before != s.staged)
        .count();
    let edits: usize = plan.counts.iter().map(|(_, n)| *n).sum();
    println!(
        "codemod `{}` dry run\n  units: {}  rules: {}  files: {}  edits: {}",
        batch.name,
        discover_units(batch).len(),
        batch.rules.len(),
        changed,
        edits
    );
    for (id, n) in &plan.counts {
        println!("  {id}: {n} matches")
    }
    for v in &plan.validations {
        println!("  validation: {v}")
    }
    for (path, state) in &plan.files {
        if state.before != state.staged {
            println!(
                "--- {}\n+++ {}",
                project_rel(&batch.project, path),
                project_rel(&batch.project, path)
            );
            print_simple_diff(&state.before, &state.staged)
        }
    }
}
fn print_simple_diff(before: &[u8], after: &[u8]) {
    let a = String::from_utf8_lossy(before);
    let b = String::from_utf8_lossy(after);
    let a_lines = a.lines().count();
    let b_lines = b.lines().count();
    println!("@@ -1,{a_lines} +1,{b_lines} @@");
    for line in a.lines() {
        println!("-{line}")
    }
    if !before.is_empty() && !before.ends_with(b"\n") {
        println!("\\ No newline at end of file")
    }
    for line in b.lines() {
        println!("+{line}")
    }
    if !after.is_empty() && !after.ends_with(b"\n") {
        println!("\\ No newline at end of file")
    }
}

fn render_v2_log(batch: &Batch, changes: &[Change]) -> String {
    let rows=changes.iter().map(|c|format!("{{\"path\":\"{}\",\"before_hash\":\"{}\",\"after_hash\":\"{}\",\"before_bytes\":\"{}\",\"after_bytes\":\"{}\"}}",json_escape(&c.path.display().to_string()),hash_bytes(&c.before),hash_bytes(&c.after),hex(&c.before),hex(&c.after))).collect::<Vec<_>>().join(",\n    ");
    format!("{{\n  \"schema\": 2,\n  \"name\": \"{}\",\n  \"project\": \"{}\",\n  \"files\": [\n    {}\n  ]\n}}\n",json_escape(&batch.name),json_escape(&batch.project.display().to_string()),rows)
}
fn undo(path: &Path) {
    if !path.exists() {
        let project = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .unwrap_or_else(|| fail("missing replay log is not beneath .jet/codemods"));
        let _lock = Transaction::lock(project);
        Transaction::recover(project);
    }
    let raw = fs::read_to_string(path).unwrap_or_else(|e| {
        fail(&format!(
            "could not read codemod log `{}`: {e}",
            path.display()
        ))
    });
    let value = Json::parse(&raw).unwrap_or_else(|e| fail(&format!("invalid codemod log: {e}")));
    let schema = match &value {
        Value::Object(o) => number_default(o, "schema", 1),
        _ => fail("codemod log must be an object"),
    };
    if schema == 2 {
        undo_v2(value, path)
    } else {
        undo_v1(value, path)
    }
}
fn undo_v2(value: Value, path: &Path) {
    let mut o = value.object().unwrap();
    let schema = take_number(&mut o, "schema");
    if schema != 2 {
        fail("unsupported codemod log schema")
    }
    let name = take_string(&mut o, "name");
    let project = PathBuf::from(take_string(&mut o, "project"));
    let rows = take_array(&mut o, "files");
    reject_unknown(o, "schema 2 replay log");
    let _lock = Transaction::lock(&project);
    Transaction::recover(&project);
    let mut changes = Vec::new();
    for row in rows {
        let mut r = row
            .object()
            .unwrap_or_else(|_| fail("replay log file must be object"));
        let p = PathBuf::from(take_string(&mut r, "path"));
        let bh = take_string(&mut r, "before_hash");
        let ah = take_string(&mut r, "after_hash");
        let before = unhex(&take_string(&mut r, "before_bytes"));
        let after = unhex(&take_string(&mut r, "after_bytes"));
        reject_unknown(r, "replay log file");
        if hash_bytes(&before) != bh || hash_bytes(&after) != ah {
            fail(&format!(
                "replay log byte hash mismatch for `{}`",
                p.display()
            ))
        }
        let current = fs::read(&p)
            .unwrap_or_else(|e| fail(&format!("could not read `{}`: {e}", p.display())));
        if current != after {
            fail(&format!(
                "checkpoint mismatch for `{}`; refusing undo; zero files written",
                p.display()
            ))
        }
        changes.push(Change {
            path: p,
            before: after,
            after: before,
        });
    }
    let undo_log = path.with_extension("undo.log.json");
    let marker = format!(
        "{{\"schema\":2,\"name\":\"{}-undo\",\"files\":[]}}\n",
        json_escape(&name)
    );
    Transaction::commit(&project, &changes, &undo_log, marker.as_bytes());
    println!(
        "codemod undo `{name}` applied\n  files: {}\n  source log: {}",
        changes.len(),
        path.display()
    )
}

// Version-1 compatibility retains the original semantic-index rename and edit log.
fn run_v1(cm: V1, apply: bool) {
    let idx = open(&cm.entry).unwrap_or_else(|e| render_index_error(&cm.entry, e));
    let mut by: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    for d in idx.definitions().iter().filter(|d| d.name == cm.from) {
        by.entry(canonicalish(Path::new(&d.module_path)))
            .or_default()
            .push(edit(d.def_span, &cm.to));
    }
    for r in idx.references().iter().filter(|r| r.name == cm.from) {
        by.entry(canonicalish(Path::new(&r.module_path)))
            .or_default()
            .push(edit(r.span, &cm.to));
    }
    if by.is_empty() {
        fail(&format!(
            "codemod `{}` found no `{}` symbols",
            cm.name, cm.from
        ))
    }
    let mut changes = Vec::new();
    for (path, edits) in by {
        let before = fs::read(&path)
            .unwrap_or_else(|e| fail(&format!("could not read `{}`: {e}", path.display())));
        let src = std::str::from_utf8(&before).unwrap();
        let after = jet::FixEngine::apply_edits(src, &edits)
            .unwrap_or_else(|_| fail("version 1 rename edits overlap"))
            .into_bytes();
        changes.push(Change {
            path,
            before,
            after,
        });
    }
    println!(
        "codemod `{}` {}\n  rename: {} -> {}\n  files: {}",
        cm.name,
        if apply { "applied" } else { "dry run" },
        cm.from,
        cm.to,
        changes.len()
    );
    if apply {
        for c in &changes {
            fs::write(&c.path, &c.after)
                .unwrap_or_else(|e| fail(&format!("could not write `{}`: {e}", c.path.display())))
        }
        let log = render_v1_log(&cm, &changes);
        let dir = cm.entry.parent().unwrap().join(".jet/codemods");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{}.log.json", sanitize_file_name(&cm.name)));
        fs::write(&p, log).unwrap();
        println!("  log: {}", p.display())
    }
}
fn render_v1_log(cm: &V1, changes: &[Change]) -> String {
    let files=changes.iter().map(|c|format!("{{\"path\":\"{}\",\"before_hash\":\"{}\",\"after_hash\":\"{}\",\"before_bytes\":\"{}\",\"after_bytes\":\"{}\",\"inverse_edits\":[]}}",json_escape(&c.path.display().to_string()),hash_bytes(&c.before),hash_bytes(&c.after),hex(&c.before),hex(&c.after))).collect::<Vec<_>>().join(",");
    format!(
        "{{\"name\":\"{}\",\"inverse_from\":\"{}\",\"inverse_to\":\"{}\",\"files\":[{}]}}\n",
        json_escape(&cm.name),
        json_escape(&cm.to),
        json_escape(&cm.from),
        files
    )
}
fn undo_v1(value: Value, _path: &Path) {
    let mut o = value.object().unwrap();
    let name = take_string_default(&mut o, "name", "UndoCodemod");
    let files = take_array(&mut o, "files");
    let mut writes = Vec::new();
    for f in files {
        let mut x = f.object().unwrap();
        let path = PathBuf::from(take_string(&mut x, "path"));
        let before_hash = take_string(&mut x, "before_hash");
        let after_hash = take_string(&mut x, "after_hash");
        let current = fs::read(&path)
            .unwrap_or_else(|e| fail(&format!("could not read `{}`: {e}", path.display())));
        if hash_bytes(&current) != after_hash {
            fail(&format!(
                "checkpoint mismatch for `{}`; refusing undo",
                path.display()
            ))
        }
        let before = if let Some(s) = take_string_opt(&mut x, "before_bytes") {
            unhex(&s)
        } else {
            let edits = take_array(&mut x, "inverse_edits")
                .into_iter()
                .map(|v| {
                    let mut e = v
                        .object()
                        .unwrap_or_else(|_| fail("legacy inverse edit must be an object"));
                    TextEdit {
                        span: Span::new(
                            take_number(&mut e, "start") as usize,
                            take_number(&mut e, "end") as usize,
                        ),
                        new_text: take_string(&mut e, "new_text"),
                    }
                })
                .collect::<Vec<_>>();
            jet::FixEngine::apply_edits(
                std::str::from_utf8(&current)
                    .unwrap_or_else(|_| fail("legacy codemod file is not UTF-8")),
                &edits,
            )
            .unwrap_or_else(|_| fail("legacy codemod inverse edits overlap"))
            .into_bytes()
        };
        if hash_bytes(&before) != before_hash {
            fail(&format!(
                "undo result mismatch for `{}`; refusing undo",
                path.display()
            ))
        }
        writes.push((path, before));
    }
    for (p, b) in writes {
        fs::write(p, b)
            .unwrap_or_else(|e| fail(&format!("could not restore legacy codemod file: {e}")))
    }
    println!("codemod undo `{name}` applied")
}

fn edit(s: jet_semindex::SourceSpan, text: &str) -> TextEdit {
    TextEdit {
        span: Span::new(s.start, s.end),
        new_text: text.into(),
    }
}
fn kind_matches(k: &SymbolKind, want: &str) -> bool {
    matches!(
        (k, want),
        (SymbolKind::Function { .. }, "function")
            | (SymbolKind::Struct { .. }, "struct")
            | (SymbolKind::Enum { .. }, "enum")
            | (SymbolKind::Trait, "trait")
            | (SymbolKind::Const, "const")
            | (SymbolKind::Local { .. }, "local")
            | (SymbolKind::Param { .. }, "param")
            | (SymbolKind::Field { .. }, "field")
            | (SymbolKind::Module, "module")
    )
}
fn read_fingerprint(path: &Path, inputs: &mut BTreeMap<PathBuf, String>) -> Vec<u8> {
    let bytes = fs::read(path)
        .unwrap_or_else(|e| fail(&format!("could not read `{}`: {e}", path.display())));
    inputs
        .entry(path.to_path_buf())
        .or_insert_with(|| hash_bytes(&bytes));
    bytes
}
fn verify_inputs(inputs: &BTreeMap<PathBuf, String>) {
    let drift = inputs
        .iter()
        .filter_map(|(p, h)| {
            let now = fs::read(p).ok().map(|b| hash_bytes(&b));
            if now.as_ref() != Some(h) {
                Some(p.display().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if !drift.is_empty() {
        fail(&format!(
            "codemod inputs drifted; no files written:\n  {}",
            drift.join("\n  ")
        ))
    }
}
fn secure_existing(project: &Path, raw: &str, allow_dir: bool) -> PathBuf {
    reject_parent_or_absolute(raw, "root path");
    let allowed = raw == "examples"
        || raw.starts_with("examples/")
        || raw == "tests/ui"
        || raw.starts_with("tests/ui/");
    if !allowed {
        fail(&format!(
            "editable root `{raw}` must be beneath examples/ or tests/ui/"
        ))
    }
    let p = secure_components(project, raw);
    let canonical = fs::canonicalize(&p)
        .unwrap_or_else(|e| fail(&format!("could not canonicalize `{raw}`: {e}")));
    if !canonical.starts_with(project) {
        fail(&format!("path `{raw}` escapes project"))
    }
    let m = fs::metadata(&canonical).unwrap();
    if !(m.is_file() || (allow_dir && m.is_dir())) {
        fail(&format!(
            "root `{raw}` must be a regular .jet file or directory"
        ))
    }
    if m.is_file() && canonical.extension().and_then(|s| s.to_str()) != Some("jet") {
        fail(&format!("file root `{raw}` must end in .jet"))
    }
    canonical
}
fn secure_destination(project: &Path, raw: &str) -> PathBuf {
    reject_parent_or_absolute(raw, "snapshot destination");
    if !raw.starts_with("tests/ui/") || !raw.ends_with(".jet") {
        fail("snapshot_after keys must name tests/ui/*.jet fixtures")
    }
    secure_components(project, raw).with_extension("stderr")
}
fn secure_existing_project_file(project: &Path, raw: &str) -> PathBuf {
    reject_parent_or_absolute(raw, "snapshot input");
    let p = secure_components(project, raw);
    let c = fs::canonicalize(&p).unwrap_or_else(|e| {
        fail(&format!(
            "could not canonicalize snapshot input `{raw}`: {e}"
        ))
    });
    if !c.starts_with(project) || !c.is_file() {
        fail("snapshot_after input must be a regular project file")
    }
    c
}
fn secure_components(project: &Path, raw: &str) -> PathBuf {
    let mut p = project.to_path_buf();
    for c in Path::new(raw).components() {
        let Component::Normal(n) = c else {
            fail(&format!("path `{raw}` contains a forbidden component"))
        };
        p.push(n);
        if p.exists() && fs::symlink_metadata(&p).unwrap().file_type().is_symlink() {
            fail(&format!("path `{raw}` traverses a symlink"))
        }
    }
    p
}
fn reject_parent_or_absolute(raw: &str, label: &str) {
    let p = Path::new(raw);
    if p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        fail(&format!(
            "{label} `{raw}` must be a relative path without `..`"
        ))
    }
}
fn canonicalish(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}
fn same_path(a: &Path, b: &Path) -> bool {
    canonicalish(a) == canonicalish(b)
}
fn project_rel(project: &Path, p: &Path) -> String {
    p.strip_prefix(project)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}
fn render_index_error(entry: &Path, e: SemIndexError) -> ! {
    match e {
        SemIndexError::Load(ds) => {
            let src = fs::read_to_string(entry).unwrap_or_default();
            eprintln!(
                "{}",
                jet::render_diagnostics(&entry.display().to_string(), &src, &ds)
            );
            exit(ExitCodes::USER_ERROR)
        }
    }
}
pub(super) fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    exit(ExitCodes::USER_ERROR)
}
pub(super) fn hash_bytes(b: &[u8]) -> String {
    format!("sha256-{}", jet::SHA256::sha256_hex(b))
}
pub(super) fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if c.is_control() => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn unhex(s: &str) -> Vec<u8> {
    if s.len() % 2 != 0 {
        fail("invalid byte encoding in replay log")
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .unwrap_or_else(|_| fail("invalid byte encoding in replay log"))
        })
        .collect()
}
fn validate_ident(n: &str) {
    let mut c = n.chars();
    if !c.next().is_some_and(|x| x == '_' || x.is_alphabetic())
        || !c.all(|x| x == '_' || x.is_alphanumeric())
    {
        fail(&format!("`{n}` is not a Jet identifier"))
    }
}
fn is_identifier_literal(text: &str) -> bool {
    let (tokens, diagnostics) = jet::Lexer::lex(text);
    let mut meaningful = tokens
        .iter()
        .filter(|token| token.span.start != token.span.end);
    diagnostics.is_empty()
        && meaningful
            .next()
            .is_some_and(|token| matches!(token.kind, jet::Lexer::TokKind::Ident(_)))
        && meaningful.next().is_none()
}
fn is_builtin(n: &str) -> bool {
    matches!(
        n,
        "print" | "Some" | "None" | "Ok" | "Err" | "Int" | "Float" | "Bool" | "String" | "Void"
    )
}
fn sanitize_file_name(n: &str) -> String {
    let x = n
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if x.is_empty() {
        "codemod".into()
    } else {
        x
    }
}
fn log_path(project: &Path, name: &str) -> PathBuf {
    project
        .join(".jet/codemods")
        .join(format!("{}.log.json", sanitize_file_name(name)))
}
fn absolutize(raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.into()
    } else {
        std::env::current_dir().unwrap().join(p)
    }
}
fn resolve_against(parent: Option<&Path>, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.into()
    } else {
        parent.unwrap_or(Path::new(".")).join(p)
    }
}
fn reject_unknown(o: BTreeMap<String, Value>, what: &str) {
    if !o.is_empty() {
        fail(&format!(
            "unknown field(s) in {what}: {}",
            o.keys().cloned().collect::<Vec<_>>().join(", ")
        ))
    }
}
fn take_string(o: &mut BTreeMap<String, Value>, k: &str) -> String {
    match o.remove(k) {
        Some(Value::String(s)) => s,
        Some(_) => fail(&format!("`{k}` must be text")),
        None => fail(&format!("codemod object missing `{k}`")),
    }
}
fn take_string_opt(o: &mut BTreeMap<String, Value>, k: &str) -> Option<String> {
    match o.remove(k) {
        Some(Value::String(s)) => Some(s),
        Some(_) => fail(&format!("`{k}` must be text")),
        None => None,
    }
}
fn take_string_default(o: &mut BTreeMap<String, Value>, k: &str, d: &str) -> String {
    take_string_opt(o, k).unwrap_or_else(|| d.into())
}
fn take_number(o: &mut BTreeMap<String, Value>, k: &str) -> u64 {
    match o.remove(k) {
        Some(Value::Number(n)) => n,
        Some(_) => fail(&format!("`{k}` must be a non-negative integer")),
        None => fail(&format!("codemod object missing `{k}`")),
    }
}
fn take_number_opt(o: &mut BTreeMap<String, Value>, k: &str) -> Option<u64> {
    match o.remove(k) {
        Some(Value::Number(n)) => Some(n),
        Some(_) => fail(&format!("`{k}` must be a non-negative integer")),
        None => None,
    }
}
fn number_default(o: &BTreeMap<String, Value>, k: &str, d: u64) -> u64 {
    match o.get(k) {
        Some(Value::Number(n)) => *n,
        Some(_) => fail(&format!("`{k}` must be a non-negative integer")),
        None => d,
    }
}
fn take_bool_default(o: &mut BTreeMap<String, Value>, k: &str, d: bool) -> bool {
    match o.remove(k) {
        Some(Value::Bool(v)) => v,
        Some(_) => fail(&format!("`{k}` must be true or false")),
        None => d,
    }
}
fn take_array(o: &mut BTreeMap<String, Value>, k: &str) -> Vec<Value> {
    match o.remove(k) {
        Some(Value::Array(v)) => v,
        Some(_) => fail(&format!("`{k}` must be an array")),
        None => fail(&format!("codemod object missing `{k}`")),
    }
}
fn take_object(o: &mut BTreeMap<String, Value>, k: &str) -> BTreeMap<String, Value> {
    match o.remove(k) {
        Some(Value::Object(v)) => v,
        Some(_) => fail(&format!("`{k}` must be an object")),
        None => fail(&format!("codemod object missing `{k}`")),
    }
}
fn take_object_opt(o: &mut BTreeMap<String, Value>, k: &str) -> Option<BTreeMap<String, Value>> {
    match o.remove(k) {
        Some(Value::Object(v)) => Some(v),
        Some(_) => fail(&format!("`{k}` must be an object")),
        None => None,
    }
}
