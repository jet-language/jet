# Gauntlet Build/update

Use this playbook only for Build/update mode. Read
[`shared.md`](shared.md) first.

## Matrix

`gauntlet/matrix.json` names the full territory: personas × domains ×
task-kinds. Task-kinds include at least application, CLI, service, web,
embedded, data/numeric, scripting, and notebook-style exploration. Personas
span true novice through domain expert plus the unattended agent; start from
`persona-audit`'s ladder. Derive domains from a real sweep of what people
program, not from what Jet currently handles well.

The matrix is the owner's goal statement. Propose it, or a diff from the
existing matrix, and get owner approval in chat before landing it. Never
silently narrow it. Every entry tags the cells it fills. Cells with no entry are
uncovered territory in every run report.

If the matrix is absent, seed only a proposal. Approval is required before the
matrix, entries, or policy changes land.

## Entries

Use `gauntlet/entries/<name>/`:

- `entry.json` gives behavior, inputs, expected observable output, matrix cell
tags, tier (`micro` | `program` | `script`), language list, authoring provenance,
and cost per implementation.
- `jet/` is idiomatic-beginner Jet and the headline row. `jet-expert/` is
optional on performance entries.
- Add one directory for each reference-language port.
- Check in expected output. The harness verifies every implementation before
timing. Wrong output is a broken entry, not a fast result.

Use Rust and Python for every entry. Add C and Zig for performance claims. Add
the domain incumbent where one exists, such as TypeScript for web, Go for
services, or C for embedded work.

## Harness contract

`gauntlet/harness/` measures each entry and implementation on one machine and
one run:

- median-of-N wall time and peak RSS;
- cold and warm compile with the competitor's own toolchain;
- `jet run` and `jet dev` first-result latency;
- binary size;
- readability proxies: LOC, tokens, distinct concepts, and ceremony ratio;
- RLI5 rubric scores (advisory);
- Luna authoring cost.

Comparisons use same-run ratios. Raw times are machine-local. A full run emits
`gauntlet/results/<date>.json`; `--entry` and `--axis` emit
`<date>-<entry>.json` and `<date>-axis-<axis>.json`, so they do not overwrite
the day's full report. These files are gitignored, and
`gauntlet/harness/status.mjs --merge` folds them into `status.json`.

The harness measures `target/release/jet`. Run and dev tiers execute inside the
compiler process; a debug compiler would measure itself, not Jet. Use
snapshot-only history unless the owner approves a different policy.

## Seed and close

The first build targets about 12–15 entries across all three tiers:

- micro performance kernels for the C/Zig claim;
- real CLI, service, or parser programs where trust lives;
- script or notebook tasks for the ergonomics claim.

Fill the matrix breadth-first. Growth targets uncovered cells.

Build/update is complete only when the approved matrix is landed, entries build
and produce expected output in every required language, the harness runs end to
end, and one Run-mode report is produced from the result.
