/// `jetpack add <ref>` — edit the project env file. `jetpack add <Component>`
/// (an exact, case-sensitive match against the starter component catalog —
/// Button/Label/Input/Container) is a distinct behavior checked first: it
/// copies real `.jet` source into `./components/` instead of touching the env
/// file (Tower c134 Phase 4, the ownable component kit). The two never
/// collide because Jetpack source names are always lowercase
/// (`nixpkgs`/`github`/`path`/user-declared names), so an exact-case
/// `Button`-style name can only ever mean a component.
fn cmd_add(theme: &Theme, parsed: &Parsed) -> i32 {
    if parsed.flags.offline {
        return offline_refusal(theme, "add");
    }
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "add what?",
            "`jetpack add` needs a ref or a starter component to add.",
            "try `jetpack add nixpkgs:ripgrep` or `jetpack add Button`.",
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
    // Classify against the env's declared sources so `add unstable:fd` works
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
            RefSpec::Source::Path => format!("path@{}", spec.package),
            RefSpec::Source::Github => format!("github@{}", spec.package),
            RefSpec::Source::Named(name) => match table.upstream(&name) {
                Some(upstream) if upstream.starts_with("path:") => {
                    format!("path@{}", upstream.trim_start_matches("path:"))
                }
                Some(upstream) if upstream.starts_with("github:") => {
                    format!("github@{}", upstream.trim_start_matches("github:"))
                }
                _ => {
                    theme.error_coded(
                        "E1270",
                        "adapter draft needs source bytes",
                        "that named source does not point at a path or GitHub source tree.",
                        "write `Pkg.adapt(...)` by hand with `source: path@...`.",
                    );
                    return 2;
                }
            },
            RefSpec::Source::Nixpkgs => {
                theme.error_coded(
                    "E1270",
                    "adapter draft needs source bytes",
                    "`nixpkgs:<pkg>` names a package in an index, not an upstream source tree.",
                    "use the package's source URL with `source: github@owner/repo#rev` or `source: path@vendor/pkg`.",
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
        .trim_end_matches(".git");
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
fn cmd_remove(theme: &Theme, parsed: &Parsed) -> i32 {
    let Some(raw) = parsed.positional.first() else {
        theme.error(
            "remove what?",
            "`jetpack remove` needs a ref to remove.",
            "try `jetpack remove nixpkgs:ripgrep`.",
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
fn cmd_push(theme: &Theme, parsed: &Parsed) -> i32 {
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

/// `jetpack bridge flake` (U16, card c9jetpackgates) — best-effort translator
/// from a foreign `flake.nix`'s devShell into jetpack's own `env.*` module
/// form. Never edits the project's `env.jet`; the shim prints to stdout for
/// the user to review and merge (I8 — one canonical env surface).
/// D-JPK-IMAGE1 (=A, ratified 2026-07-01, c9jetpackgates): `jet image <name>`
/// builds the named `.Oci` `image.<name>` module contribution into a native
/// OCI layout (`Jetpack::Image`). `.Iso` images ride the jetos installer tier
/// (Phase D, owner-gated — untouched here); `--push` is honestly gated on TLS
/// (E1268), never a fake push.
fn cmd_image(theme: &Theme, parsed: &Parsed) -> i32 {
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

    if let Some(push_ref) = &parsed.flags.push {
        theme.error_coded(
            "E1268",
            &format!("`jet image {name}` can't push to `{push_ref}` yet"),
            "D-JPK-IMAGE1: pushing to a registry needs TLS support for the connection, which jetpack doesn't have yet — `jet image` never fakes a push.",
            &format!(
                "build without `--push` (`jet image {name}`) and push the OCI layout with another tool for now; `--push` will work once TLS lands."
            ),
        );
        return 2;
    }

    if let Some(base) = &image.base {
        theme.error(
            &format!("`base: oci(\"{base}\")` isn't realized yet"),
            "D-JPK-IMAGE1: layering onto a base image needs a native registry-pull client, which doesn't exist yet — `jet image` never silently builds from scratch instead of the requested base.",
            "drop `base:` to build a from-scratch image, or track the registry-pull client.",
        );
        return 2;
    }

    // D-JPK-IMAGE1: build from what `jet build` already realized. Jetpack has
    // no dependency on the compiler's own build machinery (the dependency
    // runs the other way — `jet` depends on `jet-driver`, not vice versa), so
    // this mirrors, rather than calls into, `jet build`'s `build/<name>`
    // output convention (`Source/CmdCompile.rs::bin_path`).
    let bin_path = dir.join("build").join(&image.from);
    let Ok(bin_data) = std::fs::read(&bin_path) else {
        theme.error(
            &format!("`{}` isn't built yet", image.from),
            &format!(
                "`jet image {name}` needs `{}` already built at `{}`.",
                image.from,
                bin_path.display()
            ),
            &format!("run `jet build` first, then `jet image {name}`."),
        );
        return 2;
    };

    let mut files = vec![Image::LayerFile {
        path: format!("usr/local/bin/{}", image.from),
        data: bin_data,
        mode: 0o755,
    }];
    for rel in &image.files {
        let Ok(data) = std::fs::read(dir.join(rel)) else {
            theme.error(
                &format!("`{rel}` (from `files:`) doesn't exist"),
                &format!("`image.{name}`'s `files:` names `{rel}`, relative to the project dir."),
                "fix the path, or remove it from `files:`.",
            );
            return 2;
        };
        files.push(Image::LayerFile {
            path: rel.trim_start_matches('/').to_string(),
            data,
            mode: 0o644,
        });
    }

    let spec = Image::BuildSpec {
        files,
        entrypoint: vec![format!("/usr/local/bin/{}", image.from)],
        env: image.env_vars.clone(),
        expose: image.expose.clone(),
    };
    let out_dir = dir.join(".jet").join("images").join(name);
    match Image::build(&spec, &out_dir, name) {
        Ok(built) => {
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
