# D-NET-READINTO1 review: read_into appends into a caller-owned buffer

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-NET-READINTO1. Earlier pass: BallotFix REPORT.md, section D-NET-READINTO1.

## Question

Should byte streams gain `n: stream.read_into(buffer, limit)`? It would append at most `limit` bytes to a
buffer the caller holds, so a steady read loop asks for no memory.

## Findings that changed the ballot

1. **The gain is allocation, not speed.** The cost was measured for the first time. All three programs are
   built binaries, run five times each, on a0.0.43.

   | Program | What it does | Median | Peak RSS |
   |---|---|---|---|
   | m1 | 100,000 rounds: write 4,096 bytes, `read(4096)` until 4,096 arrive (local TCP pair) | 1.094 s (0.951–1.291) | 14.6 MB |
   | m2 | as m1, each chunk appended to one held buffer and cleared, as TLS does | 0.975 s (0.927–1.007) | 14.5 MB |
   | m3 | 100,000 copies of a 4,096-byte list, each mutated so the copy is real | 0.026 s (0.020–0.038) | 14.7 MB |

   A local read round costs about 11 µs. The exact-size list and the copy A removes cost about 0.26 µs, which
   is about 2%. m1 and m2 differ within noise. The ballot now argues A as a read loop that asks for no
   memory, which D-FREESTAND-ALLOCAPI1=A asks of Core. It adds a named loss: the speed gain is small.
   (m3's first version used an unmutated `.copy()` and ran in 1 ms, so a plain copy of a list is shared until
   written. The mutation forces the copy.)
2. **Line drift.** The TLS record read is now `Core/Net/Tls.jet:1104` and its `rx.extend(bytes)` is at `:1106`
   (were 1093 and 1095). The clear rule is `ir_list_clear_in_place`, `Compiler/JetIR/Source/BuildExpr.jet:3093-3117`.
   It keeps the buffer only for a sole owner (`jet_rt_block_unique`); otherwise the list becomes the empty list.
3. **Wording.** The situation said "We recommend". The lesson, surface lesson, explainer, Go note and A gist
   said "you" or "your". The Rust comparison's `read_buf` is tokio's, and it is now named so. std's
   `Read::read_buf` is unstable.

## Code cited (c460)

- `Compiler/JetRuntime/Source/NetHandleRoutes.jet:67-118`, `net_tcp_read_result`. The 48-byte scratch is
  `jet_rt_alloc(I64{48}, I64{8})` at `:78`, freed at `:116`. The reused block is
  `net_read_buffer_take`/`net_read_buffer_give` (`:175`, `:191`, used at `:83`, `:113`). The exact-size list
  and copy are in `net_read_publish_bytes` (`:208-211`). NetUnix.jet:59-91 has the same shape.
- `RT_HOST_U8_STRIDE` is 1 (`Compiler/JetRuntime/Source/Collections.jet:7`), so the copy is the byte count.
- Hot callers: `Core/Http/Server.jet:1217,1226`, `Core/Http/Client.jet:735,746`, `Core/Net/Tls.jet:1104`,
  `Core/Net/Net.jet:287`.
- `core.encoding.__read_into` is used by Csv.jet, Stream.jet and Xml.jet. This is unchanged.

## Probes

| Probe | Result |
|---|---|
| r02 read loop | `read 4` |
| r03 `read_into` | E0102 |

## Peers (sourced)

- Go `io.Reader` fills a caller slice from the start. <https://pkg.go.dev/io#Reader>
- tokio `AsyncReadExt::read_buf` appends into spare room. <https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html#method.read_buf>
- Java `ReadableByteChannel.read(ByteBuffer)` reads into the buffer's remaining room.
  <https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/nio/channels/ReadableByteChannel.html>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Busy servers pay for the per-read list | Measure it (m1, m3) | **Weak**: about 2% of a local round. Reframed as allocation-free |
| Steady loop allocates nothing | The 48-byte scratch at :78 | Holds only with the scratch moved to the stream, as A's technical note says |
| Clear keeps the room | Shared buffer | Holds for a sole owner only. Named as a loss |
| 0 means end of stream | limit 0 | Holds: a limit below 1 is invalid input |
| Plain language | "you", "We recommend" | **Failed**. Fixed |

## Remaining losses

- Generic Reader code cannot call `read_into` until a trait default follows.
- Kept bytes need their own memory under every option.
- The speed gain is about 2% of a local read round.
