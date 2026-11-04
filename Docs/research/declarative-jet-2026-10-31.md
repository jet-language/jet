# Jet as a declarative platform: every domain, its gaps, and one model (2026-10-31)

Tower card: #6513. Owner order, 2026-10-31: "maximize the ability of Jet to be declarative rather than imperative … hardware as a declarative/declared library, FFI as a metaprogramming declarative library, etc. Investigate and propose every gap and enhancement."
Proposed ballots: `D-DECLARE-MODEL1`, `D-HW-DECLARE1`, `D-DB-SCHEMA1`, `D-UI-VIEW1` (GoldC lane, `declarative/ballots/`). Proposed cards: `declarative/cards/*.json` in the same lane.

Code is the source of truth. All file:line facts cite dev `68c87ae07b`. Tower decision states were read from the live board on 2026-10-31.
No program in this note was compiled. Every Jet example is a design sketch in the ratified forms `name :: fn(...)` (D-DECL-BINDING1=B), `fn(n) { … }` lambdas (D-LAMBDA-SHAPE1=C), `prep` (D-PREP-BRANCH1), `#target`/`#sites` (D-MARKER-REFLECT1, D-META-HASH1), and `#{…}` pastes. Each one is marked **not compiled**.
Code from other systems is quoted from their public documentation as remembered by the author. It was not re-fetched in this pass, so read it as illustrative.
Reasoning without a code citation is marked `[INFERENCE]`.

## Plain summary

A declarative system lets an author say *what* is true and lets the tool work out *how*. Nix modules, Kubernetes, SwiftUI, Prisma, Zephyr devicetree and Bazel all win the same way. One typed description feeds many derived outputs, and the tool can check the description before anything runs.

Jet already has most of the parts. These include typed values, `marker` declarations that derive code (Compiler/Prelude/Markers.jet declares 80 of them), `prep` evaluation, a fact sigil, typed codecs, and a ratified plan for package-owned lists (D-EXTEND-ARCH1). JetOS is the most declarative part: disks, containers, networking, theming and systemd units are all ratified as declarations (D-JOS-DISK1, D-JOS-CONTAINER1, D-JOS-NETWORK1, D-JOS-THEME1).

The gaps fall into three groups.

1. **Declarations that are not values.** The board is a record of `String` fields (`Jetpack/Provider/Batteries/embedded/embedded.jet:22-42`). `package.jet` is read by hand-written text scanners (`Jetpack/PackageModel/Source/Manifest/ManifestText.jet:73`, `Blocks/Policy.jet:174-176`), not by the evaluator that runs `prep`. Environment modules have a third evaluator (`Jetpack/EnvModel/Source/ModuleEval/`). Its package depends on the parser but on neither sema nor eval (`Jetpack/EnvModel/package.jet:4-10`).
2. **Lists the compiler keeps closed.** These include `Target`, `ABI`, `FFILanguage`, `Effect` and `Layout` (`Core/Compiler/Lang.jet:8-70, 161-168`), the second `ForeignLanguage` enum (`Jetpack/PackageModel/Source/Foreign/Bridge/BinderDescriptor.jet:22`) and the compiler's synthetic trait list (`Compiler/JetSema/Source/Sema/Calls/Methods.jet:333`). Core declares only two traits in Jet source (`Compiler/Prelude/core/prelude.jet:19`, `Core/Math/Math.jet:3590`).
3. **Domains where the user still writes the "how".** UI code calls `mount` inside a render effect and diffs by hand (`Tools/gauntlet/entries/web-app/jet/run.jet:9-21`, `Examples/features/ui/ui_view_tree.jet:10-27`). Database migrations are lists of SQL strings (`Core/Db/Db.jet:941`). Log levels are strings (`Core/Log/Log.jet:38-58`). Channels carry only `Int` (`Core/Tasks/Tasks.jet:17-141`). No metrics API exists in Core.

The proposal is one model built from existing mechanisms, with no new keyword. Call it **declare, derive, check, show**.

- **Declare.** A declaration is a typed Jet value, a `::` constant of a Core type evaluated by `prep`.
- **Derive.** What a declaration produces is a Core trait implemented for its type, run by a marker or a `use` root through the ratified binder pass (D-FFI-BINDER1, D-FFI-LANG1).
- **Check.** The compiler checks declarations as values, with ordinary type errors, `#Pre` on constructors and registered diagnostics. It reads them as facts (D-HOST-CAPS1, D-BUILD-FACT3).
- **Show.** One view shows the declaration, what it derived, and where every derived item came from (D-MARKER-EXPAND1, D-TARGET-AUDIT1).

The four proposed ballots ask the owner the four choices that change the public surface:

- the cross-cutting model;
- hardware as declared libraries;
- database schema from records;
- views as values.

The rest is non-gated work under ratified law, filed as cards.

## 1. Method

- **Inventory.** Prelude markers (`Compiler/Prelude/Markers.jet`), the compiler's closed menus (`Core/Compiler/Lang.jet`), every Core module that a domain touches, the Jetpack manifest readers, the embedded battery, and the diagnostics registry.
- **Decisions.** Ratified and open decisions were read from the Tower board on 2026-10-31. The FFI v3 design is `$JET_SCRATCH_ROOT/lanes/FFIDesign/v3/ffi-v3.md`. Two earlier notes were read: `Docs/research/2026-10-29-extensibility-every-layer.md` (closed lists) and `Docs/research/2026-10-30-project-package-config-today.md` (manifests).
- **Counts.** These come from `git grep` at `68c87ae07b`. The diagnostics registry has 874 rows with a `code:` field. 80 have a structured fix and 71 name a lint (`Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`).
- **No builds.** This is a research pass. Nothing was compiled or run.

## 2. What "declarative" must mean for Jet

A Jet declaration must meet these five tests, or it is only data in a file:

1. **Typed.** It is a value of a Jet type, so a typo or a wrong unit is an ordinary type error at the line that wrote it.
2. **One evaluator.** It is evaluated by the same `prep` evaluator as every other compile-time value (I8 one mechanism). Its meaning does not depend on which tool reads it (I9 one meaning).
3. **Derives.** Something is generated from it, through a trait the author can read. The derived code is ordinary Jet that `jet inspect expand` can print (D-MARKER-EXPAND1).
4. **Checked before it runs.** Its invariants are checked before code generation. Examples are overlap and overflow of memory regions (D-TARGET-MEMORY1) and route conflicts.
5. **Zero cost.** It costs nothing at run time beyond what the derived code does. A register write is one store. A route table is a static match. A schema check runs at build time.

The existing parts that meet the tests are listed here so that the proposals reuse them:

| Mechanism | Where | Meets |
|---|---|---|
| `marker Name(params, $sites…) { body }` deriving code | `Compiler/Prelude/Markers.jet:23-201`; D-MARKER-DECL1, D-MARKER-REFLECT1 | 1, 3 |
| `prep if` / `prep loop` | D-PREP-BRANCH1 | 2 |
| Module marker binders (`use c.x`) | D-FFI-BINDER1, D-FFI-LANG1 (not built: `FFILanguage` is closed at `Core/Compiler/Lang.jet:41`) | 3 |
| Fact sigil reads | D-BUILD-FACT2, D-BUILD-FACT3, D-META-ROOT3 | 4 |
| Typed hardware descriptions | D-HOST-CAPS1, D-JOS-HARDWARE1 | 1, 4 (not built for boards, see §3.1) |
| Typed codecs with field markers | `Markers.jet:78-129`; D-ONE-CODECS1 | 1, 3, 5 |
| One provider interface per service | D-CORE-PROVIDER1 | 3 |
| Every list is a Core trait | D-EXTEND-ARCH1 | 3 (not built: Core has 2 Jet traits) |
| Expansion view | D-MARKER-EXPAND1 | show |

## 3. Domain by domain

### 3.1 Hardware and targets

**Today.**
- *Declared.* Freestanding profiles are ratified as typed Jet profile modules (D-TARGET-SURFACE1). Memory is ratified as named regions with origin, size, access and kind, checked for overlap before codegen (D-TARGET-MEMORY1). The linker script is ratified as generated from profile facts (D-TARGET-LINKER1). `allocator` and `panic` are ratified as required facts (D-TARGET-ALLOC1), and `jet inspect dossier target` as the audit view (D-TARGET-AUDIT1). All five are in `Docs/spec/syntax-decisions.md:5970-5988`. Hardware descriptions per target are ratified as extensible by packages, with requirements (D-HOST-CAPS1=A). Chip instructions are ratified as library entries (D-CORE-BOUNDARY1=A).
- *Built.* The only board is `embedded.board()` in `Jetpack/Provider/Batteries/embedded/embedded.jet:64`. It returns a `BoardDescription` whose `kind`, `access`, `linker`, `allocator`, `panic` and `mmio` are all `String` (`:7-42`). Registers are `#Layout(c)` structs of `U32` (`:133-162`), and access to them is unchecked field assignment (`:188-229`). The one MMIO write goes through `#Unsafe` volatile helpers (`:179-185`, `Core/Mem/Mem.jet:38-43`). Interrupts have a marker, `#Interrupt(vector, effects)` (`Markers.jet:184`), and a sema rule (`Compiler/JetSema/Source/Sema/MarkerRules.jet:918`). No example or test uses it, and the battery says "D-FOUND-BOARD1 still has no vector-table/ISR registration seam" (`embedded.jet:255-257`).
- *Target triples.* The codegen family is a closed enum (`Core/Compiler/Lang.jet:161-168`). Package target profiles are three closed web identities plus `Named(String)` (`Jetpack/PackageModel/Source/Manifest/Blocks/Targets.jet:38-48`). CPU features and ABI are closed menus (`Lang.jet:8-15`).
- *Friction.* Nothing checks that a register is read-only, that a field fits its bit width, or that two peripherals do not overlap. A new board means copying a record of strings. The SVD file is a `String?` in the dossier (`embedded.jet:47-48`), not an input that generates anything.

**Best in the world.**

```text
// Rust svd2rust: an SVD file generates a typed peripheral access crate
let dp = stm32f4::Peripherals::take().unwrap();
dp.GPIOA.moder.modify(|_, w| w.moder5().output());   // field writer is typed; reserved bits kept
dp.GPIOA.odr.write(|w| w.odr5().set_bit());
```

```text
/* Zephyr devicetree: the board is data; drivers get macros generated from it */
&usart2 { status = "okay"; current-speed = <115200>; pinctrl-0 = <&usart2_tx_pa2 &usart2_rx_pa3>; };
const struct device *uart = DEVICE_DT_GET(DT_NODELABEL(usart2));   /* fails to build if disabled */
```

```text
// Chisel: hardware is a Scala value; the generator elaborates it to Verilog
class Blinky extends Module { val io = IO(new Bundle { val led = Output(Bool()) })
  val cnt = RegInit(0.U(24.W)); cnt := cnt + 1.U; io.led := cnt(23) }
```

Embedded-hal adds the other half. Drivers are generic over traits such as `OutputPin` and `SpiBus`, so one driver runs on every board that implements them.

**Gap and design.** Everything ratified is built from existing mechanisms. What is missing is the declared library shape, so ballot `D-HW-DECLARE1` proposes it.

- **A chip is a value.** `Chip`, `Peripheral`, `Register<Bits, Access>`, `Field`, `Interrupt`, `Clock` and `Pin` are Core types in `core.hw`. Their constructors carry `#Pre` checks for fit, overlap and alignment.
- **SVD and devicetree are binders.** `use chip :: svd.stm32f411` runs a Language package (D-FFI-LANG1) that reads the vendor SVD file. It emits the chip value plus typed register accessors. A devicetree reader is a second binder of the same shape.
- **A board is a value built from a chip.** It names the pins, clocks and enabled peripherals. It implements the Core trait that D-HOST-CAPS1 asks for (`HardwareDescription`), so it feeds the target profile, the linker regions, the dossier and requirements.
- **Drivers are generic over traits, as in embedded-hal.** `OutputPin`, `SerialWrite` and `Spi` are implemented by derived code for each board.
- **Interrupts are declared on the board.** `#Interrupt(chip.USART2)` attaches a handler. The derive pass builds the vector table from the board value, which closes the D-FOUND-BOARD1 seam.

```jet
// not compiled: simple. A board package declares one board from a vendor SVD.
use {
    chip :: svd.stm32f411 // binder: reads STM32F411.svd, emits `chip.*`
    core.hw
}

nucleo :: hw.board(chip.device, {
    clock: hw.Hz(100_000_000),
    led: chip.GPIOA.pin(5).output(),
    console: chip.USART2.serial(baud: 115_200, tx: chip.GPIOA.pin(2), rx: chip.GPIOA.pin(3)),
})

blink :: fn() -[hw.Mmio]> {
    loop { nucleo.led.toggle(); hw.delay_ms(500) }
}
```

```jet
// not compiled: complex. A hand-declared peripheral, a DMA-fed driver, and an ISR.
Uart :: hw.peripheral("UART", base: 0x4000_4000, {
    data:    hw.register<U32, .ReadWrite>(0x00, { byte: hw.bits(0..8) }),
    state:   hw.register<U32, .ReadOnly>(0x04, { tx_full: hw.bit(0), rx_full: hw.bit(1) }),
    control: hw.register<U32, .ReadWrite>(0x08, { tx_en: hw.bit(0), rx_en: hw.bit(1), rx_irq: hw.bit(3) }),
})

#Interrupt(board.uart.rx)                    // vector chosen from the board value
on_rx :: fn() -[hw.Mmio]> {
    byte :: board.uart.data.byte.read()
    rx_queue.push(byte)                      // the bounded queue from embedded.jet, now typed
}

// Error the compiler gives, from the declaration alone:
// board.uart.state.tx_full.write(true)
//   E3301: `state` is read-only on UART at 0x40004004 (declared at uart.jet:3)
```

**What the compiler checks.** It rejects writing a read-only register or field, and storing a value wider than its field. It rejects overlapping peripherals and regions (D-TARGET-MEMORY1), and a pin used twice. It rejects an interrupt vector that does not exist on the chip, an ISR whose effects are not in the board's allowed set, and a driver whose trait the board does not implement. It also rejects a Core API that the profile's allocator and panic facts forbid (D-TARGET-ALLOC1).

**Zero-cost lowering.** A field write lowers to read-modify-write volatile operations, with the mask and shift folded at build time. A constant write lowers to one store. The board value exists only at build time. `[INFERENCE]` This matches svd2rust, whose closures inline to the same instructions as C register macros.

**Composition.** One board value feeds:

- the target profile, whose triple and CPU features come from `chip.core`;
- the linker regions and the dossier;
- the D-HOST-CAPS1 facts, so `requires: [hw.fpu]` is checked;
- FFI facts, since a vendor C HAL binds against the same chip constants;
- the performance model, with clock, cache and flash wait states as facts for budgets (D-WD14).

### 3.2 FFI and language bindings

**Today.**
- *Ratified.* Each language is a Jet package implementing the Core trait `Language` (D-FFI-LANG1=A). Readers are module-marker packages (D-FFI-BINDER1=A). Each foreign row is proven, guarded, given a safe shape, or refused, with facts traced to sources (D-FFI-FACTS1=A). `jet audit foreign --json` lists rows and the lock pins contracts (D-FFI-AUDIT1=A). Constants fold under the ordinary rule (D-FFI-COMPTIME2=A). `#Export` names hosts (D-FFI-EXPORT3=A).
- *Built.* The language set is closed twice: `enum FFILanguage { C, Cpp, Asm }` (`Core/Compiler/Lang.jet:41-45`) and `enum ForeignLanguage` (`Jetpack/PackageModel/Source/Foreign/Bridge/BinderDescriptor.jet:22`). There is no `trait Language` in any `.jet` file at this commit. A grep for `trait +Language` finds nothing.
- *Still imperative in a binder* (`ffi-v3.md` §4.2, lines 206-265). A binder author writes:
  - the type map as an `if` chain (`scalar`, :234-242);
  - the per-row loop, the refuse-and-stub branch, and the emission template (:252-263);
  - the tool invocation and its argument list (:248-249);
  - the evidence construction (:251).

  Every binder repeats this skeleton, and only the type map and the refusal rules differ by language. `[INFERENCE]` from §4.3-4.5, where the C, Python and Odin binders repeat the same loop.

**Best in the world.**

```text
# SWIG / PyO3 / wasm-bindgen: a declaration list, the tool writes the glue
#[wasm_bindgen] extern "C" { fn alert(s: &str); }          // wasm-bindgen
%module example %{ #include "example.h" %} %include "example.h"   // SWIG
```

```text
// Zig: translate-c is a library function the build calls; no binder language
const c = @cImport({ @cInclude("math.h"); });
```

The best systems make the *mapping* declarative. Examples are SWIG typemaps, cbindgen's `cbindgen.toml` rename and type rules, and bindgen's allowlist and blocklist. The *pass* stays a fixed engine.

**Gap and design.** This needs no new mechanism. A Core binder kit, `ffi.Binder`, takes three declared tables and derives the loop:

- a type map, as a value of `[ffi.CType: ffi.JetShape]`;
- refusal rules, as a list of `(match, Diagnostic)`;
- naming rules (D-WIRE-CASE1).

A binder that needs more keeps writing `expand` by hand. The simple path becomes data. The design stays inside D-FFI-LANG1, because the kit is an ordinary library over `Language`.

```jet
// not compiled: simple. The libm binder from ffi-v3 §4.2 as declared tables.
use core.ffi

CMath :: struct { header: String }

impl CMath.Language {
    root :: "cmath"
    expand :: ffi.binder(
        reader: ffi.clang("19.1.7").prototypes,
        types: { "int": I32, "long": I64, "float": F32, "double": F64 },
        refuse: [ffi.refuse(.AnyOther, code: "E3240", fix: "bind with the full c binder: m: c@system")],
    )
}
```

```jet
// not compiled: complex. A handle type with an owner rule and an error convention.
sqlite :: ffi.binder(
    reader: ffi.clang("19.1.7").prototypes,
    types: ffi.c_defaults,
    handles: { "sqlite3*": ffi.handle(close: "sqlite3_close_v2") },  // derives Drop + #SingleUse
    errors: { "int": ffi.status(ok: 0, message: "sqlite3_errmsg") },  // derives `T SqliteError!`
    names: .Snake,
)
```

**What the compiler checks.** That every reader row maps to a shape or a registered refusal (the D-FFI-FACTS1 decision per row), that a handle has exactly one close, and that the type map is total over the rows read. The derived module is visible through D-MARKER-EXPAND1 and `jet audit foreign`.

**Zero-cost lowering.** The derived wrapper is the same `ffi.extern(...).call(...)` as the hand-written binder, so lowering is unchanged (`ffi-v3.md` §9).

**Cards.** One card first closes the two `FFILanguage` and `ForeignLanguage` enums behind `trait Language`, which builds D-FFI-LANG1. A second card adds the `ffi.binder` kit.

### 3.3 Build, packages, environments and systems

**Today.**
- *Declared surface.* `package.jet` is a record-shaped file: `name:`, `version:`, `authority: { allow: [IO, Mem.Alloc] }` (`Examples/features/tooling/compiler_api_package/package.jet:1-9`). Build, Dev and Test are ordinary Core types (D-BUILD-CORE1=A), and one `b.add` call adds any output (D-BUILD-PRIM1=A). One table and one order govern every setting (D-SETTING-ORDER1=A). Environments as modules from a dev shell to JetOS is open (D-DEV-ENV1, open). JetOS is declarative across disks, containers, networking, theming, `/etc`, Flatpak and hardware (D-JOS-* rows above).
- *Imperative reading.* The manifest is not evaluated as a Jet value. Blocks are parsed from strings by `pkg_parse_*` functions: Policy.jet 11, Authority.jet 11, PackageParse.jet 10, BuildProfiles.jet 8, Targets.jet 5, Dependencies.jet 5 and Allocator.jet 4 (`git grep -c "fn pkg_parse_"`). They use helpers such as `pkg_strip_wrapped(value, ".Allow(", ")")` (`ManifestText.jet:73-78`, used at `Blocks/Policy.jet:175`). Environment modules use a third reader, `Jetpack/EnvModel/Source/ModuleEval/`, whose package depends on `jet_parser` but on neither `jet_sema` nor an evaluator (`Jetpack/EnvModel/package.jet:4-10`).
- *Friction.* A typo inside `authority:` is a parse error from a hand-written scanner, not a type error. A computed value cannot be written there unless the scanner learns it. The manifest's fields are re-declared in Jetpack structs such as `PackagePolicySurface` (`Policy.jet:101-110`) and documented separately (`Docs/research/2026-10-30-project-package-config-today.md` §2). That breaks D-ONCE-LAW1's "one home per truth".

**Best in the world.**

```text
# NixOS module: options are typed; config is merged; one evaluator for all
{ config, lib, ... }: {
  options.services.foo.port = lib.mkOption { type = lib.types.port; default = 8080; };
  config = lib.mkIf config.services.foo.enable { networking.firewall.allowedTCPPorts = [ config.services.foo.port ]; };
}
```

```text
# Bazel/Starlark: rules are functions; BUILD files are calls; analysis is hermetic
cc_binary(name = "app", srcs = ["main.cc"], deps = ["//lib:core"])
```

```text
# Cargo: one TOML table; features are declared; resolution is derived
[features] default = ["tls"]  tls = ["dep:rustls"]
```

**Gap and design.** There is one gap and one fix. The manifest, the project file, env modules, board files and system files should be read by one evaluator into typed Core values, the `prep` evaluator. The record types (`Package`, `Project`, `Env`, `System`) are declared once in Core, and Jetpack reads them as typed values instead of scanning text. The record shape that users write does not change, so this is non-gated work under D-BUILD-CORE1 and D-SETTING-ORDER1 (card). Env modules then share the merge rule that D-DEV-ENV1 proposes, rather than a separate evaluator.

```jet
// not compiled: simple. Unchanged surface; now a typed Core `Package` value.
name: "shop"
version: "1.2.0"
authority: { allow: [Net, DB] }
deps: { web: "web@2" }
```

```jet
// not compiled: complex. A computed field and a module, checked as Jet.
use core.build

name: "fleet"
version: build.git_describe()                 // ordinary prep call, typed String
targets: [build.target(boards.nucleo)]        // a hardware value from §3.1 feeds the target
dev: env.module {
    services: [env.postgres(version: "16", port: 5432)]
    tools: [env.tool("protoc", "27.1")]
}
// error (from the type, at this line): `prot: 5432` is not a field of Postgres; did you mean `port`?
```

**What the compiler checks.** Field names and types are checked as on any value. The checker also enforces the D-SETTING-ORDER1 order, refuses a pin conflict, and takes authority ceilings from the same `Effect` values code uses.

**Zero-cost lowering.** This is build time only. Hashing the evaluated value gives cache keys (D-BUILD-NOCHANGE1).

### 3.4 Data and schemas

**Today.**
- *Codecs.* These are declared by markers on types: `#Codable`, `#Encode`, `#Decode`, `#PublishedSchema`, `#RenameAll`, `#DenyUnknownFields`, `#Discriminant`, `#Untagged`, `#Redact`, `#Rename(json:, cbor:, csv:, toml:, yaml:, xml:)`, `#Skip`, `#Flatten` (`Markers.jet:78-129`). There is one codec contract (D-ONE-CODECS1=A) and one naming rule (D-WIRE-CASE1=A).
- *CLI.* `#CLI` on a type, with `#Flag`, `#Short`, `#Env` and `#Doc` on fields (`Markers.jet:101, 131-137`). Card `c0dp5efn` plans one option table for CLI flags and build config.
- *HTTP.* Route markers `#Get(path)` … `#Options(path)` are declared by Core itself (`Core/Http/Server.jet:24-30`). One route table on `web.app()` (D-ROUTE-ONE1=A). Page files become addresses (D-ROUTE-FILES2=A).
- *Config.* `jet` exports a typed config value as JSON, YAML or TOML (D-CONFIG-EXPORT1=A).
- *Storage.* `#SQL(row: Type)` names a row type on a block (`Markers.jet:192`; `Examples/features/tooling/dsl_blocks.jet:8-11`). Queries return untyped rows, `[[String: DBValue]]` (`Core/Db/Db.jet:732`), read through `row_int(row, "column")` (`:138`). Migrations are imperative: `migrate(scope, name, steps: [SQL])` (`:941`). Its ledger table is a SQL string literal (`:771-772`).
- *Friction.* A record and its table are declared twice, once as a struct and once as SQL text. A column rename is a hand-written step. No check ties a query's columns to the record it is decoded into. The traits behind the codec markers are compiler-synthetic (`Compiler/JetSema/Source/Sema/Calls/Methods.jet:333`; `Compiler/JetFoundation/Source/Facts/Checked.jet:20`), so a package cannot add a format the way Core does. D-EXTEND-ARCH1 is ratified but not built.

**Best in the world.**

```text
// Prisma: the schema is the source; client types and migrations are derived
model User { id Int @id @default(autoincrement())  email String @unique  posts Post[] }
$ prisma migrate dev --name add_posts   # diff → SQL migration → reviewed file
```

```text
// protobuf / OpenAPI: one schema → many languages, with evolution rules
message User { int64 id = 1; string email = 2; reserved 3; }
```

```text
// serde: field attributes; the derive writes the codec
#[derive(Serialize, Deserialize)] #[serde(rename_all = "camelCase")] struct User { id: u64, #[serde(default)] tags: Vec<String> }
```

**Gap and design.** Two things are needed.

1. **Schema from records.** Ballot `D-DB-SCHEMA1` proposes `#Table` on a record, with `#Key`, `#Unique` and `#Index` on its fields, all declared as Core markers in `core.db`. The table is derived from the record. A migration is derived by diffing the stored schema hash against the record and written as a reviewed file. A rename is declared with `#Rename(from:)` (an existing marker, `Markers.jet:125`). Typed queries return `[User]`, checked against the table at build time.
2. **Codec traits in Jet source.** `Encode`, `Decode`, `Equatable` and `Comparable` should be declared as Core traits, so a format package adds a codec the way Core does (D-EXTEND-ARCH1; card).

```jet
// not compiled: simple. One record is the table, the codec and the row type.
use core.db

#Table #Codable
User :: struct {
    #Key id: Int
    #Unique email: String
    name: String
}

admins :: fn(scope: db.Scope) -[DB]> [User] {
    scope.select(User, where: fn(u) { u.email.ends_with("@shop.dev") })
}
```

```jet
// not compiled: complex. A rename and a new column; the migration is derived and reviewed.
#Table
User :: struct {
    #Key id: Int
    #Unique #Rename(from: "mail") email: String     // was `mail`
    created: time.Instant{time.epoch}               // new column with a default
}
// $ jet db plan
// migration 0004_user (derived from User at user.jet:2)
//   ALTER TABLE user RENAME COLUMN mail TO email;
//   ALTER TABLE user ADD COLUMN created INTEGER NOT NULL DEFAULT 0;
// written to migrations/0004_user.sql (review, then `jet db apply`)
```

**What the compiler checks.** A query against a field the record lacks fails. So does a `#Key` that is missing or doubled. A type change with no conversion is refused until a declared step exists. A web handler that reads every row fails through the existing `DB.AllRows` effect (D-SQL-HANDLER1=A).

**Zero-cost lowering.** Queries lower to prepared statements, with SQL text built at build time. Decoding is by column index, not by a name lookup into a map. That removes today's `[String: DBValue]` per row.

### 3.5 UI, graphics and GUI

**Today.**
- The node tree, windows and previews move to the first-party `ui` package (D-UI-HOME1=A). There is one canonical node tree (D-UITREE1, cited in `Examples/features/ui/ui_view_tree.jet:6-7`). Backends are `gtk_backend`, `tui_backend` and `null_backend` (`Core/Ui/Ui.jet:177-187`).
- *Imperative.* A program calls `ui.mount(backend, tree, constraint)` inside a `reactive_render` effect (`Tools/gauntlet/entries/web-app/jet/run.jet:9-21`). The view-tree example writes its own repaint diff with a hand loop over leaves (`Examples/features/ui/ui_view_tree.jet:10-27`). The web DOM host is not wired. `--target web` ships a Wasm host runtime with no DOM creation, while the old `#jet-app` mount lives in `Compiler/Prelude/Web/DomRuntime.js:451-454`, which the web build does not emit (observed in the GoldC-60 gauntlet sweep).
- *Friction.* The author must know when to mount and how to diff. The same view cannot be previewed, tested and hot-reloaded as one value.

**Best in the world.**

```text
// SwiftUI: body is a value; state changes re-evaluate it; the framework diffs
struct Counter: View { @State var n = 0
  var body: some View { VStack { Text("count: \(n)"); Button("increment") { n += 1 } } } }
```

```text
-- Elm: view is a pure function of the model; update is a pure function of messages
view model = div [] [ text ("count: " ++ String.fromInt model), button [ onClick Increment ] [ text "increment" ] ]
```

Compose and QML follow the same rule. The view is a function of state. The runtime owns mounting, diffing and scheduling.

**Gap and design.** Ballot `D-UI-VIEW1` proposes a view as a function from state to a node-tree value. `ui.app(view)` owns mount, diff and repaint on every backend, including web. `ui.mount` and hand diffs become backend internals.

```jet
// not compiled: simple. The gauntlet web-app counter as a view value.
use ui

counter :: fn(clicks: &ui.State<Int>) -> ui.Node {
    ui.column([
        ui.text("count: {clicks}"),
        ui.button("increment", fn() { clicks += 1 }),
        ui.button("reset", fn() { clicks = 0 }),
    ])
}

run :: fn() { ui.app(counter, state: 0) }     // native, terminal or web from the target
```

```jet
// not compiled: complex. Derived list identity, a style value, and an animation as data.
Todo :: struct { id: Int, title: String, done: Bool }

todos :: fn(items: &ui.State<[Todo]>) -> ui.Node {
    ui.list(items, key: fn(t) { t.id }, row: fn(t) {
        ui.row([ui.check(t.done), ui.text(t.title).style(t.done ? theme.muted : theme.body)])
            .animate(.opacity, ui.spring(stiffness: 300))
    })
}
// error: `key:` is required when rows are reordered; E-number from the registry
```

**What the compiler checks.** It checks that a view is pure, with no effects except through `ui.State` (an existing effect check). It checks that list keys are stable and unique by type, and that accessibility roles are present (`UIAriaRole`, `Core/Ui/Ui.jet:37`). It also checks that a node is valid for the selected backend.

**Zero-cost lowering.** `[INFERENCE]` Static subtrees are hoisted at build time. Only nodes that read changed state are re-evaluated, from the signal graph that `core.reactive` already builds. Web lowers to DOM operations, native to retained widgets.

### 3.6 Concurrency and effects

**Today.**
- *Declared.* `#Job`, `#Every(schedule)` (`Markers.jet:50-52`; `Examples/features/devloop/schedule_every.jet:2-6`). Service workers whose parameter type is the message (D-SERVICE-WORKER1=A), with replies, kept state and batching (D-SERVICE-REPLY1, D-SERVICE-KEEP1, D-SERVICE-BATCH1). Game systems run in parallel from their declared data access (D-SYSSCHED1=A). Deadlines are blocks (D-DEADLINE-ONE1=A). Effects such as `DB` and `Log` are declared over machine powers (D-EFFTYPE-KERNEL1=A). State machines use the markers `#State` and `#Transition` (`Markers.jet:58-60`). The v3 design, enums whose methods are moves, is open (D-MACHINE1).
- *Imperative.* Task channels are `Receiver<Int>` only (`Core/Tasks/Tasks.jet:17-141`), so a pipeline of typed stages must encode values as integers or use services. There is no declared pipeline or dataflow form. Supervision exists, but as a setter: `set_restart(tree, restart)` refuses changes after start (`Core/Service/Service.jet:261-264`), and the strategy is tree-wide (`OneForOne`, `OneForAll`, `RestForOne`, `:467-471`), with no restart intensity.

**Best in the world.**

```text
% Erlang/OTP: supervision is a declared child spec; the runtime restarts
init(_) -> {ok, {#{strategy => one_for_one, intensity => 5, period => 10},
                 [#{id => db, start => {db, start_link, []}, restart => permanent}]}}.
```

```text
// Bevy: systems declare their data access; the scheduler derives parallelism
app.add_systems(Update, (move_players, apply_gravity).chain());
fn apply_gravity(mut q: Query<&mut Velocity, With<Body>>) { … }
```

```text
// Kotlin Flow / Rx: a pipeline is a value; the operators are the plan
flow.map { parse(it) }.filter { it.valid }.buffer(64).collect { save(it) }
```

**Gap and design.**
- *Typed channels.* This is a card, not gated: `Receiver<T>`, which existing generics already express.
- *Pipelines as values.* `tasks.pipeline(source).map(f).batch(64).sink(g)` is a value. Its stages, buffers and parallelism are data, and `run` derives the task graph. The same stage-access rule as D-SYSSCHED1 lets the scheduler run stages in parallel when they share no writes. This is a card under D-API-COMPOSE1, with no new syntax.
- *Supervision as a declared field.* `restart: .OneForOne(max: 5, within: 10s)` on the tree or worker line replaces the `set_restart` setter. It adds the intensity that OTP declares, and it is a field, not new syntax. This is a card.
- *State machines.* Defer to D-MACHINE1, which already proposes the declared form.

```jet
// not compiled: simple. A typed pipeline; parallelism derived from stage purity.
lines :: files.lines("orders.csv")
totals :: tasks.pipeline(lines)
    .map(fn(line) { csv.decode<Order>(line) })      // pure: may run in parallel
    .filter(fn(o) { o.amount > 0 })
    .sink(fn(o) { ledger.add(o) })                  // writes `ledger`: runs on one task
```

```jet
// not compiled: complex. A supervised worker with a deadline and a batch.
mailer :: tree.worker(send_batch, batch: 50, wait: flush, restart: .OneForOne(max: 5, within: 10s))

#Deadline(2s)
send_batch :: fn(mail: [Email]) -[Net]> { smtp.send_all(mail) }
```

**What the compiler checks.** A stage that writes shared state is not run in parallel. A pipeline's element types must line up from stage to stage. A restart policy on a worker whose state cannot be rebuilt is refused.

**Zero-cost lowering.** A pipeline of pure stages fuses into one loop when it has no `batch` or `buffer` stage. `[INFERENCE]` This is the same fusion `map` and `filter` already get on lists.

### 3.7 Performance and representation

**Today.**
- *Declared.* `#Layout(kind)` offers only `C` and `Columnar` (`Core/Compiler/Lang.jet:67-70`). Packed structs, overlapping fields and relative pointers are ratified (D-JAI-LAYOUT1=A), but `#Layout(packed)` and a first-argument `align` are reserved diagnostics (`DiagnosticRows.jet:9561-9563`). There are `#Inline(.Hint|.Always|.Never)` (`Markers.jet:41`), `#Kernel(.Parallel)` (`:39`), `#Scalar` (`:46`) and `#Memo` (`:31`). Sizes are language values (D-LAYOUT-SHOW1=B). Typed row handles exist for column tables (D-COLUMNS1=A).
- *Friction.* Hints are requests, not facts the compiler proves. Nothing declares "this loop must vectorize" or "this function must not allocate" and fails the build when it does not.

**Best in the world.**

```text
// Zig: comptime layout and explicit allocators are declarations the type carries
const Packet = packed struct { kind: u4, len: u12, crc: u16 };
```

```text
// Jai: SOA as a declared representation; the code that uses it is unchanged
Particle :: struct SOA { pos: Vector3; vel: Vector3; }
```

```text
// Rust: #[repr(C, packed)], #[inline(always)], #[target_feature(enable = "avx2")]
```

**Gap and design.** The model is to declare the property and let the compiler prove it or refuse. These are non-gated cards under D-JAI-LAYOUT1 and the ceiling work in #6502.

- Build `#Layout(packed)` and first-argument `align`, which D-JAI-LAYOUT1 already ratified.
- Add proven properties to `#Policy` (`Markers.jet:23`): `#Policy(no_alloc)`, `#Policy(vectorize)` and `#Policy(no_bounds_checks)`. Each fails with a registered diagnostic that names the line that broke the promise. `#Policy` already has `.Tighten` resolution and package, module, function and block sites, so the promise inherits down the tree.

```jet
// not compiled: simple.
#Layout(packed)
Header :: struct { kind: U4, length: U12, crc: U16 }   // 4 bytes; `Header.size == 4` is a fact
```

```jet
// not compiled: complex. A promise the build checks.
#Policy(no_alloc, vectorize)
dot :: fn(a: [F32], b: [F32]) -> F32 {
    total := 0.0
    loop i in 0..<a.len() -> total += a[i] * b[i]
    total
}
// error E… : `dot` promised `vectorize`; the loop at dot.jet:4 did not vectorize: `total` is a
//   floating-point reduction; add #Arithmetic(.Reassociate) or drop the promise
```

**What the compiler checks.** Each promise is checked against the optimized IR, with the evidence on the line that broke it. Layout facts feed D-LAYOUT-SHOW1.

**Zero-cost lowering.** A promise adds no code. It only makes the build fail when the optimizer did not reach the stated form.

### 3.8 Safety, security and policy

**Today.**
- *Declared.* Package authority is `authority: { allow: [...] }` (`package.jet:9`). `#Policy(settings…)` uses `.Tighten` resolution across sites (`Markers.jet:23`). `#Unsafe(reason)` is the only escape (`Markers.jet:27`). Method calls count toward rights (D-AUTH-METHOD1=A). Foreign code is audited (D-AUDIT-FOREIGN1=A). Licenses and source maps live in the policy block (`Jetpack/PackageModel/Source/Manifest/Blocks/Policy.jet:101-110`). JetOS runtime provenance is ratified (D-JOS-PROVENANCE1=A).
- *Imperative or closed.* The effect roots are a closed enum (`Lang.jet:25-38`), while D-EFFTYPE-KERNEL1 ratifies Core declaring `DB`, `Log` and `Secret` as library effects. Licenses are `[String]?`, parsed from text (`Policy.jet:174-176`), not typed SPDX values. There is no declared data-classification policy that flows through code. `PII`, `Secret` and `Credential` exist as a closed enum (`Lang.jet:155-158`) without a declared flow rule.

**Best in the world.**

```text
# Deno: permissions are declared at launch; the runtime denies the rest
deno run --allow-net=api.example.com --allow-read=./data app.ts
```

```text
# Cargo-deny: license and source policy as data, checked in CI
[licenses] allow = ["MIT", "Apache-2.0"]  [bans] deny = [{ name = "openssl" }]
```

```text
// Rust's capability crates and WASI: authority is a value you are passed
let dir = cap_std::fs::Dir::open_ambient_dir("data", ambient_authority())?;
```

**Gap and design.** These are cards, with no gate.

- Typed `License` values, with SPDX expressions as an enum plus `Custom(String)` per D-EXTEND-ARCH1.
- Declared classification flow: `#Redact` (`Markers.jet:123`) exists on fields. A `#Policy(classified: .PII, sinks: [log.redacted])` setting makes the compiler track the class through assignments and refuse an unredacted sink. `[INFERENCE]` This is the same flow analysis `#Scrub(tag)` (`Markers.jet:29`) implies.

```jet
// not compiled.
policy: { licenses: [.MIT, .Apache2], deny: ["openssl"], classified: { .PII: [log.redacted, db] } }
```

**What the compiler checks.** It refuses a dependency whose license is not allowed, and a PII value that reaches `print`, `log.info` or `http.send` without redaction.

### 3.9 Testing and verification

**Today.**
- *Declared.* `#Test(name, faults, expected_fail)` (`Markers.jet:180`). `#Pre` and `#Post` contracts (`:33-35`). `jet test` mutates changed code and runs guided fuzzing (D-TEST-MUTFUZZ1=A). Publishing runs every doc example (D-REGISTRY-EXAMPLE1=A).
- *Imperative.* Snapshot and golden tests are function calls inside a test body, `testing.snap(name, value)` and `testing.golden(name, value)` (`Core/Testing/Testing.jet:102, 136`). Property testing is a library call with a strategy record (`histories<T>`, `:532`, and `HistoryStrategy<T>`, `:517`). No marker declares a property, a fuzz target or a benchmark, and there is no `#Bench` marker in `Markers.jet`.

**Best in the world.**

```text
# Hypothesis: a property is a decorated function; generation and shrinking are derived
@given(st.lists(st.integers()))
def test_sort_idempotent(xs): assert sorted(sorted(xs)) == sorted(xs)
```

```text
// Rust: #[test], cargo-fuzz `fuzz_target!`, criterion `criterion_group!`, insta `assert_snapshot!`
fuzz_target!(|data: &[u8]| { let _ = parse(data); });
```

**Gap and design.** These are cards. `#Test` gains typed parameters, which makes it a property, with generation derived from `Decode`-like shape traits (`derive T.Arbitrary` through F6 in the extensibility note). `#Test(bench: budget)` declares a benchmark whose budget is a D-WD14 gate. `#Test(fuzz)` on a `[U8]` parameter declares a fuzz target. All three extend one existing marker, so no new marker is needed.

```jet
// not compiled.
#Test("sort is idempotent")
sort_twice :: fn(xs: [Int]) { assert(xs.sorted().sorted() == xs.sorted()) }   // xs is generated

#Test("parse never stops", fuzz: true)
parse_any :: fn(bytes: [U8]) { _ = csv.parse(bytes) }

#Test("decode under budget", bench: perf.budget(p95: 2ms))
decode_big :: fn() { _ = json.decode<Order>(fixture("orders.json")) }
```

**What the compiler checks.** That a property's parameter types have a generator, that a budget's unit is a duration, and that `#Pre` on a function under test filters the generated inputs.

### 3.10 Observability and operations

**Today.**
- *Declared.* Log spans close at block end (D-LOG-SPAN-SCOPE1=A). The level follows the setting order (D-LOG-LADDER1=A). Local services are listed in `package.jet` (D-DEV-SERVICES1=A). Infrastructure is typed Jet backed by Terraform providers (D-INFRA1=A). JetOS composes systemd units (frozen card `c0a11ldt`), and `ServicePlan` exists in `Jetpack/EnvModel/Source/ModuleEval/Plans.jet:282`.
- *Imperative.* Log levels are strings passed to `emit` (`Core/Log/Log.jet:38-58`), so a misspelled level is not a type error. No metrics API exists. A grep of Core for `counter`, `gauge` and `histogram` finds only `Collections.Counter` (`Core/Collections/Collections.jet:197-205`) and a sketch (`Core/Data/Sketch/Cms.jet:46`). There is no declared trace or metric that an exporter derives from.

**Best in the world.**

```text
// OpenTelemetry: instruments are declared once; exporters are configuration
meter.CreateCounter[long]("orders.placed", unit: "{order}", description: "Orders placed");
```

```text
# Kubernetes / compose: a service, its probes and its resources are one declared value
livenessProbe: { httpGet: { path: /healthz, port: 8080 }, periodSeconds: 10 }
```

**Gap and design.** These are cards, with no gate.

- `log.Level` becomes an enum.
- `core.metrics` declares instruments as `::` constants: `orders :: metrics.counter("orders.placed", unit: .Count)`. The exporter is a provider (D-CORE-PROVIDER1).
- A deployable service is one value: entry, probes, resources and replicas. It feeds the dev runner (D-DEV-SERVICES1), JetOS systemd units, and infrastructure (D-INFRA1), with no second description.

```jet
// not compiled.
placed :: metrics.counter("orders.placed", unit: .Count)
api :: ops.service(run, port: 8080, health: "/healthz", memory: 256.MiB, replicas: 3)
```

### 3.11 Documentation and tooling

**Today.**
- *Data-driven.* Diagnostics are rows: 874 in `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`, with `meaning`, `what`, `why`, `fix`, an optional `structured_fix` (80 rows) and `lint_name` (71 rows). The syntax dictionary is data (`Compiler/JetCli/Source/Cli/SyntaxDictionaryData.jet`), as are retired spellings (`Compiler/JetFoundation/Data/Syntax/Spellings.jet`). Markers are declared in Jet (`Markers.jet`).
- *Hand-coded copies.* Marker names are also matched by hand in the parser (`Compiler/JetParser/Source/Parser/Items.jet:1593`, census CL01091) and in sema (`MarkerRules.jet:918` matches `"Interrupt"` by string). Marker names are re-declared as string constants (`Compiler/JetFoundation/Data/Syntax/CoreSurface.jet:160`, `Markers.jet:63`). Only 80 of 874 diagnostics carry a structured fix, so `jet fix` rules are mostly prose. A package cannot register a diagnostic code; it must borrow a Core one (extensibility note §4.15).

**Best in the world.**

```text
// Clippy: a lint is a declared value; the driver registers and documents it
declare_clippy_lint! { pub NEEDLESS_RETURN, style, "using a return statement like `return expr;` where an expression would suffice" }
```

```text
# Semgrep / ast-grep: a rewrite rule is data (pattern → fix), runnable by the tool
rules: [{ id: no-unwrap, pattern: "$X.unwrap()", fix: "$X?", languages: [rust] }]
```

**Gap and design.** These are cards. `impl T.Diagnostic` with `code`, `what`, `why` and `fix` is already the FFI v3 form for package diagnostics (`ffi-v3.md:227-232`). Make it the one form for package diagnostics and lints, with registry rows generated from it (D-OPENTABLE1). Add declared rewrite rules, so a `structured_fix` is a pattern-to-template value checked against the registry. Replace the hand switches on marker names with reads of the declared `marker` (D-MARKER-REFLECT1).

```jet
// not compiled.
NoPrintInLib :: struct { at: Span }
impl NoPrintInLib.Lint {
    code :: "L2101"
    group :: .Api
    find :: fn(call: syntax.Call) -> Bool { call.callee == "print" && call.package.kind == .Library }
    fix :: syntax.rewrite("print($x)", "log.info($x)")
}
```

## 4. Gap matrix

Priority: P1 blocks a ratified decision or a gauntlet cell. P2 is a large declarative win. P3 is polish. Cost: S is under a week of lane work, M is one to three weeks, L is more than three weeks. `[INFERENCE]` The costs are estimates from file counts only.

| Domain | Today | Gap | Proposal | Gate | Priority | Cost |
|---|---|---|---|---|---|---|
| Hardware | Target profile law ratified; board is a string record; registers untyped; no ISR seam | Chip, register, pin, interrupt as typed values; SVD/devicetree binders | `core.hw` values + SVD binder + derived vector table | D-HW-DECLARE1 | P1 | L |
| Targets | `Target`, `ABI`, CPU features closed enums | Triples and features as data | Target profiles as `HardwareDescription` values (D-HOST-CAPS1) | none | P2 | M |
| FFI | `Language` trait ratified, not built; two closed language enums | Binder skeleton repeated per language | Build `trait Language`; `ffi.binder` kit of declared tables | none | P1 | M |
| Build/packages | Record surface; text-scanned by 54 `pkg_parse_*` functions | One evaluator; typed Core records | Read manifests through `prep` into Core `Package`/`Project`/`Env` values | none | P1 | L |
| Environments | D-DEV-ENV1 open; separate ModuleEval | Same evaluator and merge | Follows D-DEV-ENV1 once ratified | D-DEV-ENV1 | P2 | M |
| Codecs | Markers declared; traits compiler-synthetic | Package formats cannot join | Declare `Encode`/`Decode`/… as Core traits | none | P2 | M |
| Storage | Untyped rows; SQL-string migrations | Record is the table; derived migrations | `#Table`, typed `select`, `jet db plan` | D-DB-SCHEMA1 | P2 | L |
| HTTP/CLI | Route and CLI markers declared | One option table (card c0dp5efn) | Keep; finish c0dp5efn | none | P3 | S |
| UI | `mount` inside effects; hand diffs; web host not wired | View as value; runtime owns mount/diff | `ui.app(view, state:)` on every backend | D-UI-VIEW1 | P1 | L |
| Concurrency | `Receiver<Int>` only; no pipeline value; restart is a setter without intensity | Typed channels; pipelines; declared restart policy | `Receiver<T>`; `tasks.pipeline`; `restart:` field | none (D-MACHINE1 open) | P2 | M |
| Performance | `#Layout` C/Columnar; packed reserved; hints only | Promises the compiler proves | Build packed/align; `#Policy(no_alloc, vectorize)` | none | P2 | M |
| Safety/policy | Closed effect enum; string licenses | Library effects; typed licenses; data flow | Build D-EFFTYPE-KERNEL1; SPDX values; classified flow | none | P2 | M |
| Testing | `#Test`, `#Pre/#Post`; snap/golden/property as calls | Property, fuzz, bench declared | `#Test` typed params, `fuzz:`, `bench:` | none | P2 | M |
| Observability | String log levels; no metrics | Declared instruments; one service value | `log.Level`; `core.metrics`; `ops.service` | none | P2 | M |
| Tooling | 874 diagnostic rows; hand marker switches; 80 structured fixes | Package diagnostics, lints, rewrite rules as values | `impl T.Diagnostic`/`T.Lint`; `syntax.rewrite` | none | P2 | M |
| Cross-cutting | Four readers of declared data | One model | Declare, derive, check, show | D-DECLARE-MODEL1 | P1 | M |

## 5. The cross-cutting model: declare, derive, check, show

### 5.1 The four parts, all existing mechanisms

1. **Declare.** A declaration is a `::` constant of a Core record or enum type. Evaluation is by `prep`, the one compile-time evaluator. Files whose whole content is a declaration, such as `package.jet`, `project.jet`, env modules, board files and system files, are a record literal of their Core type. The same evaluator reads them.
2. **Derive.** A Core trait names what a declaration produces, such as `HardwareDescription`, `Language`, `Table`, `View`, `Lint` and `Diagnostic`. A marker on the declaration, or a `use` root for binders, runs the derive. It goes through the checked expansion pass that markers already use (D-FFI-BINDER1). Derived code is ordinary Jet.
3. **Check.** Constructors carry `#Pre`. Cross-declaration rules, such as overlap and coverage, run as `prep` code that returns registered diagnostics. The compiler reads declared values as facts through the fact sigil (D-BUILD-FACT3), so sema can reject a Core API the board does not allow (D-TARGET-ALLOC1).
4. **Show.** `jet inspect expand` prints every derived item with the declaration and line it came from (D-MARKER-EXPAND1). `jet inspect dossier` prints the evaluated declarations and their hashes (D-TARGET-AUDIT1).

### 5.2 Why no new mechanism

Every part above is ratified. Most are built for at least one domain:

- markers derive code in `Core/Http/Server.jet:24-30`;
- codecs derive from field markers;
- `prep` is ratified;
- D-EXTEND-ARCH1 ratifies traits for every list.

The failures in §3 are places where a domain did not use the shared parts. The board uses strings instead of values. Manifests are scanned as text instead of being evaluated. Codec traits are synthetic instead of declared. Two enums close the language list instead of a trait. I8 says a new mechanism must replace old ones. The evidence here is that the existing ones express every domain in §3, so the proposal adds none. The ballot `D-DECLARE-MODEL1` lets the owner rule out the alternatives: a `declare` keyword, or per-domain data files (TOML or devicetree) read by separate tools.

### 5.3 Composition, worked through

```jet
// not compiled: one board flows through target, HAL, FFI, perf and ops.
use {
    chip :: svd.stm32f411
    hal :: c.st_hal // C HAL bound by the c binder; its facts
}
                                             // (register addresses) are checked against `chip`
sensor :: hw.board(chip.device, { clock: hw.Hz(100_000_000), console: chip.USART2.serial(115_200) })

// package.jet (typed Package value)
targets: [build.target(sensor)]              // triple, CPU features, linker regions from `sensor`
budgets: [perf.budget(run, p99: 50us)]        // the perf model reads `sensor.clock` and flash wait states
```

- **Target.** The triple `thumbv7em-none-eabihf`, the `fpu` feature and the linker regions are derived from `sensor`. The battery's string triple at `embedded.jet:69-71` disappears.
- **HAL.** `OutputPin`/`Serial` impls are derived for `sensor`. Drivers are generic over them.
- **FFI.** The C HAL's register constants become facts with evidence (D-FFI-FACTS1). A mismatch with the SVD value is a refusal with both sources named.
- **Performance.** Budgets are checked against a model that knows the clock. `[INFERENCE]` A cycle-count budget can be a deterministic gate under D-WD14.
- **Operations.** The dossier records the board hash, so a field device's build is reproducible from it.

### 5.4 Cost rules

These are the extensibility note's rules (§3 there), applied here:

- **Zero run-time cost.** Declarations exist only while building.
- **Compile time grows with imports.** A board's SVD is read only when `use svd.x` appears, and it is cached by hash.
- **One tool view.** The editor shows derived items through D-MARKER-EXPAND1.
- **Readable.** A beginner writes `nucleo.led.toggle()`, not register code.
- **Safety stays.** Derived register access is safe because the declaration holds the facts that `#Unsafe` would otherwise assert.

## 6. Ballots and cards proposed

Ballots, all `short`, in `$JET_SCRATCH_ROOT/lanes/Gold/GoldC/declarative/ballots/`:

- `D-DECLARE-MODEL1`: one model for every declared thing (values, traits, markers, `prep`), or a `declare` keyword, or per-domain files.
- `D-HW-DECLARE1`: chips, boards, registers and interrupts as typed Jet values with SVD/devicetree binders, or kept as records plus hand-written structs, or devicetree files.
- `D-DB-SCHEMA1`: the record is the table, with derived and reviewed migrations, or SQL-first with typed results, or kept as SQL steps.
- `D-UI-VIEW1`: a view is a function from state to a node value and the runtime owns mounting, or effects call `mount`, or an Elm-style message loop.

Cards, as JSON for `tower card add --file`, in `$JET_SCRATCH_ROOT/lanes/Gold/GoldC/declarative/cards/`:

1. Build `trait Language` and retire `FFILanguage`/`ForeignLanguage`.
2. `ffi.binder` kit: declared type maps, handles, errors and refusals.
3. Read manifests and env modules through `prep` into typed Core records.
4. Declare the codec and comparison traits in Core source.
5. Typed channels: `Receiver<T>`.
6. Pipelines as values, with supervision on the worker line.
7. Build `#Layout(packed)` and first-argument `align`; add proven `#Policy` promises.
8. `#Test` properties, fuzz targets and bench budgets.
9. `log.Level`, `core.metrics` and one service value.
10. Package diagnostics, lints and rewrite rules as values; retire hand marker switches.
11. Typed licenses and declared data-classification flow.
12. `core.hw` typed hardware values, SVD reader and derived vector table (scope follows `D-HW-DECLARE1`).

The web DOM host gap found by the gauntlet (web-app builds but mounts nothing) is already card #6514.

## 7. Open questions this note does not settle

- Whether the SVD binder ships first-party or as a community package. The ballot recommends first-party for the reader and community packages for boards, as D-JOS-HARDWARE1 does for hardware profiles.
- Whether environment modules (D-DEV-ENV1) are ratified before card 3. Card 3 is written so that it does not depend on the outcome.
