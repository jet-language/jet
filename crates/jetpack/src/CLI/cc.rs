//! Hermetic C/C++ compiler-driver entry points.
//!
//! The driver is intentionally thin: it parses only the flags needed to keep
//! the action graph safe, resolves the one Jetpack C toolchain package, and
//! submits the invocation to the ordinary BuildContext executor. The actual
//! compiler remains a declared tool in the Hangar descriptor.

use crate::Comptime::Build::{
    execute_build_plan, ActionKind, ActionSpec, BuildCapability, BuildContext,
    BuildExecutionError, BuildResourcePool, TargetSpec,
};
use crate::Output::Theme;
use crate::Provider::{self, ProviderError, SourceState};
use crate::{RefSpec, Store};
use jet_foundation::ExitCodes;
use jet_foundation::Terminal::ColorChoice;
use crate::RefSpec::SourceTable;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

const MAX_INPUT_FILES: usize = 100_000;
const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_RESPONSE_FILES: usize = 64;
const MAX_RESPONSE_DEPTH: usize = 16;
const MAX_RESPONSE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_RESPONSE_ARGS: usize = 100_000;

#[derive(Debug, Default)]
struct DriverOptions {
    offline: bool,
    fixtures: Option<PathBuf>,
    project_root: Option<PathBuf>,
    build_root: Option<PathBuf>,
    working_dir: PathBuf,
    target: Option<String>,
    compile_only: bool,
    output: Option<String>,
    dep_mode: bool,
    dep_phony: bool,
    depfile: Option<String>,
    dep_target: Option<String>,
    probe: Option<DriverProbe>,
    verbose: bool,
    help: bool,
    version: bool,
    argv: Vec<String>,
    sources: Vec<String>,
    source_indices: Vec<usize>,
    path_indices: Vec<(usize, String)>,
    output_index: Option<usize>,
    depfile_index: Option<usize>,
    dep_target_index: Option<usize>,
    dep_target_option: Option<String>,
    separator_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverProbe {
    DumpMachine,
    PrintSysroot,
    DumpVersion,
}

pub(super) fn main(verb: &str, args: &[String]) -> i32 {
    let options = match parse_args(verb, args) {
        Ok(options) => options,
        Err(reason) => {
            report_usage_error(verb, &reason);
            return ExitCodes::USAGE;
        }
    };
    if options.help {
        println!("{}", usage(verb));
        return ExitCodes::OK;
    }

    let project_root = match resolve_project_root(&options) {
        Ok(path) => path,
        Err(reason) => {
            report_operational_error(
                "C/C++ driver could not determine the project root",
                &reason,
                "run the driver from an accessible project directory",
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let target = options.target.as_deref().unwrap_or("native");
    let reference = if target == "native" || target == "host" {
        "cc-toolchain@jetpack".to_string()
    } else {
        format!("cc-toolchain@jetpack#target={target}")
    };
    let spec = match RefSpec::classify(&reference) {
        Ok(spec) => spec,
        Err(error) => {
            report_usage_error(verb, &format!("invalid target selection: {error:?}"));
            return ExitCodes::USAGE;
        }
    };
    let roots = Store::resolve();
    let fixtures = Provider::fixtures_from_env(options.fixtures.clone());
    let descriptor = match resolve_toolchain(
        &roots,
        &spec,
        fixtures.as_deref(),
        options.offline,
        &project_root,
    )
    {
        Ok(descriptor) => descriptor,
        Err(error) => {
            report_provider_error(&error);
            return ExitCodes::USER_ERROR;
        }
    };
    if options.version {
        println!(
            "jet {verb} {} (target {}; ABI {})",
            descriptor.version, descriptor.target, descriptor.abi
        );
        return ExitCodes::OK;
    }
    if let Some(probe) = options.probe {
        match probe {
            DriverProbe::DumpMachine => println!("{}", descriptor.target),
            DriverProbe::PrintSysroot => println!("{}", descriptor.sysroot_path().display()),
            DriverProbe::DumpVersion => println!("{}", compiler_version(&descriptor)),
        }
        return ExitCodes::OK;
    }
    if options.verbose && options.sources.is_empty() {
        println!(
            "jet {verb} {} (target {}; sysroot {})",
            descriptor.version,
            descriptor.target,
            descriptor.sysroot_path().display()
        );
        return ExitCodes::OK;
    }

    let invocation = match build_invocation(&project_root, verb, &options) {
        Ok(invocation) => invocation,
        Err(reason) => {
            report_usage_error(verb, &reason);
            return ExitCodes::USAGE;
        }
    };
    let mut build = BuildContext::new();
    let toolchain = match build.toolchain("cc-toolchain", descriptor.toolchain_spec()) {
        Ok(toolchain) => toolchain,
        Err(error) => {
            report_operational_error(
                "C/C++ toolchain could not enter the build graph",
                &format!("{error:?}"),
                "repair the pinned Hangar descriptor and retry",
            );
            return ExitCodes::USER_ERROR;
        }
    };
    // The target is already a declared ToolchainSpec fact and action-key label.
    // Do not inject Clang's `--target` spelling into the pinned GCC driver.
    let mut action_argv = vec![verb.to_string()];
    if !cfg!(windows) {
        action_argv.push(format!("--sysroot={}", descriptor.sysroot_path().display()));
    }
    action_argv.extend(invocation.argv.iter().skip(1).cloned());
    let mut action = ActionSpec::cached(action_argv)
        .with_inputs(invocation.inputs.iter().cloned())
        .with_outputs(invocation.outputs.iter().cloned())
        .with_env("PATH", descriptor.virtual_path_env())
        .with_env("LANG", "C")
        .with_env_allowlist(["PATH", "LANG"])
        .with_kind(ActionKind::Compile)
        .with_cap(BuildCapability::Exec)
        .with_toolchain(toolchain)
        .with_variant_identity(descriptor.identity())
        .with_label("cc.driver", verb)
        .with_label("cc.target", descriptor.target.clone())
        .with_label("cc.bundle", descriptor.bundle_sha256.clone());
    if !options.compile_only {
        action = action.with_pool(BuildResourcePool::Linker);
    }
    let action = match build.action("cc", action) {
        Ok(action) => action,
        Err(error) => {
            report_operational_error(
                "C/C++ invocation could not enter the build graph",
                &format!("{error:?}"),
                "check the declared source and output paths",
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let mut target_spec = TargetSpec::new()
        .with_action(action)
        .with_toolchain(toolchain);
    for source in &invocation.sources {
        target_spec = target_spec.with_source(source.clone());
    }
    for input in &invocation.inputs {
        target_spec = target_spec.with_input(input.clone());
    }
    for output in &invocation.outputs {
        target_spec = target_spec.with_output(output.clone());
    }
    let target = match build.add_executable("cc-output", target_spec) {
        Ok(target) => target,
        Err(error) => {
            report_operational_error(
                "C/C++ invocation could not create its build target",
                &format!("{error:?}"),
                "check the declared source and output paths",
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let plan = match build.plan_with_default(target) {
        Ok(plan) => plan,
        Err(error) => {
            report_operational_error(
                "C/C++ invocation produced an invalid build graph",
                &format!("{error:?}"),
                "check the declared source and output paths",
            );
            return ExitCodes::USER_ERROR;
        }
    };
    let grants = [BuildCapability::Exec]
        .into_iter()
        .collect::<BTreeSet<_>>();
    match execute_build_plan(&plan, &project_root, &grants) {
        Ok(_) => {
            if !options.compile_only {
                if let Err(reason) = mark_link_outputs_executable(&project_root, &invocation.outputs)
                {
                    report_operational_error(
                        "C/C++ link output could not be published",
                        &reason,
                        "check that the declared link output is a regular file, then retry",
                    );
                    return ExitCodes::USER_ERROR;
                }
            }
            ExitCodes::OK
        }
        Err(error) => {
            report_execution_error(&error);
            ExitCodes::USER_ERROR
        }
    }
}

struct Invocation {
    argv: Vec<String>,
    sources: Vec<String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

fn parse_args(verb: &str, args: &[String]) -> Result<DriverOptions, String> {
    let current_dir = std::env::current_dir()
        .map_err(|error| format!("could not determine the current directory: {error}"))?;
    let current_dir = fs::canonicalize(&current_dir)
        .map_err(|error| format!("could not canonicalize the current directory: {error}"))?;
    let response_roots = response_scope_roots(args, &current_dir)?;
    let expanded = expand_response_files(args, &current_dir, &response_roots)?;
    let mut options = parse_expanded_args(verb, &expanded)?;
    options.working_dir = current_dir;
    Ok(options)
}

fn parse_expanded_args(verb: &str, args: &[String]) -> Result<DriverOptions, String> {
    let mut options = DriverOptions::default();
    let mut index = 0;
    let mut after_separator = false;
    while index < args.len() {
        let argument = &args[index];
        if after_separator {
            let argv_index = options.argv.len();
            validate_operand(argument, "source")?;
            options.sources.push(argument.clone());
            options.source_indices.push(argv_index);
            options.argv.push(argument.clone());
            index += 1;
            continue;
        }
        if argument == "--" {
            options.separator_index = Some(options.argv.len());
            after_separator = true;
            index += 1;
            continue;
        }
        match argument.as_str() {
            "-h" | "--help" => {
                options.help = true;
                index += 1;
            }
            "--version" => {
                options.version = true;
                index += 1;
            }
            "--offline" => {
                options.offline = true;
                index += 1;
            }
            "--project-root" => {
                let value = next_value(args, &mut index, "--project-root")?;
                set_once_path(&mut options.project_root, value, "--project-root")?;
            }
            value if value.starts_with("--project-root=") => {
                set_once_path(
                    &mut options.project_root,
                    value
                        .strip_prefix("--project-root=")
                        .expect("project-root prefix was checked above")
                        .to_string(),
                    "--project-root",
                )?;
                index += 1;
            }
            "--build-root" => {
                let value = next_value(args, &mut index, "--build-root")?;
                set_once_path(&mut options.build_root, value, "--build-root")?;
            }
            value if value.starts_with("--build-root=") => {
                set_once_path(
                    &mut options.build_root,
                    value
                        .strip_prefix("--build-root=")
                        .expect("build-root prefix was checked above")
                        .to_string(),
                    "--build-root",
                )?;
                index += 1;
            }
            "--fixtures" => {
                let value = next_value(args, &mut index, "--fixtures")?;
                options.fixtures = Some(PathBuf::from(value));
            }
            value if value.starts_with("--fixtures=") => {
                let value = value
                    .strip_prefix("--fixtures=")
                    .expect("fixtures prefix was checked above");
                if value.is_empty() {
                    return Err("`--fixtures` needs a directory".to_string());
                }
                options.fixtures = Some(PathBuf::from(value));
                index += 1;
            }
            "--target" | "-target" => {
                let value = next_value(args, &mut index, argument)?;
                set_once(&mut options.target, value, argument)?;
            }
            value if value.starts_with("--target=") => {
                set_once(
                    &mut options.target,
                    value
                        .strip_prefix("--target=")
                        .expect("target prefix was checked above")
                        .to_string(),
                    "--target",
                )?;
                index += 1;
            }
            "-dumpmachine" => {
                set_probe(&mut options.probe, DriverProbe::DumpMachine)?;
                index += 1;
            }
            "-print-sysroot" => {
                set_probe(&mut options.probe, DriverProbe::PrintSysroot)?;
                index += 1;
            }
            "-dumpversion" => {
                set_probe(&mut options.probe, DriverProbe::DumpVersion)?;
                index += 1;
            }
            "--sysroot" | "-isysroot" => {
                return Err(format!(
                    "`{argument}` is controlled by the pinned Hangar toolchain; remove the host sysroot override"
                ));
            }
            value if value.starts_with("--sysroot=")
                || value.starts_with("-isysroot")
                || value.starts_with("--gcc-toolchain=")
                || value.starts_with("-fuse-ld=")
                || value.starts_with("-resource-dir=")
                || value.starts_with("-B") =>
            {
                return Err(format!(
                    "`{value}` would override the pinned Hangar compiler or linker"
                ));
            }
            "--gcc-toolchain" | "-B" | "-fuse-ld" | "-resource-dir" | "-Xlinker" => {
                return Err(format!(
                    "`{argument}` would override the pinned Hangar compiler or linker"
                ));
            }
            "-c" => {
                options.compile_only = true;
                options.argv.push(argument.clone());
                index += 1;
            }
            "-MMD" | "-MD" => {
                options.dep_mode = true;
                options.argv.push(argument.clone());
                index += 1;
            }
            "-M" | "-MM" | "-MG" => {
                return Err(format!(
                    "`{argument}` does not declare a reproducible output; use `-MD` or `-MMD`"
                ));
            }
            "-MP" => {
                options.dep_phony = true;
                options.argv.push(argument.clone());
                index += 1;
            }
            "-v" => {
                options.verbose = true;
                options.argv.push(argument.clone());
                index += 1;
            }
            "-x" | "-D" | "-U" | "-l" | "-std" | "--std" | "--param" => {
                let value = next_value(args, &mut index, argument)?;
                if value.is_empty() || value.contains('\0') {
                    return Err(format!("option {argument} needs a non-empty value"));
                }
                options.argv.push(argument.clone());
                options.argv.push(value);
            }
            "-o" => {
                let value = next_value(args, &mut index, "-o")?;
                set_once(&mut options.output, value.clone(), "-o")?;
                options.argv.push(argument.clone());
                options.argv.push(value);
                options.output_index = Some(options.argv.len() - 1);
            }
            value if value.starts_with("-o") && value.len() > 2 => {
                set_once(
                    &mut options.output,
                    value
                        .strip_prefix("-o")
                        .expect("output prefix was checked above")
                        .to_string(),
                    "-o",
                )?;
                options.argv.push(argument.clone());
                options.output_index = Some(options.argv.len() - 1);
                index += 1;
            }
            "-MF" => {
                let value = next_value(args, &mut index, "-MF")?;
                set_once(&mut options.depfile, value.clone(), "-MF")?;
                options.argv.push(argument.clone());
                options.argv.push(value);
                options.depfile_index = Some(options.argv.len() - 1);
            }
            value if value.starts_with("-MF") && value.len() > 3 => {
                set_once(
                    &mut options.depfile,
                    value
                        .strip_prefix("-MF")
                        .expect("depfile prefix was checked above")
                        .to_string(),
                    "-MF",
                )?;
                options.argv.push(argument.clone());
                options.depfile_index = Some(options.argv.len() - 1);
                index += 1;
            }
            "-MT" => {
                let value = next_value(args, &mut index, "-MT")?;
                set_once(&mut options.dep_target, value.clone(), "-MT")?;
                options.argv.push(argument.clone());
                options.argv.push(value);
                options.dep_target_index = Some(options.argv.len() - 1);
                options.dep_target_option = Some("-MT".into());
            }
            value if value.starts_with("-MT") && value.len() > 3 => {
                set_once(
                    &mut options.dep_target,
                    value
                        .strip_prefix("-MT")
                        .expect("dep-target prefix was checked above")
                        .to_string(),
                    "-MT",
                )?;
                options.argv.push(argument.clone());
                options.dep_target_index = Some(options.argv.len() - 1);
                options.dep_target_option = Some("-MT".into());
                index += 1;
            }
            "-MQ" => {
                let value = next_value(args, &mut index, "-MQ")?;
                validate_operand(&value, "-MQ")?;
                set_once(&mut options.dep_target, value.clone(), "-MQ")?;
                options.argv.push(argument.clone());
                options.argv.push(value);
                options.dep_target_index = Some(options.argv.len() - 1);
                options.dep_target_option = Some("-MQ".into());
            }
            value if value.starts_with("-MQ") && value.len() > 3 => {
                let target = value
                    .strip_prefix("-MQ")
                    .expect("dep-target prefix was checked above");
                validate_operand(target, "-MQ")?;
                set_once(&mut options.dep_target, target.to_string(), "-MQ")?;
                options.argv.push(argument.clone());
                options.dep_target_index = Some(options.argv.len() - 1);
                options.dep_target_option = Some("-MQ".into());
                index += 1;
            }
            "-I" | "-L" | "-include" | "-isystem" | "-iquote" | "-idirafter"
            | "-imacros" | "-iprefix" | "-iwithprefix" | "-iwithprefixbefore" => {
                let value = next_value(args, &mut index, argument)?;
                validate_operand(&value, argument)?;
                options.argv.push(argument.clone());
                options.argv.push(value);
                options.path_indices.push((options.argv.len() - 1, String::new()));
            }
            value if value.starts_with("-I") && value.len() > 2 => {
                validate_operand(
                    value
                        .strip_prefix("-I")
                        .expect("include prefix was checked above"),
                    "-I",
                )?;
                options.argv.push(argument.clone());
                options.path_indices.push((options.argv.len() - 1, "-I".into()));
                index += 1;
            }
            value if value.starts_with("-L") && value.len() > 2 => {
                validate_operand(
                    value
                        .strip_prefix("-L")
                        .expect("library prefix was checked above"),
                    "-L",
                )?;
                options.argv.push(argument.clone());
                options.path_indices.push((options.argv.len() - 1, "-L".into()));
                index += 1;
            }
            value if attached_path_prefix(value).is_some() => {
                let prefix = attached_path_prefix(value)
                    .expect("attached path prefix was checked above");
                let path = value
                    .strip_prefix(prefix)
                    .expect("attached path prefix must be a prefix");
                validate_operand(path, prefix)?;
                options.argv.push(argument.clone());
                options
                    .path_indices
                    .push((options.argv.len() - 1, prefix.to_string()));
                index += 1;
            }
            "-E" | "-S" | "-P" | "-fsyntax-only" | "-save-temps" | "-###" => {
                return Err(format!(
                    "`{argument}` does not produce a declared build artifact"
                ));
            }
            value if value.starts_with('-') => {
                reject_embedded_tool_override(value)?;
                validate_supported_flag(value)?;
                options.argv.push(argument.clone());
                index += 1;
            }
            value => {
                validate_operand(value, "source")?;
                let argv_index = options.argv.len();
                options.sources.push(value.to_string());
                options.source_indices.push(argv_index);
                options.argv.push(argument.clone());
                index += 1;
            }
        }
    }
    if options.probe.is_some() && !options.argv.is_empty() {
        return Err("compiler metadata probes must be used without compiler flags or sources".to_string());
    }
    let standalone_verbose =
        options.verbose && options.argv.iter().all(|argument| argument == "-v");
    if options.sources.is_empty()
        && !options.help
        && !options.version
        && options.probe.is_none()
        && !standalone_verbose
    {
        return Err(format!("{verb} needs at least one source or object"));
    }
    if options.depfile.is_some() && !options.dep_mode {
        return Err("`-MF` needs `-MD` or `-MMD`".to_string());
    }
    if options.dep_target.is_some() && !options.dep_mode {
        return Err("dep target needs -MD or -MMD".to_string());
    }
    if options.dep_phony && !options.dep_mode {
        return Err("dep phony mode needs -MD or -MMD".to_string());
    }
    Ok(options)
}

fn response_scope_roots(args: &[String], current_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = vec![current_dir.to_path_buf()];
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        let value = if argument == "--project-root" || argument == "--build-root" {
            index += 1;
            args.get(index)
                .ok_or_else(|| format!("{argument} needs a directory"))?
                .as_str()
        } else if let Some(value) = argument
            .strip_prefix("--project-root=")
            .or_else(|| argument.strip_prefix("--build-root="))
        {
            value
        } else {
            index += 1;
            continue;
        };
        if value.is_empty() || value.contains('\0') {
            return Err("an explicit project/build root must be a non-empty path".to_string());
        }
        let path = absolutize_lexically(current_dir, value, "project/build root")?;
        let path = canonical_real_directory(&path, &format!("explicit root {value}"))?;
        roots.push(path);
        index += 1;
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn expand_response_files(
    args: &[String],
    current_dir: &Path,
    scope_roots: &[PathBuf],
) -> Result<Vec<String>, String> {
    let mut expanded = Vec::new();
    let mut stack = HashSet::new();
    let mut file_count = 0usize;
    expand_response_args(
        args,
        current_dir,
        scope_roots,
        0,
        &mut stack,
        &mut file_count,
        &mut expanded,
    )?;
    Ok(expanded)
}

fn expand_response_args(
    args: &[String],
    base_dir: &Path,
    scope_roots: &[PathBuf],
    depth: usize,
    stack: &mut HashSet<PathBuf>,
    file_count: &mut usize,
    expanded: &mut Vec<String>,
) -> Result<(), String> {
    if depth > MAX_RESPONSE_DEPTH {
        return Err(format!(
            "response-file nesting exceeds {MAX_RESPONSE_DEPTH} levels"
        ));
    }
    for argument in args {
        if !argument.starts_with('@') {
            expanded.push(argument.clone());
            if expanded.len() > MAX_RESPONSE_ARGS {
                return Err(format!(
                    "response-file expansion exceeds {MAX_RESPONSE_ARGS} arguments"
                ));
            }
            continue;
        }
        let raw_path = argument.strip_prefix('@').unwrap_or_default();
        if raw_path.is_empty() {
            return Err("response-file reference has no path".to_string());
        }
        if *file_count >= MAX_RESPONSE_FILES {
            return Err(format!(
                "response-file expansion exceeds {MAX_RESPONSE_FILES} files"
            ));
        }
        let path = absolutize_lexically(base_dir, raw_path, "response file")?;
        let canonical = fs::canonicalize(&path)
            .map_err(|error| format!("response file {raw_path} is unavailable: {error}"))?;
        if !scope_roots.iter().any(|root| canonical.starts_with(root)) {
            return Err(format!(
                "response file {raw_path} is outside the explicit project/build roots"
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect response file {raw_path}: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("response file {raw_path} is not a regular file"));
        }
        if !stack.insert(canonical.clone()) {
            return Err(format!(
                "response-file expansion contains a cycle at {raw_path}"
            ));
        }
        *file_count += 1;
        let bytes = read_bounded_response_file(&canonical)?;
        let words = parse_response_words(&bytes, raw_path)?;
        let nested_base = canonical.parent().unwrap_or(base_dir);
        expand_response_args(
            &words,
            nested_base,
            scope_roots,
            depth + 1,
            stack,
            file_count,
            expanded,
        )?;
        stack.remove(&canonical);
    }
    Ok(())
}

fn read_bounded_response_file(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("read response file {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("response file {} is not a regular file", path.display()));
    }
    if metadata.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "response file {} exceeds {MAX_RESPONSE_BYTES} bytes",
            path.display()
        ));
    }
    let mut file = fs::File::open(path)
        .map_err(|error| format!("read response file {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_RESPONSE_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read response file {}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(format!(
            "response file {} exceeds {MAX_RESPONSE_BYTES} bytes",
            path.display()
        ));
    }
    Ok(bytes)
}

fn parse_response_words(bytes: &[u8], source: &str) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| format!("response file {source} is not UTF-8"))?;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote = None;
    let mut escaped = false;
    for character in text.chars() {
        if escaped {
            word.push(character);
            started = true;
            escaped = false;
            continue;
        }
        match quote {
            Some('\'') => {
                if character == '\'' {
                    quote = None;
                } else {
                    word.push(character);
                }
                started = true;
            }
            Some('\"') => {
                if character == '\"' {
                    quote = None;
                } else if character == '\\' {
                    escaped = true;
                } else {
                    word.push(character);
                }
                started = true;
            }
            Some(_) => unreachable!("response quote state must be single or double"),
            None if character == '\\' => {
                escaped = true;
                started = true;
            }
            None if character == '\'' || character == '\"' => {
                quote = Some(character);
                started = true;
            }
            None if character.is_ascii_whitespace() => {
                if started {
                    push_response_word(&mut words, &mut word)?;
                    started = false;
                }
            }
            None => {
                word.push(character);
                started = true;
            }
        }
    }
    if escaped {
        return Err(format!(
            "response file {source} ends with an incomplete escape"
        ));
    }
    if quote.is_some() {
        return Err(format!("response file {source} has an unterminated quote"));
    }
    if started {
        push_response_word(&mut words, &mut word)?;
    }
    Ok(words)
}

fn push_response_word(words: &mut Vec<String>, word: &mut String) -> Result<(), String> {
    if word.len() > MAX_RESPONSE_BYTES as usize {
        return Err("one response-file argument exceeds the response-file bound".to_string());
    }
    words.push(std::mem::take(word));
    if words.len() > MAX_RESPONSE_ARGS {
        return Err(format!(
            "response-file expansion exceeds {MAX_RESPONSE_ARGS} arguments"
        ));
    }
    Ok(())
}

fn build_invocation(
    project_root: &Path,
    verb: &str,
    options: &DriverOptions,
) -> Result<Invocation, String> {
    let build_root = resolve_build_root(project_root, options)?;
    let mut sources = Vec::with_capacity(options.sources.len());
    for source in &options.sources {
        let path = validate_source(project_root, &options.working_dir, source)?;
        sources.push(path);
    }
    if options.output.as_deref() == Some("-") {
        return Err("`-o -` cannot produce a declared file output".to_string());
    }
    let output = options
        .output
        .clone()
        .map(|output| {
            normalize_output_path(
                &output,
                project_root,
                &build_root,
                &options.working_dir,
                options.build_root.is_some(),
                "-o",
            )
        })
        .transpose()?;
    if output
        .as_ref()
        .is_some_and(|output| sources.iter().any(|source| source == output))
    {
        return Err("the output path cannot overwrite a source or object input".to_string());
    }
    if options.depfile.as_deref() == Some("-") {
        return Err("`-MF -` cannot produce a declared dependency file".to_string());
    }
    let depfile = options
        .depfile
        .clone()
        .map(|depfile| {
            normalize_output_path(
                &depfile,
                project_root,
                &build_root,
                &options.working_dir,
                options.build_root.is_some(),
                "-MF",
            )
        })
        .transpose()?;
    if depfile
        .as_ref()
        .is_some_and(|depfile| sources.iter().any(|source| source == depfile))
    {
        return Err("the dependency-file path cannot overwrite a source or object input".to_string());
    }
    let outputs = if options.compile_only {
        if output.is_some() && sources.len() != 1 {
            return Err("`-c -o` needs exactly one source".to_string());
        }
        let mut outputs = sources
            .iter()
            .map(|source| output.clone().unwrap_or_else(|| replace_extension(source, "o")))
            .collect::<Vec<_>>();
        if options.dep_mode {
            if depfile.is_some() && sources.len() != 1 {
                return Err("`-MF` needs exactly one source when compiling multiple files".to_string());
            }
            if let Some(dependency) = depfile.clone() {
                outputs.push(dependency);
            } else {
                let object_outputs = outputs.clone();
                outputs.extend(
                    object_outputs
                        .iter()
                        .map(|object| replace_extension(object, "d")),
                );
            }
        }
        outputs
    } else {
        if options.dep_mode {
            return Err("dependency generation is only supported with `-c`".to_string());
        }
        vec![output.clone().unwrap_or_else(|| {
            let default = if cfg!(windows) { "a.exe" } else { "a.out" };
            normalize_output_path(
                default,
                project_root,
                &build_root,
                &options.working_dir,
                options.build_root.is_some(),
                "link output",
            )
            .expect("static default C output path is valid")
        })]
    };
    let mut unique_outputs = BTreeSet::new();
    for output in &outputs {
        validate_relative_path(output, "output")?;
        if !unique_outputs.insert(output) {
            return Err(format!("duplicate declared output `{output}`"));
        }
        if sources.iter().any(|source| source == output) {
            return Err(format!("output `{output}` overwrites an input"));
        }
    }
    let dep_target = options
        .dep_target
        .clone()
        .map(|dep_target| {
            normalize_output_path(
                &dep_target,
                project_root,
                &build_root,
                &options.working_dir,
                options.build_root.is_some(),
                "dep target",
            )
        })
        .transpose()?;
    let excluded = outputs.iter().cloned().collect::<BTreeSet<_>>();
    let ignored_build_root = (options.build_root.is_some() && build_root != project_root)
        .then_some(build_root.as_path());
    let mut inputs = collect_project_inputs(project_root, &excluded, ignored_build_root)?;
    inputs.extend(sources.iter().cloned());
    inputs.sort();
    inputs.dedup();

    let mut normalized_argv = options.argv.clone();
    for (source, index) in sources.iter().zip(&options.source_indices) {
        let slot = normalized_argv
            .get_mut(*index)
            .ok_or_else(|| "driver source index is outside the parsed argument vector".to_string())?;
        *slot = source.clone();
    }
    if let Some(index) = options.output_index {
        let output = output
            .clone()
            .ok_or_else(|| "driver output index has no normalized output path".to_string())?;
        let argument = normalized_argv
            .get_mut(index)
            .ok_or_else(|| "driver output index is outside the parsed argument vector".to_string())?;
        *argument = replace_option_operand(argument, "-o", output);
    }
    if let Some(index) = options.depfile_index {
        let depfile = depfile
            .clone()
            .ok_or_else(|| "driver depfile index has no normalized depfile path".to_string())?;
        let argument = normalized_argv
            .get_mut(index)
            .ok_or_else(|| "driver depfile index is outside the parsed argument vector".to_string())?;
        *argument = replace_option_operand(argument, "-MF", depfile);
    }
    if let Some(index) = options.dep_target_index {
        let dep_target = dep_target
            .clone()
            .ok_or_else(|| "driver dep-target index has no normalized target path".to_string())?;
        let argument = normalized_argv
            .get_mut(index)
            .ok_or_else(|| "driver dep-target index is outside the parsed argument vector".to_string())?;
        let option = options.dep_target_option.as_deref().unwrap_or("-MT");
        *argument = replace_option_operand(argument, option, dep_target);
    }
    for (index, prefix) in &options.path_indices {
        let argument = normalized_argv
            .get_mut(*index)
            .ok_or_else(|| "driver path index is outside the parsed argument vector".to_string())?;
        let raw = if prefix.is_empty() {
            argument.as_str()
        } else {
            argument
                .strip_prefix(prefix)
                .ok_or_else(|| "driver path option changed during normalization".to_string())?
        };
        let normalized = normalize_working_path(
            raw,
            project_root,
            &options.working_dir,
            "include/library path",
        )?;
        *argument = if prefix.is_empty() {
            normalized
        } else {
            format!("{prefix}{normalized}")
        };
    }
    let mut argv = vec![verb.to_string()];
    for (index, argument) in normalized_argv.into_iter().enumerate() {
        if options.separator_index == Some(index) {
            argv.push("--".to_string());
        }
        argv.push(argument);
    }
    if options.separator_index == Some(options.argv.len()) {
        argv.push("--".to_string());
    }
    if options.dep_mode && options.depfile.is_none() && sources.len() == 1 {
        let dependency = outputs
            .last()
            .ok_or_else(|| "dependency output was not declared".to_string())?;
        argv.push("-MF".to_string());
        argv.push(dependency.clone());
    }
    if options.dep_mode && options.dep_target.is_none() && sources.len() == 1 {
        let object = outputs
            .first()
            .ok_or_else(|| "object output was not declared".to_string())?;
        argv.push("-MT".to_string());
        argv.push(object.clone());
    }
    Ok(Invocation {
        argv,
        sources,
        inputs,
        outputs,
    })
}

fn resolve_toolchain(
    roots: &Store::Roots,
    spec: &RefSpec::RefSpec,
    fixtures: Option<&Path>,
    offline: bool,
    project_root: &Path,
) -> Result<Provider::cc_toolchain::CcToolchainDescriptor, ProviderError> {
    let hangar = roots.hangar_dir();
    let target = Provider::cc_toolchain::requested_target(spec)
        .map_err(ProviderError::Unsupported)?;
    match Provider::cc_toolchain::resolve_for_target(&hangar, &target, fixtures.is_some()) {
        Ok(descriptor) => return Ok(descriptor),
        Err(error)
            if error
                == format!(
                    "Hangar has no verified C/C++ toolchain descriptor for target `{target}`"
                ) => {}
        Err(error) => return Err(ProviderError::BadOutput(error)),
    }
    let nix_index = if fixtures.is_none() {
        Some(
            crate::NixIndex::NixIndexClient::from_roots_with_mode(roots, offline)
                .map_err(ProviderError::NixIndex)?,
        )
    } else {
        None
    };
    let ctx = Provider::Ctx {
        fixtures,
        store_dir: &hangar,
        offline,
        project_dir: Some(project_root),
        nix_index: nix_index.as_ref(),
        nix_roots: Some(roots),
    };
    let table = SourceTable::empty();
    let realized = Provider::realize(spec, &table, &ctx)?;
    if matches!(
        realized.source_state,
        SourceState::Downloaded | SourceState::Substituted
    ) {
        Store::record_realized_mode(roots, &realized)
            .map_err(|error| ProviderError::BadOutput(error.to_string()))?;
    }
    Provider::cc_toolchain::resolve_for_target(&hangar, &target, fixtures.is_some())
        .map_err(ProviderError::BadOutput)
}

fn set_once_path(slot: &mut Option<PathBuf>, value: String, option: &str) -> Result<(), String> {
    validate_operand(&value, option)?;
    if slot.replace(PathBuf::from(value)).is_some() {
        return Err(format!("{option} may be specified only once"));
    }
    Ok(())
}

fn validate_operand(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\0') {
        return Err(format!("{label} needs a non-empty value"));
    }
    Ok(())
}

fn attached_path_prefix(argument: &str) -> Option<&'static str> {
    [
        "-iwithprefixbefore",
        "-iwithprefix",
        "-idirafter",
        "-isystem",
        "-iquote",
        "-imacros",
        "-iprefix",
        "-include",
        "-I",
        "-L",
    ]
    .into_iter()
    .find(|prefix| argument.starts_with(prefix) && argument.len() > prefix.len())
}

fn validate_supported_flag(argument: &str) -> Result<(), String> {
    const EXACT: &[&str] = &[
        "-ansi", "-pedantic", "-pipe", "-pthread", "-v", "-g", "-g0", "-g1", "-g2", "-g3",
        "-fPIC", "-fpic", "-fPIE", "-fpie",
        "-fno-PIC", "-fno-pic", "-fno-PIE", "-fno-pie", "-fcommon", "-fno-common",
        "-fexceptions", "-fno-exceptions", "-frtti", "-fno-rtti", "-ffunction-sections",
        "-fdata-sections", "-fstack-protector", "-fstack-protector-strong",
        "-fno-stack-protector", "-fstack-clash-protection", "-fno-omit-frame-pointer",
        "-fomit-frame-pointer", "-fno-plt", "-fno-semantic-interposition", "-ffast-math",
        "-fno-fast-math", "-fwrapv", "-fno-wrapv", "-fstrict-aliasing",
        "-fno-strict-aliasing", "-fstrict-overflow", "-fno-strict-overflow", "-fno-ident",
        "-fno-asynchronous-unwind-tables", "-Winvalid-pch", "-Winvalid-offsetof", "-rdynamic",
        "-s", "-pie", "-no-pie",
    ];
    if argument == "-Wl"
        || argument.starts_with("-Wl,")
        || argument.starts_with("-X")
        || argument.starts_with("-specs=")
        || argument.starts_with("-wrapper")
        || argument.starts_with("-fplugin")
        || argument.starts_with("-fprofile")
        || argument.starts_with("-gsplit-dwarf")
    {
        return Err(format!("unsupported compiler flag {argument}"));
    }
    if EXACT.contains(&argument)
        || argument.starts_with("-O")
        || argument.starts_with("-W")
        || argument.starts_with("-Winvalid-")
        || argument.starts_with("-fdiagnostics-")
        || argument.starts_with("-fmessage-length=")
        || argument.starts_with("-fvisibility=")
        || argument.starts_with("-std=")
        || argument.starts_with("--std=")
        || argument.starts_with("-D")
        || argument.starts_with("-U")
        || argument.starts_with("-l")
    {
        return Ok(());
    }
    if argument.starts_with("-m") || argument.starts_with("-f") {
        return Err(format!("unsupported or target-changing compiler flag {argument}"));
    }
    Err(format!("unsupported compiler flag {argument}"))
}
fn normalize_scoped_path(
    raw: &str,
    allowed_root: &Path,
    base_root: &Path,
    output_root: &Path,
    label: &str,
) -> Result<String, String> {
    let candidate = absolutize_lexically(base_root, raw, label)?;
    if !candidate.starts_with(allowed_root) {
        return Err(format!(
            "{label} path {raw} must stay inside {}",
            allowed_root.display()
        ));
    }
    reject_existing_symlinks(allowed_root, &candidate, label)?;
    let relative = candidate
        .strip_prefix(output_root)
        .map_err(|_| format!("{label} path {raw} is outside the project root"))?;
    let relative = relative
        .to_str()
        .ok_or_else(|| format!("{label} path {raw} is not UTF-8"))?;
    validate_relative_path(relative, label)?;
    Ok(relative.to_string())
}

fn normalize_working_path(
    raw: &str,
    project_root: &Path,
    working_dir: &Path,
    label: &str,
) -> Result<String, String> {
    normalize_scoped_path(raw, project_root, working_dir, project_root, label)
}

fn normalize_output_path(
    raw: &str,
    project_root: &Path,
    build_root: &Path,
    working_dir: &Path,
    explicit_build_root: bool,
    label: &str,
) -> Result<String, String> {
    let base = if explicit_build_root {
        build_root
    } else {
        working_dir
    };
    normalize_scoped_path(raw, build_root, base, project_root, label)
}

fn replace_option_operand(argument: &str, option: &str, value: String) -> String {
    if argument
        .strip_prefix(option)
        .is_some_and(|operand| !operand.is_empty())
    {
        format!("{option}{value}")
    } else {
        value
    }
}

fn absolutize_lexically(base: &Path, raw: &str, label: &str) -> Result<PathBuf, String> {
    validate_operand(raw, label)?;
    let input = Path::new(raw);
    let absolute = input.is_absolute();
    let mut output = if absolute {
        PathBuf::new()
    } else {
        base.to_path_buf()
    };
    for component in input.components() {
        match component {
            Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            Component::RootDir => output.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !output.pop() || (!absolute && !output.starts_with(base)) {
                    return Err(format!("{label} path {raw} escapes its scope"));
                }
            }
            Component::Normal(part) => output.push(part),
        }
    }
    if output.as_os_str().is_empty() {
        return Err(format!("{label} path {raw} is empty"));
    }
    Ok(output)
}

fn reject_existing_symlinks(
    scope_root: &Path,
    path: &Path,
    label: &str,
) -> Result<(), String> {
    let relative = path
        .strip_prefix(scope_root)
        .map_err(|_| format!("{label} path {} escapes its scope", path.display()))?;
    let mut current = scope_root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!("{label} path {} is not normalized", path.display()));
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("{label} path {} traverses a symlink", path.display()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!("inspect {label} path {}: {error}", path.display()));
            }
        }
    }
    Ok(())
}
fn resolve_project_root(options: &DriverOptions) -> Result<PathBuf, String> {
    let current = fs::canonicalize(
        std::env::current_dir()
            .map_err(|error| format!("could not determine the current directory: {error}"))?,
    )
    .map_err(|error| format!("could not canonicalize the current directory: {error}"))?;
    let raw = options
        .project_root
        .as_deref()
        .unwrap_or(current.as_path());
    let path = absolutize_lexically(&current, &raw.to_string_lossy(), "project root")?;
    let canonical = canonical_real_directory(&path, "project root")?;
    Ok(canonical)
}

fn resolve_build_root(
    project_root: &Path,
    options: &DriverOptions,
) -> Result<PathBuf, String> {
    let Some(raw) = options.build_root.as_deref() else {
        return Ok(project_root.to_path_buf());
    };
    let path = absolutize_lexically(
        &options.working_dir,
        &raw.to_string_lossy(),
        "build root",
    )?;
    let canonical = canonical_real_directory(&path, "build root")?;
    if !canonical.starts_with(project_root) {
        return Err(format!(
            "build root {} must be inside project root {}",
            canonical.display(),
            project_root.display()
        ));
    }
    Ok(canonical)
}

fn canonical_real_directory(path: &Path, label: &str) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {label} {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{label} {} is not a real directory", path.display()));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("{label} {} is unavailable: {error}", path.display()))
}

fn validate_source(
    root: &Path,
    working_dir: &Path,
    source: &str,
) -> Result<String, String> {
    let source = normalize_working_path(source, root, working_dir, "source")?;
    let path = root.join(&source);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| format!("source {source} is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("source {source} is not a regular file"));
    }
    Ok(source)
}

fn collect_project_inputs(
    root: &Path,
    excluded: &BTreeSet<String>,
    ignored_root: Option<&Path>,
) -> Result<Vec<String>, String> {
    let mut inputs = Vec::new();
    let mut bytes = 0u64;
    collect_project_inputs_inner(
        root,
        root,
        excluded,
        ignored_root,
        &mut inputs,
        &mut bytes,
    )?;
    Ok(inputs)
}

fn collect_project_inputs_inner(
    root: &Path,
    directory: &Path,
    excluded: &BTreeSet<String>,
    ignored_root: Option<&Path>,
    inputs: &mut Vec<String>,
    bytes: &mut u64,
) -> Result<(), String> {
    let mut entries = fs::read_dir(directory)
        .map_err(|error| format!("read project inputs in `{}`: {error}", directory.display()))?
        .map(|entry| entry.map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read project inputs: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("project input `{}` escaped the project root", path.display()))?;
        let relative_text = relative
            .to_str()
            .ok_or_else(|| format!("project input `{}` is not UTF-8", path.display()))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| format!("project input `{}` is not UTF-8", path.display()))?;
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("inspect project input `{relative_text}`: {error}"))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if ignored_root.is_some_and(|ignored| path == ignored) {
                continue;
            }
            if matches!(name, ".git" | ".jet" | "target" | "build" | "CMakeFiles") {
                continue;
            }
            collect_project_inputs_inner(
                root,
                &path,
                excluded,
                ignored_root,
                inputs,
                bytes,
            )?;
            continue;
        }
        if !metadata.is_file() || excluded.contains(relative_text) {
            continue;
        }
        if inputs.len() >= MAX_INPUT_FILES {
            return Err(format!("project input set exceeds {MAX_INPUT_FILES} files"));
        }
        *bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| "project input size overflowed".to_string())?;
        if *bytes > MAX_INPUT_BYTES {
            return Err(format!("project input set exceeds {MAX_INPUT_BYTES} bytes"));
        }
        inputs.push(relative_text.to_string());
    }
    Ok(())
}

/// The action CAS stores output bytes, while a linker also communicates the
/// executable bit through the output file mode. Restore that product property
/// at the driver boundary after both a cache hit and a fresh action. Compile
/// outputs intentionally remain ordinary data files.
fn mark_link_outputs_executable(project_root: &Path, outputs: &[String]) -> Result<(), String> {
    for output in outputs {
        let path = project_root.join(output);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            const O_NOFOLLOW: i32 = 0o400000;
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW)
                .open(&path)
                .map_err(|error| format!("open link output `{output}`: {error}"))?;
            let metadata = file
                .metadata()
                .map_err(|error| format!("inspect link output `{output}`: {error}"))?;
            if !metadata.is_file() {
                return Err(format!("link output `{output}` is not a regular file"));
            }
            let mut permissions = metadata.permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            file.set_permissions(permissions)
                .map_err(|error| format!("set executable mode on link output `{output}`: {error}"))?;
        }
        #[cfg(not(unix))]
        {
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect link output `{output}`: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!("link output `{output}` is not a regular file"));
            }
        }
    }
    Ok(())
}

fn replace_extension(path: &str, extension: &str) -> String {
    let mut path = PathBuf::from(path);
    path.set_extension(extension);
    path.to_string_lossy().into_owned()
}

fn validate_relative_path(path: &str, label: &str) -> Result<(), String> {
    let value = Path::new(path);
    if path.is_empty()
        || path.contains('\0')
        || value.is_absolute()
        || value.components().any(|component| {
            matches!(
                component,
                Component::CurDir | Component::ParentDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!(
            "{label} path `{path}` must be project-relative and must not contain `..`"
        ));
    }
    Ok(())
}

fn next_value(args: &[String], index: &mut usize, option: &str) -> Result<String, String> {
    let value = args
        .get(index.saturating_add(1))
        .ok_or_else(|| format!("`{option}` needs a value"))?;
    if value.starts_with('-') {
        return Err(format!("`{option}` needs a value"));
    }
    *index += 2;
    Ok(value.clone())
}

fn set_once(slot: &mut Option<String>, value: String, option: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("`{option}` needs a non-empty value"));
    }
    if slot.replace(value).is_some() {
        return Err(format!("`{option}` may be specified only once"));
    }
    Ok(())
}

fn set_probe(slot: &mut Option<DriverProbe>, probe: DriverProbe) -> Result<(), String> {
    if slot.replace(probe).is_some() {
        return Err("only one standalone compiler metadata probe is allowed".to_string());
    }
    Ok(())
}

fn reject_embedded_tool_override(argument: &str) -> Result<(), String> {
    if argument.contains("sysroot")
        || argument.contains("gcc-toolchain")
        || argument.starts_with("-fuse-ld")
        || argument.starts_with("-B")
    {
        return Err(format!(
            "`{argument}` would override the pinned Hangar compiler or linker"
        ));
    }
    Ok(())
}

fn usage(verb: &str) -> String {
    format!(
        "jet {verb} [--offline] [--fixtures <dir>] [--project-root <dir>] [--build-root <dir>] [--target <triple>] [-c] [-o <path>] [-MD|-MMD] [-MF <path>] [--] <sources>"
    )
}

fn compiler_version(descriptor: &Provider::cc_toolchain::CcToolchainDescriptor) -> &str {
    let version = descriptor
        .version
        .strip_prefix("gcc-")
        .unwrap_or(&descriptor.version);
    version.split_once('+').map_or(version, |(version, _)| version)
}

fn report_usage_error(verb: &str, reason: &str) {
    let theme = Theme::resolve_choice(ColorChoice::Auto);
    theme.error_coded(
        "E2102",
        &format!("invalid arguments for `jet {verb}`"),
        reason,
        &usage(verb),
    );
}

fn report_provider_error(error: &ProviderError) {
    let theme = Theme::resolve_choice(ColorChoice::Auto);
    match error {
        ProviderError::Offline(reason) => theme.error_coded(
            "E1276",
            "C/C++ toolchain is unavailable offline",
            reason,
            "install or import the pinned Hangar bundle, then retry without `--offline` only when acquisition is allowed",
        ),
        ProviderError::SandboxUnavailable(reason) => theme.error_coded(
            "E1275",
            "C/C++ build sandbox is unavailable",
            reason,
            "install the native sandbox backend and retry; Jet will not run the compiler unsandboxed",
        ),
        ProviderError::Unsupported(reason) => theme.error_coded(
            "E1340",
            "no pinned C/C++ toolchain is available",
            reason,
            "provide the declared Hangar bundle for this target; Jet will not use a host or PATH compiler",
        ),
        ProviderError::BadOutput(reason) => theme.error_coded(
            "E1340",
            "the pinned C/C++ toolchain failed integrity checks",
            reason,
            "repair or remove the corrupt Hangar object and provision it again from a trusted bundle",
        ),
        _ => super::report_provider_error(&theme, error),
    }
}

fn report_execution_error(error: &BuildExecutionError) {
    let theme = Theme::resolve_choice(ColorChoice::Auto);
    match error {
        BuildExecutionError::SandboxUnavailable => theme.error_coded(
            "E1275",
            "C/C++ build sandbox is unavailable",
            "the native action boundary could not be enforced",
            "install the native sandbox backend and retry; Jet will not run the compiler unsandboxed",
        ),
        BuildExecutionError::Reported { report } => theme.error_coded(
            report.code.as_deref().unwrap_or("E3505"),
            "C/C++ compiler action failed",
            &report.render(),
            "fix the reported compiler or linker error; no artifact was accepted",
        ),
        BuildExecutionError::MissingGrant { action, capability } => theme.error_coded(
            "E1340",
            "C/C++ action lacked its required build grant",
            &format!("action `{action}` requested `{capability:?}`"),
            "retry through the canonical `jet cc` or `jet c++` entry point",
        ),
        BuildExecutionError::IO { action, detail } => theme.error_coded(
            "E1340",
            "C/C++ build action could not complete",
            &format!("action `{action}`: {detail}"),
            "check the project paths and declared toolchain object, then retry",
        ),
        BuildExecutionError::ProbeFailed { probe, detail } => theme.error_coded(
            "E1340",
            "C/C++ build probe failed",
            &format!("probe `{probe}`: {detail}"),
            "repair the pinned toolchain or target descriptor, then retry",
        ),
        BuildExecutionError::InvalidGraph(error) => theme.error_coded(
            "E1340",
            "C/C++ build graph is invalid",
            &format!("{error:?}"),
            "report the graph error; the compiler action was not accepted",
        ),
    }
}

fn report_operational_error(headline: &str, reason: &str, fix: &str) {
    Theme::resolve_choice(ColorChoice::Auto).error_coded("E1340", headline, reason, fix);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_separate_value_flags_do_not_become_source_inputs() {
        let args = [
            "-c",
            "-D",
            "FEATURE=1",
            "-x",
            "c",
            "-std",
            "c11",
            "-l",
            "m",
            "main.c",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let options = parse_args("cc", &args).unwrap();
        assert_eq!(options.sources, vec!["main.c"]);
        let expected = [
            "-c", "-D", "FEATURE=1", "-x", "c", "-std", "c11", "-l", "m",
        ]
        .map(str::to_string)
        .to_vec();
        assert_eq!(options.argv, expected);
    }

    #[test]
    fn host_toolchain_overrides_are_rejected_before_graph_creation() {
        for argument in ["--sysroot=/host", "-B/host-tools", "-fuse-ld=gold"] {
            let args = vec![argument.to_string(), "main.c".to_string()];
            let error = parse_args("cc", &args).unwrap_err();
            assert!(error.contains("pinned Hangar"));
        }
    }

    #[test]
    fn attached_output_and_depfile_forms_keep_their_option_prefix() {
        assert_eq!(
            replace_option_operand("-oout.o", "-o", "build/out.o".into()),
            "-obuild/out.o"
        );
        assert_eq!(
            replace_option_operand("-MFdeps.d", "-MF", "build/deps.d".into()),
            "-MFbuild/deps.d"
        );
        assert_eq!(
            replace_option_operand("out.o", "-o", "build/out.o".into()),
            "build/out.o"
        );
        assert_eq!(
            replace_option_operand("-MTout.o", "-MT", "build/out.o".into()),
            "-MTbuild/out.o"
        );
        assert_eq!(
            replace_option_operand("out.o", "-MQ", "build/out.o".into()),
            "build/out.o"
        );
    }

    #[test]
    fn standalone_compiler_probes_are_bounded_and_do_not_need_sources() {
        for (argument, probe) in [
            ("-dumpmachine", DriverProbe::DumpMachine),
            ("-print-sysroot", DriverProbe::PrintSysroot),
            ("-dumpversion", DriverProbe::DumpVersion),
        ] {
            let options = parse_args("cc", &[argument.to_string()]).unwrap();
            assert_eq!(options.probe, Some(probe));
            assert!(options.sources.is_empty());
        }
        assert!(parse_args("cc", &["-dumpmachine".into(), "-v".into()]).is_err());
        assert!(parse_args("cc", &["-dumpmachine".into(), "main.c".into()]).is_err());
        let options = parse_args("cc", &["-v".into()]).unwrap();
        assert!(options.verbose);
        assert!(options.sources.is_empty());
    }
}
