//! Rootless Linux harness for tests that must start without a host Nix store.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// This module is included by several test roots and each compiles it
// separately, so a root that only runs offline phases never constructs
// `Enabled`. jetpack_engine.rs does.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkMode {
    Enabled,
    Disabled,
}

pub const CHILD_MARKER: &str = "JET_NO_NIX_NAMESPACE_CHILD";
pub fn run_in_no_nix_namespace<F>(exact_test_name: &str, network_mode: NetworkMode, child_body: F)
where
    F: FnOnce(),
{
    if std::env::var_os(CHILD_MARKER).is_some() {
        assert_no_host_store();
        child_body();
        return;
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (exact_test_name, network_mode);
        panic!("no-host-store namespace proof requires Linux");
    }

    #[cfg(target_os = "linux")]
    {
        let unshare = find_helper("unshare");
        let mount = find_helper("mount");
        let umount = find_helper("umount");
        let shell = find_helper("sh");
        let mkdir = find_helper("mkdir");
        let ldd = find_helper("ldd");
        let readlink = find_helper("readlink");
        let sort = find_helper("sort");
        let ln = find_helper("ln");
        let true_binary = find_helper("true");
        let test_binary = std::env::current_exe().expect("current test executable");
        let bootstrap_root =
            std::env::temp_dir().join(format!("jet-no-nix-bootstrap-{}", std::process::id()));
        fs::create_dir_all(&bootstrap_root).expect("create no-host-store bootstrap");
        let bootstrap_unshare = patch_interpreter(&unshare, &bootstrap_root.join("unshare"));
        let bootstrap_mount = patch_interpreter(&mount, &bootstrap_root.join("mount"));
        let bootstrap_shell = patch_interpreter(&shell, &bootstrap_root.join("sh"));
        // Derive the jetpack binary from the running test executable rather
        // than `crate::common`: this harness is included by test roots that do
        // not carry that module, and depending on it breaks their build.
        let subject_binary = test_binary
            .parent()
            .and_then(|deps| deps.parent())
            .map(|profile| profile.join("jetpack"))
            .filter(|candidate| candidate.exists())
            .expect("jetpack binary beside the test executable");
        let subject_binary = &subject_binary;
        let bootstrap_subject = patch_interpreter(subject_binary, &bootstrap_root.join("jetpack"));
        let script = r#"set -eu
unshare="$1"
mount="$2"
umount="$3"
shell="$4"
mkdir="$5"
ldd="$6"
readlink="$7"
sort="$8"
ln="$9"
test_binary="${10}"
test_name="${11}"
bootstrap_unshare="${12}"
bootstrap_mount="${13}"
bootstrap_shell="${14}"
# The binary under test is spawned inside the namespace, so its own runtime
# closure has to be staged as well. Without it the process cannot load its
# interpreter and dies with ENOENT before any package resolution happens.
subject_binary="${15}"
subject_image="${16:-}"
true_binary="${17:-}"
scratch_root="${TMPDIR:-/tmp}"
host_nix="$scratch_root/jet-no-nix-host-$$"
runtime_stage="$scratch_root/jet-no-nix-runtime-$$"
mount_real="$($readlink -f "$mount")"
umount_real="$($readlink -f "$umount")"
mkdir_real="$($readlink -f "$mkdir")"
set -f
emit_store_root() {
    case "$1" in
        /nix/store/*)
            token=${1#/nix/store/}
            printf '/nix/store/%s\n' "${token%%/*}"
            resolved="$($readlink -f "$1" 2>/dev/null || true)"
            case "$resolved" in
                /nix/store/*)
                    token=${resolved#/nix/store/}
                    printf '/nix/store/%s\n' "${token%%/*}"
                    ;;
            esac
            ;;
    esac
}
store_roots() {
    if [ -f "$1" ]; then
        for token in $($ldd "$1"); do
            emit_store_root "$token"
        done
    fi
    emit_store_root "$($readlink -f "$1")"
}
runtime_roots="$(
    store_roots "$test_binary"
    [ -n "$subject_binary" ] && store_roots "$subject_binary"
    store_roots /bin
    store_roots /bin/sh
    store_roots /run/current-system/sw
    [ -n "$true_binary" ] && store_roots "$true_binary"
    for helper in "$unshare" "$mount" "$umount" "$shell" "$mkdir" "$ldd" "$readlink" "$sort" "$ln"; do
        store_roots "$helper"
    done
)"
runtime_roots=$(printf '%s\n' "$runtime_roots" | $sort -u)
loader=""
for token in $($ldd "$test_binary"); do
    case "$token" in
        */ld-linux*.so*) loader="$token" ;;
    esac
done
[ -n "$loader" ]
exec 9<"$loader"
loader=/proc/self/fd/9

stage_path() {
    case "$1" in
        /nix/store/*) printf '%s/store/%s\n' "$runtime_stage" "${1#/nix/store/}" ;;
        *) printf '%s\n' "$1" ;;
    esac
}

library_path=""
run() {
    if [ -n "$library_path" ]; then
        "$loader" --library-path "$library_path" "$@"
    else
        "$loader" "$@"
    fi
}

"$mkdir_real" --coreutils-prog=mkdir -p /nix
"$mkdir_real" --coreutils-prog=mkdir -p "$host_nix"
"$mkdir_real" --coreutils-prog=mkdir -p "$runtime_stage/store" "$runtime_stage/usr/bin"
"$mount_real" --rbind /nix "$host_nix"
"$mount_real" --make-rslave "$host_nix"
for runtime in $runtime_roots; do
    name=${runtime##*/}
    "$mkdir_real" --coreutils-prog=mkdir -p "$runtime_stage/store/$name"
    "$mount_real" --rbind "$host_nix/store/$name" "$runtime_stage/store/$name"
    "$mount_real" --make-rslave "$runtime_stage/store/$name"
done
for runtime in $runtime_roots; do
    name=${runtime##*/}
    for libdir in lib lib64; do
        path="$runtime_stage/store/$name/$libdir"
        if [ -d "$path" ]; then
            [ -z "$library_path" ] || library_path="$library_path:"
            library_path="$library_path$path"
        fi
    done
done
if [ -n "$subject_image" ]; then
    "$mount_real" --bind "$subject_image" "$subject_binary"
fi
if [ -n "$true_binary" ]; then
    true_real="$($readlink -f "$true_binary")"
    "$ln" -s "$(stage_path "$true_real")" "$runtime_stage/usr/bin/true"
fi
if [ -n "$bootstrap_unshare" ]; then
    "$ln" -s "$bootstrap_unshare" "$runtime_stage/usr/bin/unshare"
fi
if [ -n "$bootstrap_mount" ]; then
    "$ln" -s "$bootstrap_mount" "$runtime_stage/usr/bin/mount"
fi
if [ -n "$bootstrap_shell" ]; then
    "$ln" -s "$bootstrap_shell" "$runtime_stage/usr/bin/sh"
fi
if [ -n "$bootstrap_unshare$bootstrap_mount$bootstrap_shell" ]; then
    # Nix hosts often make /bin/sh a symlink into /nix/store. The projected
    # package can use that absolute interpreter after /nix/store is hidden.
    # Keep both canonical system-bin paths on the staged helper set.
    "$mount_real" --bind "$runtime_stage/usr/bin" /usr/bin
    "$mount_real" --bind "$runtime_stage/usr/bin" /bin
fi

mount="$(stage_path "$mount_real")"
umount="$(stage_path "$umount_real")"
run "$mount" -t tmpfs -o mode=0755 jet-no-nix /nix
run "$umount" -l "$host_nix"
test ! -e /nix/store
export LD_LIBRARY_PATH="$library_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ -n "$library_path" ]; then
    exec "$loader" --library-path "$library_path" "$test_binary" "$test_name" --exact --nocapture
else
    exec "$loader" "$test_binary" "$test_name" --exact --nocapture
fi
"#;

        let mut command = Command::new(&unshare);
        command
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--propagation",
                "private",
            ])
            .args((network_mode == NetworkMode::Disabled).then_some("--net"))
            .arg(&shell)
            .args(["-c", script, "jet-no-nix-namespace"])
            .arg(&unshare)
            .arg(mount)
            .arg(umount)
            .arg(&shell)
            .arg(mkdir)
            .arg(ldd)
            .arg(readlink)
            .arg(sort)
            .arg(ln)
            .arg(test_binary)
            .arg(exact_test_name)
            .arg(
                bootstrap_unshare
                    .as_deref()
                    .unwrap_or_else(|| Path::new("")),
            )
            .arg(bootstrap_mount.as_deref().unwrap_or_else(|| Path::new("")))
            .arg(bootstrap_shell.as_deref().unwrap_or_else(|| Path::new("")))
            .arg(subject_binary.as_os_str())
            .arg(bootstrap_subject.as_deref().unwrap_or_else(|| Path::new("")))
            .arg(true_binary.as_os_str())
            .env(CHILD_MARKER, "1");
        let status = command
            .status()
            .unwrap_or_else(|error| panic!("starting no-host-store namespace: {error}"));
        let _ = fs::remove_dir_all(&bootstrap_root);
        assert!(
            status.success(),
            "no-host-store namespace proof failed for `{exact_test_name}`: {status}"
        );
    }
}

#[cfg(target_os = "linux")]
fn patch_interpreter(source: &Path, destination: &Path) -> Option<PathBuf> {
    let mut bytes = fs::read(source).ok()?;
    let replacement = b"/proc/self/fd/9";
    let marker = b"/nix/store/";
    let mut search_from = 0;
    loop {
        let offset = search_from
            + bytes[search_from..]
                .windows(marker.len())
                .position(|part| part == marker)?;
        let end = bytes[offset..]
            .iter()
            .position(|byte| *byte == 0)
            .map_or(bytes.len(), |length| offset + length);
        let value = &bytes[offset..end];
        if value.windows(8).any(|part| part == b"ld-linux") {
            if replacement.len() >= value.len() {
                return None;
            }
            bytes[offset..end].fill(0);
            bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
            break;
        }
        search_from = end.saturating_add(1);
    }
    fs::write(destination, bytes).ok()?;
    fs::set_permissions(destination, fs::metadata(source).ok()?.permissions()).ok()?;
    Some(destination.to_path_buf())
}

fn assert_no_host_store() {
    match std::fs::symlink_metadata("/nix/store") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) => panic!("test child started with a visible /nix/store"),
        Err(error) => panic!("could not inspect /nix/store before Jetpack: {error}"),
    }
}

#[cfg(target_os = "linux")]
fn find_helper(name: &str) -> PathBuf {
    [
        PathBuf::from(format!("/run/current-system/sw/bin/{name}")),
        PathBuf::from(format!("/usr/bin/{name}")),
        PathBuf::from(format!("/bin/{name}")),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap_or_else(|| panic!("required namespace helper `{name}` is not installed"))
}
