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

The index key document is the single-line file at
`https://keys.jet-lang.dev/nix-index-v1.ed25519.pub`. Its key id and algorithm
are recorded in `trust-manifest.json` when the key is prepared; the signed
index manifest supplies the validity window with `issued_unix` and
`expires_unix`. The raw key file stays in the exact shape the client already
reads, so no second key parser or trust mechanism is needed.

Until the production key ceremony supplies public files, the checked-in
manifest says `awaiting-key-ceremony` and has no keys. The existing
`repo-dogfood-index-v1` key and the `jet-test-index-v1` key are rejected by the
exporter and are not official roots.

This empty state is deliberate. No test or dogfood public key is copied into
the owned-domain publication tree as a stand-in for production trust.

## Client pinning

The client reads the pinned key from:

```text
<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub
```

The signed-index client uses the reviewed host-owned configuration key
`nix-index-v1.endpoint` at `<JETPACK_ROOT>/config/nix-index-v1.endpoint`. For
the official tier, its value is `https://index.jet-lang.dev`; install it
together with the pinned key file above. The client has no implicit official
endpoint while the production key ceremony is pending.

It does not fetch a key and then trust it. A key returned by a host is only an
update candidate for a separately reviewed manual change. The existing client
checks the configured key id, algorithm, and public bytes against every
manifest and index signature; a signature from a substituted key is refused as
`E1348`.

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
an offline threshold-root operation:

1. Generate the replacement key outside this repository and give it a new key
   id. Keep the old key and its retirement time recorded in the operator log.
2. Have the old threshold root verify the replacement root metadata. Update
   the local bootstrap pin and prepare the replacement public key file.
3. Publish the replacement key file and signed catalog metadata together as
   static files. Do not overwrite an immutable target with different bytes.
4. Update each verifier's local trust file through the reviewed release or
   bootstrap update. This is how a verifier learns the old key is retired; the
   client never switches keys because a host returned different bytes.
5. Let the old signed catalog's `expires_unix` pass, then remove the old key
   from the reviewed publication set. A verifier with the old pin fails closed
   until its manual pin update.

Rotation remains manual until an owner-ratified updater exists. This procedure
does not generate production key material or perform the key ceremony.

## Publish when hosting exists

The one-command publication step is:

```sh
rsync -a --checksum --ignore-existing site/dist/keys/ "$HOST_ROOT/keys/"
```

`HOST_ROOT`, DNS, TLS, production signer custody, and owner acceptance of the
key ceremony are not available in this lane. Nothing here publishes to the
internet.
