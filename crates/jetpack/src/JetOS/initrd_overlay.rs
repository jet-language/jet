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
    exec /bin/sh /jetos/install.sh
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
    if [ -d "$system/nix" ] && [ ! -e /nix ]; then
        ln -s "$system/nix" /nix
    fi
    JETOS_TARGET_ROOT=/sysroot /bin/sh /jetos/guest-verify.sh
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
        exec /bin/sh
    fi
    mkdir -p /run
    rm -f /run/current-system
    ln -s "$system" /run/current-system
    export JETOS_SYSTEM_ROOT="$system"
    export PATH="$system/sw/bin:$PATH"
    if [ -r "$system/etc/profile" ]; then
        . "$system/etc/profile"
    fi
    if [ -d "$system/nix" ] && [ ! -e /nix ]; then
        ln -s "$system/nix" /nix
    fi
    if [ -x "$system/sbin/init" ] && command -v switch_root >/dev/null 2>&1; then
        generation_target="/var/lib/jetos/generations/@JETOS_GENERATION@"
        mkdir -p /sysroot/run /sysroot/proc /sysroot/dev /sysroot/sys
        rm -f /sysroot/run/current-system
        ln -s "$generation_target" /sysroot/run/current-system
        if [ -d "$system/nix" ] && [ ! -e /sysroot/nix ]; then
            ln -s "$generation_target/nix" /sysroot/nix
        fi
        mount --move /proc /sysroot/proc 2>/dev/null || mount -t proc proc /sysroot/proc 2>/dev/null || true
        mount --move /dev /sysroot/dev 2>/dev/null || mount -t devtmpfs devtmpfs /sysroot/dev 2>/dev/null || true
        mount --move /sys /sysroot/sys 2>/dev/null || mount -t sysfs sysfs /sysroot/sys 2>/dev/null || true
        echo "jetos run: handing off to installed systemd"
        exec switch_root /sysroot /sbin/init systemd.unit=graphical.target
        echo "jetos run: switch_root failed; falling back to emergency console"
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
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exec /bin/sh
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
    if [ -d "$system/nix" ] && [ ! -e /nix ]; then
        ln -s "$system/nix" /nix
    fi
    JETOS_TARGET_ROOT=/sysroot /bin/sh /jetos/guest-verify.sh
    poweroff -f 2>/dev/null || halt -f 2>/dev/null || exit 0
fi
exec /bin/sh /jetos/install.sh
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
    let overlay = if contains_zstd_frame(&initrd_bytes) && find_path_tool("zstd").is_some() {
        let overlay_path = initrd.with_extension("jetos-overlay.cpio");
        fs::write(&overlay_path, &overlay)?;
        let compressed = Command::new("zstd")
            .args(["-q", "-c"])
            .arg(&overlay_path)
            .output()?;
        if compressed.status.success() {
            compressed.stdout
        } else {
            overlay
        }
    } else {
        overlay
    };
    if is_newc_bytes(&initrd_bytes) && !contains_zstd_frame(&initrd_bytes) {
        let mut existing = initrd_bytes;
        if let Some(header) = cpio_trailer_header_offset(&existing) {
            existing.truncate(header);
            existing.extend_from_slice(&overlay);
            return fs::write(initrd, existing);
        }
    }
    use std::io::Write;
    let mut file = fs::OpenOptions::new().append(true).open(initrd)?;
    file.write_all(&overlay)
}

fn installer_tool_overlay_entries() -> std::io::Result<Vec<OwnedCpioEntry>> {
    let mut dirs = BTreeSet::new();
    let mut seen_files = BTreeSet::new();
    let mut files = Vec::new();
    for tool in [
        "cat",
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
        "switch_root",
    ] {
        let tool_path = find_path_tool(tool).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing installer tool `{tool}`"),
            )
        })?;
        let actual = fs::canonicalize(&tool_path).unwrap_or_else(|_| tool_path.clone());
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

fn contains_zstd_frame(bytes: &[u8]) -> bool {
    bytes
        .windows(4)
        .any(|window| window == [0x28, 0xb5, 0x2f, 0xfd])
}

fn is_newc_bytes(bytes: &[u8]) -> bool {
    bytes.starts_with(b"070701") || bytes.starts_with(b"070702")
}

fn cpio_trailer_header_offset(bytes: &[u8]) -> Option<usize> {
    let marker = b"TRAILER!!!\0";
    bytes
        .windows(marker.len())
        .rposition(|window| window == marker)
        .and_then(|name| {
            name.checked_sub(110).filter(|header| {
                bytes[*header..].starts_with(b"070701") || bytes[*header..].starts_with(b"070702")
            })
        })
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
