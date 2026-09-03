# Probe prim-time — Time: calendars, business dates, time scales

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Primitive probe: cross-cutting; you test whether library code can do this and what primitive is missing; no batteries.json. Areas this probe informs: backend, data, science.

## Build this

Build, as library Jet over the core time types: business-day arithmetic with an exchange holiday calendar; recurring schedules with time zones and DST; and astronomical time scales (UTC, TAI, TT, UT1 with leap seconds) with conversions to Julian dates. Use examples/features/time/**.

## Answer these

1. What does core time lack for library authors: leap-second tables, zone database access, calendar traits, high-resolution instants?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/fintech-payments/`, `~/.cache/jet-luna/dx2/quant-trading/`, `~/.cache/jet-luna/dx2/astronomy-astrophysics/`, `~/.cache/jet-luna/dx2/space-satellite/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/prim-time/probe.md`, `gaps.json`, and the code under `~/.cache/jet-luna/dx3/prim-time/pkg/`. Gap ids start with `prim-time-G`.
