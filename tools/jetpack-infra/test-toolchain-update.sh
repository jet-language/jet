#!/bin/sh
# End-to-end local-file proof for the self-update dry-run.
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
tools/jetpack-infra/stage-toolchain.sh \
    --version 1.2.3 \
    --channel stable \
    --output "$stage" \
    --artifact "$target=$work/jet-artifact"

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
sign_request "$manifest.sig.request" "$manifest.sig.json"
sign_request "$artifact.sig.request" "$artifact.sig.json"

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
echo "toolchain self-update dry-run passed"
