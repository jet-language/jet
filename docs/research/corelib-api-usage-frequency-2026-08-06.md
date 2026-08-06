# Core-library API usage frequency across languages and segments

Date: 2026-08-06. Purpose: empirical evidence for which standard-library operations dominate
real-world code, so Jet's prelude makes the most common operations the most frictionless.
Languages covered: Python, JavaScript/TypeScript, Rust, Go, Java, Kotlin, Swift, C#, Ruby.
Segments covered: beginners/education, learners, hobby scripts, data science/notebooks,
small-business apps, enterprise, open-source libraries, ops/scripting.

Evidence types used: corpus studies of API usage, package-registry download and dependency
stats (a hugely-downloaded gap-filler library marks a stdlib gap), intro-course curricula,
notebook corpus studies, and Stack Overflow question popularity as a friction proxy.

---

## Summary: the ~30 operations that dominate everyday programming

These operations recur at the top of every evidence source — corpus studies, registry stats,
curricula, and SO traffic — across all nine languages and all segments. This list should
drive Jet's prelude. Ordering within groups is approximate; the grouping is the signal.

**Tier 1 — universal, every segment, every language (must be zero-ceremony):**

1. Print a value to the console (`print`/`println!`/`fmt.Println`/`console.log`).
2. Format a string by interpolation (f-strings, template literals, `${}`).
3. String concatenation and length.
4. String split / join.
5. String trim (strip whitespace).
6. String contains / starts-with / ends-with.
7. String case change (upper/lower) and replace.
8. Create a list/array literal; index it; get its length.
9. Append/push to a list.
10. Iterate a list (for-each).
11. Map a function over a collection.
12. Filter a collection by predicate.
13. Sort a collection (with a key/comparator).
14. Create a map/dict literal; get/set by key; check key presence; iterate entries.
15. Sum / min / max over a collection.
16. Equality comparison that works on values, including nested structures.

**Tier 2 — near-universal (present in most programs beyond hello-world):**

17. Read a whole text file; write a whole text file.
18. Iterate lines of a file.
19. Join and manipulate filesystem paths; check a file exists.
20. Parse string→number and number→string.
21. Basic arithmetic incl. integer division, modulo, abs, rounding, power.
22. Get current time; format a date/time; parse a date string.
23. JSON encode/decode to native data structures.
24. Range/sequence generation (`0..n`, `range(n)`).
25. Enumerate with index; zip two sequences.
26. Random integer in range; random choice; shuffle.
27. Read program arguments and environment variables.
28. Assert/expect in tests (`assert_eq`, `expect(x).toBe(y)`).
29. Structured logging with levels (enterprise skews hard toward this).
30. Make an HTTP GET/POST and read the body (usually JSON).

Evidence for the tiering: Java corpus studies find `java.lang.String`, `List`, and `Map`
methods are the most-used APIs in 1.5M+ ASTs, with usage following a Zipf distribution
where 1% of packages account for 80% of all API usage ([Qiu et al., IST 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf));
the top npm gap-fillers are string/collection utilities, HTTP, and terminal printing
([npm rank](https://gist.github.com/anvaka/8e8fa57c7ee1350e3491)); the top PyPI packages are
HTTP and datetime parsing ([PyPI leaderboard](https://pypilb.vercel.app/)); the top crates
are iteration, serialization, and random ([crates.io](https://crates.io/crates?sort=downloads));
intro curricula start with print/input/len/string ops ([U. Toronto CSC110](https://www.cs.toronto.edu/~david/course-notes/csc110-111/02-functions/01-builtin-functions.html)).
The skew is the headline: a tiny prelude covering the list above covers the bulk of real
API calls in every measured corpus.

---

## Segment breakdown

Usage differs by experience level and sector. Where segments agree, the signal is strongest.

### Absolute beginners, K-12, bootcamps, CS1

- Python is the dominant intro language: 8 of the top 10 U.S. CS departments teach intro
  courses in Python ([Guo, CACM 2014](https://cacm.acm.org/blogs/blog-cacm/176450-python-is-now-the-most-popular-introductory-teaching-language-at-top-us-universities/fulltext)),
  and it remains the language developers most want to learn ([SO Survey 2025](https://survey.stackoverflow.co/2025/technology)).
- First-ten-lessons surface: `print`, `input`, `len`, `range`, `int`/`float`/`str`
  conversion, `sum`, `min`/`max`, list append/index, string methods, `random`. University
  builtin references for CS1 courses enumerate exactly this set
  ([Toronto CSC110 builtins](https://www.cs.toronto.edu/~david/course-notes/csc110-111/02-functions/01-builtin-functions.html),
  [Wellesley CS111 builtins](https://cs111.wellesley.edu/archive/cs111_fall21/public_html/reference/builtins)).
- Beginner-exercise corpora (typical "50 programs for beginners" sets) are dominated by
  console I/O, string manipulation, arithmetic, lists, and `random`
  ([example set](https://github.com/fasilofficial/50-python-programs)).
- Signal for Jet: print, read-a-line, string interpolation, numeric conversion, list/dict
  basics, and random must work with zero imports and zero ceremony.

### Learners switching languages / hobbyists / one-off scripts

- Friction proxy: the all-time most-viewed SO questions are dominated by "how do I do a
  tiny common thing" — the #1 question ever is undoing a git commit at 7M+ views
  ([Hoffa](https://hoffa.medium.com/finding-the-real-top-stack-overflow-questions-aebf35b095f1));
  "does this string contain a substring" in Java alone has 4M+ views
  ([analysis](https://medium.com/@i.walsh98/the-eternal-struggles-unpacking-the-most-popular-stack-overflow-questions-02c2dc5cf5d9));
  "how do I check whether a file exists in Python" (2008) is among the most-viewed Python
  questions ever (same source). Learners hit friction on strings, files, and dicts first.
- The most-copied SO snippet of all time is a Java function to format a byte count
  human-readably — copied into thousands of projects, and buggy
  ([programming.guide](https://programming.guide/worlds-most-copied-so-snippet.html),
  [Baltes & Diehl 2019](https://arxiv.org/abs/1802.02938)). Humane formatting is a real,
  measured stdlib gap.
- Scripts lean on: args/env, path join, glob, read/write file, subprocess, string ops.
  Python's most-referenced stdlib modules in a GitHub corpus: `sys`, `os`, `logging`,
  `collections`, `re`, `datetime`, `json`
  ([Schanely, BigQuery GitHub corpus](https://medium.com/@pschanely/the-python-standard-library-modules-by-popularity-eb1c07afc397)).

### Education-to-data-science / notebooks

- A corpus study of 1.4M Jupyter notebooks found the most-imported modules are `numpy`,
  `matplotlib`, `pandas`, then `sklearn`, `os`, `scipy`
  ([Pimentel et al. 2019](https://leomurta.github.io/papers/pimentel2019a.pdf)).
- Within pandas, usage concentrates on a small head: `read_csv`, `head`, `describe`,
  `groupby`, filtering, `merge`, plotting — analyses of top Kaggle notebooks find ~20
  functions cover the bulk of work
  ([Kaggle: 20 pandas functions for 80% of tasks](https://www.kaggle.com/code/youssef19/20-pandas-functions-for-80-data-science-tasks),
  [record_api call analysis of Kaggle notebooks](https://github.com/data-apis/dataframe-api/issues/22)).
- Signal for Jet: table/CSV ingestion, columnar map/filter/group/aggregate, and quick
  plotting are the data segment's Tier 1; `os`/paths appear even here.

### Small/medium business apps and web work

- JavaScript is the most-used language (66% of developers), with HTML/CSS and SQL next
  ([SO Survey 2025](https://survey.stackoverflow.co/2025/technology)).
- The most-depended npm packages are gap-fillers for collections/strings (lodash, #1 by
  dependents), terminal color (chalk), HTTP (request, axios), CLI args (commander)
  ([npm rank](https://gist.github.com/anvaka/8e8fa57c7ee1350e3491),
  [PkgPulse 2026](https://www.pkgpulse.com/guides/most-depended-on-npm-packages-2026)).
  lodash still sees ~164M weekly downloads and axios ~120M
  ([npm trends](https://npmtrends.com/axios-vs-lodash)).

### Enterprise

- Java corpus evidence: the most-used APIs are String/collections; fields most used are
  constants like `Integer.MAX_VALUE` and `System.out`
  ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf)).
- The perennial top Java dependencies are JUnit, SLF4J/Log4j (logging), Jackson/Gson
  (JSON), Apache Commons Lang (strings), Mockito
  ([JarCasting top 100](https://jarcasting.com/top-100-java-libraries/),
  [survey of essential Java libraries](https://finitestate.io/blog/top-10-java-libraries)).
  Enterprise adds logging, serialization, mocking, and concurrency on top of the universal set.
- .NET mirrors this: Newtonsoft.Json is the most-downloaded NuGet package ever at ~8.9B
  downloads ([NuGet](https://www.nuget.org/packages/newtonsoft.json/)); xunit and Moq join
  it at the top ([popular NuGet packages](https://dev.to/polymorphicguy/the-11-most-popular-nuget-packages-to-know-in-2026-updated-20f5)).
- Ruby: the most-downloaded gems are `concurrent-ruby`, `tzinfo`, `json`, `minitest`, and
  the rspec family, all above 1B downloads ([RubyGems stats](https://rubygems.org/stats)) —
  concurrency, timezones, JSON, testing.

### Open-source libraries (Rust/Go ecosystems as proxies)

- Top crates by all-time downloads include `itertools` (~939M), `serde` (~891M), plus
  `rand`, `regex`, `chrono`, `anyhow`, `tokio`, `clap`, `reqwest` in every popularity list
  ([crates.io](https://crates.io/crates?sort=downloads), [lib.rs](https://lib.rs/std),
  [blessed.rs](https://blessed.rs/crates)).
- Go libraries lean on the stdlib (`fmt`, `strings`, `net/http` are the canonical core
  ([overview](https://www.codingexplorations.com/blog/interview-series-most-frequently-used-standard-library-packages)))
  plus `testify` for assertions ([pkg.go.dev](https://pkg.go.dev/github.com/stretchr/testify)).

### The agreement set (strongest prelude signal)

Every segment, from CS1 to enterprise, independently tops out on the same core:
**print/format, string split-join-trim-contains-case-replace, list append/iterate/
map/filter/sort, dict get/set/contains/iterate, sum/min/max, read/write file + path +
exists, parse numbers, now + format/parse dates, JSON encode/decode, random.** Segments
diverge only in what they add: beginners add `input`; data adds tables and plots;
enterprise adds logging, mocking, and concurrency; ops adds subprocess and glob. Nothing
in the agreement set is segment-specific — it is the prelude.

---

## Domain findings

### 1. Strings and text

**Dominant operations:** interpolation/format, concat, length, split, join, trim,
contains/startsWith/endsWith, replace, case change, substring/slice, pad, parse to number.

**Evidence:**
- `java.lang.String` is the single most-used class in Java corpora; the most popular
  methods concentrate on string and collection manipulation
  ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf);
  same picture in [Lämmel et al., SAC 2011](https://dl.acm.org/doi/10.1145/1982185.1982471)).
- "How to check if a string contains a substring" is one of the most-viewed SO questions
  in multiple languages (4M+ views for Java alone)
  ([analysis](https://medium.com/@i.walsh98/the-eternal-struggles-unpacking-the-most-popular-stack-overflow-questions-02c2dc5cf5d9)).
- Go's `strings` and `fmt` are cited as its most-used stdlib packages
  ([overview](https://www.codingexplorations.com/blog/interview-series-most-frequently-used-standard-library-packages)).

**Friction signals:**
- The npm left-pad incident: an 11-line string-padding package was a transitive dependency
  of Babel, React, and webpack; its removal broke the ecosystem, and the fix was adding
  `String.prototype.padStart` to the language
  ([Wikipedia](https://en.wikipedia.org/wiki/Npm_left-pad_incident),
  [MDN padStart](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/String/padStart)).
  Missing trivial string ops get reinvented at ecosystem scale.
- Apache Commons Lang (`StringUtils`) stayed a top-10 Java dependency for two decades
  because `java.lang.String` lacked isBlank/strip/join for most of its life
  ([library surveys](https://finitestate.io/blog/top-10-java-libraries)); Java later
  absorbed these. Rails' ActiveSupport adds `blank?`/`present?` to Ruby for the same reason
  ([Rails guides](https://guides.rubyonrails.org/active_support_core_extensions.html)).
- Humane formatting (byte counts, plurals) is the most-copied SO snippet ever — a stdlib
  gap in every language ([programming.guide](https://programming.guide/worlds-most-copied-so-snippet.html)).
- Regex is heavily used but always a step behind plain string methods: Rust's `regex`
  crate has tens of millions of downloads despite not being in std
  ([Serokell](https://serokell.io/blog/most-popular-rust-libraries)).

### 2. Collections and iteration

**Dominant operations:** literal construction, index/get, append, length, for-each, map,
filter, sort(with key), contains, sum/min/max/count, enumerate, zip, dedupe, group-by,
flatten, slice/first/last.

**Evidence:**
- `List` and `Map` methods are, with String, the top-used APIs in Java corpora
  ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf)).
- `itertools` is the #1 most-downloaded crate of all time (~939M), ahead of even serde —
  iteration adapters beyond the std Iterator trait are the single biggest Rust gap-filler
  ([crates.io](https://crates.io/crates?sort=downloads)).
- lodash was the #1 most-depended npm package for a decade
  ([npm rank](https://gist.github.com/anvaka/8e8fa57c7ee1350e3491)); its most-used functions
  are `get` (safe nested access), `isEqual` (deep equality), `cloneDeep`, `debounce`
  ([Mastering JS](https://masteringjs.io/lodash)). Its decline as ES5/ES6 added
  `map`/`filter`/`find` natively shows absorption works
  ([PkgPulse 2026](https://www.pkgpulse.com/guides/most-depended-on-npm-packages-2026)).
- "How do I iterate over a dictionary" is among the most-asked Python questions
  ([TDS analysis](https://towardsdatascience.com/10-most-frequently-asked-python-dictionary-questions-on-stack-overflow-2cb345f07496/)).

**Friction signals:**
- Deep equality and deep copy: lodash `isEqual`/`cloneDeep` popularity shows reference
  equality as the default is a persistent pain ([Mastering JS](https://masteringjs.io/lodash)).
- Safe nested access (lodash `get`) → absorbed as optional chaining `?.` in JS.
- Guava and `indexmap` (~936M downloads, insertion-ordered map for Rust
  [crates.io](https://crates.io/crates?sort=downloads)) show demand for richer collection
  types: ordered maps, multimaps, immutable collections.
- Group-by, chunk, windows, dedupe: present in itertools/lodash/Kotlin stdlib but missing
  from many stdlibs; their gap-filler popularity argues for prelude inclusion.

### 3. Math and numerics

**Dominant operations:** + - * /, integer vs float division, modulo, abs, min/max, round/
floor/ceil, power, sqrt, parse/format numbers, sum/average, MAX/MIN constants.

**Evidence:**
- The most-used Java *fields* are numeric threshold constants like `Integer.MAX_VALUE`
  ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf)).
- CS1 curricula introduce `abs`, `round`, `sum`, `min`, `max`, `pow` in the first lessons
  ([Toronto CSC110](https://www.cs.toronto.edu/~david/course-notes/csc110-111/02-functions/01-builtin-functions.html)).
- numpy is the #1 import across 1.4M notebooks ([Pimentel et al. 2019](https://leomurta.github.io/papers/pimentel2019a.pdf))
  — array math is the data segment's arithmetic.

**Friction signals:**
- Floating-point surprise (`0.1 + 0.2`) is one of the most-viewed question families on SO
  ([analysis](https://medium.com/@i.walsh98/the-eternal-struggles-unpacking-the-most-popular-stack-overflow-questions-02c2dc5cf5d9)).
- Integer overflow and the float-division-by-default choice: Python 3 moved `/` to true
  division and made int arbitrary-precision; JS having only doubles forced BigInt in later.
  Jet's ratified slate (bigint Int, `/` → Float, `/%` floordiv) matches this evidence.
- Decimal/money math is a recurring gap: `decimal` in Python stdlib, `rust_decimal`,
  BigDecimal — needed but rarely the default anywhere.

### 4. I/O, filesystem, paths

**Dominant operations:** read whole file, write whole file, iterate lines, open/close,
path join, exists, list directory, glob, create/delete/copy/move, temp files.

**Evidence:**
- `os` and `sys` are the two most-referenced Python stdlib modules in a GitHub corpus
  ([Schanely](https://medium.com/@pschanely/the-python-standard-library-modules-by-popularity-eb1c07afc397));
  `os` is a top import even in data-science notebooks
  ([Pimentel et al. 2019](https://leomurta.github.io/papers/pimentel2019a.pdf)).
- "How do I check whether a file exists" (Python, 2008) is one of the most-viewed SO
  questions of all time
  ([analysis](https://medium.com/@i.walsh98/the-eternal-struggles-unpacking-the-most-popular-stack-overflow-questions-02c2dc5cf5d9)).

**Friction signals:**
- Java needed Commons IO/NIO.2 rewrites for one-line file reads; Python needed `pathlib`
  as a second path API atop `os.path` — both show that string-based path APIs plus verbose
  stream ceremony are the friction, and one-call read/write-whole-file is the demand.
- Rust gap-fillers: `tempfile` and `walkdir` are perennial top-100 crates
  ([blessed.rs](https://blessed.rs/crates)) — temp files and recursive traversal are
  stdlib misses.
- One-line whole-file read/write, `exists`, and glob should be prelude-level; streaming
  APIs are the expert layer.

### 5. Time and dates

**Dominant operations:** now(), format a date, parse a date string, add/subtract
durations, compare, timestamps, timezones (wanted rarely but painfully).

**Evidence and friction (this domain is the clearest stdlib-failure story in every language):**
- Python: `python-dateutil` is a top-15 PyPI package (~873M downloads/period, #12)
  because stdlib `datetime` can't parse arbitrary date strings
  ([PyPI leaderboard](https://pypilb.vercel.app/)); `pytz`/`tzdata` rank similarly.
- JavaScript: `Date` was so broken that moment (13M weekly downloads even in maintenance
  mode), date-fns (20M), and dayjs (40M) together exceed 70M weekly downloads, and TC39
  shipped a whole replacement API (Temporal, Stage 4, ES2026)
  ([LogRocket](https://blog.logrocket.com/master-javascript-date-time-moment-js-temporal/),
  [PkgPulse](https://www.pkgpulse.com/guides/date-fns-v4-vs-temporal-api-vs-dayjs-date-handling-2026)).
- Java: `java.util.Date`/`Calendar` (mutable, 0-based months, year-1900) were replaced by
  the third-party Joda-Time, which was then absorbed as `java.time` (JSR-310) in Java 8
  ([Joda-Time](https://www.joda.org/joda-time/),
  [Colebourne](https://blog.joda.org/2009/11/why-jsr-310-isn-joda-time_4941.html)).
- Rust: `chrono`/`time` are top-100 crates because std has no calendar time beyond
  `SystemTime` ([lib.rs](https://lib.rs/std)).
- Ruby: `tzinfo` has 1.1B+ downloads ([RubyGems stats](https://rubygems.org/stats));
  Rails adds `3.days.ago` sugar
  ([Rails guides](https://guides.rubyonrails.org/active_support_core_extensions.html)).
- Kotlin built `kotlinx-datetime` deliberately minimal, "focused on the most common
  problems", separating physical instant from civil time
  ([kotlinx-datetime](https://github.com/Kotlin/kotlinx-datetime)).
- Design consensus of the absorbed winners (java.time, Temporal, kotlinx-datetime):
  immutable values, Instant vs local-date split, explicit timezones, easy format/parse.

### 6. Serialization (JSON etc.)

**Dominant operations:** encode native data → JSON string, decode JSON → native data,
derive/auto-map to user types, pretty-print. CSV close behind in data/business segments.

**Evidence:**
- serde is the #2 crate of all time (~891M downloads) ([crates.io](https://crates.io/crates?sort=downloads)).
- Newtonsoft.Json is the most-downloaded NuGet package ever (~8.9B)
  ([NuGet](https://www.nuget.org/packages/newtonsoft.json/)); Microsoft built
  System.Text.Json into the platform to absorb it
  ([.NET blog](https://devblogs.microsoft.com/dotnet/try-the-new-system-text-json-apis/)).
- Jackson/Gson are perennial top Java dependencies ([JarCasting](https://jarcasting.com/top-100-java-libraries/)).
- `json` gem: 1.1B+ downloads ([RubyGems stats](https://rubygems.org/stats)); `json` is a
  top-7 Python stdlib module ([Schanely](https://medium.com/@pschanely/the-python-standard-library-modules-by-popularity-eb1c07afc397)).
- Swift shipped SwiftyJSON as the dominant JSON gap-filler until Codable absorbed it
  ([SwiftyJSON](https://github.com/SwiftyJSON/SwiftyJSON)).

**Friction signals:** the winning pattern everywhere is derive-based typed codecs
(serde derive, Codable, kotlinx.serialization) plus an untyped dynamic-value escape hatch.
Languages without derive (Go's reflection tags, pre-Codable Swift) generate the most
boilerplate complaints. JSON belongs in the stdlib; every ecosystem that omitted it grew
a billion-download replacement.

### 7. Networking / HTTP

**Dominant operations:** GET a URL and read body, POST JSON, set headers/auth, handle
status codes, timeouts. Serving: route + handler + JSON response.

**Evidence:**
- `urllib3` (#3) and `requests` (#8) are top-10 PyPI packages with over a billion
  downloads each ([PyPI leaderboard](https://pypilb.vercel.app/)) — despite Python having
  `urllib` in the stdlib. "HTTP for humans" beat the stdlib inside its own ecosystem.
- axios: ~120M weekly npm downloads even after `fetch` went native
  ([npm trends](https://npmtrends.com/axios-vs-lodash)).
- reqwest is a top Rust crate ([Serokell](https://serokell.io/blog/most-popular-rust-libraries));
  Alamofire dominates Swift networking ([Swift Package Registry](https://swiftpackageregistry.com/Alamofire/Alamofire)).
- Counterexample: Go's `net/http` is good enough that no third-party client dominates
  ([overview](https://www.codingexplorations.com/blog/interview-series-most-frequently-used-standard-library-packages)) —
  proof a stdlib HTTP client can win if the ergonomics are right.

**Friction signals:** the requests/axios pattern that wins: one call does URL + method +
JSON body + query params + timeout, returns status + parsed-JSON body. Low-level socket
APIs are expert-tier.

### 8. Concurrency / async

**Dominant operations:** run a task in the background, await it, run N things
concurrently and collect, sleep, timeouts, channels/queues, locks (less common).

**Evidence:**
- `concurrent-ruby` is the #1 most-downloaded gem of all time (1.12B)
  ([RubyGems stats](https://rubygems.org/stats)).
- tokio is the de-facto Rust async runtime, top of every crate ranking
  ([blessed.rs](https://blessed.rs/crates)) — Rust shipping async syntax without a runtime
  created its largest ecosystem dependency.
- Kotlin shipped coroutines as `kotlinx.coroutines`, effectively mandatory for Android.
- Enterprise segment leans hardest here; beginner code touches concurrency only via
  `sleep` and "do these N downloads at once".

**Friction signals:** function-color friction (sync/async split APIs) is the dominant
complaint in Python/JS/Rust ecosystems; Go's goroutines avoid it and are consistently
praised. Structured concurrency (task groups) is where Swift/Kotlin/Python converged.
Jet already tracks this (yielding loops/taskgroups research).

### 9. Random

**Dominant operations:** random int in range, random float 0-1, choice from list,
shuffle, sample, seeding; secure token generation as a separate, clearly-marked need.

**Evidence:**
- `rand` is a top-10 all-time crate ([crates.io](https://crates.io/crates?sort=downloads),
  [O'Reilly crate list](https://www.oreilly.com/library/view/learn-rust-in/9781633438231/OEBPS/Text/17.htm)) —
  Rust's decision to exclude random from std made a third-party crate near-universal.
- `random` is a first-lessons module in every Python intro course
  ([beginner program sets](https://github.com/fasilofficial/50-python-programs)).
- JS `Math.random` gives only float-0-1; "random int in range" is a perennially top-viewed
  SO question ([analysis](https://medium.com/@i.walsh98/the-eternal-struggles-unpacking-the-most-popular-stack-overflow-questions-02c2dc5cf5d9)).

**Friction signals:** the demanded prelude surface is exactly four calls: int-in-range,
choice, shuffle, float. Crypto-secure randomness should be separate and named so.
uuid generation is its own high-frequency need (top-25 npm package,
[PkgPulse](https://www.pkgpulse.com/guides/most-depended-on-npm-packages-2026)).

### 10. Process / OS / env

**Dominant operations:** read argv, read/set env vars, exit with code, run a subprocess
and capture output, current directory, sleep, signal basics.

**Evidence:**
- `sys` and `os` top the Python stdlib usage ranking
  ([Schanely](https://medium.com/@pschanely/the-python-standard-library-modules-by-popularity-eb1c07afc397)).
- CLI-argument parsing is a top gap-filler in every ecosystem: commander is a top-5
  most-depended npm package ([npm rank](https://gist.github.com/anvaka/8e8fa57c7ee1350e3491));
  clap is a top Rust crate ([Serokell](https://serokell.io/blog/most-popular-rust-libraries));
  click is a top PyPI package ([top-pypi-packages](https://hugovk.dev/top-pypi-packages/)).

**Friction signals:** raw argv is universally bypassed for declarative flag parsers —
a declarative args story belongs near the prelude for the scripting segment. Subprocess
APIs are notoriously fiddly (Python's `subprocess.run(..., capture_output=True, text=True)`
incantation); the demand is run-command-get-stdout in one call.

### 11. Testing / assertions

**Dominant operations:** assert equal, assert true, assert-throws, setup/teardown,
parameterized cases, mocking (enterprise), approximate float compare.

**Evidence:**
- JUnit is historically the #1 Maven dependency ([JarCasting](https://jarcasting.com/top-100-java-libraries/)).
- minitest and the rspec family each have 1B+ downloads ([RubyGems stats](https://rubygems.org/stats)).
- testify is Go's dominant third-party package because stdlib `testing` has no assertions
  ([pkg.go.dev](https://pkg.go.dev/github.com/stretchr/testify)).
- pytest is a top PyPI package; its whole pitch is plain `assert` with rich diffs
  ([top-pypi-packages](https://hugovk.dev/top-pypi-packages/)).
- xunit/Moq are top NuGet packages ([popular NuGet packages](https://dev.to/polymorphicguy/the-11-most-popular-nuget-packages-to-know-in-2026-updated-20f5)).

**Friction signals:** ecosystems where the stdlib test story is weak (Go assertions, JS
before node:test) grow dominant third-party layers; ecosystems with rich built-ins (Rust
`#[test]` + `assert_eq!`) don't. The winning shape is: built-in runner, plain
`assert x == y` with a rich structural diff on failure.

### 12. Printing / formatting / logging

**Dominant operations:** print value(s), interpolate, debug-print a structure,
formatted numbers (precision, padding, thousands), colored terminal output, leveled
logging with structured fields.

**Evidence:**
- `print` is the first function every programmer learns
  ([Toronto CSC110](https://www.cs.toronto.edu/~david/course-notes/csc110-111/02-functions/01-builtin-functions.html));
  `System.out` is among the most-used Java fields
  ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf));
  `fmt` is Go's most-used package
  ([overview](https://www.codingexplorations.com/blog/interview-series-most-frequently-used-standard-library-packages)).
- `logging` is the #3 most-referenced Python stdlib module — above collections and re
  ([Schanely](https://medium.com/@pschanely/the-python-standard-library-modules-by-popularity-eb1c07afc397)).
- SLF4J/Log4j are perennial top-5 Maven dependencies ([JarCasting](https://jarcasting.com/top-100-java-libraries/));
  Serilog/NLog top NuGet; winston/pino top npm; `log`/`tracing` top crates; Swift ships
  first-party `swift-log` ([Swift Package Registry](https://swiftpackageregistry.com/apple/swift-log)).
- chalk (terminal color) is a top-2 most-depended npm package
  ([npm rank](https://gist.github.com/anvaka/8e8fa57c7ee1350e3491)) — colored terminal
  output is a measured mass need, not a nicety.

**Friction signals:** debug-printing arbitrary structures (Rust `{:?}`, Python f-string
`=`, JS `console.log` object rendering) is the single most-exercised developer loop;
languages that require manual toString for this (older Java) generate constant friction.
Humane number/size formatting is the most-copied-snippet gap
([programming.guide](https://programming.guide/worlds-most-copied-so-snippet.html)).
Logging fragmentation (Java's four competing frameworks + facade) is the cautionary tale:
ship one leveled, structured logger in the box.

---

## Cross-cutting patterns

1. **Usage is Zipf-distributed.** 1% of API surface takes 80% of use
   ([Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf)).
   A small, perfect prelude covers most real code; the long tail can afford ceremony.
2. **Gap-filler absorption is the norm, and it works.** joda→java.time, moment→Temporal,
   Newtonsoft→System.Text.Json, SwiftyJSON→Codable, lodash→ES6+, left-pad→padStart.
   Every billion-download library above marks a gap Jet can close on day one.
3. **The absorbed winners share a design:** immutable values, one obvious call for the
   90% case, typed/derive-based where user types are involved, explicit escape hatch for
   experts. This matches Jet's beginner/expert dual-facet mission directly.
4. **Segment agreement is near-total on Tier 1.** The prelude list is not a compromise
   between segments; it is their intersection, measured independently in curricula,
   corpora, and registries.
5. **The counterexample that proves ergonomics beat purity:** Python shipped an HTTP
   client in the stdlib and still lost to requests (1B+ downloads); Go shipped a good one
   and kept the ecosystem. Being *in* the stdlib is not enough — being *frictionless* is
   the requirement.

## Limitations

Download counts overweight transitive dependencies and CI traffic. Corpus studies skew
toward open-source, professional code; the education picture rests on curricula and
teaching references rather than large student-code corpora (little is published at scale).
SO view counts measure historical friction and decay slowly after a fix ships. No
single per-method usage census exists for most languages; the Java AST studies
([Lämmel et al. 2011](https://dl.acm.org/doi/10.1145/1982185.1982471),
[Qiu et al. 2016](https://dong-qiu.github.io/papers/pdfs/ist-16-java-api-study.pdf)) are
the closest and their String/List/Map finding is treated here as representative.
