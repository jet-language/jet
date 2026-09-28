---
title: Persona audit 2026-07-23: 5 personas, 1 ship-blocker bug, slow dev loop
---
# Persona audit — 2026-07-23

Method: 5 fresh personas, from beginner to expert, in distinct domains. Each persona wrote and ran a real mini-project today. All runs used a fresh `target/debug/jet` through `scripts/agent/jet-env`. The build cache was cleared first. Project files and repros are under `/tmp/persona-*/`.

## P1 Maya — first-time programmer · guessing game
She built a number-guessing game with `input()`, `Int.parse`, and a loop. It runs.
- PULL: `print` and `input` work with no setup. Errors read like a tutor. The typo hint works inside string interpolation ("did you mean `secret`?").
- PUSH: She typed `continue`, as in every other language. She got E0003 "this line computes a value but doesn't do anything". The error does not say that Jet spells it `next`. This is the worst beginner trap found. Also, `?? { block }` is invalid, and the fix text shows no alternative. The same E0107 appears twice for one span.
- VERDICT: usable-with-friction. One keyword trap and one duplicate error away from ship-ready.

## P2 Priya — Python data analyst · CSV report
She built a ~30-line revenue report with core.encoding.csv, a map, and interpolation. The output is correct.
- PULL: The code is as short as Python without pandas. The csv, map, and string tools feel complete. Types add no ceremony.
- PUSH: The run loop is too slow for scripts. `jet run` compiles for ~14s on every run, even when the file did not change. The nix shell in jet-env adds ~10s more. `jet dev` runs the same file in 4.7s, but `jet run` never points to it. Python starts at once. That is the whole gap.
- VERDICT: usable-with-friction. The language is fine. The wait is not.

## P3 Devon — TypeScript backend dev · todo CLI
He built add/list/done subcommands with the `@CLI` enum derive and a `@Codable` JSON store. The full session works. The JSON on disk round-trips.
- PULL: The `@CLI` derive beats commander/yargs. Each command gets free help. An unknown command lists the known ones. `@Codable` beats hand-written JSON checks. The bounds panic names the count and the index.
- PUSH: The derive has no positional args. `add "buy milk"` fails and wants `--text`. The builder API has `.positional(…)`; the derive does not. A Node dev tries the positional form first. He also hit E0121 on `items[i] = item` before a print. That error taught the `~` copy fix on first read.
- VERDICT: usable-with-friction.

## P4 Klaus — Rust/C++ systems dev · particle sim + probes
Probes: use-after-take gives E0121. Aliased `&x, &x` gives E0204. A `var` captured across tasks gives E1101 with a copy/take fix. Sema catches all three in plain language. No rustc text leaked anywhere. Outside nix, the missing linker case gives a clean L2101 with the right fix.
- **BUG (headline): writes to a list element field do nothing, with no error.** `ps[0].x = 11` and `ps[0].x += 1` compile clean, change a temporary, and the old values print. Repro: `/tmp/persona-klaus/mut.jet` prints 1, 2. But plain `ns[1] += 5` IS rejected with a teaching error. So the accepted spelling is the broken one. This killed the natural array-of-structs sim with silent wrong physics. It goes against the spirit of I3: sema passes what codegen drops. Ship-blocker; card-worthy.
- PUSH: `..` is inclusive, so 0..1000 makes 1001 items. The common `0..len()` index loop panics at run time. The examples show no exclusive spelling. `&particles[idx]` cannot be borrowed, and the fix text ("bind the value first") teaches a copy that cannot change the list. The `&xs[a..b]` write-window workaround is never named. An `@Unsafe` block with no reason gets only lint L3101, and the same run prints "ok: no problems". Those two statements clash. The `_` discard binding makes nonsense errors ("give away a copy instead (`~_`)"). L0504 flags `total := 0.0` as money by mistake. E1004 leaks internal jargon ("documented M10 items").
- PULL: L0505 (heap growth in a loop → use an arena) is the exact nudge an expert wants. mem.Arena/Bump/Pool/Fixed are real. The retired-conversion error teaches `Float.from_int` well.
- VERDICT: blocked for his domain. Array-of-structs writes give silent wrong answers. The safety-error story is otherwise best-in-class.

## P5 Sam — game-jam hobbyist · roguelike tick loop
He built a turn-based roguelike: enum events, a seeded rng, and a tick function that returns a named tuple. It runs, and the combat log reads well.
- PULL: Pattern arms must be complete. A variant rename gave E0305 plus E0307, and E0307 named the missing arm. The refactor test passed. Seeded `core.random` and named tuples both helped.
- PUSH: A multi-payload variant, `Hit(String, Int)`, fails with a raw "expected `)`" parse error. The error never states the one-payload rule or the struct-payload idiom. The type `random.Rng` cannot be named, while a sibling error demands "use `random.Rng` here" (E0119 and E0112 clash; bare `Rng` works). The move checker misses a sure reassign after a move in a loop, so it forces a `~` copy every turn. Each `jet run` edit cycle takes 12.6s; `jet dev` takes 4.7s.
- VERDICT: usable-with-friction.

## Cross-cutting
1. **Ship-blocker:** `list[i].field = / +=` writes do nothing, with no error. Fix this first. All game- and sim-shaped code hits it.
2. **Shared top push: the slow dev loop.** Each run costs 12–14s, with no reuse for an unchanged file. `jet dev` (4.7s) exists, but `jet run` never names it. One tip line ("tip: jet dev re-runs on save") would change three verdicts.
3. **Cheap diagnostic fixes:** a `continue` → `next` teaching error; the one-payload rule in the parse error; the `random.Rng` naming clash; duplicate-error dedupe; the `_` cascade; the `@Unsafe` lint vs "ok: no problems" clash; the money-lint false positive; the "M10" jargon.
4. The core promise holds. No rustc text leaked. Safety errors teach. A beginner gets a real program to run in few edits — the seconds are the problem, not the edits.

Verdicts: Maya usable-with-friction · Priya usable-with-friction · Devon usable-with-friction · Klaus blocked (his domain) · Sam usable-with-friction. Nothing is ship-ready today. But the gap is narrow: one correctness bug, one latency story, one polish batch.
