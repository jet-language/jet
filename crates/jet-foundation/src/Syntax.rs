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

/// S2 / D-BIND2 (ratified): immutable binding sigil `name @= expr`
/// (D-BIND2 = A; replaces `::` from D-BIND1). `@=` is unambiguous in the
/// lexer because `@` + identifier is always a label or host selector.
pub const SIGIL_BIND_IMMUT: &str = "@=";

/// S2 / D-BIND2: the retired immutable-binding sigil `::` (D-BIND1 era).
/// Recognized only for the E0991 teaching error that points at `@=`.
pub const SIGIL_BIND_IMMUT_RETIRED: &str = "::";

/// S2 / D-BIND1 (ratified 2026-06-18): mutable binding sigil `name := expr`
/// (was the keyword `var`). `=` stays reassignment of an existing `:=` (S17).
pub const SIGIL_BIND_MUT: &str = ":=";

/// S2 / D-BIND1: the retired binding keywords, recognized only for the E0985
/// teaching error that points at the `@=` / `:=` sigils.
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

/// S32 (ratified M3): optional type suffix — `Int?` is “maybe an Int”.
pub const TYPE_OPTION_SUFFIX: &str = "?";

/// S32 (ratified M3): present / absent spellings for `T?` (lowercase like `true`).
pub const LIT_VALUE: &str = "value";
pub const LIT_NULL: &str = "null";

/// S27 (ratified M3): method receiver name.
pub const KW_SELF: &str = "self";

/// S27 (ratified M3): external method block — `impl Type { ... }`.
pub const KW_IMPL: &str = "impl";

/// M2: compile-time constant (emits Rust `const` or `static`).
pub const KW_CONST: &str = "const";

/// M1/M2: return from a function.
pub const KW_RETURN: &str = "return";

/// M2: loop statement (for SharedHandle lint checks).
pub const KW_LOOP: &str = "loop";

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

/// S14/S58: bare lowercase `unsafe` — the foreign (C/Rust) spelling, recognized
/// only for teaching errors (E0031 / E0003) pointing at the `#Unsafe` marker.
pub const FOREIGN_UNSAFE: &str = "unsafe";

/// S58 (ratified 2026-06-12): discovery gate — naming any low-level item
/// requires `use core.mem`.
pub const CORE_MEM_MODULE: &str = "core.mem";

/// D-UNINIT1 (ratified 2026-06-21, opt C): the `#Uninit` binding marker —
/// `#Uninit name: Type` declares a binding with no initializer, opting out of
/// the default zero-fill. Gated by `use core.mem` (S58); sema proves
/// write-before-read (E0420) and codegen lowers to `MaybeUninit`.
pub const ATTR_UNINIT: &str = "Uninit";

/// S58 (ratified 2026-06-12): the pointer type — `Ptr<T>`.
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

/// D-CTX1 (ratified 2026-06-22, G2): smart-context block marker.
/// `#context(field: value) { … }` swaps named ambient fields (allocator,
/// logger) for the lexical+dynamic extent of the block, then restores them.
/// The marker is PascalCase (D-CASING1); the field names inside are lower.
/// Expert-tier only (R1): never emitted in beginner-tier diagnostics or docs.
pub const CTX_BLOCK: &str = "Context";

/// D-CTX1 (ratified 2026-06-22): allowed field names inside `#context(…)`.
pub const CTX_FIELD_ALLOCATOR: &str = "allocator";
/// D-CTX1 (ratified 2026-06-22): logger field (v1 bundle).
pub const CTX_FIELD_LOGGER: &str = "logger";

/// D-TERM1 (ratified 2026-06-22): terminal direct-input block keyword.
/// `live { … }` enters un-buffered/no-echo input mode for its body and
/// guarantees terminal-state restore on every exit path including panic
/// (implemented with the D-DEFER1 scope-guard mechanism). "raw mode" jargon
/// is deliberately avoided; `live` is the user-facing name. A contextual
/// keyword: recognised only when followed by `{`.
pub const KW_LIVE: &str = "live";

/// D-DET1 (ratified 2026-06-22): the expert determinism-escape block keyword.
/// `assume_deterministic { … }` inside a `#Pure fn` suspends the determinism
/// rejections (E3401/E3403) for its body — the "I know this is deterministic"
/// hatch. A semantic footgun, v1-legal per the card. A contextual keyword:
/// recognised only when followed by `{`, so a name `assume_deterministic` still
/// works elsewhere. Erased in codegen (I3) — the block is a plain Rust block.
pub const KW_ASSUME_DET: &str = "assume_deterministic";

/// D-DET1 (ratified 2026-06-22): the deterministic injected `Clock` capability
/// type. A `#Pure fn` taking a `Clock` param may read time **through it**
/// (`clock.now()` / `clock.tick(ms)`) — reproducible, because the caller seeded
/// it (`time.clock(seed)`) — while the ambient `time.now()` stays E3403. An
/// ordinary value type, not a module alias; methods are pure-from-the-fn's-view.
pub const CLOCK_TYPE: &str = "Clock";

/// D-DET1 (ratified 2026-06-22): the deterministic injected `Rng` capability
/// type. A `#Pure fn` taking an `Rng` param may draw randomness **through it**
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

/// D-SIMD1/D-SIMD2 (ratified 2026-06-24): the built-in portable SIMD lane types.
/// `F32x4` is four `F32` lanes, `F64x2` is two `F64` lanes. Constructor
/// `F32x4(a,b,c,d)`, splat `F32x4.splat(x)`, lane index `v[i]`, element-wise
/// `+`/`-`/`*`/`/`, reduce `v.sum()` / `v.reduce(#Max)`, and the `[F32#4]` bridge
/// `from_array`/`to_array`. A closed compiler-provided family (no user `+`); ops
/// lower to a scalar-array fallback (the pinned stable rustc has no `std::simd`),
/// memory-safe by construction (I1) — no `std::simd`-feature gate, no intrinsics.
pub const SIMD_F32X4_TYPE: &str = "F32x4";
pub const SIMD_F64X2_TYPE: &str = "F64x2";

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

/// D-TERM1 (ratified 2026-06-22): the Core module that provides key reading.
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
///   reactive.effect(() => { … })             — a side effect re-run on change
/// Methods: `Signal.get()/set(v)`, `Derived.get()`. Dependency tracking is
/// explicit-by-read (a `.get()` inside a derived/effect body subscribes to it).
pub const REACTIVE_MODULE: &str = "jet.reactive";
pub const TYPE_SIGNAL: &str = "Signal";
pub const TYPE_DERIVED: &str = "Derived";

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

/// M2: shared handle type (Arc equivalent); auto-cloned across boundaries.
pub const TYPE_SHARED: &str = "Shared";

/// M1 (docs/spec/roadmap.md, owner-blessed examples 2026-06-11): branching keywords.
pub const KW_IF: &str = "if";
pub const KW_ELSE: &str = "else";

/// S19 (ratified): loop keywords. `loop` is the one true loop keyword.
/// `in` is a contextual keyword inside `loop x in …`.
pub const KW_IN: &str = "in";

/// S22 (ratified): inclusive range between two `Int` ends — `1..10`.
pub const OP_RANGE: &str = "..";

/// S22 (amended 2026-06-15, D-SG8): contextual `step n` range stride —
/// `0..10 step 2`. Only meaningful inside a range; an ordinary name elsewhere.
pub const KW_RANGE_STEP: &str = "step";

/// S23 (ratified): loop control.
pub const KW_BREAK: &str = "break";
pub const KW_CONTINUE: &str = "continue";

/// S24 / D-IF1 (ratified 2026-06-18): `when` is retired — `if` is the one
/// branching keyword (`if subject { arm -> body }`). `when` is recognized only
/// for the E0984 teaching error pointing at `if`.
pub const KW_SWITCH: &str = "when";

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
pub const OP_MINUS_EQ: &str = "-=";
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

/// S51 (ratified M10; amended 2026-06-16): compiler-known **core** library roots.
pub const CORE_SHORT: &str = "core";
pub const CORE_CANONICAL_ROOT: &str = "jet";
pub const CORE_CANONICAL: &str = "jet.core";

/// S51 (amended 2026-06-16): former `std` spellings — teaching errors only (S14).
pub const LEGACY_STD_SHORT: &str = "std";
pub const LEGACY_STD_CANONICAL: &str = "jet.std";

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
/// S59 (S82): attribute on generated C binding modules — `#bindgen module c.….__bindgen__`.
pub const ATTR_BINDGEN: &str = "bindgen"; // S59
/// S59 (S82): attribute on user C overlay modules — `#extern module c.…`.
pub const ATTR_EXTERN_MODULE: &str = "extern"; // S59 — `#extern module`, not `extern rust`
/// D-UNSAFE2 (retired marker): `#Audit("…")` is the old two-line form;
/// now the reason is the argument of `#Unsafe("reason")` itself. Recognized
/// only to emit the E0055 teaching error.
pub const ATTR_AUDIT: &str = "Audit"; // retired, D-UNSAFE2
// D-LABEL1: a loop label is `@name` immediately before `loop`
// (`@outer loop { … }`); `break @name` / `continue @name` target it.
// D-ATTR3 = B (ratified 2026-06-19): `@` stays for labels; attributes use `#`.
// No disambiguation by following keyword needed — `#` is always an attribute,
// `@` in prefix position before `loop` is always a label.
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
/// marker per D-CASING1, joining the `#Test`/`#Pure`/`#Todo`/`#Caps` family.
/// The `benchmark` manifest target (TARGET_BENCHMARK, c80) points `jet bench`
/// at a package entry; it is not a new engine — it reuses this exact machinery.
pub const KW_BENCH: &str = "Bench";

/// D-TOOL2 (ratified 2026-06-17, E2-M11; PascalCase marker D-CASING1 follow-on
/// 2026-06-21): typed hole `#Todo` — compiles everywhere, panics at runtime with
/// file, line, and expected type. Bare lowercase `todo` (FOREIGN_TODO) is the
/// retired spelling → E0054 teaching error pointing at `#Todo`.
pub const KW_TODO: &str = "Todo";

/// S60 (ratified 2026-06-12; implemented E2-M16; PascalCase marker D-CASING1
/// follow-on 2026-06-21): the purity modifier, written as the marker `#Pure fn
/// name() { … }`. A `#Pure fn` may only call other `#Pure fn`s and pure
/// builtins; impure calls are a compile error (E3401) with the call-trace path.
/// Bare lowercase `pure` (FOREIGN_PURE) is the retired spelling → E0053 teaching
/// error pointing at `#Pure`.
///
/// D-EFF2 (ratified 2026-06-22): `#Pure` also rides the front of a callback
/// parameter type — `f: #Pure fn(T) -> U` demands a pure callback; passing one
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
/// modifier in the `#Pure`/`#Unsafe` family; PascalCase per D-CASING1. Erased in
/// codegen (I3). NOTE: the ratified card spells the modifier bare `sanitizer fn`;
/// the D-CASING1 marker convention (which moved `pure fn` → `#Pure fn`) makes
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

/// D-TXN4: the type of a transaction handle bound by `#Transact(name)`. An
/// opaque sema-only handle; erased in codegen (I3).
pub const TXN_HANDLE_TYPE: &str = "Transaction";

/// S14 / D-CASING1 follow-on (2026-06-21): the retired lowercase spellings of
/// the three marker keywords, recognized only for teaching errors that point at
/// the `#Test` / `#Pure` / `#Todo` marker forms.
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
    "run", "enter", "build", "list", "clean", "add", "remove", OS_SUBCOMMAND,
];

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

/// U3 (ratified 2026-06-16): the type matching each reserved namespace.
pub const TYPE_ENV: &str = "Env";
pub const TYPE_SYSTEM: &str = "System";
pub const TYPE_IMAGE: &str = "Image";

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

/// S52 (ratified M12; amended 2026-06-16, U2): the unified single lockfile lives
/// inside the `.jet/` managed folder (SOURCE_ROOT_DIR). Replaces `jet.lock`
/// (and `pack.lock`); the manifest reshape chunk migrates the old paths.
pub const UNIFIED_LOCK_FILE: &str = ".jet/lock";

/// S52 (ratified M12; amended 2026-06-16, U2): the single shared, content-
/// addressed store ("hangar"), global and never relocated.
pub const HANGAR_DIR: &str = "/etc/jet/hangar";

/// D-JPK-FILES (ratified 2026-06-18): the monorepo manifest at repo root.
/// TOML format; holds `[repo]`, `[sources]`, and `[packages]` tables.
pub const JETPACK_TOML: &str = "jetpack.toml";

/// D-JPK-FILES (ratified 2026-06-18): `[repo]` table in `jetpack.toml`.
pub const JTOML_TABLE_REPO: &str = "repo";

/// D-JPK-FILES (ratified 2026-06-18): `[sources]` table in `jetpack.toml` —
/// named source refs (`name = "provider@target#ver"`).
pub const JTOML_TABLE_SOURCES: &str = "sources";

/// D-JPK-FILES (ratified 2026-06-18): `[packages]` table in `jetpack.toml` —
/// optional explicit index (`name = "relative/pkg.jet"`).
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
pub const OP_NAMED_CTOR: &str = ".{";

/// S75 (ratified 2026-06-16): the fan-out operator — `f.[a, b, c]` desugars to
/// `[f(a), f(b), f(c)]`. `.[` is a parser-level adjacency of `.` and `[`;
/// there is no dedicated two-character lexer token (the parser detects `.`
/// immediately followed by `[`). This constant documents the user-visible sigil.
pub const OP_FAN_OUT: &str = ".[";

/// S76 (ratified 2026-06-16): the fixed-size separator in `[T#N]` type
/// position, e.g. `[Int#3]`. Amended by VERSION-# (2026-06-16): `#` also
/// introduces pinned version numbers in package references (`pkg#1.2.0`).
/// Same token, two contexts: `[T#N]` is the type-level size form; `name#ver`
/// is the package version-pin form. No dedicated two-character token in either
/// case — the parser resolves by position.
pub const TYPE_FIXED_SIZE_SEP: &str = "#";

/// S81 (ratified 2026-06-16): loop-skip sigil — `?continue` inside a `loop`
/// iteration propagates a `None` / `Err` result as a continue (skip to next
/// element). It is the iteration-level analogue of `?` propagation for fallible
/// loops. Written as a single two-char token `?continue` (the `?` is part of
/// the keyword, not a standalone operator).
pub const KW_QUESTION_CONTINUE: &str = "?continue";

/// D-DIST1 (ratified 2026-06-19): `UserId @= distinct Int` — declares a
/// distinct type (a separate nominal type sharing the base's representation).
/// Used in the value position of a `@=` immutable binding at item level;
/// `distinct`-over-`distinct` chaining is rejected in v1.
pub const KW_DISTINCT: &str = "distinct";

/// D-DIST3 (ratified 2026-06-20): unwrap method for a distinct type —
/// `value.raw()` yields the base value. Named-cast family (S42).
pub const METHOD_DISTINCT_RAW: &str = "raw";

/// D-ATTR1 / D-DIST3 (ratified): `#Numeric` marker enables same-type
/// arithmetic on a distinct type. Written `#Numeric` on the same line
/// before the distinct-type name (uses the `#` attribute prefix D-ATTR1).
pub const ATTR_NUMERIC: &str = "Numeric";

/// D-QUAL3 (ratified 2026-06-24): `#UnitFamily(currency) { usd, eur, gbp }` —
/// declares a family of units. Each member mints one distinct `#Numeric` type
/// (`usd` → `Usd`) that erases to `Float`, so signatures read plain English
/// (`fn subtotal(price: Usd, qty: Int) -> Usd`). The family is the
/// "upgrade to D-DIST2" framing of D-UNIT1: sugar over the distinct-type
/// machinery (D-DIST1/D-DIST3). PascalCase tag per D-CASING1.
pub const ATTR_UNIT_FAMILY: &str = "UnitFamily";

/// D-MIGRATE1 (ratified 2026-06-22): `#PublishedSchema` — marks a struct whose
/// field layout is snapshotted at release time. A breaking field change without
/// a declared migration is E0910. Written `#PublishedSchema` before `struct`.
pub const ATTR_PUBLISHED_SCHEMA: &str = "PublishedSchema"; // D-MIGRATE1

/// D-LIN1 (ratified 2026-06-21, option A; gated on D-QUAL2): `#SingleUse` — marks
/// a type whose values must be consumed exactly once on every reachable path
/// (moved to a `^` parameter or returned). Using one zero times is E0140
/// (unconsumed at scope end) / E0141 (unconsumed on one branch); aliasing one
/// with `&`/`view` is E0142. `#SingleUse` implies `#NoCopy`. The tag is
/// compile-time only and erases in codegen (I3). Written `#SingleUse` before the
/// `struct`/`enum`, same marker idiom as `#PublishedSchema`.
pub const ATTR_SINGLE_USE: &str = "SingleUse"; // D-LIN1

/// D-MIGRATE1 (ratified 2026-06-22): contextual keyword `migration` — introduces
/// a migration block that declares how a `#PublishedSchema` struct changed between
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
/// reports each `#PublishedSchema` type's pinned shape; `squash --before <ver>`
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
pub const LAYOUT_PACKED: &str = "packed";     // D-REPRC1 (reserved)
pub const LAYOUT_ALIGN: &str = "align";       // D-REPRC1 (reserved)
/// D-SOA1 / D-SOA2A=C (implemented): the struct-of-arrays layout variant —
/// `#layout(columnar) struct S` stores a `[S]` collection column-per-field.
/// Whole-struct only in v1 (D-SOA2B); the partial form `#layout(columnar: …)`
/// is rejected (E1109) and the per-container prefix `columnar [T]` is reserved
/// (D-SOA2C, E1107).
pub const LAYOUT_COLUMNAR: &str = "columnar"; // D-SOA1 / D-SOA2A

// ── Serde derive markers + attributes (D-SERDE2–8, D-ENC1; bracket form D-ATTR2) ──
// Derive markers (PascalCase per D-CASING1, written `#[…]` before a struct/enum):
// `#[Codable]` derives BOTH directions (sugar for `#[Encode, Decode]`); `#[Encode]`
// is write-only; `#[Decode]` is read-only. Owner (D-SERDE4 = B, modified): the
// collapsed umbrella is `Codable`, with `Encode`/`Decode` as the one-way markers.
pub const ATTR_CODABLE: &str = "Codable"; // D-SERDE4
pub const ATTR_ENCODE: &str = "Encode"; // D-SERDE4
pub const ATTR_DECODE: &str = "Decode"; // D-SERDE4
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
    // Core structure (S1, S18, S16, S50, U3)
    KW_FN, KW_PUB, KW_USE, KW_EXTERN, KW_MODULE,
    // Control flow (M1, S19, S23, M1/M2)
    KW_IF, KW_ELSE, KW_SWITCH, KW_LOOP, KW_IN, KW_BREAK, KW_CONTINUE, KW_RETURN,
    // Types and declarations (M2, S30, S27, M2, S28, S55, S57, D-DIST1)
    KW_STRUCT, KW_ENUM, KW_IMPL, KW_TRAIT, KW_TAG, KW_DERIVE, KW_CONST, KW_COMPTIME, KW_DISTINCT,
    // Schema migrations (D-MIGRATE1 / D-MIGRATE2)
    KW_MIGRATION, KW_RENAME, KW_ADD, KW_REMOVE, KW_CHANGE, KW_VIA,
    // Ownership / borrow keywords (S10, M2). D-CAP7 retired KW_MUTATE/KW_MOVE/
    // KW_VIEW in favor of the `~`/`^`/`&` sigils — they live only as teaching
    // errors (E0056/E0057/E0058) now, so they are NOT in the keyword list.
    KW_STORED, KW_SELF,
    // Memory / expert tier (S58, D-REGION1, D-CTX1, D-TERM1, D-CTEFFECT1)
    KW_UNSAFE, KW_IMPURE, KW_REGION, CTX_BLOCK, KW_LIVE,
    // Determinism escape (D-DET1): `assume_deterministic { … }`
    KW_ASSUME_DET,
    // Transactions (D-TXN1–D-TXN4): `#Transact(name) { … }`
    KW_TRANSACT,
    // Test / tooling (S43, S60, D-TOOL2, D-BENCH1)
    KW_TEST, KW_BENCH, KW_PURE, KW_TODO,
    // Taint tracking (D-TAINT1): value-fact tag + sanitizer modifier
    KW_TAINTED, KW_SANITIZER,
    // Typestate (D-STATE1 / D-STATE-DECL / D-STATE-REQ / D-STATE-TRANS)
    KW_STATE, KW_TRANSITION, KW_STATE_DECL,
    // Literals: boolean (S11), option (S32), result (S34), synthetic (M4)
    LIT_TRUE, LIT_FALSE, LIT_NULL, LIT_OK, LIT_ERR, KW_IT,
    // Binding sigils (SIGIL_BIND_IMMUT / SIGIL_BIND_MUT) are not words; omitted.
];

/// Canonical list of built-in type names for LSP completion and rename guard.
///
/// Only types a user writes in source. `Result` is the legacy fallible type
/// (S34) kept for teaching errors; it is intentionally excluded here since
/// `T ? E` is the current spelling.
pub const JET_TYPE_LIST: &[&str] = &[
    TYPE_INT, TYPE_FLOAT, TYPE_BOOL, TYPE_STRING, TYPE_CHAR, TYPE_LIST, TYPE_MAP, TYPE_SHARED,
    TYPE_I8, TYPE_I16, TYPE_I32, TYPE_I64, TYPE_U8, TYPE_U16, TYPE_U32, TYPE_U64, TYPE_F32, TYPE_F64,
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

/// D-BUILDPROFILE1: blessed profile names — `release` and `debug` carry
/// built-in defaults and need no explicit declaration in `build { }`.
pub const BUILD_PROFILE_RELEASE: &str = "release"; // D-BUILDPROFILE1
pub const BUILD_PROFILE_DEBUG: &str = "debug"; // D-BUILDPROFILE1

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
pub const IMPURE_BUILTINS: &[&str] = &[
    BUILTIN_PRINT, "eprint", BUILTIN_INPUT, "read_all_input",
];

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
    // D-ENC1 (ratified 2026-06-24): unified serialization library `core.encoding` with
    // per-format submodules. Supersedes `core.json` + `jet.{csv,toml,yaml}` (clean break).
    "core.encoding",
    "core.encoding.json",
    "core.encoding.csv",
    "core.encoding.toml",
    "core.encoding.yaml",
    // E2-M9: first-party ring packages.
    "jet.log",
    "jet.time",
    "jet.crypto",
    // D-HTTPLIB1-4 (ratified 2026-06-26): HTTP client+server ring package.
    "jet.http",
    // D-REGEX1: linear-time regex, ships on the `regex` crate via the FFI bridge.
    "jet.regex",
    // D-DEP-ARCHIVE1=A (ratified): gzip compress/decompress via the `flate2` crate FFI bridge.
    "jet.archive",
    // D-DEP-DB1: SQLite ring package via the `rusqlite` (bundled) crate FFI bridge.
    "jet.db",
    // D-REACT1=B (ratified 2026-06-22): opt-in reactive library — signals,
    // derived values, and effects. Pure std runtime (no external crate).
    "jet.reactive",
];

pub fn is_known_core_module(name: &str) -> bool {
    KNOWN_CORE_MODULES.contains(&name)
}

pub fn core_modules_list() -> String {
    KNOWN_CORE_MODULES.join(", ")
}

/// Normalize a module import name to a canonical core-module path, or `None`
/// if the import is not a core/ring module.
pub fn normalize_core_module(name: &str) -> Option<String> {
    if name == CORE_SHORT {
        return Some(CORE_SHORT.to_string());
    }
    if let Some(rest) = name.strip_prefix("core.") {
        return Some(format!("core.{rest}"));
    }
    if name == CORE_CANONICAL {
        return Some(CORE_SHORT.to_string());
    }
    if let Some(rest) = name.strip_prefix("jet.core.") {
        return Some(format!("core.{rest}"));
    }
    // E2-M9: first-party ring packages — `jet.http`, `jet.db`, etc.
    if let Some(ring) = name.strip_prefix("jet.") {
        if is_ring_module(ring) {
            return Some(format!("jet.{ring}"));
        }
    }
    None
}

/// E2-M9: ring module names that resolve as compiler-known modules.
pub fn is_ring_module(name: &str) -> bool {
    matches!(
        name,
        "log" | "time" | "crypto" | "http" | "regex" | "reactive" | "archive" | "db"
    )
}

/// E2-M9: ring modules not yet available (always false now; kept for API stability).
pub fn is_ring_module_staged(_name: &str) -> bool {
    false
}

pub fn is_legacy_std_import(name: &str) -> bool {
    name == LEGACY_STD_SHORT
        || name.starts_with("std.")
        || name == LEGACY_STD_CANONICAL
        || name.starts_with("jet.std.")
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

/// True when a crate spec string needs an explicit version (i.e. it's not
/// "std" and doesn't already contain `@`).
pub fn crate_spec_needs_version(spec: &str) -> bool {
    spec != "std" && spec.split_once('@').is_none()
}

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
