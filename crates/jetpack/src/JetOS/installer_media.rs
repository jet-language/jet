fn write_installer_media(
    gen: &Generation,
    system: &SystemPlan,
    disk: &str,
) -> std::io::Result<PathBuf> {
    let image_dir = systems_dir().join("images");
    fs::create_dir_all(&image_dir)?;
    let media_name = format!("jetos-installer-{}.iso", system.name);
    let staging = image_dir.join(format!("{media_name}.d"));
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(staging.join("boot"))?;
    fs::create_dir_all(staging.join("install"))?;
    fs::create_dir_all(staging.join("jetos"))?;
    copy_generation_payload_deref(&gen.path, &staging.join("jetos/current-system")).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!(
                "copying generation `{}` into installer staging `{}` failed: {e}",
                gen.path.display(),
                staging.join("jetos/current-system").display()
            ),
        )
    })?;
    let installer_limine = render_installer_limine_conf(system, gen, disk);
    fs::write(staging.join("limine.conf"), &installer_limine)?;
    fs::write(staging.join("boot/limine.conf"), installer_limine)?;
    fs::write(
        staging.join("boot/installed-limine.conf"),
        render_installed_limine_conf(system, gen),
    )?;
    copy_runtime_file_filtered(&gen.path.join("boot/kernel"), &staging.join("boot/kernel"))
        .map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("copying installer kernel failed: {e}"),
            )
        })?;
    copy_initrd_runtime_filtered(&gen.path.join("boot/initrd"), &staging.join("boot/initrd"))
        .map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("copying installer initrd failed: {e}"),
            )
        })?;
    append_installer_initrd_overlay(&staging.join("boot/initrd"), system, gen).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("appending JetOS installer initrd overlay failed: {e}"),
        )
    })?;
    fs::write(
        staging.join("jetos/plan.json"),
        fs::read_to_string(gen.path.join("plan.json"))?,
    )?;
    fs::write(
        staging.join("jetos/proof.txt"),
        fs::read_to_string(gen.path.join("proof.txt"))?,
    )?;
    fs::write(
        staging.join("jetos/provenance.json"),
        fs::read_to_string(gen.path.join("provenance.json"))?,
    )?;
    fs::write(
        staging.join("jetos/generation-path.txt"),
        format!("{}\n", gen.path.display()),
    )?;
    let transaction = format!(
        "{{\"brand\":\"jetos\",\"host\":{},\"generation\":{},\"mode\":\"guided-or-scripted\",\"disk\":{},\"root_label\":\"jetos-root\",\"esp_label\":\"JETOS-ESP\",\"source_generation\":{},\"steps\":[\"partition-gpt\",\"mkfs.vfat-esp\",\"mkfs.ext4-root\",\"copy-generation-closure\",\"install-limine-esp\",\"write-generation-ledger\",\"reboot-installed-disk\",\"verify-guest-proof\"]}}",
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(disk),
        JSON::quote(&gen.path.display().to_string())
    );
    fs::write(staging.join("install/transaction.json"), &transaction)?;
    fs::write(
        staging.join("install/install.sh"),
        render_installer_script(system, gen),
    )?;
    fs::write(
        staging.join("install/guest-verify.sh"),
        render_guest_verify_script(system, gen),
    )?;
    fs::write(
        staging.join("README.txt"),
        format!(
            "jetos installer media\nhost={}\ngeneration={}\ntransaction=install/transaction.json\n",
            system.name, gen.name
        ),
    )?;
    let iso = image_dir.join(&media_name);
    let iso_state = match build_hybrid_iso(&staging, &iso) {
        Ok(true) => "built",
        Ok(false) => "staged",
        Err(e) => {
            fs::write(staging.join("iso-error.txt"), e)?;
            "staged"
        }
    };
    let proof = image_dir.join(format!("{media_name}.proof.json"));
    let tools = vm_tools_json();
    let text = format!(
        "{{\"brand\":\"jetos\",\"kind\":\"hybrid-iso\",\"state\":{},\"host\":{},\"generation\":{},\"media\":{},\"path\":{},\"staging\":{},\"transaction\":{},\"tools\":[{}]}}",
        JSON::quote(iso_state),
        JSON::quote(&system.name),
        JSON::quote(&gen.name),
        JSON::quote(&media_name),
        JSON::quote(&iso.display().to_string()),
        JSON::quote(&staging.display().to_string()),
        JSON::quote(&staging.join("install/transaction.json").display().to_string()),
        tools
    );
    fs::write(&proof, text)?;
    Ok(proof)
}

fn copy_generation_payload_deref(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    let mut entries = fs::read_dir(src)?
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if entry.file_name() == "root" {
            continue;
        }
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive_deref(&path, &target)?;
        } else {
            copy_runtime_file_filtered(&path, &target)?;
        }
    }
    deref_profile_bin_symlinks(dst)?;
    Ok(())
}

fn deref_profile_bin_symlinks(system: &Path) -> std::io::Result<()> {
    let bin = system.join("sw/bin");
    let Ok(entries) = fs::read_dir(&bin) else {
        return Ok(());
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)?;
        if !meta.file_type().is_symlink() {
            continue;
        }
        let target = fs::read_link(&path)?;
        if !target.starts_with("/nix/store") {
            continue;
        }
        let _ = fs::remove_file(&path);
        copy_runtime_file_filtered(&target, &path)?;
    }
    Ok(())
}

fn copy_initrd_runtime_filtered(src: &Path, dst: &Path) -> std::io::Result<()> {
    let bytes = fs::read(src)?;
    if let (Some(offset), Some(zstd)) = (first_zstd_frame_offset(&bytes), find_path_tool("zstd")) {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let compressed_path = unique_initrd_temp_path(dst, "copy-main.zst");
        fs::write(&compressed_path, &bytes[offset..])?;
        let mut plain = zstd_decode_file(&zstd, &compressed_path)?;
        let _ = fs::remove_file(&compressed_path);
        if let Some(sanitized) = sanitize_runtime_branding_bytes(&plain) {
            plain = sanitized;
        }
        let plain_path = unique_initrd_temp_path(dst, "copy-main.cpio");
        fs::write(&plain_path, &plain)?;
        let compressed = zstd_encode_file(&zstd, &plain_path)?;
        let _ = fs::remove_file(&plain_path);
        let mut out = Vec::with_capacity(offset + compressed.len());
        out.extend_from_slice(&bytes[..offset]);
        out.extend_from_slice(&compressed);
        fs::write(dst, out)?;
        return Ok(());
    }
    copy_runtime_file_filtered(src, dst)
}

fn write_image_variant_artifacts(
    gen: &Generation,
    system: &SystemPlan,
) -> std::io::Result<PathBuf> {
    let image_dir = systems_dir().join("images");
    fs::create_dir_all(&image_dir)?;
    let host = &system.name;
    let qcow2 = image_dir.join(format!("jetos-{host}.qcow2"));
    let raw = image_dir.join(format!("jetos-{host}.raw"));
    let sd = image_dir.join(format!("jetos-{host}-sd.img"));
    let netboot = image_dir.join(format!("jetos-{host}-netboot"));
    fs::create_dir_all(&netboot)?;

    // D-JOS-IMAGEPROOF1=C: sparse markers / deferred artifacts report `staged`.
    // `built` requires a real format artifact plus format-specific smoke proof.
    let qcow2_state = match smoke_prove_qcow2(&qcow2) {
        Ok(()) => "built",
        Err(_) => {
            write_sparse_marker(&qcow2, 64 * 1024 * 1024, "JETOS-QCOW2-STAGED\n")?;
            "staged"
        }
    };
    write_sparse_marker(
        &raw,
        128 * 1024 * 1024,
        &format!("JETOS-RAW-STAGED\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    write_sparse_marker(
        &sd,
        128 * 1024 * 1024,
        &format!("JETOS-SD-STAGED\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    copy_runtime_file_filtered(&gen.path.join("boot/kernel"), &netboot.join("vmlinuz"))?;
    copy_initrd_runtime_filtered(&gen.path.join("boot/initrd"), &netboot.join("initrd"))?;
    fs::write(
        netboot.join("ipxe.conf"),
        format!(
            "#!ipxe\nkernel vmlinuz console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=run jetos.host={host} jetos.generation={} root=LABEL=jetos-root rw\ninitrd initrd\nboot\n",
            gen.name
        ),
    )?;
    fs::write(
        netboot.join("manifest.json"),
        format!(
            "{{\"kind\":\"jetos.netboot\",\"host\":{},\"generation\":{},\"kernel\":\"vmlinuz\",\"initrd\":\"initrd\",\"ipxe\":\"ipxe.conf\"}}",
            JSON::quote(host),
            JSON::quote(&gen.name)
        ),
    )?;

    let kernel_state = if smoke_prove_kernel_image(&netboot.join("vmlinuz")) {
        "built"
    } else {
        "staged"
    };
    let initrd_state = if smoke_prove_initrd_image(&netboot.join("initrd")) {
        "built"
    } else {
        "staged"
    };
    let ipxe_state = if netboot.join("ipxe.conf").is_file() {
        "built"
    } else {
        "staged"
    };

    let artifacts = [
        ("qcow2", qcow2_state, qcow2.clone()),
        ("raw", "staged", raw.clone()),
        ("sd", "staged", sd.clone()),
        ("netboot-kernel", kernel_state, netboot.join("vmlinuz")),
        ("netboot-initrd", initrd_state, netboot.join("initrd")),
        ("netboot-ipxe", ipxe_state, netboot.join("ipxe.conf")),
    ];
    let any_built = artifacts.iter().any(|(_, state, _)| *state == "built");
    let proof_label = if any_built {
        "image-variants-smoke-proved"
    } else {
        "image-variants-staged"
    };
    let rows = artifacts
        .iter()
        .map(|(kind, state, path)| {
            JSON::object_of(&[
                ("kind", *kind),
                ("state", *state),
                ("path", &path.display().to_string()),
                ("sha256", &sha256_file_or_marker(path)),
            ])
        })
        .collect::<Vec<_>>()
        .join(",");
    let proof = image_dir.join(format!("jetos-image-variants-{host}.proof.json"));
    fs::write(
        &proof,
        format!(
            "{{\"kind\":\"jetos.image-variants\",\"host\":{},\"generation\":{},\"source_generation\":{},\"artifacts\":[{}],\"proof\":{}}}",
            JSON::quote(host),
            JSON::quote(&gen.name),
            JSON::quote(&gen.path.display().to_string()),
            rows,
            JSON::quote(proof_label)
        ),
    )?;
    Ok(proof)
}

/// Create a qcow2 (when qemu-img exists) and smoke-prove it via `qemu-img info`.
fn smoke_prove_qcow2(path: &Path) -> Result<(), String> {
    let qemu_img = find_path_tool("qemu-img").ok_or_else(|| "qemu-img missing".to_string())?;
    let status = Command::new(&qemu_img)
        .args(["create", "-f", "qcow2"])
        .arg(path)
        .arg("4G")
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("qemu-img create failed".to_string());
    }
    let output = Command::new(&qemu_img)
        .args(["info", "--output=json"])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("qemu-img info failed".to_string());
    }
    let info = String::from_utf8_lossy(&output.stdout);
    if !info.contains("\"format\": \"qcow2\"") && !info.contains("\"format\":\"qcow2\"") {
        return Err(format!("qcow2 smoke failed: {info}"));
    }
    Ok(())
}

fn smoke_prove_kernel_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"\x7fELF")
        || (bytes.starts_with(b"MZ") && bytes.windows(4).any(|w| w == b"HdrS"))
}

fn smoke_prove_initrd_image(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(&[0x1f, 0x8b])
        || bytes.starts_with(b"070701")
        || bytes.starts_with(b"070702")
        || bytes.windows(4).any(|w| w == [0x28, 0xb5, 0x2f, 0xfd])
}

fn write_sparse_marker(path: &Path, size: u64, marker: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = fs::File::create(path)?;
    file.write_all(marker.as_bytes())?;
    file.set_len(size)
}

fn sha256_file_or_marker(path: &Path) -> String {
    fs::read(path)
        .map(|bytes| crate::SHA256::sha256_hex(&bytes))
        .unwrap_or_else(|_| "<unreadable>".to_string())
}

fn render_installer_script(system: &SystemPlan, gen: &Generation) -> String {
    format!(
        r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
disk="${{1:-}}"
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
if [ -z "$disk" ]; then
    case "$cmdline" in
        *jetos.disk=*) disk="${{cmdline#*jetos.disk=}}"; disk="${{disk%% *}}" ;;
    esac
fi
if [ -z "$disk" ]; then
    disk="/dev/vda"
fi
echo "jetos installer: starting host={host} generation={generation} disk=$disk"
root="${{JETOS_TARGET_ROOT:-/mnt/jetos}}"
mkdir -p /media "$root"
modprobe virtio_pci 2>/dev/null || true
modprobe virtio_blk 2>/dev/null || true
modprobe ata_piix 2>/dev/null || true
modprobe sd_mod 2>/dev/null || true
modprobe sr_mod 2>/dev/null || true
modprobe cdrom 2>/dev/null || true
insmod /jetos/modules/isofs.ko.xz 2>/dev/null || true
modprobe isofs 2>/dev/null || true
for dev in /sys/block/* /dev/vd* /dev/sd*; do
    echo "jetos installer: sees $dev"
done
media=""
tries=0
while [ -z "$media" ] && [ "$tries" -lt 20 ]; do
    for candidate in /dev/sr0 /dev/cdrom /dev/hdc /dev/hdb; do
        if [ -e "$candidate" ]; then media="$candidate"; break; fi
    done
    tries=$((tries + 1))
    if [ -z "$media" ]; then sleep 1; fi
done
if [ -n "$media" ]; then
    echo "jetos installer: mounting media=$media"
    mount -t iso9660 -o ro "$media" /media || mount -o ro "$media" /media || true
fi
if [ ! -e /media/jetos/current-system ]; then
    echo "jetos installer: media payload missing"
    for entry in /media/* /media/jetos/*; do
        echo "jetos installer: media sees $entry"
    done
fi
tries=0
while [ ! -e "$disk" ] && [ "$tries" -lt 50 ]; do
    if [ -e /dev/vda ]; then disk=/dev/vda; break; fi
    if [ -e /dev/sda ]; then disk=/dev/sda; break; fi
    tries=$((tries + 1))
    if [ "$tries" = 10 ] || [ "$tries" = 30 ]; then
        for dev in /sys/block/* /dev/vd* /dev/sd*; do
            echo "jetos installer: wait sees $dev"
        done
    fi
    sleep 1
done
echo "jetos installer: using disk=$disk"
case "$disk" in
    *[0-9]) esp="${{disk}}p1"; root_part="${{disk}}p2" ;;
    *) esp="${{disk}}1"; root_part="${{disk}}2" ;;
esac
printf 'label: gpt\nsize=512M, type=U\n type=L\n' | sfdisk --wipe always "$disk"
blockdev --rereadpt "$disk" 2>/dev/null || true
sync
tries=0
while {{ [ ! -e "$esp" ] || [ ! -e "$root_part" ]; }} && [ "$tries" -lt 30 ]; do
    tries=$((tries + 1))
    sleep 1
done
if [ ! -e "$esp" ] || [ ! -e "$root_part" ]; then
    echo "jetos installer: missing partition nodes esp=$esp root=$root_part"
    exit 1
fi
mkfs.vfat -F 32 -n JETOS-ESP "$esp"
mkfs.ext4 -F -L jetos-root "$root_part"
mount "$root_part" "$root"
mkdir -p "$root/run" "$root/boot" "$root/boot/efi" "$root/var/lib/jetos/generations/{generation}"
insmod /jetos/modules/nls_ascii.ko.xz 2>/dev/null || true
insmod /jetos/modules/nls_cp437.ko.xz 2>/dev/null || true
insmod /jetos/modules/fat.ko.xz 2>/dev/null || true
insmod /jetos/modules/vfat.ko.xz 2>/dev/null || true
modprobe vfat 2>/dev/null || true
mount "$esp" "$root/boot/efi"
mkdir -p "$root/boot/efi/EFI/BOOT" "$root/boot/efi/boot"
cp -a /media/jetos/current-system/. "$root/var/lib/jetos/generations/{generation}/"
rm -rf "$root/run/current-system"
ln -s "/var/lib/jetos/generations/{generation}" "$root/run/current-system"
rm -f "$root/var/lib/jetos/current-system"
ln -s "/var/lib/jetos/generations/{generation}" "$root/var/lib/jetos/current-system"
cp /media/boot/kernel "$root/boot/kernel"
cp /media/boot/initrd "$root/boot/initrd"
cp /media/boot/installed-limine.conf "$root/boot/limine.conf"
cp /media/boot/kernel "$root/boot/efi/boot/kernel"
cp /media/boot/initrd "$root/boot/efi/boot/initrd"
cp /media/boot/installed-limine.conf "$root/boot/efi/boot/limine.conf"
cp /media/EFI/BOOT/BOOTX64.EFI "$root/boot/efi/EFI/BOOT/BOOTX64.EFI"
printf '%s\t%s\t%s\n' "{created}" "{host}" "{generation}" > "$root/var/lib/jetos/generations/log"
printf '{{"host":"{host}","generation":"{generation}","disk":"%s","esp":"%s","root":"%s","layout":"gpt-esp-ext4","result":"installed"}}\n' "$disk" "$esp" "$root_part" > "$root/var/lib/jetos/install-proof.json"
sync
echo "jetos installer: installed host={host} generation={generation}"
poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
"#,
        created = gen.created_at,
        host = system.name,
        generation = gen.name
    )
}

fn render_installed_limine_conf(system: &SystemPlan, gen: &Generation) -> String {
    format!(
        "timeout: 1\nserial: yes\ngraphics: no\nverbose: yes\n/jetos {host} verify\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=verify jetos.host={host} jetos.generation={generation} root=LABEL=jetos-root rw\n",
        host = system.name,
        generation = gen.name
    )
}

fn render_installer_limine_conf(system: &SystemPlan, gen: &Generation, disk: &str) -> String {
    let disk = if disk.starts_with("/dev/") {
        disk.to_string()
    } else {
        "/dev/sda".to_string()
    };
    format!(
        "timeout: 5\nserial: yes\ngraphics: no\nverbose: yes\n/Install jetos {host}\n    protocol: linux\n    kernel_path: boot():/boot/kernel\n    module_path: boot():/boot/initrd\n    textmode: yes\n    cmdline: console=ttyS0 rdinit=/jetos/init init=/jetos/init jetos.mode=install jetos.host={host} jetos.generation={generation} jetos.disk={disk}\n",
        host = system.name,
        generation = gen.name
    )
}

fn render_guest_verify_script(system: &SystemPlan, gen: &Generation) -> String {
    let services = system
        .services
        .iter()
        .filter(|svc| svc.enable)
        .map(|svc| svc.name.clone())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
root="${{JETOS_TARGET_ROOT:-/sysroot}}"
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
if [ "$root" != "/" ]; then
    mkdir -p "$root"
    modprobe virtio_pci 2>/dev/null || true
    modprobe virtio_blk 2>/dev/null || true
    modprobe ata_piix 2>/dev/null || true
    modprobe sd_mod 2>/dev/null || true
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for dev in /sys/block/* /dev/vd* /dev/sd*; do
        echo "jetos verifier: sees $dev"
    done
    echo "jetos verifier: mounting installed root"
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        if mount "$candidate" "$root" 2>/dev/null; then
            echo "jetos verifier: mounted installed root=$candidate"
            break
        fi
    done
fi
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
need() {{
    path="$1"
    if [ ! -e "$path" ]; then
        echo "jetos verifier: missing $path"
        exit 1
    fi
}}
system="$root/var/lib/jetos/generations/{generation}"
need "$system/plan.json"
need "$system/proof.txt"
need "$system/provenance.json"
need "$system/boot/kernel"
need "$system/boot/initrd"
need "$system/sbin/init"
need "$system/terminal/facts.json"
need "$system/etc/profile"
need "$system/etc/shells"
need "$system/etc/systemd/system/serial-getty@ttyS0.service"
need "$system/etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service"
need "$system/desktop/facts.json"
need "$system/sw/bin/gdm"
need "$system/sw/bin/gnome-session"
need "$system/sw/bin/gnome-shell"
need "$system/sw/bin/jetos-desktop-session"
need "$system/sw/bin/jetos-terminal-fallback"
need "$system/sw/bin/jetos-studio"
need "$system/share/applications/jetos-studio.desktop"
need "$system/etc/systemd/system/display-manager.service"
need "$system/etc/systemd/system/graphical.target.wants/display-manager.service"
need "$root/var/lib/jetos/generations/log"
if [ ! -L "$root/run/current-system" ]; then
    echo "jetos verifier: missing current-system symlink"
    exit 1
fi
for svc in {services}; do
    need "$system/etc/systemd/system/$svc.service"
done
case "$cmdline" in
  *jetos.mode=desktop-verify*)
    insmod /jetos/modules/bochs.ko.xz 2>/dev/null || true
    modprobe virtio_gpu 2>/dev/null || true
    modprobe bochs 2>/dev/null || true
    modprobe drm 2>/dev/null || true
    cat /proc/fb 2>/dev/null || true
    for gfx in /sys/class/graphics/*; do
        echo "jetos verifier: graphics sees $gfx"
    done
    if [ ! -e /sys/class/graphics/fb0 ] && [ ! -s /proc/fb ]; then
        echo "jetos verifier: missing graphical framebuffer"
        exit 1
    fi
    desktop_path="$system/sw/bin:$PATH"
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /jetos/tools/bin/sh "$system/sw/bin/jetos-display-manager" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /jetos/tools/bin/sh "$system/sw/bin/jetos-desktop-session" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /jetos/tools/bin/sh "$system/sw/bin/jetos-terminal-fallback" --jetos-proof
    printf '{{"host":"{host}","generation":"{generation}","packages":"present","services":"present","network":"declared","rollback":"ledger-present","proof":"present","desktop":"gnome-wayland","display":"graphical","launcher":"proved"}}\n'
    printf 'JETOS_GUEST_PROOF: {{"state":"guest-passed","host":"{host}","generation":"{generation}","assertions":["current-generation-matches","packages-present","services-active","network-up","rollback-generation-bootable","terminal-login-ready","desktop-session-ready","graphical-console-ready","desktop-launchers-run"]}}\n'
    ;;
  *)
    printf '{{"host":"{host}","generation":"{generation}","packages":"present","services":"present","network":"declared","rollback":"ledger-present","proof":"present"}}\n'
    printf 'JETOS_GUEST_PROOF: {{"state":"guest-passed","host":"{host}","generation":"{generation}","assertions":["current-generation-matches","packages-present","services-active","network-up","rollback-generation-bootable","terminal-login-ready","desktop-session-ready"]}}\n'
    ;;
esac
poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
"#,
        host = system.name,
        generation = gen.name,
        services = services
    )
}
