# Index and cache publication

This page defines the static publication trees consumed by Jetpack. The
producer and verifier own the index format in
[`NixIndex.rs`](../../../crates/jetpack/src/NixIndex.rs); the stage and publish
implementation is
[`stage-index-cache.mjs`](../../../tools/jetpack-infra/stage-index-cache.mjs),
and its local proof is
[`test-index-cache.sh`](../../../tools/jetpack-infra/test-index-cache.sh).
The two services have separate origins:

```text
https://index.jet-lang.dev/     signed nixpkgs index
https://cache.jet-lang.dev/     signed Jetpack/Hangar cache objects
```

Both services are static. A publisher writes one immutable generation and then
advances a local `current` pointer atomically. A static host serves that
`current` directory as its document root; no dynamic service is required.

## Signed index

The existing `jetpack-nix-index` producer owns compressed index bytes and
signature requests. The public layout is:

```text
/v1/<channel>/manifest.json
/v1/<channel>/manifest.json.sig.json
/index-v1/<revision>/<system>/<sha256>.json.zst
/index-v1/<revision>/<system>/<sha256>.json.zst.sig.json
```

The producer signs target URLs against the public document root:

```text
--endpoint https://index.jet-lang.dev
```

The manifest and its sidecar use `application/json`. Compressed targets use
`application/zstd`; target sidecars use `application/json`. Manifest and target
signatures use the existing NixIndex domain prefixes and Ed25519 sidecar shape.
The target digest is SHA-256 of the compressed bytes. Targets are immutable and
content-addressed. The producer signs a manifest only after every target and
its sidecar is present.

The signed client reads these host-owned files:

```text
<JETPACK_ROOT>/config/nix-index-v1.endpoint
<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub
```

For the official tier, the endpoint file contains:

```text
https://index.jet-lang.dev
```

Install the endpoint and pinned key together. The client has no implicit
official endpoint: it enables the signed tier only from that configuration
pair. Production use requires HTTPS; loopback HTTP remains available for local
development. The public key is one `key-id:base64-public-key` line, and the
private signing key remains with the offline signer.

## Hangar cache

The staged cache tree contains these public files:

```text
/nar/<output-hash>.nar
/<output-hash>-<entry-id>.narinfo
/trust/<output-hash>-<entry-id>.receipt
```

NAR files use `application/octet-stream`. `narinfo` and receipt files use
`text/plain`. The `narinfo` is signed by the cache-role HMAC key, and the
receipt uses the `jet-cache-receipt-v1` protocol. The role key, builder
allowlist, and witness state remain in the host Jetpack root; they are never
copied into the site tree.

The upstream Nix cache client has a separate paired configuration:

```text
<JETPACK_ROOT>/config/nix-cache-v1.endpoint
<JETPACK_ROOT>/trust/nix-cache-v1.ed25519.pub
```

Its normal layout is `nix-cache-info`, `<store-hash>.narinfo`, and `nar/`.
That endpoint and public key must also be configured together.

## Stage local trees

Generate and sign index targets with the existing producer. Then stage signed
index files and eligible Hangar entries into separate local trees:

```sh
tools/jetpack-infra/stage-index-cache.sh \
  --index-root <index-producer-output> \
  --hangar-root <jetpack-root> \
  --output <publication-staging> \
  --channel nixpkgs-unstable \
  --role public
```

The output has `index/` for `index.jet-lang.dev` and `cache/` for
`cache.jet-lang.dev`. The stage command validates a bounded lowercase ASCII
channel, requires the manifest and every target signature, rejects symlinked,
hard-linked, special, changed, and over-bound inputs, and does not copy signing
requests.

The implementation holds POSIX directory descriptors with `O_NOFOLLOW`, reads
files in bounded chunks, and creates immutable files through a temporary file
and exclusive link. Cache staging invokes `jetpack hangar cache stage`, so it
reuses Hangar provenance checks, the canonical NAR writer, cache receipts, and
signing machinery. It uses private local scratch space and never contacts a
remote endpoint.

Run the local contract proof without a remote service:

```sh
tools/jetpack-infra/test-index-cache.sh
```

The official tier is explicit. Install the endpoint and pinned key together
only after production signature verification works end to end. An unsigned or
unverified fetch is rejected; it never falls back to the local keyless
catalog. The local unofficial keyless path remains separate.

## Publish immutable generations

Publish the staged trees into two local host roots:

```sh
tools/jetpack-infra/stage-index-cache.sh publish \
  --staging <publication-staging> \
  --channel nixpkgs-unstable \
  --index-destination <index-host-root> \
  --cache-destination <cache-host-root>
```

Each destination has this shape:

```text
<host-root>/
  generations/
    g<manifest-generation>-<manifest-sha256>/
      ...existing endpoint layout...
  current -> generations/g<manifest-generation>-<manifest-sha256>
```

The publisher holds each destination root, creates the complete generation,
fsyncs files and directories, and atomically replaces `current`. Existing
generation bytes are immutable. Repeating an identical publication is safe; a
lower generation or equal-generation fork is refused. Older generations remain
available for rollback and audit.

The index and cache roots advance independently. A failed second-root update
does not rewrite or remove the first root's generation history; rerun the same
immutable generation after fixing the failed root.

Configure a static host to serve `<index-host-root>/current` at
`index.jet-lang.dev` and `<cache-host-root>/current` at `cache.jet-lang.dev`.
An object store or web host must provide the equivalent atomic alias operation.
This workflow writes local filesystem roots only; DNS, TLS, object-store
credentials, production key custody, and deployment approval remain external.
