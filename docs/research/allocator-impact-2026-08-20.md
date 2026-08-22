# Jet AOT global-allocator impact

Date: 2026-08-20. Card: #2061.

## Result

No default allocator change is warranted from this measurement. Across seven
Jet AOT binaries, mimalloc was faster only on the two larger allocation-shaped
workloads, and its largest median win was 9.4% on the synthetic JSON
round-trip. The 30-sample aggregate did not clear the card's robust 10–15%
proposal bar. Mimalloc increased maximum RSS in every workload, from 23.4% to
93.4% in the three-sample medians.

No product code, manifest row, generated-program dependency, or default change
ships. No owner ballot is drafted: the measured result is **no proposal
warranted**. A future result from a longer-running real workload should reopen
the decision lane under the Core dependency rules.

The measurement table is complete below. This shared-tree worker does not
commit or write Tower state, as required by the card brief; the orchestrator
must commit this file and add its summary to card #2061.

## Current allocator policy

The current policy is a closed system/counting choice, not a third-party
allocator selector:

- `AllocatorPolicy` defaults to `HostedDefault`; its shipped hosted variants
  are `HostedDefault` and the built-in `Counting` wrapper
  ([`crates/jet-foundation/src/TargetMachine.rs:584-600`](../../crates/jet-foundation/src/TargetMachine.rs:584)).
- `package.jet` accepts `mem.Heap` or `mem.Counting.over(mem.Heap, ...)`; other
  values fail closed
  ([`crates/jet-pkg-model/src/Package/Blocks.rs:149-186`](../../crates/jet-pkg-model/src/Package/Blocks.rs:149)).
- A missing allocator keeps the hidden system heap, and AOT emission omits the
  `__JET_PROGRAM_ALLOCATOR` static
  ([`tests/allocator_families.rs:694-708`](../../tests/allocator_families.rs:694)).
- The program-allocator kernel delegates its system mode to Rust's
  `std::alloc::System`
  ([`crates/jet-codegen/src/Prelude/ProgramAllocator.rs:423-430`](../../crates/jet-codegen/src/Prelude/ProgramAllocator.rs:423)).
- The same allocator example is checked through JIT, interpreter, and AOT
  lenses
  ([`tests/allocator_families.rs:813-840`](../../tests/allocator_families.rs:813)).

This measurement therefore tests the shipping hosted AOT baseline: the Linux
system allocator, glibc `malloc` on this machine, with zero source-code change
and `LD_PRELOAD` substitution.

## Machine and tools

| Field | Recorded value |
|---|---|
| Date | 2026-08-20 |
| Host | `halcyon-plasma-beta` |
| OS / kernel | NixOS 26.11 / `7.0.11-cachyos` |
| CPU | AMD Ryzen 9 7950X3D, 16 cores / 32 threads |
| CPU governor | `powersave`; no CPU pinning |
| glibc | 2.42 |
| Jet | 1.0.0; HEAD `f8a669e65`; shared checkout dirty |
| Hyperfine | 1.20.0 from `nixpkgs#hyperfine` |
| mimalloc | 3.3.2 from `nixpkgs#mimalloc` |
| jemalloc | 5.3.1 from `nixpkgs#jemalloc` |
| RSS tool | GNU `time` 1.10 from `nixpkgs#time` |

The host has no `/usr/bin/time`; the GNU `time -v` equivalent was used from
`/nix/store/k5369naykh8b03hhp48g65xmlb76w8wm-time-1.10/bin/time`. The host also
lacks native Hyperfine and allocator DSOs. The exact preloads were:

```text
/nix/store/skvb5hzhqvqrizrvw21aiy4prjivraxp-mimalloc-3.3.2/lib/libmimalloc.so
/nix/store/k59krzj3hnkflm623b1mhvgibkmjfnc0-jemalloc-5.3.1/lib/libjemalloc.so
```

Other workers had unrelated edits in the shared checkout. This report does
not use whole-tree cleanliness as evidence; workload sources and commands are
named so the measurement can be repeated after integration.

## Named workloads

| Workload | Source | Allocation shape and input |
|---|---|---|
| `string_churn` | scratch source embedded below | 100 batches × 2,000 formatted strings; 200,000 short-lived strings |
| `collection_churn` | scratch source embedded below | 50 batches × 3,000 map updates over 257 keys, then iteration and removal |
| `json_roundtrip` | scratch source embedded below | 5,000 typed nested `Person` JSON decode/encode cycles |
| `card_2058_sum_all_bench` | [`tests/fixtures/card_2058_sum_all_bench.jet`](../../tests/fixtures/card_2058_sum_all_bench.jet) | 2,000,000 list pushes followed by 16 complete list scans |
| `para_map_crossover_bench` | [`examples/features/tooling/para_map_crossover_bench.jet`](../../examples/features/tooling/para_map_crossover_bench.jet) | AOT `run` sanity path only: one 64-item, cost-32 `para_map`; measured `#Test` claims are not part of normal AOT execution |
| `json_typed` | [`examples/features/serde/json_typed.jet`](../../examples/features/serde/json_typed.jet) | One typed nested decode and encode; startup-dominated control |
| `encoding_json_stream` | [`examples/features/serde/encoding_json_stream.jet`](../../examples/features/serde/encoding_json_stream.jet) | Five-event file-backed JSON write/read; startup and file-I/O control |

The requested `tests/agent_workloads` adapter leg was attempted but not
measurable without changing another worker's file. The current
[`incident_report.jet`](../../tests/agent_workloads/adapters/incident_report.jet)
fails the current compiler at lines 14, 15, 16, and 33 with E0320 because it
still uses retired `[T].{...}` and `Type.{...}` spellings. No adapter source was
modified for this card.

## Method

Each source was built once with the production AOT path:

```sh
JET_NO_SCCACHE=1 scripts/agent/jet-env ./target/debug/jet build --release <source.jet>
```

Each finished `build/<stem>` binary ran serially in three independent batches.
Every batch used three warmups and ten timed samples. Hyperfine ran without a
shell. All legs used `env` so launch handling was equal:

```sh
hyperfine --shell=none --warmup 3 --runs 10 \
  --command-name system   "env -u LD_PRELOAD build/<stem>" \
  --command-name mimalloc "env LD_PRELOAD=<libmimalloc.so> build/<stem>" \
  --command-name jemalloc "env LD_PRELOAD=<libjemalloc.so> build/<stem>"
```

Timing values below are medians over the 30 timed samples (three batches × ten
runs). Delta is `(allocator - system) / system`; negative is faster. RSS is the
median of three `time -v` maximum-resident-set samples, in KiB. Semantic output
was equal across allocator legs; the card #2058 fixture's third output line is
its own elapsed-time diagnostic and was excluded from the equality check.

## Timing

| Workload | System P50 (ms) | mimalloc P50 (ms) | mimalloc Δ | jemalloc P50 (ms) | jemalloc Δ |
|---|---:|---:|---:|---:|---:|
| `string_churn` | 13.779 | 13.334 | **−3.2%** | 15.477 | +12.3% |
| `collection_churn` | 17.237 | 17.480 | +1.4% | 19.602 | +13.7% |
| `json_roundtrip` | 19.177 | 17.376 | **−9.4%** | 20.459 | +6.7% |
| `card_2058_sum_all_bench` | 55.336 | 60.133 | +8.7% | 59.585 | +7.7% |
| `para_map_crossover_bench` | 1.919 | 2.196 | +14.5% | 2.560 | +33.4% |
| `json_typed` | 1.765 | 2.125 | +20.4% | 2.411 | +36.6% |
| `encoding_json_stream` | 1.902 | 2.285 | +20.1% | 2.628 | +38.2% |

`json_roundtrip` is the only workload near the proposal threshold. Its three
batch medians were system/mimalloc `18.952/17.191`, `19.207/17.533`, and
`19.270/17.225` ms. Hyperfine flagged statistical outliers in the aggregate;
the repeated result is useful evidence of a possible JSON-specific effect, not
enough evidence for a default dependency on this synthetic micro-workload.

## Maximum RSS

| Workload | System P50 (KiB) | mimalloc P50 (KiB) | mimalloc RSS Δ | jemalloc P50 (KiB) | jemalloc RSS Δ |
|---|---:|---:|---:|---:|---:|
| `string_churn` | 2,496 | 3,080 | +23.4% | 5,164 | +106.9% |
| `collection_churn` | 2,196 | 3,168 | +44.3% | 5,272 | +140.1% |
| `json_roundtrip` | 2,296 | 3,084 | +34.3% | 5,484 | +138.9% |
| `card_2058_sum_all_bench` | 18,104 | 35,012 | +93.4% | 28,604 | +58.0% |
| `para_map_crossover_bench` | 2,296 | 3,108 | +35.4% | 5,408 | +135.5% |
| `json_typed` | 2,500 | 3,292 | +31.7% | 5,516 | +120.6% |
| `encoding_json_stream` | 2,508 | 3,216 | +28.2% | 5,524 | +120.3% |

RSS moves against a default swap. The largest list workload nearly doubles
maximum RSS under mimalloc, even while its wall time is 8.7% slower. The
jemalloc control is slower than system on six of seven workloads and uses more
RSS on every workload.

## Interpretation and transfer caveat

The video evidence (`UJ_W0O3sFnY`) measured a musl binary and reported a large
musl-malloc → mimalloc improvement. This report measures Jet's Linux shipping
question on glibc 2.42. Musl's baseline allocator and process/runtime details
do not transfer to glibc, so the video's roughly 2 seconds out of 5 seconds is
not a Jet estimate. The Jet result is smaller and mixed: one synthetic JSON
loop is 9.4% faster, while the larger list workload is 8.7% slower and uses
93.4% more RSS.

Preloading also changes allocator initialization and process-wide allocations;
that cost is part of the zero-code-change experiment. The sub-5-ms controls
are startup/I/O dominated and should not drive a product decision. The two
scratch loops and the card #2058 list workload are the relevant allocation
shapes, and none supplies a robust 10–15% mimalloc win with an acceptable RSS
trade.

## Reproducible scratch sources

These are the three temporary sources used for the table. They are included
verbatim so the measurement does not depend on an untracked fixture. The
temporary files were removed after the binaries and measurements were made.

### `string_churn`

```jet
fn churn(seed: Int) :> Int {
    values := [String]{}
    total := 0
    loop index in 0..<2000 {
        value :: "jet-{seed}-{index}-allocator"
        total += value.len()
        values.push(value)
    }
    return total + values.len()
}

fn run() {
    total := 0
    loop batch in 0..<100 :> total += churn(batch)
    print(total)
}
```

### `collection_churn`

```jet
fn churn(seed: Int) :> Int {
    counts := [String:Int]{}
    loop index in 0..<3000 {
        key :: "k-{index % 257}"
        counts[key] = (counts.get(key) ?? 0) + seed + index
    }
    total := 0
    loop (key, value) in counts :> total += value
    loop index in 0..<257 :> total += counts.pop("k-{index}") ?? 0
    return total
}

fn run() {
    total := 0
    loop batch in 0..<50 :> total += churn(batch)
    print(total)
}
```

### `json_roundtrip`

```jet
use core.encoding.json as json

#Codable
struct Address {
    city: String
    zip: String
}

#Codable
struct Person {
    name: String
    address: Address
    tags: [String]
    age: Int?
}

fn run() ? [FieldError] {
    raw :: "{{\"name\":\"Ada\",\"address\":{{\"city\":\"Reno\",\"zip\":\"89501\"}},\"tags\":[\"math\",\"code\",\"poetry\"],\"age\":36}}"
    total := 0
    loop index in 0..<5000 {
        person :: json.decode<Person>(raw)?
        wire :: json.to_string(person)
        total += wire.len() + person.tags.len() + index
    }
    print(total)
}
```

## Decision

**No proposal warranted.** Do not add a default allocator dependency. Do not
add `mem.Mimalloc` or another `AllocatorPolicy` row. If a future representative
workload shows a repeatable win above the threshold, raise an owner ballot with
the already-defined options: no change; opt-in third-party allocator with the
external dependency only when selected; or default swap with a full Core
dependency-rule analysis.
