#!/bin/sh
# Kit-owned host bridge, ported from tests/support/no_nix_namespace.rs.
# Removal condition: replace this asset with Jet when Core provides mount and
# unshare raw leaves. This is isolation plumbing, never case-authored content.
# All subject arguments arrive as argv, not interpolated shell source.
set -eu
if [ "$1" = --outer ]; then
    bootstrap=$2
    shift 2
    if "$@"; then status=0; else status=$?; fi
    if [ ! -e "$bootstrap/entered" ]; then
        printf '%s\n' JET_TESTKIT_NO_NIX_SETUP_FAILED >&2
    fi
    exit "$status"
fi
trap 'printf "%s\n" JET_TESTKIT_NO_NIX_SETUP_FAILED >&2' 0
bootstrap=$1
unshare=$2
mount=$3
umount=$4
shell=$5
mkdir=$6
ldd=$7
readlink=$8
sort=$9
shift 9
ln=$1
true_binary=$2
bootstrap_unshare=$3
bootstrap_mount=$4
bootstrap_shell=$5
subject_count=$6
host_path=$7
shift 7
subject_path_set=${PATH+x}
subject_path=${PATH-}
export PATH="$host_path"
subject_real=$("$readlink" -f "$1")
host_nix=$bootstrap/host
runtime_stage=$bootstrap/runtime
mount_real=$("$readlink" -f "$mount")
umount_real=$("$readlink" -f "$umount")
set -f
emit_store_root() {
    case "$1" in
        /nix/store/*)
            token=${1#/nix/store/}
            printf '/nix/store/%s\n' "${token%%/*}"
            resolved=$("$readlink" -f "$1" 2>/dev/null || :)
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
        for token in $("$ldd" "$1"); do emit_store_root "$token"; done
    fi
    emit_store_root "$("$readlink" -f "$1")"
}
subject_roots() {
    remaining=$subject_count
    while [ "$remaining" -gt 0 ]; do
        store_roots "$1"
        shift 2
        remaining=$((remaining - 1))
    done
}
runtime_roots=$(
    subject_roots "$@"
    store_roots /bin
    store_roots /bin/sh
    store_roots /run/current-system/sw
    for helper in "$unshare" "$mount" "$umount" "$shell" "$mkdir" "$ldd" "$readlink" "$sort" "$ln" "$true_binary"; do store_roots "$helper"; done
)
runtime_roots=$(printf '%s\n' "$runtime_roots" | "$sort" -u)
find_loader() {
    remaining=$subject_count
    while [ "$remaining" -gt 0 ]; do
        for token in $("$ldd" "$1"); do
            case "$token" in */ld-linux*.so*) printf '%s\n' "$token"; return;; esac
        done
        shift 2
        remaining=$((remaining - 1))
    done
    return 1
}
loader=$(find_loader "$@")
exec 9<"$loader"
loader=/proc/self/fd/9
stage_path() {
    case "$1" in
        /nix/store/*) printf '%s/store/%s\n' "$runtime_stage" "${1#/nix/store/}";;
        *) printf '%s\n' "$1";;
    esac
}
library_path=
run() {
    if [ -n "$library_path" ]; then "$loader" --library-path "$library_path" "$@"
    else "$loader" "$@"; fi
}
"$mkdir" -p /nix "$host_nix" "$runtime_stage/store" "$runtime_stage/usr/bin"
"$mount_real" --rbind /nix "$host_nix"
"$mount_real" --make-rslave "$host_nix"
for runtime in $runtime_roots; do
    name=${runtime##*/}
    "$mkdir" -p "$runtime_stage/store/$name"
    "$mount_real" --rbind "$host_nix/store/$name" "$runtime_stage/store/$name"
    "$mount_real" --make-rslave "$runtime_stage/store/$name"
done
for runtime in $runtime_roots; do
    name=${runtime##*/}
    for libdir in lib lib64; do
        path=$runtime_stage/store/$name/$libdir
        if [ -d "$path" ]; then
            [ -z "$library_path" ] || library_path=$library_path:
            library_path=$library_path$path
        fi
    done
done
bind_subjects() {
    remaining=$subject_count
    while [ "$remaining" -gt 0 ]; do
        if [ -n "$2" ]; then "$mount_real" --bind "$2" "$1"; fi
        shift 2
        remaining=$((remaining - 1))
    done
}
bind_subjects "$@"
true_real=$("$readlink" -f "$true_binary")
"$ln" -sf "$(stage_path "$true_real")" "$runtime_stage/usr/bin/true"
if [ -n "$bootstrap_unshare" ]; then "$ln" -sf "$bootstrap_unshare" "$runtime_stage/usr/bin/unshare"; fi
if [ -n "$bootstrap_mount" ]; then "$ln" -sf "$bootstrap_mount" "$runtime_stage/usr/bin/mount"; fi
if [ -n "$bootstrap_shell" ]; then "$ln" -sf "$bootstrap_shell" "$runtime_stage/usr/bin/sh"; fi
if [ -n "$bootstrap_unshare$bootstrap_mount$bootstrap_shell" ]; then
    "$mount_real" --bind "$runtime_stage/usr/bin" /usr/bin
    "$mount_real" --bind "$runtime_stage/usr/bin" /bin
fi
mount=$(stage_path "$mount_real")
umount=$(stage_path "$umount_real")
run "$mount" -t tmpfs -o mode=0755 jet-no-nix /nix
run "$umount" -l "$host_nix"
if [ -e /nix/store ] || [ -L /nix/store ]; then
    printf '%s\n' 'test child started with a visible /nix/store' >&2
    exit 125
fi
export LD_LIBRARY_PATH="$library_path${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
while [ "$subject_count" -gt 0 ]; do shift 2; subject_count=$((subject_count - 1)); done
: > "$bootstrap/entered"
# The original step environment is installed only after bootstrap discovery.
# The loader's staged library path remains available to nested children.
if [ "$subject_path_set" = x ]; then export PATH="$subject_path"; else unset PATH; fi
subject_format=$1
shift 2
subject=$(stage_path "$subject_real")
if [ "$subject_format" = elf ]; then
    if [ -n "$library_path" ]; then exec "$loader" --library-path "$library_path" "$subject" "$@"
    else exec "$loader" "$subject" "$@"; fi
fi
exec "$subject" "$@"
