# w11: fetch

Fetch 100 URLs, at most 32 at a time, from the local fixture server (`server.js 18411 20`, each answer after 20 ms).

Origin: the 2026-11-01 card-close bench (the cells logged on its cards). Cards: #6085.

The audit bench allowed 10 requests in flight; the cards cite the card-close limit of 32, reproduced here. Only peers with a concurrent HTTP client in their standard install run: python, node, bun, deno and ruby.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
