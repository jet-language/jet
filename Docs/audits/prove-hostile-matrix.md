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
| No mapped replay counterpart | reproduce only; no parity claim | replay evidence and terminal view report parity unavailable |
| Opaque callable or contextual boundary | E3625 exit 1 before producer | complete AST preflight rejects unknown calls, context/scope blocks, and unsupported statements |
| Unsupported Time operation (sleep/monotonic/civil read) | E3625/E3623 before producer or artifact read | the current adapter records only `core.time.now`; replay never sleeps or opens another clock |
| Multiple `core.time.now` sites | E3625 capture / E3623 replay | one bounded Time record cannot establish request order for multiple sites |
| Solver counterexample | E2950 exit 1 | `--lens solver` + certificate check |
| Noncanonical certificate tree | ICE 101 | exact node keys, ordered `and_intro` coverage, sound assumption leaves, and split-variable checks |
| Rejected canonical budget report | pass_incomplete/0 | unavailable evidence is included in `selected` and `unavailable` |
| Malformed producer report or timed-out descendant | ICE 101 / pass_incomplete/0 | bounded protocol parser and process-group supervision retain no partial evidence |
| Absolute capture path escape | refused / project-relative | capture path resolver |
| Replay path traversal, empty component, NUL, or backslash | E3622 | replay reader validates path syntax before opening the artifact |
| Safe capture reaches Rand/IO/Net | E3627 exit 1 before producer | AST effect preflight in CmdProve |

Static cross-platform design checks (no native execution required in E3):

- Artifact family is `.jetproof` / `.jetproof-replay` only (D-ARTIFACT-EXT1).
- No second ProofReport schema under a presentation lens (`--json` is complete).
- Solver is opt-in; bare `jet prove` never runs it.
- Repeated presentation lenses are set-unioned in the fixed canonical order; JSON remains the complete report.
- JSON refusal diagnostics stay on stdout; replay/capture setup does not leak stderr.
- Test evidence is bounded and member-confined before it enters ProofReport.
