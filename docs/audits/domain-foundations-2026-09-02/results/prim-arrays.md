# Primitive probe: arrays

## What I built

Scratch package: `/home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/` with `package.jet` authority for `GPU, IO, Mem.Alloc, Panic`. Programs are small library-author experiments, not proposed niche libraries.

- `run.jet`: dense `core.compute` constructor and inspection.
- `views.jet`: reshape, transpose, broadcast, and elementwise add.
- `labeled_tuple.jet`: named dimensions and coordinate labels beside a Tensor, with label lookup.
- `chunked.jet`: a user-written deferred chunk loop that reads eight chunks and maps a pure function.
- `image_inline.jet`: named image metadata beside a Tensor, nearest-neighbor resize, and RGB-to-gray conversion.
- `map_4k.jet`: two 4096x4096 Float tensors and elementwise add.
- `tensor_wrapper_ice.jet`, `set_ice.jet`, and `dtype_generic.jet`: boundary and compiler probes.

## What worked

1. `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/run.jet` returned `shape:[2, 3] rank:2 numel:6` and six `2.0` values.
2. `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/views.jet` returned `matrix:[2, 3] transpose:[3, 2] values:[1.0, 4.0, 2.0, 5.0, 3.0, 6.0]` and `broadcast_sum:[11.0, 12.0, 13.0, 14.0, 15.0, 16.0]`. The same source built AOT and `/home/nate/Projects/Github/jet/build/views` produced byte-identical lines.
3. `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/labeled_tuple.jet` returned `lookup:3.0` and `missing:null`. The same source built AOT with `jet build` and produced the same output. An anonymous named tuple can therefore carry a Tensor plus metadata.
4. `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/chunked.jet` returned `chunks:8 reads_before:0`, `first:[0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 14.0] reads_after_one:1`, and `mapped_len:64 reads_after_all:8`. AOT `jet build` also succeeded. This is manual chunking, not a Core lazy plan.
5. The inline image program runs in the default `jet run` tier and returns `image:4x4x3 dtype:f64 corner:1.0` and `gray:2x2x1 first:0.3`. The loops prove that ordinary Jet code can express a toy transform over Tensor values.

## Gaps

### prim-arrays-G1 — Tensor-bearing records cannot be code-generated

Tags: `defect`, `impossible`. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/prim-arrays/store scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/tensor_wrapper_ice.jet` exits 101 with `codegen reached a construct the typed IR does not cover: the expression BoxedTensor{tensor: values}` at `tensor_wrapper_ice.jet:9:14` (compiler bug I2/R7, `Items.rs:3409:5`). A labeled/raster value cannot be a normal user-defined record in AOT. Workaround: anonymous named tuples or parallel values. Areas: data, AI/ML, science.

### prim-arrays-G2 — AOT Tensor writes fail

Tag: `defect`. `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/prim-arrays/store scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/set_ice.jet` exits 101 with `internal compiler error: the generated Rust did not compile` (`build/set_ice.rs`). The same `compute.set(&tensor, [0, 1], 3.0)` program under `jet run` returns `values:[0.0, 3.0, 0.0, 0.0]`. This blocks AOT implementations that allocate an output and fill indexed pixels. Areas: data, AI/ML, science.

### prim-arrays-G3 — No labeled N-D Tensor primitive

Tag: `impossible`. `labeled_tuple.jet:17-29` must manually search strings, construct numeric indexes, and call `compute.get`; it returns the expected `lookup:3.0` only because metadata remains outside Tensor. Archived `climate-weather/report.md:47-63` and deleted `D-M-LABELED-ARRAYS1` agree that current Tensor exposes shape/strides but not named dimensions, coordinates, attributes, masks, or alignment. Workaround: named tuple plus hand-written label checks. Areas: data, AI/ML, science.

### prim-arrays-G4 — No lazy chunk plan/scheduler primitive

Tag: `impossible`. `chunked.jet:3-31` implements its own mutable plan, read counter, eager `compute.from_list` per chunk, map loop, and final list. It proves one-at-a-time reads but provides no overlap, fusion, cancellation, memory bound, or plan identity. Archived `climate-weather/report.md:47-51` and `oceanography-hydrology/report.md:81-86` record the same absence. Workaround: repeat this bespoke machinery per workload. Areas: data, AI/ML, science.

### prim-arrays-G5 — No first-class image/raster value or image operations

Tag: `impossible`. Archived `medical-imaging/probes/missing_image_check.out:1-4` reports `E1001: there is no core module core.image`. The inline program can hand-write nearest resize and grayscale, but `image-processing/report.md:50-61` records no image value, codec, color-space, or resampler. Workaround: named tuple metadata plus indexed loops; no file codec, range, alpha, or quality-resampling semantics. Areas: data, AI/ML, science.

### prim-arrays-G6 — Tensor dtype is not semantic or generic

Tag: `impossible`. `dtype_fixed.jet` passes `U8` inputs to `compute.from_list`, but `jet run` prints `values:[1.0, 2.0]`; `D-M-RASTER1.json:12-13` records `JetTensor.data: Arc<Vec<f64>>` and no dtype field. An explicit `Tensor<T>` wrapper builds only when specialized to Float; `jet check dtype_generic.jet` reports `E2104: type-parameterized callable has no complete TIR specialization`. Workaround: use Float/f64 and carry requested dtype as unenforced metadata. Areas: data, AI/ML, science.

### prim-arrays-G7 — Dense 4K map is far slower than incumbents

Tag: `slow`. Release `jet build` of `map_4k.jet` succeeded. `/home/nate/Projects/Github/jet/build/map_4k` with `time -p` timed out at 300 seconds (`real 300.25`, `user 2000.91`, `sys 287.53`) before printing its result. The same two-array 4096x4096 Float64 add under Nix Python printed `corner:3.0` in `0.036179 sec` with NumPy and `0.017504 sec` with xarray. This is a same-input comparison; Jet did not produce a successful output check. Workaround: smaller arrays or an external backend. Areas: data, AI/ML, science.

## Friction

- The safe user-space fallback is an anonymous tuple. A nominal `struct` containing Tensor cannot pass AOT codegen, so library APIs cannot state a durable array type this way.
- `compute.set` is usable in default `jet run` but not AOT, so an implementation that works interactively can fail at the production build boundary.
- The manual image example is 39 lines for two elementary operations. The manual chunk example is 31 lines for one eager read-and-map loop. These are evidence of missing primitives, not recommended APIs.
- `Tensor<T>` syntax parses, but the generic callable has incomplete check/TIR coverage and the Core constructors still expose Float storage.

## Defects

- G1 and G2 are exact AOT defects with compiler ICE/generated-Rust failures.
- `image_inline.jet` default execution succeeds, but `JET_STORE_DIR=/home/nate/.cache/jet-luna/dx3/prim-arrays/store scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/prim-arrays/pkg/image_inline.jet` exits 101 with `internal compiler error: the generated Rust did not compile` (`build/image_inline.rs`). This is the combined consequence of indexed Tensor writes and should be fixed with G2 before treating image code as AOT-ready.

## Research mined

- `~/.cache/jet-luna/dx2/climate-weather/report.md`: checked dense Tensor is real, but labeled N-D arrays, coordinate alignment, netCDF/Zarr, lazy chunks, and distributed halos are absent; performance was previously unclaimed.
- `~/.cache/jet-luna/dx2/remote-sensing/report.md`: Tensor shape/transpose/broadcast exists, but raster, CRS, windows, and blocked arrays are absent; the report records `core.geospatial` rejection with E1001.
- `~/.cache/jet-luna/dx2/image-processing/report.md`: Tensor/View/FFT/sparse/placement exist, but image value, dtype/range, codec, colorspace, resampler, and demand-driven pipeline are absent.
- `~/.cache/jet-luna/dx2/oceanography-hydrology/report.md` and `medical-imaging/report.md`: scientific workflows need labeled arrays, Dask/Zarr/netCDF, spatial metadata, medical image IO, and tiled inference; Jet currently has only eager Tensor numerics.
- Deleted ballots `D-M-LABELED-ARRAYS1` (recommendation C: checked facets on one Tensor) and `D-M-RASTER1` (recommendation C: one Raster over Tensor storage) both preserve the same current-vs-proposed boundary. `M-CHUNKED-LAZY-ARRAYS.json` identifies the separate chunk scheduler mechanism.

## Verdict

`core.compute` is a useful eager Float Tensor substrate with shape operations, broadcasting, strided transpose, and arithmetic. A library author can prototype labels, chunks, and image math with tuples and loops, but cannot ship them as durable AOT array values. The blocking primitives are AOT Tensor records/writes, labeled metadata and alignment, lazy chunk planning, and a first-class raster value; dtype semantics and large-map kernels hurt broadly. No `batteries.json` is produced because this is a primitive probe.
