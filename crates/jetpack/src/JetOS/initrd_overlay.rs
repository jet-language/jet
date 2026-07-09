fn append_installer_initrd_overlay(
    initrd: &Path,
    system: &SystemPlan,
    gen: &Generation,
) -> std::io::Result<()> {
    let init = r#"#!/bin/sh
set -eu
PATH=/jetos/tools/bin:/bin:/sbin:/usr/bin:/usr/sbin
export PATH
mkdir -p /proc
mount -t proc proc /proc 2>/dev/null || true
mkdir -p /dev
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /sys
mount -t sysfs sysfs /sys 2>/dev/null || true
use_system_nix() {
    system_nix="$1"
    [ -d "$system_nix" ] || return 0
    if [ -L /nix ]; then
        rm -f /nix 2>/dev/null || true
    elif [ -d /nix ]; then
        rmdir /nix 2>/dev/null || true
    fi
    [ -e /nix ] || ln -s "$system_nix" /nix 2>/dev/null || true
}
modprobe virtio_pci 2>/dev/null || true
modprobe virtio_blk 2>/dev/null || true
modprobe ata_piix 2>/dev/null || true
modprobe sd_mod 2>/dev/null || true
modprobe sr_mod 2>/dev/null || true
modprobe cdrom 2>/dev/null || true
insmod /jetos/modules/serio.ko.xz 2>/dev/null || true
insmod /jetos/modules/i8042.ko.xz 2>/dev/null || true
insmod /jetos/modules/libps2.ko.xz 2>/dev/null || true
insmod /jetos/modules/atkbd.ko.xz 2>/dev/null || true
insmod /jetos/modules/hid-generic.ko.xz 2>/dev/null || true
insmod /jetos/modules/usbhid.ko.xz 2>/dev/null || true
insmod /jetos/modules/uhci-hcd.ko.xz 2>/dev/null || true
insmod /jetos/modules/ehci-hcd.ko.xz 2>/dev/null || true
insmod /jetos/modules/xhci-hcd.ko.xz 2>/dev/null || true
modprobe atkbd 2>/dev/null || true
modprobe usbhid 2>/dev/null || true
cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
case "$cmdline" in
  *jetos.mode=install*)
    mkdir -p /media /mnt/jetos
    mount -o ro /dev/sr0 /media 2>/dev/null || mount -o ro /dev/cdrom /media 2>/dev/null || true
    exec /jetos/tools/bin/sh /jetos/install.sh
    ;;
  *jetos.mode=verify*|*jetos.mode=desktop-verify*)
    mkdir -p /sysroot
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        mount "$candidate" /sysroot 2>/dev/null && break
    done
    system="/sysroot/var/lib/jetos/generations/@JETOS_GENERATION@"
    use_system_nix "$system/nix"
    JETOS_TARGET_ROOT=/sysroot /jetos/tools/bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
    ;;
  *jetos.mode=run*)
    set +e
    mkdir -p /sysroot
    tries=0
    while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
        tries=$((tries + 1))
        sleep 1
    done
    for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
        mount "$candidate" /sysroot 2>/dev/null && break
    done
    system="/sysroot/var/lib/jetos/generations/@JETOS_GENERATION@"
    if [ ! -e "$system" ]; then
        system="/sysroot/run/current-system"
    fi
    if [ ! -e "$system/sw/bin/jetos-terminal-fallback" ]; then
        echo "jetos run: missing installed terminal fallback"
        exec /jetos/tools/bin/sh
    fi
    mkdir -p /run
    if [ -L /run/current-system ]; then
        rm -f /run/current-system 2>/dev/null || true
    elif [ -d /run/current-system ]; then
        rmdir /run/current-system 2>/dev/null || true
    else
        rm -f /run/current-system 2>/dev/null || true
    fi
    ln -s "$system" /run/current-system
    export JETOS_SYSTEM_ROOT="$system"
    export PATH="$system/sw/bin:$PATH"
    if [ -r "$system/etc/profile" ]; then
        . "$system/etc/profile"
    fi
    use_system_nix "$system/nix"
    if { [ -e "$system/sbin/init" ] || [ -L "$system/sbin/init" ]; } && command -v chroot >/dev/null 2>&1; then
        generation_target="/var/lib/jetos/generations/@JETOS_GENERATION@"
        mkdir -p /sysroot/run /sysroot/proc /sysroot/dev /sysroot/sys
        if [ -L /sysroot/run/current-system ]; then
            rm -f /sysroot/run/current-system 2>/dev/null || true
        elif [ -d /sysroot/run/current-system ]; then
            rmdir /sysroot/run/current-system 2>/dev/null || true
        else
            rm -f /sysroot/run/current-system 2>/dev/null || true
        fi
        ln -s "$generation_target" /sysroot/run/current-system
        if [ -d "$system/nix" ]; then
            if [ -L /sysroot/nix ]; then
                rm -f /sysroot/nix 2>/dev/null || true
            elif [ -d /sysroot/nix ]; then
                rmdir /sysroot/nix 2>/dev/null || true
            fi
            [ -e /sysroot/nix ] || ln -s "$generation_target/nix" /sysroot/nix 2>/dev/null || true
        fi
        for top in etc sbin sw share studio init systemd lib usr network hardware users flatpak performance module-system storage workloads theme fleet options image-variants lifecycle service-manager apps acceptance desktop store compat terminal home; do
            if [ -d "/sysroot/$top" ]; then
                rmdir "/sysroot/$top" 2>/dev/null || true
            fi
            if [ -e "$system/$top" ] && [ ! -e "/sysroot/$top" ]; then
                ln -s "$generation_target/$top" "/sysroot/$top" 2>/dev/null || true
            fi
        done
        mount --move /proc /sysroot/proc 2>/dev/null || mount -t proc proc /sysroot/proc 2>/dev/null || true
        mount --move /dev /sysroot/dev 2>/dev/null || mount -t devtmpfs devtmpfs /sysroot/dev 2>/dev/null || true
        mount --move /sys /sysroot/sys 2>/dev/null || mount -t sysfs sysfs /sysroot/sys 2>/dev/null || true
        echo "jetos run: handing off to installed systemd"
        export SYSTEMD_UNIT_PATH=/etc/systemd/system:/run/current-system/systemd/lib/systemd/system:/run/current-system/usr/lib/systemd/system:/run/current-system/lib/systemd/system
        exec chroot /sysroot /run/current-system/sbin/init systemd.unit=graphical.target
        echo "jetos run: systemd handoff failed; falling back to emergency console"
        mkdir -p /proc /dev /sys
        mount -t proc proc /proc 2>/dev/null || true
        mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
        mount -t sysfs sysfs /sys 2>/dev/null || true
    fi
    run_external() {
        "$@" &
        child=$!
        wait "$child"
        printf '\017'
    }
    tty=/dev/tty1
    case "$cmdline" in
      *console=ttyS0*) tty=/dev/ttyS0 ;;
    esac
    if [ ! -e "$tty" ]; then
        tty=/dev/console
    fi
    {
        printf '\017\033[2J\033[H'
        echo "JetOS {host}"
        echo "installed generation: $JETOS_SYSTEM_ROOT"
        echo "try: ls /run/current-system ; cat /run/current-system/studio/app.json"
        while true; do
            printf '\017JetOS {host} / # '
            IFS= read -r line || break
            case "$line" in
              exit|logout) break ;;
              reset|clear) printf '\017\033[2J\033[H' ;;
              jet) echo "jetos: bare jet starts the REPL; run 'jet repl' explicitly, or 'jet --help'" ;;
              cd' '*) cd "${line#cd }" 2>/dev/null || echo "cd: ${line#cd }: no such directory" ;;
              pwd) pwd ;;
              ls|ls' '*)
                set -- $line
                shift
                if [ "$#" -eq 0 ]; then
                    set -- .
                fi
                for target in "$@"; do
                    if [ -d "$target" ]; then
                        for item in "$target"/*; do
                            [ -e "$item" ] && printf '%s\n' "${item##*/}"
                        done
                    elif [ -e "$target" ]; then
                        printf '%s\n' "$target"
                    else
                        echo "ls: $target: no such file or directory"
                    fi
                done
                ;;
              cat' '*)
                set -- $line
                shift
                for file in "$@"; do
                    if [ ! -r "$file" ]; then
                        echo "cat: $file: no such file"
                        continue
                    fi
                    while IFS= read -r text || [ -n "$text" ]; do
                        printf '%s\n' "$text"
                    done < "$file"
                done
                ;;
              echo|echo' '*) printf '%s\n' "${line#echo }" ;;
              '') ;;
              *)
                set -- $line
                cmd=${1:-}
                if [ -z "$cmd" ]; then
                    continue
                fi
                shift
                if command -v "$cmd" >/dev/null 2>&1; then
                    run_external "$cmd" "$@"
                elif [ -x "$system/sw/bin/$cmd" ]; then
                    run_external "$system/sw/bin/$cmd" "$@"
                elif [ -x "/jetos/tools/bin/$cmd" ]; then
                    run_external "/jetos/tools/bin/$cmd" "$@"
                elif [ -x "/bin/$cmd" ]; then
                    run_external "/bin/$cmd" "$@"
                elif [ -x "$cmd" ]; then
                    run_external "$cmd" "$@"
                else
                    echo "$cmd: command not found"
                fi
                ;;
            esac
        done
        echo "JetOS console closed"
    } < "$tty" > "$tty" 2>&1
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exec /jetos/tools/bin/sh
    ;;
esac
mkdir -p /sysroot
tries=0
while [ ! -e /dev/vda ] && [ ! -e /dev/vda2 ] && [ ! -e /dev/sda ] && [ ! -e /dev/sda2 ] && [ "$tries" -lt 30 ]; do
    tries=$((tries + 1))
    sleep 1
done
for candidate in LABEL=jetos-root /dev/vda2 /dev/sda2 /dev/vda /dev/sda; do
    mount "$candidate" /sysroot 2>/dev/null && break
done
if [ -e /sysroot/var/lib/jetos/generations/log ] || [ -L /sysroot/run/current-system ]; then
    system="/sysroot/var/lib/jetos/generations/@JETOS_GENERATION@"
    use_system_nix "$system/nix"
    JETOS_TARGET_ROOT=/sysroot /jetos/tools/bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
fi
exec /jetos/tools/bin/sh /jetos/install.sh
"#
    .replace("@JETOS_GENERATION@", &gen.name);
    let install = render_installer_script(system, gen);
    let verify = render_guest_verify_script(system, gen);
    let isofs = fs::read(gen.path.join("boot/modules/isofs.ko.xz")).ok();
    let bochs = fs::read(gen.path.join("boot/modules/bochs.ko.xz")).ok();
    let fat_modules = [
        (
            "fat.ko.xz",
            fs::read(gen.path.join("boot/modules/fat.ko.xz")).ok(),
        ),
        (
            "vfat.ko.xz",
            fs::read(gen.path.join("boot/modules/vfat.ko.xz")).ok(),
        ),
        (
            "nls_ascii.ko.xz",
            fs::read(gen.path.join("boot/modules/nls_ascii.ko.xz")).ok(),
        ),
        (
            "nls_cp437.ko.xz",
            fs::read(gen.path.join("boot/modules/nls_cp437.ko.xz")).ok(),
        ),
    ];
    let input_modules = [
        "serio.ko.xz",
        "i8042.ko.xz",
        "libps2.ko.xz",
        "atkbd.ko.xz",
        "hid-generic.ko.xz",
        "usbhid.ko.xz",
        "uhci-hcd.ko.xz",
        "ehci-hcd.ko.xz",
        "xhci-hcd.ko.xz",
    ];
    let mut entries = vec![
        OwnedCpioEntry::dir("jetos"),
        OwnedCpioEntry::dir("jetos/modules"),
        OwnedCpioEntry::dir("jetos/tools"),
        OwnedCpioEntry::dir("jetos/tools/bin"),
        OwnedCpioEntry::file("jetos/init", 0o100755, init.as_bytes().to_vec()),
        OwnedCpioEntry::file("jetos/install.sh", 0o100755, install.as_bytes().to_vec()),
        OwnedCpioEntry::file(
            "jetos/guest-verify.sh",
            0o100755,
            verify.as_bytes().to_vec(),
        ),
    ];
    if let Some(isofs) = isofs {
        entries.push(OwnedCpioEntry::file(
            "jetos/modules/isofs.ko.xz",
            0o100644,
            isofs,
        ));
    }
    if let Some(bochs) = bochs {
        entries.push(OwnedCpioEntry::file(
            "jetos/modules/bochs.ko.xz",
            0o100644,
            bochs,
        ));
    }
    for (name, bytes) in fat_modules {
        if let Some(bytes) = bytes {
            entries.push(OwnedCpioEntry::file(
                &format!("jetos/modules/{name}"),
                0o100644,
                bytes,
            ));
        }
    }
    for name in input_modules {
        if let Ok(bytes) = fs::read(gen.path.join("boot/modules").join(name)) {
            entries.push(OwnedCpioEntry::file(
                &format!("jetos/modules/{name}"),
                0o100644,
                bytes,
            ));
        }
    }
    entries.extend(installer_tool_overlay_entries()?);
    let overlay = cpio_newc_owned(&entries);
    let initrd_bytes = fs::read(initrd)?;
    if let Some(offset) = first_zstd_frame_offset(&initrd_bytes) {
        let zstd = find_path_tool("zstd").ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing zstd for installer initrd overlay",
            )
        })?;
        let compressed_path = unique_initrd_temp_path(initrd, "main.zst");
        fs::write(&compressed_path, &initrd_bytes[offset..])?;
        let mut plain = zstd_decode_file(&zstd, &compressed_path)?;
        let _ = fs::remove_file(&compressed_path);
        if let Some(sanitized) = sanitize_runtime_branding_bytes(&plain) {
            plain = sanitized;
        }
        let merged = append_overlay_to_cpio_plain(plain, &overlay);
        let plain_path = unique_initrd_temp_path(initrd, "main.cpio");
        fs::write(&plain_path, &merged)?;
        let compressed = zstd_encode_file(&zstd, &plain_path)?;
        let _ = fs::remove_file(&plain_path);
        let mut out = Vec::with_capacity(offset + compressed.len());
        out.extend_from_slice(&initrd_bytes[..offset]);
        out.extend_from_slice(&compressed);
        fs::write(initrd, out)?;
        return Ok(());
    }

    let merged = append_overlay_to_cpio_plain(initrd_bytes, &overlay);
    fs::write(initrd, merged)
}

fn installer_tool_overlay_entries() -> std::io::Result<Vec<OwnedCpioEntry>> {
    let mut dirs = BTreeSet::new();
    let mut seen_files = BTreeSet::new();
    let mut files = Vec::new();
    for tool in [
        "cat",
        "sh",
        "cp",
        "ln",
        "mkdir",
        "mount",
        "rm",
        "sleep",
        "sync",
        "sfdisk",
        "blockdev",
        "mkfs.vfat",
        "mkfs.ext4",
        "poweroff",
        "halt",
        "setsid",
        "chroot",
        "switch_root",
    ] {
        let tool_path = find_path_tool(tool).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing installer tool `{tool}`"),
            )
        })?;
        let actual = fs::canonicalize(&tool_path).unwrap_or_else(|_| tool_path.clone());
        if tool == "sh" {
            add_host_file_to_cpio_as(
                &mut dirs,
                &mut seen_files,
                &mut files,
                &actual,
                "jetos/tools/bin/sh",
                host_cpio_file_mode(&actual, 0o100755),
            )?;
            for dep in ldd_dependency_paths(&actual)? {
                add_host_file_to_cpio(
                    &mut dirs,
                    &mut seen_files,
                    &mut files,
                    &dep,
                    host_cpio_file_mode(&dep, 0o100644),
                )?;
            }
            continue;
        }
        let wrapper = format!("#!/bin/sh\nexec {} \"$@\"\n", tool_path.display());
        add_cpio_file(
            &mut dirs,
            &mut seen_files,
            &mut files,
            &format!("jetos/tools/bin/{tool}"),
            0o100755,
            wrapper.into_bytes(),
        );
        add_host_file_to_cpio(&mut dirs, &mut seen_files, &mut files, &tool_path, 0o100755)?;
        for dep in ldd_dependency_paths(&actual)? {
            add_host_file_to_cpio(
                &mut dirs,
                &mut seen_files,
                &mut files,
                &dep,
                host_cpio_file_mode(&dep, 0o100644),
            )?;
        }
    }
    let mut entries = dirs
        .into_iter()
        .map(|dir| OwnedCpioEntry::dir(&dir))
        .collect::<Vec<_>>();
    entries.extend(files);
    Ok(entries)
}

fn add_host_file_to_cpio(
    dirs: &mut BTreeSet<String>,
    seen_files: &mut BTreeSet<String>,
    files: &mut Vec<OwnedCpioEntry>,
    path: &Path,
    mode: u32,
) -> std::io::Result<()> {
    let Some(name) = path
        .to_str()
        .and_then(|s| s.strip_prefix('/'))
        .map(str::to_string)
    else {
        return Ok(());
    };
    let data = fs::read(path)?;
    add_cpio_file(dirs, seen_files, files, &name, mode, data);
    Ok(())
}

fn add_host_file_to_cpio_as(
    dirs: &mut BTreeSet<String>,
    seen_files: &mut BTreeSet<String>,
    files: &mut Vec<OwnedCpioEntry>,
    path: &Path,
    name: &str,
    mode: u32,
) -> std::io::Result<()> {
    let data = fs::read(path)?;
    add_cpio_file(dirs, seen_files, files, name, mode, data);
    Ok(())
}

#[cfg(unix)]
fn host_cpio_file_mode(path: &Path, fallback: u32) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| 0o100000 | (meta.permissions().mode() & 0o777))
        .unwrap_or(fallback)
}

#[cfg(not(unix))]
fn host_cpio_file_mode(_path: &Path, fallback: u32) -> u32 {
    fallback
}

fn add_cpio_file(
    dirs: &mut BTreeSet<String>,
    seen_files: &mut BTreeSet<String>,
    files: &mut Vec<OwnedCpioEntry>,
    name: &str,
    mode: u32,
    data: Vec<u8>,
) {
    add_cpio_parent_dirs(dirs, name);
    if seen_files.insert(name.to_string()) {
        let data = sanitize_runtime_branding_bytes(&data).unwrap_or(data);
        files.push(OwnedCpioEntry::file(name, mode, data));
    }
}

fn add_cpio_parent_dirs(dirs: &mut BTreeSet<String>, name: &str) {
    let mut prefix = String::new();
    for part in name
        .split('/')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip(1)
        .rev()
    {
        if part.is_empty() {
            continue;
        }
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(part);
        dirs.insert(prefix.clone());
    }
}

fn ldd_dependency_paths(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if fs::read(path)
        .map(|bytes| bytes.starts_with(b"#!"))
        .unwrap_or(false)
    {
        return Ok(Vec::new());
    }
    let ldd = find_path_tool("ldd").unwrap_or_else(|| PathBuf::from("ldd"));
    let output = Command::new(ldd).arg(path).output()?;
    if !output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("ldd failed for `{}`", path.display()),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut deps = Vec::new();
    let mut seen = BTreeSet::new();
    for token in text.split_whitespace() {
        let candidate = token.trim_end_matches(':');
        if !candidate.starts_with('/') {
            continue;
        }
        let path = PathBuf::from(candidate);
        if path.exists() && seen.insert(candidate.to_string()) {
            deps.push(path);
        }
    }
    Ok(deps)
}

fn first_zstd_frame_offset(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .enumerate()
        .find_map(|(offset, window)| {
            if window != [0x28, 0xb5, 0x2f, 0xfd] || offset % 4 != 0 {
                return None;
            }
            if offset == 0 || cpio_trailer_header_offset(&bytes[..offset]).is_some() {
                Some(offset)
            } else {
                None
            }
        })
}

fn zstd_decode_file(zstd: &Path, input: &Path) -> std::io::Result<Vec<u8>> {
    let output = Command::new(zstd)
        .args(["-d", "-q", "-c"])
        .arg(input)
        .output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zstd could not decode installer initrd payload",
        ))
    }
}

fn zstd_encode_file(zstd: &Path, input: &Path) -> std::io::Result<Vec<u8>> {
    let output = Command::new(zstd).args(["-q", "-c"]).arg(input).output()?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "zstd could not encode installer initrd payload",
        ))
    }
}

fn unique_initrd_temp_path(initrd: &Path, label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let file_name = initrd
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("initrd");
    initrd.with_file_name(format!("{file_name}.{stamp}.{label}"))
}

fn append_overlay_to_cpio_plain(mut plain: Vec<u8>, overlay: &[u8]) -> Vec<u8> {
    if let Some(offset) = cpio_trailer_header_offset(&plain) {
        plain.truncate(offset);
    } else {
        while plain.len() % 4 != 0 {
            plain.push(0);
        }
    }
    plain.extend_from_slice(overlay);
    plain
}

fn cpio_trailer_header_offset(bytes: &[u8]) -> Option<usize> {
    let mut pos = 0;
    let mut found = None;
    while let Some(rel) = bytes[pos..]
        .windows("TRAILER!!!".len())
        .position(|window| window == b"TRAILER!!!")
    {
        let name_offset = pos + rel;
        if name_offset >= 110 {
            let header_offset = name_offset - 110;
            if bytes
                .get(header_offset..header_offset + 6)
                .map(|magic| magic == b"070701" || magic == b"070702")
                .unwrap_or(false)
            {
                found = Some(header_offset);
            }
        }
        pos = name_offset + 1;
    }
    found
}

struct OwnedCpioEntry {
    name: String,
    mode: u32,
    data: Vec<u8>,
}

impl OwnedCpioEntry {
    fn dir(name: &str) -> OwnedCpioEntry {
        OwnedCpioEntry {
            name: name.to_string(),
            mode: 0o040755,
            data: Vec::new(),
        }
    }

    fn file(name: &str, mode: u32, data: Vec<u8>) -> OwnedCpioEntry {
        OwnedCpioEntry {
            name: name.to_string(),
            mode,
            data,
        }
    }

}

fn cpio_newc_owned(entries: &[OwnedCpioEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for (ino, entry) in entries.iter().enumerate() {
        cpio_newc_entry(
            &mut out,
            (ino + 1) as u32,
            &entry.name,
            entry.mode,
            &entry.data,
        );
    }
    cpio_newc_entry(&mut out, entries.len() as u32 + 1, "TRAILER!!!", 0, &[]);
    out
}

fn cpio_newc_entry(out: &mut Vec<u8>, ino: u32, name: &str, mode: u32, data: &[u8]) {
    let namesize = name.len() + 1;
    out.extend_from_slice(
        format!(
            "070701{ino:08x}{mode:08x}{uid:08x}{gid:08x}{nlink:08x}{mtime:08x}{filesize:08x}{devmajor:08x}{devminor:08x}{rdevmajor:08x}{rdevminor:08x}{namesize:08x}{check:08x}",
            uid = 0,
            gid = 0,
            nlink = 1,
            mtime = 0,
            filesize = data.len(),
            devmajor = 0,
            devminor = 0,
            rdevmajor = 0,
            rdevminor = 0,
            check = 0
        )
        .as_bytes(),
    );
    out.extend_from_slice(name.as_bytes());
    out.push(0);
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
