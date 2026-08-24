# Jet native Nix evaluator: Snix/Tvix reuse note

Date: 2026-08-24. Card: #2162. Scope: upstream primary sources and the
existing Jet audit at
[`docs/audits/snix-tvix-license-research-2026-08-24.md`](snix-tvix-license-research-2026-08-24.md).

This sidecar adds exact package and file evidence. It does not repeat the
existing report's general warning that evaluator code is not permissively
licensed.

## Finding

Snix and Tvix both publish the same split:

- implementation crates: upstream prose says `GPL-3.0`;
- selected protocol buffer files: `MIT` or a file-specific permissive
  expression;
- direct linking or embedding of the evaluator: upstream says it falls under
  GPL-3.0.

The working SPDX inventory should record the implementation as
`GPL-3.0-only`, pending legal confirmation. `GPL-3.0-only` is the conservative
normalization of the upstream `GPL-3.0` wording. Do not record
`GPL-3.0-or-later` without an explicit upstream grant. The protocol identifiers
below are copied exactly from source headers.

## Package boundaries

Evidence is pinned to Snix commit
`6e990352dd1fe25248a9b47ca61e5b90cc829faf` and the Tvix mirror commit
`92e60f242b880f641e3346d42d3f4f4334ac3ee2`. Moving `canon` branches are not
reproducible inputs.

| Project | Package boundary | Role and reuse consequence |
| --- | --- | --- |
| Snix | `snix/eval` → `snix-eval` / `snix_eval` | Bytecode evaluator. Its manifest has no package `license` key. The evaluator README separates lightweight `MockIO` tests from store-aware tests and marks unsupported cases in `notyetpassing`. Do not link or embed it in Jet. |
| Snix | `snix/glue` → `snix-glue` | Joins `snix-eval` to `nix-compat`, `snix-castore`, `snix-store`, `snix-build`, and `snix-build-glue`. This is product implementation code, not a protocol exception. |
| Snix | `snix/nix-compat` → `nix-compat` | Nix encodings, hashes, derivations, NAR, and daemon compatibility code. The separate crate boundary does not change its GPL posture. |
| Snix | `snix/castore`, `snix/store`, `snix/build`, `snix/build-glue` | Store, content-addressed storage, and build/fetch support reached through glue. Keep all direct code reuse outside the Jet product unless the owner accepts GPL compliance. |
| Tvix | `eval` → `tvix-eval` / `tvix_eval` | Nix interpreter. The manifest has no package `license` key and enables the Nix 2.3 language tests by feature. Do not link or embed it in Jet. |
| Tvix | `glue` → `tvix-glue` | Joins `tvix-eval` to `nix-compat` and `tvix-simstore`. Same GPL implementation boundary. |
| Tvix | `nix-compat` → `nix-compat` | Shared package name, but separate pinned source and dependency graph. Audit it by source revision; do not treat it as a permissive compatibility API. |

Sources: [Snix `snix-eval/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/eval/Cargo.toml), [Snix `snix-glue/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/glue/Cargo.toml), [Snix `nix-compat/Cargo.toml`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/nix-compat/Cargo.toml), [Tvix `eval/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/eval/Cargo.toml), [Tvix `glue/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/glue/Cargo.toml), and [Tvix `nix-compat/Cargo.toml`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/nix-compat/Cargo.toml).

## Exact license evidence

| Material | Exact upstream evidence | SPDX inventory and implication |
| --- | --- | --- |
| Snix implementation | Repository README: `GPL-3.0`; root `LICENSE` is the GNU GPL version 3 text. | Record `GPL-3.0-only` for the implementation inventory after legal confirmation. Linking, embedding, copying, or modifying evaluator/store code needs a GPL decision and compliance plan. |
| Tvix implementation | Repository README: `GPL-3.0`; root `LICENSE` is the GNU GPL version 3 text. | Same `GPL-3.0-only` working posture. Tvix's read-only mirror does not add a new grant. |
| `snix/castore/protos/rpc_blobstore.proto` | Header: `// SPDX-License-Identifier: MIT`. | `MIT`. Exact schema reuse can be considered with copyright and MIT notice retention. |
| `snix/castore/protos/rpc_directory.proto` | Header: `// SPDX-License-Identifier: MIT`. | `MIT`. The imported `castore.proto` still needs its own file-level review. |
| `snix/castore/protos/castore.proto` | Header: `// SPDX-License-Identifier: OSL-3.0 OR MIT OR Apache-2.0`. | Exact expression: `OSL-3.0 OR MIT OR Apache-2.0`. Select one permitted option only if Jet's notice policy supports it, or preserve the expression and notices. |
| Generated bindings, fixtures, and helper code | No reviewed source says that these artifacts inherit a protocol file's grant. | Do not infer `MIT` for generated Rust, fixtures, tests, or helper code. Audit each source file and generator output before copying. |

Sources: [Snix README](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/README.md), [Snix `LICENSE`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/LICENSE), [Snix `rpc_blobstore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_blobstore.proto), [Snix `rpc_directory.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/rpc_directory.proto), [Snix `castore.proto`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/castore.proto), [Snix protocol `LICENSE`](https://git.snix.dev/snix/snix/src/commit/6e990352dd1fe25248a9b47ca61e5b90cc829faf/snix/castore/protos/LICENSE), [Tvix README](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/README.md), and [Tvix `LICENSE`](https://github.com/tvlfyi/tvix/blob/92e60f242b880f641e3346d42d3f4f4334ac3ee2/LICENSE).

Canonical identifier references: [SPDX `GPL-3.0-only`](https://spdx.org/licenses/GPL-3.0-only.html), [SPDX `MIT`](https://spdx.org/licenses/MIT.html), [SPDX `OSL-3.0`](https://spdx.org/licenses/OSL-3.0.html), [SPDX `Apache-2.0`](https://spdx.org/licenses/Apache-2.0.html), and [Cargo license metadata](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields).

## Recommended owner posture

Ratify this boundary:

1. Do not add Snix/Tvix Rust crates or copied implementation code to Jet's
   shipped evaluator. This includes `snix-eval`, `tvix-eval`, `snix-glue`,
   `tvix-glue`, either `nix-compat`, and store/build crates.
2. Use one immutable Snix or Tvix build as an external CI differential oracle.
   Pin the evaluator revision, Nix revision, nixpkgs revision, corpus manifest,
   and oracle output. Do not ship the oracle as part of Jet.
3. Copy a protocol definition only when Jet needs that wire contract. Preserve
   the exact SPDX header and copyright notice. Keep generated bindings and test
   inputs under separate source and license review.
4. Implement Jet behavior independently behind Jet's evaluator seam. Compare
   evaluation result, derivation identity, named output identity, evaluated
   input graph, and realized closure against Nix. Snix/Tvix can expose useful
   test methods, but their test code is not automatically reusable.

This posture preserves the existing Jet rule: Nix/Tvix may serve as
development and CI differential oracles, but Jet ships no Tvix implementation.
It also leaves the owner one explicit alternative: approve a GPL-3.0-only
product posture with source, notice, and whole-work compliance before any direct
reuse.

## Derived plan for card #2162

1. Record the owner decision above, including whether a separately distributed
   GPL oracle is allowed in CI tooling.
2. Freeze one Nix, Snix/Tvix, and nixpkgs revision tuple for each corpus run.
3. Build a whole-nixpkgs corpus with explicit `evaluated`, `Nix error`, `Jet
   error`, `identity mismatch`, and `unsupported semantic` counts per revision
   and system.
4. Admit overlays and custom flakes only after evaluated graph and closure
   identity match Nix, not after attr lookup succeeds.
5. Remove index-only user-facing limits only for surfaces covered by those
   measured results. Keep unsupported semantics named and versioned.

No source reviewed here records a permissive grant for evaluator code, generated
bindings, or language-test fixtures. This is an engineering recommendation, not
legal advice.
