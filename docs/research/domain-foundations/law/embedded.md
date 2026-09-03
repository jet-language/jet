# embedded

## Ratified

- **D-TARGET-SURFACE1=A / D-WD11** — embedded/freestanding profiles are typed Jet profile modules selected through `targets:`; hosted single-file programs do not mention them, and target profiles expose memory, linker, allocator, panic, volatile/MMIO, and audit controls only when selected. — `docs/spec/syntax-decisions.md:5009-5014`
- **D-TARGET-MEMORY1=A** — target memory uses named regions with origin, size, access, and kind; sizes use typed units, addresses stay numeric, and validation catches overflow/overlap/MMIO mistakes before codegen. — `docs/spec/syntax-decisions.md:5015-5018`
- **D-TARGET-LINKER1=A / D-TARGET-ALLOC1=A / D-TARGET-AUDIT1=A** — linker input is generated from typed profile facts by default; freestanding allocator/panic are required typed facts; `jet inspect dossier target` is the canonical human/machine audit view and builds write its stable JSON artifact. — `docs/spec/syntax-decisions.md:5019-5027`
- **D-TGT1–4 / D-ILE1** — packages declare `targets:` (not `kind:`); shipped target kinds are library/executable/test/example/benchmark, and omitted targets infer from `fn run()` with duplicate-entry diagnostics. — `docs/spec/syntax-decisions.md:4637-4648`
- **D-REPRC1=B** — `#Layout(c)` stamps `#[repr(C)]`, preserves field order, and rejects growable fields. — `crates/jet-foundation/src/Syntax/package_files.rs:349-356`
- **S58 / D-UNSAFE2 / D-UNSAFE-REASON1=A** — `use core.mem` is the discovery gate; `#Unsafe("reason")` is the audit gate for raw-pointer construction/deref, volatile/MMIO, pointer math, and foreign pointer crossings; bare reasonless forms are E3112. — `docs/spec/syntax-decisions.md:2806-2818`; `docs/spec/spec.md:1919-1936`
- **D-UNSAFE-OBLIG1=A** — typed `valid_ptr`, `aligned`, and `no_alias` obligations are optional absent policy, required when policy selects them, attached to the immediately preceding operation, and visible through `jet inspect unsafe`. — `docs/spec/spec.md:1945-1959`
- **D-FLAGSHIP-MMIO1** — MMIO writes use `mem.volatile_write(ptr, value)` paired with `mem.volatile_read(ptr)`; no pointer-assignment lvalue spelling is added. — `docs/spec/syntax-decisions.md:2851-2853`
- **D-PIN1=A / D-PIN2=A / D-PIN3=A** — `mem.pin(&place) -> Pin<T>` is a tracked write window; safe code may read/edit but may not move, replace, or resize the place while live, and field projections preserve the pin/view distinction. — `docs/spec/syntax-decisions.md:6695-6705`; `crates/jet-foundation/src/Syntax/core_surface.rs:672-682`
- **D-MEM-SENTRY1=A / D-MEM-GUARANTEE1=A / D-HARDENED1=A** — dev-tier raw accesses carry allocation provenance/liveness/alignment sentries; `contain`/`harden` fence dependencies; hardened builds retain sentries and foreign-dependency containment. — `docs/spec/syntax-decisions.md:7641-7647`
- **D-FREESTAND-ARTIFACT1=C** — source closure is semantic authority; a linked target artifact is only an “optional cache projection with the full identity key,” with same-source recompilation on a miss. — `tower` (`D-FREESTAND-ARTIFACT1` outcome C)
- **D-FREESTAND-TIME1=A** — pure calendar/Duration operations need no provider; injected clocks are deterministic; wall time, monotonic time, zone data, and sleep each require their own declared service. — `tower` (`D-FREESTAND-TIME1` outcome A)
- **D-FFI-CAP1=A** — FFI `&` is exclusive for exactly one call, `^` transfers ownership, and `#Close(fn)` joins consuming close; the proposed `extern c` block is not current grammar. — `docs/spec/syntax-decisions.md:7643-7643`

## Shipped

- The target examples define board/profile intent and show that hosted code can be written without target ceremony; the freestanding example uses only Core-level APIs and the board example documents `board.sensor_v1` / `board.virt_aarch64`. — `examples/features/lowlevel/freestanding.jet:1-20`; `examples/features/lowlevel/target_machine_board.jet:1-21`
- `#Layout(c)` is available for C-compatible register records; `mem.address_of`, `mem.volatile_read`, and `mem.volatile_write` are registered low-level operations, with volatile calls routed through `std::ptr::{read_volatile,write_volatile}`. — `crates/jet-foundation/src/Syntax/package_files.rs:349-356`; `crates/jet-foundation/src/Syntax/core_surface.rs:661-675`; `crates/jet-foundation/src/Syntax/core_calls.rs:1021-1035`
- `mem.pin(&place)` and `Pin<T>` are registered; the embedded probe used them for address stability, but ownership transitions remained user fields. — `crates/jet-foundation/src/Syntax/core_surface.rs:672-682`; `~/.cache/jet-luna/dx3/area-embedded/probe.md:13-20`
- `jet inspect unsafe FILE` reports gates, operations, discharge state, and policy provenance in human/JSON form; `core.sys.on_interrupt` is a separate process-lifetime hook with no direct AOT/JIT route. — `docs/spec/spec.md:1955-1959`; `crates/jet-foundation/src/Syntax/core_calls.rs:1493-1500`
- The embedded-shaped host package checked and native-built, modelled C-layout GPIO/timer/UART/DMA records, bounded queues/rings, pinning, signing, and image-slot policy; no physical board was used. — `~/.cache/jet-luna/dx3/area-embedded/probe.md:8-32,73-83`
- The existing hardening and sentry laws give an auditable raw-memory path, but they do not supply target admission, interrupt vectors, DMA mapping, WCET, flash, or device transport. — `docs/spec/syntax-decisions.md:7641-7647`

## Undecided

- Should Jet admit a usable freestanding Cortex-M target/toolchain profile for `thumbv7em-none-eabihf`, including startup/linker/provider facts and a reproducible build receipt?
- Should a target profile bind a Cortex-M vector table to bounded ISR functions and a typed ISR-to-task handoff with priority, nesting, overflow, and cancellation rules?
- Should Jet provide a target-aware WCET/hard-deadline contract with static or measured evidence, and how should loops, interrupts, compiler versions, and target clocks enter that evidence?
- Should Jet provide typed DMA mapping and ownership transfer that proves buffer lifetime, addressability, cache/coherency policy, and completion before reuse?
- Should Jet provide target-profile-backed typed MMIO register blocks with widths, access modes, and generated obligations instead of repeated hand-written records and base addresses?
- Should Jet define target-backed flash and authenticated A/B OTA state with persistent confirmation, rollback, power-loss behavior, and key authority?
- Should `jet flash` and hardware debug/program transport produce a typed device receipt containing target, image, programmer, observation, and failure identity?

## Conflicts

- D-TARGET-SURFACE1, D-TARGET-MEMORY1, D-TARGET-LINKER1, D-TARGET-ALLOC1, and D-TARGET-AUDIT1 already choose typed target profiles, generated linker inputs, required freestanding allocator/panic facts, and one dossier. The E3302 missing `thumbv7em` component is an implementation/toolchain blocker, not permission to invent a second profile model. — `~/.cache/jet-luna/dx3/area-embedded/probe.md:34-40`
- `D-REPRC1` already grants `#Layout(c)` and rejects growable fields; a new C-layout annotation or unbounded register-record mechanism would reopen ratified syntax. — `crates/jet-foundation/src/Syntax/package_files.rs:349-356`
- S58/D-UNSAFE2/D-FLAGSHIP-MMIO1 already fix the `core.mem` import gate, reasoned `#Unsafe`, typed volatile helpers, and no pointer-assignment spelling. A proposal that makes MMIO safe by default or adds a second volatile syntax conflicts with them. — `docs/spec/syntax-decisions.md:2806-2818,2851-2853`
- D-PIN1–3 grants address stability only. The probe correctly records `owner`, `submitted`, and `completed` as user fields; `mem.pin` is not a DMA mapping, device ownership, cache-coherency, or completion proof. — `~/.cache/jet-luna/dx3/area-embedded/probe.md:16,46`
- `core.sys.on_interrupt` is not an ISR/vector-table API; replacing it with a hardware interrupt primitive would be a new mechanism, not a stronger reading of the shipped hook. — `crates/jet-foundation/src/Syntax/core_calls.rs:1493-1500`
- D-MEM-SENTRY1 and D-HARDENED1 provide runtime raw-access witnesses and hardened containment, not WCET or target safety certification. A manual `wcet_estimate_us` comparison is not compiler WCET evidence. — `~/.cache/jet-luna/dx3/area-embedded/probe.md:17,40`
- D-FREESTAND-ARTIFACT1 makes source closure authoritative and artifacts cache projections. A flash/OTA image must not become an unverified semantic source or a second artifact identity system.
- D-FREESTAND-TIME1 requires separate `Time.Wall`, `Time.Monotonic`, `Time.ZoneData`, and `Time.Sleep` facts; a board timer must not be presented as a composite calendar/zone provider. — `tower` (`D-FREESTAND-TIME1` outcome A)
- The probe's release crypto and in-memory slot policy are working library code; they do not constitute persistent flash, secure-boot, rollback, or device-programming evidence. — `~/.cache/jet-luna/dx3/area-embedded/probe.md:18-20,50-56`
