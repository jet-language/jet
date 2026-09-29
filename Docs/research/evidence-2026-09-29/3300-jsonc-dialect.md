# #3300 — Explicit JSON-with-comments support (CORE-F080, D-CORE-JSONC1=A)

Date: 2026-09-29. Binary: `jet-debug-snapshot14` via `~/.cache/jet-luna/safe-jet.sh`,
source rev `e9c708fa7`. Author: Closer03 (evidence closer). No compiler, runtime or
Core change was made.

## Question

Is the ratified `core.encoding.jsonc` available? Does strict `core.encoding.json` keep
rejecting comments and trailing commas, and how do the existing TOML/YAML configuration
paths handle the same commented-config job?

## Method

- Probe `jsonc_absent.jet` (the decision's own example, `jsonc.decode<CountRecord>`), `run --interpret`.
- Probe `jsonc_baseline.jet`: strict `json.parse` of plain, `//` comment, `/* */` comment,
  trailing comma and `"// kept"` string inputs, typed `json.decode<CountRecord>` of a
  commented input, and `toml.parse` / `yaml.parse` of commented equivalents. Default run
  and `--interpret`.
- Source read: `Core/encoding/json.jet` (`skip_ws`, trailing-comma rejection),
  `Docs/spec/reference/core-library.md:2128-2212` (json/toml/yaml sections).
- Probes live in `~/.cache/jet-test-scratch/Closer03/`.

## Evidence

`jsonc_absent.jet` →

```
Error [E1001]: There is no core module `core.encoding.jsonc`
  --> jsonc_absent.jet:1:5
```

The runtime baseline probe `jsonc_baseline.jet` did not produce output: under `--interpret` the TOML part crashes the compiler (`TomlParser::fail` missing checked target); a default-run pass was not reached before the budget stop. Strict rejection is therefore evidenced from source: `Core/encoding/json.jet` `skip_ws` (:280) consumes only JSON whitespace, so `/` is a syntax error, and trailing commas are rejected with "trailing commas are not allowed in JSON" at :531 (arrays) and :580 (objects).

## Criteria

1. *Pin accepted comment/trailing syntax and round-trip needs.* Met by D-CORE-JSONC1=A:
   `//` line and `/* */` block comments accepted; trailing commas rejected; exact numbers,
   UTF-8, duplicate/field policy, limits and original source offsets kept; `to_string`
   emits ordinary JSON; no comment preservation, CST, round-trip editing, include or eval.
2. *Keep standards JSON rejection unchanged.* Met by source (json.jet:280, :531, :580); JSONC is a separate module under the ratified decision, so strict JSON stays unchanged. A runtime receipt is still owed with the `serde/jsonc` golden.
3. *Compare TOML/YAML configuration jobs without calling them wire-equivalent.* Met: `core-library.md:2191-2212` — TOML 1.0 subset accepts `#` comments but emits minimal output without comments and keeps datetimes as text; YAML is an input-only bounded subset with `#` comments. Both serve the hand-edited-config job, but neither is wire-equivalent to JSONC (different grammars, value models and output formats).
4. *Gate any new dialect API/library/distribution.* Met: D-CORE-JSONC1 ratified A
   (2026-09-12); no library or distribution dependency.
5. *`serde/jsonc` golden on every tier.* Not met: `core.encoding.jsonc` does not exist (E1001).

## Verdict

PARTIAL: investigation criteria 1-4 met (decision + source evidence). Criterion 5 (the `serde/jsonc` golden on every tier) is not met: `core.encoding.jsonc` does not exist (E1001). Defect: interpreter crash in `toml.parse`.
