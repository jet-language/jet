# AI/ML foundations probe

## What I built

I built a small Jet package for an AI/ML application. It trains a two-layer Tensor model on shuffled batches, broadcasts a bias row, serializes and restores model state, scans a 10,000-entry cosine embedding index, calls an OpenAI-compatible local SSE endpoint, and records a Vulkan placement receipt. The training path uses explicit chain-rule helpers because the requested `compute.gradient` path does not run on the current evaluator or native compiler.

Files:

- `pkg/package.jet` — package identity and authority.
- `pkg/run.jet` — Tensor model, batching, manual gradients, SGD, broadcast, save/restore.
- `pkg/embedding.jet` — 10,000-entry embedding index and cosine scan.
- `evidence/llm_probe.jet` — local HTTP/SSE mock and two-step bounded tool loop.
- `evidence/gpu_probe.jet` — explicit Vulkan F32 matmul and placement receipt.
- `evidence/bench.jet` — 32x32 matmul timing.

The package's normal run completed in 4.76 seconds:

```text
loader epoch=0 batch_count=2 shuffled_first=0
batch=0 loss=[0.5]
batch=2 loss=[0.32000000000000006]
loader epoch=1 batch_count=2 shuffled_first=0
batch=0 loss=[0.19749355519999995]
batch=2 loss=[0.11160484011302012]
loader epoch=2 batch_count=2 shuffled_first=0
batch=0 loss=[0.056255532078651]
batch=2 loss=[0.025151127664124015]
loader epoch=3 batch_count=2 shuffled_first=0
batch=0 loss=[0.010093249051837066]
batch=2 loss=[0.0037189569727621593]
model=[0.7738926045572292, 0.0, 0.0, 0.7738926045572292]
broadcast=[1.5, 2.5, 1.5, 2.5]
restored=[0.7738926045572292, 0.0, 0.0, 0.7738926045572292]
```

## What worked

- `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-ai/pkg/run.jet` — shuffled four examples into two batches for four epochs; loss fell from `0.5` to `0.0037189569727621593`; model state round-tripped through `compute.serialize` and `compute.deserialize`; broadcast output was `[1.5, 2.5, 1.5, 2.5]`.
- `scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/area-ai/pkg/run.jet --verbose` — native build passed; generated 5,336,873 bytes of Rust and reported `Built build/run in 33.8s`.
- `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-ai/evidence/llm_probe.jet` — `step=0 status=200 streamed=true tool_call=true`, `step=1 status=200 streamed=true tool_call=false`, `bounded_tool_loop=2`. The probe uses generic JSON HTTP and SSE; no model provider library is needed for this protocol-shaped mock.
- `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-ai/evidence/gpu_probe.jet` — Vulkan succeeded with `Placement(requested=Vulkan, selected=Vulkan, backend=vulkan, ... profile=F32Strict+Reproducible, ... ability=vulkan.f32)`. The initial F64 Vulkan request was correctly rejected with `Vulkan backend supports only F32Strict+Reproducible` and `E3002`; converting through `matmul_f32_tile` made the explicit F32 path work.
- `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-ai/evidence/bench.jet` — interpreter timing for eight 32x32 matmuls was `elapsed_ms=41360`, first result `64.0`. A native `jet build` followed by `/home/nate/Projects/Github/jet/build/bench` reported `elapsed_ms=2909`, first result `64.0`.

## Research mined

- `~/.cache/jet-luna/dx2/ml-training/report.md` and `D-M-ML-LIFECYCLE1.json`: `core.compute` has Tensor algebra, autodiff names, SGD, placement, and a checksummed single-Tensor wire. Dataset/Loader, Module/parameter registry, AdamW, AMP, distributed sharding, complete checkpoints, and run lineage remain absent or proposed. The ballot explicitly says the shipped wire is not a full checkpoint.
- `~/.cache/jet-luna/dx2/llm-apps-agents/report.md` and `~/.cache/jet-luna/dx2/ml-inference-serving/report.md`: typed JSON, HTTP, tasks, authority, and receipts are usable foundations, but provider/model abstractions, token streams, generation, model artifacts, tokenizers, schedulers, and serving lifecycle are not core surfaces. The local probe confirms a library can compose the small provider-neutral HTTP/SSE/tool-loop path today.
- `~/.cache/jet-luna/dx2/recommender-search/report.md`: no first-class collection, vector index, `top_k`, or retrieval result receipt exists. The 10,000-entry exact scan is therefore an executable library example, not a claim that Jet ships a retrieval index.
- `~/.cache/jet-luna/dx2/computer-vision/report.md` and `~/.cache/jet-luna/dx2/mlops/report.md`: image/model runtime surfaces and run, artifact, lineage, registry, label, and feature-store surfaces are absent. They were not needed for this probe.
- `~/.cache/jet-luna/dx2/_families/ai-ml/synthesis.md`: the reusable mechanisms are model state, data loader, model calls, retrieval index, Tensor/linalg, GPU kernels, tables, networking, storage, and evidence. `D-M-GPU1.json` keeps array operations and placement receipts shipped while checked custom kernel launch remains proposed. `docs/reference/core-library.md:3976-4068` confirms the current Tensor, broadcast, matmul, autodiff, serialization, and explicit-device API.

## Gaps

### area-ai-G1 — defect, blocks

A cross-tier executable Tensor autodiff path is missing: named `compute.gradient` calls must run in the evaluator and lower in native code without an internal compiler error.

Evidence:

- `scripts/agent/jet-env jet run /home/nate/.cache/jet-luna/dx3/area-ai/evidence/mini.jet` -> `Error [E0956]: \`Tensor\` isn't supported by the current evaluator yet` at `mini.jet:6`, the named `(dw, dx) :: compute.gradient(...)` call.
- `scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/area-ai/evidence/mini.jet --verbose` -> exit `101`: `internal compiler error: the generated Rust did not compile`, generated `/home/nate/Projects/Github/jet/build/mini.rs`.
- `scripts/agent/jet-env jet run examples/features/tooling/compute_autodiff.jet` -> `E0112` at `compute_autodiff.jet:18` (`compute.gradient` needs named Tensor parameters), then `E0102` at line 19 because `curvature` was not defined.

The workaround is explicit manual chain rule in `pkg/run.jet:48-64`; it trains the tiny model but is not composable autodiff.

### area-ai-G2 — boilerplate, hurts

A typed Dataset/Loader primitive is missing for bounded batching, shuffle order, and collate. The package hand-writes two near-identical batch constructors at `pkg/run.jet:16-46`, then repeats epoch and batch control at `:66-85` and prints its own loader receipt. The successful run prints `loader epoch=0 batch_count=2` through epoch 3, so the loop is buildable, but the common lifecycle remains author boilerplate.

Workaround: an author can use `[Int].shuffle().to_list()` and explicit batch helper functions, as this package does. The research baseline is PyTorch `DataLoader(dataset, batch_size, shuffle, num_workers)`; no equivalent typed Jet primitive was found.

### area-ai-G3 — slow, blocks

The compiler does not reach a bounded proof for the ordinary 10,000-entry embedding index. `scripts/agent/jet-env jet build /home/nate/.cache/jet-luna/dx3/area-ai/pkg/embedding.jet --verbose` timed out after 360 seconds while still at `Checking program and build plan`, before generated Rust or a binary. The source at `pkg/embedding.jet:25-49` constructs 10,000 small vectors and scans them with cosine similarity. The earlier integrated interpreter run also timed out after 300 seconds once this scan was appended to the otherwise successful training package.

Workaround: keep the index behind a native provider or reduce the corpus while waiting for compiler/runtime help; no Jet-native 10,000-entry timing receipt was reachable.

## Friction

- The requested NumPy/PyTorch same-size comparison could not run: `python3 -c 'import numpy, torch; print(numpy.__version__, torch.__version__)'` returned `command not found: python3` (exit 127). Therefore this probe makes no incumbent speed claim.
- Tensor operations, serialization, placement, HTTP, SSE, and bounded tasks are concise enough for a library author. The missing first-party model and loader abstractions are library-level ergonomics gaps, not blockers for the small HTTP mock or manual-SGD example.
- The default evaluator is useful for ordinary Tensor algebra but not for `Tensor` autodiff. Native AOT accepts the ordinary manual-gradient package and rejects the autodiff fixture with an ICE.
- GPU selection is explicit and fail-closed. Vulkan worked on this host; native WebGPU remains a typed unavailable-provider path per `docs/reference/core-library.md:3992-4000`.

## Defects

- `E0956` evaluator rejection and the native generated-Rust ICE are both reproducible with `evidence/mini.jet`.
- The shipped higher-order example has the paired `E0112`/`E0102` diagnostic sequence above.
- No defect is inferred from the unavailable Python incumbent or native WebGPU provider.

## Battery notes

See `batteries.json`. The battery is a small Tensor training loop, deterministic loader, checksum model round-trip, exact embedding scan, provider-neutral SSE/tool loop, explicit Vulkan receipt, and same-size matmul harness. The training battery is not library-only until `compute.gradient` is executable across tiers; the other wrappers are library-only examples around shipped primitives.

## Verdict

Jet can build a useful small AI/ML application from Tensor algebra, broadcasting, SGD, serialization, HTTP/SSE, tasks, and explicit device receipts.

The core missing primitive is cross-tier Tensor autodiff; the current evaluator rejects it and native code generation ICEs.

A first-party typed loader would remove repeated batching and collate code, but a library author can ship the workaround today.

The 10,000-entry index source is written, but its native check did not finish within six minutes, so the area is not a complete native proof.

Do not claim NumPy/PyTorch parity from this run; neither incumbent was installed on the host.
