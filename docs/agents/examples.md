# Example authoring (D-EXAMPLES-SHORTPATH1=A)

Examples are executable specs (I5). When you add or rewrite a flagship:

1. **Short path first.** The named example teaches the safe default (`#CLI`,
   `para_map` / `taskgroup`, streaming readers, `ui.mount`, …).
2. **Expert beside, not instead.** Keep the long manual form in a sibling
   `*_expert.jet` with a one-line header that says it is the expert variant.
3. **Goldens match both lenses.** Prove `jet run` and `jet run --release` when
   the example is runnable; update `examples/features/expected/…`.
4. **Do not hide floors.** If a topic has a directory (`math/`, `lowlevel/`),
   put a beginner flagship there — do not leave the only proof in tests.

See `examples/README.md` for the learning-order table.
