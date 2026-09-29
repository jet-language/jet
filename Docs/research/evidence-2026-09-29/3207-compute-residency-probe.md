# #3207 — compute residency and per-operation materialization (probe)

Closer07, 2026-09-29. Binary: `jet-debug-snapshot14` (sha256 prefix `00b1e35e25ed941c`). Hardware: AMD Radeon RX 7900 XTX (RADV NAVI31, Vulkan 1.4.348 per `vulkaninfo --summary`), Ryzen 9 7950X3D, NixOS (no system `libvulkan.so` on the default loader path).

## Question
Can a repeated F32 op chain on a device-resident tensor report real allocation/upload/readback counts, bytes and synchronization through the public receipts? Or can placement labels fabricate residency?

## Method
Probe `~/.cache/jet-test-scratch/Closer07/compute/residency_probe.jet`:
1. Build an F32 tensor with `matmul_f32_tile(matrix(2,2,1.0), eye(2))` and print `placement` / `transfer_show`.
2. Run a 16× `compute.add(x, x)` chain on CPU and print the receipts.
3. `on_device(…, device_vulkan())` and `device_cuda()`. If accepted, repeat the chain and `transfer` back to CPU.

Tiers:
- `safe-jet.sh run` (JIT);
- `run --interpret`;
- `build` then `.jet/build/residency_probe` (AOT), also run with `LD_LIBRARY_PATH=/nix/store/…-vulkan-loader-1.4.341.0/lib`.

## Evidence
All three tiers print the same thing:
```
cpu placement=Placement(requested=CPU, selected=CPU, backend=cpu-oracle, version=builtin, profile=F32Strict+Reproducible, …)
cpu transfer=Transfer(from=CPU, to=CPU, bytes=0, fallback=none)
cpu chain16 values=[65536.0, 65536.0, 65536.0, 65536.0]
cpu chain16 transfer=Transfer(from=CPU, to=CPU, bytes=0, fallback=none)
vulkan: UNAVAILABLE
cuda: UNAVAILABLE
```
Supporting observations:
- AOT with the Vulkan loader on `LD_LIBRARY_PATH`: identical output.
- The generated AOT Rust (`.jet/build/residency_probe.rs`) compiles Core's Jet-source `fn on_device` / `fn select_device`.
- Core source `Core/compute/compute.jet:152-157`: `on_device` returns `Err(ComputeError.Unsupported)` for every selected device other than CPU.
- `:163-166`: `transfer_show` is the constant `"Transfer(from={name}, to={name}, bytes=0, fallback=none)"`.
- `:41-46`: the tensor stores `data: [Float]` (F64) for every profile, F32 included.
- The Prelude `jet_compute_on_device` (`crates/jet-codegen/src/Prelude/CoreLib/Top/Compute.rs:6891-6922`) is present in the output but is not what the user call reaches. It too returns `data: tensor.data.clone()` (host `Arc<Vec<f64>>`) with `last_transfer: None` after an upload whose device buffer is dropped (source inspection only).

The golden `Examples/features/tooling/compute_vulkan_webgpu.jet` expects `vulkan:accepted` (`Examples/features/expected/tooling/compute_vulkan_webgpu.out:1`). On this machine the JIT prints `vulkan:rejected` / `webgpu:rejected` (exit 0).

## Verdict
FAIL / BLOCKED(implementation):
- **Crit 1:** no device-resident path is reachable on any tier. The only transfer receipt is a constant (`bytes=0`, from = to), so it cannot count uploads or readbacks. It is a label, not accounting.
- **Crit 2:** no in-place or ownership-transfer path exists to test.
- **Crit 3:** F32-profile tensors are stored as `[Float]` (F64) in the reachable Core path. Every op therefore runs through the f64 host representation.
- **Crit 4:** no device path to time, so no kernel-only or end-to-end measurement is possible tonight. No speed claim is made.
- **Crit 5:** `tests/compute_residency.rs` does not exist.

This card is an implementation card: typed storage, device dispatch and a counting receipt, per the plan. No evidence-only closure is possible.

## Defects
1. `transfer_show` returns a fabricated constant receipt (`Transfer(from=X, to=X, bytes=0, fallback=none)`) whatever the actual history. Expected: it reports real transfers or says `unknown`/`not-tracked`.
2. The golden `compute_vulkan_webgpu.out` expects `vulkan:accepted`, but the JIT on a Vulkan-capable NixOS host prints `vulkan:rejected`. The reachable Core source path rejects every non-CPU device, so the golden holds only where some other path is taken.
