# jetc1 native link recipe draft — S3 to SeedWork1 stage 1

Status: **not executed, not an admitted compiler, not a fixed-point receipt**. Statements marked `[SOURCE]` describe inspected implementation; `[INFERENCE]` describes proposed execution or an unmeasured required symbol. Measurement baseline is native/self-host-4's private a9ffa38bc ancestry, NOT the current production composition. The dev Core resolution snapshot is 747eef360. Production artifacts must instead pin NativeInteg3's composed compiler SHA and RuntimeJet7/RtObject3's independently pinned FINAL runtime epoch.

## 1. Entry and source identity

[SOURCE] `Compiler/JetCli/main.jet:4–6` is the hosted process entry:

```jet
fn run() {
    process.exit(jet_cli_main(process.args()))
}
```

[SOURCE] The current self-host assembler appends the equivalent no-argument Unit `run` to the assembled compiler unit, with a distinct `selfhost_process` alias. The package grants the JetCli entry authority budget. `compiler.map.json.source_segments` maps its compiler-source byte spans. `Compiler/Bootstrap/sources.list` is the source order; it includes the compiler packages and Jetpack dependencies, not an independently invented package list.

[INFERENCE] Production stage 1 uses this SAME source/unit/map identity in the Jet-front producer and native lowering driver. NativeFeed3's `run-self-front.mjs` writes schema-version/unit/map/program hashes in `self.inventory.json`; the producer/candidate, writer and consumer decoder must share a compiler/schema pin. Never decode a current `.mird` using the old a9ffa38bc driver.

## 2. Object list per package slice — actual limitation

[SOURCE] `run-slices.sh` creates `slices/<Pkg>/roots.txt`, converts a pruned `self.mird`, and collects `run/self.{progress,completed,issues,prepare-issues,stats}`. Packages are scheduled by actual root-count ascending in `SLICE-ORDER.tsv`. These are coverage measurements, not separately linkable compiler libraries.

| Package slice | Current object input to jetc1 | Coverage artifacts |
|---|---|---|
| JetFoundation | **none: measurement-only** | `slices/JetFoundation/run/self.*` |
| JetLexer | **none: measurement-only** | `slices/JetLexer/run/self.*` |
| JetParser | **none: measurement-only** | `slices/JetParser/run/self.*` |
| JetOptimizer | **none: measurement-only** | `slices/JetOptimizer/run/self.*` |
| JetSema | **none: measurement-only** | `slices/JetSema/run/self.*` |
| JetCodegen | **none: measurement-only** | `slices/JetCodegen/run/self.*` |
| JetEval | **none: measurement-only** | `slices/JetEval/run/self.*` |
| JetRepl | **none: measurement-only** | `slices/JetRepl/run/self.*` |
| JetDriver | **none: measurement-only** | `slices/JetDriver/run/self.*` |
| JetBackend | **none: measurement-only** | `slices/JetBackend/run/self.*` |
| JetCli | **none: measurement-only** | `slices/JetCli/run/self.*` |

[INFERENCE] Only packages actually present in r2's observed inventory appear in the generated slice manifest. This table names the known compiler package structure, not measured object existence or final root count. Shared Core/glue and Jetpack dependencies belong to the whole-unit closure; do not count dependency copies from multiple slices as unique compiler functions or link input objects.

[SOURCE] `SelfHostLower.jet:293–298` returns without object emission when a slice has no entry. `Image/Object.jet:85–88` requires a valid no-argument Unit entry. `Object.jet:126–137` makes compiler-function symbols LOCAL; only `main` is the defined global. `Object.jet:151–159` resolves every Function-ID call within the SAME object and rejects an absent function. Runtime calls become global undefined PLT32 symbols (`Object.jet:160–180`).

**Therefore [SOURCE]: passing `slices/*/run/self.o` to the linker is NOT an existing jetc1 build path.** Even forcing an entry into each slice would introduce multiple global `main` definitions, while local compiler symbols would not satisfy cross-package references. No such forced-entry workaround is proposed.

[INFERENCE] First jetc1 should use one whole-compiler entry closure, producing one `run/self.o`. It reuses the existing supported whole-unit object path. If measured whole-unit limits make this impossible, a real disjoint multi-object emitter with stable exported/imported compiler symbols and one startup entry is an additional prerequisite, not a shell recipe or ready feature.

### Conditional ranked blocker: separate compilation

| Rank / activation | Blocker | Required implementation | Risk covered |
|---|---|---|---|
| **[INFERENCE] conditional only; no measured coverage rank yet** | **No separate compilation: object exports only `main`; cross-object Function IDs are unresolved** | Stable global symbols for exported compiler functions; undefined symbol references for cross-object calls; deterministic disjoint function/glue/data ownership; one startup/entry object; retain current static-fixup and initialization order | Whole-unit lowering peak memory for the 13.65 MB compiler-source scale recorded in RUST-EXIT §1B; the private assembled selfhost unit is larger once dependency blocks and entry are included |

[INFERENCE] Activate this item **only if r3 whole-unit lowering exceeds its memory cap**. Package coverage data alone, a Rust-front dump memory peak, or unavailable proof resources do not justify a separate-compilation rewrite. Until that specific measurement, the one-object recipe remains the executable design path.

## 3. Proposed stage-1 link inputs

[INFERENCE] Ordered production input list:

1. **whole-compiler `self.o`**, from a current-schema Jet-front feed and SAME-tree native lowering, only after all preparation/lowering/LIR/object issues are zero;
2. **FINAL Jet runtime pack**, generated from the selected runtime epoch and admitted by its owner; not an old seed pack or reconstructed historical archive;
3. explicit vetted foreign-provider object(s) ONLY if the FINAL pack contract does not include them. Their export/ABI inventory must be observed, not inferred from Core route metadata.

[SOURCE] The current linker driver exposes `x64_link_executable(names, bufs)` and `x64_link_runtime_pack(names, bufs, extra_roots)`. `Tests/LinkRuntime.jet` accepts `unit.jet <output> <input>...`; the self-host driver can link its whole object when `SELFHOST_PACK` names a pack. These are in-process Jet linker operations, not `cc`, `ld`, `rustc`, or LLVM calls.

[INFERENCE] For the first development-only jetc1 measurement, the RUST-EXIT plan allows a recorded seed/runtime dependency that dies at cutover. This is NOT admission of a historical pack and NOT a substitute for porting the runtime. The production/fixed-point recipe requires RuntimeJet7's Jet-only FINAL artifact and its identity receipt. The held r3 script's Rust seed pack may measure an interim link but is not the Rust-free fixed-point pack.

## 4. Extern symbols expected from pack — static candidate inventory

[INFERENCE] A native compiler object's unavoidable startup candidate is `jet_rt_main`, directly emitted by `Object.jet:79`. `LINKPLAN.tsv` lists direct Core registry symbols and emitter runtime references; `LINKPLAN-RESOLVED.tsv` follows current-dev Core bodies to registered raw leaves. These tables are static over-approximations, NOT an ELF undefined-symbol list.

[INFERENCE] The 23 direct Core module calls absent from the adapter table resolve to 11 source-only bodies and 12 source bodies reaching raw leaves, all with existing route rows. Representative actual source chains:

| Compiler source call | Resolved candidate raw export |
|---|---|
| `core.files.is_file`, `is_symlink` | `jet_std_fs_stat` |
| `core.files.realpath` | `jet_std_fs_canonicalize` |
| `core.files.cwd` | `jet_std_env_current_dir` |
| `core.files.listdir` | `jet_std_fs_list_dir` |
| `core.files.copy2` | `jet_std_fs_create_dir_all`, `jet_std_fs_read_bytes`, `jet_std_fs_write_bytes` |
| `core.files.rmtree` | `jet_std_fs_remove_all` |
| `core.process.current_pid` | `jet_std_os_pid` |
| `core.sys.is_linux`, `is_macos`, `is_windows` | `jet_std_os_name` |
| `core.time.unix_ns` | `jet_time_now_utc` |

[INFERENCE] The 65 absent-table **emitted runtime references** (including allocator, exact Int and text-builder helpers) describe programs produced BY the compiler and must not automatically be treated as imports OF jetc1. The seven untyped receiver candidates likewise require MIR receiver resolution. All 72 source-missing rows are retained in `LINKPLAN-BLOCKERS.tsv`; none is silently promoted to a measured jetc1 link failure.

[INFERENCE] After the whole compiler object exists, its actual ELF undefined GLOBAL symbols replace the static inventory. Each must resolve exactly once against the pinned pack/provider objects, and the linker must produce zero unresolved or duplicate strong definitions. Runtime source definition presence or a RuntimeRoutes row is not an admitted export receipt. Runtime symbol templates containing `{...}` are patterns awaiting concrete MIR, never literal export names.

## 5. Compiler FFI reachability

[SOURCE] `FFI-REACH.tsv` is a named-call static graph over the compiler source manifest; it lists full direct/transitive compiler-function names and paths. [INFERENCE] Indirect calls, interpolation expressions and entry reachability await checked MIR, so counts are not compiler entry closure counts.

| Foreign symbol | Direct named caller functions | Transitive named caller functions |
|---|---:|---:|
| `os_syscall` | 6 | 19 |
| `os_store_word` | 4 | 13 |
| `os_dl_symbol` | 1 | 7 |
| `os_call_address` | 4 | 4 |
| `os_load_byte` | 1 | 2 |
| `os_call_address_two` | 1 | 2 |
| `os_dl_open` | 1 | 2 |
| `os_load_word` | 1 | 1 |

[INFERENCE] Assembly bodies are Jet inline assembly, not Rust pack symbols. The baseline lowerer rejects Foreign functions (`Lower.jet:921–923`), so native asm support remains a source prerequisite until the composed native line proves those bodies. `os_dl_open`/`os_dl_symbol` are FFI(c) dynamic-loader calls; whether the final compiler entry reaches them and whether the selected runtime still needs a dynamic loader must be measured. Do not stub them out or quietly add a libc/libgcc dependency to a Jet-only pack.

## 6. SeedWork1 native-fixedpoint.sh contract

[SOURCE] SeedWork1 commit `5becf3bad`, `Tools/agent/native-fixedpoint.sh:73–105`, invokes the builder as `build <unit>`, requires a fresh executable at `<source-root>/.jet/build/<entry-stem>`, copies it into `out/jetc1`, then uses that real compiler for stages 2 and 3. Runtime lookup is adjacent `<builder-dir>/jet-runtime`; it exposes the SAME runtime directory beside all stage compilers. There is no script option for injecting manually linked per-package objects.

[INFERENCE] The proposed invocation, only after a real current native hosted builder and fixed runtime epoch exist, is:

```sh
Tools/agent/native-fixedpoint.sh \
  --builder "$H0" --unit "$COMPILER_UNIT" --out "$FRESH_FP_OUT" \
  --level o0 --stages 3
```

[INFERENCE] H0's `build` must perform the whole-unit native feed → lower → object → link sequence above and write a genuinely fresh executable at the script's expected path. Copying an already built binary into that location is not a stage-1 compiler build. `--release` is forbidden here because the inspected hosted CLI release path still uses Rust/LLVM. No O1/O2 proof is claimed by this O0 draft.

[SOURCE] The fixed-point script rejects reused receipt directories and checks freshness, build exit, size and SHA. Its synthetic harness receipts do not establish a real compiler chain. [INFERENCE] Before S11 is called green, the real jetc1 must compile the gate goldens with zero wrong results and produce jetc2/jetc3 with the native fixed-point criterion; byte identity alone is not semantic parity.

## 7. Exact prerequisites still missing

- [SOURCE] M1 real package counts have not yet landed: package measurement and refreshed r2/r3 precheck are detached; new heavy admission is paused for gate-2 fixed point. A running driver may continue, but a paused wrapper is not a failure or proof.
- [SOURCE] NativeInteg3 reported no admitted current-schema compiler lowering driver or Jet-front candidate; NativeFeed3 delivered the source codec contract only.
- [SOURCE] RtObject3 FINAL `generated.sha`/admitted pack receipt was absent at the integration report; no pack contents or Rust-free symbol census has been exercised here.
- [SOURCE] Whole-unit clean MIR/native object plus actual ELF undefined-symbol inventory is absent.
- [SOURCE] Separate linkable compiler-package object emission is not implemented by the inspected object path; measurement slices must not masquerade as such objects.

This draft closes the static recipe/documentation item only. It does not close S3, native jetc1 linking, RuntimeJet7's Rust-free pack, or S11.
