---
title: Surface audit: ratified shape law vs shipped surface
---
# Surface audit — 2026-07-23

Method: verified the Class A–N inventory (docs/proposals/uniformity-paradigm.md) against today's tree, graded new surface (commit 27165ba2, expiring secrets) against the ratified atomic D-SHAPE rulings in history.json. The shape wave is decided; the drift now is implementation lag and fresh surface shipping outside the law.

## Findings (ranked)

1. **D-SHAPE6=A ratified, binary still flat.** Verdict A: group by noun — `jet inspect dossier`, `jet registry publish`. Shipped dispatch has no `inspect` or `registry` arms; flat verbs remain (`Source/main.rs:1361` publish, `:1368` keygen, `:1397` yank, `:1423` dossier, `:1167` live). Worse: user-facing fix text already teaches the grouped forms that fail today — `Source/main.rs:1173` "fix: run jet inspect live --attach <pid>", `Source/CmdDossier.rs:22-23` "Fix: jet inspect dossier …". Error text pointing at nonexistent commands breaks I4 (diagnostics are products).
2. **Spec teaches the retired copy verb.** `docs/spec/syntax-decisions.md:1218-1219` still says "`copy x` stays a verb — no third sigil (D-CAP2)". D-SHAPE-COPY1=A ratified the `~` sigil and retired `copy` to teaching error E0991 (`crates/jet-foundation/src/Syntax/core_surface.rs:191-208`); examples are migrated (`examples/interop/*/main.jet` use `~input`). Spec is authority level 3 and currently contradicts ratified law.
3. **Today's new surface ships four ctor shapes in one file.** `examples/features/memory/expiring_secret.jet` (27165ba2): `expiring.new("fresh", ttl, clk)` lowercase-module `.new` (line 10), `vault.ExpiringSecret.new(^key, …)` canonical type-static (line 21), `crypto.SigningKey.generate()` fifth verb (line 20), `time.clock(1000)` module factory (line 8). D-SHAPE3a=A keeps fresh state on `Type.new`/inferred `.new`; a module-function named `new` matches no ratified shape, and D-API-CTOR1=A says new construction shapes need a ballot. The uniformity doc predicted exactly this wrong-guess pair (`Expiring.new` vs `expiring.new`) — it shipped anyway.
4. **Duration canon drift in examples.** D-SHAPE-DURATION1=A scopes `Duration.seconds(n)?` to runtime numbers; static constants keep unit literals (`5s`, `500ms` — `examples/features/types/unit_literals.jet:8`). But new/net examples spell compile-time constants with the runtime form plus panic ceremony: `Duration.seconds(5) ?? panic("duration")` (`expiring_secret.jet:9,19`), `Duration.milliseconds(100) ?? panic(...)` (`http_server_lifecycle.jet:14`, `http_server_middleware.jet:47`). I5 makes examples executable spec; they currently teach the ceremony-heavy spelling.
5. **`constant_time_eq` vs `constant_time_equal` both live in one file.** `examples/features/crypto/crypto_suite.jet:15,33` and both codegen mappings (`crates/jet-codegen/src/Codegen/TIR/emit/core_calls.rs:1269,1362`). `eq` is not on D-API-LEN1=A's closed blessed-abbreviation list (len/fmt/args/env/mem) — this is a ratified-law violation, not an open choice.
6. **Binding drift inside one new example.** `expiring_secret.jet:9` `ttl :: …` vs `:19` `secret_ttl := …` — identical immutable job, two binding forms, same file.
7. **Proposal docs stale against the ratified wave.** `docs/proposals/uniformity-paradigm.md` and `language-shape-constitution.md` still frame D-SHAPE1–9 and the S4 copy reopen as future/open; ~30 atomic D-SHAPE decisions are ratified and retired. Decided clutter — migrate the still-live remainder, delete the rest.
8. **Known drift confirmed unchanged:** `time.clock` module factory (defended in `core_surface.rs:552`), `watcher.files` noun-first (`docs/spec/spec.md:2441`), `BigInt(100)` bare call (D-BIGINT1, deliberate).

## Bright spots

- The `~`/`^`/`&` sigil family migrated cleanly: no `copy x` survives anywhere in examples; teaching errors E0991/E0056/E0057 registered with decision IDs.
- D-SHAPE5a/5b package forms already recorded in the constitution doc with correct worked examples.

## Tracking (all minted 2026-07-23, P0 sidequests slotted behind #732)

- **#734** (wo 2) — implement D-SHAPE6=A inspect/registry groups — finding 1.
- **#735** (wo 3) — docs reconcile copy verb + stale shape-wave text — findings 2, 7. Shares syntax-decisions.md with #732; coordinate.
- **#736** (wo 3, deciding) — fresh-value verb set; **D-SHAPE-CTORVERB1=C ratified 2026-07-23**: `Type.new` deterministic, `Type.new_random` entropy; module factories retire — finding 3.
- **#737** (wo 5) — example duration/binding canon — findings 4, 6.
- **#738** (wo 5) — apply D-API-LEN1, retire `constant_time_eq` — finding 5.
- Finding 8 stays watched under #560's cross-surface matrix; no card (deliberate/grandfathered shapes pending D-SHAPE-CTORVERB1).


