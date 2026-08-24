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

## Package metadata and technical separation

The pinned evaluator, glue, and `nix-compat` manifests declare package names but
do not declare a per-package `license` key. The project-level README is therefore
the controlling licence statement for these crates: Snix says all crates are
GPL-3.0, and Tvix says all code is GPL-3.0 except the protocol definitions.
Evidence: Snix [`eval/Cargo.toml:1-4`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/eval/Cargo.toml#L1-L4),
[`glue/Cargo.toml:1-4`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/glue/Cargo.toml#L1-L4),
and [`nix-compat/Cargo.toml:1-4`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/nix-compat/Cargo.toml#L1-L4);
Tvix [`eval/Cargo.toml:1-7`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/eval/Cargo.toml#L1-L7),
[`glue/Cargo.toml:1-5`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/glue/Cargo.toml#L1-L5),
and [`nix-compat/Cargo.toml:1-3`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/nix-compat/Cargo.toml#L1-L3).

The code is technically separable at the crate boundary. Snix `snix-eval` has
its own package and `snix-glue` separately adds `nix-compat`, `snix-eval`,
castore, store, and build crates ([`snix/glue/Cargo.toml:46-64`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/glue/Cargo.toml#L46-L64)).
Tvix has the same shape: `tvix-eval` is a separate package, while `tvix-glue`
adds `nix-compat`, `tvix-eval`, and `tvix-simstore` ([`glue/Cargo.toml:5-12`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/glue/Cargo.toml#L5-L12)).
Snix also states that evaluator tests use lightweight `MockIO` and store-aware
IO lives in glue ([`snix/eval/README.md:65-72`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/eval/README.md#L65-L72)).

Inference: Jet can run an immutable Snix/Tvix evaluator as an external CI
oracle, or reimplement the needed behavior behind a clean interface. The crate
boundary does not create a permissive reuse grant: both READMEs expressly say
that direct linking or embedding the evaluator is GPL-3.0. A copied or linked
implementation therefore stays outside the native Jet product unless the owner
approves a GPL posture and its compliance plan.

## Licence obligations

- GPL-3.0 implementation code: Snix `LICENSE:174-221` requires preserved
  notices and licence text for verbatim or modified source, and corresponding
  source for object code ([`snix/LICENSE:174-221`](https://git.snix.dev/snix/snix/raw/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/LICENSE#L174-L221)).
  A modified source work must mark the change and licence the whole work under
  GPL-3.0. This is a compliance summary, not legal advice.
- MIT protocol files: retain the copyright and permission notices. The
  `castore/protos/LICENSE` file is MIT, but the individual SPDX header remains
  the controlling file-level evidence for the files listed above.
- Generated Rust, fixtures, and helper code need their own file-level review.
  Do not infer their licence from a `.proto` input.

The dependency graph needs the same review. The pinned Snix and Tvix workspace
manifests enumerate many third-party dependencies ([`snix/Cargo.toml:95-230`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/Cargo.toml#L95-L230),
[`Cargo.toml:35-87`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/Cargo.toml#L35-L87)).
Those manifests do not grant rights to third-party crates. Any distributed
oracle or reused code must audit the exact locked dependency graph and preserve
each dependency's notices and licence terms. Cargo records package licences as
SPDX expressions in `Cargo.toml` ([Cargo manifest licence fields](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields));
the upstream README is not a substitute for that dependency audit.

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
- Pinned Snix package manifests: [`snix/eval/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/eval/Cargo.toml), [`snix/glue/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/glue/Cargo.toml), [`snix/nix-compat/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/nix-compat/Cargo.toml).
- Snix protocol licence and files: [`snix/castore/protos/LICENSE`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/LICENSE), [`castore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/castore.proto), [`rpc_blobstore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_blobstore.proto), [`rpc_directory.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_directory.proto).
- Tvix read-only mirror at the recorded commit: [`README.md`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/README.md), [`LICENSE`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/LICENSE), [`eval/README.md`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/eval/README.md).
- Pinned Tvix package manifests: [`eval/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/eval/Cargo.toml), [`glue/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/glue/Cargo.toml), [`nix-compat/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/nix-compat/Cargo.toml).

Jet's existing product rule agrees with this posture: `docs/spec/syntax-decisions.md:5440-5441` says Jet ships no Tvix code and uses Nix/Tvix as development and CI differential oracles.
