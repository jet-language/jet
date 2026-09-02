# Shapes: one fact; text formats, env, and args are projections

Status: proposal, 2026-09-01, revised after the review passes; element 3 of `whole-language-frame.md`. Independently adoptable. Ballots: D-SHAPE-ONE1 (recommended C), D-SHAPE-PROJECT1 (A). Nothing here is implemented; transcripts marked illustrative do not run today.

## Executive summary

Jet already ratified and shipped most of the right substrate: one value tree (`DataTree`, D-SERDE2), one codec protocol (`Encode`/`Decode`), structs Codable by default (D-META-AUTO1), field defaults on the declaration (`port: Int{8080}`), a `migration` block that walks old wire shapes for `#PublishedSchema` types (D-MIGRATE1-4), and `env.decode<T>(prefix:, file:, allow:)` sharing one codec with JSON, TOML, and CSV (`examples/features/io/app_config.jet`). The audit's first draft called the env projection missing; the rival-family review corrected it.

What is left: a second JSON model that loses field order, a five-entry format enum for twelve codecs, one shared rename that cannot serve a legacy column name, command-line arguments decodable only at the entry function, and the ratified precedence (command input, then `#Env`, then the field default; D-CLI-GLOBAL1) applied only at the entry function. The proposal fixes exactly those. Database rows and FFI layout stay separate until a probe shows typed queries and layout survive; a field diff cannot name a table.

Score: mechanisms deleted 2 (`JSONValue`, the closed `EncodingFormat` enum); capabilities kept all; capabilities gained 3 (`args.decode<T>()` anywhere, `Config.merge` precedence, per-format rename facts).

| today | proposed | ballots |
|---|---|---|
| a second JSON model and a closed format enum | one shape fact reaches text formats, env, and args | D-SHAPE-ONE1 |
| args only at the entry function; precedence by hand | `args.decode<T>()` through the builder; `Config.merge` applies the ratified precedence | D-SHAPE-PROJECT1 |

## The problem: eleven coats

| coat | spelling | home | what it privately knows |
|---|---|---|---|
| `DataTree` | `.Object({…})` | `Prelude/CoreLib/JetStd/DataTree.rs:1-21` | the canonical tree; ships nine kinds where the law says six |
| `JSONValue` | `json.parse` | `crates/jet-foundation/src/JSON.rs:6-15` | a second tree, BTreeMap, loses order; not ratified public surface |
| `EncodingFormat` | internal | `EncodingTypes.rs:26-33` | five formats of twelve |
| CLI | `fn run(args: T)`, `#[Short("p")]`, `#[Flag]`, `#Doc` | D-CLI; `markers.rs:57-79` | decodes only at the entry function, where D-CLI-GLOBAL1's precedence applies |
| env | `env.decode<T>(prefix:, file:, allow:)`, `#Env("PORT")` | `fixed_sigs.rs:4115-4121`; `app_config.jet:38` | shipped; one codec with json |
| build settings | `@build.settings.key` | `Facts.rs:172-179` | string keys |
| DB rows | `core.db` | `core-library.md:3932-3975` | its own row mapping; unprobed |
| FFI layout | `#Layout(c)`, `#ABI` | `Prelude/Layout.rs`; `markers.rs:84-86` | shares only layout and skip with the wire facts |
| typed HTML | `#HTML("page.html")` | `core_surface.rs:464-467` | a string path (D-MARKERARGS1) |
| migration | `migration T { … }` | `Sema/SchemaMigration.rs:38-142` | ratified for `#PublishedSchema` types; `add`/`remove`/`change` ratified, only `rename` evidenced |
| dev state | `#Persist` and D-HOTSWAP1 | `examples/features/devloop/persist.jet` | a layout change restarts; the migration block is not consulted (element 4) |

Two probes from lane B: `json_integer_fidelity.jet` prints `-9223372036854775808` correctly on the typed path, while `encoding_breadth.jet` emits three L0520 display-migration warnings for `CBORError`, `JSONError`, `XMLError`; the codecs do not share one error type.

## The proposal on the page

Real today (verified on every tier, verification block `shapes`):

```jet
use core.encoding.json as json
use core.sys as env

#CLI
struct Config {
    port: Int{8080}
    host: String{"localhost"}
    #Env("DATA_DIR") data_dir: String{"./data"}
}

fn run(args: Config) ![FieldError] {
    text :: "{{\"port\": 9000, \"host\": \"example\"}}"
    file :: json.decode<Config>(text)                    // JSON: owned by the shape
    settings :: env.decode<Config>(prefix: "APP_")       // env: shipped, one codec with json
    print(file.port)                                     // 9000
}
```

Proposed (illustrative; the struct is unchanged, two things are new):

```jet
use core.args as args
use core.sys as env

#CLI
struct Config {
    port: Int{8080}
    host: String{"localhost"}
}

fn run() ![FieldError] {
    flags :: args.decode<Config>()                        // new: anywhere, through the same core.args builder as fn run(args: T)
    settings :: env.decode<Config>(prefix: "APP_")        // shipped
    cfg :: Config.merge(flags, settings)                  // new: the ratified precedence (D-CLI-GLOBAL1) outside the entry function
    print(cfg.port)
}
```

```text
$ APP_PORT=9000 jet run app.jet -- --port 7000
7000
$ APP_PORT=9000 jet run app.jet
9000
$ jet run app.jet
8080
$ APP_PORT=nine jet run app.jet
Error: at `port`: expected Int, found text "nine"        # invalid is reported, never treated as absent
```

`args.decode<T>()` lowers through the ratified `core.args` builder, so `--help` and every diagnostic are byte-identical to `fn run(args: T)`; there is no second parser (D-CLI builder floor).

### One shape, every text projection (D-SHAPE-ONE1 option C)

| field fact | spelling | json/cbor/csv/toml/yaml/xml | args | env |
|---|---|---|---|---|
| default | `port: Int{8080}` (ratified) | absent → 8080 | absent → 8080 | unset → 8080 |
| rename, shared | `#[Rename("port_number")]` (ratified) | key | `--port-number` | `PORT_NUMBER` |
| rename, per format | `#[Rename("port_number", env: "PORT")]` (proposed, D-MARKSIG1 signature) | key | `--port-number` | `PORT` |
| rename, per format only | `#[Rename(json: "port_number", env: "PORT")]` (proposed; keyword-only form of the same signature) | key | `--port` | `PORT` |
| skip | `#Skip` (ratified) | omitted | omitted | omitted |
| short | `#[Short("p")]` (ratified) | ignored | `-p` | ignored |
| doc | `#Doc("…")` (ratified) | ignored | help text | ignored |
| discriminant | `#[Discriminant("kind")]` (ratified) | tag field | n/a | n/a |
| version | `migration T { … }` on a `#PublishedSchema` type (ratified) | old shape decodes | n/a | n/a |

A format that does not understand a fact ignores it for that projection; the fact stays on the shape and `jet inspect shapes` lists it (illustrative). A fact never needs to be written twice. The three rename forms are one marker signature (D-MARKSIG1): a shared name, a shared name with per-format overrides, or per-format names only.

### Migration stays where it is ratified

```jet
#[PublishedSchema, Codable]
struct Profile {
    title: String
    score: Int
    verified: Bool{false}
}

migration Profile {
    rename name -> title
    add verified: Bool = false
}
```

An old JSON record `{"name":"ada","score":3}` decodes to `Profile{title: "ada", score: 3, verified: false}` on every text format (ratified D-MIGRATE4). Element 4 proposes the same block for a pinned live value of a `#PublishedSchema` type. Database schema scripts are not derived from it: a field diff has no table identity, constraints, or transaction policy, and that needs its own ballot after a probe.

## Rungs

| rung | who | code | what happens |
|---|---|---|---|
| 0 | beginner | `struct Config { port: Int{8080} }` | Codable by default (ratified D-META-AUTO1); `json.decode<Config>(text)` and `env.decode<Config>(prefix: "APP_")` work with no marker |
| 1 | intermediate | `#[Rename("port_number")] port: Int{8080}` | one field fact, every format honors it |
| 2 | intermediate | `#[Rename("port_number", env: "PORT")]` | one legacy name for one format (proposed) |
| 3 | intermediate | `#[PublishedSchema, Codable]` plus `migration Config { rename port_number -> port }` | old data still decodes (ratified) |
| 4 | expert | `derive Config.Wire { … }` | hand-written codec body (ratified D-ONCE-DERIVE1) replaces the generated one |

## Three exits for the auto-Codable magic

| exit | spelling |
|---|---|
| see | `jet inspect shapes app.jet` prints every type's wire shape per format with renames and defaults (illustrative); today `T.reflect()` and `T.@layout` (ratified) show pieces |
| write | `derive Config.Wire { … }` (ratified) |
| refuse | `policy: .{ lints: .{ deny: [auto_derive] } }` refuses silent generation for the package; `#!Codable` refuses both wire directions at one type (S55, D-META-AUTO1) |

## Decisions

### D-SHAPE-ONE1 — how far one shape fact reaches

| option | what |
|---|---|
| A | all projections including database rows and FFI layout now; rejected by the rival review: a field diff cannot name a table, FFI shares only layout and skip |
| B | text formats only; args keeps its own parser |
| C (recommended) | text formats, env, and args; per-format rename facts under one `#[Rename]` signature; migration stays on `#PublishedSchema`; `JSONValue` deleted; formats declared; database and FFI after their own probe |

Amends: D-ENCSTREAM-SURFACE1 (format enum becomes declared rows), D-MARKSIG1 (per-format rename signature). `JSONValue` deletion is conformance to D-SERDE13, not an amendment.

### D-SHAPE-PROJECT1 — args as a decode format

| option | what |
|---|---|
| A (recommended) | `args.decode<T>()` through the ratified builder; `env.decode<T>()` unchanged; `Config.merge(flags, settings)` applies the ratified precedence (command input, `#Env`, field default; D-CLI-GLOBAL1) outside the entry function; invalid present values are `FieldError` |
| B | keep `fn run(args: T)` only; the ratified precedence applies only there; elsewhere programs combine by hand |
| C | `Config.from_args()` and `Config.from_env()` constructors, a second verb family |

## What stays

| kept | why |
|---|---|
| `DataTree` | canonical tree |
| `Encode`/`Decode` | one codec protocol |
| auto-Codable | structs Codable by default |
| `T{expr}` defaults | field defaults on the declaration |
| `#[Rename]` | one field fact |
| `#Skip` | omitted for every projection |
| `#[Discriminant]` | tag field |
| `#[Short]` | short argument |
| `#Doc` | help text |
| `#Flag` | CLI projection |
| `#Env` | shipped env rename |
| `env.decode<T>(prefix:, file:, allow:)` | one codec with JSON, TOML, and CSV |
| `migration` on `#PublishedSchema` types | old shape decodes |
| `derive T.Wire` | hand-written codec body |
| `fn run(args: T)` as the beginner's zero-ceremony entry | beginner entry |
| `#Layout` | FFI layout |
| `#ABI` | FFI layout |

## Implementation shape

| phase | work |
|---|---|
| A | `JSONValue` callers moved to `DataTree`; format rows declared in the Prelude; `EncodingFormat` generated; one `EncodingError` |
| B | `migration add/remove/change` finished with examples on every tier (ratified D-MIGRATE2A/D/E, only `rename` evidenced); #2510 (`json.decode` with a computed text on the default tier) fixed |
| C | after D-SHAPE-PROJECT1: `args.decode`, `Config.merge`; after D-SHAPE-ONE1: per-format renames, `jet inspect shapes` |

## Where Jet loses today

| peer | what it does better | Jet today |
|---|---|---|
| Rust serde | has one protocol and many adapters, and no second value model | two JSON trees and no args projection outside `run` |
| Pydantic | reads env and JSON in one model with a prefix rule | two JSON trees and no args projection outside `run` |

## Strongest unverified assumption

That `core.args` can expose `decode<T>()` outside the entry function without a second parser. The builder floor is ratified; the card proves byte-identical help and diagnostics between `fn run(args: T)` and `args.decode<T>()`.

| assumption | how it is proved | where |
|---|---|---|
| `core.args` can expose `decode<T>()` outside the entry function without a second parser | byte-identical help and diagnostics between `fn run(args: T)` and `args.decode<T>()` | card #2503 |
