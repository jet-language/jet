# Write marks at call sites: is `&` at every write worth it? (2026-10-29)

This dated research answers the owner's question of 2026-10-29. He asked
whether every place that a call modifies must carry `&`, given that the mark is
"kinda annoying where it is done just to do basic things like push/pop." It is
evidence for the reworked ballot **D-RECEIVER-INFER1**. It does not recommend an
option, ratify anything, or own any work.

Every web source below was read on **2026-10-29**. The corpus measurements were
run the same day over the dev tree at `bb9c74e522`, and the log scan over the
agents' scratch logs. No Jet program was compiled or executed for this report.
Measured facts and quoted opinion are kept in separate sections, and each
opinion is labelled with its source.

## Plain summary

1. Google's C++ style guide banned non-`const` reference parameters from 2008.
   Outputs had to be pointers, so a call that changed `x` showed `&x`. On
   2020-05-19 Google dropped that ban (section 1).
2. Google published no reason for the reversal. The commit message says only
   "drop ban on mutable references". The new text makes required outputs
   non-null references and keeps pointers for optional outputs.
3. Most of what made Google's `&x` costly came from C++ pointers: nullability,
   `*`/`->` syntax, and a mark the compiler did not check. Jet's `&` has none of
   these costs. It is checked both ways: missing is E0202/E0224, extra is E0225.
4. Google's rule never marked method receivers. `foo.Mutate()` stayed unmarked
   for all twelve years. Jet's receiver mark therefore has no Google precedent,
   for or against.
5. Measured in Jet's own corpus: 82,775 `&` marks on places in 997,669 code
   lines, about one per 12 lines. 25,929 (31%) sit on method receivers and
   56,459 (68%) on arguments.
6. `push` alone carries 18,955 marks: 23% of all marks and 73% of receiver
   marks. Container mutators such as `push`, `pop` and `sort` on same-function
   locals account for 16,426 marks (20% of all).
7. Arguments are concentrated in the compiler: 49,463 of 56,459 (88%). Outside
   `Compiler/`, receiver marks outnumber argument marks about two to one.
8. The receiver mark is one month old. It was ratified on 2026-09-27 and added
   to the corpus in four bulk commits of 13,638 lines on 2026-09-28 to 09-30.
9. No Jet commit or Tower card records a bug that a write mark caught. Nine UI
   snapshots do record a missing receiver mark beside the error each test was
   written for.
10. In the agents' build and test logs, the three call-site mark errors are
    2.6% of 1.5 million diagnostics. E0224 ranks 12th of 676 codes
    (section 3.4).
11. No surveyed language other than Hylo marks mutating receivers. Rust, Swift,
    C#, and Go mark arguments but not receivers. Mojo, D, and Ada mark neither.
12. Swift built a call-site mark for mutating methods (`x.=f()`), then removed
    it. Hylo's designer, who co-wrote that Swift design, now marks every
    mutation with no exceptions.
13. No peer-reviewed study measuring call-site write marks was found. The
    published evidence is design rationale and developer opinion.
14. rust-analyzer underlines mutable locals and `&mut self` methods by default.
    Jet's language server colours the `&` token but shows nothing on an
    unmarked write.

## 1. Headline: Google's output-parameter rule and its reversal

### 1.1 The rule from 2008 to 2020

The first public guide (`cppguide.xml`, commit `6fb1d37`, 2008-06-27) already
held the rule. The text just before the change, under "Reference Arguments",
read:

> All parameters passed by lvalue reference must be labeled `const`.

Its listed pros:

> Defining a parameter as reference avoids ugly code like `(*pval)++`.
> Necessary for some applications like copy constructors. Makes it clear,
> unlike with pointers, that a null pointer is not a possible value.

Its only listed con:

> References can be confusing, as they have value syntax but pointer semantics.

Its decision:

> In fact it is a very strong convention in Google code that input arguments
> are values or `const` references while output arguments are pointers. Input
> parameters may be `const` pointers, but we never allow non-`const` reference
> parameters except when required by convention, e.g., `swap()`.

The separate "Output Parameters" section said that "output and input/output
parameters will be pointers to non-`const`." The 2008 text had the same rule
without the `swap()` exception.

### 1.2 The change

The change is commit `7a7a2f510efe7d7fc5ea8fbed549ddb31fac8f3e` in
`google/styleguide`. Daniel Cheng (`dcheng@chromium.org`) committed it on
**2020-05-19 22:52 UTC** as "Update C++ styleguide". Its message lists eight
"Major changes", including:

> discourage const references when the input parameter must outlive the call
> … drop ban on mutable references

No rationale for dropping the ban appears in the commit. The section anchor
moved from `#Reference_Arguments` to `#Inputs_and_Outputs`.

### 1.3 The rule since 2020

The new "Inputs and Outputs" section reads:

> Input parameters should usually be values or `const` references, while
> required (non-nullable) output and input/output parameters should usually be
> references. Generally, use `absl::optional` to represent optional inputs,
> and non-`const` pointers to represent optional outputs.

It also says: "Prefer using return values over output parameters: they improve
readability, and often provide the same or better performance."

### 1.4 The reasons Google gave

Google gave no explicit reason in public. Three documents bound what the
reasons could have been.

- The guide's own pros and cons. The only con it ever listed for references
  was confusion between "value syntax" and "pointer semantics". The pros it
  listed included non-nullability, which the new text now relies on.
- Abseil Tip of the Week #176, "Prefer Return Values to Output Parameters"
  (Etienne Dechamps, 2020-03-12, two months earlier). It lists questions a
  pointer output raises: "Can `foo_spec` be null?", lifetime, and pre-existing
  contents. It says one "cannot rely on the C++ compiler to enforce these
  contracts."
- The 2020 text itself. It assigns the null question to the type: references
  for required outputs, pointers for optional ones.

The call-site reason for the old rule was not in the guide's text. It was the
explanation Googlers gave in public.

### 1.5 Public discussion

- **coreytabaka**, a Googler, Hacker News item 18153329, 2018-10-06: "This
  argument comes up a lot within Google… readability at the call site (e.g.
  Foo(&bar) might mutate bar, whereas Foo(bar) should not) is still considered
  more valuable in a large scale codebase."
- The same comment: "Passing nullptr as a pointer argument is generally assumed
  to not be okay unless explicitly documented."
- **lemmyg**, item 18153242, 2018-10-06: references "suit indicating the
  expectation of non-nullity"; much code with pointer arguments "would explode
  if you passed one in".
- **cletus**, item 18556088, 2018-11-28: "The point of this is so you can't tell
  what function arguments might be mutated if you allow non-const reference …
  `foo(c); // could be pass by value or reference` `foo(&c); // clearly mutable`".
- **hashingroll**, item 28908152, 2021-10-18: "The style guide (recently?)
  removed the ban on non-const references."

No design note, mailing-list thread, or talk about the 2020 change was found.
Chromium's guide follows Google's unless it lists an exception, and it lists
none here. A fetch of Chromium's guide history timed out, so its own discussion
was not read.

### 1.6 What the old rule covered, and what it did not

The rule governed parameters only. A member function that changes its object,
`foo.Mutate()`, never needed a call-site sign under either version. So Google
ran for twelve years on what this report calls option B: arguments marked by
convention, receivers unmarked. In 2020 it moved arguments to unmarked as well.

### 1.7 Which of Google's reasons transfer to Jet

The C++ `&x` signal was weak in four ways. The table compares each with Jet.

| C++ pointer convention | Jet `&` today |
|---|---|
| A pointer variable `p` passes as `Foo(p)` with no `&`, yet `Foo` may write through it. | A place passed to a writing parameter needs `&` wherever it came from (E0202). Calls on `&T` parameters carry 5,466 receiver marks. |
| `&x` also passed `const T*` inputs, which the rule allowed, so `&` did not mean "writes". | `&x` to a read parameter is E0225 (spec.md:1096-1098). |
| A pointer can be null, so each call raised a contract question. | A `&T` argument must already hold a value (`Docs/research/output-parameters-vs-returns-2026-10-04.md`, item 7). Optional values use `?T`. |
| The convention was not checked by the compiler. | Missing marks are E0202 or E0224; extra marks are E0225. |

So the signal-quality reasons for Google's reversal do not transfer. Jet's mark
is complete and sound for named places at calls. The cost that remains in Jet
is the mark itself: one character per write, and a compile error when it is
forgotten.

Google's reversal therefore says little about Jet's argument marks. Its
stated costs came from the pointer type, which Jet does not use. It says
nothing direct about receiver marks, because Google never had them. What does
transfer is a practice: a very large codebase ran with unmarked receivers for
twelve years. No public report links that to a bug class.

## 2. Jet corpus census (measured)

### 2.1 Method

A lexical scanner read every `.jet` file under `Examples/`, `Core/`,
`Compiler/`, `Jetpack/`, and `tests/` of dev `bb9c74e522`. It skipped symlinks,
comments, and strings. It classed each `&` by its neighbours:

- **receiver**: `&place.method(…)`, where the last segment is a call;
- **argument**: `&place` alone in a call's argument list;
- **window**: `x :: &place` or `x := &place`;
- **match write**: `if &place …`;
- **declaration**: `&self` or `&T` in a signature, excluded from the totals;
- **logical**: `&&`, excluded.

A lexical scan cannot resolve names. Receiver roots are classed as `self`, a
parameter, or a local by the enclosing function's parameter list.

### 2.2 Totals

| Measure | Count |
|---|---:|
| Files | 16,837 |
| Code lines | 997,669 |
| `&` marks on places | **82,775** |
| Receiver marks | 25,929 (31.3%) |
| Argument marks | 56,459 (68.2%) |
| Write-window bindings | 161 |
| `if &x` match writes | 41 |
| Other marks on places | 185 |
| `&self`/`&T` in declarations (not counted) | 11,158 |

Density is 83 marks per 1,000 code lines: 26 receiver and 57 argument marks.
That is one mark per 12 code lines.

Receiver roots split into 18,732 locals, 5,466 parameters, and 1,731 `self`.
Receivers whose method is a known container mutator number 21,726. Of those,
**16,426** root at a same-function local: 19.8% of all marks.

### 2.3 By directory

| Directory | Code lines | Receivers | Arguments | Marks per kloc |
|---|---:|---:|---:|---:|
| `Examples/` | 41,200 | 912 | 405 | 32 |
| `Core/` | 66,049 | 3,101 | 1,406 | 68 |
| `Compiler/` | 514,480 | 12,652 | 49,463 | 121 |
| `Jetpack/` | 173,620 | 7,250 | 3,824 | 64 |
| `tests/` | 202,320 | 2,014 | 1,361 | 17 |

The compiler holds 88% of all argument marks. Its passes thread a context and
an output sink through most calls. Outside `Compiler/`, receivers total 13,277
and arguments 6,996. User-facing code in `Examples/` has one mark per 31 lines.

### 2.4 Top receiver methods

| Method | Marks | | Method | Marks |
|---|---:|---|---|---:|
| `push` | 18,955 | | `write` | 84 |
| `pop` | 646 | | `clear` | 82 |
| `sort` | 560 | | `next` | 79 |
| `sort_by` | 392 | | `bump` | 75 |
| `add` | 336 | | `reverse` | 67 |
| `field` | 296 | | `string` | 66 |
| `extend` | 204 | | `add_typed_text` | 66 |
| `insert` | 130 | | `set` | 58 |
| `remove` | 120 | | `flush` | 53 |
| `text` | 96 | | `update` | 47 |

`push` is 22.9% of every mark in the corpus. The next nineteen methods together
carry 3,457.

## 3. How the rule arrived, and what it has cost (measured)

### 3.1 Timeline

- 2026-06-23, `be8f02cfcd`: D-MUTSELF1 let `mut self` methods assign fields and
  the whole receiver.
- Until late September, methods could be called without a receiver mark. Tower
  card #3452 states this in its body.
- 2026-09-27: the owner ratified D-CAP-RECEIVER1=D, "Mark the value, not the
  call". Its recorded reason: "D marks the named value that changes or ends, as
  arguments already do."
- 2026-09-28 to 09-30: the receiver mark shipped with E0224. Commit
  `c0c5493a97` introduced E0224 and added 7,502 marked lines.

### 3.2 Mark-only edits in history

A script compared every changed line pair in 7,295 non-merge commits that touch
`.jet` files. A pair counts as add-only when the new line equals the old line
plus one or more `&`.

| Measure | Count |
|---|---:|
| Add-only lines | 14,689 (15,100 marks) |
| Commits containing them | 92 |
| Lines in the four bulk commits | 13,638 |
| Lines in 71 small commits (<20 lines) | 337 |
| Lines in 17 mid-sized commits | 714 |
| Remove-only lines | 1,727 (1,819 marks) |

The four bulk commits are `c0c5493a97` (+7,502), `a432c41f79` (+3,140),
`963c3cb09e` (+2,355), and `853ebc2d1e` (+641). Outside those four, 1,051
add-only lines landed in 88 ordinary commits. The 1,819 removed marks went the
other way; their causes were not classified.

One commit shows the per-package cost. `5802113831` (2026-10-24) applied
mechanical fixes to `Jetpack/Store`. Its trust check went from 39 to 4 E0224,
from 12 to 1 E0202, and from 3 to 0 E0225. It left three E0224 sites
unresolved where "`&` already before the receiver; Fix line not applicable."

### 3.3 UI snapshots

Among 2,248 expected-diagnostic snapshots under `tests/`, E0224 appears 14
times in 13 files. In nine of those files it is a second error beside the one
the test exists for. For example,
`tests/ui/ownership/named_range_view_blocks_owner_resize_and_replacement.stderr`
expects E0212 on `xs.push(4)` and also records E0224 for the same call. In
these files the call is written without the mark, and the snapshot records
E0224 beside the tested error.

### 3.4 Compile logs

A scan on 2026-10-29 read every log-like file (`.log`, `.txt`, `.stderr`,
`.out`, `.json`, `.jsonl`, `.tsv`) in the agents' build caches, discovery runs
and lane scratch directories. It skipped exported source trees, so snapshots
and examples are not counted. Identical files counted once.

It read 43,674 distinct files, 13.7 GB, holding 1,516,324 diagnostics under
676 codes.

| Code | Meaning (registry) | Occurrences | Files | Rank |
|---|---|---:|---:|---:|
| E0224 | missing receiver mark | 22,996 | 1,635 | 12 |
| E0202 | missing argument mark | 9,696 | 1,345 | 33 |
| E0225 | mark where nothing writes | 6,269 | 1,083 | 39 |
| E0205 | `self.field = v` without `&self` in the declaration | 12,195 | 1,342 | 22 |
| E0204 | same value used while a write is active | 27,400 | 1,298 | 9 |

The three call-site mark codes (E0224, E0202, E0225) total 38,961: 2.6% of all
diagnostics. The most frequent code, E0102 (unknown function), has 209,823.

These are occurrences, not distinct mistakes. A suite log repeats the same
expected errors on every run, and a failing build repeats its errors on every
retry. The counts show how often agents met each error, weighed against every
other error they met.

## 4. Evidence that a mark caught a bug

### 4.1 Jet

The git log was searched for unexpected, silent, accidental, or hidden
mutation, and for E0224 catches. One hit, `3f0245a39a`, concerns a
purity-walker merge, not call-site marks. Tower cards mentioning E0224, E0225,
or receiver marks are the implementation card #3452 and golden root #5702.
Neither records a caught bug.

No instance was found. The absence is weak evidence: the receiver mark is one
month old, and most code in the corpus was written or migrated by agents.

### 4.2 Elsewhere

- Rust removed automatic borrowing of arguments in 2013. RFC 241 quotes the
  rust-dev thread: "`sort(a); // not only does this not move a, but it mutates
  it!`". This is a design example, not a recorded bug.
- C++ Core Guidelines F.17 shows `f(buffer)` replacing a buffer the caller
  meant to be filled: "A bad logic error can happen if the writer of `g()`
  incorrectly assumes the size of the `buffer`." It too is an example.
- No defect study attributing bugs to unmarked mutating calls was found.

## 5. Other languages (measured from their specifications)

| Language | Argument write mark | Receiver write mark | Source |
|---|---|---|---|
| Rust | `&mut x` required | none: `v.push(x)` | Reference, method-call expressions |
| Swift | `&x` for `inout` | none: `somePoint.moveBy(x:y:)` | TSPL, Functions and Methods |
| Hylo (Val) | `&x` for `inout` | `&p.process()` | Abrahams, forums.swift.org/t/59823 |
| C# | `ref x`/`out x` required; `in` optional | none, including `ref this` extensions | C# 7.2 and 12 proposals; MS Learn |
| Go | `&x` when the parameter is a pointer | none: `x.m()` is `(&x).m()` | Go spec, Calls |
| Zig | `&x` when the parameter is a pointer | none: `list.append(gpa, …)` | Language reference |
| Odin | `&x`; no methods: `append(&x, 123)` | not applicable | Odin overview |
| Mojo | none: `mutate(values)` for `mut` | none | Mojo manual, Ownership |
| D | none: `inc(z)` for `ref` | none | D spec, Functions |
| Ada | none for `in out` | none | Ada RM 6.4.1 |
| Jet today | `&x` required | `&xs.push(x)` required | spec.md:1068-1099 |

Exact texts behind the table:

- **Rust Reference**: "the receiver may be automatically dereferenced or
  borrowed in order to call a method". For each candidate type `T`, "add `&T`
  and `&mut T` to the list immediately after `T`."
- **Swift TSPL**: "You place an ampersand (`&`) directly before a variable's
  name when you pass it as an argument to an in-out parameter, to indicate that
  it can be modified by the function." Mutating methods are called with plain
  dot syntax.
- **Hylo**: "Arguments to inout parameters must be unique, mutable, and marked
  with an ampersand (&) at the call site." Also: "in Val there are no
  exceptions to the rule that mutated objects are marked with &, so you'll see
  it in uses of operators and mutating methods."
- **C#, MS Learn**: "both the method definition and the calling method must
  explicitly use the `ref` keyword". Of `in`: "required in the method
  declaration but unnecessary at the call site."
- **C# 7.2**: "in some cases, such as invocation of operators, or `in`
  extension methods, there is no syntactical way to specify `in` at all."
- **Go spec**: "If `x` is addressable and `&x`'s method set contains `m`,
  `x.m()` is shorthand for `(&x).m()`."
- **Zig**: the reference's leak example calls `list.append(gpa, '☔')` on a
  `var list: std.ArrayList(u21)`. **[INFERENCE: `append` takes `self: *Self`;
  the reference text read does not state the auto-address rule in prose.]**
- **Mojo**: `mut` gives "a mutable reference", and the manual's example calls
  `mutate(values)`. It says "Mojo doesn't require a sigil on the caller side to
  pass by immutable reference" and calls this "an easier-to-use programming
  model."
- **D**: "A `ref` parameter takes an lvalue argument, so changes to its value
  will operate on the caller's argument", shown as `inc(z)`.
- **Ada RM 6.4.1(5)**: for `in out` or `out`, "the actual shall be a name that
  denotes a variable". Ada 2012 added an anti-aliasing rule for two `in out`
  actuals, 6.4.1(6.17).
- **Ada RM 6.2(11)**: for most parameters "it is unspecified whether the
  parameter is passed by copy or by reference." The compiler chooses, as the
  owner wants for Jet.
- **Jai** has no public specification. Its `array_add(*arr, x)` form was not
  verified from a primary source.

Two patterns stand out. Every surveyed language with a call-site mark puts it
on arguments. Only Hylo extends it to receivers.

## 6. Designers on the record (opinion, by source)

### 6.1 Rust

- rust-dev, November 2013, quoted in RFC 241: "Code readability does not
  necessarily benefit from autoref on arguments". Also: "Having to include an
  extra & or &mut for arguments is a slight inconvenience, but it makes it much
  easier to track ownership at a glance."
- RFC 241 principle: "Coercions should never implicitly borrow from owned
  data." Its appendix exempts receivers: "In practice, the name and context of
  a method invocation is almost always sufficient to infer its move/borrow
  semantics."
- withoutboats, internals thread 5161, 2017-04-30, on
  automatic `&mut` for arguments: "I'm concerned with how that would
  essentially create a C++ call-by-reference situation". No such coercion
  exists in Rust today.

### 6.2 Swift

- Dave Abrahams and Joe Groff wrote "In-Place Operations"
  (`swift/docs/proposals/Inplace.rst`). It noted that on mutating methods
  "`self` is *implicitly* `inout`", so `x.union(y)` hides which kind it is. It
  proposed `x.=union(y)`.
- Abrahams, forums.swift.org/t/2290, 2016-04-22: "it was implmented [sic] and later
  reverted because it raised language-design questions for which we had no good
  answers." The first question he listed: "Should the x.=f() syntax be
  required for *every* mutating method invocation?"
- Joe Groff, forums.swift.org/t/652, 2015-12-19, proposed spelling `inout` as
  `&` in declarations, "mirroring its usage in call sites". Chris Lattner
  replied "-1": "inout" is "intended to communicate (or at least hint at) the
  copy-in / copy-out behavior". Neither post questions the call-site `&`.

### 6.3 Hylo

Abrahams, forums.swift.org/t/59823, 2022-08-22, describes the opposite end. All
mutation is marked, receivers and operators included: `&target.x += delta.x`,
`&p.process()`. In Hylo the mark also has a job. It selects the `inout` variant
of a method bundle: "the first call applies the inout variant as the receiver
has been marked for mutation." Jet's mark selects nothing; a wrong mark is an
error.

### 6.4 C#

- The C# 12 `ref readonly` proposal records LDM 2022-05-11: "the primary
  motivation for ref readonly parameters are APIs which capture or return
  references from these parameters, so marker of some kind is a good thing."
- The same proposal suppresses its missing-marker warning "for the receiver in
  an extension method invocation".
- The C# 7.2 proposal explains why `in` needs no call-site mark: some calls,
  such as operators, have no place to write one.

### 6.5 C++ Core Guidelines

F.17, "For 'in-out' parameters, pass by reference to non-`const`", gives the
reason "This makes it clear to callers that the object is assumed to be
modified." The clarity it means is in the signature, not at the call. Its
enforcement asks tools to "Warn about functions regarding reference to
non-`const` parameters that do *not* write to them." Jet's E0225 is the
call-site form of that check.

### 6.6 Odin, Zig, and Jai

No statement by Ginger Bill, Andrew Kelley, or Jonathan Blow on call-site
mutation marks was found. In Odin and Zig the `&` is address-of: it follows
from the parameter being a pointer type, not from a mutation rule.

## 7. Developer feedback (opinion, weighed as anecdote)

Supportive:

- tomjakubowski, Hacker News 9669271, 2015-06-06, on Rust: "I had been concerned
  that it might be 'too much typing', but it's proven to be rare and
  unobtrusive." That experience is of argument marks only; Rust receivers are
  unmarked.
- djc, Rust internals 5161, 2017-05-02: "Making mutability explicit in argument
  passing feels very useful even independent of the borrow checker".
- deathanatos, Hacker News 9669184, 2015-06-06, reports the C++ argument that
  unmarked reference passing is "poor language design because at the call site,
  you cannot see that the variable is potentially being mutated".

Resistant or mixed, from the Swift thread on marking mutating methods
(forums.swift.org/t/2290, April 2016):

- VladimirS: a marker would be "really awesome", but "this will add a much more
  noise (and typing) in code as we often (in most cases?) use mutating methods."
- Tyler Cloutier, recalling an earlier `foo&.add(5)` thread: "the consensus was
  that that was not a particularly familiar syntax and it would add a decent
  amount of noise."
- Haravikk suggested the editor instead: "Xcode highlight inout parameters and
  mutating methods differently".

The pattern is consistent across these sources. People praise argument marks
where they are rare. They resist receiver marks because mutating method calls
are common. No survey asking about call-site mutation marks was found. The
Rust annual survey and the Stack Overflow survey were not checked year by
year.

## 8. Research

No peer-reviewed study of call-site write marks was found. Searches covered
Crossref, Semantic Scholar, and the ACM and IEEE listings they index. The
nearest work is Coblenz et al., "Exploring Language Support for Immutability",
ICSE 2016 (doi:10.1145/2884781.2884798), and "Glacier: Transitive Class
Immutability for Java", ICSE 2017 (doi:10.1109/ICSE.2017.52). Both study
declared immutability, not call-site marks. Their full text could not be
retrieved, so this report relies on neither.

The evidence in this report is therefore of three kinds. Corpus counts are
measured. Language rules are quoted from specifications. Everything about
readability or bug prevention is designer or developer opinion.

## 9. Tooling that shows mutation without a mark

- rust-analyzer defines a `mutable` semantic-token modifier, described as
  "Style for mutable locals and statics as well as functions taking `&mut
  self`". Its VS Code extension maps `*.mutable` to `markup.underline` by
  default (`editors/code/package.json`).
- Jet's language server already declares a `writeBorrow` token modifier
  (`Compiler/JetLSP/Source/LSP/Requests.jet:142`). Its integration test checks
  that modifier on the `&` token (`Lsp03Tests.jet:479`). It marks the written
  sigil; it cannot mark a write that has no sigil.
- Jet's server already sends inlay hints (`Reasoning.jet:47`). A write hint on
  an unmarked receiver would use the same channel.
  **[INFERENCE: not built or measured.]**
- Highlighting does not reach plain-text views: diffs, code review pages,
  terminals, and the text an agent reads.
  **[INFERENCE from how those views render; not measured.]**

## 10. Design space

All counts come from section 2. "Reader loss" means what a reader of plain text
can no longer see at the call. The compiler's knowledge is unchanged in every
option: the parameter or receiver convention still decides what a call writes.

No option changes generated code or speed. The diagnostic registry classes both
fixes, adding the mark (E0224) and removing it (E0225), as `BehaviorPreserving`
(`DiagnosticRows.jet:2127`, `:2143`). The owner's performance model, where the
compiler chooses copy or reference, does not depend on the mark.

### Option A: today's rule

- **Marks removed**: none. 82,775 marks remain.
- **Reader loss**: none. Every call that writes a named place shows `&` on it.
- **Exclusivity (E0204)**: the mark names the place under write, so a conflict
  such as `grow(&xs, f(xs))` points at a visible `&xs`.
- **`@` links (D-MEMREF1)**: writes through a link are already unmarked;
  `p.hp = 5` shows through `t.captain` (memory-model-2026-10-05.md:144).
- **`:=` and `::`**: a write still needs a `:=` binding (E0202), so the
  declaration and every call both carry the fact.
- **Closures**: a lambda already writes captured `:=` locals with no mark
  (spec.md:797-802). Inside a lambda, `item.touch()` is E0224.
- **Concurrency**: group children write-borrow captured places with no mark;
  E1101 enforces non-overlap (spec.md:2614-2618).
- **Expert pointers**: raw access stays behind `#Unsafe` and `mem.Ptr<T>`
  (spec.md:1941-1943). FFI out-slots keep `&`, as in `c.write(&signed)`
  (`Examples/features/lowlevel/native_c/run.jet:18`).
- **Cost**: one mark per 12 code lines. 15,100 marks were added in history, and
  9 snapshot files carry a forgotten receiver mark.

### Option B: receivers lend automatically; arguments keep `&`

This is the Rust, Swift, C#, and Go model.

- **Marks removed**: 25,929 (31.3%). `push` alone accounts for 18,955. 56,846
  marks remain.
- **Reader loss**: `xs.push(1)` no longer shows that `xs` changes. The method
  name and the `:=` binding carry that fact, as RFC 241's appendix argues.
  Names like `field`, `text`, and `string` (458 marks) say less.
- **Exclusivity**: unchanged in the checker. A conflict on a receiver points at
  an unmarked name; E0212 in section 3.3 already reports one this way.
- **`@` links**: unchanged.
- **`:=` and `::`**: unchanged. Calling a writing method on a `::` binding is
  still E0202, so mutability stays visible at the declaration.
- **Closures**: matches the existing unmarked capture write.
- **Concurrency**: unchanged rules; a child body's `xs.push` is unmarked.
- **Expert pointers**: unchanged.
- **Rule shape**: arguments and receivers follow different rules again, which
  D-CAP-RECEIVER1=D set out to unify.

### Option C: no `&` at calls; tooling shows writes

This is the Mojo, D, and Ada model, and Google C++ since 2020.

- **Marks removed**: 82,775. The 161 write windows (`x :: &place`) and 41
  `if &x` match writes need their own spelling decision, because there the
  mark creates a write borrow rather than labelling one.
- **Reader loss**: no plain-text sign that `edit(buf)` changes `buf`. This is
  the case cletus, coreytabaka, and RFC 241 each called the important one.
- **Exclusivity**: unchanged in the checker. E0204 would point at two unmarked
  uses of one name.
- **`@` links**: writes through links and writes through calls would look
  alike. Both would rely on tooling.
- **`:=` and `::`**: the binding becomes the only plain-text sign of
  mutability.
- **Closures**: consistent with capture writes.
- **Concurrency**: a spawned call's write to a passed place would be visible
  only in the signature or editor.
- **Expert pointers**: unchanged. FFI calls that fill C out-slots would lose
  their mark unless kept as a special case.
- **Diagnostics**: E0224 and E0225 retire. E0202 would remain for writes to
  `::` bindings.

### Option D: variants the evidence suggests

| Variant | Marks removed | Share |
|---|---:|---:|
| D1: receivers unmarked when the root is a same-function local | 18,732 | 22.6% |
| D2: D1 restricted to known container mutators | 16,426 | 19.8% |
| D3: receiver marks optional; the editor shows a faint `&` | up to 25,929 | up to 31.3% |
| D4: `&` only when a write crosses a module boundary | not measured | — |

- **D1** keeps a mark wherever the written value came from outside the
  function: parameters (5,466) and `self` (1,731). Writes to a caller's data
  stay visible. Writes to a function's own locals do not.
- **D2** is the owner's push/pop case. It needs a list of methods that the
  rule must name, which is a second rule.
- **D3** was option A of the earlier D-RECEIVER-INFER1 draft. It makes two
  spellings legal for one call, which philosophy.md's "one mechanism" priority
  addresses (philosophy.md:38-40).
- **D4** needs name resolution to count. The lexical census cannot measure it.

The `^` take mark is outside this report. Every option above leaves it as it is.

### How the options meet Jet's stated principles

philosophy.md states three principles that pull in different directions here.

- Principle 2 says "most of the time spent with code is reading". It favours
  marks that a reader cannot infer (philosophy.md:18).
- Principle 3 says "the most common operations … should have the least amount
  of friction". `push` is the most common marked call (philosophy.md:19).
- Principle 8 says a choice that "changes behavior, cost, safety, or authority
  is visible in the code where it is made" (philosophy.md:24).

The measured facts bear on how often each principle applies. They do not
decide between them.

## Sources

- Google C++ Style Guide, `github.com/google/styleguide`: `cppguide.html` at
  `7a7a2f510e` and its parent; `cppguide.xml` at `6fb1d37`.
- Abseil Tip of the Week #176, `abseil.io/tips/176`.
- Hacker News items 18153329, 18153242, 18556088, 28908152, 9669271, 9669184,
  via `hn.algolia.com`.
- C++ Core Guidelines F.17, `isocpp/CppCoreGuidelines`.
- Rust Reference, "Method-call expressions"; RFC 241 "deref-conversions";
  internals.rust-lang.org topic 5161.
- The Swift Programming Language, `swiftlang/swift-book`: Functions.md and
  Methods.md.
- `swiftlang/swift/docs/proposals/Inplace.rst`; forums.swift.org topics 652,
  2290, and 59823.
- C# proposals `csharp-7.2/readonly-ref.md` and `csharp-12.0/ref-readonly-parameters.md`;
  MS Learn "Method parameters".
- The Go Programming Language Specification, "Calls".
- Zig Language Reference, `ziglang.org/documentation/master`.
- Odin overview, `odin-lang.org/docs/overview`.
- Mojo manual, "Ownership", `docs.modular.com/mojo/manual/values/ownership`.
- D Language Specification, "Functions", `dlang.org/spec/function.html`.
- Ada 2022 Reference Manual, 6.2 and 6.4.1, `ada-auth.org`.
- rust-analyzer `editors/code/package.json`.
- Coblenz et al., ICSE 2016 and ICSE 2017 (metadata only).
- Jet: `Docs/spec/spec.md`, `Docs/spec/philosophy.md`,
  `Docs/proposals/memory-model-2026-10-05.md`,
  `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`, Tower cards
  #3452 and #5702, decision D-CAP-RECEIVER1, and the git history of dev
  `bb9c74e522`.
