# Core library overhaul: API doctrine and structure

Proposal, 2026-08-06. Two parts. Part A rebuilds the Core API design rules from
first principles. Part B restructures the library and defines the prelude.
Evidence lives in three cited research reports:

- [API usage frequency across languages and segments](../research/corelib-api-usage-frequency-2026-08-06.md)
- [Lauded stdlib designs](../research/corelib-lauded-designs-2026-08-06.md)
- [Prelude scope across languages](../research/prelude-scope-across-languages-2026-08-06.md)

Open owner choices are in DECISIONS at the end. Everything else implements
existing ratified law.

## Glossary

- **Prelude** — names available with zero imports.
- **Option enum** — a dedicated enum passed as a trailing labeled argument to
  vary one operation (`round(x, mode: .Down)`).
- **Magic default** — the behavior a call has when the caller passes only the
  required arguments. Safe, common-case, documented.
- **Opt-out** — an expert passing an option to replace a magic default.
- **Agreement set** — the operations every segment (beginner, education,
  scripts, business, enterprise, data, ops) independently uses most. Measured
  in the frequency report.
- **Whole-value call** — one call that does the full common job
  (`fs.read(path)`), layered over streaming primitives.

## The stance

The frequency report shows API usage is Zipf-distributed: about 1% of surface
takes about 80% of use. The agreement set is small and identical across every
segment. So the design bet is: make that small set frictionless and safe with
zero ceremony, and put everything else one obvious, shallow step away.

The owner's flip states the mechanism: **experts opt out of magic; beginners
never opt in to safety.** Every language the research lauds already works this
way in its best corner (requests, pathlib, f-strings, Kotlin defaults, Swift
labels). Every notorious failure broke it (urllib, os.path, Java Date,
`public static void main`).

---

# Part A — API doctrine

## One example first

The job: fetch a user from an API, keep the name, save it to a file.

Beginner spelling — every default is magic, every failure is visible:

```jet
use core.[files as fs, http]

fn run() ? Error {
    user :: http.get("https://api.example.com/users/7")?.json<User>()?
    fs.write("user.txt", "Saved {user.name} at {Clock.now()}")?
    print("done")
}
```

Expert spelling of the same calls — same functions, options replace magic:

```jet
resp :: http.get(url,
    timeout: 5.seconds,
    redirects: .None,
    retries: .Idempotent,
)?
fs.write(path, text, mode: .Atomic, permissions: 0o600)?
```

Nothing changed shape. The expert added trailing labeled options; the
beginner never saw them. Every doctrine rule below is visible in this example.

## The rules

Rules are grouped. Each rule names its evidence anchor in the research
reports. Existing ratified laws L1–L8 (`docs/spec/stdlib-api-laws.md`) stay in
force; the table at the end of Part A shows what each new rule extends.

### Call site rules

- **C1 — Optimize the call site, not the declaration.** An API is declared
  once and read forever. Judge every signature by how its calls read in real
  programs. (Swift guidelines; lauded R1.)
- **C2 — Calls read as sentences.** Required arguments are positional; the
  LSP shows their names inline. Labels appear where a bare value would be
  ambiguous, and label-only zones guard rarely-passed options. This is
  ratified D-APILABEL1; the doctrine makes it the review test: read the call
  aloud without the docs. (Lauded 1.1.)
- **C3 — Dataflow reads left to right.** Method chains and `?` keep the happy
  path linear. No API may force inside-out nesting for its common use.
  (Elixir pipes, pathlib chains; lauded R10.)

### Default and option rules

- **D1 — The common case takes zero configuration.** The bare call does the
  safest most-common thing the agreement set demands. If the stdlib call is
  more ceremony than the ecosystem's favorite wrapper, the stdlib call is
  wrong. (requests over urllib; frequency §7; lauded R2, R21.)
- **D2 — Every magic default is named, documented, and overridable.** The
  docs for each function list its defaults in one table: what the magic does,
  and which option replaces it. An expert can audit every default without
  reading source. (Rails CoC caveat; lauded 1.9.)
- **D3 — Defaults, never overload families.** One function with defaulted
  labeled options replaces N near-duplicate functions. Multi-head dispatch
  (S83) is for genuinely different input shapes, not for option variations.
  (Swift "surpasses four overloads"; Kotlin; lauded R3.)
- **D4 — Options are enums, never booleans or bare strings.** Every option is
  a dedicated enum passed by label with dot-shorthand: `parse(text,
  on_error: .Lenient)`. Boolean and stringly options are banned in Core.
  (Boolean-trap literature; lauded R4, anti-pattern A9.)

### Failure rules

- **F1 — Failure lives in the signature; propagation costs one character.**
  `T ? E` plus `?` is the whole story. This is ratified law (L2); the
  doctrine adds the Go lesson as a hard test: if a pattern must be written at
  most call sites, its ceremony must be one character, or the pattern is
  wrong. (Rust `?` vs `if err != nil`; lauded 1.2, R6, A6.)
- **F2 — Lookups return `T?`, never sentinels.** No `-1`, no empty-string
  flag values, no "check a status field afterward." (Lauded R17.)
- **F3 — Every failure message is a product.** Ratified I4/L6, restated as
  doctrine because the education evidence shows diagnostics are what make a
  safe language learnable. (Stefik; lauded R22.)

### Type rules

- **T1 — Domain values get types with algebra.** Paths, URLs, durations,
  instants, colors, sizes are types, not strings or bare ints. Functions that
  accept them also accept the obvious literal via a union (`String | Path`)
  so beginners never construct wrapper types by hand. (pathlib lesson; lauded
  R5; DECISIONS D-CORE-PATH1.)
- **T2 — Core values are immutable.** Times, dates, strings, options: values,
  not mutable handles. Mutation is a container property, not a value
  property. (Java Date/Calendar failure; Temporal; lauded R14.)
- **T3 — Distinct concepts get distinct types; a blessed simple entry sits on
  top.** `Instant` vs `Date` vs `DateTime` vs `Duration`, with `Clock.now()`
  and `Date.today()` as the beginner doors. (java.time/Temporal consensus;
  lauded 1.8, R15.)

### Naming rules

- **N1 — One naming grammar, enforced by review.** Extends L1. Subject first,
  same word order everywhere, S66 acronyms, no PHP-style drift. The rubric
  check is mechanical: a new name must be predictable from existing names of
  the same kind. (PHP anti-pattern A3; lauded R16.)
- **N2 — Name by side effect, in systematic pairs.** No side effect: noun or
  past participle (`sorted()`, `union(other)`). Side effect: imperative verb
  (`sort()`, `add(x)`). Every mutating/nonmutating pair follows the same
  spelling rule library-wide. (Swift sort/sorted; lauded R7.)

### Layering rules

- **L-A — Whole-value calls over streaming seams; both always exist.** Every
  I/O domain ships the one-call form (`fs.read`, `http.get`) on top of the
  streaming form (`fs.open`, request builder). Either alone fails half the
  spectrum. Extends L5's streaming requirement with the beginner half.
  (Go io + pathlib read_text; lauded R12; frequency §4.)
- **L-B — Laziness is explicit and same-shaped.** Collection adapters on
  concrete containers are eager; `.lazy` opts into the identical vocabulary
  as a lazy view. Silent deferral is banned — it is the LINQ
  multiple-enumeration trap. (Elixir Enum/Stream, Kotlin sequences; lauded
  R11; DECISIONS D-CORE-EAGER1.)
- **L-C — One small protocol funds the surface.** New container types
  implement the iteration protocol and inherit the entire adapter vocabulary.
  No container-specific method sets. (Ruby Enumerable, Rust Iterator; lauded
  R8, R9.)
- **L-D — Compose pieces; presets for beginners.** Expert surface is small
  orthogonal parts. Beginner surface is named presets built from those parts
  (`Limits.safe()`), never a parallel implementation. (SwiftUI progressive
  disclosure; lauded R19; invariant I8.)

### Evolution rules

- **E1 — Replace, never accumulate.** Greenfield law restated for APIs: a
  superseded spelling is deleted in the same change. os.path-style permanent
  duplication is the measured cost of skipping this. (Lauded R20, A4.)
- **E2 — Absorb the gap-fillers on day one.** A billion-download third-party
  library in a peer ecosystem is a measured stdlib gap; Core ships that
  capability with the wrapper's ergonomics, not the plumbing it wrapped.
  Current absorbed list: humane formatting, terminal color, declarative args,
  uuid, structured logging, assertions with rich diffs. (Frequency
  cross-cutting §2.)

### Relation to ratified laws

| Ratified | Doctrine | Effect |
|---|---|---|
| L1 naming | N1, N2 | extends: grammar test, side-effect pairs |
| L2 fallibility | F1, F2 | extends: one-character test, sentinel ban |
| L3 ownership | T2 | unchanged; values immutable |
| L4 effects | — | unchanged |
| L5 allocation | L-A, L-B | extends: whole-value layer, eager default |
| L6 diagnostics | F3 | unchanged, restated |
| L7 examples | — | unchanged |
| L8 one way | D3, L-D, E1 | extends: defaults not overloads, presets not forks |
| (new ground) | C1–C3, D1, D2, D4, T1, T3, E2 | new laws |

---

# Part B — Structure, prelude, namespaces

## Prelude policy

Membership criteria, derived from the prelude report §4 and adopted as law:

1. **Measured direct frequency.** The name is called directly in most real
   programs across segments (agreement set membership).
2. **Total and safe.** Nothing partial, nothing panicking, nothing that can
   fail invisibly. Haskell's `head` is the permanent cautionary tale.
3. **Names, never semantics.** A prelude entry adds a name. It never changes
   resolution, conversion, or overloads of existing code.
4. **No better home.** If the natural spelling is a method or an operator,
   the free name stays out (`len` is `.len()`; power is `^`).
5. **First-hour coverage.** Lesson one through the first real script needs no
   import.
6. **One fixed set.** Not user-extensible. Every Jet file shares one ambient
   vocabulary.
7. **Collision-conscious names.** Avoid names programmers reach for as
   variables.

Mechanics (prelude report §5):

- **Shadowing: user wins, compiler warns.** A user definition beats a prelude
  name, and the compiler says so. Python's silent shadowing and Go's silent
  precedence both lose.
- **Epoch-gated growth.** New prelude names land only at an epoch boundary
  with an automatic migration lint. (Rust RFC 3114 mechanics.)
- **Demote early and loudly** when a prelude entry proves wrong; never leave
  a known-bad name ambient.

## The prelude list

Jet's method-call surface does most of the agreement set's work without any
prelude cost: split/join/trim/contains, map/filter/sorted/sum, `.len()`,
`.parse()` are methods on types, and interpolation and ranges are syntax. The
free-name prelude stays small:

**Already ambient (unchanged):** built-in types (`Int`, `Float`, `Bool`,
`String`, `[T]`, `[K: V]`, `Set`), `Val`/`None`/`Ok`/`Err`, `print`, `panic`,
`?`, `??`, `?.`.

**Add — uncontested by criteria (still owner-gated as one slate,
D-CORE-PRELUDE1):**

| Name | What | Criterion |
|---|---|---|
| `eprint(value)` | print to stderr | pairs with `print`; scripts segment |
| `input(prompt?)` | read one line from stdin | the only name beginners add (frequency §segment) |
| `assert(cond)` / `assert_eq(a, b)` | rich-diff assertions, `#Test` and script use | testing evidence: pytest/testify model |
| `Clock`, `Date`, `Duration`, `Instant` | time type names + `Clock.now()`, `Date.today()` | T3 blessed doors; time is Tier 2 universal |
| `Path` | path type name (T1) | agreement set: paths |

**Contested — separate ballot (D-CORE-PRELUDE2), because criteria conflict:**

| Candidate | For | Against |
|---|---|---|
| `read_file` / `write_file` | Python `open` precedent; first-script coverage; SO friction evidence | effectful ambient names; `use core.files as fs` is one line; criterion 4 |
| `random(range)` / `shuffle` / `choice` | CS1 uses random in week one; Rust's exclusion bred a universal crate | effectful; seeding policy; criterion 4 — `use core.math.random` (or list import) is one line |

Recommendation recorded in DECISIONS. Everything else imports.

## Namespace tree

### Ceremony model (Jet-specific)

Jet has no glob import of names into scope (`use math.*` is E0612). A
module import binds a **prefix**:

```jet
use core.files as fs

fn run() ? Error {
    text :: fs.read("data.txt")?
    fs.write("out.txt", text.to_upper())?
}
```

Two taxes on every non-prelude API:

1. **Import tax** — one `use` line per module (mitigated by grouped module
   import below).
2. **Prefix tax** — every call repeats the alias. Aliasing already makes
   nesting cheap at the call site: `use core.math.random as random` and
   `random.int(1..6)` pay the same prefix as a root `core.random`.

So: prefer a **consistent tree** over one-off short doors. Do not invent
`core.json` beside `core.encoding.json`, and do not rename `core.files` to
`core.fs` — callers write `as fs` when they want the short prefix.

**Ceremony rules:**

- **Cer1 — Frequency buys frictionlessness.** Agreement-set work goes to
  prelude or to a parent re-export (`http.get`), not to parallel short
  spellings of the same module.
- **Cer2 — Alias erases nest depth at the call site.** Nest for one
  question per namespace; teach the path once; bind a short alias.
- **Cer3 — Cut import tax with grouped module `use`, not with grab-bags
  or one-off roots.** See D-CORE-USELIST1.
- **Cer4 — Parents re-export the common door.** `use core.http as http;
  http.get(url)` works; `.client` / `.server` are depth when needed.
- **Cer5 — Merge only real co-import pairs** (env+os → `sys`). Never
  args+log+term into `io`.
- **Cer6 — Prefer methods on domain types** (Path, String, Clock) so the
  hottest work pays no module prefix.

### Grouped `use` list (proposed — D-CORE-USELIST1)

One list form. Use square brackets, the same list shape Jet already uses
elsewhere. Put `as` next to a name when you want a shorter local name.

```jet
use core.[files as fs, http, encoding.json, math.random]
// local names: fs, http, json, random
```

**How each name is chosen**

- If you write `as name`, that local name wins.
- If you skip `as`, the local name is the **last part after the final
  dot**.
  - `http` → `http`
  - `encoding.json` → `json`
  - `math.random` → `random`
  - `files as fs` → `fs` (because `as` was written)

You can also hang the list on a longer prefix:

```jet
use core.encoding.[json, csv]
// local names: json, csv
```

Single-module form stays as today: `use core.files as fs`.

**What this replaces for groups**

The retired selective *item* import spelling was a different delimiter
(D-SELIMPORT1). The shipped group form uses the canonical list:

```jet
use math.[sin, cos as c]    // items
use core.[files as fs, http] // modules
```

One delimiter. Same `as` rule. No second list style.

Wildcards (`use core.*`) stay rejected.

**Ideas we are not using** (so the ballot options make sense)

1. **Trailing alias list** — `use core.[files, http] as [fs, http]`.
   Two lists must line up by position. Easy to break when you add or
   remove one name.
2. **Colon pairs** — `use core.[files: fs, http: http]`. Looks like
   building a record. Drops the `as` keyword you already know.

### Owner notes

**1. Reject: `core.io` absorbs `core.args` and `core.log`.** Still a
grab-bag. Split io → prelude + `term` + `process` argv; keep args/log
separate. Grouped `use` covers the multi-import case:

```jet
use core.[args, log, term, process, files as fs]
```

**2. Nesting is fine; consistency beats short-door one-offs.** Restore
domain nesting (`math.random`, `encoding.*`, `net.*`, …). Drop the
proposed `core.json` alias module and the `files`→`fs` rename.

**3. Keep `core.files`.** Canonical name stays readable; `as fs` is the
short prefix when wanted.

**4. Accept `env`+`os` → `core.sys`.** Real co-import pair (Cer5).

**5. Biggest ceremony wins (revised):**

| Win | Why |
|---|---|
| Prelude: print/input/assert/Clock/Path (+ contested file trio) | zero import tax |
| `core.http` re-exports get/post/serve | no `.client` on the common path |
| Grouped module `use core.[…]` | one line for several modules (D-CORE-USELIST1) |
| `env`+`os` → `sys` | one module where scripts co-import both |
| Alias + last-segment default | nesting stays consistent without call-site pain |

### Current API inventory (names only)

Source: `KNOWN_CORE_MODULES` + `core_module_items` (2026-08-06). Names
only — enough to judge overlap and placement. Huge modules are truncated
with a count.

**Script / console**

- `core.io` — Reader, Writer, args, input, confirm, choose, input_secret,
  read_all_input, print, eprint, stdin, stdout, stderr, terminal_width,
  terminal_height, style, style_force, progress
- `core.args` — spec *(+ ArgsSpec, ParsedArgs)*
- `core.log` — info, warn, error, debug, field, int, float, bool, redact,
  *_fields, span, enter, close, set_sink, sample_every, counter, otlp_file,
  set_level, set_trace_id, setup
- `core.term` — read_key *(+ Key)*
- `core.files` — read, read_bytes, write, append_all, exists, remove,
  remove_dir, remove_all, list_dir, create_dir*, copy*, symlink, read_link,
  hard_link, rename, stat, canonicalize, absolute, walk, glob, read_at,
  write_at, fsync, write_atomic, temp_*, lock, open, create, append
  *(~35)*
- `core.path` — join, parent, extension, normalize
- `core.env` — get, set, unset, vars, current_dir, home_dir
- `core.os` — name, family, arch, cpu_count, temp_dir, executable, pid,
  hostname, username, set_current_dir, on_interrupt
- `core.process` — exit, run, cmd, pipeline *(+ ProcessSpec/Child/Result,
  Terminal*)*

**Text / numbers / time**

- `core.text` — Cursor, nfc/nfd/nfkc/nfkd, casefold, caseless_eq, lower,
  upper, graphemes, words, sentences, display_width, scalar_count,
  byte_count, is_*, scalars, splitn, rsplitn, trim*, pad*, center,
  starts_any, ends_any, char_indices
- `core.text.unicode` — scalar_count, byte_count, is_ascii, lower, upper,
  scalars *(overlaps text)*
- `core.fmt` — number, decimal, percent, bytes, duration, ordinal, plural,
  pad_left, pad_right, pad_center
- `core.regex` — flags, compile*, is_match, match, find*, matches,
  replace*, split*
- `core.math` — sqrt, pow, abs, min, max, floor, ceil, round, pi, e, tau,
  trig/hyperbolic, exp/ln/log*, clamp, lerp, checked_*, saturating_*,
  wrapping_*, gcd, lcm, factorial, decimal, fraction, … *(~110)*
- `core.random` — int, float, float_range, bool, normal, exponential,
  pick, weighted_pick, sample, shuffle, seed, rng, split, bytes
- `core.time` — now, sleep, milliseconds/seconds/…, start, instant,
  now_utc, from_unix_ms, today, parse_*, local_time, period*, zone, utc,
  zoned*
- `core.time.date` — new, today, parse
- `core.time.datetime` — from_timestamp, now

**Encoding / data**

- `core.encoding` — DataTree, EncodingLimits/Error/Format/…
- `core.encoding.{json,jsonl,csv,toml,yaml,xml,cbor}` — parse, decode*,
  to_string*/to_bytes*, reader/writer where applicable
- `core.encoding.{hex,base64,base32}` — encode, decode *(+ url variants)*
- `core.data` — csv/json readers, table/rows/series, filter/sort/join,
  group_*/agg, plot helpers, lazy_* *(~45)*
- `core.sketch.{hll,tdigest,reservoir,cms}` — new
- `core.compute` — tensor constructors, matmul, fft, device/stream,
  jvp/vjp/grad_*, serialize *(~70)*
- `core.solve` — Solver
- `core.db` — open*, policy, params, row_*, transaction, migrate
- `core.science.measurement` — from

**Network / app**

- `core.http` — get, post, serve *(promote as the door — Cer4)*
- `core.http.client` — Client, Proxy, RedirectPolicy, get, post, request
- `core.http.server` — bind, mux, serve*, response, tls, sse, static_*,
  cors*, access_log, request_id, json
- `core.net` — ip_*, socket_*, tcp_*, udp_*, unix_*, dns_*, tls_* *(~80)*
- `core.tls` — ClientConfig, RootCertificates, client, read*, write*, close
- `core.ws` — connect, upgrade
- `core.url` — parse, from_parts, file, data, query, percent_*
- `core.mime` — parse, from_extension, extension
- `core.email` — Address, Message, Mailer, smtp*, serialize, …
- `core.browser` — Browser*, connect*, profile, timeout, locked
- `core.web` — app, page, live*, auth*, sync*, storage, …
- `core.ui` — backends, mount, node, text, box, button, …

**Security / concurrency / systems**

- `core.crypto` — Secret, SigningKey, seal/open, sign/verify, password_*,
  hashes, x25519, … ; `core.crypto.random.bytes`; `core.crypto.expert.*`
- `core.vault` — get/current/versions, prepare_*, authorize_*, commit_*,
  export_*, …
- `core.auth` — verify_jwt/paseto, register_user, password_login,
  session_*, magic_link_*, oauth_*
- `core.uuid` — v4, v7
- `task`, `task.all`, `task.race`, `task.any`, `task.group`; `core.tasks` — channel, after, interval
- `core.sync` — text_*/counter_*/map_*/list_*/policy_*
- `core.event` — new, with_policy, hook, decision_hook, scope, …
- `core.reactive` — signal, derived, computed, effect; `.loadable.*`
- `core.services` — tree, worker, group, start/stop, send*, workflow_*,
  directory_*, upgrade_* *(~50)*
- `core.watcher` — files, process_pid, port, set

**Other**

- `core.testing` — snap, golden, fixture, temp_dir, corpus, fake_clock,
  fake_rng
- `core.game` — Scene, Replay, Backend, run
- `core.raylib` — window_*, draw_*, key_down, load_sound, …
- `core.archive` — zip_*, tar_*
- `core.compress.{gzip,zstd}` — compress, decompress
- `core.mem` / `core.mem.alloc` — Ptr, Arena, Bump, Pool, Fixed, …
- `core.scope` — guard
- `core.reflect` — of
- `core.perf` — Perf, fidelity*
- `core.plugin` — load
- `core.compiler` — lex, parse, check, source_map
- `core.lang` — ABI, Capability, FfiLanguage, … *(15 enums)*
- `core.binary` — Reader

Notable overlaps: `print`/`input` (prelude vs io); `args` (argv vs
`args.spec`); `bytes` (random vs crypto.random); path ops (path vs files);
`temp_dir` (os/files/testing); unicode helpers duplicated under text.

### Final proposed tree

Consistent nesting. Alias + grouped `use` carry ceremony. No parallel
short roots. `*` = rename or new nest vs today.

```
core.
├── files                   # keep name; Path methods here (path deleted)
├── term *                  # streams, prompts, style from io
├── args
├── log
├── process                 # run, cmd, exit, argv
├── sys *                   # env + os
├── text
│   ├── (unicode folded in) *
│   └── fmt *               # was core.fmt
├── regex
├── math
│   ├── random *            # was core.random (PRNG)
│   ├── linalg
│   ├── simd
│   └── decimal
├── time                    # Instant/Date/…; date/datetime submodules deleted
├── encoding                # json, csv, toml, yaml, xml, cbor, hex, base64, …
├── http *                  # get/post/serve re-exported on parent
│   ├── client
│   └── server
├── net
│   ├── tls *
│   ├── ws *
│   ├── url *
│   ├── mime *
│   └── expert
├── email
├── data
│   ├── plot
│   └── sketch *
├── compute
│   └── solve *
├── db
├── crypto
│   ├── random              # CSPRNG
│   ├── uuid *
│   ├── vault *
│   └── expert
├── auth
├── tasks
├── sync
├── event
├── reactive
├── services
├── watcher
├── testing
├── game
│   └── raylib *
├── ui
├── web
│   ├── storage
│   ├── devserver
│   └── browser *
├── archive
│   ├── gzip *
│   └── zstd *
├── units *                 # was science.measurement
├── mem
│   └── scope *
├── reflect
├── perf
├── plugin
└── compiler
    └── lang *
```

Worked imports after D-CORE-USELIST1:

```jet
use core.[files as fs, http, encoding.json, math.random]
// fs / http / json / random

user :: http.get(url)?.json<User>()?
fs.write("out.txt", json.to_string(user))?
print(random.int(1..6))
```

Deleted as free namespaces: `core.io`, `core.path`, `core.time.date`,
`core.time.datetime`, `core.text.unicode`, `core.fmt`, `core.random`,
`core.env`, `core.os`, `core.tls`, `core.ws`, `core.url`, `core.mime`,
`core.uuid`, `core.vault`, `core.raylib`, `core.browser`, `core.solve`,
`core.sketch.*`, `core.compress.*`, `core.science.measurement`,
`core.mem.alloc`, `core.scope`, `core.lang`, `core.binary`.

### Disposition table (authoritative)

| Current | Disposition |
|---|---|
| `core.files` | **keep name**; Path type per T1; path free fns → Path methods |
| `core.path` | delete (E1) |
| `core.io` | delete: print/eprint/input → prelude; streams/prompts/style → `term`; argv → `process` |
| `core.term` | keep; absorbs terminal half of io |
| `core.args` / `core.log` | keep separate at root |
| `core.text` / `core.text.unicode` | fold unicode into text |
| `core.fmt` | nest → `core.text.fmt` |
| `core.regex` | keep at root |
| `core.math` | keep; add `.random`, `.linalg`, `.simd`, `.decimal` |
| `core.random` | nest → `core.math.random`; `crypto.random` stays CSPRNG |
| `core.time*` | one `core.time`; delete `.date` / `.datetime` |
| `core.encoding.*` | keep shape; **no** parallel `core.json` short door |
| `core.data` / `core.sketch.*` | nest sketch under data |
| `core.http` / `.client` / `.server` | parent re-exports get/post/serve; submodules for depth |
| `core.net` / `tls` / `ws` / `url` / `mime` | nest tls/ws/url/mime under net |
| `core.email` | keep at root |
| `core.browser` | nest → `web.browser` |
| `core.crypto` / `vault` / `uuid` | nest vault + uuid under crypto |
| `core.auth` | keep at root |
| `core.tasks` / `sync` / `event` / `reactive` / `services` / `watcher` | keep at root |
| `core.process` / `env` / `os` | process stays; env+os → `sys` |
| `core.testing` | keep; assertions → prelude |
| `core.game` / `core.raylib` | nest raylib under game |
| `core.ui` / `core.web` | keep; browser under web |
| `core.db` / `compute` / `solve` | nest solve under compute |
| `core.science.measurement` | rename → `core.units` |
| `core.archive` / `compress.*` | nest codecs under archive |
| `core.mem` / `alloc` / `scope` | fold alloc + scope under mem |
| `core.perf` / `reflect` / `plugin` / `compiler` / `lang` | expert tier; lang under compiler |
| `core.binary` | delete; Reader with encoding/stream seams |

Rust-side `Prelude/` remaps one-to-one to this tree, deleting grab-bag
files (`FSIoEnvOsTesting.rs`, `MathRandomTime.rs`,
`RingCsvLogTimeCrypto.rs`). Implementation detail; carded with migration.

## Migration

Greenfield rules apply: each rename or merge migrates every in-repo consumer
(examples, tests, docs, snapshots) in one coherent change and deletes the old
form. I9 parity per change. Implementation is carded per namespace after
ratification; the tree above is the target, not a phase plan.

---

# DECISIONS

| ID | Question | Options | Recommendation |
|---|---|---|---|
| D-CORE-DOCTRINE1 | Adopt Part A rules C1–E2 as the extended Core API law (supersedes/extends L1–L8 per table)? | A adopt / B adopt minus named rules / C reject | **A** |
| D-CORE-EAGER1 | Collection adapters on concrete containers: eager by default with explicit `.lazy` (amends D-ITERTOOLS1's lazy `Iter` default)? | A eager + `.lazy` / B keep lazy default | **A** — LINQ-trap evidence; beginner-protective; Elixir/Kotlin precedent |
| D-CORE-PATH1 | Introduce typed `Path` with `.join` / `.parent` methods; Core path/file APIs accept `String \| Path`; delete `core.path` free functions (amends "paths are plain String", D-FILES-WRITE1)? | A typed Path / B keep String paths | **A** — pathlib before/after is the clearest lesson; join is methods, not a new `/` operator |
| D-CORE-PRELUDE1 | Adopt prelude policy (7 criteria, user-wins-with-warning shadowing, epoch-gated growth) plus the uncontested additions table? | A adopt / B adopt policy, trim list / C reject | **A** |
| D-CORE-PRELUDE2 | Contested prelude entries | A add file trio + random family / B add file trio only / C add random only / D neither | **B** — files are the top import-tax win; random stays `core.math.random` behind one use (or list entry) |
| D-CORE-TREE1 | Adopt the consistent nested tree (keep `core.files`; no `core.json` short door; nest random/fmt/net leaves/crypto leaves; env+os→sys; delete io; http parent re-exports)? | A adopt / B adopt minus named rows / C reject / D grow `core.io` | **A** |
| D-CORE-USELIST1 | One grouped `use` list with `[]` and in-list `as` (also moves item groups off `{}`)? | A adopt list + migrate item `{}`→`[]` / B modules-only `[]`, keep item `{}` / C trailing `as [aliases]` / D colon pairs / E reject | **A** |
