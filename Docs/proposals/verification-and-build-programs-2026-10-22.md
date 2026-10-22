# Verification and build programs as language features

Status: proposal for owner discussion, 2026-10-22. Nothing here is ratified. Every
new spelling below is illustrative only; any that survive discussion go to a Tower
ballot first.

## Why now

On 2026-10-22, the self-hosting run kept failing on memory bugs in generated
code: a borrowed list emptied by `extend`, a match payload freed after it moved
out, a list buffer leaked, and a string header freed with the wrong size. Each
cost one full compiler build plus a manual debugger hunt. The language checker
was not at fault. The compiler generated wrong code, and nothing in the
toolchain looked for it.

Jet users will hit the same class of problem in their own projects, at a
smaller scale: an `#Unsafe` block, a foreign library, or a compiler bug.
They will also need what we needed: checks that run all at once, tests chosen
by what changed, and gates that cannot be skipped.

Jai's lesson is that a build is a program and the compiler is a library that
program drives. Jet already took the first half: `build.jet` is a Jet program.
This proposal asks how far to take the rest, for every Jet user and not only
for this repository.

## What Jet has today

- **Build program.** `fn build(b: Build) -> BuildPlan` in `build.jet`. The API
  is dispatched by method-name strings inside the compiler
  (`Compiler/JetBuildSession/Source/Session.jet:299-363`), not declared as a
  typed Core API. Errors in build programs are therefore weak.
- **Build graph data.** `Core/Build/Build.jet` already computes affected files
  (lines 152-235) and diffs build receipts (line 120). Cache keys include
  plugins and generated-module hashes (spec, lines 4414-4416).
- **Compile-time code.** It is value-only by rule S26 (spec, lines 3305-3314).
  It cannot create types or declarations, and token, AST and attribute macros
  are rejected. It runs only in the in-process evaluator, not natively.
- **Reflection.** Read-only reflection exists
  (`Examples/features/comptime/distinct_reflect.jet`).
- **Build plugins.** They exist in the spec (WASM, sandboxed), but the Jet-side
  grant reader is missing, so they are effectively dead on the Jet path.
- **Tests.** `#Test("name") { … }` blocks run by `jet test`.
- **Memory checking of generated code.** None on the current backend. The
  old backend had AddressSanitizer instrumentation, but it never covered the
  Jet runtime's own slab allocator.
- **Tier parity.** It is checked only by this repository's golden runner, not
  offered to users.

## The proposals

Each item says what a user gets, what exists, what is new, and the open
questions. They are ordered by how much they depend on each other.

### 1. Memory verification is the default for tests and development builds

**What users get.** Every `jet test` run, and every development build, uses a
checked allocator. A double free, a use after free, a free with the wrong size,
or a leak fails the test and names where the block was allocated and where it
went wrong. There is no flag. Release builds use the fast allocator.

**How it works.** The one shared runtime allocator gains a checked mode:

- each block records its size class, its live or freed state, and its
  allocation site;
- a free checks the size and the state;
- freed blocks are poisoned and held out of reuse for a while, so a stale read
  sees the poison;
- at exit, every block still live is a leak, and the report lists it by site.

Because the mode lives in the shared runtime, it means the same thing on
every tier.

**Why it matters for the language.** It turns "Jet is memory-safe" from a
claim into something every user's test suite checks continuously. It covers
the places static checking cannot reach: `#Unsafe` blocks, foreign code
handing memory across the boundary, and compiler bugs.

**Cost.** Checked builds run slower, perhaps 1.5 to 3 times. That only applies to
tests and development builds.

**Questions.**
- Should development builds (`jet run`, `jet dev`) use it, or only `jet test`?
- Should leaks fail tests, or warn? Some programs leak on purpose at exit.
- How long should freed blocks be held out of reuse? That trades memory
  against detection.

### 2. Tier parity checking for users

**What users get.** `jet test` can run each test on more than one optimization
level (and on web where it applies) and report any difference in output. A
difference is a compiler bug or undefined behavior in an `#Unsafe` block,
never a user error.

**Why.** Rule I9 says each feature has one meaning on every tier. Today only
this repository's golden runner checks that. Giving it to users finds compiler
bugs in the field, which is where the hard ones are.

**Questions.**
- Run it always (doubles test time), on a schedule, or only in CI?
- Who receives a parity failure: the user as a test failure, or a report to
  the Jet project?

### 3. A typed, declared build API

**What users get.** `Build`, `BuildPlan`, targets and actions become ordinary
Core types with declared methods. Build programs get normal type checking,
completions, documentation and diagnostics, like any other Jet code.

**Why first.** Everything below builds on it. A build program cannot be the
project's CI while its API is a set of strings the compiler routes by name.

**Questions.**
- Keep the current method names, or redesign the surface while it is being
  declared anyway?

### 4. Checks and CI are build targets

**What users get.** A project declares its checks in its build program, next
to its executables. Illustrative only:

```jet
fn build(b: Build) -> BuildPlan {
    app :: b.add_executable("app", ["run.jet"], [])
    unit :: b.add_tests("unit", ["Tests/"])
    b.add_check("fast", [unit.affected()])
    b.add_check("commit", [unit, b.parity(unit)])
    b.add_check("release", [unit, b.bench("Bench/")])
    b.default(app)
}
```

`jet check fast` on a laptop, a hook and a CI server run the same
target. A CI configuration file shrinks to one line that names the target.

**What it brings.**
- **Cached results.** A check result is keyed by the hash of its inputs and
  the compiler identity, so unchanged work is not rerun anywhere.
- **Receipts.** A passing check writes a receipt tied to the commit. A team
  can require a receipt instead of trusting that someone ran the tests.
- **Ordered gates.** An expensive check can depend on a cheap one, so it cannot
  start until the cheap check passed for that exact commit. In this repository
  that rule is prose today.

**Questions.**
- Should Jet define standard tiers (`fast`, `commit`, `release`), or leave the
  names to each project?
- Should receipts be signed?

### 5. Affected-test selection built into `jet test`

**What users get.** `jet test --affected` (illustrative) runs only the tests a
change can reach. The compiler already knows the dependency graph, so the
answer is exact rather than guessed from file paths.

**Why it matters.** Fast feedback is the difference between running tests
on every save and running them before lunch. It also makes item 6 cheap
enough to be mandatory.

**Questions.**
- How should a change to the compiler or the standard library count? It
  affects everything.

### 6. Hooks generated from the build program

**What users get.** The build program declares what runs before a commit and
before a push. Jet installs thin hooks that call those targets. Hooks are then
versioned with the project and identical for everyone.

**Questions.**
- Git only, or a version-control-neutral design?
- Installed automatically on first build, or by an explicit command?

### 7. A compiler message loop

**What users get.** This is Jai's central idea. A build program can ask the
compiler to stream events to it: each declaration after checking, each
diagnostic, each function after lowering. Jet code then reacts to them.
Uses:

- **Project rules as code.** Examples: "no `#Unsafe` outside this folder";
  "every public function documents its errors"; "no network calls from this
  module". Each one is enforced with the project's own diagnostics.
- **Collect-all by construction.** The compiler streams every item, so a
  rule reports every violation in one run.
- **Generated code from what the program declares.** Examples: a route table,
  a serializer, a registry.

**Conflict.** As written, S26 forbids compile-time code from creating
declarations. Read-only events (rules and reports) fit inside S26. Code
generation from events does not.

**Questions.**
- Read-only events first, or the full loop including generation?
- How much of the compiler's internal model may Jet expose as a stable API?
  This is a long-term compatibility promise.
- Does it run in the evaluator (slow, available now) or wait for native
  compile-time code?

### 8. Tests generated from declared types

**What users get.** Laws a type promises are tested automatically:

- encode then decode returns the same value for every serializable type;
- ordering is consistent for every `Comparable` implementation;
- equal values hash equally;
- every owning type releases what it owns exactly once (with item 1).

**Two ways to build it.**
- **Inside the test runner.** It reflects over the program's types at run
  time. This fits S26 and needs no new language feature.
- **Through item 7.** Generation at compile time needs the S26 change.

**Questions.**
- Automatic for every type, or does a type opt in?

## How they fit together

- Items 1 and 2 stand alone and protect users and the compiler right away.
- Item 3 is the foundation for items 4 to 7.
- Item 5 makes item 6 fast enough to require.
- Item 7 is the largest change and the one that makes Jet feel like Jai. It is
  also the one that touches a ratified rule.

## Questions for the owner

1. Is memory verification the default for `jet test` only, or for every
   development build too? Do leaks fail?
2. Should users get tier-parity checking, and how often should it run?
3. Should the build API become a declared Core API now, and may its surface
   change while that happens?
4. Should checks, cached results and receipts be part of the build program,
   making CI one line?
5. Should Jet get a compiler message loop? If so, read-only first, or with
   generation (which reopens S26)?
6. Should type-law tests be automatic or opt-in?
