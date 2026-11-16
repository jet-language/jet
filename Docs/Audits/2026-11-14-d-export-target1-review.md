# D-EXPORT-TARGET1 adversarial design review (2026-11-14)

This review covers the first criterion of card #6675. Ballot D-EXPORT-TARGET1 was ratified A on 2026-11-08. Under it, a transpilation target is an ordinary Jet library that implements one public, versioned trait. The trait has three parts:

- required methods for a closed list of explicit operations;
- a small table of host leaves;
- optional idiom hooks.

What the target receives is the checked program after one shared pass has made every meaning explicit. `jet test --to <target>` compares each executable example with `jet run`.

The design claims four strengths:

1. targets for any language, written as libraries;
2. one meaning across targets;
3. readable output through idiom hooks;
4. interop.

This review attacks each claim. It compares it with the six systems the card names: Haxe with Reflaxe, Fable, Kotlin's shared IR, Nim, Gleam and LLVM. It checks each claim against the code on dev `9623399213`. Every gap ends in the gap table, with the card or ballot that owns it.

## Measured facts

| # | Read | Observed |
|---|---|---|
| R1 | `Compiler/JetIR/Source/X64Select.jet:20-29` | The x86-64 selector decides what exact Int means. A word is inline when bits 63 and 62 agree, tested by `add v, v; jo`. Add, Sub and Mul spill to `jet_int_add`, `jet_int_sub` and `jet_int_mul`. Copy and Drop call `jet_rt_int_clone` only for a big integer. None of this is an IR instruction. |
| R2 | `Compiler/JetWeb/Source/Web/Select.jet:1-4,18` | The WebAssembly writer copies that meaning ("the same exact-Int fast paths and big-integer routes, the same fixed-width traps and their stop texts, and the same stop protocol"). It imports `ir_x64_tables`, `ir_x64_overflow_message` and `IR_X64_DIVIDED_BY_ZERO` to do so. A third writer would have to copy it a third time. |
| R3 | `Compiler/JetRuntime/Source`, `Compiler/JetBackend/Source/OS/Linux.jet:276` | The runtime has 306 source files and 3249 `#Export(c)` functions. It makes Linux x86-64 system calls by number through `os_syscall_*`: 168 call sites in 30 files. One example is `Alloc.jet:178`, an `mmap` written as syscall 9. Nothing in the runtime names a host leaf that another language could supply. |
| R4 | `Core/Compiler/Compiler.jet` | The public compiler library has snapshot, scope, fact and query types (`CompilerStage`, `SchemaSnapshot`, `ProgramFact`, `query`). It has no view of a lowered program: no operation list, function body or slot. |
| R5 | `Core/Target/Target.jet` | A build target is a `Machine` whose `MachineFacts` give the triple, layout, CPU features and services. Each missing service denies the effects that need it. This is already the capability mechanism that ballot item 5 describes for transpile targets. |
| R6 | `Compiler/JetCli/Source` | There is no `--to` option on `jet build` or `jet test`. The one `--to` belongs to `jet cache prune`. |

## Peer practice

**Haxe with Reflaxe.** Haxe has first-party generators for JavaScript, HashLink, C++, C#, JVM, Python, Lua, PHP and Neko. Reflaxe is a framework that lets a Haxe library add a generator: it receives Haxe's typed expression tree after typing and dead-code elimination. Third-party targets (C++, GDScript, Rust, Go and others) exist as ordinary haxelibs, so this is the one peer that matches claim 1. The Haxe manual says Int overflow behavior is target dependent, and `haxe.Int32` and `haxe.Int64` exist so that code can opt into one behavior. Per-target standard library overrides (`std/<target>/_std`) let a target swap a module for a native one, which is how its output stays fast and readable. A Reflaxe target is tied to the compiler version whose typed tree it reads.

**Fable.** Fable compiles F# from the F# compiler's typed tree into a Fable syntax tree, then into JavaScript and TypeScript (stable) and Python, Rust and Dart (beta). All of these back ends are in Fable's own repository, and plugins can only rewrite members, not add a language. The output is readable because Fable maps F# values onto host values. Its documentation lists what differs per target as a result: numeric types, reflection and some library behavior. Interop is per target, with `Import`, `Emit` and generated bindings.

**Kotlin.** Every Kotlin platform starts from one shared front end and IR. Lowerings shared by all back ends run before back-end-specific ones for the JVM, JavaScript, Wasm and Native (through LLVM). The back ends are in the compiler. Compiler plugins can transform IR, which Compose and serialization do, but they cannot add a back end. `expect` and `actual` declarations give each platform its own implementation behind one signature. Where a host lacks a primitive, Kotlin emulates it to keep one meaning, for example `Long` on JavaScript.

**Nim.** Nim has C, C++, Objective-C and JavaScript back ends, all first-party. Its back-end documentation lists what the JavaScript back end lacks next to the C back ends: pointers, some system procedures and parts of the integer model. `importc`, `importjs` and `emit` give per-back-end interop.

**Gleam.** Gleam has two first-party targets, Erlang and JavaScript. Its documentation says `Int` has arbitrary precision on Erlang and is a JavaScript number on JavaScript, so one program can give different results on its two targets. `@external(erlang, …)` and `@external(javascript, …)` bind a function to each target's own implementation. Its output is very readable.

**LLVM.** Anyone may write a machine target by implementing LLVM's target classes (TableGen descriptions plus instruction selection). In practice targets live in-tree, because the C++ API carries no stability promise between releases. Front ends such as Rust and Swift make overflow checks explicit IR (for example the `llvm.sadd.with.overflow` intrinsics) before any target sees the program, so every LLVM target spells the same checks. Low-level output turned back into source is correct but hard to read: Emscripten's JavaScript, wasm2c's C and the out-of-tree C back end are examples.

## Claim 1: any language, written as a library

**Verdict: it holds, and it goes further than every peer but Haxe with Reflaxe. Two gaps remain.**

Only Reflaxe lets a third party add a language. Fable, Kotlin, Nim and Gleam don't, and LLVM does only in-tree. Jet keeping first-party targets on the same public trait is stronger than Reflaxe, whose first-party generators are OCaml inside the compiler.

Reflaxe and LLVM both show the cost: a target written against one compiler version breaks on the next. The ballot says "versioned" but doesn't say what a version promises or what happens when a target lags behind (gap X2).

A target library also runs inside `jet build --to` and writes files. Jet's build sandbox has to say which effects it gets (gap X9). Reflaxe targets run as Haxe macros with full file and process access, which is the outcome to avoid.

## Claim 2: one meaning across targets

**Verdict: this is the right design, and it is false on dev today.**

The peers split into two groups:

- Haxe, Fable, Nim and Gleam hand each target a typed tree, and each documents where the meaning drifts. Integers are the shared failure.
- Kotlin (shared lowerings) and LLVM (front ends make the checks explicit) make meaning explicit before any target sees the program, and they keep one meaning. D-EXPORT-TARGET1 chose that second group, so its order is right.

Four gaps keep the claim from holding:

- The shared explicit pass doesn't exist (R1, R2). Today two writers each decide what exact Int, fixed-width traps and stops mean, and the web writer does it by importing the x86-64 tables. Until that pass exists, every target is one more copy (gap X1).
- Hooks can change meaning without dropping a check. A hook receives the explicit form and "can never drop a check", but meaning is more than checks:
  - a record hook that spells a record as a Lua table or a JavaScript object gives it reference semantics, unless the hook copies where Jet copies;
  - a string hook that uses host strings changes indexing and length (UTF-16 in JavaScript, bytes in Lua);
  - a hook that prints Floats with the host's formatting changes the text (`1e21` in JavaScript);
  - a hook that iterates over a host map changes the order.

  Fable's list of target differences is exactly this list. Conformance only catches what the examples exercise, so it needs a rule about what hooks may host (gap X3).
- The runtime cannot be transpiled as the ballot says (R3). The runtime is Linux x86-64 code that makes numbered system calls in 30 files. For "the runtime arrives the same way and is transpiled by the same target" to be true, those calls must sit behind a closed host-leaf table that a target implements. Card #6151 (D-HOST-LEAF1) owns how Jet declares a runtime-provided function. It does not yet hold the list of leaves a transpile target must supply, or its size (gap X4).
- There are two capability mechanisms. Item 5 describes a capability check, and Core already has one: machine services in `MachineFacts` deny effects (R5). A second, transpile-only list would let `jet build --to lua` and `jet build --target <machine>` disagree about what a program may do (gap X7).

## Claim 3: readable output through idiom hooks

**Verdict: partly. Records, enums, functions, loops and errors can be readable. Arithmetic cannot, unless the program view carries more facts.**

Fable and Gleam are readable because they give up exact meaning: `a + b` becomes a host `+`. Jet's `Int` is exact. Its explicit form says "add, and spill to a big integer on overflow", so a Lua or JavaScript target must spell every `a + b` as a runtime call or a guarded sequence. That is output in the style of Emscripten and wasm2c, which the ballot's own option C rejects.

The fix is to give targets the facts the compiler already proves. These are value ranges (`Int(lo..hi)`, loop bounds, proven-inline operands): where a range proves no spill can happen, a hook may spell the host operator, and conformance still covers it (gap X6). Without this, claim 3 holds only for code that doesn't do arithmetic.

Readability also covers what a reader sees when the program fails. Haxe, Fable, Kotlin/JS and Nim all emit source maps or `#line` directives. The trait has no channel for source locations, so a fault in transpiled code would name host lines, not Jet lines (gap X8). Card #6675 c4 requires fault reports to be unchanged for C only.

## Claim 4: interop

**Verdict: under-specified. This is the largest gap.**

Ana's story, a Jet library inside a Lua engine, needs interop in both directions:

- Lua calling Jet: exported names and how values cross. The ballot's story assumes the engine "loads them like any other module", but no rule says what a Jet `struct` or `[Int]` looks like to the host.
- Jet calling the engine's Lua functions: an extern for one target.

Every peer has a per-target extern mechanism: Gleam's `@external(target, module, name)`, Kotlin's `expect` and `actual`, Haxe's externs and `_std` overrides, Fable's `Import` and `Emit`, and Nim's `importc` and `importjs`. D-EXPORT-TARGET1 has none. Jet's `#FFI` forms name C, C++ and assembly only, and card #6508 (FFI v3, exporting Jet to C, Python and JavaScript) is about calling compiled Jet, not about transpiled output. A per-target extern is new syntax, so it needs a ballot (gap X5).

## Gaps

| # | Gap | Home |
|---|---|---|
| X1 | One shared IR pass makes exact-Int spills, fixed-width traps and the stop protocol explicit, and the x86-64 and web writers consume it instead of deciding it (R1, R2). | New build card, a prerequisite of #6675 c3 and c4 (filed by Main from this review). |
| X2 | The program view's version: what a version promises, and a registered error naming both versions when a target was built for another one. | New criterion on #6675. |
| X3 | What a hook may hand to the host: copying, string encoding, Float text and map order stay in transpiled Core unless conformance covers the hosted form. `jet test --to` runs each target twice, once with its hooks and once with the defaults only. | New criterion on #6675. |
| X4 | The closed host-leaf table that a target implements replaces the runtime's 168 numbered syscall sites. The spec gives its list and count. | #6151 (declaring a runtime leaf) plus a new criterion on #6675 (the closed table a target implements). |
| X5 | Interop in both directions: a per-target extern for calling host code, and the host-facing shape of exported Jet values. | New ballot (new syntax), drafted with this review for Main to post. |
| X6 | The program view exposes compiler-proved value ranges, so a hook may spell host arithmetic where no spill is possible. | New criterion on #6675. |
| X7 | A transpile target declares Core `MachineFacts`, and its capability errors are the existing denial rows. There is no second capability list. | New criterion on #6675. |
| X8 | The trait carries Jet source locations; C output writes `#line`, and other targets write source maps where the language has them. | New criterion on #6675 (c4 covers C fault reports only). |
| X9 | A target library runs in the build sandbox and can write only its output directory. | #3520 c22 (typed compiler, build and tool hook stages in the sandbox) plus a new criterion on #6675. |

Seeding and bootstrapping (#6683, #6186) and native writers for new machines stay out of scope, as the ballot says.
