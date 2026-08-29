# Trust-root publication

The trust root anchors signed Jet index, cache, and toolchain metadata at
`keys.jet-lang.dev`. The publication tree contains public Ed25519 key files and
`trust-manifest.json`. It never contains an HMAC `TrustKey` secret or a signing
seed.

## Contract

The manifest uses schema `1`. Each key record names its role, algorithm, key
id, exact client path, and padded base64 public key. Client trust files use the
existing `key-id:base64-public-key` format:

```text
keys.jet-lang.dev/nix-index-v1.ed25519.pub
keys.jet-lang.dev/nix-cache-v1.ed25519.pub
keys.jet-lang.dev/toolchain-v1.ed25519.pub
```

`nix-index-v1.ed25519.pub` is consumed by the existing signed `NixIndex`
verifier. The cache and toolchain paths use the same strict public-key format;
the Hangar cache writer's HMAC role key remains a private host binding and is
never exported.

Until the production key ceremony supplies public files, the checked-in
manifest says `awaiting-key-ceremony` and has no keys. The existing
`repo-dogfood-index-v1` key and all `jet-test-*` keys are rejected by the
exporter and are not official roots.

## Prepare

Build the exporter, then provide public files from the offline key ceremony:

```sh
tools/jetpack-infra/stage-trust-root.sh \
  --index-key <public-index-key> \
  --cache-key <public-cache-key> \
  --toolchain-key <public-toolchain-key> \
  --bootstrap <jetpack-root>
```

The exporter is immutable: rerunning it with changed bytes fails. Rotation is
an offline threshold-root operation. Verify the replacement root with the old
root, update the bootstrap pin, prepare the new public files, and publish the
new manifest only after that review.

## Publish when hosting exists

The one-command publication step is:

```sh
rsync -a --checksum --ignore-existing site/dist/keys/ "$HOST_ROOT/keys/"
```

`HOST_ROOT`, DNS, TLS, production signer custody, and owner acceptance of the
key ceremony are not available in this lane. Nothing here publishes to the
internet.
