# capabilities

## Ratified

- **D-EFF4 / D-EFF5** — the grantable roots are the closed set `Net`, `FS`, `IO`, `DB`, `Time`, `Rand`, `Env`, `Exec`, `Log`, `GPU`, `FFI`, `Browser`, and `Secret`; `Panic` and `Mem` are deny-only, not grantable roots. `FFI` owns language leaves, and old flat FFI language roots are retired. — `docs/spec/syntax-decisions.md:2512-2520`
- **D-EFFTREE1** — a grant/effect may be a dotted path such as `FS.Read` or `Net.HTTP.Get`; roots are closed, leaves are open/user-chosen, and ancestor matching is subsumption. `-[FS]>` accepts `FS.*`, while `-[FS.Read]>` does not authorize `FS.Write`. — `docs/spec/syntax-decisions.md:2522-2532`
- **D-EFFECT-DECL1=A** — `effect FS.Read` is a compile-time package declaration; once a root declares leaves, dotted uses must name a declared leaf exactly, while bare roots stay valid. — `docs/spec/syntax-decisions.md:2534-2541`
- **D-AUTHORITY-MODEL1=A** — effects, caps, policy, budgets, trust, sandbox, REPL, build, and boundary checks read “one rights tree, holds relation, tighten rule and gate record.” — `docs/spec/syntax-decisions.md:6987-6998`
- **D-AUTHORITY-ROOTS1=A** — the authority table has thirteen roots, with FFI language leaves such as `FFI.Go`; flat language roots are deleted. — `docs/spec/syntax-decisions.md:6999-7015`
- **D-AUTHORITY-MEM1=B / D-AUTHORITY-MEM2=A** — memory floors are effect denials such as `-[!Mem.Alloc]>` and manifest `authority: .{ holds: { deny: [Mem.Alloc] } }`; denials may carry `above: Bytes`. — `docs/spec/syntax-decisions.md:7016-7025`
- **D-AUTHORITY-NAME1=A** — `Authority` is “the one nameable rights value at process, plugin and session boundaries.” — `docs/spec/syntax-decisions.md:7026-7029`
- **D-AUTHORITY-SCOPE1=A** — one scoped block marker is used; a bare list narrows (`#FX(FS, Net)`) and a name-before-list binds a handle (`#FX(g: FS, Net)`). — `docs/spec/syntax-decisions.md:7030-7034`
- **D-AUTHORITY-MANIFEST1=A** — one `authority:` block holds package bounds, dependency grants, trust defaults, and provider bounds; replaced keys are migrated and deleted. — `docs/spec/syntax-decisions.md:7035-7038`
- **D-AUTHORITY-GATE1=A / D-ABILITY-NAME2=A** — every authority gate records in one provenance ledger and `jet inspect authority` reads the rights view; `#FX` is the expert source boundary, `Authority` is the rights value, and `Effect` is the fact menu. — `docs/spec/syntax-decisions.md:7043-7057`
- **D-ONCE-SANDBOX1=A** — “the isolated package kind is `target: sandbox`; `target: plugin` retires as a rename.” — `docs/spec/syntax-decisions.md:7583-7584`

## Shipped

- Manifest-less applications receive ambient `IO`, `Mem.Alloc`, and `Exec`; an explicit `package.jet` replaces this with `authority.holds`, while the floor still does not authorize filesystem, network, process control, or other effects and those stop with E1803. — `docs/spec/spec.md:3389-3397`
- `process.workspace()` and `Authority.from_rights([...])` produce explicit authority values, including resource-scoped grants such as `FS.Read:repo`, `FS.Write:.jet/build`, and `Exec:/usr/bin/cargo`; planning and launch record the exact policy and never fall back to ambient authority. — `docs/reference/core-library.md:1556-1585`
- The reference surface documents `target: sandbox`, isolated wasm32 Component Model packages, generated WIT/wasm, typed `plugin.load(path)` calls, homogeneous `Int`/`Float`/`Bool`/`Text` exports, zero host imports, and E1257–E1260 diagnostics. The worked proof is `examples/features/packages/sandbox_mathkit/` (including `sandbox_src/`). — `docs/spec/spec.md:4419-4492`
- **Tower status matters:** `D-PLUGIN1` and `D-DEP-WASM1` are currently **open, spec-only imports**, not ratified Tower decisions. The sandbox behavior above is shipped/documented evidence, but those IDs remain ballot candidates rather than settled law. — `tower:D-PLUGIN1`; `tower:D-DEP-WASM1`
- `core.plugin` is listed in the built Core module inventory. — `docs/reference/core-library.md:4281-4296`

## Undecided

- Whether to ratify the documented sandbox rules as Tower law under D-PLUGIN1 and D-DEP-WASM1, rather than treating the c81 spec section as provisional imported text.
- Whether `plugin.load(path)` must enforce a no-follow root, canonical path, and symlink policy, and what root/authority facts are recorded for a loaded plugin.
- Which host imports a sandbox guest may ever request, how imports are granted, and whether guest host effects always fail with E1258 or can be explicitly admitted.
- What WIT/ABI versioning and compatibility policy sits beyond E1257's exported-interface snapshot: version negotiation, additive changes, removal, and host/guest mismatch diagnostics.
- How `authority.holds` resource-scoped grants are spelled and checked for plugins and dependencies, including the exact `allow`/`deny` precedence at an FFI or sandbox boundary.

## Conflicts

- The rights model is one tree, one holds relation, and a tighten-only nested scope. A second capability/authority namespace or an implicit widening would conflict with D-AUTHORITY-MODEL1 and D-AUTHORITY-SCOPE1.
- The root table is closed. New top-level roots, flat language roots, or grantable `Mem`/`Panic` would conflict with D-EFF4/5 and D-AUTHORITY-ROOTS1.
- The sandbox text says no `#Unsafe`, no guest host effects (E1258), scalar-homogeneous exports only (E1260), and zero host imports; a proposal for arbitrary effectful plugins, arbitrary export records, or unsafe guest escape is not an undecided primitive under the current spec.
- `target: sandbox` is the current spelling; `target: plugin` is retired by D-ONCE-SANDBOX1. Do not propose a second target name.
- The sandbox IDs are open in Tower despite the c81 documentation. Treat that as a ratification/documentation gap, not permission to silently strengthen the provisional text into additional guarantees.
