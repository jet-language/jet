# Jet public trust root

This directory is the publication staging point for public trust metadata. It
contains no private keys. The checked-in manifest is deliberately marked
`awaiting-key-ceremony`; no test or dogfood key is an official Jet trust root.

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
of the new root by the old root, and a reviewed replacement manifest.

Real DNS, TLS, production signing, and hosting remain pending. The one-command
publish step, after those gates are complete, is:

```sh
rsync -a --checksum --ignore-existing site/dist/keys/ "$HOST_ROOT/keys/"
```
