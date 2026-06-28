# Compression codecs

**Card:** c67 / cviw4t7. **Decision:** D-CODECS1=A. **Status:** ready to build.

## Goal

Ship standalone `core.compress.gzip` and `core.compress.zstd` APIs. Archive containers
remain separate; a user can compress/decompress byte streams without opening zip/tar.
Brotli is a follow-on.

## Constraints

- Package/runtime-side only; no compiler `Source/` deps.
- Pure-Rust bootstrap crates are acceptable under the owner-approved stdlib bridge
  posture and carry the native-ize obligation.
- Stream APIs and whole-buffer APIs should share one implementation path.

## Build Plan

1. Add package/module registration for `core.compress.gzip` and `core.compress.zstd`.
2. Add whole-buffer APIs: `compress(bytes)`, `compress(bytes, level:)`,
   `decompress(bytes)`.
3. Add streaming reader/writer wrappers over the existing `Reader`/`Writer` model.
4. Wire HTTP content-encoding and archive `.tar.gz` use to the same codec layer where
   possible, avoiding duplicate gzip paths.
5. Add examples: read `.log.gz`, write `.zst`, stream-compress a file.

## Verification

- Round-trip tests for gzip and zstd, including empty input and corrupt input.
- Golden examples for whole-buffer APIs.
- Dependency audit: codec deps never appear in compiler `Source/` crate.

