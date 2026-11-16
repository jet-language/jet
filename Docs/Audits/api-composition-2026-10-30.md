# API composition audit: Core, Jetpack and the build API

Card #6367, criterion 2. Dev `59213de00f`. The principle and its review rules
are in
[`Docs/proposals/api-primitives-and-wrappers-2026-10-30.md`](../proposals/api-primitives-and-wrappers-2026-10-30.md)
(rules P1 to P8).

## Plain summary

- **Scope.** The audit covers all of `Core/` (167 source files), `Jetpack/` (511
  files) and the `fn build` API in the spec. It also covers the Core surface
  ledger: 1,694 rows in 75 modules, of which 514 are `__` hooks, leaving 1,180
  user-facing names. Tests are excluded.
- **The headline.** In 20 Core areas the documented surface is only wrappers.
  The function that does the work is an undocumented helper, usually with
  positional `Bool` or magic `Int` flags. It can be called, because every
  declaration is public unless it sits in a `priv module`, but no reference page
  lists it and nothing makes it stable. So a need just outside a wrapper has no
  supported route: unpadded standard base64, log fields at a level chosen at
  run time, or "replace the first three regex matches".
- **One module copies generic operations for `[Int]` only.**
  `core.math.combinatorics` has 25 list functions that accept only `[Int]`.
  Several take a `Bool` where a predicate belongs. Generic list methods that do
  the same job already exist.
- **Call tools are almost unused.** 100 of Core's 9,067 parameters have a
  default (1.1%). Core has 2 variadics and Jetpack has none. 352 Core and 981
  Jetpack functions take four or more parameters, and Jetpack has 8 defaults in
  total.
- **Twins and name lies.** `files.walk_parallel` is `walk`, `math.float32`
  returns its argument unchanged, `ui.previews` and `ui.playgrounds` are the
  same function, and `crypto.publicdecrypt` takes a secret key.
- **Output.** Eight ranked finding classes (§3, §4), ten proposed cards (§5),
  five bugs with repros (§6). Items already decided or carded elsewhere are
  marked so they are not filed twice.

## 1. Method

| Measure | How | Notes |
|---|---|---|
| Parameter census | A source scanner over every `fn` header (`census.mjs`): defaults, variadics, `*` and `/` zones, external names, `Bool` parameters, arity, sibling-name families | "Public-looking" means no `priv` and no `_` prefix. Core has one `priv module` (`core.db.sql`), so nearly everything is public; the surface ledger is used where "documented" matters. |
| Documented surface | `Compiler/Prelude/CoreSurfaceInventory.jet` `fixed_sig` rows | 1,694 rows; 1,180 without the `__` prefix |
| Constant-forwarding wrappers | A scanner (`wrappers.mjs`) for one-line bodies that call one function with at least one literal argument, grouped by callee and checked against the ledger | 204 wrappers; 27 callees have two or more (128 wrappers); 20 of those callees are not on the ledger |
| Behaviour probes | `jet check` / `jet run` with the published dev compiler on small programs | The repros in §6 and the language-gap rows in F5 |
| Decision cross-check | `stdlib-api-laws.md`, `philosophy.md`, spec "Programmable builds", Tower decisions and cards | Items already ratified or carded are marked "settled" |

The scanners read one-line headers and one-call bodies, so multi-line cases are
under-counted, never over-counted. The scripts and their raw output are in the
lane directory (`census.mjs`, `wrappers.mjs`, `census2/`).

## 2. Census

| | Core | Jetpack |
|---|---|---|
| Files / public-looking functions / their parameters | 167 / 6,096 / 9,067 | 511 / 9,713 / 16,640 |
| Parameters with a default | 100 (in 60 functions) | 8 (in 8 functions) |
| Variadic parameters | 2 (Prelude `min`, `max`) | 0 |
| Functions with a `*` / `/` zone | 13 / 7 | 0 / 0 |
| …of those on the documented top level | 2 (`ui.preview`, `ui.playground`) | |
| External parameter names (`timeout seconds:`) | 0 | 0 |
| `Bool` parameters with no default | 197 (in 174 functions) | 380 |
| Functions with ≥4 / ≥6 parameters | 352 / 79 | 981 / 264 |
| …documented top-level functions with ≥4 parameters and no `*` zone | 37 to 48 (by module-matched or bare-name ledger match) | |
| Sibling-name families (`x` and `x_with…`, `x_in`, `x_first`, …) | 122 | 148 |
| Constant-forwarding wrappers | 204 | not measured |

Per Core module, the 14 largest by public-looking functions:

| Module | Fns | Params | Defaults | `Bool` (no default) | ≥4 params |
|---|---|---|---|---|---|
| Math | 844 | 989 | 0 | 3 | 27 |
| Encoding | 796 | 1,148 | 18 | 30 | 37 |
| Text | 436 | 728 | 0 | 28 | 12 |
| Web | 434 | 632 | 1 | 10 | 25 |
| Crypto | 381 | 795 | 0 | 6 | 65 |
| Net | 380 | 514 | 0 | 8 | 6 |
| Time | 332 | 443 | **46** | 5 | 21 |
| Files | 330 | 500 | 14 | 10 | 12 |
| Http | 286 | 345 | 7 | 2 | 9 |
| Data | 202 | 287 | 0 | 2 | 11 |
| Archive | 143 | 234 | 0 | 1 | 13 |
| Db | 141 | 203 | 2 | 2 | 10 |
| Service | 111 | 230 | 0 | 12 | 18 |
| Jobs | 102 | 258 | 2 | 9 | 25 |

Regex is smaller (102 functions) but has the most positional `Bool`s per
function: 22.

Good models already in Core are worth copying. Examples: `ui.preview(name, body, *,
viewport: UIPreviewViewport?{None})`, `time.until` / `since` (4 of 5 parameters
defaulted), `files.lock(path, timeout: Duration?{None})`,
`files.rename(from, to, if_exists:)`, `csv.reader` (4 of 5 defaulted), and
`cbor.reader` / `writer`.

## 3. Findings, ranked

Severity: **S1** a need just outside a wrapper has no supported route. **S2** a
duplicate, or a name that lies. **S3** a missed call tool (ergonomics). Rank is
severity first, then reach.

### F1 (S1, P1/P3/P4). Undocumented workers behind documented wrappers: 20 areas

Each row is a callee that two or more documented wrappers forward to with
literal arguments, where the callee itself is not on the surface ledger and not
documented. A probe confirms such a worker is callable today
(`base64.encode_alphabet(data, STD, false)` runs and prints `aGk/`), but users
cannot know it exists, its flags are untyped, and nothing keeps it stable.

| Area | Documented wrappers | Undocumented worker | No supported route for |
|---|---|---|---|
| `encoding.base64` | `encode`, `encode_url`, `encode_url_padded` (and base32 `encode`, `b32hexencode`) | `encode_alphabet(data, alphabet: String, padded: Bool)` | Unpadded standard alphabet; any custom alphabet |
| `encoding.base64` | `decode`, `decode_url`, `decodebytes` | `decode_strict(text, url: Bool, whitespace: Bool, relaxed: Bool)` | URL-safe with whitespace; relaxed decoding |
| `log` | `debug`, `info`, `warn`, `error`, `critical`, `fatal`, `log`, five `*_fields` | `emit(level: String, message, fields)` | Fields at a level chosen at run time |
| `regex` | `replace`, `replace_first` (and `sub`) | `replace_n(…, first_only: Bool)`; the name says *n*, the flag is a `Bool` | Replace the first *k* matches, even through the worker |
| `files` | `walk`, `walk_files`, `walk_parallel`, `walkdir` | `walk_impl(root, ignore, files_only: Bool)` | |
| `net.url` | `quote`, `quote_plus`, `quote_from_bytes`, `percent_encode` | `encode_component(text, allow_slash: Bool, plus_space: Bool)` | Keep `/` and encode spaces as `+` |
| `text.fmt` | `pad_left`, `pad_right`, `pad_center` | `pad_mode(…, mode: Int)` (0, 1, 2) | |
| `text.fmt` | `bin`, `oct`, `hex` | `radix_signed_width(n, base, width)` | Other bases; a width for `bin` and `oct` (`hex` requires one) |
| `text` | `pad_start`, `pad_end` | `pad_side` | |
| `text` | `remove_prefix`, `removeprefix`, `remove_suffix`, `removesuffix` | `strip_affix` | |
| `text` | `find`, `rfind` | `byte_find` | |
| `text.runtime` | `split_once`, `cut_last` | `split_at` | |
| `encoding` base85 | `b85encode`, `z85encode` / `b85decode`, `z85decode` | `base85_encode`, `base85_decode` (2 rows) | |
| `encoding.json` | `parse`, `parse_allow_duplicates` | `parse_json(text, allow_duplicates: Bool, limits)` | |
| `encoding.json` | `dumps`, `dump` | `render_json` | |
| `http` | `get`, `post` | `http.__request` | |
| `service` | `set_state_snapshot`, `set_state_event_log` | `select_state_adapter` | |
| `ui` | `open_request`, `save_request`; `node`, `node_color` | `transfer_request`; `transfer_node` (2 rows) | |

For each row, pillar 9 asks for one of two fixes. Either promote the worker to
a typed, documented primitive (enums for the flags, defaults for the common
case) and reduce the wrappers to one line or delete them. Or, if the wrappers
really are the whole job, move the worker into a `priv module`.

### F2 (S1, P7). `[Int]`-only copies of generic operations: `core.math.combinatorics`

The module has 37 documented names. Nine are integer counts, which is real
math. The other 28 are sequence functions that work only on `Int`:

- 25 take `[Int]` lists: `accumulate`, `batched`, `cartesian`, `chain`,
  `combinations`, `combinations_with_replacement`, `compress`, `cycle`, `drop`,
  `dropwhile`, `filterfalse`, `flatten`, `groupby`, `islice`, `pairwise`,
  `permutations`, `powerset`, `product`, `reverse`, `starmap`, `takewhile`,
  `tee`, `unique`, `windows`, `zip_longest`. `count`, `count_from` and `repeat`
  produce only `Int`.
- `takewhile(items, pred_nonneg: Bool)`, `dropwhile` and `filterfalse` take a
  `Bool` where the predicate belongs; "x >= 0" is the only predicate there is.
- `starmap(items: [[Int]])` takes no function (it sums each row).
  `groupby(items)` takes no key.
- `chain(left, right)`, `cartesian(left, right)` and `zip_longest(left, right,
  fill)` take exactly two lists.
- Four of the nine counts are second names: `ncr` and `combinations_count` are
  `binomial`; `npr` and `falling_factorial` are `permutations_count`.

The generic list methods already exist. Probes at HEAD: `fold`, `flat_map`,
`group_by`, `any`, `position` and `sum` run; `take_while`, `skip_while`,
`chunks`, `windows` and `zip` resolve and return a one-pass `Iter`.
`text.combinators.take_while(src, predicate: String)` has the same defect: a
predicate written as a string.

### F3 (S2, P6). Identical twins and names that lie: 10 groups

| Names | Fact |
|---|---|
| `files.walk` / `walk_parallel` | Same signature, same body. Nothing runs in parallel. |
| `files.create_dir` / `create_dir_all` | Same body (`__create_dirs_os`); both create missing parents. Only the effect rows differ (`FS.Write` vs `FS`). |
| `math.float32` / `float64` | Both are `{ n }`. `float32` does not round to 32 bits. |
| `base64.decode_padded` | `{ decode(text) }` |
| `crypto.privatedecrypt` / `publicdecrypt` | Same body; both take an `X25519SecretKey`, so "public" is false. |
| `ui.previews` / `ui.playgrounds` | Same signature, same body (`preview_registry`). |
| `data.median` / `math.stats.median` | Same job, two contracts: `Float DataError!` and `Float?`. |
| `combinatorics.count` / `count_from` | `count` forwards unchanged. |
| `combinatorics.ncr` / `combinations_count` / `binomial` | One count, three names. |
| `combinatorics.npr` / `permutations_count` / `falling_factorial` | One count, three names. |

Settled elsewhere, not counted here: the Python alias spellings
(`b64encode`, `urlsafe_b64encode`, `hexlify`, `warning`, `sub`, `pwd`, `rm`, …).
D-CORE-ONE-NAME1=A deletes every alias and teaches the foreign spelling; the
deletion is #5978 criterion 14. They still ship at HEAD (for example
`Base64.jet:47`).

### F4 (S2, P3). Wrapper families that cannot be combined

| Family | Problem |
|---|---|
| `regex.replace` / `replace_first` | Law 9's own example. Folding into `replace(…, limit:)` also gives "first *k*", which no function offers. |
| `time.period` / `period_days` / `_months` / `_years` | Wrappers that pass zeros to `period(years, months, days)`. |
| `text.fmt.pad` / `pad_left` / `pad_right` / `pad_center` | `pad` is `pad_right`; the mode should be an option. |
| `hex.encode` / `encode_upper` / `encode_prefixed` / `encode_sep` | Case, prefix and separator are separate wrappers, so "uppercase with a separator" has no route. |
| `archive` gzip / zstd: `compress`, `compress_text`, `compress_file` per codec | The codec should be a value. `compress_file` returns `Bool` and an error. |
| `http.client` `session_header`, `_timeout`, `_retries`, `_redirects`, `_auth`, `_proxy`, `_cookie` | Seven copy-setters that rebuild all 8 fields; struct field defaults would do. `session_request(…, body: String)` has no per-request headers or byte body. |
| `tui.style_bold` / `style_dim` / … | Copy-setters, where struct field defaults would do. |
| `net.dns_a(name, ms)` / `dns_a_at(server, name, ms)` (× record kinds) | The server should be an option. Timeouts are settled by D-DEADLINE-ONE1=A (below). |
| Jetpack `envmodel_evaluate_env` + `_with_preset` / `_with_environment` / `_with_selections` / `_with_source_loader` | Each passes `None` for the others. The same shape: `cli_compose_env_scoped` / `_with_stats` / `_with_warm`, `run_command` / `_in`, `enter` / `_with_mode`, `plan` / `_with_default`, and `foundation_run_native_sandboxed` / `_with_mounts` / `_with_timeout_and_mounts`, where each new option multiplies the names. The census finds 148 suffix families in Jetpack; not all are constant-forwarders. |

Settled elsewhere: D-DEADLINE-ONE1=A (ratified, not yet built) retires every
per-call timeout argument and setter (`tcp_connect_timeout`,
`session_timeout`, the `ms` of the nine `dns_*` lookups, and others) in favour
of one `#Deadline(duration)` block. It is the model of pillar 9: one composable
mechanism instead of a timeout sibling per call.

### F5 (S3, language). Gaps in the call tools

| Gap | At HEAD (probed) | Peers |
|---|---|---|
| Options after a variadic | `fn join_path(parts: ...String, *, sep: String{"/"})` gives `E1310 Variadic parameter 'parts' must be last` | Python keyword-only after `*args`; Swift labelled parameter after a variadic; Kotlin named arguments after `vararg` |
| Function values keep labels and defaults | They lose them: `E0764` (bug 1) | D-APILABEL1=A requires that they keep them |
| Spreading a record into labelled arguments | `connect("h", ...opts)` gives `E1312 'connect' doesn't accept a spread argument`; into a `*` zone it gives `E0769` | Python `**kwargs`, JS object spread |

The first is ballot D-VARIADIC-OPTS1. The second is a bug against a ratified
decision. The third needs research before any ballot: an options-struct
parameter with defaults already covers most uses, and D-META-PARAMS1 covers
parameter-list pasting in markers.

### F6 (S3, P4). `Bool` and string policy parameters

197 Core and 380 Jetpack `Bool` parameters have no default; Regex has 22 of
them. Examples: `regex.replace_n(…, first_only: Bool)`,
`text.wrap.indent_with(text, prefix, predicate_nonblank: Bool)`,
`base64.decode_strict` (three), `net.url.encode_component` (two).

String policies: `log.log(level: String, …)` (carded as #6056),
`term.style(style: String, …)`, `b.probe(name, "find_program", …)`, and the
`b.action` effects list `["Exec", "FS"]`.

D4 in `stdlib-api-laws.md` already forbids these. The lint that should find the
`Bool` cases, L2401 `positional_bool_parameter`, is registered as active but
never emitted (bug 3).

### F7 (S3, P5). Long positional parameter lists and missing defaults

352 Core functions take four or more parameters (79 take six or more); Jetpack
has 981 (264). Of the documented Core top level, 37 to 48 take four or more
with no `*` zone. Examples:

- `b.action(name, inputs, outputs, argv, caps, toolchain, probes)`: three
  adjacent `[String]` lists that swap silently.
- `net.dns_a_at(server, name, ms)`: two adjacent `String`s.
- `text.parse.replace(text, old, new, count: Int)`: `count` is required.
- `files.walk(root, ignore: String?)`: `ignore` is optional but has no default.
- `files.read_at(path, offset, count)`: two adjacent `Int`s.
- `b.add_executable(name, sources, deps)`: `deps` is required; callers pass `[]`.

### F8 (S3, P5). Variadics unused

Core has 2 variadics (`min`, `max`) and Jetpack has none. Candidates: log
fields, `combinatorics.chain` / `cartesian` / `zip_longest` once generic,
`ui.previews` / `playgrounds` (every call site passes a literal list), set
`union`, path joining, and `b.default`. Most need options too, so they wait on
F5's first gap.

### Not ranked: build outputs declared two ways

`Output.Executable{…}` and its siblings (a closed sum of 9 kinds, D-SHAPE5b=A)
and `b.add_executable(…)` and its siblings (8 `add_*` methods in `fn build`)
both declare build products. Whether they are two mechanisms for one job, or a
package-product layer over a build-graph layer, needs research before it is a
finding. A closed sum also means a package cannot add a new output kind, which
pillar 5 questions. Proposed as a research card, not a ballot.

## 4. Ranking summary

| Rank | Finding | Count | Severity | Fix size |
|---|---|---|---|---|
| 1 | F1 undocumented workers | 20 areas | S1 | Medium: type and document the worker, reduce wrappers |
| 2 | F2 `[Int]`-only copies | 25 list functions + 3 generators + 4 duplicate counts | S1 | Small: delete; make `chain` / `cartesian` generic and variadic |
| 3 | F3 twins and name lies | 10 groups | S2 | Small |
| 4 | F4 unjoinable families | 8 Core families; 148 Jetpack suffix families | S2 | Medium |
| 5 | F5 language gaps | 3 (1 ballot, 1 bug, 1 research) | S3 | Language work |
| 6 | F6 `Bool` and string policies | 577 `Bool` parameters + 4 string policies | S3 | Large, mechanical once L2401 fires |
| 7 | F7 long lists, missing defaults | 1,333 functions | S3 | Large, mechanical: insert `*` zones |
| 8 | F8 variadics unused | about 8 candidates | S3 | Small after F5 |

## 5. Proposed cards

Main files these. Items 1 to 4 rest on laws that are already ratified and can
be filed now; items 5 to 9 wait on the ballots. Each card cites its rule and
the rows above.

1. **Retire the `[Int]` combinatorics copies (F2).** Delete the copies of
   generic list methods and the duplicate counts; make `chain` and `cartesian`
   generic. Fix `text.combinators.take_while(predicate: String)`. Rests on Law
   8 (one way to mean it).
2. **Twins and name lies (F3).** Fold `create_dir` / `create_dir_all`. Delete
   `walk_parallel` or make it parallel. Delete `float32` / `float64` or make
   `float32` round. Fold `previews` / `playgrounds`, fix `publicdecrypt`, and
   pick one `median` contract. Rests on Law 8.
3. **Make L2401 fire (F6, bug 3).** The spec already promises it.
4. **Function values keep labels and defaults (F5, bug 1),** as D-APILABEL1=A
   already requires.
5. **Promote the undocumented workers (F1).** For each of the 20 rows, promote
   the worker to a typed, documented primitive or move it into a `priv module`.
   Start with base64, log and regex (`replace(…, limit:)`). Needs
   D-API-COMPOSE1, because folding the wrappers into options reverses Law 9.
6. **Fold the unjoinable families (F4)** into one function with label-only
   defaulted options: `period`, `pad`, `hex.encode`, archive codecs, http
   session fields, and the Jetpack `_with_` families starting with
   `envmodel_evaluate_env`. Needs D-API-COMPOSE1.
7. **Build API call shape (F6, F7).** Label-only `b.action` arguments, typed
   effects and probe kinds (D4 already requires the enums), defaulted `deps`.
   Needs D-API-COMPOSE1 for the label-only zone.
8. **Labelled options after a variadic (F5),** then the F8 variadics. Needs
   D-VARIADIC-OPTS1.
9. **The `api` lint group and the review checklist** (proposal §5 and §6):
   `Docs/spec/contributing/api-review.md`, the six `api.*` lints with their
   ratchet on Core and Jetpack, and Law 9 replaced in `stdlib-api-laws.md`.
   Needs D-API-COMPOSE1.
10. **Research: build outputs declared two ways (§3, not ranked).**

Already carded, so not filed again: the alias deletion (#5978 criterion 14),
public `__` hooks (#5978 criterion 13), the log level ladder (#6056). The
D-DEADLINE-ONE1 implementation should be checked for a card when Main files
these.

## 6. Bugs found (with repros)

Each repro was run with the published dev compiler at `59213de00f`.

1. **Function values lose labels.**
   ```jet
   fn greet(name: String, greeting: String{"Hello"}, punct: String{"!"}) -> String {
       "{greeting}, {name}{punct}"
   }
   fn run() {
       g :: greet
       print(g("Cy", punct: "~"))
   }
   ```
   `jet check` gives `E0764 'g' has no parameter labelled 'punct'`. D-APILABEL1=A
   says function values keep their labels and defaults.
2. **An unknown label on `print` is printed as a value.**
   `print("a", "b", sep: "-")` prints `a`, `b` and `-` on three lines and exits
   0. A user variadic with the same call shape, `total(1, 2, bogus: 3)`, gives
   `E0764 'total' has no parameter labelled 'bogus'`.
3. **L2401 is registered but never emitted.** `fn parse(text: String, strict:
   Bool)` called as `parse("abc", true)` gets only `L0102 unused_parameter`.
   L2401 is `active` in the diagnostic table and the spec says a positional
   `Bool` "earns advisory lint L2401", but no checker emits it.
4. **The retired trailing-block call form is still accepted.**
   `later() { 5 }` checks clean, although D-TRAILBLOCK2=A retired it (E0335).
   The UI `button` check still *requires* a trailing block
   (`Sema/Calls/UiHandles.jet:223`), and the spec still shows
   `b.generate(name) { ... }`.
5. **`String + String` gives a misleading diagnostic.** It reports `E0109
   Arithmetic needs matching number types`. It should teach interpolation
   (`"{a}{b}"`), since string `+` is intentionally absent.

The doc conflict between Law 9 and review row L9 in `stdlib-api-laws.md` is
settled by ballot D-API-COMPOSE1 (proposal §7).
