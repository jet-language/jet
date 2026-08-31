use super::parse::Parsed;
use super::realize::{
    RealizeScope,
    channel_sources, load_project_plan, offline_refusal, realize_ref_outcome,
    report_nix_bridge_required, resolve_source_channel, RefOutcome, RowStyle,
};
use super::workspace_sources::cwd_table;
use crate::Output::{self, Theme};
use crate::RefSpec;
use crate::Store;
use crate::{Components, EnvFile, Image, Lock, Syntax};
use jet_env_model::ModuleEval;
use crate::SHA256;
use std::fs;
use std::path::Path;

enum ImagePushDestination {
    Local(std::path::PathBuf),
    Registry(String),
}

fn record_omitted_secret_integration_projection(
    projection: &mut Image::ProjectionReport,
    integration: &ModuleEval::EnvironmentIntegration,
) {
    let prefix = format!("integration:{}", integration.kind.as_str());
    projection.omitted.push(format!("{prefix}:activation"));
    for task in &integration.tasks {
        projection.omitted.push(format!("{prefix}:task:{task}"));
    }
    for provider in &integration.providers {
        projection
            .omitted
            .push(format!("{prefix}:provider:{provider}"));
    }
    for grant in &integration.grants {
        projection.omitted.push(format!("{prefix}:grant:{grant}"));
    }
    for host_check in &integration.host_checks {
        projection
            .omitted
            .push(format!("{prefix}:host-check:{host_check}"));
    }
    if !integration.options.is_empty() {
        let option_keys = integration
            .options
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        projection
            .omitted
            .push(format!("{prefix}:option-keys={option_keys}"));
    }
    if !integration.secrets.is_empty() {
        projection.omitted.push(format!(
            "{prefix}:secret-refs={}",
            integration.secrets.len()
        ));
    }
}

/// `jetpack add <ref>` — edit the project env file. `jetpack add <Component>`
/// (an exact, case-sensitive match against the starter component catalog —
/// Button/Label/Input/Container) is a distinct behavior checked first: it
/// copies real `.jet` source into `./components/` instead of touching the env
/// file (Tower c134 Phase 4, the ownable component kit). The two never
/// collide because Jetpack source names are always lowercase
/// (`nixpkgs`/`github`/user-declared names), so an exact-case
/// `Button`-style name can only ever mean a component.
pub(super) fn cmd_add(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        return offline_refusal(theme, "add");
    }
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "add what?",
            "`jetpack add` needs a ref or a starter component to add.",
            "try `jetpack add ripgrep@nixpkgs` or `jetpack add Button`.",
        );
        return 2;
    };
    if let Some(component) = Components::find(raw) {
        return cmd_add_component(theme, component);
    }
    if parsed.flags.adapt {
        return cmd_add_adapt(theme, raw);
    }
    let dir = std::env::current_dir().unwrap_or_default();
    // Classify against the env's declared sources so `add fd@unstable` works
    // when `unstable` is already declared.
    let table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    let spec = match RefSpec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            Output::ref_error(theme, &e);
            return 2;
        }
    };
    // Tier 1 is still real work: resolve before editing the manifest so the
    // quiet `✓ package version` row is a verified package fact, not a cosmetic
    // echo of the requested name. A failed resolution leaves env.jet intact.
    let roots = Store::resolve();
    let _lease = match realize_ref_outcome(
        theme,
        &roots,
        &parsed.flags,
        &table,
        &spec,
        spec.package.len().max(8),
        RowStyle::Ready,
        None,
        RealizeScope::Project,
    ) {
        RefOutcome::Realized(_entry, _state, _line, lease) => lease,
        RefOutcome::NeedsNix(need) => {
            report_nix_bridge_required(theme, &parsed.flags, &[need], &[]);
            return 2;
        }
        RefOutcome::Unavailable | RefOutcome::Failed => return 1,
    };
    match EnvFile::add(&dir, &spec) {
        Ok(ef) => {
            theme.ok(&format!(
                "added {} to {}",
                theme.bold(&spec.package),
                Syntax::ENV_FILE
            ));
            theme.detail(&theme.gray(&format!("now: {}", ef.packages.join(", "))));
            if let Ok(plan) = load_project_plan(theme) {
                for source in channel_sources(&plan.table) {
                    if let Ok(exact) = resolve_source_channel(&source, &parsed.flags) {
                        Lock::record_source_channel(
                            &dir,
                            Lock::LockedSourceChannel {
                                name: source.name.clone(),
                                channel: source.lock_channel().to_string(),
                                exact,
                            },
                        );
                    }
                }
            }
            0
        }
        Err(e) => {
            theme.error(
                "could not edit the env file",
                &format!("{e}"),
                "check write permissions here.",
            );
            1
        }
    }
}

fn cmd_add_adapt(theme: &Theme, raw: &str) -> i32 {
    let source = if raw.contains(Syntax::REF_PROVIDER_AT) {
        match RefSpec::classify_provider_ref(raw) {
            Ok(r) => r.raw,
            Err(e) => {
                Output::ref_error(theme, &e);
                return 2;
            }
        }
    } else {
        let table = cwd_table();
        let spec = match RefSpec::classify_in(raw, &table) {
            Ok(s) => s,
            Err(e) => {
                Output::ref_error(theme, &e);
                return 2;
            }
        };
        match spec.source {
            RefSpec::Source::Path => spec.package,
            RefSpec::Source::Github => format!("{}@github", spec.package),
            RefSpec::Source::Named(name) => match table.upstream(&name) {
                Some(upstream) if upstream.starts_with("path:") => {
                    upstream.trim_start_matches("path:").to_string()
                }
                Some(upstream) if upstream.starts_with("github:") => {
                    format!("{}@github", upstream.trim_start_matches("github:"))
                }
                _ => {
                    theme.error_coded(
                        "E1270",
                        "adapter draft needs source bytes",
                        "that named source does not point at a path or GitHub source tree.",
                        "write `Pkg.adapt(...)` by hand with a quoted local `source: \"./...\"` value.",
                    );
                    return 2;
                }
            },
            RefSpec::Source::Jetpack | RefSpec::Source::Nixpkgs => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "`<pkg>@nixpkgs` names a package in an index, not an upstream source tree.",
                    "use the package's source URL with `source: owner/repo@github#rev` or `source: \"./vendor/pkg\"`.",
                );
                return 2;
            }
            RefSpec::Source::Cran => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "a CRAN ref names a registry package, not an unpacked source tree.",
                    "realize the CRAN package first, then adapt its locked source artifact.",
                );
                return 2;
            }
            RefSpec::Source::LuaRocks => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "a LuaRocks ref names a registry package, not an unpacked source tree.",
                    "realize the LuaRocks package first, then adapt its locked source artifact.",
                );
                return 2;
            }
            RefSpec::Source::RubyGems | RefSpec::Source::Cpan | RefSpec::Source::Packagist => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "a scripting-registry ref names a package, not an unpacked source tree.",
                    "realize the exact registry package first, then adapt its locked source artifact.",
                );
                return 2;
            }
            RefSpec::Source::JetRegistry
            | RefSpec::Source::Npm
            | RefSpec::Source::Cargo
            | RefSpec::Source::PyPI
            | RefSpec::Source::SwiftPM
            | RefSpec::Source::Releases => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "a Jet registry, npm, or Cargo ref names a registry package, not an unpacked source tree.",
                    "realize the exact provider package first, then adapt its locked source artifact.",
                );
                return 2;
            }
        }
    };
    let name = source
        .split(['/', ':', '@', '#'])
        .filter(|s| !s.is_empty())
        .last()
        .unwrap_or("tool")
        .trim_end_matches(".git")
        .to_string();
    let source = if source.starts_with("./") || source.starts_with("../") || source.starts_with('/')
    {
        format!("\"{source}\"")
    } else {
        source
    };
    println!(
        "Pkg.adapt(\n    name: \"{name}\",\n    source: {source},\n    recipe: Recipe.copy(),\n)"
    );
    0
}

/// Copy a starter component's source into `./components/<Name>.jet`.
fn cmd_add_component(theme: &Theme, component: &Components::StarterComponent) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    match Components::add_component(&dir, component) {
        Ok(dest) => {
            theme.ok(&format!(
                "added {} to {}",
                theme.bold(component.name),
                Components::COMPONENTS_DIR
            ));
            theme.detail(&theme.gray(&format!("wrote {}", dest.display())));
            theme.detail("it's yours now — edit it freely.");
            0
        }
        Err(Components::ComponentError::AlreadyExists(path)) => {
            theme.error(
                &format!("{} already exists", path.display()),
                "it may already be customized — `jetpack add` never overwrites a component you own.",
                "edit it directly, or remove it first if you want a fresh copy.",
            );
            1
        }
        Err(e) => {
            theme.error(
                "could not add that component",
                &format!("{e}"),
                "check write permissions here.",
            );
            1
        }
    }
}

/// `jetpack remove <ref>` — edit the project env file.
pub(super) fn cmd_remove(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "remove what?",
            "`jetpack remove` needs a ref to remove.",
            "try `jetpack remove ripgrep@nixpkgs`.",
        );
        return 2;
    };
    let dir = std::env::current_dir().unwrap_or_default();
    let table = EnvFile::load(&dir)
        .map(|ef| ef.source_table())
        .unwrap_or_else(RefSpec::SourceTable::empty);
    let spec = match RefSpec::classify_in(raw, &table) {
        Ok(s) => s,
        Err(e) => {
            Output::ref_error(theme, &e);
            return 2;
        }
    };
    theme.status("Plan env edit");
    theme.plan_row(
        Output::PlanMark::Remove,
        &spec.package,
        spec.package.len().max(8),
        raw,
        Syntax::ENV_FILE,
    );
    theme.download_line(0);
    if !theme.confirm_apply(parsed.flags.assume_yes) {
        return 0;
    }
    match EnvFile::remove(&dir, &spec) {
        Ok((_ef, true)) => {
            theme.ok(&format!(
                "removed {} from {}",
                theme.bold(&spec.package),
                Syntax::ENV_FILE
            ));
            0
        }
        Ok((_ef, false)) => {
            theme.status(&format!(
                "{} was not in {}.",
                spec.package,
                Syntax::ENV_FILE
            ));
            0
        }
        Err(e) => {
            theme.error(
                "could not edit the env file",
                &format!("{e}"),
                "check write permissions here.",
            );
            1
        }
    }
}

/// `jetpack push <fleet>` (U15) — deploy a fleet's hosts. Parses and
/// cross-checks the fleet now (each host references a known `System`, E1242);
/// the ssh/closure rollout is gated on single-host jetos realization (Phase D),
/// so a valid fleet gets an honest E1243 gated notice rather than a fake deploy.
pub(super) fn cmd_push(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "no fleet here",
            &format!(
                "there is no {} declaring any `fleet.<name>`.",
                Syntax::ENV_FILE
            ),
            "declare `module fleet.<name> { hosts: { … } }`, then `jet push <name>`.",
        );
        return 2;
    };
    // evaluate_env parses, discovers imports, and cross-checks every fleet host
    // against the known systems (E1242) — a bad host fails here.
    let plan = match ModuleEval::evaluate_env(&src, &dir) {
        Ok(p) => p,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(Syntax::ENV_FILE, &src, std::slice::from_ref(&d))
            );
            return 2;
        }
    };

    let available: Vec<String> = plan.fleets.iter().map(|f| f.name.clone()).collect();
    let Some(name) = parsed.positional.first() else {
        theme.error(
            "push which fleet?",
            &if available.is_empty() {
                format!("no `fleet.<name>` is declared in {}.", Syntax::ENV_FILE)
            } else {
                format!("declared fleets: {}.", available.join(", "))
            },
            "name a fleet: `jet push <fleet>`.",
        );
        return 2;
    };

    let Some(fleet) = plan.fleets.iter().find(|f| &f.name == name) else {
        theme.error(
            &format!("no fleet `{name}`"),
            &if available.is_empty() {
                format!("no `fleet.<name>` is declared in {}.", Syntax::ENV_FILE)
            } else {
                format!("declared fleets: {}.", available.join(", "))
            },
            "declare `module fleet.<name> { hosts: { … } }`, or push an existing fleet.",
        );
        return 2;
    };

    // The fleet is valid and fully captured. Deployment is gated (Phase D).
    let host_list = fleet
        .hosts
        .iter()
        .map(|h| format!("{} → system.{}", h.name, h.system))
        .collect::<Vec<_>>()
        .join(", ");
    theme.error(
        &format!("[E1243] fleet `{name}` is validated, but `jet push` is not available yet"),
        &format!(
            "the fleet's {} host(s) ({host_list}) parse and cross-check clean, but rolling a fleet out over ssh needs single-host jetos realization, which is gated (Phase D, owner greenlight required).",
            fleet.hosts.len()
        ),
        "track the jetos realization tier; until it lands, `jet push` captures and validates fleets without deploying them.",
    );
    2
}

/// `jetpack bridge flake` (U16, card c9jetpackgates) — bounded native translator
/// from a foreign `flake.nix`'s devShell into jetpack's own `env.*` module
/// form. Never edits the project's `env.jet`; the shim prints to stdout for
/// the user to review and merge (I8 — one canonical env surface).
/// D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): `jet image <name>`
/// builds the named `.Oci` `image.<name>` module contribution into a native
/// OCI layout (`Jetpack::Image`). `.Iso` images ride the jetos installer tier
/// (Phase D, owner-gated — untouched here); --push uses a local immutable
/// copy or the native OCI Distribution transport.
pub(super) fn cmd_image(theme: &Theme, parsed: &Parsed) -> i32 {
    let dir = std::env::current_dir().unwrap_or_default();
    let Ok(src) = std::fs::read_to_string(EnvFile::path_in(&dir)) else {
        theme.error(
            "no image here",
            &format!(
                "there is no {} declaring any `image.<name>`.",
                Syntax::ENV_FILE
            ),
            "declare `module image.<name> { kind: .Oci, from: packages.<name> }`, then `jet image <name>`.",
        );
        return 2;
    };
    let plan = match ModuleEval::evaluate_env(&src, &dir) {
        Ok(p) => p,
        Err(d) => {
            eprint!(
                "{}",
                crate::Diagnostics::render_all(Syntax::ENV_FILE, &src, std::slice::from_ref(&d))
            );
            return 2;
        }
    };

    let available: Vec<String> = plan.images.iter().map(|i| i.name.clone()).collect();
    let declared = || {
        if available.is_empty() {
            format!("no `image.<name>` is declared in {}.", Syntax::ENV_FILE)
        } else {
            format!("declared images: {}.", available.join(", "))
        }
    };
    let Some(name) = parsed.positional.first() else {
        theme.error(
            "build which image?",
            &declared(),
            "name an image: `jet image <name>`.",
        );
        return 2;
    };

    let Some(image) = plan.images.iter().find(|i| &i.name == name).cloned() else {
        theme.error(
            &format!("no image `{name}`"),
            &declared(),
            "declare `module image.<name> { … }`, or build an existing image.",
        );
        return 2;
    };

    if image.kind != ModuleEval::ImageKind::Oci {
        theme.error(
            &format!("`{name}` is a `.Iso` disk image, not a container"),
            "U14: `.Iso`/`.Qcow`/`.Raw` disk images ride the jetos installer tier, which is gated (Phase D, owner greenlight required) — `jet image` only builds `.Oci` containers today.",
            "build an `.Oci` image instead, or track the jetos realization tier for disk images.",
        );
        return 2;
    }

    // An image names its environment explicitly. Do not silently project the
    // beginner-default `dev` environment when the record says `env.full`.
    let plan = if image.from_environment {
        match ModuleEval::evaluate_env_with_environment(&src, &dir, Some(&image.from)) {
            Ok(plan) => plan,
            Err(d) => {
                eprint!(
                    "{}",
                    crate::Diagnostics::render_all(
                        Syntax::ENV_FILE,
                        &src,
                        std::slice::from_ref(&d),
                    )
                );
                return 2;
            }
        }
    } else {
        plan
    };

    let push_destination = match parsed.flags.push.as_deref() {
        Some(push_ref) if push_ref.starts_with("https://") || push_ref.starts_with("http://") => {
            Some(ImagePushDestination::Registry(push_ref.to_string()))
        }
        Some(push_ref) if push_ref.starts_with("oci://") => {
            theme.error_coded(
                "E1268",
                &format!("OCI reference {push_ref} needs an HTTP registry URL"),
                "the native registry transport accepts an explicit http:// or https:// registry reference.",
                "use --push https://registry.example/repository:tag, or use file:///... for a local layout.",
            );
            return 2;
        }
        Some(push_ref) if looks_like_registry_reference(push_ref) => {
            theme.error_coded(
                "E1268",
                &format!("`jet image {name}` cannot push remote OCI reference `{push_ref}`"),
                "remote registry TLS/transport requires an explicit verified HTTP(S) endpoint; Jet never treats a registry name as a local path.",
                "use `--push https://registry.example/repository:tag` for the configured transport, or `--push file:///path/to/layout` for a local copy.",
            );
            return 2;
        }
        Some(push_ref) => Some(ImagePushDestination::Local(
            if let Some(path) = push_ref.strip_prefix("file://") {
                std::path::PathBuf::from(path)
            } else {
                std::path::PathBuf::from(push_ref)
            },
        )),
        None => None,
    };

    let base_directory = match image.base.as_deref() {
        Some(reference)
            if reference.starts_with("https://") || reference.starts_with("http://") =>
        {
            theme.error_coded(
                "E1268",
                &format!("base OCI reference `{reference}` needs a verified local copy"),
                "remote base pulls require registry transport and digest admission before their layers can become image input.",
                "copy the base into a local OCI layout and use `base: oci(\"file:///path/to/layout\")`.",
            );
            return 2;
        }
        Some(reference) => {
            let path = if let Some(file) = reference.strip_prefix("file://") {
                std::path::PathBuf::from(file)
            } else {
                let relative = std::path::Path::new(reference);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|component| component == std::path::Component::ParentDir)
                {
                    theme.error(
                        &format!("base OCI layout `{reference}` escapes the project"),
                        "a project-relative base must stay inside the project root.",
                        "use a safe relative layout path or an explicit file:/// absolute path.",
                    );
                    return 2;
                }
                dir.join(relative)
            };
            if !path.is_dir() {
                theme.error(
                    &format!("base OCI layout `{}` does not exist", path.display()),
                    "a base image must be a verified local OCI image layout.",
                    "use `file:///...` or a project-relative OCI layout directory.",
                );
                return 2;
            }
            Some(path)
        }
        None => None,
    };

    // D-JPK-IMAGE1: non-environment package images still read the compiler's
    // project-local `build/<name>` output. Environment images below consume
    // verified Hangar package outputs instead.
    let roots = Store::resolve();
    let out_dir = dir.join(".jet").join("images").join(name);
    let mut projection = Image::ProjectionReport::default();
    if !image.from_environment && !image.services.is_empty() {
        projection.rejected.push("services".to_string());
        let _ = write_rejected_projection(&out_dir, &projection);
        theme.error_coded(
            "E1336",
            &format!("package image {name} cannot project services"),
            "services is an environment projection field, not a package-image input",
            "use `from: env.<name>` for a supervised environment image, or remove `services:`",
        );
        return 2;
    }
    if image.from_environment && image.services.is_empty() && !plan.dev_services.is_empty() {
        projection.rejected.push("services".to_string());
        if let Err(error) = write_rejected_projection(&out_dir, &projection) {
            theme.error(
                &format!("couldn't write image {name} rejection projection"),
                &error.to_string(),
                "check that the image output directory is writable.",
            );
        }
        let service_names = plan
            .dev_services
            .iter()
            .map(|service| format!("\"{}\"", service.name))
            .collect::<Vec<_>>()
            .join(", ");
        let fix = format!(
            "add `services: [{service_names}]` to the image, or remove the service from the environment"
        );
        theme.error_coded(
            "E1336",
            &format!("environment image {name} cannot project services"),
            "the environment declares services but the image does not select them; Jet will not silently omit supervised service facts",
            &fix,
        );
        return 2;
    }
    let image_platform = image.target.as_deref().unwrap_or("linux.x64");
    if image.from_environment {
        for integration in &plan.integrations {
            if let Err(error) = integration.validate_target(image_platform) {
                projection
                    .rejected
                    .push(format!("integration:{}:host", integration.kind.as_str()));
                let _ = write_rejected_projection(&out_dir, &projection);
                theme.error_coded(
                    "E1333",
                    &format!("environment image {name} cannot project this integration"),
                    &error,
                    "choose a matching image target or remove the integration from the environment",
                );
                return 2;
            }
        }
        for language in &plan.language_projections {
            if !language.selection.enable {
                continue;
            }
            let unsupported = if language.license.trim().is_empty() {
                Some(format!(
                    "language pack `{}` has no license fact",
                    language.pack.name
                ))
            } else if !language.missing_tools.is_empty() {
                Some(format!(
                    "language pack `{}` is missing required tools: {}",
                    language.pack.name,
                    language.missing_tools.join(", ")
                ))
            } else if !hangar_platform_matches(image_platform, &language.platform) {
                Some(format!(
                    "language pack `{}` targets `{}`, but the image targets `{image_platform}`",
                    language.pack.name, language.platform
                ))
            } else {
                None
            };
            if let Some(error) = unsupported {
                projection
                    .rejected
                    .push(format!("language:{}:facts", language.pack.name));
                let _ = write_rejected_projection(&out_dir, &projection);
                theme.error_coded(
                    "E1333",
                    &format!("environment image {name} cannot project this language pack"),
                    &error,
                    "choose a supported language pack and image target with an admitted license and complete tool mapping",
                );
                return 2;
            }
        }
    }
    let mut files = if image.from_environment {
        environment_image_files(theme, &roots, &plan, name, image_platform, &mut projection)
    } else {
        let bin_path = dir.join("build").join(&image.from);
        let bin_data = match read_project_image_file(&dir.join("build"), &image.from) {
            Ok(data) => data,
            Err(_) => {
                theme.error(
                    &format!("{} isn't built yet", image.from),
                    &format!(
                        "jet image {name} needs {} already built at {}.",
                        image.from,
                        bin_path.display()
                    ),
                    &format!("run jet build first, then jet image {name}"),
                );
                return 2;
            }
        };
        vec![Image::LayerFile {
            path: format!("usr/local/bin/{}", image.from),
            data: bin_data,
            mode: 0o755,
        }]
    };
    if image.from_environment {
        projection.included.extend(
            plan.package_refs
                .iter()
                .map(|value| format!("package:{value}")),
        );
        projection.included.push("environment:shell".to_string());
        projection.changed.push("from:environment".to_string());
        if !plan.secrets.is_empty() {
            projection.omitted.push("environment.secrets".to_string());
        }
        if !plan.source_files.is_empty() {
            projection
                .omitted
                .push("environment.source-files".to_string());
        }
        if !plan.environment_reads.is_empty() {
            projection.omitted.push("environment.reads".to_string());
        }
        if !plan.adapters.is_empty() {
            projection.omitted.push("environment.adapters".to_string());
        }
        if plan.prompt.is_some() {
            projection.omitted.push("environment.prompt".to_string());
        }
        if !plan.systems.is_empty() {
            projection.omitted.push("environment.systems".to_string());
        }
        if !plan.fleets.is_empty() {
            projection.omitted.push("environment.fleets".to_string());
        }
        if !plan.vmtests.is_empty() {
            projection.omitted.push("environment.vmtests".to_string());
        }
        if !plan.package_profiles.is_empty() {
            projection
                .omitted
                .push("environment.package-profiles".to_string());
        }
        let lifecycle = &plan.lifecycle;
        if !lifecycle.dotenv.is_empty()
            || !lifecycle.unset.is_empty()
            || !lifecycle.on_enter.is_empty()
            || !lifecycle.checks.is_empty()
            || lifecycle.git_hooks_path.is_some()
            || lifecycle.reload_explicit
        {
            projection.omitted.push("environment.lifecycle".to_string());
        }
        if !plan.files.is_empty() {
            projection
                .omitted
                .push("environment.managed-files".to_string());
        }
        if !plan.integrations.is_empty() {
            projection
                .omitted
                .push("environment.integrations".to_string());
        }
        for integration in &plan.integrations {
            if matches!(
                integration.kind,
                ModuleEval::IntegrationKind::CloudCredentials | ModuleEval::IntegrationKind::Vault
            ) {
                record_omitted_secret_integration_projection(&mut projection, integration);
            }
        }
        for language in &plan.language_projections {
            let prefix = format!("language:{}", language.selection.name);
            projection
                .changed
                .push(format!("{prefix}:pack={}", language.pack.fingerprint()));
            projection.included.extend(
                language
                    .included
                    .iter()
                    .map(|fact| format!("{prefix}:included:{fact}")),
            );
            projection.changed.extend(
                language
                    .changed
                    .iter()
                    .map(|fact| format!("{prefix}:changed:{fact}")),
            );
            projection.omitted.extend(
                language
                    .omitted
                    .iter()
                    .map(|fact| format!("{prefix}:omitted:{fact}")),
            );
        }
        if !plan.presets.is_empty() {
            projection.omitted.push("environment.presets".to_string());
        }
    } else {
        projection.included.push(format!("package:{}", image.from));
    }
    for (key, _) in &image.env_vars {
        projection.changed.push(format!("env:{key}"));
    }
    if !image.expose.is_empty() {
        projection.changed.push("expose".to_string());
    }
    if let Some(target) = &image.target {
        projection.changed.push(format!("platform:{target}"));
    }
    if image.base.is_some() {
        projection.changed.push("base".to_string());
    }
    if image.user.is_some() {
        projection.changed.push("user".to_string());
    }
    if image.health.is_some() {
        projection.changed.push("health".to_string());
    }
    if image.entrypoint.is_some() {
        projection.changed.push("entrypoint".to_string());
    }
    if files.is_empty() {
        projection
            .rejected
            .push("environment.package-output".to_string());
        if let Err(error) = write_rejected_projection(&out_dir, &projection) {
            theme.error(
                &format!("couldn't write image {name} rejection projection"),
                &error.to_string(),
                "check that the image output directory is writable.",
            );
        }
        return 2;
    }
    for rel in &image.files {
        if image.from_environment {
            if let Some(error) = environment_image_file_rejection(&plan, rel) {
                projection.rejected.push(format!("file:{rel}"));
                let _ = write_rejected_projection(&out_dir, &projection);
                theme.error_coded(
                    "E1336",
                    &format!("environment image {name} cannot project extra file `{rel}`"),
                    &error,
                    "choose a regular, project-relative, non-secret file that is not an environment input",
                );
                return 2;
            }
        }
        let layer_path = match normalize_project_relative_image_path(rel) {
            Ok(path) => path,
            Err(error) => {
                projection.rejected.push(format!("file:{rel}"));
                let _ = write_rejected_projection(&out_dir, &projection);
                if image.from_environment {
                    theme.error_coded(
                        "E1336",
                        &format!("environment image {name} cannot project extra file `{rel}`"),
                        &error,
                        "use a regular, project-relative, non-secret file that stays inside the project root",
                    );
                } else {
                    theme.error(
                        &format!("image file {rel} cannot be projected"),
                        &error,
                        "use a regular project-relative file that stays inside the project root.",
                    );
                }
                return 2;
            }
        };
        let data = match read_project_image_file(&dir, &layer_path) {
            Ok(data) => data,
            Err(error) => {
                projection.rejected.push(format!("file:{rel}"));
                let _ = write_rejected_projection(&out_dir, &projection);
                if image.from_environment {
                    theme.error_coded(
                        "E1336",
                        &format!("environment image {name} cannot project extra file `{rel}`"),
                        &error,
                        "use a regular project-relative, non-secret file that stays inside the project root",
                    );
                } else {
                    theme.error(
                        &format!("image file {rel} cannot be projected"),
                        &error,
                        "use a regular project-relative file that stays inside the project root.",
                    );
                }
                return 2;
            }
        };
        if image.from_environment && image_layer_path_conflicts(&files, &layer_path) {
            projection.rejected.push(format!("file:{layer_path}"));
            let _ = write_rejected_projection(&out_dir, &projection);
            theme.error_coded(
                "E1336",
                &format!("environment image {name} cannot project extra file `{rel}`"),
                "the extra file conflicts with another projected image path",
                "choose an extra file path that does not replace a package, shell, or service projection",
            );
            return 2;
        }
        files.push(Image::LayerFile {
            path: layer_path.clone(),
            data,
            mode: 0o644,
        });
        projection.included.push(format!("file:{layer_path}"));
    }

    let (service_commands, service_names) = if image.from_environment {
        match image_service_commands(&plan, &image, &files) {
            Ok(projection) => projection,
            Err(error) => {
                projection.rejected.push("services".to_string());
                let _ = write_rejected_projection(&out_dir, &projection);
                theme.error_coded(
                    "E1336",
                    &format!("environment image {name} cannot supervise its services"),
                    &error,
                    "give each selected service a projected executable and a supported foreground run command",
                );
                return 2;
            }
        }
    } else {
        (Vec::new(), Vec::new())
    };
    if !service_commands.is_empty() {
        if image.from_environment && image_layer_path_conflicts(&files, "jet/supervise") {
            let conflicting_path = files
                .iter()
                .find(|file| {
                    image_layer_path_conflicts(std::slice::from_ref(file), "jet/supervise")
                })
                .map(|file| file.path.clone())
                .unwrap_or_else(|| "jet/supervise".to_string());
            projection
                .included
                .retain(|fact| fact != &format!("file:{conflicting_path}"));
            projection.rejected.push(format!("file:{conflicting_path}"));
            let _ = write_rejected_projection(&out_dir, &projection);
            theme.error_coded(
                "E1336",
                &format!("environment image {name} cannot project extra file `{conflicting_path}`"),
                "the extra file conflicts with the generated service supervisor",
                "choose an extra file path that does not replace a service projection",
            );
            return 2;
        }
        files.push(Image::LayerFile {
            path: "jet/supervise".to_string(),
            data: supervisor_script(&service_commands).into_bytes(),
            mode: 0o755,
        });
        projection
            .included
            .push("services:jet/supervise".to_string());
        projection
            .changed
            .push(format!("services:{}", service_names.join(",")));
        for service in &plan.dev_services {
            if service_names.iter().any(|name| name == &service.name) {
                projection
                    .included
                    .push(format!("service:{}", service.name));
                if !service.after.is_empty() {
                    projection.changed.push(format!(
                        "service:{}:after={}",
                        service.name,
                        crate::Services::dependency_names(service).join(",")
                    ));
                }
                if service.ports.is_empty() {
                    projection
                        .omitted
                        .push(format!("service:{}:ports-default", service.name));
                } else {
                    projection
                        .changed
                        .push(format!("service:{}:ports", service.name));
                }
            } else {
                projection.omitted.push(format!("service:{}", service.name));
            }
        }
    }

    let spec = Image::BuildSpec {
        files,
        platform: image_platform.to_string(),
        entrypoint: vec![image.entrypoint.clone().unwrap_or_else(|| {
            if !service_commands.is_empty() {
                "/jet/supervise".to_string()
            } else if image.from_environment {
                "/bin/sh".to_string()
            } else {
                format!("/usr/local/bin/{}", image.from)
            }
        })],
        env: image.env_vars.clone(),
        expose: image.expose.clone(),
        user: image.user.unwrap_or(10_001),
        healthcheck: image.health.clone(),
    };
    let projection_plan = Image::ProjectionPlan {
        source: if image.from_environment {
            format!("env:{}", image.from)
        } else {
            format!("package:{}", image.from)
        },
        platform: spec.platform.clone(),
        entrypoint: spec.entrypoint.clone(),
        user: spec.user,
        expose: spec.expose.clone(),
        healthcheck: spec.healthcheck.is_some(),
        env_keys: image.env_vars.iter().map(|(key, _)| key.clone()).collect(),
        layer_paths: spec.files.iter().map(|file| file.path.clone()).collect(),
        services: service_names,
        base: image.base.is_some(),
        lock: if Lock::load(&dir).is_some() {
            "present".to_string()
        } else {
            "absent".to_string()
        },
    };
    match Image::build_with_base(&spec, &out_dir, name, base_directory.as_deref()) {
        Ok(built) => {
            let artifact_result = if image.from_environment {
                Image::write_projection_artifacts(
                    &out_dir,
                    &built.manifest_digest,
                    &projection_plan,
                    &projection,
                )
            } else {
                Image::write_projection_report(&out_dir, &built.manifest_digest, &projection)
            };
            if let Err(error) = artifact_result {
                theme.error(
                    &format!("couldn't write image {name} projection"),
                    &error.to_string(),
                    "check that the image output directory is writable.",
                );
                return 2;
            }
            match push_destination {
                Some(ImagePushDestination::Local(destination)) => {
                    if let Err(error) = Image::copy_layout(&out_dir, &destination) {
                        theme.error(
                            &format!("couldn't copy image {name} to {}", destination.display()),
                            &error.to_string(),
                            "choose an empty or byte-identical local OCI layout destination.",
                        );
                        return 2;
                    }
                }
                Some(ImagePushDestination::Registry(reference)) => {
                    match Image::push_registry(&out_dir, &reference) {
                        Ok(report) => theme.detail(&format!(
                            "published {} ({} new blobs, {} bytes)",
                            report.reference, report.blobs_uploaded, report.bytes_uploaded
                        )),
                        Err(error) => {
                            theme.error(
                                &format!("couldn't publish image {name}"),
                                &error.to_string(),
                                "check the registry URL and its host-configured credentials.",
                            );
                            return 2;
                        }
                    }
                }
                None => {}
            }
            theme.ok(&format!(
                "built image `{name}` -> {} ({})",
                out_dir.display(),
                built.manifest_digest
            ));
            0
        }
        Err(e) => {
            theme.error(
                &format!("couldn't build image `{name}`"),
                &e.to_string(),
                "check that the output directory is writable.",
            );
            2
        }
    }
}

fn looks_like_registry_reference(reference: &str) -> bool {
    let authority = reference.split('/').next().unwrap_or_default();
    authority == "localhost" || authority.contains('.')
}

fn normalize_project_relative_image_path(relative: &str) -> Result<String, String> {
    let path = std::path::Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.bytes().any(|byte| byte.is_ascii_control())
        || path.is_absolute()
    {
        return Err("image files must be safe project-relative paths".to_string());
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => {
                let name = name
                    .to_str()
                    .ok_or_else(|| "image file path must be valid UTF-8".to_string())?;
                components.push(name);
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => {
                return Err("image files must be safe project-relative paths".to_string());
            }
        }
    }
    if components.is_empty() {
        return Err("image files must name a regular project file".to_string());
    }
    let normalized = components.join("/");
    if normalized.len() > 100 {
        return Err("image file path exceeds the OCI tar path limit".to_string());
    }
    Ok(normalized)
}

fn image_layer_path_conflicts(files: &[Image::LayerFile], candidate: &str) -> bool {
    files.iter().any(|file| {
        file.path == candidate
            || candidate.starts_with(&format!("{}/", file.path))
            || file.path.starts_with(&format!("{}/", candidate))
    })
}

fn read_project_image_file(root: &std::path::Path, relative: &str) -> Result<Vec<u8>, String> {
    let relative = normalize_project_relative_image_path(relative)?;
    let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
    let source = root.join(&relative);
    let metadata = std::fs::symlink_metadata(&source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("image file must be a regular file, not a symlink or directory".to_string());
    }
    if image_file_has_multiple_links(&metadata) {
        return Err("image file must not be a hard link to another project path".to_string());
    }
    if metadata.len() > 512 * 1024 * 1024 {
        return Err("image file exceeds the 512 MiB layer limit".to_string());
    }
    let resolved = std::fs::canonicalize(&source).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("image file resolves outside the project root".to_string());
    }
    let data = SHA256::read_file_nofollow(&resolved, 512 * 1024 * 1024)
        .map_err(|error| error.to_string())?;
    if data.len() > 512 * 1024 * 1024 {
        return Err("image file exceeded the 512 MiB layer limit while being read".to_string());
    }
    Ok(data)
}

fn image_file_has_multiple_links(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return metadata.nlink() > 1;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        return metadata.number_of_links() > 1;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = metadata;
        false
    }
}

fn image_service_commands(
    plan: &ModuleEval::EnvPlan,
    image: &ModuleEval::ImagePlan,
    files: &[Image::LayerFile],
) -> Result<(Vec<Vec<String>>, Vec<String>), String> {
    let mut selected = Vec::new();
    let mut names = Vec::new();
    let mut visiting = std::collections::BTreeSet::new();
    for name in &image.services {
        collect_image_service(
            name,
            &plan.dev_services,
            &mut selected,
            &mut names,
            &mut visiting,
        )?;
    }
    let order = crate::Services::dependency_order(&selected)?;
    let mut commands = Vec::new();
    for index in order {
        let service = &selected[index];
        if service.ready.is_some()
            || service.ready_probe.is_some()
            || service.restart.is_some()
            || service.shutdown.is_some()
            || service.data_dir.is_some()
            || !service.watch.is_empty()
            || !service.before_start.is_empty()
            || !service.sockets.is_empty()
            || crate::Services::unknown_field(service).is_some()
        {
            return Err(format!(
                "service {} declares runtime policy that the OCI supervisor cannot carry",
                service.name
            ));
        }
        let mut command = crate::Services::image_run_command(service)?;
        if command
            .iter()
            .any(|argument| argument.bytes().any(|byte| byte.is_ascii_control()))
        {
            return Err(format!(
                "service {} has a command argument with a control character",
                service.name
            ));
        }
        let executable = command
            .first()
            .cloned()
            .ok_or_else(|| format!("service {} has an empty run command", service.name))?;
        let path = if executable == "sh" || executable == "/bin/sh" {
            "/bin/sh".to_string()
        } else if executable.starts_with('/') {
            let path = std::path::Path::new(&executable);
            if path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            }) || !path.starts_with("/usr/local/bin")
            {
                return Err(format!(
                    "service {} has an unsafe executable path",
                    service.name
                ));
            }
            executable.clone()
        } else {
            if executable.is_empty() || executable.contains('/') || executable.contains('\\') {
                return Err(format!(
                    "service {} has an unsafe executable name",
                    service.name
                ));
            }
            format!("/usr/local/bin/{executable}")
        };
        let layer_path = path.strip_prefix('/').unwrap_or(&path);
        if !files.iter().any(|file| file.path == layer_path) {
            return Err(format!(
                "service {} executable {executable} is not a projected package",
                service.name
            ));
        }
        command[0] = path;
        commands.push(command);
    }
    if !commands.is_empty() && !files.iter().any(|file| file.path == "usr/local/bin/sleep") {
        return Err(
            "the OCI supervisor requires a projected `sleep` executable to monitor services"
                .to_string(),
        );
    }
    Ok((commands, names))
}

fn collect_image_service(
    name: &str,
    plans: &[ModuleEval::DevServicePlan],
    selected: &mut Vec<ModuleEval::DevServicePlan>,
    names: &mut Vec<String>,
    visiting: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    if names.iter().any(|selected| selected == name) {
        return Ok(());
    }
    if !visiting.insert(name.to_string()) {
        return Err(format!("service dependency cycle includes `{name}`"));
    }
    let service = plans
        .iter()
        .find(|candidate| candidate.name == name)
        .ok_or_else(|| format!("service {name} is not declared by the environment"))?;
    if !service.enable {
        return Err(format!("service {name} is disabled"));
    }
    for dependency in crate::Services::dependency_names(service) {
        collect_image_service(&dependency, plans, selected, names, visiting)?;
    }
    visiting.remove(name);
    selected.push(service.clone());
    names.push(name.to_string());
    Ok(())
}

fn supervisor_script(commands: &[Vec<String>]) -> String {
    let mut script = String::from(
        "#!/bin/sh\nset -eu\npids=\"\"\ncleanup() {\n  code=$?\n  for pid in $pids; do kill \"$pid\" 2>/dev/null || true; done\n  for pid in $pids; do wait \"$pid\" 2>/dev/null || true; done\n  exit \"$code\"\n}\ntrap cleanup EXIT INT TERM\nstart() {\n  \"$@\" &\n  pids=\"$pids $!\"\n}\n",
    );
    for command in commands {
        script.push_str("start");
        for argument in command {
            script.push(' ');
            script.push_str(&shell_quote(argument));
        }
        script.push('\n');
    }
    script.push_str(
        "while :; do\n  for pid in $pids; do\n    kill -0 \"$pid\" 2>/dev/null || exit 1\n  done\n  /usr/local/bin/sleep 1\ndone\n",
    );
    script
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn environment_image_files(
    theme: &Theme,
    roots: &Store::Roots,
    plan: &ModuleEval::EnvPlan,
    name: &str,
    target: &str,
    projection: &mut Image::ProjectionReport,
) -> Vec<Image::LayerFile> {
    let mut package_refs = plan.package_refs.clone();
    package_refs.sort();
    package_refs.dedup();

    let mut files: Vec<Image::LayerFile> = Vec::new();
    let mut shell_source = None;
    for reference in package_refs {
        let Some(entry) = Store::find_by_reference(roots, &reference) else {
            theme.error_coded(
                "E1336",
                &format!("environment package `{reference}` is not realized"),
                "environment images consume the verified Hangar package output; they do not read a project build scratch file",
                &format!("run `jetpack use {reference} --prep`, then `jet image {name}`"),
            );
            return Vec::new();
        };
        if let Err(error) = verify_realized_hangar_package(roots, &entry) {
            theme.error_coded(
                "E1336",
                &format!("environment package `{reference}` failed Hangar verification"),
                &error,
                &format!("realize a verified executable package for `{reference}`, then run `jet image {name}`"),
            );
            return Vec::new();
        }
        projection
            .included
            .push(format!("hangar:content:{}", entry.envelope.output_hash));
        if !entry.envelope.platform.is_empty() {
            projection
                .included
                .push(format!("hangar:platform:{}", entry.envelope.platform));
        }
        if !hangar_platform_matches(target, &entry.envelope.platform) {
            projection
                .rejected
                .push(format!("package:{reference}:platform"));
            let realized_platform = if entry.envelope.platform.is_empty() {
                "no declared platform"
            } else {
                entry.envelope.platform.as_str()
            };
            theme.error_coded(
                "E1336",
                &format!("environment image `{name}` cannot project package `{reference}`"),
                &format!(
                    "the verified Hangar output platform is `{realized_platform}`, but the image platform is `{target}`"
                ),
                "realize the environment package for the image target, or choose a matching `target`",
            );
            return Vec::new();
        }
        projection.included.push("hangar:provenance".to_string());
        projection.changed.push("hangar:cache".to_string());
        projection.changed.push(format!(
            "hangar:signing:{}",
            if entry.envelope.signature.is_empty() {
                "unsigned"
            } else {
                "signed"
            }
        ));
        let binaries = match read_realized_package_binaries(&entry) {
            Ok(binaries) => binaries,
            Err(error) => {
                theme.error_coded(
                    "E1336",
                    &format!("environment package `{reference}` has no usable binary output"),
                    &error,
                    &format!("realize an executable package for `{reference}`, then run `jet image {name}`"),
                );
                return Vec::new();
            }
        };
        for (binary, data) in &binaries {
            if matches!(binary.as_str(), "bash" | "busybox" | "dash" | "sh")
                && shell_source.is_none()
            {
                shell_source = Some(data.clone());
            }
            let target = format!("usr/local/bin/{binary}");
            if let Some(existing) = files.iter().find(|file| file.path == target) {
                if existing.data != *data {
                    theme.error_coded(
                        "E1336",
                        &format!("environment image `{name}` has conflicting binary `{binary}`"),
                        "two realized package outputs would write different bytes to one image path",
                        "remove the duplicate executable or select package outputs with one stable binary",
                    );
                    return Vec::new();
                }
                continue;
            }
            files.push(Image::LayerFile {
                path: target,
                data: data.clone(),
                mode: 0o755,
            });
        }
    }
    let Some(shell) = shell_source else {
        theme.error_coded(
            "E1336",
            &format!("environment image `{name}` has no shell"),
            "D-ENV-IMAGE1's beginner image is a runnable shell image and cannot copy a host shell or invent one",
            "add `bash`, `busybox`, `dash`, or `sh` to the environment packages and build it first",
        );
        return Vec::new();
    };
    files.push(Image::LayerFile {
        path: "bin/sh".to_string(),
        data: shell,
        mode: 0o755,
    });
    files
}

fn hangar_platform_matches(target: &str, realized: &str) -> bool {
    match target {
        "linux.x64" => matches!(realized, "linux.x64" | "x86_64-linux"),
        "linux.arm64" => matches!(realized, "linux.arm64" | "aarch64-linux"),
        _ => false,
    }
}

fn environment_image_file_rejection(plan: &ModuleEval::EnvPlan, relative: &str) -> Option<String> {
    let normalized = match normalize_project_relative_image_path(relative) {
        Ok(path) => path,
        Err(error) => return Some(error),
    };
    if normalized.split('/').any(|component| {
        let name = component.to_ascii_lowercase();
        name == ".jet"
            || name == ".env"
            || name.starts_with(".env.")
            || ["secret", "credential", "token", "password"]
                .iter()
                .any(|word| name.contains(word))
    }) {
        return Some("the selected path is secret-bearing or Jet state".to_string());
    }
    if plan
        .lifecycle
        .dotenv
        .iter()
        .any(|dotenv| normalized_project_path_matches(&dotenv.file, &normalized))
    {
        return Some(
            "dotenv files are environment inputs and never image-layer inputs".to_string(),
        );
    }
    if plan.files.iter().any(|file| {
        normalized_project_path_matches(&file.destination, &normalized)
            || file
                .source
                .as_deref()
                .map(|source| normalized_project_path_matches(source, &normalized))
                .unwrap_or(false)
    }) || plan.integrations.iter().any(|integration| {
        integration.files.iter().any(|file| {
            normalized_project_path_matches(&file.destination, &normalized)
                || file
                    .source
                    .as_deref()
                    .map(|source| normalized_project_path_matches(source, &normalized))
                    .unwrap_or(false)
        })
    }) {
        return Some(
            "managed environment files are environment inputs and never image-layer inputs"
                .to_string(),
        );
    }
    None
}

fn normalized_project_path_matches(candidate: &str, normalized: &str) -> bool {
    normalize_project_relative_image_path(candidate)
        .map(|candidate| candidate == normalized)
        .unwrap_or(false)
}

fn write_rejected_projection(
    out_dir: &Path,
    projection: &Image::ProjectionReport,
) -> std::io::Result<()> {
    fs::create_dir_all(out_dir)?;
    Image::write_projection_report(out_dir, "", projection)
}

fn verify_realized_hangar_package(
    roots: &Store::Roots,
    entry: &Store::StoreEntry,
) -> Result<(), String> {
    Store::verify_hangar_object(roots, entry).map_err(|error| format!("{error:?}"))?;
    if entry.envelope.output_hash.is_empty() {
        return Err("the Hangar entry has no canonical output digest".to_string());
    }
    let expected = fs::canonicalize(
        roots
            .hangar_dir()
            .join("objects")
            .join(&entry.envelope.output_hash),
    )
    .map_err(|error| format!("the Hangar object cannot be opened: {error}"))?;
    let output_metadata = fs::symlink_metadata(&entry.out)
        .map_err(|error| format!("the Hangar output cannot be opened: {error}"))?;
    if output_metadata.file_type().is_symlink() {
        return Err("the Hangar output is a symlink".to_string());
    }
    let output = fs::canonicalize(&entry.out)
        .map_err(|error| format!("the Hangar output cannot be resolved: {error}"))?;
    if output != expected {
        return Err(
            "the Hangar entry output is not its verified content-addressed object".to_string(),
        );
    }
    let bin_metadata = fs::symlink_metadata(&entry.bin)
        .map_err(|error| format!("the Hangar bin projection cannot be opened: {error}"))?;
    if bin_metadata.file_type().is_symlink() {
        return Err("the Hangar bin projection is a symlink".to_string());
    }
    let bin = fs::canonicalize(&entry.bin)
        .map_err(|error| format!("the Hangar bin projection cannot be resolved: {error}"))?;
    if !bin.starts_with(&output) {
        return Err("the Hangar bin projection escapes its verified output".to_string());
    }
    Ok(())
}

fn read_realized_package_binaries(
    entry: &Store::StoreEntry,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if entry.bin.is_empty() {
        return Err("the Hangar entry has no executable bin directory".to_string());
    }
    let bin = Path::new(&entry.bin);
    let metadata = fs::symlink_metadata(bin).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("the Hangar bin projection is not a real directory".to_string());
    }
    let root = fs::canonicalize(bin).map_err(|error| error.to_string())?;
    let mut paths = fs::read_dir(&root)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    paths.sort_by_key(|entry| entry.file_name());
    let mut binaries = Vec::new();
    for item in paths {
        let path = item.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("the Hangar bin projection contains a non-regular entry".to_string());
        }
        let resolved = fs::canonicalize(&path).map_err(|error| error.to_string())?;
        if !resolved.starts_with(&root) {
            return Err("the Hangar bin projection escapes its package output".to_string());
        }
        if metadata.len() > 512 * 1024 * 1024 {
            return Err("a realized package binary exceeds the 512 MiB layer limit".to_string());
        }
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && !value.bytes().any(|byte| byte.is_ascii_control()))
            .ok_or_else(|| "a realized package binary has an unsafe name".to_string())?
            .to_string();
        binaries.push((name, fs::read(resolved).map_err(|error| error.to_string())?));
    }
    if binaries.is_empty() {
        return Err("the Hangar bin projection is empty".to_string());
    }
    Ok(binaries)
}
