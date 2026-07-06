# Strict package graph plus workspace catalogs

**Card:** Tower #231. **Epoch 4.** **Scope:** planning slice for `D-WD3`.

## Goal

Package visibility is strict by default. A package can use only dependencies it
declares directly or receives through a ratified workspace catalog mechanism.
When a package is missing a dependency, Jetpack tells the user exactly which
`jet add` or catalog edit would make the graph valid.

## Current Ratified Law

- `D-WD3`: Jetpack package visibility is strict by default; workspace catalogs
  centralize shared versions.
- `S52`: `pkg.jet` owns package identity, `deps:`, `packages:`, and targets.
- `U10`: a package is a top-level `module`; one `pkg.jet` can contain multiple
  packages.
- `D-MONOREF1=A`: monorepo member refs support dot form, path form, and
  unambiguous bare names.
- `D-JPK-CHANNEL1=A`: channels resolve only on update or first add; the lock
  stays exact.
- `D-WD4`: the lock records owner package, policy, platform, provenance, and
  merge rationale.

## Vertical Slices

### T1. Visibility Checker

Build an internal package graph from manifest packages, direct deps, monorepo
members, inline script deps, and realized provider packages. For every import,
tool, service package, adapter input, and build tool, require a visible package
edge.

Exit: fixtures prove a transitive dependency cannot be imported unless directly
declared or exposed through the catalog model.

### T2. Catalog IR

Add an internal catalog IR before choosing the user surface. It maps a shared
logical name to a provider ref, version/channel rule, allowed packages, and
owning workspace. It does not weaken strict visibility; it only centralizes the
declaration a package may opt into.

Exit: checker can consume a synthetic catalog fixture and produce the same
resolved direct edges as handwritten package deps.

### T3. Lock Ownership

Record catalog resolution in `.jet/lock`: owner package, catalog source,
selected exact version, channel input if any, provider, platform, and rationale.
Merges preserve exact package ownership so two packages can use different
catalog entries without a hidden global upgrade.

Exit: lock round-trip preserves catalog rationale and merge diagnostics can name
the package whose catalog choice conflicts.

### T4. Diagnostics And Fixes

Replace "unknown package" or "module not found" failures in package contexts
with strict-graph diagnostics. The fix text chooses the smallest valid action:
add a direct dep for one package, add or update a catalog entry for workspace
reuse, or disambiguate a monorepo member.

Exit: diagnostics include what package asked, what name was used, why it is not
visible, and the exact next command/edit.

### T5. LSP And Search Facts

Expose visible packages and catalog candidates to LSP completion/hover and to
the discovery index from U26. Completion must not suggest packages that would be
invisible without a catalog or direct dep.

Exit: LSP fixture shows direct deps first, catalog candidates second, hidden
transitives absent.

## Acceptance Tests

- `strict_graph_rejects_transitive_import`.
- `strict_graph_accepts_direct_dep`.
- `catalog_edge_behaves_like_direct_dep_after_selection`.
- `lock_records_catalog_owner_and_rationale`.
- `catalog_merge_conflict_names_owner_package`.
- `missing_dep_fix_prefers_direct_add_for_single_package`.
- `missing_dep_fix_prefers_catalog_for_workspace_reuse`.
- `lsp_completion_hides_transitive_deps`.

## Dependencies

- Phase A filename/module reconciliation.
- Workspace continuation card #90, because workspace discovery decides catalog
  scope.
- Explainable lockfiles, because catalog rationale must be mergeable and
  inspectable.
- Federated providers, because catalog entries can point at multiple provider
  families.
- Migration importers, because importers should emit catalog TODOs when they
  detect repeated shared versions.

## Ratified Decisions

- `D-JPK-CATALOG1=A` — Catalog entries live in `workspace.jet` under
  `catalog:`; packages opt in through ordinary visible deps such as
  `deps: { http: catalog.http }`.
- `D-JPK-STRICTVIS1=A` — Strict visibility failures use dedicated diagnostics
  naming requester, hidden package, reason, and direct-dep or catalog fix.
