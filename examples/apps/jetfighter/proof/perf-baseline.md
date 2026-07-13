# JetPlay Perf Baseline

| Budget | Value | Proof |
| --- | --- | --- |
| Frame p99 | 16 ms | `perf.game` `FrameTime(.P99)` budget in `main.jet` |
| Memory | 128 MB | Budget transcript in `expected/run.out` |
| Assets | 512 KB | Image and sound registrations remain below budget |
| Draw calls | 8 | Text renderer preview emits one paint command |

CI runs `jet run`, `jet test`, native `jet build`, and the web editor build.
The capstone test also edits a copied source tree and reruns the game, proving
the replay stays deterministic after source-backed edits.
