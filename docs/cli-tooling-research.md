# CLI & language tooling research (2026-06-12, unratified)

Status: exploratory research, **not ratified**, decides no syntax or
semantics. A survey of the most loved CLI tools and language toolchains
— what makes them loved, and which ideas Jet could adopt. Greenfield
thinking encouraged; every adoption still passes through the normal
decision protocol (docs/02) and the simplicity ratchet (I8).

Organizing claim: the most loved tools share five traits — **fast,
zero-config, beautiful by default, scriptable when piped, and they
teach you as you use them**. Jet's philosophy already commits to four;
the survey below is mostly concrete techniques for delivering them.

---

## Part 1 — Beloved general CLI tools and why

### Speed-as-UX tier

- **ripgrep (rg)** — speed *is* the feature, but the loved part is
  sane defaults: respects .gitignore, skips binaries, colors matches,
  needs zero flags for the common case. Lesson: *the default
  invocation should be the expert invocation.*
- **fd** — find with human syntax. Lesson: ergonomic argument order
  (`fd pattern` not `find . -name`), smart case sensitivity (case-
  insensitive until you type a capital).
- **uv / bun / ruff** — each won a crowded space purely on being
  10–100× faster than the incumbent with a compatible interface.
  Lesson: speed converts users even when features are equal; "it's
  instant" is a product moat. Directly validates the `jet dev`
  rapid-prototype direction (docs/05, committed addition 10).
- **hyperfine** — benchmarking with statistical rigor (warmup runs,
  outlier detection, "Did you forget to warm up?" warnings) and
  beautiful comparative output. Lesson candidate: `jet bench` should
  be statistically honest out of the box, not a naive timer.

### Visual-excellence tier

- **bat** — cat with syntax highlighting, git gutter marks, automatic
  paging that *gets out of the way when piped*. Lesson: detect TTY
  and degrade gracefully — beautiful interactively, plain bytes in a
  pipe. Never make scripts parse ANSI.
- **delta / difftastic** — diffs that understand syntax (delta:
  highlighting + word-level diff; difftastic: AST-level structural
  diff). Lesson for Jet: we own a parser — `jet fmt --check` and
  future tooling can show *structural* diffs, and test failures can
  diff values structurally instead of textually.
- **eza, lsd, dust, duf, procs, btop** — the "modern replacement"
  family: color used to encode meaning (not decoration), columns
  aligned, units humanized. Lesson: a small consistent visual
  language across all `jet` subcommands beats per-command cleverness.
- **starship** — instant prompt, useful info only when relevant
  (shows Rust version only inside a Rust project). Lesson:
  context-sensitivity — print what matters *here*, omit the rest.

### Interaction-model tier

- **fzf** — one primitive (fuzzy pick from a stream) composable
  everywhere. Lesson: interactive selection as a *fallback* when an
  argument is omitted — e.g. a future `jet test` with no args in a
  TTY could offer a picker; with args or piped, fully scriptable.
- **gh (GitHub CLI)** — dual-mode done right: interactive prompts in
  a TTY, `--json` + flags for scripts, same command. Lesson: every
  Jet command should have a `--json` story (M13 needs machine-
  readable diagnostics anyway; extend the principle to fmt/test/build).
- **Charm suite (gum, glow, bubbletea)** — proves the terminal can
  carry real product polish: markdown rendering in-terminal (glow),
  styled components, spinners and progress that feel native. Lesson:
  `jet explain`/docs output can render markdown-quality typography in
  the terminal rather than dumping plain text.
- **lazygit / k9s / atuin** — full TUIs over complex state, loved
  because they reveal structure that flags can't. Worth holding in
  reserve: a TUI is heavy machinery; Jet should earn it (e.g. a test
  watch mode) rather than lead with it.
- **just / mise / direnv / watchexec** — the "project ergonomics"
  belt: task running, toolchain pinning, env loading, file watching.
  Lesson: Jet should *absorb the watch loop* (`jet dev`) and
  *toolchain pinning* (a `jet-version`-style pin honored by the
  launcher, rustup-style) so users never need these for Jet projects.

### Conventions the loved tools all share (adopt wholesale)

- TTY detection: color/progress/interactivity only when attached;
  plain deterministic output when piped or in CI.
- `NO_COLOR`, `FORCE_COLOR`, `--color=always|never|auto` respected.
- OSC 8 terminal hyperlinks — error codes and file:line locations can
  be *clickable* in modern terminals (iTerm2, WezTerm, Windows
  Terminal, VS Code). Almost nobody does this yet; cheap differentiator.
- "Did you mean" on typo'd subcommands/flags (clap-style). Jet already
  has the S14 teaching-error muscle; apply it to the CLI itself.
- Generated shell completions (bash/zsh/fish) and man pages from one
  source of truth.
- Help text with *examples first* (tldr-style), not flag dumps. The
  most-loved help pages lead with three copy-pasteable invocations.
- Exit codes documented and stable; stderr for diagnostics, stdout for
  product output, never mixed.
- First run is a moment: `jet` with no args should greet, orient, and
  show the three commands that matter — not print a usage error.

---

## Part 2 — Language tooling: the lauded features

### Diagnostics (Jet's home turf — steal the best, stay ahead)

- **Elm** — still the gold standard: errors as prose, written in
  second person, with the *actual fix* shown, and links to deeper
  explanation. Jet's docs/04 voice is already Elm-school; the
  remaining Elm trick to steal is **links out** — every error
  carrying a URL (or `jet explain` pointer) to a longer-form page.
- **Rust** — error codes + `rustc --explain E0382` (a full essay with
  runnable examples, offline), multi-span labels (primary span +
  "value moved here" secondary spans), and machine-applicable
  suggestions that power `cargo fix`. Jet equivalents: `jet explain
  E0203` (the M14 error-code index, but *in the terminal*, offline);
  structured suggestions tagged machine-applicable so `jet fix` /
  LSP quick-fixes share one engine (M13 already plans this — protect
  it).
- **Zig** — error *return traces* (the path an error value traveled,
  distinct from a stack trace) and comptime call traces. Jet analog:
  when a `?`-propagated error surfaces in a runtime report, show the
  propagation chain. Also directly applicable to M9.5 comptime panic
  diagnostics (already planned with call traces — keep).
- **Clang fix-its / gcc's learned politeness** — precise insertion
  suggestions. Table stakes now; Jet has the machinery via snapshots.
- **GHC/Idris/Lean hole-driven development** — typed holes: write
  `?hole`, compiler tells you the type needed and candidates that
  fit. Out-of-box candidate for Jet: a `todo` expression that
  compiles (panics at runtime) and *reports its expected type* — a
  beginner-friendly typed hole with no theory attached.

### The one-tool toolchain (cargo/go/deno school)

- **cargo** — the most-loved feature of Rust per survey after the
  language itself. One binary: build/test/run/doc/publish. Custom
  subcommand discovery (`cargo-foo` on PATH becomes `cargo foo`) let
  an ecosystem grow without bloating core. Jet candidate: same
  convention (`jet-foo` → `jet foo`) — zero-cost extensibility that
  keeps I8 intact.
- **go** — `gofmt` proved zero-config formatting wins arguments
  permanently (Jet: already committed, M6). `goimports` — *imports
  are managed for you*: reference a name, the tool adds the import.
  Beloved, invisible, and a strong fit for Jet's beginner priority
  once M6 multi-file lands (fmt or LSP adds/removes `import` lines).
  `go vet` as a separate "likely bugs" pass; Jet's lint family
  (L-codes) already covers this — keep them in the compiler, not a
  separate tool.
- **deno** — single executable containing fmt/lint/test/bench/doc/
  compile; `deno compile` produces a self-contained binary (Jet gets
  this natively). Permission flags (`--allow-net`) made *capability
  visibility* a CLI feature; a long-horizon Jet idea: since the
  compiler knows a program's std imports, `jet build` could print a
  capability summary ("this program reads files and opens sockets")
  — cheap, honest, unique.
- **rustup** — toolchain versions and components managed invisibly;
  `rust-toolchain.toml` pins per-project. Jet should plan the
  launcher/version-manager story *before* 1.0 so upgrades never
  break a team (pairs with docs/enterprise.md item 1).
- **flutter doctor** — the single most-praised onboarding feature in
  cross-platform tooling: one command that checks the environment,
  diagnoses, and prints exact fixes. Jet candidate: `jet doctor`
  (is rustc reachable, cache healthy, PATH sane, version current).
  Especially relevant since Jet hides a rustc dependency.

### Testing UX

- **jest watch mode** — interactive: re-runs only affected tests on
  save, filter by name/file with single keystrokes, "press u to
  update snapshots." The loop feels like a conversation. Fits the
  `jet dev` foundation directly.
- **insta / expect-test (Rust)** — snapshot testing with one-key
  blessing. Jet already lives this internally (UPDATE_EXPECT); ship
  the same power to users in `jet test` (S-decision needed for
  surface syntax).
- **Hypothesis / QuickCheck** — property testing with automatic
  *shrinking* (the failing case is minimized before display) and a
  failure database (regressions re-tested forever). Post-v1
  candidate; shrinking is the part users actually love.
- **Elixir doctests** — examples in doc comments *are* tests; docs
  cannot lie. This is Jet invariant **I5 generalized to user code**
  — arguably the single best philosophical fit on this list.
- **cargo-nextest** — clean per-test process isolation, beautiful
  summary, flaky-test retry policy. Visual reference for `jet test`
  output.
- **Mutation testing (Stryker, cargo-mutants)** — niche, but the
  feature experts cite for "does my test suite actually test
  anything." Far-horizon.

### Interactive / live development (the niche-but-lauded shelf)

- **Smalltalk/Pharo** — image-based: inspect and *fix code inside the
  debugger, then resume*. Too radical for Jet, but the takeaway —
  the error moment is a teaching moment with full state available —
  motivates richer runtime panic reports (locals visible, values
  printed) in dev mode.
- **rr (record & replay) / time-travel debugging** — record once,
  replay deterministically, step *backwards*. Jet's no-unsafe,
  no-shared-mutable-state model makes deterministic replay more
  tractable than in C/C++; a far-horizon differentiator for the
  debugger story (docs/enterprise.md item 2).
- **Elm reactor / elm-live & Flutter hot reload** — sub-second
  feedback transformed both communities' DX reputations. `jet dev`
  phase 1 (interpreter) is the same bet; survey evidence says it pays.
- **Unison** — content-addressed code: definitions hashed, renames
  free, no builds, no merge conflicts in the codebase manager. Not
  adoptable wholesale, but one idea travels: *Jet owns the whole
  semantic model* (R1–R7), so future tooling (rename, API diff) can
  be semantic, not textual.
- **elm diff** — computes the API diff between package versions and
  *enforces semver from it* (breaking change → major bump required —
  the tool won't publish otherwise). The single best package-registry
  feature surveyed; a perfect M12 Phase 2 candidate since Jet's sema
  knows every public signature.
- **Compiler Explorer (godbolt)** — beloved expert tool: see what the
  compiler emits. Jet analog: `jet emit --rust file.jet` showing the
  generated Rust, explicitly framed as an expert/curiosity feature.
  Tension with I2 (rustc never speaks to users) is manageable: this
  shows *our output*, not rustc's words. Owner call.
- **Go/Rust playgrounds** — a shareable run-this-snippet web page
  drives adoption, docs, and bug reports. M14-adjacent; the
  interpreter (M9.5/dev mode) makes a sandboxed playground cheap.
- **rustlings / Tour of Go / exercism** — guided in-terminal
  exercises ("fix this broken program, the compiler will coach
  you"). For a language whose *product is diagnostics*, `jet tour`
  or `jet learn` is the highest-leverage onboarding asset possible:
  the error messages are the teacher.

### Docs & discovery

- **rustdoc + docs.rs** — docs generated from source, every published
  package documented automatically, examples tested. Jet: `jet doc`
  post-M6 with doctests (see Elixir above) and auto-published docs
  when the M12 registry exists.
- **`go doc fmt.Println` in the terminal** — instant offline API
  lookup without leaving the shell. Cheap once `jet doc` exists.
- **man pages + `--help` + tldr-style examples** generated from one
  source (clap/cobra do this). One definition, three surfaces.

---

## Part 3 — Synthesis: ranked shortlist for Jet

Items that fit docs/00 priorities, keyed to where they'd land.
None of this starts without a roadmap slot or owner sign-off (I8).

**Near-term (M6 era — mostly cheap, high leverage):**

1. CLI conventions bundle: TTY-aware color/progress, NO_COLOR,
   `--color`, "did you mean" for subcommands, examples-first help,
   stable exit codes, friendly no-args greeting. (Days of work,
   permanent goodwill.)
2. `jet explain <code>` — offline terminal essays for every E/L code;
   the M14 error index, surfaced where users actually are. Errors
   print "run `jet explain E0203` to learn more."
3. OSC 8 hyperlinks on file:line and error codes.
4. `jet doctor` — environment self-diagnosis (hidden rustc, cache,
   PATH, version).
5. Generated shell completions + man pages.
6. `goimports`-style import management in fmt/LSP once S16 lands.

**Mid-term (M9.5–M13 era):**

7. `jet dev` watch loop with jest-class interactivity (already
   committed — this survey just raises the bar for its UX).
8. Doctests — examples in doc comments run under `jet test` (I5
   extended to user code; needs a ballot).
9. Snapshot testing with one-key bless in `jet test`.
10. Typed-hole `todo` expression that reports its expected type.
11. Error propagation traces in runtime reports (Zig-inspired).
12. `jet bench` with hyperfine-grade statistics.
13. External subcommand discovery (`jet-foo` → `jet foo`).

**Registry-era (M12+):**

14. **elm-diff-style enforced semver** on publish — flagship registry
    feature; sema already knows every public signature.
15. Capability summary at build time from std imports (deno-inspired,
    honesty feature).
16. Auto-generated, auto-published docs for every package.

**Far horizon (post-v1, needs plans):**

17. `jet tour` / `jet learn` — rustlings-class guided exercises where
    the diagnostics are the teacher.
18. Playground (interpreter-backed, sandboxed, shareable).
19. Deterministic record/replay debugging — the ownership model makes
    this unusually feasible; potential headline differentiator.
20. `jet emit --rust` expert window (owner call re: I2 framing).
21. Property testing with shrinking; mutation testing.

**Explicitly not recommended:** config files for any tool (the loved
tools won by *removing* config); a TUI for its own sake; plugin
systems beyond PATH-discovered subcommands; telemetry of any kind
without explicit opt-in.
