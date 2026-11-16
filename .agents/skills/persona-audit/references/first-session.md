# Persona first-session evidence

Declare `window target` for each frozen persona row before probing. Run the two checks below only when that row's project has a window target. For a non-window project, record `not-applicable` in the report. Keep both checks separate from the later project loop.

| check | measure | honest result |
| --- | --- | --- |
| `time-to-first-window` | Elapsed time from the first command to a usable first-party window. | Record milliseconds, backend, and input. Use `not-applicable` when the row has no window target. |
| `first-pixel` | Elapsed time from window creation to the first visible rendered pixel. | Record milliseconds, backend, input, and frame evidence. Never infer it from a window handle. |

## Backend gate

A window-target row must pass the complete backend chain in order: `#820` → `#821` → `#822` → `#823` → `#824` → `#825`. The current `core.game` default is headless/no-op. It is useful evidence for a headless loop, but it is not evidence of a usable window or first pixel. Until the chain is complete and a real run exposes window and frame receipts, record an honest non-result.

Keep these states distinct:

- `not-applicable`: the row's project has no window target;
- `not-proven`: a window target exists, but the run or evidence is missing;
- `blocked`: the row cannot complete its first-session loop.

A missing windowed backend blocks a window measurement. It does not justify an invented zero or a `ship-ready` verdict. For a measured result, record elapsed milliseconds, backend, input, and frame evidence where applicable. For an honest non-result, name the missing target, run, or evidence. Never leave a blank or infer a pixel from a handle.

## First-session stop

The first-session pass is complete when every frozen row has either its conditional measurement or a named honest state, and every window-target row has an explicit backend-chain result. Keep this pass separate from implementation completion and from the persona's later project verdict.
