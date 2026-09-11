# Index and cache publication

The official endpoints use separate subdomains:

~~~text
https://index.jet-lang.dev/     signed nixpkgs index
https://cache.jet-lang.dev/     signed Jetpack/Hangar cache objects
~~~

The files are static. A publisher completes one immutable generation, then
atomically advances a local current pointer. A static host serves the
current directory as its document root. No dynamic service is required.

## Index contract

The existing jetpack-nix-index producer owns the bytes and signature
requests. The served paths are:

~~~text
/v1/<channel>/manifest.json
/v1/<channel>/manifest.json.sig.json
/index-v1/<revision>/<system>/<sha256>.json.zst
/index-v1/<revision>/<system>/<sha256>.json.zst.sig.json
~~~

The producer must sign target URLs against the public document root:

~~~text
--endpoint https://index.jet-lang.dev
~~~

manifest.json and its sidecar use application/json. Compressed index
targets use application/zstd; target signature sidecars use application/json.
The manifest and target signatures use existing NixIndex domains and Ed25519
sidecar shape. The target digest is the SHA-256 of compressed bytes. Targets
are immutable and content-addressed. A manifest is signed only after every
target and target sidecar is present.

The existing client reads these host-owned knobs:

~~~text
<JETPACK_ROOT>/config/nix-index-v1.endpoint
<JETPACK_ROOT>/trust/nix-index-v1.ed25519.pub
~~~

For the official signed tier, the endpoint file contains:

~~~text
https://index.jet-lang.dev
~~~

The endpoint and pinned key must be installed together. The client has no
implicit official endpoint; it activates this tier only from the signed
configuration pair. Production use requires HTTPS. Loopback HTTP remains
available for local development. The public key uses key-id:base64-public-key;
private signing keys stay with the offline signer.

## Cache contract

The prepared Hangar cache tree contains:

~~~text
/nar/<output-hash>.nar
/<output-hash>-<entry-id>.narinfo
/trust/<output-hash>-<entry-id>.receipt
~~~

NAR files use application/octet-stream. narinfo and receipt files use
text/plain. The narinfo is signed by the existing HMAC cache-role key and
the receipt uses the existing jet-cache-receipt-v1 protocol. Role key,
builder allowlist, and witness state remain in the host Jetpack root. They are
not copied into the site tree.

This differs from the upstream Nix cache client, whose existing paired
configuration is:

~~~text
<JETPACK_ROOT>/config/nix-cache-v1.endpoint
<JETPACK_ROOT>/trust/nix-cache-v1.ed25519.pub
~~~

Its normal layout is nix-cache-info, <store-hash>.narinfo, and nar/. The
endpoint and public key must be configured together.

## Stage

Generate and sign index targets with the existing producer. Then stage signed
index files and eligible Hangar entries into separate local trees:

~~~sh
tools/jetpack-infra/stage-index-cache.sh \
  --index-root <index-producer-output> \
  --hangar-root <jetpack-root> \
  --output <publication-staging> \
  --channel nixpkgs-unstable \
  --role public
~~~

The output contains index/ for index.jet-lang.dev and cache/ for
cache.jet-lang.dev. The stage command validates the channel as a bounded
lowercase ASCII identifier, requires the manifest and every target signature,
rejects symlinked, hard-linked, special, changed, and over-bound inputs, and
does not copy signing requests.

Reads and writes use held POSIX directory descriptors with O_NOFOLLOW.
Files are read in bounded chunks and immutable files are created through a
temporary file plus exclusive link. The command invokes
jetpack hangar cache stage, reusing Hangar provenance checks, canonical NAR
writer, cache receipt, and signing machinery. Cache staging uses a private
local scratch directory, then copies through the held output descriptor. It
never contacts a remote endpoint.

The local contract proof needs no Jetpack build or remote service:

~~~sh
tools/jetpack-infra/test-index-cache.sh
~~~

The official tier is explicit. Install the index endpoint and pinned trust key
together only after production signature verification works end to end. An
unsigned or unverified fetch is rejected; it never falls back to the local
keyless catalog. The local unofficial keyless path remains unchanged.

## Publish a local static root

Publish the prepared trees into two local host roots:

~~~sh
tools/jetpack-infra/stage-index-cache.sh publish \
  --staging <publication-staging> \
  --channel nixpkgs-unstable \
  --index-destination <index-host-root> \
  --cache-destination <cache-host-root>
~~~

Each destination has this shape:

~~~text
<host-root>/
  generations/
    g<manifest-generation>-<manifest-sha256>/
      ...existing endpoint layout...
  current -> generations/g<manifest-generation>-<manifest-sha256>
~~~

The publisher holds each destination root, creates the complete generation,
fsyncs files and directories, and atomically replaces the current symlink.
Existing generation bytes are immutable. Repeating an identical publication
is safe; a lower generation or equal-generation fork is refused. Older
generations stay available for rollback and audit. No mutable-manifest sync
step remains.

The index and cache roots advance independently. A failed second-root update
does not rewrite or remove the first root's generation history; rerun the
same immutable generation after fixing the failed root.

Configure the static host to serve <index-host-root>/current at
index.jet-lang.dev and <cache-host-root>/current at
cache.jet-lang.dev. An object store or web host must provide the equivalent
atomic alias operation. This lane writes only local filesystem roots; DNS,
TLS, object-store credentials, production key custody, and deployment approval
remain external.
