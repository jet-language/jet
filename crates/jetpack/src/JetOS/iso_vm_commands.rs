fn build_hybrid_iso(staging: &Path, iso: &Path) -> Result<bool, String> {
    if find_path_tool("xorriso").is_none() || find_path_tool("limine").is_none() {
        return Ok(false);
    }
    let limine_data = Command::new("limine")
        .arg("--print-datadir")
        .output()
        .map_err(|e| format!("running limine --print-datadir failed: {e}"))?;
    if !limine_data.status.success() {
        return Err(format!(
            "limine --print-datadir exited with {}: {}",
            limine_data.status,
            String::from_utf8_lossy(&limine_data.stderr)
        ));
    }
    let data_dir = PathBuf::from(String::from_utf8_lossy(&limine_data.stdout).trim());
    let boot_dir = staging.join("boot");
    let efi_boot_dir = staging.join("EFI/BOOT");
    fs::create_dir_all(&efi_boot_dir)
        .map_err(|e| format!("creating EFI boot directory failed: {e}"))?;
    copy_file_replace(
        &data_dir.join("BOOTX64.EFI"),
        &efi_boot_dir.join("BOOTX64.EFI"),
    )
    .map_err(|e| format!("copying BOOTX64.EFI failed: {e}"))?;
    copy_file_replace(
        &data_dir.join("limine-bios.sys"),
        &boot_dir.join("limine-bios.sys"),
    )
    .map_err(|e| format!("copying limine-bios.sys failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), boot_dir.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying BOOTX64.EFI failed: {e}"))?;
    let efi_boot = staging.join("EFI/BOOT");
    fs::create_dir_all(&efi_boot).map_err(|e| format!("creating EFI boot dir failed: {e}"))?;
    fs::copy(data_dir.join("BOOTX64.EFI"), efi_boot.join("BOOTX64.EFI"))
        .map_err(|e| format!("copying EFI/BOOT/BOOTX64.EFI failed: {e}"))?;
    let efi_img = boot_dir.join("efiboot.img");
    let bootx64_len = fs::metadata(data_dir.join("BOOTX64.EFI"))
        .map_err(|e| format!("reading BOOTX64.EFI metadata failed: {e}"))?
        .len();
    let limine_len = fs::metadata(boot_dir.join("limine.conf"))
        .map_err(|e| format!("reading limine.conf metadata failed: {e}"))?
        .len();
    let kernel_len = fs::metadata(boot_dir.join("kernel"))
        .map_err(|e| format!("reading kernel metadata failed: {e}"))?
        .len();
    let initrd_len = fs::metadata(boot_dir.join("initrd"))
        .map_err(|e| format!("reading initrd metadata failed: {e}"))?
        .len();
    let min_efi_len = 96 * 1024 * 1024;
    let payload_len = bootx64_len + limine_len + (kernel_len * 2) + (initrd_len * 2);
    let efi_len = round_up_u64(
        min_efi_len.max(payload_len + 64 * 1024 * 1024),
        16 * 1024 * 1024,
    );
    let efi_file =
        fs::File::create(&efi_img).map_err(|e| format!("creating efiboot.img failed: {e}"))?;
    efi_file
        .set_len(efi_len)
        .map_err(|e| format!("sizing efiboot.img failed: {e}"))?;
    drop(efi_file);
    let mkfs = Command::new("mkfs.vfat")
        .args(["-n", "JETOS_EFI"])
        .arg(&efi_img)
        .output()
        .map_err(|e| format!("running mkfs.vfat failed: {e}"))?;
    if !mkfs.status.success() {
        return Err(format!(
            "mkfs.vfat exited with {}: {}",
            mkfs.status,
            String::from_utf8_lossy(&mkfs.stderr)
        ));
    }
    let mmd = Command::new("mmd")
        .args(["-i"])
        .arg(&efi_img)
        .args(["::/EFI", "::/EFI/BOOT", "::/boot"])
        .output()
        .map_err(|e| format!("running mmd failed: {e}"))?;
    if !mmd.status.success() {
        return Err(format!(
            "mmd exited with {}: {}",
            mmd.status,
            String::from_utf8_lossy(&mmd.stderr)
        ));
    }
    for (source, target) in [
        (data_dir.join("BOOTX64.EFI"), "::/EFI/BOOT/BOOTX64.EFI"),
        (boot_dir.join("limine.conf"), "::/boot/limine.conf"),
        (boot_dir.join("kernel"), "::/boot/kernel"),
        (boot_dir.join("initrd"), "::/boot/initrd"),
        (boot_dir.join("kernel"), "::/kernel"),
        (boot_dir.join("initrd"), "::/initrd"),
    ] {
        let mcopy = Command::new("mcopy")
            .args(["-i"])
            .arg(&efi_img)
            .arg(&source)
            .arg(target)
            .output()
            .map_err(|e| format!("running mcopy failed: {e}"))?;
        if !mcopy.status.success() {
            return Err(format!(
                "mcopy exited with {}: {}",
                mcopy.status,
                String::from_utf8_lossy(&mcopy.stderr)
            ));
        }
    }
    let xorriso = Command::new("xorriso")
        .args([
            "-as",
            "mkisofs",
            "-b",
            "boot/limine-bios.sys",
            "-no-emul-boot",
            "-boot-load-size",
            "4",
            "-boot-info-table",
            "--efi-boot",
            "boot/efiboot.img",
            "-efi-boot-part",
            "--efi-boot-image",
            "--protective-msdos-label",
            "-o",
        ])
        .arg(iso)
        .arg(staging)
        .output()
        .map_err(|e| format!("running xorriso failed: {e}"))?;
    if !xorriso.status.success() {
        return Err(format!(
            "xorriso exited with {}: {}",
            xorriso.status,
            String::from_utf8_lossy(&xorriso.stderr)
        ));
    }
    let limine = Command::new("limine")
        .args(["bios-install"])
        .arg(iso)
        .output()
        .map_err(|e| format!("running limine bios-install failed: {e}"))?;
    if !limine.status.success() {
        return Err(format!(
            "limine bios-install exited with {}: {}",
            limine.status,
            String::from_utf8_lossy(&limine.stderr)
        ));
    }
    Ok(true)
}

fn round_up_u64(value: u64, unit: u64) -> u64 {
    if unit == 0 {
        return value;
    }
    value.div_ceil(unit) * unit
}
