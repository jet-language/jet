# Example corpus modernization inventory

This inventory records the #2375 migration from retired example ceremony to the ratified current forms.
It is a source map for the #2396 semantic corpus guard. The guard owns enforcement.

## Maintained teaching roots

| Root or source | Teaching role | Covered work |
| --- | --- | --- |
| `examples/features/basics` | First-contact examples | Implicit top-level body in `hello.jet` |
| `examples/features/serde` | Routine data examples and derive lessons | Structural `Codable` inference; explicit derive stays where it is the lesson |
| `examples/features/io` | Routine file, process, and stream examples | Bare fallible propagation; read-only arguments |
| `examples/features/concurrency` | Task and cancellation examples | Standalone `task.race` and `task.any` |
| `examples/features/effects`, `examples/features/lowlevel`, `examples/features/crypto` | Effect and authority contracts | Inferred positive effects; explicit contracts stay |
| `examples/features/memory` | Expiring-value examples | Exact duration literals; ownership and runtime constructors stay |
| `examples/features/net` | Network timing examples | Exact duration literals; transport/API lessons stay |
| `examples/features/text` | Duration and text examples | Exact duration literals; runtime and range constructors stay |
| `examples/features/time` | Calendar and clock examples | Exact duration literals |
| `examples/features/types` | Typed literal and run-instruction onboarding | Use `jet run` for runnable examples |
| `tests/agent_workloads/adapters` | Agent workload programs | Indexed loops for sequence counters |
| `gauntlet/entries/bulkrename/jet/main.jet` | Real-program teaching example | Regex capture for filename numbers |
| `examples/suites`, `dogfood/jetpack`, `dogfood/tower` | Maintained story and product sources | #2396 classification perimeter |
| `gauntlet/measurement-manifest.json`, `tests/agent_workloads/manifest.tsv` | Manifest-owned sources | #2396 producer and adapter classification |
| `docs/first-hour.md`, `examples/README.md`, `tests/agent_workloads/llm_digest/first_program.jet` | Onboarding and index sources | #2396 teaching-source classification |
| `site/generate.jet`, `site/widget/counter.jet`, `site/dist/index.html` | Site source and derived artifact | #2396 producer-to-artifact classification |
| `examples/interop/*/.jet/bindings`, `Source/CmdCompile.rs`, `crates/jet-pkg-model/src/Package/Convert.rs` | Generated and template sources | #2396 generated-source classification |
| `tests/conformance/corpus` | Canonical executable conformance fixtures | Ratified behavior; not a routine-ceremony exception |
| `tests/ui` and `tests/fuzz` | Diagnostic and parser fixtures | Retired forms only when the fixture tests that failure |

`examples/features/serde/json_typed.jet:17` deliberately keeps `#Codable` on `ExactNumbers`:
`Decimal` has an explicit serde wire form, but the current structural registry does not provide
automatic `Encode`/`Decode` for that carrier. Removing the marker produces E2411. This is a
non-structural wire-type lesson, not redundant routine ceremony; #2375 does not change the compiler.

`examples/features/concurrency/shield_commit.jet` deliberately keeps its lexical `task.group`:
the `#Shield` lesson depends on the group close joining the shielded loser before the scope exits.
The local expert exception is occurrence-scoped in `tests/corpus_policy.tsv`; other one-child
`task.race` and `task.any` wrappers remain retired.

## Migration rules

| Finding | Canonical replacement | Maintained sources | Fixture or expert role |
| --- | --- | --- | --- |
| Fex-data-1 | Remove bare `#Codable` from structurally eligible structs | `examples/features/serde` | Keep explicit derive, rename, published-schema, and non-structural wire-type requirements |
| Fex-data-2 | Replace an exact constant such as `Duration.hours(2) ?? return Err(...)` with `2h` | `examples/features/text`, `examples/features/time`, and other feature examples | Keep runtime values, range-boundary values, and constructor API lessons |
| Fex-sys-1 | Use a bare fallible call in `fn run()` when its error already propagates | `examples/features/io` | Keep parse, decode, conversion, and custom-recovery errors |
| Fex-sys-2 | Pass a read-only value as `fs.read(path)`, without `~` | `examples/features/io` and `examples/features/concurrency/parallel_scan.jet` | Keep ownership-transfer calls and ownership lessons |
| Fex-sys-3 | Use `task.race(...)` or `task.any(...)` directly when no group state is needed | `examples/features/concurrency` | Keep groups with multiple children, limits, cancellation state, or group APIs |
| Fex-sys-4 | Remove positive effect rows already inferred by the body | Routine feature examples | Keep public effect ceilings, authority contracts, and negative effect rows |
| Frealprog-2 | Use `loop (index, item) in xs` for sequence numbering | `tests/agent_workloads/adapters` | Keep deliberate custom numbering or stateful counters |
| Frealprog-6 | Match the filename once and capture the number | `gauntlet/entries/bulkrename/jet/main.jet` | Keep a predicate pipeline only when it is the expert lesson |
| Fex-core-1 | Put the first-contact body at top level | `examples/features/basics/hello.jet` | Keep explicit `fn run` where the function form is the lesson |
| Fex-core-5 | Tell users to run the example with `jet run` | `examples/features/types/typed_literal_forms.jet` | None |
| Ffixtures-1 | Classify fixture roles before applying teaching-surface rules | `tests/conformance/corpus`, `tests/ui`, `tests/fuzz` | Negative and diagnostic fixtures may retain the form under a local exception |

## Regex and capture recipe

For a canonical filename operation, use one anchored pattern and one capture:

```jet
use core.regex as re

pattern :: Regex{"^IMG_(\d+)\.jpeg$"}
photos :: entries
    .filter(entry -> !entry.is_dir)
    .filter_map(entry -> re.match(pattern, entry.name).map(mat -> Photo{
        number: Int.parse(mat.group(1) ?? "0") ?? 0,
        name: entry.name,
        path: entry.path
    }))
    .to_list()
```

This replaces repeated `after("IMG_").before(".jpeg")` extraction. The pattern also rejects names that do not
match the complete filename shape. `#2396` must classify this source and reject a reintroduced repeated-extraction
form in the maintained teaching roots.

## Fixture entry roles

The corpus guard must classify each source before it applies a retirement rule:

- `examples/features/**` is routine teaching material unless the file states an explicit derive, contract, expert,
  ownership, range, runtime, or negative lesson.
- `tests/conformance/corpus/**` is executable canonical behavior. It is not a blanket exception.
- `tests/ui/**` and `tests/fuzz/**` are fixture roots. A retained retired form needs a local exception with the rule,
  stable span, expected occurrence, and reason.
- Generated site, interop, package, and compiler-template sources need producer and artifact classification in the
  #2396 manifest before a new source is accepted.
