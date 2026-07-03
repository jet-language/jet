# Workspace continuation review (#90 / c1iixish / c156)

Continuation review, not a fresh design. Verifies what card #90 claims is
done, re-checks the "capstone linker ICE in logbook" note from the
2026-06-29 log, and scopes exactly what's left. Cross-reference:
`tools/Tower/docs/sidequests/monorepo-workspace.md` (older, staged plan —
partly stale, see "Sidequest doc drift" below).

## Verified shipped (read-only test run, 2026-07-03)

Ran targeted suites, no code changes:

```
nix develop -c cargo test --test workspace   # 12/12 pass
nix develop -c cargo test --test jetpack     # 31/31 pass
nix develop -c cargo test --test pkg --test ring_layer   # pkg 71/71, ring_layer 9/9
```

Confirmed in code:

- `module workspace { members: … }` in `workspace.jet`: `[]`, explicit
  string lists, `find("./packages")` — `crates/jet-driver/src/Jetpack/WorkspaceFile.rs`.
- Unified lock: `crates/jet-driver/src/Jetpack/WorkspaceLock.rs` writes
  `[[workspace_member]]` into `.jet/lock` (`Syntax::UNIFIED_LOCK_FILE`) — this
  answers the sidequest doc's **Q2** (dedicated `.jet/workspace.lock` vs
  folding into the unified lock): **folded into unified lock**, matching the
  doc's own recommendation. The sidequest doc still frames Q2 as open —
  it's stale, not the current implementation.
- **Q1** (members: grammar shape) is also settled by what shipped: a bare
  list expression (`Vec<WorkspaceMember>`), no typed `Workspace { … }`
  record wrapper. Matches the sidequest doc's recommendation (a).
- D-MONOREF1 dot-form addressing (`source.package`) is implemented and
  tested: `crates/jet-driver/src/Jetpack/RefSpec.rs::classify_in` (dot-form
  branch), tests `dot_form_classified_when_source_is_declared` /
  `dot_form_not_classified_when_source_is_unknown` / `colon_form_still_works`
  in `tests/workspace.rs`.
- Migration: `jetpack.toml` `[packages]` retired, emits E1225 —
  `crates/jet-driver/src/Jetpack/ManifestTOML.rs`.

## "Capstone linker ICE in logbook" — not reproduced, likely stale

The 2026-06-29 log entry says a full `cargo test` run reached "an unrelated
capstone linker ICE in logbook" after the D-WORKSPACELOCK1 slice landed. No
file, example, or test fixture named `logbook` or `capstone` exists in the
tree today (`grep -ri logbook`/`capstone` across the repo turns up only two
unrelated string literals in `tests/tir.rs` fixtures — a comptime-const demo
and a foreign-enum-matching demo, neither a showcase program). E2-M17 GA
retired all 6 showcase programs from `examples/` before this note was
written. All workspace/jetpack/pkg/ring_layer suites are green. Conclusion:
either the ICE was in a scratch fixture the agent built and discarded during
verification, or it was in a showcase since retired. Not reproducible from
the current tree; not blocking this card. If it resurfaces, it is a P0 (I2)
regardless of workspace scope and gets its own card.

## Sidequest doc drift (tools/Tower/docs/sidequests/monorepo-workspace.md)

Not editable under this task's write scope (plans/epoch-4/ only), flagging
for whoever next touches it:

- Q1 and Q2 (above) are answered by the shipped code; the doc still lists
  them as "Remaining Owner-Q."
- Its "hard gate" (§Sequencing point 1) says D-CTMARKER1/D-CTEFFECT1 are
  "ratified, not implemented." **Both are now implemented** — `comptime { … }`
  block parses (`crates/jet-parser/src/Parser/Statements.rs`), formats
  (`crates/jet-parser/src/Formatter/Statements.rs`), and is wired through
  sema (`crates/jet-sema/src/Sema/{CheckerCore,Captures,Bundle,Effects,
  Capability,State}.rs`) and codegen (`crates/jet-codegen/src/Codegen/TIR/
  {subset,lower}.rs`); a working example ships at
  `examples/features/comptime/comptime_block.jet`. **The hard gate is
  cleared** — Stage 1's remainder (below) is now pure engineering, not
  blocked on any upstream decision.
- Its proposed diagnostic codes E1221/E1222 collide with an already-shipped,
  unrelated E1221 (`D-EFFBUDGET1`, malformed `effects:`/`grants:` block —
  `crates/jet-driver/src/Manifest.rs`). Next free jetpack code is E1226+
  (E1225 is the last used, per `crates/jet-driver/src/Jetpack/ManifestTOML.rs`
  doc comment) — verify against the registry at implementation time.

## Remaining slices (no open owner decision on any of them)

D-WORKSPACE1=B and D-MONOREF1=A already ratify the full target surface
(arbitrary comptime `members:`, dot/path/bare addressing, index-first sparse
fetch). Nothing below needs a ballot — this is implementation-ready.

### Slice A — wire comptime globals into `members:` evaluation

File: `crates/jet-driver/src/Jetpack/WorkspaceFile.rs::eval_members_expr`.

Today it calls `crate::Comptime::evaluate(expr, &HashMap::new(), &HashSet::new(), base_dir, &HashMap::new())`
— empty `funcs` and empty `globals`. That means a `members:` expression can
only use inline literals + the special-cased `find("./dir")` fast path; any
reference to a top-level `comptime X = …` binding declared elsewhere in
`workspace.jet`, or a call to a top-level `fn`, silently has nothing to
resolve against.

Work:
1. In `evaluate()`, before calling `eval_members_expr`, walk `program.items`
   for top-level `Item::Const`/comptime bindings and `Item::Fn`, evaluate
   bindings in source order (same pattern `CheckerCore.rs` uses for module
   fields — see D-CTMARKER1 comptime-block wiring for the pattern to mirror),
   build `funcs`/`globals` maps, thread them into `eval_members_expr`.
2. Add the D-CTEFFECT1 Tier-1 effect plumbing (`find`, `fetch(url, sha256:)`,
   `@embed`) so a `members:` expression that uses them gets hash-recorded
   into `.jet/lock` the same way other Tier-1 comptime call sites do
   (`crates/jet-comptime/src/Comptime/Methods.rs` has the existing tier
   dispatch to extend, not duplicate).
3. Tests: `tests/workspace.rs` — a `members:` expression referencing a
   sibling `comptime` const, a `comptime { … }` block, and a Tier-1
   `find(glob)` call (note: `D-CTFIND1/2`'s general `find(glob) -> [String]`
   builtin is itself still unbuilt per `docs/spec/syntax-decisions.md`
   marker "*(ratified, unbuilt — c157)*" — Slice A should either wait for
   c157 for that specific builtin or scope its own test to a `find("./dir")`-shaped
   expression composed with a comptime const, which doesn't need c157).

### Slice B — bare/path addressing against the workspace index

Files: `crates/jet-driver/src/Jetpack/RefSpec.rs`, `Output.rs`.

`RefError::AmbiguousBare` is declared and has a rendered error message in
`Output.rs:130`, but nothing ever constructs it — grep confirms the only two
occurrences of `AmbiguousBare` are the enum definition and its match arm. No
function resolves a bare name (`logging`) or a path-style sibling ref
(`infra/logging`) against a workspace member index today. A bare name with
no `:` or matching `.` currently falls through to `RefError::MissingSeparator`.

Work:
1. Build the in-memory workspace index (`{package-name → root, source-name →
   members}`) from `WorkspacePlan` (already returned by
   `WorkspaceFile::evaluate`) — this doesn't exist as a queryable structure
   yet, only as the flat `Vec<WorkspaceMember>` used for building.
2. Extend `classify_in` (or a sibling function taking the index) to try,
   in order: colon form (unchanged) → dot form (unchanged, already works)
   → path form (`infra/logging`, exact relative-path match against the
   index) → bare form (exact name match; `AmbiguousBare` on >1 match,
   new `UnknownMember`-shaped error on 0 matches).
3. New diagnostics (verify next-free code at build time, E1226+): ambiguous
   bare match (list candidates + suggest dot/path form) and unknown member
   (did-you-mean over index names).
4. Tests: `tests/workspace.rs` + `RefSpec.rs` unit tests for path-form and
   bare-form classification, ambiguous and unknown cases.

### Slice C — index-first resolution + sparse subtree fetch

File: `crates/jet-driver/src/Jetpack/Provider.rs::fetch_remote_repo` (full
clone today, confirmed — no `--filter`/sparse-checkout invocation anywhere
in the function). The only partial-clone codepath in the file is the
existing kind-probe `remote_has_pack_jet` (`git init` + `--filter=tree:0` +
`git ls-tree`, peek-only, no checkout) — this is the seed to generalize.

Work:
1. Make `realize()`/`resolve_kind()` index-first for a monorepo source:
   fetch the source's `workspace.jet` (or its manifest) first, read the
   member index (Slice B's index), then materialize only the requested
   package's subtree.
2. Generalize the `remote_has_pack_jet` partial-clone probe into a real
   sparse checkout: `git init` + `--filter=tree:0` +
   `git sparse-checkout set <subtree-path(s)>`, walking the member's
   `pkg.jet` deps against the workspace index for transitive in-repo deps.
3. Full-clone fallback when the provider lacks sparse/partial-clone support
   (reuse today's `fetch_remote_repo` path).
4. New diagnostics: sparse fetch failed + full-clone fallback also failed
   (network/provider); a transitive in-repo dep names a member outside the
   workspace.
5. Tests: resolver/sparse-fetch integration tests against a local bare git
   repo fixture (pattern already used by `git_dep_local_bare_repo_fetches_ok`
   in `tests/pkg.rs`).

### Slice D — example, golden, docs close-out

- Runnable monorepo example under `examples/` (or extend
  `examples/features/*/workspace/` if one already exists — check at
  implementation time) with ≥2 members, one addressed by `source.package`,
  one by path form, demonstrating a build that materializes only the
  addressed subtree.
- Formatter round-trip + STABILITY test for any new syntax surface touched
  (there shouldn't be new syntax — Slices A-C are semantics on the already-
  ratified surface — but re-verify per the formatter-round-trip rule if
  anything in `Syntax.rs` changes).
- `docs/spec/spec.md` + `docs/spec/syntax-decisions.md`: mark D-WORKSPACE1/
  D-MONOREF1 fully implemented once Slices A-C land; `docs/spec/diagnostics.md`:
  register the new E-codes with what/why/fix rows + ui snapshots (I4).

## Ballot check

No open owner decision blocks any of Slices A-D. D-WORKSPACE1=B and
D-MONOREF1=A already ratify the exact surface (arbitrary comptime in
`members:`; dot/path/bare addressing; index-first sparse resolution). No new
ballot to raise for this card.

## Phase recommendation

**ready** — unblocked, 100% implementable as Slices A→B→C→D (strictly
ordered: index feeds addressing feeds sparse fetch). Card's current
`building` phase is accurate; the sidequest doc's stale "hard gate" was the
only thing that could've read as a blocker, and it's cleared (see above).
