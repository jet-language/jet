#!/usr/bin/env bash
# Shared POSIX adapter for foreign build hosts.
# Jet remains the compiler, checker, and Library exporter. This adapter owns
# input identity, bounded locking, and the final host artifact-set commit marker.

set -euo pipefail

usage() {
    cat >&2 <<'EOF'
usage: jet-library.sh --jet COMMAND --project DIR --entry FILE --output NAME \
  --library NAME --dest DIR [--kind static|shared|both] [--profile NAME] \
  [--toolchain COMMAND] [--timeout SECONDS] [--loadable] [--stage-project] \
  [--input FILE ...]
EOF
    exit 2
}

die() {
    echo "jet-host: $*" >&2
    exit 1
}

json_string() {
    local value=$1
    local escaped=
    local char
    local ord
    local i
    local LC_ALL=C
    for ((i = 0; i < ${#value}; i++)); do
        char=${value:i:1}
        case "$char" in
            \\) escaped+='\\\\' ;;
            '"') escaped+='\\"' ;;
            $'\b') escaped+='\\b' ;;
            $'\f') escaped+='\\f' ;;
            $'\n') escaped+='\\n' ;;
            $'\r') escaped+='\\r' ;;
            $'\t') escaped+='\\t' ;;
            *)
                printf -v ord '%d' "'$char"
                ((ord < 32)) && die "JET-HOST-INPUT: receipt value contains a control character"
                escaped+="$char"
                ;;
        esac
    done
    printf '"%s"' "$escaped"
}

json_string_array() {
    local first=1
    local value
    printf '['
    for value in "$@"; do
        if ((first)); then first=0; else printf ','; fi
        json_string "$value"
    done
    printf ']'
}

json_fragment_array() {
    local first=1
    local value
    printf '['
    for value in "$@"; do
        if ((first)); then first=0; else printf ','; fi
        printf '%s' "$value"
    done
    printf ']'
}

sha256_stream() {
    local digest
    if command -v sha256sum >/dev/null 2>&1; then
        digest=$(sha256sum | { read -r value _; printf '%s' "$value"; })
    elif command -v shasum >/dev/null 2>&1; then
        digest=$(shasum -a 256 | { read -r value _; printf '%s' "$value"; })
    else
        die "JET-HOST-TOOL: sha256sum or shasum is required for artifact identity"
    fi
    [[ "$digest" =~ ^[0-9a-fA-F]{64}$ ]] || die "JET-HOST-TOOL: hash tool returned an invalid digest"
    printf 'sha256-%s\n' "$digest"
}

sha256_file() {
    local path=$1
    [[ -f "$path" && ! -L "$path" ]] || die "JET-HOST-INPUT: cannot hash non-regular file '$path'"
    sha256_stream < "$path"
}

file_bytes() {
    local bytes
    bytes=$(wc -c < "$1")
    bytes=${bytes//[[:space:]]/}
    [[ "$bytes" =~ ^[0-9]+$ ]] || die "JET-HOST-INPUT: could not measure '$1'"
    printf '%s\n' "$bytes"
}

reject_link_components() {
    local path=$1
    while [[ "$path" != "/" && "$path" != "." ]]; do
        [[ ! -L "$path" ]] || die "JET-HOST-INPUT: path '$path' is a symlink or reparse point"
        path=$(dirname "$path")
    done
}

canonical_dir() {
    local path=$1
    [[ -d "$path" && ! -L "$path" ]] || die "JET-HOST-INPUT: '$path' is not a real directory"
    reject_link_components "$path"
    local first second
    first=$(cd "$path" && pwd -P)
    second=$(cd "$path" && pwd -P)
    [[ "$first" == "$second" ]] || die "JET-HOST-INPUT: directory '$path' changed while it was being resolved"
    printf '%s\n' "$second"
}

resolve_executable() {
    local name=$1
    local path
    if [[ "$name" == */* ]]; then
        path=$name
    else
        path=$(command -v "$name" || true)
    fi
    [[ -n "$path" && -x "$path" ]] || die "JET-HOST-TOOL: executable '$name' is missing or not executable"
    if [[ "$path" != /* ]]; then
        path="$(cd "$(dirname "$path")" && pwd -P)/$(basename "$path")"
    fi
    printf '%s\n' "$path"
}

remove_owned_output() {
    local path=$1
    reject_link_components "$path"
    if [[ -L "$path" ]]; then
        die "JET-HOST-INPUT: refusing to replace or remove symlink '$path'"
    fi
    if [[ -d "$path" ]]; then
        die "JET-HOST-INPUT: refusing to replace or remove directory '$path'"
    fi
    [[ ! -e "$path" ]] || rm -f "$path"
}

remove_owned_tree() {
    local path=$1
    reject_link_components "$path"
    if [[ -L "$path" || -e "$path" && ! -d "$path" ]]; then
        die "JET-HOST-INPUT: refusing to replace or remove non-directory '$path'"
    fi
    [[ ! -e "$path" ]] || rm -rf "$path"
}

assert_publish_path() {
    local path=$1
    reject_link_components "$path"
    if [[ -L "$path" ]]; then
        die "JET-HOST-INPUT: refusing to replace symlink '$path'"
    fi
    if [[ -d "$path" ]]; then
        die "JET-HOST-INPUT: refusing to replace directory '$path'"
    fi
}

descendant_pids() {
    local parent=$1
    ps -eo pid=,ppid= | awk -v parent="$parent" '$2 == parent { print $1 }'
}

jet_process_group=0

kill_process_tree() {
    local root=$1
    local signal=$2
    [[ "$root" =~ ^[0-9]+$ && "$root" != "$$" ]] || return 0
    if ((jet_process_group)); then
        kill "-$signal" -- "-$root" 2>/dev/null || true
        return 0
    fi
    local child
    while IFS= read -r child; do
        [[ -n "$child" ]] || continue
        kill_process_tree "$child" "$signal"
    done < <(descendant_pids "$root")
    kill "-$signal" "$root" 2>/dev/null || true
}

stop_jet_process_tree() {
    local root=$1
    kill_process_tree "$root" TERM
    sleep 1
    kill_process_tree "$root" KILL
}

jet=
project=
entry=
output=
library=
dest=
kind=static
profile=dev
toolchain=unknown
timeout_seconds=900
loadable=0
stage_project=0
declared_inputs=()

while (($#)); do
    case "$1" in
        --jet|--project|--entry|--output|--library|--dest|--kind|--profile|--toolchain|--timeout|--input)
            (($# >= 2)) || usage
            case "$1" in
                --jet) jet=$2 ;;
                --project) project=$2 ;;
                --entry) entry=$2 ;;
                --output) output=$2 ;;
                --library) library=$2 ;;
                --dest) dest=$2 ;;
                --kind) kind=$2 ;;
                --profile) profile=$2 ;;
                --toolchain) toolchain=$2 ;;
                --timeout) timeout_seconds=$2 ;;
                --input) declared_inputs+=("$2") ;;
            esac
            shift 2
            ;;
        --loadable)
            loadable=1
            shift
            ;;
        --stage-project)
            stage_project=1
            shift
            ;;
        -h|--help)
            usage
            ;;
        *)
            usage
            ;;
    esac
done

[[ -n "$jet" && -n "$project" && -n "$entry" && -n "$output" && -n "$library" && -n "$dest" ]] \
    || usage
[[ "$kind" == static || "$kind" == shared || "$kind" == both ]] \
    || die "JET-HOST-ABI: unsupported Library kind '$kind'"
[[ "$library" =~ ^[A-Za-z0-9_-]+$ ]] \
    || die "JET-HOST-ABI: Library name '$library' is not a stable native artifact name"
[[ "$output" =~ ^[A-Za-z0-9_-]+$ ]] \
    || die "JET-HOST-INPUT: output name '$output' is not a manifest output name"
[[ "$profile" =~ ^[A-Za-z0-9_.-]*$ ]] \
    || die "JET-HOST-INPUT: profile '$profile' is not a stable profile name"
[[ "$timeout_seconds" =~ ^[1-9][0-9]*$ && "$timeout_seconds" -le 86400 ]] \
    || die "JET-HOST-INPUT: timeout must be a whole number from 1 through 86400 seconds"

[[ -d "$project" ]] || die "JET-HOST-INPUT: project directory '$project' does not exist"
project=$(canonical_dir "$project")
reject_link_components "$project"

if [[ ! -e "$dest" && ! -L "$dest" ]]; then
    reject_link_components "$(dirname "$dest")"
    mkdir -p "$dest"
fi
dest=$(canonical_dir "$dest")

clean_destination() {
    reject_link_components "$dest"
    [[ -d "$dest" && ! -L "$dest" ]] || die "JET-HOST-OUTPUT: destination root changed"
    for name in \
        jet-host.stamp jet-host.receipt \
        "lib$library.a" "lib$library.so" "lib$library.dylib" "lib$library.dll" \
        "$library.h" "$library.jetlib" \
        "bindings/$library.h" "bindings/$library.py" "bindings/$library.swift"; do
        remove_owned_output "$dest/$name"
    done
}

source_listing=
preflight_cleanup() {
    local status=$?
    trap - EXIT INT TERM HUP
    if [[ -n "$source_listing" && -e "$source_listing" ]]; then
        remove_owned_output "$source_listing" || true
    fi
    clean_destination || true
    exit "$status"
}
trap preflight_cleanup EXIT
trap 'exit 130' INT TERM HUP
jet=$(resolve_executable "$jet")

project_file() {
    local raw=$1
    local candidate
    if [[ "$raw" = /* ]]; then
        candidate=$raw
    else
        candidate="$project/$raw"
    fi
    [[ -f "$candidate" && ! -L "$candidate" ]] \
        || die "JET-HOST-INPUT: missing '$raw' under '$project'"
    reject_link_components "$candidate"
    candidate="$(cd "$(dirname "$candidate")" && pwd -P)/$(basename "$candidate")"
    case "$candidate" in
        "$project"/*) printf '%s\n' "$candidate" ;;
        *) die "JET-HOST-INPUT: '$raw' escapes the Jet project root" ;;
    esac
}

# A copied project must not contain a link that can change the source closure
# after the root and input checks.
project_link=$(find "$project" -type l -print -quit 2>/dev/null || true)
[[ -z "$project_link" ]] || die "JET-HOST-INPUT: project contains symlink '$project_link'"

entry_abs=$(project_file "$entry")
entry_rel=${entry_abs#"$project"/}
input_abs=()
input_rels=()
input_digests=()
source_closure_rels=()

source_listing=$(mktemp "$dest/.jet-source-list.XXXXXX")
if ! (cd "$project" && NO_COLOR=1 "$jet" project parts) > "$source_listing"; then
    remove_owned_output "$source_listing"
    die "JET-HOST-INPUT: Jet project parts could not derive the source closure"
fi
while IFS= read -r source_line; do
    [[ -n "$source_line" ]] || continue
    source_rel=$(printf '%s\n' "$source_line" | sed -E 's/^[^[:space:]]+[[:space:]]+[^[:space:]]+[[:space:]]+//')
    [[ "$source_rel" == *.jet ]] || continue
    source_abs=$(project_file "$source_rel")
    source_rel=${source_abs#"$project"/}
    duplicate=0
    for existing in "${source_closure_rels[@]}"; do
        if [[ "$existing" == "$source_rel" ]]; then
            duplicate=1
            break
        fi
    done
    ((duplicate == 0)) && source_closure_rels+=("$source_rel")
done < "$source_listing"
remove_owned_output "$source_listing"
[[ "${#source_closure_rels[@]}" -gt 0 ]] \
    || die "JET-HOST-INPUT: Jet project parts returned an empty source closure"

add_input() {
    local input=$1
    local current_abs current_rel duplicate existing
    current_abs=$(project_file "$input")
    current_rel=${current_abs#"$project"/}
    duplicate=0
    for existing in "${input_rels[@]}"; do
        if [[ "$existing" == "$current_rel" ]]; then
            duplicate=1
            break
        fi
    done
    if ((duplicate == 0)); then
        input_abs+=("$current_abs")
        input_rels+=("$current_rel")
        input_digests+=("$(sha256_file "$current_abs")")
    fi
}

add_input package.jet
add_input .jet/lock
add_input "$entry"
for source_rel in "${source_closure_rels[@]}"; do
    add_input "$source_rel"
done
for input in "${declared_inputs[@]}"; do
    add_input "$input"
done
lock_digest=${input_digests[1]}
[[ -n "$lock_digest" ]] || die "JET-HOST-INPUT: lock identity was not captured"

stage=
run_project=$project
stage_project_dir=
lock_dir="$project/.jet/foreign-host.lock"
lock_owned=0
jet_pid=
publish_complete=0
identity_file=

cleanup() {
    local status=$?
    trap - EXIT INT TERM HUP
    if [[ -n "$jet_pid" ]] && kill -0 "$jet_pid" 2>/dev/null; then
        stop_jet_process_tree "$jet_pid"
        wait "$jet_pid" 2>/dev/null || true
    fi
    if [[ -n "$identity_file" ]]; then
        remove_owned_output "$identity_file"
    fi
    if ((publish_complete == 0)); then
        clean_destination || true
    fi
    if [[ -n "$stage" ]]; then
        remove_owned_tree "$stage"
    fi
    if [[ -n "$stage_project_dir" ]]; then
        remove_owned_tree "$stage_project_dir"
    fi
    if ((lock_owned)); then
        remove_owned_output "$lock_dir/pid"
        remove_owned_output "$lock_dir/started"
        rmdir "$lock_dir" 2>/dev/null || true
    fi
    exit "$status"
}

trap cleanup EXIT
trap 'exit 130' INT TERM HUP

jet_dir="$project/.jet"
[[ ! -L "$jet_dir" ]] || die "JET-HOST-INPUT: .jet is a symlink or reparse point"
mkdir -p "$jet_dir"
reject_link_components "$jet_dir"
[[ ! -L "$lock_dir" && (! -e "$lock_dir" || -d "$lock_dir") ]] \
    || die "JET-HOST-TOOL: lock path '$lock_dir' is not a real directory"

# The lock is intentionally bounded. A dead owner is recoverable; a live owner
# gets a deterministic timeout instead of an unbounded host-build hang.
lock_deadline=$((SECONDS + 30))
while ! mkdir "$lock_dir" 2>/dev/null; do
    [[ ! -L "$lock_dir" ]] || die "JET-HOST-TOOL: lock path '$lock_dir' became a symlink"
    ((SECONDS >= lock_deadline)) && die "JET-HOST-TOOL: timed out waiting for '$lock_dir'"
    owner=
    started=
    [[ -r "$lock_dir/pid" ]] && owner=$(<"$lock_dir/pid")
    [[ -r "$lock_dir/started" ]] && started=$(<"$lock_dir/started")
    if [[ "$owner" =~ ^[0-9]+$ ]] && ! kill -0 "$owner" 2>/dev/null; then
        remove_owned_output "$lock_dir/pid"
        remove_owned_output "$lock_dir/started"
        rmdir "$lock_dir" 2>/dev/null || true
        continue
    fi
    sleep 1
done
lock_owned=1
[[ -d "$lock_dir" && ! -L "$lock_dir" ]] \
    || die "JET-HOST-TOOL: lock path '$lock_dir' changed during acquisition"
printf '%s\n' "$$" > "$lock_dir/pid"
printf '%s\n' "$(date +%s)" > "$lock_dir/started"

if ((stage_project)); then
    stage_project_dir=$(mktemp -d "$dest/.jet-project.XXXXXX")
    for index in "${!input_abs[@]}"; do
        relative=${input_rels[index]}
        relative_dir=$(dirname "$relative")
        [[ "$relative_dir" == . ]] || mkdir -p "$stage_project_dir/$relative_dir"
        cp -p "${input_abs[index]}" "$stage_project_dir/$relative"
    done
    if [[ -L "$stage_project_dir/target" || -e "$stage_project_dir/target" && ! -d "$stage_project_dir/target" ]]; then
        die "JET-HOST-INPUT: staged project target is not a real directory"
    fi
    remove_owned_tree "$stage_project_dir/target"
    if [[ -L "$stage_project_dir/.jet/foreign-host.lock" ]]; then
        die "JET-HOST-INPUT: staged project lock is a symlink or reparse point"
    fi
    remove_owned_tree "$stage_project_dir/.jet/foreign-host.lock"
    run_project=$stage_project_dir
fi

capture_identity() {
    local program=$1
    local label=$2
    capture_command "$program" "$label" --version
}

capture_command() {
    local program=$1
    local label=$2
    shift 2
    reject_link_components "$dest"
    [[ -d "$dest" && ! -L "$dest" ]] || die "JET-HOST-OUTPUT: destination root changed"
    identity_file=$(mktemp "$dest/.jet-host-identity.XXXXXX")
    "$program" "$@" > "$identity_file" 2>&1 \
        || die "JET-HOST-TOOL: could not query $label identity"
    CAPTURED_DIGEST=$(sha256_file "$identity_file")
    CAPTURED_VERSION=
    CAPTURED_OUTPUT=
    IFS= read -r CAPTURED_VERSION < "$identity_file" || true
    CAPTURED_OUTPUT=$(<"$identity_file")
    remove_owned_output "$identity_file"
    identity_file=
}

capture_identity "$jet" "Jet"
jet_version=$CAPTURED_VERSION
jet_identity=$CAPTURED_DIGEST

rustc_name=${RUSTC:-rustc}
rustc=$(resolve_executable "$rustc_name")
rustc_dir=$(dirname "$rustc")
capture_command "$rustc" "rustc target identity" -vV
rustc_identity=$CAPTURED_DIGEST
rustc_version=$CAPTURED_VERSION
rustc_text=$CAPTURED_OUTPUT
target_triple=
while IFS= read -r rustc_line; do
    case "$rustc_line" in
        "host: "*) target_triple=${rustc_line#host: } ;;
    esac
done <<< "$rustc_text"
[[ -n "$target_triple" ]] || die "JET-HOST-TOOL: rustc did not report a host target"

capture_command "$rustc" "rustc sysroot" --print sysroot
sysroot_identity=$CAPTURED_DIGEST
sysroot_path=$CAPTURED_OUTPUT
sysroot_path=${sysroot_path%%$'\n'*}
[[ -n "$sysroot_path" ]] || die "JET-HOST-TOOL: rustc did not report a sysroot"

capture_command "$rustc" "rustc target library directory" --print target-libdir
target_libdir_identity=$CAPTURED_DIGEST
target_libdir=$CAPTURED_OUTPUT
target_libdir=${target_libdir%%$'\n'*}
[[ -n "$target_libdir" ]] || die "JET-HOST-TOOL: rustc did not report a target library directory"

toolchain_name=$toolchain
toolchain_path=unspecified
toolchain_identity=not-specified
toolchain_version=not-specified
if [[ -n "$toolchain" && "$toolchain" != unknown ]]; then
    toolchain_path=$(resolve_executable "$toolchain")
    capture_identity "$toolchain_path" "host toolchain"
    toolchain_version=$CAPTURED_VERSION
    toolchain_identity=$CAPTURED_DIGEST
fi

linker_name=$toolchain_name
linker_path=$toolchain_path
if [[ "$toolchain" == unknown || -z "$toolchain" ]]; then
    # rustc has no stable `--print linker` query. Match its normal host
    # selection: an explicit linker/compiler wins, then the platform `cc`.
    linker_name=${RUSTC_LINKER:-${CC:-cc}}
    [[ -n "$linker_name" ]] || die "JET-HOST-TOOL: no linker was selected"
    linker_path=$(resolve_executable "$linker_name")
else
    linker_path=$toolchain_path
fi
capture_identity "$linker_path" "linker"
linker_version=$CAPTURED_VERSION
linker_identity=$CAPTURED_DIGEST
[[ -n "$linker_path" && "$linker_path" != unspecified ]] \
    || die "JET-HOST-TOOL: linker identity is incomplete"
linker_basename=$(basename "$linker_path" | tr '[:upper:]' '[:lower:]')
case "$linker_basename" in
    cl|cl.exe|link|link.exe)
        die "JET-HOST-ABI: Jet emits GNU .a artifacts; use a GNU-compatible host toolchain instead of MSVC"
        ;;
esac

command -v ps >/dev/null 2>&1 \
    || die "JET-HOST-TOOL: ps is required for process-tree cancellation"

clean_destination

jet_args=(build --lib --locked --output "$output")
if [[ -n "$profile" ]]; then
    jet_args+=("--profile=$profile")
fi
jet_args+=("$entry_rel")
if command -v setsid >/dev/null 2>&1; then
    jet_process_group=1
    (
        cd "$run_project"
        PATH="$rustc_dir:$PATH" RUSTC="$rustc" RUSTC_LINKER="$linker_path" CC="$linker_path" NO_COLOR=1 \
            exec setsid "$jet" "${jet_args[@]}"
    ) &
else
    jet_process_group=0
    (
        cd "$run_project"
        PATH="$rustc_dir:$PATH" RUSTC="$rustc" RUSTC_LINKER="$linker_path" CC="$linker_path" NO_COLOR=1 \
            exec "$jet" "${jet_args[@]}"
    ) &
fi
jet_pid=$!
build_deadline=$((SECONDS + timeout_seconds))
while kill -0 "$jet_pid" 2>/dev/null; do
    if ((SECONDS >= build_deadline)); then
        stop_jet_process_tree "$jet_pid"
        wait "$jet_pid" 2>/dev/null || true
        jet_pid=
        jet_process_group=0
        die "JET-HOST-BUILD: timed out after ${timeout_seconds}s; no new host artifact was published"
    fi
    sleep 0.1
done
if wait "$jet_pid"; then
    status=0
else
    status=$?
fi
jet_pid=
jet_process_group=0
((status == 0)) \
    || die "JET-HOST-BUILD: Jet Library build failed with status $status; no new host artifact was published"

target="$run_project/target"
target=$(canonical_dir "$target")
case "$(uname -s)" in
    Darwin*) shared_artifact="lib$library.dylib" ;;
    MINGW*|MSYS*|CYGWIN*) shared_artifact="lib$library.dll" ;;
    *) shared_artifact="lib$library.so" ;;
esac
artifacts=()
case "$kind" in
    static|both) artifacts+=("lib$library.a") ;;
esac
case "$kind" in
    shared|both) artifacts+=("$shared_artifact") ;;
esac
artifacts+=("$library.h")
if ((loadable)); then
    artifacts+=("$library.jetlib")
fi
required_artifacts=("${artifacts[@]}")

completion="$target/.$library.jet-library.complete"
reject_link_components "$completion"
[[ -f "$completion" && ! -L "$completion" ]] \
    || die "JET-HOST-ABI: Jet did not publish a complete Library artifact set"
IFS= read -r completion_header < "$completion" || true
[[ "$completion_header" == jet-library-set-v1 ]] \
    || die "JET-HOST-ABI: Jet Library completion marker is invalid"
known_artifacts=(
    "lib$library.a" "lib$library.so" "lib$library.dylib" "lib$library.dll"
    "$library.h" "$library.jetlib"
    "bindings/$library.h" "bindings/$library.py" "bindings/$library.swift"
)
marker_names=()
marker_digests=()
marker_index_for() {
    local wanted=$1
    MARKER_INDEX=-1
    local index
    for index in "${!marker_names[@]}"; do
        if [[ "${marker_names[index]}" == "$wanted" ]]; then
            MARKER_INDEX=$index
            return 0
        fi
    done
    return 1
}
while IFS= read -r marker_line; do
    [[ -n "$marker_line" ]] \
        || die "JET-HOST-ABI: completion marker contains an invalid entry"
    marker_name=${marker_line%%$'\t'*}
    marker_digest=${marker_line#*$'\t'}
    [[ "$marker_name" != "$marker_line" && "$marker_digest" != *$'\t'* ]] \
        || die "JET-HOST-ABI: completion marker contains an invalid entry"
    [[ "$marker_digest" =~ ^sha256-[0-9a-fA-F]{64}$ ]] \
        || die "JET-HOST-ABI: completion marker contains an invalid digest"
    case "$marker_name" in
        /*|*\\*|../*|*/../*|*/..|.|..|*/|*/.)
            die "JET-HOST-ABI: completion marker contains an unsafe artifact path '$marker_name'"
            ;;
    esac
    IFS=/ read -ra marker_components <<< "$marker_name"
    for marker_component in "${marker_components[@]}"; do
        [[ -n "$marker_component" && "$marker_component" != . && "$marker_component" != .. ]] \
            || die "JET-HOST-ABI: completion marker contains an unsafe artifact path '$marker_name'"
    done
    known=0
    for artifact in "${known_artifacts[@]}"; do
        if [[ "$marker_name" == "$artifact" ]]; then
            known=1
            break
        fi
    done
    ((known)) || die "JET-HOST-ABI: completion marker names an unexpected artifact '$marker_name'"
    if marker_index_for "$marker_name"; then
        die "JET-HOST-ABI: completion marker repeats target/$marker_name"
    fi
    marker_names+=("$marker_name")
    marker_digests+=("$marker_digest")
done < <(tail -n +2 "$completion")
[[ "${#marker_names[@]}" -gt 0 ]] \
    || die "JET-HOST-ABI: completion marker does not describe any artifact"
for artifact in "${marker_names[@]}"; do
    artifact_path="$target/$artifact"
    reject_link_components "$artifact_path"
    [[ -f "$artifact_path" && ! -L "$artifact_path" && -s "$artifact_path" ]] \
        || die "JET-HOST-ABI: completion marker names missing or empty target/$artifact"
    marker_index_for "$artifact"
    [[ "${marker_digests[MARKER_INDEX]}" == "$(sha256_file "$artifact_path")" ]] \
        || die "JET-HOST-ABI: completion marker does not match target/$artifact"
done
for artifact in "${required_artifacts[@]}"; do
    marker_index_for "$artifact"
    ((MARKER_INDEX >= 0)) \
        || die "JET-HOST-ABI: completion marker omits required target/$artifact"
done
for artifact in "${known_artifacts[@]}"; do
    artifact_path="$target/$artifact"
    if [[ -e "$artifact_path" || -L "$artifact_path" ]]; then
        marker_index_for "$artifact"
        ((MARKER_INDEX >= 0)) \
            || die "JET-HOST-ABI: target contains stale uncommitted artifact '$artifact'"
    fi
done

if ((loadable)); then
    jetlib_magic=$(dd if="$target/$library.jetlib" bs=1 count=14 2>/dev/null | od -An -tx1 | tr -d ' \n')
    expected_jetlib_magic=$(printf 'jet-jetlib-v3\0' | od -An -tx1 | tr -d ' \n')
    [[ "$jetlib_magic" == "$expected_jetlib_magic" ]] \
        || die "JET-HOST-ABI: target/$library.jetlib is not a Jet Library artifact"
fi

# Direct builds must not publish a receipt for a source closure that changed
# while Jet was running. Staged builds already own an immutable project copy.
if ((stage_project == 0)); then
    for index in "${!input_abs[@]}"; do
        [[ "$(sha256_file "${input_abs[index]}")" == "${input_digests[index]}" ]] \
            || die "JET-HOST-INPUT: input '${input_rels[index]}' changed during the Jet build"
    done
fi

reject_link_components "$dest"
[[ -d "$dest" && ! -L "$dest" ]] || die "JET-HOST-OUTPUT: destination root changed"
stage=$(mktemp -d "$dest/.jet-host-stage.XXXXXX")
artifact_json=()
for artifact in "${artifacts[@]}"; do
    cp -p "$target/$artifact" "$stage/$artifact"
    digest=$(sha256_file "$stage/$artifact")
    bytes=$(file_bytes "$stage/$artifact")
    artifact_json+=("$(printf '{"name":%s,"bytes":%s,"digest":%s}' \
        "$(json_string "$artifact")" "$bytes" "$(json_string "$digest")")")
done

input_json=()
for index in "${!input_rels[@]}"; do
    input_json+=("$(printf '{"path":%s,"digest":%s}' \
        "$(json_string "${input_rels[index]}")" "$(json_string "${input_digests[index]}")")")
done

command_json=$(json_string_array "${jet_args[@]}")
inputs_json=$(json_fragment_array "${input_json[@]}")
artifacts_json=$(json_fragment_array "${artifact_json[@]}")
{
    printf '{"schema":2,"jet":{"path":'
    json_string "$jet"
    printf ',"version":'
    json_string "$jet_version"
    printf ',"identity":'
    json_string "$jet_identity"
    printf '},"toolchain":{"name":'
    json_string "$toolchain_name"
    printf ',"path":'
    json_string "$toolchain_path"
    printf ',"version":'
    json_string "$toolchain_version"
    printf ',"identity":'
    json_string "$toolchain_identity"
    printf '},"target":{"triple":'
    json_string "$target_triple"
    printf ',"rustc":'
    json_string "$rustc"
    printf ',"version":'
    json_string "$rustc_version"
    printf ',"rustc_identity":'
    json_string "$rustc_identity"
    printf ',"sysroot":'
    json_string "$sysroot_path"
    printf ',"sysroot_identity":'
    json_string "$sysroot_identity"
    printf ',"target_libdir":'
    json_string "$target_libdir"
    printf ',"target_libdir_identity":'
    json_string "$target_libdir_identity"
    printf '},"linker":{"name":'
    json_string "$linker_name"
    printf ',"path":'
    json_string "$linker_path"
    printf ',"identity":'
    json_string "$linker_identity"
    printf ',"version":'
    json_string "$linker_version"
    printf '},"lock":{"path":".jet/lock","digest":'
    json_string "$lock_digest"
    printf '},"inputs":%s,"build":{"entry":' "$inputs_json"
    json_string "$entry_rel"
    printf ',"output":'
    json_string "$output"
    printf ',"library":'
    json_string "$library"
    printf ',"kind":'
    json_string "$kind"
    printf ',"profile":'
    json_string "$profile"
    printf ',"loadable":%s,"command":%s},"artifacts":%s}\n' \
        "$([[ "$loadable" == 1 ]] && printf true || printf false)" \
        "$command_json" "$artifacts_json"
} > "$stage/jet-host.receipt"

receipt_digest=$(sha256_file "$stage/jet-host.receipt")
printf 'jet-foreign-host-v2\nreceipt=%s\n' "$receipt_digest" > "$stage/jet-host.stamp"

# The completion stamp is the commit marker. Until its rename succeeds, a
# consumer must treat the destination as incomplete and the trap removes it.
for artifact in "${artifacts[@]}"; do
    assert_publish_path "$dest/$artifact"
    mv "$stage/$artifact" "$dest/$artifact"
done
assert_publish_path "$dest/jet-host.receipt"
assert_publish_path "$dest/jet-host.stamp"
mv "$stage/jet-host.receipt" "$dest/jet-host.receipt"
mv "$stage/jet-host.stamp" "$dest/jet-host.stamp"
publish_complete=1
