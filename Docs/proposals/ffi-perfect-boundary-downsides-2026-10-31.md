# FFI downsides: what each one costs, and what we do about it

Status: design draft by FFIDesign-1, 2026-10-31, written for the owner. Nothing here is ratified.
Companion files: `ffi-perfect-boundary-2026-10-31.md` (the design and gallery) and the ballots on Tower card #6366 (the owner choices that remain).

This file lists every cost or limit that remains in the FFI design: the current proposal
(the 2026-10-30 proposal this replaces) and the ratified decisions it builds on.
Each entry gives:

- **Who it hurts:** beginners, experts, or both.
- **Today:** what the ratified design or the code does now.
- **Fix:** the change that removes the cost. Every fix is folded into `ffi-perfect-boundary-2026-10-31.md`, and the
  section number points there. A fix that needs an owner choice names its ballot.
- **Residue:** what stays after the fix, and why it cannot be removed. "Inherent" is used only
  with a reason: a fact is not available anywhere, a law of the platform forbids it, or two
  safety goals conflict.

Words used throughout:

- A **fact** is one thing Jet knows about a foreign function: who frees a result, whether a
  pointer may be null, how long a borrowed pointer stays valid, which thread may call it, how it
  reports failure.
- A **pack** is a published file of facts for one library version (`ffi-perfect-boundary-2026-10-31.md` §5.3). It is an
  ordinary `#Import module` overlay, published on Jetpack.
- A **placement** is where foreign code runs: in the program, in a wasm sandbox, or in a child
  process.

## Summary table

| # | Area | Biggest remaining cost | Fixed by | Residue |
|---|---|---|---|---|
| 1 | Setup | C compiler, Cargo, Python must be installed by hand | toolchains arrive from the Jetpack store on first use | one download per toolchain; a closed library must exist |
| 2 | Toolchain | header read with other flags than the library was built with | flags from pkg-config, compile_commands and debug info | a stripped binary built with secret flags |
| 3 | Build time | clang parse, cargo build, C++ shims on every cold start | content-addressed caches plus prebuilt caches on Jetpack | the first bind of a never-seen header |
| 4 | Binary size | wrappers, shims, other runtimes | thin inline wrappers, unused rows dropped | a foreign runtime you use is shipped |
| 5 | Call overhead | text copies, thread attach, isolation hops | zero-copy text and lists, batched isolation | process isolation costs a context switch |
| 6 | Safety holes | in-process C can corrupt anything | checked edges, `contain`, wasm and process placement | in-process native code stays trusted |
| 7 | Unknown facts | headers do not say who frees, or for how long | packs, header attributes, `jet bind --suggest`, safe defaults | a library with no facts gets a plainer API |
| 8 | Lifetimes | a borrowed C pointer has no lifetime | views tied to their owner; invalidating calls take `&` | invalidation Jet cannot see costs a runtime check |
| 9 | Threads | callbacks on foreign threads, main-thread libraries | thread facts, owner delivery, attach | a library that lies about threads |
| 10 | Async | foreign futures, promises and callbacks | every foreign completion is a wait point | a blocking call with no async form holds one thread |
| 11 | Errors | status codes, errno, exceptions, panics | `#Check(fn)`, per-call errno, shims catch everything | a callback with no failure channel |
| 12 | Debugging | crashes inside C show nothing useful | one backtrace across languages, `jet perf --lens=ffi` | stripped closed binaries show addresses |
| 13 | Versioning | an upgrade silently changes the ABI | contract fingerprint checked at build and at load | behavior changes with the same signatures |
| 14 | Cross-compiling | per-target headers, sysroots and libraries | pinned clang plus target sysroots on Jetpack | Apple SDKs cannot be redistributed |
| 15 | Web tier | no native code in browsers | C source compiles to wasm, JS calls the engine | closed native binaries cannot run in a browser |
| 16 | Comptime | foreign calls refused at compile time | pure rows run in the wasm placement | target-dependent and closed rows stay refused |
| 17 | Distribution | bindings tied to one machine | packs and caches on Jetpack, `jet bind --freeze` | licenses of what you link |
| 18 | Rust | x86_64 Linux musl only, Cargo by hand | pinned Rust toolchain on every target | a crate build is slow once |
| 19 | Safe lends | `&` and `^` calls still need `#Unsafe` | checked rows need no `#Unsafe` at the call | rows nobody checked keep their vouch |
| 20 | Callbacks | only capture-free callbacks, double join at stop | closures with captures for the call; scope-end stop | a C API that keeps a callback forever |
| 21 | Exports | `#Export(c)` takes scalars; records cross as JSON | typed records, lists, errors, callbacks, async per host | hosts with no async form get a callback |
| 22 | Python | sidecar copies every array | shared-memory arrays; one placement option | a call to a child process costs microseconds |
| 23 | Effects | an opaque C call gets every effect | effect facts from packs and attributes; wasm enforces them | an in-process effect bound is a promise, not a wall |

The rest of the file explains each row.

---

## 1. Setup

**Who it hurts:** beginners most.

**Today.** Several things must be installed by hand:

- A C compiler. `jet run` of `Examples/features/lowlevel/native_c` reports E2105, "the C compiler
  is not installed", on a machine without one (probe recorded in the D-FFI-CINPUT1 ballot).
- Cargo, for Rust crates (D-FFI-RUST1=A keeps Cargo as the user's tool). Missing cargo is E0703.
- CPython, for `py.*`.
- The C library itself, through pkg-config or a system package.

The package also needs `authority: { allow: [FFI, …] }`. A beginner who forgets it gets a
diagnostic, but has to learn the word first.

**Fix (v2 §6.1).**

- Every toolchain arrives from the Jetpack store, pinned and hash-checked, the first time a
  project needs it. D-ADOPT-CC1=B already ratified this for clang; v2 applies the same rule to the
  Rust toolchain (ballot D-FFI-RUST2), CPython (D-DEP-PY1 already allows it), Node, the JVM and
  Go.
- Libraries named as `c@nixpkgs:<name>` are fetched and built by Jetpack. `c@system` stays for
  libraries the machine already has.
- A missing `FFI` authority is a diagnostic whose fix `jet fix` applies. The one-file script
  path (`jet run file.jet`) asks once, the same way `jet repl` asks for effects
  (D-REPLCOREEFFECT1).

**Residue.**

- The first use of a toolchain downloads it: about 150 MB for clang plus a sysroot, and about
  300 MB for Rust. `jet fetch` warms the store ahead of time, for offline work.
- A closed library that is only on the user's machine must be there. Jet cannot fetch what
  nobody publishes. This part is inherent.

## 2. Toolchain mismatch

**Who it hurts:** experts, silently.

**Today.** The pinned clang reads the header (D-CIMPORT1=A). The library may have been built
with other flags, such as `-D_FILE_OFFSET_BITS=64`, `-fpack-struct`, or another
`-DSQLITE_OMIT_*` choice. When that happens, the struct layouts Jet reads are wrong, with no
error.

**Fix (v2 §6.2).** The binder reads the header with the library's own flags:

- `pkg-config --cflags` for system libraries;
- `compile_commands.json` for source the project owns;
- the Jetpack build recipe for `c@nixpkgs:` libraries, whose flags Jetpack knows exactly.

When the shared object carries DWARF or BTF type information, the binder also compares every
struct it maps with the library's own record of it. A difference is a build error naming the
field: E3227 (PROPOSED), "header layout differs from the library's debug info".

**Residue.** A stripped binary built with flags it does not publish cannot be checked. Jet reports
the layout facts as `Read` (from the header) and not as `Proved`. `jet audit foreign` lists them.
This is inherent: the fact exists nowhere Jet can read it.

## 3. Build time

**Who it hurts:** everyone, on the first build and on CI.

**Today.**

- Header binding runs at compile time on a cache miss (D-FFI-UNIFY1 script tier).
- `windows.h` or the full GTK headers are tens of thousands of lines.
- A Rust crate needs a cargo build of the crate and its dependencies.
- C++ needs a shim per library, plus explicit `--instantiate` flags in today's implementation.

**Fix (v2 §6.3).**

- Every generated artifact is content-addressed. The key covers the header closure hash, the
  flags, the target, the reader version and the pack version.
- Jetpack serves prebuilt caches for the published library versions. A cold build of
  `use c.sqlite3` downloads a 40 KB binding instead of running clang.
- Binding is lazy per row. The reader indexes the header once, and only the rows the program
  calls become checked declarations and IR.
- Rust and C++ shims are built once per (crate version, target, used items) and cached in the
  same store.

**Residue.**

- The first bind of a header nobody has bound before pays one clang parse: roughly 0.2 to 2
  seconds.
- The first build of a large crate pays one cargo build, which can take minutes, once per
  machine or once per CI cache.
- Both are inherent to compiling foreign code at all. The cache removes them from every later
  build.

## 4. Binary size

**Who it hurts:** experts shipping small binaries, embedded and web.

**Today.**

- Wrappers are generated per row.
- C++ adds a shim archive.
- Python embedding adds libpython (about 5 MB). JS on native adds QuickJS in a wasm component.

**Fix (v2 §6.4).**

- Wrappers are ordinary Jet functions marked for inlining. A blittable row compiles to the bare
  call.
- Rows the program never calls are not emitted.
- C++ shims contain only the methods and template instances the program uses.
- Static linking of `c@nixpkgs:` libraries lets the linker drop unused library code.

**Residue.** A foreign runtime you use is a runtime you ship: CPython, the JVM, the Go runtime,
QuickJS. This is inherent. The `.Process` placement moves that runtime out of the binary but not
off the machine.

## 5. Call overhead

**Who it hurts:** experts in hot loops (games, numerics).

**Today.**

- A `String` argument crosses as "a packed NUL-terminated copy freed after the call"
  (`Compiler/JetIR/Source/BuildForeign.jet`).
- Each callback from a foreign thread must attach to the runtime.
- The Python sidecar sends every value through a message.

**Fix (v2 §6.5).**

- **Text.** Jet text storage keeps a trailing NUL byte and a cached "no interior NUL" bit. A
  `String` argument then passes its own pointer with no copy. Only text with an interior NUL
  copies, and only when the row accepts it.
- **Lists.** A list crosses as pointer plus length, with no copy, under `&` (D-FFI-CAP1) or a
  read borrow.
- **Checks.** A conversion that facts prove unnecessary (a blittable struct, a non-null
  annotated pointer) is not emitted.
- **Attach.** A foreign thread attaches once and stays attached until it exits, so callbacks pay
  attach once per thread, not once per call.
- **Isolated libraries.** These batch consecutive calls with no Jet-visible effect between them,
  and lists cross through shared memory.

**Residue.**

- A call into a child process costs a context switch, roughly 2 to 10 microseconds. That is
  inherent to a separate address space; batching only amortizes it.
- Wasm placement runs foreign code at roughly 1.1 to 1.5 times native time, because of bounds
  checks. That is inherent to software isolation.
- Text with an interior NUL still copies. C cannot represent it otherwise.

## 6. Safety holes

**Who it hurts:** everyone, and beginners cannot see it.

**Today.**

- In-process C can write anywhere. A `&` lend is a promise the C side "must not keep", and C can
  break it (D-FFI-CAP1 says so plainly).
- A vouched row (D-FFI-VOUCH1) can be wrong.
- Native code can call the operating system directly, bypassing Jet's effect rows.

**Fix (v2 §7, ballot D-FFI-ISOLATE1).** Jet offers four placements on one ladder, chosen by one
option per library:

1. `.InProcess` is the default: fastest, and trusted.
2. `.Contained` gives tracked-handle pointers inside a fence. This is D-MEM-GUARANTEE1's
   `contain:`.
3. `.Wasm` sandboxes C, C++ or Rust source. A memory bug cannot touch Jet memory, and the
   library can only reach the imports Jet grants it.
4. `.Process` runs any library, including closed binaries, in a supervised child process.

Every edge check that facts allow runs at the boundary, whatever the placement: null checks,
length checks, generation-checked handles, and layout checks at load. `jet audit foreign` lists
each trusted row and its reason. `--profile=hardened` (D-HARDENED1), like `policy: { harden: true }`
(D-MEM-GUARANTEE1), runs every in-process foreign dependency at least `.Contained`.

**Residue.** In-process native code is trusted. No compiler can prove arbitrary machine code safe
(Rice's theorem), and code in the same address space can always reach the same memory. The only
real guarantee is a separate memory: wasm or a process. Its cost is in row 5. Jet makes that
choice one option, never a rewrite.

## 7. Unknown facts

**Who it hurts:** beginners, through confusing APIs; experts, through boilerplate.

**Today.**

- A C header does not say who frees a returned pointer, how long a borrowed one lives, which
  thread may call, or how failure is reported.
- The spec copies a returned `char*` into `String` and never frees it, which leaks when the
  caller owns it. Other pointer returns are E3202.
- The fixture headers under `Examples/ffi/adaptations/` carry facts as `// jet-ffi-*` comment
  lines, which only work for headers you own.

**Fix (v2 §5, ballot D-FFI-PACK1).** Facts come from five places, strongest first:

1. **Checked.** Jet source, wasm or process placement, or a Jet-built library.
2. **Header attributes.** Clang nullability, `counted_by`, `noescape`, `malloc(free)`,
   `ownership_returns`, `const`/`pure`, and the `// jet-ffi-*` comments in headers you own.
3. **Packs.** An `#Import module` overlay for a library version, published on Jetpack, reviewed,
   and fetched with the library. This is what TypeScript's DefinitelyTyped and Swift's API notes
   do for their ecosystems.
4. **The project's own overlay.** This exists today.
5. **`jet bind --suggest`.** It drafts facts from naming patterns, documentation comments and an
   instrumented test run. It writes a draft overlay for review and is never trusted
   automatically.

A row with a missing fact gets the safe shape, never a guess:

| Missing fact | Safe shape |
|---|---|
| nullability | `T?` (D-FFI-CMAP1) |
| ownership of returned text | copy, no free, and warning W3230 (PROPOSED) naming the row and the one-line fix |
| retention of a callback | managed registration (D-FFI-CALLBACK2) |
| thread rule | the handle cannot cross tasks |
| error convention | the raw status is returned as an `I32` the caller must use |

**Residue.** A library with no pack and no annotations gets a plainer API until someone writes
its facts. The fact does not exist in the header, and guessing would change program meaning
(D-FFI-AUTO1 forbids it). The cost is a few lines per library, written once and shared through
Jetpack.

## 8. Lifetimes

**Who it hurts:** experts; beginners who meet use-after-free.

**Today.**

- `sqlite3_column_text` returns a pointer valid until the next `sqlite3_step`. Jet has no way to
  say that, so it copies.
- D-FFI-GUEST1 ratified generation-checked views for exports.

**Fix (v2 §5.4).**

- A foreign row may return `View<T> from owner`, which is Jet's existing returned-view
  provenance.
- The pack marks the calls that invalidate the view, and those calls take the owner as `&`.
  Jet's ordinary borrow rules then make "use after step" a compile error, with no runtime cost.
- Where invalidation is not tied to one owner (a global cache, a timer inside the library), the
  view is generation-checked. A stale use returns `ExpiredView`, as in the D-FFI-GUEST1 example.

**Residue.** Invalidation by something Jet never sees, such as another C thread freeing the
buffer, can only be caught by the generation check at access, or by copying. That is a runtime
cost of a few nanoseconds per access, and only for those rows. A library that frees memory
behind its own documented contract cannot be defended against in-process. That is inherent; see
row 6.

## 9. Threads

**Who it hurts:** both. This is the source of most real FFI crashes.

**Today.**

- D-CABI-CALLBACK1 requires every callback to be safe on any thread: no captures, no allocation.
- D-FFI-CALLBACK2 validates affinity for retained callbacks.
- Main-thread-only libraries (Cocoa, GLFW and so raylib, most GUI kits) have no stated rule.
- `errno` is thread-local and can be overwritten before Jet reads it.

**Fix (v2 §8.3).**

- **Thread facts.** Each library or row has a thread fact: `.Any`, `.Caller` (same thread as the
  handle's creator), or `.Main`. The fact comes from the pack; unknown means `.Caller`. A handle
  with `.Caller` cannot cross a task boundary (E1102, existing). A `.Main` row called from
  another task is E3231 (PROPOSED), whose fix offers `app.on_main(...)`.
- **Callbacks on foreign threads.** A callback that a C library calls on its own thread runs
  attached on that thread, if its captures are sendable. Otherwise it is a compile error with two
  fixes: make the captures sendable, or deliver to the owner task with `.deliver(.Owner)`. Jet
  never moves the callback silently, per D-FFI-AUTO1.
- **errno.** `errno`, `GetLastError` and `NSError` are captured by the wrapper immediately after
  the call, on the same thread, before any other code runs.

**Residue.**

- A library whose thread fact is wrong can still race. That is inherent to trusting in-process
  code.
- A C callback that blocks waiting for a Jet task that is itself waiting on that C call is a
  deadlock. The runtime detects this cycle and reports it with both stacks (E3232, PROPOSED), but
  it cannot resolve it.

## 10. Async

**Who it hurts:** both. Jet has no `async`/`await`. Tasks wait at wait points (E0040 teaches
`task`).

**Today.**

- D-FFI-JS1 and the Python and Dart binders reject async signatures.
- `ffi.completion<T>()` is ratified in D-FFI-PRIMS1 but not built.
- Exports cannot be async.

**Fix (v2 §8.4).** Every foreign completion becomes a wait point:

- a JS `Promise`;
- a Python awaitable;
- a Rust `Future`, polled by Jet's scheduler through a waker bridge;
- a C# `Task`;
- a C completion callback, via `ffi.completion<T>()`.

The call looks like an ordinary call inside a task. Other tasks run while it waits.
Cancellation crosses where the foreign side supports it: `AbortSignal`, Python `cancel()`,
dropping the Rust future, or a declared C cancel function. Otherwise cancelling waits for
completion. It never fakes success.

Exports run the other way. A Jet function that waits becomes:

- an `async def` in Python;
- a `Promise` in JS;
- a Rust `Future`;
- a C function taking a completion callback plus a cancel handle.

Blocking C calls marked `#Blocking` (PROPOSED fact) run on a blocking-thread pool, so they do
not stall other tasks.

**Residue.**

- A blocking C call with no fact runs inline and occupies one carrier thread while it blocks.
  The runtime notices a carrier blocked for over 50 ms and reports the row once with the fix:
  add `#Blocking`.
- A foreign API with no cancellation cannot be cancelled. That is inherent.

## 11. Errors

**Who it hurts:** beginners, who get integer codes; experts, who write the same wrapper every
time.

**Today.**

- D-CABI-RESULT1=C forbids generic fallible C declarations, and "the compiler invents no error
  adapter". The user writes a wrapper per function.
- C++ exceptions become `CppError` (D-FFI-CPP1).
- A Jet panic inside a C callback is an open question in today's code: callbacks must be
  panic-free (D-CABI-CALLBACK1).

**Fix (v2 §5.5).**

- **`#Check(fn)`** (PROPOSED marker, beside `#Close(fn)`) names an ordinary Jet function that
  turns a raw status into a Jet error. The pack writes it once for the whole library. The
  compiler still invents nothing: the function is visible Jet code in the pack, and
  `jet inspect bind` shows it.
- **errno** is captured per call (row 9).
- **C++ exceptions** become `CppError` with the exception type name and `what()`. **Rust panics**
  become `RustPanic` with the message.
- **Python exceptions** become `PyError` with the type, message and traceback lines. **JS
  throws** become `JSError`.
- **A Jet panic inside a callback** never unwinds through C. The callback returns the row's
  declared failure value (a pack fact), the registration stops, and the panic resumes at the Jet
  frame that made the outer C call, once C returns.

**Residue.** A C API whose callback has no failure channel, such as the `qsort` comparator,
cannot be stopped mid-way. Jet returns 0 for the remaining comparisons and raises the panic when
`qsort` returns. The array order is then unspecified, and the panic says so. This is inherent:
`qsort` offers no way out.

## 12. Debugging

**Who it hurts:** everyone, at the worst moment.

**Today.**

- A crash inside C shows a native signal.
- D-RT-DEFECT-REPORT1 and D-RELEASE-DEBUG1 are still open.
- There is no FFI trace.

**Fix (v2 §9, ballot D-FFI-TRACE1).**

- **One backtrace.** It is symbolized from DWARF for C, C++ and Rust, and from Jet's own maps for
  Jet frames. Each frame is labelled with its language, and the crossing frames are marked.
- **Signals.** A fault inside foreign code reports the last crossing: the row, its arguments
  summarized, and the thread.
- **Trace.** `jet perf run <app> --lens=ffi` (D-PERFSESSION1) records every crossing into the
  `.jettrace` (D-ARTIFACT-EXT1): row, thread, duration, bytes copied, and placement, as per-row
  counts by default or each call with `--lens=ffi:calls`. `jet perf view --lens=ffi` prints it.
- **Hardened tests.** `jet test --profile=hardened` fences every dependency, so owned C and C++
  source runs with bounds and use-after-free checks.

**Residue.** A stripped closed binary shows `libfoo.so+0x1a2b` frames. That is inherent: the
symbols do not exist. Jet prints the build ID, so a symbol server can resolve the frames later.

## 13. Versioning

**Who it hurts:** teams, months later.

**Today.**

- The binding stamp (`jet-ffi-descriptor-v1`, digest) is checked at build.
- At run time, a changed shared library loads silently.
- D-FFI-AUTO1 has `--freeze` and `--policy frozen`.

**Fix (v2 §6.6).**

- **At build.** A changed header or pack changes the contract fingerprint. Under `--freeze` that
  is E3225 (PROPOSED), which prints the semantic diff: "row `sqlite3_column_text`: result
  ownership changed from borrowed to owned".
- **At load, before `run` starts.** Jet checks the library's soname, required symbol versions,
  and the presence of every called symbol. If the library ships a Jet contract note (a Jet-built
  library, or a pack hash recorded in the build), Jet checks that too. A mismatch is a typed load
  error, E3226 (PROPOSED), never a crash at first call.

**Residue.** A library that changes behavior while keeping every signature and version, such as
a function that starts returning borrowed memory it used to allocate, cannot be detected. That is
inherent: behavior is not in the binary's interface. Packs are pinned to version ranges, which
narrows the window.

## 14. Cross-compiling

**Who it hurts:** experts, CI.

**Today.** C bindings are per target (`ffi.layout<T>(target:)` in D-FFI-PRIMS1). D-FFI-RUST1
limits Rust to x86_64 Linux musl.

**Fix (v2 §6.7).**

- The pinned clang reads headers for the build target.
- Jetpack serves target sysroots: Linux glibc and musl on x86_64 and aarch64, Windows through
  mingw-w64, and wasm32-wasi.
- `c@nixpkgs:` libraries cross-build through nixpkgs' cross toolchains.
- The Rust toolchain is pinned for each target (ballot D-FFI-RUST2).

**Residue.** Apple SDKs cannot be redistributed under their license. Cross-building for macOS or
iOS needs the user's own SDK path; Jet detects it and says so. A closed library must be supplied
for each target by its vendor. Both are inherent: legal and supply limits.

## 15. Web tier

**Who it hurts:** anyone shipping to a browser.

**Today.**

- `use js.X` targets the browser engine (D-FFI-JS1).
- C on the web is not described.
- `ffi.Library.open` is a compile error on web (D-FFI-PRIMS1).

**Fix (v2 §8.6).** Where the library's C, C++ or Rust source is available (from your project or
from Jetpack's nixpkgs source), `use c.X` on the web target compiles it to wasm with the same
machinery as the `.Wasm` placement. The program text does not change. Python on the web uses
Pyodide, which is opt-in because of its size. Browser APIs come through `use js.X` with
generated types.

**Residue.**

- A closed native binary cannot run in a browser. That is a platform law.
- Browsers forbid blocking the main thread, so a blocking C call must run in a worker.
- Pyodide is about 10 MB.

## 16. Comptime

**Who it hurts:** experts who want tables built from a C library at compile time.

**Today.** The spec says compile-time code links no foreign functions. Comptime itself runs
natively at O0 in a child of the compiler (I9), so a C hash function cannot fill a compile-time
table.

**Fix (v2 §8.7, ballot D-FFI-COMPTIME1).** A row may run at comptime when all four hold:

1. it is pure by fact;
2. its Jet row uses only fixed-width types (`I32`, `U64`, `F64`, `[U8]`, `String`, and
   `#Layout(c)` records of those), after a pointer and `size_t` count collapse to one list;
3. its source is available;
4. that source builds for the `.Wasm` placement.

The comptime child runs it in a wasm sandbox: deterministic and with no imports, so the build
machine is never exposed. The source builds with strict float flags (`-ffp-contract=off`), and
`jet test` replays each recorded comptime call against the native build for each target, failing
with the row and input on any difference.

**Residue.**

- Rows whose Jet types depend on the target (`long`, a bare `size_t`, pointer layouts) are
  refused at comptime, because a wasm32 run could give a different answer than the target. The
  refusal names the type.
- Undefined behaviour in the C source can still differ between wasm and native; the replay in
  `jet test` catches it, but only for inputs the build used.
- Closed binaries are refused, because they cannot run in the sandbox.

All three follow from keeping one meaning on every tier without exposing the build machine.

## 17. Jetpack distribution

**Who it hurts:** package authors and their users.

**Today.**

- A package that wraps a C library declares `deps: { z: c@nixpkgs:zlib }`. Its consumers build
  the binding on their machine.
- Generated bindings live in `.jet/bindings/` and are machine-local caches.

**Fix (v2 §6.8).**

- A package publishes its dependency line, its pack (if it adds facts), and its Jet wrapper.
- Jetpack publishes prebuilt binding caches keyed by content hash, so consumers download them.
- `jet bind --freeze` writes the accepted plan into `.jet/lock` (D-FFI-AUTO1), and that file is
  committed.
- `c@nixpkgs:` libraries link statically by default, so one binary ships.
- `jet audit licenses` reports what each linked library's license requires. LGPL static linking
  requires relinkable objects, and Jet prints that obligation.

**Residue.** A license of a library you link is your obligation. A closed vendor SDK cannot be
republished on Jetpack. Both are inherent: legal limits, not technical ones.

## 18. Rust

**Who it hurts:** everyone who wants a Rust crate; this is the most-asked foreign language after
C.

**Today.** D-FFI-RUST1=A keeps Cargo as an external tool and limits user Rust to static musl
helpers on x86_64 Linux. D-FFI-RUSTIMPORT1=A gave Rust the same `#Import module rust.X` form as
C. The bridge supports by-value types plus `&`, `^` and `#Close`; trait objects and callbacks
are out.

**Fix (v2 §8.2, ballot D-FFI-RUST2 amending D-FFI-RUST1).**

- The pinned Rust toolchain comes from Jetpack for every supported target.
- `use rust.regex` reads the crate's public API from rustdoc JSON. Rustdoc JSON is unstable, and
  it is read only by the pinned toolchain whose output format Jet knows.
- The reader generates a C-ABI bridge crate for the items used, and builds it for the target.
- Generics are instantiated on use, as for C++ templates.
- Traits used as `dyn` become Jet trait objects through a vtable shim.
- `Result<T, E>` becomes `T E!`. Panics become `RustPanic`.

**Residue.**

- A crate build is slow once per version and target, then cached.
- Rust has no stable ABI, so every crate crosses through a generated C-ABI shim. The direct cost
  is one extra call frame, usually inlined away by LTO in release builds.
- `async` crates need their executor bridged. Tokio-based crates run their runtime on a pool
  thread.

## 19. Safe lends need `#Unsafe` today

**Who it hurts:** beginners, who see `#Unsafe` around ordinary buffer reads; experts, who lose
the signal value of `#Unsafe`.

**Today.** D-FFI-VOUCH1 notes that "capability-bearing calls (`&` / `^` loans) keep their
audited `#Unsafe` region". `Examples/features/lowlevel/opaque_handles/run.jet` wraps every
harfbuzz call in one `#Unsafe` block. The ratified D-FFI-CMAP1 example calls `lib.fill(&values)`
with no `#Unsafe`, so the two ratified examples disagree.

**Fix (v2 §5.6, ballot D-FFI-VOUCH2 amending D-FFI-VOUCH1).** A `&` or `^` argument to a row that
was checked against a header, a C source, a pack or a Jet-built library needs no `#Unsafe` at the
call. The row's check carries the obligation. `#Unsafe` stays only where Jet has nothing to check
against: vouched rows (inside their one block, D-FFI-VOUCH1), raw `core.ffi` steps, and raw
pointers.

**Residue.** A checked row can still be implemented wrongly in C. That is row 6's residue,
handled by placement, not by `#Unsafe` at every call. `jet audit foreign` still lists each row by
its evidence.

## 20. Callbacks

**Who it hurts:** everyone who sorts, iterates, or handles events.

**Today.**

- D-CABI-CALLBACK1=A allows only capture-free, allocation-free callbacks with "no hidden context
  pointer". `tests/ui/cffi_callback_capture.jet` shows the result: capturing `offset` in
  `c.call((x) -> x + offset, 40)` is E3203.
- D-FFI-CALLBACK2=A ratified managed registrations for retained callbacks. Its example needs a
  double join to stop: `(stopped.join() ?? panic("task failed")) ?? panic("unsubscribe failed")`.

**Fix (v2 §5.7, ballot D-FFI-CALLBACK3 amending D-CABI-CALLBACK1).**

- **Call-scoped callbacks.** When a fact says a callback is used only during the call, any Jet
  closure works: it may capture, allocate, and write `:=` locals the way a non-escaping local
  lambda does today. The fact comes from clang `noescape`, a pack, or an overlay; an unknown fact
  is treated as retained. When the C function has a `void*` context, the binder pairs it and the
  closure travels there. With no context pointer, as with plain `qsort`, the shared stub reads a
  per-thread slot that the wrapper saves and restores, so nesting and reentry work.
- **Retained callbacks.** A registration is a `Close` resource. Scope end requests stop and
  waits for the native acknowledgment, so the common case needs no join at all.
  `source.unsubscribe(^sub)` stays for the expert who wants the completion task.

**Residue.** A C API that keeps a callback forever with no way to unregister must keep the
closure alive forever. Jet makes that a visible `forever` registration, so it is not a hidden
leak. A retained callback with no context pointer takes a slot in a prebuilt stub table, because
Jet never generates code at run time; a full table is `CallbackError.TableFull`. Both are
inherent to such APIs.

## 21. Exports

**Who it hurts:** anyone making a Jet library for other languages.

**Today.**

- `#Export(c)` accepts C primitives only: the `.jetlib` header stores a scalar code per export
  (`Compiler/JetFoundation/Source/Record/JetLib.jet`).
- The projections in `Examples/ffi/projections/` pass records as JSON text inside `JetComponent`.
  In C that is `project_component((JetComponent){…"[{\"count\":7,…}]"…})`; in Python it is a
  `dict`.

**Fix (v2 §8.5, ballot D-FFI-EXPORT2, reworked).** Each binding writer projects the export table
into the host's own idiom:

- **C:** typed structs, length-and-pointer lists, an error out-parameter, function pointer plus
  context, and completion callbacks.
- **Python:** dataclasses, lists, exception classes, and `async def`.
- **Others:** the same rule in each host's idiom.

The JSON component path stays only for the `.Wasm` component output, where WIT requires
canonical-ABI values anyway.

**Residue.**

- A host with no async form gets a completion callback, which is C's only idiom.
- A host with no tagged unions, C again, gets a tag plus a union and accessor functions.
- Both are inherent to the host language.

## 22. Python

**Who it hurts:** data users.

**Today.** D-FFI-PY1=A: the default host is a sidecar worker with a typed message boundary that
copies. `py@embed` switches to in-process for zero-copy.

**Fix (v2 §8.1, folded into ballot D-FFI-ISOLATE1).**

- **Shared memory.** Arrays and buffers cross the sidecar boundary through shared memory pages,
  so a 100 MB numpy array is not copied in either placement.
- **One placement option.** Python's default placement is `.Process`. Choosing in-process is the
  same option every library has (`isolate: { numpy: .InProcess }`), so `py@embed` retires as a
  second spelling. `jet fix` rewrites it.

**Residue.** Each call into the sidecar costs a message round trip, roughly 10 to 30
microseconds. Hot loops should run in Python (one call) or use `.InProcess`. That is inherent to
the crash isolation PY1 chose for good reason.

## 23. Effects of foreign calls

**Who it hurts:** experts who want pure functions; anyone auditing authority.

**Today.** Opaque foreign calls get the maximal effect set. A `-[]>` function cannot call `cos`.

**Fix (v2 §5.8).**

- Effect facts come from header attributes (`__attribute__((const))` and `pure`) and from packs.
  libm's `cos` is pure by its pack.
- Unknown stays maximal, because that is honest.
- Under `.Wasm` placement, the effect row is enforced: the sandbox can only reach the imports
  that Jet grants, and each import carries its effect.

**Residue.** For in-process native code, an effect bound is a promise, not a wall. Native code
can make system calls directly. This is inherent; the wall is `.Wasm` or `.Process`.

---

## Beginner path and expert path, summarized

**Beginner.**

- Writes `use sql :: c.sqlite3` or `use rl :: "raylib.h"` and calls functions.
- Sees Jet errors with codes and fixes, never a foreign toolchain message (I2).
- Never writes `#Unsafe` for a library that has a header or a pack.
- Never chooses a placement unless a library is untrusted.
- Costs that remain:
  - a toolchain download once;
  - a plainer API for libraries nobody has written facts for;
  - an `FFI` authority line that `jet fix` adds.

**Expert.**

- Climbs one rung at a time:
  - `jet inspect bind` shows every row, fact and source;
  - `--shape native` gives the raw C shape with memory checks kept;
  - `--freeze` pins the plan;
  - `core.ffi` gives the same steps by hand, inside `#Unsafe`.
- Picks placements per library and reads the `ffi` lens of `jet perf`.
- Costs that remain:
  - in-process native code is trusted;
  - process placement costs microseconds per call;
  - Apple cross builds need the SDK.
