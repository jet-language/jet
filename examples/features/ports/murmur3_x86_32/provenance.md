# MurmurHash3 x86_32 provenance

## Identity

- Library: MurmurHash3, selected API `MurmurHash3_x86_32`.
- Upstream: [PeterScott/murmur3](https://github.com/PeterScott/murmur3).
- Revision: `dae94be0c0f54a399d23ea6cbe54bca5a4e93ce4`.
- License: public domain, as stated by the upstream project.
- Selected source scope: `murmur3.c`, `rotl32`, `fmix32`, and
  `MurmurHash3_x86_32` only.
- Local C corpus: `corpus/murmur3_x86_32.c`, SHA-256
  `9f7df17053f573ddb891d2991edf3104d28ba5322d041c3c94b3add695667d92`.
- Local C oracle: `corpus/oracle.c`, SHA-256
  `07bac2a1e5bef83b2a6718f8b0f8f1e6691d9e8df12ba6222533e0561405f891`.
- Jet port and executable entry: `run.jet`, SHA-256
  `4510be1687b99a67d842aa0a20ddf6840787238aa00ca1b31b7089ddd96e3e71`.
- Golden output: `../../expected/ports/murmur3_x86_32.out`, SHA-256
  `521dca7f6ed0f4380b682fa6b6a5fca87afb575f1e3bfe2aa069435fd2ea5f52`.

The local C file is a readable snapshot of the selected algorithm. It keeps
the upstream arithmetic and tail cases, and spells the little-endian block
load explicitly. The Jet source is one self-contained source module so the
pilot exercises the AOT, default run, test, and proof paths without adding a
cross-file compiler seam.

## Scope and assumptions

- Input is an owned `[U8]` byte list.
- Seed and result are `U32`.
- Block loads are little-endian.
- `wrapping(...)` marks every fixed-width unsigned multiplication and addition
  that can overflow.
- The pilot excludes the upstream 128-bit APIs, platform-specific unaligned
  reads, allocator/runtime integration, and unrelated macros or tests.

## Oracle vectors

The vector input is UTF-8 bytes except for the final binary row.

| Input | Seed | Expected `U32` |
| --- | ---: | ---: |
| `""` | 0 | 0 (`0x00000000`) |
| `"a"` | 0 | 1009084850 (`0x3c2569b2`) |
| `"ab"` | 0 | 2613040991 (`0x9bbfd75f`) |
| `"abc"` | 0 | 3017643002 (`0xb3dd93fa`) |
| `"abcd"` | 0 | 1139631978 (`0x43ed676a`) |
| `"hello"` | 0 | 613153351 (`0x248bfa47`) |
| `"hello world"` | 0 | 1586663183 (`0x5e928f0f`) |
| `"hello"` | 42 | 3806057185 (`0xe2dbd2e1`) |
| `[00 01 02 03 ff]` | 7 | 3881383995 (`0xe759383b`) |

## Validation record

Commands were run from the repository root with
`TMPDIR="$HOME/.cache/jet-test-scratch"`.

### C oracle

Command:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env cc -std=c11 -Wall -Wextra -Werror examples/features/ports/murmur3_x86_32/corpus/murmur3_x86_32.c examples/features/ports/murmur3_x86_32/corpus/oracle.c -o "$HOME/.cache/jet-test-scratch/murmur3-oracle" && "$HOME/.cache/jet-test-scratch/murmur3-oracle"
```

Exit status: `0`.

Exact stdout:

```text
0
1009084850
2613040991
3017643002
1139631978
613153351
1586663183
3806057185
3881383995
```

### Jet named test

Command:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet test --serial --show-default examples/features/ports/murmur3_x86_32/run.jet
```

Result:

```text
MurmurHash3 x86 32 vectors: pass
1 passed, 0 failed, 0 skipped
```

### Default run and golden bytes

Command:

```sh
golden_actual="$HOME/.cache/jet-test-scratch/murmur3-golden.out"; TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet run examples/features/ports/murmur3_x86_32/run.jet > "$golden_actual" && cmp "$golden_actual" examples/features/expected/ports/murmur3_x86_32.out
```

Result: exit status `0`; `cmp` found the run output byte-for-byte equal to the
golden file. The nine output lines are the oracle stdout above.

### Jet proof

Command:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" scripts/agent/jet-env jet prove examples/features/ports/murmur3_x86_32/run.jet --json
```

The exact machine result was `"result":"pass"` with `"exitCode":0`.
The report recorded four proved front-end effect facts and one passed unit
test: `"frontEnd":{"failed":0,"proved":4,"selected":4,"skipped":0}` and
`"unit":{"failed":0,"passed":1,"selected":1,"skipped":0,"expectedFailures":0,"unexpectedPasses":0}`.

The repository-wide filtered Cargo golden harness command was also attempted:

```sh
TMPDIR="$HOME/.cache/jet-test-scratch" JET_GOLDEN_FILTER=ports/murmur3_x86_32 scripts/agent/jet-env cargo test --test golden examples_compile_and_run -- --nocapture
```

It did not reach the test because unrelated concurrent Cargo processes held
the shared build lock. The direct `jet run` plus `cmp` check above completed
the same target's published-output comparison.
