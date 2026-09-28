#!/bin/sh
# Publish immutable index targets first, then the signed mutable manifest.
# Signers and uploader are explicit executables; this script never contains a
# private key and never runs on a user machine.
set -eu

target_root=
manifest=
manifest_request=
destination=
sign_index=
sign_manifest=
upload=

while [ "$#" -gt 0 ]; do
    case "$1" in
        --target-root) target_root=${2:?missing target root}; shift 2 ;;
        --manifest) manifest=${2:?missing manifest}; shift 2 ;;
        --manifest-request) manifest_request=${2:?missing manifest request}; shift 2 ;;
        --destination) destination=${2:?missing destination}; shift 2 ;;
        --sign-index) sign_index=${2:?missing index signer}; shift 2 ;;
        --sign-manifest) sign_manifest=${2:?missing manifest signer}; shift 2 ;;
        --upload) upload=${2:?missing uploader}; shift 2 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

for required in target_root manifest destination sign_index sign_manifest upload; do
    eval "value=\${$required}"
    [ -n "$value" ] || { echo "missing --$required" >&2; exit 2; }
done

manifest_request=${manifest_request:-"$manifest.sig.request"}
[ -f "$manifest" ] || { echo "manifest is missing" >&2; exit 1; }
[ -f "$manifest_request" ] || {
    echo "manifest signing request is missing; caller must provide the domain-prefixed bytes" >&2
    exit 1
}

sign_immutable() {
    signer=$1
    request=$2
    output=$3
    temporary="$output.partial.$$"
    if ! "$signer" "$request" "$temporary"; then
        rm -f "$temporary"
        return 1
    fi
    if [ -L "$temporary" ] || [ ! -f "$temporary" ]; then
        echo "signer did not create a regular file: $temporary" >&2
        rm -f "$temporary"
        return 1
    fi
    if [ -L "$output" ] || { [ -e "$output" ] && [ ! -f "$output" ]; }; then
        echo "signature output is not a regular file: $output" >&2
        rm -f "$temporary"
        exit 1
    fi
    if [ -e "$output" ]; then
        cmp -s "$temporary" "$output" || {
            echo "immutable signature changed: $output" >&2
            rm -f "$temporary"
            exit 1
        }
        rm -f "$temporary"
    else
        mv "$temporary" "$output"
    fi
}

find "$target_root" -type f -name '*.json.zst' -print | sort | while IFS= read -r target; do
    request="$target.sig.request"
    signature="$target.sig.json"
    [ -f "$request" ] || { echo "missing signing request: $request" >&2; exit 1; }
    sign_immutable "$sign_index" "$request" "$signature"
    relative=${target#"$target_root"/}
    "$upload" "$target" "$destination/$relative"
    "$upload" "$signature" "$destination/$relative.sig.json"
done

manifest_signature="$manifest.sig.json"
sign_immutable "$sign_manifest" "$manifest_request" "$manifest_signature"
"$upload" "$manifest" "$destination/manifest.json"
"$upload" "$manifest_signature" "$destination/manifest.json.sig.json"
echo "published immutable targets, then manifest"
