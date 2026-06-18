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

/// S2 (ratified): introduces an immutable binding.
pub const KW_VAL: &str = "val";

/// S2 (ratified): introduces a mutable binding.
pub const KW_VAR: &str = "var";

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

/// S6 (ratified): separates statements inside a block.
pub const STMT_SEP: &str = ";";

/// S8 (ratified): string interpolation delimiters inside quoted text.
pub const INTERP_OPEN: &str = "{";
pub const INTERP_CLOSE: &str = "}";

/// S9 (ratified): the built-in print function (adds a newline).
pub const BUILTIN_PRINT: &str = "print";

/// S11 (ratified): built-in type names (M1).
pub const TYPE_INT: &str = "Int";
pub const TYPE_FLOAT: &str = "Float";
pub const TYPE_BOOL: &str = "Bool";
pub const TYPE_STRING: &str = "String";
pub const TYPE_ERROR: &str = "Error";

/// S10 (ratified M2): caller-site mutable borrow on a parameter or binding.
pub const KW_MUTATE: &str = "mut";

/// S10 (ratified M2): caller-site move; ownership transfers permanently.
pub const KW_MOVE: &str = "take";

/// S10 (ratified M2): return type — a borrow tied to self (elided lifetime).
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

/// S58 (ratified 2026-06-12; amended 2026-06-16): the audited expert gate,
/// written as the marker `@unsafe { … }` (statement) or `@unsafe fn` (whole-
/// function contract). The bare `unsafe` keyword is the rejected former
/// spelling, recognized only to emit a teaching error.
pub const KW_UNSAFE: &str = "unsafe";

/// S58 (ratified 2026-06-12): discovery gate — naming any low-level item
/// requires `use core.mem`.
pub const CORE_MEM_MODULE: &str = "core.mem";

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

/// S24 (ratified; keyword amended to `when` 2026-06-15, D-SG1): many-way
/// choice with condition arms.
pub const KW_SWITCH: &str = "when";

/// S24 (ratified): arm arrow inside `when` (same spelling as return types).
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

/// S13 (ratified): word forms recognized only for S14 teaching errors.
pub const FOREIGN_AND: &str = "and";
pub const FOREIGN_OR: &str = "or";
pub const FOREIGN_NOT: &str = "not";

/// S16 (ratified M6; amended 2026-06-16, D-S16-USE): file path or module `use`; optional `as`.
pub const KW_USE: &str = "use";
pub const KW_AS: &str = "as";

/// S51 (ratified M10; amended 2026-06-16): compiler-known **core** library roots.
pub const STD_SHORT: &str = "core";
pub const STD_CANONICAL_ROOT: &str = "jet";
pub const STD_CANONICAL: &str = "jet.core";

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
/// S59 (S82): attribute on generated C binding modules — `@bindgen module c.….__bindgen__`.
pub const ATTR_BINDGEN: &str = "bindgen"; // S59
/// S59 (S82): attribute on user C overlay modules — `@extern module c.…`.
pub const ATTR_EXTERN_MODULE: &str = "extern"; // S59 — `@extern module`, not `extern rust`
/// S58 / D-LL2: required reason on `@unsafe { … }` — `@audit("…")`.
pub const ATTR_AUDIT: &str = "audit"; // S58
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
pub const KW_TRAIT: &str = "trait";

/// S55 (ratified M9): opt-in built-in derive line in a type body.
pub const KW_DERIVE: &str = "derive";

/// S57 (ratified M9.5): compile-time constant binding keyword.
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
pub const BUILTIN_REQUIRE: &str = "require";
/// S43 (ratified M6): equality assertion in test blocks.
pub const BUILTIN_REQUIRE_EQ: &str = "require_eq";

/// S43 (ratified M6): top-level test block keyword.
pub const KW_TEST: &str = "test";

/// D-TOOL2 (ratified 2026-06-17, E2-M11): typed hole — compiles everywhere,
/// panics at runtime with file, line, and expected type.
pub const KW_TODO: &str = "todo";

/// S60 (ratified 2026-06-12; implemented E2-M16): `pure fn` checked modifier.
/// A `pure fn` may only call other `pure fn`s and pure builtins; impure calls
/// are a compile error (E3401) with the call-trace path.
pub const KW_PURE: &str = "pure";

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

/// M10 teaching spellings for common std/library habits.
pub const FOREIGN_EPRINTLN: &str = "eprintln";
pub const FOREIGN_OPEN: &str = "open";
pub const FOREIGN_GETENV: &str = "getenv";
pub const FOREIGN_OS: &str = "os";

/// M11 teaching spellings: async/await and mutex/lock.
pub const FOREIGN_ASYNC: &str = "async";
pub const FOREIGN_AWAIT: &str = "await";
pub const FOREIGN_MUTEX: &str = "Mutex";
pub const FOREIGN_LOCK: &str = "lock";

/// S82 (ratified 2026-06-16): attribute prefix sigil — `@Marker` / `@[a, b]`.
pub const ATTR_PREFIX: &str = "@";

/// S82 (ratified 2026-06-16): multi-attribute list delimiters after `@`.
pub const ATTR_LIST_OPEN: &str = "[";
pub const ATTR_LIST_CLOSE: &str = "]";

/// S82: rejected Rust-style attribute spelling (teaching error).
pub const FOREIGN_HASH_ATTR: &str = "#[";

/// S80 (ratified 2026-06-16): cross-type `?` conversion trait (D-ERR2).
pub const TRAIT_FALLIBLE: &str = "Fallible";

/// S80 (ratified 2026-06-16): `Fallible` method returning default `Error`.
pub const FN_TO_ERROR: &str = "to_error";

// S52's `MANIFEST_FILE`/`LOCK_FILE` (`jet.toml`/`jet.lock`) were retired in the
// manifest reshape chunk (U1/U2): the manifest is now `PAYLOAD_FILE`
// (`payload.jet`, U10 — was `pack.jet`/`PACK_FILE`) and the lockfile is
// `UNIFIED_LOCK_FILE` (`.jet/lock`). Clean break — no alias.

/// S52 (ratified M12): package source root directory inside a project.
pub const SOURCE_ROOT_DIR: &str = ".jet";

/// S52 (ratified M12): dependency kind table suffixes.
pub const DEP_TABLE_JET: &str = "dependencies";
pub const DEP_TABLE_RUST: &str = "dependencies:rust";
pub const DEP_TABLE_C: &str = "dependencies:c";

// ──────────────────────────────────────────────
// Jetpack (Phase 1) — user-typeable surface (I7).
// All decisions ratified in docs/spec/syntax-decisions.md (D-JPK*).
// These IDs start with `D`, so tests/decisions.rs leaves them alone, but
// I7 still wants every typeable token to live here with its decision ID.
// ──────────────────────────────────────────────

/// D-JPK1/9: the Jetpack package-manager binary name.
pub const JETPACK_BINARY_NAME: &str = "jetpack";

/// U1 (D-JPK20) / U10: the Jet **package manifest** is `payload.jet`
/// (`PAYLOAD_FILE`; Cargo.toml analog, replaces `jet.toml`). The old
/// `pack.jet`/`PACK_FILE` spelling was retired in the U10 rename (clean break,
/// no alias). `PACK_LOCK_FILE` is superseded by `.jet/lock` (U2/S52).
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
// Owner-ratified design-of-record: docs/plans/jetpack-jetos/unified-ecosystem.md
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

/// U10 (ratified 2026-06-16; amends U1): the package manifest is `payload.jet`
/// (renamed from `pack.jet`/`PACK_FILE` — a clean break, no alias). A payload is
/// a collection of packages; its identity block is `payload: { … }`.
pub const PAYLOAD_FILE: &str = "payload.jet";

/// U10 (ratified 2026-06-16): manifest identity block keyword — `payload: { name,
/// version, … }` (was `package:`).
pub const MANIFEST_BLOCK_PAYLOAD: &str = "payload";

/// U10 (ratified 2026-06-16): the block listing a payload's packages —
/// `packages: { name: kind }`. Each `name` is a top-level `module` (the package),
/// discovered by name in the tree; the old `exports: [module …]` folds into this.
pub const MANIFEST_BLOCK_PACKAGES: &str = "packages";

/// U10 (ratified 2026-06-16): a package's kind. `library` is imported for its
/// code; `executable` installs a binary on PATH (the devshell case). Written as a
/// bare keyword (`deploy: executable`) or inside a `{ kind: … }` block.
pub const PACKAGE_KIND_LIBRARY: &str = "library";
pub const PACKAGE_KIND_EXECUTABLE: &str = "executable";

/// U10 (ratified 2026-06-16): the per-package block field naming its kind —
/// `deploy: { kind: executable, … }`.
pub const PACKAGE_FIELD_KIND: &str = "kind";

/// D-REL3 (ratified 2026-06-16): the project compatibility marker —
/// `edition: "2026"` in the `payload: { … }` block of `payload.jet`. An edition
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
