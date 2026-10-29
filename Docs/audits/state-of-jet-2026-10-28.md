# Jet state audit — report and proposal

Auditor: StateAudit. Subject: dev f24fc8479b, using the dev compiler a01850c10c (same compiler
sources), self-hosted `jet@0.0.1#2698c35b`. Date: 2026-10-28. Tower snapshot: 4,682 cards
(1,004 open).

Method: I worked as four personas (beginner, scripter, monorepo team, systems/unsafe expert).
For each, I ran real commands from fresh scratch projects, traced each failure to code, and
searched Tower for every finding. All evidence is reproducible from `/mnt/jetscratch/lanes/Audit`:
- `env.sh`, `census_cmds.sh`, `mk_mono.sh`, `mono_probe.sh`;
- `mk_probes*.sh`, `run_probes.sh`, `perf_tiers.sh`, `fmt_probe.sh`, `doc_blocks.sh`;
- `diag_cov.py`, `tower_health.mjs`, `tq.sh`.

Logs are in `notes/`. Code is the source of truth, and docs are never used as evidence of
behaviour. Claims I did not execute are marked `[INFERENCE]`.

## Verdict in one paragraph

The core language, compiler and runtime are real and fast:
- `jet run`, `jet build`, `jet check`, `jet test` and `jet fmt` work;
- builds take 0.02–0.2 s;
- fib(30) plus a 2M-element sieve runs in 0.08 s;
- exact arithmetic, graphemes, regex, JSON/TOML/YAML, CSV, tasks and SQLite-in-memory work;
- diagnostics usually give what/why/fix.

Everything around that core is not usable by a new user today:
- there is no way to install Jet;
- there is no package manager;
- there is no editor support, debugger or profiler;
- there is no HTTP server or client, and no target other than the host;
- a third of the CLI answers "not built yet";
- the type checker accepts unknown type names;
- the performance gate has no passing cell.

The fastest path to "a stranger can install Jet and build a real program" is short and mostly
implementation. It is items 1–8 below.

## Top 25 gaps, ranked by user impact

| # | ID | Severity | Gap | Tower | Owner decision? |
|---|----|----------|-----|-------|-----------------|
| 1 | DOC-01 | blocker | No working install path: the install page says `nix profile install github:jet-language/jet`, but `flake.nix` exports no `jet` package | #4154 (partial) | Y (distribution) |
| 2 | LANG-01 | blocker | Unknown type names (`Strng`, `Nope`) pass `jet check` and `jet run` | #5948 | N |
| 3 | PKG-01 | blocker | No package manager: `add`/`fetch`/`update` are not built, and every engine verb fails E1227/E1228 | #4717 U7, #3590, #3665 | Y (jet/jetpack boundary) |
| 4 | CORE-03 | blocker | HTTP server and client cannot be built (E2104 runtime routes); backend is impossible | #3665, #4957 | N |
| 5 | TOOL-01 | blocker | No LSP, debugger, profiler or `--watch` | #5931, #4917, #4720 | N |
| 6 | CLI-01 | blocker | 52 of 120 command forms say "not built yet", but `jet help` advertises them all | #4717 (no census card) | Y (which to cut) |
| 7 | TIER-01 | blocker | `--target` is not built for any value: no web/wasm, embedded or cross builds | #758, #1914, #4136 | Y (order) |
| 8 | CLI-02 | blocker | Bare `jet`, `jet repl` and `jet eval` fail; `jet ?` crashes without a TTY | #4720 | Y (REPL) |
| 9 | PERF-01 | blocker | Perf gate: 0 wins, 10 losses, 17 unmeasured of 27 cells; the receipt is 55 days old and from another binary | #2858 | N |
| 10 | DOC-02 | major | The README's first example and 12 of 29 runnable doc blocks fail `jet check` | #3173 | N |
| 11 | LANG-02 | major | Common constructs are refused with weak teaching (`**`, `defer`, `Float(n)`, multi-value variants, `impl Stack<T>`, `extern "C"`); `..` is inclusive | #3836, #4020 | Y |
| 12 | DIAG-01 | major | `let`, `for`, `while`, `match`, comma enums and `null` from other languages get a generic E0003 | D-S14-PAUSE, #4670 | Y |
| 13 | DIAG-04 | major | Fix lines point the wrong way ("Declare `BigInt`", "Define it inside `struct DBConnection`", `__` names first) | #3720 | N |
| 14 | SEC-01 | major | A `#!` first line silently grants every authority, even under `jet run file.jet` | #3418, #4268 | Y |
| 15 | LAY-01 | major | One `.jet/` per workspace member, none at the root; this violates the owner rule | #3421 (ratified B, not landed) | N |
| 16 | LAY-04 | major | At a workspace root, check/test/build/doc refuse as "ambiguous" | none | Y |
| 17 | LAY-03 | major | `jet doc` overwrote a hand-written `docs/index.md` (data loss) | none | Y/N |
| 18 | LANG-04 | major | Four FFI mechanisms; none works without cargo or a system C compiler | #5482, #1344, #4280 | Y |
| 19 | CORE-01 | major | Core has 3–7 names for one operation (getenv, cwd, listdir, loads, b64encode …) | none | Y |
| 20 | LAY-02 | major | The lockfile is git-ignored by `jet new` (contradicts D-LOCK1) and carries a timestamp | D-LOCK1 | Y |
| 21 | LANG-03 | major | Call-site `&`/`.copy()` marks tax ordinary code | #3836 | Y |
| 22 | CORE-04 | major | No dataframe; the AI/ML Core was cut, against the owner's domain goals | #3955 | Y |
| 23 | DIAG-02 | major | `10 / 0` prints bare "invalid exact quotient" and exits 101 (the Jet-defect code) | none | N |
| 24 | SEC-03 | major | Core crypto is not constant-time | #4852, #4855 | N |
| 25 | SEC-02 | major | C calls through `#Import module` need no `#Unsafe` [INFERENCE] | #4280 | Y |

Next in line:
- FMT-01: the scaffold is not `jet fmt`-clean.
- DIAG-05: 179 active errors have no snapshot.
- CLI-04: internal names leak into errors.
- HEALTH-01: goldens are at 72%.
- HEALTH-02: tests still need cargo.
- TOWER-01: there are 51 P0 cards.

## Critical path (what blocks the next result)

1. **LANG-01, #5948.** This is a correctness hole in every program and is small. Land it first.
2. **DOC-01, install.** Publish the seed-built `jet` plus runtime as a tarball, with a
   `packages.default` in the flake. Without this, no other fix reaches a user.
3. **CORE-03 HTTP client.** #3665 unblocks both backend work and the hosted `jetpack` (PKG-01),
   so it is the single most leveraged card. After it come the #4957 server and PKG-01's minimal
   in-process deps.
4. **TOOL-01, LSP.** This can run in parallel with item 3.
5. **CLI-01/CLI-02.** Hide unbuilt forms and make bare `jet` print the greeting. This takes a
   day and makes the product honest.
6. **TIER-01, wasm32.** This adds the web tier and gives I9 something to compare against.
7. **PERF-01.** Re-baseline the gauntlet on the current compiler before any further perf work.

These can run in parallel with the above, because they are independent and pure
implementation: DIAG-02, LAY-01/03 (under #3421), DOC-02's CI guard, CLI-04's text guard, and
CORE-02.

## Owner decisions needed (ballots, detailed in the last section)

- Lockfile location
- Script authority and shebang
- Workspace-root command behaviour
- Core one-name purge
- jet/jetpack boundary for minimal deps
- `jet doc` output location
- Five surface questions (power operator, general `defer`, multi-value variants, positional
  tuple destructure, half-open `..`)
- Receiver-mark inference
- One FFI form
- Unsafe on imported C
- One optimisation flag
- Dataframe and re-scoping #3955
- REPL model
- Foreign-spelling teaching (D-S14-PAUSE)
- Formatter collapse style
- One manifest
- XDG paths
- Target order
- P0 cap and epoch hygiene
- Distribution
- Which unbuilt commands to cut

## Finding format

Each finding gives: ID, area, severity (blocker/major/minor/polish), personas, evidence (command + output or file:line), root cause, Tower cards (or none), options with trade-offs, recommendation, and whether it needs an owner decision. Source paths are relative to `git archive f24fc8479b`. The machine-readable index is `findings.tsv`.

## Area 1 — Layout, `.jet/`, manifests, lockfiles, generated artifacts

### LAY-01 — Every workspace member gets its own `.jet/`; the workspace root gets none
- Area: 1 Layout
- Severity: major (direct violation of the owner's stated rule)
- Personas: team/monorepo
- Evidence: `mk_mono.sh` + `mono_probe.sh` → `notes/mono_probe.log:165-179`. After running
  `jet build`/`test`/`run` inside members, `.jet/` exists at `packages/app/.jet`,
  `packages/geometry/.jet` and `tools/report/.jet`; the root `mono/.jet` never appears.
- Root cause: every writer roots `.jet` at the *package* root, not the workspace root:
  `jet_cli_build_dir_path` = `"{snapshot.roots[0].canonical_path}/.jet/build"`
  (`Compiler/JetCli/Source/Cli/Host.jet:952-956`), the test runner `"{root}/.jet/build"`
  (Host.jet:1288), `jet new` writes `.jet/lock` in the new package (Host.jet:1444-1478), and
  12 more `…}/.jet` joins in `Compiler/JetCli` (`git grep -E 'canonical_path\}/\.jet|root\}/\.jet|dir\}/\.jet'`).
  A workspace-root resolver exists (`ProjectRoot.jet:16 jet_cli_selected_project_root`) but
  only receipts use it. Jetpack code paths (`ProjectState.jet` `.jet/last-run`,
  `Core/Jobs/Jobs.jet` `.jet/state/jobs.db`, `.jet/services/*`, `.jet/bindings/*`) each pick
  their own root.
- Tower: #3421 (P1 ready, sidequest, no epoch) "Consolidate project-generated files under one
  selected root"; D-GENERATED-STORAGE2=B is ratified and its checkSteps already require
  "nested entries and workspace members … stay under the same top-level .jet/build/ root".
  The card is not landed; the shipped code still writes per-member `.jet/`. #4849 is adjacent.
- Options:
  1. One `jet_project_state_dir(start)` in JetFoundation that returns
     `<workspace root or package root>/.jet`, and route every writer through it; member data
     goes under `.jet/members/<name>/…` or is keyed by package id.
  2. Same, but also move binaries to `<root>/.jet/build/<member>/<name>` so `jet build -p app`
     and `jet build` from inside `packages/app` write the same file.
  3. Keep per-package `.jet/` but forbid it inside a workspace (error). Simple, but the user
     still needs a root store for workspace-wide caches.
- Recommendation: 1+2, delivered under #3421 (raise to P0: it is an owner rule), with a guard
  test that runs the mono workspace and asserts exactly one `.jet/`. Also fold LAY-03
  (`docs/`) into #3421.
- Needs owner: N (rule already given); the member key layout is implementation.

### LAY-02 — The lockfile lives in the generated-state folder, is git-ignored by `jet new`, and churns
- Area: 1 Layout / 6 Tooling (reproducibility)
- Severity: major
- Personas: team, expert (reproducible builds)
- Evidence: `jet new hello` writes `.gitignore` = `.jet/build/ .jet/lock .jet/cache/`
  (Host.jet:1478) and `.jet/lock` containing `[build.stamp] … at = "2026-10-28T10:02:44Z"`
  plus the toolchain pin. Spec says the opposite twice: "`jet new` commits the lock for
  executables, ignores it for libraries (D-LOCK1)" (`Docs/spec/spec.md:5636-5638`) and
  "`.jet/lock` … project-owned and may be committed" while ".jet/ holds only generated state"
  (`spec.md:5506`, `reference/jetpack-epoch5.md:273-278`).
- Root cause: one file mixes committed facts (toolchain pin, dependency graph, comptime input
  hashes) with per-machine build state (timestamp, dirty flag); the scaffold picked "ignore"
  to avoid churn, which drops reproducibility.
- Tower: D-LOCK1 (ratified), #4849 (lock file vs directory), #4833 (`--locked`). None covers
  the gitignore contradiction or the timestamp.
- Options:
  1. Move the lock to the project root as `jet.lock` (committed, like Cargo.lock/go.sum), keep
     `.jet/` purely disposable, move `[build.stamp]` into `.jet/build/stamp`.
  2. Keep `.jet/lock` but strip machine state from it and make `jet new` stop ignoring it.
  3. Keep as is and amend D-LOCK1 (no committed lock). Loses reproducibility.
- Recommendation: 1: "`.jet/` is always safe to delete" is the simplest rule to teach and
  makes `.gitignore` a single line `.jet/`. Needs an owner decision (path/format change).
- Needs owner: Y

### LAY-03 — `jet doc` writes `docs/` into the source tree and silently overwrites user files
- Area: 1 Layout / 6 Tooling (docs)
- Severity: major (data loss)
- Personas: all
- Evidence: `jet new site && mkdir site/docs && echo 'my handwritten guide' > site/docs/index.md
  && cd site && jet doc` → `doc: generated …/site/docs`, exit 0; `docs/index.md` now starts
  `# Jet Documentation` (the hand-written file is gone). In the workspace each member gets
  `packages/app/docs/`. For an absolute single file at `/x.jet` the target is `/docs`.
- Root cause: `Host.jet:1551-1557` hard-codes `{directory}/docs` next to the entry and writes
  `index.html`/`index.md` with no existence or ownership check; not under `.jet/`, not in
  the generated `.gitignore`.
- Tower: none found ("jet doc", "docs/index").
- Options:
  1. Write to `<project>/.jet/doc/` by default and print the path; `--out <dir>` to publish.
  2. Keep `docs/` but refuse to overwrite files not carrying a Jet "generated" header.
  3. Both.
- Recommendation: 3 (1 is the default, 2 protects `--out`). Output location is product
  behaviour → owner decision; the overwrite guard is pure implementation and urgent.
- Needs owner: Y (default path), N (guard)

### LAY-04 — Workspace roots are second-class: check/test/build/doc refuse, status lies
- Area: 1 Layout / 2 CLI
- Severity: major
- Personas: team/monorepo
- Evidence (`notes/mono_probe.log`): at the root, `jet check`, `jet test`, `jet build`,
  `jet doc` → `Error [E2104]: \`jet check\` is ambiguous — 3 workspace members can run …
  Fix: Pick one with \`jet check -p <member>\``, exit 2. `geometry` has no entry and is still
  called "runnable". `jet status` at the root prints `target  run.jet` (no such file);
  `jet project parts` prints nothing, exit 0. `jet fmt` at the root does work (reformatted all
  three member manifests). A member whose dependency fails reports only
  `E0626 Dependency package \`geometry\` has errors … Fix: Run \`jet check ../geometry\``
  instead of the errors.
- Root cause: root resolution treats a workspace as "choose one runnable member"
  (Entry.jet `jet_cli_find_package_root`), the same rule for run (where it is right) and for
  check/test/doc/build (where the peer norm — `cargo test --workspace`, `go test ./...`,
  `pnpm -r` — is "all members"). E0626 deliberately suppresses dependency diagnostics
  (Why text says so).
- Tower: none for workspace-wide check/test. #4579 (frontend stops after first module
  error) is adjacent to E0626.
- Options:
  1. At a workspace root, `check`, `test`, `fmt`, `doc`, `build` act on every member; `run`
     and `dev` keep the ambiguity error. `-p` narrows.
  2. Add an explicit `--all` (or `-p '*'`) and keep the refusal as default.
  3. Make the root package.jet declare `default-members:`.
- Recommendation: 1 (matches every peer, fewest keystrokes), with E0626 printing the
  dependency's diagnostics inline (collect-all). Owner decision (command behaviour).
- Needs owner: Y

### LAY-05 — Too many root-level config files and two workspace mechanisms
- Area: 1 Layout / 3 Language (I8)
- Severity: minor
- Personas: beginner, team
- Evidence: reserved root names `package.jet`, `pkg.jet`, `env.jet`, `config.jet`,
  `workspace.jet` (`Entry.jet:74`), command-role files `@run.jet @build.jet @dev.jet @test.jet`
  (`Entry.jet:72`), build hook `build.jet` (`Compiler/build.jet`), entries `run.jet` or
  `src/run.jet` (`Entry.jet:77`). Workspaces are declared by `members:` in `package.jet`
  (works, `proj/mono`) and by `workspace.jet` overlays (119 references, e.g.
  `Jetpack/PackageModel/Source/Workspace/OverlayParse.jet`). `pkg.jet` is reserved but no
  PackageModel code reads it.
- Root cause: each feature added its own file; nothing enforced one manifest.
- Tower: none specific; #4913 (one option table for CLI flags and build config) is adjacent.
- Options:
  1. One manifest `package.jet` with sections (`workspace:`, `env:`, `build:`, `commands:`),
     and files only when a section grows (`build.jet` for code).
  2. Keep the files but document one table and delete `pkg.jet`/duplicate workspace forms.
  3. Status quo.
- Recommendation: 2 now (delete `pkg.jet`, pick one workspace form), 1 as an owner ballot.
- Needs owner: Y

### LAY-06 — `.jet/` mixes disposable caches with committed and sensitive files; the generated `.gitignore` covers 3 of ~60 paths
- Area: 1 Layout / 8 Safety
- Severity: minor
- Personas: team
- Evidence: `git grep -o '\.jet/…'` over Compiler/Core/Jetpack finds ~60 distinct subpaths:
  `lock`, `build`, `cache/*`, `records`, `receipts`, `generated`, `bindings/{c,cpp,js}`, `bin`,
  `replays`, `last-run`, `fuzz`, `memory/ledger-v1.jsonl`, `codemods`, `state/jobs.db`,
  `services/<name>`, `secrets.age`, `secrets-recipients`, `trust`, `tools`, `images/*`,
  `target/*`, `profiles/*` …. Spec says `.jet/records/saved/` and `.jet/lock` may be
  committed (jetpack-epoch5.md:277) and `.jet/secrets.age` is the committed encrypted
  secrets store, while `jet new`'s `.gitignore` ignores only `build/`, `lock`, `cache/`.
- Root cause: no single layout table; each subsystem chose a subpath.
- Tower: none.
- Options:
  1. Rule: `.jet/` is 100% disposable; anything committed lives at the root (`jet.lock`,
     `secrets.age`, `saved-claims/`). `.gitignore` = `.jet/`.
  2. Split `.jet/` into `.jet/cache/` (ignored) and `.jet/project/` (committed).
  3. Keep and write a complete `.gitignore`.
- Recommendation: 1 (pairs with LAY-02). Owner decision on file names.
- Needs owner: Y

### LAY-07 — `jet build` is silent, names the binary after the entry file, and overwrites debug with release
- Area: 1 Layout / 2 CLI
- Severity: minor
- Personas: beginner, scripter
- Evidence: `jet new hello && cd hello && jet build` prints nothing (exit 0, 0.05 s); the
  binary is `.jet/build/run`, not `hello`. `jet build --release` writes the same path.
  `jet build --verbose` also prints nothing.
- Root cause: `jet_cli_build_binary` uses the file stem (`name`) and one `build_dir`
  (Host.jet:936-944) for every profile.
- Tower: none found.
- Options:
  1. Name the binary after `package.jet` `name:`, profile subdirs `.jet/build/{debug,release}/`,
     print `built .jet/build/debug/hello (4.6 MB)`.
  2. Only print the path.
- Recommendation: 1. Pure implementation except the directory layout (minor owner call).
- Needs owner: N

### LAY-08 — Per-user state is spread over five roots
- Area: 1 Layout
- Severity: polish
- Personas: all (disk cleanup, backups)
- Evidence: a fresh `HOME` after a few commands has `~/.cache/jet/{scripts,store}`; the real
  user home has `~/.jet/{store,git-cache,keys,registry-index,registry-roots,tools,trust}`,
  `~/.cache/jet/{dev-shells,ffi,runtime,scripts,store}`, `~/.local/share/jet/{hangar,leases,…}`,
  `~/.config/jet/env`; `jet self doctor` also reports "legacy Hangar at /etc/jet/hangar awaits
  atomic migration".
- Root cause: no XDG policy; Rust-era and Jet-era paths coexist (greenfield rule says remove
  legacy readers).
- Tower: none found.
- Options: 1. XDG only (`$XDG_CACHE_HOME/jet`, `$XDG_DATA_HOME/jet`, `$XDG_CONFIG_HOME/jet`)
  and delete `~/.jet` and `/etc/jet/hangar` readers; 2. one `~/.jet` root for everything.
- Recommendation: 1. Owner decision (paths).
- Needs owner: Y

## Area 2 — CLI UX

### CLI-01 — A third of the advertised command surface answers "is not built yet"
- Area: 2 CLI UX (also 6 Tooling)
- Severity: blocker
- Personas: all (beginner, scripter, team, expert)
- Evidence: `census_cmds.sh` → `notes/cmd_census.tsv`. 52 of 120 invocations exit 2 with
  `Error [E2104]: \`jet <form>\` is not built yet`, and 13 more fail on the missing Jetpack
  engine (PKG-01). Not built: `repl` (and bare `jet`), `debug`, `self lsp`, `fetch`, `add`,
  `remove`, `update`, `init`, `prove`, `budget`, `perf run`, `emit`,
  `jobs`, `notebook`, `db`, `package`, `flash`, `generate`, `review`, `fold`, `split`,
  `inspect sbom`, `inspect audit`, `audit copies`, `self update`, `registry keygen`,
  `new service|route|job|migration`, `explain --cost`, `eval <expr>`, `run --watch`,
  `test --watch|--docs|--coverage|--shuffle`, `build --target=<any>`, `build --lib`,
  `build --sbom`, `build --profile ci|hardened`.
  `jet help` lists every one of these with full usage text and no marker.
- Root cause: the self-hosted CLI front end (`Compiler/JetCli/Source/Cli/Front.jet:1646-1757`)
  dispatches a fixed list of ported commands; everything else falls to
  `jet_cli_not_ported` (`Front.jet:270-282`). The Rust driver that implemented the rest was
  deleted at the Rust exit (D-EXEC2=A, #4717) before the Jet port reached parity. The help
  page is generated from the old command model, so it still advertises the Rust surface.
- Tower: #4717 (umbrella Rust removal), #4833 (fetch/--locked), #4944 (add/remove/fetch/update
  wiring), #4830 (explain --web-graph), #4940 (jet db), #5931 (devserver / self lsp),
  #4917 (LSP publisher), #5508 (dev --watch=off), #5000 (emit AST/IR), #4720 (debug/REPL off
  evaluator). None enumerates the whole not-built set; no card for repl, init, prove, budget,
  package, flash, generate, review, fold/split, sbom, self update, keygen, `--target`.
- Options:
  1. Hide not-built forms from `jet help`, completions and man until ported (one `ported` bit in
     the command model). Cheap, honest; the surface looks smaller.
  2. Keep listing them but tag `(not built yet)` in help and route each to its Tower card in the
     E2104 fix text. Honest, keeps the roadmap visible, still advertises missing features.
  3. Port the missing commands in priority order (repl, lsp, debug, add/fetch, init,
     `--target`, `test --watch`, `--lib`) and delete the rest from the model as an I8 cut.
- Recommendation: 1 now (pure implementation), plus one census card that lists every
  not-built form with its owning card, then 3 in the order of the executive summary. Deleting
  forms nobody will port (fold, split, try, test-compare) is an owner decision (commands).
- Needs owner: Y (which commands to drop); hiding is implementation.

### CLI-02 — Bare `jet` and `jet repl` fail on day one
- Area: 2 CLI UX / 6 Tooling
- Severity: blocker
- Personas: beginner, scripter
- Evidence: `jet` → `Error [E2104]: \`jet repl\` is not built yet` exit 2 (Front.jet:1506).
  `jet eval '1 + 2'` → `\`jet eval <expression>\` is not built yet` (Front.jet:1352).
  `jet ?` (no TTY) → `error: failed to redirect to /dev/tty: No such device or address (os error 6)`
  exit 1, although first-hour.md:260 promises a static palette without a TTY.
- Root cause: the REPL lives on the in-process evaluator, which AGENTS.md I9 freezes
  ("do not extend it"); the native O0 in-memory tier that would host a REPL has not landed
  (#4720, #4741). Bare `jet` was wired to the REPL, so the greeting path is the broken path.
- Tower: #4719 (REPL failure semantics), #4720 (move REPL off evaluator). No card for bare
  `jet` behaviour, `jet eval <expr>`, or the `jet ?` TTY crash.
- Options:
  1. Bare `jet` prints the greeting and the five beginner commands (the greeting already
     exists: `jet_cli_greeting`, Front.jet:1580) until a REPL ships.
  2. Ship a compile-and-run REPL on the existing O0 build path: each submission appends to a
     hidden `run.jet`, rebuilds, runs; 20–40 ms builds measured below make this viable.
  3. Wait for native in-memory O0.
- Recommendation: 1 immediately (implementation), 2 as the REPL/eval path (owner decision:
  product behaviour of REPL state and side-effect replay), fix `jet ?` to the documented static
  palette.
- Needs owner: Y for option 2.

### CLI-03 — Three version strings disagree
- Area 2; severity polish; personas all.
- Evidence: `jet --version` and `jet version` → `Jet 0.1.0`; `jet new` greeting and README say
  0.0.1; manifest scaffold writes `jet: "0.0.1"`; compiler identity `jet@0.0.1#2698c35b…`.
  `Compiler/JetCli/Commands/Package/Bundle.jet:268` hard-codes `"0.1.0"`; REPL test expects
  `Jet 0.1.0` (`Compiler/JetRepl/Tests/Integration/ReplTests.jet:326`).
- Root cause: no single version constant feeding CLI, scaffold and identity.
- Tower: none.
- Options: 1. one `JET_VERSION` read from the toolchain identity everywhere; 2. drop the
  product version until 1.0 and print the identity only.
- Recommendation: 1. Pure implementation. Needs owner: N.

### CLI-04 — Error text leaks internals or the wrong cause
- Area 2/4; severity major; personas beginner, scripter.
- Evidence:
  - `jet build -o x` and `jet run nothere.jet` → "Can't securely read the file `x`" with a
    generic Why (census rows in `notes/cmd_census.tsv`).
  - `jet test-compare` prints a raw `(os error 2)`; `jet eval run.jet` → `E3401: Standard output`.
  - `jet run net_http_server.jet` → `E2104 … can't build … yet:\nrun [function:…] handle method
    HTTPRouterRegister at …:147..150\nmux [function:server.mux]: runtime route jet_ht…` — literal
    `\n`, internal route names and byte offsets (`Compiler/JetCli/Source/Cli/Host.jet:862-863`,
    `Emit.jet:71`).
  - A failing `#Test` reports `--> test_bench.jet:11 in module:14:source-root:0::test_bench.jet::test::deliberately failing@132..190`
    (internal unit identity from `Compiler/JetDriver/Source/Driver/Loader.jet:992`).
  - Runtime-stop Why text: "The shared report boundary owns the program-side stop … Jet file and
    line are shown in Jet terms (I2)." — invariant IDs and architecture jargon in user output.
- Root cause: report builders pass internal identities and raw OS errors straight into
  What/Why; no review gate on runtime/CLI text the way compile diagnostics have.
- Tower: #3720 (P0 readability sweep; compile rows), #3202 (structured source origin). No card
  for CLI/runtime-stop text or the test-name leak.
- Options: 1. extend #3720 to every `jet_cli_report` call site and runtime stop rows, with a
  guard that rejects `\n`, `@N..N`, `source-root:`, `(I\d)`, `D-` in rendered What/Why;
  2. fix the listed sites only.
- Recommendation: 1. Pure implementation. Needs owner: N.

### CLI-05 — Three optimisation knobs; tiers are indistinguishable in practice
- Area 2/7; severity minor; personas scripter, expert.
- Evidence: `jet run --help` lists `--release`, `--profile release|debug|ci|hardened|<bundle>`,
  `--optimize none|basic|full`, `--inline auto|always_marked|off`. `perf_tiers.sh` on
  `proj/perf/bench.jet` (fib 30 + sieve 2M + 200k map ops): none 0.09 s / 4,878,328 B,
  basic 0.08 s / 4,878,288 B, full 0.07 s / 4,886,744 B, `--release` 0.08 s / 4,886,744 B,
  `--profile=debug` 0.08 s; identical output. `--profile ci|hardened` is E2104 (census).
- Root cause: three overlapping spellings for one choice (I8); the optimiser does little at
  this size.
- Tower: #4913 (one option table), #5925 (x64 code quality), #4155.
- Options: 1. keep `--release` only (plus `profiles:` in package.jet for experts);
  2. keep `--profile` only; 3. status quo.
- Recommendation: 1 (matches cargo/zig mental model, one flag). Needs owner: Y.

### CLI-06 — `jet self doctor` contradicts the tool and the greenfield rule
- Area 2/6; severity minor.
- Evidence: doctor says the LSP is "built in (`jet lsp`)" while `jet self lsp` is E2104; it
  reports "legacy Hangar at /etc/jet/hangar awaits atomic migration".
- Root cause: doctor rows are static text, not probes; a legacy-path reader survives.
- Tower: #5931 (devserver/self lsp). None for the doctor.
- Options: 1. doctor runs each subcommand's readiness probe; delete legacy-hangar logic;
  2. remove the LSP row until it exists.
- Recommendation: 1. Needs owner: N.

### CLI-07 — `jet --help` is 419 lines with 249 flags in one list
- Area 2; severity minor; personas beginner.
- Evidence: `jet --help | wc -l` → 419; flag lines 249; `jet bind` summary wraps mid-line.
  Peers: `cargo --help` ≈ 40 lines, `go help` ≈ 50.
- Root cause: one global option table printed in full.
- Tower: #4913.
- Options: 1. `jet --help` shows the 12 common commands; `jet help <cmd>` shows its flags;
  `jet help --all` for the full table; 2. status quo with paging.
- Recommendation: 1. Needs owner: N (presentation).

### CLI-08 — An old `jetpack` on PATH hijacks package commands
- Area 2/6; severity minor.
- Evidence: census: package verbs dispatch to the first `jetpack` on PATH and fail with E1227
  version skew (`InspectB.jet:574-640` engine handshake).
- Root cause: PATH lookup instead of the toolchain-relative engine.
- Tower: #4944, #3590.
- Options: 1. only the engine beside the running `jet` (toolchain root) is used;
  2. keep PATH but prefer the toolchain one.
- Recommendation: 1. Needs owner: N.

## Area 3 — Language design

### LANG-01 — Unknown type names are accepted by `jet check` and `jet run`
- Area 3/8; severity blocker (soundness of the type checker); personas all.
- Evidence: `proj/probes3/unknown_type.jet` (`name: Strng`, `fn f(x: Nope)`) → `jet check`
  "ok … no errors (1 warning)" exit 0; `jet run` prints `ran`.
- Root cause: type resolution leaves unresolved names as opaque/any instead of reporting.
- Tower: #5948 (P0 ready, no epoch). Matches.
- Options: 1. land #5948 with E0107-style "did you mean `String`"; 2. —
- Recommendation: 1, first in the queue. Needs owner: N.

### LANG-02 — Common constructs are refused, and the teaching is weak
- Area 3/4; severity major; personas beginner, scripter, expert.
- Evidence (`notes/probes/*.out`, `proj/probes3`):
  - `2 ** 10` → E0396 "write `value.pow(exponent)`" (good diagnostic; no operator).
  - `defer print("cleanup")` → E0003 "`defer` only schedules a consuming resource close".
  - `Float(3)` → E0102 "Nothing named `Float` exists here" (the spelling is `Float{n}`);
    `n.to_float()` → E0311 "Check the spelling".
  - `Rect(Float, Float)` → E0003 "A positional enum variant can carry only one value";
    `Shape.Rect(w: 2.0, h: 3.5)` → E0311 "no matching static method `Rect`" (the spelling is
    `Shape.Rect{w:…, h:…}`).
  - `impl Stack<T> {` → E0003 "Expected `{` … found `<`" (spelling is `impl Stack {`).
  - `(tx, rx) :: channel<Int>(4)` → E0313 (tuple destructuring by member name only).
  - `extern "C" { … }` → E0003 "Expected a name after `extern`".
  - `comptime { loop … }` as a value → E0391 "this compiler does not support yet".
  - Ranges: `0..3` is inclusive (`loop i in 0..3` prints 0,1,2,3; `proj/probes3/string_build.jet`),
    the opposite of Rust/Python/Go/Swift half-open defaults — `0..xs.len()` indexes past the end.
- Root cause: these are deliberate one-spelling choices, but each refusal has a generic or
  wrong Fix (DIAG-04), and several (power, general defer, multi-value variants, tuple
  positional destructure) are standard in every peer.
- Tower: #3836 (last-use moves), #4020 (keyword table), #4629 (range lint), #4670/D-S14-PAUSE
  (foreign-spelling teaching retired). None for power operator, general `defer`, positional
  multi-payload variants or `Float(n)`.
- Options: 1. ballot each construct (power operator, general `defer`, multi-value positional
  variants, positional tuple destructure, half-open `..`); 2. keep the surface and fix every
  refusal's Fix to show the exact Jet spelling (registry-driven); 3. both.
- Recommendation: 3: the Fix text is pure implementation; the five surface questions need
  ballots.
- Needs owner: Y.

### LANG-03 — Call-site ownership marks and copies tax ordinary code
- Area 3; severity major; personas beginner, scripter.
- Evidence: `c.bump()` on a `self: &Counter` method → E0224 "This call writes `c`, but it has
  no `&` mark"; the fix is `&c.bump()`. Generic examples need `name.copy()` to return a String
  parameter (`Examples/features/generics/generic_callback_values.jet`). 2 of the 12 failing doc
  blocks fail on exactly this (E0224 `rng`, E0202 `clock`).
- Root cause: D-era receiver marks (`&`, `^`) are mandatory at call sites.
- Tower: #3836 (P0 ready: "Last use moves everywhere; kept parameters inferred; ^ and ~ stay
  optional exact pins"), #4620, #3645.
- Options: 1. land #3836 and extend it to `&` (infer the write mark for local mutable
  receivers, keep it optional as a pin); 2. keep marks, add auto-fix in `jet fix`.
- Recommendation: 1. Needs owner: Y (surface).

### LANG-04 — Four FFI mechanisms, none usable on a stock machine
- Area 3/8; severity major; personas systems expert, embedded.
- Evidence: `Examples/features/lowlevel/ffi.jet` (`extern rust "base64@0.22"`) → E0703 "Can't
  call foreign Rust crates without `cargo`"; `inline_c.jet` (`#[Unsafe, FFI(c)]` with C body) →
  E2105 "the C compiler is not installed"; `native_c/` uses `#Import module c.nativeffi { fn … =
  "symbol" }` plus `deps: { nativeffi: c@"c" }`; `cbind/` uses `deps: { c: c@system }`.
  `extern "C"` is a parse error.
- Root cause: FFI grew per use case; `extern rust` predates the Rust exit; no bundled C
  toolchain.
- Tower: #5482 (drop RustBridge; P1 planning), #1344 (`jet cc` hermetic C toolchain), #4280
  (P0 typed foreign contract), #5034 (C import).
- Options: 1. one form: `#Import module c.<lib>` for binary symbols + `jet cc` for C sources;
  delete `extern rust` (per #5482) and inline C bodies; 2. keep inline C as sugar over (1).
- Recommendation: 1, and ship `jet cc` (zig-cc style) so FFI works with no system cc.
  Needs owner: Y (removing a surface).

### LANG-05 — Reserved-word list is inaccurate
- Area 3; severity minor.
- Evidence: retired `take`, `mut`, `copy` still block identifiers (`CoreSurface.jet:366-378`),
  `take` missing from `jet inspect reserved`; `jet inspect digest` lists `pub` (removed, E0411)
  and `rust`; `add`, `remove`, `change`, `check`, `state`, `it`, `tag` are listed as keywords but
  `fn add` and `change :=` compile.
- Root cause: keyword table not generated from the parser's contextual-keyword set.
- Tower: #4020 (keyword table consistency). Matches.
- Recommendation: land #4020 with a test that every listed keyword is refused as an
  identifier and every refused word is listed. Needs owner: N.

### LANG-06 — Concurrency API gaps
- Area 3/5; severity minor; personas backend.
- Evidence: `sender.send(42) ?? …` → E0405 "`??` only works on a fallible value, not Unit"
  (send cannot report a closed channel); `receiver.recv()` → E0102 "`Receiver` has no method
  `recv`". `task.all { … }` works.
- Tower: #4654 (core.tasks convenience rail), #3325.
- Recommendation: fold into #4654: `send` returns `SendError!`, one receive spelling.
  Needs owner: N.

## Area 4 — Diagnostics (I4)

### DIAG-01 — Code from any other language gets a generic, misleading E0003
- Area: 4 Diagnostics / 3 Language
- Severity: major
- Personas: beginner (everyone arrives from another language)
- Evidence (`proj/mistakes/*.jet`, log `proj/mistakes.log`):
  - `let x = 5`, `for i in 0..3 {`, `while i < 3 {`, `match c {` → all
    `Error [E0003]: This line computes a value but doesn't do anything with it` with
    Fix `Use the value, e.g. \`x :: ...\` or \`print(...)\``. None says Jet spells these
    `x :: 5`, `loop i in 0..3`, `loop i < 3`, `if c == { ... }`.
  - `enum Color { Red, Green }` → `Expected a name for a variant name, found \`,\``.
  - `x: Int = "hello"` → `Expected \`:=\` or \`::\` in a binding` (no hint how to annotate).
  - `x :: null` → Fix `Declare it first: \`null :: ...\`` (wrong advice; should point to `None`/`T?`).
  - `println("hi")` → no did-you-mean `print`.
  - `fn take(b: ^Big) {}` → `Expected a name after \`fn\`, found the keyword \`take\``, but
    `jet inspect reserved` does not list `take`.
- Root cause: D-S14-PAUSE=A (Tower) retired every foreign/old-syntax teaching diagnostic
  (E0008 def/func, E0009 let, E0010 set, E0014 try, E0015 import, E0016 match … ;
  `Compiler/Prelude/Diagnostics.jet:13-33`, 135 retired rows) "until post-Epoch 6". The parser
  falls back to the statement catch-all E0003. `take`/`mut`/`copy` are retired keywords kept
  only to feed teaching errors (`JetFoundation/Data/Syntax/CoreSurface.jet:366-378`), so they
  still block identifiers while the teaching that justified them is off.
- Tower: D-S14-PAUSE (ratified A), #4670 (Python/C spellings mislead), #4579 (frontend stops
  after first module error). No card restores let/for/while/match/enum-comma teaching.
- Options:
  1. Reopen D-S14-PAUSE now for a small fixed set (let/var/const, for, while, match/switch,
     comma enums, `x: T = v`, null/nil, println/console.log, fn main/def/func, import/#include)
     with I4 snapshots. Small, highest beginner payoff.
  2. Keep the pause; improve only E0003's generic text to show the three canonical statement
     forms and a link to the syntax table.
  3. A data-driven "foreign spelling" table in the syntax registry rendered into one parser
     recovery rule (one mechanism; every row is a registry entry with a snapshot).
- Recommendation: 3 (it is the one-mechanism form of 1). Also drop `take` from the keyword
  set or list it in `jet inspect reserved`.
- Needs owner: Y (reopen D-S14-PAUSE; new teaching surface).

### DIAG-02 — Integer division by zero exits 101 (the Jet-defect code) with bare text
- Area 4/7; severity major (I2/I4); personas all.
- Evidence: `proj/probes/lang_div_zero.jet` (`10 / z`, z=0) → stdout/stderr `invalid exact
  quotient`, exit 101. `10 % z` → `Stop [E3010] divided by zero … --> file:3`, exit 70.
- Root cause: Int `/` routes to exact division (`10 / 4` = 2.5); its zero path is
  `precise_fault(rt_static_text("invalid exact quotient"))`
  (`Compiler/JetRuntime/Source/RoutePrecise.jet:134`) and `Core/Math/Math.jet:973`
  `panic("invalid exact quotient")`, not the E3010 stop row.
- Tower: none found.
- Options: 1. route the exact-quotient fault through E3010 "divided by zero" with file/line
  and exit 70; 2. compile-time lint for literal zero divisors as well.
- Recommendation: 1 (+2 later). Needs owner: N.

### DIAG-03 — `jet explain` shows raw templates and unrelated examples
- Area 4; severity minor.
- Evidence: `jet explain E3010` prints `{msg}`; `jet explain E0003` example is
  `fn save(path: String) IOError!` (unrelated); E0003 is the catch-all for dozens of parse
  failures.
- Tower: #3720, #4020.
- Recommendation: split E0003 into specific codes for the top 20 parse failures; explain pages
  render with a sample payload. Needs owner: N.

### DIAG-04 — Fix lines that send users the wrong way
- Area 4; severity major; personas beginner, scripter.
- Evidence:
  - `BigInt.parse(…)` → E0107 "Fix: Declare it first: `BigInt :: ...`".
  - `conn.execute(…)` on `DBConnection` → E0102 "Fix: Define it inside `struct DBConnection`"
    (a Core type the user cannot edit).
  - `c.abs()` on I32, `buf.as_mut_ptr()`, `n.to_float()` → E0311 "Check the spelling" with no
    list of nearby methods; `nums.sorted()` did get "Did you mean `sort`?".
  - `core.ui.column` → E1004 "Use one of: __tui_backend, __gtk_backend, __null_backend, …"
    (internal names first).
  - README sample `fn clean_name(raw: String) -> String !NameError` → E0003 "A unit-fallible
    signature has no `->`" (it is not unit-fallible; the error is prefix `!E` vs suffix `E!`).
  - `"a" + 1` → E0109 with no interpolation hint; `helo`/`println` no did-you-mean;
    `x :: null` suggests `null :: ...`.
- Root cause: generic Fix templates in the registry; no candidate ranking for methods/items;
  internal `__` items not filtered from suggestion lists.
- Tower: #3720 (P0), #4670.
- Options: 1. method/item did-you-mean from the receiver's real surface (edit distance + same
  return type), hide `__` names, never suggest editing Core types; 2. per-row hand-written fixes.
- Recommendation: 1 within #3720. Needs owner: N.

### DIAG-05 — Registry quality census
- Area 4; severity major.
- Evidence (`diag_cov.py` over `Compiler/Prelude/Diagnostics.jet`, tests/, Examples outputs):
  896 E-code rows, 756 active; 577 active codes appear in at least one snapshot, **179 active
  error codes have no snapshot** (largest group E12xx Jetpack: 49); **192 active rows keep the
  generic Why "The registered … rule applies here"**; **497 active rows carry a decision ID**
  somewhere in the row (I7: some in user-visible columns, e.g. E0224 "(D-CAP-RECEIVER1=D)",
  E1221 "(D-AUTHORITY-MANIFEST1)"); E0031's meaning still says "C-style FFI → `extern rust`".
  Only parse errors and type errors are reported together; a file with one name error
  stops other modules (#4579).
- Tower: #3720 (P0 ready), #4579, `tests/diagnostics_coverage_baseline.txt` (tracks 'reason'
  coverage, not snapshot coverage).
- Options: 1. gate: every active row has a snapshot and a specific Why, no `D-` in What/Why/Fix;
  2. sweep only.
- Recommendation: 1 (ratchet). Needs owner: N.

## Area 5 — Core library by domain

### CORE-01 — Core has several names for the same operation (I8)
- Area 5/3; severity major; personas all (and agents).
- Evidence (`jet inspect digest` module index, `notes/digest.md`):
  - environment: `core.sys.get`, `core.sys.getenv`, `core.process.env_get`, `core.process.env`,
    `core.process.env_get_or`;
  - cwd: `core.files.cwd`, `core.files.getcwd`, `core.process.cwd`, `core.sys.current_dir`,
    `core.files.path.cwd`;
  - pid: `core.sys.pid`, `core.sys.getpid`, `core.process.current_pid`;
  - listing: `core.files.list_dir`, `core.files.listdir`, `core.files.path.iterdir`;
  - JSON: `parse`, `decode`, `load`, `loads`, `dump`, `dumps`, `to_string`;
  - base64: `encode`, `b64encode`, `standard_b64encode`, `encodebytes`, `b2a_base64` — and
    `b2a_base64`/`crc32`/`b32encode` also exist in `core.encoding.hex`/`core.archive`;
  - process: `call`, `check_call`, `check_output`, `getoutput`, `getstatusoutput`, `run_spec`;
  - heap: `heappush`, `heappushpop`, `heapreplace`, `nsmallest` (Python names in Jet).
- Root cause: Python stdlib names were mirrored as aliases next to Jet names.
- Tower: none (searched getenv/alias/python names).
- Options: 1. one Jet name per operation, delete aliases, keep a migration table in
  `jet fix` (greenfield rule); 2. keep aliases but hide them from the index; 3. status quo.
- Recommendation: 1. Needs owner: Y (public API).

### CORE-02 — Internal `__` items are exported in the public module index
- Area 5; severity minor.
- Evidence: module index counts of `__`-prefixed items: core.service 135 of 199, core.net 53
  of 152, core.math 46 of 216, core.sys 38 of 106, core.files 18, core.testing 15, core.db 14,
  core.watcher 14, core.http.server 13, core.units 12. E1004 suggestion lists start with them.
- Root cause: "public by default" (#4723) with `__` as a naming convention instead of `priv`.
- Tower: #4723 (public-by-default visibility).
- Options: 1. make `__` items `priv(package)` in Core and fail the build on exported `__`;
  2. filter from index/suggestions only.
- Recommendation: 1. Needs owner: N.

### CORE-03 — Backend: HTTP server and client cannot be built
- Area 5/7; severity blocker (backend/web persona); personas backend, scripter.
- Evidence: `Examples/features/net/http_server.jet` and `proj/probes2/net_http_server.jet` →
  E2104 "The Jet backend can't build … handle method HTTPRouterRegister … runtime route
  jet_http_mux_new". `#4717` log 2026-10-26: "13 E2104 … all core.http client routes … have
  no Jet runtime implementation at all". The gauntlet `netserv.service` cell is unmeasured.
- Tower: #3665 (HTTP client), #4957 (native services/HTTP server), #4635, #2873, #4949 (TLS).
- Recommendation: make #3665 + #4957 the critical path after LANG-01; they also unblock
  Jetpack (PKG-01). Needs owner: N.

### CORE-04 — No dataframe; AI/ML Core was cut
- Area 5; severity major (against the owner's domain goals); personas data, AI/ML.
- Evidence: `core.data` exposes loaders, streams, plots, sketches, Arrow import, but no frame
  type (`data.from_csv` → E1004). `core.compute` has Tensor/matmul/gradient (`matrix` takes 3
  args). Tower #3955 (verify) "Cut the AI/ML/LLM/model Core library (owner ruling
  2026-09-30)". The owner's stated goal includes data analysis and AI/ML.
- Tower: #3955; no dataframe card found.
- Options: 1. a `core.data.frame` (Polars-like lazy columnar API over the existing Arrow
  import) as a first-party battery; 2. leave both to packages once the registry exists;
  3. re-scope #3955: keep tensor/autodiff in Core, models in packages.
- Recommendation: 1 + 3. Needs owner: Y.

### CORE-05 — Display and discoverability gaps in everyday types
- Area 5; severity minor.
- Evidence: `print(crypto.sha256("hello".bytes()))` prints `Digest256 { bytes: [44, 242, …] }`
  (no hex); `crypto.sha256("hello")` refuses String; `print(conn)` for `DBConnection` refused;
  `print(s.split(" "))` refused (one-pass Iter; needs `.to_list()`); `time.now().year()` absent;
  `time.weekday` takes `LocalDate`, not `DateTime`; `files.temp_dir` needs a prefix while
  `sys.temp_dir()` does not.
- Tower: none specific.
- Recommendation: Display = lowercase hex for digests; String overloads for hashes; DateTime
  field accessors. Needs owner: N.

### CORE-06 — Domain scorecard (sample program per domain)
- Area 5. Programs in `proj/probes*/`; outputs in `notes/probes/`.

| Domain | Result | Notes / blocker |
|---|---|---|
| Config (JSON/TOML/YAML, Jet-as-config) | works | `#Codable` decode/encode OK; JSON literals need `{{`; raw strings work |
| Build files | partial | `$build.os` comptime facts work; `jet build --target` E2104 |
| CLI/scripting | works with friction | authority prompts (Env/FS/Exec) unless a shebang (SEC-01); 1.2–1.5 s first compile when Core is used |
| Files / process | works | API duplication (CORE-01) |
| Text/unicode, regex | works | graphemes, scalar counts, display width correct |
| Date/time | partial | API discoverability (CORE-05) |
| Numerics | works | arbitrary-precision Int, exact `/`, decimal literals (`0.1+0.2` = 0.3); DIAG-02 |
| Crypto | works | bytes-only API; timing leaks open (SEC-03) |
| Databases | partial | `open_memory` works; API names unclear |
| Networking/backend | blocked | CORE-03 |
| Web/wasm | blocked | `--target` not built (#1914, #758) |
| Data/dataframes | missing | CORE-04 |
| AI/ML | cut | CORE-04, #3955 |
| Games | partial | raylib binding exists; needs GPU authority; not runnable headless |
| GUI | partial | `core.ui` node primitives + gtk/tui backends; no layout widgets |
| Concurrency | works | `task.all`; channel gaps (LANG-06) |
| Systems/unsafe/FFI | blocked on stock host | LANG-04 |
| Embedded | blocked | `--target`/boards not built |
| Testing | works | `#Test` + `jet test`; failure location leaks internals (CLI-04) |
| Bench/profiling | missing | `jet perf run` E2104 |
| Docs | works | LAY-03 data loss |
| Packaging/registry | missing | PKG-01 |
| Cross-compilation | missing | `--target` E2104 (#758) |

## Area 6 — Tooling

### PKG-01 — No package manager: every Jetpack verb fails
- Area: 6 Tooling (Jetpack) / 1 Layout
- Severity: blocker
- Personas: team, scripter, expert
- Evidence: `jet clean|search|env|image|trust|os|cc|c++|inspect outdated` →
  `Error [E1227]: \`jet\` 0.0.1 and \`jetpack\` disagree (no protocol reply)` when an old
  `jetpack` is on PATH (here a Rust-era wrapper that prints `run 'cargo build'`), and with a clean
  PATH `Error [E1228]: \`clean\` needs the \`jetpack\` engine, which isn't installed`.
  `jet add|fetch|update|remove` → E2104 not built. No dependency can be added or fetched.
- Root cause: `jet` execs a separate `jetpack` binary (InspectB.jet:574-640). The Rust jetpack
  was deleted and the Jet-hosted jetpack (#4717 step U7, #3590) does not build yet; the
  hosted census is blocked on the Jet HTTP client (#3665). E1227's text blames a version skew
  when the real cause is "no working engine".
- Tower: #4717 (U7), #3590, #3665, #4833, #4944, #4040, #4031, #4034, #4037, #4843.
- Options:
  1. Ship a minimal in-process dependency path first (path + git deps, lockfile write, `jet add`
     for path/git) inside `jet`, and keep the engine split for store/env/image later.
  2. Finish U7 as planned (whole Jetpack in Jet), gated on #3665.
  3. Both: 1 as the critical path, 2 continues behind it.
- Recommendation: 3. Also make E1227 distinguish "engine missing/broken" from "version skew"
  (implementation). Moving dependency resolution into `jet` changes the jet/jetpack boundary,
  so option 1 needs an owner decision.
- Needs owner: Y (boundary), N for the diagnostic fix.

### FMT-01 — `jet new` output is not `jet fmt`-clean; the formatter makes the scaffold worse
- Area: 6 Tooling / 1 Layout
- Severity: major
- Personas: beginner, team (CI `jet fmt --check` fails on a fresh project)
- Evidence: `jet new fresh && cd fresh && jet fmt --check` → lists `package.jet` and `run.jet`,
  exit 1. `--diff`: the formatter rewrites `authority: { allow: [IO, Mem.Alloc] }` to three
  lines, deletes the blank line before `deps: {}`, and collapses
  `fn run() {\n    print(greeting("world"))\n}` to `fn run() { print(greeting("world")) }`.
- Root cause: the scaffold text is hand-written in the CLI (`jet_cli_new`, Front.jet ~1378) and
  never round-tripped through the formatter; the formatter's block-collapse and record-expand
  rules disagree with the style the docs and examples teach (first-hour.md shows the
  multi-line `fn run`).
- Tower: none found (searched "jet new", "scaffold", "fmt").
- Options:
  1. Generate scaffolds by running the formatter over the template at build time (one home).
  2. Change the formatter rules to keep single-statement multi-line bodies and keep blank lines
     between top-level manifest fields.
  3. Add a test that `jet new <t> && jet fmt --check` passes for every template.
- Recommendation: 3 plus 2 (blank-line preservation is a formatter defect; collapsing is a style
  rule). Formatter style is product behaviour, so the collapse rule needs an owner call.
- Needs owner: Y (formatter style), N (test + scaffold fix).

### TOOL-01 — No editor support, debugger or profiler
- Area 6; severity blocker (team/expert daily loop); personas all.
- Evidence: census: `jet self lsp`, `jet debug`, `jet perf run`, `jet run --watch`,
  `jet test --watch|--coverage` → E2104 "not built yet". `Tools/editors` exists; there is no
  working server for it.
- Tower: #5931 (devserver/self lsp), #4917 (LSP publisher), #3410 (LSP 3.18), #4719/#4720
  (REPL/debug off evaluator), #5008, #4632, #5508.
- Options: 1. LSP first (diagnostics + go-to-def + hover from `jet check` data), then
  `--watch`, then DAP; 2. all three in parallel.
- Recommendation: 1; LSP is the highest-leverage tool after the compiler. Needs owner: N.

- FMT-01 additional evidence (`fmt_probe.sh`): 27 probe files, `jet fmt` changed 6, no
  idempotence failures, refused 4 files with parse errors; it collapses a three-line function
  body to one line (`fn make_adder(n: Int) -> fn(Int) -> Int { x -> x + n }`).

## Area 7 — Execution tiers (I9) and performance gate

### PERF-01 — The performance gate has no passing cell and its evidence is stale
- Area 7; severity blocker (against AGENTS.md "strictly beat every matched peer").
- Evidence: `Tools/gauntlet/status.json` generated 2026-09-03 (55 days old, a pre-self-host
  binary `45808f92…`): 27 cells → win 0, parity 0, loss 10, unmeasured 17; metrics win 20,
  loss 86, unmeasured 150; publication `complete: false` with 2022 blockers. Required areas GUI,
  games, embedded, web, backend service have no measured cell. Losses include `text.kernel`
  (aot ratio 9.4× C, 1.3× node), `text.regex-find-all-large` (1.5–1.9× C/Go/Rust),
  `cli.app` (2.07× Rust aot), `numerics.float-kernel` (1.25× C). Local probe: fib(30)+sieve(2M)
  runs in 0.07–0.09 s at every tier.
- Tower: #2858 (P1 measurement gate, blocked by 14 cards), #2873, #3104, #3404, #4566, #5476,
  #5925, #4551.
- Options: 1. re-baseline the gauntlet on the current self-hosted compiler now (one run, all
  cells), then work losses by cell; 2. wait for HTTP/targets so every cell can be measured.
- Recommendation: 1: a 55-day-old receipt from another compiler says nothing about today.
  Needs owner: N.

### PERF-02 — Binary size and first-compile latency
- Area 7; severity minor.
- Evidence: hello world 4.8 MB at every tier; first `jet run` of a file that imports Core
  1.2–1.5 s, warm 0.02 s; prelude-only files 0.02 s.
- Tower: #4155 (P0 codegen size), #5926 (warm cache slower than cold), #5476 (hello ≤ 2 ms).
- Recommendation: as carded. Needs owner: N.

### TIER-01 — No target other than the host: web, wasm, embedded and cross builds are not built
- Area 7; severity blocker for web/embedded personas.
- Evidence: `jet build --target=<any>` → E2104 (census). I9 requires web parity "where
  applicable"; there is no web tier to compare.
- Tower: #758, #1914, #4136 (aarch64 P0), e13 Cross Platform (**out of current scope**).
- Recommendation: wasm32 first (web + sandbox), then aarch64. Needs owner: Y (order).

## Area 8 — Safety and security (I1, supply chain, sandboxing)

### SEC-01 — A `#!` first line silently grants every authority, even under `jet run file.jet`
- Area 8; severity major; personas scripter (and anyone running downloaded code).
- Evidence: `proj/probes3/shebang.jet` (`#!/usr/bin/env jet`, writes a file, no package block):
  `jet run shebang.jet` → "wrote a file with no authority declared", exit 0. Same file without
  the first line → `E1803 Application authority is undecided for FS.Write`. Help text:
  "A file starting with a #!/ line runs this way" (`--allow-all`).
  Root cause `Compiler/JetCli/Source/Cli/Authority.jet:57` `source.starts_with("#!/")`.
- Tower: #3418 (unify inferred effects, trusted defaults), #4268 (P0 authority boundaries). No
  card names the shebang rule.
- Options: 1. shebang grants nothing extra; scripts declare `authority:` in a leading package
  block (or `#!/usr/bin/env -S jet run --allow=FS`); 2. shebang grants only when executed
  directly (argv[0] is the file), never under `jet run`; 3. a script-default authority set
  (IO, Env, FS.Read of cwd) for all single files.
- Recommendation: 3 + 1: ergonomic defaults for every file, no hidden escalation.
  Needs owner: Y.

### SEC-02 — C calls through `#Import module c.…` need no `#Unsafe`
- Area 8; severity major [INFERENCE: read from examples, not executed — no C compiler here].
- Evidence: `Examples/features/lowlevel/native_c/run.jet` calls `c.write(&signed)` and
  `c.text(...)` with no `#Unsafe` block; `inline_c.jet` requires `#[Unsafe("…"), FFI(c)]` and an
  `#Unsafe` call block. I1: "Expert unsafe code needs `#Unsafe("reason")`".
- Tower: #4280 (P0 typed foreign contract), #4268.
- Options: 1. calls into imported C are unsafe unless the import row is marked audited
  (`#Import(audited: "reason")`); 2. all foreign calls require `#Unsafe`.
- Recommendation: 1. Needs owner: Y.

### SEC-03 — Core crypto is not constant-time
- Area 8; severity major.
- Evidence: Tower #4852 (AES-GCM secret-indexed S-box, data-dependent GHASH) and #4855
  (X25519/Ed25519/Poly1305 secret branches, BigInt on secrets), both P0 planning since
  2026-10-08; owner ruling 2026-10-28 keeps crypto Jet-native (#5482 log).
- Recommendation: label `core.crypto` "not for production secrets" in docs and `jet inspect`
  until both land; add a timing-variance test. Needs owner: N.

### SEC-04 — Examples teach over-granting
- Area 8/9; severity polish.
- Evidence: `Examples/features/lowlevel/ffi.jet` grants `[Browser, DB, Env, Exec, FFI, FS, GPU,
  IO, Log, Mem.Alloc, Net, Rand, Secret, Time]` to call one base64 function.
- Recommendation: examples grant the minimum; `jet check` lint for unused grants already exists
  (`authority_unused_grant.jet`) — run it on Examples in CI. Needs owner: N.

## Area 9 — Docs, website, examples

### DOC-01 — There is no working install path
- Area 9; severity blocker; personas all new users.
- Evidence: `Docs/site/dist/install.html`: "the supported install path is Nix …
  `nix profile install github:jet-language/jet` … or `nix build`". `flake.nix` (308 lines)
  exports only `packages = { rust-musl-toolchain; musl-linker }` and a cargo dev shell; there is
  no `jet` package or default package. No release binaries are referenced elsewhere.
- Tower: #4154 (bootstrap resilience, seed binaries), D-SEED-DIST1=A; no card for a user
  install path.
- Options: 1. publish the seed-built `jet` + runtime as a tarball with a one-line installer
  and a `packages.default` in the flake; 2. Nix only, fixed flake; 3. source build only (needs
  the seed).
- Recommendation: 1 (2 follows). Needs owner: Y (distribution).

### DOC-02 — The README's first example and 41% of runnable doc examples do not compile
- Area 9; severity major.
- Evidence: `doc_blocks.sh`: 29 ```jet blocks with `fn run` in README, guides, site and
  reference → 17 pass, 12 fail `jet check`. README block → E0003 (prefix `!NameError`);
  `core-library.md` 7 (E0107 `process`, E0302, E0202, E0740, E0224, E0357, E0433),
  `metaprogramming.md` 3 (teaches `@WIDTH`/`@CONFIG` prefix constants, E0357; #3662 pending),
  `foreign-octave.md` 1 (E1002).
- Root cause: no doc-block check in CI; the language changes in place.
- Tower: #3173 (reconcile published examples), #3065 (P0 restore shipped example corpus). None
  for Markdown blocks.
- Options: 1. `jet test --docs` over every ```jet block (the flag exists but is E2104); 2. a
  guard script in CI now.
- Recommendation: 2 now, 1 when built. Needs owner: N.

### DOC-03 — Docs and examples carry internal provenance
- Area 9/10; severity minor (I7).
- Evidence: decision-ID citations: Examples 754 lines in 641 files (e.g. `first_hour.jet`
  "D-EXAMPLES-SHORTPATH1=A"); effects example says "this program generates the same Rust";
  `jet inspect digest` lists removed `pub`.
- Tower: #3173.
- Recommendation: strip IDs from Examples and user docs in one codemod; guard. Needs owner: N.

## Area 10 — Project health

### HEALTH-01 — Goldens 72%; 210 single-symptom failure cards
- Evidence: #4717 log 2026-10-26: goldens 808 pass / 312 fail; 210 open Tower cards titled
  `golden: …` (60 of them `golden: E2104 runtime route …`).
- Tower: #4599 (P0 master golden suite), #3065.
- Recommendation: group failures by root cause (runtime routes, E0108 coercions, CSV decode),
  one card per cause; close the per-symptom cards into them. Needs owner: N.

### HEALTH-02 — Rust is not gone; the test suite still needs cargo
- Evidence: `git ls-tree` at f24fc8479b: 197 `.rs` files — Jetpack/Integrations 96
  (RustBridge), Tools/gauntlet 30 and Tools/perf 27 (peer benchmarks, kept by ruling),
  23 top-level `tests/*.rs` + support. This host has no cargo, so those tests cannot run.
  The `jetpack` engine binary does not exist (#4717 log: hosted build blocked on #3665).
- Tower: #4717 (P0 building), #5453, #5482, #5078 (P0, 638 Jet files absent from master).
- Recommendation: finish #5453 (port tests) before #5482; record a Rust-free contributor
  check (`jet test` at the repo root) as the exit gate. Needs owner: N.

### HEALTH-03 — 8,015 comment lines point at deleted Rust sources
- Evidence: `git grep -c -E '//.*\b[A-Za-z_/]+\.rs\b' -- Compiler Core Jetpack` → 8015 lines
  in 1242 files (e.g. `Entry.jet:64` "Syntax.rs `DEFAULT_ENTRY_FILE` …", `Request.jet:814`
  "Lock.rs `build_stamp`"); decision-ID text: ~1079 lines in Compiler, 2280 in Jetpack.
- Tower: none (searched comment + Rust).
- Recommendation: one mechanical codemod after #4717 closes; a guard rejects new `.rs`
  references in Jet comments. Needs owner: N.

## Area 11 — Tower

### TOWER-01 — 51 open P0 cards
- Evidence (`tower_health.mjs`): open 1004 (planning 670, ready 204, frozen 122, building 4,
  verify 3, deciding 1); P0 51, P1 273. P0 includes 10M lines/s front end (#5914), 500k LOC/s
  (#4999), 300k lines < 2 s (#4551) next to #5948 (unknown types accepted).
- Recommendation: cap P0 at ~10 user-visible blockers (this report's top 25 is a candidate
  list); demote throughput targets to P1 under one perf epic. Needs owner: Y.

### TOWER-02 — Structure: 639 open cards have no epoch; e16 "arrived" still holds 144
- Evidence: open by epoch NONE 639, e16 144, e9 111 (out of current scope), e3 39, e11 16,
  e5 15, e13 14 (out of current scope), e17 9; 684 of 1004 are `sidequest`. Epoch e16 is
  marked arrived with 144 open cards.
- Recommendation: every open card gets an epoch or is closed; arrived epochs cannot hold open
  cards (move or close). Needs owner: Y.

### TOWER-03 — Staleness and missing acceptance
- Evidence: 385 open cards have no log entry in 30+ days (351 planning); 624 open cards have no
  criteria (571 planning, 50 ready); 38 open cards list done cards in `blockedBy`.
- Recommendation: a weekly hygiene report; `ready` requires criteria. Needs owner: N.

### TOWER-04 — Duplicates and conflicts
- Evidence: 108 near-duplicate open title pairs (token Jaccard ≥ 0.6), e.g. #5583 = #5591
  ("golden: typed CSV decode does not parse text cells as Int"), #5686/#5697, #5693/#5739.
  Conflicts: #3955 body still says "Cranelift for dev builds and rustc/LLVM for release"
  (Rust removed, #4717); #3955 cuts AI/ML while the owner goal lists AI/ML; #3421 is P1/no
  epoch although it carries an owner rule.
- Recommendation: merge duplicates; re-ballot #3955 scope; promote #3421. Needs owner: Y (#3955).

### TOWER-05 — Missing cards (proposed in the final section)
- install path (DOC-01), Core one-name purge (CORE-01), `__` export (CORE-02), dataframe
  (CORE-04), workspace-wide commands (LAY-04), lockfile location (LAY-02), `jet doc` output
  (LAY-03), shebang authority (SEC-01), doc-block CI (DOC-02), exact-division stop (DIAG-02),
  CLI/runtime text gate (CLI-04), comment provenance codemod (HEALTH-03), version constant
  (CLI-03), gauntlet re-baseline (PERF-01 — could be a step of #2858).

## Out of current scope (noted only)
- e9 jetos: 111 open cards, mostly frozen (#325, #839–#841 image variants are near-duplicates).
- e13 Cross Platform: 14 open cards; #1182 Windows ConPTY is `building` since 2026-09-29 with no
  newer log; macOS/Windows entropy and runtime matrices (#524, #526, #998) open. No non-Linux
  build exists to probe.

## Proposed Tower cards and ballots (priority order)

Nothing here was written to Tower. "Extend" means add criteria to an existing card rather than
create a new one.

### Cards

| # | Priority | Proposed card | Findings | Relation to existing cards |
|---|----------|---------------|----------|----------------------------|
| 1 | P0 | Land #5948 (unknown type names) with did-you-mean, ahead of other P0s | LANG-01 | existing #5948 (re-rank) |
| 2 | P0 | **User install path**: publish a seed-built `jet` + `jet-runtime/` tarball (Linux x64), a one-line installer, `packages.default` in `flake.nix`; the install page is generated from it and tested in CI | DOC-01 | new; relates #4154, D-SEED-DIST1 |
| 3 | P0 | Jet HTTP client (#3665) then native HTTP server (#4957) as one critical-path epic; acceptance = `Examples/features/net/http_server.jet` and `http_get.jet` run under `jet run` | CORE-03, PKG-01 | existing #3665, #4957 (group) |
| 4 | P0 | Promote #3421 (one top-level `.jet/`, D-GENERATED-STORAGE2=B) and add criteria: mono-workspace guard test asserts exactly one `.jet/`; `jet doc` writes `.jet/doc` and never overwrites non-generated files | LAY-01, LAY-03 | extend #3421 |
| 5 | P1 | `jet self lsp`: diagnostics, hover, go-to-definition from `jet check` data; editor extension wired | TOOL-01 | existing #5931/#4917 (merge) |
| 6 | P1 | **Not-built command census**: one `ported` bit in the command model; help, completions and man hide unported forms; each E2104 names its owning card | CLI-01 | new |
| 7 | P1 | Bare `jet` prints the greeting and five commands; `jet ?` falls back to the static palette without a TTY | CLI-02 | new |
| 8 | P1 | **Doc-block CI**: every ```jet block in README, Docs/spec/guides, reference and site is checked; fix the 12 failing blocks | DOC-02 | new; relates #3173 |
| 9 | P1 | Exact division by zero stops with E3010 (file, line, exit 70) on every tier | DIAG-02 | new |
| 10 | P1 | **Rendered-text guard** for CLI reports and runtime stops: no literal `\n`, `@N..N`, `source-root:`, `(I\d)`, `D-…`, raw `os error` in What/Why/Fix; fix secure-read and test-location messages | CLI-04, DIAG-05 | extend #3720 |
| 11 | P1 | Gauntlet re-baseline on the self-hosted compiler (all 27 cells, fresh receipt) | PERF-01 | step on #2858 |
| 12 | P1 | wasm32 target (`--target=wasm32-wasip2`) for run/build/test | TIER-01 | existing #1914 |
| 13 | P1 | Minimal in-process dependency path in `jet` (path + git deps, lock write, `jet add`); E1227 distinguishes "engine missing" from "skew" | PKG-01 | new (after ballot B5) |
| 14 | P1 | Core `__` items become `priv(package)`; the build fails on exported `__`; suggestion lists filter them | CORE-02, DIAG-04 | new; relates #4723 |
| 15 | P1 | Did-you-mean for methods and module items from the receiver's real surface; never suggest editing Core types or declaring built-in names | DIAG-04 | extend #3720 |
| 16 | P2 | Registry ratchet: every active row has a snapshot, a specific Why, and no decision IDs in user columns (179 / 192 / 497 today) | DIAG-05 | extend #3720 |
| 17 | P2 | `jet new` templates round-trip through `jet fmt --check`; formatter keeps blank lines | FMT-01 | new |
| 18 | P2 | `jet build` prints the artifact path and size, names the binary from `package.jet`, uses profile subdirectories | LAY-07 | new |
| 19 | P2 | Single version constant (CLI, scaffold, identity) | CLI-03 | new |
| 20 | P2 | Doctor runs real readiness probes; delete the legacy `/etc/jet/hangar` migration path | CLI-06, LAY-08 | new |
| 21 | P2 | Engine lookup is toolchain-relative only | CLI-08 | extend #4944 |
| 22 | P2 | Golden failures grouped by root cause; the 210 `golden:` cards close into ~15 cause cards | HEALTH-01, TOWER-04 | relates #4599 |
| 23 | P2 | Port the remaining `tests/*.rs` (23 files) before RustBridge removal; "Rust-free contributor check" exit gate | HEALTH-02 | existing #5453, #5482 |
| 24 | P2 | Codemod: remove `.rs` provenance comments (8,015 lines) and decision IDs from Examples and user docs; guard | HEALTH-03, DOC-03 | new |
| 25 | P2 | Keyword table generated from the parser; two-way test | LANG-05 | existing #4020 |
| 26 | P2 | `core.tasks`: `send` returns `SendError!`; one receive spelling | LANG-06 | extend #4654 |
| 27 | P3 | Short top-level help; `jet help <cmd>`; `jet help --all` | CLI-07 | extend #4913 |
| 28 | P3 | Display for digests (hex), DB handles; DateTime accessors; String hash overloads | CORE-05 | new |
| 29 | P3 | Examples use minimum grants; the unused-grant lint runs over Examples in CI | SEC-04 | new |
| 30 | P3 | Tower hygiene report: open cards without epoch/criteria, stale 30+ days, blocked by done cards, near-duplicate titles | TOWER-02/03/04 | new (tooling) |

### Ballots (owner decisions), in priority order

Each ballot names the alternatives, the trade-off, and my recommendation. Same-program examples
follow the AGENTS.md ballot rule.

**B1 D-INSTALL1 — distribution channel** (DOC-01)
- A: tarball + installer script + Nix flake package. Works everywhere, two artifacts to maintain.
- B: Nix only. One artifact, but Nix is a hard prerequisite for beginners.
- C: source only. Needs a seed and is not viable for beginners.
- Recommend A.

**B2 D-SCRIPT-AUTH1 — script authority** (SEC-01)
- A: a script-default set (IO, Env read, FS read/write under cwd and temp) for every single
  file; `#!` grants nothing extra; anything more needs an `authority:` block or `--allow`.
- B: `#!` grants allow-all only when executed directly, never under `jet run`.
- C: status quo.
- Recommend A.
- Beginner path: `print`, file and env work with no flags.
- Expert path: a narrow `authority:` block.

**B3 D-WORKSPACE-CMD1 — commands at a workspace root** (LAY-04)
- A: check/test/fmt/doc/build act on all members; run/dev keep the ambiguity error; `-p`
  narrows.
- B: an `--all` flag.
- C: `default-members:`.
- Recommend A, which matches cargo/go/pnpm.

**B4 D-LOCKFILE1 — lock location** (LAY-02, LAY-06)
- A: a root `jet.lock`, committed; `.jet/` fully disposable; `.gitignore` = `.jet/`.
- B: `.jet/lock`, committed, with machine state removed.
- C: amend D-LOCK1 to "never committed".
- Recommend A.

**B5 D-JPK-INPROC1 — minimal dependency resolution inside `jet`** (PKG-01)
- A: path/git deps and lock write in `jet`; the engine keeps store/env/image.
- B: everything waits for the U7 engine.
- Recommend A as the critical path.

**B6 D-CORE-ONENAME1 — one name per Core operation** (CORE-01)
- A: delete aliases; `jet fix` maps old names.
- B: hide aliases from the index.
- C: keep.
- Recommend A, per I8 and the greenfield rule.
- Needs a list review: the env, cwd, pid, list_dir, JSON, base64, process and heap families.

**B7 D-SURFACE-COMMON1 — five constructs** (LANG-02). One ballot, one row each:
- (a) a `**` power operator vs `.pow()` only;
- (b) general `defer stmt` vs resource-close only;
- (c) multi-value positional variants `Rect(Float, Float)` vs named only;
- (d) positional tuple destructure `(a, b) :: pair` vs by-name only;
- (e) `..` half-open plus `..=` inclusive vs today's inclusive `..` plus `..<`.
- Recommend: (a) yes, (b) yes, (c) yes, (d) yes, (e) half-open. These are the forms every
  arriving user types.

**B8 D-RECEIVER-INFER1 — infer `&` on mutable receivers** (LANG-03)
- Extends #3836: `c.bump()` is accepted when `c` is a local `:=` binding; `&` stays as an
  optional pin.
- Recommend yes.

**B9 D-FFI-ONE1 — one FFI form** (LANG-04)
- A: `#Import module c.<lib>` plus a bundled `jet cc`; delete `extern rust` (#5482) and inline
  C bodies.
- B: keep inline C as sugar.
- Recommend A.

**B10 D-FFI-UNSAFE1 — unsafe on imported foreign calls** (SEC-02)
- A: calls are unsafe unless the import row carries an audit reason.
- B: every foreign call needs `#Unsafe`.
- Recommend A.

**B11 D-OPTFLAG1 — one optimisation flag** (CLI-05)
- A: `--release` only, with profiles in `package.jet`.
- B: `--profile` only.
- Recommend A.

**B12 D-DATA-FRAME1 and re-scope #3955** (CORE-04)
- A: a first-party `core.data.frame` over the Arrow import; keep tensor/autodiff in Core;
  models live in packages.
- B: leave both to packages.
- Recommend A, because the owner goal names data and AI/ML.

**B13 D-REPL1 — REPL model** (CLI-02)
- A: compile-and-run REPL on the O0 build path. Each entry appends to a hidden unit; side
  effects are not replayed, because only the new statement runs.
- B: wait for native in-memory O0.
- Recommend A.

**B14 D-FOREIGN-TEACH1 — reopen D-S14-PAUSE as one registry table** (DIAG-01)
- Covers let/var/const, for, while, match/switch, comma enums, `x: T = v`, null/nil,
  println/console.log, `fn main`/def/func, and import/#include.
- Recommend yes.

**B15 D-FMT-COLLAPSE1 — formatter collapses short bodies?** (FMT-01)
- A: keep multi-line bodies as written.
- B: collapse single statements.
- Recommend A, which matches the docs.

**B16 D-MANIFEST-ONE1 — one manifest with sections** (LAY-05)
- A: `package.jet` sections (`workspace:`, `env:`, `build:`, `commands:`).
- B: keep the files; delete `pkg.jet` and one workspace form.
- Recommend B now and A later.

**B17 D-XDG1 — per-user paths** (LAY-08)
- A: XDG only.
- B: a single `~/.jet`.
- Recommend A.

**B18 D-TARGET-ORDER1** (TIER-01)
- Order: wasm32 → aarch64 (#4136) → embedded boards.
- e13 Cross Platform is out of current scope; the order is recorded only.

**B19 D-P0-CAP1 — Tower discipline** (TOWER-01/02)
- At most ~10 open P0 cards.
- Every open card has an epoch.
- Arrived epochs hold no open cards.
- Re-ballot #3955.

**B20 D-COMMAND-CUT1 — which unbuilt commands to delete rather than port** (CLI-01)
- Candidates: fold, split, try, test-compare, notebook, flash, review, generate, budget, prove.
