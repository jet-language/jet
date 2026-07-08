fn write_etc_tree(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let etc = dir.join("etc");
    fs::create_dir_all(&etc)?;
    let host = option_value(system, &["network.hostName", "network.hostname"])
        .unwrap_or_else(|| system.name.clone());
    fs::write(etc.join("hostname"), format!("{host}\n"))?;
    if let Some(zone) = option_value(system, &["filesystem.timeZone", "filesystem.timezone"]) {
        fs::write(etc.join("timezone"), format!("{zone}\n"))?;
    }
    let root_device = option_value(system, &["filesystem.root.device"])
        .unwrap_or_else(|| "LABEL=jetos-root".to_string());
    let root_type = option_value(system, &["filesystem.root.type"])
        .unwrap_or_else(|| "ext4".to_string())
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let mut fstab = format!("{root_device}\t/\t{root_type}\tdefaults\t0\t1\n");
    for swap in collect_names(system, "filesystem.swap") {
        let device = option_value(system, &[&format!("filesystem.swap.{swap}.device")])
            .unwrap_or_else(|| format!("LABEL=jetos-swap-{swap}"));
        let priority = option_value(system, &[&format!("filesystem.swap.{swap}.priority")])
            .map(|p| format!("pri={p}"))
            .unwrap_or_else(|| "defaults".to_string());
        fstab.push_str(&format!("{device}\tnone\tswap\t{priority}\t0\t0\n"));
    }
    fs::write(etc.join("fstab"), fstab)?;
    write_identity_files(&etc, system)
}

fn write_identity_files(etc: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let users = collect_names(system, "users");
    let groups = collect_names(system, "groups");
    let mut passwd = String::from("root:x:0:0:root:/root:/bin/sh\n");
    let mut group = String::from("root:x:0:\n");
    let mut sysusers = String::new();
    for (idx, user) in users.iter().enumerate() {
        let uid = 1000 + idx;
        let home = option_value(system, &[&format!("users.{user}.home")])
            .unwrap_or_else(|| format!("/home/{user}"));
        let shell = option_value(system, &[&format!("users.{user}.shell")])
            .map(|s| package_path_or_literal(&s))
            .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
        passwd.push_str(&format!("{user}:x:{uid}:{uid}:{user}:{home}:{shell}\n"));
        group.push_str(&format!("{user}:x:{uid}:{user}\n"));
        sysusers.push_str(&format!("u {user} {uid} \"{user}\" {home} {shell}\n"));
    }
    for (idx, name) in groups.iter().enumerate() {
        let gid = 2000 + idx;
        let members = option_value(system, &[&format!("groups.{name}.members")])
            .map(|v| parse_list_items(&v).join(","))
            .unwrap_or_default();
        group.push_str(&format!("{name}:x:{gid}:{members}\n"));
        sysusers.push_str(&format!("g {name} {gid}\n"));
        if !members.is_empty() {
            sysusers.push_str(&format!("m {} {name}\n", members.replace(',', " ")));
        }
    }
    fs::write(etc.join("passwd"), passwd)?;
    fs::write(etc.join("group"), group)?;
    let sysusers_dir = etc.join("sysusers.d");
    fs::create_dir_all(&sysusers_dir)?;
    fs::write(sysusers_dir.join("jetos.conf"), sysusers)
}

fn write_boot_facts(
    dir: &Path,
    system: &SystemPlan,
    realized: &[Store::StoreEntry],
) -> std::io::Result<()> {
    let boot = boot_profile(system);
    let boot_dir = dir.join("boot");
    fs::create_dir_all(&boot_dir)?;
    let kernel_entry = cachyos_kernel_entry(realized);
    let kernel_path = kernel_entry
        .and_then(|entry| boot_artifact(entry, &["boot/vmlinuz-cachyos", "bzImage", "vmlinuz"]))
        .unwrap_or_else(|| PathBuf::from(&boot.kernel));
    let initrd_path = kernel_entry
        .and_then(|entry| boot_artifact(entry, &["boot/initrd-cachyos", "initrd", "initrd.img"]));
    fs::write(
        boot_dir.join("limine.conf"),
        format!(
        "timeout: 5\nserial: yes\ngraphics: no\nverbose: yes\n/jetos {}\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 root=LABEL=jetos-root rw init={}\n",
            system.name, boot.init
        ),
    )?;
    if kernel_path.is_file() {
        link_or_copy_file(&kernel_path, &boot_dir.join("kernel"))?;
    } else {
        fs::write(
            boot_dir.join("kernel"),
            format!("{}\n", kernel_path.display()),
        )?;
    }
    match initrd_path {
        Some(path) if path.is_file() => link_or_copy_file(&path, &boot_dir.join("initrd"))?,
        Some(path) => fs::write(
            boot_dir.join("initrd"),
            format!(
                "{}\nmodules={}\n",
                path.display(),
                boot.initrd_modules.join(",")
            ),
        )?,
        None => fs::write(
            boot_dir.join("initrd"),
            format!("modules={}\n", boot.initrd_modules.join(",")),
        )?,
    }
    for module_name in [
        "isofs.ko.xz",
        "bochs.ko.xz",
        "fat.ko.xz",
        "vfat.ko.xz",
        "nls_ascii.ko.xz",
        "nls_cp437.ko.xz",
    ] {
        if let Some(module) = kernel_entry
            .and_then(|entry| boot_artifact(entry, &[&format!("boot/modules/{module_name}")]))
        {
            fs::create_dir_all(boot_dir.join("modules"))?;
            link_or_copy_file(&module, &boot_dir.join("modules").join(module_name))?;
        }
    }
    fs::write(
        boot_dir.join("facts.json"),
        render_boot_facts(system, realized),
    )
}

fn cachyos_kernel_entry(realized: &[Store::StoreEntry]) -> Option<&Store::StoreEntry> {
    realized
        .iter()
        .find(|entry| entry.name == CACHYOS_KERNEL_PACKAGE)
}

fn boot_artifact(entry: &Store::StoreEntry, candidates: &[&str]) -> Option<PathBuf> {
    let out = Path::new(&entry.out);
    candidates
        .iter()
        .map(|rel| out.join(rel))
        .find(|path| path.is_file())
}

fn is_linux_kernel_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"\x7fELF")
        || (bytes.starts_with(b"MZ") && bytes.windows(4).any(|w| w == b"HdrS"))
}

fn is_initrd_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(&[0x1f, 0x8b]) || bytes.starts_with(b"070701") || bytes.starts_with(b"070702")
}

fn missing_kernel_source_files(entry: &Store::StoreEntry) -> Option<&'static str> {
    let out = Path::new(&entry.out);
    [
        "source/recipe.jet",
        "source/build.sh",
        "source/config",
        "source/patches.manifest",
        "source/initrd-inputs.manifest",
    ]
    .into_iter()
    .find(|rel| !out.join(rel).is_file())
}

fn render_boot_facts(system: &SystemPlan, realized: &[Store::StoreEntry]) -> String {
    let boot = boot_profile(system);
    let kernel_package = cachyos_kernel_entry(realized)
        .map(kernel_package_json)
        .unwrap_or_else(|| "null".to_string());
    let modules = boot
        .initrd_modules
        .iter()
        .map(|m| JSON::quote(m))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"loader\":{},\"kernel\":{},\"init\":{},\"initrd_modules\":[{}],\"kernel_package\":{}}}",
        JSON::quote(&boot.loader),
        JSON::quote(&boot.kernel),
        JSON::quote(&boot.init),
        modules,
        kernel_package
    )
}

fn kernel_package_json(entry: &Store::StoreEntry) -> String {
    let source = kernel_source_json(entry);
    format!(
        "{{\"name\":{},\"reference\":{},\"out\":{},\"output_hash\":{},\"provenance\":{},\"bootstrap\":\"source-built\",\"source_recipe\":{}}}",
        JSON::quote(&entry.name),
        JSON::quote(&entry.reference),
        JSON::quote(&entry.out),
        JSON::quote(&entry.envelope.output_hash),
        JSON::quote(&entry.envelope.provenance),
        source
    )
}

fn kernel_source_json(entry: &Store::StoreEntry) -> String {
    let out = Path::new(&entry.out);
    let facts = [
        ("recipe", "source/recipe.jet"),
        ("builder", "source/build.sh"),
        ("config", "source/config"),
        ("patches", "source/patches.manifest"),
        ("initrd_inputs", "source/initrd-inputs.manifest"),
    ]
    .iter()
    .map(|(name, rel)| {
        let path = out.join(rel);
        let path_text = path.display().to_string();
        let sha = fs::read(&path)
            .map(|bytes| crate::SHA256::sha256_hex(&bytes))
            .unwrap_or_else(|_| "<missing>".to_string());
        JSON::object_of(&[("name", name), ("path", &path_text), ("sha256", &sha)])
    })
    .collect::<Vec<_>>()
    .join(",");
    format!("{{\"mode\":\"source-built\",\"files\":[{}]}}", facts)
}
