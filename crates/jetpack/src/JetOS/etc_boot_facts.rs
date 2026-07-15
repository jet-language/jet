use super::generation_files::sanitize_runtime_branding_file;
use super::identity::{jetos_release_label, render_jetos_os_release, write_jetos_identity_assets};
use super::options_rendering::{
    boot_profile, collect_names, option_value, package_path_or_literal, parse_list_items,
};
use super::root_projection::copy_file_replace;
use super::store_realize::RealizedPackage;
use super::types::CACHYOS_KERNEL_PACKAGE;
use jet_env_model::ModuleEval::SystemPlan;
use crate::JSON;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn write_etc_tree(dir: &Path, system: &SystemPlan) -> std::io::Result<()> {
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
    write_identity_files(dir, &etc, system)?;
    write_jetos_identity_assets(dir)
}

fn write_identity_files(dir: &Path, etc: &Path, system: &SystemPlan) -> std::io::Result<()> {
    let users = collect_names(system, "users");
    let groups = collect_names(system, "groups");
    let mut passwd = String::from("root:x:0:0:root:/root:/bin/sh\n");
    let mut shadow = String::from("root:!:1::::::\n");
    let mut group = String::from("root:x:0:\nmessagebus:x:81:\ngdm:x:120:\nvideo:x:44:\nrender:x:303:\ninput:x:304:\naudio:x:18:\n");
    let mut sysusers = String::new();
    passwd.push_str("messagebus:x:81:81:D-Bus system message bus:/run/dbus:/usr/sbin/nologin\n");
    passwd.push_str("gdm:x:120:120:GDM display manager:/var/lib/gdm:/usr/sbin/nologin\n");
    for (idx, user) in users.iter().enumerate() {
        let uid = 1000 + idx;
        let home = option_value(system, &[&format!("users.{user}.home")])
            .unwrap_or_else(|| format!("/home/{user}"));
        let shell = option_value(system, &[&format!("users.{user}.shell")])
            .map(|s| package_path_or_literal(&s))
            .unwrap_or_else(|| "/run/current-system/sw/bin/sh".to_string());
        passwd.push_str(&format!("{user}:x:{uid}:{uid}:{user}:{home}:{shell}\n"));
        group.push_str(&format!("{user}:x:{uid}:{user}\n"));
        shadow.push_str(&format!("{user}:!:1::::::\n"));
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
    fs::write(etc.join("shadow"), shadow)?;
    let os_release = render_jetos_os_release(false);
    fs::write(etc.join("os-release"), &os_release)?;
    let usr_lib = dir.join("usr/lib");
    fs::create_dir_all(&usr_lib)?;
    fs::write(usr_lib.join("os-release"), &os_release)?;
    fs::write(
        etc.join("machine-id"),
        format!("{}\n", &crate::SHA256::sha256_hex(system.name.as_bytes())[..32]),
    )?;
    fs::write(
        etc.join("nsswitch.conf"),
        "passwd: files\ngroup: files\nshadow: files\nhosts: files dns\nservices: files\n",
    )?;
    write_pam_files(etc)?;
    let sysusers_dir = etc.join("sysusers.d");
    fs::create_dir_all(&sysusers_dir)?;
    fs::write(sysusers_dir.join("jetos.conf"), sysusers)
}

fn write_pam_files(etc: &Path) -> std::io::Result<()> {
    let pam = etc.join("pam.d");
    fs::create_dir_all(&pam)?;
    let login = "auth sufficient pam_unix.so nullok\naccount sufficient pam_unix.so\npassword sufficient pam_unix.so nullok\nsession required pam_unix.so\nsession optional pam_systemd.so\n";
    for name in [
        "login",
        "gdm",
        "gdm-password",
        "gdm-launch-environment",
        "polkit-1",
        "system-local-login",
    ] {
        fs::write(pam.join(name), login)?;
    }
    Ok(())
}

pub(super) fn write_boot_facts(
    dir: &Path,
    system: &SystemPlan,
    realized: &[RealizedPackage],
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
            "timeout: 5\nserial: yes\ngraphics: no\nverbose: yes\n/{} — {}\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 root=LABEL=jetos-root rw init={}\n",
            jetos_release_label(false), system.name, boot.init
        ),
    )?;
    if kernel_path.is_file() {
        copy_file_replace(&kernel_path, &boot_dir.join("kernel"))?;
        sanitize_runtime_branding_file(&boot_dir.join("kernel"))?;
    } else {
        fs::write(
            boot_dir.join("kernel"),
            format!("{}\n", kernel_path.display()),
        )?;
    }
    match initrd_path {
        Some(path) if path.is_file() => {
            copy_file_replace(&path, &boot_dir.join("initrd"))?;
            sanitize_runtime_branding_file(&boot_dir.join("initrd"))?;
        }
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
        "serio.ko.xz",
        "i8042.ko.xz",
        "libps2.ko.xz",
        "atkbd.ko.xz",
        "hid-generic.ko.xz",
        "usbhid.ko.xz",
        "uhci-hcd.ko.xz",
        "ehci-hcd.ko.xz",
        "xhci-hcd.ko.xz",
    ] {
        if let Some(module) = kernel_entry
            .and_then(|entry| boot_artifact(entry, &[&format!("boot/modules/{module_name}")]))
        {
            fs::create_dir_all(boot_dir.join("modules"))?;
            copy_file_replace(&module, &boot_dir.join("modules").join(module_name))?;
        }
    }
    fs::write(
        boot_dir.join("facts.json"),
        render_boot_facts(system, realized),
    )
}

pub(super) fn cachyos_kernel_entry(realized: &[RealizedPackage]) -> Option<&RealizedPackage> {
    realized
        .iter()
        .find(|entry| entry.name == CACHYOS_KERNEL_PACKAGE)
}

pub(super) fn boot_artifact(entry: &RealizedPackage, candidates: &[&str]) -> Option<PathBuf> {
    let out = entry.consumption_path(&entry.out).ok()?;
    candidates
        .iter()
        .map(|rel| out.join(rel))
        .find(|path| path.is_file())
}

pub(super) fn is_linux_kernel_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"\x7fELF")
        || (bytes.starts_with(b"MZ") && bytes.windows(4).any(|w| w == b"HdrS"))
}

pub(super) fn is_initrd_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(&[0x1f, 0x8b]) || bytes.starts_with(b"070701") || bytes.starts_with(b"070702")
}

pub(super) fn missing_kernel_source_files(entry: &RealizedPackage) -> Option<&'static str> {
    let out = entry.consumption_path(&entry.out).ok()?;
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

fn render_boot_facts(system: &SystemPlan, realized: &[RealizedPackage]) -> String {
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

pub(super) fn kernel_package_json(entry: &RealizedPackage) -> String {
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

fn kernel_source_json(entry: &RealizedPackage) -> String {
    // Provenance names the durable provider output, never the process-scoped
    // lease projection used while copying runtime files into a generation.
    let out = entry.original_output();
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
