//! OWNER-CONTROLLED SURFACE.
//!
//! Every keyword, sigil, and built-in name a user can type lives in this
//! file and nowhere else (invariant I7). Each constant maps to a decision
//! ID in docs/02-syntax-decisions.md. Changing a provisional choice means:
//! change it here, update docs/02, re-bless the ui snapshots. Done.
//!
//! Agents: do NOT add an entry here without a decision ID approved by the
//! owner in docs/02-syntax-decisions.md.

/// N1 (ratified): language name.
pub const LANG_NAME: &str = "Jet";

/// N1 (ratified): compiler binary name.
pub const BINARY_NAME: &str = "jet";

/// N2 (ratified): source file extension (without the dot).
pub const FILE_EXT: &str = "jet";

/// S1 (ratified): keyword that starts a function definition.
pub const KW_FN: &str = "fn";

/// S18 (ratified): marks an item as visible to other files (via import).
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

/// M2 tier 2: unsafe block for expert code.
pub const KW_UNSAFE: &str = "unsafe";

/// M5 preview: list type constructor.
pub const TYPE_LIST: &str = "List";

/// M2: shared handle type (Arc equivalent); auto-cloned across boundaries.
pub const TYPE_SHARED: &str = "Shared";

/// M1 (docs/05, owner-blessed examples 2026-06-11): branching keywords.
pub const KW_IF: &str = "if";
pub const KW_ELSE: &str = "else";

/// S19 (ratified): loop keywords.
pub const KW_WHILE: &str = "while";
pub const KW_FOR: &str = "for";
pub const KW_IN: &str = "in";

/// S22 (ratified): inclusive range between two `Int` ends — `1..10`.
pub const OP_RANGE: &str = "..";

/// S23 (ratified): loop control.
pub const KW_BREAK: &str = "break";
pub const KW_CONTINUE: &str = "continue";

/// S24 (ratified): many-way choice with condition arms.
pub const KW_SWITCH: &str = "switch";

/// S24 (ratified): arm arrow inside `switch` (same spelling as return types).
pub const OP_ARM_ARROW: &str = "->";

/// S11 (ratified): the two `Bool` literals.
pub const LIT_TRUE: &str = "true";
pub const LIT_FALSE: &str = "false";

/// M1 (docs/05): arithmetic operators. `+ - * /` on Int and Float;
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

/// S16 (ratified M6): file path or module name import; optional `as`.
pub const KW_IMPORT: &str = "import";
pub const KW_AS: &str = "as";

/// S14: foreign forms recognized only for teaching errors.
pub const FOREIGN_TRY: &str = "try";
pub const FOREIGN_LET: &str = "let";
pub const FOREIGN_LET_MUT: &str = "let mut";
pub const FOREIGN_SET: &str = "set";
pub const FOREIGN_FUNC: &str = "func";
pub const FOREIGN_DEF: &str = "def";
pub const FOREIGN_USE: &str = "use";
pub const FOREIGN_PRINTLN: &str = "println";
pub const FOREIGN_TEXT: &str = "Text";

/// S24: `match` recognized only for a teaching error naming `switch`.
pub const FOREIGN_MATCH: &str = "match";

/// S32 (ratified M3): foreign optional spellings for teaching error E0020.
pub const FOREIGN_NONE: &str = "None";
pub const FOREIGN_SOME: &str = "Some";
pub const FOREIGN_NIL: &str = "nil";
pub const FOREIGN_NONE_LOWER: &str = "none";
pub const FOREIGN_SOME_LOWER: &str = "some";

/// S29 (ratified M3): `class` recognized only for teaching error E0021.
pub const FOREIGN_CLASS: &str = "class";

/// S28 (deferred M9): foreign trait spellings for teaching error E0022.
pub const FOREIGN_INTERFACE: &str = "interface";
pub const FOREIGN_TRAIT: &str = "trait";

/// S24 (ratified M3): foreign switch arm spellings for teaching error E0023.
pub const FOREIGN_CASE: &str = "case";
pub const FOREIGN_DEFAULT: &str = "default";

/// S10 (ratified M2): foreign read/write forms for teaching errors.
pub const FOREIGN_READ: &str = "read";
pub const FOREIGN_WRITE: &str = "write";
pub const FOREIGN_OWNED: &str = "owned";
