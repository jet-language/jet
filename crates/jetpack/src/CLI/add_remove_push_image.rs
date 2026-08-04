use super::parse::Parsed;
use super::realize::{
    channel_sources, load_project_plan, offline_refusal, realize_ref_outcome,
    report_nix_bridge_required, resolve_source_channel, RefOutcome, RowStyle,
};
use super::workspace_sources::cwd_table;
use crate::Output::{self, Theme};
use crate::RefSpec;
use crate::Store;
use crate::{Components, EnvFile, Image, Lock, Syntax};
use jet_env_model::ModuleEval;
use std::fs;
use std::io::Read;
use std::path::Path;

enum ImagePushDestination {
    Local(std::path::PathBuf),
    Registry(String),
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
    ) {
        RefOutcome::Realized(_entry, _state, _line, lease) => lease,
        RefOutcome::NeedsNix(need) => {
            report_nix_bridge_required(theme, &parsed.flags, &[need], &[]);
            return 2;
        }
        RefOutcome::Failed => return 1,
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
                                channel: source.channel.as_str().to_string(),
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
            RefSpec::Source::Nixpkgs => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "`<pkg>@nixpkgs` names a package in an index, not an upstream source tree.",
                    "use the package's source URL with `source: owner/repo#rev@github` or `source: \"./vendor/pkg\"`.",
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

    let Some(image) = plan.images.iter().find(|i| &i.name == name) else {
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

    let push_destination = match parsed.flags.push.as_deref() {
        Some(push_ref)
            if push_ref.starts_with("https://") || push_ref.starts_with("http://") =>
        {
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
        Some(push_ref) => Some(ImagePushDestination::Local(if let Some(path) =
            push_ref.strip_prefix("file://")
        {
            std::path::PathBuf::from(path)
        } else {
            std::path::PathBuf::from(push_ref)
        })),
        None => None,
    };

    let base_directory = match image.base.as_deref() {
        Some(reference) if reference.starts_with("https://") || reference.starts_with("http://") => {
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
                    || relative.components().any(|component| {
                        component == std::path::Component::ParentDir
                    })
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
    if !image.services.is_empty() {
        projection.rejected.push("services".to_string());
        if let Err(error) = write_rejected_projection(&out_dir, &projection) {
            theme.error(
                &format!("couldn't write image {name} rejection projection"),
                &error.to_string(),
                "check that the image output directory is writable.",
            );
        }
        theme.error_coded(
            "E1336",
            &format!("environment image {name} cannot project services"),
            "the image path has no typed service supervisor; starting shell PIDs would bypass readiness, restart, cancellation, and cleanup policy",
            "run the declared services with `jetpack services`, or build an image without `services:` until the one supervisor owns the image runtime",
        );
        return 2;
    }
    let mut files = if image.from_environment {
        environment_image_files(theme, &roots, &plan, name)
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
        projection
            .included
            .extend(plan.package_refs.iter().map(|value| format!("package:{value}")));
        projection.included.push("environment:shell".to_string());
        projection.changed.push("from:environment".to_string());
        if !plan.secrets.is_empty() {
            projection.omitted.push("environment.secrets".to_string());
        }
        if !plan.files.is_empty() {
            projection.omitted.push("environment.managed-files".to_string());
        }
        if !plan.integrations.is_empty() {
            projection.omitted.push("environment.integrations".to_string());
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
        if !plan.profiles.is_empty() {
            projection.omitted.push("environment.profiles".to_string());
        }
    } else {
        projection
            .included
            .push(format!("package:{}", image.from));
    }
    for (key, _) in &image.env_vars {
        projection.changed.push(format!("env:{key}"));
    }
    if !image.expose.is_empty() {
        projection.changed.push("expose".to_string());
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
        projection.rejected.push("environment.package-output".to_string());
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
        let data = match read_project_image_file(&dir, rel) {
            Ok(data) => data,
            Err(error) => {
                projection.rejected.push(format!("file:{rel}"));
                let _ = write_rejected_projection(&out_dir, &projection);
                theme.error(
                    &format!("image file {rel} cannot be projected"),
                    &error,
                    "use a regular project-relative file that stays inside the project root.",
                );
                return 2;
            }
        };
        files.push(Image::LayerFile {
            path: rel.to_string(),
            data,
            mode: 0o644,
        });
        projection.included.push(format!("file:{rel}"));
    }

    let spec = Image::BuildSpec {
        files,
        entrypoint: vec![image.entrypoint.clone().unwrap_or_else(|| {
            if image.from_environment {
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
    match Image::build_with_base(&spec, &out_dir, name, base_directory.as_deref()) {
        Ok(built) => {
            if let Err(error) =
                Image::write_projection_report(&out_dir, &built.manifest_digest, &projection)
            {
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

fn read_project_image_file(root: &std::path::Path, relative: &str) -> Result<Vec<u8>, String> {
    let path = std::path::Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.bytes().any(|byte| byte.is_ascii_control())
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("image files must be safe project-relative paths".to_string());
    }
    let root = std::fs::canonicalize(root).map_err(|error| error.to_string())?;
    let source = root.join(path);
    let metadata = std::fs::symlink_metadata(&source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("image file must be a regular file, not a symlink or directory".to_string());
    }
    if metadata.len() > 512 * 1024 * 1024 {
        return Err("image file exceeds the 512 MiB layer limit".to_string());
    }
    let resolved = std::fs::canonicalize(&source).map_err(|error| error.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("image file resolves outside the project root".to_string());
    }
    let file = std::fs::File::open(resolved).map_err(|error| error.to_string())?;
    let mut data = Vec::new();
    file.take(512 * 1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(|error| error.to_string())?;
    if data.len() > 512 * 1024 * 1024 {
        return Err("image file exceeded the 512 MiB layer limit while being read".to_string());
    }
    Ok(data)
}

fn environment_image_files(
    theme: &Theme,
    roots: &Store::Roots,
    plan: &ModuleEval::EnvPlan,
    name: &str,
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
                &format!("run `jetpack build {reference}`, then `jet image {name}`"),
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
        return Err("the Hangar entry output is not its verified content-addressed object".to_string());
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

fn read_realized_package_binaries(entry: &Store::StoreEntry) -> Result<Vec<(String, Vec<u8>)>, String> {
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
