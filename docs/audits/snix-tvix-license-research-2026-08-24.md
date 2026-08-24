# Snix/Tvix reuse and licence source note

Date: 2026-08-24. Scope: primary upstream sources only.

## Finding

Snix is the project fork and new chapter of Tvix, not a separate permissive
implementation. The Snix announcement describes the 2025 fork. Both projects
state the same licence split: implementation code is GPL-3.0; protocol buffer
definitions are the stated MIT exception; direct linking or embedding the
evaluator falls under GPL-3.0.

Jet should therefore keep `snix-eval`, `tvix-eval`, `snix-glue`, `nix-compat`,
store code, and copied implementation tests out of the shipped native
evaluator until the owner and legal review approve a separate GPL posture.
Use Snix/Tvix as external conformance oracles and design references.

The refs checked on 2026-08-24 are moving `canon` branches, so any future use
must pin an immutable commit:

- Snix: `canon` at `6e990352dd1fe25248a9b47ca61e5b90cc829faf`.
- Tvix read-only mirror: `canon` at `92e60f242b880f641e3346d42d3f4f4334ac3ee2`.

## What Jet can reuse

| Material | Jet posture | Source basis |
| --- | --- | --- |
| Evaluator, glue, store, and `nix-compat` Rust | Read for design; run as a separately managed oracle. Do not link, embed, or copy into the current Jet product without an owner/legal decision. | Snix says `snix-eval` is a bytecode VM with pluggable builtins and `snix-glue` adds store/build builtins; its repository licence statement covers direct use. Tvix states the same rule. |
| Protocol schemas | Reuse only exact files with a permissive file-level grant. Preserve copyright, SPDX, and licence text. | `snix/castore/protos/rpc_blobstore.proto` and `rpc_directory.proto` declare MIT. `castore.proto` declares `OSL-3.0 OR MIT OR Apache-2.0`; select one allowed option and keep its notices. |
| Test method and public docs | Use as reading material or an external test input. Do not copy source tests or generated code by assuming the protocol exception covers them. | Snix and Tvix document `notyetpassing` language suites and an Nix differential-oracle pattern. |

The low dependency count of `nix-compat` is not a licence grant. Snix calls it
easy to include in non-Snix projects, but the repository-wide licence statement
still places Snix crates under GPL-3.0 unless a narrower file-level grant says
otherwise.

## Licence obligations

- GPL-3.0 implementation code: a distributed copy must retain copyright,
  licence, and warranty notices and include the licence. A modified source
  work must mark the change and licence the whole work under GPL-3.0. A
  distributed object form needs the corresponding source required by GPL-3.0.
- MIT protocol files: retain the copyright and permission notices. The
  `castore/protos/LICENSE` file is MIT, but the individual SPDX header remains
  the controlling file-level evidence for the files listed above.
- Generated Rust, fixtures, and helper code need their own file-level review.
  Do not infer their licence from a `.proto` input.

This is an engineering posture, not a legal opinion. Whether a CI-only,
separately installed Snix process creates any distribution obligation depends
on how Jet obtains and ships it. Legal review is still required before Jet
distributes Snix or any GPL-derived code.

## Derived next plan

1. Ratify: no GPL Snix/Tvix implementation in Jet product paths; external
   oracle and design reference only.
2. Pin every oracle run to immutable Snix/Tvix and Nix revisions. Keep a
   source/licence manifest for every copied schema or test input.
3. Build the whole-nixpkgs differential corpus around the oracle pattern, but
   keep copied tests and generated bindings outside the product until cleared.
4. Remove evaluator limitation claims only after Jet matches Nix derivation,
   output, graph, and closure identity for the measured corpus.

## Remaining uncertainty

- The upstream README gives a broad protocol exception, while current Snix
  files use both MIT-only and multi-licence SPDX expressions. File-level
  review is required for every candidate file.
- Snix has no stable API promise and no full-featured Nix replacement. Its
  evaluator README targets compatibility on a Nix 2.3 foundation and records
  expected failures. Tvix's evaluator README records the same Nix 2.3 basis.
- No source reviewed here grants Jet permission to copy evaluator code,
  generated bindings, or language-test fixtures under Jet's product licence.

## Primary sources

All Snix repository links below use the immutable commit recorded above unless
the link is to project documentation.

- Snix fork announcement: [`snix.dev/blog/announcing-snix`](https://snix.dev/blog/announcing-snix/).
- Snix repository commit and licence statement: [`snix/snix` commit](https://git.snix.dev/snix/snix/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf), [`README.md`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/README.md), [`LICENSE`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/LICENSE).
- Snix evaluator scope: [`snix/eval/README.md`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/eval/README.md), [`snix.dev/about`](https://snix.dev/about/), [`component overview`](https://snix.dev/docs/components/overview/), [`use as a library`](https://snix.dev/docs/guides/use-as-library/).
- Snix protocol licence and files: [`snix/castore/protos/LICENSE`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/LICENSE), [`castore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/castore.proto), [`rpc_blobstore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_blobstore.proto), [`rpc_directory.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_directory.proto).
- Tvix read-only mirror at the recorded commit: [`README.md`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/README.md), [`LICENSE`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/LICENSE), [`eval/README.md`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/eval/README.md).

Jet's existing product rule agrees with this posture: `docs/spec/syntax-decisions.md:5440-5441` says Jet ships no Tvix code and uses Nix/Tvix as development and CI differential oracles.
