# Post-Epoch-3 capstone design

Design two serious, personally useful projects that stress Jet as a language,
standard library, toolchain, package system, and deployment platform. Assume
Epoch 3 is complete; do not constrain the design to today's implementation.

- **jetplay:** a Jet-built 2D game engine and editor.
- **jetlab:** a local-first Jet workbench for documents, code, agents, evals,
  datasets, model adapters, and repeatable workflows.

For each, define product vision, useful MVP, full capstone, required language
features, stdlib APIs, packages/dependencies, LSP/debugger/profiler/formatter/
test/bench tooling, FFI, deployment, likely Jet weaknesses, performance
benchmarks, readability targets, safety/security risks, dogfooding sequence,
milestones, and executable exit criteria.

Compare jetplay with Godot, Bevy, LÖVE, Unity workflows, and raylib. Compare
jetlab with Jupyter, LangGraph-style systems, Open WebUI, coding agents,
Weights & Biases-style evals, local knowledge tools, and Python/TypeScript glue.
Use current primary sources for any claims that may have changed.

Apply `AGENTS.md` philosophy and the beginner/expert passes. Jet owns semantics
and diagnostics; rustc stays hidden; safety is default and expert control
explicit; beginner ergonomics cannot hide runtime cost; each operation has one
canonical mechanism; packaging and deployment remain approachable.

One Sol author produces the design. A fresh Sol reviewer challenges coverage,
benchmarks, and whether the projects expose real weaknesses; author fixes and
reviewer rechecks. A fresh Terra reviewer independently challenges the revised
design and rechecks fixes. Reviewers do not rewrite it.

Recommend one primary and one secondary capstone. Include “Jet must improve
before this is credible” and “this proves Jet's thesis if successful.” Expose
where Jet loses to Rust, Go, Python, TypeScript, Nix, Zig, or C; do not hide it.
