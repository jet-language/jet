fn generation_dir(system: &SystemPlan, explicit: Option<&str>) -> PathBuf {
    let name = explicit
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{}-{}", system.name, now_secs()));
    systems_dir().join("generations").join(name)
}

fn systems_dir() -> PathBuf {
    Store::resolve().root.join("systems")
}

fn generations_log() -> PathBuf {
    systems_dir().join("generations.log")
}

fn write_generation_files(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
    plan: &EnvPlan,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let packages_json = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let services_json = system
        .services
        .iter()
        .map(|s| {
            JSON::object_of(&[
                ("name", &s.name),
                ("enable", if s.enable { "true" } else { "false" }),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let options_json = system
        .options
        .iter()
        .map(|o| JSON::object_of(&[("key", &o.key), ("value", &o.value)]))
        .collect::<Vec<_>>()
        .join(",");
    let plan_text = render_plan_json(
        system,
        realized,
        Some((&packages_json, &services_json, &options_json)),
    );
    fs::write(dir.join("plan.json"), &plan_text)?;
    write_root_closure(dir, realized)?;
    write_etc_tree(dir, system)?;
    write_network_facts(dir, system)?;
    write_boot_facts(dir, system, realized)?;
    write_init_facts(dir, system, realized)?;
    fs::write(dir.join("proof.txt"), render_proof(system, realized, plan))?;
    write_systemd_units(dir, system)?;
    write_systemd_timer_socket_units(dir, system)?;
    write_terminal_environment(dir, system)?;
    write_activation_diff(dir, system, realized)?;
    write_health_checks(dir, system)?;
    write_hardware_facts(dir, system)?;
    write_user_environment_facts(dir, system)?;
    write_flatpak_facts(dir, system)?;
    write_performance_facts(dir, system)?;
    write_module_priority_facts(dir, system)?;
    write_storage_facts(dir, system)?;
    write_workload_facts(dir, system)?;
    write_theme_facts(dir, system)?;
    write_fleet_deploy_facts(dir, system, plan)?;
    write_options_reference(dir, system)?;
    write_image_variant_facts(dir, system, plan)?;
    write_lifecycle_facts(dir, system)?;
    write_service_manager_depth(dir, system)?;
    write_app_module_facts(dir, system)?;
    write_acceptance_fixture(dir, system)?;
    write_desktop_facts(dir, system)?;
    write_store_cache_facts(dir, realized)?;
    write_compat_escape_hatches(dir, system)?;
    write_studio_app_projection(dir, system)?;
    write_provenance(dir, system, realized)?;
    write_vm_proof(dir, system, &plan_text)?;
    write_secret_manifest(dir, system)?;
    write_bootable_root_projection(dir)?;
    Ok(())
}

fn render_plan_json(
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
    prebuilt: Option<(&str, &str, &str)>,
) -> String {
    let (packages_json, services_json, options_json) = match prebuilt {
        Some((p, s, o)) => (p.to_string(), s.to_string(), o.to_string()),
        None => {
            let packages = system
                .packages
                .iter()
                .map(|p| {
                    let raw = if p.source.is_empty() {
                        p.name.clone()
                    } else {
                        format!("{}:{}", p.source, p.name)
                    };
                    JSON::object_of(&[("name", &p.name), ("source", &p.source), ("ref", &raw)])
                })
                .collect::<Vec<_>>()
                .join(",");
            let services = system
                .services
                .iter()
                .map(|s| {
                    JSON::object_of(&[
                        ("name", &s.name),
                        ("enable", if s.enable { "true" } else { "false" }),
                    ])
                })
                .collect::<Vec<_>>()
                .join(",");
            let options = system
                .options
                .iter()
                .map(|o| JSON::object_of(&[("key", &o.key), ("value", &o.value)]))
                .collect::<Vec<_>>()
                .join(",");
            (packages, services, options)
        }
    };
    let closure_json = realized
        .iter()
        .map(|p| {
            JSON::object_of(&[
                ("name", &p.name),
                ("reference", &p.reference),
                ("out", &p.out),
                ("bin", &p.bin),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let boot = boot_profile(system);
    format!(
        "{{\"host\":{},\"target\":{},\"boot\":{},\"packages\":[{}],\"closure\":[{}],\"services\":[{}],\"options\":[{}]}}",
        JSON::quote(&system.name),
        JSON::quote(&system.target),
        boot.to_json(),
        packages_json,
        closure_json,
        services_json,
        options_json
    )
}

fn write_root_closure(dir: &Path, realized: &[Store::StoreEntry]) -> std::io::Result<()> {
    let sw_bin = dir.join("sw/bin");
    fs::create_dir_all(&sw_bin)?;
    let mut manifest = String::new();
    manifest.push_str("jetos system package closure\n");
    for pkg in realized {
        manifest.push_str(&format!("{} {} {}\n", pkg.name, pkg.reference, pkg.out));
        if pkg.bin.is_empty() {
            continue;
        }
        let bin = Path::new(&pkg.bin);
        let Ok(entries) = fs::read_dir(bin) else {
            continue;
        };
        let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let src = entry.path();
            if !src.is_file() {
                continue;
            }
            let dst = sw_bin.join(entry.file_name());
            link_or_copy_file(&src, &dst)?;
        }
    }
    write_jetos_toolchain(dir, &sw_bin, &mut manifest)?;
    fs::write(dir.join("sw/closure.txt"), manifest)
}

fn write_jetos_toolchain(
    dir: &Path,
    sw_bin: &Path,
    manifest: &mut String,
) -> std::io::Result<()> {
    let candidates = jet_toolchain_candidates();
    for name in ["jet", "jetpack", "jetos"] {
        let Some(src) = candidates.iter().find(|path| {
            path.file_name()
                .and_then(|part| part.to_str())
                .map(|part| part == name)
                .unwrap_or(false)
        }) else {
            continue;
        };
        let dst = sw_bin.join(name);
        copy_file_replace(src, &dst)?;
        make_executable(&dst)?;
        manifest.push_str(&format!("jetos-toolchain {name} {}\n", src.display()));
        copy_toolchain_runtime_deps(dir, src)?;
    }
    Ok(())
}

fn jet_toolchain_candidates() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            if dir.file_name().and_then(|part| part.to_str()) == Some("deps") {
                if let Some(parent) = dir.parent() {
                    dirs.push(parent.to_path_buf());
                }
            }
        }
    }
    dirs.push(PathBuf::from("target/debug"));
    dirs.push(PathBuf::from("target/release"));

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for dir in dirs {
        for name in ["jet", "jetpack", "jetos"] {
            let path = dir.join(name);
            if path.is_file() && seen.insert(path.clone()) {
                out.push(path);
            }
        }
    }
    out
}

fn copy_toolchain_runtime_deps(dir: &Path, binary: &Path) -> std::io::Result<()> {
    for dep in ldd_dependency_paths(binary)? {
        copy_absolute_runtime_file(dir, &dep)?;
        if let Ok(real) = fs::canonicalize(&dep) {
            copy_absolute_runtime_file(dir, &real)?;
        }
    }
    Ok(())
}

fn copy_absolute_runtime_file(dir: &Path, src: &Path) -> std::io::Result<()> {
    if !src.is_absolute() || !src.is_file() {
        return Ok(());
    }
    let Ok(relative) = src.strip_prefix("/") else {
        return Ok(());
    };
    let dst = dir.join(relative);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    copy_file_replace(src, &dst)
}
