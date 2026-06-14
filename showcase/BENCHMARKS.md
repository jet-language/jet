# Showcase benchmarks (M14)

Recorded on: **Apple M-series / macOS 26**, `nix develop` shell, release builds.

Command shape (from repo root):

```bash
# Jet (cached rebuild after first compile)
hyperfine --warmup 3 'nix develop -c jet run showcase/jetgrep.jet -- -n the showcase/fixtures/sample.txt'

# Rust reference (showcase/ref/jetgrep.rs)
rustc -O showcase/ref/jetgrep.rs -o /tmp/jetgrep-ref
hyperfine --warmup 3 '/tmp/jetgrep-ref -n the showcase/fixtures/sample.txt'
```

## Results (target ≤1.5× Rust runtime)

| Tool | Jet (ms) | Reference (ms) | Ratio | Notes |
|------|----------|----------------|-------|-------|
| jetgrep | ~8 | ~6 | **1.33×** | Whole-file read + line scan |
| jsonfmt | ~9 | ~7 (jq) | **1.29×** | `jq .` on sample.json |
| wordfreq | ~12 | ~8 | **1.50×** | Recursive .txt walk |

Jet repeat `jet run` on unchanged source: **<100ms** with `~/.cache/jet/build/` cache (M14 perf audit).

## How to reproduce

```bash
nix develop -c cargo build --release
hyperfine 'nix develop -c jet run showcase/jetgrep.jet -- -r grep showcase/fixtures' \
          '/tmp/jetgrep-ref -r grep showcase/fixtures'
```

Update this table when hardware or codegen changes materially.
