# Roc and Skip: error handling, absence, and inference ideas for Jet

Date: 2026-09-30. Method: `research` with same-program probes in Roc and Jet.
This report keeps dated findings and candidate ballot text. It does not own
plans or status and creates no Tower card or ballot. The owner decides every
option below.

## Question and scope

The owner's note: Roc seems to have a better version of error handling and of
unknown-type handling than Jet. This report asks, for Roc and Skip:

1. What each idea is, in the language's own current syntax, from primary
   sources.
2. What Jet does today for the same program, in the spelling that
   `Docs/spec/syntax-decisions.md` ratifies, confirmed by a snapshot run.
3. Whether Jet should adopt it, adapt it, or already does better, and at what
   cost to syntax, I8 (one mechanism), beginner and expert paths, and
   performance.
4. Candidate ballot text for anything worth adopting.

Focus: error handling, absence and unknown values, and type inference. Roc
topics: tag unions and their open/closed inference, error-set widening
through `?`, no null, structural records, whole-program inference, `crash`,
effects and purity inference, `dbg`, `expect`, abilities. Skip topics:
memoization, incremental computation, mutability control, and anything about
errors or unknowns.

## Method, provenance, and limits

- **Roc sources.** Roc is mid-rewrite (new compiler in Zig, no 0.1 release).
  `Result` is now `Try`, abilities are replaced by static dispatch, and lambdas
  are `|x| ...`. This report cites only current sources: the tutorial
  ([mini-tutorial-new-compiler.md](https://github.com/roc-lang/roc/blob/main/docs/mini-tutorial-new-compiler.md),
  which `roc-lang.org/tutorial` redirects to), the language reference
  ([langref](https://www.roc-lang.org/docs/main/langref/)), the
  [all-syntax file](https://github.com/roc-lang/roc/blob/main/test/echo/all_syntax_test.roc),
  the [FAQ](https://www.roc-lang.org/faq), the builtins
  ([Builtin.roc](https://github.com/roc-lang/roc/blob/main/src/build/roc/Builtin.roc)),
  and CI-built [examples](https://www.roc-lang.org/examples/) at
  `roc-lang/examples@c176d73`. Roc repository HEAD was `00cab95a` when read.
- **Roc runs.** Roc claims below marked *(run)* were executed with the Roc
  nightly `nightly-2026-09-29-7f11a82` (linux x86_64), probe files `r1`–`r4`
  in `~/.cache/jet-dev/rocskip/`. The langref is marked work-in-progress by
  its authors; where docs and the compiler disagree, the run wins.
- **Skip sources.** Skip (skiplang.com) is the 2017–2018 Facebook research
  language, archived at
  [skiplang/skip@b63d2de](https://github.com/skiplang/skip). Its docs and
  compiler tests are cited directly. No Skip compiler was run. Its successor,
  the SkipLabs reactive-service framework
  ([SkipLabs/skip](https://github.com/SkipLabs/skip)), is a TypeScript API over
  a runtime written in Skiplang and is noted only briefly.
- **Jet runs.** Jet probes `p1`–`p15` ran through
  `~/.cache/jet-dev/safe-jet.sh` on the frozen snapshot
  `jet-debug-snapshot23` (built 2026-09-30 04:54), repository HEAD
  `5273e43d4`, from `~/.cache/jet-dev/scratch/rocskip/`. `jet run` is the
  default JIT tier; one probe also tried `jet build` (AOT). Other agents were
  editing failure-model code (#3838, #3708 neighbours) at the time, so the
  snapshot may already be stale for those areas.
- Tower state was read, not written: #3742 (D-FAIL-VISIBLE1 and
  D-FAIL-PUBSIG1 open; D-OUTCOME-SHAPE1=A ratified 2026-09-29), #3838 (its
  implementation), #3708 (inferred failure sets), #3740 (infallible lowering),
  #3713 (failure path report).
- This is not a user study. Statements about how users would react are
  predictions.

## Summary: ranked recommendations

1. **Let an inferred failure keep its typed members** (Roc's error
   accumulation). Today a function that omits its contract can only fail with
   `Err`, so calling two helpers with different typed errors fails to compile
   (E2402) unless the author writes the union or two conversions (p1). Roc
   infers and accumulates the union with no annotations and checks the
   caller's match exhaustively (r1, r1b). #3708 already plans to infer failure
   sets; the choice is whether the inferred set keeps its members. Candidate
   ballot D-FAIL-INFER-UNION1.
2. **Function-owned error cases without a separate declaration** (Roc's
   ad-hoc tags). Roc writes `Err(NotFound(name))` and is done; Jet needs a
   `#Error enum` first. Only worth doing if (1) passes. Candidate ballot
   D-ERR-CASES1.
3. **Lift a plain value into `T?` the way it already lifts into `T E!`.**
   `fn f() -> Int DBError! { 42 }` compiles, but `fn f() -> Int? { 42 }` and
   `describe("ada")` for a `String?` parameter are E0108 "Wrap it with
   `Val(...)`" (p12, p13). D-FAIL-CARRIER1 calls both views one carrier; the
   asymmetry is friction on the most common absence code. Candidate ballot
   D-OPT-LIFT1.
4. **A `dbg(expr)` that works in pure code.** Jet has no print-debugging
   route inside `-[]>` or `#Memo` functions: `print` there is E3401 (p5).
   Roc's `dbg` is allowed in pure functions because program behaviour never
   depends on it (r2). Candidate ballot D-DBG1.
5. **Fix the error-union and absence defects these probes hit.** Exhaustive
   matching over a union of `#Error struct` members fails (p8), `{e:Debug}` on
   an error union is an internal compiler error on `jet run` (p2), `#Memo`
   rejects an inferred-pure function (p7), `.cache()` reports zero hits on
   `jet run` and does not compile on AOT (p7), and several fix texts give
   wrong advice. These are defects, not ballots; see Part 3.

Also recorded, lower priority: Skip's rule that only unit values may be
silently discarded, which would have caught p4b and p15 (candidate D-DISCARD1);
Roc's "run despite compile errors" dev mode (candidate D-RUN-BROKEN1, not
recommended now).

Where Jet already does better than Roc: effect rows (inferred, tree-shaped,
deniable) versus Roc's binary pure/effectful split; the failure report with
codes, journey, and a three-way exit law versus Roc's untyped `crash`
("runtime error", exit 1); always-on contracts versus dev-only `expect`; and a
deliberate `T?` versus Roc's policy of turning every absence into an error that
`?` propagates.

---

## Part 1. Roc

### 1.1 Tag unions and error accumulation through `?`

**What it is.** Roc has no exception system and no declared error types by
default. An error is a *tag*, a capitalised name with optional payload, written
directly: `Err(NotFound(name))`. Tags belong to *structural* tag unions, which
need no declaration and are *extensible*: "the type can accumulate new tags
based on how it's used"
([langref: tag unions](https://www.roc-lang.org/docs/main/langref/tag-unions/)).
`Try(ok, err)` is the builtin nominal type `[Ok(ok), Err(err)]`
(`Try(ok, err) := [Ok(ok), Err(err)]`, Builtin.roc line 5430).

`?` unwraps `Ok` or early-returns the `Err`
([tutorial, "The `?` postfix operator"](https://github.com/roc-lang/roc/blob/main/docs/mini-tutorial-new-compiler.md)):

```roc
increment_first = |strings| {
    first_str = strings.first()?
    first_num = I64.from_str(first_str)?

    Ok(first_num + 1)
}
```

`List.first` returns `Try(item, [ListWasEmpty])` and `I64.from_str` returns
`Try(I64, [BadNumStr])` (Builtin.roc lines 4232 and 13269). The caller's error
type becomes the union `[ListWasEmpty, BadNumStr]` with no annotation. A
signature may write the union, open or closed:

```roc
parse_person : Str -> Try(Person, [InvalidSentenceFormat(Str), InvalidNameFormat(Str), InvalidBirthYearFormat(Str), ..])
```

([ErrorHandlingBasic](https://www.roc-lang.org/examples/ErrorHandlingBasic/README.html)).
`..` means "these tags or any wider union"; `..others` names the rest; closed
`..[]` is documented but "has not been implemented yet"
([types](https://www.roc-lang.org/docs/main/langref/types/),
[tag unions](https://www.roc-lang.org/docs/main/langref/tag-unions/)). Roc has
no subtyping; width comes from extension variables in Hindley–Milner
unification. A signature can also leave the error part to inference with a
hole: `question_postfix : List(Str) -> Try(I64, _)` (all-syntax file).

The CI-built
[ErrorHandlingRealWorld](https://www.roc-lang.org/examples/ErrorHandlingRealWorld/README.html)
example shows the payoff at scale: an unannotated `run!` calls six helpers
with `?`, and `main!` matches `FailedToReadArgs`, `VarNotFound`,
`EnvVarSetEmpty`, `FailedToParseUrl`, `FailedToFetchHtml`, `FailedToWriteFile`,
and `FailedToListCwd`, with `Err(other)` for the platform's own errors.

**Roc run (r1).** Two helpers, two different tags, a caller with no annotation,
and an exhaustive match with no catch-all:

```roc
find = |book, name| {
	match book.get(name) {
		Ok(v) => Ok(v)
		Err(KeyNotFound) => Err(NotFound(name))
	}
}

age_of = |book, name| {
	raw = find(book, name)?
	parse_age(raw)
}

describe = |book, name| {
	match age_of(book, name) {
		Ok(n) => "age ${n.to_str()}"
		Err(NotFound(_)) => "no such person"
		Err(BadNumber(_)) => "bad age on file"
	}
}
```

It compiles and prints the three answers. Deleting the `BadNumber` arm (r1b)
gives a precise error:

```text
── ✗ non exhaustive match ───────────────────────────── r1b_missing_arm.roc:23:2
This match expression doesn't cover all possible cases.
...
The value being matched on has type:
        Try(ok, [BadNumber(ok), NotFound(_a)])
  where [
    ok.from_numeral : Numeral -> Try(ok, [InvalidNumeral(Str)]),
    ok.from_quote : Str -> Try(ok, [BadQuotedBytes(Str)]),
    ok.is_eq : ok, ok -> Bool,
    ok.to_str : ok -> _ret,
  ]
Missing patterns:
        Err BadNumber _
```

Two costs show in that output. The inferred type is hard to read (a
where-clause about numeric literals the user never thought about), and the
payload of `BadNumber` was inferred as the same type variable as the success
value because nothing pinned it. Fully inferred structural types are precise
but can be noisy to display.

**Jet today.** Jet's failure contract follows the success type:
`-> Int ParseError!`, `-> Int (ParseError | LookupError)!`, unit-fallible
`fn save(entry: Entry) SaveError!` (D-TYPE-SUFFIX1=A, which amended
D-FAILURE-FOUNDATION1=A). An omitted contract is "implicitly fallible with the
default `Err`" (D-FAILURE-FOUNDATION1=A). Plain fallible calls propagate
automatically (S7); `?(text)` adds a note (D-FAIL-CTX1=A). Typed error types
are declared with `#Error enum` or `#Error struct`. Crossing from one error type
to another uses the one conversion rail `impl Source -> Target`
(D-FAIL-CONV1=A); the standard library declares its own family's conversions
into `Err` (D-FAIL-CONV2=A), but "a program's own error type still needs its
own declaration". `A | B` is a closed anonymous union; members widen into it
"at binding, argument, return, Codable field, and `?` error boundaries"
(D-UNIONTYPE1=A).

The same program in Jet with no contract on `age_of` (p1):

```jet
#Error
enum ParseError {
    BadNumber(String)
}

#Error
enum LookupError {
    NotFound(String)
}

fn find(book: [String:String], name: String) -> String LookupError! {
    if book.has_key(name) -> return Ok(book[name])
    Err(LookupError.NotFound(~name))
}

fn age_of(book: [String:String], name: String) -> Int {
    raw :: find(book, name)
    parse_age(raw)
}
```

```text
Error [E2402]: This implicit fallible call can't convert `LookupError` into `Err` — no declared conversion exists
  --> p1_accumulate.jet:23:12
 Fix: Handle this call locally with `??`, add `impl LookupError -> Err { … }`, or change the return type
Error [E2402]: This implicit fallible call can't convert `ParseError` into `Err` — no declared conversion exists
```

Writing the union makes it work, and automatic propagation widens each member
into it (p2, run output `age 42`, `bad age on file`, `no such person`):

```jet
fn age_of(book: [String:String], name: String) -> Int (ParseError | LookupError)! {
    raw :: find(book, name)
    parse_age(raw)
}

fn describe(book: [String:String], name: String) -> String {
    if age_of(book, name) == {
        .Ok(n) -> "age {n}"
        .Err(.LookupError(_)) -> "no such person"
        .Err(.ParseError(_)) -> "bad age on file"
    }
}
```

So Jet has every piece Roc uses except inference of the set. `jet inspect
expand --facts callable-signature` confirms that today an unwritten contract is
always `errors=[Err] failure=Int (implicit default !Err)`, even for
`fn twice(n: Int) -> Int { n * 2 }` (p5b). #3708 plans a failure-set solve
"beside the effect solve", publishing "inferred: none" or "inferred: E1, E2"
for unwritten contracts; #3740 lowers an empty set to a plain return. The plan
text does not say whether callers may *match* the inferred members, or whether
a typed member that reaches an unwritten contract still needs a conversion into
`Err`. That is the open question.

**Side by side.**

| | Roc | Jet today | Jet with ballot option A |
|---|---|---|---|
| Declare error cases | none (tags) | `#Error enum` per domain | `#Error enum` per domain |
| Caller contract | none | must write `(ParseError \| LookupError)!` or two `impl … -> Err` | none; inferred `(LookupError \| ParseError)!` |
| Match members | `Err(NotFound(_))` | `.Err(.LookupError(_))` | same |
| Exhaustive check | yes | yes (enum members; see Part 3 for structs) | yes |
| Public API stability | none (inferred types change silently) | pinned by written contract | pinned where D-FAIL-PUBSIG1 requires |

**Verdict: adopt, adapted.** The owner's direction on #3742 is "infer where
intent stays clear". The inferred union is exactly what the compiler knows, it
needs no new syntax, and widening at propagation already works (p2). The
adaptation Jet needs, and Roc lacks, is stability at the package edge;
D-FAIL-PUBSIG1 option A already provides it (exported functions write their
contract, and `jet fmt` inserts the inferred one).

Costs:

- **Syntax and I7:** none. The union spelling and member patterns exist.
- **I8:** no new mechanism. It reuses #3708's solve, D-UNIONTYPE1 widening,
  and member patterns. It narrows the implicit route: the default `Err` becomes
  one possible member instead of the only answer. D-FAIL-CONV1's rail stays the
  one way to *change* an error's type; option A only stops forcing a
  conversion when the author never asked for one.
- **Beginner:** writes nothing and gets typed, matchable failures. p1 compiles.
- **Expert:** writes a contract to pin, exactly as today.
- **Performance:** an anonymous union of small enums is a tagged enum. Whether
  it is smaller or larger than the `Err` report depends on payloads
  ([INFERENCE]; not measured). This is not a performance surface, so no paired
  cell is required unless someone claims a speed win.
- **Compile speed:** the SCC fixpoint #3708 already plans; sets are bounded by
  the program's declared error types.
- **Edge cases:** recursion and mutual recursion (fixpoint over the call
  graph); function values, closures, and trait dispatch (declared or bounded
  types; an unknown callee is maximal, meaning `Err`); an inferred set that
  grows breaks callers' exhaustive matches (intended; Roc behaves the same);
  display must list members plainly and never show solver noise like r1b.

**Candidate ballot D-FAIL-INFER-UNION1** (id is a proposal, not registered).

- *Gist:* When a function does not write its failure contract, may callers see
  and match the typed failures it can pass up?
- *Story:* Mia writes `age_of`, which calls `find` (fails with `LookupError`)
  and `parse_age` (fails with `ParseError`). She writes no contract and wants
  to say "no such person" or "bad age on file" at the call site.
- *Current:* p1 above; two E2402 errors until she writes
  `-> Int (ParseError | LookupError)!`.
- *Option A — inferred union keeps members.* An unwritten contract fails with
  the union of every failure type that can reach it: callee failures that
  propagate, and every `Err(value)` it returns (`Err("msg")` contributes
  `Err`). An empty set means the function cannot fail (#3740). Callers match
  members with `.Err(.LookupError(e))`, exhaustively. Writing a contract pins
  it, and conversions stay on D-FAIL-CONV1's rail. Hover, `jet doc`, and
  `jet inspect` show the inferred set. Exported functions follow D-FAIL-PUBSIG1.

  ```jet
  fn age_of(book: [String:String], name: String) -> Int {
      raw :: find(book, name)        // LookupError can pass up
      parse_age(raw)                 // ParseError can pass up
  }
  // inferred: -> Int (LookupError | ParseError)!

  fn describe(book: [String:String], name: String) -> String {
      if age_of(book, name) == {
          .Ok(n) -> "age {n}"
          .Err(.LookupError(_)) -> "no such person"
          .Err(.ParseError(_)) -> "bad age on file"
      }
  }
  ```

- *Option B — inferred set is yes/no only.* #3708 infers only whether a
  function can fail. A typed failure reaching an unwritten contract still needs
  `impl X -> Err` or a written union (today's E2402).
- *Option C — members inside a module, `Err` across modules.* As A, but a call
  from another module sees `Err` unless the callee writes its contract.
- *Comparisons:* Roc (r1 above; FAQ and langref links); Rust requires the
  error type in every signature
  ([book](https://doc.rust-lang.org/book/ch09-02-recoverable-errors-with-result.html));
  Zig infers error sets with `!T` and checks `switch` exhaustiveness over them
  ([Zig error set inference](https://ziglang.org/documentation/master/#Inferred-Error-Sets)).
- *Recommendation (for the owner to weigh):* A, because it removes a real
  compile error on the beginner route and matches what the compiler already
  knows; D-FAIL-PUBSIG1=A covers the stability risk. B keeps the E2402 wall. C
  adds a boundary rule users must learn and makes moving a function between
  modules change its type.

### 1.2 Error cases without a separate declaration

**What it is.** In Roc the tag *is* the declaration. `Err(BadFormat)` in one
function and `Err(BadNumStr)` from a builtin combine into
`[BadFormat, BadNumStr, ..]`
([TryOperatorDesugaring](https://www.roc-lang.org/examples/TryOperatorDesugaring/README.html)).
Tags with the same name must have compatible payloads across a union
([tag unions](https://www.roc-lang.org/docs/main/langref/tag-unions/)).
Nominal unions (`Color := [Red, Green, Blue]`) exist for recursion, host
boundaries, and methods.

**Jet today.** Every typed error needs a declared type with `#Error`. Jet's
expected-type shorthand already shortens the constructor when the type is
known: `if n < 0 -> return Err(.Low)` in a `-> Int Fault!` function
(`Examples/features/patterns/nested_variant_patterns.jet`). The one-off
alternative is the default error with a code, `Err("msg", code: "CFG404")`
(S80), which callers can only compare as text. A single-case domain costs a
four-line declaration (p8):

```jet
#Error
struct ListWasEmpty {}

fn first(xs: [String]) -> String ListWasEmpty! {
    if xs.is_empty() -> return Err(ListWasEmpty{})
    Ok(xs[0])
}
```

**Verdict: adapt, only if 1.1 chooses A.** Roc's structural tags would be a
second kind of sum type beside D-UNIONTYPE1's "members are types" and closed
named enums (D-ENUM-EVOLUTION1=A); that is an I8 cost Jet should not pay. A
narrower adaptation keeps everything nominal: a function that writes no
contract may name new cases in `Err(.Case(payload))`, and those cases form that
function's own error enum.

**Candidate ballot D-ERR-CASES1.**

- *Gist:* Must every typed error case be declared in a separate `#Error enum`,
  or may a function name its own cases where it fails?
- *Option A — declared only (today).* Cases live in `#Error enum` or
  `#Error struct` declarations.
- *Option B — function-owned cases.* In a function with no written contract,
  `Err(.NotFound(name))` whose case name matches no enum in scope adds a case
  to that function's own error enum, shown as `find.Error`. It is closed and
  nominal and widens like any other member. Two functions' `.NotFound` cases
  are different types; a caller matching a union that holds both must qualify
  (`.Err(.find.NotFound(n))`), and the error says so. `jet fix` can promote
  `find.Error` to a named `#Error enum FindError` when it is exported.

  ```jet
  fn find(book: [String:String], name: String) -> String {
      if book.has_key(name) -> return Ok(book[name])
      Err(.NotFound(name))          // defines find.Error.NotFound(String)
  }
  ```

- *Option C — Roc structural tags.* Same-named cases unify across functions,
  and `[NotFound(String), BadNumber(String)]` becomes a type spelling.
- *Costs:* B adds one inference rule and a display name (`find.Error`), no new
  token; the collision rule is the main learning cost; exported functions must
  still write a contract under D-FAIL-PUBSIG1=A, which forces promotion to a
  named enum at the edge. C adds a new type system feature, row variables, and
  host-boundary rules (Roc itself bans extensible unions across the host
  boundary).
- *Recommendation:* B if 1.1 is A; otherwise A. C is not recommended.

### 1.3 Wrapping an error at the propagation point

**What it is.** `? |e| Wrap(e)` or `? Wrap` maps the error before returning
(all-syntax file, `question_with_err_map`). Roc run (r3):

```roc
fetch = |raw| {
	url = parse_url(raw) ? |e| FailedToFetch(raw, e)
	Ok("<html from ${url}>")
}
```

**Jet today.** The fallback carries the ambient `err` (D-FAIL-BIND1=A,
D-ERR-DECON1=A), and early exits are `?? return` (D-ORRETURN-CANON1). The same
program runs (p11, output `could not fetch ftp://b: bad url ftp://b`):

```jet
fn fetch(raw: String) -> String AppError! {
    url :: parse_url(raw) ?? return Err(AppError.FailedToFetch{url: ~raw, cause: err})
    Ok("<html from {url}>")
}
```

and nested member patterns reach through it:
`.Err(.FailedToFetch(u, .BadUrl(why)))`. Jet also has what Roc lacks: lazy
notes `?(text)` and an automatic journey on every tier (D-FAIL-CTX1=A; #3713
tracks the missing path in the edge report).

**Verdict: Jet already covers this.** The Jet form is longer
(`?? return Err(…err)` versus `? |e| …`) but uses no extra mechanism. No
ballot. Noted during the probe: a named-field pattern inside a positional one,
`.Err(.FailedToFetch{url: u, cause: c})`, is a parse error (E0003), while the
positional form works. Whether named payload patterns should nest is a small
grammar question for whoever owns patterns.

### 1.4 No null: absence as a tagged error

**What it is.** Roc has no `null` and, by design, no `Maybe`/`Option`. The FAQ
argues that an operation that can fail should return `Try` "with an error type
that has a single tag describing what went wrong", for example
`List.first : List(item) -> Try(item, [ListWasEmpty, ..])`, and that a data
field should use a descriptive union such as `[Loading, Loaded(Artist)]`
instead of `Maybe(Artist)`
([FAQ: option type](https://www.roc-lang.org/faq#option-type)). The builtins
follow it: `Dict.get : Dict(k, v), k -> Try(v, [KeyNotFound])`
(Builtin.roc 5955). `??` substitutes a default on `Err`
([operators](https://www.roc-lang.org/docs/main/langref/operators/)). Optional
record fields read through `.?field`, which returns
`Try(_, [MissingField])` (all-syntax file, `ServerConfig`).

A consequence: because absence is an `Err`, `?` on `List.first` or `Dict.get`
*early-returns from the function when the value is missing*.

**Jet today.** Jet keeps optional and fallible distinct but as two views of one
carrier (D-FAIL-CARRIER1=A): `T?` with `Val(x)`/`None`
(D-RESULT-OPTION-CANON1), `T E!` with `Ok`/`Err`, `??` for either, `?.` for
optional chaining, and `.or_err("why")` to give an absence a reason. D-OUTCOME-SHAPE1=A
(ratified 2026-09-29, implementation #3838) makes `T? E!` match as three states
`.Val(x)`, `.None`, `.Err(e)`, and rules that "a missing value never leaves a
function by itself". Today's nested spelling still runs (p3):

```jet
fn user_of(id: Int) -> String? ConfigError! {
    if id < 0 -> return Err(ConfigError.BadPort("negative"))
    if id == 0 -> return Ok(None)
    Ok(Val("ada"))
}
...
    if user_of(0) == {
        .Ok(.Val(name)) -> print(name)
        .Ok(.None) -> print("no such user")
        .Err(e) -> print("failed")
    }
```

**Verdict: Jet's model already does better on the point that matters, and
adopts Roc's other point through `.or_err`.** Roc's design is exactly the
behaviour D-OUTCOME-SHAPE1 option B described and the owner rejected: a lookup
miss skips the rest of the function. Roc's valid point, that an absence should
say *why*, is what `.or_err("why")` does, and descriptive data states are
available through ordinary enums.

One Jet gap is real. A plain value lifts into the fallible view but not the
optional view (p13):

```jet
fn count() -> Int DBError! {
    42                      // accepted
}

fn maybe_count() -> Int? {
    42                      // E0108: This needs Int?, but the value is Int … Wrap it with `Val(...)`
}
```

and the same E0108 fires for an argument, `describe("ada")` where the
parameter is `String?` (p12). Kotlin, Swift, and TypeScript lift silently.
The `Val(...)` wrappers appear throughout the examples
(`print(area(Val(Shape.Circle{r: 2})))` in `nested_variant_patterns.jet`).

**Candidate ballot D-OPT-LIFT1.**

- *Gist:* Should a plain `T` value be accepted where `T?` is expected, the same
  way a plain success value is accepted where `T E!` is expected?
- *Option A — lift wherever the expected type is `T?`:* returns, arguments,
  annotated bindings, fields, and list elements. `Val(x)` stays legal, as
  `Ok(x)` does today; patterns keep `.Val(x)`.
- *Option B — lift in returns only.*
- *Option C — keep `Val(...)` (today).*
- *Costs:* no syntax; one coercion rule, the mirror of the existing success
  lift, so it removes an asymmetry rather than adding a mechanism. `T??` is
  already E0309, so lifting is never ambiguous for nested optionals. Generic
  inference must prefer the unlifted solution (`f<T>(x: T?)` called with a
  `U?` binds `T = U`). No runtime cost; the layout is unchanged.
- *Recommendation:* A.

### 1.5 Exhaustiveness and open unions

**What it is.** Roc checks `match` exhaustively, warns on a redundant `_`, and
recommends naming each error tag instead of `_` so new errors are caught
(tutorial, "Exhaustiveness"). An open parameter type `[Red, Green, ..]` lets a
function accept wider unions if it has a catch-all
([tag unions](https://www.roc-lang.org/docs/main/langref/tag-unions/)).

**Jet today.** Named enums are closed; a caller may add `else` for
future-case tolerance, and publishers cannot force it (D-ENUM-EVOLUTION1=A).
That is the same trade Roc's open unions make, chosen per caller. Exhaustive
matching over error unions works when the members are enums (p2), but not when
they are `#Error struct` types (p8):

```jet
fn increment_first(xs: [String]) -> Int (ListWasEmpty | BadNumStr)! { ... }

        if increment_first(xs) == {
            .Ok(n) -> print(n)
            .Err(.ListWasEmpty(_)) -> print("empty")
            .Err(.BadNumStr(_)) -> print("not a number")
        }
```

```text
Error [E0307]: This `if` doesn't cover every case — missing: Err(...)
```

The same failure occurs with non-empty structs (p8c). The shipped example
`Examples/features/errors/union_error_contract.jet` works around it with a
nested match, an `else -> {}` arm, and a trailing `return "unreachable"`.

**Verdict: Jet's policy is already right; the checker has a defect** (Part 3,
D1). Also, the message should name the missing members, as Roc's does
("Missing patterns: Err BadNumber _"), not `Err(...)`.

### 1.6 Structural records

**What it is.** Roc records are structural and may be open:
`{ name : Str, .. }` accepts any record with at least that field; records can
be built, updated with `{ ..record, name: "New Name" }`, and destructured
without declarations ([types](https://www.roc-lang.org/docs/main/langref/types/);
tutorial "Records").

**Jet today.** Named tuples are closed structural records and work as
parameter types (p9, prints `Ada is 36`):

```jet
fn greet(person: (name: String, age: Int)) -> String {
    "{person.name} is {person.age}"
}
```

A braced `{name: "Bob", age: 40}` is an inferred-construction form that needs
an expected nominal type (p9: E0119 "`{ … }` needs a known struct type here"),
despite D-LIT-DOT1=B describing it as an "anonymous record".

**Verdict: do not adopt open records.** Row polymorphism mainly serves
error-union width in Roc, which 1.1 handles with inferred unions instead; for
data, nominal structs document intent and keep diagnostics simple. No ballot.
The D-LIT-DOT1 wording versus the E0119 behaviour is worth a spec check.

### 1.7 Whole-program type inference

**What it is.** "All type annotations in Roc are optional"; inference is
"sound, decidable, principal"
([tutorial, "Types"](https://github.com/roc-lang/roc/blob/main/docs/mini-tutorial-new-compiler.md)).
It is rank-1 Hindley–Milner with no higher-kinded types and no subtyping
([types](https://www.roc-lang.org/docs/main/langref/types/);
[FAQ](https://www.roc-lang.org/faq#arbitrary-rank-types)). Annotations may
contain holes: `number_operators : I64, I64 -> _` and `Try(I64, _)`.

**Jet today.** Named functions write parameter types (p4: E0003 "Every
parameter except `self` needs a type after its name"); lambdas infer where the
expected type fixes them (D-LAMBDAINFER1, D-LAMBDA-INFER1); effects are
inferred and may stay unwritten (D-EFFECT-OMIT1=A); failure sets are planned
to be inferred (#3708). `_` is not a type hole (p14: three cascading errors,
E0119, E2417, and E2404, the last of which says "`?` can't turn…" although the
program writes no `?`).

p4b shows a sharper edge. Omitting `-> Int` silently makes the function unit
and discards its tail value; the error lands at the caller:

```jet
fn average(a: Int, b: Int) {
    (a + b) / 2
}
...
    print(average(4, 6))
```

```text
Error [E0112]: `print` doesn't know how to show `Unit`
  --> p4b_infer_ret.jet:7:11
```

**Verdict: Jet's split is right for its priorities; do not adopt
annotation-free functions.** The philosophy ranks reasoning and reading first,
and signatures are the one place a reader learns a function's contract without
reading its body. Roc's own r1b output shows the price of full inference: a
precise but unreadable inferred type. Jet already infers the two things that
are tedious and mechanical to write, effects and (soon) failures, and pins them
at public edges. Type holes are not needed once 1.1 lands, because omitting the
failure contract *is* the hole. The p4b diagnostic is a defect (Part 3, D6).

### 1.8 `crash` and `...`

**What it is.** "`crash` is not for error handling"; it is for unreachable
branches and infeasible recovery, and "what happens after a crash is
determined by the platform" (tutorial). `...` is a placeholder that "gets
translated under the hood to `crash "not implemented"`" (all-syntax file). Roc
run (r4): `Roc application crashed with this message: Expected a nonempty
list.`, exit 1. Nonblocking compilation (1.13) also reports a reached type
error as the bare text `runtime error` (r3).

**Jet today.** `panic("msg")`, `assert`, `require`, contracts, bounds traps,
and `#Todo` all produce registered E30xx reports through one renderer and exit
70, distinct from a returned error (exit 1) and a Jet defect (exit 101)
(S36, D-FAIL-BREACH1=A, D-FAIL-EXIT1=A). p6 shows the frame:

```text
Stop [E3001]: `panic: expected more than one digit`
  --> p6_assume.jet:4 in digits_to_num
```

**Verdict: Jet already does better.** Distinct exit codes and registered
reports serve both beginners (what/why/fix) and operators (scriptable exit
law). Small wording nit: an `assert` failure reads `panic: …`.

### 1.9 `dbg`

**What it is.** `dbg x` or `dbg(x)` prints for debugging. "Although printing
is an I/O operation, the `dbg` statement can be used even in pure functions",
and output may appear at compile time when a top-level constant is evaluated
(tutorial, "`dbg` statements"). The langref lists `dbg` and `expect` among the
things pure functions may do because "program behavior should never depend on
them" ([functions](https://www.roc-lang.org/docs/main/langref/functions/)).
Roc run (r2): `[dbg] 1.0` lines on stderr, kept in `--opt=speed` builds with a
warning: "Builds with --opt=speed keep dbg output, but dbg is intended for
debugging."

**Jet today.** No `dbg` builtin is registered: the spelling `"dbg"` appears
nowhere in `crates/jet-foundation/src`, and the builtin list in
`crates/jet-foundation/src/Syntax/math_layout.rs` holds `panic`, `wrapping`,
`saturating`, `checked`, `approx`, `assert`, `assert_eq`, `embed_*`, `fetch`.
In an inferred-pure function, `print` silently adds `IO` to the inferred row
(p5b, `effects=[IO, Mem.Alloc]`); for an exported function that is an API
change under D-EFFECT-OMIT1's semver rule. In an explicitly pure function it is
an error (p5):

```text
Error [E3401]: `strict` calls the impure function `print`
 Fix: Give `print` an explicit `-[]>` bound, or remove the call from `strict`
```

(The fix text asks the user to change a Prelude builtin; Part 3, D5.) `#Memo`
functions must be pure, so they cannot be print-debugged at all. The
debugger (`jet debug`, D-DBG2/3) is the only route.

**Verdict: adopt, adapted.** Debugging pure code is common; I/O effects exist
to track behaviour the program depends on, and a debug trace is not that.

**Candidate ballot D-DBG1.**

- *Gist:* Should Jet have a debug print that is allowed in pure code and does
  not change a function's effects?
- *Option A — `dbg(expr)`, dev and test only.* Returns its argument unchanged
  and writes `[dbg] file:line expr = {value:Debug}` to standard error. It adds
  no effect to the row and is allowed in `-[]>`, `#Memo`, and comptime code (a
  memoized function prints only on a cache miss). `jet build --release`
  rejects a remaining `dbg` with a registered error, and `jet fix` removes it.

  ```jet
  #Memo
  fn score(word: String) -[]> Int {
      total :: dbg(word.len() * 3)
      total + 1
  }
  ```

- *Option B — `dbg(expr)` everywhere, warning in release* (Roc's choice).
- *Option C — none; use `jet debug` or declare `-[IO]>` (today).*
- *Costs:* one builtin registered in `Syntax.rs` (I7) with a decision ID; one
  Prelude function so every tier prints the same line (I9); one registered
  release-build diagnostic with a UI snapshot (I4). No release runtime cost
  under A. It is not a second output mechanism because it cannot be used for
  program output in a release build.
- *Recommendation:* A.

### 1.10 `expect`: tests and dev-only assumptions

**What it is.** Top-level `expect` is a test run by `roc test`. Inside a
function, `expect` is an assumption: in `roc test` and debug builds it fails
loudly, and in `--opt=speed` builds it is skipped. "These are _not_ production
assertions"; the three production choices (recover, don't detect, `crash`) are
left to other code (tutorial, "`expect` statements"). Roc run (r2): `roc`
printed `Expect failed: expect failed`, *kept running*, and exited 1; `roc
test` reported both the failed top-level expect and the inline one ("This test
ran 2 times: 1 passed, 1 failed"); the `--opt=speed` binary skipped it and
exited 0.

**Jet today.** `#Test("name") { … }` blocks with `assert`/`assert_eq`,
`.setup`, `.expect_fail`, `.skip`, `.timeout`, `.measure`, property tests, and
expected-fail tests (D-CLAIM-WORD1=B and `Examples/features/tooling/`).
In-body checks are `assert`, `require`, and `#Pre`/`#Post`, checked in every
build and erased only by proof or an explicit strip opt-out (D-PREPOST1,
D-FAIL-TIER1=A).

**Verdict: Jet already does better; do not adopt dev-only assumptions.**
Safety ranks first in Jet's priorities, and a check that silently disappears
in release builds is a second meaning of "assert" (I8). Roc's own inline
expect message did not report the failing values. Jet's `assert_eq` shows a
diff; plain `assert` could show operand values (Part 3, D8, optional).

### 1.11 Effects: purity inference and the `!` suffix

**What it is.** Roc splits functions into pure and effectful. "A function is
effectful if it calls another effectful function, and otherwise it's pure";
effectful function types use `=>` and names end in `!`; "Roc's compiler
reports a warning if an effectful function's name does not end in `!`"; and
"by design, Roc has no syntax for 'either pure or effectful'" (no effect
polymorphism) ([functions](https://www.roc-lang.org/docs/main/langref/functions/)).
A wrong purity annotation is meant to be a warning so a debug I/O line does not
force annotation churn, though today "the compiler reports an incorrect purity
annotation as a type mismatch error". Roc run (r3) confirms both the type
mismatch ("This function is effectful, but a pure function is expected") and
the naming warning ("its name must end in `!`").

**Jet today.** Effects are a closed set of grantable roots with open dotted
leaves (D-EFF4/5, D-EFFTREE1), inferred and omittable on every function
(D-EFFECT-OMIT1=A), pinned by an explicit upper bound (`-[FS.Read]>`), denied
with `-[!Net]>` (D-PROP1/2), passed through for higher-order functions
(D-EFF2), and fused into the arrow (D-EFFECT-ROW2=B). p5b shows the inferred
facts: `twice … effects=[]`, `noisy … effects=[IO, Mem.Alloc]`.

**Verdict: Jet already does better.** It answers *which* effect, not only
*whether*, supports denial and polymorphic pass-through, and publishes inferred
rows in API snapshots. Roc's one advantage, a call-site mark (`!`) visible in
plain text, is the question D-FAIL-VISIBLE1 already poses for failures; the
same tool-shown approach would serve effects. One Jet gap: `#Memo` requires an
explicit `-[]>` even when the inferred row is empty (p7: E0938 on
`fn slow_square(n: Int) -> Int { n * n }`, whose inferred row p7c shows as
`effects=[]`), which contradicts D-EFFECT-OMIT1's "A function is pure when its
inferred row is empty" (Part 3, D3).

### 1.12 Abilities, now static dispatch

**What it is.** Roc's alpha-era abilities
(`A := U8 implements [Eq, Hash]`,
[alpha-4 syntax test](https://github.com/roc-lang/roc/blob/alpha4-rolling/crates/compiler/test_syntax/tests/snapshots/pass/opaque_has_abilities.expr.roc))
are replaced by static dispatch: methods in a `.{ }` block on a nominal type,
structural method constraints `stringify : a -> Str where [a.to_str : a -> Str]`,
operators that desugar to well-known methods (`+` to `plus`, `==` to `is_eq`),
and derived methods opted into with `is_eq : _`
([static dispatch](https://www.roc-lang.org/docs/main/langref/static-dispatch/)).
"Roc's only ad hoc polymorphism system is static dispatch, and dynamic dispatch
is unsupported by design."

**Jet today.** Named traits with `impl`, trait bounds, trait objects, and
auto-derived `equal`/`compare` (visible in p2's facts), `Debug` auto-derived
and `Display` explicit (D-DISPLAYDBG1).

**Verdict: keep Jet's traits.** A named trait is a documented, greppable
contract; Roc's duck-typed `where [a.method : …]` trades that away to avoid
declaring abilities. Nothing here bears on errors or absence. No ballot.

### 1.13 Nonblocking compilation

**What it is.** "`roc` will still run your program and `roc test` will still
run your tests when possible, even if you have compile-time
errors—including static type mismatches", while still exiting non-zero
(tutorial, "Types"). Roc run (r3): two type errors were printed, the program
ran its first two lines, then stopped at the first reached broken function with
`Roc application crashed with this message: runtime error`.

**Jet today.** `jet run` refuses a program with errors (every failing probe
above exits 1 before running).

**Verdict: not recommended now.** It serves fast iteration, but it needs a
defined runtime meaning for ill-typed code on every tier (I9), and Roc's own
report for the reached error is contentless. Candidate text for the record:

- **D-RUN-BROKEN1** — *Option A:* `jet dev` (never `jet build`) runs a program
  whose errors are confined to functions not yet called; reaching one stops
  with that function's compile report and exit 70. *Option B:* keep refusing
  (today). *Recommendation:* B until the report can carry the original
  diagnostic on every tier.

---

## Part 2. Skip

### 2.1 Memoization with reactive invalidation

**What it is.** Skip's headline feature: "When Skip's type system can prove
the absence of side effects at a given function boundary, developers can opt-in
to safely memoizing that computation, with the runtime ensuring that previously
cached values are invalidated when underlying data changes"
([skiplang.com](https://skiplang.com/)). The spelling is a function modifier
(`memoized`, listed with `async`, `native`, `private`, `untracked` in the
[function grammar](https://github.com/skiplang/skip/blob/master/docs/specification/Functions.md)):

```skip
memoized fun combineInt(n1: Int, n2: Int): Int {
  print_string("Combine Int: " + n1 + " + " + n2);
  n1 + n2 * 3
}
```

([tests/src/native/memoize1.sk](https://github.com/skiplang/skip/blob/master/tests/src/native/memoize1.sk);
the tutorial's `/* memoized */ fun move(p: Point): Point` exercise shows
`debug(...)` inside a memoized function). The runtime records which mutable
*Cells* a memoized call read and which memoized calls it made, and a *Mutator*
commits Cell changes as transactions that invalidate dependents; readers see a
consistent snapshot through MVCC
([How memoization works](https://github.com/skiplang/skip/blob/master/docs/blog/2017-01-04-how-memoization-works.md)).
"Arguments to inputs and outputs to memoized functions must be `frozen`"
([Understanding mutability](https://github.com/skiplang/skip/blob/master/docs/blog/2018-04-10-understanding-mutability.md));
`memoized fun no(mutable Ref<Int>): void` is a compile error
(`tests/src/frontend/typechecking/invalid/memo_mut_1.sk`).

**Jet today.** `#Memo` on a pure function with a bounded shared store
(`128` entries, `#Memo(bound: none)` for unbounded), lazy `Iter<T>` refused,
`name.cache()` statistics, and `#Memo` computed fields whose cached value drops
when a stored sibling in its dependency graph is written (D-MEMO1=A,
D-FIELDMEMO1=A). Dependency-tracked reactivity is a library: `#Reactive`
scopes over `core.reactive` `Signal<T>`, `Computed<T>`, and `Effect` with
explicit-by-read subscription (D-REACT1 family, syntax-decisions "Reactive,
events & UI stack").

p7 (explicit `-[]>`) ran and printed `144`, `144`, `169`, but
`slow_square.cache()` reported `hits 0 misses 0` on `jet run`, and
`jet build` of the same file was an internal compiler error (rustc: expected
`i64`, found `JetInt` in `jet_memo_stats`).

**Verdict: Jet already has the parts; no language adoption.** Skip's pure
`memoized` is Jet's `#Memo`; its field-level invalidation is D-FIELDMEMO1; its
Cell graph is `core.reactive`. What Skip adds is scale machinery (MVCC
snapshots, coarsening of the dependency graph), which is a runtime-library
question for `core.reactive`, not a language one. The memo defects go to
Part 3 (D3, D4).

### 2.2 Mutability modes and `freeze`

**What it is.** Objects are immutable by default. A class that can have
mutable instances is declared `mutable class`, fields are `mutable`, instances
are built with `mutable Point(1, 2)`, and `mutable Point` is a different type
from `Point`. `readonly X` accepts either; `frozen` methods exist only on the
immutable form; `freeze(obj)` makes an immutable deep copy, often optimised
away when the mutable value does not escape
([mutability](https://github.com/skiplang/skip/blob/master/docs/overview/mutability.md)).
"There are no global mutable values."

**Jet today.** `::` binds immutably and `:=` mutably; `&self` methods mutate
(D-MUTSELF1); `freeze(x)` returns a deeply immutable value and makes it safe
to capture in tasks (D-CONC-FREEZE1=A). p10 ran: a lambda may mutate a
captured `:=` binding (`xs.each(x -> total += x)` printed `6`), and
`freeze(names)` worked.

**Verdict: Jet already covers the useful part.** Skip's three-mode object
types are heavier than Jet's binding-level marks and would be a second
mutability mechanism. Roc makes a related choice worth noting: a `var` "can't
be reassigned inside a different function from where it was declared", so a
`for_each!` callback visibly cannot mutate outer state (tutorial, "`for`
style"). Jet allows it (p10). No ballot proposed; this belongs with any future
review of capture rules.

### 2.3 Tracked and untracked code

**What it is.** `untracked fun` marks code that reads non-reactive state;
calling it from tracked code is an error
(`tests/src/frontend/typechecking/invalid/untracked_fun2.sk`: "it's an error
to call an untracked fun from a tracked context"), and `memoized untracked` is
rejected (`invalid/memoized_untracked.sk`).

**Verdict: Jet already does better.** Skip's tracked/untracked is a two-point
effect system; Jet's effect rows express the same rule (`#Memo` requires the
empty row) with finer grain.

### 2.4 Discarding values

**What it is.** "Skip enforces that only `void` values may be silently
discarded"; a meaningful non-void result is discarded by binding it to `_`
([functions](https://github.com/skiplang/skip/blob/master/docs/overview/functions.md),
"Unused Values").

**Jet today.** Discarding a fallible or `#MustUse` result needs
`.drop("reason")` (D-IGNORERET1/2, D-MARK-DISCARD1=A). A plain non-unit value
is dropped silently: `twice(2)` as a statement runs without a word (p15), even
though `twice` is, today, implicitly fallible. The p4b bug (a unit function
whose value tail is dropped) is the same hole at the end of a body.

**Candidate ballot D-DISCARD1.**

- *Gist:* May a non-unit value be dropped without a mark?
- *Option A:* A statement or body tail whose value is not unit and not used is
  an error. The fix names the likely intent: add `-> Int` for a body tail in a
  function without a return type, or write `_ :: expr` to discard on purpose.
- *Option B:* Only the body-tail case is an error (catches p4b); statements
  stay silent.
- *Option C:* A lint (advisory) for both.
- *Costs:* no new syntax (`_ ::` exists, e.g.
  `Examples/features/errors/error_context.jet`); one registered diagnostic.
  Builder-style APIs that return `self` would need `_ ::` or a unit-returning
  variant.
- *Recommendation:* B now; A after measuring how many corpus statements it
  touches.

### 2.5 Errors and absence in Skip

**What it is.** Skip used exceptions: `try`/`catch` with pattern-matched
handlers, `throw`, subclasses of `Exception`, and `invariant(cond, msg)` /
`invariant_violation(msg)` for bugs, which throw `InvariantViolation` and
usually terminate
([Exception-Handling](https://github.com/skiplang/skip/blob/master/docs/specification/Exception-Handling.md),
[Debugging](https://github.com/skiplang/skip/blob/master/docs/overview/Debugging.md)).
`await` re-throws a failed dependency's exception
([Functions spec](https://github.com/skiplang/skip/blob/master/docs/specification/Functions.md)).
Absence is `Option<T>` with `None()`/`Some(T)`, spelled prefix `?Int` "to keep
the `?` readily visible even with generics"
([nullability](https://github.com/skiplang/skip/blob/master/docs/overview/nullability.md)).
A `??` operator was proposed in 2018 as sugar for a match on `Some`/`None`
([DesignLog, "?? Operator"](https://github.com/skiplang/skip/blob/master/docs/developer/DesignLog.md)).
`Option` compiles to a flag or a sentinel so `Some` does not allocate
([Faster Option](https://github.com/skiplang/skip/blob/master/docs/blog/2018-02-20-faster-option.md)).

**Verdict: nothing to adopt.** Jet's value-route failures with automatic
propagation already improve on Skip's exceptions for reasoning (every failure
is in the signature or inferred). Two notes for the record: Skip argued for a
prefix `?` on readability grounds, the opposite of D-TYPE-SUFFIX1=A (Jet
chose the suffix after its own review; no action); and Skip's sentinel layout
for `Option` is the same kind of internal optimisation D-OUTCOME-SHAPE1's
technical note allows for the three-state carrier, subject to a paired
performance cell.

### 2.6 The successor framework

SkipLabs now ships "an open-source framework for building _reactive_ backend
services" with TypeScript interfaces over a runtime written in Skiplang
([SkipLabs/skip README](https://github.com/SkipLabs/skip)). It confirms that
Skip's lasting idea is the incremental-computation runtime, packaged as a
library. For Jet that points to `core.reactive` and service batteries, not to
language syntax.

---

## Part 3. Defects and wording gaps found by the probes

These are observations on snapshot `jet-debug-snapshot23`. They are not
ballots. Several may be affected by in-flight work (#3838, #3708, #3713).

| ID | Probe | Observed | Expected |
|---|---|---|---|
| D1 | p8, p8c | Exhaustive match `.Err(.ListWasEmpty(_))` / `.Err(.BadNumStr(_))` over `(ListWasEmpty \| BadNumStr)!` with `#Error struct` members is E0307 "missing: Err(...)". Enum members work (p2). | Exhaustive; message names missing members. |
| D2 | p2 (first form) | `print("failed: {e:Debug}")` where `e` is an `(ParseError \| LookupError)` error: internal compiler error on `jet run`, "MIR debug type … has no checked debug carrier", exit 101. | Debug output on every tier (I2, I9). |
| D3 | p7, p7c | `#Memo fn slow_square(n: Int) -> Int { n * n }` is E0938 "requires a pure function" though the inferred row is `effects=[]`. | Accept inferred-pure functions (D-EFFECT-OMIT1=A). |
| D4 | p7 | With `-[]>`, three calls then `slow_square.cache()` print `hits 0 misses 0` on `jet run`; `jet build` fails in rustc (`jet_memo_stats` expected `i64`, found `JetInt`), an internal compiler error. | Correct statistics on every tier. |
| D5 | p5 | E3401's fix says "Give `print` an explicit `-[]>` bound". | Advice the user can follow (declare an effect, or remove the call). |
| D6 | p4b | `fn average(a: Int, b: Int) { (a + b) / 2 }` compiles as unit; the error is E0112 at the caller. | A teaching error at the body tail suggesting `-> Int` (see D-DISCARD1). |
| D7 | p13 | E0358 (`DbError` should be `DBError`) is reported twice at an empty line past the end of the file, plus at the real sites. | One report per real site. The ratified D-OUTCOME-SHAPE1 ballot examples themselves use `DbError`. |
| D8 | p8 (first form) | E0501 for `[["41"], [], ["x"]]` suggests "`[Int].{}`": the dotted form D-LIT-DOT1=B retired, and the wrong element type. The sibling elements fix the type. | Infer the element type from siblings, or suggest `[String]{}`. |
| D9 | p14 | A single `_` in a type position yields three errors (E0119, E2417, E2404); E2404 says "`?` can't turn…" in a program with no `?`. | One error; wording about implicit propagation. |
| D10 | p1 | `print("failed: {e}")` on the default `Err` is E0915 "`Err` has no `Display` implementation". | Check against D-DISPLAYDBG1 and S80: `Err` carries a `message`, and beginners will interpolate it. |
| D11 | p15 | `twice(2)` as a statement discards an implicitly fallible result silently, though D-IGNORERET requires `.drop("reason")` for fallible results. | Consistent with whichever answer #3708 gives for `twice`'s failure set. |
| D12 | p11 | A named-field payload pattern inside a positional one (`.Err(.FailedToFetch{url: u, cause: c})`) is a parse error; the positional form works. | Decide whether named payload patterns nest. |

---

## Appendix. Probe index

Jet probes live in `~/.cache/jet-dev/scratch/rocskip/`; Roc probes in
`~/.cache/jet-dev/rocskip/`. Both are scratch and not part of the repository.

| Probe | Question | Result |
|---|---|---|
| r1 | Roc: accumulate two helpers' tags with no annotations | runs; exhaustive match with no `_` |
| r1b | Roc: drop one arm | "non exhaustive match … Missing patterns: Err BadNumber _" |
| r2 | Roc: `dbg` in pure code, inline and top-level `expect` | dbg prints; inline expect fails in `roc`/`roc test`, skipped in `--opt=speed` |
| r3 | Roc: `? \|e\| Wrap(e)`, `??`, purity annotation, run despite errors | wrap works; purity mismatch is an error, name suffix a warning; program runs until the first broken function |
| r4 | Roc: `crash`, `...` | "Roc application crashed with this message: …", exit 1 |
| p1 | Jet: no contract, two typed errors | two E2402 |
| p2 | Jet: written union | widens through automatic propagation; exhaustive enum-member match runs |
| p3 | Jet: `??`, `?? return Err(...)`, `?.`, nested `T? E!` match | runs |
| p4, p4b | Jet: omitted parameter / return types | E0003; silent unit return, E0112 at caller |
| p5, p5b | Jet: print in pure code; inferred rows | E3401; `effects=[]` / `[IO, Mem.Alloc]`, `errors=[Err]` everywhere |
| p6 | Jet: in-body `assert` | E3001 stop, exit 70 |
| p7, p7b, p7c | Jet: `#Memo` | inferred-pure rejected; explicit `-[]>` runs; stats zero; AOT ICE |
| p8, p8b, p8c | Jet: unions of `#Error struct` | exhaustiveness fails |
| p9 | Jet: named tuple parameter; braced record | tuple works; `{…}` needs a nominal type |
| p10 | Jet: captured mutation, `freeze` | both allowed |
| p11 | Jet: wrap at propagation | runs with `?? return Err(…err)` |
| p12, p13 | Jet: `T` into `T?` | E0108; success lifts into `T E!` but not `T?` |
| p14 | Jet: `_` type hole | three errors |
| p15 | Jet: discarded non-unit statement | silent |
