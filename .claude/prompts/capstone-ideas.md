We are ready to design Jet’s post-Epoch-3 capstone stress tests.

  Goal: propose two serious, personally useful, all-in-one projects that pressure-test Jet
  as a language, standard library, toolchain, package system, and deployment story.

  Do not scope to Jet’s current implementation limits. Assume Epoch 3 is complete. Design
  for what Jet must prove long-term.

  Project A: `jetplay`
  A Jet-built 2D game engine and editor.

  Project B: `jetlab`
  A Jet-built local-first AI workbench for documents, code, agents, evals, datasets, model
  adapters, and repeatable workflows.

  For each project, produce:

  1. Product vision
  2. MVP that is still real and useful
  3. Full capstone version
  4. Required Jet language features
  5. Required standard-library APIs
  6. Required package/dependency capabilities
  7. Required tooling: LSP, debugger, profiler, formatter, test/bench
  8. Required FFI/interoperability
  9. Required deployment/distribution story
  10. Likely weak spots exposed in Jet
  11. Performance benchmarks
  12. Readability/ergonomics comparison targets
  13. Safety/security risks
  14. Dogfooding plan
  15. Milestone sequence
  16. Exit criteria with concrete tests

  Compare `jetplay` against Godot, Bevy, Love2D, Unity workflow, and Raylib.

  Compare `jetlab` against Jupyter, LangChain/LangGraph, Open WebUI, Cursor/Claude Code-
  style agents, Weights & Biases evals, Obsidian/local-search workflows, and Python/
  TypeScript glue code.

  Important constraints:
  - Jet front end owns all semantics and diagnostics.
  - rustc never speaks to users.
  - Safe by default, expert tier explicit.
  - No hidden runtime cost for beginner ergonomics.
  - One mechanical path per operation.
  - Package/config/deploy story must stay beginner-friendly.
  - Capstone should reveal where Jet is worse than Rust, Go, Python, TypeScript, Nix, Zig,
  or C, not hide it.

  Final output:
  - Recommend one primary capstone and one secondary capstone.
  - Include a “Jet must improve here before this is credible” list.
  - Include a “this proves Jet’s thesis if successful” list.
