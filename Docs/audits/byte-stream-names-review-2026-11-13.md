# Adversarial review: one naming rule for Core's streams (D-BYTE-TRAITS2=A, rework)

Date: 2026-11-13. Author: Claude Opus, lane G5DevX2, who also wrote the
reworked ballot. This file is the claim-by-claim evidence the rework rests on,
with each claim attacked as the review rule asks. It does not replace the
fresh-agent review the rule requires: until a reviewer who did not write A
signs the verdict below, no text may call A better than the designs it is
compared with.

Code basis: dev at 8e1c67443b. Probe r3 ran on the seed-b17 compiler at
`--optimize=none`, the default level and `--optimize=full`; its source is in
the ballot's probe folder, `$JET_SCRATCH_ROOT/lanes/Main/ballots/probes/D-BYTE-TRAITS2/`.
A is not built, so every statement about A's behavior is a prediction from the
code A's cutover changes, and is marked as one.

## What A claims

1. One naming rule fits every Core stream: the plain name takes text and the
   `_bytes` name takes bytes, as core.files and the screen already do.
2. Writer's `write_bytes` writes every byte or fails, so no generic write can
   silently send part of a buffer.
3. A user writer joins every codec with two methods, `write_bytes` and
   `flush`; `write(text)` and `write_line(text)` come from default bodies.
4. A required `flush`, with a codec flush that ends the pending record line,
   lets a tailed JSONL or CSV log show each flushed record.
5. The beginner's `out.write("hi")` keeps working.

## Claim 1: one rule fits every stream

**Holds within stated limits.**

The rule exists today in two places. core.files has `read(path)` and
`write(path, text)` for text and `read_bytes` and `write_bytes` for bytes
(`Core/Files/Files.jet:954-992`). Stdout and Stderr have `write(text)`,
`write_line(text)` and `write_bytes(bytes)` (`Core/Term/Term.jet:113-152`).
The other rule, `write` for bytes and `write_text` for text, is on a child's
stdin (`ProcessHandles.jet:516-545`), TCPStream (`NetworkHandles.jet:102-114`)
and the Writer trait (`Compiler/Prelude/Traits.jet:249-253`).

Strongest alternatives, fairly stated:

- **Rust and Go** name the byte method `write` and reach text through a
  formatting layer (`write!`, `fmt.Fprint`, Go's `io.StringWriter.WriteString`).
  Their best case: in a byte-first standard library, one primitive name for
  the one primitive operation. Jet's standard library is not byte-first:
  `files.write` takes text, so copying their names leaves Jet with two rules.
- **Python** keeps text and bytes on separate objects (`sys.stdout` and
  `sys.stdout.buffer`). Its best case: no method ever takes both kinds. The
  cost is a second object per stream, which is option F.
- **Java** separates `Writer` (text) from `OutputStream` (bytes) as two type
  families. Its best case is the same as Python's, at the cost of wrapping
  every byte stream in a text adapter to write text.

Attacks:

- *Migration.* Pipes, sockets, UnixStream, FileWriter and the Reader trait
  rename their byte methods. `census-v3.mjs` over 20,620 `.jet` files counts
  195 `write_all`, 53 `write_text` and 80 `read_text` calls, plus 137 lines
  to read by hand. The renames are mechanical, and each wrong old call is a
  compile error (a `[U8]` reaching a `String` parameter is E0112). None
  becomes wrong output.
- *Habits.* Rust and Go users write `conn.write(bytes)`. Under A that is
  E0112. Today's E0112 fix line for this case advises interpolation (probe
  p3), which is wrong, so A must add a fix line naming `write_bytes`. This is
  a cost that can't be removed, mitigated by the diagnostic.
- *Read side.* TCPStream's `read()` with no limit returns text today, and
  `read(n)` returns bytes. Under A, `read(n)` takes the text meaning and
  `read_bytes(n)` the byte one. A reader of old code could misread which
  `read(n)` they hold; the types differ (`String` against `[U8]`), so every
  confusion is a compile error. [INFERENCE: no probe; A is not built.]

## Claim 2: no silent short writes

**Holds.**

The trap is real and present in Jet. A child's stdin `write(bytes)` takes
what the pipe accepts and returns the count (core-library.md:2988).
`Compiler/JetCli/Source/Cli/Doctor.jet:116` and `Repl.jet:620` call it and
`.drop(...)` the result, so a body larger than the pipe accepts would be cut
short with no error. Rust's Clippy treats the same pattern as a correctness
lint, `unused_io_amount`, denied by default
(https://rust-lang.github.io/rust-clippy/master/index.html#unused_io_amount).

Go's best case removes it at the contract: "Write must return a non-nil error
if it returns n < len(p)" (`src/io/io.go`). A's `write_bytes` takes that
contract, and the trait has no partial write at all.

Attacks:

- *Lost capability.* A generic writer can no longer write part of a buffer
  and stop at a deadline. Core's generic writer users, the codecs, never do:
  each writes whole buffers and flushes. FileWriter's present `write` is a
  fake partial write that writes everything and returns the length
  (`Files.jet:430-436`). The concrete pipe and socket types keep the
  capability as `write_some`, which TLS and the HTTP client use with
  deadlines (`Core/Net/Tls.jet:1094`, `Core/Http/Client.jet:680`).
- *A new trap?* `write_some` can still be called and its count ignored. The
  name now says it may stop early, and it is off the trait, so generic code
  can't reach it. [INFERENCE: Jet has no unused-result lint for `Int`; the
  rename is the only guard.]

## Claim 3: a user writer needs two methods

**Holds, given one compiler addition.**

Probe r3 declares a user trait with required `write_bytes` and `flush` and
default `write(text)` and `write_line(text)`. `Counter` writes only the two
required methods. `Screen` replaces `write` with its own. A generic
`record<W: Sink>` calls all four. At all three levels it prints
`counter bytes 11 flushes 1`, `n=8` and `screen texts 1 bytes 7`: the
defaults ran for Counter, and Screen's own `write` ran in place of the
default.

Attack: the Prelude's trait rows carry no default bodies today
(`Compiler/JetFoundation/Source/Registry/OwnedTraitRows.jet:45-48`), and no
Prelude trait declares one (`Compiler/Prelude/Traits.jet`). A has to add
them. That is work, not a design cost: user traits already follow the rule
(`Examples/Modules/Library.jet:10-17`).

Comparison: Go's io.Writer has one method, so a Go writer needs one; Go then
reaches text through `fmt` and flush through separate interfaces. Rust's
Write needs `write` and `flush`, with `write_all` and `write_fmt` provided.
A matches Rust's count and adds text writes to every writer.

## Claim 4: a tailed log shows each flushed record

**Holds, with the held-back line end fixed in the same change.**

Each codec's flush calls the file's flush directly today
(`CborStream.jet:694`, `Csv.jet:729`, `Stream.jet:1525` and `:2001`,
`Xml.jet:304`). Through a generic `W` it can call only the trait, so the
trait needs `flush`. The JSONL and CSV writers hold back each record's line
end (`Stream.jet:1955-2013`, `Csv.jet:660-671`). The ballot's probe p4 shows
the file one whole line behind after each flush, so the codec's flush must
write the pending line end first.

Attack: the old ballot listed a loss here, that a file cut after a flush no
longer lacks its last line end. Neither format gives that missing line end a
meaning. JSON Lines says a final line terminator is "strongly recommended but
not required" (https://jsonlines.org/). RFC 4180, section 2 item 2, says the
last record "may or may not have an ending line break". No reader could rely
on it, so A loses nothing a reader could use.

Comparison: Java's OutputStream gives flush a body that does nothing (option
G). Its best case is less code in unbuffered writers. The cost is that a
buffering writer that forgets flush compiles and stalls a log silently. A
requires a one-line flush instead.

## Claim 5: the beginner's screen write keeps working

**Holds.** Stdout and Stderr keep every name they have under A, and the
reference example `out.write("done")` (core-library.md:2260) is unchanged. The
old A made that line E0112.

## Verdict

**Holds within stated limits**, pending the fresh-agent review:

- The renames on pipes, sockets, FileWriter and Reader are a one-time
  migration. Their compile errors need the new E0112 fix line.
- Generic code gives up partial writes. The concrete pipe and socket types
  keep them as `write_some`.
- The Prelude's trait rows must gain default bodies.

## Gaps to put on cards

Each gap goes on #6759 as a criterion, unless Main links an existing one.

1. E0112 names `write_bytes` when a `[U8]` reaches a text write, with a UI
   snapshot.
2. Prelude trait rows support default method bodies, and Writer's `write` and
   `write_line` defaults run through a generic bound, as a golden at O0 and O2.
3. Every codec's flush writes its pending record line end before flushing its
   writer, as a golden that reads the file after each flush.
4. Doctor.jet and Repl.jet write the child's stdin with `write_bytes`, so a
   short pipe write is an error.
