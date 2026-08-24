# C library port pipeline

Card #1920 defines a small, repeatable port lane. The lane targets known-good,
deterministic C code. It does not translate a whole project by default.

## Plan of record

1. Select one bounded API from a source with a stable revision and license.
2. Keep the selected C source as a local corpus record.
3. Port the selected API to one Jet source module with typed inputs and outputs.
4. Run the C source as an oracle and record its vectors.
5. Run the Jet port with `jet test`, its golden example, and `jet prove`.
6. Record source identity, scope, hashes, vectors, and every validation command.

## Corpus selection

Choose code that meets all of these rules:

- The source has a public revision, license, and stable source URL.
- The selected API is deterministic for fixed bytes and a fixed seed.
- The selected code has no allocator, thread, platform, or foreign-runtime need.
- The selected surface is small enough for a reviewer to read in one change.
- The corpus includes full-block, tail, empty-input, and boundary cases when the API has them.

Record rejected scope. Do not silently port adjacent APIs, macros, tests, or
platform branches. A later card can select them as a new corpus.

## Port

Translate the selected API into one reusable Jet source module. Keep the
public surface close to the C surface, but use Jet ownership and typed values
at the boundary. Make C unsigned overflow explicit with `wrapping(...)`. State
any source assumption, such as byte order, in the module and provenance
record. Keep the pilot entry source self-contained when a cross-file AOT seam
would add unrelated compiler risk.

Do not add CFFI when the goal is a Jet port. CFFI is an adapter; this lane
replaces the selected implementation with checked Jet code.

## Validation

Run the C corpus as an oracle. Compare its output with the recorded vectors.
Then run these Jet checks:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet test \
  --show-default examples/features/ports/<library>/run.jet
TMPDIR="$HOME/.cache/jet-test-scratch" JET_GOLDEN_FILTER=ports/<library> \
  scripts/agent/jet-env cargo test --test golden examples_compile_and_run -- --nocapture
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet prove \
  examples/features/ports/<library>/run.jet --json
```

`jet test` checks named behavioral cases. The golden example checks the
published output. `jet prove` records front-end and test evidence for the same
source. Compile the C oracle with warnings as errors before comparing output.
Run only the target checks for the port during the pilot.

## Provenance record

Put one `provenance.md` beside the port. Record:

- library name and selected API;
- upstream URL, revision, license, and source file or line scope;
- local corpus SHA-256;
- Jet port SHA-256;
- oracle vectors and their input encoding;
- exact validation commands and result lines;
- explicit non-goals and known source assumptions.

Do not record a vague “translated from C” claim. A reviewer must be able to
identify the source bytes, the port bytes, and the proof inputs.

## Pilot

The first pilot ports `MurmurHash3_x86_32` from Peter Scott's public-domain
standard-C port. It uses `[U8]` input, a `U32` seed, and a `U32` result. The
pilot selects only the 32-bit API. The 128-bit APIs and source-specific
unaligned-read optimization remain out of scope.

Files:

- `examples/features/ports/murmur3_x86_32/corpus/murmur3_x86_32.c` — selected C corpus;
- `examples/features/ports/murmur3_x86_32/corpus/oracle.c` — vector oracle;
- `examples/features/ports/murmur3_x86_32/run.jet` — Jet port, tests, and golden entry;
- `examples/features/ports/murmur3_x86_32/provenance.md` — source and proof record;
- `examples/features/expected/ports/murmur3_x86_32.out` — golden output.
