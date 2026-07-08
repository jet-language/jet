fn write_bootable_root_projection(dir: &Path) -> std::io::Result<()> {
    let root = dir.join("root");
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(root.join("run/current-system"))?;
    fs::create_dir_all(root.join("var/lib/jetos/generations"))?;
    for top in [
        "boot",
        "etc",
        "sbin",
        "sw",
        "share",
        "studio",
        "init",
        "network",
        "hardware",
        "users",
        "flatpak",
        "performance",
        "module-system",
        "storage",
        "workloads",
        "theme",
        "fleet",
        "options",
        "image-variants",
        "lifecycle",
        "service-manager",
        "apps",
        "acceptance",
        "desktop",
        "store",
        "compat",
        "terminal",
        "home",
    ] {
        let src = dir.join(top);
        if !src.exists() {
            continue;
        }
        copy_dir_recursive(&src, &root.join("run/current-system").join(top))?;
        match top {
            "boot" | "etc" | "sbin" | "home" => copy_dir_recursive(&src, &root.join(top))?,
            _ => {}
        }
    }
    for file in [
        "plan.json",
        "proof.txt",
        "provenance.json",
        "health-checks.txt",
        "activation-diff.txt",
        "secrets.tmpfs.manifest",
        "vm-proof.txt",
    ] {
        let src = dir.join(file);
        if src.is_file() {
            link_or_copy_file(&src, &root.join("run/current-system").join(file))?;
        }
    }
    fs::write(
        root.join("var/lib/jetos/generations/current"),
        format!("{}\n", dir.display()),
    )
}

#[cfg(unix)]
fn link_or_copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    match std::os::unix::fs::symlink(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => fs::copy(src, dst).map(|_| ()),
    }
}

#[cfg(not(unix))]
fn link_or_copy_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst).map(|_| ())
}

fn copy_file_replace(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = fs::remove_file(dst);
    fs::copy(src, dst)?;
    let mut perms = fs::metadata(dst)?.permissions();
    perms.set_readonly(false);
    fs::set_permissions(dst, perms)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            link_or_copy_file(&path, &target)?;
        }
    }
    Ok(())
}

fn copy_dir_recursive_deref(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let target = dst.join(entry.file_name());
        let meta = fs::metadata(&path)?;
        if meta.is_dir() {
            copy_dir_recursive_deref(&path, &target)?;
        } else if meta.is_file() {
            copy_file_replace(&path, &target)?;
        }
    }
    Ok(())
}

fn enable_unit(unit_dir: &Path, target: &str, unit_name: &str) -> std::io::Result<()> {
    let wants = unit_dir.join(format!("{target}.wants"));
    fs::create_dir_all(&wants)?;
    let dst = wants.join(unit_name);
    let _ = fs::remove_file(&dst);
    enable_unit_link(
        Path::new("..").join(unit_name),
        unit_dir.join(unit_name),
        dst,
    )
}

#[cfg(unix)]
fn enable_unit_link(rel_src: PathBuf, _abs_src: PathBuf, dst: PathBuf) -> std::io::Result<()> {
    std::os::unix::fs::symlink(rel_src, dst)
}

#[cfg(not(unix))]
fn enable_unit_link(_rel_src: PathBuf, abs_src: PathBuf, dst: PathBuf) -> std::io::Result<()> {
    fs::copy(abs_src, dst).map(|_| ())
}
