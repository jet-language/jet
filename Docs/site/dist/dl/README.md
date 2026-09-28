# Jet toolchain channel

This directory is the static publication root for `dl.jet-lang.dev`. The
channel protocol is defined by
[`toolchain-channel-v1.schema.json`](toolchain-channel-v1.schema.json) and
consumed by [`ToolchainUpdate.rs`](../../../../crates/jetpack/src/ToolchainUpdate.rs).

## Manifest and artifact contract

A v1 manifest contains `schema`, `channel`, `version`, positive monotonic
`sequence`, positive `published_at` and `expires_at` timestamps, a
`min_version` client floor, and one to 64 artifacts. Each artifact names a
platform target and versioned path, a lowercase SHA-256 digest, a size from 1
through 512 MiB, and its detached signature path. The client verifies the
manifest signature, artifact signature, digest, size, freshness, and client
floor before accepting an update. The default endpoint is
`https://dl.jet-lang.dev` and the default channel is `stable`; local staging
and endpoint overrides are separate configuration paths.

The runtime bounds are a 1 MiB manifest, 16 KiB signature sidecars, 512 MiB per
artifact, 64 artifacts, and a 30-day maximum manifest age. Signed installation
stages beside the current executable and activates through the platform's
safe handoff rather than replacing a running file in place.

## Stage a channel

[`stage-toolchain.mjs`](../../../../Tools/jetpack-infra/stage-toolchain.mjs)
creates immutable unsigned files and domain-prefixed signing requests. Private
keys stay with the external release signer:

```sh
node Tools/jetpack-infra/stage-toolchain.mjs \
  --version <version> \
  --sequence <n> \
  --output <dir> \
  [--channel <name>] \
  [--min-version <version>] \
  [--published-at <unix-seconds>] \
  [--expires-at <unix-seconds>] \
  --artifact <target>=<file> ...
```

Defaults are channel `stable`, minimum version `1.0.0`, and a 30-day
freshness window. The command limits the manifest to 64 artifacts and each
artifact to 512 MiB, rejects symlinked ancestors, and refuses mutable changes
to an existing publication file. It writes
`v1/<channel>/manifest.json`, its `.sig.request`, versioned artifact paths, and
artifact `.sig.request` files. The manifest request is prefixed with
`jet-toolchain-channel-v1\n`; an artifact request is prefixed with
`jet-toolchain-artifact-v1\n`.

The release workflow supplies signatures and hosting separately. This staging
root does not create a signing key or deploy an unsigned file as a release.

## Local unsigned fixture

[`fixtures/unsigned/`](fixtures/unsigned/) exercises the explicit
`--allow-unofficial` local-source path. Its manifest and artifacts intentionally
have no `.sig.json` sidecars. The client rejects that tree by default and never
accepts it through the default `https://dl.jet-lang.dev` endpoint. The fixture
contains entries for both x86_64 and aarch64 Linux targets and is not a
production release.
