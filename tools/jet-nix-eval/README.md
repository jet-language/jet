# Nix/Jet evaluator differential report

[`differential-report.mjs`](differential-report.mjs) compares two completed
evaluations: a Nix oracle and a Jet result. It consumes evidence only; it does
not start Nix, read `/nix/store`, grant process authority, or evaluate a
source graph.

## Run it

The built-in self-check exercises matching, unsupported, incomplete-identity,
revision validation, and empty-input behavior:

```sh
node tools/jet-nix-eval/differential-report.mjs --self-test
```

Compare pinned inputs:

```sh
node tools/jet-nix-eval/differential-report.mjs \
  --nix nix-oracle.json \
  --jet jet-evaluation.json \
  --output differential-report.json
```

`--output` creates a new file and refuses to overwrite an existing one. Without
it, the canonical JSON report is written to stdout. The process exits non-zero
unless every compared row matches.

## Input contract

Each input is an object with a non-empty system, exactly 40 lowercase
hexadecimal characters in `revision`, and a `records` array. `schema` is
optional, but when present it must be `1`. If `nixpkgs.revision` (or its
`rev` alias) is present, it must equal the top-level revision. Attribute paths
are non-empty string arrays and must be unique within each input.

```json
{
  "schema": 1,
  "revision": "40 lowercase hex characters",
  "system": "x86_64-linux",
  "nix": {"version": "2.34.8", "source_commit": "..."},
  "nixpkgs": {"revision": "...", "nar_hash": "..."},
  "records": [{
    "attrpath": ["ripgrep"],
    "version": "15.2.0",
    "drvPath": "/nix/store/example.drv",
    "outputs": [{"name": "out", "storePath": "/nix/store/example"}],
    "directReferences": ["/nix/store/reference"],
    "closureDigest": "sha256:..."
  }]
}
```

An output set can also be an object map (`{"out": "/nix/store/..."}`). The
reader accepts `storePath`/`store_path`, `drvPath`/`drv_path`, and the direct
reference aliases used by the producer. A record defaults to `status: "ok"`;
a non-`ok` record may omit identity fields and carry `errorClass` and `error`.

## Comparison and report

The tool first requires Nix and Jet to agree on revision and system. For each
Nix record it compares, in order, status, version, derivation path, output-name
set, output paths, direct references, and closure digest. Missing direct
references or closure digests are not treated as equal: they produce the
`missing_identity` class.

The report has schema `1`, kind
`jet-nix-evaluator-differential`, the pinned system and revision metadata, an
inventory digest, `records_total`, per-class `counts`, and sorted `rows`.
Possible row classes are:

- `matched`, `version_mismatch`, `drv_mismatch`,
  `output_set_mismatch`, and `output_path_mismatch`;
- `graph_mismatch`, `closure_mismatch`, `missing_identity`, and
  `missing_source`; and
- `unsupported`, `jet_error`, and `nix_error`.

A non-empty inventory is `pass` only when every row is `matched`. An empty
inventory is `not-measured`, never a passing zero-coverage result, and exits
non-zero. The existing
[`tools/jetpack-nix-index/oracle.nix`](../jetpack-nix-index/oracle.nix) can
produce the Nix-side record evidence; a separate evaluator must produce the
Jet-side file.
