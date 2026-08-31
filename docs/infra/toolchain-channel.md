# Toolchain channel publication

The prepared toolchain endpoint is dl.jet-lang.dev. V1 uses a signed stable
channel manifest and immutable, versioned artifacts.

The served contract is described by
`site/dist/dl/toolchain-channel-v1.schema.json`. The checked-in tree also has
an unsigned local-only fixture under `site/dist/dl/fixtures/`; it is not a
production release and has no signing ceremony or deployment claim.

## Contract

The served layout is:

~~~text
/v1/<channel>/manifest.json
/v1/<channel>/manifest.json.sig.json
/v1/<channel>/<version>/jet-<version>-<target>
/v1/<channel>/<version>/jet-<version>-<target>.sig.json
~~~

The manifest and signature sidecars use application/json. Toolchain binaries
use application/octet-stream. The manifest has this canonical shape:

~~~json
{"schema":1,"channel":"stable","version":"1.0.0","sequence":42,"published_at":1788037412,"expires_at":1790629412,"min_version":"1.0.0","artifacts":[{"target":"x86_64-unknown-linux-gnu","path":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu","sha256":"<64 lowercase hex>","size":123,"signature":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu.sig.json"}]}
~~~

The artifact list is sorted by target. `sequence` is a positive release
counter and must increase for each channel/platform state. `published_at` and
`expires_at` are Unix seconds; clients reject future, stale, and expired
manifests. `min_version` is the minimum client version that may consume the
manifest. All these fields are covered by the manifest signature. The
canonical manifest ends with one newline. It is signed over
jet-toolchain-channel-v1 followed by a newline and the exact manifest bytes.
Each raw artifact is signed over jet-toolchain-artifact-v1 followed by a
newline and the exact artifact bytes. Both sidecars use the existing strict
shape:

~~~json
{"schema":1,"key_id":"<key-id>","algorithm":"ed25519","signature":"<base64>"}
~~~

The public verifier reads trust/toolchain-v1.ed25519.pub from the Jetpack root.
It checks the manifest before selecting the host target, then checks the
artifact size, SHA-256, and signature. It records the accepted sequence in a
per-channel/platform state file only after activation; an equal or older
sequence is a replay and is refused. It refuses a candidate below the running
version. There is no downgrade override in the ratified update authority.
It supports file:// for a local staging tree and HTTPS for the real endpoint.
Endpoint authorities are parsed structurally: userinfo, percent-encoded
authority text, malformed ports, and unbracketed IPv6 are rejected. Plain HTTP
is limited to a structurally parsed loopback host.
For an HTTPS endpoint, every redirect must keep HTTPS and the configured host
and port. Cross-origin redirects, HTTPS-to-HTTP downgrades, file redirects,
and redirects that use another authority are refused at the redirect hop.
Absolute or root-relative redirects on the same origin remain allowed, within
the bounded redirect count.
An unsigned tree is never accepted by default. `--allow-unofficial` is an
explicit local-only opt-in; it requires a `file://` endpoint, still requires
the manifest's size and SHA-256, and reports `unofficial-keyless` provenance.

## Prepare

Stage one or more release artifacts. This writes only artifacts and external
signing requests; it never reads a private key:

~~~sh
tools/jetpack-infra/stage-toolchain.sh \
  --version <version> \
  --sequence <release-counter> \
  --channel stable \
  --min-version 1.0.0 \
  --output <publication-staging> \
  --artifact x86_64-unknown-linux-gnu=<path-to-jet-binary> \
  --artifact aarch64-unknown-linux-gnu=<path-to-jet-binary>
~~~

The publisher rejects empty or over-512-MiB artifacts, limits the manifest to
64 artifacts and 1 MiB, and refuses symlinked inputs, output directories, and
publication ancestors. The release signer must create the matching .sig.json
files from the domain-prefixed .sig.request bytes.

`jet self update --dry-run` verifies a local tree without writing or replacing
the current executable. `--apply` accepts only the exact host target. Unix
stages beside the current executable, health-checks the staged image, creates a
rollback hard link, atomically replaces the path, fsyncs its parent, commits
the monotonic state, and restores the old image if a post-activation step
fails. Windows stages a new image, a rollback copy, and a separate helper
image; the caller exits, the helper waits for the old image lock to clear,
replaces the path, health-checks it, commits state, or restores the rollback.

For a deliberately unsigned local fixture, pass both the local endpoint and
the explicit override:

~~~sh
jet self update \
  --endpoint file:///absolute/path/to/site/dist/dl/fixtures/unsigned \
  --channel stable \
  --platform x86_64-unknown-linux-gnu \
  --allow-unofficial \
  --dry-run
~~~

## Configure the client

The endpoint precedence is explicit jet self update --endpoint <url>, then
JET_TOOLCHAIN_ENDPOINT, then the host-owned file
JETPACK_ROOT/config/toolchain-v1.endpoint, then the default
https://dl.jet-lang.dev. The trust key defaults to
JETPACK_ROOT/trust/toolchain-v1.ed25519.pub; --trust-key is an explicit test or
operator override. Monotonic state is under
`JETPACK_ROOT/config/toolchain-v1/`; `--allow-unofficial` never changes the
default endpoint or the default signed path.

Every rejected manifest, signature, digest, or install operation is reported
through registered diagnostic `E2105`; the existing executable is left in
place when verification or activation fails.

## Publish when hosting exists

After the production signer has created all sidecars and the owner has
approved DNS, TLS, and key custody, the one-command file publication step is:

~~~sh
rsync -a --checksum --ignore-existing <publication-staging>/ "$HOST_ROOT/"
~~~

The release workflow only uploads unsigned artifacts and signing requests as
reviewable GitHub release assets. It does not invent a key or deploy to
`dl.jet-lang.dev`. HOST_ROOT, production signing, DNS, TLS, and release
approval are not available in this lane. Nothing here publishes to the
internet.
