# Primitives first, pareto wrappers on top: an API composition principle

Card #6367. Status: proposal, dev `59213de00f`. Owner choices are the two
ballots in §8. The full audit is
[`Docs/audits/api-composition-2026-10-30.md`](../audits/api-composition-2026-10-30.md);
this page summarises it, so you can read this page on its own.

## Plain summary

- **The rule.** Every API area has a complete, documented set of primitives that
  can do everything the area needs. On top of them sit a few high-quality
  wrappers for the common jobs. A wrapper is always a one-step composition of
  public primitives. When you need one option more than a wrapper offers, you
  step down one rung to the primitive. You never rewrite and you never copy
  Core's source.
- **Options, not siblings.** If two variants of an operation differ only in a
  policy, they become one function with a labelled, defaulted, enum-typed
  option: `base64.encode(data, alphabet: .Url)`, not `encode_url`. Separate
  names are for separate jobs: a different result type, failure type, safety
  level, effect, or call shape.
- **Use the call tools Jet already has.** Jet has defaults, labels, label-only
  (`*`) and positional-only (`/`) zones, variadics and spreads. With them the
  common call is the shortest one, and every other call is still one call. Core barely uses
  them: 1.1% of its parameters have defaults, 2 functions are variadic, and 2
  documented top-level functions use a `*` zone.
- **What the audit found.** In 20 Core areas the documented surface is only
  wrappers, and the function that does the work is an undocumented helper with
  positional `Bool` flags. Examples are `base64` (no unpadded standard
  alphabet), `log` (no fields at a computed level) and `regex` (no "replace the
  first 3"). One module, `core.math.combinatorics`, ships 25 list functions
  that work only on `[Int]`, with a `Bool` where the predicate belongs. Core
  also has identical twins and names that promise behaviour the body does not
  have (`walk_parallel` is not parallel, `float32` does not round), and the
  language refuses options after a variadic.
- **How it stays true.** It adds a ninth philosophy pillar and an
  eight-question API review checklist in the contributing docs. The rules a
  machine can check become an `api` lint group in `jet lint`. The group starts
  with lint L2401, which is registered today but never fires.
- **Owner choices.** Two ballots: adopt the pillar (this also replaces the
  "siblings are the house style" law, which contradicts its own review table),
  and allow labelled options after a variadic parameter.

---

## 1. The principle (draft for `Docs/spec/philosophy.md`, pillar 9)

> **9. Primitives First, Pareto Wrappers On Top.** Every API area has a
> complete primitives layer that can express everything the area needs, and a
> small set of high-quality wrappers for the most common work. A primitive is
> documented, typed and stable; an undocumented helper is not a primitive. A
> wrapper is a documented composition of public primitives. It removes friction
> for the common case and never seals a path: if a wrapper can do something the
> primitives cannot, the primitives are incomplete. Stepping from a wrapper to
> its primitive is one rung on the same ladder, with the same names, types and
> errors. Variants of one operation are options on one function: labelled,
> defaulted and typed. Separate names are for separate jobs. APIs use every call
> tool the language provides: defaults, labels, positional-only and label-only
> zones, variadics and spreads. Then the common call is the shortest one, and
> every other call is still one call.

How it fits the existing pillars and priorities:

| Pillar | Relation |
|---|---|
| 1 Beginner magic, expert control | Wrappers and defaults are the magic; primitives and options are the control. Experts opt out by naming an option, as pillar 1 requires. |
| 2 Easy to read | A labelled option reads aloud at the call site: `encode(data, alphabet: .Url)` says as much as `encode_url(data)`, and it composes with the next option. |
| 3 Friction inverse to commonality | Wrappers exist only for common work. A rare need costs one more rung, not a fork. |
| 4 Ladders, not cliffs | Wrapper to primitive is a rung. An undocumented worker behind a wrapper is a cliff. |
| 5 The language is a library | Pillar 5 says "no names only Core may call" for the compiler. Pillar 9 says the same for every API: the worker a wrapper uses is one a user may use. |
| 7 Fewer layers | One function with options replaces a family of siblings. |
| 8 Explicit where quality depends on it | A choice that changes behaviour is a written label at the call; the default is the documented common case. |
| Priority 4, one mechanism | A wrapper is never a second mechanism; it expands into the first. |

It strengthens laws that are already in
[`stdlib-api-laws.md`](../spec/stdlib-api-laws.md). L-D says "beginner presets
compose small expert primitives; they do not fork them". D3 says "defaulted
options replace overload families". D4 asks for enum options, never `Bool` or
string flags. Review row L9 asks for one root method with a label-only option
enum, and Law 8 asks for one way to mean it. It replaces Law 9 ("sibling
methods stay the house style", D-STDLIB-OPTPARAM1=A), which contradicts L9 and
D3 on the same page (§7).

## 2. The rules

| Rule | Statement | Machine check |
|---|---|---|
| **P1 Complete primitives** | Each area names its primitives in its reference page. Everything a wrapper does is reproducible from documented primitives. A worker that is not meant to be a primitive lives in a `priv module`. | Partly: `api.constant_siblings` finds the wrapper families and names their shared worker |
| **P2 Wrappers are compositions** | A wrapper's body composes primitives and adds no behaviour of its own. Its doc names the primitive. | Partly |
| **P3 Options, not siblings** | Same operation with the same result, failure, safety and effect shape: one function with options. Otherwise a separate, plainly named job. | Yes: constant-forwarding sibling families |
| **P4 Typed options** | A policy is an enum or a typed value, never a `Bool` or a bare string. Every option has a default unless the caller must decide. | Yes: positional `Bool` (L2401), string compared only with literals |
| **P5 Use the call tools** | Four or more parameters, or two adjacent parameters of one type, put the later ones in a `*` zone. A list parameter usually called with a literal list is a variadic. An optional parameter defaults to `None`. | Yes |
| **P6 No twins, no name lies** | Two public names with identical behaviour, or a name that promises behaviour the body lacks, are defects. | Identical bodies: yes. Name lies: review |
| **P7 Generic before specific** | Do not ship an `[Int]`-only copy of an operation the language already offers generically. | Review |
| **P8 Wrappers earn their place** | A wrapper exists for work that is common in real programs: the competitive gate's frozen tasks (D-STDRUBRIC1) or counted call sites. | Review |

**When a sibling is still right.** The result type differs (`read` returns
`String`, `read_bytes` returns `[U8]`). The failure type differs. The safety
differs (an expert rung, as in D-ONCE-LAYER1=B). The effect row differs. Or the
call shape differs (a callback replacement such as `replace_all_with`). A
sibling is never right just because a constant differs.

## 3. Worked before and after (real Core and build code)

Every "today" line is a real signature at dev `59213de00f`. Every "after" is a
proposal under the rules above. The audit has the full lists.

### 3.1 `core.encoding.base64`: 31 documented names, worker undocumented (P1, P3, P4)

```jet
// Today: encode_alphabet(data, alphabet: String, padded: Bool) does the work.
// It is callable, but no reference page lists it and its flag is a bare Bool.
base64.encode(data)              // standard, padded
base64.encode_url(data)          // URL-safe, unpadded
base64.encode_url_padded(data)
base64.urlsafe_b64encode(data)   // a second name for encode_url
// Documented way to write unpadded standard base64: none.
```

```jet
// After: one primitive with typed options; the common call is unchanged.
base64.encode(data)
base64.encode(data, alphabet: .Url, padding: .Omit)
base64.encode(data, alphabet: .Custom(BCRYPT), padding: .Omit)
base64.decode(text, alphabet: .Url, whitespace: .Skip)
```

31 documented names become 4 (`encode`, `decode`, `pad`, `is_base64`), and
two jobs with no documented route get one.

### 3.2 `core.log`: five `_fields` twins and an undocumented `emit` (P1, P3, P5)

```jet
// Today
log.info("saved")
log.info_fields("saved", [log.field("user", id)])
// Fields at a level chosen at run time: only through undocumented log.emit.
```

```jet
// After: fields are a variadic tail; the level is an enum
log.info("saved")
log.info("saved", log.field("user", id), log.field("ms", ms))
log.log(level, "slow", log.field("ms", ms))
```

The level's string type is already carded as #6056 (log level ladder). This
example only removes the `_fields` twins and documents the primitive.

### 3.3 `core.regex`: `replace` and `replace_first` (Law 9's own example) (P3, P4)

```jet
// Today: both forward to replace_n(pattern, text, replacement, first_only: Bool).
regex.replace(re, text, "x")
regex.replace_first(re, text, "x")
// Replace the first three matches: no route.
```

```jet
// After
regex.replace(re, text, "x")
regex.replace(re, text, "x", limit: 1)
regex.replace(re, text, "x", limit: 3)
```

The callback form `replace_all_with` stays a sibling: its call shape differs.

### 3.4 `core.time.period`: three wrappers that pass zeros (P3, P5)

```jet
// Today
time.period(0, 0, 3)
time.period_days(3)
```

```jet
// After: label-only parameters with defaults
time.period(days: 3)
time.period(months: 1, days: 2)
```

### 3.5 `core.net`: a model the board already chose (P3)

```jet
// Today: a timeout needs a SocketAddr, and a host name cannot have one.
net.tcp_connect("db.local:5432")
net.tcp_connect_timeout(addr, 2000)
```

```jet
// After D-DEADLINE-ONE1=A (ratified, not yet built): one connect, one deadline block
#Deadline(2s) {
    conn :: net.tcp_connect("db.local:5432")?
}
```

This is pillar 9 at work: one composable mechanism (the deadline block) replaces
a timeout sibling on every waiting call. It needs no new ballot, only the
implementation card.

### 3.6 `fn build`: seven positional arguments, stringly typed (P4, P5)

```jet
// Today (spec "Programmable builds")
shell :: b.probe("shell", "find_program", "sh")
stamp :: b.action("stamp", ["assets/version.txt"], ["build/version.txt"],
    ["sh", "-c", "cp assets/version.txt build/version.txt"], ["Exec", "FS"], native, [shell])
app :: b.add_executable("app", ["main.jet"], [stamp])
```

```jet
// After
shell :: b.probe("shell", .Program("sh"))
stamp :: b.action("stamp", inputs: ["assets/version.txt"], outputs: ["build/version.txt"],
    run: ["sh", "-c", "cp assets/version.txt build/version.txt"], effects: [.Exec, .FS],
    toolchain: native, probes: [shell])
app :: b.add_executable("app", sources: ["main.jet"], deps: [stamp])
```

Three adjacent `[String]` lists can no longer be swapped silently, and the
effect and probe kinds are checked enums instead of strings.

### 3.7 Jetpack: four `_with_` wrappers that each pass `None` for the others (P3)

```jet
// Today: which wrapper takes a preset and an environment?
envmodel_evaluate_env(src, dir)
envmodel_evaluate_env_with_preset(src, dir, preset)
envmodel_evaluate_env_with_environment(src, dir, env)
envmodel_evaluate_env_with_selections(src, dir, preset, env)
```

```jet
// After
envmodel_evaluate_env(src, dir, preset: "ci", environment: "linux")
```

### 3.8 `core.math.combinatorics`: `[Int]`-only copies of generic operations (P7)

```jet
// Today: [Int] only, and a Bool where a predicate belongs
combinatorics.takewhile([3, 1, -2, 4], true)   // the only predicate is "x >= 0"
combinatorics.chain(a, b)                      // exactly two lists
```

```jet
// After: the generic list methods already exist (take_while, chunks, windows, zip, group_by)
xs.take_while(x -> x > limit)
combinatorics.chain(a, b, c)                   // variadic and generic
```

### 3.9 A language gap: options after a variadic (ballot D-VARIADIC-OPTS1)

```jet
fn join_path(parts: ...String, sep: String{"/"}) -> String
join_path("usr", "local", "bin")
join_path("C:", "Users", sep: "\\")
```

Today the declaration is `E1310 Variadic parameter 'parts' must be last`.
Python (keyword-only parameters after `*args`), Swift (a labelled parameter
after a variadic) and Kotlin (named arguments after a `vararg`) all allow it.
Without it, a variadic API cannot have options, so authors fall back to list
parameters or sibling families. That is the main reason Core has 2 variadics.

## 4. What the audit counted

Scope: all of `Core/` (167 files), `Jetpack/` (511 files), the `fn build` API,
and the Core surface ledger (1,180 user-facing names in 75 modules). Tests are
excluded. Ranked by severity, then reach:

| Rank | Finding | Count | Fix |
|---|---|---|---|
| 1 | Undocumented workers behind documented wrappers | 20 areas | Promote the worker to a typed, documented primitive; reduce wrappers to one line or delete them |
| 2 | `[Int]`-only copies of generic operations | 25 functions, plus 4 second names for the same count | Delete; make `chain` and `cartesian` generic and variadic |
| 3 | Identical twins and names that lie | 10 groups (aliases are carded separately) | Fold or fix |
| 4 | Wrapper families that cannot be combined | 8 Core families; 148 suffix families in Jetpack | One function with label-only options |
| 5 | Call-tool gaps in the language | 3 (options after a variadic; function values drop labels; spreading a record into labels) | One ballot, one bug, one research item |
| 6 | `Bool` and string policy parameters | 197 Core and 380 Jetpack `Bool` parameters with no default | L2401 must fire; then enums |
| 7 | Long positional parameter lists | 352 Core and 981 Jetpack functions with 4 or more parameters | `*` zones, mechanically |
| 8 | Missing defaults | 100 of 9,067 Core parameters have one | Per site |

The audit also found five bugs (§6 of the audit), including L2401 never firing
and the retired trailing-block call form still being accepted.

## 5. Review checklist (for a new `Docs/spec/contributing/api-review.md`)

A reviewer answers each question for every new or changed public API. A "no"
blocks the change unless the review record explains why.

1. **Primitive named.** Which documented primitive does this area use? (P1)
2. **Wrapper expands.** Can you write this wrapper's body using only documented
   primitives, and does its doc name them? (P2)
3. **Same job, one name.** Is there a sibling that differs only in a constant?
   Fold it into an option. (P3)
4. **Typed options.** Does a `Bool` or a string pick a policy? Make it an enum.
   Does every option have a default unless the caller must decide? (P4)
5. **Call shape.** Are the later parameters label-only? Should a list parameter
   be a variadic? Do optional parameters default to `None`? (P5)
6. **No twins.** Does another public name already do this? Does the name
   promise only what the body does? (P6)
7. **Generic first.** Does a generic operation already exist? (P7)
8. **Earned.** Which frozen task or counted call sites justify this wrapper? (P8)

The review record in `stdlib-api-laws.md` gains one field, "Pillar 9 answers",
with the eight answers. The checklist page is the only copy; the laws page
links to it.

## 6. The `api` lint group (the mechanical rules)

D-STDRUBRIC1=A makes Core API laws "lints first", with the rubric generated
from lint metadata. These rules are mechanical and belong in `jet lint` as one
group. They run on Core and Jetpack under the same diagnostic ratchet as other
lints, and any package can turn them on (pillar 5).

| Lint | Detects | Example hit at HEAD |
|---|---|---|
| `positional_bool_parameter` (L2401, registered) | A `Bool` parameter outside a `*` zone with no default | `regex.replace_n(…, first_only: Bool)` |
| `api.constant_siblings` | Two or more public functions whose bodies are one call to the same callee, differing only in literal arguments. The fix hint names the callee as the primitive to promote. | `log.info` / `warn` / `debug` → `emit`; `base64.encode` / `encode_url` → `encode_alphabet` |
| `api.string_policy` | A `String` parameter compared only with string literals | `term.style(style: String, …)`, `b.probe(…, "find_program", …)` |
| `api.long_positional` | Four or more parameters with no `*` zone, or two adjacent parameters of one type | `b.action` (7), `net.dns_a_at(server, name, ms)` |
| `api.list_literal_param` | A list parameter that most call sites in the repo pass as a literal list | `ui.previews([phone, fallback])`, `ui.playgrounds([lab])` |
| `api.identical_twin` | Two public functions with identical bodies and signatures | `ui.previews` / `ui.playgrounds`, `files.walk` / `walk_parallel` |
| `api.copy_setter` | A function that copies a struct and changes one field, where struct field defaults would do | `tui.style_bold` |

The rules read declarations and one-call bodies (`api.list_literal_param` also
reads the package's call sites), so none needs a list of "documented" names.
The lints are ordinary compiler-API clients, so a third-party package gets the
same checks.

## 7. Conflicts this resolves

- **Law 9 against review row L9 and D3.** Law 9 says "do not add a mode
  parameter" and cites `replace` / `replace_first`. Row L9 says variants with
  the same result and safety shape "use one root method with a safe default and
  a label-only option enum". D3 says "defaulted options replace overload
  families". The same page asks for opposite things. Ballot D-API-COMPOSE1
  settles it for options.
- **Everything is public, but not everything is a primitive.** D-VIS-MODULE1
  makes every declaration public unless it is inside a `priv module`. Core has
  one `priv module` (in `core.db.sql`). So the 20 workers in rank 1 can already
  be called, but they are undocumented, untyped and unstable. Pillar 9 makes
  the choice explicit: promote the worker or make it private.

Already decided elsewhere, so not re-balloted here: Python alias spellings
(D-CORE-ONE-NAME1=A, carded as #5978 criterion 14), public `__` hooks (#5978
criterion 13), per-call timeouts (D-DEADLINE-ONE1=A), and the log level ladder
(#6056).

## 8. Owner ballots

| Ballot | Question | Options | Recommendation | Profile |
|---|---|---|---|---|
| D-API-COMPOSE1 | Make "primitives first, pareto wrappers on top, options not siblings" philosophy pillar 9, enforced by a review checklist and an `api` lint group? It supersedes D-STDLIB-OPTPARAM1 (Law 9). | A: pillar 9 + checklist + lint group. B: primitives first only; sibling names stay the house style for variants. C: checklist guidance only, no pillar and no lints. | A | Short (stdlib group) |
| D-VARIADIC-OPTS1 | Allow label-only parameters after a variadic parameter? It amends D-VARIADIC1's "last parameter only" and E1310. | A: parameters after a variadic are label-only, as after Python's `*args` (`join_path("C:", "Users", sep: "\\")`). B: keep the variadic last; functions with options take a list. C: options go before the variadic, positionally. | A | Full (syntax group), so it needs a fresh adversarial review |

Reconcile D-VARIADIC-OPTS1 with D-CALLPOS1 (names are required in a `*` zone,
and the variadic acts as the zone opener), D-META-PARAMS1, and spreads
(`f(...xs, sep: "-")`). Peers for the comparisons: Python `base64.b64encode(s,
altchars=)` and keyword-only parameters after `*args`; Go
`base64.StdEncoding.WithPadding(base64.NoPadding)`; Swift labelled parameters
after a variadic; Kotlin named arguments after `vararg`.

## 9. Cards

The audit (§5) lists ten cards with the rows they fix. Four rest on laws that
are already ratified and can be filed now: retire the `[Int]` combinatorics
copies, fold the twins, make L2401 fire, and keep labels on function values.
Five wait on the ballots above, including promoting the base64, log and regex
workers, because folding wrappers into options reverses Law 9. One is a
research card on build outputs.
