# Library legitimacy survey — Core vs. the lauded standard libraries

Owner-requested 2026-06-22: compare Jet's libraries to popular, highly-regarded
libraries from other languages and propose improvements that make ours
substantially better and reinforce Jet as a serious language. This is the survey;
the concrete decisions it spawns are full ballots in `decision-ballots.md`.

**Method.** Benchmark Core against the libraries practitioners cite as best-in-class
(Rust std + serde/itertools/rayon, Python stdlib + requests/pathlib, Go stdlib,
Swift stdlib/Codable, C# LINQ). Keep only gaps that (a) experts hit daily, (b) fit
Jet's philosophy (safe-by-default, no hidden machinery), and (c) aren't already
ratified or carded. Dedup is aggressive — most of the obvious wins are already in
`idea-cards.md` §4.

## The three top gaps → full ballots queued (this batch)

| Area | Benchmark | Gap | Ballot |
|------|-----------|-----|--------|
| Numeric tower | Rust sized ints + checked/wrapping/saturating + `T::MAX`; Swift traps | sized-int spellings ratified (D-SG9) but unimplemented; no overflow policy, no per-type constants/bit-ops | **D-NUMOPS1** (c103) |
| Serialization | serde (one derive → every format + Deserialize) | `Serialize` exists (S55) but no format-agnostic data model, no `Deserialize`, no field attrs | **D-SERDE1** (c104) |
| Iterators | Rust `Iterator` (~70 lazy adapters), Python `itertools`, LINQ | only `map`/`filter`/`sum`; missing enumerate/zip/chunks/windows/group_by/flat_map/scan/… | **D-ITER1** (c105) |

These three are the highest-leverage legitimacy wins: a real numeric tower, serde-
grade serialization, and a rich lazy-iterator surface are precisely the libraries
practitioners check first when judging whether a language is serious for systems,
data, and services work.

## Already covered — no new ballot (dedup)

Most cross-language staples are ratified or already carded (see `idea-cards.md` §4):
errors-as-values + cause chains (D-ERR-CONV), no-color concurrency
(E2-M1/S53), composable Reader/Writer/streaming I/O (E2-M7), regex (ReDoS-safe,
D-REGEX1), http/tls/csv/toml/log/time/crypto/db ring (E2-M9/M10/M12), property +
doctest (c51), CLI parsing (D-ARGS1), arena/allocators (c05/c26), path objects +
atomic write + dir-walk (E2-M7/D-LSDIR1). Decimal money type and misuse-resistant
crypto and uuid/base64/hex/bigint/grapheme fill-ins are already idea-carded
(`idea-cards.md` forks 2.4 / §3-Stdlib). Don't re-card these.

## Remaining smaller gaps — stubs (expand to a ballot when reached)

- **Collections breadth.** Core has `List` + `Map` (BTreeMap, S38) but no first-class
  **`Set`** and no **`Deque`/ring buffer**. Rust (HashSet/BTreeSet/VecDeque), Python
  (set/deque), Swift (Set) all ship these as table-stakes. *Rec:* add `Set<T>` (Core)
  + `Deque<T>` (ring library); `Set` is the more urgent — its absence is conspicuous.
  Needs a small surface decision (literal? `{1,2,3}` collides with blocks — likely a
  constructor only, mirroring map's no-literal stance). → candidate ballot D-SET1.
- **Iterator terminal richness on collections** rides D-ITER1; no separate card.
- **Datetime ergonomics.** `jet.time` exists (E2-M12); verify it covers the chrono/
  `time`-crate surface experts expect (durations, formatting/parsing, timezone-aware
  instants, monotonic clock). If gaps, a focused enhancement — not a syntax decision.
  *Likely library work, no ballot.*
- **Text/Unicode.** Grapheme-cluster iteration + normalization is already an
  idea-cards §3-Stdlib fill-in; promote there, not here.

## Note on scope

These reinforce legitimacy without bending the simplicity ratchet: every item is
vocabulary or a blessed-protocol method (D-EXT1 Tier 0/1), not new grammar. The
numeric and iterator work needs no new syntax; serde needs the user-derive engine
(S56, Epoch 3) but its *shape* is decided by D-SERDE1 now.
