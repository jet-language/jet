# Library Extensibility: How Far Can a Library Bend a Language?

**TL;DR.** A library can *always* add **vocabulary** (types, functions, methods, traits). The real question is whether it can add **grammar** (operators, literals, keywords, sigils). The answer depends on **how deep into the compiler pipeline you let a library reach** — and the deeper it reaches, the more power it gains in exchange for readability, tooling, and (critically for Jet) **error-message ownership.**

**Analogy.** A compiler is an assembly line: source text in one end, machine code out the other, through four stations. A library is an outside contractor. "What can libraries do?" really means *"how early on the line do you let the contractor bolt on their own station?"* Bolting a new part onto a **late** station (a new function) is safe and universal. Rebuilding an **early** station (the part that reads raw characters) is maximally powerful and maximally dangerous — because every station downstream, including your error messages and your IDE, now depends on a stranger's machine.

---

## 1. The pipeline (and where each kind of extension hooks in)

```
source text
   │
   ▼  LEX    — chop text into tokens ("words").     "5km" → [5][km]
   ▼  PARSE  — assemble tokens into a tree ("sentences").
   ▼  CHECK  — verify meaning: types, safety, YOUR diagnostics (sema).
   ▼  EMIT   — codegen → Rust → machine code.
```

Deeper hook = more power **and** bigger blast radius:

| Hooks at…            | Extension kind                          | Power   | What it threatens                |
|----------------------|-----------------------------------------|---------|----------------------------------|
| CHECK                | new types / fns / traits                | low     | nothing                          |
| PARSE ↔ CHECK        | blessed protocols (syntax → trait call) | low–med | nothing (grammar stays fixed)    |
| LEX ↔ PARSE          | token macros / DSL blocks               | medium  | readability                      |
| PARSE                | AST / proc macros                       | high    | error spans, tooling             |
| LEX / PARSE *itself* | reader macros, mutable grammar          | total   | the whole language + every tool  |

---

## 2. The five layers, concretely

| Tier | What it is (plain language) | Example | Danger |
|---|---|---|---|
| **0 Vocabulary** | New named things using existing grammar. | A `Matrix` type, a `sort()` fn, a `Drawable` trait. | none |
| **1 Blessed protocols** | Core defines a *fixed* piece of syntax + a hook; a library fills the hook. The library never invents grammar — it plugs into a slot the language already ratified. | `for x in coll` works on your type if you implement the iterator trait; `5km` works if you implement a literal trait. | low |
| **2 Token macros / DSL blocks** | A library rewrites a *marked* region of tokens before parsing. Looks like new syntax, but it's flagged at the call site and lives inside the existing tokenizer. | `vec![1,2,3]`, `sql!{ … }` | readability |
| **3 AST / procedural macros** | A library runs arbitrary compile-time code that emits tree nodes. Max power short of touching the grammar. | `#[derive(Serialize)]`, Template Haskell, Nim/Lisp macros. | **errors point at code the user never wrote** |
| **4 Reader macros & mutable grammar** | The library redefines the lexer/parser itself. **The only thing that lets a library add a real new sigil or keyword.** Needs a uniform syntax with no reserved words (Lisp) or a parser you can rewrite at parse time (Raku, Forth). | Common Lisp `set-macro-character`, Racket `#lang`, Raku "slangs". | the entire language + all tooling |

---

## 3. Cross-language reality

✓ = a library can do it · ~ = only via a blessed protocol or a weak/textual form · ✗ = core/compiler only

| Capability | Go | Rust | C++ | Python | Swift | Nim | Lisp/Racket |
|---|---|---|---|---|---|---|---|
| New types / fns / traits (Tier 0) | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Overload **existing** operators | ✗ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Define **new** operator symbols | ✗ | ✗ | ✗ | ✗ | ✓ | ✓ | ✓ |
| Custom literals | ✗ | ✗ | ✓ | ✗ | ~ | ~ | ✓ |
| Token macros / DSL blocks | ✗ | ✓ | ~ | ✗ | ✓ | ✓ | ✓ |
| AST / proc macros | ✗ | ✓ | ~ | ✗ | ✓ | ✓ | ✓ |
| New **keywords** (via library) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| New **sigils** (via library) | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |
| Mutate the grammar/parser | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | ✓ |

*(Haskell ≈ Swift/Nim for custom operators + macros, via custom fixity + Template Haskell. Raku/Forth ≈ Lisp's last column — mutable grammar. C++'s "macros" are two weak forms: the textual, unhygienic preprocessor and type-level template metaprogramming — neither is a real syntactic macro.)*

**The empirical lesson:** Python and Go are beloved with *almost no* library-level grammar extension — their power is great built-in syntax + blessed protocols (dunders, interfaces). Lisp/Raku sit at the opposite extreme and stay niche partly *because* unlimited extension fragments the ecosystem and breaks tooling. That data points straight at Jet's instincts.

---

## 4. Your two specific asks: keywords & sigils

These are the **hardest** things to make library-extensible, for one architectural reason: the lexer must recognize them **before** any parsing or checking happens — so a library would have to reach all the way to the front of the line.

- **Keywords** — impossible in any language that reserves keywords in the lexer (C, Go, Rust, Swift, …). Possible only where "keywords" are really just macro/function names (Lisp) or where the grammar is mutable (Raku). Most languages instead ship *contextual keywords* (`async`, `await`, `var`) — but those are added by the **language**, never by a library.
- **Sigils** (`$ @ # ~ %`) — same story. The one mechanism that unlocks them is a **programmable reader** (Lisp/Racket reader macros, Raku). Outside those, sigils are a closed set baked into core.

---

## 5. What Jet could do (technically) vs should do (philosophically)

Technically, Jet could adopt anything up to Tier 4. But three Jet invariants draw the line earlier **on purpose**:

1. **A human ratifies all surface syntax** → uncurated, library-invented grammar is out by default.
2. **The front end owns every diagnostic** (code + what/why/fix + snapshot), pointing at **user-written** source → naïve Tier 3+ expansion is out, because errors would point inside generated code.
3. **Simplicity ratchet** → prefer a great rejection over a powerful, fragile feature.

### The reframe worth adopting: *local* vs *global* footguns

Your philosophy says experts may opt into footguns. Split that in two:

| | **Local footgun** | **Global footgun** |
|---|---|---|
| Scope | only your program | the shared language + every reader & tool |
| Examples | disabling a check in your code; `unsafe` (post-v1) | new sigils; redefining `+` ecosystem-wide; mutable grammar |
| Jet stance | allow — gated + explicit | **reject, even for experts** |

Beginners are hurt **most** by global footguns (they open a library and the language looks alien). So Jet can be generous with local escape hatches and stingy with global ones — *without* betraying "control for experts."

### A tiered model that fits Jet

| Tier | Third-party libs | Stdlib / first-party | Marked at call site? | Likely version |
|---|---|---|---|---|
| **0 Vocabulary** | ✓ | ✓ | n/a | v1 |
| **1 Blessed protocols** | ✓ (implement hooks) | ✓ | invisible — feels native | v1 candidate |
| **2 Marked DSL blocks** | gated, opt-in | ✓ | **yes, required** | v2+ |
| **3 Compile-time codegen** | heavily gated / maybe never | ✓ (re-checked) | yes | v2+ |
| **4 Sigils / keywords / grammar** | **never** | core-only, ratified | — | — |

- **Tier 1 is the workhorse.** It lets third-party types feel native (`5km`, `for`, `[]`) with **zero new grammar**. This is how Jet delivers "magic by default" *safely*.
- **Tier 2** must be **visibly marked** so a beginner can tell core from library; hygiene + span-remapping mandatory; the library must ship Jet-grade diagnostics for its DSL.
- **Tier 3** — the front end re-checks emitted code and refuses to surface un-attributed errors. Possibly first-party-only at first.
- **First-party gets more rope than third-party** — and that's normal: Rust's lang items, Go's builtins, Swift's stdlib magic all do this. Jet just makes it **principled**: stdlib may use deeper magic because it's ratified and held to the same diagnostic bar by the same team.

### Two principles to bank

- **Mark library syntax.** Anything a library introduces should be visually distinct from core (a `!`, an attribute, a block form). Protects the beginner's mental model.
- **Diagnostics are the real ceiling.** The depth Jet can safely expose = the depth at which it can still guarantee a clean, attributed error. That single rule decides every tier above.

---

## 6. Tiny illustrative Jet

*(Syntax illustrative — subject to your spec in docs/01–02 and owner ratification.)*

```jet
// Tier 1 — blessed protocol: a type opts into a literal slot core already defined
unit Length
impl Literal for Length {
  suffix "km"
  fn from(n: Float) -> Length { Length(n * 1000.0) }
}
let d = 5km                  // feels native; no new grammar invented

// Tier 2 — marked DSL block: the marker tells a reader "library, not core"
let q = sql!{ select name from users where age > 18 }
//      └ visible flag; errors must map back to THESE tokens

// Tier 4 — rejected: a library cannot introduce a new sigil
// ~user                    // ✗ sigils are core-only and ratified
```

---

## 7. If you ever act on this

This is a landscape, not a feature — but the obvious first ballot is **Tier 1 (blessed protocols): which surface forms get a hook (`[]`, `for`, literal suffixes, `?`-style operators…), and is the hook-set open to third parties or stdlib-only?** That's the highest-value, lowest-risk extensibility Jet can ship, and it's the foundation everything else (if ever) builds on.
