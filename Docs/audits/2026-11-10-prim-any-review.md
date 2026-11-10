# D-PRIM-ANY1 review: an explicit Any box on the trait-value carrier

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-PRIM-ANY1. Earlier pass: BallotFix REPORT.md, section D-PRIM-ANY1.

## Question

Should Jet get an explicit `Any(value)` box that keeps any owned value, including files, functions and duties,
with checked reads and no hidden duties? A builds it on today's trait-value carrier.

## Findings that changed the ballot

1. **The cleanup paragraph still named a descriptor.** A's own "No descriptor" paragraph removes the type
   record, yet "Cleanup" said drops run "through the descriptor". The text now says cleanup is selected by
   the key word, which is what `Compiler/JetIR/Source/BuildTraits.jet:1-6` does today: "the last owner drops
   the payload and frees the box (ir_trait_drop_body)". The internal label "S63" is also removed.
2. **The cost of today's box is now measured, not disclaimed.** `probes/cost/trait_box.jet` and `enum_box.jet`
   build one million small `Square` and `Rect` values and sum their areas. One version uses a `[Shape]` of
   trait values, the other a closed enum matched by a function. Built with `jet build` and run 5 times each
   (output 9500005 for both):

   | Form | runs (s) | median | peak RSS |
   |---|---|---|---|
   | trait values (today's counted heap box) | 0.119 0.114 0.119 0.136 0.115 | 0.119 | 78.6 MB |
   | closed enum | 0.106 0.110 0.110 0.111 0.107 | 0.110 | 78.6 MB |

   So today's box costs about 9 ms per million values, about 9 ns each, on this shape. That bounds what A's
   inline buffer saves here. The owner asked for the allocation removed, so A keeps the buffer. The
   designAway records the measured fallback: Any on today's box, with no second layout.
   [INFERENCE] Peak memory is equal because the list's growth dominates. This was not checked further.
3. **The citations hold on dev c460ba91e5.** BuildTraits.jet is unchanged since t6c. The key is 56 bits of
   SHA-256, and a collision stops the build (`:10-27`). `close: fn(&&self) Never!` is at
   `Compiler/Prelude/Traits.jet:201`.

## Probes (a0.0.43; outputs in probes/out43)

| Probe | Result | Board claim |
|---|---|---|
| p01 `[Any]{3, "three"}` | E0112 | same |
| p02, p13 Dyn | `true`, `wrong type`; `true`, `not text`, `14` | same |
| p03 trait list | `10` | same |
| p05 closed enum | `flag true`, `size 14` | same |
| p10 `.Square(sq)` on a trait value | E0305, E0107 | same |
| p11 unknown name | E0112 | same |
| p14 copy a trait value | E0211 | same |
| c01 branch on `#Secret` | passes (no tag error) | branch tagging unbuilt, same |

## Peers (sourced)

- Rust: `Box<dyn Any>`; `downcast` returns `Err(self)` on a mismatch; `Any` needs `'static`.
  <https://doc.rust-lang.org/std/any/index.html>
- Swift: existentials use "a fixed-size buffer, which is three pointers" and inline values that fit.
  <https://github.com/swiftlang/swift/blob/main/docs/ABI/TypeLayout.rst>
- Go: `any` converts implicitly, and a type switch needs no exhaustiveness. <https://go.dev/ref/spec#Type_switches>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| No descriptor | Cleanup paragraph | **Failed**: it still named one. Fixed |
| No cost comparison claimed | Is one cheap to measure? | Measured: about 9 ns per value for today's box |
| Second layout is the one irremovable cost | Use today's box for Any | Possible, at the measured cost. Recorded as the fallback in designAway |
| Close needs no effect | Read Traits.jet | Holds |
| Branch tagging unbuilt | c01 | Holds on a0.0.43 |

## Remaining losses

- A second backend layout, unless the fallback is chosen.
- Four words per box, and side storage for large payloads.
- Four carrier forms.
- A map holding one duty must be drained on every success path.
- The secrecy rule waits on branch tagging.
