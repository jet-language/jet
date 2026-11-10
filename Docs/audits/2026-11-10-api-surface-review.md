# D-API-SURFACE1 review: the public surface in every check, and computed versions

Date: 2026-11-10. Author: BallotLead-3 (Claude Opus). Compiler: a0.0.43 (dev c460ba91e5).
Board: ballot D-API-SURFACE1 on card #6281. Earlier pass: BallotFix REPORT.md, section D-API-SURFACE1.

## Question

Should every `jet check` compare a package's public surface, including effects and errors, with a committed
`package.api` file? And should publish compute the version?

## Findings that changed the ballot

1. **Method rows already carry solved effects.** The ballot said "a method's row carries only its written
   effects". In `Compiler/JetSema/Source/Sema/PublicApi.jet`, `sema_public_api_methods` (`:327-330`) pushes
   `sema_public_api_fn` for every public method. `sema_public_api_fn` (`:322-324`) uses
   `sema_public_api_solved_effects` (`:311-319`), the body's solved row. Only a trait requirement row
   (`sema_public_api_trait_method`, `:415-418`) uses written effects, which is correct because a requirement
   has no body. The file has not changed since t6c (`git log 76955986d0..HEAD` is empty), so the earlier
   claim was wrong, not stale. The "methods carry solved effects" build item is dropped from A.
2. **`jet check --locked` does not exist.** The ballot called it "a flag that already exists". On a0.0.43,
   `jet check --locked lib.jet` gives E2102: "`--locked` is only valid with `jet build`, `jet fetch`, `jet jobs`,
   `jet run` or `jet inspect`". A now says it extends `--locked` to check.
3. **Per-check cost, measured.** `probes/gen_lib.mjs` writes a library with 1,000 public functions and 100
   structs with two methods each. `timeit.sh` ran 5 cold runs each, under a 4G cap:

   | Command | min | median | peak RSS |
   |---|---|---|---|
   | `jet check lib.jet` | 0.228 s | 0.291 s | 112 MB |
   | `jet doc --json lib.jet` | 1.253 s | 1.369 s | 117 MB |
   | `jet doc --check lib.jet` | 1.359 s | 1.429 s | 119 MB (rc 1: undocumented-item warnings) |

   Publish renders rows from a separate Doc-mode compile (`Compiler/JetCli/Source/Cli/Publish.jet:502-525`,
   `jet_cli_compile_file(..., JetDriverCompileMode.Doc)`). Copying that into check would cost up to about
   1.1 s per check on this library; the doc figure includes rendering. A therefore renders rows from the
   check's own checked items, one pass over public items. [INFERENCE] That pass is linear in public items,
   like `sema_public_api_items`. It was not timed separately, because no CLI verb exposes it alone.

## Probes (a0.0.43)

- `jet check v2.jet` (the body now reads the clock): `ok: v2.jet has no problems`.
- `jet review v1.jet v2.jet`: `verdict authority widened` and `widened: effect:greeting`. It also prints
  10 "semantic changes", several of which are remove and add pairs. Review keys items by module, and the
  two files have different names (v1, v2), so the same function appears removed and re-added. That noise is
  review's; A's rows are keyed by public name.

## Census (dev c460ba91e5)

- 2,490 `package.jet` files; 237 outside `tests/` and `Examples/` (Compiler 102, Tools 103, Jetpack 30, Docs 2).
- 14 committed `.api` files, all plugin fixture caches under `fixture-state/cache/api/`.

## Peers (sourced)

- Elm: "Elm automatically enforces semantic versioning by comparing API changes"; `elm bump` computes the
  version. <https://github.com/elm/compiler/blob/master/docs/elm.json/package.md>
- Kotlin's binary-compatibility-validator checks a committed `.api` dump.
  <https://github.com/Kotlin/binary-compatibility-validator>
- TypeScript API Extractor commits an `.api.md` report. <https://api-extractor.com/pages/overview/demo_api_report/>
- cargo-semver-checks checks the chosen bump against the API. <https://github.com/obi1kenobi/cargo-semver-checks>

## Adversarial review (per claim)

| Claim | Attack | Verdict |
|---|---|---|
| Method rows lack solved effects | Read the row builder | **Failed**: they have them (PublicApi.jet:327-330). Removed from the gaps and from A |
| `--locked` already exists | Run it on check | **Failed**: E2102 on check. A extends it |
| Checking every time is cheap | Publish uses a second compile | Measured: a second compile would add up to about 1.1 s per 1,000 functions. A uses the check's own items |
| Inferred errors and markers are absent from rows | Read the row builder | Holds: no error-set or marker text in `sema_public_api_fn` |
| A narrowed effect is not breaking | A caller relying on the exact row | No caller can break from a narrower set; A counts it as fixed |
| Tradeoff "an accept step per surface edit" | Added rows need no step | Reworded to "per breaking surface edit" |

## Remaining losses

- A breaking edit needs one accept command. That is the deliberate point.
- Every library check does one more pass over its public items.
- Some computed bumps are larger than an author would pick.
