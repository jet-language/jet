# Output parameters vs returns: evidence for D-OUTPUT-RETURNS1 (2026-10-04)

This dated research supports the owner's question on ballot **D-OUTPUT-RETURNS1**
("Must public Jet APIs return new results, rather than fill dummy input
values?"). It does not ratify anything, propose syntax as decided, or own any
work. Every web source below was read on **2026-10-04**. The local
measurements in section 4.1 were run on the same day by this research; nothing
was run against the Jet compiler, and no Jet program was executed.

## Plain summary

1. Almost every official style source that addresses the question says the same
   thing: return new results; use a writable parameter only for real edits,
   reusable buffers, or large storage you cannot afford to copy.
2. That is the rule in the C++ Core Guidelines (F.20/F.21), the Google C++ guide,
   Abseil, Rust's API guidelines (C-NO-OUT), Effective Go, .NET (CA1021), and
   the Python FAQ. No surveyed source recommends output-only parameters as the
   default.
3. The same sources keep the same exceptions: reusing a buffer's capacity
   (`read(&mut buf)`, `io.Reader`), in-place numerics (BLAS, NumPy `out=`,
   Julia `mul!`), and C/foreign boundaries.
4. Measured here: small multi-value results cost the same either way (about
   1.2–1.6 ns per call in Rust and C++). Allocating a fresh 4 KiB `Vec` on every
   call was 11–18% slower than reusing one, and taking the buffer then
   returning it (`^T -> T`) matched reuse.
5. One case matters for Jet: rustc 1.97.1 did not build a returned 4 KiB array
   directly in the caller's slot. It cost 116–128 ns, against 51 ns for an
   output parameter. Clang built the C++ version in place at 50–51 ns.
6. That cost comes from code generation, not from the API shape. Jet already
   lowers some `String` returns to compiler-supplied destinations
   (D-EFF-FORMAT-WRITER1). Doing the same for large returned values would keep
   returns in the source **[INFERENCE: not checked against Jet codegen]**.
7. Bug evidence is about C, where an output pointer can be left uninitialized.
   Jet's `&T` already requires an initialized value, so most of that bug class
   does not carry over. What carries over is a parameter's role being unclear
   from its signature, plus the boilerplate of building a dummy value first.
8. Jet's required call-site `&` already fixes the old C++ complaint that changes
   are invisible at the call. What `&` cannot say is whether an argument is
   edited or only filled.
9. The evidence supports the direction of option A. A's text already lists most
   of the standard exceptions. The large-fixed-result case is left to "existing
   mechanisms" and has a cost that has not been measured in Jet.
10. Section 7 lists the ballot wording most likely to confuse, and suggests how
   A could be reworded without adding syntax.

## 1. What the ballot decides, and the Jet facts it rests on

- Jet parameters are read `T` (the default), write `&T`, or take `^T`. The
  call site must repeat the `&` or `^` marker (`Docs/spec/spec.md:895-920`).
- The ballot (`decision show D-OUTPUT-RETURNS1`, read 2026-10-04) asks only
  about **exported, output-only** `&` parameters: storage whose previous value
  plays no part in the operation. Under both options, edits, appends,
  stateful handles, streams, caller-chosen capacity, no-heap storage and
  foreign signatures stay as they are. Neither option adds syntax or a compiler
  check. A rejects the shape in API review; B allows it with documentation.
- In option B's example, the `&Parts` argument must be initialized before the
  call, so the uninitialized-pointer bugs familiar from C do not apply. The
  remaining cost is the dummy `Parts{quotient: 0, remainder: 0}` and the
  unclear role of the argument.
- A grep of `Core/**/*.jet` on 2026-10-04 for parameters named
  `out`/`dst`/`dest`/`output`/`into` with `&` found only private helpers that
  **append** into an existing `&[U8]` or `&[String]` (for example
  `write_json(out: &[U8], …)` in `Core/encoding/json.jet:953`). These are
  accumulation, which both options allow. This is a naming sample, not the
  inventory the ballot calls for.

## 2. Language conventions and official guidance

| Language | Official position (source, read 2026-10-04) | Mechanism for multiple results | Out-style escape hatch |
|---|---|---|---|
| C++ | F.20 says to prefer returns: "A return value is self-documenting, whereas an `&` could be either in-out or out-only and is liable to be misused." F.21 says to return a struct for several outputs. F.20 makes exceptions for expensive-to-move types and for capacity reuse, which it calls "in/out". The enforcement notes propose flagging non-const `&` parameters that are never written (F.17), or that are "not read before being written to" and could be cheaply returned (F.20). F.15: "Use the advanced techniques only after demonstrating need". [CppCoreGuidelines](https://isocpp.github.io/CppCoreGuidelines/CppCoreGuidelines#Rf-out) | struct, structured bindings | `T&` |
| C++ (Google) | Current text: "Prefer using return values over output parameters: they improve readability, and often provide the same or better performance." [cppguide](https://google.github.io/styleguide/cppguide.html#Inputs_and_Outputs). Before May 2020 the guide required output arguments to be **pointers** so the call site showed `&x`, and banned non-const reference parameters ([commit 967e157a00, 2019-09-05](https://raw.githubusercontent.com/google/styleguide/967e157a00/cppguide.html)). Commit [7a7a2f510e, 2020-05-19](https://raw.githubusercontent.com/google/styleguide/7a7a2f510e/cppguide.html) switched to references. | struct | `T&` / `T*` |
| C++ (Abseil) | TotW #176, 2020: "Prefer return values to output parameters". It lists five questions an output parameter leaves open (out or in/out, what happens to old data, null, lifetime, state on failure): "One cannot answer any of these questions from the function signature alone". The caveats cover in/out, caller-chosen arenas, and allocation inside loops. [abseil.io/tips/176](https://abseil.io/tips/176) | `std::optional<struct>` | — |
| C++ (cppfront) | Herb Sutter's experimental Cpp2 has explicit `in`/`inout`/`out`/`copy`/`move`/`forward` parameters, plus named multiple returns. `out` "must `=` assign/construct before other uses". [cppfront functions](https://hsutter.github.io/cppfront/cpp2/functions/) | named return lists | `out` keyword |
| Rust | C-NO-OUT: "Functions do not take out-parameters … The primary exception: sometimes a function is meant to modify data that the caller already owns, for example to re-use a buffer". [api-guidelines/predictability](https://rust-lang.github.io/api-guidelines/predictability.html#c-no-out) | tuples, structs | `&mut [u8]` buffers (`Read::read`) |
| Go | Effective Go: multiple returns "improve on a couple of clumsy idioms in C programs: in-band error returns … and modifying an argument passed by address." [effective_go](https://go.dev/doc/effective_go#multiple-returns) | multiple returns | `Read(p []byte) (n int, err error)`. The reader "may use all of p as scratch space" and "must not retain p". [io.go](https://go.dev/src/io/io.go) |
| C# / .NET | CA1021 "Avoid out parameters" applies to public members and is off by default in .NET 10: "Library architects who design for a general audience should not expect users to become proficient in working with `out` or `ref` parameters." [CA1021](https://learn.microsoft.com/en-us/dotnet/fundamentals/code-analysis/quality-rules/ca1021). In the other direction, the Framework Design Guidelines *recommend* the Try-Parse pattern (`bool TryParse(string, out T)`) for "extremely performance-sensitive APIs". [exceptions-and-performance](https://github.com/dotnet/docs/blob/main/docs/standard/design-guidelines/exceptions-and-performance.md). The compiler requires an `out` parameter to be assigned before return. [method-parameters](https://github.com/dotnet/docs/blob/main/docs/csharp/language-reference/keywords/method-parameters.md) | value tuples; `Deconstruct` uses `out` | `out`, `ref` |
| Swift | `inout` is "copy-in copy-out", with `&` at the call site. [Declarations](https://github.com/swiftlang/swift-book/blob/main/TSPL.docc/ReferenceManual/Declarations.md). The API guidelines name mutating/nonmutating pairs `x.sort()` / `x.sorted()` and `y.formUnion(z)` / `y.union(z)`. [api-design-guidelines](https://www.swift.org/documentation/api-design-guidelines/) | tuples | `inout` (no out-only form) |
| Zig | "Result Location Semantics" is part of the language spec: each expression may carry "where the resulting value should be placed in memory". [langref](https://ziglang.org/documentation/master/#Result-Location-Semantics). The standard library also takes caller buffers: `bufPrint(buf: []u8, …) ![]u8` returns the filled slice. [fmt.zig:597](https://raw.githubusercontent.com/ziglang/zig/master/lib/std/fmt.zig) | multiple values via structs | slices, pointers |
| Odin | "A procedure in Odin can return any number of results", and results can be named. [overview](https://odin-lang.org/docs/overview/#multiple-results) | multiple results | pointers |
| Jai | **Not researched.** No official public language reference was located in this pass. | — | — |
| C / POSIX | No official preference found. Out pointers are the common shape in POSIX and kernel interfaces; see section 3 for the bug evidence. | struct return (available) | pointers |
| Java / Kotlin | No out parameters. Kotlin docs: to return "a result object and a status … declare a data class and return its instance". [destructuring-declarations](https://github.com/JetBrains/kotlin-web-site/blob/master/docs/topics/destructuring-declarations.md) | objects, `Pair`, data classes | mutable objects |
| Python | FAQ "How do I write a function with output parameters": "Your best choice is to return a tuple containing the multiple results." [programming FAQ](https://github.com/python/cpython/blob/main/Doc/faq/programming.rst) | tuples | mutable objects |
| JS / TS | **No official guidance found.** Arguments are passed by value; returning an object or array and destructuring it is the common idiom (see the Stack Overflow numbers in 4.4). | object/array destructuring | mutable objects |
| Fortran | `intent(in)`, `intent(out)`, `intent(inout)`; "It is good programming practice for functions not to modify their arguments". [fortran-lang](https://fortran-lang.org/learn/quickstart/organising_code/) | subroutine arguments | `intent(out)` |
| Julia | "Append `!` to names of functions that modify their arguments". Base pairs `sort`/`sort!`. [style guide](https://github.com/JuliaLang/julia/blob/master/doc/src/manual/style-guide.md). `mul!(Y, A, B)` overwrites Y, and "Y must not be aliased with either A or B". [LinearAlgebra](https://docs.julialang.org/en/v1/stdlib/LinearAlgebra/) | tuples | `!` functions |
| Ada | Modes `in`, `out`, `in out`. "Out parameters behave a bit like return values for functions", and GNAT warns on simple misuse. [learn.adacore.com](https://learn.adacore.com/courses/intro-to-ada/chapters/subprograms.html) | records | `out` |
| Pascal | `var` parameters. **No primary source read**; the evidence is only a Delphi question on Stack Overflow ([14507310](https://stackoverflow.com/q/14507310), score 61). | records | `var`, `out` (Delphi) |

## 3. Domains

- **Games and engines.** Unreal's coding standard prefixes writable reference
  parameters with `Out` ("This makes it obvious that the value passed in this
  argument is replaced by the function"). It solves the problem with a naming
  convention, not by banning the pattern
  ([Epic C++ Coding Standard](https://dev.epicgames.com/documentation/en-us/unreal-engine/epic-cplusplus-coding-standard-for-unreal-engine)).
  John Carmack (2012) argues for returning values in pure functions, but warns
  that "relying on compilers to always perform return value optimization can be
  hazardous to performance, so passing reference parameter for output of
  complex data structures is often justifiable". He also says a pure
  `DrawTriangle()` that returns a new framebuffer is the wrong choice ("Don't
  do that")
  ([Game Developer reprint](https://www.gamedeveloper.com/programming/in-depth-functional-programming-in-c-)).
  Casey Muratori / Handmade Hero and Mike Acton's data-oriented talks were
  **not accessed** (video only, no transcript read); nothing is claimed for
  them.
- **HPC and numerics.** BLAS `DGEMM` overwrites `C` in place, and "when BETA
  is supplied as zero then C need not be set on entry". The same argument is
  in/out or output-only depending on a value
  ([netlib dgemm](https://netlib.org/lapack/explore-html/d7/d2b/dgemm_8f_source.html)).
  NumPy ufuncs accept `out=`, return the output anyway, and handle overlapping
  memory with temporary copies. The docs advise: "The optional output arguments
  can be used to help you save memory for large calculations"
  ([ufuncs](https://numpy.org/doc/stable/reference/ufuncs.html)). Julia teaches
  "Pre-allocate outputs" with measured numbers (section 4.2).
- **Embedded and no-heap.** Caller-provided buffers are the norm. Microsoft's
  SAL annotations `_Out_` and `_Inout_` exist "to help identify possible
  uninitialized values and invalid null pointer uses"
  ([understanding-sal](https://github.com/MicrosoftDocs/cpp-docs/blob/main/docs/code-quality/understanding-sal.md)).
  The MISRA C text is paywalled and was **not read**.
- **Systems and kernels.** The Linux kernel style returns error codes from
  actions ("the function should return an error-code integer") and returns
  computed pointers with `ERR_PTR`. Results otherwise travel through pointer
  arguments
  ([coding-style.rst §16](https://raw.githubusercontent.com/torvalds/linux/master/Documentation/process/coding-style.rst)).
  glibc deprecated `readdir_r()` (since 2.24) partly because "the interface
  does not allow the caller to specify the length of the buffer"
  ([readdir_r(3)](https://man7.org/linux/man-pages/man3/readdir_r.3.html)).
- **Web, backend and data science.** These are mostly Java, Kotlin, Python
  and JS/TS, which have no out parameters; results come back as objects. C#
  backends have `out`, but CA1021 discourages it. NumPy `out=` is the main
  expert exception in data science. The
  heavy "output parameter" traffic on Stack Overflow is SQL stored procedures
  (4.4), a different meaning of the term.
- **Beginner teaching.** The Rust Book's chapter 2 guessing game introduces
  `read_line(&mut guess)` on day one, explained as "append that into a string
  (without overwriting its contents)"
  ([ch02](https://github.com/rust-lang/book/blob/main/src/ch02-00-guessing-game-tutorial.md)).
  Even a return-first language puts an append-style `&mut` in front of
  beginners. CA1021's stated reason is that `out`/`ref` "requires intermediate
  design and coding skills".

## 4. Numbers

### 4.1 Local measurement (this research, 2026-10-04)

Setup: x86-64 host running Linux 7.0.11-cachyos with 32 logical CPUs. Load
average was about 10–12 from unrelated jobs, so every run was pinned with
`taskset -c 31`. Toolchains came from `Tools/agent/jet-env`: rustc 1.97.1
(`-C opt-level=3 -C codegen-units=1`) and clang 21.1.8 (`-O2 -std=c++20`).
Each case ran 1,000,000 calls through a `#[inline(never)]`/`noinline`
function, after one warm-up pass. There were five runs per binary and the
tables give the range. A counting global allocator, or `operator new` in C++,
recorded heap allocations in the timed pass. The scratch sources lived in
`/tmp/outparam/`, were deleted after the run, and are not part of the
repository; the measured functions are reproduced in the appendix.

| Case (Rust) | ns/call (5 runs) | Heap allocs per 1M calls |
|---|---|---|
| `split_ret(n,d) -> Parts` (two `i64`) | 1.20–1.31 | 0 |
| `split_out(n,d,&mut Parts)` | 1.38–1.55 | 0 |
| `big_ret(seed) -> [u8; 4096]` (zero, fill, return) | 115.95–127.94 | 0 |
| `big_ret_from_fn` (`core::array::from_fn`) | 89.66–91.68 | 0 |
| `big_out(seed, &mut [u8; 4096])` | 50.81–51.86 | 0 |
| `vec_ret(seed) -> Vec<u8>` (fresh 4 KiB) | 57.52–60.75 | 1,000,000 |
| `vec_fill(seed, &mut Vec<u8>)` (clear + refill) | 50.60–52.31 | 1 |
| `vec_take_return(seed, Vec<u8>) -> Vec<u8>` (Jet `^T -> T` shape) | 51.26–52.08 | 1 |

| Case (C++) | ns/call (5 runs) | Heap allocs per 1M calls |
|---|---|---|
| `split_ret -> Parts` | 1.27–1.30 | 0 |
| `split_out(Parts&)` | 1.18–1.23 | 0 |
| `big_ret -> std::array<uint8_t,4096>` | 50.26–51.23 | 0 |
| `big_out(std::array&)` | 50.83–52.05 | 0 |
| `vec_ret -> std::vector` (fresh) | 2449.94–2472.12 | 1,000,000 |
| `vec_fill(std::vector&)` | 2435.59–2640.72 | 13 |

Assembly inspection (`--emit=asm` / `-S`) for the same functions:

- Rust `split_ret` returns both fields in registers and stores nothing.
  `split_out` writes two words through the pointer. Either way the cost is
  dominated by the division.
- Rust `big_ret` calls `memset` for 4096 bytes, fills a stack temporary, then
  `memcpy`s 4096 bytes into the caller's slot. `big_ret_from_fn` skips the
  `memset` but keeps the `memcpy`. Clang's `big_ret` contains neither and
  writes straight into the caller's slot (NRVO).
- A follow-up Rust check returned 64-byte and 512-byte structs filled by a
  fixed loop. Both were written directly into the caller's slot with no
  `memcpy`/`memset`. With these patterns the copy therefore appeared somewhere
  between 512 B and 4 KiB; the exact threshold was not bisected.
- The C++ `vector` fill loop (`push_back`) did not vectorize. Element writes
  dominate it, so the fresh-vs-reuse difference cannot be measured at this
  size: that comparison is **inconclusive**.

Reading these numbers: for small multi-value results, the shape makes no
practical difference. For heap buffers in a hot loop, reuse saves one
allocation per call; per-run ratios were 11–18% here. The system allocator
probably recycles a freed 4 KiB block cheaply **[INFERENCE]**, so other
allocators or sizes may show a larger gap; that was not measured. Taking the
buffer and returning it matches `&mut` reuse,
which supports the expert path the ballot describes. Large fixed-size returns
through rustc can cost more than twice as much as an output parameter. Rust
has long-standing open issues on this:
[#116541 "Missed optimization: RVO isn't applied"](https://github.com/rust-lang/rust/issues/116541)
(opened 2023-10-08),
[#62446 unnecessary copy of returned arrays](https://github.com/rust-lang/rust/issues/62446)
(2019-07-06), and
[#53827 stack overflow with `Box::new([..; 3000000])`](https://github.com/rust-lang/rust/issues/53827)
(2018-08-30). All three were open on 2026-10-04 according to the GitHub API.

### 4.2 Published performance numbers

- **C++17** makes copy elision mandatory for returning a prvalue of the same
  type. NRVO for a named local is still permitted but not required.
  ([cppreference copy elision](https://en.cppreference.com/w/cpp/language/copy_elision.html)).
  Abseil TotW #11 says "all modern C++ compilers perform RVO by default, even
  in debug builds" ([tips/11](https://abseil.io/tips/11)); this research did
  not verify that claim across compilers.
- **Julia** "Pre-allocate outputs" example: returning a new 3000-element array
  on each of 10^5 iterations took 0.297454 s with 200.00 k allocations
  (2.239 GiB) and 39.80% GC time. The `xinc!` version that writes into a
  preallocated array took 0.009410 s with 2 allocations (23.477 KiB), about 32×
  faster ([performance tips](https://docs.julialang.org/en/v1/manual/performance-tips/#Pre-allocate-outputs)).
  The same page adds that "pre-allocation can make your code uglier, so
  performance measurements and some judgment may be required."
- **Rust `read_buf` RFC 2930** explains why caller-buffer reads matter, quoting
  others' measurements without republishing them: the standard library's
  uninitialized buffers gave "a 7% improvement in benchmarks" (2015); Quinn
  measured "0.2%-2.45%"; and a 2015 `read_to_end` buffer-sizing fix "improved
  the performance of small reads by over 4,000x"
  ([RFC 2930](https://rust-lang.github.io/rfcs/2930-read-buf.html)). These
  numbers are about avoiding zero-initialization of caller buffers, not about
  returns versus outputs.
- **NumPy** documents `out=` as a memory saver (`G = A * B; add(G, C, G)`)
  but gives no number. No NumPy benchmark was run (constraint: no Python).

### 4.3 Bug and readability evidence

- **No controlled study** comparing the readability or defect rate of out
  parameters against returns was found. The evidence below is rule rationale
  and bug-class data, mostly from C.
- **Uninitialized-output bug class (C/kernel).** UniSan (CCS 2016) found that
  about 60% of Linux kernel information leaks reported after 2013 were
  uninitialized reads (figure: 57.3%). It cites an earlier survey in which 28
  of 37 leaks from January 2010 to March 2011 had the same cause, and it
  reports 19 new leaks confirmed by Linux and Google
  ([UniSan PDF](https://www-users.cse.umn.edu/~kjlu/papers/unisan.pdf)). These
  leaks typically copy a partly filled struct out to user space
  **[INFERENCE from the paper's framing; individual CVEs not checked]**.
  CERT EXP33-C gives the textbook out-pointer form: `set_flag(number, &sign)`
  never writes `sign` when `number == 0`
  ([EXP33-C](https://cmu-sei.github.io/secure-coding-standards/sei-cert-c-coding-standard/rules/expressions-exp/exp33-c/)).
  Rust RFC 2930 shows a misbehaving reader that, combined with uninitialized
  memory, compiles to a bare `ret` returning garbage, which breaks a
  `NonZeroU32` invariant.
- **How much transfers to Jet.** Jet's `&T` needs an initialized argument and
  obeys exclusivity (ballot option B technical text; `spec.md:895-912`). The
  "never written" and "uninitialized read" classes therefore become "stale or
  default value read". That is a logic bug, not undefined behavior. The
  questions that do transfer are Abseil's: in or in/out? old contents kept or
  replaced? what remains on failure? Option B answers them with required
  documentation; option A answers them with the type.
- **Aliasing.** Out parameters create alias rules that returns do not have.
  Julia: "Y must not be aliased with either A or B". NumPy pays for temporary
  copies when operands overlap. Abseil notes that returns can be faster
  because "the optimizer doesn't have to worry about aliasing". Jet's
  exclusivity rule rejects aliasing for `&`, so this cost moves to the
  compiler rather than the user.
- **Tool cost.** Microsoft SAL (`_Out_`, `_Inout_`), C# definite assignment
  for `out`, Ada/GNAT warnings and cppfront's `out` all exist to recover
  information that a plain mutable reference does not carry.

### 4.4 Usage counts

GitHub code search. These are the raw `total_count` values from
`gh api search/code` on 2026-10-04: substring matches over files, forks
included, not deduplicated. Treat them as orders of magnitude.

| Ecosystem | Caller-buffer / in-place form | Returning form | Caller-buffer share |
|---|---|---|---|
| Rust | `"read_to_string(&mut"` 215,552 | `"fs::read_to_string("` 1,107,968 | 16% |
| Rust | `"read_to_end(&mut"` 190,976 | `"fs::read("` 529,408 | 27% |
| Go | `"io.ReadFull("` 180,736 | `"io.ReadAll("` 1,054,720 | 15% |
| C# | `"int.TryParse("` (out) 954,368 | `"int.Parse("` 1,343,488 | 42% |
| Julia | `"sort!("` 32,192 | `"sort("` 44,160 | 42% |
| Julia | `"mul!("` 12,672 | — | — |

Where a returning convenience exists, it is used 3–6× more often for
whole-result I/O. The output and in-place forms are still a large minority
(15–42%) everywhere, so every ecosystem keeps both. C#'s `TryParse` share
reflects failure handling as much as performance.

Stack Overflow, via api.stackexchange.com on 2026-10-04 (score / views /
asked):

- C#: "Which is better, return value or out parameter?" scored 179 with
  161,211 views (2009, [810797](https://stackoverflow.com/q/810797)). The
  accepted answer by Jon Skeet (score 166) says: "Return values are almost
  always the right choice when the method doesn't have anything else to
  return."
- C#: "When should I use out parameters?" scored 63 with 42,260 views (2009,
  [1169786](https://stackoverflow.com/q/1169786)). "Difference between ref and
  out parameters in .NET" scored 412 with 273,504 views (2008,
  [135234](https://stackoverflow.com/q/135234)).
- C++: "Efficient way to return a std::vector in c++" scored 200 with 376,907
  views (2013, [15704565](https://stackoverflow.com/q/15704565)). The accepted
  answer (score 265) reads "In C++11, this is the preferred way: …return by
  value". The pre-C++11 out-parameter answer scored 5.
- Demand for multiple returns: "Return multiple values in JavaScript?" scored
  1244 with 1,315,163 views ([2917175](https://stackoverflow.com/q/2917175)).
  C# "Return multiple values to a method caller" scored 702 with 1,347,026
  views ([748062](https://stackoverflow.com/q/748062)).
- Newcomers looking for out parameters in languages without them: Java scored
  66 with 153,523 views ([2824910](https://stackoverflow.com/q/2824910)).
  Python scored 64 with 110,935 views
  ([4702249](https://stackoverflow.com/q/4702249)).
- The highest-voted "output parameter" titles are SQL stored-procedure
  questions (for example [1589466](https://stackoverflow.com/q/1589466),
  score 258, 992,319 views). That is a different feature with the same name.

A count of out-style signatures versus return signatures in Rust `std` was
**not obtained**: the local toolchain has no `rust-src`, and a full checkout was
not attempted.

## 5. Community preference by domain and level

| Domain / level | Prevailing preference | Representative evidence |
|---|---|---|
| Beginners (all languages) | Return; often unaware out parameters exist | Python FAQ ("return a tuple"); CA1021 ("requires intermediate … skills"); Java/Python "how do I…" questions (4.4) |
| Application / backend / web | Return, with objects or tuples | Kotlin data-class guidance; JS destructuring volume; Jon Skeet's answer |
| Library designers (C++, Rust, Go, .NET) | Return by rule; out parameters only for buffer reuse or failure-plus-value | F.20/F.21; Google; Abseil #176; C-NO-OUT; Effective Go; Try-Parse |
| Games / engines | Return for value logic; reference output for big structures; mark it by name | Carmack 2012; Unreal `Out` prefix |
| HPC / numerics / data science | Return by default; in-place / `out=` for large arrays, flagged by name or keyword | NumPy `out=`; Julia `!` with a 32× measured win; BLAS |
| Embedded / kernels / C | Caller buffers and status returns, with annotations or checkers | Kernel style; SAL; CERT EXP33-C; UniSan |
| Experts in performance work | Measure first; out or in-place where allocation or copying is proven | F.15 ("only after demonstrating need"); Julia "measurements and some judgment"; Abseil "only cater to performance if you have evidence" |

Across all levels, no surveyed community prefers output-only parameters for
ordinary new results. They differ only in how wide the exception is and in
how the exception is marked: by keyword (C#, Ada, Fortran, cppfront), by name
(Julia `!`, Unreal `Out`, Swift `form…`), by convention (Rust, Go), or not at
all (C, plain C++ `T&`).

## 6. Options and hybrids: pros, cons, tradeoffs

"New Jet syntax?" means the design would need its own owner ballot. Nothing
here proposes syntax as decided.

| Shape | Used by | Pros | Cons | Jet beginner path | Jet expert path | New Jet syntax? |
|---|---|---|---|---|---|---|
| **A: returns required for new results (public APIs)** | C++ CG F.20, Google, Abseil, Rust C-NO-OUT, .NET CA1021 (all as guidance, not compiler bans) | One shape for answers; no dummy values; composes in expressions; failure cannot leave a half-filled result; matches nearly all modern guidance | Needs migration where output-only public APIs exist; large fixed results may pay a copy unless the backend lowers returns to destinations (4.1); "output-only" against "capacity reuse" needs judgment in review | Learns `->` and struct/tuple results only; `&` always means "this changes something you already have" | Keeps `&` for edits, appends, state and streams; `^T -> T` for buffer reuse (measured at parity in Rust); fixed storage per D-FREESTAND-ALLOCAPI1 | No |
| **B: returns taught, output-only `&` allowed** | Plain C++, C, Unreal (with naming) | Authors can expose caller storage directly; easier faithful ports of C-style APIs | `&` means two things; dummy value boilerplate; must document each argument's role and failure state (B adds this requirement); the "same `&`, different meaning" problem F.20 names | Must learn that `&` can mean "fill in" as well as "change" | Direct control; no reliance on backend copy elision | No |
| **Try-pattern** (`bool TryX(…, out T)`) | .NET | Avoids exceptions; widely used (42% share for `int.Parse` vs `TryParse`) | Exists because C# lacked sum-type results; Jet already returns `Result`/optional values | Not needed: return `T?` or `T E!` | Not needed | No (and redundant) |
| **Buffer-reuse API** (`read(&mut buf) -> n`) | Rust `Read`, Go `io.Reader`, Zig `bufPrint` | Zero per-call allocation; caller controls capacity and lifetime | Partial-fill and error-state rules must be documented (Go's `io.Reader` comment covers several cases) | Use the returning convenience (`fs::read_to_string`-style) | Use the buffer form; allowed by both A and B as capacity reuse | No |
| **Take-and-return** (`fn f(…, buf: ^Buf) -> Buf`) | Rust (by-value `Vec` in/out), Abseil #176 caveat | Keeps "results are returns"; reuses capacity; no dummy; measured equal to `&mut` reuse (4.1) | The caller must rebind the result; slightly more ceremony than `&` | Rarely needed | The ballot's suggested expert path | No (uses existing `^`) |
| **Mutating/non-mutating naming pairs** (`sort`/`sorted`, `sort`/`sort!`, `formUnion`) | Swift, Julia | Clear at the call; beginners use the returning name | Two functions per operation (A's text discourages "return/output pairs"); naming discipline needed | Uses the returning name | Uses the in-place name | Plain names: no. A `!`-suffix: **[INFERENCE]** likely yes, since `!` already appears in Jet error rows (`Never!`) |
| **Optional destination** (`out=` keyword) | NumPy ufuncs | One function; default allocates; expert passes storage; still returns the result | Needs optional write-access parameters; overlap rules | Ignores the parameter | Passes storage | **[INFERENCE]** likely yes: optional or defaulted `&` parameters (no default-parameter rule found in `spec.md`) |
| **Dedicated `out` marker** | C# `out`, Ada `out`, Fortran `intent(out)`, cppfront `out`, SAL `_Out_` | Role visible in the signature; compiler can check "assigned before return" and allow an uninitialized argument | Third/fourth access mode to learn; still not composable in expressions | One more marker to learn | Precise contracts | **Yes** |
| **Naming convention only** (`Out` prefix) | Unreal | Zero language cost | Unchecked; drifts | Reads the name | Reads the name | No |
| **Compiler-lowered destination** (return in source, caller slot in machine code) | C++17 mandatory elision, Zig result locations, Jet D-EFF-FORMAT-WRITER1 for `String` | Source keeps returns; machine code matches out-parameter performance | Backend work; guarantees need a closed rule and tests (as D-EFF-FORMAT-WRITER1 did) | Invisible | Removes the main performance argument for B | No syntax; possibly its own ballot if made a guarantee |

## 7. What this means for D-OUTPUT-RETURNS1

**Direction.** The evidence supports option A's direction. No surveyed
official guide or style rule favors output-only parameters for new results.
The owner's objection to the C++ habit matches the C++ community's own current
guidance. B's main benefit, direct caller storage, is already kept by A
through capacity reuse, stateful `&`, `^T -> T` and fixed storage. B
additionally gives `&` two meanings.

**The one real tradeoff.** Large fixed-size results. On the Rust backend,
returning a 4 KiB array was 2.3–2.5× slower than an output parameter in this
measurement; clang avoided the copy. Whether Jet's codegen copies in this case
was **not measured**. Option A's wording says such results use "existing
ownership and storage mechanisms" without saying which. Either the backend
should guarantee destination-passing for large returns (the
D-EFF-FORMAT-WRITER1 approach, extended), or A's exception list should name
large fixed results explicitly, as C++ Core Guidelines F.20 does for
"expensive to move" types.

**Suggested rewording of A (no new syntax).** "Public Jet APIs return newly
computed results (one struct or tuple for several). A writable `&` parameter
is accepted only when the operation edits, appends to, or reuses state or
capacity the caller already owns, or for raw foreign signatures. Reuse that
replaces contents must say so in the contract. Large fixed-size results may
use caller storage until the compiler guarantees return-in-place." This is the
C++ F.20 rule plus its exceptions, phrased for Jet.

**Wording in the current ballot that may confuse the owner:**

1. **Many names for one idea.** "Output-only parameter", "dummy input value",
   "blank output argument", "fake answer", "caller-supplied storage",
   "internal destination", "hidden caller destination storage" and "machine
   calling convention" all appear. Only the first is defined.
2. **"Public API review" is the only enforcement**, and the ballot does not
   say what that review is. An owner could read A as a compiler ban; the
   check instructions themselves have to ask the owner to "confirm … not a
   compiler ban".
3. **The A/B difference is narrow but reads broad.** Both options keep the
   same edits, streams, capacity, no-heap storage and foreign rules. The only
   difference is exported output-only `&`. The title ("New results come back
   as returns") does not show how narrow that is.
4. **The capacity boundary is the crux and is buried.** "A buffer's capacity
   can be a real input even when its previous bytes are not" and "A function
   named fill is not automatically exempt" pull in opposite directions. The
   ballot never settles whether clear-then-refill of a reused buffer is
   accepted, and that is exactly where the performance argument lives.
5. **B carries a hidden new obligation.** "Teach returns; let authors choose"
   also *requires* documenting every mutable argument's role and failure
   state. That cost is not in B's name or gist.
6. **The performance stance is spread out.** "This ballot adds no allocation
   guarantee", "any performance claim still needs its own exercised proof" and
   "Ordinary returns may use hidden caller destination storage" are all true.
   They do not say that large returns can cost more today (4.1).
7. **The "C++ convention" framing.** The owner's note says the choice "broke
   convention in c++". The C++ language allows both shapes and does not pick
   one. Its main written guidance (Core Guidelines, Google since 2020,
   Abseil) says: return new results, and keep references for in/out and
   reuse. Option A turns that guidance into Jet API policy; option B matches
   what the C++ language permits. Neither departs from current C++ practice.
   Jet's mandatory call-site `&` already fixes what pre-2020 Google tried to
   fix with pointer outputs.
8. **Heuristic checking.** The ballot rejects inferring output-only intent
   from read-before-write. C++ Core Guidelines F.20 proposes exactly that
   heuristic as an advisory lint, not a rejection. Leaving it out is a scope
   choice the ballot makes, not a gap the evidence reveals.

## Sources not accessed or not verified

- Jai language documentation (not researched; no official public reference
  located).
- Casey Muratori / Handmade Hero and Mike Acton data-oriented design talks
  (video; not reviewed).
- MISRA C:2012/2023 rule text (paywalled).
- Pascal/Delphi primary reference for `var`/`out` (only a Stack Overflow
  question was seen).
- JS/TS official guidance on out-style patterns (none found; MDN not read).
- ACM DL page for UniSan (HTTP 403); the authors' PDF was read instead.
- Wayback Machine snapshots of the Google style guide (HTTP 429); the
  `google/styleguide` git history was used instead.
- Semantic Scholar API (HTTP 429).
- Rust `std` signature counts (no local `rust-src`).
- NumPy `out=` timing (not run; no Python allowed).
- Individual kernel CVEs as out-parameter infoleaks: CVE-2018-11508's NVD text
  ("obtain sensitive information from kernel memory via adjtimex", CWE-200)
  was read, but its root cause was not confirmed from the fix commit.
- Abseil's claim that all modern compilers perform RVO even in debug builds.

## Appendix: measured functions

Rust (scratch `bench.rs`, deleted after the run), with timing loops as
described in 4.1:

```rust
pub struct Parts { pub q: i64, pub r: i64 }
#[inline(never)] pub fn split_ret(n: i64, d: i64) -> Parts { Parts { q: n / d, r: n % d } }
#[inline(never)] pub fn split_out(n: i64, d: i64, out: &mut Parts) { *out = Parts { q: n / d, r: n % d }; }
#[inline(never)] pub fn big_ret(seed: u8) -> [u8; 4096] {
    let mut a = [0u8; 4096];
    for (i, b) in a.iter_mut().enumerate() { *b = seed.wrapping_add(i as u8); }
    a
}
#[inline(never)] pub fn big_ret_from_fn(seed: u8) -> [u8; 4096] { core::array::from_fn(|i| seed.wrapping_add(i as u8)) }
#[inline(never)] pub fn big_out(seed: u8, out: &mut [u8; 4096]) {
    for (i, b) in out.iter_mut().enumerate() { *b = seed.wrapping_add(i as u8); }
}
#[inline(never)] pub fn vec_ret(seed: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(4096);
    v.extend((0..4096).map(|i| seed.wrapping_add(i as u8)));
    v
}
#[inline(never)] pub fn vec_fill(seed: u8, out: &mut Vec<u8>) {
    out.clear();
    out.extend((0..4096).map(|i| seed.wrapping_add(i as u8)));
}
#[inline(never)] pub fn vec_take_return(seed: u8, mut buf: Vec<u8>) -> Vec<u8> {
    buf.clear();
    buf.extend((0..4096).map(|i| seed.wrapping_add(i as u8)));
    buf
}
```

The C++ counterparts (scratch `bench.cpp`) use the same bodies with
`std::array<uint8_t,4096>` and a `std::vector<uint8_t>` filled by
`reserve` + `push_back`. Commands:
`Tools/agent/jet-env rustc -C opt-level=3 -C codegen-units=1 --edition 2021 bench.rs`
and `Tools/agent/jet-env clang++ -O2 -std=c++20 bench.cpp`, each run with
`taskset -c 31`.
