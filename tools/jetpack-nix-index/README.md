# Jetpack Nix index producer

This tool turns staged nixpkgs evidence into immutable Jetpack index targets.
The binary consumes files; it does **not** invoke Nix, download data, compress
inputs, or hold a signing secret. An outer producer job supplies those steps.
A Jetpack client receives signed targets and does not need a Nix executable to
read them. The implementation is
[`jetpack-nix-index.rs`](../../crates/jetpack/src/bin/jetpack-nix-index.rs).

## Staged evidence

`generate` binds one channel release, revision, and system. Its required inputs
are:

- `--git-revision`: a file containing the requested 40-character lowercase
  revision;
- `--release-metadata`: JSON with `released_unix`, `timestamp`, or
  `release_time`;
- `--packages-json`: decompressed channel package metadata used for version
  cross-checks;
- `--store-paths`: newline-separated full store paths from the release;
- `--hydra-eval`: an evaluation whose revision matches the requested revision;
- `--hydra-build-dir`: Hydra build records for the selected system; and
- `--oracle`: records produced from the pinned, clean oracle expression.

The producer joins exact oracle records with the staged release and Hydra
paths. It does not infer an attribute path from a Hydra job or select a
derivation from a `.narinfo` `Deriver` field.

The off-device oracle is [`oracle.nix`](oracle.nix). It imports only the
staged `nixexprs` tree with an explicit system, empty config, and no overlays,
then enumerates `packages-info.nix` as records containing `attrpath`, `version`,
`drvPath`, named outputs, and `cache: false`. A separate caller may evaluate
[`differential.nix`](differential.nix) against clean pinned packages in sorted
batches of at most 256 attributes.

## Generate an index

```text
jetpack-nix-index generate \
  --channel nixpkgs-unstable \
  --system x86_64-linux \
  --revision <40-hex-revision> \
  --git-revision <staged/git-revision> \
  --release-metadata <staged/release.json> \
  --packages-json <staged/packages.json> \
  --store-paths <staged/store-paths> \
  --hydra-eval <staged/eval.json> \
  --hydra-build-dir <staged/builds> \
  --oracle <staged/oracle.json> \
  --output <publication-staging>
```

The immutable target is
`index-v1/<revision>/<system>/<sha256-of-compressed-bytes>.json.zst`. Regular
generation also writes a domain-prefixed `.sig.request`, a coverage report,
and a generation report. The signing request is input to an external signer;
the producer never sees its private key.

For an explicitly local, unofficial import, use `generate-local` with the same
staged inputs. It may additionally accept `--native-recipes` and
`--provenance`; its generation report marks the target `trust_tier` as
`local-unofficial` and does not emit an index signing request.

## Verify candidate records

`verify-differential` compares an already generated target with an oracle. It
performs no Nix evaluation and does not contact a store:

```text
jetpack-nix-index verify-differential \
  --candidate <target.json.zst> \
  --oracle <oracle.json> \
  --channel nixpkgs-unstable \
  --revision <40-hex-revision> \
  --system x86_64-linux \
  --report <run-report.json>
```

The command first binds the candidate identity to channel, revision, and
system, then compares the complete record set by attribute path, version,
`drvPath`, output-name set, and output paths. The report has schema `1` and
contains `channel`, `revision`, `system`, `records_compared`, `mismatches`, and
`status` (`passed` or `failed`). A mismatch writes the report and exits
non-zero.

## Build and publish a manifest

After every target has its signed `.sig.json` and generation report, scan the
staging root into a manifest:

```text
jetpack-nix-index manifest \
  --channel nixpkgs-unstable \
  --endpoint https://index.example.invalid/nixpkgs \
  --generation 42 \
  --issued-unix <publisher-time> \
  --expires-unix <issued-plus-seven-days> \
  --target-root <publication-staging> \
  --output <publication-staging>/manifest.json
```

Manifest generation verifies each compressed target's filename digest,
signature sidecar, and `released_unix` report, then writes the manifest and its
`.sig.request`. It marks the newest twelve revisions per system discoverable;
older immutable targets remain addressable by exact revision. Signature and
upload executables are external. [`publish.sh`](publish.sh) signs and uploads
immutable targets first, then the manifest, and never contains a private key.

The client-side cache and manifest acceptance rules are specified in
[`docs/spec/packaging/index-cache.md`](../../docs/spec/packaging/index-cache.md).
