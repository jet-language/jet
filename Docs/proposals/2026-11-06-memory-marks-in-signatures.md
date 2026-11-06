# Memory marks live in signatures, and `&&` marks a move (2026-11-06)

This proposal backs two ballots, **D-CALLSITE-MARK2** (where marks are
written) and **D-MOVE-SIGIL1** (which symbol marks a move). The owner asked
on 2026-11-06 for both: "keep annotations in the function declarations, …
at every call site gets a little ugly and tedious and distracting, splitting
the rule gets messy and harder to teach", and "investigate and propose `&`
for editing and `&&` for moving as a prefix sigil".

Counts below were measured on dev `b486d6c09a` over every `.jet` file in the
repository (18,352 files, 1,254,763 code lines, comment lines removed), with
`~/.cache/jet-dev/scratch/Main/ballots/census-marks.js`. They are regex counts,
so treat them as accurate to a few percent. No program was compiled for this
proposal. The examples show the proposed syntax, which the compiler does not
accept yet.

## 1. Where we are today

Three ratified rules decide which marks a call carries:

- **D-CALLSITE-MARK1=A (2026-10-06).** A call that edits a named argument
  writes `&`: `grow(&c)`. A whole local passed to a take parameter at its last
  use needs no `^`. A `^` is still required for a value with cleanup, a field
  take, a value of generic type, and anything used again.
- **D-RECEIVER-MARK1=A (2026-10-30).** A method that changes its receiver is
  called with no mark: `c.bump()`. `#Policy(writes: .Explicit)` brings the
  mark back. *This is not built:* the compiler still demands `&values.push(3)`
  (E0224).
- **D-COPY-MOVE-SPELL1=A (2026-10-07).** A copy is `x.copy()`, an early move
  is `x.move()`, and the `^` move mark was to go. *Also not built:* `^x` is
  still the move spelling, and no code calls `.move()`.

So today a reader has to learn that arguments are marked, receivers are not
(on paper), last-use takes are not, but four kinds of take still are. That is
the split you called messy.

### How often each mark appears

| Mark | Where | Count |
|---|---|---|
| `&` | on a call argument, `f(&x)` | 69,758 |
| `&` | on a method receiver, `&xs.push(1)` | about 29,800 |
| `^` | on a call argument, `f(^x)` | 4,222 |
| `^` | on a receiver, `^buf.seal()` | 125 |
| `^` | in a value position, `ys := ^xs`, `return ^x` | about 1,230 |
| `^` | in `defer close(^res)` and `task ^name { }` | 123 |
| `&` | on a parameter or `&self` in a signature | 14,246 |
| `^` | on a parameter or `^self` in a signature | 3,147 |
| `&` | a write window, `edit :: &values[2..3]` | 146 |
| `&` | a match that writes, `if &x == { … }` | 312 |

About **105,000 marks sit at calls or in value positions**. About 17,400 sit
in signatures. The call marks outnumber the signature marks six to one, and
each one repeats a fact the signature already states.

## 2. The research we already have

`Docs/research/2026-10-29-write-sigil-at-call-sites.md` studied the `&` mark
when you asked about push and pop. Its findings that matter here:

- Rust, Swift, C# and Go mark *arguments* that change but never receivers.
  Only Hylo marks every change. Mojo, D, Ada, C++, Kotlin, Java and Python
  mark neither.
- Swift built a call mark for changing methods (`x.=f()`) and removed it.
- Google's C++ guide required `&x` at calls for twelve years and dropped the
  rule in 2020. Its reasons were mostly C++ pointer costs Jet does not have.
- No Jet commit or Tower card records a bug that a call mark caught.
- No published study measures whether call marks help readers.

Nobody had studied dropping argument marks as well. The new evidence is this:

- **Mojo** is the closest design to the one you describe. A parameter says
  `mut` or `owned` in the signature, and the call is plain. Mojo writes a mark
  at a call only to end a value's life early (`x^`).
- **C++** references are unmarked at calls, and its move parameter is written
  `T&&`. See section 5.
- In Jet, a call can change only a value that was declared changeable (`:=`)
  or that arrived as a `&` parameter. So the place where a value *can* change
  is already visible at its declaration. The call mark adds the place where it
  *does* change.

## 3. Ballot D-CALLSITE-MARK2: marks live in signatures

### The rule

> A signature says what a function does to each input. A call looks like a
> read. Marks are written only where no signature decides.

- `fn bump(c: &Counter)` edits its input. `fn seal(&&self)` takes its
  receiver (using D-MOVE-SIGIL1's symbol; with today's symbol, `^self`).
- Every call is plain: `bump(score)`, `items.push(3)`, `archive(title)`,
  `buf.seal()`.
- A mark stays only where there is no signature to read:
  - a write window bound to a name: `edit :: &values[2..3]`;
  - a match that writes into its subject: `if &shape == { … }`;
  - a task that takes a name: `task &&name { … }`.
- An early move is `x.move()`, as D-COPY-MOVE-SPELL1 already ratified. The
  `^x` value form retires.

### What the compiler checks, unchanged

Safety does not depend on the call mark. The checks read the signature:

- A call that edits `score` needs `score` to be changeable. If it was
  declared `score :: 0`, the error names the signature: "`bump` edits its
  input (`c: &Counter`, line 3), but `score` is fixed. Declare it `score :=
  0`."
- Two inputs that overlap, `transfer(a, a)` with one `&`, stay an error.
- A value passed to a take parameter and used again is an error at the later
  use, naming the call: "`title` moved into `archive` on line 9. Use
  `title.copy()` there, or move the call after this line." A call never
  copies on its own.
- A part of a list passed to a writing parameter becomes a write window by
  itself: `sort_part(values[0..2])` writes those two items.

### Downsides, and how each is designed away

1. **You cannot see in plain text which call changes a value.** The editor
   shows the `&` and `&&` as faint hints at each call, as D-RECEIVER-MARK1
   already planned for receivers, and the language server underlines a
   changed argument, as rust-analyzer does for Rust. In the terminal,
   `jet audit writes <file>` lists every call that edits or takes a named
   value, the way `jet audit copies` already lists copies.
2. **A team may want the marks in text.** `#Policy(writes: .Explicit)` in a
   package requires them on arguments and receivers alike, with a Safe fix
   that inserts them. This is the policy D-RECEIVER-MARK1 already ratified,
   extended to arguments, so it adds no new mechanism.
3. **A value with cleanup closes at a call you cannot see.** The take is in
   the signature, the editor shows it, and the end of the value's life is
   checked: any later use is an error that names the closing call.
4. **Error messages lose the mark they pointed at.** Each memory error now
   quotes the signature line that causes it, so the reason is on screen.
5. **About 105,000 marks change.** The compiler already knows every one, so
   `jet fix` removes them in one cutover commit, with the grammars, spec,
   examples and snapshots, as the greenfield rule requires.

Nothing is lost that a reader needs, and the rule is one sentence.

### Before and after

Today:

```jet
fn run() {
    buf := Buffer{text: "hello"}
    &buf.append(" world")
    edit(&buf)
    &items[1].append("!")
    archive(^title)
    ^buf.seal()
}
```

Proposed:

```jet
fn run() {
    buf := Buffer{text: "hello"}
    buf.append(" world")
    edit(buf)
    items[1].append("!")
    archive(title)
    buf.seal()
}
```

From the compiler (`Compiler/JetSema/Source/Sema/Ownership/Checks.jet`):

```jet
// today
&diagnostics.push(sema_receiver_mark_replace(mark, written, "&", "writes"))
sema_comptime_body_capture(&context, environment, program, scopes, key, expr, ty, source, site)

// proposed
diagnostics.push(sema_receiver_mark_replace(mark, written, "&", "writes"))
sema_comptime_body_capture(context, environment, program, scopes, key, expr, ty, source, site)
```

### Options on the ballot

- **A. Signatures only** (recommended): the rule above.
- **B. Keep today's split:** arguments marked, receivers plain, last-use
  takes plain, four take cases marked.
- **C. Mark every call:** back to D-CAP-RECEIVER1=D, `&` and `^` on every
  argument and receiver that changes or moves.

## 4. Why a move still needs a symbol

Under A, the take mark appears only in signatures (3,147 sites) and in a task
that takes a name (24 sites). Value positions use `x.move()`. So the symbol
question is now small, and it decides whether `^` is freed.

## 5. Ballot D-MOVE-SIGIL1: `&&` for a move

### What you proposed

`&` marks an input the function may edit. `&&` marks an input the function
takes for good.

```jet
fn bump(c: &Counter) { … }
fn seal(&&self) { … }
fn gzclose(file: &&GzFile) -> Int = "gzclose"
task &&job { job.run() }
```

### Evaluation

**Frequency.** Edits are written 14,246 times in signatures, moves 3,147
times: moves are 18% of signature marks. Giving the rarer mark the longer
symbol matches how often each is typed and read.

**Clashes.** `&&` is also Jet's logical "and", used 23,545 times. Under
ballot A the move symbol never appears where an expression can: it appears in
a parameter type, after `(` before `self`, and after the word `task`.
Logical `&&` always sits between two values. The census found **no** `&&` in
any type position today, so neither the parser nor a reader meets both
meanings in one place. If ballot D-CALLSITE-MARK2 keeps call marks (B or C),
`f(&&x)` and `a && &&b` become possible, and this symbol is a worse choice.

**Readability.** `&` reads as "this function changes it"; `&&` reads as
"more than changes it: keeps it". The two marks form one family, so a reader
learns one idea with two strengths.

**Intuitiveness.** C++ writes its move parameter as `T&&`, the "rvalue
reference" that `std::move` hands over. C++ readers already link `&&` with
moving. In Rust, `&&T` is a reference to a reference, which Rust code rarely
writes. Hylo writes `sink`, Swift `consuming`, Mojo `owned`: keywords are
clearest to a beginner but cost five to nine letters per parameter.

**Typing.** `^` is a dead key on German, French, Spanish, Portuguese, Swiss
and Nordic keyboards: it waits for the next key, so `^self` needs an extra
space press. `&&` is one key pressed twice on every common layout.

**Freeing `^`.** With `&&` for moves and `x.move()` in value positions, `^`
has no meaning left. D-SIGIL-FREED2 already says a freed symbol stays an
error until a new ballot gives it a job.

### Downsides, and how each is designed away

1. **`&&` also means "and".** Kept apart by position, as above, and the
   editor colours the two differently. A mistyped `a &&b` is still "and",
   because a move mark cannot start an expression.
2. **Two keystrokes instead of one.** Moves are 18% of marks, and on
   non-US keyboards `^` costs two keystrokes too.
3. **Rust users read `&&T` as a double reference.** Jet has no reference
   types (`&T` is not a type you can store), so the Rust reading has nothing
   to attach to, and hover text says "takes the value".

### Options on the ballot

- **A. `&&`** (recommended): `fn seal(&&self)`, `file: &&GzFile`.
- **B. A word, `move`:** `fn seal(move self)`, `file: move GzFile`. Clearest
  for a beginner, longest to write, and adds a keyword.
- **C. Keep `^`:** `fn seal(^self)`. No change, `^` stays taken, dead key
  remains.

## 6. The two ballots together

The full rule a beginner learns:

> `&` in a signature: the function may change this input.
> `&&` in a signature: the function takes this input for good.
> Calls carry no marks. A value declared with `:=` can be changed; one
> declared with `::` cannot.

Before:

```jet
fn grow(list: &[Int], extra: ^[Int]) { … }
fn run() {
    xs := [1, 2]
    more :: [3]
    grow(&xs, ^more)
    &xs.push(4)
}
```

After:

```jet
fn grow(list: &[Int], extra: &&[Int]) { … }
fn run() {
    xs := [1, 2]
    more :: [3]
    grow(xs, more)
    xs.push(4)
}
```
