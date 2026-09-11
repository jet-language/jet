# Media and creative family synthesis

## Family verdict
The nine-domain family is not ready for stock media production. Incumbents win by making the domain object executable from the first loop: a raster carries pixels and color, a scene carries transforms and assets, an audio graph carries deadlines and sample-time events, a media graph carries packets and time bases, a timeline carries editorial meaning, and a document carries layout and glyph provenance. Jet already has typed compute, scalar text, deterministic plots, bounded files, explicit provider authority, and a headless game Scene/Replay/SceneProbe substrate. The missing work is concentrated in thirteen new media mechanisms plus reused numerical, packaging, graph, evidence, and device seams.

| Mechanism | Domains | Gate kind | Jet state | Reuse |
|---|---:|---|---|---|
| `M-RASTER` First-class raster/image values and bounded pixel storage | 3 | public-api | real-gap | `new` |
| `M-GPU-GRAPHICS` Portable graphics devices, shaders, pipelines, and frame resources | 5 | public-api | real-gap | `new` |
| `M-SCENE-GRAPH` Typed scene graphs, transforms, assets, and graph identity | 6 | public-api | real-gap | `new` |
| `M-AUDIO-GRAPH` Sample-accurate audio graphs and realtime callback contracts | 2 | public-api | real-gap | `new` |
| `M-MEDIA-PIPELINE` Media codec, demux/decode/filter/encode/mux pipeline | 3 | stdlib-dependency | real-gap | `new` |
| `M-COLOR` Color spaces, transfer functions, gamut, and display transforms | 2 | public-api | real-gap | `new` |
| `M-TEXT-SHAPING` Font discovery, Unicode shaping, and positioned glyphs | 2 | stdlib-dependency | real-gap | `new` |
| `M-CANVAS-2D` Portable 2D canvas and creative frame loop | 2 | ui | real-gap | `new` |
| `M-TIMELINE` Typed timelines, time bases, keyframes, and editorial interchange | 3 | public-api | real-gap | `new` |
| `M-HEADLESS-REPLAY` Headless deterministic scene replay and performance probes | 3 | invariant | mixed | `new` |
| `M-PLUGIN-ABI` Capability-scoped media/audio plugin ABI and lifecycle | 2 | public-api | real-gap | `new` |
| `M-BVH-ACCEL` Typed BVH construction and ray traversal acceleration | 2 | public-api | real-gap | `new` |
| `M-DOCUMENT-PUBLISHING` Checked document AST, layout, and loss-aware publication | 1 | public-api | real-gap | `new` |
| `M-LINALG` Typed dense tensors and linear algebra | 2 | none | mixed | `M-LINALG` |
| `M-FFT` FFT and inverse spectral transforms | 1 | public-api | mixed | `M-FFT` |
| `M-GPU-KERNELS` Explicit GPU kernels and placement receipts | 3 | public-api | mixed | `M-GPU-KERNELS` |
| `M-SCI-FORMATS` Standard scientific and engineering file formats | 3 | public-api | mixed | `M-SCI-FORMATS` |
| `M-UNITS` Typed dimensional and affine quantities | 1 | invariant | mixed | `M-UNITS` |
| `M-PLOTTING` Deterministic and interactive visualization | 1 | ui | mixed | `M-PLOTTING` |
| `M-NOTEBOOK` Notebook and interactive REPL loop | 2 | none | mixed | `M-NOTEBOOK` |
| `M-WORKFLOW-DAG` Inspectable workflow graphs and execution plans | 1 | public-api | mixed | `M-WORKFLOW-DAG` |
| `M-MESH-STENCILS` Meshes, stencils, and discretized field operators | 1 | public-api | real-gap | `M-MESH-STENCILS` |
| `M-DSP-FILTERS` Typed filter design and multirate DSP | 1 | public-api | real-gap | `M-DSP-FILTERS` |
| `M-HARDWARE-IO` Authority-safe hardware and instrument I/O | 1 | public-api | real-gap | `M-HARDWARE-IO` |
| `M-PACKAGING` Package, dependency, and native-provider boundary | 2 | stdlib-dependency | mixed | `M-PACKAGING` |
| `M-EVIDENCE` Inspection, diagnostics, and reproducible evidence | 5 | none | mixed | `M-EVIDENCE` |

## Domain matrix

| Domain | Persona | Benchmark peer | P0 gaps | Performance incumbent | Gauntlet | Biggest beat vector |
|---|---|---|---:|---|---|---|
| `audio-music-production` | Audio plugin authors, sound designers, composers, and live electronic performers learn C++ or a visual dataflow tool and reach for JUCE, Faust, Max/MSP, or SuperCollider. | Faust-generated DSP with JUCE/CLAP hosting | 24 | Faust-generated DSP for sample-level compiled kernels; JUCE with VST3/AU/standalone wrappers and CLAP for plugin hosting; Max/MSP and SuperCollider for graph authoring, live control, and server execution | none | B1 typed graph and identity: buses, ports, parameters, note dialects, nodes, resources, and artifacts are explicit. |
| `video-vfx-compositing` | A compositor, editor, colorist, or pipeline engineer learns Python and C/C++, then reaches for FFmpeg/GStreamer, Nuke or Resolve, and OpenEXR/OCIO/OTIO/OpenFX. | FFmpeg libavformat/libavcodec/libavfilter | 40 | FFmpeg | none | One typed media ledger can carry source, stream, frame, time, pixel/audio format, provider, cache, and output identity. |
| `3d-animation-dcc` | A 3D technical artist or pipeline engineer learns Python and a DCC, then uses Houdini for procedural assets, Blender for interactive scripting, and OpenUSD for shared scenes. | Embree 4.4.1 plus OpenUSD 26.08 | 31 | Embree 4.4.1 for geometry acceleration and OpenUSD 26.08 UsdStage/Pcp for scene composition | none | One typed numeric and evidence substrate can unify mesh, scene, frame, provider, and render facts. |
| `rendering-graphics` | A rendering engineer writes C++ or Rust, knows GPU synchronization and shader layouts, and reaches for Vulkan/wgpu, Slang, Embree/OptiX, or Mitsuba. | C++ Vulkan plus Embree 4 and OptiX | 26 | C++ Vulkan renderer with Embree 4 CPU traversal and OptiX hardware ray tracing where available | none | One typed evidence ledger can cover shader artifacts, resources, passes, devices, captures, and output images. |
| `image-processing` | An imaging engineer or scientist uses scikit-image for concise algorithms, libvips for large pipelines, Halide for scheduled kernels, and Pillow/OpenCV/ImageMagick for codecs and cameras. | Halide and libvips | 28 | Halide scheduled pipelines and libvips demand-driven pipelines, with Pillow/OpenCV/ImageMagick as practical codec and resampling baselines | none | One raster value can keep dimensions, channels, range, alpha, color, and provenance beside Tensor data. |
| `ar-vr-xr` | A spatial developer learns C#, C++, Swift, or Kotlin and starts from Unity XR, Unreal, RealityKit, OpenXR, or WebXR according to the device and studio. | Unreal Engine 5 native OpenXR | 21 | Unreal Engine 5 OpenXR/native C++ render path, with Unity XR and native OpenXR as the practical comparison rails | none | Deterministic headless scene/replay can test spatial input before hardware or a display exists. |
| `creative-coding` | A creative coder starts with a browser sketch or visual graph, then reaches for p5.js, Processing, openFrameworks, Nannou, TouchDesigner, or Cables. | openFrameworks native OpenGL | 24 | openFrameworks | none | A typed source-backed visual loop can keep sketch, input, frame, shader, capture, and diagnostics in one authority. |
| `typography-publishing` | A technical author, publisher, or data scientist uses Typst for fast markup, Quarto/Pandoc for executable interchange, LaTeX for legacy science, and HarfBuzz at the shaping boundary. | Typst plus HarfBuzz | 23 | Typst CLI compiler plus HarfBuzz shaping, compared with LuaLaTeX/pdflatex and Pandoc/Quarto conversion paths | none | One checked source can combine document structure, computation, typography, and output provenance without a hidden template language. |
| `acoustics-audio-engineering` | An acoustician, audio measurement engineer, or spatial-audio developer uses MATLAB Audio Toolbox, pyroomacoustics, Faust, ODEON/EASE, or COMSOL according to the measurement or solver task. | ODEON plus Faust and MATLAB Audio Toolbox | 21 | ODEON for room-acoustic ray tracing and auralization; MATLAB Audio Toolbox and Max/MSP for measurement and live DSP; pyroomacoustics and Faust for scriptable/compiled signal processing; COMSOL Acoustics for FEM/FDTD-class field simulation | none | Typed contracts can name device, sample frame, geometry, units, calibration, and metric field in every failure. |

The 238 P0 rows are preserved in `domains.json`, and each shared row is copied into its mechanism's `p0_rows` in `mechanisms.json`. The benchmark records are source claims, not Jet wins: every domain performance object reports no registered gauntlet cell. The main quantitative leads are Faust's source-local 2.8x vector example, Embree's 34M/131M triangle-per-second BVH examples, Halide's reported 22x box-filter comparison, libvips' 0.15 s/9.5x 10,000×10,000 pipeline, Zerodha's approximately 1 minute versus 18 minute 2,000-page case, and XR platform frame budgets; none is a matched Jet result.

## Owner gates

| Suggested decision | Gate | Affected mechanisms/domains |
|---|---|---|
| `D-MEDIA-RASTER1` | Ratify the raster/image value: dimensions, layout, channels, dtype/range, alpha, color profile, metadata, views, tiles, ownership, and codecs. | `M-RASTER`, `M-COLOR`, `M-SCI-FORMATS`; video, rendering, image, 3D, AR, creative, publishing |
| `D-MEDIA-GRAPHICS1` | Choose the portable graphics device/shader contract, supported targets, pipeline/resource lifecycle, validation, capture, precision, and GPU authority; keep it distinct from `M-GPU-KERNELS` compute. | `M-GPU-GRAPHICS`, `M-GPU-KERNELS`, `M-CANVAS-2D`; rendering, video, 3D, AR, creative |
| `D-MEDIA-SCENE1` | Ratify typed scene/entity graphs, mesh topology, transform stacks, assets, layers, composition, inspectors, and mutation/cache rules. | `M-SCENE-GRAPH`, `M-MESH-STENCILS`, `M-WORKFLOW-DAG`; video, 3D, rendering, AR, creative, acoustics |
| `D-MEDIA-AUDIO1` | Set the sample-accurate audio graph and realtime callback law: buffers, ports, rates, events, deadlines, allocation, state, plugin ABI, devices, and offline equivalence. | `M-AUDIO-GRAPH`, `M-PLUGIN-ABI`, `M-HARDWARE-IO`, `M-FFT`, `M-DSP-FILTERS`; audio, acoustics, video, creative |
| `D-MEDIA-TIME1` | Choose media clocks, frame/sample time bases, keyframes, ranges, seeking, EOS, A/V sync, timeline values, and OTIO/editorial loss reporting. | `M-MEDIA-PIPELINE`, `M-TIMELINE`; video, audio, acoustics, 3D, creative |
| `D-MEDIA-TEXT1` | Choose font discovery, fallback, HarfBuzz/OpenType shaping, source clusters, glyph buffers, document layout, and publication targets. | `M-TEXT-SHAPING`, `M-DOCUMENT-PUBLISHING`, `M-NOTEBOOK`; typography, creative, video titles |
| `D-MEDIA-REPLAY1` | Reuse the e14 game Scene/Replay/SceneProbe identity for media frames, assets, timing, and captures; add provider-specific metrics without a second replay law. | `M-HEADLESS-REPLAY`, `M-EVIDENCE`; all nine domains; e14 cards #2484–#2491 |

## Contradictions and defects

| Topic | Source 1 | Source 2 | Synthesis |
|---|---|---|---|
| Raylib status | `dx2/rendering-graphics/probes/raylib_window.out:1-4` and the creative-coding twin emit `frames 1`, `input false`, `audio false`, and `core.game.raylib bridge ready`. | `~/.cache/jet-luna/dx/games/probes/raylib_window.txt:1-10` exits 1 with E1803 because native display requires undecided GPU authority. | Count raylib as an implemented headless bridge only; native graphics remains an authority-gated gap. `DEF-MEDIA-RAYLIB-AUTH-001`. |
| Headless evaluator | `dx2/3d-animation-dcc/probes/core_game_headless.txt:7-18` reports E0956 at `GameSceneNew` and explicitly calls it an evaluator boundary. | `dx2/ar-vr-xr/probes/headless_game.out:2-12` and `dx2/rendering-graphics/probes/game_headless.out:2-14` emit successful deterministic transcripts. | The Scene/Replay contract is source/test-backed, but cheap `jet run` evaluator coverage is not uniform; retain E0956 as an open boundary, not a semantic contradiction. |
| Tensor versus raster | `image-processing/census.json` marks ranked Tensor/View, broadcasting, and shape operations already implemented. | The same census marks first-class raster, codecs, color, display, and interop as real gaps; `check_sources.json:20-23` proves E1001 for `core.image`. | Reuse `M-LINALG`, but add `M-RASTER`; Tensor is not an image value. |
| Shared audio device rows | `acoustics-audio-engineering/report.md` owns duplex device and callback rows and has the `core.audio` E1001 probe. | `audio-music-production/report.md` says device capture/playback is covered by acoustics and focuses on plugin/DSP/graph production. | Keep `M-HARDWARE-IO` rows in acoustics and cite them from audio production; do not duplicate the device mechanism. |
| Performance evidence | All nine `performance.json` files carry incumbent mechanisms and published numbers. | All nine mark no registered domain gauntlet cell; the reports warn that no Jet result exists. | Preserve numbers as leads and gates; no performance win is claimed. |

Open defects are enumerated in `defects.json`: one raylib authority boundary, one E0956 evaluator boundary, and six E1001 missing-module rows for audio, video, image, graphics, XR, and document surfaces. The exact probe output remains the authority for each diagnostic.

## Source ledger

The synthesis reads the nine domain `census.json`, `performance.json`, `report.md`, and `claims.json` files under `~/.cache/jet-luna/dx2/`, the prior family mechanism files under `~/.cache/jet-luna/dx2/_families/{science-numerics,engineering,life-sciences-health}/`, the e14 games census/report and card table, and the local probe captures named above. External API and benchmark URLs are retained in each domain report and in `mechanisms.json` prior-art entries. No Tower writes, builds, or new measurements were performed.
