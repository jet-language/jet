# Elegance audit — 2026-09-30

Source state: commit `d382af63b` plus the working tree at audit time. Another task had 461 uncommitted files under `crates/`, `Source/`, and `Core/` (an acronym re-casing sweep, such as `FfiLanguage` → `FFILanguage`). The counts below read that working tree; the files that the effect, diagnostic, and alias findings depend on differ from HEAD only in casing. Method: `.agents/skills/elegance-audit`. The run was report-only at first; on the owner's later instruction it filed Tower cards #3986–#3998 and seven short ballots. It changes no code.

## 1. Summary

The owner chose to dive into all four top candidates. Each law below has two separate verdicts; they are never merged into one score.

| Law | Area | Click (prediction test) | Kill-check |
| --- | --- | --- | --- |
| **Diagnostic codes:** the letter says when, the number says where. | Code shapes 6 → 1; groups 52 → 28 topics in 10 areas; a mixed group rate of 15 of 52 → 0; the 133 retired rows go | 12 hit, 0 near, 0 miss; 812 of 835 codes place cleanly | Survives; lookup of pre-cutover codes is lost under option A |
| **One name per operation:** foreign names are taught, never aliased. | ≥146 alias functions go; 3 teaching mechanisms → 1 | 3 hit, 3 near, 2 miss | Survives, narrowed |
| **One effect list** | 5 homes → 1; 3 phantom names go | 6 hit, 2 near, 0 miss | Survives; it also repairs a ratified-law defect |
| **One owner per job in the module map** | 8 renames, 2 folds; `Session` has 3 owners → 1 | 7 hit, 0 near, 3 miss | Survives, narrowed |

- **Strongest existing click.** Most of Jet's vocabularies have one executable home, which every tool reads: markers, facts, diagnostics, and CLI commands. When a user learns where a word lives, they learn where every word of that kind lives. Effects are the counterexample: their list claims one home but has five (dive 3.3).
- **Biggest false rhyme.** `core.sync` holds replicated data types (CRDTs), while the locks and conditions that "sync" means in every peer language live in `core.tasks`. A close second is that Jet spells its effect list two ways: the 15 source effects in `Effects.jet` and a different 12-name `core.compiler.lang.Effect` enum.
- **Biggest area sink.** Core carries at least 146 alias functions: 131 one-line forwarders plus 15 that duplicate another function's body. Most of them are Python spellings of an operation that already has a Jet name. Syntax takes the opposite approach and teaches foreign spellings through diagnostics instead of accepting them.
- **Defect found on the way.** The header of `Effects.jet` carries ratified D-META-ONE1=A: "an effect exists exactly when it is written here". Four other homes add effect names. Sema also still emits `E0038`, which the registry marks retired.

## 2. Census

### Ledger

| Layer | Parts | Exceptions (counted) | Main laws | Law coverage |
| --- | --- | --- | --- | --- |
| Syntax and lexical space | 98 token spellings; 30 keyword mappings (21 active, 4 literals, 5 retired); 110 markers (83 active, 27 retired); 49 facts, 4 fact roots, 22 fixed fact members; 38 name categories; 99 acronym entries | 2 lowercase active markers; ≥22 tokens with more than one meaning; 42 retired spellings kept for teaching; 1 keyword-list omission | Markers are `#PascalCase`; keywords are lowercase; sigils come in pairs; `$` reads compiler facts | Markers 81/83; keywords 25/25; sigil pairs 6/6; one meaning per token is `unknown` (no executable role table) |
| Concepts and semantics | ≈175 mechanisms across 14 concept areas | 5 one-job-two-mechanism pairs; 6 false rhymes; 10 extra rules | Every failure normalizes to one `FailureContract`; access is read by default, with `&` for write and `^` for move; one registry row per marker and effect | Failure 8/8; access 3/3; declarations 25/26 `Item` |
| Diagnostics | 968 codes (819 active, 133 retired, 16 reserved); 6 code shapes; 52 hundreds bands; 25 stage labels | 13 exception kinds; 15 mixed bands; 30 codes outside `E####`/`L####` | The letter names the severity; codes are unique; the band names the domain | Letter 957/968; unique 968/968; band ≈⅓ of `E` codes |
| Core API | 46 roots, 67 nested modules; 2,389 `pub fn` declarations in `Core/**/*.jet` | ≥131 pure alias functions; ≥20 non-snake Python word spellings; 3 module-path breakers; mixed module grammar | Paths are lowercase `core.x.y`; values are snake_case and types PascalCase, with one acronym lexicon; encoding formats share `parse`/`to_string` | Paths 112/114 rows; codec verbs 7/12 formats |
| CLI and tooling | 62 commands (51 leaves, 11 groups), 66 nested actions, 16 retired; 234 flag rows (216 long); 25 manifest keys, 15 sections; 2 canonical envelopes plus ≥11 more schema IDs | 2 command-casing breakers; 5 double flag spellings; 53 flag-check exemptions; 3 overlap clusters | Commands are lowercase kebab; long flags are `--kebab`; one registry drives help, completion, and dispatch | Casing 60/62; flags 216/216; verbs 45/62 |

### Syntax and lexical space

**Home.** `crates/jet-foundation/src/Syntax.rs:20-64` (43 `LexicalEntry` rows), `crates/jet-codegen/src/Prelude/Markers.jet:24-253` (`^marker `), `crates/jet-codegen/src/Prelude/Facts.jet:10-67` (`^fact `), `crates/jet-lexer/src/Lexer/mod.rs:53-82` (keyword mappings), `crates/jet-lexer/src/Lexer/Tokens.rs:20-189` (`TokKind`).

**Exceptions.**

| Exception | Evidence | Extra rule a reader learns |
| --- | --- | --- |
| `#allow` and `#wire` are lowercase | `Markers.jet:207,211` | "Markers are PascalCase, except these two." `#allow` takes a snake_case lint name, which may explain it. |
| `!` has three meanings | `math_layout.rs:448-450`; `core_surface.rs:428-442`; `Syntax.rs:47-50` | Logical not, the error-contract suffix `T E!`, and the deny-only effect root. |
| `#` has three meanings | `Syntax/markers.rs:7-12`; `package_files.rs:88-99` | A marker prefix, the fixed-size separator `[T#N]`, and the package-version separator `pkg#1.2`. |
| `$` has four roles | `core_surface.rs:126-136`; `Scan.rs:646-672` | Facts, declaration metadata, template splices, and config environment reads. |
| `&` and `^` carry ownership and arithmetic | `core_surface.rs:374-376`; `math_layout.rs:437-444` | They are prefixes for write and move, and infix operators for bitwise and power. |
| `yield` is lexed but is not in the keyword list | `Lexer/mod.rs:79`; `package_files.rs:464-551` | Completion and the keyword guard miss one keyword. |
| 42 retired spellings stay recognized | 27 retired markers, 6 retired operators (`=>`, `++`, `--`, `~~`, `@[`, `]@`), 9 `@` fact spellings (`Scan.rs:496-527,683-725`; `core_surface.rs:160-183`) | These are counted as parts a user can meet. They serve as teaching routes, not as second meanings. |
| The AST comment still describes prefix `?T`/`!E` | `crates/jet-foundation/src/AST/types.rs:957-961`, against the parser's suffix-only rule (`E-TYPE-PREFIX`) and Core code written `T?`, `T E!` | This is prose drift, not surface. A reader of the source is misled. |

**Clicks.** Sigils come in pairs: `::`/`:=` bind, `^`/`&` move and write, `<:`/`:>` fence. The `?` family (`T?`, `?.`, `??`, `?(note)`) all concern "this might not hold a value", which is a working law. It holds whether the carrier is optional or failing.

**Latent click.** "One sigil, one meaning." `::`, `:=`, `->`, `-[…]>`, and `??` already obey it. `!`, `#`, `$`, `&`, and `^` block it.

### Concepts and semantics

**Home.** `crates/jet-foundation/src/AST/{types,items,statements,lvalues}.rs`, `crates/jet-codegen/src/Prelude/{Markers,Effects,Core,Derives}.jet`, `crates/jet-foundation/src/Policy.rs`, and `Examples/features/**`.

**Mechanisms by area.** Values 11 · types 23 · bindings and access 6 · loops and control 11 · declarations 26 · effects 15 · authority and policy 20 · memory 8 · abstraction 12 · metaprogramming 10 · modules 5 · equality 3 · errors 11 · concurrency and reactivity 14.

**One job, two mechanisms** (I8 risk):

| Job | Mechanism A | Mechanism B |
| --- | --- | --- |
| Make an owned copy | `~x` (`core_surface.rs:397-401`) | Implicit copy into an owning slot (`CheckerItems.rs:504-510`), which `copies: .Explicit` refuses |
| Attach callable policy | `#Policy(...)` | `apply(policy(...), fn)` (`callable_policies.jet:3-15`); one wrapper seam in sema |
| Structural equality | `==` | `.equal()` (`value_semantics.jet:21-38`) |
| Local failure recovery | `?? fallback` | `if r == { .Ok … .Err … }` (`errors.jet:25-38`) |
| Serialization contract | `#Codable` | `#Encode` + `#Decode` (`Markers.jet:79-86`) |

**False rhymes.**

| Shape | Meaning 1 | Meaning 2 |
| --- | --- | --- |
| `#Context { }` / `#Region { }` | Swaps an ambient resource | Enforces a lexical lifetime |
| `#FX(...) { }` / `-[Net]>` | Narrows a block's authority | Bounds a callable's effects |
| `#Error` / `Err(...)` | Declares an error family | Builds a result value |
| `Shared<T>` / `Cell<T>` | Cross-task locked state | Local cached cell |
| `Effects.jet` / `core.compiler.lang.Effect` | 15 names: `Rand`, `Log`, `GPU`, `FFI`, `FS.Read`, `FS.Write` | 12 names: `Random`, `Crypto`, `Proc`; no `Log`, `GPU`, or `FFI` (`Core/compiler/lang.jet:25`; `Core.jet:33`) |

**Clicks.** `FailureContract` gives every error spelling one carrier (`expressions.rs:34-67`). `AccessConvention` gives read, write, and move one algebra (`types.rs:929-935`). One derive body predicts every `#Equatable` type (`Derives.jet:13-52`).

### Diagnostics

**Home.** `crates/jet-codegen/src/Prelude/Diagnostics.jet`, counted with `diag_census.py` and `diag_bands.py` (tab-split rows; band = digits ÷ 100). Shape check: `crates/jet-foundation/src/Registry.rs:2759-2769`.

**Parts.** 968 codes; shapes `E####` 874, `L####` 64, `E-WORD-WORD` 23, `JT####` 3, `R####` 3, `W####` 1; 52 hundreds bands; 25 stage labels; 2 severities; 3 moments; 68 lint names.

**Exceptions.**

| Exception | Evidence | Extra rule |
| --- | --- | --- |
| The spec names three shapes, but the registry has six | `Docs/spec/diagnostics.md:18-20`; `Registry.rs:2763` accepts `E`/`L`/`R`/`W` plus any rest | "There are also `R` and `W` codes, and the digits are not checked." |
| `W0410` is the only `W` code | `Diagnostics.jet:689` | "One reserved lint is `W`, not `L`." |
| Runtime faults are split | `R0801-R0803` (`:959-961`) and `E3001-E3014` | "Runtime failures are `E30xx`, except raw-memory faults." |
| `L1101` is an error | `:688` | "`L` means lint, except here." |
| Word codes sit outside the bands | 23 `E-WORD` rows: `E-WEB-*`, `E-OSTARGET-*`, `E-ERR-*`, `E-SUBJECT-*` | "Targeting is `E33xx`, `E-WEB-*`, or `E-OSTARGET-*`." Tower #1721 records tools that skipped these codes. |
| Stage labels drift | `parse`/`parser`, `compile`/`compiler`, `parse/sema`/`sema/parse`, `retired` used as a stage, and `jet` as a catch-all for 126 rows | "The stage field is free text." |
| A stage does not predict a band | `sema` appears in 26 bands, `parse` in 11, `compile` in 10 | "You cannot find parse errors by number." |
| Mixed bands | `E00` lexing, teaching, and loops; `E01` entry, fmt defect, range types, `#Persist`, protocols; `E09` traits, `#Memo`, spread, jetOS; `E13` variadics, CLI flags, services, `.jetlib`; `E24` queries, delegation, conversion, decoding; `E25` geometry and file handles; `E29` perf budgets, reactive, a11y, proof | "This band has no theme." |
| Split domains | TLS in `E2802` and `E4201-E4203`; views in `E23xx`, apart from ownership in `E02xx`; project proof `E2389-E2391` inside the views band | "One domain lives in two bands." |
| A code that is never shown | `E2303` is "emitted as E1102" (`:692`) | "Look this one up under another code." |
| Bands start at different numbers | `E1300`, `E3620`; the rest start at `01` | — |
| Retired rows keep their numbers | 133 retired; `E00xx` is 51 of 81 retired | "Most of the first band is dead." |
| Placeholder Why text | 295 rows say "The registered … rule applies here" | "Why often says nothing." |

**Clicks.** One registry projects to the CLI, LSP, JSON, and web pages. Codes never repeat. Lints carry readable snake_case names, which `#allow(name)` and package policy use. `E21xx` is a clean command-line band: retired commands, unknown flags, bad input, driver failure.

**Latent click.** *"The letter says what kind of problem it is; the number says where; the same number means the same place for every letter."* This is HTTP's law. About a third of `E` codes and a sixth of `L` codes already follow it.

### Core API

**Home.** `Core/**/*.jet` and `crates/jet-codegen/src/Prelude/Core.jet` (module registry). Alias count: `core_aliases.py` finds a `pub fn A(params) { B(params) }` where `B` is another `pub fn` in the same file. This is a lower bound, because forwarders that call a native directly (`dumps` → `render_json`, `getpid` → `core.sys.getpid`) are not counted.

**Exceptions.**

| Exception | Evidence | Extra rule |
| --- | --- | --- |
| ≥131 alias functions in 35 files | `core.text.parse` 16, `core.math` 14, `core.time` 11, `core.files` 9, `core.encoding.base64` 8, `core.math.random` 7, `core.net.url` 7, `core.regex` 5, `core.encoding.hex` 4, `core.tasks` 4 … | "Most operations have two or more names." |
| Python names break Jet's word law | `startswith`, `endswith`, `removeprefix`, `isnan`, `isinf`, `isfinite`, `expm1`, `log1p`, `getcwd`, `listdir`, `normpath`, `joinpath`, `fullmatch`, `findall`, `finditer`, `hexlify`, `unhexlify`, `urlparse`, `getpid`, `getenv` | "snake_case separates words, except in Python names." |
| Codec verbs have up to four spellings | `json`: `parse`/`decode`/`loads`/`load` and `to_string`/`dumps`/`dump` (`Core/encoding/json.jet:83-104`); `toml`, `jsonl` | "Pick any of four." |
| US and UK spellings | `metres`, `kilometres` (`Core/units/units.jet:92-94`) | — |
| Module path breakers | `app` has no `core.` prefix; `core` is synthetic; `core.text.combinators` is parsed but unregistered | — |
| Constructor spellings | `of`, `from`, `new`, `make_*`, `queue`, `counter_new` | "Each family names its constructor differently." |
| `core.sync` is not synchronization | `core.sync` exports CRDTs (`SyncCounter`, `SyncMap`, `*_merge`); locks (`lock`, `acquire`, `release`, `notify`) live in `core.tasks` (`Core.jet:169,173`) | "Look for locks in tasks." |
| Mixed module grammar | Plurals `args`, `collections`, `devtools`, `files`, `jobs`, `tasks`, `units`; gerunds `encoding`, `testing`; verbs `compute`, `reflect`, `sync`; and many abbreviations | "Guess the form." (judged, not counted) |
| Overlap clusters | `ui`/`tui`/`term`/`app`; `tasks`/`jobs`/`service`; `sys`/`process`/`rt`; `event`/`reactive`/`watcher`; `web`/`http`/`net` | "Which module owns this?" |

No ratified decision authorizes the alias rails. Ratified D-CRYPTO-DIGEST1=A deleted a similar crypto alias set on the grounds that "greenfield law says delete the replaced form".

**Clicks.** Every encoding format parses into one `DataTree` (`Core/encoding/encoding.jet:20-31`). Archives own containers, while gzip and zstd own stream codecs. Web layers on HTTP. One casing checker and one acronym lexicon govern both source and library names.

### CLI and tooling

**Home.** `crates/jet-cli/src/CLI.rs` (`COMMANDS` `:1139-1712`, nested actions `:727-998`, `RETIRED_COMMANDS` `:1808-1937`, `BASE_FLAGS` `:1971-2238`); `Source/main.rs`; `crates/jet-pkg-model/src/Package/{mod,Blocks}.rs`; `crates/jet-foundation/src/{Report,MachineOutput}.rs`.

**Exceptions.**

| Exception | Evidence | Extra rule |
| --- | --- | --- |
| `Fold` is capitalized | `CLI.rs:1619`; dispatch `Source/main.rs:3660`; its own status payload says `"fold"` (`main.rs:7159`); ratified D-ECO-TRANSITION1 names it `jet fold` | "One command has a capital letter." This contradicts its own decision. |
| `c++` | `CLI.rs:1333-1344` | A punctuation command. |
| Destination flag | `--output` and `--out` (`CLI.rs:1980,2015`) | — |
| Machine-output flags | `--json`, plus `--machine` and `--jsonl` on `db` (`CLI.rs:2124-2129`) | — |
| Color flags | `--color=…`, plus `--no-color` and `--force-color` on `db` | — |
| Alias pairs | `--path`/`--route`, `--cook`/`--cook-mode`, `--clean`/`--force-clean` | — |
| 53 flag-check exemptions | `owns_flag_vocabulary`, `CLI.rs:2629-2688` | Flags outside the registry exist, and their count is `unknown`. |
| `install` is hidden and teaches | `main.rs:4117-4121` | — |
| Four adjacent proof commands | `check`, `prove`, `review`, `status` | "Which one tells me it works?" |
| `run --watch` and `dev` | `main.rs:4122-4142,4593-4623` | Two entrances to one runner. |
| Schema IDs are not in one registry | 2 canonical envelopes; ≥11 more `jet.*/vN` IDs from producers | — |

**Clicks.** One registry feeds help, man pages, completions, and dispatch. Long flags are `--kebab` with no exceptions. Four commands own a package-root file: `jet run`/`build`/`dev`/`test` ↔ `@run.jet`/`@build.jet`/`@dev.jet`/`@test.jet` (`package_files.rs:47-67`, D-ROLEFILE1). The command name is also the file name.

### Rhyme map

| Pair | Strongest rhyme | Kind | Evidence |
| --- | --- | --- | --- |
| Syntax ↔ Concepts | A marker's `$sites` list names declaration kinds that match the `Item` variants | near rhyme | `Markers.jet` `$sites`; `items.rs:8-85` |
| Syntax ↔ Diagnostics | `#allow(lint_name)` types the lint's snake_case name in source | rhyme, with a casing cost | `Markers.jet:207`; 68 lint names |
| Syntax ↔ Core | One casing law and acronym lexicon for source and library | rhyme, broken by Python names | `acronyms.rs`; `startswith`, `isnan` |
| Syntax ↔ CLI | Word joiners differ by layer: snake in source, kebab in flags and commands, UPPER-kebab in word codes, `jet.x/vN` in schemas | false rhyme (four joiners for one "compound word") | ledger above |
| Concepts ↔ Diagnostics | Some bands match a concept: `E02` ownership, `E05` collections, `E31` unsafe, `E33` targets, `E34` comptime effects, `E35` build | near rhyme | band table |
| Concepts ↔ Core | Effect names almost name modules: `Net`↔`core.net`, `DB`↔`core.db`, `Time`↔`core.time`, `Log`↔`core.log`; breakers `FS`↔`files`, `Rand`↔`math.random`, `Env`↔`sys`, `Exec`↔`process` | near rhyme; the second effect enum is a false rhyme | `Effects.jet:8-22`; `Core/compiler/lang.jet:25` |
| Concepts ↔ CLI | `jet test` ↔ `#Test`; `jet build` ↔ `fn build`; `jet run` ↔ `fn run`; `--gate impure=allow` ↔ `#Impure` | rhyme | `package_files.rs:47-67`; `E3411` |
| Diagnostics ↔ Core | `E28xx` net and HTTP, `E42xx` TLS, `E27xx` library parse; no module owns a band | near rhyme, split | band table |
| Diagnostics ↔ CLI | Bands match tools: `E18` REPL, `E21` command line, `E22` dev and debug, `E26` registry, `E35` build, `E36` replay, `E29` perf budget | rhyme | band table |
| Core ↔ CLI | `jet perf`↔`core.perf`, `jet db`↔`core.db`, `jet build`↔`core.build`; breakers `jet test`↔`core.testing`, `jet env`↔`core.sys` | near rhyme | ledger above |

Within one layer: syntax has paired sigils (a rhyme) and `!`/`#`/`$` (false rhymes); concepts have `FailureContract` (a rhyme) and `#Context`/`#Region` (a false rhyme); diagnostics have the letter-severity rule (a near rhyme); Core has `DataTree` codecs (a rhyme) and alias rails (second spellings); the CLI has its registry (a rhyme) and a verb/noun mix (a near rhyme).

### Celebrate

1. One executable home per vocabulary: markers, facts, diagnostics, and commands. (Effects claim this too, but they have five homes; see dive 3.3.)
2. `FailureContract`: every error spelling is one carrier.
3. Sigil pairs: `::`/`:=`, `^`/`&`, `<:`/`:>`.
4. Command, file, and entry share one name: `jet run` ↔ `@run.jet` ↔ `fn run`.
5. `DataTree`: every encoding parses into one tree.
6. Syntax teaches foreign spellings rather than accepting them: `println!` → `print` (`E0037`), `os.environ` → `env.get` (`E0039`), `jet install` → `jet fetch` (`E0043`).

### Dive candidates

The candidates are ordered by how many counted exceptions the law would explain. This order is not a score. Each verdict below is an estimate made before any dive.

| # | Target | Draft law | Area (estimate) | Click (estimate) | Kill-check risk | Rules amended |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Diagnostic codes | "The letter says what kind; the number says where; the same number means the same place for every letter." | Shapes 6 → 2–3; bands 52 → ≈25–30 with themes; stages 25 → ≈8; removes ≈15 mixed-band and ≈30 odd-shape exceptions | High: a newcomer predicts the band of an unseen code | Low: no safety, performance, or capability change. The cost is one 968-row cutover. | `Docs/spec/diagnostics.md:20` no-renumber rule; the spec's shape list; decisions behind the `E-WEB`/`E-OSTARGET`/`R08xx` rows |
| 2 | Core second spellings | "Every operation has one Jet name; foreign names are taught, never aliased — in the library as in syntax." | Removes ≥131 parts and ≥131 exceptions; also ≥20 word-law breaks | High, and it rhymes across layers with `E0037`/`E0039` | Medium: Python users lose familiar names. This is mitigated by teaching diagnostics and `jet fix`. The change touches public API. | Public Core API (owner gate); precedent D-CRYPTO-DIGEST1 |
| 3 | Effect vocabulary | "An effect is named after the Core module that performs it, and Jet has one list of them." | Two vocabularies → one; about 6–8 name exceptions | Medium-high: `-[Net]>` reads as "calls into `core.net`" | Low-medium: renaming `FS` or `Rand` touches ratified spellings | Decisions behind `Effects.jet`; `core.compiler.lang.Effect` |
| 4 | Core module map | "A module is a singular noun for one domain; overlapping domains nest under one parent." | Fixes `sync` versus `tasks`, grammar forms, and ≈5 overlap clusters | Medium | Medium: many imports and examples change | Public module paths (owner gate) |
| 5 | Sigil meanings | "One sigil, one meaning." | `!`, `#`, `$`, `&`, `^` each lose one or more meanings; ≈8 exceptions | Medium | High: syntax is owner-gated, and `&`/`^` arithmetic is common | Syntax decisions in `Syntax.rs` |
| 6 | CLI shape | "Commands are lowercase verbs; one flag per meaning; one command per question." | `Fold`, 5 double flags, the proof-command cluster, `run --watch`/`dev` | Medium | Low | D-ECO-TRANSITION1 (already says `fold`) |

## 3. Dives

Method for every dive: close the target, draft two or more laws, keep the losers with reasons, then run a closed-book prediction test. A fresh helper saw only the law (plus its key where the law has one). The held-out items were drawn with a fixed seed (`20260930`) before any showcase existed, and the answer key was written to disk before the answers came back. Scoring: **hit** is exactly right; **near** is right after one named extra fact; **miss** is anything else. All scripts are in `~/.cache/jet-dev/scratch/elegance/`; the answer keys are in `answers-diag.md` and `answers-other.md`, and the verbatim answers with helper identities are in `raw-answers.md`.

### 3.1 Diagnostic codes

**Closure.** All 968 registry rows (835 active or reserved, 133 retired), the shape check at `Registry.rs:2759-2769`, the spec's shape list (`Docs/spec/diagnostics.md:18-21`), `jet explain`, the coverage test, generated error pages, and every tracked file that names a code.

**Laws considered.**

| Law | Why it won or lost |
| --- | --- |
| **A. Letter says when; first digit says area; first two digits say topic; a topic keeps its digits under every letter.** | Chosen. It places 97% of codes cleanly, and 23 of the 28 topics already hold codes under more than one letter, so "same number, same place" does real work. |
| B. A flat list of about 25 two-digit topics with no areas | Lost. Same placement power, but 25 unordered topics are harder to hold than 10 areas with 2 or 3 topics each. |
| C. The first digit is the compiler stage (lex, parse, sema …) | Lost. `sema` alone spans 26 of today's groups; users look for what went wrong, not which pass saw it. |
| D. Word codes for everything (`E-OWN-MOVE`) | Lost. They read well alone but give no order or grouping, and 23 word codes today already needed their own tooling fix (Tower #1721). |

**The law.** *A code is a letter and four digits. `E` means Jet stops before your program runs (the compiler or a tool cannot continue), `L` is advice, and `R` means the running program stopped. The first digit is the area, the first two digits are the topic, and a topic keeps its two digits under every letter.*

Declared revision after review: the tested wording defined `E` as "the compiler rejects the program". That left tool failures, such as `JT0198` import I/O failure, `JT0199` merge conflict, and `E2106`, without a letter. The widened `E` covers them without a fourth letter, and the test items 7 and 8 (tool failures scored `E91`/`E93`) already assumed it.

Key (10 areas, 28 topics): `0` Source (00 text, 01 grammar and foreign spellings) · `1` Structure (10 names, 11 data declarations, 12 functions and calls) · `2` Types (20 types, 21 traits and generics, 22 values and collections) · `3` Flow (30 control flow, 31 failure) · `4` Memory (40 ownership, 41 unsafe) · `5` Effects (50 effects and authority, 51 concurrency) · `6` Compile time (60) · `7` Libraries (70 data, 71 network and web, 72 apps and UI, 73 foreign code, 74 targets, 75 files and processes) · `8` Assurance (80 testing and docs, 81 contracts and proof, 82 performance) · `9` Project (90 packages, 91 build, 92 command line, 93 registry and supply).

**Placement test.** Five fresh classifiers placed every active and reserved row by its meaning, without using its current number. 812 rows placed clearly, 20 were genuine ties, and 3 did not fit. A first keyword-matching classifier was discarded after a spot check caught nonsense placements. The spot check of the final shards found only tie-level disagreements.

| Result | Count | What it means |
| --- | --- | --- |
| Clear | 812 (97.2%) | — |
| Ties | 20 | Most are `50`/`70` (crypto misuse: authority or data?) and `40`/`51` (views crossing tasks). They need one tie rule: *a code lives where its fix is made.* |
| No fit | 3 | `E2501` file handles and `L2201` documentation led to adding topic `75` and widening `80`; `E0425` is a placeholder "never a Jet diagnostic" and is deleted. |
| Largest topic | `E90`: 81 codes | Fits within the 99 slots. |

**Prediction test: 12 hit, 0 near, 0 miss.** There were eight forward items (the message, then predict letter and topic) and four reverse items (a bare code, then predict the topic). The questions paraphrase the real message, which is how users meet a code. One weak real message ("`{source}` was rejected: `{detail}`") was kept in the draw and still hit.

**Showcase.**

| Today | Under the law |
| --- | --- |
| `E2901` perf budget · `E2912` reactive · `E2931` accessibility · `E2940` proof evidence (one group, four topics) | `E8201` perf budget · `E7201` reactive · `L7202` accessibility · `E8101` proof evidence |
| TLS failures: `E2802` and `E4201-E4203` (two groups) | `E71xx`: all TLS failures in one topic |
| Runtime faults: `E3010` arithmetic, `R0801` raw memory | `R2201` arithmetic, `R4101` raw memory: the letter says "at run time", and the number says where, as for compile errors |
| `L1101` is an error that starts with `L` | `E5101`: an unjoined task is a concurrency error |

*The thought to have: "an ownership lint is `L40`, so an ownership error is `E40` — I can find it."* Numbers in the right column are `proposed`.

**Area recount.**

| | Before | After |
| --- | --- | --- |
| Code shapes | 6 (`E####`, `L####`, `R####`, `W####`, `JT####`, `E-WORD-WORD`) | 1 (letter + 4 digits, 3 letters) |
| Groups | 52 unnamed hundreds groups | 10 areas, 28 named topics |
| Mixed groups | 15 | 0 |
| Stage labels | 25 free-text labels | 6 (lex, parse, sema, build, run, tool); internal only |
| Rows | 968 | 834 (133 retired rows deleted, and the `E0425` placeholder) |
| Exceptions | 13 kinds (see census) | 1 tie rule and 20 tie cases |

**Kill-check.** Safety and performance are unaffected. The beginner path gets easier. No language capability is lost. One tool path is lost under option A: `jet explain` stops answering pre-cutover codes found in old logs. Greenfield law in `AGENTS.md` classes such lookups as legacy readers to remove, so option A is recommended. Keeping them (option C) needs an owner-ratified compatibility exception with a removal condition, and its lookup rows would sit outside the active registry. I4 still holds after the snapshots are regenerated. No parallel mechanism appears: the six shapes become one, and the word-code support added by Tower #1721 becomes deletable. **Survives, with the lookup loss named.**

**Amendments.** `Docs/spec/diagnostics.md:18-21` (the shape list and "do not reuse or renumber"). The rows behind the word codes cite D-JSBIND1, D-WASM1, D-WEBRUN1=A, D-SUBJECT-CALL1=A, D-TYPE-SUFFIX1, and D-APP-UNIFY1. Whether those decisions fixed the code spelling itself is `unknown`, so each must be checked before it is amended.

**Migration surface.** 834 registry rows re-keyed; 64,090 code references in 4,052 tracked files: `Docs` 36,888 (mostly the generated site; historical audits stay unchanged as dated evidence), `Tools` 9,211, `crates` 5,363, `tests/ui` 3,435 in 1,320 snapshots, `Compiler` 2,707, `Source` 1,197, `Jetpack` 837, and about 1,500 more in other tests. Also the code grammar in `Explain::is_code` and `tests/diagnostics_coverage.rs`.

### 3.2 One name per operation

**Closure.** All 2,389 `pub fn` declarations in `Core/`; the 146 alias functions (131 same-file forwarders from `core_aliases.py`, plus 15 that duplicate another public function's body, from the names closure); today's three teaching mechanisms; the casing checker; the 21-name ambient prelude; and every call site.

Today's teaching is split three ways:

| Mechanism | Covers | Code |
| --- | --- | --- |
| Hard-coded sema branches (`direct_calls.rs:1212-1300`, `expr.rs:7301-7310`) | `println!`, `open`, `getenv`, `os.environ`, `async` | `E0037`–`E0040` (`E0038` is emitted but marked retired) |
| `FOREIGN_METHOD_ALIASES`, 28 rows (`Sema/Diagnostics.rs:1150-1186`) | Receiver methods only, such as `append`, `size`, `contains_key` | `E0311` |
| Nothing | Module functions (`json.loads`, `files.getcwd`) | They compile through 146 aliases |

**Laws considered.**

| Law | Why it won or lost |
| --- | --- |
| **A. Every operation has one Jet name; any foreign spelling is taught by one table and never aliased.** | Chosen. It deletes the aliases and merges three teaching mechanisms into one. |
| B. Keep the foreign names in an opt-in `core.compat.python` | Lost. It is a parallel mechanism (I8): readers still meet two vocabularies in other people's code. |
| C. Keep the aliases but hide them from docs and completion | Lost. Hidden second names are worse for readers than visible ones. |

**The law.** *Every Core operation has exactly one name, written as whole English words joined by underscores. A spelling from another language or an older Jet name does not compile, and the error names the one Jet spelling.*

**Prediction test: 3 hit, 3 near, 2 miss** (after one declared key correction; see below).

| Form | Answer | Score |
| --- | --- | --- |
| `base32.b32encode`, `regex.regex`, `text.islower` | error → `encode`, `compile`, `is_lower` | 3 hit |
| `base64.a2b_base64` | error → `decode` (corrected key: the MIME decode, today `decodebytes`) | near; the missing fact is that the MIME form accepts whitespace and plain `decode` does not (`plain_calls.rs:1741-1746`) |
| `jsonl.loads` | error → `load` (key: `parse`) | near; the missing fact is "formats read with `parse` and write with `to_string`", which is an existing click |
| `text.rindex` | error → `last_index` (key: `find_last`) | near; the missing fact is "searches use `find`" |
| `units.kilometres` | compiles | miss; British spelling is also whole English words, so the law needs "American spelling" |
| `sync.text_edit` | compiles | miss; an internal Jet duplicate looks like a normal name, so the law cannot see it |

Hits outnumber misses, so the click holds, **narrowed**. The law adds four short rules: American spelling, `parse`/`to_string` for formats, `find` for searches, and a distinct name for the MIME decode. Internal duplicates such as `text_edit`/`text_set` need a check on Core itself ("no two public functions share a body"), not a user rule.

**Showcase.**

```jet
// Today: syntax teaches, the library accepts
println!("hi")               // E0037: use print
data := json.loads(text)     // compiles
same := json.parse(text)

// Proposed: one table teaches both
println!("hi")               // error: use print
data := json.loads(text)     // error: use json.parse
data := json.parse(text)
```

*The thought to have: "Jet never has two names for one thing; if my Python name fails, the error tells me the Jet one."*

**Area recount.**

| | Before | After |
| --- | --- | --- |
| Public functions | 2,389 | ≤2,243 (−146 aliases) |
| Teaching mechanisms | 3 (branches, method table, none) | 1 table with one code; about 28 + 146 rows plus the branch spellings |
| Non-word names | ≥20 confirmed (`startswith`, `isnan`, `getcwd`, …); exact total `unknown` because the casing check does not segment words | 0 in the alias set; canonical non-word names such as `rfind`, `fabs`, `exp_m1` become rename questions |
| Exceptions | Many names per operation | 4 short rules (spelling, format verbs, find, and the MIME decode name) |

Key correction, declared: the pre-registered key named `decode` for `a2b_base64`, but that alias forwards to `decodebytes`, which accepts whitespace. A one-name cutover must keep that behavior under one Jet name (for example `decode_mime`), not merge it into `decode`. The raw answers and the correction are recorded in `raw-answers.md`.

Only 27 of the 146 aliases have any qualified call site in `Examples/`, `tests/`, or `Core/` (126 sites in all). The other 119 have none.

**Kill-check.** A beginner coming from Python loses names that silently worked. Each one becomes an error with the exact fix, which `jet fix` can apply as a structured edit. No capability is lost, and performance is unaffected. I8 improves, since three teaching mechanisms become one. The law is narrowed so that it does not claim to catch internal duplicates. **Survives, narrowed.**

**Amendments.** No ratified decision authorizes the alias rails; the source comments call them "Python-compatible" without a decision ID. Ratified D-CRYPTO-DIGEST1=A is precedent for deletion. Public Core API is an owner gate.

**Migration surface.** 146 alias declarations in about 40 files; 126 call sites across 27 aliases; the export rows in `Core.jet`; the sema teaching branches and `FOREIGN_METHOD_ALIASES` fold into one table; and snapshots for the new teaching rows.

### 3.3 One effect list

**Closure.** Every place that names an effect:

| Home | Names it adds |
| --- | --- |
| `Effects.jet:8-22`, whose header says "an effect exists exactly when it is written here" (D-META-ONE1=A) | 13 roots, 2 leaves |
| `BUILTIN_EFFECT_LEAVES`, `effects_surface.rs:37-67` | 29 leaves; 27 are not in `Effects.jet` (`Time.Wait`, `Exec.Args`, `DB.Read`, `Rand.Draw`, 21 `FFI.*` …) |
| `effect_action`, `Authority.rs:549-581` | Roots `Mem` and `Panic`, and the leaf `Mem.Alloc` |
| `TargetMachine.rs:320-335` | `Time.Sleep` |
| `core.compiler.lang.Effect`, `Core/compiler/lang.jet:25-38` | 12 names, three of them unknown elsewhere (`Crypto`, `Random`, `Proc`); no `Log`, `GPU`, `FFI`, `Mem`, or `Panic` |
| `core_calls.rs:448-655` | A hard-coded module → leaf table |

Probe on the current binary: `-[Crypto]>` fails with `E0119` "`Crypto` isn't a known effect", although `lang.Effect.Crypto` is a public name, and `-[Time.Wait]>` is accepted. A qualified `#FX(lang.Effect.Random)` probe was blocked, because importing `core.compiler.lang` fails on this binary mid-sweep (`unknown`).

Usage in Core signatures: `FS` 136 · `Time.Wait` 119 · `Net` 109 · `Env` 98 · `Rand` 59 · `IO` 34 · `Time` 31 · `Secret` 25 · `Log` 25 · `DB` 22 · `GPU` 17 · `FS.Write` 15 · `Exec` 14 · `FS.Read` 9 · `Exec.Args` 3 · `Browser` 2.

**Laws considered.**

| Law | Why it won or lost |
| --- | --- |
| **A. One list; an effect names the part of the world a call touches; a dot narrows it; permission for a name covers what is under it.** | Chosen. It restores the ratified rule and keeps D-EFFTREE1 unchanged. |
| B. An effect is named after the Core module that performs it (`-[Files]>`, `-[Process]>`) | Lost. Only 4 of 13 roots match a module today (`Net`, `DB`, `Time`, `Log`). Modules such as `files` (`FS` and `Env`) perform several effects, and `Net` is performed by five modules. |
| C. Keep today's homes and generate a check that they agree | Lost. That is still five homes, and it hides the defect rather than removing it. |

**The law.** *Jet has one list of effects. An effect names the part of the outside world a call touches; a dot narrows it (`FS.Read`), and permission for a name covers everything under it. Two roots, `Mem` and `Panic`, are limits you can only deny, never grant.*

The second sentence is part of the law, not a hidden exception: ratified D-PANICROOT1=A and D-AUTHORITY-MEM1=B make both roots deny-only (`crates/jet-sema/src/Sema/Effects.rs:325-337`), and the prediction test did not probe them.

**Prediction test: 6 hit, 2 near, 0 miss.** Six real Core functions were drawn with the seed, plus two structural questions (`-[Crypto]>` is invalid; a block granted `FS` may call `-[FS.Write]>`). Both near answers dropped `Time.Wait`: a blocking task receive was answered `Time`, and an HTTP call that waits was answered `Net`. The missing fact: *waiting always adds `Time.Wait`, even on a network call.* The first near answer matches today's code, since `core.tasks.get` declares `-[Time]>` while other blocking calls declare `Time.Wait`. That is an exception to fix.

**Showcase.**

```jet
// Today: two spellings meet in one program
use core.compiler.lang as lang
fn f() -[Rand]> Int { … }        // accepted
#FX(lang.Effect.Random) { … }    // a name the effect list does not have
fn g() -[Crypto]> Int { … }      // E0119, though lang.Effect.Crypto exists

// Proposed: one list, every reader agrees
fn f() -[Rand]> Int { … }
#FX(lang.Effect.Rand) { … }      // generated from the one list
```

*The thought to have: "effects are one list I can read; a dot narrows, a root covers."*

**Area recount.** Homes 5 → 1. Distinct root spellings 18 → 15 (`Crypto`, `Random`, and `Proc` go; `Mem` and `Panic` join the list and are marked deny-only there). About 31 leaves move into `Effects.jet` (including `Mem.Alloc` and `Mem.Rc`), and the reflective enum is generated from it or deleted. Exceptions: 3 phantom names and 1 mis-declared call → 0; the waiting rule and the deny-only rule each become one written rule.

**Kill-check.** Safety: nothing loosens, since the change only moves names, keeps D-EFFTREE1 ancestor coverage, and keeps D-EFFECT-DECL1=A package leaves. I9 is unaffected. Renaming roots to whole words (`Files`, `Random`, `Process`) was killed from this proposal, because it churns ratified spellings for a small gain; it remains an optional ballot below. **Survives.** Part of this is a defect, not a taste call: the code breaks ratified D-META-ONE1=A.

**Amendments.** None; this restores D-META-ONE1=A and keeps D-EFFTREE1=A, D-EFFECT-DECL1=A, D-PANICROOT1=A, and D-AUTHORITY-MEM1=B. The kill-check must confirm that a positive `Panic` or `Mem` grant is still rejected after the move.

**Migration surface.** `Effects.jet` (+2 roots, about 29 leaves); `effects_surface.rs:37-67`; `Authority.rs:549-581` (reads the list); `TargetMachine.rs:320-335`; `Core/compiler/lang.jet:25-38`; `core_calls.rs:448-655`; `core.tasks.get` (`Time` → `Time.Wait`); `tests/marker_rule_signatures.rs:108-115`.

### 3.4 One owner per job in the module map

**Closure.** 46 top-level and 67 nested modules (`Core.jet:15-230`), two parsed but unregistered modules (`text.combinators`, `text.string`), the dependency graph (`RingLayer.rs:20-117`), and the overlap clusters from the modules closure.

Strongest overlaps:

| Job | Owners today |
| --- | --- |
| `Session` | `app`, `web`, `auth` (`Core/app/app.jet:31-41`; `Core/web/web.jet:31-75`; `Core.jet:27`) |
| Working directory, environment, pid | `sys`, `process`, `files`, `files.path` |
| Terminal presentation | `term` (I/O and style) and `tui` (width, fill, style, table) |
| Locks | `tasks`, while `sync` holds CRDTs |

**Laws considered.**

| Law | Why it won or lost |
| --- | --- |
| **A. One domain per module, named by a singular noun, a standard acronym, or one of a fixed list of conventional abbreviations; narrower domains nest; every job has one owner.** | Chosen. |
| B. Whole words for every module name (`system`, `terminal`, `memory`, `cryptography`) | Split out as a separate ballot. It predicts spelling perfectly but renames the 9 listed abbreviations. |
| C. Keep the names and only fix the overlaps | Lost. It leaves plural/singular and verb/noun guessing in place. |

**The law.** *Each Core module covers one domain and is named by a singular noun, a standard acronym, or one of a fixed list of abbreviations (`auth`, `crypto`, `mem`, `mod`, `perf`, `regex`, `rt`, `sys`, `term`); a narrower domain nests under its parent with a dot; every job has exactly one module that owns it.*

**Proposed map changes** (`proposed`): `collections`→`collection`, `files`→`file`, `jobs`→`job`, `tasks`→`task`, `units`→`unit`, `testing`→`test` (it rhymes with `jet test` and `#Test`), `args`→`cli` (it rhymes with the `#CLI` marker), `sync`→`replica`, `tui`→`term.ui`, and `app` folds away. `process` owns the working directory, environment, pid, arguments, and exit; `sys` narrows to host facts. `text.combinators` and `text.string` are registered or folded.

**Prediction test: 7 hit, 0 near, 3 miss.** The hits were pid → `process`, terminal table → `term.ui`, task lock → `task`, CRDT counter → `replica`, flags → `cli`, golden test → `test`, and TLS → `net.tls`. The misses:

| Job | Answer | Key | Lesson |
| --- | --- | --- | --- |
| Read an environment variable | `sys` | `process` | Keeping the name `sys` invites the environment. Rename it to a narrower host word, or add the rule "per-process state lives in `process`". |
| Working directory | `file.path` | `process` | Same rule needed. |
| Signed-in web session | `auth` | `web` | The helper's answer is the better design: identity is `auth`'s domain, and `auth` already exports `Session`. This is a declared revision after the test; the score stays 3 misses. |

**Revised after the test** (not rescored): `Session` belongs to `auth`, `web` re-uses it, and `app` folds away.

**Area recount.** Roots 46 → 44 (`app` folded, `tui` nested). Grammar breakers 7 plurals → 1 (`devtools`, a product name). `Session` owners 3 → 1. Working directory, environment, and pid owners 3–4 → 1. Unregistered modules 2 → 0. Remaining exceptions: 11. They are `devtools`, the synthetic `core` root, and the 9 listed abbreviations. Ballot 7 option B (whole words) would bring the abbreviations to 0 at the cost of 9 more renames.

**Kill-check.** No capability is lost. The churn is 1,113 import lines for the renames (866 in tests, 119 in examples, 71 in `Tools`), plus 159 `use core.sys` lines that need review as `sys` narrows. The `app` fold may touch D-APP-UNIFY1 (ratified B, scope `unknown`), so it moves to a ballot option. The whole-word renames are split out. **Survives, narrowed.**

**Amendments.** D-RANDSPLIT1 is kept (`math.random` and `crypto.random` are two domains with different guarantees). D-APP-UNIFY1 may be touched (`unknown`). D-RINGLAYER1 is unaffected.

**Migration surface.** 1,113 import lines; `Core.jet` module rows; `RingLayer.rs` generated edges; the `Core/` directory layout; the module list in `tests/ui` snapshots such as `core_async_loadable_retired.stderr`.

### Cross-layer rhymes the dives create

- The CLI verb, the marker, and the module share one word: `jet test` ↔ `#Test` ↔ `core.test`, and `#CLI` ↔ `core.cli`.
- Diagnostic topics mirror the library: `E71` network and web, `E75` files and processes, `E70` data.
- Syntax and library teach foreign spellings the same way, through one table and one code.

## 4. Owner decisions (filed as ballots on the owner's instruction)

Ballots: D-DIAG-CODE-LAW1 (#3994), D-CORE-ONE-NAME1 and D-CORE-SPELLING1 (#3995), D-EFFECT-ENUM1 (#3986), D-EFFECT-ROOT-WORDS1 (#3996), D-CORE-MODULE-MAP1 and D-CORE-MODULE-ABBREV1 (#3997). Defect cards need no ballot: #3987 (`jet fold`), #3988 (`E0038`), #3989 (`Time.Wait`), #3990 (AST comment), #3991 (code shapes, blocked by #3994), #3992 (unregistered modules), and #3993 (CLI flag pairs). #3998 records the sigil dive that is still owed.

1. **Adopt the diagnostic code law.** A: adopt the letter, area, and topic scheme with one cutover (recommended). B: adopt it for new codes only. C: adopt it but keep retired codes answerable by `jet explain`.
2. **Retire foreign-name aliases in Core.** A: one teaching table, and delete all aliases (recommended). B: an opt-in `core.compat.python` module. C: keep the aliases.
3. **Spelling rules for Core names.** American spelling; `parse`/`to_string` for formats; `find` for searches; one name for the MIME decode (for example `decode_mime`). Each rule can be adopted separately.
4. **Restore the one effect list (D-META-ONE1=A).** This is a defect repair; it needs a card, not a ballot. A: generate `core.compiler.lang.Effect` from `Effects.jet`. B: delete the reflective enum.
5. **Effect root spelling.** A: keep `FS`, `Rand`, `Env`, `Exec` (recommended). B: whole words (`Files`, `Random`, `Environment`, `Process`).
6. **Core module map.** Decide each rename separately: the plurals, `args`→`cli`, `testing`→`test`, `sync`→`replica`, `tui`→`term.ui`, `process` owning per-process state, and `Session` owned by `auth` with `app` folded.
7. **Module name abbreviations.** A: keep conventional short names (recommended). B: whole words (`system`, `terminal`, `memory`).
8. **`jet Fold`.** Fix the casing to match D-ECO-TRANSITION1; this is a defect card, not a ballot.

## 5. Unknowns

- The exact number of tokens with more than one meaning: there is no executable role table, and ≥22 is a lower bound.
- The total number of non-word Core names: the casing check does not segment words; 20 are confirmed.
- The number of flags outside the CLI registry: 53 routes skip the generic flag check.
- The number of `jet.*/vN` schema IDs: there is no central registry; ≥13.
- Whether the decisions behind the word codes fixed the code spelling itself.
- The scope of D-APP-UNIFY1 with respect to folding `app`.
- The qualified `#FX(lang.Effect.X)` behavior: blocked by the in-flight acronym sweep on the current binary.
- One probe run exited 134 once and did not reproduce; it is not attributed.
- **Strongest unverified assumption:** the prediction tests used paraphrased messages and short keys. Real users may recall bare numbers less well than a model given the key.

## 6. Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 diagnostic code law | card | #3994 |
| F2 Core alias rails and split teaching | card | #3995 |
| F3 effect vocabulary breaks D-META-ONE1=A | card | #3986 |
| F4 Core module map overlaps | card | #3997 |
| F5 `jet Fold` casing against D-ECO-TRANSITION1 | card | #3987 |
| F6 `E0038` emitted while registry says retired | card | #3988 |
| F7 `core.tasks` blocking calls declare `Time` | card | #3989 |
| F8 stale AST comment describes prefix `?T`/`!E` | card | #3990 |
| F9 word codes previously outside tooling | no-action | archived: fixed by Tower #1721; cited as evidence only |
| F10 diagnostic code shapes disagree with the spec | card | #3991 |
| F11 `core.text.combinators`/`core.text.string` unregistered | card | #3992 |
| F12 CLI flags with second spellings | card | #3993 |
| F13 effect root spelling | card | #3996 |
| F14 sigils with several meanings (not dived) | card | #3998 |
<!-- /audit-dispositions -->
