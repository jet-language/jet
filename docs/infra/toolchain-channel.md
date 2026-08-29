# Toolchain channel publication

The prepared toolchain endpoint is dl.jet-lang.dev. V1 uses a signed stable
channel manifest and immutable, versioned artifacts.

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
{"schema":1,"channel":"stable","version":"1.0.0","artifacts":[{"target":"x86_64-unknown-linux-gnu","path":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu","sha256":"<64 lowercase hex>","size":123,"signature":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu.sig.json"}]}
~~~

The artifact list is sorted by target. The manifest is signed over
jet-toolchain-channel-v1 followed by a newline and the exact manifest bytes.
Each raw artifact is signed over jet-toolchain-artifact-v1 followed by a
newline and the exact artifact bytes. Both sidecars use the existing strict
shape:

~~~json
{"schema":1,"key_id":"<key-id>","algorithm":"ed25519","signature":"<base64>"}
~~~

The public verifier reads trust/toolchain-v1.ed25519.pub from the Jetpack root.
It checks the manifest before selecting the host target, then checks the
artifact size, SHA-256, and signature. It supports file:// for a local staging
tree and HTTPS for the real endpoint. Plain HTTP is limited to loopback.

## Prepare

Stage one or more release artifacts. This writes only artifacts and external
signing requests; it never reads a private key:

~~~sh
tools/jetpack-infra/stage-toolchain.sh \
  --version <version> \
  --channel stable \
  --output <publication-staging> \
  --artifact x86_64-unknown-linux-gnu=<path-to-jet-binary> \
  --artifact aarch64-unknown-linux-gnu=<path-to-jet-binary>
~~~

The release signer must create the matching .sig.json files from the
domain-prefixed .sig.request bytes. jet self update --dry-run verifies a local
tree without writing or replacing the current executable. --apply stages
beside the current executable and swaps only after verification.

## Configure the client

The endpoint precedence is explicit jet self update --endpoint <url>, then
JET_TOOLCHAIN_ENDPOINT, then the host-owned file
JETPACK_ROOT/config/toolchain-v1.endpoint, then the default
https://dl.jet-lang.dev. The trust key defaults to
JETPACK_ROOT/trust/toolchain-v1.ed25519.pub; --trust-key is an explicit test or
operator override.

## Publish when hosting exists

After the production signer has created all sidecars and the owner has
approved DNS, TLS, and key custody, the one-command file publication step is:

~~~sh
rsync -a --checksum --ignore-existing <publication-staging>/ "$HOST_ROOT/"
~~~

HOST_ROOT, production signing, DNS, TLS, and release approval are not
available in this lane. Nothing here publishes to the internet.
