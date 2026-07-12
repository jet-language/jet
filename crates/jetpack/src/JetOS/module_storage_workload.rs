pub(super) fn write_module_priority_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let module_dir = dir.join("module-system");
    fs::create_dir_all(&module_dir)?;
    let mut keys = system
        .options
        .iter()
        .filter(|o| !is_option_priority_metadata(&o.key))
        .map(|o| o.key.clone())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    let resolved = keys
        .iter()
        .filter_map(|key| resolved_option(system, key))
        .map(|r| r.to_json())
        .collect::<Vec<_>>()
        .join(",");
    let disabled = option_value(system, &["packages.disabledModules"])
        .map(|v| parse_list_items(&v))
        .unwrap_or_default();
    fs::write(
        module_dir.join("disabled-modules.manifest"),
        manifest_lines(&disabled),
    )?;
    fs::write(
        module_dir.join("explain.json"),
        format!(
            "{{\"kind\":\"jetos.option-explain\",\"tiers\":[\"Default\",\"Normal\",\"Force\",\"Priority(n)\"],\"module_ids\":\"stable-source-paths\",\"resolved\":[{}],\"disabled_modules\":[{}]}}",
            resolved,
            disabled.iter().map(|m| JSON::quote(m)).collect::<Vec<_>>().join(",")
        ),
    )
}

pub(super) fn write_storage_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let storage_dir = dir.join("storage");
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&storage_dir)?;
    fs::create_dir_all(&bin_dir)?;
    let mut rows = prefixed_options(system, "storage.");
    rows.extend(prefixed_options(system, "filesystem."));
    let persist = prefixed_options(system, "storage.persist.");
    let disk = option_value(system, &["storage.disk.main.device"])
        .unwrap_or_else(|| "guided-ext4".to_string());
    let table = option_value(system, &["storage.disk.main.table"])
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "GPT".to_string());
    let esp_size = option_value(system, &["storage.disk.main.partitions.esp.size"])
        .unwrap_or_else(|| "512M".to_string());
    let root_fs = option_value(system, &["storage.filesystem.root.type"])
        .or_else(|| option_value(system, &["filesystem.root.type"]))
        .map(|s| clean_symbol(&s))
        .unwrap_or_else(|| "ext4".to_string());
    let ephemeral = option_value(system, &["storage.ephemeralRoot", "storage.root.ephemeral"])
        .unwrap_or_else(|| "false".to_string());
    fs::write(
        storage_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.storage-tree\",\"installer_consumes\":true,\"activation_consumes\":true,\"disk\":{},\"table\":{},\"root_fs\":{},\"ephemeral_root\":{},\"options\":[{}],\"commands\":[\"jetos-storage-plan\",\"jetos-storage-apply\",\"jetos-persist-activate\"],\"proof\":\"storage-plan-ready\"}}",
            JSON::quote(&disk),
            JSON::quote(&table),
            JSON::quote(&root_fs),
            clean_bool_json(&ephemeral),
            option_rows_json(&rows)
        ),
    )?;
    fs::write(
        storage_dir.join("plan.json"),
        format!(
            "{{\"kind\":\"jetos.storage-plan\",\"host\":{},\"disk\":{},\"table\":{},\"root_fs\":{},\"partitions\":[{{\"name\":\"esp\",\"size\":{},\"fs\":\"vfat\",\"mount\":\"/boot\"}},{{\"name\":\"root\",\"size\":\"rest\",\"fs\":{},\"mount\":\"/\"}}],\"ephemeral_root\":{},\"persistence\":[{}],\"destructive_actions\":[\"partition\",\"format\"],\"safety\":\"requires --manual plus --execute\"}}",
            JSON::quote(&system.name),
            JSON::quote(&disk),
            JSON::quote(&table),
            JSON::quote(&root_fs),
            JSON::quote(&esp_size),
            JSON::quote(&root_fs),
            clean_bool_json(&ephemeral),
            option_rows_json(&persist)
        ),
    )?;
    fs::write(
        storage_dir.join("mounts.fstab"),
        format!(
            "LABEL=JETOS-ESP\t/boot\tvfat\tumask=0077\t0\t1\nLABEL=jetos-root\t/\t{}\tdefaults\t0\t1\n",
            root_fs.to_ascii_lowercase()
        ),
    )?;
    fs::write(
        storage_dir.join("persistence.manifest"),
        persist
            .iter()
            .map(|(key, value)| format!("{key}\t{value}\n"))
            .collect::<String>(),
    )?;
    let plan_script = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\ncat \"$root/storage/plan.json\"\nprintf '\\n'\n";
    let plan_path = bin_dir.join("jetos-storage-plan");
    fs::write(&plan_path, plan_script)?;
    make_executable(&plan_path)?;
    let apply_script = format!(
        "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ndisk=${{JETOS_STORAGE_DISK:-{}}}\nmanual=false\nexecute=false\nfor arg in \"$@\"; do\n  case \"$arg\" in\n    --manual) manual=true ;;\n    --execute) execute=true ;;\n    *) echo \"usage: jetos-storage-apply --manual [--execute]\" >&2; exit 2 ;;\n  esac\ndone\nif [ \"$manual\" != true ]; then\n  echo 'jetos storage: destructive disk plan requires --manual' >&2\n  exit 2\nfi\nlog=${{JETOS_STORAGE_LOG:-$root/storage/apply-plan.sh}}\nproof_dir=${{JETOS_STORAGE_PROOF_DIR:-$root/storage}}\nmkdir -p \"$proof_dir\"\n{{\n  printf '%s\\n' '#!/usr/bin/env sh'\n  printf '%s\\n' 'set -eu'\n  printf 'sfdisk --wipe always %s <<EOF\\nlabel: gpt\\nsize={}, type=U\\ntype=L\\nEOF\\n' \"$disk\"\n  printf 'mkfs.vfat -n JETOS-ESP %s1\\n' \"$disk\"\n  printf 'mkfs.{} -L jetos-root %s2\\n' \"$disk\"\n}} > \"$log\"\nif [ \"$execute\" = true ]; then\n  sh \"$log\"\nfi\nprintf '{{\"kind\":\"jetos.storage-apply\",\"state\":\"planned\",\"executed\":%s,\"disk\":\"%s\",\"proof\":\"manual-storage-plan-reviewed\"}}\\n' \"$execute\" \"$disk\" > \"$proof_dir/apply-proof.json\"\ncat \"$proof_dir/apply-proof.json\"\n",
        shell_single_quote(&disk),
        esp_size,
        root_fs.to_ascii_lowercase()
    );
    let apply_path = bin_dir.join("jetos-storage-apply");
    fs::write(&apply_path, apply_script)?;
    make_executable(&apply_path)?;
    let persist_script = "#!/usr/bin/env sh\nset -eu\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\npersist_root=${JETOS_PERSIST_ROOT:-/persist}\nephemeral_root=${JETOS_EPHEMERAL_ROOT:-/}\nproof_dir=${JETOS_STORAGE_PROOF_DIR:-$root/storage}\nmkdir -p \"$proof_dir\"\nproof=\"$proof_dir/persistence-proof.json\"\nmanifest=\"$root/storage/persistence.manifest\"\ncount=0\n: > \"$proof.tmp\"\nif [ -f \"$manifest\" ]; then\n  while IFS='	' read -r key path; do\n    [ -n \"$path\" ] || continue\n    case \"$path\" in /*) rel=${path#/} ;; *) rel=$path ;; esac\n    mkdir -p \"$persist_root/$rel\" \"$ephemeral_root/$rel\"\n    printf '%s\\t%s\\n' \"$key\" \"$path\" >> \"$proof.tmp\"\n    count=$((count + 1))\n  done < \"$manifest\"\nfi\nprintf '{\"kind\":\"jetos.persistence\",\"state\":\"activated\",\"count\":%s,\"proof\":\"impermanence-persist-ready\"}\\n' \"$count\" > \"$proof\"\ncat \"$proof\"\n";
    let persist_path = bin_dir.join("jetos-persist-activate");
    fs::write(&persist_path, persist_script)?;
    make_executable(&persist_path)
}

pub(super) fn write_workload_facts(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let workloads_dir = dir.join("workloads");
    let unit_dir = dir.join("etc/systemd/system");
    fs::create_dir_all(&workloads_dir)?;
    fs::create_dir_all(&unit_dir)?;
    let names = collect_names(system, "workload");
    let mut facts = Vec::new();
    let bin_dir = dir.join("sw/bin");
    fs::create_dir_all(&bin_dir)?;
    for name in names {
        let backend = option_value(system, &[&format!("workload.{name}.backend")])
            .map(|s| clean_symbol(&s))
            .unwrap_or_else(|| "Container".to_string());
        let image = option_value(system, &[&format!("workload.{name}.image")])
            .or_else(|| option_value(system, &[&format!("workload.{name}.package")]))
            .unwrap_or_else(|| name.clone());
        let ports = option_value(system, &[&format!("workload.{name}.ports")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let ports_json = ports
            .iter()
            .map(|p| JSON::quote(p))
            .collect::<Vec<_>>()
            .join(",");
        let mounts = option_value(system, &[&format!("workload.{name}.mounts")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let mounts_json = strings_json(&mounts);
        let secrets = option_value(system, &[&format!("workload.{name}.secrets")])
            .map(|v| parse_list_items(&v))
            .unwrap_or_default();
        let secrets_json = strings_json(&secrets);
        let memory = option_value(
            system,
            &[
                &format!("workload.{name}.resources.memory"),
                &format!("workload.{name}.microvm.memory"),
            ],
        )
        .unwrap_or_else(|| {
            if backend == "MicroVM" {
                "1024M".to_string()
            } else {
                "host-shared".to_string()
            }
        });
        let cpus = option_value(
            system,
            &[
                &format!("workload.{name}.resources.cpus"),
                &format!("workload.{name}.microvm.cpus"),
            ],
        )
        .unwrap_or_else(|| "1".to_string());
        let health = option_value(system, &[&format!("workload.{name}.health.command")])
            .unwrap_or_else(|| {
                ports.first().map_or_else(
                    || "true".to_string(),
                    |port| format!("nc -z 127.0.0.1 {port}"),
                )
            });
        let rollback_keep = option_value(system, &[&format!("workload.{name}.rollback.keep")])
            .unwrap_or_else(|| "2".to_string());
        let command =
            option_value(system, &[&format!("workload.{name}.command")]).unwrap_or_else(|| {
                let ports_flags = ports
                    .iter()
                    .map(|p| format!("-p {p}:{p}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mount_flags = mounts
                    .iter()
                    .map(|m| format!("-v {m}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                let secret_flags = secrets
                    .iter()
                    .map(|s| format!("--secret {s}"))
                    .collect::<Vec<_>>()
                    .join(" ");
                if backend == "MicroVM" {
                    format!("qemu-system-x86_64 -m {memory} -smp {cpus} -nographic -kernel {image}")
                } else {
                    format!("${{JETOS_CONTAINER_BIN:-podman}} run --rm {ports_flags} {mount_flags} {secret_flags} {image}")
                }
            });
        fs::write(
            workloads_dir.join(format!("{name}.plan.json")),
            format!(
                "{{\"kind\":\"jetos.workload-plan\",\"name\":{},\"backend\":{},\"image\":{},\"ports\":[{}],\"mounts\":[{}],\"secrets\":[{}],\"resources\":{{\"memory\":{},\"cpus\":{}}},\"health\":{},\"rollback_keep\":{},\"command\":{},\"proof\":\"workload-proof-ready\"}}",
                JSON::quote(&name),
                JSON::quote(&backend),
                JSON::quote(&image),
                ports_json,
                mounts_json,
                secrets_json,
                JSON::quote(&memory),
                JSON::quote(&cpus),
                JSON::quote(&health),
                JSON::quote(&rollback_keep),
                JSON::quote(&command)
            ),
        )?;
        fs::write(
            workloads_dir.join(format!("{name}.rollback.manifest")),
            format!("keep\t{rollback_keep}\ncurrent\t/run/current-system/workloads/{name}\n"),
        )?;
        let health_path = workloads_dir.join(format!("health-{name}.sh"));
        fs::write(
            &health_path,
            format!(
                "#!/usr/bin/env sh\nset -eu\ncmd={}\nsh -c \"$cmd\"\n",
                shell_single_quote(&health)
            ),
        )?;
        make_executable(&health_path)?;
        let script_path = workloads_dir.join(format!("run-{name}.sh"));
        fs::write(
            &script_path,
            format!(
                "#!/usr/bin/env sh\nset -eu\nroot=${{JETOS_SYSTEM_ROOT:-/run/current-system}}\ncmd={}\nsh -c \"$cmd\"\n\"$root/workloads/health-{name}.sh\"\n",
                shell_single_quote(&command)
            ),
        )?;
        make_executable(&script_path)?;
        fs::write(
            unit_dir.join(format!("workload-{name}.service")),
            format!(
                "[Unit]\nDescription=jetos workload {name}\n\n[Service]\nExecStart=/run/current-system/sw/bin/jetos-workload-run {name}\nRestart=on-failure\n\n[Install]\nWantedBy=multi-user.target\n"
            ),
        )?;
        enable_unit(
            &unit_dir,
            "multi-user.target",
            &format!("workload-{name}.service"),
        )?;
        facts.push(format!(
            "{{\"name\":{},\"backend\":{},\"image\":{},\"ports\":[{}],\"mounts\":[{}],\"secrets\":[{}],\"resources\":{{\"memory\":{},\"cpus\":{}}},\"health\":{},\"rollback_keep\":{},\"proof\":\"workload-rollout-ready\"}}",
            JSON::quote(&name),
            JSON::quote(&backend),
            JSON::quote(&image),
            ports_json,
            mounts_json,
            secrets_json,
            JSON::quote(&memory),
            JSON::quote(&cpus),
            JSON::quote(&health),
            JSON::quote(&rollback_keep)
        ));
    }
    let runner = "#!/usr/bin/env sh\nset -eu\nname=${1:-}\nroot=${JETOS_SYSTEM_ROOT:-/run/current-system}\nif [ -z \"$name\" ]; then\n  echo 'usage: jetos-workload-run <name>' >&2\n  exit 2\nfi\nscript=\"$root/workloads/run-$name.sh\"\nif [ ! -x \"$script\" ]; then\n  echo \"jetos workload: no runnable workload named $name\" >&2\n  exit 2\nfi\nexec /bin/sh \"$script\"\n";
    let runner_path = bin_dir.join("jetos-workload-run");
    fs::write(&runner_path, runner)?;
    make_executable(&runner_path)?;
    fs::write(
        workloads_dir.join("facts.json"),
        format!(
            "{{\"kind\":\"jetos.workloads\",\"items\":[{}]}}",
            facts.join(",")
        ),
    )
}
