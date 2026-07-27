/// N1 (ratified): language name.
pub const LANG_NAME: &str = "Jet";

/// N1 (ratified): compiler binary name.
pub const BINARY_NAME: &str = "jet";

/// D-UNSAFE-OBLIG1=A: CI/admin input naming a manifest-shaped organization
/// policy file. The loader fails closed when a configured file cannot be read.
pub const ENV_ORG_UNSAFE_POLICY: &str = "JET_ORG_UNSAFE_POLICY";

/// D-UNSAFE-OBLIG1=A: typed compile-time unsafe proof statement and its
/// parser-only call sentinel (removed by sema before TIR).
pub const KW_ASSERT: &str = "assert";
pub const INTERNAL_UNSAFE_ASSERT: &str = "__jet_unsafe_assert";
pub const UNSAFE_OBLIGATION_VALID_PTR: &str = "valid_ptr";
pub const UNSAFE_OBLIGATION_ALIGNED: &str = "aligned";
pub const UNSAFE_OBLIGATION_NO_ALIAS: &str = "no_alias";

/// N2 (ratified): source file extension (without the dot).
pub const FILE_EXT: &str = "jet";

/// D-ARTIFACT-EXT1=A: closed product-named artifact family. New kinds require
/// an owner ballot; consumers compare against these exact suffixes.
pub const ARTIFACT_EXT_SOURCE_MAP: &str = ".jetmap";
pub const ARTIFACT_EXT_NOTEBOOK: &str = ".jetnb";
pub const ARTIFACT_EXT_PROOF: &str = ".jetproof";
pub const ARTIFACT_EXT_TRACE: &str = ".jettrace";
pub const ARTIFACT_EXT_GAME_REPLAY: &str = ".jetreplay";
pub const ARTIFACT_EXT_PROOF_REPLAY: &str = ".jetproof-replay";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactKind { SourceMap, Notebook, Proof, Trace, GameReplay, ProofReplay }

pub const ARTIFACT_KINDS: &[(ArtifactKind, &str)] = &[
    (ArtifactKind::SourceMap, ARTIFACT_EXT_SOURCE_MAP),
    (ArtifactKind::Notebook, ARTIFACT_EXT_NOTEBOOK),
    (ArtifactKind::Proof, ARTIFACT_EXT_PROOF),
    (ArtifactKind::Trace, ARTIFACT_EXT_TRACE),
    (ArtifactKind::GameReplay, ARTIFACT_EXT_GAME_REPLAY),
    (ArtifactKind::ProofReplay, ARTIFACT_EXT_PROOF_REPLAY),
];

pub fn artifact_kind(path: &str) -> Option<ArtifactKind> {
    ARTIFACT_KINDS.iter().find_map(|(kind, suffix)| path.ends_with(suffix).then_some(*kind))
}

/// S1 (ratified): keyword that starts a function definition.
pub const KW_FN: &str = "fn";

/// S18 (ratified): marks an item as visible to other files (via `use`).
pub const KW_PUB: &str = "pub";

/// D-VISDEFAULT2=A (ratified): marks an item private inside a `#PubFile` file.
pub const KW_PRIV: &str = "priv";

/// D-VISDEFAULT2=A (ratified): file-scope marker that flips default visibility to
/// public-by-default for following top-level items (D-VISDEFAULT1=C).
pub const MARKER_PUB_FILE: &str = "PubFile";

/// D-PRELUDEX1=A (ratified 2026-06-28): file-scope marker that disables ambient
/// prelude auto-imports (`print` / `input`). Expert escape hatch only — no
/// library may inject into the no-prefix surface.
pub const MARKER_NO_PRELUDE: &str = "NoPrelude";

/// D-VISDEFAULT2 option B (rejected): retired spelling for the private exception
/// keyword — recognized only for E0412 teaching diagnostics.
pub const FOREIGN_PRIVATE: &str = "private";
/// D-SHAPE-CLI-COMPLETE1=A: external program schema source for
/// `jet self completions SHELL --for PROGRAM`.
pub const CLI_COMPLETIONS_FOR: &str = "--for";

/// D-VISDEFAULT2 option B (rejected): retired spelling for the file marker —
/// recognized only for E0418 teaching diagnostics.
pub const MARKER_PUBLIC_FILE: &str = "PublicFile";

/// D-PUBPKG1=A (ratified): the `pub(package)` visibility qualifier — restricts
/// access to sibling packages in the same payload/workspace.
pub const PUB_PACKAGE_QUALIFIER: &str = "package";

/// S2 / D-BIND1 / D-BIND4 / D-BIND-BARE1: immutable binding sigil `name :: expr`.
/// D-BIND-BARE1 retires typed bindings (`name: Type :: expr`); types ride the
/// value (`Type.{ … }`) or live on signatures and fields.
pub const SIGIL_BIND_IMMUT: &str = "::";

/// S2 / D-BIND1 (ratified 2026-06-18): mutable binding sigil `name := expr`
/// (was the keyword `var`). `=` stays reassignment of an existing `:=` (S17).
/// D-BIND-BARE1 retires typed bindings (`name: Type := expr`).
pub const SIGIL_BIND_MUT: &str = ":=";

/// D-PROVENANCE1=B: binding-level tracking marker, written before the binding:
/// `#Track name :: expr` / `#Track name := expr`.
pub const ATTR_TRACK: &str = "Track";

/// D-CANVASSTATE1=D (ratified 2026-07-09): statement switch-off attribute.
/// `#Off <stmt>` parses and type-checks the statement, then emits no code.
pub const ATTR_OFF: &str = "Off";

/// D-CANVASSTATE1=D (ratified 2026-07-09): debug-only statement attribute.
/// `#DebugOnly <stmt>` emits only in debug/dev builds; release builds strip it.
pub const ATTR_DEBUG_ONLY: &str = "DebugOnly";

/// D-CANVASMETA1=B (ratified 2026-07-09): tooling metadata attribute for
/// bindings, top-level consts, and functions.
pub const ATTR_META: &str = "Meta";

/// D-CANVASMETA1=B: `#Meta` category field name.
pub const META_FIELD_CATEGORY: &str = "category";

/// D-CANVASMETA1=B: `#Meta` tunable flag field name.
pub const META_FIELD_TUNABLE: &str = "tunable";

/// D-MARK-META1=B: `#Meta` maturity field name.
pub const META_FIELD_MATURITY: &str = "maturity";

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
/// Both `print` and `input` form the interactive I/O subset. All other core.io
/// members stay qualified behind `use core.io`.
pub const BUILTIN_INPUT: &str = "input";

/// D-PRELUDE-LAW1=A: complete closed no-prefix registry. The first group is
/// always ambient; the second exists only under its ratified comptime gates.
/// User declarations shadow these names and libraries cannot inject new ones.
pub const PRELUDE_ALWAYS_IDENTS: &[&str] =
    &["print", "input", "panic", "require"];
pub const PRELUDE_COMPTIME_IDENTS: &[&str] =
    &["embed_file", "embed_bytes", "find", "fetch"];
pub const PRELUDE_IDENTS: &[&str] = &[
    "print", "input", "panic", "require", "embed_file", "embed_bytes", "find", "fetch",
];

/// S11 (ratified): built-in type names (M1).
pub const TYPE_INT: &str = "Int";
pub const TYPE_FLOAT: &str = "Float";
pub const TYPE_BOOL: &str = "Bool";
pub const TYPE_STRING: &str = "String";
pub const TYPE_ERROR: &str = "Error";
pub const TYPE_VOID: &str = "Void";

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

/// D-MEM1 / D-MEM-PARAM1=A / D-SHAPE-PLACE1=A (ratified, supersedes D-CAP7): memory model v5 sigils. Three
/// sigils plus unmarked: unmarked = read (enforced in S2), `&T` = exclusive
/// write, `^T` = move (consume), `~T` = copy (D-SHAPE-COPY1=A, supersedes
/// D-CAP2/S4's `copy` verb).
pub const SIGIL_MOVE: &str = "^";
pub const SIGIL_WRITE: &str = "&";

/// D-SHAPE-COPY1=A (supersedes D-CAP2/S4): the one copy spelling — `~x`
/// produces an owned, independent value. A temporary (no named binding
/// survives to be used-after), so it never needs `^` and never trips E0209.
/// `.clone()` is not user-typable Jet syntax (I8 — one way to mean it).
pub const SIGIL_COPY: &str = "~";

/// D-SHAPE-COPY1=A: the retired `copy` word (was D-CAP2/S4's one copy
/// spelling). Recognized only for the E0991 teaching error that points at
/// the `~` sigil.
pub const KW_COPY: &str = "copy";

/// S10 (M2) → D-MEM1: the retired write keyword. Recognized only for the E0056
/// teaching error that points at the `&` sigil.
pub const KW_MUTATE: &str = "mut";

/// S10 (M2) → D-CAP7: the retired move keyword. Bare `take value` is recognized
/// only for the paused capability teaching path. `.take(n)` stays a valid method
/// name, while `take(names) () =>` is recognized only to report E0057.
pub const KW_MOVE: &str = "take";

/// M2: struct definition keyword (construction spelling: S29).
pub const KW_STRUCT: &str = "struct";

/// S30 (ratified M3): sum-type definition keyword.
pub const KW_ENUM: &str = "enum";

/// D-TYPEALIAS1 (ratified 2026-06-28): transparent type alias — `alias Name<T> = …`
/// for generic type shortcuts only (not primitive newtypes).
pub const KW_ALIAS: &str = "alias";

/// S32 (ratified M3): optional type suffix — `Int?` is “maybe an Int”.
pub const TYPE_OPTION_SUFFIX: &str = "?";

/// D-UNIONTYPE1=A: anonymous closed structural sum — `Int | String`.
/// Order-insensitive; nested unions flatten; duplicates disappear. Underneath
/// it is compiler-generated enum sugar; named enums stay the documenting form.
pub const TYPE_UNION_SEP: &str = "|";

/// S32 / D-OPT-SPELL1 / D-SHAPE3b: Optional variants are `Val` / `None`.
/// Both also support the expected-type forms `.Val` / `.None`.
///
/// D-FLOWTYPE1=A: for a direct immutable local or parameter of type `T?`,
/// `x != None` narrows `x` to `T` in the true branch; `x == None` narrows `x`
/// to `T` in the false branch. The fact reaches the right side of short-circuit
/// `&&` (not `||`) and ends at the branch boundary. Mutable locals, fields,
/// indexes, aliases, and calls never narrow. `x == Val(v)` keeps S31 binding
/// behavior. Sema records the proven unwrap as an S31 Present/`IfLet` fact;
/// codegen performs no proof.
pub const LIT_VALUE: &str = "Val";
pub const LIT_NULL: &str = "None";

/// S27 (ratified M3): method receiver name.
pub const KW_SELF: &str = "self";

/// S27 (ratified M3): external method block — `impl Type { ... }`.
pub const KW_IMPL: &str = "impl";

/// D-EXTMETH1=B (ratified): external inherent methods attach with dot:
/// `fn Type.method(self)`. S83's earlier `~~` connector is retired and exists
/// only for E0325 teaching diagnostics.
pub const EXTERNAL_METHOD_CONNECTOR: &str = ".";
pub const EXTERNAL_METHOD_CONNECTOR_RETIRED: &str = "~~";

/// M2: compile-time constant (emits Rust `const` or `static`). Foundational
/// keyword predating the S-numbered decision log (card #447 KW_DECISION_ID_EXEMPT).
pub const KW_CONST: &str = "const";

/// M1/M2: return from a function. Foundational keyword predating the
/// S-numbered decision log (card #447 KW_DECISION_ID_EXEMPT).
pub const KW_RETURN: &str = "return";

/// D-SHAPE-RESOURCE2=A (ratified 2026-07-15, card #647): the only deferred
/// statement is `defer close(^resource)`. `defer` is contextual so ordinary
/// identifiers keep their existing meaning outside statement-head position.
pub const KW_DEFER: &str = "defer";
pub const RESOURCE_CLOSE: &str = "close";
/// D-SHAPE-RESOURCE2=A: nominal, consuming, infallible cleanup capability.
pub const TRAIT_CLOSE: &str = "Close";

/// Parser-only collision-proof spelling for the desugared deferred close.
/// NUL cannot occur in a source identifier, so user code cannot invoke it.
pub const INTERNAL_DEFER_CLOSE: &str = "\0jet.defer.close";

/// S19 (ratified): loop statement (for SharedHandle lint checks) — same
/// governing decision as the loop-header keywords in Syntax/math_layout.rs.
pub const KW_LOOP: &str = "loop";

/// D-STREAMYIELD1 (ratified): `yield expr` — hand a value to a `Stream<T>`
/// consumer and suspend until the next pull. Legal only in a function whose
/// return type is `Stream<T>`.
pub const KW_YIELD: &str = "yield";

/// D-STREAMYIELD1: the generator return-type constructor `Stream<T>`.
pub const TYPE_STREAM: &str = "Stream";

/// D-UNSAFE2 (ratified 2026-06-22, opt B; prev S58 2026-06-12) and
/// D-UNSAFE-REASON1=A: the audited
/// expert gate. Block form: `#Unsafe("reason") { … }`. Whole-function form:
/// `#Unsafe("reason") fn`. Bare `#Unsafe { … }` / `#Unsafe fn` are E3112.
/// The reason is the argument of `#Unsafe` itself; the separate
/// `#Audit` marker is retired (E0055). The bare lowercase `unsafe` keyword
/// (FOREIGN_UNSAFE) is the rejected foreign spelling, recognized only to emit a
/// teaching error.
pub const KW_UNSAFE: &str = "Unsafe";

/// D-SHIELDNAME1=A (ratified 2026-07-11): `#Shield { … }` — the cancellation-shield
/// block marker, joining the `#Unsafe`/`#Context` sigil family. Any cancellation
/// (or blown deadline) pending against a task running inside the block is deferred
/// until the block exits; at exit the deadline lands first, then the cancel. Bare
/// `#Shield {` only — no argument list (`#Shield(...)` is a parse error). Lowers to
/// the `jet_scheduler_shield_enter`/`_leave` runtime (SHIELD_DEPTH thread-local);
/// a no-op outside a task. Expert-tier concurrency marker.
pub const KW_SHIELD: &str = "Shield";

/// D-CTEFFECT1 (ratified 2026-06-25): `#Impure("reason") { … }` — the audited
/// Tier-2 comptime effect gate. Both this block AND `--allow-impure` at build
/// are required to execute ambient comptime I/O (FS/Env/Exec/IO). PascalCase
/// per D-CASING1 (consistent with `Unsafe`).
pub const KW_IMPURE: &str = "Impure";

/// D-REACTCORE1 (ratified 2026-06-27, opt D): `#Reactive fn` / `#Reactive { … }` —
/// an explicit opt-in scope marker. Inside it, signal `.get()` reads register with
/// the active reactive observer (library machinery in `core.reactive`). Lowers to
/// `jet_reactive_scope` / `jet_reactive_effect` — no new evaluation semantics.
pub const KW_REACTIVE: &str = "Reactive";

/// D-WASM1=A (ratified 2026-06-28, c123), respelled by D-MARK-TARGET1=A
/// (ratified 2026-07-11, card #498): `#Target(Wasm|JS)` is the one target-
/// marker family, covering both the module-/file-level partition ceiling
/// AND the per-function bucket override (the retired bare `#Wasm`/`#JS`
/// spellings). Sema validates it against inferred `Browser` effects.
pub const ATTR_TARGET: &str = "Target";

/// D-WASM1=A: export this WASM function to the generated JS loader. A
/// different job (export surface) from the `#Target(Wasm|JS)` partition
/// family above — D-MARK-TARGET1=A leaves it untouched.
pub const ATTR_WASM_EXPORT: &str = "WasmExport";

/// D-WASM1=A: `#Target(JS)` argument spelling.
pub const WEB_BUCKET_JS: &str = "JS";

/// D-WASM1=A: `#Target(Wasm)` argument spelling.
pub const WEB_BUCKET_WASM: &str = "Wasm";

/// D-WEBKIND1=A (c123): `jet build --target=web` Jet backend target (not a rustc triple).
pub const BUILD_TARGET_WEB: &str = "web";

/// D-WEBDEFAULT1 (ratified 2026-07-01, c134): `#Target(Web)` argument spelling — a file-level
/// marker distinct from the `Wasm`/`JS` partition-ceiling values above (same
/// `#Target(...)` marker, different axis: "build me for the web backend by
/// default" rather than "cap this file's partition ceiling").
pub const WEB_TARGET_DEFAULT_WEB: &str = "Web";

/// D-HTMLPAIR1 (ratified 2026-07-01, c134): `#HTML("path.html")` — an explicit, file-level
/// declaration of this program's companion host page for `--target=web`
/// builds, replacing the silent `<stem>.html` filename convention.
pub const ATTR_HTML: &str = "HTML";

/// D-DSLBLOCK1=A (ratified 2026-07-06): `#SQL<Row> { ... }` — a stdlib-owned,
/// checked DSL block. Third-party DSL block markers are not user-extensible.
pub const DSL_BLOCK_SQL: &str = "SQL";

/// D-DSLBLOCK1=A: initial fixed stdlib DSL block marker whitelist. `HTML`
/// reuses `ATTR_HTML`; block form (`#HTML { ... }`) is distinct from the
/// existing file-level companion-page form (`#HTML("path.html")`).
pub const STDLIB_DSL_BLOCK_MARKERS: &[&str] = &[DSL_BLOCK_SQL, ATTR_HTML];

/// D-TYPEDTEXT2: parser-only sentinels for `sql"..."` / `html"..."` literals.
/// These are impossible user identifiers; sema rewrites them to the existing
/// synthetic `SQL`/`HTML` typed-text calls before codegen.
pub const TYPED_TEXT_SQL_PREFIX_CALL: &str = "$typed_text_sql";
pub const TYPED_TEXT_HTML_PREFIX_CALL: &str = "$typed_text_html";
/// D-FFI-SH1=A: `Sh` is D-TYPEDTEXT1's argv-safe shell-command instance.
pub const TYPE_SH: &str = "Sh";
/// D-FFI-SH1=A / D-TYPEDTEXT2: parser sentinel for user spelling `sh"…"`.
pub const TYPED_TEXT_SH_PREFIX_CALL: &str = "$typed_text_sh";

/// D-OSTARGET1=A (ratified 2026-07-01, c134): `#Target(OS. … )` namespace — the
/// second, mutually-exclusive axis of the `#Target(...)` marker family
/// (`Wasm`/`JS`/`Web` above are the first, web-bucket axis). Attaches at
/// `impl` block scope, not file/module scope.
pub const TARGET_OS_NAMESPACE: &str = "OS";

/// D-OSTARGET1=A: `#Target(OS.Linux)`.
pub const TARGET_OS_LINUX: &str = "Linux";

/// D-OSTARGET1=A: `#Target(OS.MacOS)`.
pub const TARGET_OS_MACOS: &str = "MacOS";

/// D-OSTARGET1=A: `#Target(OS.Windows)`.
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

/// D-UNINIT-SENTINEL2=A (ratified 2026-07-24; amends D-UNINIT-SENTINEL1):
/// contextual keyword `uninit`, legal only as the whole body of a typed-literal
/// head — `name := Type.{ uninit }`. Bare `name := uninit` is E0421. Still gated
/// by `use core.mem` (E0424) and restricted to plain-data types (E0423). Flow
/// proof E0420 is unchanged. Contextual like `region`/`state`/`migration`: the
/// word `uninit` stays usable as an ordinary identifier everywhere else; the
/// lexer emits it as a plain `Ident`, and only a whole `Type.{ uninit }` body
/// is the uninit trigger.
pub const KW_UNINIT: &str = "uninit"; // D-UNINIT-SENTINEL2

/// D-SOLVER-LIB1=A (ratified 2026-07-06): explicit finite solver library.
pub const CORE_SOLVE_MODULE: &str = "core.solve";

/// D-SOLVER-LIB1=A: deterministic solver state handle.
pub const SOLVER_TYPE: &str = "Solver";

/// S58 / D-CAP9 / D-TYPE-ALIAS-CANON1 (ratified): raw-pointer type is `*T`.
/// `Ptr<T>` is not live syntax; this string remains only as the internal Rust
/// dispatch key for legacy TIR/codegen paths until they are renamed to `RawPtr`.
pub const TYPE_PTR: &str = "Ptr";

/// S58 (ratified 2026-06-12): `mem.Ptr<T>.from_addr(addr)` — typed pointer
/// from an integer address.
pub const MEM_FROM_ADDR: &str = "from_addr";

/// S58 (ratified 2026-06-12): `mem.volatile_read(p)` — volatile/MMIO read.
pub const MEM_VOLATILE_READ: &str = "volatile_read";

/// D-FLAGSHIP-MMIO1=A (ratified 2026-07-07): `mem.volatile_write(p, value)` —
/// volatile/MMIO write.
pub const MEM_VOLATILE_WRITE: &str = "volatile_write";

/// S58 (ratified 2026-06-12): `mem.address_of(x)` — the address of a value as
/// an Int (taking a pointer is inert; using it needs `#Unsafe`).
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
/// D-SHAPE3a=A (ratified 2026-07-14): also the sole fresh-state constructor
/// spelling, optionally written `.new(...)` when expected type resolves receiver.
/// D-SHAPE-OPAQUE-INFER1=A (ratified 2026-07-14): generic receiver arguments may
/// be omitted only when ordinary constructor inputs/expected type resolve them.
pub const MEM_ALLOC_NEW: &str = "new";

/// D-ALLOC1 (ratified 2026-06-19): allocate a value into an arena/bump/pool/fixed.
pub const MEM_ALLOC_ALLOC: &str = "alloc";

/// D-ALLOC-D (ratified 2026-06-19): reset the allocator, keeping the backing buffer.
pub const MEM_ALLOC_RESET: &str = "reset";

/// D-SHAPE-RESOURCE2=A: retired allocator-specific terminal verb. Kept only so
/// diagnostics can teach the sole `close(^allocator)` resource protocol.
pub const MEM_ALLOC_FREE: &str = "free";

/// D-ALLOC-C (ratified 2026-06-19): wider allocator API namespace.
pub const CORE_MEM_ALLOC_MODULE: &str = "core.mem.alloc";

/// D-ARGS1 (ratified 2026-06-22): declarative CLI argument parsing module.
pub const CORE_ARGS_MODULE: &str = "core.args";

// D-EMAIL1=A / D-EMAIL-SMTP-SURFACE1=A / D-EMAIL-SMTP-CONFIG1=A: bounded
// message/MIME plus exact SMTP policy values and one verified transport.
pub const CORE_EMAIL_MODULE: &str = "core.email";
pub const TYPE_EMAIL_ADDRESS: &str = "Address";
pub const TYPE_EMAIL_MESSAGE: &str = "Message";
pub const TYPE_EMAIL_ATTACHMENT: &str = "Attachment";
pub const TYPE_EMAIL_ENVELOPE: &str = "Envelope";
pub const TYPE_EMAIL_SMTP_SECURITY: &str = "SMTPSecurity";
pub const TYPE_EMAIL_RECIPIENT_POLICY: &str = "RecipientPolicy";
pub const TYPE_EMAIL_RECIPIENT_REPORT: &str = "RecipientReport";
pub const TYPE_EMAIL_SEND_REPORT: &str = "SendReport";
pub const TYPE_EMAIL_ERROR: &str = "EmailError";
pub const TYPE_EMAIL_LIMITS: &str = "Limits";
pub const TYPE_EMAIL_SMTP_AUTH: &str = "SMTPAuth";
pub const TYPE_EMAIL_TLS_TRUST: &str = "TLSTrust";
pub const TYPE_EMAIL_SMTP_CONFIG: &str = "SMTPConfig";
pub const TYPE_EMAIL_DKIM_CONFIG: &str = "DkimConfig"; // D-EMAIL-DKIM-CONFIG1=A
pub const TYPE_EMAIL_MAILER: &str = "Mailer";
pub const EMAIL_LIMITS_SAFE_METHOD: &str = "safe";
pub const CORE_EMAIL_ADDRESS_FN: &str = "address";
pub const CORE_EMAIL_ATTACHMENT_FN: &str = "attachment";
pub const CORE_EMAIL_MESSAGE_FN: &str = "message";
pub const CORE_EMAIL_ENVELOPE_FN: &str = "envelope";
pub const CORE_EMAIL_SERIALIZE_FN: &str = "serialize";
pub const CORE_EMAIL_SMTP_FN: &str = "smtp";
pub const CORE_EMAIL_SMTP_FROM_ENV_FN: &str = "smtp_from_env";

/// D-REGION1 / D-BLOCKPLANE1: explicit allocation-region block
/// `#Region(r) { … }`.
/// Names a region spanning multiple arenas or narrower than the enclosing
/// function; arena `view`s allocated inside may not escape the region (E0631).
/// The beginner default is an implicit scope-inferred region (opt A) and never
/// writes `region`.
pub const ATTR_REGION: &str = "Region"; // D-BLOCKPLANE1

/// D-TASKSCOPE1=A + D-ARROW-CONTROL1=A: structured task group scope. The
/// keyword remains `taskgroup`; child callable bodies use `=>`.
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

/// D-TERM1 / D-BLOCKPLANE1: terminal direct-input block marker.
/// `#Live { … }` enters un-buffered/no-echo input mode for its body and
/// guarantees terminal-state restore on every exit path including panic
/// (implemented with the D-DEFER1 scope-guard mechanism). "raw mode" jargon
/// is deliberately avoided; `live` is the user-facing name. A contextual
/// keyword: recognised only when followed by `{`.
pub const ATTR_LIVE: &str = "Live"; // D-BLOCKPLANE1

/// D-DET1 / D-BLOCKPLANE1: expert determinism-escape marker.
/// `#Nondeterministic("reason") { … }` inside a `#Pure fn` suspends determinism
/// rejections (E3401/E3403) for its body — the "I know this is deterministic"
/// hatch. A semantic footgun, v1-legal per the card. A contextual keyword:
/// recognised only when followed by `{`, so a name `assume_deterministic` still
/// works elsewhere. Erased in codegen (I3) — the block is a plain Rust block.
pub const ATTR_NONDETERMINISTIC: &str = "Nondeterministic"; // D-BLOCKPLANE1

/// D-DET1 / D-SHAPE-CTORVERB1=C: the deterministic injected `Clock` capability
/// type. A `#Pure fn` taking a `Clock` param may read time **through it**
/// (`clock.now()` / `clock.tick(ms)`) — reproducible, because the caller seeded
/// it (`Clock.new(seed)`) — while the ambient `time.now()` stays E3403. An
/// ordinary value type, not a module alias; methods are pure-from-the-fn's-view.
pub const CLOCK_TYPE: &str = "Clock";

/// D-SHAPE-CTORVERB1=C: fresh state uses `new`; entropy-drawing construction
/// uses `new_random`. The generic TTL wrapper is the type-owned
/// `ExpiringValue.new(value, ttl, clock)`.
pub const EXPIRING_VALUE_TYPE: &str = "ExpiringValue";
pub const METHOD_FRESH_NEW_RANDOM: &str = "new_random";

/// D-DET1 (ratified 2026-06-22): the deterministic injected `Rng` capability
/// type. A `#Pure fn` taking an `Rng` param may draw randomness **through it**
/// (`rng.int(lo, hi)` / `rng.float()`) — reproducible from the caller's seed
/// (`random.rng(seed)`) — while the ambient `random.int(…)` stays E3403.
/// D-DET-CAPAPI (ratified 2026-06-25) widens `Rng` with `bool()` / `pick(list)`
/// / `shuffle(&list)`, mirroring the ambient `random.*` set.
pub const RNG_TYPE: &str = "Rng";

/// D-SHAPE-DURATION1=A / D-SHAPE-DURATIONCONVERT1=A (ratified 2026-07-14):
/// runtime numbers become checked durations through type-owned unit methods;
/// whole-unit reads use one checked enum-taking method. Static unit literals
/// remain unchanged.
pub const DURATION_TYPE: &str = "Duration";
pub const DURATION_UNIT_TYPE: &str = "DurationUnit";
pub const DURATION_RANGE_ERROR_TYPE: &str = "RangeError";
pub const DURATION_CONSTRUCTORS: &[&str] =
    &["milliseconds", "seconds", "minutes", "hours"];
pub const DURATION_UNITS: &[&str] =
    &["Milliseconds", "Seconds", "Minutes", "Hours"];
pub const METHOD_DURATION_IN: &str = "in";

pub fn duration_unit_for_constructor(method: &str) -> Option<&'static str> {
    match method {
        "milliseconds" => Some("Milliseconds"),
        "seconds" => Some("Seconds"),
        "minutes" => Some("Minutes"),
        "hours" => Some("Hours"),
        _ => None,
    }
}

/// D-BIGINT1 (ratified 2026-06-28): arbitrary-precision integer. Construct
/// explicitly with `BigInt(100)` or `BigInt("…")`; fixed `Int` never promotes.
pub const TYPE_BIGINT: &str = "BigInt";

/// D-DECIMAL1 (ratified 2026-06-26): exact base-10 decimal. Construct with
/// `Decimal("12.34")` or `core.math.decimal("12.34")`; no implicit `Float`.
pub const TYPE_DECIMAL: &str = "Decimal";

/// D-BUILDENTRY1/D-BUILDTARGET1/D-BUILDACTION1: typed build-program values.
/// These names are compiler-known only while the selected root `fn build`
/// runs; build entry is removed before runtime codegen.
pub const TYPE_BUILD_CONTEXT: &str = "BuildContext";
pub const TYPE_BUILD_PLAN: &str = "BuildPlan";
pub const TYPE_BUILD_ACTION: &str = "BuildAction";
pub const TYPE_BUILD_TARGET: &str = "BuildTarget";
pub const TYPE_BUILD_TOOLCHAIN: &str = "BuildToolchain";
pub const TYPE_BUILD_PROBE: &str = "BuildProbe";
pub const TYPE_PROGRAM_INFO: &str = "ProgramInfo";
pub const TYPE_TYPE_INFO: &str = "TypeInfo";
pub const TYPE_SOURCE_SPAN: &str = "SourceSpan";
