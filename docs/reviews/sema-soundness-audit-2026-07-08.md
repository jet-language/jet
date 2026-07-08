# Sema soundness audit — 2026-07-08

Card: #353. Scope: accepts-invalid and miscompile risks in `jet-sema`, with I1
and I2 as hard gates. This pass audited the highest-risk surfaces named on the
card and expanded the executable fuzz corpus that CI can run through
`tests/fuzz_sema.rs`.

## Verdict

No confirmed sema soundness hole remained after this pass. The only concrete bug
found was in the audit harness itself: `tests/fuzz_sema.rs` still generated the
retired `val` binding spelling, so part of the fuzz budget was spent on syntax
teaching instead of sema soundness. The mutator now emits ratified `::` bindings.

## Area review

- Ownership / move / borrow: reviewed the existing E2-M5 regression surface and
  kept fuzz coverage on examples that compile without `#Unsafe`. Any accepted
  safe source is checked for generated Rust containing no `unsafe`.
- Sendability / tasks / channels: no accepted-invalid candidate found in the
  current corpus. Seeds still flow through example loading, so existing task and
  channel examples stay in the rustc-agreement battery when safe.
- `#Unsafe` containment: the fuzz harness rejects generated `unsafe` for any
  mutated source that lacks a user-written `#Unsafe` gate. Seeds whose baseline
  uses vetted prelude/runtime `unsafe` are skipped to avoid false failures.
- Generics / trait resolution: added a curated generic identity seed and a
  generic mutator. Accepted programs must still lower to Rust accepted by rustc.
- Comptime / `@Pure` boundary: added a pure-function seed to keep erased purity
  markers in the I2 battery without widening the language surface.
- Newer surface: added curated fixed-list, fan-out, and D-REFINE1 refined-index
  seeds. Added mutators for fixed-list indexing, fan-out over fixed lists, and
  `#Invariant`-backed distinct indexes.

## Corpus contract

`tests/fuzz_sema.rs` now combines:

- all safe examples under `examples/features/**`;
- curated seeds for generics, fixed lists, fan-out, pure functions, and refined
  indexes;
- mutation strategies that append ratified top-level bindings/functions while
  preserving the original seed.

The runner stays short enough for #211 CI: `VARIANTS = 50`, deterministic
default seed `42`, override via `FUZZ_SEED=<u64>`. The oracle is unchanged:
accepted Jet must produce Rust accepted by rustc; rejected Jet must carry Jet
diagnostic codes only; safe Jet must not emit generated `unsafe`.

## Proof

Target gate:

```sh
nix develop -c env TMPDIR=/home/nate/Projects/Github/jet/target/codex-tmp \
  cargo test --test fuzz_sema -- --nocapture
```

Full epoch gate remains `nix develop -c cargo test` after all Epoch 3 cards
reach verify.
