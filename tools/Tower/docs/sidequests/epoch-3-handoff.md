# Epoch 3 handoff

**Status:** PM handoff after 2026-06-27 ratification sweep.

## Fully Unblocked Implementation Cards

- c60 `core.raylib`: D-RAYLIB1=A, plan `raylib-graphics`.
- c66 Unicode text: D-GRAPHEME1=B, plan `unicode-text`.
- c67 compression codecs: D-CODECS1=A, plan `compression-codecs`.
- c74 `pub(package)`: D-PUBPKG1=A, parser/sema/docs/ui snapshots needed.
- c82 vector swizzles: D-SWIZZLE1=A, include lvalue swizzles and overlap diagnostic.
- c125 Cranelift JIT: D-JITDEP1 + D-JIT2 closed, plan `jit-cranelift`.
- c129/c130/c131 user derives/reflection: D-METAREFLECT1=B + D-METADERIVE1=A closed;
  remaining template-quote choice is follow-up, not a blocker.
- c164 HTTP core library: D-NETDEP1 + D-HTTPLIB1-4 closed; client stage first because it
  unblocks comptime `fetch(url, sha256:)`.
- c1hixgdn compiler seams: D-COMPILERSEAMS1/2 closed; split workspace crates with
  `jet-<seam>` names.

## Still Decision-Gated

- c18 effect prohibition waits on D-PROP2 spelling.
- c20 protocol/session types waits on D-PROTO2 declaration spelling.
- c23 replay waits on c18 plus D-REPLAY2 marker name.
- c31 richer match arms waits on D-MATCHARM2 precedence/grouping.
- c37 discard fallible result waits on D-IGNORERET2 sigil.
- c51 Display/Debug waits on D-DISPLAYDBG2 interpolation spelling.
- c65 BigInt gates c57 Decimal implementation; float-money lint can ship first.
- c69 opt-in GC waits on D-DEP-GC1.
- c102 structured concurrency is the top-level concurrency surface gate. D-ASYNCRT1=A
  already chooses M:N green threads under tasks/channels; c36 and c103 fold into nursery
  combinators. See `tools/Tower/docs/plans/epoch-3/concurrency-vision.md`.
- UI stack cards wait on D-SIGNAL1 and D-RENDERTGT2 before backend/component work.

## Agent Order

1. Burn down ready compiler-surface cards with small blast radius: `pub(package)`,
   swizzles, compiler seams.
2. Then build foundation tracks: JIT, user-derives/reflection, HTTP client.
3. Then package/library bridges: compression, Unicode, raylib.
4. Keep decision-gated cards in `deciding`; do not invent spellings.
