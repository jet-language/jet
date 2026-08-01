# jet prove hostile platform matrix (E3 Linux)

E3 closes the Linux hostile fail-closed matrix for `jet prove`. Native
macOS/Windows execution remains Epoch 9 (owner-directed deferral).

| Case | Expected | Linux evidence |
|------|----------|----------------|
| Unknown `--lens` | E2941 exit 2, no `.jetproof` | CLI validation in `CmdProve` |
| Sensitive capture non-TTY | E3627 exit 1 | `ProveReplay::run_safe_capture` |
| Corrupt `.jetproof-replay` | E3622 | footer/frame hash checks |
| Schema mismatch | E3620 | major/minor gate |
| Identity mismatch | E3621 | entry/source/adapter/triple |
| Missing Time on replay | E3628 | `extract_first_time_ms` |
| Solver counterexample | E2950 exit 1 | `--lens solver` + certificate check |
| Absolute capture path escape | refused / project-relative | capture path resolver |

Static cross-platform design checks (no native execution required in E3):

- Artifact family is `.jetproof` / `.jetproof-replay` only (D-ARTIFACT-EXT1).
- No second ProofReport schema under a presentation lens (`--json` is complete).
- Solver is opt-in; bare `jet prove` never runs it.
