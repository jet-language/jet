fn cmd_check(theme: &Theme, args: &[String]) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    match load_target(theme, &target) {
        Some((_, system)) => {
            theme.ok(&format!(
                "jetos {} checked ({})",
                theme.bold(&system.name),
                system.target
            ));
            0
        }
        None => 2,
    }
}

fn cmd_plan(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((_, system)) = load_target(theme, &target) else {
        return 2;
    };
    let plan = render_plan_json(&system, &[], None);
    if flags.json {
        println!("{plan}");
    } else {
        theme.ok(&format!("jetos plan for {}", theme.bold(&system.name)));
        println!("{plan}");
    }
    0
}

fn cmd_proof(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    if load_target(theme, &target).is_none() {
        return 2;
    }
    let Some(gen) = latest_generation_for(&target.host) else {
        theme.error_coded(
            "E1278",
            "jetos proof is missing",
            &format!(
                "no built generation exists for `{}`; proof is read from generation artifacts.",
                target.host
            ),
            "run `jet os build <host>` first.",
        );
        return 2;
    };
    match render_generation_proof_json(&gen) {
        Ok(proof) => {
            if flags.json {
                println!("{proof}");
            } else {
                theme.ok(&format!(
                    "jetos proof for {} generation {}",
                    theme.bold(&gen.host),
                    theme.bold(&gen.name)
                ));
                println!("{proof}");
            }
            0
        }
        Err(e) => {
            theme.error_coded(
                "E1278",
                "jetos proof is incomplete",
                &format!("reading proof artifacts for `{}` failed: {e}", gen.name),
                "run `jet os build <host>` again so plan, proof, provenance, health, and rollback facts are regenerated.",
            );
            2
        }
    }
}

fn cmd_build(theme: &Theme, args: &[String], flags: &OsFlags, activate: bool) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    match build_generation(theme, &plan, &system, flags) {
        Some(gen) => {
            theme.ok(&format!(
                "jetos generation {} built for {}",
                theme.bold(&gen.name),
                theme.bold(&system.name)
            ));
            theme.detail(&gen.path.display().to_string());
            if activate {
                if !prove_activation(theme, &gen, &system) {
                    return 2;
                }
                match activate_generation(&gen) {
                    Ok(()) => theme.ok(&format!(
                        "jetos generation {} activated",
                        theme.bold(&gen.name)
                    )),
                    Err(e) => {
                        theme.error(
                            "could not activate the generation",
                            &format!("updating the current/default pointers failed: {e}"),
                            "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                        );
                        return 2;
                    }
                }
            }
            0
        }
        None => 2,
    }
}

fn cmd_rollback(theme: &Theme, args: &[String]) -> i32 {
    let host = args.first().map_or("", String::as_str);
    if host.is_empty() {
        theme.error(
            "rollback needs a host",
            "`jet os rollback` activates a recorded generation for one host.",
            "run `jet os generations`, then `jet os rollback <host> [<name>]`.",
        );
        return 2;
    }
    let requested = args.get(1).map(String::as_str);
    let Some(gen) = find_rollback_generation(host, requested) else {
        theme.error(
            "no generation is available for rollback",
            &format!("no recorded jetos generation matches host `{host}`."),
            "run `jet os build <host>` or `jet os generations`.",
        );
        return 2;
    };
    match activate_generation(&gen) {
        Ok(()) => {
            theme.ok(&format!(
                "rolled back {} to {}",
                host,
                theme.bold(&gen.name)
            ));
            0
        }
        Err(e) => {
            theme.error(
                "could not activate the rollback generation",
                &format!("updating the current/default pointers failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}

fn cmd_generations(args: &[String]) -> i32 {
    let host = args.first().map(String::as_str);
    let mut gens = read_generations();
    if let Some(host) = host {
        gens.retain(|g| g.host == host);
    }
    gens.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.name.cmp(&a.name))
    });
    for gen in gens {
        println!(
            "{}\t{}\t{}\t{}",
            gen.created_at,
            gen.host,
            gen.name,
            gen.path.display()
        );
    }
    0
}

fn cmd_init(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let host = args.first().map_or("host", String::as_str);
    let path = default_config_path();
    if path.exists() {
        theme.error(
            "config.jet already exists",
            "`jet os init` never overwrites a repo's OS config.",
            "edit config.jet, or move it aside and run init again.",
        );
        return 2;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            theme.error(
                "could not create the jetos config directory",
                &format!("creating `{}` failed: {e}", parent.display()),
                "check permissions, or create the directory yourself.",
            );
            return 2;
        }
    }
    let disk = flags
        .manual_disk
        .as_deref()
        .map(|d| {
            format!(
                "            filesystem.root.device: \"{}\",\n",
                d.replace('"', "\\\"")
            )
        })
        .unwrap_or_else(|| "            filesystem.layout: \"guided-ext4\",\n".to_string());
    let contents = format!(
        "module {host} {{\n    system.{host}: {{\n        target: linux.x64,\n        packages: [],\n        services: {{\n            systemd: {{ enable: true, exec: \"/usr/bin/env true\" }},\n        }},\n        options: [\n{disk}            network.hostName: {host},\n            packages.base: true,\n        ],\n    }}\n}}\n"
    );
    match fs::write(&path, contents) {
        Ok(()) => {
            theme.ok(&format!("wrote jetos config {}", path.display()));
            theme.detail("generated systemd-ready system skeleton");
            0
        }
        Err(e) => {
            theme.error(
                "could not write the jetos config",
                &format!("writing `{}` failed: {e}", path.display()),
                "check permissions, or pass a writable HOME.",
            );
            2
        }
    }
}

fn cmd_lift(theme: &Theme, args: &[String]) -> i32 {
    let host = args.first().map_or("host", String::as_str);
    let root = args.get(1).map_or("/", String::as_str);
    println!(
        "module {host} {{\n    system.{host}: {{\n        target: linux.x64,\n        packages: [],\n        options: [\n            filesystem.root.source: \"{root}\",\n        ],\n    }}\n}}"
    );
    theme.status("drafted jetos config from host root");
    0
}

fn cmd_image(theme: &Theme, args: &[String], flags: &OsFlags) -> i32 {
    let Some(target) = parse_target_or_report(theme, args.first().map(String::as_str)) else {
        return 2;
    };
    let Some((plan, system)) = load_target(theme, &target) else {
        return 2;
    };
    let Some(gen) = build_generation(theme, &plan, &system, flags) else {
        return 2;
    };
    let disk = flags.manual_disk.as_deref().unwrap_or("guided-ext4");
    match write_installer_media(&gen, &system, disk) {
        Ok(path) => match write_image_variant_artifacts(&gen, &system) {
            Ok(variant_proof) => {
                theme.detail(&format!(
                    "wrote image variant proof {}",
                    variant_proof.display()
                ));
                theme.ok(&format!(
                    "wrote jetos installer media proof {}",
                    path.display()
                ));
                0
            }
            Err(e) => {
                theme.error(
                    "could not write jetos image variants",
                    &format!("writing image variant artifacts failed: {e}"),
                    "check permissions on the Jetpack root, or set JETPACK_ROOT.",
                );
                2
            }
        },
        Err(e) => {
            theme.error(
                "could not write the jetos installer media",
                &format!("writing installer media artifacts failed: {e}"),
                "check permissions on the Jetpack root, or set JETPACK_ROOT.",
            );
            2
        }
    }
}
