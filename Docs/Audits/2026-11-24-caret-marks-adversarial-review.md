# Adversarial review: `^` and `^^` as the write and take marks

Date: 2026-11-24. Card: #6998, criterion 1.

Subject: the respelling of Jet's write mark from `&` to `^` and its take mark
from `&&` to `^^`. The registry rows are `SIGIL_WRITE: "^"` and
`SIGIL_MOVE: "^^"` in `Compiler/JetFoundation/Data/Syntax/CoreSurface.jet`,
and the migration plan is `mark-caret1` in
`Compiler/JetFoundation/Data/Syntax/Spellings.jet`. The decision text is
D-CALLSITE-MARK2 and D-MOVE-SIGIL1 in `Docs/Spec/syntax-decisions.md`. This
review attacks each claimed strength. Probes ran on the combined-batch stage
compiler of 2026-11-24 (stage D of the `dev` batch built on `b872a051a81`),
from single files under `$JET_SCRATCH_ROOT/orch/caretprobe/`.

## The strongest alternatives, at their best

- **Today's `&` and `&&` (the spelling replaced).** One character for write,
  two for take, and the reading most programmers already carry from C, C++
  and Rust that `&` means "a reference". Its cost is that the same two
  glyphs are also bitwise and and logical and, so a reader and the parser
  tell them apart only by position.
- **C++ `T&`** (cppreference, "Reference declaration",
  https://en.cppreference.com/w/cpp/language/reference). A postfix reference
  mark on the type. Its best case is decades of familiarity. It does not
  separate "may change" from "reads" (`const T&` is the read form), and a
  call site never shows the mark.
- **Rust `&mut T` and moves by value** (The Rust Reference, "Pointer types",
  https://doc.rust-lang.org/reference/types/pointer.html). The keyword `mut`
  spells the write in words, so it cannot be mistyped into another mark, and
  a take needs no mark at all, since a by-value parameter moves. It writes the
  mark at every call (`f(&mut x)`), which Jet's D-CALLSITE-MARK2 removes.
- **Swift `inout`** (The Swift Programming Language, "In-Out Parameters",
  https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/#In-Out-Parameters).
  A keyword in the type position (`x: inout Int`) with `&x` at the call.
  Words cannot be mistyped on any keyboard layout and read aloud plainly;
  they cost four extra characters per write parameter. Swift has `consuming`
  for a take.
- **Odin `^T`** (Odin overview, "Pointers",
  https://odin-lang.org/docs/overview/#pointers). Odin, and Pascal before it,
  spell a pointer type `^T`. That is the strongest evidence that `^` reads
  well as a type prefix, and it is also the strongest source of a wrong
  reading (see Claim 2).

## Claim 1: `^` and `^^` never meet an operator

The claim: `^` has no operator meaning (exclusive-or is `~`, power is
`x.pow(n)`; `OP_CARET` and `OP_TILDE` in
`Compiler/JetFoundation/Data/Syntax/MathLayout.jet`), and the take mark only
appears in a parameter type, before `self`, and after `task`, so `^^` never
meets logical `&&`.

Probe (`ok.jet`): one program declares `bump: fn(x: ^Int)` and
`consume_it: fn(s: ^^String) -> Int`, and uses `n > 0 && n < 5` and `n & 3`
in one body. It runs and prints `2 true 2 3`.

Attack: the operators a reader brings from other languages. `a ^ b` is
exclusive-or in C, C++, Rust, Java, JavaScript, Go and Python, and power in
Lua, R, Julia and spreadsheets. In Jet it is an ordinary parse error, and the
error names neither meaning: `print(a ^ b)` gives `Jet.InvalidStatement:
Expected , between call arguments` (pinned by `Tests/UI/caret_is_not_power`),
and `c: a ^ b` gives `Something unexpected appeared after this line, found ^`.
The foreign-operator teaching code E0396 (`RetiredForeignPowerOperator`) is
retired, and D-MOVE-SIGIL1 keeps `^` an ordinary error until a ballot gives it
a job.

Verdict: **holds** for the grammar: no Jet program can read a mark as an
operator or an operator as a mark. The cost is named: a reader who brings
xor or power to `^` gets a parse message about commas or line ends. The best
mitigation within the ratified decision is the existing math-layout note,
which `jet explain` prints; a teaching row for binary `^` needs a ballot.

## Claim 2: `^` reads as "may change" at a glance

Attack: readers of Odin, Pascal and Delphi read `^T` as "pointer to T". In
Jet `^T` is not a pointer: it is call-long write access to the caller's
value, with no address, arithmetic or null. A reader carrying the pointer
meaning expects `p^` dereferences and nullable values, and finds neither.
The `&` spelling had the same problem in the other direction (a C++ or Rust
reader expects a reference type).

Verdict: **holds only within stated limits.** The mark is short, sits only in
signatures, write windows, written match subjects and task captures
(D-CALLSITE-MARK2), and every call is plain, so the glyph is read far less
often than Rust's call-site marks. The pointer reading is the named cost. Its
mitigation is that the editor hover prints each parameter's promise in words
(D-PARAM-CONST1) and that raw pointers are a separate `#Unsafe`-gated
mechanism (`p.*`), so the wrong reading never compiles into a pointer
operation.

## Claim 3: the marks are easy to type

Attack: on German, French, Swiss, Nordic, Portuguese and other European
layouts, `^` is a dead key. The X11 compose table shipped with libX11
(`share/X11/locale/en_US.UTF-8/Compose`, read on the probe machine) has:

```text
<dead_circumflex> <dead_circumflex> : "^"   asciicircum
<dead_circumflex> <I>               : "Î"   Icircumflex
<dead_circumflex> <e>               : "ê"   ecircumflex
<dead_circumflex> <s>               : "ŝ"   U015D
```

So on Linux with a dead-key layout:

- typing `^^` gives one `^`, so `^^self` comes out as a write mark, `^self`;
- typing `^self` gives `ŝelf`, and `^Int` gives `Înt`;
- typing `^entries` (a write window name) gives `êntries`.

Probes: `deadkey_type.jet` (`bump: fn(x: Înt)`) reports
`Jet.UnknownType: There's no type called Înt` with the generic fix "Check the
spelling, or define the struct or enum first"; nothing names the dead key or
`^Int`. `deadkey_name.jet` (`grow: fn(êntries: [Int])`) compiles and runs,
because identifiers accept non-ASCII letters: the intended write parameter is
silently a read parameter named `êntries`. `&` and `&&` had no such failure:
`&` is not a dead key on these layouts.

Verdict: **fails as claimed.** The respelling is easy to type on US and UK
layouts and on any layout without dead keys, and costly on dead-key layouts.
Gaps, now on #6998 as a criterion: a name or type that starts with a
circumflex-composed letter (`Î`, `ŝ`, `ê` and the rest), where the mark plus
the base letter names something in scope, gets a fix naming the dead key and
the intended `^` spelling, on every diagnostic that can see it (unknown type,
unknown name, and a write through a read parameter whose name starts with a
composed letter). The `^^` to `^` collapse can only be caught where a
take-only use follows; the take parameter's diagnostics name the dead key as
a possible cause. The named cost that remains: a dead-key user types `^^` as
`^ space ^ space`, or switches to the layout's no-dead-keys variant, which
the getting-started guide should say.

## Claim 4: the cutover leaves no diff noise behind

Attack: step B (`f5d6176d368`) rewrote 2,921 files with 20,111 lines
changed. Every signature line now blames that commit, and the repository has
no `.git-blame-ignore-revs`.

Verdict: **holds only within stated limits.** The noise is one commit, made
once, and later edits blame normally. The named cost is `git blame` on mark
lines; the mitigation is `git blame --ignore-rev f5d6176d368`, or a
`.git-blame-ignore-revs` file if the owner wants one at the top level.

## Claim 5: tooling handles the new marks

Attack: `^` is the line-start anchor in every regular-expression dialect, so
`grep '^self'` finds lines that begin with `self`, not marks; a search for a
mark must escape it (`\^self`, `git grep -F '^self'`). `&` was special only
in sed replacement text. The registry still carried stale notes at review
time: `OP_AMP` said "as a prefix it marks a borrow", and `OP_CARET` said "`^`
is free: no grammar rule reads it". Both are #6998 criterion 4 work, now
queued.

Verdict: **holds only within stated limits.** The compiler, formatter and
highlight rows (`SIGIL_WRITE` and `SIGIL_MOVE` in
`Compiler/JetFoundation/Data/Syntax/Highlights.jet`) read the registry, so
they follow it. The regex anchor is a named cost with a one-character
mitigation (escape or `-F`). The stale notes are criterion-4 defects, not
design gaps.

## Summary

The respelling keeps marks and operators apart (holds), and reads and diffs
acceptably within the limits above. It fails its typing claim on dead-key
layouts: a take can silently become a write, and `^self` can become `ŝelf`.
That gap is engineered on #6998 through dead-key-aware fixes on the
diagnostics that can see it, and its remaining cost (extra keystrokes on
dead-key layouts) is named here.
