# Example authoring

Examples are executable specifications under I5. When you add or rewrite a flagship example:

1. Put the short path first. Teach the safe default, such as `para_map` / `task`, streaming readers, or `ui.mount`.
2. Put expert control beside it. Keep the long manual form in a sibling `*_expert.jet` file with a one-line header that names it as the expert variant.
3. Match both lenses. For runnable examples, prove `jet run` and `jet run --release`, then update `examples/features/expected/…`.
4. Do not hide a floor. If a topic has a directory such as `math/` or `lowlevel/`, put a beginner flagship there instead of leaving the only proof in tests.

Use `examples/README.md` for the learning-order table. Example paths can be embedded in expected output, harness lists, and decision records; a move requires a complete path-consumer cutover. See `docs/spec/contributing/agent-engineering.md` for the known fixture traps.
