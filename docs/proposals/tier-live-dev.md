# The live tier: swap by default, keep the pinned world

Status: proposal, 2026-09-01, revised after the review passes; element 4 of `whole-language-frame.md`. Independently adoptable; strongest with `shapes-one-fact.md`. Ballots: D-LIVE1 (recommended A), D-LIVE-PERSIST1 (A). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

Jet already ratified and partly built a live tier. `jet dev` is the watch loop and can swap changed functions into the resident process (D-DEV4, D-HOTSWAP1). `#Persist` pins state across a swap, and it is the one module binding a function may write, because surviving the reload is the entire reason it exists (D-PERSIST-DEVSTATE1=A). A layout change to a pinned type triggers a clean announced restart (D-HOTSWAP1). What is missing is the default and the migration: swap is chosen by flag or auto-detection, every save prints nothing about what it kept, and a pinned value whose published type gained a field is thrown away even when the one-line `migration` that wire data already uses could carry it across.

The proposal makes swap the default, prints one line per save naming what was swapped, kept, migrated, or restarted, and migrates pinned values of `#PublishedSchema` types through the same step functions decode uses. Every other layout change keeps the ratified announced restart. `#Persist` stays. Meaning never changes: what a program does is identical under `jet run`, `jet build`, and a live session (I9). A precondition proof (a long-lived edit with callers mid-execution on every engine) lands before the default flips.

Score: mechanisms deleted 1 (the closed tier enum becomes a ledger value); capabilities kept all; capabilities gained 3 (swap by default, a per-save receipt line, migration of pinned published values).

| today | proposed | ballots |
|---|---|---|
| swap is a flag or auto-detection | swap on save by default; one line per save | D-LIVE1 |
| a layout change restarts even when a migration exists | pinned published values migrate; restart otherwise | D-LIVE-PERSIST1 |

## The problem

| today | evidence |
|---|---|
| swap is a flag or auto-detection | `jet dev --swap` (`Source/CmdDevTools.rs:211-254,954-1006`; D-DEV4) |
| a layout change restarts even when a migration exists | D-HOTSWAP1 (`syntax-decisions.md:5394-5395`); `SchemaMigration.rs` is consulted only by decode |
| no per-save receipt | the dev loop prints program output only |
| tier is a closed enum | `Interpreter`, `Jit`, `Aot` (`Source/CmdCompile.rs:1177-1181`); web, REPL, notebook outside |
| modes do not compose | `jet run --interpret --release` → E2102 |
| watch identity is weak | `PathStamp` is `{exists, mtime, len}` (`WatchService.rs:49-72`) |
| Jet loses | Dart keeps state on reload by contract and reports what it reloaded; Erlang runs a migration callback (lane D) |

The audit's first draft proposed persisting unmarked module values; the rival-family review showed that only `#Persist` bindings are writable from a function (E0111 otherwise) and that migration is ratified only for `#PublishedSchema` types. The design changed to respect both.

## The proposal on the page

Real today (`examples/features/devloop/persist.jet` shape):

```jet
#Persist counter := 0

fn run() {
    counter += 1
    print("run {counter}")
}
```

```text
$ jet dev --swap app.jet     # swap by flag; `counter` survives; a layout change restarts
```

Proposed (illustrative; `#Persist` stays, one marker and one block are added for the day the shape changes):

```jet
#[PublishedSchema, Codable]
struct Enemy {
    speed: Float{1.0}
}

struct World {
    player: Point
    enemies: [Enemy]
}

#Persist world := World{}

fn chase(e: Enemy, w: World) Enemy -> { return e.move_toward(w.player) }

fn run() {
    loop {
        world.enemies = world.enemies.map((e: Enemy) -> chase(e, world))
        draw(world)
    }
}
```

The timeline, with the edits named:

```text
$ jet dev game.jet
live  game.jet  watching 1 file
# 1. edit the body of chase(), save
saved game.jet  swapped chase  kept world
# 2. add `health: Int{100}` to Enemy and `migration Enemy { add health: Int = 100 }`, save
saved game.jet  swapped chase  migrated world.enemies (Enemy +health = 100)
# 3. add `armor: Int{0}` to Enemy with no migration, save
saved game.jet  layout changed: Enemy gained armor  restarting; to keep the world write `migration Enemy { add armor: Int = 0 }`
# 4. rename the binding `world` to `arena`, save
saved game.jet  `world` was renamed to `arena`: state reset
```

Line 3 is the ratified restart with its repair named. Line 4 is the ratified identity rule (module path plus binding name) made visible. An edit that only changes a field initializer, such as `speed: Float{2.0}`, does not rewrite existing enemies: the value already exists, and the line says `kept world`; a `migration` or a restart is the explicit way to change stored values.

## Rungs

| rung | who | spelling | what happens |
|---|---|---|---|
| 0 | beginner | `jet dev game.jet` | save, keep running; swap by default with one line per save (proposed) |
| 1 | intermediate | `#Persist world := World{}` | the pinned world survives a swap (ratified) |
| 2 | intermediate | `#[PublishedSchema, Codable] struct Enemy` plus `migration Enemy { add health: Int = 100 }` | a shape change migrates the pinned value in place (proposed) |
| 3 | expert | `jet dev --restart game.jet` | fresh process on every save (ratified flag) |
| 4 | expert | `dev: .{ swap: false }` in `package.jet` | the project refuses swap (proposed) |
| 5 | expert | `jet prove game.jet --capture` | the session is a safe replay (element 6) |

Rung 0 is untouched by every rung above it.

## Three exits

| exit | spelling |
|---|---|
| see | every save prints swapped, kept, migrated, or restarting; `jet inspect build --live PID` lists the pinned values with their shapes (illustrative) |
| write | `#Persist` for what survives; `migration` for how a published shape moves |
| refuse | `jet dev --restart` for the session; `dev: .{ swap: false }` for the project |

## What a tier is (proposed)

| tier | which facts stay alive | receipt |
|---|---|---|
| comptime | none at runtime; facts fold into values | build identity |
| check | none run; conflicts reported | status object |
| test | claims run and produce evidence | evidence report |
| dev | contracts checked, sentries on (D-MEM-SENTRY1), safe capture on (element 6), pinned values kept across swaps | replay and index |
| release | no fact remains (D-FACT-LAW1) except contracts kept by policy | build receipt |
| replay | boundary facts replayed from a record | the record |

The enum in `CmdCompile.rs` becomes a value on the ledger query, and `--interpret` becomes a choice of engine, not a mode that refuses build flags.

## Decisions

### D-LIVE1 — swap by default

| option | what |
|---|---|
| A (recommended) | swap on save by default; `#Persist` values kept as ratified; layout change restarts as ratified unless D-LIVE-PERSIST1 migrates it; one line per save; `--restart` and `dev: .{ swap: false }` refuse; precondition proof before the flip |
| B | swap by default; every value resets, including `#Persist`; would amend D-PERSIST-DEVSTATE1 away |
| C | keep today's flag and auto-detection |

Amends: D-DEV4 and D-DEVMODE1 defaults; D-HOTSWAP1's default. The module mutation law (D-PERSIST-DEVSTATE1=A) and the layout-restart rule are unchanged.

### D-LIVE-PERSIST1 — pinned state when its type changes

| option | what |
|---|---|
| A (recommended) | a pinned value of a `#PublishedSchema` type migrates in place through its migration block; any other layout change restarts as ratified, with the migration to write named; a renamed binding resets and says so |
| B | keep today: announced restart on any layout change |
| C | migrate any Codable pinned value; needs a second migration eligibility rule (amends D-MIGRATE1) |

Amends: D-MIGRATE4 (step functions run on resident values). D-HOTSWAP1's restart stays the fallback.

## What stays

| kept | why |
|---|---|
| `#Persist` | one module binding a function may write |
| `jet dev --restart` | fresh process on every save |
| `jet dev --watch=off` | the watch loop can be disabled |
| `--trace-tiers` | reason visible for the tier |
| the watch service and its replacement transactions | watch identity and replacement |
| the announced restart on a layout change without a migration | ratified fallback |
| `jet run` and `jet build` as they are | meaning never changes |
| I9 | execution parity |

## Implementation shape

| phase | work |
|---|---|
| A | tier as a ledger value; `--interpret` composes with build flags; watch stamps include a content digest; the long-lived edit test with callers mid-execution on the JIT and interpreter engines |
| B | resident-value migration through the `SchemaMigration` step functions for `#PublishedSchema` types; the per-save line with snapshots |
| C | after D-LIVE1 and D-LIVE-PERSIST1: defaults flipped, `dev: .{ swap: false }` field, rename reset line |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Dart | keeps state on reload by contract and reports what it reloaded | swap, pin, and migration block are not on by default |
| Erlang | names old and new code versions and runs a migration callback | swap, pin, and migration block are not on by default |
| Smalltalk | saves the image | swap, pin, and migration block are not on by default |

## Strongest unverified assumption

That the resident tier-1 image can swap a function whose callers are mid-execution without a restart in the common case. Lane D verified `jet dev --watch=off` runs and read the swap code; it did not exercise a long-lived edit. The phase A test owns that proof on every engine, and the default does not flip until it is green.

| assumption | how it is proved | where |
|---|---|---|
| the resident tier-1 image can swap a function whose callers are mid-execution without a restart in the common case | the phase A long-lived edit test on every engine | card #2504 |
