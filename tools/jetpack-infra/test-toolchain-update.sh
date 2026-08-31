#!/bin/sh
# End-to-end local-file proof for signed, keyless, and corrupt self-update inputs.
# The Ed25519 key is generated in disposable scratch at runtime.
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
jet=$(printenv JET_BIN 2>/dev/null || true)
if [ -z "$jet" ]; then jet="$repo/target/debug/jet"; fi
scratch=$(printenv JET_TEST_SCRATCH 2>/dev/null || true)
if [ -z "$scratch" ]; then scratch="$HOME/.cache/jet-test-scratch"; fi
work="$scratch/toolchain-update-$$"
stage="$work/site"
root="$work/root"
key_id=local-channel-v1
target=x86_64-unknown-linux-gnu
secondary_target=aarch64-unknown-linux-gnu

cleanup() {
    rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

[ -x "$jet" ] || {
    echo "missing $jet; build jet first" >&2
    exit 1
}
command -v openssl >/dev/null 2>&1 || {
    echo "openssl is required for the ephemeral signing witness" >&2
    exit 1
}
mkdir -p "$work" "$root/trust"
openssl genpkey -algorithm ED25519 -out "$work/private.pem" >/dev/null 2>&1
openssl pkey -in "$work/private.pem" -pubout -outform DER -out "$work/public.der" >/dev/null 2>&1
public_key=$(tail -c 32 "$work/public.der" | base64 | tr -d '\n')
printf '%s:%s\n' "$key_id" "$public_key" >"$work/toolchain.pub"

printf '#!/bin/sh\nprintf local-toolchain\\n\n' >"$work/jet-artifact"
chmod 755 "$work/jet-artifact"
printf '#!/bin/sh\nprintf local-toolchain-arm\\n\n' >"$work/jet-artifact-arm"
chmod 755 "$work/jet-artifact-arm"
tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 \
    --sequence 1 \
    --published-at "$(date -u +%s)" \
    --channel stable \
    --output "$stage" \
    --artifact "$target=$work/jet-artifact" \
    --artifact "$secondary_target=$work/jet-artifact-arm"

sign_request() {
    request=$1
    signature=$2
    openssl pkeyutl -sign -rawin -inkey "$work/private.pem" \
        -in "$request" -out "$work/signature.bin" >/dev/null 2>&1
    signature_b64=$(base64 "$work/signature.bin" | tr -d '\n')
    printf '{"schema":1,"key_id":"%s","algorithm":"ed25519","signature":"%s"}' \
        "$key_id" "$signature_b64" >"$signature"
}

manifest="$stage/v1/stable/manifest.json"
artifact="$stage/v1/stable/1.2.3/jet-1.2.3-$target"
secondary_artifact="$stage/v1/stable/1.2.3/jet-1.2.3-$secondary_target"
sign_request "$manifest.sig.request" "$manifest.sig.json"
sign_request "$artifact.sig.request" "$artifact.sig.json"
sign_request "$secondary_artifact.sig.request" "$secondary_artifact.sig.json"

output=$(JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$target" \
    --trust-key "$work/toolchain.pub" \
    --dry-run)
case "$output" in
    *"verified jet 1.2.3 for $target from file://$stage"*) ;;
    *)
        echo "unexpected self-update proof: $output" >&2
        exit 1
        ;;
esac

cross_output=$(JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$secondary_target" \
    --trust-key "$work/toolchain.pub" \
    --dry-run)
case "$cross_output" in
    *"verified jet 1.2.3 for $secondary_target"*) ;;
    *)
        echo "cross-platform dry-run was not accepted: $cross_output" >&2
        exit 1
        ;;
esac

if JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$secondary_target" \
    --trust-key "$work/toolchain.pub" \
    --apply >"$work/cross-apply.out" 2>"$work/cross-apply.err"; then
    echo "cross-platform apply was accepted" >&2
    exit 1
fi
grep -q "exact host platform" "$work/cross-apply.err"

rm "$manifest.sig.json" "$artifact.sig.json" "$secondary_artifact.sig.json"
if JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$target" \
    --trust-key "$work/toolchain.pub" \
    --dry-run >"$work/default.out" 2>"$work/default.err"; then
    echo "unsigned channel was accepted without an explicit override" >&2
    exit 1
fi
grep -q "E2105" "$work/default.err"

output=$(JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$target" \
    --allow-unofficial \
    --dry-run)
case "$output" in
    *"unofficial keyless source"*) ;;
    *)
        echo "explicit keyless proof did not identify its trust tier: $output" >&2
        exit 1
        ;;
esac

printf 'X' | dd of="$artifact" bs=1 count=1 conv=notrunc >/dev/null 2>&1
if JETPACK_ROOT="$root" "$jet" self update \
    --endpoint "file://$stage" \
    --channel stable \
    --platform "$target" \
    --allow-unofficial \
    --dry-run >"$work/corrupt.out" 2>"$work/corrupt.err"; then
    echo "corrupted artifact was accepted" >&2
    exit 1
fi
grep -q "E2105" "$work/corrupt.err"
grep -q "digest" "$work/corrupt.err"

: >"$work/empty-artifact"
if tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 --sequence 2 --output "$work/empty-stage" \
    --artifact "$target=$work/empty-artifact" >"$work/empty.out" 2>"$work/empty.err"; then
    echo "empty publisher input was accepted" >&2
    exit 1
fi
grep -q "empty" "$work/empty.err"

truncate -s $((512 * 1024 * 1024 + 1)) "$work/oversize-artifact"
if tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 --sequence 3 --output "$work/oversize-stage" \
    --artifact "$target=$work/oversize-artifact" >"$work/oversize.out" 2>"$work/oversize.err"; then
    echo "oversize publisher input was accepted" >&2
    exit 1
fi
grep -q "bound" "$work/oversize.err"

mkdir "$work/real-input-parent"
cp "$work/jet-artifact" "$work/real-input-parent/jet-artifact"
ln -s "$work/real-input-parent" "$work/symlink-input-parent"
if tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 --sequence 4 --output "$work/symlink-input-stage" \
    --artifact "$target=$work/symlink-input-parent/jet-artifact" >"$work/symlink-input.out" 2>"$work/symlink-input.err"; then
    echo "symlinked publisher input ancestor was accepted" >&2
    exit 1
fi
grep -q "symlink" "$work/symlink-input.err"

mkdir "$work/real-output"
ln -s "$work/real-output" "$work/symlink-output"
if tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 --sequence 5 --output "$work/symlink-output" \
    --artifact "$target=$work/jet-artifact" >"$work/symlink-output.out" 2>"$work/symlink-output.err"; then
    echo "symlinked publisher output ancestor was accepted" >&2
    exit 1
fi
grep -q "symlink" "$work/symlink-output.err"

"$jet" --version >/dev/null
echo "toolchain self-update and publisher boundary proofs passed"
