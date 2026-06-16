# E2-M12 — Debugging and observability

**Status:** draft — **blocked on D-OBS1…D-OBS3** (Group M12).
**Depends on:** E2-M10 (services emit logs/metrics), E2-M4/M13 (dev + LSP for
breakpoints/watch). A debugger is enterprise table stakes (owner-todo §2 — no
team ships a language it cannot step through).
**Error codes:** E30xx block (claim in docs/spec/diagnostics.md).

## Goal

Make production failures and local debugging understandable **in Jet terms**.
Because Jet transpiles to Rust, the pragmatic v1 is line-directive source mapping
so debuggers show Jet files and lines, not generated Rust (I2 — rustc/Rust stays
an implementation detail for normal users).

## Owner decisions — ratify before any code

| ID | Question | Rec | Default if deferred | Ratified |
|---|---|---|---|---|
| D-OBS1 | DAP timing | **A** — ship for VS Code/Cursor in M12 (before GA) | A | OPEN — needs owner |
| D-OBS2 | Panic local-value privacy | **A** — show *safe* locals only, dev mode only | A | ✅ ratified 2026-06-16 — A: panic shows safe locals in dev mode only |
| D-OBS3 | Metrics conventions | **A** — simple structured logs first; OTel-aligned metrics later | A | OPEN — needs owner |

## Scope

- **DAP / source maps (D-OBS1).** Emit line directives / a source map so
  gdb/lldb/VS Code step at Jet source lines. Ship for VS Code/Cursor before GA.
- **Panic reports in dev mode (D-OBS2).** Include relevant local values *when
  safe* (no secrets, no moved-from/uninitialized values). Off in release.
- **Error propagation traces.** For `?`-propagated errors, show the propagation
  chain where useful (Zig-style error-return traces, distinct from a stack
  trace — Zig-style error-return traces).
- **Structured logging / trace context / metrics (D-OBS3).** Build on `jet.log`
  (E2-M9); start with structured logs + trace context; metrics conventions
  OpenTelemetry-aligned but added later, not a framework.
- **`jet lsp` / `jet dev` integration** for breakpoints and watch values where
  possible.
- **Machine-readable runtime reports** for CI/service logs (`--json`).

## Panic report (example, dev mode)

```
panic: index out of bounds
  --> report.jet:18 in summarize
   |
18 |     val first = rows[0];
   |                 ^^^^^^^ rows has length 0
locals: rows = [],  path = "empty.csv"        # safe locals only (D-OBS2)
propagated from: load_rows (report.jet:9) via `?`
```

## Diagnostics to register

- **E3001** runtime panic report with Jet source location + safe locals.
- **E3002** error-return trace annotation on a propagated `?` failure.
- (Most of this milestone is *reporting* infrastructure; few new compile-time
  codes.)

## Examples & tests

- `examples/features/47_debug.jet` — a program whose panic shows Jet lines +
  safe locals.
- A DAP smoke test: set a breakpoint, hit it at a Jet line (scripted).
- `tests/observe/structured_log.txt` — `jet.log` JSON line shape.
- A test proving moved-from/secret values are *not* shown in panic locals.

## Out of scope

- Time-travel / record-replay debugging (far horizon, owner-todo notes the
  ownership model makes it feasible later).
- A metrics backend or dashboard; OTel exporters as a framework.
- Stepping through generated Rust (the opposite of the goal).
- Profiler/flamegraph tooling (post-epoch).

## Exit criteria

- VS Code/Cursor can step through a Jet program at Jet source lines.
- Panic/error reports stay beginner-readable and never leak unsafe locals.
- Service examples emit structured logs (and trace context) without a framework.
- Generated Rust remains an implementation detail for normal users.
- `nix develop -c cargo test` green.
