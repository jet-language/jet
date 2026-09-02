# Probe area-web — Web, full stack

Read `docs/research/domain-foundations/probes/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: web.

## Build this

An "Orders" full-stack app as one Jet package: typed routes (list, detail, create) with query params; a server-rendered page plus a browser island with reactive state; a form with validation and error display; a query/cache layer that refetches after a mutation; SQLite persistence; session auth (login, protected route); static assets; `jet dev` live reload while editing a route; `jet build --target web` for the client and a native server build; one end-to-end test that drives the flow. Start from examples/features/web/**, examples/features/net/http_*.jet, examples/features/io/db*.jet, examples/features/ui/**, and the ratified e14 web suite rulings (Tower decisions D-DX-SUITE1, D-DX-WEBARCH1, D-DX-LIVE1, D-DX-PROD1: `node plugins/tower/tower.mjs decision show <id> --json`) so you do not re-decide what is ratified; test whether today's binary honors it.

## Answer these

1. Can a library author ship a router/query/forms/table/store suite as packages on today's primitives, or which piece needs the compiler (typed route params, server/client split, hydration boundaries)?
2. Does the live loop keep state across an edit?
3. What does a first-party web battery need beyond the ratified suite?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/web-frameworks/`, `~/.cache/jet-luna/dx2/cms-ecommerce/`, `~/.cache/jet-luna/dx2/serverless-edge/`, `~/.cache/jet-luna/dx2/browser-extensions/`, `~/.cache/jet-luna/dx2/documentation-static-sites/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-web/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-web/pkg/`. Gap ids start with `area-web-G`.
