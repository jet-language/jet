# Boundaries: one crossing law

Audit of FFI, external dependencies, strings/literals/text, and data at boundaries. 2026-08-07. Card #1655. Ballot slate D-BOUND-*.

Vocabulary: [Jet vocabulary](../spec/vocabulary.md).

## Executive summary

Jet has already ratified every piece of one boundary law — as five separate laws that stop at their own borders. Typed heads check literal SQL and HTML but not URLs or paths. Codable decodes wire data but drops what it does not recognize. The FFI arrow names a foreign call but `#Transact` trusts every foreign runtime to roll back. jetpack locks a hash but nobody can read where the bytes came from. The fact ledger is ratified to hold taint and provenance rows, and nothing at any boundary writes one.

The one idea: **a value crossing a trust or format boundary is always the same act — a decode through a schema the compiler can name, and the crossing leaves a fact.** The act happens at four times: comptime (a literal), build time (a manifest, a dependency), link time (a foreign signature), run time (wire data). Jet built each time as its own world. They are one world.

The law: **nothing foreign becomes Jet silently — every crossing names its schema, and every crossing leaves its fact.** Every ratified boundary rule is already a theorem of this law. E0119 rejects an effect name the compiler cannot name. D-UNIFYLIT1 makes a typed head the one spelling for checked literal text. D-VALIDATE-DECODE1 gives every refused decode one error shape. D-MIGRATE1 makes schemas evolve by named steps. E1204 refuses dependency bytes that do not match their locked hash. The law is not new — it is the sentence these rules were always spelling.

What the ballots ask, in one breath: finish the typed-head row (URL, Path, DateTime — D-BOUND-HEAD1), let heads own their escapes so `Regex.{"\d+"}` works (D-BOUND-RAW1), let users declare heads so a sink is just a parameter type (D-BOUND-SINK1), let `jet inspect bind` eat data schemas the way it already eats C++ headers (D-BOUND-BIND1), make a successful decode the act that clears origin taint (D-BOUND-TAINT1), make `#Transact` refuse a foreign call with no undo contract (D-BOUND-UNDO1), give `#PublishedSchema` types protobuf-grade unknown-field preservation (D-BOUND-EVOLVE1), and give provenance a readable surface with a sane default (D-BOUND-PROV1). D-BOUND-LAW1 adopts the law itself, so every future boundary feature must land in a named cell instead of minting a sixth world.

What does not change: `DataTree` stays the one dynamic tree (D-SERDE-ACCESS=B). `Type.{"…"}` stays the one literal spelling (D-UNIFYLIT1=A). The fact ledger stays the one ledger (D-FACT-LAW1=B). The declined String surface stays declined (D-STR-DECLINE1=C). No invisible imports return (D-NAME-FILES1=C — `jet inspect bind` writes a file you can read). **Zero new mechanisms.** Every element is an instance of a rail the owner already ratified; the score is mechanisms deleted (sink registration as a concept, the FFI/data-schema divide, six hand-rolled JSON escapers, a duplicate JSON parser) and capabilities gained.

Below the surface, the audit also found the compiler not eating its own law: at least eight hand-written JSON escapers (two produce invalid or mangled JSON), a second full JSON parser in `jet-pkg-model`, the reader/writer format enum hand-copied into the JIT instead of `include!`d, and the Codable derive built by `format!`-ing source text. Those are defect cards, not ballots — they fall out of the law for free.

## The problem, briefly

One job — a foreign value entering Jet — is done by whichever mechanism happened to be nearest. The table is the proof; every row is the same underlying act wearing a different coat.

| # | The crossing | Mechanism today | Home | Defect |
|---|---|---|---|---|
| 1 | Literal SQL/HTML/Sh/Regex/bytes | Typed head, compile-checked | `crates/jet-codegen/src/Prelude/TypedText.rs`, E0152 | Solid — the model row |
| 2 | Literal URL/path/datetime | Runtime `url.parse(…) ?? panic` | `examples/features/net/url_mime.jet:5` | Compile-time knowledge, run-time check, panic ceremony |
| 3 | Backslash in a head body | 4-entry `ESCAPES` table applies first | `crates/jet-foundation/src/Syntax/math_layout.rs:318` | `Regex.{"\\d+"}` — the head never sees `\d` |
| 4 | Wire data → struct | `#Codable` decode | `crates/jet-sema/src/Sema/Registration/Serde.rs:39` | Derive `format!`s source text; unknown fields silently dropped. Adjacent: 49 pre-existing corelib failures, many root causes (#1618) |
| 5 | Old wire data → new struct | `migration { }` chain | `docs/spec/spec.md:842` (D-MIGRATE1) | Only `#PublishedSchema` types evolve; everything else breaks silently |
| 6 | Exact JSON numbers | `DataTree::Float` first, always | `EncodingTraits.rs` Decimal decode | Fractional JSON loses exactness before a typed target can see it (#1395, ratified, blocked) |
| 7 | Foreign call | `=[Java]=>` effect root, 16 flat variants | `crates/jet-sema/src/Sema/Effects.rs:60-93` | ROOTS1=A (one `FFI` root) ratified, unbuilt; `Py`/`Octave` ratified, never added |
| 8 | Foreign call in `#Transact` | `is_irreversible_effect` = Net, FS, Exec | `crates/jet-sema/src/Sema/Effects.rs:594-596` | All 16 foreign runtimes treated as rollbackable — they are not |
| 9 | Foreign binder errors | 17 `*Bind.rs`, 17 hand-rolled `BindError`s | `crates/jet-pkg-model/src/*Bind.rs` | D-FFI-UNIFY1=A ratified, unbuilt; PhpBind/PowerShellBind tests red |
| 10 | `extern` declarations | Hand-written externs with no binder root get `Effect::all()` | `crates/jet-sema/src/Sema/CheckerInfer/calls/direct_calls.rs:783` | The crossing with no named schema gets every effect; binder-generated externs get their language root |
| 11 | Manifest data | Three parsers: `pkg.jet` recursive descent, `env.jet` comptime eval, overlay line-parsers | `crates/jet-pkg-model/src/Package/mod.rs:49`, `Overlay.rs:444` | None shares the encoding law; plus a second full JSON parser at `jet-pkg-model/src/JSON.rs:1` |
| 12 | Dependency bytes | Locked hash (E1204); TUF roots typed | `crates/jetpack/src/TrustRoot.rs:18` | Signature verification not live ("JP6B"); no surface to read provenance |
| 13 | Values from net/fs/env | Nothing — arrive as clean `String` | — | Taint plane ratified (D-FACT-FLOW1/HOME1), no boundary writes a fact |
| 14 | JSON the compiler itself emits | At least eight hand-rolled escapers | `WatchService.rs:133`, `WebHost.rs:703` + 6 more | Two are buggy: invalid JSON on control chars, silent data mangling |

Rows 1, 5, and the fact ledger are the law done right, three times, separately. Rows 2-14 are the law missing, drifted, or hand-rolled.

## The grid

Four times a value can cross, four questions every crossing must answer. Ratified mechanisms in plain text; **gaps in bold**. The ballots below are exactly the bold cells.

| When | What crosses | Schema named by | Checked by | Evolves by | Fact left |
|---|---|---|---|---|---|
| **comptime** | a literal | typed head (D-UNIFYLIT1) — **URL/Path/DateTime missing** | E0152-style head check — **escapes fight the head** | — (source edits) | none needed — source is already trusted |
| **build** | manifest, dependency | manifest vocabulary (D-CONF); lockfile | E1204 hash check | editions (`"2026"`→`"2027"`) | **hash logged, provenance unreadable, attestation unenforced** |
| **link** | foreign signature | binder descriptor (D-FFI-UNIFY1) | bind check per language | **none — rebind and pray** | effect leaf `=[FFI.Java]=>` (ROOTS1) — **no undo contract for #Transact** |
| **run** | wire data | `#Codable` type | `FieldError` list (D-VALIDATE-DECODE1) | `migration { }` (D-MIGRATE1) — **unknown fields dropped** | **no origin fact; decode does not clear taint** |

One more cell hides in plain sight: **the schema itself can cross.** A C++ header crossing into Jet is `jet inspect bind cpp` — a binder that reads a foreign schema and writes a Jet module. A JSON sample, a CSV header row, and a SQL DDL file are foreign schemas too. Today only languages get binders. That asymmetry is D-BOUND-BIND1.

## The proposal

### 1. The law itself — D-BOUND-LAW1 *(new)*

Adopt the sentence as spec law: **nothing foreign becomes Jet silently — every crossing names its schema, and every crossing leaves its fact.** The grid above goes in the spec. Every future boundary feature must name its cell; a feature that needs a fifth column or a fifth row is a design smell to kill early, exactly like I8 for mechanisms.

This is the cheap ballot with the long reach: it converts "where should streaming AVRO support live?" from a design debate into a table lookup.

### 2. Finish the head row: URL, Path, DateTime — D-BOUND-HEAD1 *(new; rides ratified D-UNIFYLIT1 + D-DOTCTOR3 + #1577)*

The compiler already checks literal SQL, HTML, shell, regex, and byte patterns at compile time. A literal URL — known just as completely at compile time — is parsed at run time and unwrapped with a panic.

Before — the shipped pattern (shape from `examples/features/net/url_mime.jet`):

```jet
base :: url.parse("https://api.example.com/v2") ?? panic("bad url")
```

After, proposed:

```jet
base :: URL.{"https://api.example.com/v2"}          // typo => compile error, no Result, no panic
log  :: Path.{"/var/log/app.log"}
t0   :: DateTime.{"2026-08-07T12:00:00Z"}
```

The hole law comes free, and it is the same law typed text already has: a hole is one safely-encoded item of the head's grammar. SQL hole = bound parameter. HTML hole = escaped text. Sh hole = one argv item. **URL hole = percent-encoded segment. Path hole = one path component (no `..` smuggling).**

```jet
user :: "ada lovelace/../etc"
u :: URL.{"https://api.example.com/users/{user}"}    // hole percent-encodes: no path escape
p :: Path.{"/data/{user}.json"}                      // hole is ONE component: `..` cannot climb
```

Rungs: beginner types the literal and gets the check without asking. Intermediate keeps runtime strings through the existing `url.parse` / `Path` constructors — dynamic input never touches heads. Expert audits elaborated values with `jet inspect` the same way SQL heads are inspected today. No rung changes the lowest rung.

Respects D-CORE-PATH1 (#1577): strings stay accepted everywhere `Path` is; the head is the checked literal spelling, not a gate.

### 3. Heads own their escapes — D-BOUND-RAW1 *(amends the D-UNIFYLIT1 head-body lexing; the 4-entry escape law stays for plain strings)*

Today the plain-string escape table runs before the head grammar ever sees the text, so the two checked-text systems fight:

```jet
digits :: Regex.{"\\d+"}          // today: double backslash or E0001
win    :: "C:\\logs\\app"          // today: same fight, and no head to help yet
```

Proposed: inside a `Type.{"…"}` body, backslash is a literal character — the head's own grammar owns every escape. RAW1 changes backslash handling only. Hole lexing is untouched, and each head's hole policy stays its own ratified law — Regex keeps refusing interpolation entirely (D-REGEX-LIT1=D, E0152), named here so nothing shifts silently.

```jet
digits :: Regex.{"\d+"}
win    :: Path.{"C:\logs\app"}     // with HEAD1
```

Plain `"…"` strings keep the 4-entry table unchanged — beginners see no difference anywhere. This also gives Jet the raw-string capability it currently lacks without minting an `r"…"` spelling: raw text is what a head body always was.

### 4. Users declare heads; sinks are just types — D-BOUND-SINK1 *(new surface on the ratified D-META marker rail; implements the half deferred by the 2026-07-28 type-unification audit F9)*

The head list is closed: SQL, HTML, Sh, Regex, plus binary patterns. D-META-DSL1=A already ratified that a library may declare a block language with `marker … on [.Block] { check …; add … }` — ratified, unbuilt (#1508). The same rail, pointed at text, opens heads. One prior direction is amended openly here: the 2026-07-28 type-unification audit's F9 note recorded "types, not markers" for future user heads, before D-META-DSL1 ratified the marker rail for the sibling job. The ballot names that divergence and lets the owner pick either shape:

```jet
// library code — proposed spelling, option A
marker Selector on [.Text] {
    check css.parse(@body)?              // comptime: bad selector = compile error
    hole  css.escape(@value)             // the hole law for this head
}
```

```jet
// user code — identical shape to SQL today
row :: Selector.{"#cart > .item[data-id={id}]"}
```

And the concept "sink registration" dies, because a sink was never a mechanism — it is a parameter type:

```jet
fn query(q: SQL) { … }          // only a checked head or an audited .raw() can construct SQL
fn style(s: Selector) { … }     // user sinks fall out for free, zero new machinery
```

Rungs: beginner uses stdlib heads and never declares one. Library author declares a head with one marker. Expert audits every `.raw()` escape hatch in the gate ledger. Kill-check passed: this deletes a planned mechanism (sink registry) rather than adding one.

### 5. `jet inspect bind` eats schemas, not just languages — D-BOUND-BIND1 *(new; generalizes the shipped binder pattern; respects D-NAME-FILES1=C)*

Jet already answers "how does a foreign schema become Jet types" — for languages. `jet inspect bind cpp` reads a clang AST and writes a Jet module with content-addressed provenance. A JSON sample is a smaller foreign schema than a C++ header. The same inspect surface handles both kinds of schema:

```sh
jet inspect bind json fixtures/repo.sample.json --type Repo
```

writes `bindings/repo.jet` — a file you read, commit, and own:

```jet
// generated by: jet inspect bind json fixtures/repo.sample.json  (sha256:9f2a…)
#Codable
struct Repo {
    name: String
    stars: Int
    owner: RepoOwner
}
```

Same verb for `csv` (header row), `sql` (DDL), `xml`, `proto`. This is F#-type-provider power with the brittleness removed: the sample is checked in, generation is an explicit command, and drift shows up as an ordinary diff on regeneration — not a CI network failure. F# proved the value and the failure mode; the binder pattern already avoids the failure mode.

The three exits every magic owes: **see it** — the generated file is on disk with its source hash in the header, and `jet inspect bind json` shows the derivation. **Spell it** — write the `#Codable` struct by hand; the output is ordinary Jet with nothing generated-only about it. **Refuse it** — never run the command; nothing runs it for you (D-NAME-FILES1=C stays law: no invisible discovery).

### 6. Decode clears taint — D-BOUND-TAINT1 *(new connection between two ratified planes: D-FACT-FLOW1/HOME1 taint+provenance rows, and D-VALIDATE-DECODE1 decode)*

The fact plane (ratified 2026-08-07) gives Jet one flow store where taint spreads silently and one gate word, `#Scrub`, where it clears. What nothing ratified yet says: **who writes the first taint fact, and why a decode is not a scrub.** This ballot answers both with the lesson that killed Perl taint and Ruby `$SAFE` twice: laundering must construct a value that could not have been built from garbage — which is exactly what a typed decode is.

- Core boundary functions — `net.*`, `fs.read*`, `env.*`, `process.*` output, FFI marshalling — write an origin fact on what they return: `@origin.net`, `@origin.fs`, `@origin.ffi`.
- The fact spreads through ordinary code silently, free of ceremony, exactly as D-FACT-LAW1=B already rules (facts tighten silently; gates loosen loudly).
- A successful typed construction clears it: `json.decode<Config>` — the value that comes out has the shape `Config` promised, so the origin fact has done its job. `#Scrub` stays as the expert word for hand-written sanitizers, per D-TAG-SURFACE1.
- The audited escape hatches refuse tainted raw text: `SQL.raw(s)`, `HTML.raw(s)`, `Sh.raw(s)` on an `@origin.*` string is a compile-time error naming the flow path.

Beginner writes this, types nothing new, and is injection-safe end to end:

```jet
body :: net.get(endpoint)?               // body: String, fact @origin.net — invisible
cfg  :: json.decode<Config>(body)?       // decode succeeds => origin fact cleared
q    :: SQL.{"select * from t where name = {cfg.name}"}   // fine: cfg is shaped data
```

The one program the compiler now stops — the injection every taint system was invented for:

```jet
body :: net.get(endpoint)?
q :: SQL.raw(body)          // error: @origin.net text reaches an unaudited sink
                            // fix: decode it, match it, or gate the fn with #Scrub
```

The three exits: **see it** — `jet inspect gates` already ledgers every scrub site (ratified); origin facts join the same listing. **Spell it** — `#Scrub(origin)` on your own sanitizer. **Refuse it** — one typed-settings row on the D-CONF rail turns origin seeding off project-wide (proposed spelling `settings: { origin_facts: off }`; the final key is the owner's).

Kill-check passed: zero ceremony on the beginner path (decode was already the thing to do), no new plane (two ratified planes connected), and the expert keeps the full ledger.

### 7. `#Transact` refuses what it cannot undo — D-BOUND-UNDO1 *(amends the E0746 scope set in D-TXN2's implementation; rides ROOTS1's `FFI` root)*

Today `is_irreversible_effect` names exactly `Net | FS | Exec` (`Effects.rs:594-596`). A call into a live Java or Perl runtime inside `#Transact` — which can charge a card, send mail, delete files — is silently treated as rollbackable.

```jet
#Transact(order) {
    inventory.reserve(item)?
    charge_card(order)?     // =[Java, Net]=> in its signature — compiles; rollback cannot un-charge
}
```

Proposed: the `FFI` root joins Net/FS/Exec under E0746 — irreversible by default, because the compiler cannot see the foreign body. The new expert rung is a declared undo contract on the binding, riding the ratified `Rollback` trait law (D-ROLLBACK-TRAIT) instead of a new mechanism:

```jet
// binding declares its inverse; #Transact registers it like any on_rollback
#Undo(refund_card)
fn charge_card(order: Order) =[FFI.Java, Net]=> Receipt
```

With `#Undo`, the call is legal inside `#Transact` and the rollback path calls the inverse. Without it, the fix is the same as for Net today: hoist the call out of the block, or declare the contract. Beginners lose nothing — they were losing money, silently.

### 8. Published schemas keep what they do not recognize — D-BOUND-EVOLVE1 *(extends ratified D-MIGRATE1-4; protobuf's one proven evolution rule)*

Protobuf's evolution story works for one reason: unknown fields survive the round-trip. Jet's `#PublishedSchema` types have the migration chain but not the preservation — decode v2 data with a v1 binary, re-encode, and the v2 fields are gone. That makes rolling deploys corrupt data through the oldest replica.

Proposed: `#PublishedSchema` types preserve unknown fields across decode→encode by default. They are the wire-stability tier; that is what the marker means. Plain `#Codable` types keep today's cheap drop — no hidden holder on every struct. `#DenyUnknownFields` stays the strict refusal, unchanged, giving the full spectrum: refuse / drop / preserve, each one word.

```jet
#[PublishedSchema, Codable]
struct UserRecord { id: Int, display_name: String }

// v1 binary receives v2 data { id, display_name, verified: true }
rec :: json.decode<UserRecord>(wire)?     // verified: unknown, PRESERVED
json.encode(rec)                          // verified survives the round-trip
```

### 9. Provenance gets a surface and a default — D-BOUND-PROV1 *(implements ratified D-JPK-TRUSTROOT1=D; decides only the enforcement default and the read surface)*

The trust machinery is typed and ratified — TUF roles, hybrid publisher identity, SLSA attestation — and verification is not live, with no way to see any of it. Provenance nobody can read is provenance nobody has. Everything below is proposed; today this command does not exist.

```sh
$ jet inspect provenance textkit          # proposed
textkit 1.2.0
  integrity    sha256:4be1… — matches .jet/lock            (enforced, E1204)
  transparency logged 2026-08-01, registry log #48122      (verified)
  publisher    ed25519:ak3f… "textkit team"                (verified)
  build        slsa v1.0 — github.com/acme/textkit@8c00d1  (recorded)
```

The default the ballot decides: integrity stays enforced always (shipped), everything above it is **verified and shown when present, required only on request**. The expert requirement lives in the manifest's ratified authority block (D-AUTHORITY-MANIFEST1=A, #1570), whose trust vocabulary today is `trust: { default, ci, services }` with allow/prompt/deny values. The `require:` row below is a **named amendment** — new vocabulary inside that ratified block, and the same row is the refusal switch:

```jet
authority: .{
    trust: { require: attested }      // proposed amendment: fail resolve on unattested deps
}
```

### 10. The compiler eats the law — defect cards, no ballots

- One JSON writer in `jet-foundation`; delete the eight-plus escapers (two of which emit broken JSON today) and the ~40 `format!`-built JSON sites that feed dashboards and diagnostics.
- Delete the second JSON parser (`jet-pkg-model/src/JSON.rs`); route through the foundation reader.
- `include!` the `EncodingFormat`/`EncodingError` type definitions into the JIT like every sibling module already does with the algorithms — the hand-copied enum is a silent I9 drift trap.
- Existing open cards already own the rest: BindError unification (D-FFI-UNIFY1 slate), the four hand copies of `datatree_kind` (#1628), Py/Octave missing effect roots, the 49 pre-existing corelib failures (#1618, many root causes), exact JSON numbers (#1395, blocked on #1436).

## The ladder

One table, whole model, every rung opt-in, and no upper rung changes what the rung below it does.

| Rung | You type | You get |
|---|---|---|
| nothing | `json.decode<Config>(body)?` | shape check, one error list, migration chain, origin taint cleared — all invisible |
| a literal | `URL.{"https://…"}` | compile-time check, safe holes, no Result ceremony |
| a marker | `#[PublishedSchema, Codable]` | evolution: migration steps + unknown-field preservation |
| a command | `jet inspect bind json sample.json` | a visible, ownable Jet module from a foreign schema |
| a declaration | `marker Selector on [.Text] { … }` | your own head, your own hole law; sinks by parameter type |
| a contract | `#Undo(refund_card)` on an FFI fn | foreign calls legal inside `#Transact` |
| a gate | `#Scrub(origin)` / `.raw("…")` | hand-audited laundering, every site in `jet inspect gates` |
| the authority | `authority: .{ trust: { require: attested } }` | you decide what bytes may enter the build |

## The final vision

The same small service, today and proposed. Every changed line is marked.

```jet
// ============ TODAY ============
use core.encoding.json as json
use core.url as url
use core.net as net

#Codable
struct Repo {                         // hand-transcribed from API docs, drifts silently
    name: String
    stars: Int
}

fn sync(base: String, db: DB) =[Net, Java, IO]=> Result<(), Error> {
    endpoint :: url.parse("https://api.example.com/v2") ?? panic("bad url")
    body :: net.get(endpoint)?        // origin: nobody knows
    repo :: json.decode<Repo>(body)?  // unknown fields silently dropped
    #Transact(tx) {
        db.run(SQL.{"insert into repos values ({repo.name}, {repo.stars})"})?
        notify_legacy(repo)           // Java call — rollback cannot undo it, compiles anyway
    }
}
```

```jet
// ============ PROPOSED ============
use core.encoding.json as json
use core.net as net
use bindings.repo                     // from: jet inspect bind json fixtures/repo.sample.json  [BIND1]

fn sync(db: DB) =[Net, FFI.Java, IO]=> Result<(), Error> {      // one FFI root [ROOTS1, ratified]
    endpoint :: URL.{"https://api.example.com/v2"}              // compile-checked      [HEAD1]
    body :: net.get(endpoint)?                                  // fact: @origin.net    [TAINT1]
    repo :: json.decode<Repo>(body)?                            // decode clears taint  [TAINT1]
                                                                // unknowns preserved   [EVOLVE1]
    #Transact(tx) {
        db.run(SQL.{"insert into repos values ({repo.name}, {repo.stars})"})?
        notify_legacy(repo)           // legal only because notify_legacy declares #Undo [UNDO1]
    }
}
```

And the end state as one tree — the whole audit area under one law:

```
one crossing law: name the schema, leave the fact
│
├─ comptime — the literal crosses
│    ├─ stdlib heads     SQL.{}  HTML.{}  Sh.{}  Regex.{}  [U8].{}     (shipped)
│    ├─ new heads        URL.{}  Path.{}  DateTime.{}                  [HEAD1]
│    ├─ head-owned text  Regex.{"\d+"}                                 [RAW1]
│    └─ user heads       marker X on [.Text] { check…; hole… }         [SINK1]
│
├─ build time — the manifest and the bytes cross
│    ├─ hash integrity   E1204 + .jet/lock                             (shipped)
│    └─ provenance       jet inspect provenance · trust: { require: }  [PROV1]
│
├─ link time — the foreign schema crosses
│    ├─ language binders jet inspect bind cpp / java / …                (shipped, unifying)
│    ├─ format binders   jet inspect bind json / csv / sql / xml / proto [BIND1]
│    └─ undo contracts   #Undo(inverse) => legal in #Transact          [UNDO1]
│
└─ run time — the wire data crosses
     ├─ shape            #Codable + FieldError list                    (shipped)
     ├─ evolution        migration { } + unknown-field preservation    [EVOLVE1]
     ├─ exactness        raw number token => Decimal                   (#1395, ratified)
     └─ origin           @origin.* seeded at the boundary,
                         cleared by decode, gated at .raw()            [TAINT1]
```

## What this unlocks

- **Web services**: injection-safe by default — net input is origin-tainted, decode launders, heads encode holes; the beginner path is the safe path with zero annotations.
- **Data work**: `jet inspect bind csv data.csv` and you have typed rows in one command; exact decimals survive JSON (#1395); rolling deploys stop eating fields (EVOLVE1).
- **Enterprise integration**: the `=[FFI.Java]=>` story becomes tellable — typed binders, undo contracts in transactions, and a provenance chain an auditor can read. This is the scalability story the owner named.
- **Critical systems**: `trust: { require: attested }` plus the gate ledger gives a full audit surface: every dependency's chain, every scrub, every raw escape, one `jet inspect` away.
- **One-liners**: nothing changed — `json.decode<Config>(text)?` was already one line; now it also clears taint.

## What stays

- `DataTree` as the one dynamic tree with string-keyed accessors (D-SERDE-ACCESS=B, archived record; the broader D-SERDE slate) — dynamic access is the honest spelling for schemaless exploration.
- The 4-entry escape table for plain strings — small on purpose; RAW1 touches head bodies only.
- `#DenyUnknownFields`, field markers, the whole shipped Codable surface — EVOLVE1 adds a tier, changes none.
- `env.jet` as an evaluated module — it is not a data format, it is a program; the law does not apply to source.
- The String ledger outcome (D-STR-DECLINE1=C) — the four high-frequency names shipped directly, the rest routed to existing mechanisms; nothing here adds String methods or reopens the routing.
- `#Unsafe("reason")` free-prose reasons — the reason is for the human auditor; structuring it bought nothing in review.

## Decisions for the owner

Each ballot stands alone; any subset can be adopted. Full profiles and options are on the Tower card.

| Ballot | Asks | Touches ratified law |
|---|---|---|
| D-BOUND-LAW1 | Adopt the crossing law + grid as spec law | records D-UNIFYLIT1, D-VALIDATE-DECODE1, D-MIGRATE1, D-FACT-*, D-JPK-* as its instances |
| D-BOUND-HEAD1 | URL/Path/DateTime typed heads with the shared hole law | rides D-UNIFYLIT1, D-DOTCTOR3, D-CORE-PATH1 — no amendment |
| D-BOUND-RAW1 | Head bodies own their escapes (backslash literal) | **amends** D-UNIFYLIT1's body lexing; hole policy per head unchanged (D-REGEX-LIT1 stays); plain strings untouched |
| D-BOUND-SINK1 | User-declared heads on the marker rail; sinks = parameter types | extends D-META-DSL1 from `[.Block]` to `[.Text]`; diverges openly from the F9 audit note ("types, not markers") — both shapes on the ballot |
| D-BOUND-BIND1 | `jet inspect bind` accepts data schemas (json/csv/sql/xml/proto) | rides the shipped binder pattern; respects D-NAME-FILES1=C |
| D-BOUND-TAINT1 | Boundary fns seed `@origin.*`; typed decode clears it; `.raw()` gates it | connects D-FACT-FLOW1/HOME1 with D-VALIDATE-DECODE1 — no amendment |
| D-BOUND-UNDO1 | FFI joins E0746; `#Undo(inverse)` legalizes it in `#Transact` | **amends** the D-TXN2-era irreversible set; rides D-ROLLBACK-TRAIT |
| D-BOUND-EVOLVE1 | `#PublishedSchema` preserves unknown fields by default | extends D-MIGRATE1-4 |
| D-BOUND-PROV1 | Provenance read surface + verified-not-required default | implements D-JPK-TRUSTROOT1=D; **amends** D-AUTHORITY-MANIFEST1's trust block with a `require:` row |

## Implementation shape

**Phase A — re-found, no surface change, all tests green.** One JSON writer; delete the six escapers and the duplicate parser; `include!` the JIT enum; land the D-FFI-UNIFY1 descriptor so the 17 `BindError`s collapse; fix the 49 red codec tests (#1618). Pure deletion and repair.

**Phase B — land ratified-but-unbuilt work on the substrate, built once.** ROOTS1's one `FFI` root (#1567, + the missing Py/Octave roots), the fact-plane flow store (#1621-#1624), exact JSON numbers (#1395, blocked on #1436; the #1394 edition split already shipped), D-META-DSL1 block markers (#1508). These are the rails the ballots above stand on.

**Phase C — the balloted surface, each a coherent greenfield migration.** HEAD1+RAW1 together (one lexer/sema change, examples + goldens migrate in the same change). SINK1 after #1508. BIND1 as a `jet inspect bind` extension. TAINT1 after #1621. UNDO1 after #1567. EVOLVE1 and PROV1 independent.

**Epoch-3 reconciliation (after ratification).** The audit card carries this as exit criteria: mint one implementation card per adopted ballot; re-home or close the overlapped cards (#1567, #1570, #1577, #1628, #1618, #1395); add the Phase-A defect cards (JSON writer, duplicate parser, JIT enum); check no e3 card still plans against the pre-law shape. One board, one plan.
