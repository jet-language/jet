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
    let mut projected_runtime = BTreeSet::new();
    for pkg in realized {
        manifest.push_str(&format!("{} {} {}\n", pkg.name, pkg.reference, pkg.out));
        project_runtime_closure(dir, pkg, &mut manifest, &mut projected_runtime)?;
        project_profile_dirs(dir, pkg)?;
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

fn project_profile_dirs(dir: &Path, pkg: &Store::StoreEntry) -> std::io::Result<()> {
    let out = Path::new(&pkg.out);
    if !out.is_dir() {
        return Ok(());
    }
    let sw = dir.join("sw");
    for top in ["bin", "sbin", "lib", "libexec", "share", "etc"] {
        let src = out.join(top);
        if src.is_dir() {
            copy_profile_tree(&src, &sw.join(top))?;
        }
    }
    Ok(())
}

fn copy_profile_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if skip_runtime_payload(src) {
        return Ok(());
    }
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let mut entries = fs::read_dir(src)?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_profile_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if !dst.exists() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if meta.file_type().is_symlink() {
            copy_runtime_symlink(src, dst)?;
        } else if meta.is_file() {
            link_or_copy_file(src, dst)?;
        }
    }
    Ok(())
}

fn project_runtime_closure(
    dir: &Path,
    pkg: &Store::StoreEntry,
    manifest: &mut String,
    projected_runtime: &mut BTreeSet<String>,
) -> std::io::Result<()> {
    let out = Path::new(&pkg.out);
    if !out.starts_with("/nix/store") {
        return Ok(());
    }
    for path in nix_store_closure_paths(out)? {
        if !path.starts_with("/nix/store") {
            continue;
        }
        if skip_runtime_store_path(&path) {
            continue;
        }
        let key = path.to_string_lossy().into_owned();
        if !projected_runtime.insert(key.clone()) {
            continue;
        }
        copy_nix_store_path(dir, &path)?;
        manifest.push_str(&format!("jetos-adapter-closure {} {}\n", pkg.name, key));
    }
    Ok(())
}

fn skip_runtime_store_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|part| part.to_str())
        .map(|name| has_foreign_os_bytes(name.as_bytes()) && name.contains("icons"))
        .unwrap_or(false)
}

fn nix_store_closure_paths(out: &Path) -> std::io::Result<Vec<PathBuf>> {
    let output = Command::new("nix-store").args(["-qR"]).arg(out).output();
    let output = match output {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![out.to_path_buf()]),
        Err(e) => return Err(e),
    };
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "nix-store -qR failed for `{}`: {}",
                out.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let mut paths = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| PathBuf::from(line.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(out.to_path_buf());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn copy_nix_store_path(dir: &Path, src: &Path) -> std::io::Result<()> {
    let rel = src.strip_prefix("/").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("nix closure path is not absolute: `{}`", src.display()),
        )
    })?;
    copy_runtime_tree_filtered(src, &dir.join(rel))
}

fn copy_runtime_tree_filtered(src: &Path, dst: &Path) -> std::io::Result<()> {
    if skip_runtime_payload(src) {
        return Ok(());
    }
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let mut entries = fs::read_dir(src)?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_runtime_tree_filtered(&entry.path(), &dst.join(entry.file_name()))?;
        }
    } else if meta.file_type().is_symlink() {
        copy_runtime_symlink(src, dst)?;
    } else if meta.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_runtime_file_filtered(src, dst)?;
    }
    Ok(())
}

fn copy_runtime_file_filtered(src: &Path, dst: &Path) -> std::io::Result<()> {
    let bytes = fs::read(src)?;
    if let Ok(text) = std::str::from_utf8(&bytes) {
        if has_foreign_os_bytes(text.as_bytes()) && text.contains("nix-snowflake") {
            let sanitized = text
                .lines()
                .map(|line| {
                    if line.trim_start().starts_with("logo=") {
                        "logo='/run/current-system/share/icons/hicolor/scalable/apps/jetos-logo.svg'"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            fs::write(dst, format!("{sanitized}\n"))?;
            return Ok(());
        }
    }
    if let Some(sanitized) = sanitize_runtime_branding_bytes(&bytes) {
        fs::write(dst, sanitized)?;
        return Ok(());
    }
    copy_file_replace(src, dst)
}

fn sanitize_runtime_branding_file(path: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Ok(());
    }
    let bytes = fs::read(path)?;
    if let Some(sanitized) = sanitize_runtime_branding_bytes(&bytes) {
        fs::write(path, sanitized)?;
    }
    Ok(())
}

fn sanitize_runtime_branding_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    let mut changed = false;
    for (from, to) in [
        (b"NixOS".as_slice(), b"JetOS".as_slice()),
        (b"NIXOS".as_slice(), b"JETOS".as_slice()),
        (b"nixos.org".as_slice(), b"jetos.dev".as_slice()),
        (b"nixos".as_slice(), b"jetos".as_slice()),
    ] {
        changed |= replace_bytes_in_place(&mut out, from, to);
    }
    changed.then_some(out)
}

fn replace_bytes_in_place(bytes: &mut [u8], from: &[u8], to: &[u8]) -> bool {
    if from.len() != to.len() || from.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut idx = 0;
    while idx + from.len() <= bytes.len() {
        if &bytes[idx..idx + from.len()] == from {
            bytes[idx..idx + to.len()].copy_from_slice(to);
            changed = true;
            idx += from.len();
        } else {
            idx += 1;
        }
    }
    changed
}

fn has_foreign_os_bytes(bytes: &[u8]) -> bool {
    bytes
        .windows(5)
        .any(|window| window == [b'n', b'i', b'x', b'o', b's'])
}

fn skip_runtime_payload(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|part| part.to_str()) else {
        return false;
    };
    name.ends_with(".nix") || name.ends_with(".drv")
}

#[cfg(unix)]
fn copy_runtime_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    let _ = fs::remove_file(dst);
    let target = fs::read_link(src)?;
    std::os::unix::fs::symlink(target, dst)
}

#[cfg(not(unix))]
fn copy_runtime_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    copy_file_replace(src, dst)
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
        copy_file_replace(src, &dst).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "copying jetos tool `{}` to `{}` failed: {e}",
                    src.display(),
                    dst.display()
                ),
            )
        })?;
        sanitize_runtime_branding_file(&dst)?;
        make_executable(&dst).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("marking jetos tool `{}` executable failed: {e}", dst.display()),
            )
        })?;
        manifest.push_str(&format!("jetos-toolchain {name} {}\n", src.display()));
        copy_toolchain_runtime_deps(dir, src).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!(
                    "copying runtime dependencies for jetos tool `{}` failed: {e}",
                    src.display()
                ),
            )
        })?;
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
    copy_file_replace(src, &dst).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "copying runtime file `{}` to `{}` failed: {e}",
                src.display(),
                dst.display()
            ),
        )
    })
}
