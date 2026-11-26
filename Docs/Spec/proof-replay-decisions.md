# Jet proof and replay contracts

This document states the durable contracts for `jet prove`, `.jetproof`
evidence, `.jetproof-replay` capture/replay, opt-in solver proofs, and proof
lenses. Exact CLI registrations live in
`Compiler/JetFoundation/Source/Registry/CLICommands.jet`, path checks in
`Compiler/JetCLI/Source/CLI/Inspect.jet`, exercised cases in
`Tests/Cases/Prove/`, diagnostics in `Compiler/Prelude/Diagnostics.jet`, and
ratified ballots in Tower. Use the [Jet vocabulary](vocabulary.md) for domain
terms. (Full historical prose lives in git history:
`git show 20be9d938a5:Docs/Spec/proof-replay-decisions.md`.)

## Command surface and target identity

- **Single command:** `jet prove TARGET` accepts exactly one positional target
  (a `.jet` file, package directory with `package.jet`, or workspace directory).
  Options: `--lens VALUE` / `--lens=VALUE` (repeatable), `--json`,
  `--replay ARTIFACT` / `--replay=ARTIFACT`, `--capture[=ARTIFACT]`,
  `--capture-sensitive[=ARTIFACT]`. Capture and replay are mutually exclusive.
  No separate `jet replay` command exists.
- **Deterministic input closure:** File targets and ancestors may not be
  symlinks. Directory members are sorted by UTF-8 path bytes and hashed with
  package/lock/build/import/generated-input metadata into
  `{kind, root, members: [{path, sha256}], inputSha256}`.
- **Single runnable member for capture/replay:** Capture or replay requires
  exactly one runnable member (`E3624` before build or effects otherwise).

## Proof execution and persisted evidence

- **Deterministic reporting:** Proof reports separate verified facts, failed
  obligations, and skipped/unproven items; unknown or timed-out solver queries
  never report as proved.
- **`.jetproof` artifacts:** Persisted proof evidence binds results to the exact
  `inputSha256`, compiler/toolchain identity, and selected lenses so stale
  proofs cannot validate modified code.

## Replay capture, wire format, and privacy

- **Boundary capture:** `.jetproof-replay` records boundary inputs and
  nondeterminism in a canonical, versioned wire format validated before
  execution. Invalid, tampered, or incompatible replay artifacts are rejected
  before running user code.
- **Privacy by default:** `--capture` redacts sensitive inputs (secrets,
  environment credentials, personal/machine paths) by default; retaining raw
  sensitive payloads requires explicit `--capture-sensitive`.
- **Replay authority:** Replay executes inside Jet's checked authority model
  without granting ambient host capabilities beyond the captured boundary.

## Solver boundary and lenses

- **Opt-in solver:** Formal solver verification (`--lens solver`) is strictly
  opt-in, bounded by explicit resource/time budgets, and never alters ordinary
  type-checking or runtime semantics (I3/I9).
- **Lenses as projections:** Proof lenses project facets of the same checked
  program and evidence model; a lens never invents a second parser or semantic
  checker.
