# Jet public trust root

This directory is the static publication staging point for public trust
metadata. It contains no private keys. The checked-in
[`trust-manifest.json`](trust-manifest.json) has status
`awaiting-key-ceremony`, with no root or keys; a test or dogfood key is never
an official Jet trust root.

The stable Nix index key address is
`https://keys.jet-lang.dev/nix-index-v1.ed25519.pub`. That address is stable
across key rotation. Once a production public key is prepared, the file at
that address contains one `key-id:base64-public-key` line. The manifest records
the key id and `ed25519` algorithm, and a signed index manifest records
`issued_unix` and `expires_unix` for its validity window.

Review the published key and install its value as the client's pinned:

```text
<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub
```

The index endpoint is configured separately in:

```text
<JETPACK_ROOT>/config/nix-index-v1.endpoint
```

A client never trusts a key returned by a host automatically. The exporter
accepts only `key-id:base64-public-key` files containing a 32-byte Ed25519 key.
HMAC `TrustKey` secrets are host-only and must not enter this directory.

## Stage trust metadata

After the owner-controlled production key ceremony, stage the complete public
set with:

```sh
Tools/jetpack-infra/stage-trust-root.sh \
  --index-key <public-index-key> \
  --cache-key <public-cache-key> \
  --toolchain-key <public-toolchain-key> \
  --bootstrap <jetpack-root>
```

Rotation requires an offline threshold-root decision, verification of the new
root by the old root, a manual update of each verifier's local pin, and a
reviewed replacement manifest. DNS, TLS, production signing, and hosting are
separate publication controls; this directory supplies public metadata and
signing inputs, not private signing authority.

The trust-root procedure is
[`Docs/spec/packaging/trust-root.md`](../../../spec/packaging/trust-root.md),
and the publication checks are exercised by
[`tests/jetpack_trust_root.rs`](../../../../tests/jetpack_trust_root.rs).
