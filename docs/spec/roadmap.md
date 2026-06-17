# Roadmap

Each milestone is done when its exit criteria pass as tests. Examples are the
executable spec: a milestone ships with new `examples/` programs and new
`tests/ui` fixtures, all green.

> **Naming canon (owner, 2026-06-15):** **jet** is the language + compiler;
> **jetpack** is the package-manager engine/binary; **jetos** is the operating
> system (working title), built on jetpack. Single-file `jet run` stays
> ceremony-free forever (R9).

**Where detail lives (single source of truth):**

| Topic | Authoritative doc |
|---|---|
| Ratified syntax & owner decisions | [`syntax-decisions.md`](syntax-decisions.md) |
| Language behavior today | [`spec.md`](spec.md) |
| Open owner ballots | [`decision-ballots.md`](decision-ballots.md) |
| Epoch 1 milestone plans (done) | [`docs/plans/epoch-1/`](../plans/epoch-1/) |
| Epoch 2 milestone plans (active) | [`docs/plans/epoch-2/`](../plans/epoch-2/) |
| Jetpack & jetos sequencing + live status | [`docs/plans/jetpack-jetos/`](../plans/jetpack-jetos/) |
| Implementing-agent protocol | [`docs/plans/README.md`](../plans/README.md) |

Plans are gated on ratified decisions in `syntax-decisions.md` (see
`decision-ballots.md` for what is still open).

---

## Completed

**Epoch 1 — v1.0** verified 2026-06-14 (M0–M14). See epoch-1 plans for exit
criteria and examples.

**E2-M1 — Concurrency** verified 2026-06-14
([`m1-concurrency.md`](../plans/epoch-2/m1-concurrency.md)).

**E2-M2 — Release policy, editions, epoch contract** verified 2026-06-16
([`m2-release-policy.md`](../plans/epoch-2/m2-release-policy.md)). Ratified
compatibility/release policy ([`release-policy.md`](release-policy.md));
`edition:` marker in `payload.jet`; enriched `jet --version` banner; E2001
reachable, E2002/L2001 registered (honestly empty pre-1.0 deprecation registry).

**E2-M3 — Developer command UX** verified 2026-06-16
([`m3-dx-cli.md`](../plans/epoch-2/m3-dx-cli.md)). Stable exit-code table,
TTY-aware color (NO_COLOR/FORCE_COLOR/--color), versioned `--json` schema,
`jet explain` for every code, `jet doctor` (offline + `--fix` + C-FFI section),
no-args greeting + did-you-mean (E2101/E2102/L2101), completions + man page
from one registry, unified CLI/LSP fix engine, external `jet-<name>` discovery,
OSC 8 hyperlinks, `jet build -v`. Digit separators (S67) already shipped.

**E2-M13 — Expert low-level tier (S58)** verified 2026-06-17
([`m13-low-level-tier.md`](../plans/epoch-2/m13-low-level-tier.md)). `use
core.mem` discovery gate, `@audit("…")` + `@unsafe { … }` / `@unsafe fn` audit
gate, `Ptr<T>` (→ `*mut`), `mem.volatile_read`/`address_of`/`from_addr`;
diagnostics E3101/E3102/E3103 + lint L3101; the I1 amendment (D-LL1) recorded in
`architecture.md` (generated `unsafe` only inside user gates; safe Jet emits
none, enforced by `tests/golden.rs`). Deferred (open ballots): arenas (D-REF2),
wider `std.mem` API (D-LL3, name TBD).

**Post-v1 language features already shipped on `master`:** fan-out `f.[…]` (S75)
and fixed-size lists `[T#N]` (S76) — ratified and implemented 2026-06-16; see
`spec.md` and `syntax-decisions.md`.

---

## Active / not yet verified

### Epoch 2 — production platform

Consolidated overview, dependency order, and ballot gates:
[`docs/plans/epoch-2/README.md`](../plans/epoch-2/README.md).

All **E2-M2…E2-M18** remain open except **E2-M1**. Work may land on feature
branches before the roadmap marks a milestone verified — check git history and
`cargo test` rather than this file alone.

### Jetpack & jetos

Phase 1 environments and the typed `module { … }` surface: see
[`jetpack-jetos/README.md`](../plans/jetpack-jetos/README.md). **Live
built-vs-pending status:**
[`jetpack-jetos/IMPLEMENTATION-STATUS.md`](../plans/jetpack-jetos/IMPLEMENTATION-STATUS.md).

### Epoch 1 tail

**M12.2** — registry, semver resolver, `jet publish` / `vendor` / `audit`
([`m12-packages.md`](../plans/epoch-1/m12-packages.md)). M12.1 verified
2026-06-13.

---

## Deferred unless owner promotes

Items with Epoch 2/3 plans are tracked in those plan directories — not
duplicated here:

- Async/await, Go-scale networking → [`docs/plans/epoch-3/`](../plans/epoch-3/)
- User token macros (rejected by S26; sanctioned path is S56 comptime derives)
- Self-hosting; JetOS as a shipped OS product (jetpack/jetos research track)
- Comptime layer 3 / user-defined derives (S56) → Epoch 3

When a deferred item is promoted, add a milestone slot in the appropriate epoch
README and ratify any new syntax in `syntax-decisions.md` before implementation.
