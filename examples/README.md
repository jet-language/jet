# Examples

`canon.jet` is the compiling syntax showcase. `features/` groups every feature
example by topic (D-REPO-EXAMPLES1); `features/expected/` mirrors the tree with
each example's golden output. Run any example directly:

```
jet run examples/features/basics/hello.jet
```

Suggested learning order:

| Topic | What lives there |
|---|---|
| `basics/` | hello, functions, values, branches, loops, closures, pattern matching |
| `types/` | structs, enums, traits, generics, distinct types, typestate, tuples |
| `errors/` | error families, `?` propagation, panic, rollback, discard rules |
| `collections/` | lists, maps, sets, deques, iter adapters, parallel iteration |
| `text/` | strings, regex, unicode, hex/base64, bigint, decimal |
| `modules/` | imports, module files/dirs, packages, visibility, re-export |
| `comptime/` | comptime blocks, splice, reflect, embed, doctests |
| `effects/` | capability sigils, taint, pure, effect prohibition, grants |
| `memory/` | ownership, arenas, stored refs, rawptr, uninit, zero-copy, GC |
| `serde/` | json/csv/toml/yaml, derives, schema migrations, fidelity |
| `io/` | cli, files, stdin, paths, logging, terminal |
| `net/` | http client/server, routes |
| `concurrency/` | tasks, channels, select, race/cancel, deadlines, scheduler |
| `crypto/` | envelope, signing, key migration |
| `ui/` | view tree, styles, component kit, motion, a11y, reactive TUI |
| `web/` | hybrid JS DOM + Wasm compute — see `docs/sidequests/web-backend-wasm.md` for the full example index, build commands, and unsupported-breadth list |
| `lowlevel/` | ffi, c layout, simd, freestanding, cross-compile |
| `tooling/` | tests, bench, debug, property tests, build profiles |
