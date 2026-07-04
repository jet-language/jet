//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/spec/syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/spec/syntax-decisions.md, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/spec/syntax-decisions.md.

/// N1 (ratified): language name.
pub const LANG_NAME: &str = "Jet";

/// N1 (ratified): compiler binary name.
pub const BINARY_NAME: &str = "jet";

/// N2 (ratified): source file extension (without the dot).
pub const FILE_EXT: &str = "jet";

/// S1 (ratified): keyword that starts a function definition.
pub const KW_FN: &str = "fn";

/// S18 (ratified): marks an item as visible to other files (via `use`).
pub const KW_PUB: &str = "pub";

/// D-VISDEFAULT2=A (ratified): marks an item private inside a `#PubFile` file.
pub const KW_PRIV: &str = "priv";

/// D-VISDEFAULT2=A (ratified): file-scope marker that flips default visibility to
/// public-by-default for following top-level items (D-VISDEFAULT1=C).
pub const MARKER_PUB_FILE: &str = "PubFile";

/// D-VISDEFAULT2 option B (rejected): retired spelling for the private exception
/// keyword — recognized only for E0412 teaching diagnostics.
pub const FOREIGN_PRIVATE: &str = "private";

/// D-VISDEFAULT2 option B (rejected): retired spelling for the file marker —
/// recognized only for E0418 teaching diagnostics.
pub const MARKER_PUBLIC_FILE: &str = "PublicFile";

/// D-PUBPKG1=A (ratified): the `pub(package)` visibility qualifier — restricts
/// access to sibling packages in the same payload/workspace.
pub const PUB_PACKAGE_QUALIFIER: &str = "package";

/// S2 / D-BIND1 / D-BIND4: immutable binding sigil `name :: expr`.
/// D-BIND4: explicit-type immutable form is `name: Type :: expr`; explicit mutable is
/// `name: Type := expr`. Inferred mutable stays `name := expr`; `=` reassigns `:=`
/// bindings (S17).
pub const SIGIL_BIND_IMMUT: &str = "::";

/// S2 / D-BIND1 (ratified 2026-06-18): mutable binding sigil `name := expr`
/// (was the keyword `var`). `=` stays reassignment of an existing `:=` (S17).
/// D-BIND4: explicit-type mutable form is `name: Type := expr`.
pub const SIGIL_BIND_MUT: &str = ":=";

/// S2 / D-BIND1: the retired binding keywords, recognized only for the E0985
/// teaching error that points at the `::` / `:=` sigils.
pub const FOREIGN_VAL: &str = "val";
pub const FOREIGN_VAR: &str = "var";

/// S3 (ratified): block delimiters.
pub const BLOCK_OPEN: &str = "{";
pub const BLOCK_CLOSE: &str = "}";

/// S7 (ratified M4): propagates a fallible result from the callee.
pub const OP_TRY_SUFFIX: &str = "?";

/// S5 (ratified): line comments run from this prefix to end of line.
pub const COMMENT_PREFIX: &str = "//";

/// S5 (ratified 2026-06-15): block comments `/* … */`, nesting allowed so a
/// region containing other comments can always be commented out.
pub const BLOCK_COMMENT_OPEN: &str = "/*";
pub const BLOCK_COMMENT_CLOSE: &str = "*/";

/// S6 / S6-R (ratified 2026-06-18): the statement terminator. Under S6-R it is
/// **never user-typed** — the lexer inserts a synthetic `;` at each line end
/// after a statement-ending token (Go-style), so the grammar stays
/// terminator-based while source has no visible semicolons. `jet fmt` emits
/// none. The constant remains because the token still exists internally.
pub const STMT_SEP: &str = ";";

/// S8 (ratified): string interpolation delimiters inside quoted text.
pub const INTERP_OPEN: &str = "{";
pub const INTERP_CLOSE: &str = "}";

/// S9 (ratified): the built-in print function (adds a newline).
pub const BUILTIN_PRINT: &str = "print";

/// D-PRELUDE1 option B (ratified): `input` is ambient (no `use core.io` required).
/// Both `print` and `input` form the prelude set — the two symbols a first interactive
/// program reaches for. All other core.io members stay qualified behind `use core.io`.
pub const BUILTIN_INPUT: &str = "input";

/// The full prelude set (D-PRELUDE1 = B). Kept as a constant slice so sema and
/// codegen can agree on membership without drifting from each other.
pub const PRELUDE_IDENTS: &[&str] = &["print", "input"];

/// S11 (ratified): built-in type names (M1).
pub const TYPE_INT: &str = "Int";
pub const TYPE_FLOAT: &str = "Float";
pub const TYPE_BOOL: &str = "Bool";
pub const TYPE_STRING: &str = "String";
pub const TYPE_ERROR: &str = "Error";

/// D-SG9/S42 (ratified): explicit fixed-width numeric spellings for expert and
/// FFI/binary code. `Int`/`Float` stay the beginner defaults (64-bit); `I64`
/// and `F64` are the explicit-width aliases for the same two types and
/// canonicalise to `Type::Int`/`Type::Float` at parse time. The other widths
/// are distinct types (no implicit narrowing/mixing — D-NUMOPS1).
pub const TYPE_I8: &str = "I8";
pub const TYPE_I16: &str = "I16";
pub const TYPE_I32: &str = "I32";
pub const TYPE_I64: &str = "I64";
pub const TYPE_U8: &str = "U8";
pub const TYPE_U16: &str = "U16";
pub const TYPE_U32: &str = "U32";
pub const TYPE_U64: &str = "U64";
pub const TYPE_F32: &str = "F32";
pub const TYPE_F64: &str = "F64";

/// D-SG9: every fixed-width numeric spelling, for keyword/reserved-name checks.
pub const SIZED_NUMERIC_TYPES: &[&str] = &[
    TYPE_I8, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_F32,
    TYPE_F64,
];

/// D-CAP7 (ratified): capability sigils. These SUPERSEDE the word spellings of
/// S10 (`mut`/`take`/`view`). The keyword forms below are retired — recognized
/// only to fire the E0056/E0057/E0058 teaching errors that point at the sigil.
///
/// `~T` = write (edit in place), `^T` = move (consume), `&T` = share (borrow).
/// `copy` stays a verb (no sigil — D-CAP7 closed the set at these three sigils
/// plus `copy`).
pub const SIGIL_MUTATE: &str = "~";
pub const SIGIL_MOVE: &str = "^";
pub const SIGIL_VIEW: &str = "&";

/// S10 (M2) → D-CAP7: the retired write keyword. Recognized only for the E0056
/// teaching error that points at the `~` sigil.
pub const KW_MUTATE: &str = "mut";

/// S10 (M2) → D-CAP7: the retired move keyword. Recognized only for the E0057
/// teaching error that points at the `^` sigil. (`.take(n)` stays a valid method
/// name in dot position; `take(names)` stays the lambda capture prefix.)
pub const KW_MOVE: &str = "take";

/// S10 (M2) → D-CAP7: the retired share/borrow keyword. Recognized only for the
/// E0058 teaching error that points at the `&` sigil.
pub const KW_VIEW: &str = "view";

/// S10 (ratified M2, tier 2): field annotation — a stored reference.
pub const KW_STORED: &str = "ref";

/// M2: struct definition keyword (construction spelling: S29).
pub const KW_STRUCT: &str = "struct";

/// S30 (ratified M3): sum-type definition keyword.
pub const KW_ENUM: &str = "enum";

/// D-TYPEALIAS1 (ratified 2026-06-28): transparent type alias — `alias Name<T> = …`
/// for generic type shortcuts only (not primitive newtypes).
pub const KW_ALIAS: &str = "alias";

/// S32 (ratified M3): optional type suffix — `Int?` is “maybe an Int”.
pub const TYPE_OPTION_SUFFIX: &str = "?";

/// S32 (ratified M3): present / absent spellings for `T?` (lowercase like `true`).
pub const LIT_VALUE: &str = "value";
pub const LIT_NULL: &str = "null";

/// S27 (ratified M3): method receiver name.
pub const KW_SELF: &str = "self";

/// S27 (ratified M3): external method block — `impl Type { ... }`.
pub const KW_IMPL: &str = "impl";

/// D-EXTMETH1=B (ratified): external inherent methods attach with dot:
/// `fn Type.method(self)`. S83's earlier `~~` connector is retired and exists
/// only for E0325 teaching diagnostics.
pub const EXTERNAL_METHOD_CONNECTOR: &str = ".";
pub const EXTERNAL_METHOD_CONNECTOR_RETIRED: &str = "~~";

/// M2: compile-time constant (emits Rust `const` or `static`).
pub const KW_CONST: &str = "const";

/// M1/M2: return from a function.
pub const KW_RETURN: &str = "return";

/// M2: loop statement (for SharedHandle lint checks).
pub const KW_LOOP: &str = "loop";

/// D-STREAMYIELD1 (ratified): `yield expr` — hand a value to a `Stream<T>`
/// consumer and suspend until the next pull. Legal only in a function whose
/// return type is `Stream<T>`.
pub const KW_YIELD: &str = "yield";

/// D-STREAMYIELD1: the generator return-type constructor `Stream<T>`.
pub const TYPE_STREAM: &str = "Stream";

/// D-UNSAFE2 (ratified 2026-06-22, opt B; prev S58 2026-06-12): the audited
/// expert gate. Block form: `#Unsafe("reason") { … }`. Whole-function form:
/// `#Unsafe("reason") fn`. The reason is the argument of `#Unsafe` itself;
/// the separate `#Audit` marker is retired (E0055). The bare lowercase `unsafe`
/// keyword (FOREIGN_UNSAFE) is the rejected foreign spelling, recognized only
/// to emit a teaching error.
pub const KW_UNSAFE: &str = "Unsafe";

/// D-CTEFFECT1 (ratified 2026-06-25): `#Impure("reason") { … }` — the audited
/// Tier-2 comptime effect gate. Both this block AND `--allow-impure` at build
/// are required to execute ambient comptime I/O (Fs/Env/Exec/Io). PascalCase
/// per D-CASING1 (consistent with `Unsafe`).
pub const KW_IMPURE: &str = "Impure";

/// D-REACTCORE1 (ratified 2026-06-27, opt D): `#Reactive fn` / `#Reactive { … }` —
/// an explicit opt-in scope marker. Inside it, signal `.get()` reads register with
/// the active reactive observer (library machinery in `core.reactive`). Lowers to
/// `jet_reactive_scope` / `jet_reactive_effect` — no new evaluation semantics.
pub const KW_REACTIVE: &str = "Reactive";

/// D-WASM1=A (ratified 2026-06-28, c123): `#Target(Wasm|Js)` — module- or file-level
/// default web partition ceiling. Sema validates it against inferred `Browser` effects.
pub const ATTR_TARGET: &str = "Target";

/// D-WASM1=A: per-function override — force WASM compilation bucket.
pub const ATTR_WASM: &str = "Wasm";

/// D-WASM1=A: per-function override — force JS compilation bucket.
pub const ATTR_JS: &str = "Js";

/// D-WASM1=A: export this WASM function to the generated JS loader.
pub const ATTR_WASM_EXPORT: &str = "WasmExport";

/// D-WASM1=A: `#Target(Js)` argument spelling.
pub const WEB_BUCKET_JS: &str = "Js";

/// D-WASM1=A: `#Target(Wasm)` argument spelling.
pub const WEB_BUCKET_WASM: &str = "Wasm";

/// D-WEBKIND1=A (c123): `jet build --target=web` Jet backend target (not a rustc triple).
pub const BUILD_TARGET_WEB: &str = "web";

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Web)` argument spelling — a file-level
/// marker distinct from the `Wasm`/`Js` partition-ceiling values above (same
/// `#Target(...)` marker, different axis: "build me for the web backend by
/// default" rather than "cap this file's partition ceiling").
pub const WEB_TARGET_DEFAULT_WEB: &str = "Web";

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#Html("path.html")` — an explicit, file-level
/// declaration of this program's companion host page for `--target=web`
/// builds, replacing the silent `<stem>.html` filename convention.
pub const ATTR_HTML: &str = "Html";

/// D-OSTARGET1=A (ratified 2026-07-01, c134): `#Target(Os. … )` namespace — the
/// second, mutually-exclusive axis of the `#Target(...)` marker family
/// (`Wasm`/`Js`/`Web` above are the first, web-bucket axis). Attaches at
/// `impl` block scope, not file/module scope.
pub const TARGET_OS_NAMESPACE: &str = "Os";

/// D-OSTARGET1=A: `#Target(Os.Linux)`.
pub const TARGET_OS_LINUX: &str = "Linux";

/// D-OSTARGET1=A: `#Target(Os.Macos)`.
pub const TARGET_OS_MACOS: &str = "Macos";

/// D-OSTARGET1=A: `#Target(Os.Windows)`.
pub const TARGET_OS_WINDOWS: &str = "Windows";

/// D-OSTARGET2=B (ratified 2026-07-03): the compiler-known comptime value
/// `build` and its `.os` field — the subject of a `comptime if build.os == { }`
/// switch that folds to the arm matching the build's active OS. `build` is not
/// a reserved keyword: it is recognized only in that syntactic position (an
/// ordinary local named `build` is still fine); a `build.os` anywhere else has
/// no compiler meaning.
pub const BUILD_INFO: &str = "build";
/// D-OSTARGET2=B: the `.os` field of the comptime `build` value.
pub const BUILD_INFO_OS: &str = "os";

/// S14/S58: bare lowercase `unsafe` — the foreign (C/Rust) spelling, recognized
/// only for teaching errors (E0031 / E0003) pointing at the `#Unsafe` marker.
pub const FOREIGN_UNSAFE: &str = "unsafe";

/// S58 (ratified 2026-06-12): discovery gate — naming any low-level item
/// requires `use core.mem`.
pub const CORE_MEM_MODULE: &str = "core.mem";

/// D-UNINIT1 (ratified 2026-06-21, opt C): the `#Uninit` binding marker.
/// **Retired by D-UNINIT-SENTINEL1** (2026-07-02) — the spelling moved to the
/// `uninit` contextual keyword (see `KW_UNINIT` below); this constant is kept
/// only so the parser can recognize the old marker and reject it with a
/// teaching error (E0426) pointing at the new spelling.
pub const ATTR_UNINIT: &str = "Uninit";

/// D-UNINIT-SENTINEL1 (ratified 2026-07-02, opt D): contextual keyword
/// `uninit`, legal only as the RHS of `:=` on a binding with an explicit type
/// annotation — `name: Type := uninit`. Supersedes the `#Uninit name: Type`
/// marker (D-UNINIT1); reuses that decision's sema flow-analysis engine
/// unchanged (`CheckerCore.rs`'s uninit tracking map) — only the trigger that
/// flips `Binding.uninit` moves from seeing the marker to seeing this word in
/// initializer position. Still gated by `use core.mem` (E0424) and restricted
/// to plain-data types (E0423). Contextual like `region`/`state`/`migration`:
/// the word `uninit` stays usable as an ordinary identifier everywhere else;
/// the lexer emits it as a plain `Ident`, and only the parser's initializer
/// position recognizes it.
pub const KW_UNINIT: &str = "uninit"; // D-UNINIT-SENTINEL1

/// D-REF-SHORTHAND1/2 (ratified 2026-07-02): a stored-reference field spells
/// its type `&T`; the owner is inferred from the single in-scope value of the
/// referent type. `#Ref(owner) field: &T` only *disambiguates* the owner when
/// several candidates exist (E0207). Stays on the `#` directive plane, never
/// `@Ref` (D-REF-SHORTHAND2). The retired `#Ref(owner) field: T` form (plain
/// type, no `&`) is E0427. No `use core.mem` gate.
pub const ATTR_REF: &str = "Ref";

/// D-OPTGC1 (ratified 2026-06-26): opt-in traced GC library module.
pub const CORE_GC_MODULE: &str = "core.gc";

/// D-OPTGC1: `Gc<T>` smart pointer type name in `core.gc`.
pub const GC_TYPE: &str = "Gc";

/// D-OPTGC1: construct a `Gc<T>` value.
pub const GC_NEW: &str = "new";

/// S58 / D-CAP9 / D-TYPE-ALIAS-CANON1 (ratified): raw-pointer type is `*T`.
/// `Ptr<T>` is not live syntax; this string remains only as the internal Rust
/// dispatch key for legacy TIR/codegen paths until they are renamed to `RawPtr`.
pub const TYPE_PTR: &str = "Ptr";

/// S58 (ratified 2026-06-12): `mem.Ptr<T>.from_addr(addr)` — typed pointer
/// from an integer address.
pub const MEM_FROM_ADDR: &str = "from_addr";

/// S58 (ratified 2026-06-12): `mem.volatile_read(p)` — volatile/MMIO read.
pub const MEM_VOLATILE_READ: &str = "volatile_read";

/// S58 (ratified 2026-06-12): `mem.address_of(x)` — the address of a value as
/// an Int (taking a pointer is inert; using it needs `@unsafe`).
pub const MEM_ADDRESS_OF: &str = "address_of";

/// D-ALLOC1 (ratified 2026-06-19): arena allocator type name.
/// Construct with `mem.Arena.new()`, allocate with `arena.alloc(value)`.
/// Gated by `use core.mem` (E3102); no `#Unsafe` needed.
pub const MEM_ARENA: &str = "Arena";

/// D-ALLOC-C (ratified 2026-06-19): bump allocator (append-only, O(1)).
/// Grouped under `core.mem.alloc` together with Arena/Pool/Fixed.
pub const MEM_BUMP: &str = "Bump";

/// D-ALLOC-C (ratified 2026-06-19): pool allocator (fixed-slot slab).
pub const MEM_POOL: &str = "Pool";

/// D-ALLOC-C (ratified 2026-06-19): fixed allocator (static backing buffer).
pub const MEM_FIXED: &str = "Fixed";

/// D-ALLOC1 (ratified 2026-06-19): allocator constructor method name.
pub const MEM_ALLOC_NEW: &str = "new";

/// D-ALLOC1 (ratified 2026-06-19): allocate a value into an arena/bump/pool/fixed.
pub const MEM_ALLOC_ALLOC: &str = "alloc";

/// D-ALLOC-D (ratified 2026-06-19): reset the allocator, keeping the backing buffer.
pub const MEM_ALLOC_RESET: &str = "reset";

/// D-ALLOC-D (ratified 2026-06-19): free the backing memory, returning it to the OS.
pub const MEM_ALLOC_FREE: &str = "free";

/// D-ALLOC-C (ratified 2026-06-19): wider allocator API namespace.
pub const CORE_MEM_ALLOC_MODULE: &str = "core.mem.alloc";

/// D-ARGS1 (ratified 2026-06-22): declarative CLI argument parsing module.
pub const CORE_ARGS_MODULE: &str = "core.args";

/// D-REGION1 (ratified 2026-06-21, opt B): explicit allocation-region block
/// `region r { … }`. A lowercase contextual block keyword (D-CASING1) that
/// names a region spanning multiple arenas or narrower than the enclosing
/// function; arena `view`s allocated inside may not escape the region (E0631).
/// The beginner default is an implicit scope-inferred region (opt A) and never
/// writes `region`.
pub const KW_REGION: &str = "region";

/// D-TASKSCOPE1=A: structured task group scope — owns child tasks until scope exit.
pub const KW_TASKGROUP: &str = "taskgroup";

/// D-CTX1 (ratified 2026-06-22, G2): smart-context block marker.
/// `#context(field: value) { … }` swaps named ambient fields (allocator,
/// logger) for the lexical+dynamic extent of the block, then restores them.
/// The marker is PascalCase (D-CASING1); the field names inside are lower.
/// Expert-tier only (R1): never emitted in beginner-tier diagnostics or docs.
pub const CTX_BLOCK: &str = "Context";

/// D-CTX1 + D-DEADLINE1 (ratified 2026-06-22/2026-06-28): allowed field names
/// inside `#Context(…)`.
pub const CTX_FIELD_ALLOCATOR: &str = "allocator";
/// D-CTX1 (ratified 2026-06-22): logger field (v1 bundle).
pub const CTX_FIELD_LOGGER: &str = "logger";
/// D-DEADLINE1 (ratified 2026-06-28): absolute deadline (epoch millis) carried
/// through wait/IO points in the current task context.
pub const CTX_FIELD_DEADLINE: &str = "deadline";

/// D-TERM1 (ratified 2026-06-22): terminal direct-input block keyword.
/// `live { … }` enters un-buffered/no-echo input mode for its body and
/// guarantees terminal-state restore on every exit path including panic
/// (implemented with the D-DEFER1 scope-guard mechanism). "raw mode" jargon
/// is deliberately avoided; `live` is the user-facing name. A contextual
/// keyword: recognised only when followed by `{`.
pub const KW_LIVE: &str = "live";

/// D-DET1 (ratified 2026-06-22): the expert determinism-escape block keyword.
/// `assume_deterministic { … }` inside a `@Pure fn` suspends the determinism
/// rejections (E3401/E3403) for its body — the "I know this is deterministic"
/// hatch. A semantic footgun, v1-legal per the card. A contextual keyword:
/// recognised only when followed by `{`, so a name `assume_deterministic` still
/// works elsewhere. Erased in codegen (I3) — the block is a plain Rust block.
pub const KW_ASSUME_DET: &str = "assume_deterministic";

/// D-DET1 (ratified 2026-06-22): the deterministic injected `Clock` capability
/// type. A `@Pure fn` taking a `Clock` param may read time **through it**
/// (`clock.now()` / `clock.tick(ms)`) — reproducible, because the caller seeded
/// it (`time.clock(seed)`) — while the ambient `time.now()` stays E3403. An
/// ordinary value type, not a module alias; methods are pure-from-the-fn's-view.
pub const CLOCK_TYPE: &str = "Clock";

/// D-DET1 (ratified 2026-06-22): the deterministic injected `Rng` capability
/// type. A `@Pure fn` taking an `Rng` param may draw randomness **through it**
/// (`rng.int(lo, hi)` / `rng.float()`) — reproducible from the caller's seed
/// (`random.rng(seed)`) — while the ambient `random.int(…)` stays E3403.
/// D-DET-CAPAPI (ratified 2026-06-25) widens `Rng` with `bool()` / `pick(list)`
/// / `shuffle(~list)`, mirroring the ambient `random.*` set.
pub const RNG_TYPE: &str = "Rng";

/// D-DET-CAPAPI (ratified 2026-06-25): the deterministic `Duration` value type.
/// A small std-only span of milliseconds, constructed via `time.ms(n)` /
/// `time.secs(n)` (pure — no ambient effect, like `time.clock`). Read back with
/// `duration.millis()`; the injected `Clock` advances by one via `clock.wait(d)`
/// (relative), alongside the absolute `clock.advance(to_ms)`.
pub const DURATION_TYPE: &str = "Duration";

/// D-BIGINT1 (ratified 2026-06-28): arbitrary-precision integer. Construct
/// explicitly with `BigInt(100)` or `BigInt("…")`; fixed `Int` never promotes.
pub const TYPE_BIGINT: &str = "BigInt";

/// D-DECIMAL1 (ratified 2026-06-26): exact base-10 decimal. Construct with
/// `Decimal("12.34")` or `core.numeric.decimal("12.34")`; no implicit `Float`.
pub const TYPE_DECIMAL: &str = "Decimal";

/// D-SIMD1/D-SIMD2 (ratified 2026-06-24): the built-in portable SIMD lane types.
/// `F32x4` is four `F32` lanes, `F64x2` is two `F64` lanes. Constructor
/// `F32x4(a,b,c,d)`, splat `F32x4.splat(x)`, lane index `v[i]`, element-wise
/// `+`/`-`/`*`/`/`, reduce `v.sum()` / `v.reduce(#Max)`, and the `[F32#4]` bridge
/// `from_array`/`to_array`. A closed compiler-provided family (no user `+`); ops
/// lower to a scalar-array fallback (the pinned stable rustc has no `std::simd`),
/// memory-safe by construction (I1) — no `std::simd`-feature gate, no intrinsics.
pub const SIMD_F32X4_TYPE: &str = "F32x4";
pub const SIMD_F64X2_TYPE: &str = "F64x2";

/// D-SWIZZLE1 (ratified 2026-06-27): named lane swizzles on vector/SIMD types.
/// Members `x`/`y`/`z`/`w` name lanes 0..3; patterns like `v.xyz`, `v.yx`, and
/// write-swizzles `v.xy = Vec2(…)` are blessed field access on `Vec2`/`Vec3`/`Vec4`
/// and `F32x4`/`F64x2` only (matrices are not swizzleable). Overlapping write
/// patterns (`v.xx = …`) are diagnosed (E3111); out-of-range lanes (E3110).

/// D-LINALG1 (ratified 2026-06-24): the built-in linear-algebra value types.
/// Vectors `Vec2`/`Vec3`/`Vec4` and square matrices `Mat3`/`Mat4` (column-major,
/// `F64` components). Methods `.dot()`/`.cross()` (Vec3 only)/`.matmul()`, plus
/// `.length()`/`.normalize()`; element-wise `+`/`-`, scalar `*`, and `Mat * Vec`.
/// A closed compiler-provided family — the user-facing aliases over the generic
/// `Vec<N>`/`Matrix<M,N>` substrate. Plain std math (no `unsafe`).
pub const LINALG_VEC2_TYPE: &str = "Vec2";
pub const LINALG_VEC3_TYPE: &str = "Vec3";
pub const LINALG_VEC4_TYPE: &str = "Vec4";
pub const LINALG_MAT3_TYPE: &str = "Mat3";
pub const LINALG_MAT4_TYPE: &str = "Mat4";

/// D-LAYOUT1 / D-LAYOUT-GATES1 (ratified 2026-06-28/29): the built-in
/// constraint-layout value types. `HVar`/`VVar` are axis-typed layout
/// variables (horizontal/vertical); `LengthVar` is an axis-neutral scalar
/// length that combines with either axis. GATE 1: comparison operators
/// (`>=`/`<=`/`==`) on these types produce a `Constraint`, not `Bool` — a
/// closed-operator blessing on this family only (same category as D-SIMD2).
/// GATE 2: all five names enter the compiler's closed type family
/// (`core_type_known`). A closed compiler-provided family — no user `+`/
/// comparison overload. Cross-axis combination (`HVar` with `VVar`) is
/// E-LAYOUT-AXIS-MISMATCH (E2932). `LayoutHandle` is the `layout NAME { … }`
/// container/solver value; `Constraint` is a registered, prioritizable
/// constraint handle (`.required()`/`.strong()`/`.medium()`/`.weak()`).
pub const LAYOUT_HVAR_TYPE: &str = "HVar";
pub const LAYOUT_VVAR_TYPE: &str = "VVar";
pub const LAYOUT_LENGTHVAR_TYPE: &str = "LengthVar";
pub const LAYOUT_CONSTRAINT_TYPE: &str = "Constraint";
pub const LAYOUT_HANDLE_TYPE: &str = "LayoutHandle";

/// D-LAYOUT1 (ratified 2026-06-28): `layout NAME { … }` — a lexical block
/// that binds `NAME` (a `LayoutHandle`) in the enclosing scope (the handle
/// outlives the block, unlike `taskgroup`/`region`, since solved values are
/// read after layout is defined). Each line in the body must be a
/// `>=`/`<=`/`==` comparison of layout values (a `Constraint`); the parser
/// desugars bare `box.anchor` reads (`left`/`right`/`top`/`bottom`/`width`/
/// `height`) into `NAME.h(box, anchor)` / `NAME.v(box, anchor)` calls, which
/// sema/codegen treat exactly like any other `LayoutHandle` method call — no
/// parallel checking mechanism, GATE 1/2 do all the real work. A lowercase
/// contextual block keyword (D-CASING1), recognized only when followed by
/// `name {`.
pub const KW_LAYOUT: &str = "layout";
/// `use core.term as term` — exposes `term.read_key() -> Key`.
pub const CORE_TERM_MODULE: &str = "core.term";

/// D-TERM1 (ratified 2026-06-22): the key-event type returned by `term.read_key()`.
/// Variants: `Key.Char(c)`, `Key.Enter`, `Key.Escape`, `Key.Backspace`,
/// `Key.Tab`, `Key.Delete`, `Key.Up`, `Key.Down`, `Key.Left`, `Key.Right`,
/// `Key.F(n)`, `Key.Ctrl(c)`, `Key.Unknown`.
pub const TYPE_KEY: &str = "Key";

/// D-REACT1 (ratified 2026-06-22, option B): reactivity is an opt-in *library*,
/// not core semantics. Ordinary binding semantics are unchanged; runtime
/// reactivity ships as the `jet.reactive` ring package. The surface is three
/// explicit producers and their handle types — no new keyword or sigil (reactive
/// values are ordinary values made with library calls, exactly as option B
/// requires). `use jet.reactive as reactive` exposes:
///   reactive.signal(initial) -> Signal<T>   — a mutable reactive source
///   reactive.derived(() => expr) -> Derived<T> — a value recomputed from signals
///   reactive.computed(() => expr)            — D-SIGNAL1 alias for `derived`
///   reactive.effect(() => { … })             — a side effect re-run on change
/// Methods: `Signal.get()/set(v)`, `Derived.get()`/`Computed.get()`. Dependency
/// tracking is explicit-by-read (a `.get()` inside a derived/effect body subscribes).
/// `#Reactive { … }` lowers to the effect job (D-REACTCORE1).
pub const REACTIVE_MODULE: &str = "jet.reactive";
pub const TYPE_SIGNAL: &str = "Signal";
pub const TYPE_DERIVED: &str = "Derived";
/// D-SIGNAL1 (ratified 2026-06-28, opt A): canonical name for a derived reactive
/// value. `Derived` remains accepted as a backward-compatible alias.
pub const TYPE_COMPUTED: &str = "Computed";
/// D-SIGNAL1: the runtime value created by `#Reactive` / `reactive.effect`.
pub const TYPE_EFFECT: &str = "Effect";
/// D-HONESTNUM1=A: the science measurement type name.
pub const TYPE_MEASUREMENT: &str = "Measurement";

/// S33 (ratified M5): legacy list type constructor.
/// S65 (ratified 2026-06-15): `[T]` is canonical; `List<T>` remains accepted.
pub const TYPE_LIST: &str = "List";

/// S38 (ratified M5): map type constructor.
pub const TYPE_MAP: &str = "Map";

/// S41 (ratified M5): character type.
pub const TYPE_CHAR: &str = "Char";

/// S66 (ratified 2026-06-15): standard acronyms are fully capitalized in Jet source.
pub const TYPE_IO_ERROR: &str = "IOError";
pub const TYPE_UTF8_ERROR: &str = "UTF8Error";
pub const TYPE_JSON: &str = "JSON";
pub const TYPE_JSON_ERROR: &str = "JSONError";

/// D-ENC-DYN1=A+ (ratified 2026-06-25): the one dynamic encoding value every
/// format's `parse` returns. `Data` is canonical (the user-facing face of the
/// internal `DataTree`); `Json`/`Toml`/`Yaml`/`Csv` are type aliases over it, so
/// `json.parse` is typed `Json`, `toml.parse` is typed `Toml`, etc., but they are
/// the same structure (one walker, one accessor set). Variants: `Null`, `Bool`,
/// `Int`, `Float`, `Text`, `Array`, `Object`.
pub const TYPE_DATA: &str = "Data";
pub const TYPE_DATA_JSON: &str = "Json";
pub const TYPE_DATA_TOML: &str = "Toml";
pub const TYPE_DATA_YAML: &str = "Yaml";
pub const TYPE_DATA_CSV: &str = "Csv";

/// The five accepted spellings of the dynamic encoding value (D-ENC-DYN1=A+).
pub fn is_data_type_name(name: &str) -> bool {
    matches!(name, "Data" | "Json" | "Toml" | "Yaml" | "Csv")
}

/// The variants of the dynamic `Data` value (D-ENC-DYN1=A+), the user-facing face
/// of `DataTree`. `Bytes` stays internal (no dynamic constructor).
pub fn is_data_variant(variant: &str) -> bool {
    matches!(
        variant,
        "Null" | "Bool" | "Int" | "Float" | "Text" | "Array" | "Object"
    )
}

/// D-DBDRIVER1 (ratified): the tagged SQL parameter/column value. Construct with
/// `DbValue.Int(n)` / `.Float(f)` / `.Text(s)` / `.Bool(b)` / `.Null`; a `[DbValue]`
/// is the parameterized-query bind list, never a raw SQL string. A dedicated
/// dynamic-value type (mirrors `Data`'s construction mechanism, D-ENC-DYN1=A+)
/// — not a user-registrable enum, so it never appears in `match`.
pub const TYPE_DB_VALUE: &str = "DbValue";

/// D-DBDRIVER1: is `name` the `DbValue` dynamic-value type name?
pub fn is_db_value_type_name(name: &str) -> bool {
    name == TYPE_DB_VALUE
}

/// D-DBDRIVER1: the variants of `DbValue`.
pub fn is_db_value_variant(variant: &str) -> bool {
    matches!(variant, "Null" | "Int" | "Float" | "Text" | "Bool")
}

/// M2: shared handle type (Arc equivalent); auto-cloned across boundaries.
pub const TYPE_SHARED: &str = "Shared";

/// M1 (docs/spec/roadmap.md, owner-blessed examples 2026-06-11): branching keywords.
pub const KW_IF: &str = "if";
pub const KW_ELSE: &str = "else";

/// S19 (ratified): loop keywords. `loop` is the one true loop keyword.
/// `in` is a contextual keyword inside `loop x in …`.
/// D-LOOP-SEMICOLON1=A (ratified 2026-06-29): `loop init; cond; step { }` three-part
/// counted loop — semicolons are the separators in the header, nowhere else.
pub const KW_IN: &str = "in";

/// S22 (ratified): inclusive range between two `Int` ends — `1..10`.
pub const OP_RANGE: &str = "..";

/// S22 (amended 2026-06-15, D-SG8): contextual `step n` range stride —
/// `0..10 step 2`. Only meaningful inside a range; an ordinary name elsewhere.
pub const KW_RANGE_STEP: &str = "step";

/// S23 (ratified): loop control.
pub const KW_BREAK: &str = "break";
pub const KW_CONTINUE: &str = "continue";

/// S24 / D-IF1 (ratified 2026-06-18): `if` is the one branching keyword.
pub const KW_SWITCH: &str = "if";

/// S24 / D-IF1 (ratified): arm arrow inside a multi-arm `if` (same spelling as
/// return types).
pub const OP_ARM_ARROW: &str = "->";

/// S46 (ratified M8): lambda arrow — distinct from `->` return/arm arrow.
pub const OP_LAMBDA_ARROW: &str = "=>";

/// S11 (ratified): the two `Bool` literals.
pub const LIT_TRUE: &str = "true";
pub const LIT_FALSE: &str = "false";

/// M1 (docs/spec/roadmap.md): arithmetic operators. `+ - * /` on Int and Float;
/// `% & | ^ << >>` on Int only. No `+` on String (S8: interpolate instead).
pub const OP_PLUS: &str = "+";
pub const OP_MINUS: &str = "-";
pub const OP_STAR: &str = "*";
pub const OP_SLASH: &str = "/";
pub const OP_PERCENT: &str = "%";
pub const OP_AMP: &str = "&";
pub const OP_PIPE: &str = "|";
pub const OP_CARET: &str = "^";
pub const OP_SHL: &str = "<<";
pub const OP_SHR: &str = ">>";

/// S20 (ratified): escape sequences inside quoted text, and `{{` `}}` for
/// literal braces.
pub const ESCAPES: &[(char, char)] = &[('n', '\n'), ('t', '\t'), ('"', '"'), ('\\', '\\')];

/// S67 (ratified 2026-06-15): numeric literal forms — `_` digit separators
/// (stripped before parsing), base prefixes, and a `e`/`E` float exponent.
pub const DIGIT_SEPARATOR: char = '_';
pub const NUM_PREFIX_HEX: &str = "0x";
pub const NUM_PREFIX_OCTAL: &str = "0o";
pub const NUM_PREFIX_BINARY: &str = "0b";

/// S13 (ratified): logical operators.
pub const OP_AND: &str = "&&";
pub const OP_OR: &str = "||";
pub const OP_NOT: &str = "!";

/// S13 (ratified): comparison operators.
pub const OP_EQ: &str = "==";
pub const OP_NE: &str = "!=";
pub const OP_LT: &str = "<";
pub const OP_GT: &str = ">";
pub const OP_LE: &str = "<=";
pub const OP_GE: &str = ">=";

/// S17 (ratified): compound assignment operators (M1).
pub const OP_PLUS_EQ: &str = "+=";
/// D-INCR1 (ratified 2026-06-30): C-style increment/decrement operators.
pub const OP_PLUS_PLUS: &str = "++";
pub const OP_MINUS_EQ: &str = "-=";
pub const OP_MINUS_MINUS: &str = "--";
pub const OP_STAR_EQ: &str = "*=";
pub const OP_SLASH_EQ: &str = "/=";
pub const OP_PERCENT_EQ: &str = "%=";
pub const OP_AMP_EQ: &str = "&=";
pub const OP_PIPE_EQ: &str = "|=";
pub const OP_CARET_EQ: &str = "^=";
pub const OP_SHL_EQ: &str = "<<=";
pub const OP_SHR_EQ: &str = ">>=";

/// D-PATW (ratified 2026-06-19): `_` in a variant payload slot ignores that field and binds nothing.
/// `_` remains a legal identifier character (digit-separator, S34) in all other positions.
/// No bare `_` arm in a switch — only `else ->` acts as a catch-all.
pub const PAT_WILDCARD_SLOT: &str = "_";

// D-PATR (ratified 2026-06-19): range patterns (`lo..hi`) reuse OP_RANGE (S22) at arm-head
// level and inside variant payload slots. Open Int/Char subjects always still require `else`.

// D-PATO (ratified 2026-06-19): structural or-patterns use OP_PIPE (single `|`).
// `Active(id) | Reconnecting(id) -> …`; alternatives must bind the same names at the same types.
// `||` remains value-or / boolean-or; `|=` is bitwise-or-assign.

// D-ENUMDOT1 (ratified 2026-06-26, implemented): a leading `.` before a variant name in pattern
// position (`.Circle(r)`, `.Empty`) is now accepted everywhere a variant pattern is written —
// `if subject == { .Variant(b) -> … }`, `if x == .Variant(b)`, switch arms. The dot reads as
// "a member of the inferred enum" and resolves S31's bare-name-vs-variable ambiguity without
// requiring a qualified `Enum.Variant` spelling. Bare form still accepted; dot form is canonical
// (the formatter always emits `.` before a Pattern::Variant name). No new keyword or sigil —
// reuses OP_DOT (S21). Value-position dot (`.Red` where type is known) is D-ENUMDOT2 (open).

// D-TAG1 (ratified 2026-07-03): enum variant groups. A variant may enclose sub-variants
// in `{ }` (`enum Damage { Physical { Blunt, Pierce } Fire { Burn, Scald } Cold }`), to any
// depth. A group name matches its whole subtree in `==` pattern tests and dispatch arms
// (`d == .Fire` is true for `.Fire.Burn`); exhaustiveness is checked at the group level;
// payloads live on leaves only (E0331); a value is always a leaf — a group name is not a
// value (E0332). Ships with the core counted multiset `Bag<T>` (`Bag.new()`, `add`,
// `remove`, `has`, `count`; subtree queries stay an explicit `any` closure). No new keyword
// or sigil — reuses `{ }` blocks, OP_DOT paths, and D-ENUMDOT1 leading-dot patterns.

// D-RANGE2 (c25, ratified): porting-hazard teaching errors for constructs Jet does NOT use.
// E0318: `..=` (Rust inclusive range) — Jet's `..` is already inclusive; `0..=9` → teach `0..9`.
// E0319: `step` in an arm head — `step` is a loop modifier (S22/S72), not an arm construct.
// These use `..` (OP_RANGE) and `step` (KW_RANGE_STEP) which are already registered above.

/// S13 (ratified): word forms recognized only for S14 teaching errors.
pub const FOREIGN_AND: &str = "and";
pub const FOREIGN_OR: &str = "or";
pub const FOREIGN_NOT: &str = "not";

/// S16 (ratified M6; amended 2026-06-16, D-S16-USE): file path or module `use`; optional `as`.
pub const KW_USE: &str = "use";
pub const KW_AS: &str = "as";

/// S51 / D-CORENS-CANON1: compiler-known `core.*` library root.
pub const CORE_SHORT: &str = "core";
pub const CORE_CANONICAL_ROOT: &str = "core";
pub const CORE_CANONICAL: &str = "core";

/// S51 (ratified M10): first-party short names reserved before packages land.
pub const FIRST_PARTY_RESERVED: &[&str] = &[
    "core", "jet", "c", "http", "regex", "csv", "toml", "crypto", "archive",
];

/// S50 (ratified M7): Rust FFI block introducers — `extern rust "…" { … }`.
pub const KW_EXTERN: &str = "extern"; // S50
pub const KW_RUST: &str = "rust"; // S50

/// S59 (ratified E2-M14): C FFI module path root — `c.<lib>`, `c.<lib>.__bindgen__`.
pub const C_MODULE_ROOT: &str = "c"; // S59
/// S59: reserved final segment for compiler-generated bindgen modules.
pub const C_BINDGEN_SEGMENT: &str = "__bindgen__"; // S59
/// S59 / D-CFFI-CANON1: marker on generated C binding modules — `#Bindgen module`.
pub const ATTR_BINDGEN: &str = "Bindgen"; // S59 / D-CFFI-CANON1
/// S59 / D-CFFI-CANON1: marker on user C overlay modules — `#Extern module`.
pub const ATTR_EXTERN_MODULE: &str = "Extern"; // S59 — `#Extern module`, not `extern rust`
/// D-CFFI-SYNTAX-REOPEN / D-CFFI-CANON1: retired C FFI marker spellings,
/// recognized only for E0060 teaching diagnostics.
pub const ATTR_BINDGEN_RETIRED: &str = "bindgen";
pub const ATTR_EXTERN_MODULE_RETIRED: &str = "extern";
/// D-UNSAFE2 (retired marker): `#Audit("…")` is the old two-line form;
/// now the reason is the argument of `#Unsafe("reason")` itself. Recognized
/// only to emit the E0055 teaching error.
pub const ATTR_AUDIT: &str = "Audit"; // retired, D-UNSAFE2
                                      // D-LOOPLABEL2=A (ratified 2026-06-26): loop label `@` is a SUFFIX on the name.
                                      // `outer@ loop { … }` / `break outer@` / `continue outer@`. Reverses D-LABEL1
                                      // (which had `@outer loop`). Old prefix form emits E0988 teaching error.
                                      // D-ATTR3 = B (ratified 2026-06-19): `@` stays for labels; attributes use `#`.
                                      // D-QUAL4=A (ratified 2026-06-26): `#Marker T` is a value-tag qualifier in type
                                      // position. Transparent to type identity (the underlying type is still `T`).
                                      // Documented intent only; does not affect codegen or runtime behaviour.
                                      // Parser: `TokKind::Hash` followed by PascalCase ident → `Type::Tagged { marker, inner }`.
/// S59: cache directory segment under `.jet/` for generated C bindings.
pub const BINDINGS_C_SUBDIR: &str = "bindings/c"; // S59

/// S14: foreign forms recognized only for teaching errors.
/// S19-amend (2026-06-17): `while`/`for` are now teaching errors pointing at `loop`.
pub const FOREIGN_WHILE: &str = "while";
pub const FOREIGN_FOR: &str = "for";
pub const FOREIGN_TRY: &str = "try";
pub const FOREIGN_LET: &str = "let";
pub const FOREIGN_LET_MUT: &str = "let mut";
pub const FOREIGN_SET: &str = "set";
pub const FOREIGN_FUNC: &str = "func";
pub const FOREIGN_DEF: &str = "def";
pub const FOREIGN_IMPORT: &str = "import";
pub const FOREIGN_PRINTLN: &str = "println";
pub const FOREIGN_TEXT: &str = "Text";

/// S24: `match` recognized only for a teaching error naming `when`.
pub const FOREIGN_MATCH: &str = "match";

/// S24 (D-SG1): `switch` recognized only for a teaching error naming `when`
/// (the keyword was `switch` before the 2026-06-15 rename).
pub const FOREIGN_SWITCH: &str = "switch";

/// S32 (ratified M3): foreign optional spellings for teaching error E0020.
pub const FOREIGN_NONE: &str = "None";
pub const FOREIGN_SOME: &str = "Some";
pub const FOREIGN_NIL: &str = "nil";
pub const FOREIGN_NONE_LOWER: &str = "none";
pub const FOREIGN_SOME_LOWER: &str = "some";

/// S29 (ratified M3): `class` recognized only for teaching error E0021.
pub const FOREIGN_CLASS: &str = "class";

/// S28 (ratified M9): trait declaration keyword.
/// D-IMPLDOT1=A (ratified 2026-06-26): the trait separator in top-level impl blocks
/// is `.` — `impl Type.Trait { … }` / `impl Type.Trait using field`. The old `:`
/// separator emits E0320 (teaching error). Amends S28/S62; retires `~~` (S83).
pub const KW_TRAIT: &str = "trait";

/// D-QUAL2 (ratified 2026-06-21): tag declaration keyword — a marker qualifier
/// with no methods that erases at runtime. The beginner rule: methods → trait,
/// no methods → tag. Declaring a method on a `tag` is E0732; using a tag where
/// dispatch is expected is E0731.
pub const KW_TAG: &str = "tag";

/// S55 (ratified M9): opt-in built-in derive line in a type body.
pub const KW_DERIVE: &str = "derive";

/// S57 (ratified M9.5): compile-time constant binding keyword.
/// Also used in `comptime if` (D-WHEN1, ratified 2026-06-19) — the two-word
/// sequence `comptime if` is parsed as a compile-time conditional; no new
/// keyword is required. The unselected arm gets name-resolution only (D-WHEN2).
pub const KW_COMPTIME: &str = "comptime";

/// S28: foreign trait spellings for teaching error E0022.
pub const FOREIGN_INTERFACE: &str = "interface";
pub const FOREIGN_TRAIT: &str = "trait";
/// D-NAMESPACE1=A: `namespace { }` is declined; in-file grouping uses `module name { }`.
/// Recognized to emit teaching error E0323 (S14-style).
pub const FOREIGN_NAMESPACE: &str = "namespace";

/// S48 (M9): foreign dynamic-dispatch spellings for teaching error E0036.
pub const FOREIGN_DYN: &str = "dyn";
pub const FOREIGN_BOX: &str = "Box";

/// S24 (ratified M3): foreign switch arm spellings for teaching error E0023.
pub const FOREIGN_CASE: &str = "case";
pub const FOREIGN_DEFAULT: &str = "default";

/// S10 (ratified M2): foreign read/write forms for teaching errors.
pub const FOREIGN_READ: &str = "read";
pub const FOREIGN_WRITE: &str = "write";
pub const FOREIGN_OWNED: &str = "owned";

/// S34 (legacy M4): old fallible type constructor, kept only for diagnostics.
pub const TYPE_RESULT: &str = "Result";

/// S34 (ratified M4): success / failure constructors for fallible `T ? E`.
pub const LIT_OK: &str = "ok";
pub const LIT_ERR: &str = "err";

/// S35 (ratified M4; spelling updated by S71/D-SG6): the fallback operator,
/// supplying a value, `return`, or `panic` when a `T?` is absent or a `T ? E`
/// failed. Spelled `??` since the 2026-06-15 rename (was the word `or`).
pub const OP_FALLBACK: &str = "??";

/// S71 (ratified 2026-06-15, D-SG6): optional chaining — `a?.b` yields a `T?`
/// and short-circuits to absent on the first missing link.
pub const OP_OPTIONAL_CHAIN: &str = "?.";

/// S71 (ratified 2026-06-15, D-SG6): the retired word fallback, kept only for
/// the teaching error that points at `??`.
pub const FOREIGN_OR_FALLBACK: &str = "or";

/// S36 (ratified M4): bug-stop builtins (like `print`).
pub const BUILTIN_PANIC: &str = "panic";
/// D-NUMOPS1 (ratified 2026-06-22): per-op overflow opt-ins. Each wraps a single
/// integer `+`/`-`/`*`/`/`: `wrapping(…)` wraps around, `saturating(…)` clamps to
/// the type's range, `checked(…) -> T?` returns `null` on overflow.
pub const BUILTIN_WRAPPING: &str = "wrapping";
pub const BUILTIN_SATURATING: &str = "saturating";
pub const BUILTIN_CHECKED: &str = "checked";
pub const BUILTIN_REQUIRE: &str = "require";
/// S43 (ratified M6): equality assertion in test blocks.
pub const BUILTIN_REQUIRE_EQ: &str = "require_eq";

/// D-CTIO1 (ratified 2026-06-22): the two sanctioned build-time I/O builtins.
/// `embed_file("path") -> String` bakes a file's UTF-8 text into the binary;
/// `embed_bytes("path") -> [U8]` bakes its raw bytes (binary-safe). The path
/// must be a string literal, resolved relative to the source file, with no
/// `..`-escape past the project root. Only valid in a `comptime` binding.
pub const BUILTIN_EMBED_FILE: &str = "embed_file";
pub const BUILTIN_EMBED_BYTES: &str = "embed_bytes";

/// S43 (ratified M6; PascalCase marker D-CASING1 follow-on 2026-06-21):
/// top-level test-declaration block, written as the marker `#Test("name") { … }`.
/// D-TESTPAREN1=A (ratified 2026-06-26): the name is now a parenthesized string
/// argument, matching the `#Caps(…)` / `#Grant(…)` marker family.
/// The bare lowercase `test` keyword (FOREIGN_TEST) is the retired spelling,
/// recognized only to emit the E0052 teaching error pointing at `#Test("name")`.
pub const KW_TEST: &str = "Test";

/// D-BENCH1 (ratified 2026-06-24): top-level region-benchmark block, written as
/// the marker `#Bench "name" { … }` — the exact sibling of `#Test "name" { … }`.
/// The existing `jet bench` verb (D-TOOL5) discovers and runs these, reporting
/// per-region ops/sec + ns/iter (today it times a whole program). PascalCase
/// marker per D-CASING1, joining the `#Test`/`@Pure`/`#Todo`/`#Caps` family.
/// The `benchmark` manifest target (TARGET_BENCHMARK, c80) points `jet bench`
/// at a package entry; it is not a new engine — it reuses this exact machinery.
pub const KW_BENCH: &str = "Bench";

/// D-DOTSCOPE1 (ratified 2026-07-02): scope-member vocabulary for `#Test`.
/// Inside a `#Test { … }` body a statement-position `.name { … }` /
/// `.name(args) { … }` (I8: the ONE spelling for scope vocabulary) resolves
/// against this list. `.setup` runs first (init region), `.expect_fail` marks
/// a region that must fail, `.timeout(dur)` bounds a region's elapsed time,
/// `.skip` skips a region (or the whole test when it is the first statement).
pub const SCOPE_TEST_SETUP: &str = "setup";
pub const SCOPE_TEST_EXPECT_FAIL: &str = "expect_fail";
pub const SCOPE_TEST_TIMEOUT: &str = "timeout";
pub const SCOPE_TEST_SKIP: &str = "skip";

/// D-DOTSCOPE1: recognized duration suffixes for `.timeout(<dur>)` and their
/// nanosecond multiplier. `.timeout` reads a bare unit literal directly, so its
/// accepted units are fixed here rather than resolved through a `#UnitFamily`
/// (D-UNITLIT1). Returns `None` for any other suffix. `u128` so the multiply
/// can't overflow before codegen narrows to `u64` nanos.
pub fn duration_suffix_nanos(suffix: &str) -> Option<u128> {
    match suffix {
        "ns" => Some(1),
        "us" | "µs" => Some(1_000),
        "ms" => Some(1_000_000),
        "s" | "sec" | "secs" => Some(1_000_000_000),
        _ => None,
    }
}

/// D-DOTSCOPE1: the scope-member vocabulary a `#Marker { }` block declares, or
/// `None` if the marker declares no members. Each marker that grows a member
/// vocabulary is added here (an API decision, not a syntax one — the `.name { }`
/// grammar is fixed). `#Test` is the only marker with members today; `#Bench`
/// (and every other block marker) declares none, so a member statement inside
/// it is rejected against this empty vocabulary.
pub fn scope_members(marker: &str) -> Option<&'static [&'static str]> {
    match marker {
        KW_TEST => Some(&[
            SCOPE_TEST_SETUP,
            SCOPE_TEST_EXPECT_FAIL,
            SCOPE_TEST_TIMEOUT,
            SCOPE_TEST_SKIP,
        ]),
        _ => None,
    }
}

/// D-TOOL2 (ratified 2026-06-17, E2-M11; PascalCase marker D-CASING1 follow-on
/// 2026-06-21): typed hole `#Todo` — compiles everywhere, panics at runtime with
/// file, line, and expected type. Bare lowercase `todo` (FOREIGN_TODO) is the
/// retired spelling → E0054 teaching error pointing at `#Todo`.
pub const KW_TODO: &str = "Todo";

/// S60 (ratified 2026-06-12; implemented E2-M16; PascalCase marker D-CASING1
/// follow-on 2026-06-21): the purity modifier, written as the marker `@Pure fn
/// name() { … }`. A `@Pure fn` may only call other `@Pure fn`s and pure
/// builtins; impure calls are a compile error (E3401) with the call-trace path.
/// Bare lowercase `pure` (FOREIGN_PURE) is the retired spelling → E0053 teaching
/// error pointing at `@Pure`.
///
/// D-EFF2 (ratified 2026-06-22): `@Pure` also rides the front of a callback
/// parameter type — `f: @Pure fn(T) -> U` demands a pure callback; passing one
/// with any effect is E0747. Sibling of the `#(E, …) fn(…)` bounded form.
pub const KW_PURE: &str = "Pure";

/// D-TAINT1 (ratified 2026-06-21, option A; gated on D-EFF1): the value-fact tag
/// that marks an untrusted value at its source — `#Tainted input`. The taint
/// **spreads** along dataflow (assignment, interpolation, field store, return,
/// arithmetic); a tainted value reaching a sink effect (`Db`/`Exec`/`Net`)
/// without passing through a `#Sanitizer fn` is E0721. A value fact, not a
/// declaration: it rides the value (D-QUAL1). PascalCase per D-CASING1 (the
/// ratified card's lowercase `#tainted` is normalized to the tag convention).
/// Static, erased in codegen (I3).
pub const KW_TAINTED: &str = "Tainted";

/// D-TAINT1: the `#Sanitizer fn name(…)` modifier — the one blessed way to strip
/// taint. A sanitizer's return value is untainted by contract, regardless of
/// whether its inputs were tainted (it is the audited cleaning step). A fn
/// modifier in the `@Pure`/`#Unsafe` family; PascalCase per D-CASING1. Erased in
/// codegen (I3). NOTE: the ratified card spells the modifier bare `sanitizer fn`;
/// the D-CASING1 marker convention (which moved `pure fn` → `@Pure fn`) makes
/// `#Sanitizer fn` the consistent default — a spelling fork queued as D-TAINT-SAN.
pub const KW_SANITIZER: &str = "Sanitizer";

/// D-STATE1 (ratified 2026-06-22, option A): the typestate **require-state** fn
/// modifier — `#State(Confirmed) fn check_in(self, …)`. Declares the method valid
/// only when its receiver is currently in state `Confirmed`. Calling it on a value
/// in any other state is E0150. The state is an ordinary `tag` (D-QUAL2); the
/// current state of a value is a compile-time fact threaded by forward dataflow,
/// erased in codegen (I3 — zero runtime cost). A paren-arg fn marker, parallel to
/// `#layout(c)` / `#UnitFamily(currency)`. The exact spelling is the implemented
/// default queued for owner confirmation as D-STATE-REQ.
pub const KW_STATE: &str = "State";

/// D-STATE1 (ratified 2026-06-22, option A): the typestate **transition** fn
/// modifier — `#Transition(Pending -> Confirmed) fn confirm(self) -> Reservation`.
/// Declares a function that consumes a value in state `Pending` and yields one in
/// state `Confirmed` (the ratified mechanism: "a fn takes the old state tag and
/// returns the next"). The from-state may be `_` for an **entry** transition (a
/// constructor that produces the initial state from nothing). Wrong from-state at a
/// call site is E0150; the call advances the receiver/result to the to-state. The
/// `->` inside reuses the return arrow. Tags erase (I3). Implemented default queued
/// for owner confirmation as D-STATE-TRANS.
pub const KW_TRANSITION: &str = "Transition";

/// D-STATE-DECL (ratified 2026-06-25, option B): the typestate **state-set
/// declaration** contextual keyword — `state TypeName { Pending, Confirmed, CheckedIn }`.
/// Declares the bounded set of states for a type, tied to the type by name. The set
/// erases at runtime (pure compile-time, no discriminant). A dead-end state (no
/// outgoing `#Transition`) is a warning (L0151). A state referenced in `#State(X)` or
/// `#Transition(A -> B)` that is not in the declared set is E0151. Contextual: the
/// word `state` stays usable as an ordinary identifier outside a top-level declaration
/// position (like `migration`). Declaration family sibling of `tag`/`struct`/`enum`.
pub const KW_STATE_DECL: &str = "state"; // D-STATE-DECL

/// D-PROTO1 / D-PROTO2 (ratified 2026-06-27, options A+A): the session/protocol
/// declaration contextual keyword — `protocol Name { client -> server: Msg(…) }`.
/// Declares an ordered request/response exchange once; sema expands it into
/// `#SingleUse` `.Client`/`.Server` handle types with typestate-checked send/recv
/// methods (out-of-order use = E0150). Contextual like `state`/`migration`.
pub const KW_PROTOCOL: &str = "protocol"; // D-PROTO1, D-PROTO2

/// D-PROTO2: endpoint labels in a protocol message line.
pub const PROTO_CLIENT: &str = "client"; // D-PROTO2
pub const PROTO_SERVER: &str = "server"; // D-PROTO2

/// D-STATE1: the entry-transition placeholder — `#Transition(_ -> Pending)` means
/// "from no prior state". Reuses the existing `_` wildcard glyph.
pub const STATE_ENTRY: &str = "_";

/// D-EFF1 / D-QUAL1 (ratified 2026-06-22): the effect-restriction region marker,
/// written `#Caps(Net, Db) { … }`. Inside the block, the body (and everything it
/// transitively calls) may use only the listed effects; an out-of-set effect is
/// E0741. PascalCase per D-CASING1. Erased in codegen (I3).
pub const KW_CAPS: &str = "Caps";

/// D-SCAP1 (ratified 2026-06-21): the scoped-capability grant marker, written
/// `#grant(Fs) { caps -> … }`. Grants (authorizes) the listed effect(s) inside
/// the block through the first-class handle bound after `{` (here `caps`), and
/// **revokes** the capability at scope end (RAII, S63) — the handle is bound only
/// for the block. The dual of `#Caps` (which restricts): an effect used inside
/// that the grant doesn't cover has no capability (E0712); letting the handle
/// escape is E0711. Erased in codegen (I3). PascalCase per D-MARKERCASE1=A.
pub const KW_GRANT: &str = "Grant";

/// D-SCAP1: the `->` token between the grant handle and the block body —
/// `#grant(Fs) { caps -> … }`.
pub const GRANT_ARROW: &str = "->";

/// D-SCAP1: the type of a capability handle bound by `#grant(…) { caps -> … }`.
/// An opaque sema-only handle (authority to perform the granted effects); erased
/// in codegen (I3). Mirrors `TXN_HANDLE_TYPE`.
pub const CAP_HANDLE_TYPE: &str = "Capability";

/// D-TASKSCOPE1=A / D-NURSERY1=A: the sema-only handle type bound by
/// `taskgroup g { … }`. Erased in codegen (I3); routes `g.task` / `g.all`.
pub const TYPE_TASKGROUP: &str = "TaskGroup";

/// D-TASKSCOPE1=A: scoped spawn method on a taskgroup handle — `g.task { … }`.
pub const TASKGROUP_SPAWN_METHOD: &str = "task";

/// D-NURSERY1=A: join every task handle in a list — `g.all([h1, h2])`.
pub const TASKGROUP_ALL_METHOD: &str = "all";

/// D-CONCCOMB1=A: first completed task wins — `g.race([h1, h2])`.
pub const TASKGROUP_RACE_METHOD: &str = "race";

/// D-CONCCOMB1=A: first completed result — `g.any([h1, h2])` (v1: same join race).
pub const TASKGROUP_ANY_METHOD: &str = "any";

/// D-CONCSELECT1=A: fluent scoped select — `g.select().recv(...).after(...).wait()?`.
pub const TASKGROUP_SELECT_METHOD: &str = "select";

/// D-CONCSELECT1=A: sema/codegen builder type for chained select arms.
pub const TYPE_SELECT_BUILDER: &str = "SelectBuilder";

/// D-CONCSELECT1=A: register a channel receive arm on a select builder.
pub const SELECT_RECV_METHOD: &str = "recv";

/// D-CONCSELECT1=A: register a timer arm — `.after(ms: N)`.
pub const SELECT_AFTER_METHOD: &str = "after";

/// D-CONCSELECT1=A: register a readable I/O arm — `.read(stream)`.
pub const SELECT_READ_METHOD: &str = "read";

/// D-CONCSELECT1=A: block until one arm wins — `.wait()`.
pub const SELECT_WAIT_METHOD: &str = "wait";

/// D-NURSERY1=A: wait for a task result (alias for `.join()` on `Task<T>`).
pub const METHOD_TASK_WAIT: &str = "wait";
/// D-COROUTINE1=A: mark a task paused in the control plane.
pub const METHOD_TASK_PAUSE: &str = "pause";
/// D-COROUTINE1=A: clear the paused marker in the control plane.
pub const METHOD_TASK_RESUME: &str = "resume";
/// D-COROUTINE1=A: request cancellation for a task in the control plane.
pub const METHOD_TASK_CANCEL: &str = "cancel";
/// D-COROUTINE1=A: inspect task control-plane state.
pub const METHOD_TASK_TRACE: &str = "trace";

/// D-TXN4 (ratified 2026-06-24): the transaction-block marker, written
/// `#Transact(order) { … }`. `order` binds a user-chosen transaction handle
/// (any lowercase ident, mirroring `region r { … }`). Inside the block an
/// irreversible effect (Net/Fs/Exec) is rejected (E0746, D-TXN2); the fix is to
/// move it after the block or register it on the handle via
/// `order.on_commit(() => { … })` (D-TXN3), which runs Drop-backed on a clean
/// commit. PascalCase per D-CASING1. Erased in codegen (I3).
pub const KW_TRANSACT: &str = "Transact";

/// D-TXN3 (ratified 2026-06-24): the post-commit hook method on a transaction
/// handle — `order.on_commit(() => { … })`. Drop-backed (D-DEFER1 model), runs
/// LIFO on a clean commit and is dropped (not run) on a `?`-failure/rollback.
/// NO new keyword (library form, I7 untouched).
pub const TXN_ON_COMMIT: &str = "on_commit";

/// D-TXN-ROLLBACK (ratified 2026-06-25, layer 3): the explicit rollback-hook
/// method on a transaction handle — `order.on_rollback(() => { … })`. The exact
/// mirror of `on_commit`: Drop-backed (D-DEFER1 model), runs LIFO on a
/// `?`-failure/rollback and is dropped (not run) on a clean commit. A value handled
/// by an explicit `on_rollback` is the author's to undo, so it is NOT auto-snapshot
/// (layer 1) — they took control and skip the perf cost. NO new keyword (library
/// form, I7 untouched).
pub const TXN_ON_ROLLBACK: &str = "on_rollback";

/// D-TXN-ROLLBACK (ratified 2026-06-25, layer 2): the trait a type may derive/impl
/// to customize how a mutated value is snapshotted and restored inside a `#Transact`
/// block (e.g. a cheap diff instead of a full deep copy). When a mutated value's
/// type implements `Rollback`, the auto-snapshot (layer 1) uses it instead of a
/// generic clone. A user-derivable trait name (I7).
pub const TRAIT_ROLLBACK: &str = "Rollback";

/// D-DISPLAYDBG1 / D-DISPLAY-SHAPE: user-facing string rendering for `{}` interpolation.
pub const TRAIT_DISPLAY: &str = "Display";
/// D-DISPLAYDBG1: developer-facing debug rendering for `{value@Debug}` interpolation.
pub const TRAIT_DEBUG: &str = "Debug";
/// D-ITER-HOOK: expert opt-in hook enabling zero-copy `for x in mytype`.
pub const TRAIT_ITERABLE: &str = "Iterable";
/// D-ITER-HOOK: cursor type for `Iterable::iter`.
pub const TRAIT_ITERATOR: &str = "Iterator";
/// D-INDEX-HOOK: expert opt-in hook enabling `mytype[key]` read syntax.
pub const TRAIT_INDEX: &str = "Index";
/// D-INDEX-HOOK: expert opt-in hook enabling `mytype[key] = v` write syntax.
pub const TRAIT_INDEX_MUT: &str = "IndexMut";
/// D-DISPLAYDBG2: closed interpolation selector spelling after `@`.
pub const INTERP_SELECTOR_DEBUG: &str = "Debug";
/// D-DEBUG-REDACT / D-MARKERMOVE1 (contract plane, `@Redact`): hide a field
/// from auto-derived Debug output.
pub const ATTR_REDACT: &str = "Redact";

/// D-TXN4: the type of a transaction handle bound by `#Transact(name)`. An
/// opaque sema-only handle; erased in codegen (I3).
pub const TXN_HANDLE_TYPE: &str = "Transaction";

/// S14 / D-CASING1 follow-on (2026-06-21): the retired lowercase spellings of
/// the three marker keywords, recognized only for teaching errors that point at
/// the `#Test` / `@Pure` / `#Todo` marker forms.
pub const FOREIGN_TEST: &str = "test";
pub const FOREIGN_PURE: &str = "pure";
pub const FOREIGN_TODO: &str = "todo";

/// D-TAINT-SAN (ratified 2026-06-25, option B): the taint-strip modifier is the
/// PascalCase marker `#Sanitizer fn`. Bare lowercase `sanitizer` in fn-modifier
/// position (`sanitizer fn …`) is the retired spelling, recognized only for the
/// teaching error E0059 that points at `#Sanitizer`. An ordinary identifier named
/// `sanitizer` elsewhere is unaffected.
pub const FOREIGN_SANITIZER: &str = "sanitizer";

/// D-LIN1-DROP (ratified 2026-06-25, option A): `drop(x)` is the deliberate
/// discard of a `#SingleUse` value. Legal ONLY inside an `#Unsafe("reason")`
/// region/fn — the `#Unsafe` reason IS the audit note (reuses D-UNSAFE2's audited
/// gate). It satisfies the single-use consume duty by moving the value to nowhere;
/// the value's Rust `Drop` runs. Outside an `#Unsafe` context it is E0143. Erased
/// to a plain `drop(x)` in codegen (I3 — no `unsafe` emitted). Shadowed by any
/// user-defined `drop` function or local.
pub const BUILTIN_DROP: &str = "drop";

/// D-TOOL4 (ratified 2026-06-16, E2-M11): snapshot testing builtin.
/// `expect(value).snapshot()` records or compares a golden snapshot.
pub const BUILTIN_EXPECT: &str = "expect";
pub const BUILTIN_SNAPSHOT: &str = "snapshot";

/// M4: synthetic name for a `switch` subject that isn't a plain identifier.
pub const KW_IT: &str = "it";

/// S42 (ratified M5): `as` recognized only for teaching error E0030.
pub const FOREIGN_AS: &str = "as";

/// S46 (M8): foreign anonymous-fn spellings for teaching error E0032.
pub const FOREIGN_LAMBDA: &str = "lambda";

/// S46 (M8): Rust pipe closures for teaching error E0033.
pub const FOREIGN_PIPE_CLOSURE: &str = "|";

/// S14 (M5): foreign collection spellings for teaching errors.
pub const FOREIGN_VEC: &str = "Vec";
pub const FOREIGN_HASHMAP: &str = "HashMap";
pub const FOREIGN_DICT: &str = "dict";
pub const FOREIGN_APPEND: &str = "append";

/// S14 (M4): foreign error spellings for teaching errors.
pub const FOREIGN_THROW: &str = "throw";
pub const FOREIGN_RAISE: &str = "raise";
pub const FOREIGN_CATCH: &str = "catch";
pub const FOREIGN_EXCEPT: &str = "except";
pub const FOREIGN_UNWRAP: &str = "unwrap";
pub const FOREIGN_EXPECT: &str = "expect";

/// M10 teaching spellings for common Core/library habits.
pub const FOREIGN_EPRINTLN: &str = "eprintln";
pub const FOREIGN_OPEN: &str = "open";
pub const FOREIGN_GETENV: &str = "getenv";
pub const FOREIGN_OS: &str = "os";

/// M11 teaching spellings: async/await and mutex/lock.
pub const FOREIGN_ASYNC: &str = "async";
pub const FOREIGN_AWAIT: &str = "await";
pub const FOREIGN_MUTEX: &str = "Mutex";
pub const FOREIGN_LOCK: &str = "lock";

/// D-ATTR1 (ratified 2026-06-19): attribute prefix sigil — `#Marker` / `#[a, b]`.
/// Replaces S82's `@` spelling. Loop labels keep `@` (D-ATTR3 = B).
pub const ATTR_PREFIX: &str = "#";

/// S82 (ratified 2026-06-16): multi-attribute list delimiters after `@`.
pub const ATTR_LIST_OPEN: &str = "[";
pub const ATTR_LIST_CLOSE: &str = "]";

/// D-ATTR1: rejected old `@` attribute spelling (teaching error). S82 reversed.
pub const FOREIGN_AT_ATTR: &str = "@";

/// S80 (ratified 2026-06-16): cross-type `?` conversion trait (D-ERR2).
pub const TRAIT_FALLIBLE: &str = "Fallible";

/// S80 (ratified 2026-06-16): `Fallible` method returning default `Error`.
pub const FN_TO_ERROR: &str = "to_error";

// S52's `MANIFEST_FILE`/`LOCK_FILE` (`jet.toml`/`jet.lock`) were retired in the
// manifest reshape chunk (U1/U2): the manifest is now `PAYLOAD_FILE`
// (`pkg.jet`, D-JPK-FILES — prior filename iterations retired) and
// the lockfile is `UNIFIED_LOCK_FILE` (`.jet/lock`). Clean break — no alias.

/// S52 (ratified M12): package source root directory inside a project.
pub const SOURCE_ROOT_DIR: &str = ".jet";

/// S52 (ratified M12): dependency kind table suffixes.
pub const DEP_TABLE_JET: &str = "dependencies";
pub const DEP_TABLE_RUST: &str = "dependencies:rust";

/// S59 / D-CFFI2 (ratified): the native-C-library dependency provider name,
/// written as a `provider@target` ref inside the `deps: { … }` block —
/// `lib: c@system` (pkg-config, with a bare `-l <lib>` fallback) or
/// `lib: c@"vendor/path"` (local dir: `-L`/`-I`/`-l`). Replaces the retired
/// TOML `[dependencies:c]` table. A C dep is a link dep, not a Jet package: it
/// is never realized as source or written to the package lock.
pub const DEP_PROVIDER_C: &str = "c";

/// S59 / D-CFFI2 (ratified): the `c@<target>` system-library target —
/// `lib: c@system` resolves via `pkg-config <lib>`, falling back to a bare
/// `-l <lib>` when there is no `.pc` (e.g. libc). Any other target is a local
/// directory path.
pub const SYSTEM_LIB_TARGET: &str = "system";

// ──────────────────────────────────────────────
// Jetpack (Phase 1) — user-typeable surface (I7).
// All decisions ratified in docs/spec/syntax-decisions.md (D-JPK*).
// These IDs start with `D`, so tests/decisions.rs leaves them alone, but
// I7 still wants every typeable token to live here with its decision ID.
// ──────────────────────────────────────────────

/// D-JPK1/9: the Jetpack package-manager binary name.
pub const JETPACK_BINARY_NAME: &str = "jetpack";

/// U1 (D-JPK20) / U10 / D-JPK-FILES: the Jet **package manifest** is `pkg.jet`
/// (`PAYLOAD_FILE`; Cargo.toml analog, replaces `jet.toml`). Prior filenames
/// (pack.jet, the U10 interim name) were retired (clean break, no alias).
/// `PACK_LOCK_FILE` is superseded by `.jet/lock` (U2/S52).
pub const PACK_LOCK_FILE: &str = "pack.lock";

/// D-JPK7/15: the `<source>:<package/path>` ref separator. Users never type
/// Nix's `#` selector; Jetpack translates `:` to the provider's form.
pub const REF_SEPARATOR: &str = ":";

/// D-JPK7/15: recognized ref source prefixes.
pub const REF_SOURCE_NIXPKGS: &str = "nixpkgs";
pub const REF_SOURCE_GITHUB: &str = "github";
pub const REF_SOURCE_PATH: &str = "path";

/// D-JPK2/9: the Phase 1 verb set.
pub const JETPACK_VERBS: &[&str] = &[
    "run",
    "enter",
    "build",
    "list",
    "hangar",
    "vendor",
    "audit",
    "clean",
    "add",
    "remove",
    "push",
    OS_SUBCOMMAND,
];

/// D-JPK-DISPATCH1=B (A1, card c9jetpackgates): `jet` execs the engine
/// binary (`jetpack`, later `jetos`) for every engine verb instead of linking
/// it in-process — git/kubectl-style dispatch by executable name. Before
/// exec-ing the real command, `jet` runs `<engine> --engine-protocol`, a
/// hidden handshake flag every engine binary answers with its own
/// `CARGO_PKG_VERSION` on stdout; a mismatch against `jet`'s own version is
/// E1227 (`engine-version-skew`). This is process-dispatch plumbing between
/// two binaries jet ships together, never a token a user writes in a `.jet`
/// file, so I7 (every user-typeable keyword lives here with a decision ID)
/// does not require it to gate a Tower ballot of its own — it is this
/// gate's own implementation surface.
pub const ENGINE_PROTOCOL_FLAG: &str = "--engine-protocol";

/// D-JPK14: the default visible prompt label inside a Jetpack shell.
pub const JETPACK_PROMPT_LABEL: &str = "jetpack";

/// D-JPK14: shell marker env var set inside a Jetpack shell.
pub const JETPACK_ENV_MARKER: &str = "JETPACK_ENV";

/// D-JPK3/17: the directive calls an `env.jet` author writes. `pkg.source`
/// takes one arg (default built-in source) or two (named source + upstream/pin,
/// D-JPK17). Packages reference named sources inline via `<name>:<package>`.
pub const PACK_DIRECTIVE_SOURCE: &str = "pkg.source";
pub const PACK_DIRECTIVE_PACKAGES: &str = "pkg.packages";
pub const PACK_DIRECTIVE_PROMPT: &str = "pkg.prompt";

// ──────────────────────────────────────────────
// Unified ecosystem (jet + jetpack + jetos) — user-typeable surface (I7).
// Owner-ratified design-of-record: tools/Tower/docs/plans/epoch-5/unified-ecosystem.md
// (U1–U7, ratified 2026-06-16). These IDs start with `U`, enforced by
// tests/decisions.rs alongside the S/N decisions. Tokens are recorded here;
// behavior lands in the Jetpack/Jetos implementation chunks (no syntax beyond
// what is ratified). The S52 amendment names (U1/U2) live with the S52 block.
// ──────────────────────────────────────────────

/// U3 (ratified 2026-06-16): module declaration keyword — `module name { … }`.
pub const KW_MODULE: &str = "module";

/// D-GENMOD2=A (ratified 2026-06-28): generic module parameter list uses `<…>`.
/// Type params: `K: Hash` (name starts uppercase; bound is a trait).
/// Value params: `capacity: Int` (name starts lowercase; annotation is a concrete type).
/// Instantiation: `module Alias = Module<TypeArg, value_arg>`.
/// Reuses existing `<`/`>` angle-bracket tokens (no new sigil, I7 satisfied).
pub const GENMOD_OPEN: &str = "<"; // reuses OP_LT
pub const GENMOD_CLOSE: &str = ">"; // reuses OP_GT

/// U3 (ratified 2026-06-16): a leading underscore on a module name disables it
/// (`module _name { … }` is not discovered or merged). One char, reversible.
pub const MODULE_DISABLE_PREFIX: &str = "_";

/// S84 (ratified 2026-06-16): *dashed names* — the kebab-case naming rule for
/// package / module / system / image / env **names** (and `from: system.<name>`
/// references). The grammar is `ident (-ident)*`: a `-` joins two segments only
/// when it is span-adjacent to both (no surrounding whitespace), matching
/// nixpkgs/npm package-name convention (e.g. `image.halcyon-iso`,
/// `system.my-host`). No new sigil — this reuses the existing `-`/Minus token;
/// span adjacency is what keeps a spaced `a - b` as subtraction, so the rule
/// never bleeds into the expression grammar. No leading, trailing, or doubled
/// hyphen. Code identifiers (variables, fields, types, functions) stay plain
/// `ident`. Enforced in `parser.rs::expect_dashed_name`.
pub const NAME_SEGMENT_SEP: &str = "-";

/// U3 (ratified 2026-06-16): reserved namespaces any module may contribute to.
pub const NS_ENV: &str = "env";
pub const NS_SYSTEM: &str = "system";
pub const NS_IMAGE: &str = "image";

/// D-WORKSPACE2 (ratified 2026-06-25, option A): the monorepo index is the
/// reserved namespace `workspace` — `module workspace { members: … }` in
/// `workspace.jet` (D-WORKSPACE1=B; see WORKSPACE_FILE). Owner kept the
/// industry-standard term over the aviation menu (`fleet`/`wing`/…). Not yet wired
/// (resolver rides board card c156).
pub const NS_WORKSPACE: &str = "workspace";

/// U15 (D-JPK-FLEET1=A, ratified 2026-07-02): a fleet is a map of named hosts to
/// `System` refs — `module fleet.<name> { hosts: { web1: system.<sys>.{ … } } }`.
/// Distinct from `workspace` (the monorepo index): a fleet is a deployment target.
/// Parse/capture/cross-check now; ssh realization rides single-host jetos (Phase D).
pub const NS_FLEET: &str = "fleet";

/// U3 (ratified 2026-06-16): the type matching each reserved namespace.
pub const TYPE_ENV: &str = "Env";
pub const TYPE_SYSTEM: &str = "System";
pub const TYPE_IMAGE: &str = "Image";
/// U15: the type name of a `fleet.<name>` contribution record.
pub const TYPE_FLEET: &str = "Fleet";

/// U15: a `Fleet`'s one required field — the `hosts:` map.
pub const FLEET_FIELD_HOSTS: &str = "hosts";

/// U12 (ratified 2026-06-16): the element type of a `System`'s `services:` map.
/// `Service` is not a top-level namespace (it never appears as `service.<name>:`);
/// it is the inferred type of each bare `{ … }` record written under `services:`.
pub const TYPE_SERVICE: &str = "Service";

/// U11 (ratified 2026-06-16): a `System`'s four fields —
/// `target` / `packages` / `services` / `options`. Anything else is unknown.
pub const SYSTEM_FIELD_TARGET: &str = "target";
pub const SYSTEM_FIELD_PACKAGES: &str = "packages";
pub const SYSTEM_FIELD_SERVICES: &str = "services";
pub const SYSTEM_FIELD_OPTIONS: &str = "options";

/// U12 (ratified 2026-06-16): the required first field of every `Service` record.
pub const SERVICE_FIELD_ENABLE: &str = "enable";

/// U13 (ratified 2026-06-16): the typed platform values a `System.target` (and a
/// cross-compile `Image.target`) may hold — `linux.x64` / `linux.arm64`. Written
/// as a dotted typed value (an OS namespace `.` an arch), never a quoted string.
pub const PLATFORM_OS_LINUX: &str = "linux";
pub const PLATFORM_ARCH_X64: &str = "x64";
pub const PLATFORM_ARCH_ARM64: &str = "arm64";

/// U14 (ratified 2026-06-16): an `Image`'s fields — required `from: system.<name>`
/// and optional `format:` (default `iso`). `target`/`packages`/`services`/
/// `options` are inherited from the referenced `System`, never restated (the lone
/// exception is an explicit cross-compile `target:`).
pub const IMAGE_FIELD_FROM: &str = "from";
pub const IMAGE_FIELD_FORMAT: &str = "format";

/// U14 (ratified 2026-06-16): the disk-image formats — `iso` (default) / `qcow` /
/// `raw`.
pub const IMAGE_FORMAT_ISO: &str = "iso";
pub const IMAGE_FORMAT_QCOW: &str = "qcow";
pub const IMAGE_FORMAT_RAW: &str = "raw";

/// U3 (ratified 2026-06-16): project environment file (`env` namespace) and the
/// master jetos system config (`system`/`image` namespaces, default dir ~/.jet/).
pub const ENV_FILE: &str = "env.jet";
pub const CONFIG_FILE: &str = "config.jet";

/// D-WORKSPACE1 (B) + D-WORKSPACE2 (A), ratified 2026-06-25: the monorepo index
/// is a `module workspace { members: … }` written in `workspace.jet`, parallel to
/// `env.jet`/`config.jet` — retiring the root `jetpack.toml` index so the whole
/// project is one grammar (Jet). `members:` may run arbitrary `comptime`
/// (D-WORKSPACE1=B). Wired by the resolver (board card c156). `NS_WORKSPACE` is
/// declared with the other reserved namespaces near `NS_ENV`.
pub const WORKSPACE_FILE: &str = "workspace.jet";
/// D-WORKSPACE1=B: the `members:` field in a workspace module — the comptime
/// expression that evaluates to the list of member package paths.
pub const MODULE_FIELD_MEMBERS: &str = "members";

/// U15 (ratified 2026-06-16): the jetos tier is the `jetpack os <verb>`
/// subcommand group — not a separate `jetos` binary, not under `jet`.
pub const OS_SUBCOMMAND: &str = "os";

/// U15 (ratified 2026-06-16): the jetos verbs, mirroring `nixos-rebuild` —
/// `switch` (build + activate + set boot default) and `build` (build only).
/// `boot`/`test` may be added later under the same protocol.
pub const OS_VERB_SWITCH: &str = "switch";
pub const OS_VERB_BUILD: &str = "build";
pub const OS_VERBS: &[&str] = &[OS_VERB_SWITCH, OS_VERB_BUILD];

/// c146 (D-PKGSIGN1, ratified): package-signing CLI verbs (I7). `jet keygen`
/// creates the Ed25519 author key; `jet key backup` copies the secret key out
/// for safekeeping. `jet publish` signs by default and takes `--no-sign`.
pub const KEYGEN_SUBCOMMAND: &str = "keygen";
pub const KEY_SUBCOMMAND: &str = "key";
pub const KEY_VERB_BACKUP: &str = "backup";
pub const KEY_VERBS: &[&str] = &[KEY_VERB_BACKUP];
pub const PUBLISH_FLAG_NO_SIGN: &str = "--no-sign";

/// U16 (ratified 2026-06-16): the `@host` selector in a `jetpack os` target
/// `[<config-path>]@<host>`. Reuses jet's `@` source-selector convention.
pub const OS_HOST_SELECTOR: &str = "@";

/// U16 (ratified 2026-06-16): the default config location when no explicit path
/// prefix is given — `~/.jet/config.jet`.
pub const CONFIG_DEFAULT_DIR: &str = ".jet";

/// U4 (ratified 2026-06-16): import-tree discovery builtin — `find("./modules")`
/// auto-discovers and merges every `.jet` module in the tree.
pub const BUILTIN_FIND: &str = "find";

/// U8 (ratified 2026-06-16): `sources:` and `imports:` are module-body fields,
/// nested inside `module name { … }` as siblings of the typed contributions
/// (`env.dev: Env { … }`) — not file top-level fields. Amends U4. `sources:`
/// holds `name: provider@target` entries; `imports:` holds `find(…)` directives.
pub const MODULE_FIELD_SOURCES: &str = "sources";
pub const MODULE_FIELD_IMPORTS: &str = "imports";

/// U6/U8: the conventional name of the default source (`sources: { default: … }`)
/// that bare packages and `default.ripgrep` sugar resolve against. Not a
/// reserved keyword — just the well-known name `jetpack` falls back to.
pub const DEFAULT_SOURCE: &str = "default";

/// U3/U8: the `Env` contribution field carrying the shell prompt label.
pub const ENV_FIELD_PROMPT: &str = "prompt";

/// U6 (ratified 2026-06-16): package value type, and the `provider@target`
/// source-ref separator (`github@owner/repo/rev`, `path@../local`, `nixpkgs@…`).
/// Provider names reuse REF_SOURCE_* (github / path / nixpkgs).
pub const TYPE_PKG: &str = "Pkg";
pub const REF_PROVIDER_AT: &str = "@";

/// U10 (ratified 2026-06-16; amends U1) / D-JPK-FILES (ratified 2026-06-18;
/// amends U10): the package manifest is `pkg.jet` (D-JPK-FILES rename; prior
/// interim names retired, clean break, no alias). A payload is a collection
/// of packages; its identity block is `payload: { … }`.
pub const PAYLOAD_FILE: &str = "pkg.jet";

/// D-JPK-FILENAME2=B (A2, card c9jetpackgates, ratified 2026-07-02): retired
/// manifest filenames from earlier reshapes of this same file (U1 `jet.toml`
/// -> U10 `pack.jet` -> D-JPK-FILES `pkg.jet`). Finding one of these instead
/// of `PAYLOAD_FILE` is E1226, not a silent fallback — D-JPK-FILENAME2
/// reconfirmed `pkg.jet` as final, so these never come back as aliases.
/// `jetpack.toml` is a *different*, still-live file (D-JPK-FILES repo
/// metadata: `[repo]`/`[sources]`) and does not belong on this list.
pub const STALE_MANIFEST_NAMES: &[&str] = &["pack.jet", "payload.jet", "jet.toml"];

/// U10 (ratified 2026-06-16): manifest identity block keyword — `payload: { name,
/// version, … }` (was `package:`).
pub const MANIFEST_BLOCK_PAYLOAD: &str = "payload";

/// U10 (ratified 2026-06-16): the block listing a payload's packages —
/// `packages: { name: kind }`. Each `name` is a top-level `module` (the package),
/// discovered by name in the tree; the old `exports: [module …]` folds into this.
pub const MANIFEST_BLOCK_PACKAGES: &str = "packages";

/// D-TGT1/D-TGT2 (ratified 2026-06-21): a package's build targets, replacing the
/// removed `kind:` (U10). The five shipped targets — `library` is imported for its
/// code, `executable` installs a binary on PATH, `test`/`example` build their own
/// artifacts, `benchmark` (c80, D-TGT2) points `jet bench` at the package entry.
/// Written as a bare keyword (`deploy: executable`, D-TGT3) or inside a
/// `{ targets: [ … ] }` list.
pub const TARGET_LIBRARY: &str = "library";
pub const TARGET_EXECUTABLE: &str = "executable";
pub const TARGET_TEST: &str = "test";
pub const TARGET_EXAMPLE: &str = "example";
/// D-TGT2 / c80 (ratified 2026-06-21; backend shipped 2026-06-25): the manifest
/// target that routes `jet bench` at the package entry — same engine as `#Bench`/
/// `jet bench file.jet`, now addressable from a `packages:` declaration.
pub const TARGET_BENCHMARK: &str = "benchmark";

/// D-TGT2 (ratified 2026-06-21): target keywords reserved for a future increment —
/// recognized but rejected (no backend yet) until their tooling lands.
/// `benchmark` shipped (c80); only `plugin` remains reserved (c81/D-DEP-WASM1).
pub const TARGET_RESERVED: &[&str] = &["plugin"];

/// D-TGT1 (ratified 2026-06-21): the per-package field listing build targets —
/// `app: { targets: [library, executable { entry: "src/cli.jet" }] }`. A bare
/// keyword value (`app: library`) is the single-target shorthand (D-TGT3). The
/// former `kind:` field is removed; using it is a teaching error (E1211).
pub const PACKAGE_FIELD_TARGETS: &str = "targets";

/// D-TGT1 (ratified 2026-06-21): the removed per-package kind field. Recognized
/// only to emit a migration teaching error pointing at `targets:`.
pub const PACKAGE_FIELD_KIND_REMOVED: &str = "kind";

/// D-TGT3/D-TGT4/D-CAP4 (ratified 2026-06-21): fields a target block may carry —
/// `entry:` (D-TGT4 entry module), `name:` (output/bin name), `api:` (D-CAP4
/// capability mode). Parsed when present; behavior lands with the realize pipeline.
pub const TARGET_FIELD_ENTRY: &str = "entry";
pub const TARGET_FIELD_NAME: &str = "name";
pub const TARGET_FIELD_API: &str = "api";

/// D-CAP4/D-CAP6 (ratified 2026-06-21): library capability-API modes. Default is
/// inference (no `api:`); `stable` records signatures + flags breaks, `explicit`
/// requires hand-written capability annotations on every `pub` signature.
pub const API_MODE_STABLE: &str = "stable";
pub const API_MODE_EXPLICIT: &str = "explicit";

/// D-CAP1 (ratified 2026-06-21): the four-capability vocabulary —
/// `view`/`edit`/`take`/`share`. `view` and `take` are ratified ownership keywords
/// (S10); `edit` and `share` are reserved here. Parameter-position placement
/// (D-CAP3) and the copy/share call form (D-CAP2) are still open, so these are
/// reserved spellings only — not yet wired into the parser.
pub const CAPABILITY_EDIT: &str = "edit";
pub const CAPABILITY_SHARE: &str = "share";

/// D-REL3 (ratified 2026-06-16): the project compatibility marker —
/// `edition: "2026"` in the `payload: { … }` block of `pkg.jet`. An edition
/// opts a project into a specific era of Jet syntax; a toolchain advertises the
/// editions it supports and rejects a future edition it can't provide (E2001).
/// Single-file `jet run file.jet` has no edition marker and always uses the
/// newest stable edition (E2-V4). Not an `S`/`N`/`U` surface decision, so it is
/// not enforced by tests/decisions.rs; it is a release-policy key recorded here
/// per I7.
pub const MANIFEST_FIELD_EDITION: &str = "edition";

/// D-RINGLAYER1=A: optional package runtime-layer ceiling in `payload: { … }`.
pub const MANIFEST_FIELD_LAYER: &str = "layer";

pub use crate::RingLayer::{
    core_module_layer, core_usage_layer, layer_ceiling_exceeded, RuntimeLayer,
};
pub use crate::WebPartition::{
    is_abi_safe_type, web_abi_type, web_cross_partition, web_target_browser, WebBucket,
    WebPartitionMarker,
};
pub use crate::OsTarget::{
    os_target_build_context, os_target_dispatch_arm, os_target_dispatch_exhaustive,
    os_target_mixed_axis, os_target_unmatched_call, OsTarget,
};

/// S52 (ratified M12; amended 2026-06-16, U2): the unified single lockfile lives
/// inside the `.jet/` managed folder (SOURCE_ROOT_DIR). Replaces `jet.lock`
/// (and `pack.lock`); the manifest reshape chunk migrates the old paths.
pub const UNIFIED_LOCK_FILE: &str = ".jet/lock";

/// S52 (ratified M12; amended 2026-06-16, U2): the single shared, content-
/// addressed store ("hangar"), global and never relocated.
pub const HANGAR_DIR: &str = "/etc/jet/hangar";

/// D-JPK-FILES (ratified 2026-06-18): repo metadata and source defaults at
/// repo root. TOML format; holds `[repo]` and `[sources]`. D-WORKSPACE1 moved
/// the old `[packages]` monorepo index to `workspace.jet`.
pub const JETPACK_TOML: &str = "jetpack.toml";

/// D-JPK-FILES (ratified 2026-06-18): `[repo]` table in `jetpack.toml`.
pub const JTOML_TABLE_REPO: &str = "repo";

/// D-JPK-FILES (ratified 2026-06-18): `[sources]` table in `jetpack.toml` —
/// named source refs (`name = "provider@target#ver"`).
pub const JTOML_TABLE_SOURCES: &str = "sources";

/// D-WORKSPACE1: retired `[packages]` table in `jetpack.toml`. Kept only so
/// the parser can emit E1225 with a targeted migration hint.
pub const JTOML_TABLE_PACKAGES: &str = "packages";

/// D-JPK-FILES (ratified 2026-06-18): `name` key in `[repo]`.
pub const JTOML_KEY_NAME: &str = "name";

/// D-JPK-FILES (ratified 2026-06-18): `version` key in `[repo]`.
pub const JTOML_KEY_VERSION: &str = "version";

/// D-DOTCTOR1 (ratified 2026-06-25): named struct construction `Type.{ field: val }`.
/// The dot immediately before `{` is the canonical construction sigil.
/// Inferred form (type from context): `.{ field: val }` — leading dot with no type name.
/// Both are parser-level adjacency of `.` and `{`; the lexer emits no dedicated token.
/// The old dotless `Type { … }` form is teaching error E0320, auto-fixed by `jet fmt`.
/// D-UITREE1 (ratified 2026-06-30): the same sigil also constructs a named-payload
/// enum variant — `.Variant.{ field: val }` / `Type.Variant.{ field: val }` (S30
/// multi-field variants). No new token; `enum_lit_named_fields` in
/// `Parser/Expressions.rs` reuses this `.{` adjacency after a leading-dot variant name.
pub const OP_NAMED_CTOR: &str = ".{";

/// S75 (ratified 2026-06-16): the fan-out operator — `f.[a, b, c]` desugars to
/// `[f(a), f(b), f(c)]`. `.[` is a parser-level adjacency of `.` and `[`;
/// there is no dedicated two-character lexer token (the parser detects `.`
/// immediately followed by `[`). This constant documents the user-visible sigil.
pub const OP_FAN_OUT: &str = ".[";

/// D-VARIADIC1 (ratified 2026-06-27): spread/rest sigil — `name: ...T` variadic
/// parameters (last position only), `f(...xs)` call spread, `[...a, x, ...b]` list spread.
pub const SIGIL_SPREAD: &str = "...";

/// S76 (ratified 2026-06-16): the fixed-size separator in `[T#N]` type
/// position, e.g. `[Int#3]`. Amended by VERSION-# (2026-06-16): `#` also
/// introduces pinned version numbers in package references (`pkg#1.2.0`).
/// Same token, two contexts: `[T#N]` is the type-level size form; `name#ver`
/// is the package version-pin form. No dedicated two-character token in either
/// case — the parser resolves by position.
pub const TYPE_FIXED_SIZE_SEP: &str = "#";

/// D-DIST1 (ratified 2026-06-19): `UserId :: distinct Int` — declares a
/// distinct type (a separate nominal type sharing the base's representation).
/// Used in the value position of a `::` immutable binding at item level;
/// `distinct`-over-`distinct` chaining is rejected in v1.
pub const KW_DISTINCT: &str = "distinct";

/// D-DIST3 (ratified 2026-06-20): unwrap method for a distinct type —
/// `value.raw()` yields the base value. Named-cast family (S42).
pub const METHOD_DISTINCT_RAW: &str = "raw";

/// D-DYNARRAY1 (ratified 2026-07-01): zero-copy window constructor —
/// `list.view(a..b)` — the sole legal spelling of the `View<T>` constructor
/// (parsed specially: the `..` between the two ends is required, a
/// comma-separated arg list is rejected so there is exactly one way to write
/// it, I8). See `docs/spec/stdlib-api-laws.md` and `View<T>` in CoreLib.
pub const METHOD_VIEW: &str = "view";

/// D-SHIFT1 (ratified 2026-07-01, c7shift): `cursor.take_pattern("…")` reuses
/// the D-PARSESTR1 interpolation-literal-as-pattern grammar (`{hole}` /
/// `{hole:Type}`), but a typed hole isn't a legal ordinary interpolation
/// value expression, so the string-literal argument in THIS ONE call
/// position is parsed as a pattern (`try_str_match_pattern`'s engine, reused
/// not duplicated — I8) instead of an ordinary `Expr::Str`. Parsed specially
/// the same way `.view(a..b)` (above) is: one method name, one fixed
/// argument shape, no second call-argument grammar.
pub const METHOD_TAKE_PATTERN: &str = "take_pattern";

/// D-DIST3 / D-CAPBUNDLE1 / D-MARKERMOVE1 (ratified): `@Numeric` marker
/// enables same-type arithmetic on a distinct type. Written `@Numeric` on
/// the same line before the distinct-type name (contract-plane prefix,
/// D-MARKER-FAMILY1). This is the single merged spelling for what used to
/// be two markers doing the same job — the `@Numeric` distinct-type marker
/// (D-DIST3) and the `@numeric` capability bundle (D-CAPBUNDLE1) — folded
/// per D-MARKERMOVE1=B (I8: one way to mean it). `CONTRACT_BUNDLE_NUMERIC`
/// no longer exists as a separate constant; use this one.
pub const ATTR_NUMERIC: &str = "Numeric";

/// D-QUAL3 (ratified 2026-06-24): `#UnitFamily(currency) { usd, eur, gbp }` —
/// declares a family of units. Each member mints one distinct `@Numeric` type
/// (`usd` → `Usd`) that erases to `Float`, so signatures read plain English
/// (`fn subtotal(price: Usd, qty: Int) -> Usd`). The family is the
/// "upgrade to D-DIST2" framing of D-UNIT1: sugar over the distinct-type
/// machinery (D-DIST1/D-DIST3). PascalCase tag per D-CASING1.
pub const ATTR_UNIT_FAMILY: &str = "UnitFamily";

/// D-MIGRATE1 (ratified 2026-06-22): `@PublishedSchema` — marks a struct whose
/// field layout is snapshotted at release time. A breaking field change without
/// a declared migration is E0910. Written `@PublishedSchema` before `struct`.
pub const ATTR_PUBLISHED_SCHEMA: &str = "PublishedSchema"; // D-MIGRATE1

/// D-LIN1 (ratified 2026-06-21, option A; gated on D-QUAL2): `#SingleUse` — marks
/// a type whose values must be consumed exactly once on every reachable path
/// (moved to a `^` parameter or returned). Using one zero times is E0140
/// (unconsumed at scope end) / E0141 (unconsumed on one branch); aliasing one
/// with `&`/`view` is E0142. `#SingleUse` implies `#NoCopy`. The tag is
/// compile-time only and erases in codegen (I3). Written `#SingleUse` before the
/// `struct`/`enum`, same marker idiom as `@PublishedSchema`.
pub const ATTR_SINGLE_USE: &str = "SingleUse"; // D-LIN1

/// D-MUSTUSE1 (c18iwxqx): `@MustUse` — marks a type, function, or method whose
/// result cannot be silently ignored as a bare expression statement (E0419).
/// Explicit discard uses `.drop("reason")` or `#Suppress(MustUse) { … }`
/// (D-IGNORERET2). Compile-time only; erases in codegen (I3).
pub const ATTR_MUST_USE: &str = "MustUse"; // D-MUSTUSE1

/// D-MIGRATE1 (ratified 2026-06-22): contextual keyword `migration` — introduces
/// a migration block that declares how a `@PublishedSchema` struct changed between
/// releases. Used as `migration TypeName { rename old -> new }`.
pub const KW_MIGRATION: &str = "migration"; // D-MIGRATE1

/// D-MIGRATE1 (ratified 2026-06-22): contextual keyword `rename` inside a
/// `migration { }` block — declares that field `old` was renamed to `new`.
pub const KW_RENAME: &str = "rename"; // D-MIGRATE1

/// D-MIGRATE2A (ratified): `add f: T = default` inside a `migration { }` block —
/// declares a new field plus the default old records are read with.
pub const KW_ADD: &str = "add"; // D-MIGRATE2

/// D-MIGRATE2D (ratified): `remove f` inside a `migration { }` block — deletes a
/// field. The verb is `remove`, NOT `drop` (a `drop` op is taught back to this).
pub const KW_REMOVE: &str = "remove"; // D-MIGRATE2

/// D-MIGRATE2E (ratified): `change f: Old -> New [via { … }]` inside a
/// `migration { }` block — a field type change with an optional inline converter.
pub const KW_CHANGE: &str = "change"; // D-MIGRATE2

/// D-MIGRATE2E (ratified): the `via { expr }` clause that supplies the inline
/// converter for a `change` op (`change price: Int -> Usd via { (c) => Usd(c) }`).
///
/// D-EFF2 (ratified 2026-06-22): also the pass-through marker in a `#(via f)`
/// signature annotation — a function whose published effect set IS whatever the
/// callback parameter `f` carries (a tight pass-through that holds when the value
/// escapes, vs. the conservative flow-through default). Erased in codegen (I3).
pub const KW_VIA: &str = "via"; // D-MIGRATE2 / D-EFF2

/// D-MIGRATE2F (ratified): the rejected `reorder` verb — field reordering is not
/// a tracked breaking change and needs no migration. Kept only to teach.
pub const KW_REORDER_RETIRED: &str = "reorder"; // D-MIGRATE2F

/// D-MIGRATE1: the `drop` verb a user might reach for instead of `remove` — kept
/// only to emit the E0911 teaching error pointing at `remove`.
pub const KW_DROP_RETIRED: &str = "drop"; // D-MIGRATE2D

/// D-MIGRATE2C (ratified): `jet schema` subcommand and its verbs. `status`
/// reports each `@PublishedSchema` type's pinned shape; `squash --before <ver>`
/// re-baselines snapshots to the current shape. There is NO `check` verb —
/// `jet build`'s E0910 is already the CI gate.
pub const CMD_SCHEMA: &str = "schema"; // D-MIGRATE2C
pub const SCHEMA_VERB_STATUS: &str = "status"; // D-MIGRATE2C
pub const SCHEMA_VERB_SQUASH: &str = "squash"; // D-MIGRATE2C

/// D-DBG1 (ratified 2026-06-19 = A): `jet debug <file>` — the dedicated
/// source-level debugger entry verb, parallel to `jet run`/`jet test`. The
/// editor launches the same command.
pub const CMD_DEBUG: &str = "debug"; // D-DBG1

/// D-DBG3 (ratified 2026-06-24 = A): the in-session `jet debug` command
/// surface. The prompt is `(jet)`; the step verbs are lldb-familiar with
/// single-letter aliases. Only Jet frames/lines/safe-locals are shown by
/// default (I2; D-DBG2 `--raw-frames` is the expert carve-out).
pub const DBG_PROMPT: &str = "(jet)"; // D-DBG3
pub const DBG_STEP: &str = "step"; // D-DBG3 (alias `s`)
pub const DBG_NEXT: &str = "next"; // D-DBG3 (alias `n`)
pub const DBG_CONTINUE: &str = "continue"; // D-DBG3 (alias `c`)
pub const DBG_FINISH: &str = "finish"; // D-DBG3 (alias `f`)
pub const DBG_BREAK: &str = "break"; // D-DBG3 (alias `b`): set a line breakpoint
pub const DBG_LIST: &str = "list"; // D-DBG3 (alias `l`): show source around `here`
pub const DBG_PRINT: &str = "print"; // D-DBG3 (alias `p`): show one local's value
pub const DBG_LOCALS: &str = "locals"; // D-DBG3: dump all locals in the frame
pub const DBG_BACKTRACE: &str = "backtrace"; // D-DBG3 (alias `bt`): the Jet call stack
pub const DBG_HELP: &str = "help"; // D-DBG3 (alias `h`): list the verbs
pub const DBG_QUIT: &str = "quit"; // D-DBG3 (alias `q`): end the session (E2204)

/// D-MIGRATE1 (ratified 2026-06-22): subdirectory under the project `.jet/`
/// managed folder where schema snapshots are stored. Full path is
/// `<project_root>/.jet/cache/schema/<TypeName>.snapshot`.
pub const SCHEMA_CACHE_SUBDIR: &str = "cache/schema"; // D-MIGRATE1

/// c129 (D-CAP4/D-CAP6/D-CAP8): subdirectory under the project `.jet/` managed
/// folder where frozen public-API capability snapshots are stored. Full path is
/// `<project_root>/.jet/cache/api/<package>.api`. Written at `jet publish` time
/// for an `api: stable|explicit` library target; read by the sema drift pass
/// (E0912). Committed — it is a durable interface contract, not a build artifact.
pub const API_CACHE_SUBDIR: &str = "cache/api";

/// D-DETACH1 (ratified; D-DETACH1 = A): consumes a Task handle without joining
/// — fire-and-forget daemon semantics. Main may return while the task runs.
/// Only valid on owned tasks; capturing a `view` borrow is rejected at spawn
/// time (E1102) and flagged again at the detach site (E1103).
pub const TASK_DETACH: &str = "detach"; // D-DETACH1

/// D-REPRC1 (ratified; D-REPRC1 = B): `#Layout(…)` struct attribute — controls
/// the memory layout of the generated Rust struct. `#Layout(c)` stamps
/// `#[repr(C)]` for C interop. Field order is preserved as written.
/// Growable fields (`[T]`, `Map`, `String`) are rejected (E1104).
/// PascalCase per D-MARKERCASE1=A.
pub const ATTR_LAYOUT: &str = "Layout"; // D-REPRC1 / D-MARKERCASE1
/// D-REPRC1: the C-compatible layout variant — `#layout(c)` → `#[repr(C)]`.
pub const LAYOUT_C: &str = "c"; // D-REPRC1
/// D-REPRC1: reserved layout variants — parse-and-error until their milestones ship.
pub const LAYOUT_PACKED: &str = "packed"; // D-REPRC1 (reserved)
pub const LAYOUT_ALIGN: &str = "align"; // D-REPRC1 (reserved)
/// D-SOA1 / D-SOA2A=C (implemented): the struct-of-arrays layout variant —
/// `#layout(columnar) struct S` stores a `[S]` collection column-per-field.
/// Whole-struct only in v1 (D-SOA2B); the partial form `#layout(columnar: …)`
/// is rejected (E1109) and the per-container prefix `columnar [T]` is reserved
/// (D-SOA2C, E1107).
pub const LAYOUT_COLUMNAR: &str = "columnar"; // D-SOA1 / D-SOA2A

// ── Serde derive markers + attributes (D-SERDE2–8, D-ENC1; bracket form D-ATTR2) ──
// Derive markers (PascalCase per D-CASING1, written `@[…]` before a struct/enum,
// D-MARKERMOVE1=B): `@[Codable]` derives BOTH directions (sugar for `@[Encode,
// Decode]`); `@[Encode]` is write-only; `@[Decode]` is read-only. Owner (D-SERDE4 = B,
// modified): the
// collapsed umbrella is `Codable`, with `Encode`/`Decode` as the one-way markers.
pub const ATTR_CODABLE: &str = "Codable"; // D-SERDE4
pub const ATTR_ENCODE: &str = "Encode"; // D-SERDE4
pub const ATTR_DECODE: &str = "Decode"; // D-SERDE4
                                        // D-MARKERMOVE3 (B, ratified 2026-07-02): the other built-in derive markers
                                        // that join Codable/Encode/Decode on the contract plane (`@`). `TRAIT_DEBUG`
                                        // ("Debug") is reused as the auto-derive marker name. User derives
                                        // (`derive T.Wire { … }`, applied as `#[Wire]`) stay `#` — the built-in/user
                                        // line is the `@`/`#` plane line.
pub const ATTR_SUMMARIZE: &str = "Summarize"; // D-MARKERMOVE3
pub const ATTR_COMPARABLE: &str = "Comparable"; // D-MARKERMOVE3
                                                // Per-field attributes (D-SERDE5 = A), written `#[…]` before a field.
pub const ATTR_RENAME: &str = "Rename"; // D-SERDE5  #[Rename("wire_key")]
pub const ATTR_SKIP: &str = "Skip"; // D-SERDE5  #[Skip]
pub const ATTR_DEFAULT: &str = "Default"; // D-SERDE5  #[Default] / #[Default(expr)]
pub const ATTR_FLATTEN: &str = "Flatten"; // D-SERDE5  #[Flatten]
                                          // Container attributes (D-SERDE3/7/8), written `#[…]` before a struct/enum.
pub const ATTR_RENAME_ALL: &str = "RenameAll"; // D-SERDE3  #[RenameAll(camel)]
pub const ATTR_DENY_UNKNOWN_FIELDS: &str = "DenyUnknownFields"; // D-SERDE8
pub const ATTR_TAG: &str = "Tag"; // D-SERDE7  #[Tag("type")] internal tagging
pub const ATTR_UNTAGGED: &str = "Untagged"; // D-SERDE7  #[Untagged]
                                            // D-SERDE3 (= C) RenameAll casing keywords — closed typed menu, own-case args.
pub const RENAME_ALL_CAMEL: &str = "camel"; // D-SERDE3
pub const RENAME_ALL_SNAKE: &str = "snake"; // D-SERDE3
pub const RENAME_ALL_PASCAL: &str = "pascal"; // D-SERDE3
pub const RENAME_ALL_KEBAB: &str = "kebab"; // D-SERDE3
pub const RENAME_ALL_SCREAMING: &str = "screaming"; // D-SERDE3

// ── Maturity tags (D-MATURITY1=B, ratified 2026-06-28) ──────────────────────
// Doc-convention markers; parser accepts+ignores them before `fn`/`pub fn`.
// No sema propagation; no codegen effect. I7: registered here so the LSP and
// formatter recognise them as valid items.
pub const ATTR_EXPERIMENTAL: &str = "Experimental"; // D-MATURITY1
pub const ATTR_TESTED: &str = "Tested"; // D-MATURITY1
pub const ATTR_HARDENED: &str = "Hardened"; // D-MATURITY1

// ── Explicit discard (D-IGNORERET2=A, ratified 2026-06-28) ──────────────────
// `.drop("reason")` — method-style terminal that silences E0402 for a fallible
// or @MustUse result.  `#Suppress(MustUse) { … }` is the lexical-scope form.
pub const METHOD_DROP: &str = "drop"; // D-IGNORERET2 (method form; distinct from BUILTIN_DROP fn)
pub const ATTR_SUPPRESS: &str = "Suppress"; // D-IGNORERET2  #Suppress(MustUse)
pub const SUPPRESS_MUST_USE: &str = "MustUse"; // D-IGNORERET2  argument of #Suppress

// ──────────────────────────────────────────────────────────────────────────────
// Canonical keyword/type/builtin tables (c44: single source of truth).
//
// These slices are the authoritative lists for the LSP, formatter, TextMate
// grammar, and any other consumer that needs to enumerate Jet's keyword surface.
// Add a word here (with its decision ID) rather than in each consumer.
//
// Rules for each list:
//   JET_KEYWORD_LIST  — real Jet keywords a user can type; FOREIGN_* teaching
//                       words must NOT appear here.
//   JET_TYPE_LIST     — built-in primitive / collection type names for completion
//                       and rename-guard. Only types a user writes in source.
//   IMPURE_BUILTINS   — bare-name builtins that write to I/O or read from stdin;
//                       used by both Sema/Purity and Comptime/Purity.
// ──────────────────────────────────────────────────────────────────────────────

/// Canonical list of real Jet keywords for LSP completion and rename validation.
///
/// Every entry corresponds to a `KW_*`, `LIT_*`, or `BUILTIN_*` constant above.
/// FOREIGN_* teaching-error words must NOT appear here — they are recognized only
/// to emit a diagnostic, not valid syntax.
pub const JET_KEYWORD_LIST: &[&str] = &[
    // Core structure (S1, S18, D-VISDEFAULT2, S16, S50, U3)
    KW_FN,
    KW_PUB,
    KW_PRIV,
    KW_USE,
    KW_EXTERN,
    KW_MODULE,
    // Control flow (M1, S19, S23, M1/M2)
    KW_IF,
    KW_ELSE,
    KW_LOOP,
    KW_IN,
    KW_BREAK,
    KW_CONTINUE,
    KW_RETURN,
    // Types and declarations (M2, S30, S27, M2, S28, S55, S57, D-DIST1)
    KW_STRUCT,
    KW_ENUM,
    KW_ALIAS,
    KW_IMPL,
    KW_TRAIT,
    KW_TAG,
    KW_DERIVE,
    KW_CONST,
    KW_COMPTIME,
    KW_DISTINCT,
    // Schema migrations (D-MIGRATE1 / D-MIGRATE2)
    KW_MIGRATION,
    KW_RENAME,
    KW_ADD,
    KW_REMOVE,
    KW_CHANGE,
    KW_VIA,
    // Ownership / borrow keywords (S10, M2). D-CAP7 retired KW_MUTATE/KW_MOVE/
    // KW_VIEW in favor of the `~`/`^`/`&` sigils — they live only as teaching
    // errors (E0056/E0057/E0058) now, so they are NOT in the keyword list.
    KW_STORED,
    KW_SELF,
    // Memory / expert tier (S58, D-REGION1, D-CTX1, D-TERM1, D-CTEFFECT1)
    KW_UNSAFE,
    KW_IMPURE,
    KW_REGION,
    KW_TASKGROUP,
    CTX_BLOCK,
    KW_LIVE,
    // Determinism escape (D-DET1): `assume_deterministic { … }`
    KW_ASSUME_DET,
    // Transactions (D-TXN1–D-TXN4): `#Transact(name) { … }`
    KW_TRANSACT,
    // Test / tooling (S43, S60, D-TOOL2, D-BENCH1)
    KW_TEST,
    KW_BENCH,
    KW_PURE,
    KW_TODO,
    // Taint tracking (D-TAINT1): value-fact tag + sanitizer modifier
    KW_TAINTED,
    KW_SANITIZER,
    // Typestate (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS)
    KW_STATE,
    KW_TRANSITION,
    KW_STATE_DECL,
    // Session/protocol types (D-PROTO1 / D-PROTO2)
    KW_PROTOCOL,
    PROTO_CLIENT,
    PROTO_SERVER,
    // Literals: boolean (S11), option (S32), result (S34), synthetic (M4)
    LIT_TRUE,
    LIT_FALSE,
    LIT_NULL,
    LIT_OK,
    LIT_ERR,
    KW_IT,
    // Binding sigils (SIGIL_BIND_IMMUT / SIGIL_BIND_MUT) are not words; omitted.
];

/// Canonical list of built-in type names for LSP completion and rename guard.
///
/// Only types a user writes in source. `Result` is the legacy fallible type
/// (S34) kept for teaching errors; it is intentionally excluded here since
/// `T ? E` is the current spelling.
pub const JET_TYPE_LIST: &[&str] = &[
    TYPE_INT,
    TYPE_FLOAT,
    TYPE_BOOL,
    TYPE_STRING,
    TYPE_CHAR,
    TYPE_LIST,
    TYPE_MAP,
    TYPE_SHARED,
    TYPE_I8,
    TYPE_I16,
    TYPE_I32,
    TYPE_I64,
    TYPE_U8,
    TYPE_U16,
    TYPE_U32,
    TYPE_U64,
    TYPE_F32,
    TYPE_F64,
];

/// D-BUILDPROFILE1 (ratified 2026-06-25): the `build { }` block in `pkg.jet`
/// where named build profiles are defined. A profile is a `Build.{ optimize: … }`
/// value; blessed names `release`/`debug` carry built-in defaults; all others
/// must be declared here. Active profile is chosen by `--release` (sugar for
/// `--profile=release`) or `--profile=<name>` — never by ambient environment.
pub const MANIFEST_BLOCK_BUILD: &str = "build"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: the constructor type for a build profile value inside
/// `pkg.jet`'s `build { }` block — written as `Build.{ optimize: … }`.
pub const BUILD_CTOR: &str = "Build"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: the field inside a `Build.{ … }` profile value that sets
/// the optimization level.
pub const BUILD_FIELD_OPTIMIZE: &str = "optimize"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: blessed profile names — `release`, `debug`, and `ci`
/// carry built-in defaults and need no explicit declaration in `build { }`.
pub const BUILD_PROFILE_RELEASE: &str = "release"; // D-BUILDPROFILE1
pub const BUILD_PROFILE_DEBUG: &str = "debug"; // D-BUILDPROFILE1
pub const BUILD_PROFILE_CI: &str = "ci"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: optional fields inside `Build.{ … }` profile values.
pub const BUILD_FIELD_DEBUG_INFO: &str = "debug_info"; // D-BUILDPROFILE1
pub const BUILD_FIELD_SMALL: &str = "small"; // D-BUILDPROFILE1
pub const BUILD_FIELD_PANIC: &str = "panic"; // D-BUILDPROFILE1
pub const BUILD_FIELD_FEATURES: &str = "features"; // D-BUILDPROFILE1
pub const BUILD_FIELD_ENV: &str = "env"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: `panic:` values for `Build.{ panic: … }`.
pub const BUILD_PANIC_ABORT: &str = "abort"; // D-BUILDPROFILE1
pub const BUILD_PANIC_UNWIND: &str = "unwind"; // D-BUILDPROFILE1

/// D-BUILDPROFILE1: `optimize:` levels for `Build.{ optimize: … }`:
/// `none` (no optimization, fastest compile), `basic` (opt-level=2, the
/// driver default), `full` (opt-level=3, maximum throughput).
pub const BUILD_OPTIMIZE_NONE: &str = "none"; // D-BUILDPROFILE1
pub const BUILD_OPTIMIZE_BASIC: &str = "basic"; // D-BUILDPROFILE1
pub const BUILD_OPTIMIZE_FULL: &str = "full"; // D-BUILDPROFILE1

/// Canonical list of impure builtins (write stdout/stderr or read stdin).
///
/// Used by Sema/Purity and Comptime/Purity to detect I/O calls inside
/// `pure fn` or comptime contexts. Both consumers must agree on this set;
/// having it here prevents silent divergence.
pub const IMPURE_BUILTINS: &[&str] = &[BUILTIN_PRINT, "eprint", BUILTIN_INPUT, "read_all_input"];

// ── Marker family + syntax wave (ratified 2026-07-01, D-MARKERMOVE2/3 2026-07-02) ──
//
// D-MARKER-FAMILY1 (B): two-plane sigil law. `@` precedes a declaration and
// states a checkable contract about it; `#` instructs the compiler (modes,
// regions, effects, compile-time values) and may appear in type/expression
// position where `@` never does; `$` stays splice-only (D-CTMARKER1).
//
// D-CONTRACTCASE1 (A): PascalCase everywhere on the `@` plane — one casing
// rule for the whole language, extending D-CASING1's `#`-plane rule to `@`.
//
// D-MARKERMOVE1 (B): the fixed move list — `Pure`, `MustUse`, `Codable`,
// `Encode`, `Decode`, `Experimental`, `Tested`, `Hardened`, `PublishedSchema`,
// `Redact`, `Numeric` move from `#` to `@` (e.g. `@Pure` → `@Pure`), exact
// PascalCase spelling kept. `@Numeric` (D-DIST3) and the `@numeric` capability
// bundle (D-CAPBUNDLE1) are the same job (I8) and merge into one `@Numeric` —
// see `ATTR_NUMERIC` above; there is no separate bundle constant. Serde field +
// container markers (`#Rename`, `#Skip`, `#Default`, `#Flatten`,
// `#RenameAll`, `#DenyUnknownFields`, `#Tag`, `#Untagged`) are wire-format
// machinery, not promises, and stay on `#`.
//
// D-MARKERMOVE2 (B, ratified 2026-07-02): whole-move, no carve-out by
// position — `@Pure` is one spelling everywhere, including the type-position
// callback bound (`f: @Pure fn(T) -> U`). The plane law gains exactly one
// exception: a contract marker may prefix a function TYPE to state a bound;
// "declarations only" reads "declarations, plus contract bounds on function
// types".
//
// D-MARKERMOVE3 (B, ratified 2026-07-02): all built-in derive markers move,
// user derives stay `#`. `@Debug`, `@Summarize`, `@Comparable` join
// `@Codable`/`@Encode`/`@Decode` as contract-plane capability promises;
// `derive T.Wire { … }` bodies applied as `#[Wire]` remain `#` generation
// machinery — the built-in/user line IS the plane line.

/// D-MARKER-FAMILY1: the contract-plane prefix — sibling of `ATTR_PREFIX` ("#").
/// `@` markers attach only to declarations (fn, type, field) plus the
/// D-MARKERMOVE2 function-type bound carve-out, never to other expressions or
/// type positions. Loop-label suffix `@` (D-LOOPLABEL2) is a different
/// grammatical slot and unaffected.
pub const CONTRACT_PREFIX: &str = "@"; // D-MARKER-FAMILY1

/// D-PREPOST1 / D-CONTRACTCASE1: precondition contract on a function
/// signature — `@Pre(cond, "msg")`. The condition is a pure expression (same
/// checker as `@Pure`). Checked in every build by default; per-module
/// build-policy strip is the explicit opt-out.
pub const CONTRACT_PRE: &str = "Pre"; // D-PREPOST1
/// D-PREPOST1 / D-CONTRACTCASE1: postcondition contract — `@Post(cond,
/// "msg")`; `result` names the return value inside `cond`.
pub const CONTRACT_POST: &str = "Post"; // D-PREPOST1

/// D-PERSIST1 / D-CONTRACTCASE1: dev-tier contract on a module-level
/// binding — the value survives `jet dev` hot reloads (identity = module
/// path + binding name). Inert in release builds.
pub const CONTRACT_PERSIST: &str = "Persist"; // D-PERSIST1

/// D-METHODMACRO1=A / D-CONTRACTCASE1: `@Inline fn` / `@Inline` method — a
/// soft hint that this function/method should be inlined (`#[inline]` in
/// codegen). Never rejected by sema; the compiler is free to ignore it.
/// Methods stay ordinary functions — no macro-rewrite hooks (D-METHODMACRO1).
pub const CONTRACT_INLINE: &str = "Inline"; // D-METHODMACRO1
/// D-METHODMACRO1=A / D-CONTRACTCASE1: `@InlineAlways fn` / method — a
/// checked promise (`#[inline(always)]` in codegen). Sema rejects it
/// (E0917 self-recursive / E0918 address-taken / E0919 too large) when the
/// compiler can prove it genuinely cannot inline the call — a compile error
/// naming why, never a silent miss. Mutually exclusive with `CONTRACT_INLINE`
/// on the same declaration (E0920).
pub const CONTRACT_INLINE_ALWAYS: &str = "InlineAlways"; // D-METHODMACRO1

/// D-CAPBUNDLE1 / D-CONTRACTCASE1: capability bundles on a nominal distinct
/// type — each re-exposes a curated slice of the base type's operations
/// while keeping nominal identity. Stackable. The `numeric` bundle merged
/// into `ATTR_NUMERIC` (`@Numeric`, D-MARKERMOVE1) — there is no
/// `CONTRACT_BUNDLE_NUMERIC` constant.
pub const CONTRACT_BUNDLE_COMPARABLE: &str = "Comparable"; // D-CAPBUNDLE1
pub const CONTRACT_BUNDLE_PRINTABLE: &str = "Printable"; // D-CAPBUNDLE1
pub const CONTRACT_BUNDLE_CODABLE_AS_BASE: &str = "CodableAsBase"; // D-CAPBUNDLE1

/// D-CLIFLAG1 (rides D-CONTRACTCASE1/D-MARKERMOVE1, plane+casing fixed
/// 2026-07-02): struct-level CLI-derive marker — `@Cli`. The CLI-generation
/// feature itself is a separate card (c7cliflag); this constant exists so
/// that card builds against a fixed name. Never shipped as `#`, so no
/// teaching error.
pub const CONTRACT_CLI: &str = "Cli"; // D-CLIFLAG1
/// D-PATCH1 (card #181): struct-level derive — generates nested `T.Patch` with
/// `apply`/`diff`/`merge`, Codable by construction (Encode+Decode on Patch).
pub const CONTRACT_PATCHABLE: &str = "Patchable"; // D-PATCH1
/// D-CLIFLAG1: field-level doc marker for CLI-derived help text — `@Doc`.
/// Same status as `CONTRACT_CLI`: registered here, feature built elsewhere.
pub const CONTRACT_DOC: &str = "Doc"; // D-CLIFLAG1

/// D-MARKER-FAMILY1 / D-MARKERMOVE1 / D-MARKERMOVE3 (I7/R3 chokepoint): every
/// name that lives on the `@` contract plane. Union of the D-MARKERMOVE1
/// move list (§2a), the D-CONTRACTCASE1 recase set (§2b), D-MARKERMOVE3's
/// three extra built-in derives, and the D-CLIFLAG1 placeholders (G4). One
/// source of truth for "which plane is this name" — parser, formatter, sema,
/// and LSP all dispatch off `is_contract_marker`/`is_directive_marker`
/// instead of hand-rolled match arms.
pub const CONTRACT_MARKERS: &[&str] = &[
    // D-MARKERMOVE1 move list (§2a)
    KW_PURE,
    ATTR_MUST_USE,
    ATTR_CODABLE,
    ATTR_ENCODE,
    ATTR_DECODE,
    ATTR_EXPERIMENTAL,
    ATTR_TESTED,
    ATTR_HARDENED,
    ATTR_PUBLISHED_SCHEMA,
    ATTR_REDACT,
    ATTR_NUMERIC,
    // D-MARKERMOVE3: built-in derive markers (user derives stay `#`).
    // ATTR_COMPARABLE ("Comparable") also names the D-CAPBUNDLE1 capability
    // bundle below — same spelling, disambiguated by declaration position
    // (struct/enum derive vs. distinct-type bundle), listed once here.
    TRAIT_DEBUG,
    ATTR_SUMMARIZE,
    ATTR_COMPARABLE,
    // D-CONTRACTCASE1 recase set (§2b) — pre/post/persist/bundles
    CONTRACT_PRE,
    CONTRACT_POST,
    CONTRACT_PERSIST,
    // D-METHODMACRO1=A — checked inline contracts
    CONTRACT_INLINE,
    CONTRACT_INLINE_ALWAYS,
    CONTRACT_BUNDLE_PRINTABLE,
    CONTRACT_BUNDLE_CODABLE_AS_BASE,
    // D-CLIFLAG1 (G4) — registered, feature not yet implemented
    CONTRACT_CLI,
    CONTRACT_DOC,
    // D-PATCH1 (card #181)
    CONTRACT_PATCHABLE,
];

/// D-MARKER-FAMILY1: every `#`-plane directive name that a moved-marker
/// reader might confuse for a contract, i.e. the E0063 "did you mean `#`"
/// set (§2c). Not exhaustive of every `#` spelling in the language — just
/// the ones with parser-visible dispatch that needs to reject a stray `@`.
pub const DIRECTIVE_MARKERS: &[&str] = &[
    KW_UNSAFE,
    KW_IMPURE,
    KW_REACTIVE,
    KW_TEST,
    KW_BENCH,
    KW_TODO,
    KW_TAINTED,
    KW_SANITIZER,
    KW_STATE,
    KW_TRANSITION,
    KW_CAPS,
    KW_GRANT,
    KW_TRANSACT,
    ATTR_TARGET,
    ATTR_WASM,
    ATTR_JS,
    ATTR_WASM_EXPORT,
    ATTR_HTML,
    // ATTR_UNINIT intentionally absent (D-UNINIT-SENTINEL1): `#Uninit` is
    // retired outright, not merely on the wrong plane, so `@Uninit` isn't
    // taught "add `#`" — it falls through to an ordinary unknown-marker error.
    ATTR_REF,
    ATTR_UNIT_FAMILY,
    ATTR_SINGLE_USE,
    ATTR_LAYOUT,
    ATTR_SUPPRESS,
    ATTR_EXTERN_MODULE,
    ATTR_BINDGEN,
    "Caller",
    ATTR_RENAME,
    ATTR_SKIP,
    ATTR_DEFAULT,
    ATTR_FLATTEN,
    ATTR_RENAME_ALL,
    ATTR_DENY_UNKNOWN_FIELDS,
    ATTR_TAG,
    ATTR_UNTAGGED,
];

/// D-MARKER-FAMILY1: is `name` a contract-plane (`@`) marker? The I7/R3
/// dispatch chokepoint — parser/formatter/sema/LSP ask here, never hand-roll
/// the move list.
pub fn is_contract_marker(name: &str) -> bool {
    CONTRACT_MARKERS.contains(&name)
}

/// D-MARKER-FAMILY1: is `name` a directive-plane (`#`) marker in the E0063
/// confusable set? Used to detect `@` written before a directive name.
pub fn is_directive_marker(name: &str) -> bool {
    DIRECTIVE_MARKERS.contains(&name)
}

// D-UNITLIT1: unit-suffix numeric literals (`500ms`) are not an enumerable
// keyword — the lexer resolves a literal's identifier suffix against
// #UnitFamily members in scope (ATTR_UNIT_FAMILY, D-QUAL3). One fixed rule:
/// D-UNITLIT1: a literal suffix shaped `e` + digits is reserved for float
/// exponent notation (`1e5`) and may never resolve as a unit name.
pub const UNIT_SUFFIX_EXPONENT_RESERVED: &str = "e"; // D-UNITLIT1

// D-TRAILBLOCK1: no new token — `{` directly after a call's `)` parses as the
// trailing zero-parameter lambda argument. Parser-position rule, not lexical.
// D-DESTRUCT1: no new token — reuses the D-DOTCTOR1 `.{` sigil in pattern
// position and `..` (OP_RANGE) as the now-mandatory partial-pattern rest
// marker.
// D-CHAINCMP1: no new token — same-direction `<`/`<=`/`>`/`>=` chains are a
// parser/sema desugaring (`0 <= sev < 10` → `0 <= sev && sev < 10`, middle
// operand evaluated once).
// D-CLIFLAG1: the struct-level CLI-derive marker and field-level doc marker
// spellings ride D-CONTRACTCASE1/D-MARKERMOVE1 — constants land with them.
// D-EFFBUDGET1: `effects`/`allow`/`deny`/`grants` are pkg.jet manifest keys
// (Jetpack/PackageManifest), not language tokens; effect names reuse D-EFF4.
// D-ANY-JAI1 + D-VARARGBOUND1: reuses D-VARIADIC1 `...T`; multi-trait
// bounds use the owner-amended list form (`T: [A, B]`, `...items: [A, B]`).
// D-UFCS1 (B), D-POINTERCHAIN1 (A), D-ERRCTX1 (D): no typeable surface.

// ── Module name resolution helpers ───────────────────────────────────────────
//
// These are pure string functions used by both Sema and Codegen to identify
// compiler-known ("core") modules. They live here so that neither Sema nor
// Codegen need to depend on Loader (which does file I/O and belongs in the
// driver layer).

/// Single canonical source of truth for all known Core modules (c45).
///
/// `is_known_core_module` and `core_modules_list` both derive from this slice.
/// `core_module_items` in Sema/CheckerCoreLib.rs has per-module item data and
/// cannot collapse here, but a drift-guard test (tests/corelib.rs) asserts its
/// key set equals this slice.
pub const KNOWN_CORE_MODULES: &[&str] = &[
    "core",
    "core.fs",
    "core.io",
    "core.env",
    "core.process",
    "core.math",
    "core.random",
    "core.time",
    "core.tasks",
    "core.mem",
    // D-ALLOC-C (ratified 2026-06-19): wider allocator API bucket.
    "core.mem.alloc",
    // D-OPTGC1 / D-DEP-GC1: opt-in traced `Gc<T>` library.
    "core.gc",
    // E2-M7: streaming file handles and path helpers (D-IO1, D-IO2).
    "core.files",
    "core.path",
    // E2-M10: TCP/UDP sockets.
    "core.net",
    // D-DEFER1 option B: scope-exit guard (RAII cleanup via closure).
    "core.scope",
    // D-ARGS1 (ratified 2026-06-22): declarative CLI arg parsing builder.
    "core.args",
    // D-TERM1 (ratified 2026-06-22): terminal direct-input — `term.read_key() -> Key`.
    "core.term",
    // D-ANY-JAI1 (c7jaiany §6, ratified 2026-07-01): runtime reflection floor —
    // `reflect.of(x) -> Value` with `.type_name()`/`.display()`/`.fields()`.
    "core.reflect",
    // D-ENC1 (ratified 2026-06-24): unified serialization library `core.encoding` with
    // per-format submodules. Supersedes `core.json` + `jet.{csv,toml,yaml}` (clean break).
    "core.encoding",
    "core.encoding.json",
    "core.encoding.csv",
    "core.encoding.toml",
    "core.encoding.yaml",
    // D-UUIDENC1=A (ratified 2026-06-26): hex and base64 codecs (pure, no deps).
    "core.encoding.hex",
    "core.encoding.base64",
    // D-TEXTUNICODE1: std-only Unicode scalar helpers. Grapheme segmentation stays
    // future work because it needs a Unicode data table/engine.
    "core.text.unicode",
    // D-SHIFT1 (c7shift): `binary.Reader` / `text.Cursor` — the constructors are
    // bare (no import needed); the modules exist for discoverability/docs.
    "core.binary",
    "core.text",
    // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
    "core.uuid",
    // D-CORENS1: ring packages now spelled `core.*` (canonical user-facing name).
    // Most ring packages still dispatch through legacy `jet.*` keys; archive is
    // canonical end-to-end as `core.archive`.
    "core.log",
    "core.crypto",
    // D-RANDSPLIT1=A: CSPRNG submodule — `core.crypto.random.bytes(n)`.
    "core.crypto.random",
    // D-CRYPTOENV1=A: expert-only raw crypto primitives.
    "core.crypto.expert",
    // D-HTTPLIB1-4 (ratified 2026-06-26): HTTP client+server ring package.
    "core.http",
    // D-REGEX1: linear-time regex, ships on the `regex` crate via the FFI bridge.
    "core.regex",
    // D-DEP-ARCHIVE1=A (ratified): gzip compress/decompress via the `flate2` crate FFI bridge.
    "core.archive",
    // D-CODECS1 (ratified): standalone compression codecs, separate from `core.archive`.
    // `flate2` (gzip) and `zstd` FFI bridges.
    "core.compress.gzip",
    "core.compress.zstd",
    // D-DEP-DB1: SQLite ring package via the `rusqlite` (bundled) crate FFI bridge.
    "core.db",
    // D-REACT1=B (ratified 2026-06-22): opt-in reactive library — signals,
    // derived values, and effects. Pure std runtime (no external crate).
    "core.reactive",
    // D-HONESTNUM1=A (ratified 2026-06-26): Measurement<T> — value ± uncertainty
    // with standard uncertainty propagation. Pure float arithmetic; no external crates.
    "core.science.measurement",
    // D-BIGINT1 / D-DECIMAL1 (ratified 2026-06-28 / 2026-06-26): precise numerics.
    "core.numeric",
    // D-PENDING1=B (ratified 2026-06-26): Loadable<T, E> — async UI state machine
    // (Idle / Loading / Loaded(T) / Failed(E)). Pure stdlib enum; no external crates.
    "core.async.loadable",
    // D-ADAPTFID1=A (ratified 2026-06-26): Perf.fidelity() / Perf.set_fidelity() —
    // adaptive quality/perf knob backed by a global atomic f32.
    "core.perf",
    // D-RENDERTGT1=A + D-RENDERTGT2=A (c133 M1): render-target backend trait seam.
    "core.ui",
    // D-APPROX1=A (ratified 2026-06-26): approximate data structures under core.sketch.
    "core.sketch.hll",
    "core.sketch.tdigest",
    "core.sketch.reservoir",
    "core.sketch.cms",
    // D-TIMEDEPTH1=A (ratified 2026-06-26): civil-time constructors.
    "core.time.date",
    "core.time.datetime",
    // D-TTLVAL1=A: TTL-wrapped cache values.
    "core.time.expiring",
    // D-TTLVAL1=A: rotting secrets with zeroize-on-expiry.
    "core.secrets",
    // D-NETDEP1=A / D-HTTPLIB2=B (ratified 2026-06-26): full HTTP library.
    "core.http.client",
    "core.http.server",
    // c-devserver (owner-directed 2026-07-01): a `.jet` file's own `jet dev`
    // behavior — a configurable server value (`for_app`/`.html`/`.port`/`.serve`).
    "core.devserver",
];

pub fn is_known_core_module(name: &str) -> bool {
    if KNOWN_CORE_MODULES.contains(&name) {
        return true;
    }
    // D-CORENS1: internal dispatch key `jet.<ring>` (from normalize_core_module)
    // is valid even though only `core.<ring>` is in KNOWN_CORE_MODULES.
    if let Some(ring) = name.strip_prefix("jet.") {
        return is_ring_module(ring);
    }
    false
}

pub fn core_modules_list() -> String {
    KNOWN_CORE_MODULES.join(", ")
}

/// Normalize a module import name to a canonical core-module path, or `None`
/// if the import is not a core/ring module.
///
/// D-CORENS-CANON1: `core.<ring>` is the only user-facing spelling. Ring modules
/// still normalize to the internal `jet.<ring>` key used by sema dispatch.
pub fn normalize_core_module(name: &str) -> Option<String> {
    if name == CORE_SHORT {
        return Some(CORE_SHORT.to_string());
    }
    if name == CORE_CANONICAL {
        return Some(CORE_SHORT.to_string());
    }
    // `core.archive` is canonical end-to-end. Other ring modules still use
    // internal `jet.<ring>` keys until their package cleanup lands.
    if let Some(ring) = name.strip_prefix("core.") {
        if ring == "archive" {
            return Some(name.to_string());
        }
        if is_ring_module(ring) {
            return Some(format!("jet.{ring}"));
        }
        return Some(format!("core.{ring}"));
    }
    None
}

/// E2-M9: ring module names that resolve as compiler-known modules.
pub fn is_ring_module(name: &str) -> bool {
    matches!(
        name,
        "log" | "crypto" | "http" | "regex" | "reactive" | "archive" | "db"
    )
}

/// The env var that names the active realized toolchain object directory
/// (D-JPK-TOOLCHAIN1 / #179). Tests set it to a fixture; #179's realizer points
/// it at the hangar object. The object carries prebuilt ring artifacts under
/// `<dir>/ring/<name>` (D-JPK-RINGSHIP1=C).
pub const TOOLCHAIN_OBJECT_ENV: &str = "JET_TOOLCHAIN_FIXTURE";

/// D-JPK-TOOLCHAIN1=A (#179): re-exec guard marker. Before `jet` execs a
/// version-pinned toolchain, it sets this env var to the pinned version. The
/// child, seeing its own version match the marker, runs natively and never
/// re-realizes or re-execs — this breaks the exec loop and lets the pinned
/// toolchain run the program directly.
pub const TOOLCHAIN_EXEC_MARKER_ENV: &str = "JET_TOOLCHAIN_EXEC";

/// D-JPK-RINGSHIP1=C: is this ring lib present as a realized hangar object for
/// the active toolchain? True when the active toolchain object carries a
/// prebuilt artifact for `name` on this platform; false otherwise (the loader
/// then falls back to the compiler-embedded template — rung-0 magic preserved).
pub fn is_ring_module_staged(name: &str) -> bool {
    staged_ring_artifact(name).is_some()
}

/// The prebuilt ring artifact path for `name` in the active toolchain object, or
/// `None` when there is no active object or it carries no artifact for `name`.
pub fn staged_ring_artifact(name: &str) -> Option<std::path::PathBuf> {
    if !is_ring_module(name) {
        return None;
    }
    let dir = std::env::var_os(TOOLCHAIN_OBJECT_ENV)?;
    let artifact = std::path::Path::new(&dir).join("ring").join(name);
    artifact.exists().then_some(artifact)
}

pub fn is_legacy_std_import(name: &str) -> bool {
    name == "std" || name.starts_with("std.") || name == "jet.std" || name.starts_with("jet.std.")
}

/// D-ALLOC1/D-ALLOC-C: allocator opaque types → jet_mem Rust types.
pub fn alloc_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Arena" => Some("jet_mem::JetArena"),
        "Bump" => Some("jet_mem::JetBump"),
        "Pool" => Some("jet_mem::JetPool"),
        "Fixed" => Some("jet_mem::JetFixed"),
        _ => None,
    }
}

/// D-ARGS1: ArgsSpec/ParsedArgs → prelude Rust types.
pub fn args_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "ArgsSpec" => Some("JetArgsSpec"),
        "ParsedArgs" => Some("JetParsedArgs"),
        _ => None,
    }
}

/// D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)`'s handle types are top-level
/// prelude structs, same shape as `args_handle_rust_type` above.
pub fn reflect_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Value" => Some("JetReflectValue"),
        "Field" => Some("JetReflectField"),
        _ => None,
    }
}

/// D-SHIFT1 (c7shift): `binary.Reader`/`text.Cursor` handle types are
/// top-level prelude structs, same shape as `reflect_handle_rust_type`
/// above — including the caller's `!type_names.contains(name)` collision
/// guard, since "Reader"/"Cursor" are plausible user type names.
pub fn binary_text_handle_rust_type(name: &str) -> Option<&'static str> {
    match name {
        "Reader" => Some("JetReader"),
        "Cursor" => Some("JetCursor"),
        _ => None,
    }
}

/// True when a crate spec string needs an explicit version (i.e. it's not
/// "std" and doesn't already contain `@`).
pub fn crate_spec_needs_version(spec: &str) -> bool {
    spec != "std" && spec.split_once('@').is_none()
}

/// D-EFFBUDGET1 (ratified 2026-07-01): the `effects { allow: […], deny: […] }`
/// block in `pkg.jet` that turns on whole-dependency-graph effect enforcement.
/// Manifest keys only — no language grammar (§0.4 DO-NOT).
pub const MANIFEST_BLOCK_EFFECTS: &str = "effects"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `allow:` field inside `effects { … }` — the closed list of
/// effect names the whole dependency graph may use.
pub const EFFECTS_FIELD_ALLOW: &str = "allow"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `deny:` field inside `effects { … }` — effect names the
/// dependency graph must never use.
pub const EFFECTS_FIELD_DENY: &str = "deny"; // D-EFFBUDGET1
/// D-EFFBUDGET1: the `grants { "dep": [Effect] }` block — an audited
/// per-dependency escape from the `effects:` budget, recorded in the lockfile.
pub const MANIFEST_BLOCK_GRANTS: &str = "grants"; // D-EFFBUDGET1

/// Levenshtein edit distance between two strings (used for "did you mean?" suggestions).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}
