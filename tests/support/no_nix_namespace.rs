//! Rootless Linux harness for tests that must start without a host Nix store.

use std::path::PathBuf;
use std::process::Command;

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
        let rmdir = find_helper("rmdir");
        let ldd = find_helper("ldd");
        let readlink = find_helper("readlink");
        let sed = find_helper("sed");
        let sort = find_helper("sort");
        let test_binary = std::env::current_exe().expect("current test executable");
        let script = r#"set -eu
mount="$1"
umount="$2"
mkdir="$3"
rmdir="$4"
ldd="$5"
readlink="$6"
sed="$7"
sort="$8"
test_binary="$9"
test_name="${10}"
scratch_root="${TMPDIR:-/tmp}"
host_nix="$scratch_root/jet-no-nix-host-$$"
runtime_stage="$scratch_root/jet-no-nix-runtime-$$"
mount_real="$($readlink -f "$mount")"
umount_real="$($readlink -f "$umount")"
mkdir_real="$($readlink -f "$mkdir")"
rmdir_real="$($readlink -f "$rmdir")"
ldd_real="$($readlink -f "$ldd")"
readlink_real="$($readlink -f "$readlink")"
sed_real="$($readlink -f "$sed")"
sort_real="$($readlink -f "$sort")"
set -f
emit_store_root() {
    case "$1" in
        /nix/store/*)
            token=${1#/nix/store/}
            printf '/nix/store/%s\n' "${token%%/*}"
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
    store_roots /bin
    store_roots /bin/sh
    store_roots /run/current-system/sw
    for helper in "$mount" "$umount" "$mkdir" "$rmdir" "$ldd" "$readlink" "$sed" "$sort"; do
        store_roots "$helper"
    done
)"
runtime_roots=$(printf '%s\n' "$runtime_roots" | $sort -u)
mount="$mount_real"
umount="$umount_real"
mkdir="$mkdir_real"
rmdir="$rmdir_real"
ldd="$ldd_real"
readlink="$readlink_real"
sed="$sed_real"
sort="$sort_real"
"$mkdir" --coreutils-prog=mkdir -p /nix
"$mkdir" --coreutils-prog=mkdir -p "$host_nix"
"$mkdir" --coreutils-prog=mkdir -p "$runtime_stage/store"
"$mount" --rbind /nix "$host_nix"
"$mount" --make-rslave "$host_nix"
for runtime in $runtime_roots; do
    name=${runtime##*/}
    "$mkdir" --coreutils-prog=mkdir -p "$runtime_stage/store/$name"
    "$mount" --rbind "$host_nix/store/$name" "$runtime_stage/store/$name"
"$mount" --make-rslave "$runtime_stage/store/$name"
done
mount="$runtime_stage${mount_real#/nix}"
umount="$runtime_stage${umount_real#/nix}"
mkdir="$runtime_stage${mkdir_real#/nix}"
rmdir="$runtime_stage${rmdir_real#/nix}"
ldd="$runtime_stage${ldd_real#/nix}"
readlink="$runtime_stage${readlink_real#/nix}"
sed="$runtime_stage${sed_real#/nix}"
sort="$runtime_stage${sort_real#/nix}"
"$mount" --rbind "$runtime_stage" /nix
"$umount" -l "$host_nix"
exec "$test_binary" "$test_name" --exact --nocapture
"#;

        let mut command = Command::new(unshare);
        command
            .args([
                "--user",
                "--map-root-user",
                "--mount",
                "--propagation",
                "private",
            ])
            .args((network_mode == NetworkMode::Disabled).then_some("--net"))
            .arg(shell)
            .args(["-c", script, "jet-no-nix-namespace"])
            .arg(mount)
            .arg(umount)
            .arg(mkdir)
            .arg(rmdir)
            .arg(ldd)
            .arg(readlink)
            .arg(sed)
            .arg(sort)
            .arg(test_binary)
            .arg(exact_test_name)
            .env(CHILD_MARKER, "1");
        let status = command
            .status()
            .unwrap_or_else(|error| panic!("starting no-host-store namespace: {error}"));
        assert!(
            status.success(),
            "no-host-store namespace proof failed for `{exact_test_name}`: {status}"
        );
    }
}

fn assert_no_host_store() {
    let host_mount_prefix = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("jet-no-nix-host-")
        .to_string_lossy()
        .into_owned();
    match std::fs::symlink_metadata("/nix/store") {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(metadata) if metadata.is_dir() => {
            assert!(
                !std::fs::read_to_string("/proc/self/mountinfo")
                    .expect("read namespace mountinfo")
                    .contains(&host_mount_prefix),
                "the host Nix mount remained visible in the test namespace"
            );
        }
        Ok(_) => panic!("test child started with a visible non-directory /nix/store"),
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
