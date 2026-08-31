//! jet learn — the toolchain's offline first kata arc.

use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{report_problems, OutputMode};
use jet::ExitCodes;

const ARC_DIR: &str = ".jet/learn/first-arc";

struct Exercise {
    file: &'static str,
    title: &'static str,
    prompt: &'static str,
    diagnostic: &'static str,
    broken: &'static str,
    solution: &'static str,
    expected_output: &'static str,
}

const EXERCISES: &[Exercise] = &[
    Exercise {
        file: "01_unknown_function.jet",
        title: "Name the function",
        prompt: "Correct the function name, then save the file.",
        diagnostic: "E0102",
        broken: include_str!("../examples/learn/first_arc/01_unknown_function.jet"),
        solution: include_str!("../examples/learn/first_arc/01_unknown_function.solution.jet"),
        expected_output: "hello, Jet\n",
    },
    Exercise {
        file: "02_type_mismatch.jet",
        title: "Match the argument type",
        prompt: "Pass text to the greeting function, then save the file.",
        diagnostic: "E0112",
        broken: include_str!("../examples/learn/first_arc/02_type_mismatch.jet"),
        solution: include_str!("../examples/learn/first_arc/02_type_mismatch.solution.jet"),
        expected_output: "hello, Jet\n",
    },
    Exercise {
        file: "03_entry_body.jet",
        title: "Keep one entry body",
        prompt: "Move the loose statement into the explicit run function.",
        diagnostic: "E0621",
        broken: include_str!("../examples/learn/first_arc/03_entry_body.jet"),
        solution: include_str!("../examples/learn/first_arc/03_entry_body.solution.jet"),
        expected_output: "move me\nready\n",
    },
];

pub(crate) fn run(args: &[String], mode: OutputMode) -> ! {
    let check = args.iter().any(|arg| arg == "--check");
    let once = args.iter().any(|arg| arg == "--watch=off");
    if let Some(arg) = args.iter().find(|arg| {
        !matches!(
            arg.as_str(),
            "--check"
                | "--watch"
                | "--watch=on"
                | "--watch=true"
                | "--watch=off"
                | "--json"
                | "--quiet"
                | "--color"
        ) && !arg.starts_with("--color=")
    }) {
        crate::cli_error!(
            @fix "E2104",
            format!("{arg} is not a jet learn argument"),
            "run jet learn, jet learn --watch=off, or jet learn --check"
        );
        std::process::exit(ExitCodes::USAGE);
    }
    if check {
        exit_check_curriculum(mode);
    }

    let root = std::env::current_dir()
        .unwrap_or_else(|error| {
            crate::cli_error!("E2105", "couldn't read the current directory: {error}");
            std::process::exit(ExitCodes::USER_ERROR);
        })
        .join(ARC_DIR);
    let index = current_exercise(&root);
    if index == EXERCISES.len() {
        print_complete(mode);
        std::process::exit(ExitCodes::OK);
    }
    let status = run_arc(&root, index, once, mode);
    std::process::exit(status);
}

fn current_exercise(root: &Path) -> usize {
    for (index, exercise) in EXERCISES.iter().enumerate() {
        let path = exercise_path(root, exercise);
        if !path.exists() {
            return index;
        }
        match evaluate(exercise, &path) {
            Evaluation::Complete => {}
            Evaluation::Problems(_) | Evaluation::WrongOutput { .. } => return index,
        }
    }
    EXERCISES.len()
}

fn run_arc(root: &Path, mut index: usize, once: bool, mode: OutputMode) -> i32 {
    if let Err(error) = fs::create_dir_all(root) {
        crate::cli_error!(
            @fix "E2105",
            format!("couldn't create the learn directory {}: {error}", root.display()),
            "run jet learn from a writable directory"
        );
        return ExitCodes::USER_ERROR;
    }

    loop {
        let exercise = &EXERCISES[index];
        let path = exercise_path(root, exercise);
        if let Err(error) = materialize(exercise, &path) {
            crate::cli_error!(
                @fix "E2105",
                format!("couldn't write {}: {error}", path.display()),
                "run jet learn from a writable directory"
            );
            return ExitCodes::USER_ERROR;
        }
        print_exercise(index, exercise, &path, mode);

        let mut watch = None;
        loop {
            match evaluate(exercise, &path) {
                Evaluation::Complete => {
                    if !mode.quiet {
                        println!("Completed {}.", exercise.title);
                    }
                    index += 1;
                    if index == EXERCISES.len() {
                        print_complete(mode);
                        return ExitCodes::OK;
                    }
                    if once {
                        return ExitCodes::OK;
                    }
                    break;
                }
                Evaluation::Problems(diags) => {
                    show_problems(&path, &diags, mode);
                    if once {
                        return ExitCodes::USER_ERROR;
                    }
                }
                Evaluation::WrongOutput { stdout, stderr } => {
                    show_wrong_output(exercise, &stdout, &stderr, mode);
                    if once {
                        return ExitCodes::USER_ERROR;
                    }
                }
            }

            if once {
                return ExitCodes::USER_ERROR;
            }
            if watch.is_none() {
                // D-LEARN1: learning uses the same dependency-aware
                // WatchSession as `jet dev`; only curriculum progression and
                // the hint renderer are specific to this command.
                watch = match jet::DevServer::WatchSession::open(&path) {
                    Ok(session) => Some(session),
                    Err(diagnostic) => {
                        let display = path.display().to_string();
                        eprint!(
                            "{}",
                            jet::render_all_colored(
                                &display,
                                "",
                                &[diagnostic],
                                mode.color_stderr(),
                            )
                        );
                        return ExitCodes::USER_ERROR;
                    }
                };
                if !mode.quiet {
                    println!("Watching {} for a fix …", path.display());
                }
            }
            loop {
                jet_jit::scheduler_sleep_ms(120);
                let Some(session) = watch.as_mut() else {
                    break;
                };
                let Some(receipt) = session.poll() else {
                    continue;
                };
                if receipt.change_kinds.iter().all(|kind| *kind == "stale") {
                    continue;
                }
                if let Err(diagnostic) = session.acknowledge(&receipt) {
                    let display = path.display().to_string();
                    eprint!(
                        "{}",
                        jet::render_all_colored(&display, "", &[diagnostic], mode.color_stderr(),)
                    );
                    return ExitCodes::USER_ERROR;
                }
                break;
            }
        }
    }
}

enum Evaluation {
    Complete,
    Problems(Vec<jet::Diagnostics::Diagnostic>),
    WrongOutput { stdout: String, stderr: String },
}

fn evaluate(exercise: &Exercise, path: &Path) -> Evaluation {
    let file = path.to_string_lossy();
    match jet::Interpreter::dev_iteration_with_gates_profile_and_settings_with_lints(
        &file,
        false,
        false,
        jet::Policy::GateSet::default(),
        "dev",
        &std::collections::BTreeMap::new(),
    )
    .outcome
    {
        jet::Interpreter::RunOutcome::Problems(diags) => Evaluation::Problems(diags),
        jet::Interpreter::RunOutcome::Ran {
            stdout,
            stderr: _,
            exit_code,
        } if exit_code == 0 && stdout == exercise.expected_output => Evaluation::Complete,
        jet::Interpreter::RunOutcome::Ran { stdout, stderr, .. } => {
            Evaluation::WrongOutput { stdout, stderr }
        }
    }
}

fn exercise_path(root: &Path, exercise: &Exercise) -> PathBuf {
    root.join(exercise.file)
}

fn materialize(exercise: &Exercise, path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, exercise.broken)
}

fn print_exercise(index: usize, exercise: &Exercise, path: &Path, mode: OutputMode) {
    if mode.quiet {
        return;
    }
    println!("Jet Learn — First Arc");
    println!(
        "Exercise {}/{}: {}",
        index + 1,
        EXERCISES.len(),
        exercise.title
    );
    println!("{}", exercise.prompt);
    println!("Edit {} and save.", path.display());
}

fn show_problems(path: &Path, diags: &[jet::Diagnostics::Diagnostic], mode: OutputMode) {
    let source = fs::read_to_string(path).unwrap_or_default();
    let display = path.display().to_string();
    report_problems(mode, &display, &source, diags);
    if mode.json {
        return;
    }
    let Some(diagnostic) = diags.first() else {
        return;
    };
    let Some(explanation) = jet::Explain::lookup(&diagnostic.code) else {
        return;
    };
    println!("Hint: jet explain {}", diagnostic.code);
    print!(
        "{}",
        jet::Explain::render(
            &explanation,
            mode.color.resolve(std::io::stdout().is_terminal())
        )
    );
}

fn show_wrong_output(exercise: &Exercise, stdout: &str, stderr: &str, mode: OutputMode) {
    if mode.json || mode.quiet {
        return;
    }
    println!("The source runs, but its output is not complete yet.");
    println!("Expected: {:?}", exercise.expected_output);
    println!("Received: {:?}", stdout);
    if !stderr.is_empty() {
        println!("Stderr: {:?}", stderr);
    }
}

fn print_complete(mode: OutputMode) {
    if mode.json {
        println!(
            "{}",
            jet::Diagnostics::render_status_json(
                "ok",
                true,
                "learn",
                &format!(",\"arc\":\"first\",\"exercises\":{}", EXERCISES.len())
            )
        );
    } else if !mode.quiet {
        println!("First arc complete. Run jet learn --check to verify it offline.");
    }
}

fn exit_check_curriculum(mode: OutputMode) -> ! {
    let root = std::env::current_dir()
        .unwrap_or_else(|error| {
            crate::cli_error!("E2105", "couldn't read the current directory: {error}");
            std::process::exit(ExitCodes::USER_ERROR);
        })
        .join(ARC_DIR)
        .join(format!(
            ".check-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ));
    if let Err(error) = fs::create_dir_all(&root) {
        crate::cli_error!(
            "E2105",
            "couldn't create curriculum check directory: {error}"
        );
        std::process::exit(ExitCodes::USER_ERROR);
    }
    let result = validate_curriculum(&root);
    let _ = fs::remove_dir_all(&root);
    match result {
        Ok(()) => {
            if mode.json {
                println!(
                    "{}",
                    jet::Diagnostics::render_status_json(
                        "ok",
                        true,
                        "learn.check",
                        &format!(",\"arc\":\"first\",\"exercises\":{}", EXERCISES.len())
                    )
                );
            } else {
                println!("first arc: {} exercises passed", EXERCISES.len());
            }
            std::process::exit(ExitCodes::OK);
        }
        Err(error) => {
            crate::cli_error!(
                @fix "E2105",
                format!("the first arc is broken: {error}"),
                "repair the packaged exercise or solution, then run jet learn --check again"
            );
            std::process::exit(ExitCodes::USER_ERROR);
        }
    }
}

fn validate_curriculum(root: &Path) -> Result<(), String> {
    for exercise in EXERCISES {
        let broken = root.join(format!("broken-{}", exercise.file));
        let solution = root.join(format!("solution-{}", exercise.file));
        fs::write(&broken, exercise.broken).map_err(|error| error.to_string())?;
        fs::write(&solution, exercise.solution).map_err(|error| error.to_string())?;

        match evaluate(exercise, &broken) {
            Evaluation::Problems(diags)
                if diags
                    .iter()
                    .any(|diagnostic| diagnostic.code == exercise.diagnostic) => {}
            Evaluation::Problems(diags) => {
                return Err(format!(
                    "{} expected {}, got {}",
                    exercise.file,
                    exercise.diagnostic,
                    diags
                        .first()
                        .map(|diagnostic| diagnostic.code.as_str())
                        .unwrap_or("no diagnostic")
                ));
            }
            Evaluation::Complete | Evaluation::WrongOutput { .. } => {
                return Err(format!("{} no longer fails as designed", exercise.file));
            }
        }
        if !matches!(evaluate(exercise, &solution), Evaluation::Complete) {
            return Err(format!("{} solution no longer runs", exercise.file));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_arc_is_versioned_and_has_one_solution_per_exercise() {
        assert_eq!(EXERCISES.len(), 3);
        for exercise in EXERCISES {
            assert!(exercise.broken.contains("fn run") || exercise.broken.contains("print"));
            assert!(!exercise.solution.contains("pirnt"));
            assert!(!exercise.expected_output.is_empty());
            assert!(jet::Explain::lookup(exercise.diagnostic).is_some());
        }
    }
}
