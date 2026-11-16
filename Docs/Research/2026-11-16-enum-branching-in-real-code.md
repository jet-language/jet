# How real code branches on enums (2026-11-16)

Dated findings for D-GUARD-LINT1: how many cases enums have, how often code
tests a single case, and what an "every case must be named" rule would cost.
The numbers come from a heuristic scan (regular expressions plus brace
matching) of the sources listed below, run on 2026-11-16. They are evidence
for a decision, not a description of Jet's behavior.

## Corpora

|Language|Source|Files|
|---|---|---|
|Rust|About 1,040 crate versions in a local crates.io registry (syn, tokio, hyper, wgpu, regalloc2, rustls, arrow and others)|32,997|
|Java|OpenJDK 21 `src.zip`|15,619|
|Go|Go 1.26 standard library (enums are `iota` const groups)|7,709|
|TypeScript/JS|`node_modules` of four web apps|62,135|
|C|Linux 6.18 and glibc 2.42 headers (definitions only)|1,507|
|Jet|this repository: Compiler, Core, Jetpack, Tools, Examples, Tests|5,634|

Python's standard library defines too few `Enum` classes for the scan to say
anything (13 found), so it is left out. Rust's `Option` and `Result` are
counted apart from every other figure, since they would swamp them.

## 1. Most enums have more than a couple of cases

|Language|Enums|Median cases|3 or more|5 or more|10 or more|
|---|---|---|---|---|---|
|Rust|12,089|3|60%|29%|11%|
|Go|632|3|69%|39%|22%|
|Java|601|4|79%|40%|16%|
|TypeScript|712|5|80%|51%|26%|
|C headers|2,575|4|72%|47%|20%|
|Jet|2,108|3|66%|33%|12%|

Two-case enums are common in Rust (40%), and Option and Result are two-case
too. Every other language has at least three cases in most of its enums, and
a fifth to a quarter have ten or more: tokens, opcodes, keys, error kinds,
syntax nodes.

## 2. Testing one case is the most common branch outside Rust

These are the places where code distinguishes the cases of one value: a test
of one case (`x == E.A`, `if let E::A`, `matches!(x, E::A)`, `x === E.A`), or
a `switch`/`match`/table over several cases.

|Language|Single-case tests|Multi-case sites|Single share|Multi-case sites with a default|
|---|---|---|---|---|
|Java|2,918|539|84%|73%|
|TypeScript|2,571|579|82%|78%|
|Go|1,354|213|86%|48%|
|Rust|5,421|24,277|18% (34% with one-arm matches)|32%|
|Jet|3,067|15,678|16%|0.5% (E0641 refuses `else`)|

In Rust, 60% of the `match` sites that end in `_` name exactly one case.
They are single-case tests written as a `match`, so a third of Rust's
enum branches are "this case or not". Option and Result add 21,769 more
single-case tests (`if let Some`, `matches!(x, Ok(..))`), against 28,132
`match`es on them.

Where Rust tests a single case (5,421 sites):

|Context|Share|
|---|---|
|Inside a branch's body, as one step of the logic|59%|
|Inside a boolean expression (`&&`, `||`, `let b =`)|17%|
|A guard that returns, continues or breaks|9%|
|An assertion|9%|
|An `is_x()` helper|5%|
|An iterator predicate (`filter`, `any`, `position`)|2%|

57% of those tests are on enums with four or more cases, so single-case tests
are not a small-enum habit.

## 3. When code names some cases and defaults the rest

Rust's figures, where the compiler already forces every `match` to be
exhaustive and the `_` arm is the explicit choice:

- A default becomes more common as the enum grows. It appears in 19% of
  matches on enums with up to 3 cases, 40% at 4–5, 50% at 6–10, 64% at 11–20
  and 78% at 21–50.
- 80% of default sites name one or two cases.
- The default stands for 1 case at 21% of sites, 2–3 cases at 20%, 4–10 at
  23%, and more than 10 at 24%. Its median is 4 cases. It is 10 in Go, 15 in
  Java and 22 in TypeScript.

What the Rust default arm does:

|Default arm|Share|
|---|---|
|A constant: `false`, `None`, `0`, `Err(..)`|27%|
|`panic!`, `unreachable!`, `todo!`|25%|
|`return`, `continue` or `break`|11%|
|Nothing (`{}` or `()`)|7%|
|Other code|27%|

About 70% of defaults say "the rest does not matter here" (a constant,
nothing or a jump), or "the rest cannot happen here" (a panic).

The pattern holds across Rust domains, using each crate's `Cargo.toml`
categories. Defaults appear at 20–43% of match sites: highest in GUI,
graphics and games (43%), lowest in crypto, web and OS crates (20–26%). The
median rest is 2–3 cases, except CLI tools at 6. CLI tools also test single
cases the most, with 0.48 single tests per match against 0.18–0.35
elsewhere.

## 4. What naming every case costs: Jet's own code

Jet's tables already refuse `else` over an enum (E0641), so this repository
shows what the strict rule produces at scale:

- 6,152 table arms join two or more cases with `|`, and 1,879 of them join
  more than 20.
- 3,571 of 15,678 tables (23%) have an arm that joins four or more cases.
- The 3,156 arms that join ten or more cases hold 1.86 MB of case names,
  which is 2.3% of all Jet source text in the repository.
- 68% of those long arms do nothing (`{}`: 791) or return a constant or fail
  (1,348). For example, `Compiler/JetIR/Source/BuildLayout.jet` lists 19
  operators to say "any other operator fails", and
  `Compiler/JetSema/Source/Sema/Statements.jet` lists 12 statement kinds whose
  arm is `{}`.

## 5. What other languages do

|Language|Rule|
|---|---|
|Rust|`match` must be exhaustive; `_` is always allowed. Clippy's `wildcard_enum_match_arm`, which flags `_`, is an opt-in restriction lint. None of the scanned crates turns it on, and hyper turns it off explicitly. One crate warns on the narrower `match_wildcard_for_single_variants`.|
|Rust libraries|`#[non_exhaustive]` (16,458 uses in the scanned crates) forces downstream code to write `_`, so that a library can add a case without breaking its users.|
|Swift|`switch` must be exhaustive; `default` is allowed; `@unknown default` covers future cases of non-frozen library enums.|
|Kotlin|`when` on an enum must be exhaustive, statements included since 1.7; `else` is allowed.|
|Java 21|Switch expressions and pattern switches must be exhaustive; `default` is allowed; classic switch statements need not be exhaustive.|
|C/C++|`-Wswitch` (in `-Wall`) reports a missing case only when there is no `default`. `-Wswitch-enum`, which reports it even with a `default`, is in neither `-Wall` nor `-Wextra`.|
|Go|No exhaustiveness. The third-party `exhaustive` linter counts `default` as not covering cases, unless `default-signifies-exhaustive` is set.|
|OCaml|Exhaustiveness is warning 8, on by default. Warning 4, the "fragile match" that flags `_` over a variant, is off by default.|
|TypeScript|None built in; the `never` assignment idiom is used by hand.|

No mainstream language refuses a catch-all over an enum by default. Every rule
that does is an opt-in lint. Jet's E0641 is already stricter than all of them.

## Findings for D-GUARD-LINT1

1. **Single-case tests must stay free.** They are 82–86% of enum branches in
   Java, TypeScript and Go, and a third in Rust. They appear in boolean
   expressions, guards, assertions and predicates, where a table cannot go.
2. **"Name every case" touches most enums.** Over 60% of enums have three or
   more cases.
3. **The cost depends on the size of the rest, not on the rule.** About half
   of Rust's default sites cover three cases or fewer, where a generated list
   is short and catches a later case. About a quarter cover more than ten,
   where the list is a wall that mostly says "nothing" or "fail". Jet's own
   code already carries 1.86 MB of such lists.
4. **Big enums are exactly where writers want a default** (78% of Rust
   matches on enums of 21–50 cases), and they are the enums that grow most
   often: tokens, opcodes, syntax nodes, error kinds.
5. **Library enums.** If every multi-case branch must list the cases, adding a
   case to a published package's enum breaks every user's build. Rust and
   Swift added `#[non_exhaustive]` and `@unknown default` for this.

An error plus a `jet fix` that writes the missing cases fits finding 3's short
rests, and costs the writer nothing. The measured annoyance is the long rest
whose arm does nothing or fails. Any design needs an explicit, short spelling
for "every other case, now or later", or it keeps producing those walls.
