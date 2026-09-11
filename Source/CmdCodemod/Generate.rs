//! D-DX-GENERATE1=A: explicit, auditable source generation.
//!
//! A generator is a checked `#Job` with declared `inputs` and `outputs`.  The
//! command always evaluates the job in a disposable project copy, compares the
//! resulting tree with that copy, and only an explicit `--apply` can publish
//! the declared source files through the codemod transaction seam.

use super::Transaction::{self, Change};
use super::fail;
use jet_foundation::AST::{Item, JobMetadata};
use jet_foundation::JSON::json_escape;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const RECEIPT_SCHEMA: &str = "jet-generate-v1";
const GENERATED_HEADER: &str = "// jet-generated: schema=1";

#[derive(Default)]
struct Options {
    job: Option<String>,
    apply: bool,
    entry: Option<PathBuf>,
    json: bool,
    dry_run: bool,
    quiet: bool,
}

struct InputFact {
    path: String,
    absolute: PathBuf,
    digest: String,
}

struct AuthorityFact {
    path: String,
    absolute: Option<PathBuf>,
    digest: String,
}

struct GeneratorFact {
    name: String,
    source: String,
    line: usize,
    input_digest: String,
    authority: AuthorityFact,
    metadata: JobMetadata,
}

struct OutputPlan {
    path: String,
    absolute: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}

struct TempProject {
    path: PathBuf,
}

impl TempProject {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "jet-generate-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap_or_else(|error| {
            fail(&format!(
                "could not create source-generation staging directory `{}`: {error}",
                path.display()
            ))
        });
        Self { path }
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn run_generate(args: &[String], json: bool) {
    let options = parse_options(args);
    let machine = json || options.json;
    let job = options
        .job
        .as_deref()
        .unwrap_or_else(|| fail("`jet generate` needs a generator name"));
    let cwd = std::env::current_dir()
        .unwrap_or_else(|error| fail(&format!("could not read the current directory: {error}")));
    let entry = resolve_entry(&cwd, options.entry.as_deref());
    let entry = fs::canonicalize(&entry).unwrap_or_else(|error| {
        fail(&format!(
            "could not resolve generator entry `{}`: {error}",
            entry.display()
        ))
    });
    let project = entry
        .parent()
        .and_then(jet::Loader::find_manifest_root)
        .or_else(|| entry.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| fail("generator entry has no project root"));
    let project = fs::canonicalize(&project).unwrap_or_else(|error| {
        fail(&format!(
            "could not resolve generator project `{}`: {error}",
            project.display()
        ))
    });
    let source = fs::read_to_string(&entry).unwrap_or_else(|error| {
        fail(&format!(
            "could not read generator source `{}`: {error}",
            entry.display()
        ))
    });
    let entry_text = entry.to_string_lossy().into_owned();
    let (diagnostics, bundle, _) = jet::Driver::check_file_with_effect_facts_for_run_and_entry(
        &entry_text,
        "dev",
        &BTreeMap::new(),
        Some(job),
    );
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == jet::Diagnostics::Severity::Error)
    {
        fail_with_diagnostics(&entry, &source, job, &diagnostics);
    }
    let bundle = bundle.unwrap_or_else(|| {
        fail_with_source(
            &entry,
            &source,
            job,
            "the source check returned no checked bundle",
        )
    });
    let function = bundle.modules[bundle.entry]
        .items
        .iter()
        .find_map(|item| match item {
            Item::Func(function) if function.name == job => Some(function),
            _ => None,
        })
        .unwrap_or_else(|| {
            fail_with_source(
                &entry,
                &source,
                job,
                "generator entrypoint is not declared in the project entry module",
            )
        });
    if !function.is_job {
        fail_with_source(
            &entry,
            &source,
            job,
            "generator entrypoint must be marked with `#Job`",
        );
    }
    if !function.params.is_empty() {
        fail_with_source(
            &entry,
            &source,
            job,
            "generator entrypoints cannot take parameters; put configuration in declared inputs",
        );
    }
    let metadata = function.job_metadata.clone().unwrap_or_else(|| {
        fail_with_source(
            &entry,
            &source,
            job,
            "generator `#Job` must declare typed `inputs` and `outputs` metadata",
        )
    });
    if metadata.inputs.is_empty() {
        fail_with_source(
            &entry,
            &source,
            job,
            "generator `#Job` must declare at least one input",
        );
    }
    if metadata.outputs.is_empty() {
        fail_with_source(
            &entry,
            &source,
            job,
            "generator `#Job` must declare at least one output",
        );
    }
    let source_rel = relative_path(&project, &entry);
    let line = source_line(&source, function.job_span.unwrap_or(function.span).start);
    let source_digest = digest_file(&entry);
    let authority = authority_fact(&project, &source_digest);
    let mut inputs = Vec::new();
    add_input(
        &mut inputs,
        InputFact {
            path: source_rel.clone(),
            absolute: entry.clone(),
            digest: source_digest,
        },
    );
    for raw in &metadata.inputs {
        let absolute = declared_path(
            &project,
            raw,
            "input",
            true,
            &entry,
            &source,
            job,
        );
        let path = relative_path(&project, &absolute);
        ensure_generated_input_current(&absolute, &path, &entry, &source, job);
        let digest = digest_declared(&absolute);
        add_input(
            &mut inputs,
            InputFact {
                path,
                absolute,
                digest,
            },
        );
    }
    let input_digest = digest_input_facts(&inputs);
    let fact = GeneratorFact {
        name: job.to_string(),
        source: source_rel,
        line,
        input_digest,
        authority,
        metadata,
    };
    let outputs = output_paths(&project, &entry, &source, &fact);
    for input in &inputs {
        if outputs.iter().any(|output| output.path == input.path) {
            fail_with_source(
                &entry,
                &source,
                job,
                &format!("generator input/output overlap at `{}`", input.path),
            );
        }
    }
    let receipt_path = project
        .join(".jet")
        .join("codemods")
        .join(format!("generate-{}.receipt.json", safe_component(&fact.name)));
    let log_path = project
        .join(".jet")
        .join("codemods")
        .join(format!("generate-{}.log.json", safe_component(&fact.name)));
    let staging = TempProject::new();
    copy_project(&project, &staging.path);
    let baseline = collect_files(&staging.path);
    let staged_entry = staging.path.join(
        entry
            .strip_prefix(&project)
            .unwrap_or_else(|_| fail("generator entry is outside its project root")),
    );
    let staged_entry_text = staged_entry.to_string_lossy().into_owned();
    let run_args = [fact.name.as_str()];
    let run = jet::Interpreter::run_jit_once_with_args_opts_and_gates_and_settings_with_lints_and_authority_and_entry(
        &staged_entry_text,
        &run_args,
        machine,
        jet_foundation::Policy::GateSet::default(),
        &BTreeMap::new(),
        None,
        None,
    );
    match run.outcome {
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } if exit_code == 0 => {
            if !machine && !options.quiet {
                if !stdout.is_empty() {
                    print!("{stdout}");
                }
                if !stderr.is_empty() {
                    eprint!("{stderr}");
                }
            }
        }
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr,
            exit_code,
        } => {
            let detail = format_run_failure(exit_code, &stdout, &stderr);
            let receipt = render_receipt(&fact, &inputs, &[], "failed", Some(&detail));
            publish_failure_receipt_if_apply(
                options.apply,
                &project,
                &receipt_path,
                &log_path,
                receipt.as_bytes(),
            );
            fail_with_source(&entry, &source, &fact.name, &detail);
        }
        jet::Interpreter::RunOutcome::Problems(diagnostics) => {
            let detail = diagnostics
                .iter()
                .map(|diagnostic| format!("{}: {}", diagnostic.code, diagnostic.what))
                .collect::<Vec<_>>()
                .join("; ");
            let receipt = render_receipt(&fact, &inputs, &[], "failed", Some(&detail));
            publish_failure_receipt_if_apply(
                options.apply,
                &project,
                &receipt_path,
                &log_path,
                receipt.as_bytes(),
            );
            fail_with_source(&entry, &source, &fact.name, &detail);
        }
    }
    let after_tree = collect_files(&staging.path);
    let declared = outputs
        .iter()
        .map(|output| output.path.clone())
        .collect::<BTreeSet<_>>();
    let changed = changed_paths(&baseline, &after_tree);
    let undeclared = changed
        .iter()
        .filter(|path| !declared.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    if !undeclared.is_empty() {
        let detail = format!(
            "generator changed undeclared project files: {}",
            undeclared.join(", ")
        );
        let receipt = render_receipt(&fact, &inputs, &[], "failed", Some(&detail));
        publish_failure_receipt_if_apply(
            options.apply,
            &project,
            &receipt_path,
            &log_path,
            receipt.as_bytes(),
        );
        fail_with_source(&entry, &source, &fact.name, &detail);
    }
    let mut plans = Vec::new();
    for output in outputs {
        let raw_after = after_tree.get(&output.path).cloned().unwrap_or_else(|| {
            let detail = format!("generator did not produce declared output `{}`", output.path);
            let receipt = render_receipt(&fact, &inputs, &[], "failed", Some(&detail));
            publish_failure_receipt_if_apply(
                options.apply,
                &project,
                &receipt_path,
                &log_path,
                receipt.as_bytes(),
            );
            fail_with_source(&entry, &source, &fact.name, &detail)
        });
        let after = generated_source(&fact, &output.path, &raw_after);
        let before = fs::symlink_metadata(&output.absolute)
            .ok()
            .map(|metadata| {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    fail_with_source(
                        &entry,
                        &source,
                        &fact.name,
                        &format!("declared output `{}` is not a regular file", output.path),
                    )
                }
                fs::read(&output.absolute).unwrap_or_else(|error| {
                    fail_with_source(
                        &entry,
                        &source,
                        &fact.name,
                        &format!("could not read declared output `{}`: {error}", output.path),
                    )
                })
            });
        if let Some(existing) = before.as_deref() {
            ensure_output_collision(existing, &fact.name, &output.path, &entry, &source);
        }
        plans.push(OutputPlan {
            path: output.path,
            absolute: output.absolute,
            before,
            after,
        });
    }
    ensure_inputs_current(&inputs, &entry, &source, &fact.name);
    ensure_authority_current(&fact.authority, &entry, &source, &fact.name);
    let receipt = render_receipt(&fact, &inputs, &plans, "ok", None);
    if !options.apply {
        render_preview(
            &fact,
            &inputs,
            &plans,
            &project,
            &receipt_path,
            machine,
            options.quiet,
        );
        return;
    }
    let receipt_before = fs::symlink_metadata(&receipt_path)
        .ok()
        .map(|metadata| {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                fail_with_source(
                    &entry,
                    &source,
                    &fact.name,
                    "generation receipt path is not a regular file",
                )
            }
            fs::read(&receipt_path).unwrap_or_else(|error| {
                fail_with_source(
                    &entry,
                    &source,
                    &fact.name,
                    &format!("could not read generation receipt: {error}"),
                )
            })
        });
    let receipt_changed = receipt_before.as_deref() != Some(receipt.as_bytes());
    let output_changed = plans.iter().any(output_is_changed);
    if !receipt_changed && !output_changed {
        if machine {
            print_machine_result(&fact, &inputs, &plans, &project, &receipt_path, "unchanged");
        } else if !options.quiet {
            println!("generate: `{}` is unchanged", fact.name);
            println!("receipt: {}", receipt_path.display());
        }
        return;
    }
    let mut changes = plans
        .iter()
        .filter(|plan| output_is_changed(plan))
        .map(|plan| Change {
            path: plan.absolute.clone(),
            before: plan.before.clone().unwrap_or_default(),
            after: plan.after.clone(),
        })
        .collect::<Vec<_>>();
    let mut absent = plans
        .iter()
        .filter(|plan| plan.before.is_none())
        .map(|plan| plan.absolute.clone())
        .collect::<Vec<_>>();
    if receipt_changed {
        changes.push(Change {
            path: receipt_path.clone(),
            before: receipt_before.clone().unwrap_or_default(),
            after: receipt.as_bytes().to_vec(),
        });
        if receipt_before.is_none() {
            absent.push(receipt_path.clone());
        }
    }
    let transaction_log = render_transaction_log(&fact, &plans, &project, &receipt_path);
    let lock = Transaction::lock(&project);
    Transaction::recover(&lock);
    Transaction::commit_generate(&lock, &changes, &absent, &log_path, transaction_log.as_bytes());
    if machine {
        print_machine_result(&fact, &inputs, &plans, &project, &receipt_path, "applied");
    } else if !options.quiet {
        println!("generate: applied `{}`", fact.name);
        for plan in &plans {
            if output_is_changed(plan) {
                println!("  wrote {}", project.join(&plan.path).display());
            }
        }
        println!("receipt: {}", receipt_path.display());
    }
}

fn parse_options(args: &[String]) -> Options {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--apply" => options.apply = true,
            "--dry-run" => options.dry_run = true,
            "--json" => options.json = true,
            "--quiet" => options.quiet = true,
            "--verbose" | "-v" | "--color" => {}
            value if value.starts_with("--color=") => {}
            "--entry" => {
                index += 1;
                let value = args.get(index).unwrap_or_else(|| fail("`--entry` needs a `.jet` file"));
                options.entry = Some(PathBuf::from(value));
            }
            value if value.starts_with('-') => fail(&format!("unknown `jet generate` flag `{value}`")),
            value => {
                if options.job.replace(value.to_string()).is_some() {
                    fail("`jet generate` accepts one generator name")
                }
            }
        }
        index += 1;
    }
    let job = options
        .job
        .as_deref()
        .unwrap_or_else(|| fail("`jet generate` needs a generator name"));
    validate_identifier(job);
    if options.dry_run {
        options.apply = false;
    }
    options
}

fn resolve_entry(cwd: &Path, raw: Option<&Path>) -> PathBuf {
    if let Some(raw) = raw {
        return if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            cwd.join(raw)
        };
    }
    crate::find_project_entry(cwd)
}

fn declared_path(
    project: &Path,
    raw: &str,
    kind: &str,
    must_exist: bool,
    entry: &Path,
    source: &str,
    generator: &str,
) -> PathBuf {
    let relative = Path::new(raw);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        fail_with_source(
            entry,
            source,
            generator,
            &format!(
                "generator {kind} `{raw}` must be a project-relative path without `.` or `..`"
            ),
        );
    }
    if raw.starts_with(".git/") || raw.starts_with("target/") {
        fail_with_source(
            entry,
            source,
            generator,
            &format!("generator {kind} `{raw}` points into compiler state"),
        );
    }
    let absolute = project.join(relative);
    validate_parent(project, relative, kind, entry, source, generator);
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("generator {kind} `{raw}` is a symlink"),
            )
        }
        Ok(metadata) if !metadata.is_file() && !metadata.is_dir() => {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("generator {kind} `{raw}` is not a regular path"),
            )
        }
        Ok(_) if must_exist => {}
        Ok(metadata) if kind == "output" && !metadata.is_file() => {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("generator output `{raw}` must be a regular file"),
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && must_exist => {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("generator {kind} `{raw}` does not exist"),
            )
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("could not inspect generator {kind} `{raw}`: {error}"),
            )
        }
        _ => {}
    }
    absolute
}

fn validate_parent(
    project: &Path,
    relative: &Path,
    kind: &str,
    entry: &Path,
    source: &str,
    generator: &str,
) {
    let mut current = project.to_path_buf();
    for component in relative.parent().into_iter().flat_map(Path::components) {
        let Component::Normal(name) = component else {
            fail_with_source(
                entry,
                source,
                generator,
                &format!("generator {kind} path has a non-normal parent"),
            );
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).unwrap_or_else(|error| {
            fail_with_source(
                entry,
                source,
                generator,
                &format!(
                    "generator {kind} parent `{}` is unavailable: {error}",
                    current.display()
                ),
            )
        });
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            fail_with_source(
                entry,
                source,
                generator,
                &format!(
                    "generator {kind} parent `{}` must be a real directory",
                    current.display()
                ),
            );
        }
    }
}

fn output_paths(
    project: &Path,
    entry: &Path,
    source: &str,
    fact: &GeneratorFact,
) -> Vec<OutputPath> {
    let mut seen = BTreeSet::new();
    fact.metadata
        .outputs
        .iter()
        .map(|raw| {
            let absolute = declared_path(
                project,
                raw,
                "output",
                false,
                entry,
                source,
                &fact.name,
            );
            let path = relative_path(project, &absolute);
            if !seen.insert(path.clone()) {
                fail_with_source(
                    entry,
                    source,
                    &fact.name,
                    &format!("generator declares output `{path}` more than once"),
                );
            }
            if Path::new(&path).extension().and_then(|value| value.to_str()) != Some("jet") {
                fail_with_source(
                    entry,
                    source,
                    &fact.name,
                    &format!("generator output `{path}` must be a `.jet` source file"),
                );
            }
            OutputPath { path, absolute }
        })
        .collect()
}

struct OutputPath {
    path: String,
    absolute: PathBuf,
}

fn copy_project(project: &Path, destination: &Path) {
    copy_tree(project, destination).unwrap_or_else(|error| {
        fail(&format!(
            "could not stage project `{}` for generation: {error}",
            project.display()
        ))
    });
}

fn copy_tree(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::create_dir_all(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if matches!(name.to_str(), Some(".git" | "target" | ".jet")) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let metadata = fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("staging refuses symlink `{}`", source_path.display()),
            ));
        }
        if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("staging refuses special file `{}`", source_path.display()),
            ));
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_files_at(root, root, &mut files).unwrap_or_else(|error| {
        fail(&format!(
            "could not inspect staged generator project `{}`: {error}",
            root.display()
        ))
    });
    files
}

fn collect_files_at(
    root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> std::io::Result<()> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        if current == root && matches!(name.to_str(), Some(".git" | "target" | ".jet")) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("staged generator tree contains symlink `{}`", path.display()),
            ));
        }
        if metadata.is_dir() {
            collect_files_at(root, &path, files)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| std::io::Error::other("staged path escaped root"))?;
            files.insert(path_string(relative), fs::read(&path)?);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("staged generator tree contains special file `{}`", path.display()),
            ));
        }
    }
    Ok(())
}

fn changed_paths(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect()
}

fn output_is_changed(plan: &OutputPlan) -> bool {
    match plan.before.as_deref() {
        Some(before) => before != plan.after.as_slice(),
        None => true,
    }
}

fn toolchain_id() -> String {
    format!(
        "jet/{}-{}-{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

fn generated_source(fact: &GeneratorFact, path: &str, raw: &[u8]) -> Vec<u8> {
    let body = strip_generated_header(raw);
    let body_digest = jet::SHA256::sha256_hex(body);
    format!(
        "{GENERATED_HEADER}\n// generator={}\n// source={}:{}\n// output={}\n// inputs={}\n// authority={}\n// authority-digest={}\n// toolchain=jet/{}-{}-{}\n// body-sha256={}\n\n",
        fact.name,
        fact.source,
        fact.line,
        path,
        fact.input_digest,
        fact.authority.path,
        fact.authority.digest,
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        body_digest,
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

fn strip_generated_header(bytes: &[u8]) -> &[u8] {
    if !bytes.starts_with(format!("{GENERATED_HEADER}\n").as_bytes()) {
        return bytes;
    }
    bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|offset| &bytes[offset + 2..])
        .unwrap_or(bytes)
}

fn ensure_output_collision(
    bytes: &[u8],
    generator: &str,
    output: &str,
    entry: &Path,
    source: &str,
) {
    let Some(header) = generation_header(bytes) else {
        fail_with_source(
            entry,
            source,
            generator,
            &format!(
                "refusing collision with existing source output `{output}`; the file is not attributed to this generator"
            ),
        );
    };
    if header.get("generator").map(String::as_str) != Some(generator)
        || header.get("output").map(String::as_str) != Some(output)
    {
        fail_with_source(
            entry,
            source,
            generator,
            &format!(
                "refusing collision with existing source output `{output}`; another generator owns it"
            ),
        );
    }
    let body = strip_generated_header(bytes);
    let body_digest = jet::SHA256::sha256_hex(body);
    if header.get("body-sha256").map(String::as_str) != Some(body_digest.as_str()) {
        fail_with_source(
            entry,
            source,
            generator,
            &format!(
                "refusing stale source output `{output}`; its attributed body was edited outside the generator"
            ),
        );
    }
}

fn generation_header(bytes: &[u8]) -> Option<BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split('\n');
    if lines.next()? != GENERATED_HEADER {
        return None;
    }
    let mut fields = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            return Some(fields);
        }
        let value = line.strip_prefix("// ")?;
        let (key, value) = value.split_once('=')?;
        fields.insert(key.to_string(), value.to_string());
    }
    None
}

fn render_receipt(
    fact: &GeneratorFact,
    inputs: &[InputFact],
    outputs: &[OutputPlan],
    status: &str,
    error: Option<&str>,
) -> String {
    let mut receipt = format!(
        "{{\n  \"schema\":\"{}\",\n  \"status\":\"{}\",\n  \"command\":\"jet generate {}{}\",\n  \"generator\":{{\"name\":\"{}\",\"source\":\"{}\",\"line\":{}}},\n  \"authority\":{{\"path\":\"{}\",\"digest\":\"{}\"}},\n  \"toolchain\":\"{}\",\n  \"inputs\":[",
        RECEIPT_SCHEMA,
        status,
        json_escape(&fact.name),
        if status == "ok" { " --apply" } else { "" },
        json_escape(&fact.name),
        json_escape(&fact.source),
        fact.line,
        json_escape(&fact.authority.path),
        fact.authority.digest,
        json_escape(&toolchain_id()),
    );
    for (index, input) in inputs.iter().enumerate() {
        if index > 0 {
            receipt.push(',');
        }
        receipt.push_str(&format!(
            "{{\"path\":\"{}\",\"digest\":\"{}\"}}",
            json_escape(&input.path),
            input.digest
        ));
    }
    receipt.push_str("],\n  \"outputs\":[");
    for (index, output) in outputs.iter().enumerate() {
        if index > 0 {
            receipt.push(',');
        }
        receipt.push_str(&format!(
            "{{\"path\":\"{}\",\"digest\":\"{}\",\"changed\":{}}}",
            json_escape(&output.path),
            jet::SHA256::sha256_hex(&output.after),
            output_is_changed(output),
        ));
    }
    receipt.push(']');
    if let Some(error) = error {
        receipt.push_str(&format!(",\n  \"error\":\"{}\"", json_escape(error)));
    }
    receipt.push_str("\n}\n");
    receipt
}

fn render_transaction_log(
    fact: &GeneratorFact,
    outputs: &[OutputPlan],
    project: &Path,
    receipt: &Path,
) -> String {
    let mut log = format!(
        "{{\n  \"schema\":\"jet-generate-transaction-v1\",\n  \"generator\":\"{}\",\n  \"receipt\":\"{}\",\n  \"files\":[",
        json_escape(&fact.name),
        json_escape(&relative_path(project, receipt)),
    );
    for (index, output) in outputs.iter().enumerate() {
        if index > 0 {
            log.push(',');
        }
        log.push_str(&format!(
            "{{\"path\":\"{}\",\"before\":{},\"after\":\"{}\"}}",
            json_escape(&output.path),
            output
                .before
                .as_deref()
                .map(|bytes| format!("\"{}\"", jet::SHA256::sha256_hex(bytes)))
                .unwrap_or_else(|| "null".to_string()),
            jet::SHA256::sha256_hex(&output.after),
        ));
    }
    log.push_str("]\n}\n");
    log
}
fn render_preview(
    fact: &GeneratorFact,
    inputs: &[InputFact],
    outputs: &[OutputPlan],
    project: &Path,
    receipt: &Path,
    machine: bool,
    quiet: bool,
) {
    if machine {
        print_machine_result(fact, inputs, outputs, project, receipt, "preview");
        return;
    }
    if quiet {
        return;
    }
    println!("generator: {}", fact.name);
    println!("source: {}:{}", fact.source, fact.line);
    println!("preview: planned source writes (nothing applied)");
    for output in outputs {
        let action = if !output_is_changed(output) {
            "unchanged"
        } else if output.before.is_some() {
            "update"
        } else {
            "create"
        };
        println!("  {action} {}", output.path);
    }
    println!(
        "apply: jet generate {} --apply (receipt {})",
        fact.name,
        receipt.display()
    );
}

fn print_machine_result(
    fact: &GeneratorFact,
    inputs: &[InputFact],
    outputs: &[OutputPlan],
    project: &Path,
    receipt: &Path,
    status: &str,
) {
    let mut result = format!(
        "{{\"schema\":\"jet-generate-result-v1\",\"status\":\"{}\",\"generator\":{{\"name\":\"{}\",\"source\":\"{}\",\"line\":{}}},\"authority\":{{\"path\":\"{}\",\"digest\":\"{}\"}},\"inputs_digest\":\"{}\",\"inputs\":[",
        json_escape(status),
        json_escape(&fact.name),
        json_escape(&fact.source),
        fact.line,
        json_escape(&fact.authority.path),
        fact.authority.digest,
        fact.input_digest,
    );
    for (index, input) in inputs.iter().enumerate() {
        if index > 0 {
            result.push(',');
        }
        result.push_str(&format!(
            "{{\"path\":\"{}\",\"digest\":\"{}\"}}",
            json_escape(&input.path),
            input.digest
        ));
    }
    result.push_str(&format!(
        "],\"toolchain\":\"{}\",\"outputs\":[",
        json_escape(&toolchain_id())
    ));
    for (index, output) in outputs.iter().enumerate() {
        if index > 0 {
            result.push(',');
        }
        let action = if !output_is_changed(output) {
            "unchanged"
        } else if output.before.is_some() {
            "update"
        } else {
            "create"
        };
        result.push_str(&format!(
            "{{\"path\":\"{}\",\"action\":\"{}\",\"digest\":\"{}\"}}",
            json_escape(&output.path),
            action,
            jet::SHA256::sha256_hex(&output.after),
        ));
    }
    result.push_str(&format!(
        "],\"receipt\":\"{}\"}}\n",
        json_escape(&relative_path(project, receipt))
    ));
    print!("{result}");
}

fn publish_failure_receipt(project: &Path, receipt: &Path, log: &Path, bytes: &[u8]) {
    let before = fs::symlink_metadata(receipt).ok().map(|metadata| {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            fail("generation receipt path is not a regular file")
        }
        fs::read(receipt).unwrap_or_else(|error| fail(&format!("could not read generation receipt: {error}")))
    });
    if before.as_deref() == Some(bytes) {
        return;
    }
    let mut absent = Vec::new();
    if before.is_none() {
        absent.push(receipt.to_path_buf());
    }
    let changes = [Change {
        path: receipt.to_path_buf(),
        before: before.unwrap_or_default(),
        after: bytes.to_vec(),
    }];
    let lock = Transaction::lock(project);
    Transaction::recover(&lock);
    Transaction::commit_generate(&lock, &changes, &absent, log, bytes);
}
fn publish_failure_receipt_if_apply(
    apply: bool,
    project: &Path,
    receipt: &Path,
    log: &Path,
    bytes: &[u8],
) {
    if apply {
        publish_failure_receipt(project, receipt, log, bytes);
    }
}

fn ensure_generated_input_current(
    path: &Path,
    relative: &str,
    entry: &Path,
    source: &str,
    generator: &str,
) {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|error| {
        fail_with_source(
            entry,
            source,
            generator,
            &format!("could not inspect generator input `{relative}`: {error}"),
        )
    });
    if !metadata.is_file() {
        return;
    }
    let bytes = fs::read(path).unwrap_or_else(|error| {
        fail_with_source(
            entry,
            source,
            generator,
            &format!("could not read generator input `{relative}`: {error}"),
        )
    });
    if !bytes.starts_with(format!("{GENERATED_HEADER}\n").as_bytes()) {
        return;
    }
    let header = generation_header(&bytes).unwrap_or_else(|| {
        fail_with_source(
            entry,
            source,
            generator,
            &format!("generated input `{relative}` has an incomplete provenance header"),
        )
    });
    if header.get("output").map(String::as_str) != Some(relative) {
        fail_with_source(
            entry,
            source,
            generator,
            &format!("generated input `{relative}` has a mismatched provenance output"),
        );
    }
    let body_digest = jet::SHA256::sha256_hex(strip_generated_header(&bytes));
    if header.get("body-sha256").map(String::as_str) != Some(body_digest.as_str()) {
        fail_with_source(
            entry,
            source,
            generator,
            &format!(
                "stale generated input `{relative}` was edited outside its generator; regenerate it first"
            ),
        );
    }
}


fn ensure_inputs_current(inputs: &[InputFact], entry: &Path, source: &str, generator: &str) {
    for input in inputs {
        let digest = digest_declared(&input.absolute);
        if digest != input.digest {
            fail_with_source(
                entry,
                source,
                generator,
                &format!(
                    "stale generator input `{}` changed while generation was running; rerun the command",
                    input.path
                ),
            );
        }
    }
}

fn ensure_authority_current(authority: &AuthorityFact, entry: &Path, source: &str, generator: &str) {
    if let Some(path) = &authority.absolute {
        let digest = digest_file(path);
        if digest != authority.digest {
            fail_with_source(
                entry,
                source,
                generator,
                "package authority changed while generation was running; rerun the command",
            );
        }
    }
}

fn authority_fact(project: &Path, source_digest: &str) -> AuthorityFact {
    if let Some(path) = jet::Loader::manifest_path(project) {
        let digest = digest_file(&path);
        AuthorityFact {
            path: relative_path(project, &path),
            absolute: Some(path),
            digest,
        }
    } else {
        AuthorityFact {
            path: "ambient".to_string(),
            absolute: None,
            digest: source_digest.to_string(),
        }
    }
}

fn add_input(inputs: &mut Vec<InputFact>, input: InputFact) {
    if let Some(existing) = inputs.iter().find(|existing| existing.path == input.path) {
        if existing.digest != input.digest {
            fail(&format!("generator input `{}` resolves to conflicting bytes", input.path));
        }
        return;
    }
    inputs.push(input);
}

fn digest_input_facts(inputs: &[InputFact]) -> String {
    let identity = inputs
        .iter()
        .map(|input| format!("{}\0{}\n", input.path, input.digest))
        .collect::<String>();
    jet::SHA256::sha256_hex(identity.as_bytes())
}

fn digest_declared(path: &Path) -> String {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|error| {
        fail(&format!("could not inspect generator input `{}`: {error}", path.display()))
    });
    if metadata.file_type().is_symlink() {
        fail(&format!("generator input `{}` is a symlink", path.display()));
    }
    if metadata.is_file() {
        return digest_file(path);
    }
    if metadata.is_dir() {
        validate_tree(path);
        return jet::SHA256::tree_hash(path);
    }
    fail(&format!("generator input `{}` is not a regular path", path.display()))
}

fn digest_file(path: &Path) -> String {
    jet::SHA256::sha256_file_hex(path).unwrap_or_else(|error| {
        fail(&format!("could not hash generator input `{}`: {error}", path.display()))
    })
}

fn validate_tree(path: &Path) {
    let mut entries = fs::read_dir(path)
        .unwrap_or_else(|error| fail(&format!("could not inspect `{}`: {error}", path.display())));
    while let Some(entry) = entries.next() {
        let entry = entry.unwrap_or_else(|error| fail(&format!("could not inspect `{}`: {error}", path.display())));
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)
            .unwrap_or_else(|error| fail(&format!("could not inspect `{}`: {error}", child.display())));
        if metadata.file_type().is_symlink() {
            fail(&format!("generator input tree contains symlink `{}`", child.display()));
        }
        if metadata.is_dir() {
            validate_tree(&child);
        } else if !metadata.is_file() {
            fail(&format!("generator input tree contains special file `{}`", child.display()));
        }
    }
}

fn relative_path(project: &Path, path: &Path) -> String {
    path.strip_prefix(project)
        .unwrap_or_else(|_| fail(&format!("path `{}` escapes project", path.display())))
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_string_lossy().into_owned(),
            _ => fail("generator path contains a non-normal component"),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn path_string(path: &Path) -> String {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_string_lossy().into_owned(),
            _ => fail("staged generator path contains a non-normal component"),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn source_line(source: &str, offset: usize) -> usize {
    source
        .as_bytes()
        .get(..offset.min(source.len()))
        .unwrap_or_default()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn validate_identifier(value: &str) {
    let mut chars = value.chars();
    if !chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        || !chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        fail(&format!("`{value}` is not a valid generator name"));
    }
}

fn format_run_failure(exit_code: i32, stdout: &str, stderr: &str) -> String {
    let mut detail = format!("generator job exited with status {exit_code}");
    if !stderr.trim().is_empty() {
        detail.push_str(&format!(": {}", stderr.trim()));
    } else if !stdout.trim().is_empty() {
        detail.push_str(&format!(": {}", stdout.trim()));
    }
    detail
}

fn fail_with_diagnostics(
    entry: &Path,
    source: &str,
    generator: &str,
    diagnostics: &[jet::Diagnostics::Diagnostic],
) -> ! {
    let detail = diagnostics
        .iter()
        .map(|diagnostic| {
            let line = diagnostic
                .span
                .map(|span| source_line(source, span.start))
                .unwrap_or(1);
            format!("{}:{}: {}: {}", entry.display(), line, diagnostic.code, diagnostic.what)
        })
        .collect::<Vec<_>>()
        .join("; ");
    fail_with_source(entry, source, generator, &detail)
}

fn fail_with_source(entry: &Path, source: &str, generator: &str, detail: &str) -> ! {
    let marker = format!("fn {generator}");
    let line = source
        .find(&marker)
        .map(|offset| source_line(source, offset))
        .unwrap_or(1);
    fail(&format!(
        "generator `{generator}` failed at {}:{}: {detail}",
        entry.display(),
        line
    ))
}
