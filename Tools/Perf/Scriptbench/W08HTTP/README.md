# w08: http

200 sequential GETs against the local fixture server (`server.js 18080 0`, about 1 KB bodies).

Origin: the 2026-10-28 audit bench (w01-w15, the bench the scripting cards were filed from). Cards: #5072 c7.

The audit's Lua peer needed LuaSocket, and no pinned Lua environment with it is present, so Lua is not a peer here. The bash peer uses curl.

The expected output is `expected.out`; `run.jet` is the Jet program, and `peer.*` are the peers named in `../manifest.json`.
