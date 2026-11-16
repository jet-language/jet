# Primitive probe: text

## What I built
I built a Jet package that reads a valid-UTF-8 1 MiB multilingual corpus, trains 12 greedy BPE merges, and encodes a mixed-script sample. It segments combining marks, emoji ZWJ, Cyrillic, Greek, Devanagari, Arabic, and sentence punctuation; it also scans a 100,000,000-byte log with a typed regular expression. It invokes HarfBuzz's `hb-shape` through Jet's process bridge for Arabic and Devanagari and records glyph positions or missing-glyph output.

Files:
- `pkg/package.jet` — package authority (`Exec`, `FS`, `IO`, `Mem.Alloc`, `Time`).
- `pkg/main.jet` — BPE, segmentation, normalization, regex scan, and HarfBuzz process bridge.
- `pkg/stream_scan.jet` — streaming `files.open(...).lines()` scanner.
- `pkg/nlp_gap.jet` — intentional missing-tokenizer-module probe.
- `pkg/regex_gap.jet` — intentional lookaround probe.
- `pkg/byte_gap.jet` — intentional raw-byte-view probe.
- `shape_ffi_gap/pointer_gap.jet`, `shape_ffi_gap/harfbuzz.h` — intentional C pointer/bindgen probes.
- `bench_python.py`, `bench_rust.rs` — same-fixture incumbent comparisons.

The source-of-truth checks were `scripts/agent/jet-env jet --help` and `scripts/agent/jet-env jet inspect facts`; both ran with the supplied binary. Archived research mined: `dx2/nlp-text/report.md` (Hugging Face BPE/alignment and missing `core.nlp`), `dx2/typography-publishing/report.md` (HarfBuzz buffer-to-positioned-glyph contract), `dx2/text-processing-parsing/report.md` (linear regex/search tradeoffs), and deleted `dx2/ballots/D-M-NLP1.json` (offset-preserving tokenizer/document model).

## What worked

- **Unicode counts and segmentation:** `jet run pkg/main.jet` printed `bytes=89 scalars=48 graphemes=40 words=7 sentences=4`. The family emoji stayed one grapheme; combining and mixed-script input ran without errors.
- **Normalization and caseless matching:** the same run printed `normalized=Café` and `casefold_equal=true` for NFC and `Straße`/`STRASSE`.
- **BPE over the full corpus:** the same run printed `bpe bytes=1048576 words=146653 merges=12 tokens=549946 sample_tokens=10 elapsed_ms=108233`; training and sample encoding are real Jet code, not a stub.
- **Regex compilation and scanning:** the same run printed `scan bytes=100000000 lines=1327434 matches=442478 elapsed_ms=6894`; the scanner uses runtime `re.compile` and `pattern.is_match` over every line.
- **Streaming log input:** `jet run pkg/stream_scan.jet` printed `lines=1327434 matches=442478 elapsed_ms=7031`; `files.open(...).lines()` works on the 100 MB fixture without materializing a list of lines.
- **HarfBuzz process bridge:** the same run returned Arabic positioned glyph JSON: `uni0645 cl=3 ax=1268`, `uniFEFC cl=1 ax=1222`, `uniFEB3 cl=0 ax=1716`. The Devanagari call also ran, but the available DejaVu font returned six `.notdef` glyphs; this is a font-coverage limitation, not a Jet text result.
- **Existing Unicode implementation:** `fixed_sigs.rs:2244-2297` exposes scalar/byte counts, normalization, casefold, graphemes, words, sentences, and `char_indices`; `Top/Text.rs:300-305,387-424` documents generated UAX #29 tables and implementation. `Top/Text.rs:831-832` exposes character indices as formatted strings.
- **Existing regex implementation:** `fixed_sigs.rs:3421-3498` exposes checked literals, runtime compilation, matching, captures, replacement, and splitting. The implementation is intentionally linear and rejects lookaround.
- **Incumbent timing runs:** `nix shell nixpkgs#python3 --command python3 bench_python.py` printed BPE `3197.409 ms` and scan `581.307 ms`; an optimized `rustc` binary on the same fixtures printed BPE `621.350 ms` and scan `33.083 ms`. These baselines use ordinary Unicode code-point/substring operations, while Jet's BPE uses UAX grapheme and word segmentation; the comparison is directional, not an equivalence claim.

## Gaps

### prim-text-G1 — reusable subword-tokenizer model/training capability

- **Tag:** `boilerplate`; **severity:** hurts.
- **Capability missing:** a reusable BPE/WordPiece/Unigram model value with vocabulary, merge training, encode/decode, and alignment metadata.
- **Exact evidence:** `jet check pkg/nlp_gap.jet` returned `Error [E1001]: There is no core module \`core.nlp\``. The working package has to author BPE training and encoding manually in `pkg/main.jet:48-104` (12 rounds, nested `[[String]]`, pair-count map, sequence rebuild, and sample encoding). Archived HF evidence (`dx2/nlp-text/report.md:20-24,47-54`) includes trainer, persisted tokenizer JSON, IDs, offsets, masks, overflow, padding, truncation, and decoding; none is a Jet primitive.
- **Shared areas:** `ai-ml`, `web`, `cli`, `gui` (search, completion, model input, and text controls).
- **Workaround:** implement the model in Jet, as this probe does; alignment, special tokens, batching, and persistence still require more author code.

### prim-text-G2 — allocation-aware byte/scalar/grapheme views

- **Tags:** `slow`, `boilerplate`, `call-site`; **severity:** hurts.
- **Capability missing:** a zero-copy/streaming text view that yields bytes or structured scalar/grapheme spans and offsets without allocating one `String` per unit.
- **Exact evidence:** `jet check pkg/byte_gap.jet` returned `Error [E1004]: \`core.text\` has no item \`bytes\``; changing to `core.text.fmt.bytes("é")` returned `Error [E0112]` because that formatter wants `Int`. `fixed_sigs.rs:2250-2263,2294-2297` types `scalars`, `graphemes`, `words`, `sentences`, and `char_indices` as `List<String>`; `Top/Text.rs:387-390` collects `Vec<char>` and builds `Vec<String>`, while `831-832` formats indices as strings. The 1 MiB Jet BPE took `108233 ms` versus Python `3197.409 ms` and Rust `621.350 ms`; the 100 MB streaming regex scan took `7031 ms` versus Python `581.307 ms` and Rust `33.083 ms` on the same fixtures. The incumbent code paths are not fully equivalent, but the gap is large and the Jet implementation visibly pays eager unit/allocation costs.
- **Shared areas:** `cli`, `gui`, `web`, `ai-ml`.
- **Workaround:** use eager lists and formatted indices; use `files.open(...).lines()` for bounded input. This is workable but not a byte-native or offset-rich hot path.

### prim-text-G3 — lookaround-capable regex primitive

- **Tag:** `impossible`; **severity:** hurts.
- **Capability missing:** lookahead/lookbehind semantics in the checked regex language.
- **Exact evidence:** `jet check pkg/regex_gap.jet` returned `Error [E0152]: This regex pattern is invalid at position 2`, with `Why: Lookaround is not supported; use a linear rewrite`. The implementation says the same at `crates/jet-codegen/src/Prelude/CoreLib/JetStd/Regex.rs:949-950`.
- **Shared areas:** `cli`, `web`, `ai-ml`, `gui` rule filters and search patterns.
- **Workaround:** rewrite the condition as separate linear scans or a larger explicit pattern; supported literals, runtime compilation, captures, replacement, and splitting remain usable.

### prim-text-G4 — safe typed opaque-handle FFI for shaping engines

- **Tag:** `unsafe`; **severity:** blocks direct in-process shaping.
- **Capability missing:** a safe, lifetime-tracked C handle plus glyph-buffer/position-array boundary for stateful engines such as HarfBuzz.
- **Exact evidence:** `jet check shape_ffi_gap/pointer_gap.jet` returned `Error [E3202]: Type \`*Int\` cannot cross the C boundary here`; the diagnostic requires `core.mem` and an `#Unsafe` region. `jet inspect bind shape_ffi_gap/harfbuzz.h --pkg harfbuzz -o shape_ffi_gap/harfbuzz_bind.jet` returned `Error [E3208]: ... No bindable C function prototypes found for \`harfbuzz\`` because the header exposes an opaque pointer return. The spec states pointer returns remain E3202 (`docs/spec/spec.md:1475-1480`). The package therefore used `process.cmd(... hb-shape ...)`, which produced Arabic positions, rather than an in-process typed shaping call.
- **Shared areas:** `gui`, `web`; also any package needing font shaping or another opaque C state machine.
- **Workaround:** invoke a bounded external shaping executable (the probe does this) or maintain an audited native shim; neither gives a safe Jet glyph-buffer API.

## Friction

- The BPE author code repeats pair counting and sequence rebuilding for 12 merges (`pkg/main.jet:48-89`), then repeats the merge walk during sample encoding (`92-104`). This is common model/tokenizer work, so it is counted under G1 rather than dismissed as private implementation detail.
- The text API returns copied `String` units. A caller needing source spans must parse the formatted `"byte:char"` strings from `char_indices`; there is no structured offset value at the call site.
- The streaming scanner itself is straightforward (`open`, `lines`, regex, counter), but each regex check still crosses the generic String/list boundary. The checked regex grammar is deliberately safer than backtracking engines, at the cost of pattern rewrites.
- `jet check pkg/main.jet` passed with seven warnings, including `L0507` branch-arm-table suggestions and `L2510` hidden-cost-in-loop warnings. Runtime output was correct despite the warnings.

## Defects

No Jet defects observed. The `.notdef` Devanagari shaping result came from the selected DejaVu font lacking Devanagari coverage. The intentional gap probes produced the documented diagnostics, not crashes or wrong compiler claims.

## Battery notes

Not applicable: this is a primitive probe, so no `batteries.json` is required.

## Verdict

**Buildable with listed gaps fixed.** Unicode normalization, UAX segmentation, checked regex scanning, streaming file input, and external HarfBuzz invocation all work today. A library author can implement BPE in Jet, but the implementation is large and far slower than code-point/substring incumbents on this fixture. AI/ML tokenizer alignment/model artifacts and direct safe in-process shaping remain the material capability gaps; lookaround is intentionally unavailable and must be rewritten.
