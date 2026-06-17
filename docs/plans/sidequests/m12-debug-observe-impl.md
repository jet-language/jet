# Sidequest: E2-M12 — Debugging and observability implementation

**Plan:** `docs/plans/epoch-2/m12-debug-observe.md`  
**Status:** all decisions ratified; ready to implement after M10 + M4 ✅  
**Depends on:** E2-M10 (service log emission), E2-M4 ✅ (interpreter/LSP for breakpoints)

## Critical amendment: D-OBS1 split

**Full DAP step-through debugging does NOT ship in M12.** It is a GA gate (E2-M17).

M12 scope is the **foundation only**:
- Source maps / line directives (debuggers show Jet lines, not generated Rust)
- Jet-line panic and error reports (I2 — rustc/Rust stays hidden)
- Error-return traces for `?` propagation (Zig-style)
- Panic reports with safe locals in dev mode only (D-OBS2)

Full DAP integration (VS Code/Cursor step-through) ships at M17 GA, not here.

## D-OBS3 amendment

Structured logs/metrics in M12 are **std-only** and OTel-aligned by name convention.
An OTel *exporter* (the Rust OTel SDK wrapped as a Jet package per D-DEP1) ships later — it is NOT a compiler dependency and NOT shipped in M12.

```jet
// M12: built-in OTel-aligned structured log (std-only, no dep)
log.info("request handled", { method: "GET", status: 200, latency_ms: 12 });

// Later milestone: jet.otel package wraps the Rust OTel SDK
use jet.otel as otel;
otel.init("my-service");
```

## Other decisions (no amendments)

| Decision | What to implement |
|---|---|
| D-OBS2 | Panic shows safe locals (non-moved, non-secret) in dev mode only; off in release |

## Diagnostics to register (E30xx)

E3001 (runtime panic with Jet source location + safe locals), E3002 (error-return trace annotation).

## Exit criteria

See `m12-debug-observe.md` — with the amendment that DAP step-through is NOT an M12 exit criterion; it is deferred to M17. Foundation = source maps + Jet-line panic + error traces + structured logs. `nix develop -c cargo test` green.
