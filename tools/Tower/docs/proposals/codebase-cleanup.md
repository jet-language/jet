# Codebase cleanup & consolidation — proposal

**Status:** proposed 2026-07-01 · owner decision needed on §1 (ballot D-REPO-EXAMPLES1 in Tower) · §2–§5 need only greenlight
**Why now:** core syntax is about to lock. The repo grew organically through 3 epochs; stale artifacts mislead agents writing Jet, 258 examples under one flat directory are painful to navigate, and 68 separate test binaries obscure what is actually covered. One structural pass now gives a clean slate before Epoch 4.

Audit basis: four read-only sweeps (root/misc, docs, tests, examples) run 2026-07-01. Facts below are from those sweeps.

---

## 1. Examples — regroup `examples/features/` (owner ballot)

**Today:** 246 `.jet` files + 12 project dirs flat in one directory, numbered `01`–`197` with **44 duplicate-numbered files** (four different `100_*.jet`), 2 gaps, 6 unnumbered strays, plus 2 stale `migrations*/` cache dirs. Golden harness (`tests/golden.rs`) discovers by stem and matches `expected/{stem}.out`.

The numbering has already collapsed as an ordering device; it only survives as a uniqueness hack — and it isn't even unique anymore.

### Option A — topic directories, names only (recommended)

Drop numbers. Group by topic. `expected/` mirrors the tree. Golden harness goes recursive (one function change) and IDs tests by relative path.

```
examples/
  canon.jet                      # unchanged — the compiling syntax showcase
  features/
    basics/        hello.jet functions.jet values.jet branches.jet fizzbuzz.jet …
    types/         structs.jet enums.jet traits.jet generics.jet associated_types.jet typestate.jet …
    memory/        arena.jet arena_regions.jet rawptr.jet uninit.jet zerocopy.jet gc_cyclic.jet …
    effects/       capability_sigils.jet taint.jet pure.jet effect_prohibition.jet …
    comptime/      comptime_core.jet comptime_splice.jet reflect.jet doctests.jet …
    modules/       imports/ packages/ module_dir/ publishable_pkg/ selective_imports.jet …
    errors/        errors.jet panic.jet discard_fallible.jet rollback_trait.jet …
    collections/   lists.jet map_key.jet set.jet deque.jet filter_map.jet …
    text/          strings.jet regex.jet unicode_text.jet hex_base64.jet bigint.jet decimal.jet …
    serde/         json.jet csv_typed.jet serde_derive.jet user_derive.jet …
    io/            cli.jet stdin_filter.jet dir_entry.jet log_human.jet …
    net/           http_client.jet http_server.jet http_routes.jet …
    concurrency/   tasks.jet taskgroup.jet race_cancel.jet select_channel.jet deadline_context.jet …
    crypto/        crypto_envelope.jet crypto_sign.jet crypto_migration.jet …
    ui/            ui_view_tree.jet ui_typed_style.jet ui_component_kit.jet ui_a11y.jet …
    web/           web_hello.jet web_compute.jet ui_web_click.jet ui_showcase.jet …
    lowlevel/      lowlevel.jet ffi.jet cbind/ external_methods.jet sized_floats.jet …
    tooling/       bench.jet bench_target/ property_tests.jet debug.jet build_profiles/ …
```

Running one: `jet run examples/features/net/http_server.jet` — tab-completion does the navigation the numbers never did. Learning order lives in one `examples/README.md` index (basics → types → …), which is cheaper to maintain than 250 filename prefixes.

### Option B — topic directories, per-topic numbering

Same tree, files keep two-digit order prefixes inside each topic: `basics/01_hello.jet`, `basics/02_functions.jet`. Preserves a guided path in `ls` output; costs renumber churn every time an example lands mid-sequence (the exact failure mode that produced today's 44 collisions, now per-directory).

### Option C — stay flat, renumber once

One-time renumber `001`–`254`, fix collisions, keep flat. Smallest diff; solves duplicate IDs, solves nothing about navigating 258 files, and re-decays immediately.

**Recommendation: A.** Numbers already failed twice; topics are how everyone (owner, agents, tests) actually looks things up. B if the owner wants a visible learning order in the filesystem itself.

**Mechanics (any option):** golden/dev/cli harnesses take stems from relative paths; hardcoded stem lists in `golden.rs:57–59,149–155` and `dev.rs` update in the same commit; `expected/` moves with its sources; `jet fmt` round-trip on every moved file; full suite green before and after. Stale `examples/migrations*/` dirs deleted.

---

## 2. Tests — 68 binaries → ~55, one shared harness (greenlight)

Each `tests/*.rs` is its own compiled binary. Audit found clear merges, all coverage-preserving:

| Merge | Into | Why safe |
|---|---|---|
| `web_dev.rs` + `web_dev_fn.rs` | `web_dev.rs` | near-identical spawn/TCP/rebuild harness, two entry modes |
| `debug.rs` + `debug_native.rs` | `debug.rs` | same feature, interpreter vs lldb tier — sections in one file |
| `ffi.rs` (93 ln smoke) | `cffi.rs` | one example fast-path case inside the C-FFI matrix |
| `pub_package.rs` (105 ln) | `pkg.rs` | package-visibility cases belong in the package suite |
| `m3_lexer_smoke.rs` + `m3_parse_smoke.rs` | `grammar.rs` | M3-era smokes, subsumed by grammar suite; keep the cases, kill the binaries |
| `ui.rs` + `lint_snapshots.rs` | `diagnostic_snapshots.rs` | same `UPDATE_EXPECT` snapshot harness, `.stderr` + `.warn` extensions |
| `canon.rs` + `small.rs` + `ga.rs` | `release_gates.rs` (with `release.rs`) | milestone gate checks, one lane |

Plus: shared `tests/common/mod.rs` for the `build_and_run` helper duplicated across `tir.rs`/`dev.rs`/golden-style suites. Big feature files (`tir.rs` 4.8k, `pkg.rs` 1.9k, `jetpack.rs`, `lsp.rs`) stay separate — feature-scoped and CI-parallel. `hardening.rs`/`truthfulness.rs`/`ice_regressions.rs` stay — they are regression ratchets, not milestone leftovers.

Rule going forward: **a new test file requires a new subsystem**, otherwise the case joins the existing suite.

---

## 3. Docs — kill the traps (greenlight)

The docs audit found the specific items that mislead agents:

1. **`docs/spec/spec.md` teaches retired syntax** — dotless struct literals at lines 282/309/333 (`Player { hp: 100 }`), retired by D-DOTCTOR2 (E0320). Fix to `T.{ }`.
2. **`docs/reference/syntax-comparison-proposal.md` (2,050 lines, unratified)** sits in the user-facing reference tree where it reads as current syntax. Move to `tools/Tower/docs/proposals/` (agents already know reference/ as ground truth — this is the single worst trap found).
3. **`spec.md` M10 duplicates `docs/reference/core-library.md`** (~180 lines of stdlib API). Per single-source rule: shrink M10 to a shipped-summary + link; core-library.md is canonical.
4. **`AGENTS.md` duplicates `CLAUDE.md`** (legacy Codex manual, drifting). Replace body with a 3-line pointer to CLAUDE.md.
5. `docs/README.md` index states plainly: syntax facts live in `syntax-decisions.md` only.

Not touched: `syntax-decisions.md` history (`Ptr<T>`, `pack.jet` mentions are the decision log — correct as history), `formal-core.md` (clearly marked deferred), `diagnostics.md` (clean).

---

## 4. Root & misc hygiene (greenlight)

- `.gitignore` += `editors/zed/grammar-repo/` — generated nested repo, hundreds of git-status lines (known: install.sh artifact).
- `crates/jet-jit/src/{collections,concurrency}.rs` → PascalCase, matching every other crate.
- `tools/web-serve.mjs` — superseded by `jet dev --target=web` (`Source/CmdDevWeb.rs`); delete once web_dev suite is the verified path.
- `examples/test.jet` (4-line scratch) — fold into golden smoke or delete.

Everything else is already clean: build/target/result properly ignored, no orphan fixtures, no *.bak/scratch files, Source/↔crates/ seam split coherent.

---

## 5. Syntax-lock sweep (after current ballot round ratifies)

The moment the open round (25 ballots) is decided, run one sweep so the clean slate is also *correct*:

- re-verify every example + doc code block against final ratified syntax (the audit already confirmed examples are compliant with today's ratified set; the new round may change spellings — e.g. D-MARKER-FAMILY1 redraws `@`/`#`/`$`);
- regenerate `canon.jet` to cover every newly ratified form;
- `jet fmt` stability pass over the full example tree (formatter round-trip is the ratchet that catches dropped tokens);
- prune ratified items out of ballot docs per house rule.

---

## Sequencing

1. §3 + §4 immediately (no decisions needed, pure fixes) — small commits, full suite green each.
2. §2 test merges — one cluster per commit, targeted suite per merge, full suite at end.
3. §1 after owner picks A/B/C — one mechanical commit (moves + harness discovery + expected/ mirror), full suite + golden green.
4. §5 after ballot round ratifies.

Nothing here changes language surface; only §1 needs a ballot because the owner owns repo ergonomics he touches daily.
