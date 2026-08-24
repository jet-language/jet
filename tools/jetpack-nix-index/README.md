# jetpack nixpkgs index producer

This directory is build infrastructure. Nix is required on the producer host
only. A Jetpack user receives signed immutable index targets and never needs a
Nix executable.

## Inputs

Stage all inputs from one immutable channel release and one Hydra evaluation:

- `git-revision`: one 40-character lowercase nixpkgs revision.
- `release-metadata`: JSON containing `released_unix`, `timestamp`, or
  `release_time`.
- `packages-json`: decompressed channel `packages.json` metadata. It seeds
  package version cross-checks; it does not select paths.
- `store-paths`: newline-separated full `/nix/store/...` paths from the same
  release closure.
- `hydra-eval`: one Hydra evaluation JSON whose input revision matches
  `git-revision`.
- `hydra-build-dir`: Hydra build JSON records for the candidate evaluation.
- `oracle`: JSON emitted by `oracle.nix`, for one explicit system. Each record
  has `attrpath` as exact segments, `version`, `drvPath`, every named output,
  and `cache: true` only after the upstream cache verifier accepts every
  output's `.narinfo`.

The producer joins these inputs. It never splits a Hydra `job` to invent an
attrpath and never uses `.narinfo` `Deriver` as the selected derivation. The
oracle and a second fresh Nix evaluation supply the exact selected record.

## Commands

Generate one whole index:

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

The output target is
`index-v1/<revision>/<system>/<sha256-of-compressed-bytes>.json.zst`. Its
`.sig.request` contains the fixed index signing domain prefix plus canonical
uncompressed JSON. The producer does not hold a private key.

Differential verification consumes the generated target and the oracle after a
fresh off-device `nix build --no-link --json` batch:

```text
jetpack-nix-index verify-differential \
  --candidate <target.json.zst> \
  --oracle <fresh-nix-results.json> \
  --system x86_64-linux \
  --report <run-report.json>
```

The publication gate requires every candidate record to compare byte-for-byte:
attrpath, system, version, `drvPath`, the complete output-name set, and every
output path. Batches contain at most 256 attrs and use a fresh evaluator
process. Any mismatch aborts publication.

## Coverage and retention

One index is published per `(channel, revision, system)`. Coverage uses the
exact per-system `packages-info.nix` oracle inventory. Every inventory attr is
exactly once in `indexed` or `notIndexed`. Overlays, overrides, user Nix config,
custom flakes/package sets, local inputs, and impure evaluation are outside
the denominator. Whole-index refresh is used in v1; immutable targets remain
addressable permanently and the signed manifest retains every target while
marking the newest 12 per system discoverable.
