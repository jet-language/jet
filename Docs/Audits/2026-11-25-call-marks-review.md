# Adversarial review: calls write `^` before each named argument they change

Date: 2026-11-25. Card: #7037.

Subject: owner ballot D-CALLSITE-MARK3, which reopens D-CALLSITE-MARK2=A. The
owner's ruling of 2026-11-25 is binding and is not reopened here. A call
writes `^` before every named argument it changes, with no exceptions, so a
forwarded `^` parameter is marked too. Receivers are never marked. A move at
a name's last use needs no mark, but a written `^^` is accepted and checked,
and `jet fmt` leaves it as written. `jet fmt` inserts every required mark from
the checked compile, with no manual work at any call site, and its costs must
be engineered away. `#Policy(writes: .Explicit)` is deleted. UFCS stays
rejected, and pipes give chaining.

Sites were read at dev `de8a554abe1` (`.agent-worktrees/Main-dev-collectall`).
Probes ran on the installed `jet a0.0.46` (c137227f92), which spells the write
mark `&` and the take mark `&&`, so every probe used those spellings under
`#Policy(writes: .Explicit)`. They ran from single files and one small package
in a scratch directory that was deleted afterwards. Dev was not built or
probed. Decisions and the card were read with `node Tools/Tower/tower.mjs
decision show` and `card show`. The drafter's count script was rerun at both
revisions, and a copy of it was extended to count `x.move()` arguments. A
fresh agent made this review and repaired the ballot in place. Each verdict
below is for the repaired text, and the finding that forced each repair is
stated.

## The strongest existing solutions, at their best

- **Swift `inout`**
  (https://docs.swift.org/swift-book/documentation/the-swift-programming-language/functions/).
  "You place an ampersand (`&`) directly before a variable's name when you
  pass it as an argument to an in-out parameter." The `&` is written even
  when the caller forwards its own `inout` parameter. Mutating methods are
  called with plain dot syntax. Swift built a receiver mark (`x.=f()`) and
  reverted it (Docs/Research/2026-10-29-write-sigil-at-call-sites.md, section
  6.2). Moves are unmarked, and SE-0366 added an optional, checked
  `consume x` (https://github.com/swiftlang/swift-evolution/blob/main/proposals/0366-move-function.md).
  This is option A's closest peer, optional take mark included.
- **C# `ref` and `out`**
  (https://learn.microsoft.com/en-us/dotnet/csharp/language-reference/keywords/ref).
  "Both the method definition and the calling method must explicitly use the
  `ref` keyword," including when a `ref` parameter is passed on. `in` needs
  no call-site mark, and operators have no place to write one. Receivers,
  including `ref this` extensions, are plain. Its weakness for Jet is that
  `in` made a mark optional for reads, a second spelling A avoids.
- **Rust `&mut`**
  (https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html).
  A fresh mutable borrow is written `&mut xs`. A function forwards its own
  `&mut` parameter plainly, by implicit reborrow, so forwarding is unmarked.
  Method receivers borrow automatically. Moves are never marked. The owner's
  "no exceptions" rule departs from Rust on forwarding, on purpose.
- **Mojo** (https://mojolang.org/docs/manual/values/ownership/). `mut`
  sits only in the signature, and the call is plain: `mutate(values)`. An
  owned `var` parameter is passed a move with the postfix transfer sigil
  `take_text(message^)`; without it a copyable value is copied. The draft
  said "mut and owned sit only in the signature; the call is plain", which
  is wrong for moves: Mojo marks them at the call.
- **Hylo (Val)** (https://github.com/hylo-lang/specification/blob/main/spec.md
  and Docs/Research/2026-10-29-write-sigil-at-call-sites.md, section 6.3).
  "Arguments to inout parameters must be unique, mutable, and marked with an
  ampersand (&) at the call site," with no exceptions: `swap(&x, &y)`,
  `&p.process()` and `&target.x += delta.x`. It is the one surveyed language
  that marks receivers and operators, and there the mark also selects a
  method-bundle variant. The draft omitted it; it is the fair counterexample
  to "receivers are never marked, as in other languages".
- **Ada** (Ada RM 6.2 and 6.4.1, cited in the research report, section 5).
  `in out` and `out` modes sit only in the declaration; the actual must name
  a variable, and the call is plain. Named association (`Grow (Items => Xs)`)
  is the only call-site signal.

## Claim 1: the rule is "a named argument is a place rooted at a name"

Attack. The root recognizer exists
(`Compiler/JetSema/Source/Sema/Ownership/Checks.jet:4765-4773`), and the probe
gave right fixes for fields (`put_one(&st.items)`), labels
(`copy_to(to: &xs, …)`) and indexes (`bump(&xs[0])`). But the rule left four
cases open. `^^` on a field: today `seal(&&st.buf)` gets E0228 saying `seal`
"neither changes nor takes `st`" and "reads this input", which is false.
`^^` on a copied type: `take_n(&&n)` on an `Int` gets the same false text,
and `n` stays usable after the call (it printed 3). Both come from
`CallMarks.jet:115`, which wants a take only for a whole non-copy name, and
`CallMarks.jet:184-185`, which says "reads" for every other case. Operators
were not mentioned. The new rule refusing `name.move()` into a `^^` parameter
goes beyond the ruling and was not flagged as such. Constructors such as
`Dyn.Text(text.move())` (`Docs/Spec/spec.md:2213-2214`) needed a ruling.

Repair. The ballot now states each: field `^^` is E0225 rewriting to
`owner.field.move()`; `^^` on a copied type or a function item is E0225 with a
removal; operators are never marked, as in C#. The `.move()` rule is marked
as going beyond the ruling and amending D-MOVE-SIGIL1 for call arguments only.
Constructors and variant payloads keep `x.move()`.

Verdict: **holds after repair.**

## Claim 2: generic, trait, dynamic, function-value, closure, task and branch calls follow the rule

Attack. Probed on a0.0.46 under the policy. Generic (`put_one(&xs, 9)`),
trait-bound (`use_filler(Ones{n: 2}, &xs)`), a dynamic trait value and a
stored function value (the message named "this function value") all got the
right fix. A closure's write was marked inside its body, and calling the
closure was plain. Branch-only writers are `^` parameters by signature. Tasks
were read from the spec only.

Repair. None needed beyond stating that a stored function value's marks amend
D-FNVAL-WRITE1's "called plainly" back to that decision's first text,
`.call(&m)`.

Verdict: **holds.**

## Claim 3: a computed `<>` is fresh and takes no mark, and `^<>` needs nothing new

Attack. False. `make() |> grow(<>)` was refused with
`Jet.FieldWriteWithoutMarker: Cannot edit through `__jet_pipe_174` — it has
read access only`. A computed head lowers to a hidden immutable binding
(`Docs/Spec/syntax-decisions.md:949`). The message leaks the hidden name: the
dev text "cannot edit through `{root}`" at
`Compiler/JetSema/Source/Sema/Ownership/Checks.jet:3540-3570` prints the root
without asking `pipe_value_is_name`
(`Compiler/JetFoundation/Source/AST/Expressions.jet:87`) [INFERENCE: the dev
path matches the a0.0.46 text; dev was not probed]. `^<>` gives the
registered `<>` token a new use, a writable place, so it needs its registry
row (`Compiler/JetFoundation/Data/Syntax/Spellings.jet:71`) and sigil table
row (`Tests/Guards/SigilMeaningTable/sigil_meaning_table.jet:209`).

Repair. A computed head or a later step stays a hidden read-only value; a
writing step there is refused, and the message names `<>`. Both rows are
named in the cutover.

Verdict: **failed in the draft; holds after repair.**

## Claim 4: pipe writes "contradict syntax-decisions.md:952-953", and the fix corrects those lines

Attack. Confirmed, and worse than stated. `xs |> grow(<>)` printed `3`, then
`xs.len()` printed `3`, so the step changed `xs` in place. The ratified
D-PIPE1=A text says "a first step that also writes the name keeps the hidden
binding" (`syntax-decisions.md:951-953`), which line 949 makes immutable. By
AGENTS.md, code that conflicts with a ratified decision is a defect, not a
behavior to bless by "correcting lines". The policy message also says the call
"writes `&xs`" while its fix inserts `&<>`.

Repair. The ballot now says openly that A amends D-PIPE1, on the authority of
the ruling's "pipes give chaining": a first step on a named head changes it in
place through `^<>`. Lines 949-953 are rewritten, and the losses list the
amendment.

Verdict: **confirmed; holds after repair.**

## Claim 5: "another error blocks marks only in the function body that holds it"

Attack. False. With `badsig: fn(n: &Missing)`, the call `badsig(k)` in a
different body, `run`, got no E0228. `jet fix` then rewrote `k := 2` to
`k: 2`, making the binding immutable although the unresolved call may change
it, and renamed the parameter to `_n`. Conversely a type error inside
`broken`'s body did not stop E0228 there: `bump(m)` was still marked. The
real boundary is the call, not the body.

Repair. An error in a declaration blocks the mark at every call resolving
through it, in any body. An error inside a body blocks only calls whose callee
or argument place did not resolve. fmt names each blocked call and inserts
marks only, never other fixes.

Verdict: **failed in the draft; holds after repair.**

## Claim 6: fmt needs one compile per package, and the old two-compile pass is deleted

Attack. The cutover named `Compiler/JetCLI/Source/CLI/Host.jet:4159-4164` and
`4199-4269` for deletion. Those lines are the `#Policy(copies: .Explicit)` and
`#Policy(failures: .Explicit)` constants and the shared
`jet_cli_fmt_policy_marks`, `jet_cli_fmt_marks_of` and `jet_cli_fmt_add_mark`
behind `--explicit-copies` and `--explicit-failures`
(`Compiler/JetCLI/Source/CLI/Front.jet:358-361`, `Host.jet:4280-4290`).
Deleting them would remove two unrelated features. No fmt pass writes call
marks today; under the policy only `jet fix` does. The speed claim was
unmeasured. On a0.0.46, `jet fmt --check` of
`Examples/Systems/Files/CompressZstdStream.jet` (at c137227f92) took 18 to
56 ms, and `jet check` of it 1.4 to 2.3 s; a nine-line file checked in about
27 ms against 8 ms for fmt.

Repair. The shared machinery is kept and generalized into one
checked-insertion pass that serves call marks, copies and failures from one
compile per package, removing two compiles per file for all three (I8). The
cost is stated with the measurements, editor fix-on-save reuses the
server's last check, and the build card owns the repository measurement.

Verdict: **failed in the draft; holds after repair, with the cost measured on
a0.0.46 only.**

## Claim 7: deleting `jet audit writes` loses nothing

Attack. `Compiler/JetCLI/Source/CLI/AuditWrites.jet:18` works only by
prepending the policy, so it cannot survive the policy's deletion. Keeping a
listing would need a second site rule beside E0228, which I8 forbids. But the
audit listed changed receivers and unwritten takes too
(`Docs/Spec/Reference/cli.md:1428-1438`), and under A no command-line listing
replaces those. The draft also missed most of its plan arm.

Repair. The loss is named, with editor hints as the remaining view. The full
plan arm is in the cutover: `Front.jet:141,326`, `Host.jet:4973,4998,5166-5168`,
`CheckReceipt.jet:295`, `Entry.jet:1557`, `Publish.jet:678`,
`RemoteRun.jet:188`, `TestSettingLayers.jet:401`, `InspectB.jet:993-1004`,
`Compiler/JetCLI/hot-text.list:5765,8971` and six `Compiler/JetCLI/Tests`
files.

Verdict: **holds after repair.**

## Claim 8: reuse E0228 and E0225 rather than add codes

Attack. Reuse is right: both codes already mean a call mark that disagrees
with the signature. But E0225 `UnusedPlaceMark`
(`Compiler/Prelude/Diagnostics.jet:4279-4301`) is defined as "a place mark
sits where nothing writes or takes a named place". A receiver mark on a
writing method does write, so the text would be false (I4). E0228's
`safety_reason` at line 4331 cites the policy as well as 4332, 4339 and
4343. Today an extra mark under the policy is E0228 "remove the `^`"
(`CallMarks.jet:182-187`), not E0225.

Repair. E0225's meaning, message, why and safety text are all rewritten, with
a UI snapshot per case: receiver, read argument, fresh value, `.move()`
argument, field `^^` and copied-type `^^`. E0228 loses all four policy lines.

Verdict: **holds after repair.**

## Claim 9: about 105,000 call sites gain a mark, against 69,758 before

Attack. The script reran: at `de8a554abe1`, 22,953 files and 1,050,119 calls
give 104,619 certain sites in 1,616 files (Compiler 82,909, Core 16,266,
Tests 2,312, Jetpack 2,002, Tools 995, Examples 135); possible 126,336, of
which 83,038 forward a `^` parameter. Calibration at `5ef77a52ad^`: 67,629
written marks, 67,003 certain predictions, 65,448 right. The ballot's 0.977
and 0.968 are certain precision and recall, but the script prints 0.840 and
0.979 for the looser count, which the draft did not say. 69,758 is
D-CALLSITE-MARK2's raw text count at `b486d6c09a` (2026-11-05), not a count
of the cutover; a text pattern finds 69,715 there and 69,817 at
`5ef77a52ad^`, against 67,629 place-shaped code marks. "4,097 files" is the
cutover commit's whole diff, which changed other spellings too. Growth since
`5ef77a52ad^` was attributed to Core's database, TLS and crypto code; the
certain count grew most in Compiler (60,524 to 82,909) and Core (1,325 to
16,266), and calls grew 54%. The draft said "at most 79" whole-name
`x.move()` arguments reach a `^^` slot; the extended script finds 909 such
arguments, 408 certain and 610 possible `^^` slots.

Repair. The count section and the check instructions state the formula, the
source and revision of 69,758, the commit-size caveat, the per-folder growth
and the `.move()` figures. 104,619 × 0.977 / 0.968 ≈ 105,600 supports
"about 105,000".

Verdict: **the headline holds; the comparison and the `.move()` count failed
in the draft and hold after repair.**

## Claim 10: the misplaced slice fix is a position bug, and `jet fix` reported it applied

Attack. Confirmed. `sort_part(xs[0..2])` got E0228 underlining `[0..2]`, and
`jet fix` printed "slice.jet: applied 1 fix" and wrote `sort_part(xs&[0..2])`.
The result fails with `Jet.OperatorTypeMismatch: This operator works on
integer values only`. Package mode (`jet fix .`) and `--dry-run` ("would
apply 1 fix") agree. The index case `bump(xs[0])` was fixed right. The root
cause the draft gave was incomplete. `CallMarks.jet:122` uses
`expr_source_start`, which has no `.Slice` arm
(`Compiler/JetFoundation/Source/AST/Expressions.jet:809-818`), so a slice
falls to the default arm at line 816. Second, the rollback in
`jet_cli_fix_checked` (`Compiler/JetCLI/Source/CLI/FixCommands.jet:427-456`,
check at 430) missed the new error. Its recompile runs in Doc mode (47-66),
and the origin filter sits at 102-110 [INFERENCE: one of these hides the
error; not isolated].

Repair. Both defects are in the diagnostics section with file and line, each
to be fixed with a regression case.

Verdict: **confirmed; holds after repair.**

## Claim 11: no manual work at any existing call site

Attack. True for compiled `.jet` sources, false for Jet no compile checks:
853 Jet fences in Markdown, 433 `.template` files (427 under `Tests/Cases`),
Jet inside test strings, and about 3,059 `.stderr` snapshots quoting source.
`Tests/Guards/doc_blocks.jet` already checks runnable fences in `README.md`
and `Docs/Spec`, and `Compiler/Migrations/MarkCaret1.jet` handled fences and
snapshots for the last spelling change. The 408 to 610 `x.move()` arguments
become errors under A, and fmt does not rewrite them.

Repair. The migration row runs the checked insertion on every runnable
fence, regenerates snapshots, applies the `.move()` rewrite, and lists every
fragment that does not check by path. That residue is still uncounted (gap 2).

Verdict: **failed in the draft; holds after repair, with one named gap.**

## Claim 12: the editor writes every mark, and hints narrow to receivers and takes

Attack. Verified: `codeActionKinds` holds only quickfix, refactor.rewrite and
source.organizeImports (`Compiler/JetLSP/Source/LSP/Requests.jet:123`), so
`source.fixAll` is new work. Editor formatting is text-only
(`Formatting.jet:24-31`). The hint label joins parameter name and mark
(`Hints.jet:132-175`), so "drop the argument hint" would also drop the name.
Changed names are underlined (`Tokens.jet:120-127`).

Repair. The ballot says the label is split, the name stays, and a `^^` hint
shows only where none is written.

Verdict: **holds after repair.**

## Claim 13: the cutover list is complete

Attack. Beyond the Host.jet error of claim 6, the draft missed the policy
plumbing and many sites: `Compiler/JetFoundation/Source/Policy/Policy.jet:30,
59, 67, 222, 319, 409, 480`, `SyntaxCodec.jet:4278,4308`,
`Formatter/Statements.jet:181`, `FunctionMemory.jet:656`, the
`writes_explicit` flag in `Scopes.jet` (ten sites), `Statements.jet:1431`,
`Checks.jet:4125,4700,4971`, `AuditCopies.jet:48,77-78`,
`Parser/Expressions.jet:2576-2584`, `syntax-decisions.md:949-953, 2246, 3686`,
`DiagnosticRows.jet:3862-3871` and `Spellings.jet:71-72`. The sigil table row
at `sigil_meaning_table.jet:174` teaches `^items.push(x)`, which A refuses.
Tests missed: `Tests/Cases/Migrations/MarkCaret1RewritesEveryMark`,
`Tests/UI/policy_conflicting_module.stderr`, `Tests/diagnostics_census.tsv`
and the E0225 snapshots `Tests/UI/move_mark_*`. The tree-sitter generated
sources were not named.

Repair. All are in the ballot's cutover list.

Verdict: **failed in the draft; holds after repair.**

## Claim 14: the decisions this amends are named

Attack. D-PIPE1 was listed as "lines corrected" rather than amended (claim 4).
D-MOVE-SIGIL1's "value positions use `x.move()`" is narrowed by the
`.move()` rule and was not named for it. D-BIND-MARK1=A (ratified
2026-11-25) uses `^count: 0` bindings, which the examples follow correctly,
and its text on `^` surviving only as the policy's mark is listed. D-SHAPE-PLACE1=A
writes `decrypt(&packet.payload[16..19])`, the same root position A uses.
D-PROCESS-BUILDER-RECEIVER1 cites D-CALLSITE-MARK2's law that a call never
copies on its own; that law stands, and the ballot now says so.

Repair. Each amendment is stated.

Verdict: **holds after repair.**

## Claim 15: the options are fair and none is missing

Attack. The draft said "no option below re-asks" the rule, while B, C and D
are other rules. B's technical text claimed "the policy keeps its two-compile
fmt pass", which does not exist. Alternatives within the ruling were
considered: inserting marks only through `jet fix` (excluded, the ruling names
fmt), a Rust-style forwarding exemption (excluded, "no exceptions"), and
accepting `name.move()` into a `^^` slot as a second spelling. The last is a
real choice the ruling leaves open; I8 favors one spelling, so it stays in A
and is flagged as beyond the ruling instead of becoming an option.

Repair. B, C and D are framed as the alternatives the ruling set aside, kept
for the record. B's technical text names the real defects.

Verdict: **holds after repair.**

## Claim 16: peers are stated fairly

Attack. Mojo's entry was wrong (moves are marked `x^` at the call, and
`owned` is now `var`). Hylo, the one peer that marks receivers, was missing,
which made "as in other languages" look universal.

Repair. Mojo is corrected and Hylo added in both the comparisons and the
in-the-wild block, with sources.

Verdict: **failed in the draft; holds after repair.**

## Claim 17: the plain-language text is clear

Attack. Lint first flagged the missing review pass, then two sentences over 32
words after repair.

Repair. Both were split. Lint now reports both gap lists empty.

Verdict: **holds after repair.**

## Gaps

1. fmt's cost was measured on a0.0.46 single files only. The build card owns
   the repository and per-package measurement, and making it cheap.
2. The residue of Jet fragments that no compile checks is uncounted; the
   migration row counts it when built.
3. Today's code, a0.0.46: with an unknown type in `badsig`'s signature,
   `jet fix` rewrote `k := 2` to `k: 2` although `badsig(k)` may change `k`.
   The advice that does this was not located; `jet check --json` did not show
   it. It needs its own card.
4. Today's code: the `jet fix` rollback miss
   (`Compiler/JetCLI/Source/CLI/FixCommands.jet:427-456`) was not isolated
   between the Doc-mode recompile (47-66) and the origin filter (102-110).
5. Today's code: `jet fix` on a file with a parse error prints "no changes
   made" with no line naming the error; `jet fmt` exits 2 and names it.
6. Today's code: the pipe refusal leaks `__jet_pipe_174`
   (dev text at `Compiler/JetSema/Source/Sema/Ownership/Checks.jet:3540-3570`).
7. Today's code: E0228's extra-mark text is false for takes
   (`CallMarks.jet:115, 184-185`).
8. The `^^` probes for function items and generic `T` parameters were read,
   not run, on dev. On a0.0.46 the policy proposed `&&v` for a generic read
   parameter `v: T` where the take itself is an error.
9. Dev was not built or probed; every probe ran on a0.0.46.
10. Ada and C# entries rest on the repository's research report
    (`Docs/Research/2026-10-29-write-sigil-at-call-sites.md`); Mojo, Hylo and
    Swift SE-0366 were fetched for this review.
11. This audit must move to `Docs/Audits/` for card #7037.

## Cutover owned by this ballot

E0228 becomes a default error with its policy text removed, and its Safe edit
moves to the place root (fixing `expr_source_start` for slices). E0225 is
rewritten for receiver, read, fresh, field-take, copied-take and `.move()`
marks. The writes policy, its scope flag, its parser and codec rows, and
`jet audit writes` with its plan arm are deleted. The copies and failures
fmt passes are merged with call marks into one checked-insertion pass at one
compile per package. `^<>` gets its registry and sigil rows, D-PIPE1's text
is rewritten, and the leaked pipe name is fixed. The LSP gains
`source.fixAll`, completion of `^name`, split hint labels and no argument `^`
hint. Editor grammars, Spellings rows 71-74, CoreSurface, the syntax
dictionary, the spec chapters, prose and tests follow. A `call-mark3`
migration runs the checked insertion over every compiled source and runnable
fence, applies the `.move()` rewrite, regenerates snapshots, and lists any
fragment that does not check. The ballot lists every site with its path and
line.

## Summary

The draft had the right rule, exactly as ruled, and a sound count. Its
mechanism did not hold. Its cutover would have deleted the copies and failures
fmt passes. Its "a body error blocks only that body" was false, and so was
"a computed `<>` takes no mark". It treated a conflict with ratified D-PIPE1
as a line correction. It gave the slice bug half its cause and missed the
rollback defect. It misstated where 69,758 came from and undercounted
`.move()` arguments five-fold. It missed most of the policy plumbing, the
uncompiled sources, four take-mark edge cases, the E0225 rewording, and
Hylo.

All of it is repaired in the ballot, which now lints with both gap lists
empty. A stays recommended: it is the owner's ruling, it matches Swift and C#
at the call, and its costs are named with mitigations. The ballot is ready to
post once this audit lands in `Docs/Audits/`.
