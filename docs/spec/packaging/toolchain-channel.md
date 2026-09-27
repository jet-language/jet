# Toolchain channel publication

This page defines the static toolchain channel consumed by `jet self update`.
The executable contract is the
[`toolchain-channel-v1` schema](../../../site/dist/dl/toolchain-channel-v1.schema.json),
the verifier is in
[`ToolchainUpdate.rs`](../../../crates/jetpack/src/ToolchainUpdate.rs), and the
staging and update checks are
[`stage-toolchain.mjs`](../../../tools/jetpack-infra/stage-toolchain.mjs) and
[`test-toolchain-update.sh`](../../../tools/jetpack-infra/test-toolchain-update.sh).
The production endpoint is `https://dl.jet-lang.dev`; a checked-in unsigned
tree is only a local fixture.

## Served layout

A channel is a static tree with this layout:

```text
/v1/<channel>/manifest.json
/v1/<channel>/manifest.json.sig.json
/v1/<channel>/<version>/jet-<version>-<target>
/v1/<channel>/<version>/jet-<version>-<target>.sig.json
```

Manifests and signature sidecars use `application/json`. Toolchain binaries
use `application/octet-stream`. The manifest has this canonical shape:

```json
{"schema":1,"channel":"stable","version":"1.0.0","sequence":42,"published_at":1788037412,"expires_at":1790629412,"min_version":"1.0.0","artifacts":[{"target":"x86_64-unknown-linux-gnu","path":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu","sha256":"<64 lowercase hex>","size":123,"signature":"v1/stable/1.0.0/jet-1.0.0-x86_64-unknown-linux-gnu.sig.json"}]}
```

The artifact list is sorted by target. `sequence` is a positive monotonic
counter for each channel and platform state. `published_at` and `expires_at`
are Unix seconds. A client rejects a future, stale, or expired manifest.
`min_version` is the minimum client version allowed to consume the manifest.
All fields are covered by the manifest signature, and canonical manifest bytes
end with one newline.

The manifest signature covers the domain prefix followed by a newline and the
exact manifest bytes:

```text
jet-toolchain-channel-v1
```

Each raw artifact is signed over the corresponding prefix and the exact
artifact bytes:

```text
jet-toolchain-artifact-v1
```

Both sidecars use this strict JSON shape:

```json
{"schema":1,"key_id":"<key-id>","algorithm":"ed25519","signature":"<base64>"}
```

The schema limits a manifest to 64 artifacts and 1 MiB. Each artifact is
non-empty and no larger than 512 MiB. The staging tool gives a manifest a
30-day freshness window by default and rejects an expiration at or before its
publication time.

## Verify and install

The verifier reads `trust/toolchain-v1.ed25519.pub` under the Jetpack root. It
checks the manifest before selecting a host target, then checks the selected
artifact's size, SHA-256 digest, and signature. It records the accepted
sequence in per-channel/platform state only after activation. An equal or
older sequence is a replay, and a candidate below the running version is a
downgrade; both are refused.

The client accepts `file://` for a local staging tree and HTTPS for a network
endpoint. It parses endpoint authorities structurally: userinfo,
percent-encoded authority text, malformed ports, and unbracketed IPv6 are
rejected. Plain HTTP is limited to a structurally parsed loopback host. For
HTTPS, every redirect must preserve HTTPS and the configured host and port.
Cross-origin redirects, HTTPS-to-HTTP downgrades, file redirects, and another
authority are refused at the redirect hop; same-origin absolute or
root-relative redirects remain allowed within the redirect limit.

Unsigned trees are rejected by default. `--allow-unofficial` is a local-only
exception: it requires a `file://` endpoint, still verifies manifest size and
artifact digest, and records `unofficial-keyless` provenance. It does not
weaken the default signed endpoint.

## Stage a release

Stage one or more artifacts. This command writes artifacts and external signing
requests; it never reads a private key:

```sh
tools/jetpack-infra/stage-toolchain.sh \
  --version <version> \
  --sequence <release-counter> \
  --channel stable \
  --min-version 1.0.0 \
  --output <publication-staging> \
  --artifact x86_64-unknown-linux-gnu=<path-to-jet-binary> \
  --artifact aarch64-unknown-linux-gnu=<path-to-jet-binary>
```

The release signer creates each matching `.sig.json` from the
`.sig.request` bytes and the domain prefix. A staging tree is not a signed
release until those sidecars exist and the verifier accepts them.

Verify a local tree without replacing the current executable:

```sh
jet self update \
  --endpoint file:///absolute/path/to/publication-staging \
  --channel stable \
  --platform x86_64-unknown-linux-gnu \
  --dry-run
```

`--apply` accepts only the exact host target. On Unix, the updater stages beside
the current executable, health-checks the staged image, creates a rollback hard
link, atomically replaces the executable, fsyncs its parent, and commits
monotonic state. If a post-activation step fails, it restores the old image.
On Windows, a helper waits for the old image lock to clear before replacing and
health-checking the executable; it commits state or restores the rollback.

For the deliberately unsigned checked-in fixture, pass both the local endpoint
and the explicit local-only override:

```sh
jet self update \
  --endpoint file:///absolute/path/to/site/dist/dl/fixtures/unsigned \
  --channel stable \
  --platform x86_64-unknown-linux-gnu \
  --allow-unofficial \
  --dry-run
```

## Configure the client

Endpoint selection is ordered from most explicit to least explicit:

1. `jet self update --endpoint <url>`;
2. `JET_TOOLCHAIN_ENDPOINT`;
3. `JETPACK_ROOT/config/toolchain-v1.endpoint`;
4. `https://dl.jet-lang.dev`.

The default trust file is
`JETPACK_ROOT/trust/toolchain-v1.ed25519.pub`. `--trust-key` is an explicit
test or operator override. Monotonic state lives under
`JETPACK_ROOT/config/toolchain-v1/`. `--allow-unofficial` does not change the
default endpoint or signed path.

Rejected manifests, signatures, digests, and installation operations are
reported through the registered diagnostic E2105. Verification and activation
leave the existing executable in place when they fail.

## Publish a static tree

After a signer has produced every sidecar and the hosting operator has
approved DNS, TLS, and key custody, copy the immutable publication tree into
the host root:

```sh
rsync -a --checksum --ignore-existing <publication-staging>/ "$HOST_ROOT/"
```

The staging workflow uploads unsigned artifacts and signing requests as
reviewable release assets. It does not create a key or deploy to
`dl.jet-lang.dev`; `HOST_ROOT`, production signing, DNS, TLS, and release
approval remain external to this repository.
