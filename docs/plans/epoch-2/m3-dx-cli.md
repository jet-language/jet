# E2-M3 — Developer command UX

**Status:** draft — **blocked on D-DX1…D-DX6** (Group M3) and, for the free
readability win, **D-SUGAR1** (digit separators). See
docs/spec/decision-ballots.md.
**Depends on:** E2-M2 (exit-code/version contract). Foundation for E2-M4 and the
shared fix engine used by E2-M13 LSP.
**Error codes:** E21xx block; lints L21xx (claim in docs/spec/diagnostics.md).

## Goal

Make the command line feel as intentional as the language. The loved CLI tools
(ripgrep, fd, gh, bat, cargo, flutter doctor — distilled into
docs/spec/decision-ballots.md Groups 17/19) share five traits: fast, zero-config,
beautiful by default, scriptable when piped, and teaching as they go. Jet's
philosophy already commits to four; this milestone delivers the techniques.

## Owner decisions — ratify before any code

| ID | Question | Rec | Ratified |
|---|---|---|---|
| D-DX1 | `--json` diagnostic schema stable + versioned by M3 exit | **A** — stable | ✅ ratified 2026-06-16 — A: stable `--json` |
| D-DX2 | `jet doctor` scope | **A** — rustc/cache/PATH/LSP/registry health | ✅ ratified 2026-06-16 — A&B: health checks AND auto-fix |
| D-DX3 | Zed dev extension this epoch (= E2-V9) | **A** — yes, dev-tier | ✅ ratified 2026-06-16 — A: Zed dev extension |
| D-DX4 | Shell completions + man pages from one source | **A** — ship in M3 | ✅ ratified 2026-06-16 — A: ship completions + man pages |
| D-DX5 | External subcommands (`jet-foo` → `jet foo`) | **A** — PATH discovery now; formal plugin API → Epoch 3 | A | ✅ ratified 2026-06-16 — A now; B → `docs/plans/epoch-3/plugin-api.md` |
| D-DX6 | OSC 8 terminal hyperlinks on file:line / codes | **A** — when supported | ✅ ratified 2026-06-16 — A: OSC-8 hyperlinks |
| D-SUGAR1 | Digit separators `1_000_000` | **A** — lexer-only, free | ✅ S67, already implemented (src/lexer.rs; example `34_digits.jet`) |
| D-BUILD1 | `jet doctor` FFI section | — | ✅ ratified 2026-06-16 — A: `jet doctor` FFI section |
| D-BUILD2 | `jet build -v` bridge steps | — | ✅ ratified 2026-06-16 — A: `jet build -v` prints bridge steps |

## Scope

- **TTY-aware presentation.** Color/progress/interactivity only when attached;
  plain deterministic bytes when piped or in CI. Respect `NO_COLOR`,
  `FORCE_COLOR`, `--color=auto|always|never`. Never make scripts parse ANSI.
- **Stable exit-code table** (extends E2-M2's contract): 0 ok, 1 user error, 2
  usage, 70 runtime panic, 101 ICE (I2). Documented and golden-tested.
- **`--json` diagnostics** for `check`, `build`, `test`, and package commands —
  one versioned schema shared with the LSP and the fix engine (D-DX1).
- **`jet explain <code>`** — offline terminal essays for every E/L code (the
  rustc `--explain` idea, but in the terminal and offline). Errors print
  `run \`jet explain E2103\` to learn more` without making the message noisy.
- **`jet doctor`** — environment self-diagnosis: rustc reachable, cache healthy,
  PATH sane, LSP wired, registry reachable; actionable fixes, no network unless
  asked. Especially important because Jet hides a rustc dependency.
- **Examples-first help** and a friendly no-args `jet` that greets and shows the
  three commands that matter (not a usage error). "Did you mean" on typo'd
  subcommands/flags reusing the S14 teaching-error muscle.
- **Completions + man pages** generated from one source of truth (D-DX4).
- **Unified fix engine** shared by CLI `jet fix` and LSP code actions; structured
  machine-applicable suggestions tagged in the `--json` schema.
- **OSC 8 hyperlinks** on file:line and error codes when the terminal supports
  them (cheap differentiator; D-DX6).
- **External subcommand discovery** (D-DX5): an executable named `jet-foo` on
  PATH is invokable as `jet foo`, cargo-style — zero-cost extensibility that
  keeps I8 intact.
- **Digit separators** (D-SUGAR1): `1_000_000` lexes identically to `1000000`;
  the formatter neither inserts nor strips them.

## Diagnostics to register

- **E2101** unknown subcommand (with "did you mean").
- **E2102** unknown/ambiguous flag (with suggestion).
- **L2101** `jet doctor` advisory: rustc/cache/PATH problem with a fix.
- Plus the `jet explain` index entry obligation for *every* existing code.

## Examples & tests

- Golden tests pin human output *and* `--json` output for one diagnostic each in
  check/build/test.
- `tests/cli/no_args_greeting.txt`, `tests/cli/did_you_mean.txt`,
  `tests/cli/doctor_ok.txt`, `tests/cli/explain_E2101.txt`.
- A CI-mode test proving output is deterministic and ANSI-free under
  `NO_COLOR=1` and when piped.
- `examples/features/34_digits.jet` — `1_000_000` prints `1000000`.

## Out of scope

- A TUI for its own sake (earn it later, e.g. a test watch mode).
- A plugin system beyond PATH-discovered subcommands.
- Telemetry of any kind (explicitly not, even opt-in, this milestone).
- `jet emit --rust` (that is D-TOOL3, owner-gated, separate).

## Exit criteria

- Golden tests pin human + JSON output; CI mode is deterministic and ANSI-free.
- Every diagnostic points to `jet explain` without noisy error text.
- `jet doctor` gives actionable fixes offline by default.
- `jet` with no args greets and orients; typo'd subcommands suggest the right one.
- Completions (bash/zsh/fish) + man pages generate from one source.
- `nix develop -c cargo test` green; no new compiler crates (I6); any
  tooling-binary crate for completions/line-handling is owner-approved.
