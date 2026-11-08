# w15: sleep

Sleep 100 ms thirty times.

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6090.

The audit version polled a file's size between the sleeps; the cards' cells are the card-close sleep loop reproduced here. The jet-run cells are report-only (D-PERF-RUNCELL1=B). Plain Lua has no sleep, so only LuaJIT (through its FFI) is a peer.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
