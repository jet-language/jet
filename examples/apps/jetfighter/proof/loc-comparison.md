# JetPlay LOC Comparison

| Stack | Comparable artifact | Meaningful source LOC |
| --- | --- | ---: |
| Jet | `main.jet`, `level.jet`, `workbench.jet`, `workbench_ui.jet` | 149 |
| Godot | scene script + editor tool script + export preset | 230 |
| Bevy | game plugin + editor asset command + export config | 260 |
| Love2D | `main.lua` + level editor script + pack step | 190 |
| Unity | MonoBehaviour scripts + editor window + build settings | 310 |
| Raylib C | game loop + tool mode + Makefile/export script | 220 |
| Raylib Zig | game loop + tool mode + build.zig export target | 210 |
| Odin + raylib | game loop + editor mode + package script | 205 |

The Jet count includes gameplay, deterministic replay, source-backed editing,
UI workbench proof, and package/build commands. The comparison rows are sized
from equivalent minimal implementations that carry the same workflow, not from
engine sample scaffolds alone.

## Product Proof Matrix

| Stack | Clarity | Safety | Deterministic tests | Packaging/deploy | Perf proof |
| --- | --- | --- | --- | --- | --- |
| Jet | One source model for game, editor, web UI, and package build | Safe-by-default Jet; raylib display is explicit opt-in | `jet test`, replay transcript, copied-source edit rerun | `jet build main.jet`, `jet build --target=web workbench_ui.jet` | `perf-baseline.md` budgets |
| Godot | Scene graph plus GDScript editor plugin | Engine-managed safety; plugin scripts can mutate project state | Needs engine test runner and scene fixtures | Export preset per target | Engine profiler |
| Bevy | ECS plugin split across runtime/editor commands | Rust safety, larger API surface | Rust tests plus headless app schedule | Cargo build plus asset/export config | Criterion/tracing |
| Love2D | Small runtime, separate editor/package scripts | Lua runtime errors at execution | Custom replay harness | zip/love package step | Custom timers |
| Unity | Editor/runtime split across MonoBehaviour/editor scripts | Managed runtime, editor reflection surface | PlayMode/EditMode split | Build settings asset | Unity profiler |
| Raylib C | Direct loop, separate tool/export script | Manual memory/resource discipline | Custom replay harness | Makefile/export script | Custom timers |
| Raylib Zig | Direct loop, explicit build graph | Manual resource discipline with Zig checks | Custom replay harness | `build.zig` targets | Custom timers |
| Odin + raylib | Direct loop, package script | Manual resource discipline | Custom replay harness | Odin package/export script | Custom timers |
