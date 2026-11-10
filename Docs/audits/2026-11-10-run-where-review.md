# D-RUN-WHERE1 review: report kernel eligibility for any function

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-RUN-WHERE1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-RUN-WHERE1.

## Question

Should `jet explain kernel FILE:LINE` and hover report, for any function, whether the existing `#Kernel` proof
would pass and which line stops it? `#Kernel` stays the only launch point.

## Findings that changed the ballot

1. **No kernel may call any helper today.** D-GPU-HELPER-SCOPE1=A (ratified 2026-09-12, "Check the complete
   reachable helper in its kernel context") is not built.
   - k05: a `#Kernel` that calls the pure helper `triple(x) = x * 3` gets E1102, "cannot prove no reachable
     effects or opaque calls", at the marker.
   - k04, with a helper that prints, gets the same error.
   - Cause: `Compiler/JetSema/Source/Sema/Kernel.jet:156-160`. `sema_kernel_check_effects` fails when
     `call_edges.len() > 0` and reports `pending.span`, the marker.

   The board said A's report "shows that helper's lines" under D-GPU-HELPER-SCOPE1=A. Today that report
   would fail almost every real function at its first call. A now names the helper proof as build work, and
   until it lands the report lists each call as not yet proved, with its line.
2. **The effects check keeps no line.** The check takes only effect names and call-edge names
   (`Kernel.jet:156`), so it cannot point at the print. The allocation check does keep a span
   (`Kernel.jet:146-149`, which k03 shows at 3:12). A's build work therefore includes effect facts that keep
   their line.
3. **`jet explain kernel k02_print.jet:2` today** prints the generic marker topic and ignores the location,
   with no error. Under A the location gets its meaning.
4. **The story said line 2.** The print in k02 is on line 3; line 2 is the marker. Fixed.
5. **Peer notes overclaimed.**
   - "Mojo: the parallel call is … checked there" is unsourced; it now reads "an explicit library call".
   - Julia's manual says avoiding data races is the user's job, and the note now says so.
   - Rust's Send and Sync are auto traits.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result |
|---|---|
| k01 pure kernel | prints 42 |
| k02 kernel prints | E1102 at 2:1, the marker |
| k03 kernel builds a list | E1102 at 3:12, the literal |
| k04 kernel calls a printing helper | E1102 at 8:1, the marker |
| k05 kernel calls a pure helper | E1102 at 5:1, the marker |

`#Kernel` appears in 20 Jet files on dev (`git grep -l`).

## Peers (sourced)

- Rust's Send and Sync are auto traits, inferred by the compiler; Rayon's `par_iter` is the explicit launch.
  <https://doc.rust-lang.org/nomicon/send-and-sync.html>
- Julia's `Threads.@threads` marks the loop. The manual shows `@threads` exposing a data race and leaves
  avoiding races to the user. <https://docs.julialang.org/en/v1/manual/multi-threading/>
- Mojo's `parallelize` is a library call in `algorithm.functional`.
  <https://docs.modular.com/mojo/stdlib/algorithm/functional/>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| The report shows a helper's lines (D-GPU-HELPER-SCOPE1=A) | Is the helper proof built? | **Failed**: k05 shows a pure helper is refused. A now includes the helper proof as build work |
| The effects error can name the print | Does the check have the line? | **Failed today**: no span is kept (`Kernel.jet:156-160`). It is now named build work |
| Builds pay nothing | The proof runs per query | Holds: the proof runs only on request or where `#Kernel` is written |
| `jet explain kernel FILE:LINE` does not collide | Today it ignores the location | Holds: a fixed topic with an argument; no package lookup |
| Story | Line number | Fixed: line 3 |

## Remaining losses

- A plain function's label can change with its body. It is a report, not a promise.
- Until the helper proof lands, functions with calls report "not yet proved" at each call.
