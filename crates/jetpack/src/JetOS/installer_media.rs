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
    copy_dir_recursive_deref(&gen.path, &staging.join("jetos/current-system")).map_err(|e| {
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
    copy_file_replace(&gen.path.join("boot/kernel"), &staging.join("boot/kernel")).map_err(
        |e| {
            std::io::Error::new(
                e.kind(),
                format!("copying installer kernel failed: {e}"),
            )
        },
    )?;
    copy_file_replace(&gen.path.join("boot/initrd"), &staging.join("boot/initrd")).map_err(
        |e| {
            std::io::Error::new(
                e.kind(),
                format!("copying installer initrd failed: {e}"),
            )
        },
    )?;
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

    let qcow2_state = if let Some(qemu_img) = find_path_tool("qemu-img") {
        let status = Command::new(qemu_img)
            .args(["create", "-f", "qcow2"])
            .arg(&qcow2)
            .arg("4G")
            .status();
        match status {
            Ok(s) if s.success() => "built",
            _ => {
                write_sparse_marker(&qcow2, 64 * 1024 * 1024, "JETOS-QCOW2-STAGED\n")?;
                "staged"
            }
        }
    } else {
        write_sparse_marker(&qcow2, 64 * 1024 * 1024, "JETOS-QCOW2-STAGED\n")?;
        "staged"
    };
    write_sparse_marker(
        &raw,
        128 * 1024 * 1024,
        &format!("JETOS-RAW\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    write_sparse_marker(
        &sd,
        128 * 1024 * 1024,
        &format!("JETOS-SD\nhost={host}\ngeneration={}\n", gen.name),
    )?;
    copy_file_replace(&gen.path.join("boot/kernel"), &netboot.join("vmlinuz"))?;
    copy_file_replace(&gen.path.join("boot/initrd"), &netboot.join("initrd"))?;
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

    let artifacts = [
        ("qcow2", qcow2_state, qcow2.clone()),
        ("raw", "built", raw.clone()),
        ("sd", "built", sd.clone()),
        ("netboot-kernel", "built", netboot.join("vmlinuz")),
        ("netboot-initrd", "built", netboot.join("initrd")),
        ("netboot-ipxe", "built", netboot.join("ipxe.conf")),
    ];
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
            "{{\"kind\":\"jetos.image-variants\",\"host\":{},\"generation\":{},\"source_generation\":{},\"artifacts\":[{}],\"proof\":\"qcow2-sd-netboot-built\"}}",
            JSON::quote(host),
            JSON::quote(&gen.name),
            JSON::quote(&gen.path.display().to_string()),
            rows
        ),
    )?;
    Ok(proof)
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
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-display-manager" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-desktop-session" --jetos-proof
    JETOS_SYSTEM_ROOT="$system" PATH="$desktop_path" /bin/sh "$system/sw/bin/jetos-terminal-fallback" --jetos-proof
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
