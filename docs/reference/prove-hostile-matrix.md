# jet prove hostile platform matrix (E3 Linux)

E3 closes the Linux hostile fail-closed matrix for `jet prove`. Native
macOS/Windows execution remains Epoch 9 (owner-directed deferral).

| Case | Expected | Linux evidence |
|------|----------|----------------|
| Unknown `--lens` | E2941 exit 2, no `.jetproof` | CLI validation in `CmdProve` |
| Sensitive capture non-TTY | E3627 exit 1 | `ProveReplay::prepare_safe_capture` |
| Corrupt `.jetproof-replay` | E3622 | footer/frame hash checks |
| Schema mismatch | E3620 | major/minor gate |
| Identity mismatch | E3621 | full entry/source/build/lock/TIR/adapter identity |
| Target cardinality | E3624 exit 1 | capture/replay checks one resolved member before authority setup |
| Missing or extra Time authority | E3628 / E3622 | bounded frame validation and exact one-record consumption |
| Replay outcome divergence | E3623 exit 1 | consumed authority and captured status/outcome comparison |
| Solver counterexample | E2950 exit 1 | `--lens solver` + certificate check |
| Absolute capture path escape | refused / project-relative | capture path resolver |

| Safe capture reaches Rand/IO/Net | E3627 exit 1 before producer | AST effect preflight in CmdProve |

Static cross-platform design checks (no native execution required in E3):

- Artifact family is `.jetproof` / `.jetproof-replay` only (D-ARTIFACT-EXT1).
- No second ProofReport schema under a presentation lens (`--json` is complete).
- Solver is opt-in; bare `jet prove` never runs it.
- JSON refusal diagnostics stay on stdout; replay/capture setup does not leak stderr.
- Test evidence is bounded and member-confined before it enters ProofReport.
