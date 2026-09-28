# Defect characterization

The seven requested follow-ups are recorded in `defects2.json`.

- **Arrays / G7:** 256² mapping completes in 62.43s including `jet run`; 512² exceeds 300s. The CPU path allocates coordinate vectors and performs raw coordinate lookup for each output element.
- **AI / G3:** checker timings are 4.91s at 1,250 entries and 20.66s at 2,500; 5,000 and 10,000 both exceed 60s. This is superlinear generic-list construction checking.
- **FFT / G2:** release transform work is 30,853,298 ns, 116,669,574 ns, and 476,150,738 ns for 1,024, 2,048, and 4,096 inputs. The implementation is nested O(n²) DFT.
- **Exact numerics / G6:** default run is 10.12s and release compile+run is 25.47s, but the built binary is 1.85s. No native arithmetic defect over 2s was found.
- **Data / G03:** the expected default E0956 is not reproducible. Default and `--interpret` pass; release instead emits the generic Rust-compile ICE. Current TIR source contains `inner_join` and `left_join` arms.
- **Backend / G5:** a five-line repro reaches E0956 in default TIR and passes under `--release`. A direct five-line `log.set_trace_id` program does not fail unless `core.service.state_store` forces the fallback tier.
- **ICE banner:** `CmdCompile.rs` reads rustc stderr, uses it only for linker classification, then renders only the generic banner and generated path. It neither stores nor echoes the rejected Rust diagnostics.

All commands used scratch stores under `_defects2`; no repository files were edited.
