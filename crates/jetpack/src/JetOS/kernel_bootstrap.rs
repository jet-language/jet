use super::etc_boot_facts::{
    boot_artifact, cachyos_kernel_entry, is_initrd_image, is_linux_kernel_image,
    missing_kernel_source_files,
};
use super::generation_files::copy_profile_tree;
use super::store_realize::RealizedPackage;
use super::types::{BootProfile, CACHYOS_KERNEL_PACKAGE, SYSTEMD_INIT_PACKAGE};
use crate::Output::Theme;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn run_kernel_bootstrap_builder(
    theme: &Theme,
    boot: &BootProfile,
    realized: &mut [RealizedPackage],
    use_host_defaults: bool,
    generation_dir: &Path,
) -> bool {
    if boot.kernel != "CachyOS" {
        return true;
    }
    let Some(index) = realized
        .iter()
        .position(|entry| entry.name == CACHYOS_KERNEL_PACKAGE)
    else {
        return true;
    };
    let entry = &realized[index];
    if missing_kernel_source_files(entry).is_some() {
        return true;
    }
    let source_out = match entry.consumption_path(&entry.out) {
        Ok(path) => path,
        Err(_) => return false,
    };
    let out = generation_dir.join("kernel-build").join(&entry.name);
    if out.exists() {
        let _ = fs::remove_dir_all(&out);
    }
    if let Err(error) = copy_profile_tree(&source_out, &out) {
        theme.error_coded(
            "E1286",
            "jetos CachyOS source build failed",
            &format!("copying the leased kernel package into generation scratch failed: {error}."),
            "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
        );
        return false;
    }
    realized[index].set_consumption_override(out.clone());
    let script = out.join("source/build.sh");
    if !script.is_file() {
        return true;
    }
    let boot_dir = out.join("boot");
    if let Err(e) = fs::create_dir_all(&boot_dir).and_then(|_| make_tree_writable(&boot_dir)) {
        theme.error_coded(
            "E1286",
            "jetos CachyOS source build failed",
            &format!("could not create the cachyos-kernel boot artifact directory: {e}."),
            "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
        );
        return false;
    }
    let output = Command::new(&script)
        .current_dir(&out)
        .env("PATH", jetos_bootstrap_path())
        .env("JETOS_KERNEL_OUT", out.join("boot"))
        .env("JETOS_KERNEL_SOURCE", out.join("source"))
        .env("JETOS_KERNEL_PACKAGE", out)
        .envs(default_cachyos_kernel_env(use_host_defaults))
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) => {
            theme.error_coded(
                "E1286",
                "jetos CachyOS source build failed",
                &format!("running `source/build.sh` failed: {e}."),
                "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
            );
            return false;
        }
    };
    if !output.status.success() {
        theme.error_coded(
            "E1286",
            "jetos CachyOS source build failed",
            &format!(
                "`source/build.sh` exited with {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            "check the first-party cachyos-kernel source recipe and rerun `jet os build`.",
        );
        return false;
    }
    true
}

fn make_tree_writable(path: &Path) -> std::io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_readonly(false);
    fs::set_permissions(path, perms)?;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            make_tree_writable(&entry.path())?;
        }
    }
    Ok(())
}

fn jetos_bootstrap_path() -> String {
    let existing = std::env::var("PATH").unwrap_or_default();
    let defaults = "/run/current-system/sw/bin:/usr/bin:/bin:/usr/sbin:/sbin";
    if existing.is_empty() {
        defaults.to_string()
    } else {
        format!("{defaults}:{existing}")
    }
}

fn default_cachyos_kernel_env(use_host_defaults: bool) -> Vec<(&'static str, PathBuf)> {
    let mut env = Vec::new();
    if !use_host_defaults {
        return env;
    }
    if std::env::var_os("JETOS_CACHYOS_KERNEL").is_none() {
        if let Some(kernel) = first_existing_path(&[
            "/run/booted-system/kernel",
            "/run/current-system/kernel",
        ]) {
            env.push(("JETOS_CACHYOS_KERNEL", kernel));
        }
    }
    if std::env::var_os("JETOS_CACHYOS_INITRD").is_none() {
        if let Some(initrd) = first_existing_path(&[
            "/run/booted-system/initrd",
            "/run/current-system/initrd",
        ]) {
            env.push(("JETOS_CACHYOS_INITRD", initrd));
        }
    }
    if std::env::var_os("JETOS_CACHYOS_MODULES").is_none() {
        if let Some(modules) = first_existing_path(&[
            "/run/booted-system/kernel-modules",
            "/run/current-system/kernel-modules",
        ]) {
            env.push((
                "JETOS_CACHYOS_MODULES",
                kernel_module_tree(&modules).unwrap_or(modules),
            ));
        }
    }
    env
}

fn kernel_module_tree(modules: &Path) -> Option<PathBuf> {
    let lib_modules = modules.join("lib/modules");
    fs::read_dir(lib_modules)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("kernel").is_dir())
}

fn first_existing_path(paths: &[&str]) -> Option<PathBuf> {
    paths
        .iter()
        .map(PathBuf::from)
        .find(|path| path.exists())
}

pub(super) fn validate_boot_payloads(
    theme: &Theme,
    boot: &BootProfile,
    realized: &[RealizedPackage],
) -> bool {
    if boot.kernel == "CachyOS" {
        let Some(entry) = cachyos_kernel_entry(realized) else {
            return true;
        };
        let kernel = boot_artifact(entry, &["boot/vmlinuz-cachyos", "bzImage", "vmlinuz"]);
        let initrd = boot_artifact(entry, &["boot/initrd-cachyos", "initrd", "initrd.img"]);
        let kernel_ok = kernel
            .as_ref()
            .map(|path| is_linux_kernel_image(path))
            .unwrap_or(false);
        let initrd_ok = initrd
            .as_ref()
            .map(|path| is_initrd_image(path))
            .unwrap_or(false);
        if !kernel_ok || !initrd_ok {
            theme.error_coded(
                "E1282",
                "jetos CachyOS boot artifacts are missing",
                "D-JOS-KERNELSRC1=A: the first-party `cachyos-kernel` package must provide a Linux kernel image and initrd with bootable file headers so the generation and installer can boot the same payload.",
                "add boot/vmlinuz-cachyos and boot/initrd-cachyos with real boot payloads, or select a different ratified kernel.",
            );
            return false;
        }
        if missing_kernel_source_files(entry).is_some() {
            theme.error_coded(
                "E1284",
                "jetos CachyOS source recipe is missing",
                "D-JOS-KERNELBOOTSTRAP1=A: the first-party `cachyos-kernel` package must carry source-built recipe, builder, config, patch, and initrd-input provenance beside the boot artifacts.",
                "add source/recipe.jet, source/build.sh, source/config, source/patches.manifest, and source/initrd-inputs.manifest to the package output.",
            );
            return false;
        }
    }
    if boot.init == "/sbin/init" {
        let Some(entry) = realized
            .iter()
            .find(|entry| entry.name == SYSTEMD_INIT_PACKAGE)
        else {
            return true;
        };
        if boot_artifact(entry, &["bin/systemd", "lib/systemd/systemd", "sbin/init"]).is_none() {
            theme.error_coded(
                "E1283",
                "jetos systemd init artifact is missing",
                "D-JPK-OSINIT1=A: the first-party `systemd` package must provide a bootable init binary for `/sbin/init`.",
                "add bin/systemd, lib/systemd/systemd, or sbin/init to the package output, or select a ratified init override.",
            );
            return false;
        }
    }
    true
}
