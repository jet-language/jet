//! D-CODEMOD1: replayable semantic codemods over semindex + fix engine.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use jet::Diagnostics::{Span, TextEdit};
use jet::ExitCodes;
use jet_semindex::{open, SemIndexError};

#[derive(Debug, Clone)]
struct RenameCodemod {
    name: String,
    entry: PathBuf,
    from: String,
    to: String,
}

#[derive(Debug, Clone)]
struct FilePlan {
    path: PathBuf,
    before: String,
    after: String,
    edits: Vec<TextEdit>,
}

pub(crate) fn run_codemod(args: &[String]) {
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if !a.starts_with('-') {
            positional.push(a.as_str());
        }
    }

    match positional.as_slice() {
        ["dry-run", object] => {
            let object_path = absolutize(object);
            let cm = load_codemod(&object_path);
            let plans = plan_rename(&cm);
            print_dry_run(&cm, &plans);
        }
        ["apply", object] => {
            let object_path = absolutize(object);
            let cm = load_codemod(&object_path);
            let plans = plan_rename(&cm);
            apply_plans(&cm, &plans);
        }
        ["undo", log] => {
            let log_path = absolutize(log);
            let cm = load_inverse_log(&log_path);
            let plans = plan_rename(&cm);
            apply_undo(&cm, &plans, &log_path);
        }
        _ => {
            eprintln!("error: `jet codemod` needs `dry-run`, `apply`, or `undo`");
            eprintln!(" Fix: jet codemod dry-run rename.codemod.json");
            exit(ExitCodes::USER_ERROR);
        }
    }
}

fn load_codemod(path: &Path) -> RenameCodemod {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("error: could not read codemod `{}`: {e}", path.display());
        exit(ExitCodes::USER_ERROR);
    });
    let name = json_string_field(&text, "name").unwrap_or_else(|| "Codemod".to_string());
    let op = json_string_field(&text, "operation").unwrap_or_else(|| "rename".to_string());
    if op != "rename" {
        eprintln!("error: only `rename` codemods are supported in this slice");
        exit(ExitCodes::USER_ERROR);
    }
    let entry = required_field(&text, "entry");
    let from = required_field(&text, "from");
    let to = required_field(&text, "to");
    validate_ident(&from);
    validate_ident(&to);
    let entry = resolve_against(path.parent(), &entry);
    RenameCodemod {
        name,
        entry,
        from,
        to,
    }
}

fn load_inverse_log(path: &Path) -> RenameCodemod {
    let text = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!(
            "error: could not read codemod log `{}`: {e}",
            path.display()
        );
        exit(ExitCodes::USER_ERROR);
    });
    let name = json_string_field(&text, "name").unwrap_or_else(|| "UndoCodemod".to_string());
    let entry = required_field(&text, "entry");
    let from = required_field(&text, "inverse_from");
    let to = required_field(&text, "inverse_to");
    let expected_current = required_field(&text, "after_hash");
    let entry_path = PathBuf::from(entry);
    let current = fs::read(&entry_path)
        .map(|b| format!("sha256-{}", jet::SHA256::sha256_hex(&b)))
        .unwrap_or_default();
    if current != expected_current {
        eprintln!(
            "error: checkpoint mismatch for `{}`; refusing undo",
            entry_path.display()
        );
        eprintln!(" Why: current file hash does not match codemod apply log");
        exit(ExitCodes::USER_ERROR);
    }
    RenameCodemod {
        name,
        entry: entry_path,
        from,
        to,
    }
}

fn plan_rename(cm: &RenameCodemod) -> Vec<FilePlan> {
    let idx = match open(&cm.entry) {
        Ok(idx) => idx,
        Err(SemIndexError::Load(diags)) => {
            for d in &diags {
                eprintln!(
                    "{}",
                    jet::render_diagnostics(
                        &cm.entry.display().to_string(),
                        "",
                        std::slice::from_ref(d)
                    )
                );
            }
            exit(ExitCodes::USER_ERROR);
        }
    };

    let mut by_file: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    for def in idx.definitions().iter().filter(|d| d.name == cm.from) {
        by_file
            .entry(PathBuf::from(&def.module_path))
            .or_default()
            .push(edit(def.def_span, &cm.to));
    }
    for r in idx.references().iter().filter(|r| r.name == cm.from) {
        by_file
            .entry(PathBuf::from(&r.module_path))
            .or_default()
            .push(edit(r.span, &cm.to));
    }
    if by_file.is_empty() {
        eprintln!(
            "error: codemod `{}` found no `{}` symbols",
            cm.name, cm.from
        );
        exit(ExitCodes::USER_ERROR);
    }

    let mut plans = Vec::new();
    for (path, edits) in by_file {
        let before = fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("error: could not read `{}`: {e}", path.display());
            exit(ExitCodes::USER_ERROR);
        });
        let after = jet::FixEngine::apply_edits(&before, &edits).unwrap_or_else(|e| {
            eprintln!(
                "error: codemod edits overlap in `{}`: {e:?}",
                path.display()
            );
            exit(ExitCodes::USER_ERROR);
        });
        if after != before {
            plans.push(FilePlan {
                path,
                before,
                after,
                edits,
            });
        }
    }
    plans
}

fn edit(span: jet_semindex::SourceSpan, text: &str) -> TextEdit {
    TextEdit {
        span: Span::new(span.start, span.end),
        new_text: text.to_string(),
    }
}

fn print_dry_run(cm: &RenameCodemod, plans: &[FilePlan]) {
    println!("codemod `{}` dry run", cm.name);
    println!("  rename: {} -> {}", cm.from, cm.to);
    println!("  files:  {}", plans.len());
    for p in plans {
        println!("  modify {} ({} edit(s))", p.path.display(), p.edits.len());
    }
    println!("inverse");
    println!(
        "  {{\"name\":\"{}-inverse\",\"entry\":\"{}\",\"operation\":\"rename\",\"from\":\"{}\",\"to\":\"{}\"}}",
        json_escape(&cm.name),
        json_escape(&cm.entry.display().to_string()),
        json_escape(&cm.to),
        json_escape(&cm.from)
    );
}

fn apply_plans(cm: &RenameCodemod, plans: &[FilePlan]) {
    for p in plans {
        fs::write(&p.path, &p.after).unwrap_or_else(|e| {
            eprintln!("error: could not write `{}`: {e}", p.path.display());
            exit(ExitCodes::USER_ERROR);
        });
    }
    let log = write_log(cm, plans);
    println!("codemod `{}` applied", cm.name);
    println!("  files: {}", plans.len());
    println!("  log: {}", log.display());
}

fn apply_undo(cm: &RenameCodemod, plans: &[FilePlan], log_path: &Path) {
    for p in plans {
        fs::write(&p.path, &p.after).unwrap_or_else(|e| {
            eprintln!("error: could not write `{}`: {e}", p.path.display());
            exit(ExitCodes::USER_ERROR);
        });
    }
    println!("codemod undo `{}` applied", cm.name);
    println!("  files: {}", plans.len());
    println!("  source log: {}", log_path.display());
}

fn write_log(cm: &RenameCodemod, plans: &[FilePlan]) -> PathBuf {
    let dir = cm
        .entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".jet")
        .join("codemods");
    fs::create_dir_all(&dir).unwrap_or_else(|e| {
        eprintln!("error: could not create `{}`: {e}", dir.display());
        exit(ExitCodes::USER_ERROR);
    });
    let log_path = dir.join(format!("{}.log.json", sanitize_file_name(&cm.name)));
    let before_hash = plans
        .first()
        .map(|p| format!("sha256-{}", jet::SHA256::sha256_hex(p.before.as_bytes())))
        .unwrap_or_default();
    let after_hash = plans
        .first()
        .map(|p| format!("sha256-{}", jet::SHA256::sha256_hex(p.after.as_bytes())))
        .unwrap_or_default();
    let text = format!(
        "{{\n  \"name\": \"{}\",\n  \"entry\": \"{}\",\n  \"operation\": \"rename\",\n  \"from\": \"{}\",\n  \"to\": \"{}\",\n  \"inverse_from\": \"{}\",\n  \"inverse_to\": \"{}\",\n  \"before_hash\": \"{}\",\n  \"after_hash\": \"{}\",\n  \"files\": {}\n}}\n",
        json_escape(&cm.name),
        json_escape(&cm.entry.display().to_string()),
        json_escape(&cm.from),
        json_escape(&cm.to),
        json_escape(&cm.to),
        json_escape(&cm.from),
        json_escape(&before_hash),
        json_escape(&after_hash),
        plans.len()
    );
    fs::write(&log_path, text).unwrap_or_else(|e| {
        eprintln!("error: could not write `{}`: {e}", log_path.display());
        exit(ExitCodes::USER_ERROR);
    });
    log_path
}

fn required_field(text: &str, key: &str) -> String {
    json_string_field(text, key).unwrap_or_else(|| {
        eprintln!("error: codemod object missing `{key}`");
        exit(ExitCodes::USER_ERROR);
    })
}

fn json_string_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut rest = text;
    while let Some(pos) = rest.find(&needle) {
        rest = &rest[pos + needle.len()..];
        let colon = rest.find(':')?;
        rest = &rest[colon + 1..];
        let start = rest.find('"')?;
        rest = &rest[start + 1..];
        let mut out = String::new();
        let mut escaped = false;
        for c in rest.chars() {
            if escaped {
                out.push(match c {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    '"' => '"',
                    '\\' => '\\',
                    other => other,
                });
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                return Some(out);
            } else {
                out.push(c);
            }
        }
    }
    None
}

fn validate_ident(name: &str) {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        eprintln!("error: empty codemod rename symbol");
        exit(ExitCodes::USER_ERROR);
    };
    if (!first.is_alphabetic() && first != '_') || !chars.all(|c| c.is_alphanumeric() || c == '_') {
        eprintln!("error: `{name}` is not a Jet identifier");
        exit(ExitCodes::USER_ERROR);
    }
}

fn resolve_against(parent: Option<&Path>, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        parent.unwrap_or_else(|| Path::new(".")).join(p)
    }
}

fn absolutize(path: &str) -> PathBuf {
    let p = Path::new(path);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

fn sanitize_file_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "codemod".to_string()
    } else {
        out
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
