# Native Nix evaluator differential report

`differential-report.mjs` compares two completed evaluations. It does not run
Nix. It does not read `/nix/store`. It only consumes a Nix oracle and a Jet
result produced by separate, trusted runners.

Run the self-check:

```text
node tools/jet-nix-eval/differential-report.mjs --self-test
```

Run one pinned revision and system:

```text
node tools/jet-nix-eval/differential-report.mjs \
  --nix nix-oracle.json \
  --jet jet-evaluation.json \
  --output differential-report.json
```

Both input files use this shape:

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
    "drvPath": "/nix/store/...drv",
    "outputs": [{"name": "out", "storePath": "/nix/store/..."}],
    "directReferences": ["/nix/store/..."],
    "closureDigest": "sha256:..."
  }]
}
```

`status: "unsupported"` or `status: "error"` may replace the identity
fields on a Jet row. The report classifies missing rows, unsupported results,
Nix errors, Jet errors, derivation mismatches, output mismatches, graph
mismatches, closure mismatches, and missing graph/closure identity. A row is a
match only when derivation path, version, named outputs, output paths, direct
references, and closure digest all match.

The existing `tools/jetpack-nix-index/oracle.nix` is a producer for the Nix
side. A future Jet runner must emit the same record shape after it evaluates
the pinned nixpkgs source graph. An empty input reports `not-measured`; it never
creates a passing zero-coverage claim.

