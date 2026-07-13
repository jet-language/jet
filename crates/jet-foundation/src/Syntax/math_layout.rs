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
/// D-EVENT1 (ratified 2026-07-07): first-party typed Event/Hook family.
/// Library values, compiler-known for typing/tooling; no new syntax.
pub const EVENT_MODULE: &str = "core.event";
pub const TYPE_EVENT: &str = "Event";
pub const TYPE_HOOK: &str = "Hook";
pub const TYPE_SUBSCRIPTION: &str = "Subscription";
pub const TYPE_EVENT_SCOPE: &str = "EventScope";
pub const TYPE_EVENT_POLICY: &str = "EventPolicy";
pub const TYPE_EVENT_TRACE: &str = "EventTrace";
/// D-EVENT2=A (ratified 2026-07-11): scheduler-backed typed async event family.
pub const TYPE_ASYNC_EVENT: &str = "AsyncEvent";
pub const TYPE_ASYNC_POLICY: &str = "AsyncPolicy";
pub const TYPE_EVENT_OVERFLOW: &str = "Overflow";
pub const TYPE_FAILURE_POLICY: &str = "FailurePolicy";
pub const TYPE_DISPATCH_REPORT: &str = "DispatchReport";
pub const TYPE_DISPATCH_FAILURE: &str = "DispatchFailure";
pub const TYPE_DISPATCH_STATE: &str = "DispatchState";
pub const TYPE_EVENT_CONFIG_ERROR: &str = "EventConfigError";
/// D-WATCH-SCOPE1 (ratified 2026-07-07): unified file/process/port watcher values.
pub const WATCHER_MODULE: &str = "core.watcher";
pub const TYPE_WATCH_HANDLE: &str = "WatchHandle";
pub const TYPE_WATCH_SET: &str = "WatchSet";
pub const TYPE_WATCH_EVENT: &str = "WatchEvent";
/// D-HONESTNUM1=A: the science measurement type name.
pub const TYPE_MEASUREMENT: &str = "Measurement";

/// D-LISTMAP-CANON1=A: legacy list spelling; `[T]` is canonical.
pub const TYPE_LIST: &str = "List";

/// D-LISTMAP-CANON1=A: legacy default map spelling; `[K: V]` is canonical.
pub const TYPE_MAP: &str = "Map";
/// D-LISTMAP-CANON1=A: named specific collection types.
pub const TYPE_HASH_MAP: &str = "HashMap";
pub const TYPE_BTREE_MAP: &str = "BTreeMap";
pub const TYPE_DEQUE: &str = "Deque";
pub const TYPE_SET: &str = "Set";
/// D-ITERTOOLS1=A: expanded collection handles.
pub const TYPE_SORTED_SET: &str = "SortedSet";
pub const TYPE_PRIORITY_QUEUE: &str = "PriorityQueue";
pub const TYPE_LRU: &str = "Lru";
pub const TYPE_BIT_SET: &str = "BitSet";
pub const TYPE_BYTE_BUFFER: &str = "ByteBuffer";

/// S41 (ratified M5): character type.
pub const TYPE_CHAR: &str = "Char";

/// S66 (ratified 2026-06-15): standard acronyms are fully capitalized in Jet source.
pub const TYPE_IO_ERROR: &str = "IOError";
/// D-IOERROR-TREE1=A: shared structured byte-stream error context and operation.
pub const TYPE_IO_CONTEXT: &str = "IOContext";
pub const TYPE_IO_OPERATION: &str = "IOOperation";
pub const IO_ERROR_VARIANTS: &[&str] = &[
    "InvalidInput", "NotFound", "PermissionDenied", "TimedOut", "Cancelled", "Closed", "Other",
];
pub const IO_OPERATION_VARIANTS: &[&str] = &[
    "Read", "Write", "Flush", "Connect", "Accept", "Close", "Resolve", "Codec",
];
pub const IO_CONTEXT_FIELDS: &[&str] = &["operation", "resource", "os_code", "cause"];
pub const TYPE_UTF8_ERROR: &str = "UTF8Error";
pub const TYPE_JSON: &str = "JSON";
pub const TYPE_JSON_ERROR: &str = "JSONError";

/// D-ENC-DYN1=A+ (ratified 2026-06-25) + D-SERDE13=B (ratified 2026-07-11): the
/// one dynamic encoding value every format's `parse` returns and every hand codec
/// constructs and returns. `DataTree` is the single canonical user-facing
/// spelling — renamed from the old `Data` face by D-SERDE13=B (it is a tree of
/// data, distinct from any user type named `Data`; the retired `Data` spelling is
/// a teaching error, E0351, not an alias, per I8). `Json`/`Toml`/`Yaml`/`Csv` are
/// format-tagged aliases over the same structure, so `json.parse` is typed
/// `Json`, `toml.parse` is typed `Toml`, etc., but one walker and one accessor
/// set back them. Variants: `Null`, `Bool`, `Int`, `Float`, `Text`, `Array`,
/// `Object`.
pub const TYPE_DATA: &str = "DataTree";
/// D-SERDE16=A (ratified 2026-07-11): public target-directed subtree dispatch.
/// `tree.decode<T>()` calls only `T`'s ordinary `Decode` protocol impl.
pub const METHOD_DATATREE_DECODE: &str = "decode";
pub const TYPE_DATA_JSON: &str = "Json";
pub const TYPE_DATA_TOML: &str = "Toml";
pub const TYPE_DATA_YAML: &str = "Yaml";
pub const TYPE_DATA_CSV: &str = "Csv";

/// The accepted spellings of the dynamic encoding value (D-ENC-DYN1=A+ /
/// D-SERDE13=B): canonical `DataTree` plus the four format-tagged aliases. The
/// old bare `Data` spelling is intentionally absent — it is caught as E0351.
pub fn is_data_type_name(name: &str) -> bool {
    matches!(name, "DataTree" | "Json" | "Toml" | "Yaml" | "Csv")
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

/// S68 (ratified) / M1 (docs/spec/roadmap.md, owner-blessed examples
/// 2026-06-11): branching keywords — `if` is the one branching keyword.
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

/// D-MEM1/S7 (D-NOALLOC-SEM1=A, ratified 2026-07-04): `policy no_alloc;` — a
/// module-level allocation floor, file-scoped like `web_target_ceiling`/
/// `#PubFile`. Sema flags allocation-shaped expressions written directly in
/// this file's own function bodies (E0921) — local only, never follows calls
/// into other modules. `no_alloc` is the only ratified policy name; the full
/// policy list is a follow-on ballot (an unknown name after `policy` is E0003).
pub const KW_POLICY: &str = "policy";
pub const POLICY_NO_ALLOC: &str = "no_alloc";

/// S51 / D-CORENS-CANON1: compiler-known `core.*` library root.
pub const CORE_SHORT: &str = "core";
pub const CORE_CANONICAL_ROOT: &str = "core";
pub const CORE_CANONICAL: &str = "core";

/// S51 (ratified M10): first-party short names reserved before packages land.
pub const FIRST_PARTY_RESERVED: &[&str] = &[
    "core", "jet", "c", "rust", "py", "js", "swift", "go", "fortran", "http", "regex", "csv", "toml", "crypto",
    "archive",
];

/// S50 (ratified M7): Rust FFI block introducers — `extern rust "…" { … }`.
pub const KW_EXTERN: &str = "extern"; // S50
pub const KW_RUST: &str = "rust"; // S50

/// D-FFI-UNIFY1: every foreign language mounts as `<lang>.<lib>`.
pub const FOREIGN_ROOTS: &[&str] = &[
    C_MODULE_ROOT,
    KW_RUST,
    PY_MODULE_ROOT,
    JS_MODULE_ROOT,
    SWIFT_MODULE_ROOT,
    GO_MODULE_ROOT,
    FORTRAN_MODULE_ROOT,
];
pub const PY_MODULE_ROOT: &str = "py"; // D-FFI-PY1 / D-FFI-UNIFY1
pub const JS_MODULE_ROOT: &str = "js"; // D-FFI-JS1 / D-FFI-UNIFY1
pub const SWIFT_MODULE_ROOT: &str = "swift"; // D-FFI-SWIFT1 / D-FFI-UNIFY1
pub const GO_MODULE_ROOT: &str = "go"; // D-FFI-GO1 / D-FFI-UNIFY1
pub const FORTRAN_MODULE_ROOT: &str = "fortran"; // D-FFI-FORTRAN1 / D-FFI-UNIFY1

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

/// D-FFI-INLINE1=A (ratified 2026-07-11, card #501): the inline foreign tier
/// directive marker — `#FFI(<lang>) fn name(sig) { """<foreign source>""" }`.
/// The Jet signature is the checked contract; the body is one string of
/// foreign source the per-language binder compiles on cache miss. Unsafe
/// languages (`c`, `cpp`, `asm`) additionally require the enclosing
/// `#Unsafe("reason")` gate (I1/S58). Spelled fully capitalized (S66).
pub const ATTR_FFI: &str = "FFI"; // D-FFI-INLINE1
/// D-FFI-CPP1=A (ratified 2026-07-11, card #501): C++ foreign root — the
/// `cpp.<lib>` namespace binder and the `#FFI(cpp)` inline-tier language name.
pub const CPP_MODULE_ROOT: &str = "cpp"; // D-FFI-CPP1 / D-FFI-UNIFY1
/// D-FFI-ASM1=A (ratified 2026-07-11, card #501): the assembly inline-tier
/// language name — `#FFI(asm) fn`. Assembly has no library namespace (it is
/// inline-only); it never appears as a `<lang>.<lib>` mount.
pub const ASM_LANG: &str = "asm"; // D-FFI-ASM1
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
/// D-FFI-UNIFY1: generated foreign bindings live under `.jet/bindings/<lang>/`.
pub const BINDINGS_ROOT_SUBDIR: &str = "bindings"; // D-FFI-UNIFY1

/// S14: foreign forms recognized only for teaching errors.
/// S19-amend (2026-06-17): `while`/`for` are now teaching errors pointing at `loop`.
pub const FOREIGN_WHILE: &str = "while";
pub const FOREIGN_FOR: &str = "for";
pub const FOREIGN_TRY: &str = "try";
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

/// S32 / D-OPT-SPELL1 (ratified 2026-07-04): foreign optional spellings for
/// teaching error E0020. `None` is RETIRED from this list — it's the real
/// absent spelling now, not a foreign guess. `Some`/`nil`/`none`/`some`
/// remain wrong; all point learners at `Val`/`None`.
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

/// D-CTIO1 (ratified 2026-06-22): the sanctioned build-time I/O builtins.
/// `embed_file("path") -> String` bakes a file's UTF-8 text into the binary;
/// `embed_bytes("path") -> [U8]` bakes its raw bytes (binary-safe). The path
/// must be a string literal, resolved relative to the source file, with no
/// `..`-escape past the project root. Only valid in a `comptime` binding.
/// D-CTFIND1/2: `find(glob) -> [String]` returns sorted relative file paths and
/// records each matched file hash for `.jet/lock`.
pub const BUILTIN_EMBED_FILE: &str = "embed_file";
pub const BUILTIN_EMBED_BYTES: &str = "embed_bytes";

/// S43 (ratified M6; PascalCase marker D-CASING1 follow-on 2026-06-21):
/// top-level test-declaration block, written as the marker `#Test("name") { … }`.
/// D-TESTPAREN1=A (ratified 2026-06-26): the name is now a parenthesized string
/// argument, matching the `#Caps(…)` / `#Grant(…)` marker family.
/// The bare lowercase `test` keyword (FOREIGN_TEST) is the retired spelling,
/// recognized only to emit the E0052 teaching error pointing at `#Test("name")`.
pub const KW_TEST: &str = "Test";

/// D-BENCH1 + D-BENCH-MARKER1=A: top-level region-benchmark block, written as
/// the marker `#Bench("name") { … }` — the exact sibling of `#Test("name") { … }`.
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

/// D-SCHEDULE1 (ratified 2026-07-11, card #505): `#Task fn` — a top-level
/// function jetpack can invoke by name (D-JPK-TASKRUN1), living beside
/// `fn run()`. Bare marker, no arguments.
pub const KW_TASK: &str = "Task";

/// D-JPK-TASKRUN1=A: lifecycle verbs a `#Task fn` must not reuse — they already
/// name Jet's built-in entry points (`fn run`/`fn dev`/`fn build`/`fn test`).
/// Sema rejects a collision as E0928.
pub const TASK_RESERVED_LIFECYCLE: &[&str] = &["run", "dev", "build", "test"];

/// D-SCHEDULE1: `#Every(…)` — a declarative schedule marker on a `#Task fn`.
/// Legal only alongside `#Task` (E0925 otherwise).
pub const ATTR_EVERY: &str = "Every";

/// D-SCHEDULE1: recognized duration suffixes for `#Every(<dur>)` — extends
/// `duration_suffix_nanos` (`ns`/`us`/`ms`/`s`) with `min`, the ratified
/// example spelling (`#Every(5min)`); a schedule finer than a second is
/// never realistic, so `min` is added here rather than widening the shared
/// `.timeout` table. `u128` nanoseconds, same overflow-safety reasoning as
/// `duration_suffix_nanos`.
pub fn schedule_duration_suffix_nanos(suffix: &str) -> Option<u128> {
    if let Some(nanos) = duration_suffix_nanos(suffix) {
        return Some(nanos);
    }
    match suffix {
        "min" => Some(60_000_000_000),
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
