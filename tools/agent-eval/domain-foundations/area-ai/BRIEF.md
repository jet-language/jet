# Probe area-ai — AI/ML applications

Read `~/.cache/jet-luna/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: ai-ml.

## Build this

An AI application as one Jet package: tensors with broadcasting and a matmul on the core substrate; a tiny two-layer network trained with autodiff on a toy dataset via a data loader that batches and shuffles; model state saved and restored; an embeddings index with cosine search over 10k vectors; a streaming call to an OpenAI-compatible HTTP endpoint (mock server locally) with a bounded tool-call loop; and a GPU path for the matmul if the machine has one (record the receipt either way). Start from examples/features/math/**, examples/features/lowlevel/linalg_simd.jet and simd_*.jet, examples/features/net/http_client.jet, examples/features/web/web_compute_webgpu.jet, and `jet inspect facts` for core.compute.

## Answer these

1. Which of autodiff, GPU placement, and tensor layout need the compiler versus library code?
2. Measure the matmul and the training step against numpy/PyTorch on the same sizes.
3. What does an AI/ML battery need?

## Research to mine

Domain census and reports: `~/.cache/jet-luna/dx2/llm-apps-agents/`, `~/.cache/jet-luna/dx2/ml-training/`, `~/.cache/jet-luna/dx2/ml-inference-serving/`, `~/.cache/jet-luna/dx2/recommender-search/`, `~/.cache/jet-luna/dx2/computer-vision/`, `~/.cache/jet-luna/dx2/mlops/` (report.md, census.json, claims.json). Deleted ballots with worked code: `~/.cache/jet-luna/dx2/ballots/` (grep the mechanism name). Family syntheses: `~/.cache/jet-luna/dx2/_families/*/synthesis.md`.

## Output

`~/.cache/jet-luna/dx3/area-ai/probe.md`, `gaps.json`, `batteries.json`, and the code under `~/.cache/jet-luna/dx3/area-ai/pkg/`. Gap ids start with `area-ai-G`.
