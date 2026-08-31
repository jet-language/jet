#!/bin/sh
# Hostile local proof for bounded, no-follow staging and atomic publication.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
scratch=${JET_TEST_SCRATCH:-"$repo/.tmp/jet-test-scratch"}
case "$scratch" in
    /tmp|/tmp/*)
        echo "index/cache staging proof must not use /tmp" >&2
        exit 1
        ;;
esac
work="$scratch/index-cache-stage-$$"

cleanup() {
    if [ "${race_pid:-}" ]; then kill "$race_pid" 2>/dev/null || true; wait "$race_pid" 2>/dev/null || true; fi
    rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

channel=nixpkgs-unstable
revision=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
system=x86_64-linux
index="$work/producer"
root="$work/jetpack-root"
fake="$work/fake-jetpack"
stage="$work/stage-1"
index_host="$work/index-host"
cache_host="$work/cache-host"
mkdir -p "$index/index-v1/$revision/$system" "$root"

write_fixture() {
    fixture_root=$1
    fixture_generation=$2
    fixture_text=$3
    fixture_index="$fixture_root/index-v1/$revision/$system"
    mkdir -p "$fixture_index"
    printf '%s\n' "$fixture_text" >"$fixture_root/target.bytes"
    target_digest=$(sha256sum "$fixture_root/target.bytes" | awk '{print $1}')
    target="$fixture_index/$target_digest.json.zst"
    cp "$fixture_root/target.bytes" "$target"
    printf '%s' '{"schema":1,"key_id":"jet-test-index-v1","algorithm":"ed25519","signature":"AA=="}' >"$target.sig.json"
    target_length=$(wc -c <"$target")
    signature_digest=$(sha256sum "$target.sig.json" | awk '{print $1}')
    cat >"$fixture_root/manifest.json" <<EOF
{"schema":1,"channel":"$channel","generation":$fixture_generation,"issued_unix":1,"expires_unix":2147483647,"targets":[{"revision":"$revision","system":"$system","url":"https://index.jet-lang.dev/index-v1/$revision/$system/$target_digest.json.zst","signature_url":"https://index.jet-lang.dev/index-v1/$revision/$system/$target_digest.json.zst.sig.json","sha256":"$target_digest","compressed_length":$target_length,"decoded_length":1,"record_count":0,"index_signature_sha256":"$signature_digest","discoverable":true}]}
EOF
    truncate -s -1 "$fixture_root/manifest.json"
    printf '%s' '{"schema":1,"key_id":"jet-test-index-v1","algorithm":"ed25519","signature":"AA=="}' >"$fixture_root/manifest.json.sig.json"
}

write_fixture "$index" 1 "compressed index fixture"
target_digest_1=$target_digest

cat >"$fake" <<'EOF'
#!/bin/sh
set -eu
destination=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--to" ]; then destination=$2; shift 2; else shift; fi
done
[ -n "$destination" ]
mkdir -p "$destination/nar" "$destination/trust"
printf 'cache object\n' >"$destination/nar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.nar"
printf 'StorePath: /nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache\n' >"$destination/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.narinfo"
printf 'jet-cache-receipt-v1\n' >"$destination/trust/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.receipt"
EOF
chmod 755 "$fake"

tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$index" \
    --hangar-root "$root" \
    --output "$stage" \
    --channel "$channel" \
    --role public \
    --jetpack "$fake"

target_path=$(find "$stage/index/index-v1" -name '*.json.zst' -type f -print)
[ -f "$stage/index/v1/$channel/manifest.json" ]
[ -f "$stage/index/v1/$channel/manifest.json.sig.json" ]
[ -f "$target_path.sig.json" ]
[ -f "$stage/cache/nar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.nar" ]
[ ! -e "$stage/index/v1/$channel/manifest.json.sig.request" ]

tools/jetpack-infra/stage-index-cache.sh publish \
    --staging "$stage" \
    --channel "$channel" \
    --index-destination "$index_host" \
    --cache-destination "$cache_host"

manifest_digest=$(sha256sum "$index/manifest.json" | awk '{print $1}')
generation_1="g1-$manifest_digest"
[ -L "$index_host/current" ]
[ "$(readlink "$index_host/current")" = "generations/$generation_1" ]
[ -f "$index_host/current/v1/$channel/manifest.json" ]
[ -f "$index_host/current/index-v1/$revision/$system/$(basename "$target_path")" ]
[ -f "$cache_host/current/nar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.nar" ]
[ -d "$index_host/generations/$generation_1" ]
[ -d "$cache_host/generations/$generation_1" ]
[ ! -e "$index_host/current/v1/$channel/manifest.json.sig.request" ]

mkdir "$work/real-index-host"
ln -s "$work/real-index-host" "$work/symlink-index-host"
if tools/jetpack-infra/stage-index-cache.sh publish \
    --staging "$stage" --channel "$channel" \
    --index-destination "$work/symlink-index-host" \
    --cache-destination "$work/cache-host-symlink-check"; then
    echo "symlinked publication root was accepted" >&2
    exit 1
fi

# Same generation is idempotent. Existing immutable bytes cannot be replaced.
tools/jetpack-infra/stage-index-cache.sh publish \
    --staging "$stage" \
    --channel "$channel" \
    --index-destination "$index_host" \
    --cache-destination "$cache_host"

stage_2="$work/stage-2"
index_2="$work/producer-2"
write_fixture "$index_2" 2 "refreshed compressed index fixture"
tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$index_2" \
    --hangar-root "$root" \
    --output "$stage_2" \
    --channel "$channel" \
    --jetpack "$fake"
manifest_digest_2=$(sha256sum "$index_2/manifest.json" | awk '{print $1}')
generation_2="g2-$manifest_digest_2"
tools/jetpack-infra/stage-index-cache.sh publish \
    --staging "$stage_2" \
    --channel "$channel" \
    --index-destination "$index_host" \
    --cache-destination "$cache_host"
[ "$(readlink "$index_host/current")" = "generations/$generation_2" ]
[ -d "$index_host/generations/$generation_1" ]
[ -d "$index_host/generations/$generation_2" ]
[ -f "$index_host/generations/$generation_1/v1/$channel/manifest.json" ]
[ -f "$cache_host/generations/$generation_1/nar/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-cache.nar" ]

if tools/jetpack-infra/stage-index-cache.sh publish \
    --staging "$stage" \
    --channel "$channel" \
    --index-destination "$index_host" \
    --cache-destination "$cache_host" >"$work/rollback.out" 2>"$work/rollback.err"; then
    echo "publisher accepted a generation rollback" >&2
    exit 1
fi
grep -q "generation rollback" "$work/rollback.err"

expect_stage_failure() {
    label=$1
    output="$work/reject-$label"
    if tools/jetpack-infra/stage-index-cache.sh \
        --index-root "$index" --hangar-root "$root" --output "$output" \
        --channel "$channel" --jetpack "$fake" >"$work/$label.out" 2>"$work/$label.err"; then
        echo "$label input was accepted" >&2
        exit 1
    fi
}

# Canonical channel validation blocks traversal, separators, uppercase, and overlong names.
if tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$index" --hangar-root "$root" --output "$work/bad-channel" \
    --channel '../escape' --jetpack "$fake"; then
    echo "traversal channel was accepted" >&2
    exit 1
fi
if tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$index" --hangar-root "$root" --output "$work/bad-channel-case" \
    --channel 'NIXPKGS-UNSTABLE' --jetpack "$fake"; then
    echo "noncanonical channel was accepted" >&2
    exit 1
fi

outside="$work/outside"
printf 'outside\n' >"$outside"
target="$index/index-v1/$revision/$system/$(basename "$target_path")"
rm "$target"
ln -s "$outside" "$target"
expect_stage_failure target-symlink
rm "$target"
cp "$index/target.bytes" "$target"

rm "$target.sig.json"
ln -s "$outside" "$target.sig.json"
expect_stage_failure signature-symlink
rm "$target.sig.json"
printf '%s' '{"schema":1,"key_id":"jet-test-index-v1","algorithm":"ed25519","signature":"AA=="}' >"$target.sig.json"

rm "$target"
ln "$index/target.bytes" "$target"
expect_stage_failure target-hardlink
rm "$target"
cp "$index/target.bytes" "$target"

rm "$target"
mkfifo "$target"
expect_stage_failure target-special
rm "$target"
cp "$index/target.bytes" "$target"

rm "$target"
truncate -s $((32 * 1024 * 1024 + 1)) "$target"
expect_stage_failure target-oversize
rm "$target"
cp "$index/target.bytes" "$target"

mkdir "$work/real-output"
ln -s "$work/real-output" "$work/symlink-output"
if tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$index" --hangar-root "$root" --output "$work/symlink-output" \
    --channel "$channel" --jetpack "$fake"; then
    echo "symlinked output root was accepted" >&2
    exit 1
fi

# Ancestor race: success may target only held real descriptors; rejection is also safe.
race_parent="$work/race-parent"
race_root="$race_parent/input"
race_trap="$race_parent/trap"
race_saved="$race_parent/saved"
mkdir -p "$race_parent" "$race_root" "$race_trap"
cp -a "$index/." "$race_root/"
cp -a "$index/." "$race_trap/"
race_target="$race_trap/index-v1/$revision/$system/$(basename "$target_path")"
rm "$race_target"
printf 'attacker bytes\n' >"$race_trap/attacker.bytes"
attacker_digest=$(sha256sum "$race_trap/attacker.bytes" | awk '{print $1}')
attacker_target="$race_trap/index-v1/$revision/$system/$attacker_digest.json.zst"
cp "$race_trap/attacker.bytes" "$attacker_target"
printf '%s' '{"schema":1,"key_id":"jet-test-index-v1","algorithm":"ed25519","signature":"AA=="}' >"$attacker_target.sig.json"
attacker_length=$(wc -c <"$attacker_target")
attacker_signature_digest=$(sha256sum "$attacker_target.sig.json" | awk '{print $1}')
cat >"$race_trap/manifest.json" <<EOF
{"schema":1,"channel":"$channel","generation":1,"issued_unix":1,"expires_unix":2147483647,"targets":[{"revision":"$revision","system":"$system","url":"https://index.jet-lang.dev/index-v1/$revision/$system/$attacker_digest.json.zst","signature_url":"https://index.jet-lang.dev/index-v1/$revision/$system/$attacker_digest.json.zst.sig.json","sha256":"$attacker_digest","compressed_length":$attacker_length,"decoded_length":1,"record_count":0,"index_signature_sha256":"$attacker_signature_digest","discoverable":true}]}
EOF
truncate -s -1 "$race_trap/manifest.json"
(
    i=0
    while [ "$i" -lt 2000 ]; do
        i=$((i + 1))
        if [ -d "$race_root" ] && [ ! -L "$race_root" ]; then
            mv "$race_root" "$race_saved" 2>/dev/null || continue
            mv "$race_trap" "$race_root" 2>/dev/null || {
                mv "$race_saved" "$race_root" 2>/dev/null || true
                continue
            }
            mv "$race_saved" "$race_trap" 2>/dev/null || true
        fi
        if [ -d "$race_root" ] && [ ! -L "$race_root" ]; then
            mv "$race_root" "$race_saved" 2>/dev/null || continue
            mv "$race_trap" "$race_root" 2>/dev/null || {
                mv "$race_saved" "$race_root" 2>/dev/null || true
                continue
            }
            mv "$race_saved" "$race_trap" 2>/dev/null || true
        fi
    done
) &
race_pid=$!
race_output="$work/race-output"
if tools/jetpack-infra/stage-index-cache.sh \
    --index-root "$race_root" --hangar-root "$root" --output "$race_output" \
    --channel "$channel" --jetpack "$fake" >"$work/race.out" 2>"$work/race.err"; then
    race_result=$(find "$race_output/index/index-v1" -name '*.json.zst' -type f -exec sha256sum {} \;)
    case "$race_result" in
        *"$target_digest_1"*) ;;
        *)
            echo "ancestor race published attacker bytes" >&2
            exit 1
            ;;
    esac
fi
wait "$race_pid" 2>/dev/null || true
race_pid=

rm "$target.sig.json"
expect_stage_failure missing-signature

echo "index/cache staging, hostile-input, race, refresh, and rollback proofs passed"
