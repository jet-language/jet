//! `jet package` source adapter.
//!
//! The checked [`PackageBundlePlan`] owns package meaning and file layout.
//! This module only materializes that plan into native bundle transports
//! through the checked process authority where a platform tool is required,
//! and records the typed receipt returned by the shared store.


use jet::Comptime::Build::{self, NativeSandboxError};
use jet::ExitCodes;
use jet_foundation::PerformanceBudget::CanonicalJson;
use jet_foundation::Report::{StatusEnvelope, StatusFields, StatusValue};
use jet_pkg_model::Bundler::{
    BundleExecutable, BundleFile, BundleIcon, BundleKind, BundleMaterializerFact, BundleMetadata,
    BundleSigningFact, BundleSpec, BundleTarget, GameBundleAsset, GameBundleSpec,
    GameCrashReporterSpec, IconFormat, PackageBundleArtifact, PackageBundlePlan, SigningHook,
    SigningPhase, UpdaterSpec, GAME_CRASH_REPORTER_ABI,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const PACKAGE_NATIVE_AUTHORITY: &str = "Build.native_sandboxed";
const GAME_PACKAGE_RECEIPT_SCHEMA: &str = "jet.game.package.receipt";
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, Default)]
struct PackageOptions {
    target: Option<BundleTarget>,
    kind: BundleKind,
    package: Option<String>,
    name: Option<String>,
    version: Option<String>,
    executable: Option<PathBuf>,
    output: Option<PathBuf>,
    icon: Option<PathBuf>,
    icon_format: Option<IconFormat>,
    icon_size: u32,
    publisher: String,
    description: String,
    update_channel: Option<String>,
    update_url: Option<String>,
    source: Option<PathBuf>,
    profile: Option<String>,
    phase: Option<String>,
    backend: Option<String>,
    renderer: Option<String>,
    cook_mode: Option<String>,
    export_preset: Option<String>,
    deploy_to: Option<PathBuf>,
    crash_reporter: Option<String>,
    crash_consent: Option<String>,
    dry_run: bool,
    explain: bool,
    cancel: bool,
    resume: bool,
    force_clean: bool,
    scripts_only: bool,
    run_now: bool,
}

struct BuiltPackage {
    artifact: PackageBundleArtifact,
    plan: PackageBundlePlan,
    input_paths: Vec<PathBuf>,
    output: PathBuf,
}

/// Materialized facts used to bind the typed receipt to the output transport.
struct MaterializedFacts {
    sha256: String,
    bytes: u64,
    materializer: BundleMaterializerFact,
    signing: Option<BundleSigningFact>,
}
struct SigningRun {
    requested: bool,
    verified: bool,
    tools: Vec<(String, String, String)>,
}

/// Run `jet package` against an explicit checked executable input.
/// Run `jet package` against the checked desktop or game package path.
pub(crate) fn run_package(args: &[String], mode: crate::OutputMode) -> i32 {
    let options = match parse_package_args(args) {
        Ok(options) => options,
        Err(error) => {
            if mode.json {
                println!(
                    "{}",
                    StatusEnvelope::new("package", false)
                        .with_field("error", error.as_str())
                        .json()
                );
            } else {
                eprintln!("jet package: {error}");
            }
            return ExitCodes::USER_ERROR;
        }
    };
    if options.kind == BundleKind::Game {
        return run_game_package(args, options, mode);
    }
    let explain = options.explain;
    let dry_run = options.dry_run || explain;
    let result = build_package(options).and_then(|built| {
        let receipt_digest = if dry_run {
            String::new()
        } else {
            record_package_receipt(args, &built.input_paths, &built.artifact)?.digest
        };
        render_package_report(
            &built.artifact,
            &built.plan,
            &built.output,
            &receipt_digest,
            explain,
            mode,
        );
        Ok::<(), String>(())
    });
    match result {
        Ok(()) => ExitCodes::OK,
        Err(error) => {
            if mode.json {
                println!(
                    "{}",
                    StatusEnvelope::new("package", false)
                        .with_field("error", error.as_str())
                        .json()
                );
            } else {
                eprintln!("jet package: {error}");
            }
            ExitCodes::USER_ERROR
        }
    }
}

fn build_package(options: PackageOptions) -> Result<BuiltPackage, String> {
    if options.source.is_some()
        || options.profile.is_some()
        || options.phase.is_some()
        || options.backend.is_some()
        || options.renderer.is_some()
        || options.cook_mode.is_some()
        || options.export_preset.is_some()
        || options.deploy_to.is_some()
        || options.crash_reporter.is_some()
        || options.crash_consent.is_some()
        || options.dry_run
        || options.explain
        || options.cancel
        || options.resume
        || options.force_clean
        || options.scripts_only
        || options.run_now
    {
        return Err("game package options require `--kind game`".into());
    }
    let target = options
        .target
        .ok_or_else(|| "`jet package` needs an explicit `--target`".to_string())?;
    let executable_path = options
        .executable
        .ok_or_else(|| "`jet package` needs `--executable <path>`".to_string())?;
    ensure_regular_game_file(&executable_path, "executable")?;
    let executable_bytes = fs::read(&executable_path)
        .map_err(|error| format!("could not read executable `{}`: {error}", executable_path.display()))?;
    let executable_name = options
        .name
        .clone()
        .or_else(|| executable_path.file_stem().map(|value| value.to_string_lossy().into_owned()))
        .ok_or_else(|| "`jet package` needs a non-empty executable name".to_string())?;
    let package = options.package.clone().unwrap_or_else(|| executable_name.clone());
    let version = options.version.clone().unwrap_or_else(|| "0.1.0".into());
    let mut metadata = BundleMetadata::new(package.clone(), executable_name.clone());
    metadata.publisher = options.publisher;
    metadata.description = options.description;
    let executable = BundleExecutable::new(
        executable_name,
        executable_path.to_string_lossy().into_owned(),
        executable_bytes,
    );
    let mut spec = BundleSpec::new(
        options.kind,
        target,
        package.clone(),
        version,
        executable,
        metadata,
    );
    let mut input_paths = vec![executable_path];
    if let Some(icon_path) = options.icon {
        ensure_regular_game_file(&icon_path, "icon")?;
        let icon_bytes = fs::read(&icon_path)
            .map_err(|error| format!("could not read icon `{}`: {error}", icon_path.display()))?;
        let format = options
            .icon_format
            .or_else(|| icon_path.extension().and_then(|ext| IconFormat::parse(&ext.to_string_lossy())))
            .ok_or_else(|| "`jet package --icon` needs `--icon-format` when the extension is unknown".to_string())?;
        let icon = BundleIcon::new(package.clone(), format, options.icon_size, icon_bytes)
            .with_source(icon_path.to_string_lossy().into_owned());
        spec.icons.push(icon);
        input_paths.push(icon_path);
    }
    if options.update_channel.is_some() != options.update_url.is_some() {
        return Err("`--update-channel` and `--update-url` must be supplied together".into());
    }
    if let (Some(channel), Some(url)) = (options.update_channel, options.update_url) {
        spec.updater = Some(UpdaterSpec::new(channel, url));
    }
    let plan = PackageBundlePlan::from_spec(&spec).map_err(|error| error.to_string())?;
    let output = resolve_output_path(options.output, &plan.artifact_name)?;
    let artifact = if options.dry_run || options.explain {
        plan.materialized_artifact(plan.logical_artifact_sha256(), plan.logical_artifact_bytes())
    } else {
        let facts = materialize_package(&plan, &output)?;
        let mut artifact = plan.materialized_artifact(facts.sha256, facts.bytes);
        artifact.receipt.record_materializer(facts.materializer);
        if let Some(signing) = facts.signing {
            artifact.receipt.record_signing(signing);
        }
        artifact
    };
    Ok(BuiltPackage {
        artifact,
        plan,
        input_paths,
        output,
    })
}
fn parse_package_args(args: &[String]) -> Result<PackageOptions, String> {
    let mut options = PackageOptions {
        icon_size: 256,
        ..PackageOptions::default()
    };
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "package" {
            index += 1;
            continue;
        }
        if matches!(argument, "--json" | "--quiet" | "--no-color" | "--color")
            || argument.starts_with("--color=")
        {
            index += 1;
            continue;
        }
        if argument == "--game" {
            options.kind = BundleKind::Game;
            index += 1;
            continue;
        }
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(flag, value)| (flag, Some(value)));
        let value = |name: &str, inline: Option<&str>, index: &mut usize| {
            if let Some(value) = inline {
                if value.is_empty() {
                    return Err(format!("`{name}` needs a value"));
                }
                Ok(value.to_string())
            } else {
                *index += 1;
                args.get(*index)
                    .filter(|value| !value.starts_with('-'))
                    .cloned()
                    .ok_or_else(|| format!("`{name}` needs a value"))
            }
        };
        match flag {
            "--target" => {
                let value = value("--target", inline, &mut index)?;
                options.target = BundleTarget::parse(&value);
                if options.target.is_none() {
                    return Err(format!("unknown package target `{value}`"));
                }
            }
            "--kind" => {
                let value = value("--kind", inline, &mut index)?;
                options.kind = match value.as_str() {
                    "desktop" => BundleKind::Desktop,
                    "game" => BundleKind::Game,
                    _ => return Err(format!("unknown package kind `{value}`")),
                };
            }
            "--package" => options.package = Some(value("--package", inline, &mut index)?),
            "--name" => options.name = Some(value("--name", inline, &mut index)?),
            "--version" => options.version = Some(value("--version", inline, &mut index)?),
            "--executable" => options.executable = Some(PathBuf::from(value(
                "--executable",
                inline,
                &mut index,
            )?)),
            "--output" => options.output = Some(PathBuf::from(value("--output", inline, &mut index)?)),
            "--icon" => options.icon = Some(PathBuf::from(value("--icon", inline, &mut index)?)),
            "--icon-format" => {
                let value = value("--icon-format", inline, &mut index)?;
                options.icon_format = IconFormat::parse(&value);
                if options.icon_format.is_none() {
                    return Err(format!("unknown icon format `{value}`"));
                }
            }
            "--icon-size" => {
                let value = value("--icon-size", inline, &mut index)?;
                options.icon_size = value
                    .parse()
                    .map_err(|_| "`--icon-size` must be a positive integer".to_string())?;
                if options.icon_size == 0 {
                    return Err("`--icon-size` must be a positive integer".into());
                }
            }
            "--publisher" => options.publisher = value("--publisher", inline, &mut index)?,
            "--description" => options.description = value("--description", inline, &mut index)?,
            "--update-channel" => {
                options.update_channel = Some(value("--update-channel", inline, &mut index)?)
            }
            "--update-url" => options.update_url = Some(value("--update-url", inline, &mut index)?),
            "--source" => options.source = Some(PathBuf::from(value("--source", inline, &mut index)?)),
            "--profile" => options.profile = Some(value("--profile", inline, &mut index)?),
            "--phase" => options.phase = Some(value("--phase", inline, &mut index)?),
            "--backend" => options.backend = Some(value("--backend", inline, &mut index)?),
            "--renderer" => options.renderer = Some(value("--renderer", inline, &mut index)?),
            "--cook" | "--cook-mode" => {
                options.cook_mode = Some(value("--cook-mode", inline, &mut index)?)
            }
            "--export" => {
                if inline.is_some() {
                    options.export_preset = Some(value("--export-preset", inline, &mut index)?);
                } else {
                    options.phase = Some(match options.phase.take() {
                        Some(phases) => format!("{phases},export"),
                        None => "export".into(),
                    });
                }
            }
            "--export-preset" => {
                options.export_preset = Some(value("--export-preset", inline, &mut index)?);
            }
            "--deploy-to" => options.deploy_to = Some(PathBuf::from(value("--deploy-to", inline, &mut index)?)),
            "--crash-reporter" => {
                options.crash_reporter = Some(value("--crash-reporter", inline, &mut index)?)
            }
            "--crash-consent" => {
                options.crash_consent = Some(value("--crash-consent", inline, &mut index)?)
            }
            "--dry-run" => options.dry_run = true,
            "--explain" => options.explain = true,
            "--cancel" => options.cancel = true,
            "--resume" => options.resume = true,
            "--clean" | "--force-clean" => options.force_clean = true,
            "--scripts-only" => options.scripts_only = true,
            "--run-now" => options.run_now = true,
            "--run" => {
                options.phase = Some(match options.phase.take() {
                    Some(phases) => format!("{phases},run"),
                    None => "run".into(),
                });
            }
            "--deploy" => {
                options.phase = Some(match options.phase.take() {
                    Some(phases) => format!("{phases},deploy"),
                    None => "deploy".into(),
                });
            }
            "--help" | "help" => return Err(package_help()),
            _ if argument.starts_with('-') => return Err(format!("unknown package flag `{argument}`")),
            _ => {
                if options.source.is_some() {
                    return Err(format!("unexpected package argument `{argument}`"));
                }
                options.source = Some(PathBuf::from(argument));
            }
        }
        index += 1;
    }
    Ok(options)
}

fn resolve_output_path(output: Option<PathBuf>, artifact_name: &str) -> Result<PathBuf, String> {
    let path = output.unwrap_or_else(|| PathBuf::from(artifact_name));
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| format!("could not read current directory: {error}"))?
            .join(path)
    };
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("package output `{}` has no file name", path.display()))?;
    let suffix = artifact_name
        .rfind('.')
        .map(|index| &artifact_name[index..])
        .filter(|suffix| *suffix != ".")
        .ok_or_else(|| format!("package artifact `{artifact_name}` has no native suffix"))?;
    if !name.ends_with(suffix) {
        return Err(format!(
            "package output `{name}` must retain the native artifact suffix `{suffix}`"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err(format!("package output `{}` contains a control character", path.display()));
    }
    Ok(path)
}

fn materialize_package(plan: &PackageBundlePlan, output: &Path) -> Result<MaterializedFacts, String> {
    let parent = output.parent().filter(|path| !path.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create package output directory `{}`: {error}", parent.display()))?;
    match plan.target {
        BundleTarget::LinuxAppImage => materialize_linux(plan, output, parent),
        BundleTarget::MacOSApp => materialize_macos(plan, output, parent),
        BundleTarget::WindowsMsix => materialize_msix(plan, output, parent),
    }
}

fn materialize_linux(
    plan: &PackageBundlePlan,
    output: &Path,
    parent: &Path,
) -> Result<MaterializedFacts, String> {
    let output_name = output_name(output)?;
    ensure_destination_free(output)?;
    let source_stage = stage_path(parent, &output_name, "appdir");
    let output_stage = stage_path(parent, &output_name, "appimage-output");
    ensure_destination_free(&source_stage)?;
    ensure_destination_free(&output_stage)?;
    let result = (|| {
        fs::create_dir(&source_stage).map_err(|error| {
            format!(
                "could not stage Linux AppDir `{}`: {error}",
                source_stage.display()
            )
        })?;
        fs::create_dir(&output_stage).map_err(|error| {
            format!(
                "could not stage AppImage output `{}`: {error}",
                output_stage.display()
            )
        })?;
        write_bundle_tree(plan, &source_stage, false)?;

        let tool = resolve_declared_tool("appimagetool")?;
        let tool_identity = jet::SHA256::sha256_file_hex(&tool).map_err(|error| {
            format!(
                "could not hash declared AppImage tool `{}`: {error}",
                tool.display()
            )
        })?;
        let status = Build::native_sandbox_status();
        if !status.available {
            return Err(format!(
                "AppImage materialization requires the checked native process authority; {} ({})",
                status.reason, status.mechanism
            ));
        }
        let environment = materializer_environment();
        let mut signing = SigningRun::new(plan);
        run_signing_hooks(
            plan,
            SigningPhase::PreSign,
            &source_stage,
            &output_stage,
            &environment,
            &mut signing,
        )?;
        let version_run = run_authorized_tool(
            &tool,
            &["--version".to_string()],
            &source_stage,
            &output_stage,
            &environment,
        )?;
        if !version_run.output.status.success() {
            return Err(format!(
                "declared AppImage tool `{}` failed its --version probe ({}): {}",
                tool.display(),
                version_run.output.status,
                native_tool_output_detail(&version_run.output)
            ));
        }
        let tool_version = native_tool_version(&version_run.output)?;
        let staged_artifact = output_stage.join(&output_name);
        let package_run = run_authorized_tool(
            &tool,
            &[
                authorized_tool_path(&source_stage, "/work/source"),
                authorized_tool_path(&staged_artifact, &format!("/work/output/{output_name}")),
            ],
            &source_stage,
            &output_stage,
            &environment,
        )?;
        if !package_run.output.status.success() {
            return Err(format!(
                "declared AppImage tool `{}` failed to materialize `{}` ({}): {}",
                tool.display(),
                output_name,
                package_run.output.status,
                native_tool_output_detail(&package_run.output)
            ));
        }
        let metadata = fs::symlink_metadata(&staged_artifact).map_err(|error| {
            format!(
                "AppImage tool did not produce `{}`: {error}",
                staged_artifact.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "AppImage tool output `{}` is not a regular file",
                staged_artifact.display()
            ));
        }
        set_mode(&staged_artifact, 0o755)?;
        let artifact_sha256 = jet::SHA256::sha256_file_hex(&staged_artifact).map_err(|error| {
            format!(
                "could not hash materialized AppImage `{}`: {error}",
                staged_artifact.display()
            )
        })?;
        let artifact_bytes = metadata.len();
        run_signing_hooks(
            plan,
            SigningPhase::PostSign,
            &source_stage,
            &output_stage,
            &environment,
            &mut signing,
        )?;
        run_signing_hooks(
            plan,
            SigningPhase::Verify,
            &source_stage,
            &output_stage,
            &environment,
            &mut signing,
        )?;
        let materializer = BundleMaterializerFact {
            tool: "appimagetool".into(),
            tool_identity,
            tool_version,
            executable: tool.to_string_lossy().into_owned(),
            authority: PACKAGE_NATIVE_AUTHORITY.into(),
            mechanism: package_run.mechanism,
            policy: package_run.policy,
        };
        fs::rename(&staged_artifact, output).map_err(|error| {
            format!(
                "could not publish materialized AppImage `{}`: {error}",
                output.display()
            )
        })?;
        Ok(MaterializedFacts {
            sha256: artifact_sha256,
            bytes: artifact_bytes,
            materializer,
            signing: signing.finish()?,
        })
    })();
    cleanup_staging(&source_stage, true);
    cleanup_staging(&output_stage, true);
    result
}

fn materialize_macos(
    plan: &PackageBundlePlan,
    output: &Path,
    parent: &Path,
) -> Result<MaterializedFacts, String> {
    ensure_destination_free(output)?;
    let output_name = output_name(output)?;
    let stage = stage_path(parent, &output_name, "app");
    ensure_destination_free(&stage)?;
    let result = (|| {
        fs::create_dir(&stage)
            .map_err(|error| format!("could not stage macOS app `{}`: {error}", stage.display()))?;
        write_bundle_tree(plan, &stage, true)?;
        let environment = materializer_environment();
        let mut signing = SigningRun::new(plan);
        run_signing_hooks(
            plan,
            SigningPhase::PreSign,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        let entries = plan_materialized_entries(plan, None, true)?;
        let mut facts = materialized_facts(&entries)?;
        run_signing_hooks(
            plan,
            SigningPhase::PostSign,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        run_signing_hooks(
            plan,
            SigningPhase::Verify,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        facts.signing = signing.finish()?;
        fs::rename(&stage, output)
            .map_err(|error| format!("could not publish macOS app `{}`: {error}", output.display()))?;
        Ok(facts)
    })();
    if result.is_err() {
        cleanup_staging(&stage, true);
    }
    result
}
fn materialize_msix(
    plan: &PackageBundlePlan,
    output: &Path,
    parent: &Path,
) -> Result<MaterializedFacts, String> {
    if !cfg!(target_os = "windows") {
        return Err(
            "windows-msix materialization requires a Windows host with the native `makeappx` adapter"
                .into(),
        );
    }
    ensure_destination_free(output)?;
    let output_name = output_name(output)?;
    let stage = stage_path(parent, &output_name, "msix");
    ensure_destination_free(&stage)?;
    let result = (|| {
        fs::create_dir(&stage)
            .map_err(|error| format!("could not stage MSIX source `{}`: {error}", stage.display()))?;
        write_bundle_tree(plan, &stage, false)?;
        let tool = resolve_declared_tool("makeappx.exe")?;
        let tool_identity = jet::SHA256::sha256_file_hex(&tool).map_err(|error| {
            format!(
                "could not hash declared MSIX tool `{}`: {error}",
                tool.display()
            )
        })?;
        let status = Build::native_sandbox_status();
        if !status.available {
            return Err(format!(
                "MSIX materialization requires the checked native process authority; {} ({})",
                status.reason, status.mechanism
            ));
        }
        let environment = materializer_environment();
        let mut signing = SigningRun::new(plan);
        run_signing_hooks(
            plan,
            SigningPhase::PreSign,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        let version_run = run_authorized_tool(
            &tool,
            &["/?".to_string()],
            &stage,
            parent,
            &environment,
        )?;
        if !version_run.output.status.success() {
            return Err(format!(
                "declared MSIX tool `{}` failed its version probe ({}): {}",
                tool.display(),
                version_run.output.status,
                native_tool_output_detail(&version_run.output)
            ));
        }
        let tool_version = native_tool_version(&version_run.output)?;
        let staged_artifact = parent.join(format!(".{output_name}.jet-package-msix-output-{}", std::process::id()));
        ensure_destination_free(&staged_artifact)?;
        let package_run = run_authorized_tool(
            &tool,
            &[
                "pack".into(),
                "/d".into(),
                authorized_tool_path(&stage, &stage.to_string_lossy()),
                "/p".into(),
                authorized_tool_path(&staged_artifact, &staged_artifact.to_string_lossy()),
                "/o".into(),
            ],
            &stage,
            parent,
            &environment,
        )?;
        if !package_run.output.status.success() {
            return Err(format!(
                "declared MSIX tool `{}` failed to materialize `{}` ({}): {}",
                tool.display(),
                output_name,
                package_run.output.status,
                native_tool_output_detail(&package_run.output)
            ));
        }
        let metadata = fs::symlink_metadata(&staged_artifact).map_err(|error| {
            format!(
                "MSIX tool did not produce `{}`: {error}",
                staged_artifact.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "MSIX tool output `{}` is not a regular file",
                staged_artifact.display()
            ));
        }
        run_signing_hooks(
            plan,
            SigningPhase::PostSign,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        run_signing_hooks(
            plan,
            SigningPhase::Verify,
            &stage,
            parent,
            &environment,
            &mut signing,
        )?;
        let artifact_sha256 = jet::SHA256::sha256_file_hex(&staged_artifact).map_err(|error| {
            format!(
                "could not hash materialized MSIX `{}`: {error}",
                staged_artifact.display()
            )
        })?;
        let artifact_bytes = metadata.len();
        let materializer = BundleMaterializerFact {
            tool: "makeappx".into(),
            tool_identity,
            tool_version,
            executable: tool.to_string_lossy().into_owned(),
            authority: PACKAGE_NATIVE_AUTHORITY.into(),
            mechanism: package_run.mechanism,
            policy: package_run.policy,
        };
        fs::rename(&staged_artifact, output).map_err(|error| {
            format!(
                "could not publish materialized MSIX `{}`: {error}",
                output.display()
            )
        })?;
        Ok(MaterializedFacts {
            sha256: artifact_sha256,
            bytes: artifact_bytes,
            materializer,
            signing: signing.finish()?,
        })
    })();
    cleanup_staging(&stage, true);
    let staged_artifact = parent.join(format!(".{output_name}.jet-package-msix-output-{}", std::process::id()));
    cleanup_staging(&staged_artifact, false);
    result
}

fn resolve_declared_tool(name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return Err(format!(
            "package materializer tool `{name}` must be a bare executable name"
        ));
    }
    let paths = std::env::var_os("PATH")
        .ok_or_else(|| "package materializer cannot resolve tools without PATH".to_string())?;
    for directory in std::env::split_paths(&paths) {
        let candidate = directory.join(name);
        let metadata = match fs::symlink_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "could not inspect package materializer tool `{}`: {error}",
                    candidate.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        return fs::canonicalize(&candidate).map_err(|error| {
            format!(
                "could not canonicalize package materializer tool `{}`: {error}",
                candidate.display()
            )
        });
    }
    Err(format!(
        "required package materializer tool `{name}` was not found on PATH"
    ))
}

fn materializer_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("LC_ALL".to_string(), "C".to_string()),
        ("SOURCE_DATE_EPOCH".to_string(), "0".to_string()),
        ("TZ".to_string(), "UTC".to_string()),
    ])
}

fn authorized_tool_path(host_path: &Path, linux_path: &str) -> String {
    if cfg!(target_os = "linux") {
        linux_path.to_string()
    } else {
        host_path.to_string_lossy().into_owned()
    }
}

fn run_authorized_tool(
    executable: &Path,
    args: &[String],
    source_dir: &Path,
    output_dir: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<Build::NativeSandboxOutput, String> {
    Build::run_native_sandboxed(
        executable,
        args,
        source_dir,
        Some(output_dir),
        environment,
        false,
    )
    .map_err(|error| match error {
        NativeSandboxError::Unsupported(detail) => {
            format!("checked native process authority rejected package tool: {detail}")
        }
        NativeSandboxError::Io(detail) => {
            format!("checked native process authority could not run package tool: {detail}")
        }
    })
}
impl SigningRun {
    fn new(plan: &PackageBundlePlan) -> Self {
        Self {
            requested: !plan.signing_hooks.is_empty(),
            verified: false,
            tools: Vec::new(),
        }
    }

    fn finish(self) -> Result<Option<BundleSigningFact>, String> {
        if !self.requested {
            return Ok(None);
        }
        if !self.verified {
            return Err(
                "signing hooks were requested but no successful verify-phase hook ran".into(),
            );
        }
        let tool = match self.tools.as_slice() {
            [] => "unknown".to_string(),
            [single] => single.0.clone(),
            _ => "multiple".to_string(),
        };
        let identity = if self.tools.len() == 1 {
            self.tools[0].1.clone()
        } else {
            let mut identities = self
                .tools
                .iter()
                .map(|(_, identity, _)| identity.as_str())
                .collect::<Vec<_>>();
            identities.sort_unstable();
            jet::SHA256::sha256_hex(identities.join("\0").as_bytes())
        };
        let mut fact = BundleSigningFact::signed();
        fact.tool = Some(tool);
        fact.tool_identity = Some(identity);
        fact.tool_version = Some("unspecified".into());
        Ok(Some(fact))
    }
}

fn run_signing_hooks(
    plan: &PackageBundlePlan,
    phase: SigningPhase,
    source_dir: &Path,
    output_dir: &Path,
    environment: &BTreeMap<String, String>,
    run: &mut SigningRun,
) -> Result<(), String> {
    for hook in plan.signing_hooks.iter().filter(|hook| hook.phase == phase) {
        validate_signing_hook_inputs(plan, hook)?;
        let executable = resolve_signing_tool(&hook.program)?;
        let tool_identity = jet::SHA256::sha256_file_hex(&executable).map_err(|error| {
            format!(
                "could not hash signing hook `{}`: {error}",
                executable.display()
            )
        })?;
        let result = run_authorized_tool(
            &executable,
            &hook.args,
            source_dir,
            output_dir,
            environment,
        )?;
        if !result.output.status.success() {
            return Err(format!(
                "signing hook `{}` ({}) failed in {} phase ({}): {}",
                hook.name,
                executable.display(),
                phase.as_str(),
                result.output.status,
                native_tool_output_detail(&result.output)
            ));
        }
        run.tools
            .push((hook.program.clone(), tool_identity, "unspecified".into()));
        if phase == SigningPhase::Verify {
            run.verified = true;
        }
    }
    Ok(())
}

fn resolve_signing_tool(program: &str) -> Result<PathBuf, String> {
    if program.is_empty() {
        return Err("signing hook program cannot be empty".into());
    }
    if program.contains('/') || program.contains('\\') {
        let path = PathBuf::from(program);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("could not inspect signing hook `{program}`: {error}")
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "signing hook program `{program}` is not a regular file"
            ));
        }
        return fs::canonicalize(&path)
            .map_err(|error| format!("could not canonicalize signing hook `{program}`: {error}"));
    }
    resolve_declared_tool(program)
}

fn validate_signing_hook_inputs(
    plan: &PackageBundlePlan,
    hook: &SigningHook,
) -> Result<(), String> {
    for expected in &hook.inputs {
        let matches = plan
            .inputs
            .iter()
            .any(|input| input.path == expected.path && input.digest == expected.digest)
            || plan.files.iter().any(|file| {
                file.path == expected.path && file.digest() == expected.digest
            });
        if !matches {
            return Err(format!(
                "signing hook `{}` names an input with an unverified digest: `{}`",
                hook.name, expected.path
            ));
        }
    }
    Ok(())
}


fn native_tool_version(output: &std::process::Output) -> Result<String, String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version = stdout
        .lines()
        .chain(stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .ok_or_else(|| "package materializer tool returned no version text".to_string())?;
    if version.chars().any(char::is_control) {
        return Err("package materializer tool version contains a control character".into());
    }
    Ok(version.to_string())
}

fn native_tool_output_detail(output: &std::process::Output) -> String {
    fn bounded(bytes: &[u8]) -> String {
        const MAX_DETAIL_BYTES: usize = 4096;
        let bytes = &bytes[..bytes.len().min(MAX_DETAIL_BYTES)];
        String::from_utf8_lossy(bytes).trim().to_string()
    }
    let stdout = bounded(&output.stdout);
    let stderr = bounded(&output.stderr);
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => "no tool output".to_string(),
        (false, true) => format!("stdout: {stdout}"),
        (true, false) => format!("stderr: {stderr}"),
        (false, false) => format!("stdout: {stdout}; stderr: {stderr}"),
    }
}

fn builtin_materializer_fact() -> Result<BundleMaterializerFact, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not resolve jet package executable identity: {error}"))?;
    let metadata = fs::symlink_metadata(&executable).map_err(|error| {
        format!(
            "could not inspect jet package executable `{}`: {error}",
            executable.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "jet package executable `{}` is not a regular file",
            executable.display()
        ));
    }
    let executable = fs::canonicalize(&executable).map_err(|error| {
        format!(
            "could not canonicalize jet package executable `{}`: {error}",
            executable.display()
        )
    })?;
    let tool_identity = jet::SHA256::sha256_file_hex(&executable).map_err(|error| {
        format!(
            "could not hash jet package executable `{}`: {error}",
            executable.display()
        )
    })?;
    Ok(BundleMaterializerFact {
        tool: "jet-package-materializer".into(),
        tool_identity,
        tool_version: env!("CARGO_PKG_VERSION").into(),
        executable: executable.to_string_lossy().into_owned(),
        authority: "in-process".into(),
        mechanism: "filesystem-staging".into(),
        policy: "checked-plan-only".into(),
    })
}

fn output_name(output: &Path) -> Result<String, String> {
    output
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("package output `{}` has no UTF-8 file name", output.display()))
}

fn ensure_destination_free(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(format!("package output `{}` already exists", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not inspect package output `{}`: {error}", path.display())),
    }
}

fn stage_path(parent: &Path, name: &str, suffix: &str) -> PathBuf {
    parent.join(format!(".{name}.jet-package-{suffix}-{}", std::process::id()))
}

fn cleanup_staging(path: &Path, directory: bool) {
    if directory {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn write_bundle_tree(
    plan: &PackageBundlePlan,
    stage: &Path,
    strip_macos_root: bool,
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for file in &plan.files {
        let relative = materialized_file_path(plan, file, strip_macos_root)?;
        let key = relative.to_string_lossy().replace('\\', "/");
        if !seen.insert(key.clone()) {
            return Err(format!("package plan emits duplicate materialized path `{key}`"));
        }
        let destination = stage.join(&relative);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(format!("package staging path `{}` already exists", destination.display()));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create package path `{}`: {error}", parent.display()))?;
        }
        fs::write(&destination, &file.bytes)
            .map_err(|error| format!("could not write package file `{}`: {error}", destination.display()))?;
        set_mode(&destination, file.mode)?;
    }
    Ok(())
}

fn materialized_file_path(
    plan: &PackageBundlePlan,
    file: &BundleFile,
    strip_macos_root: bool,
) -> Result<PathBuf, String> {
    let raw = Path::new(&file.path);
    let relative = if strip_macos_root {
        let root_name = format!("{}.app", plan.metadata.identifier);
        raw.strip_prefix(Path::new(&root_name)).map_err(|_| {
            format!(
                "macOS package path `{}` is outside `{root_name}`",
                file.path
            )
        })?
    } else {
        raw
    };
    let mut safe = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => safe.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("package path `{}` escapes its staging root", file.path))
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(format!("package path `{}` is empty", file.path));
    }
    Ok(safe)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| format!("could not set package mode on `{}`: {error}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
    Ok(())
}

struct MaterializedEntry<'a> {
    path: String,
    kind: &'static str,
    mode: u32,
    bytes: &'a [u8],
}

fn plan_materialized_entries<'a>(
    plan: &'a PackageBundlePlan,
    prefix: Option<&str>,
    strip_macos_root: bool,
) -> Result<Vec<MaterializedEntry<'a>>, String> {
    plan.files
        .iter()
        .map(|file| {
            let relative = materialized_file_path(plan, file, strip_macos_root)?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            let path = prefix
                .map(|prefix| format!("{prefix}/{relative}"))
                .unwrap_or(relative);
            Ok(MaterializedEntry {
                path,
                kind: file.kind.as_str(),
                mode: file.mode,
                bytes: &file.bytes,
            })
        })
        .collect()
}

fn materialized_facts(entries: &[MaterializedEntry<'_>]) -> Result<MaterializedFacts, String> {
    let mut ordered = entries.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.path.cmp(&right.path));
    let mut digest_input = Vec::new();
    digest_input.extend_from_slice(b"jet-package-materialized-v1\0");
    let mut bytes = 0_u64;
    for entry in ordered {
        frame(&mut digest_input, entry.path.as_bytes());
        frame(&mut digest_input, entry.kind.as_bytes());
        digest_input.extend_from_slice(&entry.mode.to_be_bytes());
        frame(&mut digest_input, entry.bytes);
        bytes = bytes
            .checked_add(entry.bytes.len() as u64)
            .ok_or_else(|| "materialized package exceeds the supported byte count".to_string())?;
    }
    Ok(MaterializedFacts {
        sha256: jet::SHA256::sha256_hex(&digest_input),
        bytes,
        materializer: builtin_materializer_fact()?,
        signing: None,
    })
}


fn frame(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}

fn record_package_receipt(
    args: &[String],
    input_paths: &[PathBuf],
    artifact: &PackageBundleArtifact,
) -> Result<jet::ReceiptStore::Receipt, String> {
    let cwd = std::env::current_dir().map_err(|error| format!("could not read current directory: {error}"))?;
    let root = jet::ReceiptStore::receipt_root_for("package", args, &cwd);
    let store = jet::ReceiptStore::ReceiptStore::new(root);
    let section = jet::ReceiptStore::ReceiptSection::from_json(
        "package",
        "PackageBundleReceipt",
        artifact.receipt.to_json(),
    )
    .map_err(|error| format!("could not encode package receipt section: {error}"))?;
    let stdout = artifact.receipt.json().into_bytes();
    store.record_with_sections("package", args, input_paths, 0, &stdout, &[], &[section])
}

fn render_package_report(
    artifact: &PackageBundleArtifact,
    plan: &PackageBundlePlan,
    output: &Path,
    receipt_digest: &str,
    explain: bool,
    mode: crate::OutputMode,
) {
    let execution = if artifact.receipt.materializer.is_some() {
        "materialized"
    } else {
        "planned"
    };
    let materialization_status = if artifact.receipt.materializer.is_some() {
        "materialized"
    } else {
        "planned"
    };
    if mode.json {
        let artifact_value = status_value(&artifact.to_json());
        let receipt_value = status_value(&artifact.receipt.to_json());
        let materialization = StatusValue::object(
            StatusFields::new()
                .with("status", materialization_status)
                .with(
                    "artifact_sha256",
                    if artifact.receipt.materializer.is_some() {
                        StatusValue::from(artifact.digest())
                    } else {
                        StatusValue::Null
                    },
                )
                .with(
                    "bytes",
                    if artifact.receipt.materializer.is_some() {
                        StatusValue::from(artifact.bytes())
                    } else {
                        StatusValue::Null
                    },
                )
                .with(
                    "materializer",
                    artifact
                        .receipt
                        .materializer
                        .as_ref()
                        .map(game_materializer_json)
                        .map(|value| status_value(&value))
                        .unwrap_or(StatusValue::Null),
                ),
        );
        let mut fields = StatusFields::new()
            .with("artifact", artifact_value)
            .with("artifact_status", artifact.receipt.artifact_status().as_str())
            .with("execution", execution)
            .with("materialization", materialization)
            .with("output", output.to_string_lossy().into_owned())
            .with("plan_identity", plan.identity.as_str())
            .with("receipt", receipt_value)
            .with("receipt_digest", receipt_digest);
        if explain {
            let explanation = StatusValue::object(
                StatusFields::new()
                    .with("authority", "checked package inputs")
                    .with("cache", "shared bundle plan identity")
                    .with("execution", execution)
                    .with("files", plan.files.len())
                    .with("inputs", plan.inputs.len())
                    .with("materialization_status", materialization_status)
                    .with("planned", execution == "planned")
                    .with("plan_identity", plan.identity.as_str()),
            );
            fields = fields.with("explain", explanation);
        }
        println!(
            "{}",
            StatusEnvelope::new("package", true)
                .with_fields(fields)
                .json()
        );
        return;
    }
    println!("package plan: {}", plan.identity);
    println!("target: {}", plan.target.as_str());
    println!("artifact: {}", artifact.artifact_name);
    println!("output: {}", output.display());
    println!("execution: {execution}");
    println!("materialization: {materialization_status}");
    println!("files: {}", artifact.files.len());
    if explain {
        println!("explain: {} inputs, {} files, checked plan", plan.inputs.len(), plan.files.len());
    }
    println!("receipt: {}", receipt_digest);
}

fn package_help() -> String {
    "jet package --kind <desktop|game> --target <linux-appimage|macos-app|windows-msix> [--executable <path>] [<source.jet>] [--output <path>] [--profile <dev|release|name>] [--phase <build,cook,stage,package,export,deploy,run>] [--renderer <headless|raylib>] [--backend <aot>] [--cook-mode <fast|reproducible|scripts-only>] [--export-preset <default|store|headless>] [--deploy-to <path>] [--crash-reporter <off|on|opt-in>] [--crash-consent <not-requested|granted|denied>] [--dry-run] [--explain] [--resume|--cancel] [--clean|--scripts-only] [--run-now] [--package <id>] [--name <name>] [--version <version>] [--icon <path>] [--icon-format <png|icns|ico|svg>] [--icon-size <size>] [--publisher <name>] [--description <text>] [--update-channel <channel> --update-url <url>]".into()
}

fn canonical_json_text(value: CanonicalJson) -> String {
    String::from_utf8(value.bytes())
        .expect("canonical package report JSON is UTF-8")
        .trim_end_matches('\n')
        .to_string()
}

fn json_text(value: &str) -> String {
    canonical_json_text(CanonicalJson::String(value.into()))
}

fn status_value(value: &CanonicalJson) -> StatusValue {
    match value {
        CanonicalJson::Null => StatusValue::Null,
        CanonicalJson::Bool(value) => StatusValue::Bool(*value),
        CanonicalJson::Integer(value) => StatusValue::Integer(
            value
                .parse()
                .expect("canonical package integer must fit status integer"),
        ),
        CanonicalJson::String(value) => StatusValue::String(value.clone()),
        CanonicalJson::Array(values) => StatusValue::array(values.iter().map(status_value)),
        CanonicalJson::Object(values) => {
            let mut fields = StatusFields::new();
            for (name, value) in values {
                fields = fields.with(name.as_str(), status_value(value));
            }
            StatusValue::object(fields)
        }
    }
}

#[derive(Clone, Debug)]
struct GameHeader {
    target: String,
    profile: String,
    backend: String,
    renderer: String,
    cook_mode: String,
    export_preset: String,
    source_revision: String,
    build_identity: String,
    authority_identity: String,
    effect_identity: String,
    development_stripping: bool,
    crash_reporter: String,
    crash_consent: String,
    crash_runtime: GameCrashReporterSpec,
}
fn requested_game_crash_runtime(
    reporter: &str,
    consent: &str,
) -> GameCrashReporterSpec {
    if reporter == "off" || consent != "granted" {
        GameCrashReporterSpec::not_installed()
    } else {
        GameCrashReporterSpec::planned()
    }
}


#[derive(Clone, Debug)]
struct GameDiagnostic {
    code: String,
    what: String,
    fix: String,
}

#[derive(Clone, Debug)]
struct GamePhaseReceipt {
    phase: String,
    status: String,
    header: GameHeader,
    cache_hit: bool,
    invalidation: String,
    output: Option<String>,
    sha256: Option<String>,
    bytes: Option<u64>,
    duration_ms: u64,
    diagnostic: Option<GameDiagnostic>,
}

struct GameRun {
    report: CanonicalJson,
    phases: Vec<GamePhaseReceipt>,
    input_paths: Vec<PathBuf>,
    root: PathBuf,
    output: Option<PathBuf>,
    success: bool,
}

struct GameTreeFile {
    relative: String,
    mode: u32,
    bytes: Vec<u8>,
}

struct CookedGameAssets {
    assets: Vec<GameBundleAsset>,
    digest: String,
    cache_hit: bool,
    invalidation: String,
}

fn run_game_package(args: &[String], options: PackageOptions, mode: crate::OutputMode) -> i32 {
    let planned = options.dry_run || options.explain;
    let result = execute_game_package(&options);
    let run = match result {
        Ok(run) => run,
        Err(error) => {
            if mode.json {
                println!(
                    "{}",
                    StatusEnvelope::new("package", false)
                        .with_field("kind", "game")
                        .with_field("error", error.as_str())
                        .json()
                );
            } else {
                eprintln!("jet package: {error}");
            }
            return ExitCodes::USER_ERROR;
        }
    };
    let report_path = run.root.join(".jet").join("game-package.json");
    if !planned {
        if let Err(error) = write_game_receipt(&report_path, &run.report) {
            if mode.json {
                    println!(
                        "{}",
                        StatusEnvelope::new("package", false)
                            .with_field("kind", "game")
                            .with_field("error", error.as_str())
                            .json()
                    );
            } else {
                eprintln!("jet package: {error}");
            }
            return ExitCodes::USER_ERROR;
        }
    }
    let receipt_digest = if planned {
        String::new()
    } else {
        match record_game_receipt(args, &run.input_paths, &run.report, &run.phases) {
            Ok(receipt) => receipt.digest,
            Err(error) => {
                if mode.json {
                    println!(
                        "{}",
                        StatusEnvelope::new("package", false)
                            .with_field("kind", "game")
                            .with_field("error", error.as_str())
                            .json()
                    );
                } else {
                    eprintln!("jet package: {error}");
                }
                return ExitCodes::USER_ERROR;
            }
        }
    };
    render_game_report(&run, &report_path, &receipt_digest, mode);
    if run.success {
        ExitCodes::OK
    } else {
        ExitCodes::USER_ERROR
    }
}

fn execute_game_package(options: &PackageOptions) -> Result<GameRun, String> {
    let planned = options.dry_run || options.explain;
    let target = options
        .target
        .ok_or_else(|| "`jet package --kind game` needs an explicit `--target`".to_string())?;
    let profile = options.profile.clone().unwrap_or_else(|| "release".into());
    validate_game_profile_name(&profile)?;
    let backend = options.backend.clone().unwrap_or_else(|| "aot".into());
    if backend != "aot" {
        return Err(format!(
            "unsupported game backend `{backend}`; the only artifact-producing backend is `aot`"
        ));
    }
    let renderer = options.renderer.clone().unwrap_or_else(|| "headless".into());
    if !matches!(renderer.as_str(), "headless" | "raylib") {
        return Err(format!("unknown game renderer `{renderer}`; use `headless` or `raylib`"));
    }
    let export_preset = options
        .export_preset
        .clone()
        .unwrap_or_else(|| "default".into());
    if !matches!(export_preset.as_str(), "default" | "store" | "headless") {
        return Err(format!(
            "unknown game export preset `{export_preset}`; use `default`, `store`, or `headless`"
        ));
    }
    if export_preset == "headless" && renderer != "headless" {
        return Err("the `headless` export preset requires `--renderer headless`".into());
    }
    let requested_cook_mode = options.cook_mode.clone();
    let crash_reporter = options
        .crash_reporter
        .clone()
        .unwrap_or_else(|| "off".into());
    if !matches!(crash_reporter.as_str(), "off" | "on" | "opt-in") {
        return Err(format!(
            "unknown crash reporter policy `{crash_reporter}`; use `off`, `on`, or `opt-in`"
        ));
    }
    let crash_consent = options
        .crash_consent
        .clone()
        .unwrap_or_else(|| "not-requested".into());
    if !matches!(crash_consent.as_str(), "not-requested" | "granted" | "denied") {
        return Err(format!(
            "unknown crash reporter consent `{crash_consent}`; use `not-requested`, `granted`, or `denied`"
        ));
    }
    if crash_reporter == "on" && crash_consent != "granted" {
        return Err("crash reporter `on` requires `--crash-consent granted`".into());
    }
    if crash_reporter == "off" && crash_consent == "granted" {
        return Err("crash reporter consent cannot be granted when the reporter is off".into());
    }
    let phases = game_phase_names(options.phase.as_deref())?;
    let (root, source_entry) = resolve_game_root(options)?;
    let tree = collect_game_tree(&root)?;
    let facts = jet::Package::PackageFacts::load_checked(&root)
        .map_err(|error| format!("could not load package authority in `{}`: {error}", root.display()))?;
    let source_revision = game_source_revision(&tree, facts.as_ref());
    validate_declared_game_profile(&profile, facts.as_ref())?;
    let declared_profile = facts
        .as_ref()
        .and_then(|facts| facts.build_profiles.iter().find(|candidate| candidate.name == profile));
    let cook_mode = requested_cook_mode.unwrap_or_else(|| {
        declared_profile
            .and_then(|candidate| candidate.game_cook_mode.clone())
            .unwrap_or_else(|| {
                if options.scripts_only {
                    "scripts-only".into()
                } else if profile_is_development(&profile) {
                    "fast".into()
                } else {
                    "reproducible".into()
                }
            })
    });
    if !matches!(cook_mode.as_str(), "fast" | "reproducible" | "scripts-only") {
        return Err(format!(
            "unknown game cook mode `{cook_mode}`; use `fast`, `reproducible`, or `scripts-only`"
        ));
    }
    if options.scripts_only && cook_mode != "scripts-only" {
        return Err("`--scripts-only` conflicts with an explicit non-script cook mode".into());
    }
    let development_stripping = declared_profile
        .and_then(|candidate| candidate.game_development_stripping)
        .unwrap_or_else(|| !profile_is_development(&profile));
    let authority_result = game_authority_identity(facts.as_ref(), &renderer);
    let authority_error = authority_result.as_ref().err().cloned();
    let authority_identity = authority_result.unwrap_or_else(|_| "rejected".into());
    let effect_identity = game_effect_identity(&renderer, &backend);
    let mut header = GameHeader {
        target: target.as_str().into(),
        profile: profile.clone(),
        backend: backend.clone(),
        renderer: renderer.clone(),
        cook_mode: cook_mode.clone(),
        export_preset: export_preset.clone(),
        source_revision: source_revision.clone(),
        build_identity: "planned".into(),
        authority_identity,
        effect_identity,
        development_stripping,
        crash_reporter: crash_reporter.clone(),
        crash_consent: crash_consent.clone(),
        crash_runtime: requested_game_crash_runtime(&crash_reporter, &crash_consent),
    };
    if let Some(error) = authority_error.as_deref() {
        let authority_started = Instant::now();
        let diagnostic = game_authority_diagnostic(error);
        let phase = game_phase(
            "authority",
            "error",
            &header,
            authority_started,
            false,
            "renderer effect is not authorized".into(),
            None,
            None,
            None,
            Some(diagnostic),
        );
        let authority_phases = vec!["authority".to_string()];
        let report = game_report_json(
            &header,
            &authority_phases,
            std::slice::from_ref(&phase),
            None,
            None,
            None,
            None,
            None,
            options.explain,
            planned,
        );
        return Ok(GameRun {
            report,
            phases: vec![phase],
            input_paths: tree.iter().map(|file| root.join(&file.relative)).collect(),
            root,
            output: None,
            success: false,
        });
    }
    if let Err(error) = validate_game_renderer_selection(&renderer, &tree) {
        return Err(error);
    }
    if options.cancel {
        let cancel_started = Instant::now();
        let (cancelled, status, invalidation) = if planned {
            (
                None,
                "planned",
                "dry-run cancellation planned".to_string(),
            )
        } else {
            let (cancelled, invalidation) = cancel_game_stages(&root)?;
            (cancelled, "cancelled", invalidation)
        };
        let phase = game_phase(
            "cancel",
            status,
            &header,
            cancel_started,
            false,
            invalidation,
            cancelled.map(|path| path.display().to_string()),
            None,
            None,
            None,
        );
        let cancel_phases = vec!["cancel".to_string()];
        let report = game_report_json(
            &header,
            &cancel_phases,
            std::slice::from_ref(&phase),
            None,
            None,
            None,
            None,
            None,
            options.explain,
            planned,
        );
        return Ok(GameRun {
            report,
            phases: vec![phase],
            input_paths: tree.iter().map(|file| root.join(&file.relative)).collect(),
            root,
            output: None,
            success: true,
        });
    }
    let mut input_paths = tree
        .iter()
        .map(|file| root.join(&file.relative))
        .collect::<Vec<_>>();
    let mut phases_receipt = Vec::new();
    let build_started = Instant::now();
    let (executable_path, executable_bytes, build_cache_hit, build_invalidation) =
        if let Some(path) = options.executable.as_ref() {
            ensure_regular_game_file(path, "executable")?;
            let bytes = fs::read(path)
                .map_err(|error| format!("could not read executable `{}`: {error}", path.display()))?;
            (
                path.clone(),
                bytes,
                true,
                "explicit executable input".into(),
            )
        } else if let Some(entry) = source_entry.as_ref() {
            if planned {
                (
                    root.join("build").join(crate::CmdCompile::stem(&entry.to_string_lossy())),
                    Vec::new(),
                    false,
                    "dry-run build planned from source".into(),
                )
            } else {
                let path = build_game_entry(&root, entry, &profile, target)?;
                let bytes = fs::read(&path)
                    .map_err(|error| format!("could not read built game executable `{}`: {error}", path.display()))?;
                (path, bytes, false, "source revision changed or build cache missed".into())
            }
        } else if planned {
            (
                root.join("build").join("game"),
                Vec::new(),
                false,
                "dry-run build planned without a source entry".into(),
            )
        } else {
            return Err("game packaging needs `--executable <path>` or a project source entry".into());
        };
    if !executable_bytes.is_empty()
        && header.crash_reporter != "off"
        && header.crash_consent == "granted"
    {
        if !executable_bytes
            .windows(GAME_CRASH_REPORTER_ABI.len())
            .any(|window| window == GAME_CRASH_REPORTER_ABI.as_bytes())
        {
            return Err(
                "active game crash reporter requires a binary built with Jet's game reporter bootstrap; \
                 build from a game source entry or use `--crash-reporter off`"
                    .into(),
            );
        }
        header.crash_runtime = GameCrashReporterSpec::installed();
    }

    header.build_identity = if executable_bytes.is_empty() {
        "planned".into()
    } else {
        let executable_digest = jet::SHA256::sha256_hex(&executable_bytes);
        format!(
            "sha256:{}",
            jet::SHA256::sha256_hex(
                format!(
                    "jet-game-build-v1\0{}\0{}\0{}\0{}\0{}",
                    source_revision,
                    target.as_str(),
                    backend,
                    renderer,
                    executable_digest
                )
                .as_bytes(),
            )
        )
    };
    phases_receipt.push(game_phase(
        "build",
        "planned",
        &header,
        build_started,
        build_cache_hit,
        build_invalidation,
        Some(executable_path.display().to_string()),
        if executable_bytes.is_empty() {
            None
        } else {
            Some(jet::SHA256::sha256_hex(&executable_bytes))
        },
        Some(executable_bytes.len() as u64),
        None,
    ));
    if !planned {
        if let Some(phase) = phases_receipt.last_mut() {
            phase.status = "ok".into();
        }
    }
    input_paths.push(executable_path.clone());
    dedup_paths(&mut input_paths);
    let mut cooked = None;
    if phases.iter().any(|phase| phase == "cook") {
        let cook_started = Instant::now();
        let result = cook_game_assets(
            &root,
            &tree,
            &header,
            options.force_clean,
            options.resume,
            planned,
        )?;
        phases_receipt.push(game_phase(
            "cook",
            if planned { "planned" } else { "ok" },
            &header,
            cook_started,
            result.cache_hit,
            result.invalidation.clone(),
            Some(if header.cook_mode == "fast" {
                "on-the-fly cook (persistent cache bypassed)".into()
            } else {
                root.join(".jet").join("game-cache").display().to_string()
            }),
            Some(result.digest.clone()),
            Some(result.assets.iter().map(|asset| asset.bytes.len() as u64).sum()),
            None,
        ));
        cooked = Some(result);
    }
    let mut plan = None;
    let mut stage_root = None;
    let mut output = None;
    let mut artifact = None;
    if phases.iter().any(|phase| phase == "stage") {
        let cooked = cooked
            .as_ref()
            .ok_or_else(|| "game stage requires the cook phase".to_string())?;
        let (game_plan, package_inputs) = make_game_plan(
            options,
            target,
            &header,
            &executable_path,
            &executable_bytes,
            cooked,
        )?;
        input_paths.extend(package_inputs);
        dedup_paths(&mut input_paths);
        let stage_started = Instant::now();
        let stage_path = stage_game_plan(
            &root,
            &game_plan,
            options.force_clean,
            options.resume,
            planned,
        )?;
        stage_root = Some(stage_path.clone());
        phases_receipt.push(game_phase(
            "stage",
            "planned",
            &header,
            stage_started,
            false,
            if options.force_clean {
                "forced-clean requested".into()
            } else if options.resume {
                "resumed canonical plan staging".into()
            } else {
                "new canonical plan staging".into()
            },
            Some(stage_path.display().to_string()),
            Some(game_plan.identity.clone()),
            Some(game_plan.files.iter().map(|file| file.bytes.len() as u64).sum()),
            None,
        ));
        if !planned {
            if let Some(phase) = phases_receipt.last_mut() {
                phase.status = "ok".into();
            }
        }
        output = Some(resolve_output_path(options.output.clone(), &game_plan.artifact_name)?);
        plan = Some(game_plan);
    }
    if phases.iter().any(|phase| phase == "package") {
        let package_started = Instant::now();
        let game_plan = plan
            .as_ref()
            .ok_or_else(|| "game package requires the stage phase".to_string())?;
        let package_output = output
            .as_ref()
            .ok_or_else(|| "game package has no output path".to_string())?;
        if planned {
            phases_receipt.push(game_phase(
                "package",
                "planned",
                &header,
                package_started,
                false,
                "dry-run materialization planned".into(),
                Some(package_output.display().to_string()),
                Some(game_plan.identity.clone()),
                Some(game_plan.files.iter().map(|file| file.bytes.len() as u64).sum()),
                None,
            ));
        } else {
            let Some(stage_path) = stage_root.as_deref() else {
                return Err("game package requires the canonical stage output".into());
            };
            verify_game_stage_files(stage_path, game_plan)?;
            let facts = materialize_package(game_plan, package_output)?;
            let mut packaged = game_plan.materialized_artifact(facts.sha256.clone(), facts.bytes);
            packaged.receipt.record_materializer(facts.materializer);
            if let Some(signing) = facts.signing {
                packaged.receipt.record_signing(signing);
            }
            phases_receipt.push(game_phase(
                "package",
                "ok",
                &header,
                package_started,
                false,
                "canonical staged plan materialized".into(),
                Some(package_output.display().to_string()),
                Some(facts.sha256),
                Some(facts.bytes),
                None,
            ));
            artifact = Some(packaged);
        }
    }
    if phases.iter().any(|phase| phase == "export") {
        let export_started = Instant::now();
        if output.is_none() {
            return Err("game export requires the package phase".to_string());
        }
        let game_plan = plan
            .as_ref()
            .ok_or_else(|| "game export requires the stage plan".to_string())?;
        let export_destination = root
            .join(".jet")
            .join("game-exports")
            .join(&export_preset)
            .join(&game_plan.artifact_name);
        let mut export_status = if planned { "planned" } else { "ok" };
        let mut export_invalidation = format!("export preset `{export_preset}`");
        let mut export_output = Some(export_destination);
        let mut export_digest = artifact.as_ref().map(|value| value.digest());
        let mut export_bytes = artifact.as_ref().map(|value| value.bytes());
        let mut export_diagnostic = None;
        if planned {
            export_invalidation = "dry-run native export planned".into();
        } else {
            match export_game_output(&root, game_plan, target, &export_preset, options.force_clean) {
                Ok((path, facts)) => {
                    export_output = Some(path.clone());
                    export_digest = Some(facts.sha256.clone());
                    export_bytes = Some(facts.bytes);
                    let mut exported = game_plan.materialized_artifact(facts.sha256.clone(), facts.bytes);
                    exported.receipt.record_materializer(facts.materializer);
                    if let Some(signing) = facts.signing {
                        exported.receipt.record_signing(signing);
                    }
                    artifact = Some(exported);
                    output = Some(path.clone());
                    export_invalidation = "native export adapter materialized output".into();
                }
                Err(error) => {
                    export_status = "error";
                    export_output = None;
                    export_digest = None;
                    export_bytes = None;
                    export_invalidation = "export adapter refused output".into();
                    export_diagnostic = Some(GameDiagnostic {
                        code: "E2109".into(),
                        what: error,
                        fix: "choose a supported preset/target and pass `--clean` for an owned export".into(),
                    });
                }
            }
        }
        phases_receipt.push(game_phase(
            "export",
            export_status,
            &header,
            export_started,
            false,
            export_invalidation,
            export_output.map(|path| path.display().to_string()),
            export_digest,
            export_bytes,
            export_diagnostic,
        ));
    }
    if phases.iter().any(|phase| phase == "deploy") {
        let deploy_started = Instant::now();
        let destination = options
            .deploy_to
            .as_ref()
            .map(|path| resolve_game_destination(&root, path));
        let mut status = "planned";
        let mut invalidation = "deployment is explicit and destination-scoped".to_string();
        let mut diagnostic = None;
        let mut deployed = destination.clone();
        if destination.is_none() {
            status = "skipped";
            invalidation = "deploy destination was not provided".into();
            diagnostic = Some(GameDiagnostic {
                code: "E2104".into(),
                what: "deploy phase has no destination".into(),
                fix: "pass `--deploy-to <path>` or omit the deploy phase".into(),
            });
            deployed = None;
        } else if planned {
            invalidation = "dry-run deployment planned".into();
        } else if let Some(package_output) = output.as_ref() {
            match deploy_game_output(package_output, destination.as_ref().expect("destination checked"), options.force_clean) {
                Ok(path) => {
                    deployed = Some(path);
                    invalidation = "package artifact deployed atomically".into();
                }
                Err(error) => {
                    status = "skipped";
                    invalidation = "deployment refused".into();
                    diagnostic = Some(GameDiagnostic {
                        code: "E2106".into(),
                        what: error,
                        fix: "choose an empty destination or pass `--clean` for an owned output".into(),
                    });
                    deployed = None;
                }
            }
        } else {
            status = "skipped";
            invalidation = "deploy requires a materialized package output".into();
            diagnostic = Some(GameDiagnostic {
                code: "E2107".into(),
                what: "deploy phase has no package artifact".into(),
                fix: "include the package phase before deploy".into(),
            });
            deployed = None;
        }
        phases_receipt.push(game_phase(
            "deploy",
            status,
            &header,
            deploy_started,
            false,
            invalidation,
            deployed.map(|path| path.display().to_string()),
            artifact.as_ref().map(|value| value.digest()),
            artifact.as_ref().map(|value| value.bytes()),
            diagnostic,
        ));
    }
    if phases.iter().any(|phase| phase == "run") {
        let run_started = Instant::now();
        let mut status = "skipped";
        let mut invalidation = "run requires explicit `--run-now`".to_string();
        let mut diagnostic = None;
        if options.run_now {
            if planned {
                invalidation = "dry-run launch planned".into();
            } else if let Some(game_output) = output.as_ref() {
                match launch_game_output(game_output, target, &renderer) {
                    Ok(pid) => {
                        status = "ok";
                        invalidation = format!("launched packaged game process pid {pid}");
                    }
                    Err(error) => {
                        invalidation = "run launch refused".into();
                        diagnostic = Some(GameDiagnostic {
                            code: "E2108".into(),
                            what: error,
                            fix: "package for the host target or omit `--run-now`".into(),
                        });
                    }
                }
            } else {
                invalidation = "run requires a materialized package output".into();
                diagnostic = Some(GameDiagnostic {
                    code: "E2107".into(),
                    what: "run phase has no package artifact".into(),
                    fix: "include the package phase before run".into(),
                });
            }
        } else {
            diagnostic = Some(GameDiagnostic {
                code: "E2105".into(),
                what: "run phase was requested without `--run-now`".into(),
                fix: "pass `--run-now` to opt into process launch".into(),
            });
        }
        phases_receipt.push(game_phase(
            "run",
            status,
            &header,
            run_started,
            false,
            invalidation,
            output.as_ref().map(|path| path.display().to_string()),
            artifact.as_ref().map(|value| value.digest()),
            artifact.as_ref().map(|value| value.bytes()),
            diagnostic,
        ));
    }
    let artifact_sha256 = artifact.as_ref().map(|value| value.digest());
    let plan_identity = plan.as_ref().map(|value| value.identity.clone());
    let report = game_report_json(
        &header,
        &phases,
        &phases_receipt,
        plan_identity.as_deref(),
        plan.as_ref(),
        artifact_sha256.as_deref(),
        output.as_deref(),
        artifact.as_ref(),
        options.explain,
        planned,
    );
    let success = !phases_receipt
        .iter()
        .any(|phase| phase.status == "error" || phase.diagnostic.is_some());
    Ok(GameRun {
        report,
        phases: phases_receipt,
        input_paths,
        root,
        output,
        success,
    })
}
fn validate_game_profile_name(profile: &str) -> Result<(), String> {
    if profile.is_empty()
        || profile.chars().any(|character| {
            character.is_control()
                || matches!(character, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err(format!("invalid game profile `{profile}`"));
    }
    Ok(())
}

fn profile_is_development(profile: &str) -> bool {
    matches!(profile, "dev" | "debug" | "ci" | "development")
}

fn validate_declared_game_profile(
    profile: &str,
    facts: Option<&jet::Package::PackageFacts>,
) -> Result<(), String> {
    if let Some(facts) = facts {
        if !facts.build_profiles.is_empty()
            && !facts.build_profiles.iter().any(|candidate| candidate.name == profile)
        {
            let names = facts
                .build_profiles
                .iter()
                .map(|candidate| candidate.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "game profile `{profile}` is not declared by package.jet; available profiles: {names}"
            ));
        }
    } else if !matches!(profile, "dev" | "debug" | "release" | "hardened" | "ci" | "development") {
        return Err(format!(
            "game profile `{profile}` needs a package.jet build declaration"
        ));
    }
    Ok(())
}

fn game_phase_names(raw: Option<&str>) -> Result<Vec<String>, String> {
    const ORDER: [&str; 7] = ["build", "cook", "stage", "package", "export", "deploy", "run"];
    let raw = raw.unwrap_or("build,cook,stage,package,export");
    let mut requested = BTreeSet::new();
    for phase in raw.split(',').map(str::trim).filter(|phase| !phase.is_empty()) {
        if phase == "all" {
            requested.extend(ORDER);
            continue;
        }
        if !ORDER.contains(&phase) {
            return Err(format!(
                "unknown game phase `{phase}`; use build,cook,stage,package,export,deploy,run"
            ));
        }
        requested.insert(phase);
    }
    if requested.is_empty() {
        return Err("`--phase` needs at least one game phase".into());
    }
    let last = ORDER
        .iter()
        .rposition(|phase| requested.contains(phase))
        .unwrap_or(0);
    Ok(ORDER[..=last].iter().map(|phase| (*phase).into()).collect())
}

fn resolve_game_root(options: &PackageOptions) -> Result<(PathBuf, Option<PathBuf>), String> {
    let base = options.source.clone().unwrap_or(
        std::env::current_dir().map_err(|error| format!("could not read current directory: {error}"))?,
    );
    let metadata = fs::symlink_metadata(&base)
        .map_err(|error| format!("could not inspect game source `{}`: {error}", base.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("game source `{}` must not be a symlink", base.display()));
    }
    if metadata.is_file() {
        let entry = fs::canonicalize(&base)
            .map_err(|error| format!("could not canonicalize game source `{}`: {error}", base.display()))?;
        let parent = entry.parent().unwrap_or_else(|| Path::new("."));
        if entry.file_name().and_then(|name| name.to_str()) == Some("package.jet") {
            let root = jet::Loader::find_manifest_root(parent).unwrap_or_else(|| parent.to_path_buf());
            return Ok((root.clone(), find_game_entry(&root)));
        }
        let root = jet::Loader::find_manifest_root(parent).unwrap_or_else(|| parent.to_path_buf());
        return Ok((root, Some(entry)));
    }
    if !metadata.is_dir() {
        return Err(format!("game source `{}` is not a regular file or directory", base.display()));
    }
    let root = jet::Loader::find_manifest_root(&base).unwrap_or(base);
    let entry = find_game_entry(&root);
    Ok((root, entry))
}

fn find_game_entry(root: &Path) -> Option<PathBuf> {
    [
        root.join("main.jet"),
        root.join("src").join("main.jet"),
        root.join("src").join("Scene.jet"),
        root.join("game.jet"),
    ]
    .into_iter()
    .find(|candidate| {
        fs::symlink_metadata(candidate)
            .ok()
            .is_some_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
    })
}
fn collect_game_tree(root: &Path) -> Result<Vec<GameTreeFile>, String> {
    let mut files = Vec::new();
    collect_game_tree_at(root, Path::new(""), &mut files)?;
    files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(files)
}
fn game_source_revision(
    tree: &[GameTreeFile],
    facts: Option<&jet::Package::PackageFacts>,
) -> String {
    let mut input = Vec::new();
    input.extend_from_slice(b"jet-game-source-v2\0");
    frame(&mut input, env!("CARGO_PKG_VERSION").as_bytes());
    if let Some(facts) = facts {
        frame(&mut input, facts.semantic_digest().as_bytes());
    }
    for file in tree {
        frame(&mut input, file.relative.as_bytes());
        input.extend_from_slice(&file.mode.to_be_bytes());
        frame(&mut input, &file.bytes);
    }
    format!("sha256:{}", jet::SHA256::sha256_hex(&input))
}

fn collect_game_tree_at(
    root: &Path,
    relative: &Path,
    files: &mut Vec<GameTreeFile>,
) -> Result<(), String> {
    const MAX_GAME_FILE_BYTES: u64 = 256 * 1024 * 1024;
    let directory = root.join(relative);
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("could not inspect game path `{}`: {error}", directory.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("game path `{}` must not be a symlink", directory.display()));
    }
    if metadata.is_file() {
        if metadata.len() > MAX_GAME_FILE_BYTES {
            return Err(format!(
                "game input `{}` exceeds the {} byte limit",
                directory.display(),
                MAX_GAME_FILE_BYTES
            ));
        }
        let bytes = fs::read(&directory)
            .map_err(|error| format!("could not read game input `{}`: {error}", directory.display()))?;
        let relative = relative
            .to_str()
            .ok_or_else(|| format!("game input `{}` is not valid UTF-8", directory.display()))?
            .replace('\\', "/");
        let mode = game_file_mode(&metadata);
        files.push(GameTreeFile {
            relative,
            mode,
            bytes,
        });
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!("game path `{}` is not a regular file or directory", directory.display()));
    }
    let mut entries = fs::read_dir(&directory)
        .map_err(|error| format!("could not enumerate game path `{}`: {error}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not enumerate game path `{}`: {error}", directory.display()))?;
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    for entry in entries {
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| format!("game path `{}` is not valid UTF-8", entry.path().display()))?
            .to_string();
        if relative.as_os_str().is_empty()
            && matches!(
                name.as_str(),
                ".git" | ".jet" | "build" | "target" | "node_modules" | "dist"
            )
        {
            continue;
        }
        collect_game_tree_at(root, &relative.join(name), files)?;
    }
    Ok(())
}

fn game_file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode == 0 { 0o644 } else { mode }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}
fn ensure_regular_game_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {label} `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label} `{}` is not a regular file", path.display()));
    }
    Ok(())
}

fn game_build_target(target: BundleTarget) -> Result<&'static str, String> {
    let arch = std::env::consts::ARCH;
    let triple = match (target, arch) {
        (BundleTarget::LinuxAppImage, "x86_64") => "x86_64-unknown-linux-gnu",
        (BundleTarget::LinuxAppImage, "aarch64") => "aarch64-unknown-linux-gnu",
        (BundleTarget::LinuxAppImage, "i686") => "i686-unknown-linux-gnu",
        (BundleTarget::MacOSApp, "x86_64") => "x86_64-apple-darwin",
        (BundleTarget::MacOSApp, "aarch64") => "aarch64-apple-darwin",
        (BundleTarget::WindowsMsix, "x86_64") => "x86_64-pc-windows-gnu",
        (BundleTarget::WindowsMsix, "aarch64") => "aarch64-pc-windows-msvc",
        (BundleTarget::WindowsMsix, "i686") => "i686-pc-windows-gnu",
        (target, arch) => {
            return Err(format!(
                "game target `{}` has no compiler target mapping for host architecture `{arch}`",
                target.as_str()
            ))
        }
    };
    Ok(triple)
}

fn build_game_entry(
    root: &Path,
    entry: &Path,
    profile: &str,
    target: BundleTarget,
) -> Result<PathBuf, String> {
    let target_triple = game_build_target(target)?;
    let jet = std::env::current_exe()
        .map_err(|error| format!("could not resolve jet executable for game build: {error}"))?;
    let profile = match profile {
        "debug" | "development" => "dev",
        "hardened" => "release",
        value => value,
    };
    let output = Command::new(&jet)
        .arg("build")
        .arg(entry)
        .arg("--profile")
        .arg(profile)
        .arg("--target")
        .arg(target_triple)
        .arg("--quiet")
        .current_dir(root)
        .output()
        .map_err(|error| format!("could not start game build: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "game build failed ({}){}",
            output.status,
            command_output_detail(&output.stdout, &output.stderr)
        ));
    }
    let path = root
        .join("build")
        .join(crate::CmdCompile::stem(&entry.to_string_lossy()));
    ensure_regular_game_file(&path, "built game executable")?;
    Ok(path)
}

fn command_output_detail(stdout: &[u8], stderr: &[u8]) -> String {
    const MAX_DETAIL_BYTES: usize = 4096;
    let stdout = String::from_utf8_lossy(&stdout[..stdout.len().min(MAX_DETAIL_BYTES)])
        .trim()
        .to_string();
    let stderr = String::from_utf8_lossy(&stderr[..stderr.len().min(MAX_DETAIL_BYTES)])
        .trim()
        .to_string();
    match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(": stdout: {stdout}"),
        (true, false) => format!(": stderr: {stderr}"),
        (false, false) => format!(": stdout: {stdout}; stderr: {stderr}"),
    }
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    paths.sort();
    paths.dedup();
}
fn game_authority_identity(
    facts: Option<&jet::Package::PackageFacts>,
    renderer: &str,
) -> Result<String, String> {
    let mut authority = if let Some(facts) = facts {
        jet_foundation::Authority::ApplicationAuthority::from_policy(
            facts.authority.holds.allow.as_deref(),
            facts.authority.holds.deny.as_deref(),
            "package.jet authority.holds",
        )
    } else {
        jet_foundation::Authority::ApplicationAuthority::ambient_basics()
    };
    let policy_identity = format!("{authority:?}");
    let required_effect = if renderer == "raylib" { "GPU" } else { "none" };
    if renderer == "raylib" {
        authority.required_effects.insert("GPU".into());
        if let Some(diagnostic) = authority.policy_diagnostic() {
            return Err(format!(
                "{}: renderer `{renderer}` requires effect `GPU`; {}; {}; fix: {}",
                diagnostic.code, diagnostic.what, diagnostic.why, diagnostic.fix
            ));
        }
    }
    Ok(format!(
        "sha256:{}",
        jet::SHA256::sha256_hex(
            format!("jet-game-authority-v1\0{policy_identity}\0{required_effect}").as_bytes()
        )
    ))
}
fn game_authority_diagnostic(error: &str) -> GameDiagnostic {
    let (code, body) = error
        .split_once(':')
        .map_or(("E1803", error), |(code, body)| (code.trim(), body.trim()));
    let (what, fix) = body
        .split_once("fix:")
        .map_or((body.trim(), "declare `GPU` in package.jet authority.holds.allow"), |(what, fix)| {
            (what.trim(), fix.trim())
        });
    GameDiagnostic {
        code: code.into(),
        what: what.into(),
        fix: fix.into(),
    }
}


fn validate_game_renderer_selection(
    renderer: &str,
    tree: &[GameTreeFile],
) -> Result<(), String> {
    if renderer != "raylib" {
        return Ok(());
    }
    let bridge = b"core.game.raylib";
    if tree.iter().any(|file| {
        file.relative.ends_with(".jet")
            && file.bytes.windows(bridge.len()).any(|window| window == bridge)
    }) {
        return Ok(());
    }
    Err(
        "renderer `raylib` was selected, but the source does not use the checked `core.game.raylib` bridge"
            .into(),
    )
}

fn game_effect_identity(renderer: &str, backend: &str) -> String {
    let effect = if renderer == "raylib" { "GPU" } else { "none" };
    format!(
        "sha256:{}",
        jet::SHA256::sha256_hex(format!("jet-game-effect-v1\0{backend}\0{effect}").as_bytes())
    )
}

fn cook_game_assets(
    root: &Path,
    tree: &[GameTreeFile],
    header: &GameHeader,
    force_clean: bool,
    resume: bool,
    dry_run: bool,
) -> Result<CookedGameAssets, String> {
    let mut assets = tree
        .iter()
        .filter_map(|file| file.relative.strip_prefix("assets/").map(|path| (path, file)))
        .filter(|(path, _)| header.cook_mode != "scripts-only" || is_game_script(path))
        .filter(|(path, _)| !header.development_stripping || !is_game_debug_asset(path))
        .map(|(path, file)| {
            validate_game_asset_relative_path(path)?;
            Ok(GameBundleAsset::new(path, file.mode, file.bytes.clone()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    assets.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.digest().cmp(&right.digest()))
    });
    let mut digest_input = Vec::new();
    digest_input.extend_from_slice(b"jet-game-cook-assets-v1\0");
    for asset in &assets {
        frame(&mut digest_input, asset.path.as_bytes());
        digest_input.extend_from_slice(&asset.mode.to_be_bytes());
        frame(&mut digest_input, &asset.bytes);
    }
    let asset_digest = jet::SHA256::sha256_hex(&digest_input);
    let identity = jet::SHA256::sha256_hex(
        format!(
            "jet-game-cook-v1\0{}\0{}\0{}\0{}\0{}",
            header.target, header.profile, header.backend, header.cook_mode, asset_digest
        )
        .as_bytes(),
    );
    let invalidation = if force_clean {
        if header.cook_mode == "scripts-only" {
            "forced-clean requested; scripts-only asset set rebuilt".into()
        } else {
            "forced-clean requested; asset set rebuilt".into()
        }
    } else if header.cook_mode == "scripts-only" {
        "scripts-only invalidation scope".into()
    } else {
        "asset content or cook identity changed".into()
    };
    if dry_run {
        return Ok(CookedGameAssets {
            assets,
            digest: format!("sha256:{asset_digest}"),
            cache_hit: false,
            invalidation: format!("dry-run {} cook planned", header.cook_mode),
        });
    }
    if header.cook_mode == "fast" {
        return Ok(CookedGameAssets {
            assets,
            digest: format!("sha256:{asset_digest}"),
            cache_hit: false,
            invalidation: if force_clean {
                "forced-clean requested; fast on-the-fly cook bypassed persistent cache".into()
            } else {
                "fast on-the-fly cook; persistent cache bypassed".into()
            },
        });
    }
    let cache_root = root.join(".jet").join("game-cache");
    let object_root = cache_root.join("objects");
    let cooked_root = cache_root.join("cooked");
    fs::create_dir_all(&object_root)
        .map_err(|error| format!("could not create game asset cache `{}`: {error}", object_root.display()))?;
    fs::create_dir_all(&cooked_root)
        .map_err(|error| format!("could not create game cook cache `{}`: {error}", cooked_root.display()))?;
    for asset in &assets {
        write_content_addressed_blob(&object_root, asset)?;
    }
    let final_dir = cooked_root.join(&identity);
    let marker = game_cook_marker(&identity, header, &asset_digest);
    if fs::symlink_metadata(&final_dir).is_ok() {
        if force_clean {
            ensure_owned_directory(&final_dir, &marker, "game cook cache")?;
            fs::remove_dir_all(&final_dir).map_err(|error| {
                format!("could not remove game cook cache `{}`: {error}", final_dir.display())
            })?;
        } else {
            ensure_owned_directory(&final_dir, &marker, "game cook cache")?;
            let cached_assets = load_cached_game_assets(&final_dir, &assets)?;
            return Ok(CookedGameAssets {
                assets: cached_assets,
                digest: format!("sha256:{asset_digest}"),
                cache_hit: true,
                invalidation: "content-addressed cooked assets reused".into(),
            });
        }
    }
    let staging = cooked_root.join(format!(".{identity}-staging"));
    if fs::symlink_metadata(&staging).is_ok() {
        if force_clean {
            ensure_owned_directory(&staging, &marker, "game cook staging")?;
            fs::remove_dir_all(&staging).map_err(|error| {
                format!("could not remove game cook staging `{}`: {error}", staging.display())
            })?;
        } else if resume {
            ensure_owned_directory(&staging, &marker, "game cook staging")?;
            fs::remove_dir_all(&staging).map_err(|error| {
                format!("could not resume game cook staging `{}`: {error}", staging.display())
            })?;
        } else {
            return Err(format!(
                "game cook staging `{}` already exists; use `--resume` only after inspecting it or `--clean` for owned staging",
                staging.display()
            ));
        }
    }
    let result: Result<(), String> = (|| {
        fs::create_dir(&staging)
            .map_err(|error| format!("could not create game cook staging `{}`: {error}", staging.display()))?;
        write_atomic_bytes(&staging.join("staging.json"), marker.as_bytes())?;
        for asset in &assets {
            let destination = staging.join("assets").join(&asset.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("could not create cooked asset directory `{}`: {error}", parent.display())
                })?;
            }
            fs::write(&destination, &asset.bytes)
                .map_err(|error| format!("could not write cooked asset `{}`: {error}", destination.display()))?;
            set_mode(&destination, asset.mode)?;
        }
        fs::rename(&staging, &final_dir).map_err(|error| {
            format!(
                "could not atomically publish cooked game assets `{}`: {error}",
                final_dir.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        cleanup_staging(&staging, true);
    }
    result?;
    Ok(CookedGameAssets {
        assets,
        digest: format!("sha256:{asset_digest}"),
        cache_hit: false,
        invalidation,
    })
}
fn load_cached_game_assets(
    cooked_dir: &Path,
    expected: &[GameBundleAsset],
) -> Result<Vec<GameBundleAsset>, String> {
    expected
        .iter()
        .map(|asset| {
            let path = cooked_dir.join("assets").join(&asset.path);
            ensure_regular_game_file(&path, "cooked game asset")?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("could not read cooked game asset `{}`: {error}", path.display()))?;
            if jet::SHA256::sha256_hex(&bytes) != asset.digest() {
                return Err(format!("cooked game asset `{}` has a foreign digest", path.display()));
            }
            Ok(GameBundleAsset::new(&asset.path, asset.mode, bytes))
        })
        .collect()
}


fn validate_game_asset_relative_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.contains('\\') || path.chars().any(char::is_control) {
        return Err(format!("game asset path `{path}` is invalid"));
    }
    if path
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(format!("game asset path `{path}` is invalid"));
    }
    Ok(())
}

fn is_game_script(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "jet" | "lua" | "js" | "ts" | "gd" | "json" | "txt")
        })
}

fn is_game_debug_asset(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "pdb" | "dsym" | "debug" | "dbg" | "map"
            )
        })
}

fn game_cook_marker(identity: &str, header: &GameHeader, asset_digest: &str) -> String {
    format!(
        "jet-game-cook-v1\nidentity={identity}\ntarget={}\nprofile={}\nbackend={}\ncook_mode={}\nassets_sha256={asset_digest}\n",
        header.target, header.profile, header.backend, header.cook_mode
    )
}

fn write_content_addressed_blob(root: &Path, asset: &GameBundleAsset) -> Result<(), String> {
    let digest = asset.digest();
    let destination = root.join(&digest);
    if fs::symlink_metadata(&destination).is_ok() {
        ensure_regular_game_file(&destination, "game asset cache object")?;
        let existing = fs::read(&destination)
            .map_err(|error| format!("could not read game asset cache object `{}`: {error}", destination.display()))?;
        if jet::SHA256::sha256_hex(&existing) != digest {
            return Err(format!(
                "game asset cache object `{}` has a foreign digest",
                destination.display()
            ));
        }
        return Ok(());
    }
    let temporary = root.join(format!(".{digest}-{}", std::process::id()));
    if fs::symlink_metadata(&temporary).is_ok() {
        return Err(format!(
            "game asset cache staging `{}` already exists",
            temporary.display()
        ));
    }
    fs::write(&temporary, &asset.bytes)
        .map_err(|error| format!("could not write game asset cache object `{}`: {error}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, &destination) {
        cleanup_staging(&temporary, false);
        if fs::symlink_metadata(&destination).is_err() {
            return Err(format!(
                "could not publish game asset cache object `{}`: {error}",
                destination.display()
            ));
        }
    }
    Ok(())
}

fn ensure_owned_directory(path: &Path, marker: &str, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {label} `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("{label} `{}` is not an owned directory", path.display()));
    }
    let marker_path = path.join("staging.json");
    let existing = fs::read_to_string(&marker_path)
        .map_err(|error| format!("could not read {label} marker `{}`: {error}", marker_path.display()))?;
    if existing != marker {
        return Err(format!(
            "{label} `{}` has a foreign or stale identity; refusing to reuse it",
            path.display()
        ));
    }
    Ok(())
}

fn write_atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path `{}` has no parent", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create staging parent `{}`: {error}", parent.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("staging");
    let temporary = parent.join(format!(".{file_name}-{}", std::process::id()));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("could not write staging file `{}`: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| {
        cleanup_staging(&temporary, false);
        format!("could not publish staging file `{}`: {error}", path.display())
    })
}
fn make_game_plan(
    options: &PackageOptions,
    target: BundleTarget,
    header: &GameHeader,
    executable_path: &Path,
    executable_bytes: &[u8],
    cooked: &CookedGameAssets,
) -> Result<(PackageBundlePlan, Vec<PathBuf>), String> {
    let executable_name = options
        .name
        .clone()
        .or_else(|| executable_path.file_stem().map(|value| value.to_string_lossy().into_owned()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "game package needs a non-empty executable name".to_string())?;
    let package = options.package.clone().unwrap_or_else(|| executable_name.clone());
    let version = options.version.clone().unwrap_or_else(|| "0.1.0".into());
    let display_name = options.name.clone().unwrap_or_else(|| package.clone());
    let mut metadata = BundleMetadata::new(package.clone(), display_name);
    metadata.publisher = if options.publisher.is_empty() {
        "Jet".into()
    } else {
        options.publisher.clone()
    };
    metadata.description = options.description.clone();
    let executable = BundleExecutable::new(
        executable_name,
        executable_path.to_string_lossy().into_owned(),
        executable_bytes.to_vec(),
    );
    let mut game = GameBundleSpec::new(
        header.profile.clone(),
        header.export_preset.clone(),
        header.source_revision.clone(),
        header.backend.clone(),
        header.renderer.clone(),
        header.cook_mode.clone(),
    );

    game.build_identity = header.build_identity.clone();
    game.development_stripping = header.development_stripping;
    game.crash_reporter = header.crash_reporter.clone();
    game.crash_consent = header.crash_consent.clone();
    game.crash_runtime = header.crash_runtime.clone();
    game.assets = cooked.assets.clone();
    let mut spec = BundleSpec::game(
        target,
        package.clone(),
        version,
        executable,
        metadata,
    );
    spec.game = Some(game);
    let mut input_paths = Vec::new();
    if let Some(icon_path) = options.icon.as_ref() {
        ensure_regular_game_file(icon_path, "icon")?;
        let icon_bytes = fs::read(icon_path)
            .map_err(|error| format!("could not read icon `{}`: {error}", icon_path.display()))?;
        let format = options
            .icon_format
            .or_else(|| icon_path.extension().and_then(|extension| IconFormat::parse(&extension.to_string_lossy())))
            .ok_or_else(|| "`--icon` needs `--icon-format` when the extension is unknown".to_string())?;
        spec.icons.push(
            BundleIcon::new(package, format, options.icon_size, icon_bytes)
                .with_source(icon_path.to_string_lossy().into_owned()),
        );
        input_paths.push(icon_path.clone());
    }
    if options.update_channel.is_some() != options.update_url.is_some() {
        return Err("`--update-channel` and `--update-url` must be supplied together".into());
    }
    if let (Some(channel), Some(url)) = (options.update_channel.as_ref(), options.update_url.as_ref()) {
        spec.updater = Some(UpdaterSpec::new(channel, url));
    }
    let plan = PackageBundlePlan::from_spec(&spec).map_err(|error| error.to_string())?;
    Ok((plan, input_paths))
}
fn launch_game_output(path: &Path, target: BundleTarget, renderer: &str) -> Result<u32, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect game package `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("game package `{}` must not be a symlink", path.display()));
    }
    let mut command = match target {
        BundleTarget::LinuxAppImage if metadata.is_file() => Command::new(path),
        BundleTarget::MacOSApp if metadata.is_dir() && cfg!(target_os = "macos") => {
            let mut command = Command::new("open");
            command.arg(path);
            command
        }
        BundleTarget::LinuxAppImage | BundleTarget::MacOSApp => {
            return Err(format!(
                "game target `{}` is not runnable by this host",
                target.as_str()
            ))
        }
        BundleTarget::WindowsMsix if metadata.is_file() && cfg!(target_os = "windows") => {
            let mut command = Command::new("explorer.exe");
            command.arg(path);
            command
        }
        BundleTarget::WindowsMsix => {
            return Err("windows-msix game packages require a Windows host for `--run-now`".into())
        }
    };
    if renderer == "raylib" {
        command.env("JET_RAYLIB_DISPLAY", "1");
    } else {
        command.env_remove("JET_RAYLIB_DISPLAY");
    }
    let child = command
        .spawn()
        .map_err(|error| format!("could not launch game package `{}`: {error}", path.display()))?;
    Ok(child.id())
}

fn resolve_game_destination(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn deploy_game_output(source: &Path, destination: &Path, force_clean: bool) -> Result<PathBuf, String> {
    let source_metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("could not inspect game package `{}`: {error}", source.display()))?;
    if source_metadata.file_type().is_symlink() {
        return Err(format!("game package `{}` must not be a symlink", source.display()));
    }
    if source_metadata.is_file() {
        let destination_metadata = fs::symlink_metadata(destination).ok();
        if let Some(metadata) = destination_metadata.as_ref() {
            if metadata.file_type().is_symlink() {
                return Err(format!("deploy destination `{}` must not be a symlink", destination.display()));
            }
        }
        let final_path = if destination_metadata.as_ref().is_some_and(|metadata| metadata.is_dir()) {
            destination.join(
                source
                    .file_name()
                    .ok_or_else(|| format!("game package `{}` has no file name", source.display()))?,
            )
        } else {
            destination.to_path_buf()
        };
        if let Ok(metadata) = fs::symlink_metadata(&final_path) {
            if metadata.file_type().is_symlink() {
                return Err(format!("deploy destination `{}` must not be a symlink", final_path.display()));
            }
            if !force_clean {
                return Err(format!(
                    "deploy destination `{}` already exists; pass `--clean` to replace it",
                    final_path.display()
                ));
            }
            if metadata.is_dir() {
                return Err(format!("deploy destination `{}` is a directory", final_path.display()));
            }
            fs::remove_file(&final_path)
                .map_err(|error| format!("could not replace deploy destination `{}`: {error}", final_path.display()))?;
        }
        let parent = final_path
            .parent()
            .ok_or_else(|| format!("deploy destination `{}` has no parent", final_path.display()))?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create deploy directory `{}`: {error}", parent.display()))?;
        let temporary = parent.join(format!(
            ".{}-deploy-{}",
            final_path.file_name().and_then(|name| name.to_str()).unwrap_or("game"),
            std::process::id()
        ));
        if fs::symlink_metadata(&temporary).is_ok() {
            return Err(format!("deploy staging `{}` already exists", temporary.display()));
        }
        let result: Result<(), String> = (|| {
            fs::copy(source, &temporary)
                .map_err(|error| format!("could not copy game package `{}`: {error}", source.display()))?;
            set_mode(&temporary, game_file_mode(&source_metadata))?;
            fs::rename(&temporary, &final_path).map_err(|error| {
                format!(
                    "could not atomically publish deployed game package `{}`: {error}",
                    final_path.display()
                )
            })?;
            Ok(())
        })();
        if result.is_err() {
            cleanup_staging(&temporary, false);
        }
        result?;
        return Ok(final_path);
    }
    if !source_metadata.is_dir() {
        return Err(format!("game package `{}` is not a regular file or directory", source.display()));
    }
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() {
            return Err(format!("deploy destination `{}` must not be a symlink", destination.display()));
        }
        if !force_clean {
            return Err(format!(
                "deploy destination `{}` already exists; pass `--clean` to replace it",
                destination.display()
            ));
        }
        if metadata.is_dir() {
            fs::remove_dir_all(destination)
                .map_err(|error| format!("could not replace deploy destination `{}`: {error}", destination.display()))?;
        } else {
            fs::remove_file(destination)
                .map_err(|error| format!("could not replace deploy destination `{}`: {error}", destination.display()))?;
        }
    }
    let parent = destination
        .parent()
        .ok_or_else(|| format!("deploy destination `{}` has no parent", destination.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create deploy directory `{}`: {error}", parent.display()))?;
    let temporary = parent.join(format!(
        ".{}-deploy-{}",
        destination.file_name().and_then(|name| name.to_str()).unwrap_or("game"),
        std::process::id()
    ));
    if fs::symlink_metadata(&temporary).is_ok() {
        return Err(format!("deploy staging `{}` already exists", temporary.display()));
    }
    let result: Result<(), String> = (|| {
        fs::create_dir(&temporary)
            .map_err(|error| format!("could not create deploy staging `{}`: {error}", temporary.display()))?;
        copy_game_directory(source, &temporary)?;
        set_mode(&temporary, game_file_mode(&source_metadata))?;
        fs::rename(&temporary, destination).map_err(|error| {
            format!(
                "could not atomically publish deployed game package `{}`: {error}",
                destination.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        cleanup_staging(&temporary, true);
    }
    result?;
    Ok(destination.to_path_buf())
}

fn copy_game_directory(source: &Path, destination: &Path) -> Result<(), String> {
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("could not enumerate game package `{}`: {error}", source.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not enumerate game package `{}`: {error}", source.display()))?;
    entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("could not inspect game package `{}`: {error}", source_path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!("game package `{}` must not contain symlinks", source_path.display()));
        }
        if metadata.is_dir() {
            fs::create_dir(&destination_path)
                .map_err(|error| format!("could not create deployed game directory `{}`: {error}", destination_path.display()))?;
            copy_game_directory(&source_path, &destination_path)?;
            set_mode(&destination_path, game_file_mode(&metadata))?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("could not copy deployed game file `{}`: {error}", source_path.display()))?;
            set_mode(&destination_path, game_file_mode(&metadata))?;
        } else {
            return Err(format!("game package `{}` is not a regular file or directory", source_path.display()));
        }
    }
    Ok(())
}

fn stage_game_plan(
    root: &Path,
    plan: &PackageBundlePlan,
    force_clean: bool,
    resume: bool,
    dry_run: bool,
) -> Result<PathBuf, String> {
    let stage_root = root.join(".jet").join("game-stages");
    let identity = plan.identity.replace(':', "-");
    let final_dir = stage_root.join(&identity);
    if dry_run {
        return Ok(final_dir);
    }
    let marker = CanonicalJson::object([
        ("plan_identity".into(), CanonicalJson::String(plan.identity.clone())),
        ("schema".into(), CanonicalJson::String("jet.game.stage".into())),
    ])
    .map_err(|error| format!("could not create game stage marker: {error}"))?
    .bytes();
    if fs::symlink_metadata(&final_dir).is_ok() {
        ensure_game_stage_marker(&final_dir, &marker)?;
        if force_clean {
            fs::remove_dir_all(&final_dir)
                .map_err(|error| format!("could not remove game stage `{}`: {error}", final_dir.display()))?;
        } else {
            if resume {
                return Ok(final_dir);
            }
            return Ok(final_dir);
        }

    } else if resume {
        return Err(format!(
            "no resumable game stage exists for plan `{}`",
            plan.identity
        ));
    }
    fs::create_dir_all(&stage_root)
        .map_err(|error| format!("could not create game stage root `{}`: {error}", stage_root.display()))?;
    let staging = stage_root.join(format!(".{identity}-staging"));
    if fs::symlink_metadata(&staging).is_ok() {
        return Err(format!(
            "game stage `{}` already exists; refusing stale or foreign staging",
            staging.display()
        ));
    }
    let result: Result<(), String> = (|| {
        fs::create_dir(&staging)
            .map_err(|error| format!("could not create game stage `{}`: {error}", staging.display()))?;
        write_atomic_bytes(&staging.join("staging.json"), &marker)?;
        write_atomic_bytes(&staging.join("plan.json"), &plan.to_json().bytes())?;
        for file in &plan.files {
            let destination = staging.join("files").join(&file.path);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("could not create staged game directory `{}`: {error}", parent.display())
                })?;
            }
            fs::write(&destination, &file.bytes)
                .map_err(|error| format!("could not write staged game file `{}`: {error}", destination.display()))?;
            set_mode(&destination, file.mode)?;
        }
        fs::rename(&staging, &final_dir).map_err(|error| {
            format!(
                "could not atomically publish game stage `{}`: {error}",
                final_dir.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        cleanup_staging(&staging, true);
    }
    result?;
    Ok(final_dir)
}
fn verify_game_stage_files(stage: &Path, plan: &PackageBundlePlan) -> Result<(), String> {

    let marker = CanonicalJson::object([
        ("plan_identity".into(), CanonicalJson::String(plan.identity.clone())),
        ("schema".into(), CanonicalJson::String("jet.game.stage".into())),
    ])
    .map_err(|error| format!("could not create game stage marker: {error}"))?
    .bytes();
    ensure_game_stage_marker(stage, &marker)?;
    for file in &plan.files {
        let path = stage.join("files").join(&file.path);
        ensure_regular_game_file(&path, "staged game file")?;
        let bytes = fs::read(&path)
            .map_err(|error| format!("could not read staged game file `{}`: {error}", path.display()))?;
        if bytes != file.bytes || jet::SHA256::sha256_hex(&bytes) != file.digest() {
            return Err(format!("staged game file `{}` has a foreign digest", path.display()));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect staged game file `{}`: {error}", path.display()))?;
        if game_file_mode(&metadata) != file.mode {
            return Err(format!("staged game file `{}` has a foreign mode", path.display()));
        }
    }
    Ok(())
}
fn export_game_output(
    root: &Path,
    plan: &PackageBundlePlan,
    target: BundleTarget,
    preset: &str,
    force_clean: bool,
) -> Result<(PathBuf, MaterializedFacts), String> {
    if plan.target != target {
        return Err(format!(
            "export target `{}` does not match package plan target `{}`",
            target.as_str(),
            plan.target.as_str()
        ));
    }
    if preset == "store" && target != BundleTarget::WindowsMsix {
        return Err("the `store` export preset requires `windows-msix`".into());
    }
    if preset == "headless"
        && plan
            .game
            .as_ref()
            .is_some_and(|game| game.renderer != "headless")
    {
        return Err("the `headless` export preset requires a headless renderer".into());
    }
    let export_root = root.join(".jet").join("game-exports").join(preset);
    fs::create_dir_all(&export_root)
        .map_err(|error| format!("could not create game export directory `{}`: {error}", export_root.display()))?;
    let destination = export_root.join(&plan.artifact_name);
    prepare_export_destination(&destination, force_clean)?;
    let facts = materialize_package(plan, &destination)?;
    Ok((destination, facts))
}

fn prepare_export_destination(path: &Path, force_clean: bool) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(format!(
                "could not inspect existing export `{}`: {error}",
                path.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(format!("export destination `{}` must not be a symlink", path.display()));
    }
    if !force_clean {
        return Err(format!(
            "export destination `{}` already exists; pass `--clean` to replace an owned export",
            path.display()
        ));
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| format!("could not remove existing export `{}`: {error}", path.display()))
}

fn cancel_game_stages(root: &Path) -> Result<(Option<PathBuf>, String), String> {
    let stage_root = root.join(".jet").join("game-stages");
    let metadata = match fs::symlink_metadata(&stage_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((None, "no owned game staging exists".into()))
        }
        Err(error) => {
            return Err(format!(
                "could not inspect game stage root `{}`: {error}",
                stage_root.display()
            ))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "game stage root `{}` is not an owned directory",
            stage_root.display()
        ));
    }
    let mut cancelled = None;
    let entries = fs::read_dir(&stage_root)
        .map_err(|error| format!("could not enumerate game stages `{}`: {error}", stage_root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not enumerate game stages `{}`: {error}", stage_root.display()))?;
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') && !name.ends_with("-staging") {
            continue;
        }
        let path_metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("could not inspect game stage `{}`: {error}", path.display()))?;
        if path_metadata.file_type().is_symlink() || !path_metadata.is_dir() {
            return Err(format!("game stage `{}` is not an owned directory", path.display()));
        }
        let marker = fs::read(path.join("staging.json")).map_err(|error| {
            format!("could not read game stage marker `{}`: {error}", path.join("staging.json").display())
        })?;
        if !marker
            .windows(b"\"schema\":\"jet.game.stage\"".len())
            .any(|window| window == b"\"schema\":\"jet.game.stage\"")
        {
            return Err(format!(
                "game stage `{}` has a foreign marker; refusing cancellation",
                path.display()
            ));
        }
        fs::remove_dir_all(&path)
            .map_err(|error| format!("could not cancel game stage `{}`: {error}", path.display()))?;
        cancelled = Some(path);
    }
    Ok((
        cancelled,
        "owned canonical game stages cancelled".into(),
    ))
}

fn ensure_game_stage_marker(path: &Path, expected: &[u8]) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect game stage `{}`: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("game stage `{}` is not an owned directory", path.display()));
    }
    let marker = path.join("staging.json");
    let bytes = fs::read(&marker)
        .map_err(|error| format!("could not read game stage marker `{}`: {error}", marker.display()))?;
    if bytes != expected {
        return Err(format!(
            "game stage `{}` has a foreign or stale identity; refusing to reuse it",
            path.display()
        ));
    }
    Ok(())
}

fn game_phase(
    phase: &str,
    status: &str,
    header: &GameHeader,
    started: Instant,
    cache_hit: bool,
    invalidation: String,
    output: Option<String>,
    sha256: Option<String>,
    bytes: Option<u64>,
    diagnostic: Option<GameDiagnostic>,
) -> GamePhaseReceipt {
    GamePhaseReceipt {
        phase: phase.into(),
        status: status.into(),
        header: header.clone(),
        cache_hit,
        invalidation,
        output,
        sha256,
        bytes,
        duration_ms: started.elapsed().as_millis() as u64,
        diagnostic,
    }
}

fn game_phase_json(phase: &GamePhaseReceipt) -> CanonicalJson {
    CanonicalJson::object([
        ("authority_identity".into(), CanonicalJson::String(phase.header.authority_identity.clone())),
        ("backend".into(), CanonicalJson::String(phase.header.backend.clone())),
        ("build_identity".into(), CanonicalJson::String(phase.header.build_identity.clone())),
        ("bytes".into(), phase.bytes.map(json_integer).unwrap_or(CanonicalJson::Null)),
        ("cache_hit".into(), CanonicalJson::Bool(phase.cache_hit)),
        ("cook_mode".into(), CanonicalJson::String(phase.header.cook_mode.clone())),
        ("crash_consent".into(), CanonicalJson::String(phase.header.crash_consent.clone())),
        ("crash_reporter".into(), CanonicalJson::String(phase.header.crash_reporter.clone())),
        ("crash_runtime".into(), game_crash_runtime_json(&phase.header.crash_runtime)),
        ("development_stripping".into(), CanonicalJson::Bool(phase.header.development_stripping)),
        ("diagnostic".into(), phase.diagnostic.as_ref().map(game_diagnostic_json).unwrap_or(CanonicalJson::Null)),
        ("duration_ms".into(), json_integer(phase.duration_ms)),
        ("effect_identity".into(), CanonicalJson::String(phase.header.effect_identity.clone())),
        ("invalidation".into(), CanonicalJson::String(phase.invalidation.clone())),
        ("output".into(), phase.output.as_deref().map(json_text_value).unwrap_or(CanonicalJson::Null)),
        ("phase".into(), CanonicalJson::String(phase.phase.clone())),
        ("profile".into(), CanonicalJson::String(phase.header.profile.clone())),
        ("renderer".into(), CanonicalJson::String(phase.header.renderer.clone())),
        ("sha256".into(), phase.sha256.as_deref().map(json_text_value).unwrap_or(CanonicalJson::Null)),
        ("source_revision".into(), CanonicalJson::String(phase.header.source_revision.clone())),
        ("status".into(), CanonicalJson::String(phase.status.clone())),
        ("target".into(), CanonicalJson::String(phase.header.target.clone())),
    ])
    .expect("fixed game phase JSON keys")
}

fn game_diagnostic_json(diagnostic: &GameDiagnostic) -> CanonicalJson {
    CanonicalJson::object([
        ("code".into(), CanonicalJson::String(diagnostic.code.clone())),
        ("fix".into(), CanonicalJson::String(diagnostic.fix.clone())),
        ("what".into(), CanonicalJson::String(diagnostic.what.clone())),
    ])
    .expect("fixed game diagnostic JSON keys")
}

fn game_crash_runtime_json(runtime: &GameCrashReporterSpec) -> CanonicalJson {
    CanonicalJson::object([
        ("abi".into(), CanonicalJson::String(GAME_CRASH_REPORTER_ABI.into())),
        ("installation".into(), json_text_value(&runtime.installation)),
        (
            "memory".into(),
            CanonicalJson::object([
                ("max_logs".into(), json_integer(runtime.memory_logs)),
                ("max_stacks".into(), json_integer(runtime.memory_stacks)),
            ])
            .expect("fixed game crash memory JSON keys"),
        ),
        (
            "retention".into(),
            CanonicalJson::object([
                ("max_reports".into(), json_integer(runtime.retention_limit)),
                ("mode".into(), json_text_value(&runtime.retention_mode)),
                ("path".into(), json_text_value(&runtime.retention_path)),
            ])
            .expect("fixed game crash retention JSON keys"),
        ),
        ("routing".into(), json_text_value(&runtime.routing)),
        ("upload_policy".into(), json_text_value(&runtime.upload_policy)),
    ])
    .expect("fixed game crash runtime JSON keys")
}

fn game_materialization_json(
    status: &str,
    artifact_sha256: Option<&str>,
    output: Option<&Path>,
    artifact: Option<&PackageBundleArtifact>,
) -> CanonicalJson {
    let materializer = artifact
        .and_then(|value| value.receipt.materializer.as_ref())
        .map(game_materializer_json)
        .unwrap_or(CanonicalJson::Null);
    CanonicalJson::object([
        ("artifact_sha256".into(), artifact_sha256.map(json_text_value).unwrap_or(CanonicalJson::Null)),
        (
            "bytes".into(),
            artifact
                .map(|value| json_integer(value.bytes()))
                .unwrap_or(CanonicalJson::Null),
        ),
        (
            "output".into(),
            output
                .map(|path| json_text_value(&path.display().to_string()))
                .unwrap_or(CanonicalJson::Null),
        ),
        ("materializer".into(), materializer),
        ("status".into(), json_text_value(status)),
    ])
    .expect("fixed game materialization JSON keys")
}

fn game_materializer_json(materializer: &BundleMaterializerFact) -> CanonicalJson {
    CanonicalJson::object([
        ("authority".into(), json_text_value(&materializer.authority)),
        ("executable".into(), json_text_value(&materializer.executable)),
        ("mechanism".into(), json_text_value(&materializer.mechanism)),
        ("policy".into(), json_text_value(&materializer.policy)),
        ("tool".into(), json_text_value(&materializer.tool)),
        ("tool_identity".into(), json_text_value(&materializer.tool_identity)),
        ("tool_version".into(), json_text_value(&materializer.tool_version)),
    ])
    .expect("fixed game materializer JSON keys")
}

fn game_report_json(
    header: &GameHeader,
    phases: &[String],
    receipts: &[GamePhaseReceipt],
    plan_identity: Option<&str>,
    plan: Option<&PackageBundlePlan>,
    artifact_sha256: Option<&str>,
    output: Option<&Path>,
    artifact: Option<&PackageBundleArtifact>,
    explain: bool,
    planned: bool,
) -> CanonicalJson {
    let status = if receipts
        .iter()
        .any(|phase| phase.status == "error" || phase.diagnostic.is_some())
    {
        "error"
    } else if receipts.iter().any(|phase| phase.status == "cancelled") {
        "cancelled"
    } else {
        "ok"
    };
    let execution = if planned {
        "planned"
    } else if artifact.is_some() {
        "materialized"
    } else {
        "not-materialized"
    };
    let artifact_status = if artifact.is_some() {
        "materialized"
    } else if plan.is_some() {
        "planned"
    } else {
        "none"
    };
    let signing_status = artifact
        .map(|value| value.receipt.signing_status().as_str())
        .or_else(|| {
            plan.map(|value| {
                if value.signing_hooks.is_empty() {
                    "not-requested"
                } else {
                    "requested"
                }
            })
        })
        .unwrap_or("not-requested");
    let materialization = game_materialization_json(
        execution,
        artifact_sha256,
        output,
        artifact,
    );
    let mut fields = vec![
        (
            "artifact".into(),
            artifact.map(|value| value.to_json()).unwrap_or(CanonicalJson::Null),
        ),
        ("artifact_status".into(), json_text_value(artifact_status)),
        ("artifact_sha256".into(), artifact_sha256.map(json_text_value).unwrap_or(CanonicalJson::Null)),
        ("execution".into(), json_text_value(execution)),
        ("authority_identity".into(), CanonicalJson::String(header.authority_identity.clone())),
        ("backend".into(), CanonicalJson::String(header.backend.clone())),
        ("build_identity".into(), CanonicalJson::String(header.build_identity.clone())),
        ("cook_mode".into(), CanonicalJson::String(header.cook_mode.clone())),
        ("crash_consent".into(), CanonicalJson::String(header.crash_consent.clone())),
        ("crash_reporter".into(), CanonicalJson::String(header.crash_reporter.clone())),
        ("development_stripping".into(), CanonicalJson::Bool(header.development_stripping)),
        ("crash_runtime".into(), game_crash_runtime_json(&header.crash_runtime)),
        ("effect_identity".into(), CanonicalJson::String(header.effect_identity.clone())),
        ("export_preset".into(), CanonicalJson::String(header.export_preset.clone())),
        ("format".into(), CanonicalJson::String(GAME_PACKAGE_RECEIPT_SCHEMA.into())),
        ("output".into(), output.map(|path| json_text_value(&path.display().to_string())).unwrap_or(CanonicalJson::Null)),
        ("materialization".into(), materialization),
        ("phases".into(), CanonicalJson::Array(receipts.iter().map(game_phase_json).collect())),
        ("plan".into(), plan.map(|value| value.to_json()).unwrap_or(CanonicalJson::Null)),
        ("plan_identity".into(), plan_identity.map(json_text_value).unwrap_or(CanonicalJson::Null)),
        ("profile".into(), CanonicalJson::String(header.profile.clone())),
        ("renderer".into(), CanonicalJson::String(header.renderer.clone())),
        ("requested_phases".into(), CanonicalJson::Array(phases.iter().map(|phase| json_text_value(phase)).collect())),
        ("schema".into(), CanonicalJson::String(GAME_PACKAGE_RECEIPT_SCHEMA.into())),
        ("source_revision".into(), CanonicalJson::String(header.source_revision.clone())),
        ("status".into(), CanonicalJson::String(status.into())),
        ("planned".into(), CanonicalJson::Bool(planned)),
        ("target".into(), CanonicalJson::String(header.target.clone())),
        ("signing_status".into(), json_text_value(signing_status)),
    ];
    if explain {
        let cook = receipts.iter().find(|phase| phase.phase == "cook");
        let cache_identity = cook
            .and_then(|phase| phase.sha256.as_deref())
            .unwrap_or("not computed");
        fields.push((
            "explain".into(),
            CanonicalJson::object([
                ("authority_identity".into(), json_text_value(&header.authority_identity)),
                ("cache_identity".into(), json_text_value(cache_identity)),
                ("execution".into(), json_text_value(execution)),
                ("planned".into(), CanonicalJson::Bool(planned)),
                (
                    "inputs".into(),
                    artifact
                        .map(|value| value.receipt.to_json())
                        .or_else(|| plan.map(|value| value.to_json()))
                        .unwrap_or(CanonicalJson::Null),
                ),
                ("phases".into(), CanonicalJson::Array(receipts.iter().map(|phase| json_text_value(&phase.phase)).collect())),
                ("pipeline".into(), CanonicalJson::Array(phases.iter().map(|phase| json_text_value(phase)).collect())),
            ])
            .expect("fixed game explanation JSON keys"),
        ));
    }
    CanonicalJson::object(fields).expect("fixed game package receipt JSON keys")
}

fn json_text_value(value: &str) -> CanonicalJson {
    CanonicalJson::String(value.into())
}

fn json_integer(value: impl ToString) -> CanonicalJson {
    CanonicalJson::Integer(value.to_string())
}
fn write_game_receipt(path: &Path, report: &CanonicalJson) -> Result<(), String> {
    let bytes = report.bytes();
    write_atomic_bytes(path, &bytes)
}

fn record_game_receipt(
    args: &[String],
    input_paths: &[PathBuf],
    report: &CanonicalJson,
    phases: &[GamePhaseReceipt],
) -> Result<jet::ReceiptStore::Receipt, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("could not read current directory: {error}"))?;
    let root = jet::ReceiptStore::receipt_root_for("package", args, &cwd);
    let store = jet::ReceiptStore::ReceiptStore::new(root);
    let mut sections = Vec::with_capacity(phases.len() + 1);
    sections.push(
        jet::ReceiptStore::ReceiptSection::from_json(
            "game",
            "GamePackageReceipt",
            report.clone(),
        )
        .map_err(|error| format!("could not encode game package receipt section: {error}"))?,
    );
    for phase in phases {
        sections.push(
            jet::ReceiptStore::ReceiptSection::from_json(
                format!("game.{}", phase.phase),
                "GamePhaseReceipt",
                game_phase_json(phase),
            )
            .map_err(|error| format!("could not encode {} receipt section: {error}", phase.phase))?,
        );
    }
    store.record_with_sections(
        "package",
        args,
        input_paths,
        0,
        &report.bytes(),
        &[],
        &sections,
    )
}

fn render_game_report(
    run: &GameRun,
    report_path: &Path,
    receipt_digest: &str,
    mode: crate::OutputMode,
) {
    if mode.json {
        let game = status_value(&run.report);
        let fields = StatusFields::new()
            .with("kind", "game")
            .with("game", game)
            .with("receipt", report_path.display().to_string())
            .with("receipt_digest", receipt_digest);
        println!(
            "{}",
            StatusEnvelope::new("package", run.success)
                .with_fields(fields)
                .json()
        );
        return;
    }
    println!("game package: {}", if run.success { "ok" } else { "error" });
    if let Some(phase) = run.phases.first() {
        println!(
            "target: {} profile: {} backend: {} renderer: {}",
            phase.header.target, phase.header.profile, phase.header.backend, phase.header.renderer
        );
    }
    for phase in &run.phases {
        println!(
            "{}: {}{}",
            phase.phase,
            phase.status,
            phase
                .output
                .as_ref()
                .map(|output| format!(" ({output})"))
                .unwrap_or_default()
        );
    }
    if let Some(output) = run.output.as_ref() {
        println!("output: {}", output.display());
    }
    println!("receipt: {} ({receipt_digest})", report_path.display());
}
