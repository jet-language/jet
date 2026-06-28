# Plan: Release and CI hygiene for v1 credibility

**Status:** implemented through the CI baseline. No language decision is required.

## Problem

Jet currently looks less release-ready than its public story implies:

- `Cargo.toml` says `version = "1.0.0"` while `flake.nix` packages `0.1.0`.
- There is no `.github/workflows/` CI/release workflow in the repo.
- `cargo check` succeeds but emits warnings.
- The plain local `cargo test` path can fail outside the Nix shell because the C
  toolchain/linker is provided by `nix develop`.

This is credibility work, not language design.

## Scope

1. Align version metadata across Cargo, Nix, docs, and release fixtures.
2. Add a CI workflow that runs the documented Nix path.
3. Make warnings actionable and eventually fatal.
4. Document the supported local test path clearly.
5. Add a release workflow only after CI is green and version metadata is aligned.

## Implementation Steps

### 1. Version source of truth

- Pick one release version for the current tree.
- Update `Cargo.toml`, `flake.nix`, release fixtures, and docs that print or package
  the version.
- Add a small test or script that fails when Cargo and flake versions drift.

### 2. CI baseline

- Create `.github/workflows/ci.yml`. **Done 2026-06-28.**
- Run:
  - `nix flake check`
  - `nix develop -c cargo check`
  - `nix develop -c cargo test --lib`
  - focused integration suites that are stable in the Nix shell
  - `node tools/Tower/Tower.mjs status`
- Keep any known long-tail or flaky suites explicit rather than silently skipped.

### 3. Warnings policy

- First pass: fix or explicitly allow current `cargo check` warnings.
- Then add `RUSTFLAGS="-D warnings"` to CI for `cargo check`.
- Do not make repo-wide `cargo fmt --check` blocking until the existing broad rustfmt
  drift is cleaned in a separate mechanical pass.

### 4. Local developer path

- Update docs to say the supported full test path is through `nix develop`.
  **Done 2026-06-28** in `README.md`.
- Keep plain `cargo check` usable where possible, but do not imply plain `cargo test`
  is the official environment when it requires system linker/toolchain setup.

### 5. Release workflow

- Add `.github/workflows/release.yml` after CI passes.
- Gate release on:
  - clean CI
  - version metadata match
  - `nix build .#jet`
  - generated artifact checksum
- Keep publish/upload steps manual or dry-run until registry upload is implemented.

## Verification

- `node tools/Tower/Tower.mjs status`
- `nix flake check`
- `nix develop -c cargo check`
- `nix develop -c cargo test --lib`
- `nix build .#jet`

## Risks

- Enabling `-D warnings` before fixing existing warnings will break CI immediately.
- Release automation can overpromise if it uploads nowhere; keep upload dry-run until
  registry publish is real.
- A broad rustfmt cleanup will touch many files and should be kept separate from the
  CI workflow patch.
