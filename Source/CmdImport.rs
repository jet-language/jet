//! Semantic source importer framework (D-MIGRATE-SRC1=A).
//!
//! Output is ordinary editable Jet. Unsupported source is never discarded or
//! represented by fake behavior: each gap is recorded in the omissions report.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use jet::Diagnostics::Diagnostic;
use jet::ExitCodes;
use jet::RecordIndex::{RecordIndex, RecordIndexEntry, RecordKind, RecordLink};
use jet::ReceiptStore::{read_path, receipt_root_for, ReceiptSection, ReceiptStore};
use jet_foundation::Evidence::{
    EvidenceBuild, EvidenceKind, EvidenceOutcome, EvidenceProducerKind, EvidenceRecord,
    EvidenceReport, EvidenceRevision, EvidenceSource, EvidenceAttachment, EvidenceIdentity,
};
use jet_foundation::Facts::{DerivationDisposition, DerivationMethod, DerivationRecord};
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::Report::{ReportEnvelope, StatusEnvelope, StatusFields, StatusValue};
use jet_foundation::TestingComparison::ComparisonRecord;
use jet_foundation::JSON::json_escape;
const TODO_CODE: &str = "JT0101";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Create,
    Update,
    DryRun,
}

struct Todo {
    what: String,
    why: String,
    fix: String,
    source: String,
    target: String,
    status: &'static str,
}

impl Todo {
    fn diagnostic(&self) -> Diagnostic {
        Diagnostic::from_row(
            TODO_CODE,
            &[
                ("construct", &self.what),
                ("source", &self.source),
                ("reason", &self.why),
                ("fix", &self.fix),
                ("target", &self.target),
            ],
            None,
        )
    }
}

struct Generated {
    relative: PathBuf,
    contents: String,
}

struct Plan {
    language: String,
    source: PathBuf,
    target: PathBuf,
    generated: Vec<Generated>,
    todos: Vec<Todo>,
    functions: usize,
    tests: usize,
    boundary: Option<jet::ForeignBridge::ForeignBoundaryContract>,
    evidence: Option<EvidenceReport>,
    boundary_digest: String,
    source_digest: String,
    replacement_digest: String,
}

pub(crate) fn run(raw: &[String], json: bool) -> i32 {
    let (raw_lang, source, mode) = match parse_args(raw) {
        Ok(args) => args,
        Err((what, fix)) => return usage_error(&what, &fix, json),
    };
    let lang = canonical_language(&raw_lang);
    if matches!(lang.as_str(), "c" | "cpp") {
        return usage_error(
            &format!("source importer '{lang}' is intentionally unavailable"),
            &format!(
                "use {lang}.<lib> for call-in-place binding, then replace one module at a time with a checked #Extern overlay; source conversion is not maintainable"
            ),
            json,
        );
    }
    if !matches!(
        lang.as_str(),
        "py" | "pascal" | "ada" | "java" | "csharp" | "ts" | "js" | "go"
    ) {
        return usage_error(
            &format!("source importer '{lang}' is not installed"),
            "use jet import py|pascal|ada|java|csharp|ts|js|go <dir>; C and C++ use their binder plus checked overlays",
            json,
        );
    }
    let cwd = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => return operation_error("read current directory", &error.to_string(), json),
    };
    let source = if source.is_absolute() {
        source
    } else {
        cwd.join(source)
    };
    let source = match source.canonicalize() {
        Ok(path) if path.is_dir() => path,
        Ok(_) => {
            return operation_error(
                &format!("import {lang} source"),
                "source is not a directory",
                json,
            )
        }
        Err(error) => {
            return operation_error(&format!("import {lang} source"), &error.to_string(), json)
        }
    };
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("imported");
    let target = cwd.join("jet").join(name);
    match build_plan(&source, &target, &lang) {
        Ok(plan) => apply_plan(plan, mode, json),
        Err(error) => operation_error(&format!("import {lang} source"), &error, json),
    }
}

fn parse_args(raw: &[String]) -> Result<(String, PathBuf, Mode), (String, String)> {
    let mut positionals = Vec::new();
    let mut mode = Mode::Create;
    for arg in raw.iter().skip(1) {
        match arg.as_str() {
            jet::CLI::DRY_RUN_FLAG if mode == Mode::Create => mode = Mode::DryRun,
            "--update" if mode == Mode::Create => mode = Mode::Update,
            "--dry-run" | "--update" => {
                return Err((
                    "--dry-run and --update cannot be combined".into(),
                    "choose preview (--dry-run) or merge (--update)".into(),
                ))
            }
            value if value.starts_with('-') => {
                return Err((
                    format!("'{value}' is not an import flag"),
                    "use jet import <lang> <dir> [--dry-run|--update]".into(),
                ))
            }
            value => positionals.push(value.to_string()),
        }
    }
    if positionals.len() != 2 {
        return Err((
            "source import needs a language and directory".into(),
            "use jet import <lang> <dir> [--dry-run|--update]".into(),
        ));
    }
    Ok((
        positionals.remove(0),
        PathBuf::from(positionals.remove(0)),
        mode,
    ))
}

fn canonical_language(language: &str) -> String {
    let language = language.to_ascii_lowercase();
    match language.as_str() {
        "python" => "py",
        "cs" | "c#" => "csharp",
        "typescript" => "ts",
        "javascript" => "js",
        "golang" => "go",
        "c++" => "cpp",
        other => other,
    }
    .to_string()
}

fn foreign_language(language: &str) -> Option<jet::AST::ForeignLanguage> {
    Some(match language {
        "py" => jet::AST::ForeignLanguage::Py,
        "pascal" => jet::AST::ForeignLanguage::Pascal,
        "ada" => jet::AST::ForeignLanguage::Ada,
        "java" => jet::AST::ForeignLanguage::Java,
        "csharp" => jet::AST::ForeignLanguage::DotNet,
        "ts" | "js" => jet::AST::ForeignLanguage::JS,
        "go" => jet::AST::ForeignLanguage::Go,
        _ => return None,
    })
}

fn import_tree_identity(
    source: &Path,
    files: &[PathBuf],
    schema: &str,
) -> Result<String, String> {
    let mut identity = jet::ForeignBridge::IdentityBuilder::new(schema);
    for path in files {
        let relative = path.strip_prefix(source).map_err(|error| error.to_string())?;
        let raw = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
        identity.field("path", relative.to_string_lossy().as_bytes());
        identity.field("bytes", &raw);
    }
    Ok(identity.finish())
}

fn generated_tree_identity(
    generated: &[Generated],
    schema: &str,
) -> String {
    let mut identity = jet::ForeignBridge::IdentityBuilder::new(schema);
    for file in generated {
        identity.field("path", file.relative.to_string_lossy().as_bytes());
        identity.field("bytes", file.contents.as_bytes());
    }
    identity.finish()
}

fn boundary_for_import(
    language: &str,
    source: &Path,
    target: &Path,
    source_digest: String,
    replacement_digest: String,
) -> Result<jet::ForeignBridge::ForeignBoundaryContract, String> {
    let foreign_language = foreign_language(language)
        .ok_or_else(|| format!("source importer has no foreign boundary for `{language}`"))?;
    let target_identity = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    let source_identity = format!("source:{}:sha256-{source_digest}", source.display());
    let replacement_identity =
        format!("generated:{}:sha256-{replacement_digest}", target.display());
    let identity = jet::ForeignBridge::ForeignBoundaryIdentity::new(
        source_identity.clone(),
        replacement_identity,
        "jet-source-import-v1",
        source_identity.clone(),
        format!("jet-source-import:{}", env!("CARGO_PKG_VERSION")),
        target_identity.clone(),
    );
    let coverage = jet::ForeignBridge::ForeignArtifactCoverage::new(
        source_identity,
        target_identity,
        "jet-source-import-v1",
    )
    .with_transitive_dependencies(std::iter::empty::<String>())
    .with_reachable_callbacks(std::iter::empty::<String>())
    .with_compiler_flags(std::iter::empty::<String>());
    jet::ForeignBridge::source_import_boundary(
        foreign_language,
        format!("{language}:{}", source.display()),
        identity,
        coverage,
    )
    .map(|boundary| {
        boundary.with_assumptions([
            "D-MIGRATE-SRC1 proves only the declared source subset".to_string(),
            "foreign source remains authoritative until an accepted comparison receipt".to_string(),
        ])
    })
}

fn build_plan(source: &Path, target: &Path, language: &str) -> Result<Plan, String> {
    let mut files = Vec::new();
    collect_sources(source, source, language, &mut files)?;
    if files.is_empty() {
        let extensions = match language {
            "pascal" => ".pas or .pp",
            "ada" => ".ads",
            "java" => ".java",
            "csharp" => ".cs",
            "ts" => ".ts",
            "js" => ".js",
            "go" => ".go",
            _ => ".py",
        };
        return Err(format!(
            "no {extensions} files found under {}",
            source.display()
        ));
    }
    files.sort();
    let mut plan = Plan {
        language: language.to_string(),
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        generated: Vec::new(),
        todos: Vec::new(),
        functions: 0,
        tests: 0,
        boundary: None,
        evidence: None,
        boundary_digest: String::new(),
        source_digest: String::new(),
        replacement_digest: String::new(),
    };
    for path in &files {
        let relative = path.strip_prefix(source).map_err(|e| e.to_string())?;
        let mut output = relative.to_path_buf();
        output.set_extension("jet");
        let raw = fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let generated_target = target.join(&output);
        let translation = match language {
            "pascal" => translate_pascal_file(&raw, path, &generated_target),
            "ada" => translate_ada_file(&raw, path, &generated_target),
            "java" | "csharp" | "ts" | "js" | "go" => {
                translate_enterprise_file(language, &raw, path, &generated_target)
            }
            _ => translate_file(&raw, path, &generated_target),
        };
        plan.functions += translation.functions;
        plan.tests += translation.tests;
        plan.todos.extend(translation.todos);
        plan.generated.push(Generated {
            relative: output,
            contents: translation.source,
        });
    }
    let source_digest = import_tree_identity(source, &files, "jet-source-import-input-v1")?;
    let replacement_digest =
        generated_tree_identity(&plan.generated, "jet-source-import-replacement-v1");
    let boundary = boundary_for_import(
        language,
        source,
        target,
        source_digest.clone(),
        replacement_digest.clone(),
    )?;
    let boundary_digest = boundary.digest();
    let evidence = import_evidence(
        &boundary,
        source,
        target,
        &source_digest,
        &replacement_digest,
        files.len(),
        plan.functions,
        plan.tests,
        plan.todos.len(),
    )?;
    plan.boundary = Some(boundary);
    plan.evidence = Some(evidence);
    plan.boundary_digest = boundary_digest;
    plan.source_digest = source_digest;
    plan.replacement_digest = replacement_digest;
    Ok(plan)
}

fn collect_sources(
    root: &Path,
    dir: &Path,
    language: &str,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read {}: {e}", dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            collect_sources(root, &path, language, out)?;
        } else if ty.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| match language {
                    "pascal" => matches!(extension, "pas" | "pp"),
                    "ada" => extension == "ads",
                    "java" => extension == "java",
                    "csharp" => extension == "cs",
                    "ts" => extension == "ts",
                    "js" => extension == "js",
                    "go" => extension == "go",
                    _ => extension == "py",
                })
        {
            let relative = path.strip_prefix(root).map_err(|e| e.to_string())?;
            if relative.components().any(|part| {
                matches!(
                    part,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            }) {
                return Err(format!("unsafe source path {}", relative.display()));
            }
            out.push(path);
        }
    }
    Ok(())
}

struct Translation {
    source: String,
    todos: Vec<Todo>,
    functions: usize,
    tests: usize,
}

struct Function {
    name: String,
    params: Vec<(String, String)>,
    result: String,
    body: Vec<(usize, String)>,
}

fn translate_file(raw: &str, source: &Path, target: &Path) -> Translation {
    let lines: Vec<&str> = raw.lines().collect();
    let mut rendered = Vec::new();
    let mut todos = Vec::new();
    let mut functions = 0;
    let mut tests = 0;
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            at += 1;
            continue;
        }
        if indentation(line) != 0 || !trimmed.starts_with("def ") {
            todos.push(gap(
                format!(
                    "unsupported Python top-level construct '{}'",
                    shorten(trimmed)
                ),
                "first Python importer translates annotated functions only",
                "port construct into ordinary Jet; no source was silently emitted",
                source,
                target,
                at + 1,
            ));
            at += 1;
            continue;
        }
        let header_line = at + 1;
        let parsed = parse_header(trimmed);
        at += 1;
        let mut body = Vec::new();
        while at < lines.len() {
            if !lines[at].trim().is_empty() && indentation(lines[at]) == 0 {
                break;
            }
            if !lines[at].trim().is_empty() {
                body.push((at + 1, lines[at].to_string()));
            }
            at += 1;
        }
        let Ok((name, params, result)) = parsed else {
            todos.push(gap(
                format!("Python function signature at line {header_line} is not type-safe"),
                parsed.unwrap_err(),
                "add supported parameter and return annotations, then rerun import",
                source,
                target,
                header_line,
            ));
            continue;
        };
        let function = Function {
            name,
            params,
            result,
            body,
        };
        match translate_body(&function) {
            Ok(body) => {
                let marker = if function.name.starts_with("test_") && function.params.is_empty() {
                    tests += 1;
                    "#Test "
                } else {
                    ""
                };
                rendered.push(render_function(marker, &function, &body));
                functions += 1;
            }
            Err((line, reason)) => todos.push(gap(
                format!("Python body '{}' was not translated", function.name),
                reason,
                "port body into ordinary Jet; source remains in omissions report",
                source,
                target,
                line,
            )),
        }
    }
    let mut output = header("py");
    for (index, item) in rendered.into_iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&item);
    }
    if functions == 0 {
        output.push_str("// No construct was safe to translate. See import-report.json.\n");
    }
    Translation {
        source: output,
        todos,
        functions,
        tests,
    }
}

fn translate_pascal_file(raw: &str, source: &Path, target: &Path) -> Translation {
    let library = pascal_library_name(raw);
    let mut output = header("pascal");
    output.push_str(&format!("// Source: {}\n", source.display()));
    let (what, why, fix) = if let Some(library) = &library {
        output.push_str(&format!("use pascal.{library} as {library}\n\n"));
        output.push_str(&format!(
            "// Binder stub: use pascal.{library} as {library}\n"
        ));
        output.push_str(&format!(
            "// TODO JT0101: Pascal library `{library}` remains an FFI source.\n"
        ));
        (
            format!("Pascal library `{library}` was not translated"),
            "Object Pascal bodies, classes, and runtime ownership have no proven source-to-Jet mapping; the original source remains authoritative",
            format!(
                "run `jet inspect bind pascal {} --pkg {library}` for call-in-place bindings, then port unsupported bodies explicitly",
                source.display()
            ),
        )
    } else {
        output.push_str("// TODO JT0101: Pascal source has no recognizable library declaration.\n");
        (
            "Pascal source has no recognizable library declaration".into(),
            "the importer cannot establish a library boundary or ABI contract",
            "add a Pascal `library` declaration or use a hand-written Jet FFI overlay; no source was silently emitted".to_string(),
        )
    };
    output.push_str("// The Pascal source remains the canonical source of truth.\n");
    Translation {
        source: output,
        todos: vec![gap(what, why, fix, source, target, 1)],
        functions: 0,
        tests: 0,
    }
}

fn translate_ada_file(raw: &str, source: &Path, target: &Path) -> Translation {
    let package = ada_package_name(raw);
    let mut output = header("ada");
    output.push_str(&format!("// Source: {}\n", source.display()));
    let (what, why, fix) = if let Some(package) = &package {
        output.push_str(&format!("use ada.{package} as {package}\n\n"));
        output.push_str(&format!("// Binder stub: use ada.{package} as {package}\n"));
        output.push_str(&format!(
            "// TODO JT0101: Ada package `{package}` remains an FFI source.\n"
        ));
        (
            format!("Ada package `{package}` was not translated"),
            "Ada bodies, range checks, exceptions, tasking, and representation clauses have no proven source-to-Jet mapping; the original package spec and body remain authoritative",
            format!(
                "run `jet inspect bind ada {} --pkg {package}` for call-in-place bindings, then port unsupported bodies explicitly",
                source.display()
            ),
        )
    } else {
        output.push_str("// TODO JT0101: Ada source has no recognizable package declaration.\n");
        (
            "Ada source has no recognizable package declaration".into(),
            "the importer cannot establish a library boundary or ABI contract",
            "add a top-level Ada `package` declaration or use a hand-written Jet FFI overlay; no source was silently emitted".to_string(),
        )
    };
    output.push_str("// The Ada source remains the canonical source of truth.\n");
    Translation {
        source: output,
        todos: vec![gap(what, why, fix, source, target, 1)],
        functions: 0,
        tests: 0,
    }
}

/// Translate the safe, deliberately small enterprise subset shared by the
/// source importers. The parsers accept straight-line scalar functions and
/// leave object/runtime-heavy constructs with a TODO instead of guessing.
fn translate_enterprise_file(
    language: &str,
    raw: &str,
    source: &Path,
    target: &Path,
) -> Translation {
    let mut output = header(language);
    output.push_str(&format!("// Source: {}\n", source.display()));
    output.push_str(
        "// Provenance: law=D-MIGRATE-SRC1; generated spans remain tied to the source file.\n",
    );

    let lines: Vec<&str> = raw.lines().collect();
    let mut rendered: Vec<Option<(String, bool, String)>> = Vec::new();
    let mut todos = Vec::new();
    let mut names = BTreeSet::new();
    let mut rendered_by_name: BTreeMap<String, usize> = BTreeMap::new();
    let mut functions = 0;
    let mut tests = 0;
    let float_literals = language == "ts";
    let mut at = 0;

    while at < lines.len() {
        let trimmed = lines[at].trim();
        if trimmed.is_empty() || foreign_comment_line(trimmed) {
            at += 1;
            continue;
        }
        if foreign_ignored_line(language, trimmed) {
            at += 1;
            continue;
        }
        if foreign_import_line(language, trimmed) {
            if !foreign_supported_import(language, trimmed) {
                todos.push(gap(
                    format!("unsupported {language} dependency declaration"),
                    "the importer does not execute or infer foreign dependency behavior",
                    format!(
                        "bind the dependency with `jet inspect bind {language}` or port it explicitly before rerunning import"
                    ),
                    source,
                    target,
                    at + 1,
                ));
            }
            at += 1;
            continue;
        }

        let mut signature = trimmed.to_string();
        let mut open_line = at;
        if foreign_function_candidate(language, trimmed)
            && !has_unquoted_char(trimmed, '{')
            && lines.get(at + 1).is_some_and(|next| next.trim() == "{")
            && (!trimmed.contains("=>") || trimmed.trim_end().ends_with("=>"))
        {
            signature.push_str(" {");
            open_line += 1;
        }

        if foreign_function_candidate(language, trimmed) {
            match parse_foreign_header(language, &signature) {
                Ok(mut header) => {
                    let expression_body = header.expression_body.take();
                    let is_expression_body = expression_body.is_some();
                    let (body, end_line) = if let Some(expression) = expression_body {
                        (vec![(at + 1, expression)], at)
                    } else {
                        match collect_foreign_body(&lines, open_line) {
                            Ok(body) => body,
                            Err(reason) => {
                                todos.push(gap(
                                    format!("malformed {language} function at line {}", at + 1),
                                    reason,
                                    "repair the source function and rerun import; the original source was preserved",
                                    source,
                                    target,
                                    at + 1,
                                ));
                                at += 1;
                                continue;
                            }
                        }
                    };
                    header.function.body = body;
                    if language == "js" {
                        let jsdoc = javascript_doc_types(&lines, at);
                        if let Err(reason) = infer_javascript_types(&mut header.function, &jsdoc) {
                            todos.push(gap(
                                format!(
                                    "JavaScript function `{}` was not translated",
                                    header.function.name
                                ),
                                reason,
                                "add a supported scalar JSDoc type or port the function explicitly",
                                source,
                                target,
                                at + 1,
                            ));
                            at = end_line + 1;
                            continue;
                        }
                    }
                    let function_name = header.function.name.clone();
                    let canonical_name = canonical_identifier(&function_name);
                    if !names.insert(canonical_name.clone()) {
                        if let Some(index) = rendered_by_name.remove(&canonical_name) {
                            let prior: Option<(String, bool, String)> = rendered[index].take();
                            if let Some((_, carried_test, _)) = prior {
                                functions -= 1;
                                if carried_test {
                                    tests -= 1;
                                }
                            }
                        }
                        todos.push(gap(
                            format!("ambiguous overloaded {language} function `{function_name}`"),
                            "multiple source functions would share one Jet name and call shape",
                            "rename or manually port the overloads behind one explicit Jet API",
                            source,
                            target,
                            at + 1,
                        ));
                        at = end_line + 1;
                        continue;
                    }
                    let float_context = header.function.params.iter().any(|(_, ty)| ty == "Float")
                        || header.function.result == "Float";
                    match translate_foreign_body(
                        language,
                        &header.function,
                        float_literals || float_context,
                        is_expression_body,
                    ) {
                        Ok(body) => {
                            let carried_test = header.function.name.starts_with("test_")
                                && header.function.params.is_empty();
                            let marker = if carried_test {
                                tests += 1;
                                "#Test "
                            } else {
                                ""
                            };
                            let rendered_index = rendered.len();
                            rendered_by_name.insert(canonical_name, rendered_index);
                            rendered.push(Some((
                                header.function.name.clone(),
                                carried_test,
                                format!(
                                    "// Source span: {}:{}-{}\n{}",
                                    source.display(),
                                    at + 1,
                                    end_line + 1,
                                    render_function(marker, &header.function, &body)
                                ),
                            )));
                            functions += 1;
                        }
                        Err((line, reason)) => todos.push(gap(
                            format!(
                                "{language} body `{}` was not translated",
                                header.function.name
                            ),
                            reason,
                            "port the body into ordinary Jet; the original source remains authoritative",
                            source,
                            target,
                            line,
                        )),
                    }
                    at = end_line + 1;
                    continue;
                }
                Err(reason) => {
                    let end_line = if has_unquoted_char(&signature, '{') {
                        collect_foreign_body(&lines, open_line)
                            .map(|(_, line)| line)
                            .unwrap_or(at)
                    } else {
                        at
                    };
                    todos.push(gap(
                        format!("unsupported {language} function declaration"),
                        reason,
                        "use the supported scalar function subset or port the declaration explicitly",
                        source,
                        target,
                        at + 1,
                    ));
                    at = end_line + 1;
                    continue;
                }
            }
        }

        if foreign_structural_line(trimmed) {
            at += 1;
            continue;
        }
        todos.push(gap(
            format!("unsupported {language} top-level construct `{}`", shorten(trimmed)),
            "the importer only emits proven scalar functions and does not model foreign runtime state",
            "port the construct into ordinary Jet or keep it behind the language binder",
            source,
            target,
            at + 1,
        ));
        at += 1;
    }

    if functions == 0 {
        if let Some(unit) = enterprise_unit_name(language, raw, source) {
            let root = foreign_binder_root(language);
            output.push_str(&format!("use {root}.{unit} as {unit}\n\n"));
            output.push_str(&format!(
                "// Binder fallback: use {root}.{unit} as {unit}\n"
            ));
        }
        output.push_str("// No construct was safe to translate. See import-report.json.\n");
    }
    for (index, (_, _, item)) in rendered.into_iter().flatten().enumerate() {
        if index == 0 {
            output.push('\n');
        }
        output.push_str(&item);
    }
    output.push_str(&format!(
        "\n// The {language} source remains the canonical source of truth until every TODO is resolved.\n"
    ));
    Translation {
        source: output,
        todos,
        functions,
        tests,
    }
}

struct ForeignHeader {
    function: Function,
    expression_body: Option<String>,
}

fn foreign_comment_line(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with('*')
        || line.starts_with("<!--")
        || line.starts_with("*/")
        || (line.starts_with("/*")
            && line
                .find("*/")
                .is_none_or(|end| line[end + 2..].trim().is_empty()))
}

fn foreign_function_candidate(language: &str, line: &str) -> bool {
    match language {
        "java" | "csharp" => {
            line.contains('(')
                && !line.starts_with("if ")
                && !line.starts_with("if(")
                && !line.starts_with("for ")
                && !line.starts_with("for(")
                && !line.starts_with("while ")
                && !line.starts_with("while(")
                && !line.starts_with("switch ")
                && !line.starts_with("switch(")
                && !line.starts_with("catch ")
                && !line.starts_with("catch(")
        }
        "ts" | "js" => line.contains("function") || line.contains("=>"),
        "go" => line.starts_with("func "),
        _ => false,
    }
}

fn parse_foreign_header(language: &str, line: &str) -> Result<ForeignHeader, String> {
    let open = line
        .find('(')
        .ok_or_else(|| "function declaration has no parameter list".to_string())?;
    let close = line
        .rfind(')')
        .filter(|close| *close >= open)
        .ok_or_else(|| "function declaration has no complete parameter list".to_string())?;
    let prefix = line[..open].trim();
    let suffix = line[close + 1..].trim();
    let arrow = matches!(language, "ts" | "js") && line.contains("=>");
    let (name, result_text, require_static) = match language {
        "java" | "csharp" => {
            let tokens: Vec<&str> = prefix.split_whitespace().collect();
            let name = tokens
                .last()
                .copied()
                .ok_or_else(|| "function declaration has no name".to_string())?;
            if !identifier(name) {
                return Err(format!(
                    "function name `{name}` is not a safe Jet identifier"
                ));
            }
            if tokens.iter().any(|token| *token == "async") {
                return Err("async methods need an explicit migration adapter".into());
            }
            if !tokens.iter().any(|token| *token == "public") {
                return Err(
                    "only public static methods have a proven source-to-Jet mapping".into(),
                );
            }
            if tokens.iter().any(|token| {
                matches!(
                    *token,
                    "abstract"
                        | "extern"
                        | "in"
                        | "out"
                        | "override"
                        | "ref"
                        | "synchronized"
                        | "unsafe"
                        | "virtual"
                )
            }) || suffix.starts_with("throws ")
                || suffix.contains(" where ")
            {
                return Err(
                    "method modifiers or exception contracts need an explicit migration adapter"
                        .into(),
                );
            }
            let result = tokens
                .get(tokens.len().saturating_sub(2))
                .copied()
                .ok_or_else(|| "method declaration has no return type".to_string())?;
            (name.to_string(), result.to_string(), true)
        }
        "ts" | "js" if arrow => {
            if prefix.contains("async") {
                return Err("async arrow functions need an explicit migration adapter".into());
            }
            let equals = prefix
                .rfind('=')
                .ok_or_else(|| "arrow function has no bound name".to_string())?;
            let name = prefix[..equals]
                .split_whitespace()
                .last()
                .ok_or_else(|| "arrow function has no bound name".to_string())?;
            if !identifier(name) {
                return Err(format!(
                    "function name `{name}` is not a safe Jet identifier"
                ));
            }
            let result = if let Some(annotation) = suffix.strip_prefix(':') {
                annotation
                    .split_once("=>")
                    .map(|(ty, _)| ty.trim().to_string())
                    .filter(|ty| !ty.is_empty())
                    .ok_or_else(|| "arrow function has no complete return annotation".to_string())?
            } else if language == "ts" {
                return Err("TypeScript arrow function needs a return type".into());
            } else {
                "__js__".into()
            };
            (name.to_string(), result, false)
        }
        "ts" | "js" => {
            let marker = prefix
                .rfind("function")
                .ok_or_else(|| "function declaration has no function keyword".to_string())?;
            let before = prefix[..marker].trim();
            if before.split_whitespace().any(|token| token == "async") {
                return Err("async functions need an explicit migration adapter".into());
            }
            let after = prefix[marker + "function".len()..].trim();
            if after.starts_with('*') {
                return Err("generator functions need an explicit migration adapter".into());
            }
            let name = after
                .split_whitespace()
                .next()
                .ok_or_else(|| "function declaration has no name".to_string())?;
            if !identifier(name) {
                return Err(format!(
                    "function name `{name}` is not a safe Jet identifier"
                ));
            }
            let result = if let Some(annotation) = suffix.strip_prefix(':') {
                annotation
                    .split('{')
                    .next()
                    .unwrap_or_default()
                    .split("=>")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            } else if language == "ts" {
                return Err("TypeScript function needs a return type".into());
            } else {
                "__js__".into()
            };
            (name.to_string(), result, false)
        }
        "go" => {
            let after = prefix
                .strip_prefix("func")
                .map(str::trim)
                .ok_or_else(|| "Go function needs `func`".to_string())?;
            if after.starts_with('(') {
                return Err("Go methods with receivers need an explicit migration adapter".into());
            }
            let name = after
                .split_whitespace()
                .next()
                .ok_or_else(|| "Go function declaration has no name".to_string())?;
            if !identifier(name) {
                return Err(format!(
                    "function name `{name}` is not a safe Jet identifier"
                ));
            }
            let result = suffix.split('{').next().unwrap_or_default().trim();
            (name.to_string(), result.to_string(), false)
        }
        _ => return Err(format!("no source importer for {language}")),
    };
    if require_static && !prefix.split_whitespace().any(|token| token == "static") {
        return Err("only static Java/C# methods have a proven source-to-Jet mapping".into());
    }
    if prefix.contains('<') || prefix.contains('>') {
        return Err("generic functions need an explicit migration adapter".into());
    }
    let params = parse_foreign_params(language, &line[open + 1..close])?;
    let result = if result_text.is_empty() {
        "()".into()
    } else {
        map_foreign_type(language, &result_text)?
    };
    let expression_body = if arrow {
        suffix.split_once("=>").and_then(|(_, expression)| {
            let expression = expression.trim();
            if expression.is_empty() || expression.starts_with('{') {
                None
            } else {
                Some(expression.trim_end_matches(';').to_string())
            }
        })
    } else {
        suffix
            .strip_prefix("=>")
            .map(|expression| expression.trim().trim_end_matches(';').to_string())
    };
    if expression_body.is_none() && !suffix.contains('{') {
        return Err("function declaration needs a body".into());
    }
    let name = if name == "main" && params.is_empty() && result == "()" {
        "run".into()
    } else {
        name
    };
    Ok(ForeignHeader {
        function: Function {
            name,
            params,
            result,
            body: Vec::new(),
        },
        expression_body,
    })
}

fn parse_foreign_params(language: &str, raw: &str) -> Result<Vec<(String, String)>, String> {
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    if language == "go" {
        return parse_go_params(raw);
    }
    split_commas(raw)
        .into_iter()
        .map(|part| {
            let part = part.trim();
            if part.contains("...") || part.contains('?') || part.contains('=') {
                return Err(
                    "optional, default, or variadic parameters need an explicit migration adapter"
                        .into(),
                );
            }
            match language {
                "ts" => {
                    let (name, ty) = part
                        .split_once(':')
                        .ok_or_else(|| "TypeScript parameter needs a type".to_string())?;
                    let name = name.trim();
                    if !identifier(name) {
                        return Err(format!("parameter `{name}` is not a safe Jet identifier"));
                    }
                    Ok((name.to_string(), map_foreign_type(language, ty.trim())?))
                }
                "js" => {
                    if !identifier(part) {
                        return Err(format!("parameter `{part}` is not a safe Jet identifier"));
                    }
                    Ok((part.to_string(), "__js__".into()))
                }
                _ => {
                    let tokens: Vec<&str> = part.split_whitespace().collect();
                    if tokens.len() < 2 {
                        return Err(
                            "Java/C# parameters must use one scalar type and one name".into()
                        );
                    }
                    let name = tokens[tokens.len() - 1];
                    if !identifier(name)
                        || tokens[..tokens.len() - 1]
                            .iter()
                            .any(|token| *token == "ref" || *token == "out" || *token == "in")
                    {
                        return Err(format!("parameter `{name}` has no safe scalar mapping"));
                    }
                    let ty = tokens[tokens.len() - 2];
                    Ok((name.to_string(), map_foreign_type(language, ty)?))
                }
            }
        })
        .collect()
}

fn parse_go_params(raw: &str) -> Result<Vec<(String, String)>, String> {
    let mut pending = Vec::new();
    let mut params = Vec::new();
    let mut names = BTreeSet::new();
    for part in split_commas(raw) {
        let tokens: Vec<&str> = part.split_whitespace().collect();
        match tokens.as_slice() {
            [name] if identifier(name) => pending.push((*name).to_string()),
            [name, ty] if identifier(name) => {
                let mapped = map_foreign_type("go", ty)?;
                pending.push((*name).to_string());
                for name in pending.drain(..) {
                    if !names.insert(name.clone()) {
                        return Err(format!("duplicate Go parameter `{name}`"));
                    }
                    params.push((name, mapped.clone()));
                }
            }
            _ => {
                return Err(
                    "Go parameters must use one or more names followed by one scalar type".into(),
                )
            }
        }
    }
    if !pending.is_empty() {
        return Err("Go parameter names need a scalar type".into());
    }
    Ok(params)
}

fn map_foreign_type(language: &str, raw: &str) -> Result<String, String> {
    let ty = raw.trim().trim_end_matches(';');
    let ty = ty.strip_prefix("java.lang.").unwrap_or(ty);
    let ty = ty.strip_prefix("System.").unwrap_or(ty);
    if ty.contains('[') || ty.contains('*') || ty.contains('&') {
        return Err(format!(
            "foreign type `{raw}` is outside the scalar importer subset"
        ));
    }
    match language {
        "java" => match ty {
            "byte" | "short" | "int" | "long" => Ok("Int".into()),
            "float" | "double" => Ok("Float".into()),
            "boolean" => Ok("Bool".into()),
            "String" => Ok("String".into()),
            "void" => Ok("()".into()),
            _ => Err(format!("Java type `{raw}` has no proven Jet mapping")),
        },
        "csharp" => match ty {
            "byte" | "short" | "int" | "long" | "Int32" | "Int64" => Ok("Int".into()),
            "float" | "double" | "Single" | "Double" => Ok("Float".into()),
            "bool" | "Boolean" => Ok("Bool".into()),
            "string" | "String" => Ok("String".into()),
            "void" => Ok("()".into()),
            _ => Err(format!("C# type `{raw}` has no proven Jet mapping")),
        },
        "ts" => match ty {
            "number" => Ok("Float".into()),
            "boolean" => Ok("Bool".into()),
            "string" => Ok("String".into()),
            "void" => Ok("()".into()),
            _ => Err(format!("TypeScript type `{raw}` has no proven Jet mapping")),
        },
        "go" => match ty {
            "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16"
            | "uint32" | "uint64" => Ok("Int".into()),
            "float32" | "float64" => Ok("Float".into()),
            "bool" => Ok("Bool".into()),
            "string" => Ok("String".into()),
            _ => Err(format!("Go type `{raw}` has no proven Jet mapping")),
        },
        "js" => Ok("__js__".into()),
        _ => Err(format!("no scalar type mapping for {language}")),
    }
}

fn translate_foreign_body(
    language: &str,
    function: &Function,
    float_literals: bool,
    expression_body: bool,
) -> Result<Vec<String>, (usize, String)> {
    if function.body.is_empty() {
        return if function.result == "()" {
            Ok(Vec::new())
        } else {
            Err((1, "a value-returning function has an empty body".into()))
        };
    }
    let mut bindings = BTreeSet::new();
    let mut output = Vec::new();
    let mut returned = false;
    for (line_no, raw) in &function.body {
        for statement in split_foreign_statements(raw) {
            let line = strip_foreign_line_comment(&statement)
                .trim()
                .trim_end_matches(';')
                .trim();
            if line.is_empty() {
                continue;
            }
            if foreign_control_statement(line) {
                return Err((
                    *line_no,
                    "control flow, exceptions, and foreign runtime operations have no proven mapping in this importer".into(),
                ));
            }
            if line == "return" {
                returned = true;
                if function.result == "()" {
                    output.push("return".into());
                    continue;
                }
                return Err((
                    *line_no,
                    "value-returning function has no return value".into(),
                ));
            }
            if let Some(value) = line.strip_prefix("return ") {
                returned = true;
                if function.result == "()" {
                    return Err((*line_no, "unit function cannot return a value".into()));
                }
                output.push(format!(
                    "return {}",
                    materialize_borrowed_string(
                        function,
                        value,
                        foreign_expression(language, value, float_literals)
                            .map_err(|reason| (*line_no, reason))?,
                    )
                ));
                continue;
            }
            if let Some(value) = line.strip_prefix("assert ") {
                let (left, right) = split_top(value, "==").ok_or_else(|| {
                    (
                        *line_no,
                        "only equality assertions have a proven Jet mapping".into(),
                    )
                })?;
                output.push(format!(
                    "assert_eq({}, {})",
                    foreign_expression(language, left, float_literals)
                        .map_err(|reason| (*line_no, reason))?,
                    foreign_expression(language, right, float_literals)
                        .map_err(|reason| (*line_no, reason))?
                ));
                continue;
            }

            if let Some((left, right)) = split_top(line, ":=") {
                let name = canonical_identifier(left.trim());
                if !identifier(&name) || name == "_" {
                    return Err((
                        *line_no,
                        "binding target is not a safe Jet identifier".into(),
                    ));
                }
                let value = materialize_borrowed_string(
                    function,
                    right,
                    foreign_expression(language, right, float_literals)
                        .map_err(|reason| (*line_no, reason))?,
                );
                let operator = if bindings.insert(name.clone()) {
                    ":="
                } else {
                    "="
                };
                output.push(format!("{name} {operator} {value}"));
                continue;
            }
            if let Some((left, right)) = split_top(line, "=") {
                let left = left.trim();
                let name = foreign_local_name(language, left)
                    .or_else(|| identifier(left).then(|| left.to_string()));
                let Some(name) = name else {
                    return Err((
                        *line_no,
                        "assignment or declaration has no safe scalar mapping".into(),
                    ));
                };
                let value = materialize_borrowed_string(
                    function,
                    right,
                    foreign_expression(language, right, float_literals)
                        .map_err(|reason| (*line_no, reason))?,
                );
                let operator = if bindings.insert(name.clone()) {
                    ":="
                } else {
                    "="
                };
                output.push(format!("{name} {operator} {value}"));
                continue;
            }
            output.push(
                foreign_expression(language, line, float_literals)
                    .map_err(|reason| (*line_no, reason))?,
            );
        }
    }
    if function.result != "()" && !expression_body && !returned {
        let line = function.body.last().map(|(line, _)| *line).unwrap_or(1);
        return Err((
            line,
            "value-returning function has no return statement".into(),
        ));
    }
    Ok(output)
}

fn split_foreign_statements(raw: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in raw.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if matches!(character, '(' | '[') {
            depth += 1;
        } else if matches!(character, ')' | ']') {
            depth -= 1;
        } else if character == ';' && depth == 0 {
            statements.push(raw[start..index].to_string());
            start = index + character.len_utf8();
        }
    }
    if start < raw.len() {
        statements.push(raw[start..].to_string());
    }
    statements
}

fn foreign_control_statement(line: &str) -> bool {
    [
        "if ", "if(", "for ", "for(", "while ", "while(", "switch ", "switch(", "try", "catch",
        "finally", "throw ", "defer ", "go ", "select", "else", "case ", "default:",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
        || line.contains("++")
        || line.contains("--")
}

fn foreign_local_name(language: &str, left: &str) -> Option<String> {
    let tokens: Vec<&str> = left.split_whitespace().collect();
    let name = match language {
        "ts" | "js" => tokens
            .strip_prefix(&["const"])
            .or_else(|| tokens.strip_prefix(&["let"]))
            .or_else(|| tokens.strip_prefix(&["var"]))
            .and_then(|tokens| tokens.first().copied())
            .map(|name| name.trim_end_matches(':')),
        "go" => tokens
            .strip_prefix(&["var"])
            .and_then(|tokens| tokens.first().copied()),
        "java" | "csharp" => tokens.last().copied().filter(|_| {
            tokens.len() >= 2
                && (tokens[..tokens.len() - 1]
                    .iter()
                    .any(|token| *token == "var")
                    || map_foreign_type(language, tokens[tokens.len() - 2]).is_ok())
        }),
        _ => None,
    }?;
    identifier(name).then(|| canonical_identifier(name))
}

fn foreign_expression(language: &str, raw: &str, float_literals: bool) -> Result<String, String> {
    let mut expression = raw.trim().trim_end_matches(';').trim().to_string();
    if expression.contains('`') {
        return Err("raw/template strings need an explicit migration adapter".into());
    }
    if matches!(language, "java" | "csharp" | "ts" | "js" | "go")
        && (has_unquoted_char(&expression, '/') || has_unquoted_char(&expression, '%'))
    {
        return Err(
            "foreign division or remainder semantics need an explicit migration adapter".into(),
        );
    }
    if matches!(language, "java" | "csharp" | "go")
        && has_unquoted_single_quote(&expression)
    {
        return Err("foreign character literals need an explicit migration adapter".into());
    }
    if matches!(language, "ts" | "js") {
        expression = expression.replace("!==", "!=").replace("===", "==");
    }
    for (foreign, jet) in [
        ("System.out.println(", "print("),
        ("System.Console.WriteLine(", "print("),
        ("Console.WriteLine(", "print("),
        ("fmt.Println(", "print("),
        ("println(", "print("),
        ("console.log(", "print("),
    ] {
        expression = replace_unquoted(&expression, foreign, jet);
    }
    parse_expression_with_float(&expression, float_literals)
}

fn materialize_borrowed_string(function: &Function, source: &str, expression: String) -> String {
    let source = source.trim().trim_end_matches(';').trim();
    let borrowed_string = function.params.iter().any(|(name, ty)| {
        ty == "String" && canonical_identifier(name) == canonical_identifier(source)
    });
    if borrowed_string && !expression.starts_with(['~', '^']) {
        format!("~{expression}")
    } else {
        expression
    }
}

fn strip_foreign_line_comment(raw: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let bytes = raw.as_bytes();
    for (index, character) in raw.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == '/' && bytes.get(index + 1) == Some(&b'/') {
            return &raw[..index];
        }
    }
    raw
}

fn replace_unquoted(text: &str, needle: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut quote = None;
    let mut escaped = false;
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let character = rest
            .chars()
            .next()
            .expect("text slice must have a leading character");
        if let Some(active) = quote {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
            at += character.len_utf8();
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            output.push(character);
            at += character.len_utf8();
        } else if rest.starts_with(needle) {
            output.push_str(replacement);
            at += needle.len();
        } else {
            output.push(character);
            at += character.len_utf8();
        }
    }
    output
}

fn has_unquoted_char(text: &str, needle: char) -> bool {
    find_unquoted(text, needle).is_some()
}

fn has_unquoted_single_quote(text: &str) -> bool {
    let mut quote = None;
    let mut escaped = false;
    for character in text.chars() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
        } else if character == '\'' {
            return true;
        } else if matches!(character, '"' | '`') {
            quote = Some(character);
        }
    }
    false
}

fn foreign_binder_root(language: &str) -> &str {
    match language {
        "csharp" => "cs",
        "ts" | "js" => "js",
        other => other,
    }
}

fn collect_foreign_body(
    lines: &[&str],
    open_line: usize,
) -> Result<(Vec<(usize, String)>, usize), String> {
    let opening = lines
        .get(open_line)
        .ok_or_else(|| "function body is missing".to_string())?;
    let open = find_unquoted(opening, '{').ok_or_else(|| "function body needs `{`".to_string())?;
    let mut level = 0i32;
    let mut body = Vec::new();
    for (index, raw) in lines.iter().enumerate().skip(open_line) {
        let fragment = if index == open_line {
            &raw[open + 1..]
        } else {
            raw
        };
        let before = level;
        let delta = brace_delta(fragment);
        let content = strip_unquoted_braces(fragment).trim().to_string();
        if !content.is_empty() && (index != open_line || before > 0 || delta < 0) {
            body.push((index + 1, content));
        }
        level += delta;
        if index == open_line {
            level += 1;
        }
        if level == 0 {
            return Ok((body, index));
        }
    }
    Err("function body has unmatched braces".into())
}

fn find_unquoted(line: &str, wanted: char) -> Option<usize> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == wanted {
            return Some(index);
        }
    }
    None
}

fn brace_delta(line: &str) -> i32 {
    let mut quote = None;
    let mut escaped = false;
    let mut delta = 0;
    for character in line.chars() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        } else if character == '{' {
            delta += 1;
        } else if character == '}' {
            delta -= 1;
        }
    }
    delta
}

fn strip_unquoted_braces(line: &str) -> String {
    let mut output = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in line.chars() {
        if let Some(active) = quote {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == active {
                quote = None;
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            output.push(character);
        } else if !matches!(character, '{' | '}') {
            output.push(character);
        }
    }
    output
}

fn foreign_ignored_line(language: &str, line: &str) -> bool {
    if line == "{" || line == "}" || line == ");" || line == ")" {
        return true;
    }
    match language {
        "java" => line.starts_with("package "),
        "csharp" => line.starts_with("namespace ") || line == "using System;",
        "ts" | "js" => false,
        "go" => line.starts_with("package ") || line == "import (" || line == ")",
        _ => false,
    }
}

fn foreign_import_line(language: &str, line: &str) -> bool {
    match language {
        "java" => line.starts_with("import "),
        "csharp" => line.starts_with("using "),
        "ts" | "js" => line.starts_with("import ") || line.starts_with("export *"),
        "go" => line.starts_with("import ") || (line.starts_with('"') && line.ends_with('"')),
        _ => false,
    }
}

fn foreign_supported_import(language: &str, line: &str) -> bool {
    match language {
        "csharp" => line == "using System;",
        "go" => line == "import (" || line == "import \"fmt\"" || line == "\"fmt\"",
        _ => false,
    }
}

fn foreign_structural_line(line: &str) -> bool {
    let tokens: Vec<&str> = line.trim_matches(['{', '}']).split_whitespace().collect();
    tokens.iter().any(|token| {
        matches!(
            *token,
            "package" | "namespace" | "class" | "interface" | "struct" | "enum" | "record"
        )
    }) || line == "{"
        || line == "}"
}

#[derive(Default)]
struct JavaScriptDocs {
    params: BTreeMap<String, String>,
    result: Option<String>,
    error: Option<String>,
}

fn javascript_doc_types(lines: &[&str], at: usize) -> JavaScriptDocs {
    let mut comments = Vec::new();
    let mut cursor = at;
    while cursor > 0 {
        let line = lines[cursor - 1].trim();
        if line.is_empty() {
            break;
        }
        if !foreign_comment_line(line) {
            break;
        }
        comments.push(line);
        cursor -= 1;
    }
    comments.reverse();

    let mut docs = JavaScriptDocs::default();
    for line in comments {
        for tag in ["@param", "@arg", "@argument"] {
            if let Some(rest) = line.split_once(tag).map(|(_, rest)| rest) {
                if let Some((ty, name)) = parse_javascript_param_doc(rest) {
                    match map_javascript_doc_type(ty) {
                        Ok(mapped) => {
                            docs.params.insert(name.to_string(), mapped);
                        }
                        Err(reason) => docs.error = Some(reason),
                    }
                } else {
                    docs.error = Some("JavaScript parameter JSDoc needs `{type} name`".into());
                }
                break;
            }
        }
        for tag in ["@returns", "@return"] {
            if let Some(rest) = line.split_once(tag).map(|(_, rest)| rest) {
                let Some(ty) = rest
                    .trim()
                    .strip_prefix('{')
                    .and_then(|rest| rest.split_once('}').map(|(ty, _)| ty.trim()))
                else {
                    docs.error = Some("JavaScript return JSDoc needs a type".into());
                    break;
                };
                match map_javascript_doc_type(ty) {
                    Ok(mapped) => docs.result = Some(mapped),
                    Err(reason) => docs.error = Some(reason),
                }
                break;
            }
        }
    }
    docs
}

fn parse_javascript_param_doc(raw: &str) -> Option<(&str, &str)> {
    let raw = raw.trim();
    let raw = raw.strip_prefix('{')?;
    let (ty, rest) = raw.split_once('}')?;
    let name = rest.split_whitespace().next()?.trim_matches(['[', ']']);
    identifier(name).then_some((ty.trim(), name))
}

fn map_javascript_doc_type(raw: &str) -> Result<String, String> {
    match raw.trim() {
        "number" => Ok("Float".into()),
        "integer" | "int" => Ok("Int".into()),
        "boolean" | "bool" => Ok("Bool".into()),
        "string" => Ok("String".into()),
        "void" | "undefined" => Ok("()".into()),
        other => Err(format!(
            "JavaScript JSDoc type `{other}` has no proven Jet mapping"
        )),
    }
}

fn infer_javascript_types(function: &mut Function, docs: &JavaScriptDocs) -> Result<(), String> {
    if let Some(error) = &docs.error {
        return Err(error.clone());
    }
    let body = function
        .body
        .iter()
        .map(|(_, line)| line.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let mut known = BTreeMap::new();
    for (name, ty) in &mut function.params {
        if ty == "__js__" {
            *ty = docs
                .params
                .get(name)
                .cloned()
                .or_else(|| infer_javascript_parameter_type(&body, name))
                .unwrap_or_else(|| "__js__".into());
        }
        if ty != "__js__" {
            if ty == "()" {
                return Err(format!(
                    "JavaScript parameter `{name}` cannot have unit type"
                ));
            }
            known.insert(name.clone(), ty.clone());
        }
    }
    if function.result == "__js__" {
        function.result = if let Some(result) = &docs.result {
            result.clone()
        } else if let Some(return_value) = body
            .split(';')
            .find_map(|statement| statement.trim().strip_prefix("return "))
        {
            infer_javascript_value_type(return_value, &known).unwrap_or_else(|| "__js__".into())
        } else {
            "()".into()
        };
    } else if let Some(result) = &docs.result {
        if result != &function.result {
            return Err("JavaScript JSDoc return type conflicts with the declaration".into());
        }
    }
    if function.params.iter().any(|(_, ty)| ty == "__js__") || function.result == "__js__" {
        return Err("JavaScript value type is ambiguous".into());
    }
    Ok(())
}

fn infer_javascript_parameter_type(body: &str, name: &str) -> Option<String> {
    let string_concat = body.split('+').any(|piece| {
        contains_identifier(piece, name)
            && body
                .split('+')
                .any(|other| other != piece && contains_quote(other))
    });
    if string_concat {
        Some("String".into())
    } else if body.contains(&format!("{name} === true"))
        || body.contains(&format!("true === {name}"))
        || body.contains(&format!("{name} === false"))
        || body.contains(&format!("false === {name}"))
    {
        Some("Bool".into())
    } else {
        None
    }
}

fn contains_identifier(text: &str, wanted: &str) -> bool {
    text.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|part| part == wanted)
}

fn infer_javascript_value_type(
    expression: &str,
    known: &BTreeMap<String, String>,
) -> Option<String> {
    let expression = expression
        .trim()
        .trim_end_matches(';')
        .trim()
        .replace("!==", "!=")
        .replace("===", "==");
    if let Some(ty) = known.get(&expression) {
        return Some(ty.clone());
    }
    let tokens = lex(&expression).ok()?;
    let mut saw_value = false;
    let mut has_numeric = false;
    let mut has_float = false;
    let mut has_string = false;
    let mut has_comparison = false;
    let mut has_boolean = false;
    let mut has_arithmetic = false;
    for token in tokens {
        match token {
            Token::Word(word) => match word.as_str() {
                "true" | "false" => {
                    saw_value = true;
                    has_boolean = true;
                }
                _ => {
                    let ty = known.get(&word)?;
                    saw_value = true;
                    match ty.as_str() {
                        "Float" => {
                            has_float = true;
                            has_numeric = true;
                        }
                        "String" => has_string = true,
                        "Bool" => has_boolean = true,
                        "Int" => has_numeric = true,
                        _ => return None,
                    }
                }
            },
            Token::Number(value) => {
                saw_value = true;
                has_numeric = true;
                has_float |= value.contains('.');
            }
            Token::Text(_) => {
                saw_value = true;
                has_string = true;
            }
            Token::Op(op) => match op.as_str() {
                "==" | "!=" | "<" | ">" | "<=" | ">=" => has_comparison = true,
                "&&" | "||" | "!" => has_boolean = true,
                "+" | "-" | "*" | "/" | "%" => has_arithmetic = true,
                _ => return None,
            },
            Token::Open | Token::Close => {}
            Token::Comma => return None,
        }
    }
    if !saw_value {
        return None;
    }
    if has_comparison || (has_boolean && !has_arithmetic) {
        Some("Bool".into())
    } else if has_string {
        Some("String".into())
    } else if has_arithmetic || has_float || has_numeric {
        Some(if has_float { "Float" } else { "Int" }.into())
    } else {
        None
    }
}

fn contains_quote(value: &str) -> bool {
    value.contains('"') || value.contains('\'')
}

/// Recover the declared module boundary for one enterprise source file.
fn enterprise_unit_name(language: &str, raw: &str, source: &Path) -> Option<String> {
    let keyword = match language {
        "java" => "package",
        "csharp" => "namespace",
        "go" => "package",
        "ts" | "js" => {
            return source
                .file_stem()
                .and_then(|name| name.to_str())
                .filter(|name| identifier(name))
                .map(str::to_string)
        }
        _ => return None,
    };
    for line in raw.lines() {
        let line = line.trim_start();
        if line.starts_with("//") || line.starts_with("/*") || line.starts_with('*') {
            continue;
        }
        let Some(rest) = line.strip_prefix(keyword) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let name: String = rest
            .trim()
            .trim_end_matches(';')
            .trim_end_matches('{')
            .trim()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
            .collect();
        let name = name.rsplit('.').next().unwrap_or_default();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn header(language: &str) -> String {
    format!("// Generated by jet import {language} (D-MIGRATE-SRC1). Editable Jet source.\n")
}

fn pascal_library_name(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim_start();
        if line.starts_with("//") || line.starts_with('{') || line.starts_with("(*") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let Some(tail) = lower.strip_prefix("library ") else {
            continue;
        };
        let name = tail
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .next()
            .filter(|value| identifier(value))?;
        return Some(name.to_ascii_lowercase());
    }
    None
}

fn ada_package_name(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim_start();
        if line.starts_with("--") {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        let Some(tail) = lower.strip_prefix("package ") else {
            continue;
        };
        if tail.starts_with("body ") || tail.starts_with("renames ") {
            continue;
        }
        let name = tail
            .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
            .next()
            .filter(|value| identifier(value))?;
        return Some(name.to_string());
    }
    None
}

fn parse_header(line: &str) -> Result<(String, Vec<(String, String)>, String), String> {
    let body = line
        .strip_prefix("def ")
        .and_then(|value| value.strip_suffix(':'))
        .ok_or_else(|| "expected def name(args) -> Type:".to_string())?;
    let open = body
        .find('(')
        .ok_or_else(|| "function needs (".to_string())?;
    let close = body
        .rfind(')')
        .ok_or_else(|| "function needs )".to_string())?;
    let name = body[..open].trim();
    if close < open || !identifier(name) {
        return Err("function name is not a safe Jet identifier".into());
    }
    let name = canonical_identifier(name);
    let result = body[close + 1..]
        .trim()
        .strip_prefix("->")
        .ok_or_else(|| "function needs a return annotation".to_string())?;
    let result = map_type(result.trim())?;
    let mut params = Vec::new();
    let raw_params = body[open + 1..close].trim();
    if !raw_params.is_empty() {
        for raw in split_commas(raw_params) {
            let (param, ty) = raw
                .split_once(':')
                .ok_or_else(|| format!("parameter {raw} needs a type annotation"))?;
            let param = param.trim();
            if !identifier(param) || param == "self" || param.contains('=') {
                return Err(format!("parameter {param} has no safe importer mapping"));
            }
            params.push((canonical_identifier(param), map_type(ty.trim())?));
        }
    }
    Ok((name.to_string(), params, result))
}

fn map_type(ty: &str) -> Result<String, String> {
    match ty {
        "int" => Ok("Int".into()),
        "float" => Ok("Float".into()),
        "str" => Ok("String".into()),
        "bool" => Ok("Bool".into()),
        "None" => Ok("()".into()),
        _ => Err(format!("Python type {ty} has no proven Jet mapping")),
    }
}

fn split_commas(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0;
    for (index, ch) in raw.char_indices() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(raw[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(raw[start..].trim());
    out
}

fn translate_body(function: &Function) -> Result<Vec<String>, (usize, String)> {
    if function.body.is_empty() {
        return Err((1, "function body is empty".into()));
    }
    let base = function
        .body
        .iter()
        .map(|(_, line)| indentation(line))
        .min()
        .unwrap_or(0);
    if let Some(nested) = function
        .body
        .iter()
        .position(|(_, line)| indentation(line) > base)
    {
        let line = function
            .body
            .iter()
            .take(nested)
            .rev()
            .find(|(_, line)| indentation(line) == base)
            .map(|(line, _)| *line)
            .unwrap_or(function.body[nested].0);
        return Err((
            line,
            "nested control flow has no proven equivalent in this importer".into(),
        ));
    }
    let mut bindings = BTreeSet::new();
    let mut output = Vec::new();
    for (line_no, raw) in &function.body {
        let line = raw.trim();
        if line == "pass" && function.result == "()" {
            continue;
        }
        if let Some(value) = line.strip_prefix("return ") {
            let source_value = value;
            let value = parse_expression(source_value).map_err(|why| (*line_no, why))?;
            output.push(format!(
                "return {}",
                materialize_borrowed_string(function, source_value, value)
            ));
            continue;
        }
        if line == "return" && function.result == "()" {
            output.push("return".into());
            continue;
        }
        if let Some(value) = line.strip_prefix("assert ") {
            let (left, right) = split_top(value, "==").ok_or_else(|| {
                (
                    *line_no,
                    "only equality assertions have a proven Jet test mapping".into(),
                )
            })?;
            output.push(format!(
                "assert_eq({}, {})",
                parse_expression(left).map_err(|why| (*line_no, why))?,
                parse_expression(right).map_err(|why| (*line_no, why))?
            ));
            continue;
        }
        if let Some((name, value)) = assignment(line) {
            let name = canonical_identifier(name);
            if !identifier(&name) {
                return Err((*line_no, "assignment target is not a local name".into()));
            }
            let source_value = value;
            let value = parse_expression(source_value).map_err(|why| (*line_no, why))?;
            let value = materialize_borrowed_string(function, source_value, value);
            let operator = if bindings.insert(name.clone()) {
                ":="
            } else {
                "="
            };
            output.push(format!("{name} {operator} {value}"));
            continue;
        }
        output.push(parse_expression(line).map_err(|why| (*line_no, why))?);
    }
    Ok(output)
}

#[derive(Clone)]
enum Token {
    Word(String),
    Number(String),
    Text(String),
    Op(String),
    Open,
    Close,
    Comma,
}

fn parse_expression(raw: &str) -> Result<String, String> {
    parse_expression_with_float(raw, false)
}

fn parse_expression_with_float(raw: &str, float_literals: bool) -> Result<String, String> {
    let tokens = lex(raw)?;
    if tokens.is_empty() {
        return Err("empty expression".into());
    }
    let mut depth = 0;
    let mut expect_value = true;
    let mut output = String::new();
    for token in tokens {
        match token {
            Token::Word(word) => {
                if !expect_value && !matches!(word.as_str(), "and" | "or") {
                    return Err(format!("unsupported expression near {word}"));
                }
                let word = match word.as_str() {
                    "True" | "true" => "true".to_string(),
                    "False" | "false" => "false".to_string(),
                    "and" => "&&".to_string(),
                    "or" => "||".to_string(),
                    "None" => return Err("None needs an explicit Option mapping".into()),
                    "null" | "nil" | "undefined" | "new" | "this" => {
                        return Err("dynamic or reference value has no proven scalar mapping".into())
                    }
                    _ => canonical_identifier(&word),
                };
                push_piece(&mut output, &word);
                expect_value = matches!(word.as_str(), "&&" | "||");
            }
            Token::Number(value) => {
                if !expect_value {
                    return Err("two adjacent expression values".into());
                }
                let value = if float_literals && !value.contains('.') {
                    format!("{value}.0")
                } else {
                    value
                };
                push_piece(&mut output, &value);
                expect_value = false;
            }
            Token::Text(value) => {
                if !expect_value {
                    return Err("two adjacent expression values".into());
                }
                push_piece(&mut output, &value);
                expect_value = false;
            }
            Token::Op(op) => {
                if expect_value && op != "-" && op != "!" {
                    return Err(format!("operator {op} needs a left operand"));
                }
                push_piece(&mut output, &op);
                expect_value = true;
            }
            Token::Open => {
                output.push('(');
                depth += 1;
                expect_value = true;
            }
            Token::Close => {
                if depth == 0 || expect_value {
                    return Err("unbalanced expression parentheses".into());
                }
                output.push(')');
                depth -= 1;
                expect_value = false;
            }
            Token::Comma => {
                if depth == 0 || expect_value {
                    return Err("comma outside a call".into());
                }
                output.push_str(", ");
                expect_value = true;
            }
        }
    }
    if depth != 0 || expect_value {
        return Err("incomplete expression".into());
    }
    Ok(output)
}

fn lex(raw: &str) -> Result<Vec<Token>, String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
            at += 1;
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            out.push(Token::Word(raw[start..at].to_string()));
        } else if bytes[at].is_ascii_digit() {
            at += 1;
            let mut dots = 0;
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'.') {
                if bytes[at] == b'.' {
                    dots += 1;
                }
                at += 1;
            }
            if dots > 1 || raw[start..at].ends_with('.') {
                return Err("malformed numeric literal".into());
            }
            out.push(Token::Number(raw[start..at].to_string()));
        } else if matches!(bytes[at], b'\'' | b'"') {
            let quote = bytes[at];
            at += 1;
            let content = at;
            while at < bytes.len() && bytes[at] != quote {
                if bytes[at] == b'\\' {
                    at += 1;
                }
                at += 1;
            }
            if at >= bytes.len() {
                return Err("unterminated Python string".into());
            }
            let value = raw[content..at].replace('"', "\\\"");
            at += 1;
            out.push(Token::Text(format!("\"{value}\"")));
        } else {
            let two = raw.get(at..at + 2).unwrap_or("");
            if matches!(two, "==" | "!=" | "<=" | ">=" | "&&" | "||") {
                out.push(Token::Op(two.into()));
                at += 2;
                continue;
            }
            out.push(match bytes[at] as char {
                '(' => Token::Open,
                ')' => Token::Close,
                ',' => Token::Comma,
                '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!' => {
                    Token::Op((bytes[at] as char).to_string())
                }
                other => return Err(format!("unsupported Python expression token {other}")),
            });
            at += 1;
        }
    }
    Ok(out)
}

fn push_piece(output: &mut String, piece: &str) {
    if !output.is_empty() && !output.ends_with(['(', ' ']) {
        output.push(' ');
    }
    output.push_str(piece);
}

fn render_function(marker: &str, function: &Function, body: &[String]) -> String {
    let params = function
        .params
        .iter()
        .map(|(name, ty)| format!("{}: {ty}", canonical_identifier(name)))
        .collect::<Vec<_>>()
        .join(", ");
    let result = if function.result == "()" {
        String::new()
    } else {
        format!(" {} ->", function.result)
    };
    let mut output = format!(
        "{marker}fn {}({params}){result} {{\n",
        canonical_identifier(&function.name)
    );
    for line in body {
        output.push_str("    ");
        output.push_str(line);
        output.push('\n');
    }
    output.push_str("}\n");
    output
}

fn import_evidence(
    boundary: &jet::ForeignBridge::ForeignBoundaryContract,
    source: &Path,
    target: &Path,
    source_digest: &str,
    replacement_digest: &str,
    file_count: usize,
    function_count: usize,
    test_count: usize,
    omission_count: usize,
) -> Result<EvidenceReport, String> {
    let boundary_digest = boundary.digest();
    let report_id = format!("source-import-{boundary_digest}");
    let source_ref = EvidenceSource::new(source.display().to_string(), 0, 0);
    let derivation_identity = boundary.identity.derivation_identity();
    let revision = EvidenceRevision::new(
        derivation_identity.source.clone(),
        derivation_identity.build.clone(),
        derivation_identity.run.clone(),
    );
    let build = EvidenceBuild::new(
        derivation_identity.build.clone(),
        derivation_identity.target.clone(),
        "source-import",
    );
    let outcome = if omission_count == 0 {
        EvidenceOutcome::Generated
    } else {
        EvidenceOutcome::Incomplete
    };
    let detail = if omission_count == 0 {
        format!(
            "source import generated {file_count} files, {function_count} functions, and {test_count} tests"
        )
    } else {
        format!(
            "source import translated {file_count} files, {function_count} functions, and {test_count} tests with {omission_count} reported omissions"
        )
    };
    let count = u64::try_from(file_count)
        .map_err(|_| "source import file count is too large".to_string())?;
    let identity = EvidenceIdentity::for_record(
        &report_id,
        "source-import",
        EvidenceProducerKind::Compile,
        EvidenceKind::Contract,
        &source_ref,
        count,
        &detail,
    );
    let mut evidence = EvidenceRecord::new(
        identity,
        EvidenceKind::Contract,
        EvidenceProducerKind::Compile,
        outcome,
        count,
        source_ref.clone(),
        build.clone(),
        revision.clone(),
    );
    evidence.detail = detail;
    evidence.attachments.extend([
        EvidenceAttachment::new("boundary", boundary_digest.clone()),
        EvidenceAttachment::new("source-digest", source_digest),
        EvidenceAttachment::new("replacement-digest", replacement_digest),
        EvidenceAttachment::new("source-identity", boundary.identity.source.clone()),
        EvidenceAttachment::new("replacement-identity", boundary.identity.overlay.clone()),
        EvidenceAttachment::new("tool-identity", boundary.identity.toolchain.clone()),
        EvidenceAttachment::new("target-identity", boundary.identity.target.clone()),
        EvidenceAttachment::new("coverage", boundary.artifact_coverage.digest()),
        EvidenceAttachment::new(
            "import-report",
            target.join("import-report.json").display().to_string(),
        ),
    ]);
    evidence.attachments.extend(
        boundary
            .assumptions
            .iter()
            .cloned()
            .map(|value| EvidenceAttachment::new("assumption", value)),
    );
    let disposition = if omission_count == 0 {
        DerivationDisposition::Current
    } else {
        DerivationDisposition::Unavailable
    };
    let derivation = DerivationRecord::new(
        evidence.identity.evidence_id.clone(),
        evidence.identity.claim_id.clone(),
        evidence.producer.as_str(),
        DerivationMethod::StaticDerivation,
        "foreign-source-import",
        [
            boundary_digest,
            boundary.artifact_coverage.digest(),
            source_digest.to_string(),
            replacement_digest.to_string(),
        ],
        derivation_identity,
    )
    .with_disposition(disposition);
    let mut report = EvidenceReport::new(
        report_id,
        EvidenceProducerKind::Compile,
        source_ref,
        build,
        revision,
    );
    report
        .add_record_with_derivation(evidence, derivation)
        .map_err(|error| error.to_string())?;
    Ok(report)
}

fn attach_import_evidence(plan: &mut Plan) -> Result<(), String> {
    let Some(boundary) = plan.boundary.take() else {
        return Err("source import has no foreign boundary".to_string());
    };
    let Some(report) = plan.evidence.as_ref() else {
        return Err("source import has no evidence report".to_string());
    };
    let derivation = report
        .derivations
        .first()
        .ok_or_else(|| "source import evidence has no derivation".to_string())?;
    let evidence = report
        .records
        .first()
        .ok_or_else(|| "source import evidence has no record".to_string())?;
    let boundary = boundary
        .attach_derivation(derivation)?
        .attach_evidence(evidence)?;
    boundary.validate()?;
    plan.boundary = Some(boundary);
    Ok(())
}

fn indexed_artifact_path(index: &RecordIndex, entry: &RecordIndexEntry, cwd: &Path) -> PathBuf {
    if entry.path.is_absolute() {
        entry.path.clone()
    } else if entry.path.starts_with(Path::new(".jet")) {
        cwd.join(&entry.path)
    } else {
        index.root().join(&entry.path)
    }
}

fn receipt_store_for_artifact(path: &Path) -> Result<ReceiptStore, String> {
    let objects = path
        .parent()
        .ok_or_else(|| format!("receipt artifact has no parent: {}", path.display()))?;
    if objects.file_name().and_then(|name| name.to_str()) != Some("objects") {
        return Err(format!(
            "indexed comparison path is not a receipt object: {}",
            path.display()
        ));
    }
    let root = objects
        .parent()
        .ok_or_else(|| format!("receipt object has no store root: {}", path.display()))?;
    Ok(ReceiptStore::new(root))
}

fn attachment_values(record: &EvidenceRecord, name: &str) -> Vec<String> {
    record
        .attachments
        .iter()
        .filter(|attachment| attachment.name == name)
        .map(|attachment| attachment.value.clone())
        .collect()
}

fn require_attachment(
    record: &EvidenceRecord,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    let values = attachment_values(record, name);
    if values.len() != 1 || values.first().map(String::as_str) != Some(expected) {
        return Err(format!(
            "indexed evidence attachment `{name}` does not match the current import"
        ));
    }
    Ok(())
}

fn require_attachment_set(
    record: &EvidenceRecord,
    name: &str,
    expected: &[String],
) -> Result<(), String> {
    let mut values = attachment_values(record, name);
    values.sort();
    if values != expected {
        return Err(format!(
            "indexed evidence attachments `{name}` do not match the current import"
        ));
    }
    Ok(())
}

fn validate_import_binding(
    record: &EvidenceRecord,
    boundary: &jet::ForeignBridge::ForeignBoundaryContract,
    boundary_digest: &str,
    source_digest: &str,
    replacement_digest: &str,
) -> Result<(), String> {
    require_attachment(record, "boundary", boundary_digest)?;
    require_attachment(record, "source-digest", source_digest)?;
    require_attachment(record, "replacement-digest", replacement_digest)?;
    require_attachment(record, "source-identity", &boundary.identity.source)?;
    require_attachment(record, "replacement-identity", &boundary.identity.overlay)?;
    require_attachment(record, "tool-identity", &boundary.identity.toolchain)?;
    require_attachment(record, "target-identity", &boundary.identity.target)?;
    require_attachment(
        record,
        "coverage",
        &boundary.artifact_coverage.digest(),
    )?;
    require_attachment_set(record, "assumption", &boundary.assumptions)?;
    let derivation_identity = boundary.identity.derivation_identity();
    if record.build.toolchain != derivation_identity.build
        || record.build.target != derivation_identity.target
        || record.build.profile != "source-import"
    {
        return Err("indexed import evidence build identity is stale".into());
    }
    if record.revision.source != derivation_identity.source
        || record.revision.build != derivation_identity.build
        || record.revision.revision != derivation_identity.run
    {
        return Err("indexed import evidence revision identity is stale".into());
    }
    Ok(())
}

fn indexed_import_evidence(
    index: &RecordIndex,
    cwd: &Path,
    boundary: &jet::ForeignBridge::ForeignBoundaryContract,
    boundary_digest: &str,
    source_digest: &str,
    replacement_digest: &str,
) -> Result<(EvidenceRecord, DerivationRecord), String> {
    let report_id = format!("source-import-{boundary_digest}");
    let entry = index
        .find(RecordKind::Evidence, &report_id, true)
        .ok_or_else(|| {
            "source import update requires prior indexed import evidence for the current source"
                .to_string()
        })?;
    if entry.identity.target_inputs_sha256 != source_digest
        || entry.identity.engine != EvidenceProducerKind::Compile.as_str()
    {
        return Err("indexed import evidence is stale against the current source digest".into());
    }
    let path = indexed_artifact_path(index, &entry, cwd);
    let report = EvidenceReport::read(&path)
        .map_err(|error| format!("cannot read indexed import evidence: {error}"))?;
    if !report.is_complete() {
        return Err("indexed import evidence is incomplete".into());
    }
    let record = report
        .records
        .iter()
        .find(|record| {
            record.identity.report_id == report_id && record.identity.claim_id == "source-import"
        })
        .cloned()
        .ok_or_else(|| "indexed import evidence has no source-import record".to_string())?;
    record
        .validate()
        .map_err(|error| format!("indexed import evidence is invalid: {error}"))?;
    if record.outcome != EvidenceOutcome::Generated {
        return Err(format!(
            "indexed import evidence is not a complete generated candidate: {}",
            record.outcome.as_str()
        ));
    }
    validate_import_binding(
        &record,
        boundary,
        boundary_digest,
        source_digest,
        replacement_digest,
    )?;
    let reference = record
        .derivation
        .as_ref()
        .ok_or_else(|| "indexed import evidence has no derivation reference".to_string())?;
    let derivation = report
        .derivation(reference)
        .cloned()
        .ok_or_else(|| "indexed import evidence has no derivation payload".to_string())?;
    derivation.validate()?;
    if derivation.identity != boundary.identity.derivation_identity() {
        return Err("indexed import derivation identity is stale".into());
    }
    if derivation.method != DerivationMethod::StaticDerivation
        || derivation.rule != "foreign-source-import"
        || derivation.disposition != DerivationDisposition::Current
    {
        return Err("indexed import derivation is not current".into());
    }
    let mut premises = vec![
        boundary_digest.to_string(),
        boundary.artifact_coverage.digest(),
        source_digest.to_string(),
        replacement_digest.to_string(),
    ];
    premises.sort();
    premises.dedup();
    if derivation.premises != premises {
        return Err("indexed import derivation premises are stale".into());
    }
    Ok((record, derivation))
}

fn validate_comparison_binding(
    record: &EvidenceRecord,
    comparison: &ComparisonRecord,
    artifact_id: &str,
    boundary: &jet::ForeignBridge::ForeignBoundaryContract,
    boundary_digest: &str,
) -> Result<(), String> {
    require_attachment(record, "comparison-artifact", artifact_id)?;
    require_attachment(record, "boundary", boundary_digest)?;
    require_attachment(record, "source-identity", &boundary.identity.source)?;
    require_attachment(record, "replacement-identity", &boundary.identity.overlay)?;
    require_attachment(record, "tool-identity", &boundary.identity.toolchain)?;
    require_attachment(record, "target-identity", &boundary.identity.target)?;
    require_attachment(record, "coverage", &boundary.artifact_coverage.digest())?;
    require_attachment_set(record, "assumption", &boundary.assumptions)?;
    if record.build.toolchain != boundary.identity.toolchain
        || record.build.target != boundary.identity.target
        || record.build.profile != "comparison"
        || record.revision.source != artifact_id
        || record.revision.build != "jet-comparison-v1"
        || record.revision.revision != artifact_id
    {
        return Err("indexed comparison evidence build identity is stale".into());
    }
    if comparison.samples.iter().any(|sample| {
        sample.identity.source != boundary.identity.source
            || sample.identity.tool != boundary.identity.toolchain
            || sample.identity.target != boundary.identity.target
    }) {
        return Err("indexed comparison samples are not bound to the current foreign tool and target".into());
    }
    Ok(())
}

fn indexed_comparison(
    index: &RecordIndex,
    cwd: &Path,
    boundary: &jet::ForeignBridge::ForeignBoundaryContract,
    boundary_digest: &str,
) -> Result<ComparisonRecord, String> {
    let mut entries = index.query_kind(RecordKind::Comparison, true);
    entries.sort_by(|left, right| {
        right
            .recorded_sequence
            .cmp(&left.recorded_sequence)
            .then(left.artifact_id.cmp(&right.artifact_id))
    });
    let mut stale_reason = None;
    for entry in entries {
        let path = indexed_artifact_path(index, &entry, cwd);
        let raw_receipt = match read_path(&path) {
            Ok(receipt) => receipt,
            Err(error) => {
                stale_reason = Some(format!(
                    "indexed comparison receipt `{}` could not be authenticated: {error}",
                    entry.artifact_id
                ));
                continue;
            }
        };
        let store = receipt_store_for_artifact(&path)?;
        let receipt = match store.lookup(&raw_receipt.claim)? {
            Some(receipt) => receipt,
            None => {
                stale_reason = Some(format!(
                    "indexed comparison `{}` is stale against its current corpus inputs",
                    entry.artifact_id
                ));
                continue;
            }
        };
        let Some(section) = receipt.sections.iter().find(|section| {
            section.name == "comparison" && section.type_name == "ComparisonRecord"
        }) else {
            continue;
        };
        let payload = section.value()?;
        let comparison = ComparisonRecord::from_json(&payload)?;
        let artifact_id = comparison.artifact_id()?;
        if section.payload_digest != artifact_id || artifact_id != entry.artifact_id {
            return Err(format!(
                "indexed comparison `{}` has a non-canonical artifact digest",
                entry.artifact_id
            ));
        }
        let mut bound = false;
        let mut bound_error = None;
        for link in receipt
            .consumed
            .iter()
            .filter(|link| link.kind == RecordKind::Evidence)
        {
            let Some(evidence_entry) = index.find(RecordKind::Evidence, &link.artifact_id, true)
            else {
                bound_error = Some(format!(
                    "comparison `{}` links to missing evidence `{}`",
                    entry.artifact_id, link.artifact_id
                ));
                continue;
            };
            let evidence_path = indexed_artifact_path(index, &evidence_entry, cwd);
            let evidence_report = EvidenceReport::read(&evidence_path).map_err(|error| {
                format!(
                    "cannot read comparison evidence `{}`: {error}",
                    link.artifact_id
                )
            })?;
            if !evidence_report.is_complete() {
                bound_error = Some(format!(
                    "comparison evidence `{}` is incomplete",
                    link.artifact_id
                ));
                continue;
            }
            for evidence in evidence_report.records.iter() {
                if evidence.identity.report_id != link.artifact_id
                    || evidence.identity.claim_id != "comparison"
                {
                    continue;
                }
                if validate_comparison_binding(
                    evidence,
                    &comparison,
                    &artifact_id,
                    boundary,
                    boundary_digest,
                )
                .is_err()
                {
                    continue;
                }
                bound = true;
                evidence
                    .validate()
                    .map_err(|error| format!("indexed comparison evidence is invalid: {error}"))?;
                if evidence.outcome != EvidenceOutcome::Passed {
                    return Err(format!(
                        "indexed comparison `{}` is not accepted: evidence outcome is {}",
                        entry.artifact_id,
                        evidence.outcome.as_str()
                    ));
                }
                let Some(reference) = evidence.derivation.as_ref() else {
                    return Err(format!(
                        "indexed comparison `{}` has no checked derivation",
                        entry.artifact_id
                    ));
                };
                let Some(derivation) = evidence_report.derivation(reference) else {
                    return Err(format!(
                        "indexed comparison `{}` has a missing derivation payload",
                        entry.artifact_id
                    ));
                };
                if derivation.disposition != DerivationDisposition::Current {
                    return Err(format!(
                        "indexed comparison `{}` has stale derivation evidence",
                        entry.artifact_id
                    ));
                }
                if *derivation != evidence.checked_derivation() {
                    return Err(format!(
                        "indexed comparison `{}` derivation does not match its evidence",
                        entry.artifact_id
                    ));
                }
            }
        }
        if !bound {
            if let Some(error) = bound_error {
                stale_reason = Some(error);
            }
            continue;
        }
        comparison.assert_equal().map_err(|error| {
            format!(
                "indexed comparison `{}` was not matched: {error}",
                entry.artifact_id
            )
        })?;
        return Ok(comparison);
    }
    Err(stale_reason.unwrap_or_else(|| {
        "source import update requires an indexed matched comparison bound to the current source, generated candidate, tool, target, coverage, and assumptions".into()
    }))
}

fn accept_update_replacement(
    plan: &mut Plan,
    cwd: &Path,
) -> Result<jet::ForeignBridge::ForeignSourceReceipt, String> {
    let boundary = plan
        .boundary
        .as_ref()
        .ok_or_else(|| "source import has no foreign boundary".to_string())?
        .clone();
    if !boundary.has_complete_artifact_coverage() {
        return Err("source import replacement coverage is incomplete".into());
    }
    let mut current_files = Vec::new();
    collect_sources(
        &plan.source,
        &plan.source,
        &plan.language,
        &mut current_files,
    )?;
    current_files.sort();
    let current_source_digest =
        import_tree_identity(&plan.source, &current_files, "jet-source-import-input-v1")?;
    if current_source_digest != plan.source_digest {
        return Err("source import update source changed after planning".into());
    }
    let index = RecordIndex::load_for_project(cwd.to_path_buf())
        .map_err(|error| format!("cannot load indexed reasoning records: {error}"))?;
    let (evidence, derivation) = indexed_import_evidence(
        &index,
        cwd,
        &boundary,
        &plan.boundary_digest,
        &plan.source_digest,
        &plan.replacement_digest,
    )?;
    let comparison = indexed_comparison(&index, cwd, &boundary, &plan.boundary_digest)?;
    let mut accepted = boundary;
    let current_identity = accepted.identity.clone();
    let current_coverage = accepted.artifact_coverage.clone();
    let replacement_identity = current_identity.overlay.clone();
    let receipt = accepted.accept_reasoned_replacement(
        &current_identity,
        &current_coverage,
        &derivation,
        &evidence,
        &comparison,
        replacement_identity,
    )?;
    plan.boundary = Some(accepted);
    Ok(receipt)
}

fn accepted_receipt_json(
    receipt: &jet::ForeignBridge::ForeignSourceReceipt,
    assumptions: &[String],
) -> Result<CanonicalJson, String> {
    let identity = CanonicalJson::object([
        ("generator".into(), CanonicalJson::String(receipt.boundary_identity.generator.clone())),
        (
            "implementation".into(),
            CanonicalJson::String(receipt.boundary_identity.implementation.clone()),
        ),
        ("overlay".into(), CanonicalJson::String(receipt.boundary_identity.overlay.clone())),
        ("source".into(), CanonicalJson::String(receipt.boundary_identity.source.clone())),
        ("target".into(), CanonicalJson::String(receipt.boundary_identity.target.clone())),
        (
            "toolchain".into(),
            CanonicalJson::String(receipt.boundary_identity.toolchain.clone()),
        ),
    ])?;
    let evidence = receipt
        .evidence
        .iter()
        .map(|identity| {
            CanonicalJson::object([
                ("claim_id".into(), CanonicalJson::String(identity.claim_id.clone())),
                (
                    "evidence_id".into(),
                    CanonicalJson::String(identity.evidence_id.clone()),
                ),
                ("report_id".into(), CanonicalJson::String(identity.report_id.clone())),
            ])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let derivation = CanonicalJson::object([(
        "id".into(),
        CanonicalJson::String(receipt.derivation.id.clone()),
    )])?;
    CanonicalJson::object([
        ("artifact_coverage_digest".into(), CanonicalJson::String(receipt.artifact_coverage_digest.clone())),
        ("assumptions".into(), CanonicalJson::Array(assumptions.iter().cloned().map(CanonicalJson::String).collect())),
        ("boundary_digest".into(), CanonicalJson::String(receipt.boundary_digest.clone())),
        ("boundary_identity".into(), identity),
        ("comparison_artifact_id".into(), CanonicalJson::String(receipt.comparison_artifact_id.clone())),
        ("comparison_reason".into(), receipt.comparison_reason.clone().map(CanonicalJson::String).unwrap_or(CanonicalJson::Null)),
        ("comparison_relation".into(), CanonicalJson::String(receipt.comparison_relation.clone())),
        ("comparison_schema_version".into(), CanonicalJson::Integer(receipt.comparison_schema_version.to_string())),
        ("comparison_status".into(), CanonicalJson::String(receipt.comparison_status.clone())),
        ("compared_samples".into(), CanonicalJson::Integer(receipt.compared_samples.to_string())),
        ("derivation".into(), derivation),
        ("discarded_cases".into(), CanonicalJson::Integer(receipt.discarded_cases.to_string())),
        ("evidence".into(), CanonicalJson::Array(evidence)),
        ("first_difference".into(), receipt.first_difference.map(|value| CanonicalJson::Integer(value.to_string())).unwrap_or(CanonicalJson::Null)),
        ("replacement_identity".into(), CanonicalJson::String(receipt.replacement_identity.clone())),
        ("schema".into(), CanonicalJson::String(receipt.schema.into())),
        ("source_authority".into(), CanonicalJson::String(receipt.source_authority.as_str().into())),
        ("source_identity".into(), CanonicalJson::String(receipt.source_identity.clone())),
    ])
}

fn persist_accepted_receipt(
    plan: &Plan,
    receipt: &jet::ForeignBridge::ForeignSourceReceipt,
    cwd: &Path,
) -> Result<(), String> {
    let boundary = plan
        .boundary
        .as_ref()
        .ok_or_else(|| "accepted source import has no boundary".to_string())?;
    receipt.validate()?;
    let mut files = Vec::new();
    collect_sources(&plan.source, &plan.source, &plan.language, &mut files)?;
    files.sort();
    if files.is_empty() {
        return Err("accepted source import has no source inputs".into());
    }
    let argv = vec![
        "import".to_string(),
        plan.source.display().to_string(),
        "--update".to_string(),
    ];
    let root = receipt_root_for("import", &argv, cwd);
    let store = ReceiptStore::new(root);
    let section = ReceiptSection::from_json(
        "foreign-source-replacement",
        "ForeignSourceReceipt",
        accepted_receipt_json(receipt, &boundary.assumptions)?,
    )?;
    let mut consumed = Vec::new();
    for evidence in &receipt.evidence {
        let link = RecordLink::new(RecordKind::Evidence, evidence.report_id.clone())?;
        if !consumed.contains(&link) {
            consumed.push(link);
        }
    }
    let comparison = RecordLink::new(
        RecordKind::Comparison,
        receipt.comparison_artifact_id.clone(),
    )?;
    if !consumed.contains(&comparison) {
        consumed.push(comparison);
    }
    store.record_with_sections_and_links(
        "import",
        &argv,
        &files,
        0,
        &[],
        &[],
        &[section],
        &consumed,
        &[],
    )?;
    Ok(())
}

fn apply_plan(mut plan: Plan, mode: Mode, json: bool) -> i32 {
    if mode != Mode::DryRun {
        if let Err(error) = attach_import_evidence(&mut plan) {
            return operation_error("attach import evidence", &error, json);
        }
    }
    let accepted_receipt = if mode == Mode::Update {
        let cwd = match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(error) => {
                return operation_error("read current directory", &error.to_string(), json)
            }
        };
        match accept_update_replacement(&mut plan, &cwd) {
            Ok(receipt) => Some(receipt),
            Err(error) => return operation_error("accept source replacement", &error, json),
        }
    } else {
        None
    };
    let report = report(&plan);
    let baseline_root = plan.target.join(".jet-import/baseline");
    let mut writes = Vec::new();
    let mut conflicts = Vec::new();
    for generated in &plan.generated {
        let target = plan.target.join(&generated.relative);
        let baseline = baseline_root.join(&generated.relative);
        let existing = fs::read_to_string(&target).ok();
        let prior = fs::read_to_string(&baseline).ok();
        match (mode, existing.as_deref(), prior.as_deref()) {
            (Mode::DryRun, _, _) => {}
            (_, None, _) => writes.push((target.clone(), generated.contents.clone())),
            (_, Some(current), _) if current == generated.contents => {}
            (Mode::Create, Some(_), _) => conflicts.push(target),
            (Mode::Update, Some(current), Some(old)) if current == old => {
                writes.push((target, generated.contents.clone()))
            }
            (Mode::Update, Some(_), Some(old)) if generated.contents == old => {}
            (Mode::Update, Some(_), _) => conflicts.push(target),
        }
        if mode != Mode::DryRun && prior.as_deref() != Some(generated.contents.as_str()) {
            writes.push((baseline, generated.contents.clone()));
        }
    }
    let report_path = plan.target.join("import-report.json");
    let report_baseline = baseline_root.join("import-report.json");
    let existing_report = fs::read_to_string(&report_path).ok();
    let prior_report = fs::read_to_string(&report_baseline).ok();
    match (mode, existing_report.as_deref(), prior_report.as_deref()) {
        (Mode::DryRun, _, _) => {}
        (_, None, _) => writes.push((report_path.clone(), report.clone())),
        (_, Some(current), _) if current == report => {}
        (Mode::Create, Some(_), _) => conflicts.push(report_path.clone()),
        (Mode::Update, Some(current), Some(old)) if current == old => {
            writes.push((report_path.clone(), report.clone()))
        }
        (Mode::Update, Some(_), Some(old)) if report == old => {}
        (Mode::Update, Some(_), _) => conflicts.push(report_path.clone()),
    }
    if mode != Mode::DryRun && prior_report.as_deref() != Some(report.as_str()) {
        writes.push((report_baseline, report.clone()));
    }
    if !conflicts.is_empty() {
        let conflict_paths = conflicts
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>();
        let diagnostic =
            Diagnostic::from_row("JT0199", &[("paths", &conflict_paths.join(", "))], None);
        if json {
            let report = ReportEnvelope::new(
                "tool",
                "error",
                diagnostic.code.clone(),
                diagnostic.what.clone(),
                diagnostic.why.clone(),
                diagnostic.fix.clone(),
            );
            let paths = StatusValue::array(
                conflicts
                    .iter()
                    .map(|path| StatusValue::from(path.display().to_string())),
            );
            println!(
                "{}",
                StatusEnvelope::new("import", false)
                    .with_report(report)
                    .with_field("paths", paths)
                    .json()
            );
        } else {
            eprintln!("error[{}]: {}", diagnostic.code, diagnostic.what);
            for path in &conflicts {
                eprintln!("  {path}", path = path.display());
            }
            eprintln!("  why: {}", diagnostic.why);
            eprintln!("  fix: {}", diagnostic.fix);
        }
        return ExitCodes::USER_ERROR;
    }
    for (path, contents) in writes {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return operation_error("create import output", &error.to_string(), json);
            }
        }
        if let Err(error) = fs::write(path, contents) {
            return operation_error("write import output", &error.to_string(), json);
        }
    }
    if mode != Mode::DryRun {
        let evidence = match plan.evidence.as_ref() {
            Some(evidence) => evidence,
            None => return operation_error(
                "persist import evidence",
                "source import has no evidence report",
                json,
            ),
        };
        if let Err(error) =
            crate::CmdCompile::persist_evidence_report_for_inputs(evidence, &plan.source_digest)
        {
            return operation_error("persist import evidence", &error, json);
        }
    }
    if let Some(receipt) = accepted_receipt.as_ref() {
        let cwd = match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(error) => {
                return operation_error("read current directory", &error.to_string(), json)
            }
        };
        if let Err(error) = persist_accepted_receipt(&plan, receipt, &cwd) {
            return operation_error("persist accepted source receipt", &error, json);
        }
    }
    if json {
        let import = report_value(&plan);
        println!(
            "{}",
            StatusEnvelope::new("import", true)
                .with_field("import", import)
                .json()
        );
    } else {
        let verb = if mode == Mode::DryRun {
            "would import"
        } else {
            "imported"
        };
        println!(
            "{verb} {} -> {}",
            plan.source.display(),
            plan.target.display()
        );
        println!(
            "  {} functions, {} carried tests, {} TODO diagnostics",
            plan.functions,
            plan.tests,
            plan.todos.len()
        );
        for todo in &plan.todos {
            let diagnostic = todo.diagnostic();
            println!("  {} {}", diagnostic.code, diagnostic.what);
            println!("    why: {}", diagnostic.why);
            println!("    fix: {}", diagnostic.fix);
            println!("    target: {}  status: {}", todo.target, todo.status);
        }
    }
    ExitCodes::OK
}

fn boundary_projection_json(plan: &Plan) -> String {
    let Some(boundary) = plan.boundary.as_ref() else {
        return "[]".into();
    };
    let rows = boundary
        .provenance_fields()
        .into_iter()
        .map(|(name, value)| {
            format!(
                "{{\"name\":\"{}\",\"value\":\"{}\"}}",
                json_escape(&name),
                json_escape(&value)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}

fn boundary_projection_value(plan: &Plan) -> StatusValue {
    let Some(boundary) = plan.boundary.as_ref() else {
        return StatusValue::array(std::iter::empty::<StatusValue>());
    };
    StatusValue::array(boundary.provenance_fields().into_iter().map(|(name, value)| {
        StatusValue::object(
            StatusFields::new()
                .with("name", name)
                .with("value", value),
        )
    }))
}

fn report(plan: &Plan) -> String {
    let boundary = boundary_projection_json(plan);
    let mut output = format!(
        "{{\"schema\":\"jet.source-import.v1\",\"law\":\"D-MIGRATE-SRC1\",\"language\":\"{}\",\"source\":\"{}\",\"target\":\"{}\",\"provenance\":{{\"source_root\":\"{}\",\"generated_root\":\"{}\",\"boundary\":{boundary}}},\"summary\":{{\"files\":{},\"translated_functions\":{},\"carried_tests\":{},\"omissions\":{}}},\"omissions\":[",
        json_escape(&plan.language),
        json_escape(&plan.source.display().to_string()),
        json_escape(&plan.target.display().to_string()),
        json_escape(&plan.source.display().to_string()),
        json_escape(&plan.target.display().to_string()),
        plan.generated.len(),
        plan.functions,
        plan.tests,
        plan.todos.len()
    );
    for (index, todo) in plan.todos.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        let diagnostic = todo.diagnostic();
        output.push_str(&format!(
            "{{\"code\":\"{}\",\"what\":\"{}\",\"why\":\"{}\",\"fix\":\"{}\",\"source\":\"{}\",\"source_span\":\"{}\",\"generated_target\":\"{}\",\"provenance\":\"D-MIGRATE-SRC1\",\"migration_status\":\"{}\"}}",
            diagnostic.code,
            json_escape(&diagnostic.what),
            json_escape(&diagnostic.why),
            json_escape(&diagnostic.fix),
            json_escape(&todo.source),
            json_escape(&todo.source),
            json_escape(&todo.target),
            todo.status
        ));
    }
    output.push_str("]}\n");
    output
}

fn report_value(plan: &Plan) -> StatusValue {
    let omissions = StatusValue::array(plan.todos.iter().map(|todo| {
        let diagnostic = todo.diagnostic();
        StatusValue::object(
            StatusFields::new()
                .with("code", diagnostic.code.as_str())
                .with("what", diagnostic.what.as_str())
                .with("why", diagnostic.why.as_str())
                .with("fix", diagnostic.fix.as_str())
                .with("source", todo.source.as_str())
                .with("source_span", todo.source.as_str())
                .with("generated_target", todo.target.as_str())
                .with("provenance", "D-MIGRATE-SRC1")
                .with("migration_status", todo.status),
        )
    }));
    StatusValue::object(
        StatusFields::new()
            .with("schema", "jet.source-import.v1")
            .with("law", "D-MIGRATE-SRC1")
            .with("language", plan.language.as_str())
            .with("source", plan.source.display().to_string())
            .with("target", plan.target.display().to_string())
            .with(
                "provenance",
                StatusValue::object(
                    StatusFields::new()
                        .with("source_root", plan.source.display().to_string())
                        .with("generated_root", plan.target.display().to_string())
                        .with("boundary", boundary_projection_value(plan)),
                ),
            )
            .with(
                "summary",
                StatusValue::object(
                    StatusFields::new()
                        .with("files", plan.generated.len())
                        .with("translated_functions", plan.functions)
                        .with("carried_tests", plan.tests)
                        .with("omissions", plan.todos.len()),
                ),
            )
            .with("omissions", omissions),
    )
}

fn gap(
    what: String,
    why: impl Into<String>,
    fix: impl Into<String>,
    source: &Path,
    target: &Path,
    line: usize,
) -> Todo {
    Todo {
        what,
        why: why.into(),
        fix: fix.into(),
        source: format!("{}:{line}", source.display()),
        target: target.display().to_string(),
        status: "omitted-reported",
    }
}

fn assignment(line: &str) -> Option<(&str, &str)> {
    let (left, right) = split_top(line, "=")?;
    if right.starts_with('=') || left.ends_with(['!', '<', '>']) {
        return None;
    }
    Some((left.trim(), right.trim()))
}

fn split_top<'a>(raw: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    let bytes = raw.as_bytes();
    let needle = needle.as_bytes();
    let mut depth = 0;
    let mut quote = None;
    let mut at = 0;
    while at + needle.len() <= bytes.len() {
        if let Some(active) = quote {
            if bytes[at] == active {
                quote = None;
            } else if bytes[at] == b'\\' {
                at += 1;
            }
            at += 1;
            continue;
        }
        match bytes[at] {
            b'\'' | b'"' => quote = Some(bytes[at]),
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && &bytes[at..at + needle.len()] == needle {
            return Some((&raw[..at], &raw[at + needle.len()..]));
        }
        at += 1;
    }
    None
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

fn canonical_identifier(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut output = String::with_capacity(value.len());
    for (index, character) in chars.iter().copied().enumerate() {
        if character.is_ascii_uppercase() {
            let previous = index.checked_sub(1).and_then(|index| chars.get(index));
            let next = chars.get(index + 1);
            let boundary = previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
            if boundary && !output.ends_with('_') {
                output.push('_');
            }
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn shorten(value: &str) -> String {
    if value.chars().count() <= 48 {
        value.to_string()
    } else {
        format!("{}...", value.chars().take(48).collect::<String>())
    }
}

fn usage_error(what: &str, fix: &str, json: bool) -> i32 {
    if json {
        println!(
            "{}",
            StatusEnvelope::new("import", false)
                .with_report(ReportEnvelope::new(
                    "tool",
                    "error",
                    "E2102",
                    what,
                    "D-MIGRATE-SRC1 keeps source import arguments explicit",
                    fix,
                ))
                .json()
        );
    } else {
        crate::emit_cli_report(
            "E2102",
            what.to_string(),
            "D-MIGRATE-SRC1 keeps source import arguments explicit".to_string(),
            fix.to_string(),
            false,
        );
    }
    ExitCodes::USAGE
}

fn operation_error(what: &str, why: &str, json: bool) -> i32 {
    let diagnostic = Diagnostic::from_row("JT0198", &[("operation", what), ("reason", why)], None);
    if json {
        println!(
            "{}",
            StatusEnvelope::new("import", false)
                .with_report(ReportEnvelope::new(
                    "tool",
                    "error",
                    diagnostic.code,
                    diagnostic.what,
                    diagnostic.why,
                    diagnostic.fix,
                ))
                .json()
        );
    } else {
        eprintln!("error[{}]: {}", diagnostic.code, diagnostic.what);
        eprintln!("  why: {}", diagnostic.why);
        eprintln!("  fix: {}", diagnostic.fix);
    }
    ExitCodes::USER_ERROR
}
