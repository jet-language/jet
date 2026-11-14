# Gauntlet Build/update

Use this playbook only for Build/update mode. Read
[`shared.md`](shared.md) first.

## Matrix

`Tools/gauntlet/matrix.json` names the full territory: personas × domains ×
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

Use `Tools/gauntlet/entries/<name>/`:

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

`Tools/gauntlet/harness/` measures each entry and implementation on one machine and
one run:

- median-of-N wall time and peak RSS;
- cold and warm compile with the competitor's own toolchain;
- `jet run` and `jet dev` first-result latency;
- binary size;
- readability proxies: LOC, tokens, distinct concepts, and ceremony ratio;
- RLI5 rubric scores (advisory);
- authoring cost.

Comparisons use same-run ratios. Raw times are machine-local. A full run emits
`Tools/gauntlet/results/<date>.json`; `--entry` and `--axis` emit
`<date>-<entry>.json` and `<date>-axis-<axis>.json`, so they do not overwrite
the day's full report. These files are gitignored, and
`Tools/gauntlet/harness/status.mjs --merge` folds them into `status.json`.

The harness measures the admitted Jet compiler: `--jet-bin`, else `$JET`, else `jet`
on PATH, run in place beside its runtime pack. Run and dev tiers execute inside the
compiler process, so measure an optimized self-hosted build, not a debug one. A tool
entry's cases run with that compiler's directory first on PATH and as `$JET`, so the
`jet` an original or its twin starts is the one measured.

Every run also writes the results file's `standings` section (#6834,
`Tools/gauntlet/harness/standings.mjs`). Each comparison entry is measured
against every peer in `matrix.json` `rails.standings` (Rust, C, Zig, Go,
Python, Bun, Node, Vite) and its hand-written asm ceiling
(`Tools/perf/asm-parity/<cell>/ref.S`, linked by the cell's `gauntlet` list).
The metrics are startup, run wall, run cycles (`perf stat -e cycles:u`), peak
RSS and size; interpreted peers count script plus runtime. Each metric gets
Jet/peer with a floor verdict (pass below 1.00) and, for asm, a ceiling verdict
(meet at 1.00 or below, gap = ratio − 1). A peer with no program is
`uncovered`; one the manifest's `standings.not_applicable` excludes is
`not_applicable` with its reason; a missing toolchain is `unavailable`. Failed
and mismatched runs stay rows too. `--dry-run` lists every entry × peer ×
metric cell and the toolchains without running anything. Tower's Gauntlet page
and `tower gauntlet standings` show the newest results file's section.

Use snapshot-only history unless the owner approves a different policy.

## Seed and close

The first build targets about 12–15 entries across all three tiers:

- micro performance kernels for the C/Zig claim;
- real CLI, service, or parser programs where trust lives;
- script or notebook tasks for the ergonomics claim.

Fill the matrix breadth-first. Growth targets uncovered cells.

Build/update is complete only when the approved matrix is landed, entries build
and produce expected output in every required language, the harness runs end to
end, and one Run-mode report is produced from the result.
