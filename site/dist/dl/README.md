# Jet toolchain channel

This directory is the static publication root for `dl.jet-lang.dev`.

Production release files are intentionally absent until the owner-controlled
trust-root and signing ceremony from Tower #2199 is complete. The schema is
checked in for the publisher. It requires a positive monotonic `sequence`,
freshness/expiry timestamps, and a `min_version` client floor. The publisher
also rejects empty or over-512-MiB inputs and symlinked publication ancestors.
`fixtures/unsigned/` is local proof data only; it has no signature sidecars
and must not be published as a release. The release workflow emits unsigned
signing requests only; it does not deploy this directory or create a key.
