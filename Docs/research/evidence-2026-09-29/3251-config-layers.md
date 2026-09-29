# #3251 — Explicit multi-file configuration inclusion (CORE-F024, D-CORE-CONFIG-LAYERS1=A)

Question: does Core load an explicit, rooted, bounded JSON configuration graph
under the ratified contract, while plain decoding stays IO-free?

Binary: `jet-debug-snapshot14` (safe-jet.sh), 2026-09-29.

## Current state (source read)

- There is no loader: `load_json_layers`, `ConfigLimits` and `include_key`
  appear nowhere in Core, Compiler, crates/jet-codegen/src, Examples or tests
  (grep).
- Reusable parts:
  - `core.args.merge(base, overlay)` (Core/args/args.jet:124): objects
    overlay, arrays and scalars replace;
  - `core.encoding.json.parse/decode(text) -> DataTree EncodingError!`
    (Core/encoding/json.jet:83,90), with no effect row;
  - `core.files.read_bytes`, `canonicalize`, `is_symlink`, and
    `Path.is_within` (lexical).
- There is no rooted open (openat/O_NOFOLLOW) primitive.

## c1 — contract statement (from the ratified ballot, not an implementation)

D-CORE-CONFIG-LAYERS1=A:

```
load_json_layers<T>(paths: [Path], root: Path, include_key: String,
                    limits: ConfigLimits = ConfigLimits.safe()) -> T
```

The call lives in `core.args` and is fallible, with structured
`[FieldError]`/`EncodingError`/`IOError` causes.

| Rule | Contract |
| --- | --- |
| Base directory | Relative includes resolve against the containing file. |
| Rejected paths | Absolute paths, `..` escape, symlink/reparse traversal and active-stack cycles fail before any access outside `root`. |
| Limits (`ConfigLimits.safe()`) | `EncodingLimits.safe()` plus `max_files=64`, `max_include_depth=32`, `max_total_source_bytes=67108864`. |
| Provenance | Errors carry the file/field/include chain and never secret values. |
| Permissions | The caller needs FS read authority, visible in the call. |
| Excluded | No env interpolation, no global cache, no `core.config` tree. |

This contract is recorded, but nothing implements it. c1 is met only as a
statement.

## c2 / c7 — pure decode stays IO-free

Probe `~/.cache/jet-test-scratch/Closer04/nofs/cfg_decode.jet`:
- The package authority is only `[IO, Mem.Alloc]`, with no FS.
- `base.json` beside the program contains `{"count": 1, "secret": "leak"}`.

The program parses `{"include": "base.json", "count": 2}` and merges it with
an in-memory base.

`jet run`:

```
app: {"count":2,"include":"base.json"}
merged (base then app): {"count":2,"include":"base.json","name":"base"}
merged (app then base): {"count":1,"include":"base.json","name":"base"}
```

What the output shows:
- The include member stays plain data.
- `base.json` is never read, since `secret` is absent from the output.
- The program compiles and runs without FS authority.
- Layer order is caller-controlled through `merge`.

Unrelated imports cannot activate a loader, because no loader exists.

Other tiers of the same probe:
- `--interpret`: E0956 ``core.builtin.map_has_key()`` isn't supported by the
  current evaluator yet (reached through `args.merge`).
- AOT: rustc E0308 in generated cfg_decode.rs:178648,
  `jet_std::DataEvent::Int(..)`: expected `i64`, found `JetInt`.

So c2/c7 are shown on `jet run` only.

## c3 — owner routing

The API and placement choice was balloted and ratified as
D-CORE-CONFIG-LAYERS1=A (home `core.args`, reviewed via #3396). No further
routing is needed. Met.

## c4, c5, c6, c8

Not met: the loader, rooted open, caps and provenance are unimplemented, and
the `io/config_layers` golden cannot exist yet.

## Follow-ups

- Implement per the card plan: `ConfigLimits` and `load_json_layers` in
  `core.args`, plus a host rooted-open primitive. This is the E_BUILD work.
- Defect: `args.merge` is unsupported in the interpreter (map_has_key).
- Defect: AOT fails on JSON DataTree integer lowering (DataEvent::Int i64 vs JetInt).
