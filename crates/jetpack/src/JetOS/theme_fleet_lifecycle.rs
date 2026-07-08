fn write_theme_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let theme_dir = dir.join("theme");
    let gtk_dir = dir.join("share/themes/jetos/gtk-4.0");
    let qt_dir = dir.join("share/qt6ct/colors");
    let terminal_dir = dir.join("share/terminal");
    let editor_dir = dir.join("share/editor");
    let dm_dir = dir.join("share/display-manager");
    let studio_dir = dir.join("studio");
    fs::create_dir_all(&theme_dir)?;
    fs::create_dir_all(&gtk_dir)?;
    fs::create_dir_all(&qt_dir)?;
    fs::create_dir_all(&terminal_dir)?;
    fs::create_dir_all(&editor_dir)?;
    fs::create_dir_all(&dm_dir)?;
    fs::create_dir_all(&studio_dir)?;
    let name = option_value(system, &["theme.name", "theme.profile"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "default".to_string());
    let polarity = option_value(system, &["theme.polarity"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "Dark".to_string());
    let wallpaper = option_value(system, &["theme.wallpaper"]).unwrap_or_default();
    let font = option_value(system, &["theme.fonts.ui", "theme.font"])
        .unwrap_or_else(|| "Inter".to_string());
    let accent =
        option_value(system, &["theme.palette.accent"]).unwrap_or_else(|| "#4f8cff".to_string());
    fs::write(
        gtk_dir.join("gtk.css"),
        format!("* {{ font-family: \"{font}\"; }}\n:root {{ --jetos-accent: {accent}; }}\n"),
    )?;
    fs::write(
        qt_dir.join("jetos.conf"),
        format!("[ColorScheme]\nname={name}\naccent={accent}\npolarity={polarity}\nfont={font}\n"),
    )?;
    fs::write(
        terminal_dir.join("theme.toml"),
        format!("name = \"{name}\"\npolarity = \"{polarity}\"\nfont = \"{font}\"\naccent = \"{accent}\"\n"),
    )?;
    fs::write(
        editor_dir.join("theme.json"),
        format!(
            "{{\"name\":{},\"type\":{},\"ui_font\":{},\"accent\":{}}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&font),
            JSON::quote(&accent)
        ),
    )?;
    fs::write(
        dm_dir.join("theme.conf"),
        format!("Theme={name}\nAccent={accent}\nWallpaper={wallpaper}\n"),
    )?;
    fs::write(
        studio_dir.join("theme-preview.json"),
        format!(
            "{{\"kind\":\"jetos.theme-preview\",\"name\":{},\"polarity\":{},\"accent\":{},\"font\":{},\"wallpaper\":{}}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&accent),
            JSON::quote(&font),
            JSON::quote(&wallpaper)
        ),
    )?;
    fs::write(
        theme_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.theme\",\"name\":{},\"polarity\":{},\"wallpaper\":{},\"font\":{},\"accent\":{},\"targets\":[\"gtk\",\"qt\",\"terminal\",\"editor\",\"display-manager\",\"studio\"],\"proof\":\"theme-projected\"}}",
            JSON::quote(&name),
            JSON::quote(&polarity),
            JSON::quote(&wallpaper),
            JSON::quote(&font),
            JSON::quote(&accent)
        ),
    )
}

fn write_fleet_deploy_facts(
    dir: &Path,
    system: &SystemPlan,
    plan: &EnvPlan,
) -> std::io::Result<()> {
    let fleet_dir = dir.join("fleet");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&fleet_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut hosts = Vec::new();
    let mut host_names = Vec::new();
    for fleet in &plan.fleets {
        for host in &fleet.hosts {
            if host.system != system.name {
                continue;
            }
            let target = option_value(system, &[&format!("deploy.host.{}.target", host.name)])
                .unwrap_or_else(|| format!("{}@{}", host.name, host.name));
            let health = option_value(system, &[&format!("deploy.host.{}.health", host.name)])
                .unwrap_or_else(|| "system-health".to_string());
            let generation_name = "${generation}";
            let push = option_value(system, &[&format!("deploy.host.{}.pushCommand", host.name)])
                .unwrap_or_else(|| {
                    format!(
                        "tar -C \"$root\" -cf - . | ssh {} \"mkdir -p ~/.jetos/generations/{generation_name} && tar -C ~/.jetos/generations/{generation_name} -xf -\"",
                        target
                    )
                });
            let proof = option_value(system, &[&format!("deploy.host.{}.proofCommand", host.name)])
                .unwrap_or_else(|| {
                    format!(
                        "ssh {} \"test -f ~/.jetos/generations/{generation_name}/proof.txt && test -f ~/.jetos/generations/{generation_name}/activation-diff.txt\"",
                        target
                    )
                });
            let switch = option_value(
                system,
                &[&format!("deploy.host.{}.switchCommand", host.name)],
            )
            .unwrap_or_else(|| {
                format!(
                    "ssh {} \"ln -sfn ~/.jetos/generations/{generation_name} ~/.jetos/current\"",
                    target
                )
            });
            let health_cmd = option_value(
                system,
                &[&format!("deploy.host.{}.healthCommand", host.name)],
            )
            .unwrap_or_else(|| {
                format!(
                    "ssh {} \"test -f ~/.jetos/current/health-checks.txt\"",
                    target
                )
            });
            let rollback =
                option_value(system, &[&format!("deploy.host.{}.rollbackCommand", host.name)])
                    .unwrap_or_else(|| {
                        format!(
                            "ssh {} \"test -L ~/.jetos/previous && ln -sfn $(readlink ~/.jetos/previous) ~/.jetos/current || true\"",
                            target
                        )
                    });
            host_names.push(host.name.clone());
            let script_path = fleet_dir.join(format!("deploy-{}.sh", host.name));
            fs::write(
                &script_path,
                render_fleet_host_script(
                    &fleet.name,
                    &host.name,
                    &host.system,
                    &target,
                    &push,
                    &proof,
                    &switch,
                    &health_cmd,
                    &rollback,
                ),
            )?;
            make_executable(&script_path)?;
            hosts.push(JSON::object_of(&[
                ("fleet", &fleet.name),
                ("host", &host.name),
                ("system", &host.system),
                ("target", &target),
                ("policy", "staged-proof-gated-rollback-stop"),
                ("health", &health),
                ("script", &format!("fleet/deploy-{}.sh", host.name)),
            ]));
        }
    }
    fs::write(
        fleet_dir.join("deploy-plan.json"),
        format!(
            "{{\"kind\":\"jetos.fleet-deploy\",\"host\":{},\"hosts\":[{}],\"proofs\":[\"build-closure\",\"ssh-push\",\"remote-proof-before-switch\",\"health-window\",\"rollback-on-fail\"]}}",
            JSON::quote(&system.name),
            hosts.join(",")
        ),
    )?;
    let default_host = host_names.first().cloned().unwrap_or_default();
    let launcher = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nhost=${{1:-{}}}\nif [ -z \"$host\" ]; then\n  echo 'usage: jetos-fleet-deploy <host>' >&2\n  exit 2\nfi\nscript=\"$root/fleet/deploy-$host.sh\"\nif [ ! -x \"$script\" ]; then\n  echo \"jetos fleet deploy: unknown host $host\" >&2\n  exit 2\nfi\nexec /bin/sh \"$script\"\n",
        shell_single_quote(&default_host)
    );
    let launcher_path = bin_dir.join("jetos-fleet-deploy");
    fs::write(&launcher_path, launcher)?;
    make_executable(&launcher_path)
}

fn render_fleet_host_script(
    fleet: &str,
    host: &str,
    system: &str,
    target: &str,
    push: &str,
    proof: &str,
    switch_cmd: &str,
    health: &str,
    rollback: &str,
) -> String {
    format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ngeneration=$(cat \"$root/generation.txt\" 2>/dev/null || basename \"$root\")\nproof_dir=${{JETOS_DEPLOY_PROOF_DIR:-$root/fleet/proofs}}\nmkdir -p \"$proof_dir\"\nproof_file=\"$proof_dir/{fleet}-{host}.json\"\npush_cmd={push}\nproof_cmd={proof}\nswitch_cmd={switch_cmd}\nhealth_cmd={health}\nrollback_cmd={rollback}\nrun_step() {{\n  name=$1\n  cmd=$2\n  printf '%s\\n' \"jetos deploy {host}: $name\"\n  sh -c \"$cmd\"\n}}\nif [ \"${{JETOS_FLEET_DRY_RUN:-}}\" = \"1\" ]; then\n  printf '{{\"state\":\"dry-run\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 0\nfi\nrun_step push \"$push_cmd\"\nrun_step proof \"$proof_cmd\"\nif [ \"${{JETOS_FLEET_STAGE_ONLY:-}}\" = \"1\" ]; then\n  printf '{{\"state\":\"staged\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 0\nfi\nrun_step switch \"$switch_cmd\"\nif ! sh -c \"$health_cmd\"; then\n  sh -c \"$rollback_cmd\" || true\n  printf '{{\"state\":\"rolled-back\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\"}}\\n' \"$generation\" > \"$proof_file\"\n  cat \"$proof_file\"\n  exit 2\nfi\nprintf '{{\"state\":\"deployed\",\"fleet\":{fleet_json},\"host\":{host_json},\"system\":{system_json},\"target\":{target_json},\"generation\":\"%s\",\"proofs\":[\"push\",\"remote-proof-before-switch\",\"health-window\"]}}\\n' \"$generation\" > \"$proof_file\"\ncat \"$proof_file\"\n",
        fleet_json = JSON::quote(fleet),
        host_json = JSON::quote(host),
        system_json = JSON::quote(system),
        target_json = JSON::quote(target),
        push = shell_single_quote(push),
        proof = shell_single_quote(proof),
        switch_cmd = shell_single_quote(switch_cmd),
        health = shell_single_quote(health),
        rollback = shell_single_quote(rollback)
    )
}

fn write_options_reference(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let options_dir = dir.join("options");
    fs::create_dir_all(&options_dir)?;
    let mut keys = system
        .options
        .iter()
        .filter(|o| !is_option_priority_metadata(&o.key))
        .map(|o| o.key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    let rows = keys
        .iter()
        .filter_map(|key| {
            let resolved = resolved_option(system, key)?;
            let ns = key.split('.').next().unwrap_or("");
            Some(format!(
                "{{\"key\":{},\"namespace\":{},\"type\":{},\"value\":{},\"default\":{},\"example\":{},\"doc\":{},\"source\":\"config.jet options\",\"tier\":{},\"priority\":\"{}\",\"provenance\":\"system option resolver\"}}",
                JSON::quote(key),
                JSON::quote(ns),
                JSON::quote(&option_type(&resolved.value)),
                JSON::quote(&resolved.value),
                JSON::quote(&option_default(ns)),
                JSON::quote(&resolved.value),
                JSON::quote(&option_doc(key)),
                JSON::quote(&resolved.tier),
                resolved.priority
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(
        options_dir.join("reference.json"),
        format!(
            "{{\"kind\":\"jetos.option-reference\",\"host\":{},\"options\":[{}]}}",
            JSON::quote(&system.name),
            rows
        ),
    )?;
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    let search = "#!/usr/bin/env sh\nset -eu\nmode=search\ncase \"${1:-}\" in\n  --exact) mode=exact; shift ;;\n  --explain) mode=explain; shift ;;\nesac\nterm=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nref=\"$root/options/reference.json\"\nexplain=\"$root/module-system/explain.json\"\nif [ -z \"$term\" ]; then\n  cat \"$ref\"\n  exit 0\nfi\ncase \"$mode\" in\n  exact) grep -F \"\\\"key\\\": \\\"$term\\\"\" \"$ref\" || grep -F \"\\\"key\\\":\\\"$term\\\"\" \"$ref\" || true ;;\n  explain) grep -F \"$term\" \"$explain\" || true ;;\n  *) grep -F \"$term\" \"$ref\" || true ;;\nesac\n";
    let search_path = bin_dir.join("jetos-options-search");
    fs::write(&search_path, search)?;
    make_executable(&search_path)
}

fn write_image_variant_facts(
    dir: &Path,
    system: &SystemPlan,
    plan: &EnvPlan,
) -> std::io::Result<()> {
    let image_dir = dir.join("image-variants");
    fs::create_dir_all(&image_dir)?;
    let mut variants = vec![
        JSON::object_of(&[
            ("name", "default-qcow2"),
            ("kind", "qcow2"),
            ("format", "qcow2"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-raw"),
            ("kind", "raw"),
            ("format", "raw"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-sd"),
            ("kind", "sd"),
            ("format", "sd"),
            ("target", &system.target),
        ]),
        JSON::object_of(&[
            ("name", "default-netboot"),
            ("kind", "netboot"),
            ("format", "pxe"),
            ("target", &system.target),
        ]),
    ];
    for image in &plan.images {
        if image.kind == ImageKind::Iso && image.from == system.name {
            variants.push(JSON::object_of(&[
                ("name", &image.name),
                ("kind", "iso"),
                ("format", &image.format),
                ("target", image.target.as_deref().unwrap_or(&system.target)),
            ]));
        }
    }
    for (key, value) in prefixed_options(system, "packages.imageVariant.") {
        variants.push(JSON::object_of(&[
            ("name", &key),
            ("kind", &clean_symbol(&value)),
            ("format", &clean_symbol(&value)),
            ("target", &system.target),
        ]));
    }
    fs::write(
        image_dir.join("matrix.json"),
        format!(
            "{{\"kind\":\"jetos.image-variant-matrix\",\"host\":{},\"variants\":[{}],\"proof\":\"image-variant-plan-ready\"}}",
            JSON::quote(&system.name),
            variants.join(",")
        ),
    )
}

fn write_lifecycle_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let lifecycle_dir = dir.join("lifecycle");
    let bin_dir = dir.join("sw/bin");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&lifecycle_dir)?;
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let keep = option_value(system, &["packages.generations.keep"])
        .or_else(|| option_value(system, &["deploy.generations.keep"]))
        .unwrap_or_else(|| "10".to_string());
    let auto_upgrade =
        option_value(system, &["deploy.autoUpgrade.enable"]).unwrap_or_else(|| "false".to_string());
    let channel =
        option_value(system, &["packages.channel"]).unwrap_or_else(|| "locked".to_string());
    let schedule = option_value(system, &["deploy.autoUpgrade.schedule"])
        .unwrap_or_else(|| "daily".to_string());
    fs::write(
        lifecycle_dir.join("policy.json"),
        format!(
            "{{\"kind\":\"jetos.lifecycle-policy\",\"keep_generations\":{},\"channel\":{},\"auto_upgrade\":{},\"schedule\":{},\"gc\":\"explain-before-delete\",\"rollback_window\":\"kept-generations\",\"proof\":\"lifecycle-policy-ready\"}}",
            JSON::quote(&keep),
            JSON::quote(&channel),
            clean_bool_json(&auto_upgrade),
            JSON::quote(&schedule)
        ),
    )?;
    fs::write(
        lifecycle_dir.join("channel.json"),
        format!(
            "{{\"kind\":\"jetos.channel-policy\",\"channel\":{},\"update_command\":\"sw/bin/jetos-channel-update\",\"proof\":\"channel-policy-ready\"}}",
            JSON::quote(&channel)
        ),
    )?;
    fs::write(
        lifecycle_dir.join("auto-upgrade.json"),
        format!(
            "{{\"kind\":\"jetos.auto-upgrade\",\"enabled\":{},\"schedule\":{},\"steps\":[\"fetch-channel\",\"build\",\"proof\",\"switch\",\"health\",\"rollback-on-fail\"],\"proof\":\"auto-upgrade-proof-gated\"}}",
            clean_bool_json(&auto_upgrade),
            JSON::quote(&schedule)
        ),
    )?;
    let gc = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nsystems=${{JETOS_SYSTEMS_DIR:-${{JETPACK_ROOT:-$HOME/.jetpack}}/systems}}\nlog=\"$systems/generations.log\"\nkeep={}\nhost={}\napply=false\nif [ \"${{1:-}}\" = \"--apply\" ]; then apply=true; fi\nmkdir -p \"$root/lifecycle\"\nout=\"$root/lifecycle/gc-plan.txt\"\n: > \"$out\"\nif [ ! -f \"$log\" ]; then\n  echo 'no generations log' | tee -a \"$out\"\n  exit 0\nfi\ncount=0\nsort -r \"$log\" | while IFS='	' read -r created entry_host name path; do\n  [ \"$entry_host\" = \"$host\" ] || continue\n  count=$((count + 1))\n  if [ \"$count\" -le \"$keep\" ]; then\n    printf 'keep\\t%s\\t%s\\t%s\\treason=within-retention\\n' \"$created\" \"$name\" \"$path\" | tee -a \"$out\"\n  else\n    printf 'delete\\t%s\\t%s\\t%s\\treason=older-than-retention\\n' \"$created\" \"$name\" \"$path\" | tee -a \"$out\"\n    if [ \"$apply\" = true ]; then\n      rm -rf -- \"$path\"\n    fi\n  fi\ndone\n",
        keep.parse::<usize>().unwrap_or(10),
        shell_single_quote(&system.name)
    );
    let gc_path = bin_dir.join("jetos-lifecycle-gc");
    fs::write(&gc_path, gc)?;
    make_executable(&gc_path)?;
    let channel_update = format!(
        "#!/usr/bin/env sh\nset -eu\nchannel={}\ncmd=${{JETOS_CHANNEL_UPDATE_CMD:-jetpack channel update \"$channel\"}}\nsh -c \"$cmd\"\n",
        shell_single_quote(&channel)
    );
    let channel_path = bin_dir.join("jetos-channel-update");
    fs::write(&channel_path, channel_update)?;
    make_executable(&channel_path)?;
    let upgrade = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\nproof_dir=${{JETOS_LIFECYCLE_PROOF_DIR:-$root/lifecycle}}\nmkdir -p \"$proof_dir\"\nfetch=${{JETOS_UPGRADE_FETCH_CMD:-/run/current-system/sw/bin/jetos-channel-update}}\nbuild=${{JETOS_UPGRADE_BUILD_CMD:-jet os build {host}}}\nproof=${{JETOS_UPGRADE_PROOF_CMD:-jet os proof {host}}}\nswitch_cmd=${{JETOS_UPGRADE_SWITCH_CMD:-jet os switch {host}}}\nhealth=${{JETOS_UPGRADE_HEALTH_CMD:-true}}\nrollback=${{JETOS_UPGRADE_ROLLBACK_CMD:-jet os rollback {host}}}\nrun_step() {{ name=$1; shift; printf '%s\\n' \"jetos lifecycle: $name\"; sh -c \"$*\"; }}\nrun_step fetch \"$fetch\"\nrun_step build \"$build\"\nrun_step proof \"$proof\"\nrun_step switch \"$switch_cmd\"\nif run_step health \"$health\"; then\n  printf '{{\"kind\":\"jetos.auto-upgrade-proof\",\"state\":\"switched\",\"rollback\":\"available\",\"proof\":\"health-passed\"}}\\n' > \"$proof_dir/auto-upgrade-proof.json\"\nelse\n  sh -c \"$rollback\" || true\n  printf '{{\"kind\":\"jetos.auto-upgrade-proof\",\"state\":\"rolled-back\",\"rollback\":\"executed\",\"proof\":\"health-failed\"}}\\n' > \"$proof_dir/auto-upgrade-proof.json\"\n  exit 1\nfi\ncat \"$proof_dir/auto-upgrade-proof.json\"\n",
        host = system.name
    );
    let upgrade_path = bin_dir.join("jetos-auto-upgrade");
    fs::write(&upgrade_path, upgrade)?;
    make_executable(&upgrade_path)?;
    if clean_bool_json(&auto_upgrade) == "true" {
        fs::write(
            unit_dir.join("jetos-auto-upgrade.service"),
            "[Unit]\nDescription=jetos proof-gated auto-upgrade\n\n[Service]\nType=oneshot\nExecStart=/run/current-system/sw/bin/jetos-auto-upgrade\n",
        )?;
        fs::write(
            unit_dir.join("jetos-auto-upgrade.timer"),
            format!(
                "[Unit]\nDescription=jetos auto-upgrade schedule\n\n[Timer]\nOnCalendar={schedule}\nUnit=jetos-auto-upgrade.service\n\n[Install]\nWantedBy=timers.target\n"
            ),
        )?;
        enable_unit(&unit_dir, "timers.target", "jetos-auto-upgrade.timer")?;
    }
    Ok(())
}

fn write_service_manager_depth(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let service_dir = dir.join("service-manager");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&service_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let tmpfiles_dir = dir.join("etc/tmpfiles.d");
    fs::create_dir_all(&tmpfiles_dir)?;
    let mut facts = Vec::new();
    for svc in system.services.iter().filter(|s| s.enable) {
        if let Some(tmpfiles) = service_extra(svc, &["tmpfiles"]) {
            fs::write(
                tmpfiles_dir.join(format!("{}.conf", svc.name)),
                format!("{}\n", tmpfiles),
            )?;
        }
        let hardening =
            service_extra(svc, &["hardening"]).unwrap_or_else(|| "default-sandbox".to_string());
        let journal = service_extra(svc, &["journal"]).unwrap_or_else(|| "structured".to_string());
        facts.push(JSON::object_of(&[
            ("name", &svc.name),
            ("hardening", &hardening),
            ("journal", &journal),
            (
                "timers",
                if service_extra(svc, &["timer", "schedule"]).is_some() {
                    "true"
                } else {
                    "false"
                },
            ),
            (
                "sockets",
                if service_extra(svc, &["socket", "listen"]).is_some() {
                    "true"
                } else {
                    "false"
                },
            ),
        ]));
    }
    fs::write(
        service_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.service-manager-depth\",\"services\":[{}],\"features\":[\"services\",\"timers\",\"sockets\",\"tmpfiles\",\"hardening\",\"journal\"]}}",
            facts.join(",")
        ),
    )?;
    fs::write(
        service_dir.join("log-policy.json"),
        "{\"kind\":\"jetos.service-logs\",\"backend\":\"journalctl\",\"fallback\":\"service-manager/logs/<unit>.log\",\"proof\":\"logs-query-ready\"}",
    )?;
    let logs = "#!/usr/bin/env sh\nset -eu\nunit=${1:-}\nif [ -z \"$unit\" ]; then\n  echo 'usage: jetos-service-logs <unit> [--since <time>]' >&2\n  exit 2\nfi\nshift || true\nsince=''\nif [ \"${1:-}\" = '--since' ]; then\n  shift\n  since=${1:-}\nfi\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\njournal=${JETOS_JOURNALCTL_BIN:-journalctl}\nif command -v \"$journal\" >/dev/null 2>&1; then\n  if [ -n \"$since\" ]; then\n    exec \"$journal\" -u \"$unit\" --since \"$since\"\n  fi\n  exec \"$journal\" -u \"$unit\"\nfi\nfallback=\"$root/service-manager/logs/$unit.log\"\nif [ -f \"$fallback\" ]; then\n  cat \"$fallback\"\n  exit 0\nfi\necho \"jetos logs: no journal backend or fallback log for $unit\" >&2\nexit 2\n";
    let logs_path = bin_dir.join("jetos-service-logs");
    fs::write(&logs_path, logs)?;
    make_executable(&logs_path)
}

fn write_app_module_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let apps_dir = dir.join("apps");
    let programs_dir = apps_dir.join("programs");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&apps_dir)?;
    fs::create_dir_all(&programs_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut rows = prefixed_options(system, "apps.program.");
    rows.extend(prefixed_options(system, "user."));
    rows.extend(prefixed_options(system, "theme."));
    let modules = [
        "git",
        "ssh",
        "fish",
        "starship",
        "ghostty",
        "helix",
        "yazi",
        "btop",
        "bat",
        "eza",
        "fzf",
        "zoxide",
        "ripgrep",
        "tealdeer",
        "fastfetch",
        "vscode",
        "cursor",
        "discord",
        "spicetify",
        "browser",
    ];
    let mut module_json = Vec::new();
    for module in modules {
        let module_dir = programs_dir.join(module);
        fs::create_dir_all(&module_dir)?;
        let options = prefixed_options(system, &format!("apps.program.{module}."));
        let enabled = option_value(system, &[&format!("apps.program.{module}.enable")])
            .unwrap_or_else(|| (!options.is_empty()).to_string());
        let config_path = app_module_config_path(module);
        let package = option_value(system, &[&format!("apps.program.{module}.package")])
            .unwrap_or_else(|| module.to_string());
        fs::write(
            module_dir.join("module.json"),
            format!(
                "{{\"kind\":\"jetos.app-module\",\"name\":{},\"enabled\":{},\"package\":{},\"config_path\":{},\"options\":[{}],\"proof\":\"app-module-ready\"}}",
                JSON::quote(module),
                clean_bool_json(&enabled),
                JSON::quote(&package),
                JSON::quote(&config_path),
                option_rows_json(&options)
            ),
        )?;
        let mut config = format!("# managed by jetos apps.program.{module}\n");
        for (key, value) in &options {
            config.push_str(&format!("{key} = {value}\n"));
        }
        if module == "git" {
            if let Some(name) = option_value(system, &["apps.program.git.userName"]) {
                config.push_str(&format!("user.name = {name}\n"));
            }
            if let Some(email) = option_value(system, &["apps.program.git.userEmail"]) {
                config.push_str(&format!("user.email = {email}\n"));
            }
        }
        fs::write(module_dir.join("config"), config)?;
        module_json.push(format!(
            "{{\"name\":{},\"enabled\":{},\"config\":{},\"proof\":\"ready\"}}",
            JSON::quote(module),
            clean_bool_json(&enabled),
            JSON::quote(&format!("apps/programs/{module}/config"))
        ));
    }
    fs::write(
        apps_dir.join("coverage.manifest"),
        modules
            .iter()
            .map(|module| format!("{module}\tmodule\n"))
            .collect::<String>(),
    )?;
    fs::write(
        apps_dir.join("gap-cards.manifest"),
        "vscode-extension-provider\tcovered-by-#330\ncursor-extension-provider\tcovered-by-#330\nspicetify-patching-provider\tcovered-by-#330\n",
    )?;
    let apply = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nhome=${JETOS_USER_HOME:-$HOME}\nproof_dir=${JETOS_APP_PROOF_DIR:-$home/.jetos/proof}\nmkdir -p \"$proof_dir\"\ncount=0\nfor module in \"$root\"/apps/programs/*; do\n  [ -d \"$module\" ] || continue\n  name=${module##*/}\n  config_path=$(sed -n 's/.*\"config_path\":\"\\([^\"]*\\)\".*/\\1/p' \"$module/module.json\")\n  [ -n \"$config_path\" ] || config_path=\".config/$name/config\"\n  dest=\"$home/$config_path\"\n  mkdir -p \"${dest%/*}\"\n  cp \"$module/config\" \"$dest\"\n  count=$((count + 1))\ndone\nprintf '{\"kind\":\"jetos.app-modules\",\"state\":\"applied\",\"count\":%s,\"proof\":\"app-config-applied\"}\\n' \"$count\" > \"$proof_dir/app-modules.json\"\ncat \"$proof_dir/app-modules.json\"\n";
    let apply_path = bin_dir.join("jetos-app-module-apply");
    fs::write(&apply_path, apply)?;
    make_executable(&apply_path)?;
    fs::write(
        apps_dir.join("modules.json"),
        format!(
            "{{\"kind\":\"jetos.app-module-library\",\"host\":{},\"modules\":[{}],\"catalog\":[{}],\"apply\":\"sw/bin/jetos-app-module-apply\",\"proof\":\"app-config-projected\"}}",
            JSON::quote(&system.name),
            option_rows_json(&rows),
            module_json.join(",")
        ),
    )
}

fn app_module_config_path(module: &str) -> String {
    match module {
        "git" => ".config/git/config".to_string(),
        "ssh" => ".ssh/config".to_string(),
        "fish" => ".config/fish/config.fish".to_string(),
        "starship" => ".config/starship.toml".to_string(),
        "ghostty" => ".config/ghostty/config".to_string(),
        "helix" => ".config/helix/config.toml".to_string(),
        "vscode" => ".config/Code/User/settings.json".to_string(),
        "cursor" => ".config/Cursor/User/settings.json".to_string(),
        "browser" => ".config/browser/policies.json".to_string(),
        _ => format!(".config/{module}/config"),
    }
}
