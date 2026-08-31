# Jet public trust root

This directory is the publication staging point for public trust metadata. It
contains no private keys. The checked-in manifest is deliberately marked
`awaiting-key-ceremony`; no test or dogfood key is an official Jet trust root.

The stable index key address is
`https://keys.jet-lang.dev/nix-index-v1.ed25519.pub`.
This is the canonical public trust-root path under the owned `keys.jet-lang.dev`
domain; key rotation changes the recorded key, not this address.
When production trust is ready, that file must contain one
`key-id:base64-public-key` line. The manifest records its key id and `ed25519`
algorithm when the key is prepared; the signed index manifest records
`issued_unix` and `expires_unix` as the validity window.

Review that document, then install its value as the client's pinned
`<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub` file. The signed index URL is
configured separately in `<JETPACK_ROOT>/config/nix-index-v1.endpoint`; the
client never trusts a key returned by a host automatically.

The missing key file is intentional. This tree must not carry a test, dogfood,
or generated stand-in key while the owner-controlled ceremony is pending.

After the owner-controlled production key ceremony, prepare the complete set
with:

```sh
tools/jetpack-infra/stage-trust-root.sh \
  --index-key <public-index-key> \
  --cache-key <public-cache-key> \
  --toolchain-key <public-toolchain-key> \
  --bootstrap <jetpack-root>
```

The exporter accepts only `key-id:base64-public-key` files with a 32-byte
Ed25519 key. HMAC `TrustKey` secrets are host-only and must never enter this
directory. Rotation requires an offline threshold-root decision, verification
of the new root by the old root, a manual update of each verifier's local pin,
and a reviewed replacement manifest. A host-returned key is never an automatic
replacement for the local pin.

Real DNS, TLS, production signing, and hosting remain pending. The one-command
publish step, after those gates are complete, is:

```sh
rsync -a --checksum --ignore-existing site/dist/keys/ "$HOST_ROOT/keys/"
```
