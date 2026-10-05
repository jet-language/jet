# #3395 — I/O and network iterator source contracts (ITER-F064)

Question: do line/split producers, accept loops and address resolution keep
source-specific delimiter, partial-line, error, resume, waiting and cleanup
contracts through the one iteration boundary?

Binary: `jet-debug-snapshot14` (safe-jet.sh), 2026-09-29. Probes in
`~/.cache/jet-dev/scratch/Closer04/`: `lines.jet` (file) and
`stdin_lines.jet` (stdin).

## c1 — delimiters, partial final line, byte errors, resume

`lines.jet` writes the bytes `a LF b CR LF LF c` (no final newline) with
`files.write_bytes` and prints each `files.open(p).lines()` item as bytes. It
then breaks after the first item, rewrites the same file, and iterates a
second file with the bytes `x LF 0xFF LF y LF`.

`jet run` and AOT agree up to the invalid byte:

```
mixed a LF b CRLF empty c(partial):
  item 1: [97]
  item 2: [98]
  item 3: []
  item 4: [99]
  end after 4 items
early break first: a
reopen for write after break: rewritten
invalid utf8 middle line:
  item 1: [120]
```

At the invalid line:
- `jet run`: `internal compiler error: InvalidInput(IOContext { operation:
  Read, resource: Ok(".../bad.txt"), … cause: Ok("stream did not contain
  valid UTF-8") })`, exit 101.
- AOT: `Stop [E3001]: panic: invalid input during read '.../bad.txt': stream
  did not contain valid UTF-8`, exit 70.
- `--interpret`: E0956 "interpreter file line iterator requires a checked
  FileReader value" at `loop line in input.lines()` (lines.jet:7). No item
  runs.

`stdin_lines.jet` (`io.stdin().lines()`) with the same inputs piped in:

| input | `jet run` | `--interpret` | AOT |
| --- | --- | --- | --- |
| `a\nb\r\n\nc` | `[97] [98] [] [99]`, end after 4 | same | same, rc 0 |
| `x\n\xff\ny\n` | `[120]`, then ICE "read stdin: stream did not contain valid UTF-8" | E0956 "Read stdin: stream did not contain valid UTF-8" (stdin_lines.jet:7; Why: "The interpreter line-reader adapter rejected the checked operation") | `[120]`, then E3001 panic, rc 70 |

Findings:
- LF and CRLF are stripped.
- Empty lines are yielded as `""`.
- The partial final line is yielded.
- An early `break` releases the reader: rewriting the file succeeds on run
  and AOT.
- An invalid UTF-8 line has no typed per-item error. It stops the program,
  with a different stop on each tier (ICE rc 101 on run, E3001 rc 70 on AOT,
  E0956 on interpret), and nothing resumes.

Verdict c1: FAIL. The byte-error and resume cells break I9 and have no
typed outcome.

## c2 — accept states

Source read: `net.tcp_accept(listener) -[Net, Time.Wait]> TCPStream NetError!`
(Core/net/net.jet:118) is a single blocking call. There is no accept
iterator or `incoming()` producer. "Waiting" is the blocking call, a
"failed accept" is `Err(NetError)`, and there is no "permanent end" state.
Verdict: partial and unsupported as an iterator. An `incoming()` producer
over the generic iterator protocol is new API and needs its own ballot.

## c3 — address resolution

Source read:
- `dns_a/dns_aaaa(name, ms)` and `dns_a_at/dns_aaaa_at(server, name, ms)`
  (Core/net/net.jet:357-379) are effectful `-[Net, Time.Wait]>` calls.
- Each takes an explicit timeout and returns an eager finished
  `[IPAddr] NetError!`.
- The host resolver and the explicit wire resolver are distinct functions.

So resolution is not modelled as a pure zero-cost source.
`net/dns_lookup` golden: MATCH on `jet run`. `--interpret` fails with E3412 "`core.net.getservbyname()` is not available at comptime". AOT fails with rustc E0308 in generated dns_lookup.rs:137946 (`jet_net_dns_ptr` argument). Verdict c3: the effectful shape holds (source), tier parity FAIL.

## c4 — cleanup and readiness

The file reader closes on early `break` (c1 evidence on run and AOT). On
channel readiness (F031), no new evidence was produced; see #3224/#3226.

## Goldens

`golden_diff.sh` (stdout diff vs `Examples/features/expected/`; stdin fed with the
`tests/common/mod.rs` answers `alpha\nbeta\n` where registered):

- `io/stream`: run MATCH, AOT MATCH. Interpret fails with E0956 "interpreter
  file line iterator requires a checked FileReader value" (stream.jet:17).
- `io/stdin_readline`, `io/stdin_input`: MATCH on run, interpret and AOT.
- `io/stdin_read_all`: the output is identical on all tiers. It differs only
  by the golden's final blank line, which the `$(…)` capture strips; that is
  a harness artifact.
- `io/stdin_filter` (null stdin): empty on all tiers, the same as the golden.
- `net/socket_echo`: fails on all tiers with E2404 (`socket_host` /
  `socket_to_string` error route), E0111/E1101 (`listener` captured by a
  task).

## c5

`io/line_producer_contract` was not added. The observed invalid-line
behaviour is a defect and must not be blessed.
