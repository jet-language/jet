# Decision ballots — open owner queue

Every open decision, and **nothing else**. The instant a decision is submitted it
leaves this file: it is recorded in the decision log in
[`syntax-decisions.md`](syntax-decisions.md) and removed here. No "recently
ratified" section, no decided history — decided decisions never reappear.

**House rule for whoever edits this file:** a full decision card carries a worked,
user-story example for each option (what a real person types, sees, and hits as an
error) — not abstract option tables. Decisions not yet drafted to that bar are
listed below as one-liners with a recommendation; expand one into a full card
(with examples) when it's time to decide it.

---

## Open decisions

Every open decision, listed and available now — nothing parked or hidden. Decide
any directly when it has a full card; for the one-liners, ask the dashboard to
**expand into a full card** (options + worked examples) when you want to decide it.
Submitting a decision records it in `syntax-decisions.md` and removes it from here.

### Language surface

### S83 — External-definition connector for `derive` / `impl` / `fn` (rec C)

You want ONE `Type<sep>name` connector, used consistently and Type-first, to attach things to a type from outside its body — `fn` keeps its keyword, the connector just injects the type before the name:

```jet
fn Point<sep>dist(self) -> Int { … }   // external method
impl Point<sep>Drawable { … }          // trait impl
derive Point<sep>Serialize             // derive (S56)
```

The constraint: the connector must be a token that isn't already spent. **Taken:** `=>` (lambda arrow, S46), `->` (return type / match arm), `::` (binding, D-BIND1), `.` (module + field, D-MOD1), `:` (type annotation), `@` (attribute/label), `#`. **Free (unused by the lexer today):** the whole tilde family, `:>`, `<-`, `<~`, and `$`/`\`. Each option below shows the `fn` form (same shape applies to `impl` and `derive`).

- **Option A — `=>` (your first pick).** *Taken* — it's the lambda arrow. Parseable here (item position can't be a lambda), but it gives `=>` two meanings and sits oddly next to `->`: `fn Point=>dist(self) -> Int`.

    ```jet
    fn Point=>dist(self) -> Int { self.x + self.y }
    ```

- **Option B — `~` (single tilde).** *Free.* Minimal, quiet, Type-first. Reads "Point's dist."

    ```jet
    fn Point~dist(self) -> Int { … }   //  impl Point~Drawable   derive Point~Serialize
    ```

- **Option C — `~>` (tilde-arrow, recommended).** *Free.* Keeps the arrow feel you liked, but is unmistakably distinct from both `->` and `=>`. Reads "Point extends-to dist."

    ```jet
    fn Point~>dist(self) -> Int { … }   //  impl Point~>Drawable   derive Point~>Serialize
    ```

- **Option D — `~~` (double tilde).** *Free.* More visually weighted than `~`, clearly a "binder" not an operator.

    ```jet
    fn Point~~dist(self) -> Int { … }
    ```

- **Option E — `~~~` (triple tilde).** *Free.* Maximum visual distinctness; heavier to type, more "ceremony."

    ```jet
    fn Point~~~dist(self) -> Int { … }
    ```

- **Option F — `:>` (colon-arrow).** *Free.* Reads "Point provides dist." Risk: visually near `:` (type annotation) and `>`.

    ```jet
    fn Point:>dist(self) -> Int { … }
    ```

- **Option G — `<-` (left arrow).** *Free.* "Point receives dist." Direction reads backwards for `fn` (the member flows *into* the type).

    ```jet
    fn Point<-dist(self) -> Int { … }
    ```

- **Option H — `.` in definition position.** *Free in this position* (no expression here to confuse with field/module access). Familiar — looks like the call site `point.dist()`.

    ```jet
    fn Point.dist(self) -> Int { … }   //  impl Point.Drawable   derive Point.Serialize
    ```

- **Option I — `for` keyword (Rust-style).** Familiar, but **reverses order** (trait/name first, not Type-first) and reads awkwardly for free `fn`s.

    ```jet
    impl Drawable for Point { … }
    fn dist for Point(self) -> Int { … }   // awkward
    ```

- **Option J — `extend Type { … }` block.** No per-item connector at all — a keyword block groups out-of-body members. Clean and Type-first, but it's a block, not the inline `Type<sep>name` shape.

    ```jet
    extend Point {
        fn dist(self) -> Int { … }
    }
    ```

**Recommendation:** **C (`~>`)** — free, keeps the arrow you wanted, and can never be confused with `->` (return) or `=>` (lambda); applies identically to `fn` / `impl` / `derive` and stays Type-first. If you want it quieter, **B (`~`)** is the minimal version of the same idea. Whatever you pick here also fixes S56's derive spelling.

### D-TOOL-SPLIT — Split lsp/fmt/lint out of the `jet` binary (no rec)

Editor tooling — format, lint, language server — can live inside the one `jet` binary or ship separately. This shapes install size, release cadence, and how an editor finds the LSP.

- **Option A — One bundled binary.** Everything is a `jet` subcommand; one install, one version.

    ```jet
    // shell
    jet fmt src/
    jet lint src/
    ```

- **Option B — Separate binaries.** Ship `jet-fmt`, `jet-lint`, `jet-lsp` independently so each can release and update on its own.

    ```jet
    // shell
    jet-fmt src/
    jet-lsp --stdio
    ```

- **Option C — Plugin model.** `jet` loads tools as plugins discovered at runtime.

    ```jet
    // shell
    jet fmt src/      // dispatched to the loaded fmt plugin
    ```

**Prior art — how other languages ship fmt / lint / lsp:**

| Language | fmt | lint / vet | LSP | Shape |
|----------|-----|------------|-----|-------|
| **Deno** | `deno fmt` | `deno lint` | `deno lsp` | One binary, all subcommands (Option A, pure) |
| **Gleam** | `gleam format` | (in compiler) | `gleam lsp` | One binary, all subcommands (Option A, pure) |
| **Zig** | `zig fmt` | (in compiler) | ZLS — separate community project | Mostly A; LSP carved out |
| **Go** | `go fmt` → bundled `gofmt` | `go vet` bundled | `gopls` — separate `go install` | A for fmt/vet; LSP separate |
| **Rust** | `cargo fmt` → `rustfmt` | `cargo clippy` → `clippy` | `rust-analyzer` — separate project | Separate binaries (B) behind subcommand wrappers, version-locked via rustup |
| **Python (Ruff)** | `ruff format` | `ruff check` | `ruff server` | One binary, all subcommands (Option A) |
| **C/C++ (LLVM)** | `clang-format` | `clang-tidy` | `clangd` | Separate binaries (B), no unified driver |

The dominant modern pattern is **A**: one binary, tool subcommands (Deno, Gleam, Ruff, Bun). The recurring exception is the **LSP**, which several toolchains ship separately (Zig/ZLS, Go/gopls, Rust/rust-analyzer) because an editor spawns it as a long-lived process on a different release cadence. **Pure Option C (runtime plugins)** has essentially no precedent among compiled-language toolchains — eslint/ruff plugins extend *rules*, not the binary.

**Tradeoffs:**

- **A — bundled.** One install, one version; fmt/lint/lsp can never disagree with the compiler about the language. Bigger binary; can't patch the formatter without a compiler release. Best beginner UX.
- **B — separate binaries.** Independent release cadence; smaller core install. But each tool must still link the same front end (see below), so you pay version-skew risk and N release artifacts for little gain. Editors must discover the right `jet-lsp` on `PATH`.
- **C — plugins.** Maximum flexibility, but a runtime discovery/ABI surface to maintain, no real prior art, and it fights "codegen is dumb / one source of truth." Highest complexity for the least demonstrated payoff.

**Jet-specific consideration:** the front end owns *all* semantics and every error message (I2/I3). fmt, lint, and the LSP each need the real lexer/parser/sema — the same crates the compiler uses. Under B or C they'd all have to link that front end as a shared library and stay version-locked anyway, so separation buys the *costs* (multiple artifacts, skew risk) with little of the benefit. That argues for **A**, with the LSP as the one candidate for a separate *artifact* (not a separate codebase) if editor release cadence ever demands it.

**Recommendation:** none on the owner's packaging-philosophy call, but prior art + the shared-front-end constraint both point at **A** (one `jet` binary, tool subcommands), reserving the option to split only the LSP artifact later.


### Pattern matching & ranges (cards c20, c25)

### D-PATW — Wildcard token in pattern position (rec A)

When matching an enum, you often want to match a variant but ignore its payload, or write a structural catch-all. Today you can bind-then-ignore (`Active(u) ->` and never use `u`, which warns) or use `else ->`. There's no "match, bind nothing" token. `_` is **not free**: it is a legal identifier char (so a bare `_` lexes today as a throwaway *name*) and the S34 digit separator in numerics — so any `_`-as-wildcard option means special-casing `_` in pattern position (the Rust precedent), not adding a new token. Pick the spelling.

- **Option A — `_` (underscore, recommended).** The universal "I don't care" token from Rust/Swift/Haskell/Go. Reads as a hole. Participates in the witness algorithm as "any value."

    ```jet
    if c {
        Active(_)  -> "someone is connected"
        Closing(_) -> "shutting down"
        _          -> "idle or unknown"
    }
    ```

    Forgot a variant and have no `_`:
    ```
    error[E0307]: this `if` doesn't cover every case — missing: Idle
    help: add an arm `Idle -> …`, or a `_ -> …` catch-all
    ```

- **Option B — bare `else` only (no pattern wildcard).** Keep D-IF1's `else ->` as the only catch-all; for ignored payloads, require a bound name (which then warns if unused) or a future `_`-as-name. Smaller surface, but `Active(_)` (ignore one field) is then impossible without naming + suppressing.

    ```jet
    if c {
        Active(name) -> "someone is connected"   // `name` unused → warning
        else         -> "other"
    }
    ```

- **Option C — `*` (star).** Free-ish token, "anything." But `*` is pointer deref in the expert tier (S58) and multiplication; overloading it in pattern position reads oddly next to those.

    ```jet
    if c {
        Active(*) -> "connected"
        *         -> "other"
    }
    ```

- **Option D — `_` for fields, `else` for catch-all (split).** Use `_` only as an ignored *payload field* (`Active(_)`), but keep `else ->` as the only tail catch-all (no bare `_` arm). Cleanest separation of "ignore a slot" vs "match anything," at the cost of two concepts.

    ```jet
    if c {
        Active(_) -> "connected"   // `_` = ignore this field
        else      -> "other"       // `else` = catch-all (not `_`)
    }
    ```

**Recommendation:** A. `_` is the single most recognized pattern token across languages; one spelling for both "ignore a field" and "match anything" is the mechanical-uniqueness answer (priority #4). It coexists with `else ->`, which stays for value/condition arms (D-IF1).

### D-PATR — Range patterns: semantics & exhaustiveness (rec A)

c20/D-PATR owns range-pattern **meaning + exhaustiveness at all positions** — both an arm head tested against the subject (`0..59 -> …`) and a range nested in a destructured payload slot (`Closing(500..599)`). One spelling, one exhaustiveness rule (open `Int`/`Char` always requires `else`/`_`). The range **token** is S22's single inclusive `..` (no `..=`) and is not re-decided here; **card c25** owns only the arm-head *sugar* (the terse `lo..hi ->` desugaring) plus its porting-hazard teaching errors, deferring to this card's checking. The one c20 question: are range patterns in scope, with the checker gap-checking their coverage?

- **Option A — yes, range patterns at all positions, reuse S22 `..` (recommended).** An arm head or a payload position may hold `lo..hi`; the checker gap-checks it, and the open `Int`/`Char` domain still always requires a trailing `else`/`_`. Falls out of the `bindings: Vec<Pattern>` widening this card already needs.

    ```jet
    // Closing(code: Int)
    if c {
        Closing(500..599) -> "server crash"
        Closing(_)        -> "clean close"
        _                 -> "still up"
    }
    ```

    Drop `Closing(_)` so the slot has a gap below 500:
    ```
    error[E0307]: this `if` leaves a gap — `Closing(_)` below 500 is not covered
    help: add an arm `Closing(_) -> …`
    ```

- **Option B — no; write the `&&` guard.** Keep payload slots bind-only; to test the inner value, bind it and add a guard (D-PAT2). Smaller surface (I8), but the guard "covers nothing for exhaustiveness," so you always need a fallback arm even when the bands tile.

    ```jet
    if c {
        Closing(code) && code >= 500 -> "server crash"
        Closing(code)                -> "clean close"   // required: guard can fail
        _                            -> "still up"
    }
    ```

**Recommendation:** A. Reuse S22's `..` (one spelling across loops, slices, and patterns), gap-check both arm-head and payload positions, and own the exhaustiveness rule (open `Int`/`Char` always needs `else`/`_`). The token defers to S22; c25 owns only the arm-head sugar shape + porting errors and defers its checking here — exactly one range concept, one exhaustiveness story.

### D-PATO — Structural or-patterns binding shared names (rec A)

Jet already has two "or" mechanisms in arm heads: S25 value-distribution (`200 || 404 -> …`) and D-IF1 bare-value arms. Neither lets you OR two *enum patterns that bind a payload*. Should `Active(u) || Closing(u)` (alternatives binding the same name `u`) be allowed?

- **Option A — reuse `||`, require identical bindings (recommended).** The alternatives must each bind the same set of names at the same types; the arm body sees those names. Same token as logical-or and S25 — no new sigil.

    ```jet
    // Status = enum { Active(id: Int); Reconnecting(id: Int); Closed }
    if s {
        Active(id) || Reconnecting(id) -> "live session {id}"
        Closed                         -> "done"
    }
    ```

    Mismatched bindings:
    ```
    error[E0317]: or-pattern alternatives must bind the same names
      --> s.jet:3:9
       |
     3 |     Active(id) || Closing(code) -> …
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ left binds `id`, right binds `code`
    help: bind the same name in both, or split into two arms
    ```

- **Option B — `|` (single pipe) for structural or-patterns.** Match Rust exactly (`A(x) | B(x)`), reserving `||` for the value-distribution / boolean sense. Clear visual split between "pattern alternation" and "logical or," but introduces a new sigil (`|` is currently only bitwise-or, S17 `|=`), and two near-identical spellings risk confusion.

    ```jet
    if s {
        Active(id) | Reconnecting(id) -> "live session {id}"
        Closed                        -> "done"
    }
    ```

- **Option C — reject; use separate arms.** Keep the surface minimal (I8). If two variants share handling, write two arms — or, when they share a *field*, bind it in each. Costs duplication for the genuinely-shared-body case.

    ```jet
    if s {
        Active(id)       -> live(id)
        Reconnecting(id) -> live(id)
        Closed           -> "done"
    }
    ```

**Recommendation:** A. Reuse `||` (already the or-spelling for S25/value arms), require identical bindings across alternatives (checked, E0317), and exclude this from the *minimum* c20 scope — land nested exhaustiveness first, add structural or-patterns as a clean follow-on once the matrix checker exists (it makes or-patterns nearly free).

### D-RANGE1 — Range arms in multi-arm `if` (rec A)

Multi-arm `if` (D-IF1) lets `200 ->` mean `subject == 200`. This card adds the range analog. Jet's range is inclusive `..` (S22); there is no `..=`.

- **Option A — reuse inclusive `..`, desugar to `>= && <=` (recommended).** One range syntax across the whole language (loops S22, slices S40, now arms).

    ```jet
    if score {
        90..100 -> "A";      // score >= 90 && score <= 100
        80..89  -> "B";
        else    -> "F";
    }
    ```

- **Option B — introduce `..=` for arm ranges only.** Matches Rust/Odin muscle memory, but splits S22 (two range tokens, one inclusive-by-default and one explicit-inclusive) — an I8/S22 violation for cosmetic familiarity.

    ```jet
    if score {
        90..=100 -> "A";     // a second range token Jet doesn't have
        else     -> "F";
    }
    ```

- **Option C — no sugar; keep the `&&` form (I8 default-no).** Smallest language. Cost: every band restates the subject twice; against the owner's standing anti-repetition direction.

    ```jet
    if score {
        score >= 90 && score <= 100 -> "A";
        else                        -> "F";
    }
    ```

**Recommendation: A.** Reuses the one ratified range token, reads as English, desugars to existing machinery (codegen untouched), and cuts the repetition the owner consistently trims. Exhaustiveness behaviour is governed by c20/D-PATR (gap-checking; open `Int`/`Char` always requires `else`) — see D-RANGE2.

### D-RANGE2 — Ownership of arm-head range semantics across c25 and c20 (rec A)

c20's **D-PATR** governs ranges in `if` arm heads — the *same* construct this card proposes. They are not two positions (arm vs. destructuring); they are two depths of the *one* arm-head feature. Jet must end up with exactly one range spelling (S22/I8) **and** one exhaustiveness rule.

- **Option A — S22 owns the `..` token; c20/D-PATR owns arm-head range *semantics* (checking + exhaustiveness); c25 owns only the desugaring shape + porting-hazard teaching errors; c25 may ship the sugar first, deferring to D-PATR's rules (recommended).** One spelling, one exhaustiveness story. c25 delivers the terse `lo..hi ->` arm and the `..=`/`step`/inverted-band errors now; when c20 lands its checker, the *same* arms gain gap-checking with no syntax change.

    ```jet
    // c25 ships this arm-head sugar (desugars to >= && <=, else mandatory):
    if code { 400..499 -> "client error"; 500..599 -> "server error"; else -> "?"; }

    // c20/D-PATR later deepens the SAME arm with gap-checking (no syntax change):
    if code { 400..499 -> "client error"; else -> "?"; }
    //         ^ checker can now report an uncovered band between arms
    ```

- **Option B — c20/D-PATR owns the whole arm-head range feature; c25 is folded into c20 and ships nothing on its own.** Single owner, zero divergence risk, but blocks the cheap, shippable-now sugar on the larger c20 sema effort.

    ```jet
    // c25 ships nothing until c20's range-pattern checker lands.
    ```

- **Option C — c25 and c20 each own arm-head ranges independently.** Rejected: two cards editing the same arm-head classifier and the same `..` spelling with possibly different exhaustiveness rules — a direct I8/S22 split. Listed only to close it.

    ```jet
    // two ratification paths converge on one grammar — the divergence hazard.
    ```

**Recommendation: A.** S22 owns the `..` token; c20/D-PATR owns arm-head range *meaning*; c25 ships the terse sugar + porting-error teaching now, under D-PATR's spelling and exhaustiveness rules. One spelling, one checker, no competing decisions.

### Type system (cards c22, c23)

### D-ERR-CONV — Typed error→error conversion across `?` (rec A)

Today `?` crosses a typed-error boundary only when the target is the universal `Error` (via `Fallible`, D-ERR2). A library with its own typed error family (`enum ConfigError { … }`) can't fold a lower-level `IoError`/`ParseError` into it without per-call-site ceremony. This card picks the mechanism *and its spelling* for declaring "a `Source` error becomes a `Target` error", which `?` then applies automatically. In every option, the conversion is declared once, total, and rejected unless declared (no silent/blanket coercion); the orphan rule (S28) applies.

- **Option A — `impl Source -> Target { … }` (recommended).** Reuses the existing `->` token and `impl` keyword; reads "Source becomes Target". `self` is the source error; the block returns the target. `Fallible` becomes the prelude's `impl T -> Error` instance, unifying the two mechanisms.

    ```jet
    enum ConfigError { Missing(String); BadInt(ParseError); Io(IoError); }

    impl IoError    -> ConfigError { ConfigError.Io(self) }
    impl ParseError -> ConfigError { ConfigError.BadInt(self) }

    fn load(p: String) -> Config ? ConfigError {
        val raw  = read_file(p)?;    // IoError    -> ConfigError.Io
        val port = parse_int(raw)?;  // ParseError -> ConfigError.BadInt
        Config { port }
    }
    ```

    Seen on a missing conversion:

    ```
    Error [E2404]: `?` can't turn an `IoError` into a `ConfigError` here
      --> config.jet:7:25
        |
      7 |     val raw = read_file(p)?;
        |                         ^
     Why: `?` only changes an error's type when you've declared how; there's no
          declared way to turn `IoError` into `ConfigError`
     Fix: add `impl IoError -> ConfigError { ConfigError.Io(self) }`
    ```

- **Option B — `convert Source -> Target { … }` keyword.** A dedicated word instead of `impl`, so error conversions read distinctly from trait impls. Costs a new keyword (`Syntax.rs` + I7 ID); `Fallible` stays separate rather than unifying.

    ```jet
    convert IoError    -> ConfigError { ConfigError.Io(self) }
    convert ParseError -> ConfigError { ConfigError.BadInt(self) }

    fn load(p: String) -> Config ? ConfigError {
        val raw = read_file(p)?;
        Config { port: parse_int(raw)? }
    }
    ```

- **Option C — method on the target via a trait (`FromError`).** Mirror Rust's `From`: a trait whose method builds the target from the source. Familiar to Rustaceans, but `from`/`Into` directionality is a known beginner stumbling block, and it needs a generic trait (`trait FromError<E>`), heavier than v1's signature-only traits (S28).

    ```jet
    impl ConfigError: FromError<IoError> {
        fn from_error(e: IoError) -> ConfigError { ConfigError.Io(e) }
    }

    fn load(p: String) -> Config ? ConfigError {
        val raw = read_file(p)?;   // ? calls ConfigError.from_error(e)
        Config { port: parse_int(raw)? }
    }
    ```

- **Option D — call-site explicit map, no auto-conversion.** Reject silent crossing entirely; the author maps at each `?` with a fallback. Most explicit, zero new declaration form — but repeats the mapping per call site and clutters the happy path (the very thing the card exists to remove).

    ```jet
    fn load(p: String) -> Config ? ConfigError {
        val raw  = read_file(p) ?? return err(ConfigError.Io(it));
        val port = parse_int(raw) ?? return err(ConfigError.BadInt(it));
        Config { port }
    }
    ```

**Recommendation:** **A (`impl Source -> Target { … }`)** — no new sigil or keyword (I7-clean), reuses `impl`'s mental model and orphan rule, and unifies with the existing `Fallible`/`to_error` path (which becomes the prelude's `impl T -> Error`), so the language gains zero net concepts (I8). It keeps the happy path clean (Option D's flaw), avoids Rust's `from`/`Into` directionality confusion (Option C), and avoids spending a keyword (Option B).

### D-DIST1 — Declaration spelling for distinct types (rec C)

Two families. **Keyword-first** matches existing type declarations (`struct`, `enum` are keyword-led items). **Binding-form** matches Odin's exact `distinct` spelling and reuses Jet's ratified `::` immutable-binding sigil (D-BIND1). Both introduce a new word `distinct`; the question is where it sits and what separator joins the name to the base. `struct UserId(Int)` is **not** an option — positional tuple structs and `.0` access are rejected (S73, E0048/E0049). Reopens D-SUGAR4 (newtype keyword declined 2026-06-16) on the new evidence of zero-cost transparent lowering + opt-in arithmetic + primitive feel.

- **Option A — `distinct UserId = Int` (keyword-first).** Reads like a type declaration; `distinct` is an item keyword beside `struct`/`enum`. `=` joins name to base (only ambiguous in expression position, not here).

    ```jet
    distinct UserId = Int
    distinct Meters = Float
    ```

- **Option B — `distinct type UserId = Int` (keyword + `type`).** Closest to Go/Rust alias spelling, but adds a second word `type` that exists nowhere else in Jet today. Heavier.

    ```jet
    distinct type UserId = Int
    ```

- **Option C — `UserId :: distinct Int` (binding form, recommended).** Reuses the ratified `::` immutable binding (D-BIND1): a type-level constant whose value is "a distinct version of `Int`." Exactly Odin's word in Jet's sigil. No new separator token. The `distinct` keyword is load-bearing — `UserId :: Int` (no keyword) would be the transparent alias D-SUGAR3 declined; the keyword makes this a *separate* type, not the rejected alias.

    ```jet
    UserId    :: distinct Int
    Meters    :: distinct Float
    ProductId :: distinct Int
    ```

- **Option D — `UserId := distinct Int` (mutable-binding form).** Rejected on sight — `:=` is the *mutable* binding sigil; a type is never reassigned. Listed only to close it.

    ```jet
    UserId := distinct Int   // wrong sigil; types aren't mutable
    ```

**Recommendation:** **Option C** — `UserId :: distinct Int`. It is Odin's spelling, reuses the already-spent `::` immutable sigil with no new token, and reads as plain English. Option A is the runner-up if the owner prefers type declarations keyword-first beside `struct`/`enum`. (Whichever wins, distinct-over-distinct chaining is rejected in v1.)

### D-DIST2 — Units of measure: in scope now, or deferred (rec A — defer)

Nominal distinct types (a `Meters` that won't mix with `Seconds`) are the small, contained feature. *Units of measure* add **dimensional algebra**: multiplying and dividing distinct numeric types yields *derived* units, and the compiler tracks the dimension through expressions.

- **Option A — distinct types only now; units deferred (recommended).** `Meters + Meters` works (opt-in same-type arithmetic via `#Numeric`, D-DIST3); `Meters * Seconds` is E0127 ("can't multiply two different distinct types"). No derived units. Small, shippable, doesn't foreclose units later.

    ```jet
    #Numeric
    Meters  :: distinct Float
    #Numeric
    Seconds :: distinct Float

    d :: Meters(100.0)
    t :: Seconds(9.58)
    speed :: d / t          // E0127: dividing two different distinct types isn't defined
                            //  (units of measure are a future feature)
    ```

- **Option B — full units of measure now.** `Meters / Seconds` yields a derived `Float<m/s>`; the compiler does the dimensional bookkeeping. Powerful, but a whole type-algebra subsystem (derived-unit synthesis, normalization, display) — far larger than nominal wrappers.

    ```jet
    Meters  :: unit Float
    Seconds :: unit Float

    d :: Meters(100.0)
    t :: Seconds(9.58)
    speed :: d / t          // type: MetersPerSecond, derived automatically
    print("{speed.raw()} m/s")
    ```

**Recommendation:** **Option A — defer units.** Ship nominal distinct types; leave a clean seam (`E0127` already says "units are a future feature"). Units are a strict superset and deserve their own card when the type system can carry dimensional algebra. Deferring is a widening, not a breaking change. Aligns with I8 and "measure twice, cut once."

### D-DIST3 — Coercion, unwrap, and arithmetic rules (rec A)

How a distinct type relates to its base. The safety of the whole feature lives here: any *implicit* base↔distinct coercion defeats the point.

- **Option A — explicit both ways; opt-in same-type arithmetic (recommended).** Construct with `UserId(expr)`. Unwrap with one named method `.raw()` (matches S42's `.to_int()`/`.to_float()` casts). **No** implicit coercion either direction. Arithmetic is **opt-in** via a `#Numeric` marker: a `#Numeric` distinct type inherits base operators only when both operands are the *same* distinct type, yielding that type; an unmarked distinct type gets `==` but no arithmetic (E0127).

    ```jet
    UserId :: distinct Int   // no #Numeric -> id, no arithmetic

    #Numeric
    Meters :: distinct Float

    u :: UserId(42)          // explicit construct
    n :: u.raw()             // explicit unwrap -> Int
    m :: Meters(3.0) + Meters(4.0)   // -> Meters(7.0)  (#Numeric)
    bad :: u + UserId(1)     // E0127: a UserId is an id, not a number
    ```

- **Option B — implicit unwrap to base (one-way coercion).** A distinct value is accepted anywhere its base is expected; only base → distinct needs `UserId(...)`. More convenient, but a `UserId` silently becoming an `Int` argument re-opens the mixup the feature exists to prevent.

    ```jet
    fn log_id(n: Int) { print("{n}") }
    u :: UserId(42)
    log_id(u)                // compiles under B — UserId silently decays to Int
    ```

- **Option C — explicit, but unwrap via field-like accessor `.value`.** Same as A but the unwrap reads `u.value` instead of a method `u.raw()`. Risk: looks like struct-field access and invites treating the distinct type as a struct.

    ```jet
    u :: UserId(42)
    n :: u.value             // unwrap via accessor
    ```

**Recommendation:** **Option A.** Explicit both directions keeps the safety guarantee whole; opt-in same-type-only arithmetic gives `#Numeric` distinct types primitive feel without leaking; `.raw()` reads as an intentional conversion in the S42 named-cast family.

### Compile-time (cards c24, c61)

### D-WHEN1 — Compile-time conditional spelling (rec A)

Jet has no compile-time `if`. The card asks for Odin's `when`, but `when` is retired (D-IF1). Below: how the user spells "compile this branch only". Gated on the owner accepting an extension of S57's "bindings-only" comptime scope.

- **Option A — `comptime if` (recommended).** Reuses two ratified words; reads as "the compile-time form of `if`." Condition is a comptime expression; only the selected arm is checked and lowered.

    ```jet
    comptime if target.pointer_bits == 64 {
        fold_u64(buf)        // only this arm compiles on a 64-bit build
    } else {
        fold_u32(buf)
    }
    ```

- **Option B — bare `comptime { }` block + ordinary `if` inside.** Smaller grammar delta, but it conflates "run at comptime" with "select at comptime" and reopens the general comptime-block can S57 closed.

    ```jet
    comptime {
        if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    }
    ```

- **Option C — `static if` (D / C++ spelling).** Familiar to D users, but `static` is an unspent word that would mean *only* this; adds vocabulary for one feature against I8, and "static" is jargon (diagnostics voice bans it).

    ```jet
    static if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    ```

- **Option D — reject; tell users to use runtime `if`.** Simplest (I8 default answer is no). Cost: no conditional compilation → off-target intrinsics can't be guarded → forecloses the freestanding/embedded story.

    ```jet
    if target.pointer_bits == 64 { fold_u64(buf) } else { fold_u32(buf) }
    // both arms compiled; fold_u64 must link on every target — the blocker.
    ```

**Recommendation: A.** It reuses ratified words, reads plainly, and is the narrow Odin form. Gated on the owner accepting an extension of S57's "bindings-only" comptime scope. The S26 dispatch law (no comptime type/trait/generic selection) is enforced by sema, not relaxed.

### D-WHEN2 — Checking of the unselected arm (rec A)

- **Option A — name-resolution only (recommended).** The dropped arm is scanned for unknown names so typos still teach, but it is not type-checked against its surroundings (an off-target intrinsic is allowed).

    ```jet
    comptime if false {
        wobble(x)        // E: nothing named `wobble` exists  (still caught)
    } else {
        ok(x)
    }
    ```

- **Option B — zero checking (pure Odin).** The dead arm is parsed and ignored. A typo survives silently until that arm is selected on some other build.

    ```jet
    comptime if false {
        wbidth_64()      // typo passes today, breaks the 64-bit build later
    } else { ok(x) }
    ```

**Recommendation: A** — matches Jet's "diagnostics are the product" priority #2.

### D-CT-L2NAME — Reconcile the two "Layer 2" labels (rec A)

"Layer 2" is overloaded across ratified spec text and the implementer needs to know which doc the new comptime work attaches to:

- **S26 layering**: Layer 1 = `comptime` bindings; **Layer 2 = built-in derives**; Layer 3 = reflection / user derives.
- **S60 card / card c61**: "comptime **Layer 2** = compile-time pure evaluation + data embedding."

Both are ratified phrasings on *different axes* (S26 = derive-machinery ladder; S60 = pure-eval capability tier). This card only decides how to label the work when it lands — no behaviour changes either way.

- **Option A — log it as an S60 extension; cross-reference S26 (recommended).** The pure-eval + embedding work is filed under **S60**, and the spec entry adds a one-line note that "Layer 2" here is the S60 *capability tier*, distinct from the S26 *derive layer*. No renaming of existing ratified text.

    ```jet
    // Doc/changelog framing only — code is identical under any option:
    comptime ROWS = parse_rows(embed_file("app.csv"))
    ```

- **Option B — rename one axis to drop the collision.** Re-label the S60 tier (e.g. "comptime *embedding tier*") so the word "Layer 2" is used by exactly one of them. Cleanest end state, but it edits ratified spec prose and breaks existing references.

    ```jet
    // Same code; the spec calls this the "comptime embedding tier".
    comptime ROWS = parse_rows(embed_file("app.csv"))
    ```

**Recommendation:** **A** — attach to S60 with an explicit cross-reference to S26. It settles the ambiguity without re-opening or rewording any ratified decision, and keeps the derive-layer numbering (S26) intact.

### Cleanup & error diagnostics (cards c21, c60)

### D-DEFER1 — `defer` for deterministic cleanup (rec B)

You declined `defer` once (D-SUGAR5) in favor of RAII (S63). RAII works well for std resource types, but a user **cannot write their own scope-exit cleanup today** — there's no user-implementable Drop. So when someone wants to restore a flag, log a span, or tear down a non-std handle on every exit path (including `?`), they hand-place the cleanup before each return and can miss one. This card asks how to close that gap.

- **Option A — keep declined; RAII (S63) stays the only cleanup story.** No new surface. The gap closes later via *user-definable Drop* (a separate roadmap item), not a control-flow keyword. Today's workaround for a flag is explicit:

    ```jet
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        if input.is_empty() {
            depth = (depth - 1)        // restore before THIS error return
            return err(ParseError.Empty)
        }
        node :: parse_node(input)?     // `?` exits WITHOUT decrementing — the missed path
        depth = (depth - 1)            // success path only
        return ok(node)
    }
    ```

- **Option B — add a stdlib `Guard` value (no new syntax, recommended).** Ship a `core` type whose Drop runs a stored lambda (closures S46/S47 exist; FileWriter's Drop already proves runs-code-on-scope-exit works). The user binds a guard; it fires on every exit path, LIFO with other Drops:

    ```jet
    use core.scope as scope
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        _g :: scope.guard(() => { depth = (depth - 1) })  // fires on every exit
        node :: parse_node(input)?                    // restore runs here too
        return ok(node)
    }
    ```

- **Option C — add an expert-tier `defer` keyword (block-scoped, Zig/Swift).** Real LIFO scope-exit on all paths; could later add `errdefer`.

    ```jet
    fn parse(input: String) -> Tree ? ParseError {
        depth := 0
        depth = (depth + 1)
        defer depth = (depth - 1)      // LIFO, fires on every exit path
        node :: parse_node(input)?
        return ok(node)
    }
    ```

**Recommendation: B — ship the stdlib `Guard` now; the `defer` keyword (C) stays declined; user-definable Drop is the real long-term roadmap item.** The gap behind c21 is *not* the absence of `defer` — it's the absence of user-writable cleanup. Option A alone leaves the gap open (its `?` line leaks). Option B closes it *today* with zero new syntax and zero ratification, on existing Drop + lambda machinery, staying inside S63 (a guard value *is* RAII). The keyword (C) reintroduces the Go leak-by-omission bug class S63 named; keep it declined unless **errdefer** (partial-build rollback) proves a recurring need user-Drop can't serve.

### D-NARG-DIAG — diagnostic codes/text for the named-args follow-ups (rec A)

D-NARG-D4 splits the call-site label-mismatch error out of the generic arity code (E0104). That needs a new sema code + its house-voice text. Separately, referencing a *later* parameter in a default needs a code. This card blesses both — implementation can't ship the snapshots (I4) until the text is settled. (D-NARG-D2 and D-NARG-D4 themselves are already ratified; this is product-copy only.)

- **Option A — mint E0125 for label mismatch + E0126 for later-param ref (recommended).** Two purpose-built codes; each teaches its own rule. E0125 covers both the transposed and unknown-label sub-cases.

    ```jet
    // E0125 (transposed): label names a real param, wrong position
    r :: Rect.square(height: 5, width: 3)
    // Error [E0125]: label `height:` doesn't match the parameter `width` here
    //  Why: labels are checked documentation — each names the parameter at its
    //       own position, and arguments stay in the order they're declared
    //  Fix: write `width:` here, or drop the label

    // E0126 (later-param ref in a default)
    fn f(a: Int = b, b: Int) -> Int { return a }
    // Error [E0126]: a default can only use a parameter declared before it
    //  Why: defaults fill left to right; `b` isn't bound yet when `a` defaults
    //  Fix: reorder so `b` comes before `a`, or use a constant default
    ```

- **Option B — mint E0125 for label mismatch, reuse E0107 for later-param ref.** One new code; the forward-reference reuses the existing "unknown name" code.

    ```jet
    fn f(a: Int = b, b: Int) -> Int { return a }
    // Error [E0107]: unknown name `b`
    //  Why: that name isn't in scope here
    //  Fix: define `b`, or check the spelling
    ```

**Recommendation: A.** E0125 is needed either way (D-NARG-D4 is ratified). For the forward-ref, E0126's reorder hint is the teaching win — E0107's generic "unknown name" sends the user looking for a missing definition when the real issue is parameter order. Both codes are free and sit naturally after E0124.

### Tooling & CLI (cards c11, c12, c13, c52, c54)

### D-CLI1 — Unknown `--`-flag before the `--` separator (rec A)

`--` cleanly forwards everything after it. The remaining question is the *ambiguous* case: a `--`-flag that appears **before** any `--`, which jet doesn't recognise as one of its own. What should `jet run app.jet --port 8080` do?

- **Option A — Error and teach (recommended).** Reject the unknown flag with a diagnostic that names the `--` form. Honest, no silent loss, teaches the convention once. The machinery already exists: `check_flags` already errors on unknown `--`-flags via E2102 — this just extends its Fix line to point at `--` for `jet run`.

    ```shell
    $ jet run app.jet --port 8080
    Error [E2102]: `--port` isn't a flag jet understands
     Why: flags before `--` belong to jet; everything after `--` is forwarded to your program
     Fix: jet run app.jet -- --port 8080
    ```

- **Option B — Silently forward unknown flags.** Any `--`-flag jet doesn't know gets forwarded to the program. Convenient, no `--` needed — but a *typo'd* jet flag (`--smal`) is then silently handed to the program instead of being caught.

    ```shell
    $ jet run app.jet --port 8080      # program gets --port 8080; no error
    $ jet run app.jet --smal           # typo silently forwarded, not caught
    ```

- **Option C — Forward with a one-time warning.** Forward the unknown flag but print a lint-style note suggesting `--`. Middle ground; adds noise to a common path and still forwards typos.

    ```shell
    $ jet run app.jet --port 8080
    Warning: forwarding `--port` to your program; use `jet run app.jet -- --port 8080` to be explicit
    ```

**Recommendation: A.** Jet rejects-with-a-great-message over guessing (philosophy + I8), and the E2102 path that does it already exists. `--` is one keystroke; teaching it once at the first ambiguous flag is cheaper than the class of silent typo-forwarding bugs B admits.

### D-PRELUDE1 — Which IO symbols are ambient (no `use`)? (rec B)

`print` is already ambient. The question is its siblings. Each option shows the same first program; the difference is whether line 2 needs a `use`.

- **Option A — Output only (Rust-style).** `print` ambient; *everything else*, including `input`, stays behind `use core.io`. Consistent with Rust; input is explicit.

    ```jet
    use core.io as io;          // ← required just to read a line
    fn main() {
        print("name?")
        let name = io.input()
    }
    ```

- **Option B — `print` + `input` (Python-leaning, recommended).** The two symbols a first interactive program needs are ambient; `eprint`, `args`, `read_all_input` stay qualified.

    ```jet
    fn main() {
        print("name?")
        let name = input()      // ← just works, like print
    }
    ```

- **Option C — All of `core.io` ambient.** `print`, `input`, `eprint`, `args`, `read_all_input` all ambient, no `use core.io` ever.

    ```jet
    fn main() {
        print("name?")
        let name = input()
        eprint("debug")         // stderr, also ambient
        let argv = args()       // CLI args, also ambient
    }
    ```

- **Option D — Status quo (`print` only, special-cased).** Leave it: `print` ambient, `input` requires `use`. (Listed for honesty; this is the inconsistency the card is trying to fix.)

    ```jet
    use core.io as io;
    fn main() { print("name?"); let n = io.input() }
    ```

**Recommendation: B.** It makes the model *consistent for beginners* (the two primitives a first program reaches for are both magic) while keeping the prelude tiny and the expert/tooling IO (`eprint`, `args`) explicit behind `use`. Concrete answer to the crux: under B (and C), `input()` works with no `use core.io`; under A and D it does not.

### D-L0201 — How to cut implicit-clone (L0201) noise (rec C — defer)

L0201 warns on every implicit `.clone()` even when the user can't usefully avoid it. Three honest responses:

- **Option A — Liveness gate (warn only on a wasteful clone).** Fire L0201 only when the value is dead after the call; stay silent when it's reused. Makes the lint *correct*, kills false positives by construction. Cost: a real last-use analysis threaded through four firing sites.

    ```jet
    let a = User(name)   // name reused below → silent (clone is necessary)
    print(name)

    let c = User(name)   // name never used again → L0201 (clone is wasteful, `move` helps)
    ```

- **Option B — Off-by-default + opt-in.** L0201 quiet by default; surfaced only on `jet run --lint=clones`. Cheap, no dataflow, clippy's resolution for true-but-noisy lints.

    ```shell
    $ jet run app.jet                 # quiet
    $ jet run app.jet --lint=clones   # opt-in, for tuning hot paths
    ```

- **Option C — Defer to post-v1, gather evidence (recommended).** Leave L0201 as it is. Before spending on A or B, collect a real corpus and count the false-positive rate, so the fix is sized to measured noise, not guessed noise.

    ```shell
    # no change today; the lint stays. Decision deferred until a corpus of real
    # Jet programs exists and the false-positive rate is measured.
    ```

**Recommendation: C (defer), with A as the eventual fix if evidence warrants.** The card already says "revisit post-v1 with evidence," and I8 backs it: A is a dataflow pass funded against a *hypothesized* noise level, before v1, with no baseline. Ship, measure the false-positive rate on real programs, then choose. Picking A or B *now*, pre-evidence, is the move I8 is meant to stop.

### D-DBG1 — Debugger entry point (rec A)

You can step through a Jet program in the editor or from the terminal. What does a person type to start a debug session, and where does the command live?

- **Option A — `jet debug <file>` (recommended).** A dedicated verb, parallel to `jet run` / `jet test`. Discoverable in `jet --help`; the editor launches the same command under the hood.

    ```shell
    $ jet debug examples/features/05_loops.jet
    breakpoint hit  loops.jet:7  in main()
    (jet-dbg) step
    ```

- **Option B — `jet run --debug <file>`.** No new verb; debugging is a mode of `run`. Fewer top-level commands, but couples a long-running interactive session to the "build and run to completion" verb, and the flag is easy to miss.

    ```shell
    $ jet run --debug examples/features/05_loops.jet
    breakpoint hit  loops.jet:7  in main()
    ```

- **Option C — editor-only, no terminal verb.** The DAP adapter is an internal binary the editor spawns; there is no documented terminal command. Smallest CLI surface, but no terminal-first debugging and harder to script/test.

    ```shell
    # nothing to type — press the editor's "Debug" button
    ```

**Recommendation: A** — a dedicated `jet debug` verb mirrors `jet run`/`jet test`, shows up in `--help`, and is scriptable/testable; the editor drives the same path.

### D-EVAL1 — Default output shape for `jet eval --pure` (rec A)

`jet eval --pure` produces a value. Is the default what a person reads, or what a machine parses?

- **Option A — pretty by default, `--json` for stable machine output (recommended).** Humans get indented, Jet-typed output; pipelines opt into the existing compact stable JSON. Reuses the global `--json` flag — no new surface.

    ```shell
    $ jet eval --pure totals.jet
    Report { total: 42, items: [Item { name: "a", qty: 3 }] }

    $ jet eval --pure totals.jet --json
    {"total":42,"items":[{"name":"a","qty":3}]}
    ```

- **Option B — JSON by default, `--pretty` to opt in.** Preserves today's exact behavior; machine consumers need no flag, but the common interactive case is the less readable one, and it adds a new `--pretty` flag.

    ```shell
    $ jet eval --pure totals.jet
    {"total":42,"items":[{"name":"a","qty":3}]}

    $ jet eval --pure totals.jet --pretty
    Report { total: 42, items: [Item { name: "a", qty: 3 }] }
    ```

**Recommendation: A** — the interactive case is the common one, pretty is the friendlier default, and it reuses the existing global `--json` flag instead of adding `--pretty`.
