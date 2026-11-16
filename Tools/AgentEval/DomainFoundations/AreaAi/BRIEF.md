# Probe area-ai — AI/ML applications

Read `$JET_STATE_ROOT/dx3/COMMON.md` first; it holds the rule, the gap rubric, the working method, and the output contract. This file adds only what is specific to this probe.

## Kind

Critical-area probe: build the program end to end; every part matters; write batteries.json. Areas this probe informs: ai-ml.

## Build this

An AI application as one Jet package: tensors with broadcasting and a matmul on the core substrate; a tiny two-layer network trained with autodiff on a toy dataset via a data loader that batches and shuffles; model state saved and restored; an embeddings index with cosine search over 10k vectors; a streaming call to an OpenAI-compatible HTTP endpoint (mock server locally) with a bounded tool-call loop; and a GPU path for the matmul if the machine has one (record the receipt either way). Start from Examples/Math/**, Examples/LowLevel/LinalgSIMD.jet and simd_*.jet, Examples/Network/HTTP/HTTPClient.jet, Examples/Apps/Web/WebComputeWebGPU.jet, and `jet inspect facts` for core.compute.

## Answer these

1. Which of autodiff, GPU placement, and tensor layout need the compiler versus library code?
2. Measure the matmul and the training step against numpy/PyTorch on the same sizes.
3. What does an AI/ML battery need?

## Research to mine

Domain census and reports: `$JET_STATE_ROOT/dx2/llm-apps-agents/`, `$JET_STATE_ROOT/dx2/ml-training/`, `$JET_STATE_ROOT/dx2/ml-inference-serving/`, `$JET_STATE_ROOT/dx2/recommender-search/`, `$JET_STATE_ROOT/dx2/computer-vision/`, `$JET_STATE_ROOT/dx2/mlops/` (report.md, census.json, claims.json). Deleted ballots with worked code: `$JET_STATE_ROOT/dx2/ballots/` (grep the mechanism name). Family syntheses: `$JET_STATE_ROOT/dx2/_families/*/synthesis.md`.

## Output

`$JET_STATE_ROOT/dx3/area-ai/probe.md`, `gaps.json`, `batteries.json`, and the code under `$JET_STATE_ROOT/dx3/area-ai/pkg/`. Gap ids start with `area-ai-G`.
