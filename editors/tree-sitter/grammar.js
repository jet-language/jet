// tree-sitter grammar for Jet.
//
// Tracks the canonical syntax in Source/Syntax.rs (keyword/sigil/marker
// constants, each with a decision ID — invariant I7) plus the real code in
// examples/features/*.jet. This grammar exists for editor syntax highlighting
// (Zed), not full compilation: it is permissive where the real parser is
// strict, but every keyword/sigil/marker it names is a *current* one. Retired
// spellings (`val`/`var`, `mut`/`take`/`view`, `when`/`switch`/`while`/
// `for`, bare `test`/`pure`/`todo`) are deliberately NOT recognized here.
//
// Run `tree-sitter generate` in this directory after editing, then rebuild the
// wasm via editors/zed/install.sh (FORCE=1).

// BEGIN GENERATED JET SYNTAX HIGHLIGHTS
const JET_HIGHLIGHT_KEYWORD_CONTROL = ["break", "continue", "else", "if", "in", "loop", "return", "step"];
const JET_HIGHLIGHT_KEYWORD_DECLARATION = ["Bench", "Context", "Impure", "Pure", "Reactive", "Sanitizer", "State", "Tainted", "Test", "Todo", "Transact", "Transition", "Unsafe", "add", "alias", "as", "assume_deterministic", "change", "client", "comptime", "const", "derive", "distinct", "enum", "extern", "fn", "impl", "live", "migration", "module", "policy", "priv", "protocol", "pub", "region", "remove", "rename", "rust", "server", "state", "struct", "tag", "taskgroup", "trait", "use", "via"];
const JET_HIGHLIGHT_KEYWORD_OWNERSHIP = ["copy", "uninit"];
const JET_HIGHLIGHT_KEYWORD_OTHER = ["it", "self"];
const JET_HIGHLIGHT_LITERAL = ["None", "Val", "err", "false", "ok", "true"];
const JET_HIGHLIGHT_TYPE_BUILTIN = ["BTreeMap", "BigInt", "BitSet", "Bool", "Budget", "BudgetApplies", "ByteBuffer", "Char", "Computed", "Csv", "DataTree", "DbValue", "Decimal", "Deque", "Derived", "Effect", "Error", "Event", "EventPolicy", "EventScope", "EventTrace", "F32", "F64", "Float", "HashMap", "Hook", "I16", "I32", "I64", "I8", "IOError", "Int", "JSON", "JSONError", "Json", "Key", "Lru", "Measurement", "PriorityQueue", "Ptr", "SelectBuilder", "Set", "Shared", "Signal", "SortedSet", "Stream", "String", "Subscription", "TaskGroup", "Toml", "U16", "U32", "U64", "U8", "UTF8Error", "Void", "WatchEvent", "WatchHandle", "WatchSet", "Yaml"];
const JET_HIGHLIGHT_BUILTIN = ["input", "print"];
const JET_HIGHLIGHT_MARKER_DIRECTIVE = ["Bench", "Bindgen", "Caller", "Caps", "DebugOnly", "Default", "DenyUnknownFields", "Extern", "Flatten", "Grant", "Html", "Impure", "Invariant", "Js", "Layout", "Meta", "Off", "Reactive", "Rename", "RenameAll", "Replayable", "Sanitizer", "SingleUse", "Skip", "Sql", "State", "Suppress", "Tag", "Tainted", "Target", "Test", "Todo", "Track", "Transact", "Transition", "UnitFamily", "Unsafe", "Untagged", "Wasm", "WasmExport"];
const JET_HIGHLIGHT_MARKER_CONTRACT = ["Cli", "Codable", "CodableAsBase", "Comparable", "Debug", "Decode", "Doc", "Encode", "Experimental", "Hardened", "Inline", "InlineAlways", "MustUse", "Numeric", "Patchable", "Persist", "Post", "Pre", "Printable", "PublishedSchema", "Pure", "Redact", "Summarize", "Tested"];
const JET_HIGHLIGHT_SIGIL = ["#", "&", "...", "::", ":=", "@", "^"];
const JET_HIGHLIGHT_OPERATOR = ["!", "!=", "%", "%=", "&&", "&=", "*", "*=", "+", "++", "+=", "-", "--", "-=", "->", "..", ".[", ".{", "/", "/=", "<", "<<", "<<=", "<=", "==", "=>", ">", ">=", ">>", ">>=", "?", "?.", "??", "^=", "|", "|=", "||"];
// END GENERATED JET SYNTAX HIGHLIGHTS

module.exports = grammar({
  name: "jet",

  word: ($) => $.identifier,

  conflicts: ($) => [
    [$._expr, $.lambda_param],
    [$._expr, $.list_pattern],
    [$._type, $._expr],
    [$.named_type_field, $.lambda_param],
    // Inside a struct/enum/trait body, leading markers can precede either a
    // field/method-sig or a method — fork until `fn`/name disambiguates.
    [$.function_def, $.struct_field],
    [$.function_def, $.trait_method_sig],
    // `(a, b)` — destructure target vs tuple expr vs lambda params; the `::`/
    // `=>` after the `)` disambiguates.
    [$.tuple_pattern, $._expr, $.lambda_param],
    // In a bindings module body, `fn name(…)` may be an extern fn (`= "path"`)
    // or a normal fn (with a block); fork until the `=`/`{` appears.
    [$.extern_fn, $.function_def],
  ],

  // S6-R: source has no visible semicolons (the lexer inserts synthetic ones).
  // A few examples still write a trailing `;`, so treat it as an ignorable
  // separator rather than threading it through every rule.
  extras: ($) => [/\s/, ";", $.line_comment, $.doc_comment, $.block_comment],

  rules: {
    source_file: ($) => repeat($._item),

    // ── Markers / attributes (D-ATTR1/2/3, D-CASING1) ──────────────────────
    // `#Marker`, `#Marker(args)`, `#[Marker, Marker(args)]` bracket lists, and
    // `#(Effect, …)` effect annotations. Loop labels keep `@` (D-ATTR3=B).
    _marker: ($) => choice($.attribute, $.attribute_list, $.effect_set),

    attribute: ($) =>
      prec.right(
        seq("#", field("name", $.marker_name), optional($.marker_args)),
      ),

    attribute_list: ($) =>
      seq(
        "#",
        "[",
        commaSep1(seq(field("name", $.marker_name), optional($.marker_args))),
        "]",
      ),

    // `#(Io, Db)` effect set on a signature, and `#(via name)` pass-through.
    effect_set: ($) =>
      seq(
        "#",
        "(",
        commaSep1(choice(seq("via", $.identifier), $.type_identifier)),
        ")",
      ),

    marker_name: (_) => /[A-Z][a-zA-Z0-9_]*/,

    // Lowercase markers also exist: `#grant`, `#layout`, `#context`.
    _lower_marker: ($) =>
      prec.right(
        seq("#", field("name", $.lower_marker_name), optional($.marker_args)),
      ),

    lower_marker_name: (_) =>
      choice("grant", "layout", "context", "bindgen", "extern"),

    marker_args: ($) => seq("(", commaSep($._marker_arg), ")"),

    _marker_arg: ($) => choice($._expr, $.named_arg),

    named_arg: ($) => seq(field("name", $.identifier), ":", $._expr),

    // ── Top-level items ────────────────────────────────────────────────────
    // Marker-capable items (`function_def`, `struct_def`, `enum_def`,
    // `distinct_def`) carry their own leading markers; the rest take none.
    _item: ($) =>
      choice(
        $.function_def,
        $.struct_def,
        $.enum_def,
        $.impl_block,
        $.trait_def,
        $.tag_def,
        $.const_def,
        $.distinct_def,
        $.comptime_stmt,
        $.use_stmt,
        $.module_def,
        $.extern_block,
        $.test_block,
        $.bench_block,
        $.marker_decl,
        $.migration_block,
        $.config_field,
      ),

    // Top-level manifest/config record field: `payload: { … }`, `name: "x"`,
    // `system.my-host: { … }` in a `pkg.jet` / `env.jet` / `config.jet`. The
    // declarative-config dialect.
    config_field: ($) =>
      seq(
        field("key", $.config_key),
        ":",
        choice($._expr, $.record_literal),
        optional(","),
      ),

    // A dotted config key whose segments may be kebab-case (S84 dashed names):
    // `system.my-host`, `users.nate.shell`, `net.hostName`.
    config_key: (_) =>
      token(
        /[a-z_][A-Za-z0-9_]*(-[A-Za-z0-9_]+)*(\.[a-z_][A-Za-z0-9_]*(-[A-Za-z0-9_]+)*)*/,
      ),

    // A marker that introduces a top-level brace-list declaration:
    // `#UnitFamily(currency) { usd, eur }` (D-QUAL3) mints one type per member.
    marker_decl: ($) => seq($.attribute, "{", commaSep($.identifier), "}"),

    // ── Use (S16, D-MOD3) ────────────────────────────────────────────────────
    // `use core.encoding.json as json`, `use "./file.jet"`, or a group
    // `use math.{double, triple}`.
    use_stmt: ($) =>
      seq(
        optional("pub"),
        "use",
        field("path", choice($.module_path, $.string_literal)),
        optional(field("group", $.use_group)),
        optional(seq("as", field("alias", $.identifier))),
      ),

    // `.{ a, b }` import-group suffix on a `use` path (D-MOD3). The whole `.{`
    // is one token so it never competes with a path-extending `.`.
    use_group: ($) =>
      seq(alias(token(/\.\s*\{/), "."), commaSep1($.identifier), "}"),

    module_path: ($) => sep1($.identifier, "."),

    // ── Module (U3, S59) ─────────────────────────────────────────────────────
    // `module name { … }`, file-scoped `module name`, or a generated-bindings
    // module `#bindgen module c.lib.__bindgen__ { fn … = "…" }` (the marker is
    // consumed by the leading `_marker` prefix; the name may be a dotted path).
    module_def: ($) =>
      seq(
        repeat(choice($._marker, $._lower_marker)),
        "module",
        field("name", $.module_path),
        optional(seq("{", repeat(choice($._item, $.extern_fn)), "}")),
      ),

    // ── Migration block (D-MIGRATE1/2) ─────────────────────────────────────
    // `migration Type { rename a -> b; add f: T = d; remove f; change f: O -> N }`.
    migration_block: ($) =>
      seq(
        "migration",
        field("type", $.type_identifier),
        "{",
        repeat($.migration_op),
        "}",
      ),

    migration_op: ($) =>
      choice(
        seq("rename", $.identifier, "->", $.identifier),
        seq("add", $.identifier, ":", $._type, optional(seq("=", $._expr))),
        seq("remove", $.identifier),
        seq(
          "change",
          $.identifier,
          ":",
          $._simple_type,
          "->",
          $._simple_type,
          optional($.via_clause),
        ),
      ),

    via_clause: ($) => seq("via", $.block),

    // A type that does not itself contain a top-level `->` (used where an `->`
    // delimiter follows, e.g. a `change Old -> New` migration op).
    _simple_type: ($) =>
      choice(
        $.primitive_type,
        $.generic_type,
        $.type_identifier,
        $.option_type,
        $.list_type,
        $.map_type,
      ),

    // ── Extern (S50) ───────────────────────────────────────────────────────
    // `extern rust "crate@ver" { fn name(…) -> T = "rust::path" }`.
    extern_block: ($) =>
      seq("extern", "rust", $.string_literal, "{", repeat($.extern_fn), "}"),

    // C/Rust binding fns keep their foreign casing, so the name may be Upper.
    extern_fn: ($) =>
      seq(
        "fn",
        field("name", choice($.identifier, $.type_identifier)),
        $.param_list,
        optional(seq("->", field("return_type", $._type))),
        optional(seq("=", field("rust_path", $.string_literal))),
      ),

    // ── Function definition (S1) ───────────────────────────────────────────
    // Leading markers (`#Pure fn`, `#Unsafe("…") fn`, `#Sanitizer fn`); trailing
    // `#(effects)` set on the signature.
    function_def: ($) =>
      seq(
        repeat(choice($._marker, $._lower_marker)),
        optional("pub"),
        "fn",
        field("name", $.identifier),
        optional($.type_params),
        $.param_list,
        optional(seq("->", field("return_type", $._type))),
        optional($.effect_set),
        $.block,
      ),

    // ── Struct definition ──────────────────────────────────────────────────
    struct_def: ($) =>
      seq(
        repeat(choice($._marker, $._lower_marker)),
        optional("pub"),
        "struct",
        field("name", $.type_identifier),
        optional($.type_params),
        "{",
        repeat(
          choice(
            $.struct_field,
            $.derive_stmt,
            $.function_def,
            $.trait_impl_block,
          ),
        ),
        "}",
      ),

    struct_field: ($) =>
      seq(
        repeat($._marker),
        optional("pub"),
        optional(seq("ref", optional(seq("[", $.identifier, "]")))),
        field("name", $.identifier),
        ":",
        field("type", $._type),
        optional(","),
      ),

    derive_stmt: ($) => seq("derive", commaSep1($.type_identifier)),

    // ── Enum definition (S30) ──────────────────────────────────────────────
    enum_def: ($) =>
      seq(
        repeat(choice($._marker, $._lower_marker)),
        optional("pub"),
        "enum",
        field("name", $.type_identifier),
        optional($.type_params),
        "{",
        repeat(
          choice(
            $.enum_variant,
            $.derive_stmt,
            $.function_def,
            $.trait_impl_block,
          ),
        ),
        "}",
      ),

    enum_variant: ($) =>
      seq(field("name", $.type_identifier), optional($.variant_payload)),

    // Tuple payload `Char(c)` or record payload `{ x: Int }`.
    variant_payload: ($) =>
      choice(
        seq("(", commaSep($._type), ")"),
        seq("{", repeat($.struct_field), "}"),
      ),

    // ── Tag definition (D-QUAL2) ───────────────────────────────────────────
    // `tag Reviewed;` or `tag Internal {}` — marker qualifier, no methods.
    tag_def: ($) =>
      seq(
        optional("pub"),
        "tag",
        field("name", $.type_identifier),
        optional(seq("{", "}")),
      ),

    // ── Impl block (S27) ───────────────────────────────────────────────────
    // `impl Type { … }`, `impl Type: Trait { … }`, delegation
    // `impl Type: Trait using field` (S62), and error-conversion
    // `impl FromErr -> ToErr { … }` (D-ERR-CONV).
    impl_block: ($) =>
      choice(
        // Conversion impl `impl FromErr -> ToErr { return … }` — body is a block.
        seq(
          "impl",
          field("type", choice($.type_identifier, $.generic_type)),
          "->",
          field("into", choice($.type_identifier, $.generic_type)),
          $.block,
        ),
        // Inherent / trait / delegation impl — body is method definitions.
        seq(
          "impl",
          field("type", choice($.type_identifier, $.generic_type)),
          optional(
            seq(
              ":",
              field("trait", $.type_identifier),
              optional(seq("using", $.identifier)),
            ),
          ),
          optional(seq("{", repeat($.function_def), "}")),
        ),
      ),

    trait_impl_block: ($) =>
      seq(
        "impl",
        field("trait", $.type_identifier),
        "{",
        repeat($.function_def),
        "}",
      ),

    // ── Trait definition (S28) ─────────────────────────────────────────────
    trait_def: ($) =>
      seq(
        optional("pub"),
        "trait",
        field("name", $.type_identifier),
        optional($.type_params),
        "{",
        repeat(choice($.function_def, $.trait_method_sig)),
        "}",
      ),

    // A method signature with no body: `fn greet(self) -> String;`, optionally
    // marked (`#Pure fn area(self) -> Int`).
    trait_method_sig: ($) =>
      seq(
        repeat($._marker),
        "fn",
        field("name", $.identifier),
        optional($.type_params),
        $.param_list,
        optional(seq("->", field("return_type", $._type))),
      ),

    // ── Const definition ───────────────────────────────────────────────────
    const_def: ($) =>
      seq(
        optional("pub"),
        "const",
        field("name", $._value_name),
        optional(seq(":", field("type", $._type))),
        "=",
        field("value", $._expr),
      ),

    // ── Distinct type (D-DIST1): `UserId :: distinct Int`, `#Numeric M :: …` ──
    distinct_def: ($) =>
      seq(
        repeat(choice($._marker, $._lower_marker)),
        field("name", $.type_identifier),
        "::",
        "distinct",
        field("base", $._type),
      ),

    // ── Test / Bench blocks (S43, D-BENCH1, D-CASING1) ─────────────────────
    test_block: ($) => seq("#", "Test", $.string_literal, $.block),
    bench_block: ($) => seq("#", "Bench", $.string_literal, $.block),

    // ── Type params / generics ─────────────────────────────────────────────
    // `<T>`, `<T, U>`, with optional trait bounds `<T: Comparable>`.
    type_params: ($) => seq("<", commaSep1($.type_param), ">"),

    type_param: ($) =>
      seq($.type_identifier, optional(seq(":", sep1($.type_identifier, "+")))),

    // ── Param list ─────────────────────────────────────────────────────────
    param_list: ($) => seq("(", commaSep(choice($.self_param, $.param)), ")"),

    // Receiver: `self`, `^self`, `&self` (D-MEM1).
    self_param: ($) => seq(optional($.capability_sigil), "self"),

    // A parameter, with an optional default value `clamp: Bool = false`.
    param: ($) =>
      seq(
        optional("ref"),
        field("name", $.identifier),
        ":",
        field("type", $._type),
        optional(seq("=", field("default", $._expr))),
      ),

    // ── Types ──────────────────────────────────────────────────────────────
    _type: ($) =>
      choice(
        $.capability_type,
        $.primitive_type,
        $.generic_type,
        $.type_identifier,
        $.option_type,
        $.fallible_type,
        $.list_type,
        $.map_type,
        $.fn_type,
        $.tuple_type,
        $.paren_type,
      ),

    // `(T, U)` or named `(max: Int, min: Int)` tuple type. At least one named
    // element or two elements, so a single `(T)` stays a `paren_type`.
    tuple_type: ($) =>
      choice(
        seq("(", $.named_type_field, repeat(seq(",", $.named_type_field)), ")"),
        seq("(", $._type, ",", commaSep1($._type), ")"),
      ),

    named_type_field: ($) => seq($.identifier, ":", $._type),

    // `^T` move, `&T` write, `*T` raw pointer (D-MEM1).
    capability_type: ($) => prec(2, seq($.capability_sigil, $._type)),

    capability_sigil: (_) => choice("^", "&", "*"),

    primitive_type: (_) =>
      choice(
        "Int",
        "Float",
        "Bool",
        "String",
        "Char",
        "Void",
        "Error",
        "I8",
        "I16",
        "I32",
        "I64",
        "U8",
        "U16",
        "U32",
        "U64",
        "F32",
        "F64",
      ),

    type_identifier: (_) => /[A-Z][a-zA-Z0-9_]*/,

    // Generics: `Pair<T>`, `Stack<Int>`. The `<` is adjacent (no space) so a
    // spaced `T < x` in expression position stays a comparison.
    generic_type: ($) =>
      seq(
        field("base", $.type_identifier),
        token.immediate("<"),
        commaSep1($._type),
        ">",
      ),

    // `T?` optional (S32).
    option_type: ($) => prec(1, seq($._type, "?")),

    // `T ? E` fallible result (S34).
    fallible_type: ($) => prec.left(seq($._type, "?", $._type)),

    // `[T]` list, `[T#N]` fixed-size (S65/S76).
    list_type: ($) =>
      choice(
        seq("[", $._type, "]"),
        seq("[", $._type, "#", $.integer_literal, "]"),
      ),

    // `[K: V]` map (D-LISTMAP-CANON1=A).
    map_type: ($) =>
      seq("[", $._type, ":", $._type, "]"),

    // `#Pure fn(T) -> U` or `#(Io) fn(T) -> U` callback type (D-EFF2).
    fn_type: ($) =>
      seq(
        optional(choice($.attribute, $.effect_set)),
        "fn",
        "(",
        commaSep($._type),
        ")",
        optional(seq("->", $._type)),
      ),

    paren_type: ($) => seq("(", $._type, ")"),

    // ── Statements ─────────────────────────────────────────────────────────
    block: ($) => seq("{", repeat($._stmt), "}"),

    _stmt: ($) =>
      choice(
        $.bind_stmt,
        $.assign_stmt,
        $.return_stmt,
        $.break_stmt,
        $.continue_stmt,
        $.loop_stmt,
        $.comptime_stmt,
        $.comptime_if_stmt,
        $.region_stmt,
        $.live_stmt,
        $.marker_block_stmt,
        $.expr_stmt,
      ),

    // A marker-introduced block: `#Caps(Io) { … }` (D-EFF1), `#grant(Fs) { caps
    // -> … }` (D-SCAP1), `#Transact(order) { … }` (D-TXN4), `#context(…) { … }`.
    marker_block_stmt: ($) =>
      seq(choice($.attribute, $._lower_marker), $.scoped_block),

    scoped_block: ($) =>
      seq(
        "{",
        optional(seq(field("handle", $.identifier), "->")),
        repeat($._stmt),
        "}",
      ),

    // Binding sigils (D-BIND4): `name :: expr` immutable, `name := expr` mutable.
    bind_stmt: ($) =>
      seq(
        field(
          "name",
          choice(
            $.identifier,
            $.type_identifier,
            $.list_pattern,
            $.tuple_pattern,
          ),
        ),
        optional(seq(":", field("type", $._type))),
        choice("::", ":="),
        field("value", $._expr),
      ),

    // Destructuring bind targets: `[a, b, c] :: …`, `(a, b) :: …`.
    list_pattern: ($) => seq("[", commaSep($.identifier), "]"),
    tuple_pattern: ($) =>
      seq("(", $.identifier, repeat(seq(",", $.identifier)), ")"),

    assign_stmt: ($) =>
      seq(
        field("target", $._expr),
        choice(
          "=",
          "+=",
          "-=",
          "*=",
          "/=",
          "%=",
          "&=",
          "|=",
          "^=",
          "<<=",
          ">>=",
        ),
        field("value", $._expr),
      ),

    return_stmt: ($) => prec.right(seq("return", optional($._expr))),

    dispatch_arm: ($) =>
      seq(field("pattern", $._expr), "->", choice($.block, seq($._expr))),

    dispatch_else: ($) => seq("else", "->", choice($.block, seq($._expr))),

    break_stmt: ($) => prec.right(seq("break", optional($.loop_label))),
    continue_stmt: ($) => prec.right(seq("continue", optional($.loop_label))),

    // `loop { }`, `loop cond { }`, `loop x in iter { }`, optional `@label`.
    loop_stmt: ($) =>
      seq(optional($.loop_label), "loop", optional($._loop_head), $.block),

    _loop_head: ($) =>
      choice(
        seq(
          field("var", $.identifier),
          optional(seq(",", field("var2", $.identifier))),
          "in",
          field("iter", $._expr),
          optional(seq("step", $._expr)),
        ),
        field("cond", $._expr),
      ),

    loop_label: (_) => /@[a-z_][a-zA-Z0-9_]*/,

    comptime_stmt: ($) =>
      seq(
        "comptime",
        field("name", $._value_name),
        optional(seq(":", field("type", $._type))),
        choice("::", ":=", "="),
        field("value", $._expr),
      ),

    // Binding names are usually lower_snake, but constants are UPPER (a
    // type_identifier lexically) — accept either for const/comptime targets.
    _value_name: ($) => choice($.identifier, $.type_identifier),

    comptime_if_stmt: ($) =>
      seq(
        "comptime",
        "if",
        field("cond", $._expr),
        $.block,
        optional(seq("else", choice($.comptime_if_stmt, $.block))),
      ),

    // Expert region/live/assume_deterministic blocks (contextual keywords).
    region_stmt: ($) => seq("region", optional($.identifier), $.block),
    live_stmt: ($) => seq(choice("live", "assume_deterministic"), $.block),

    expr_stmt: ($) => seq($._expr),

    // ── Expressions ────────────────────────────────────────────────────────
    _expr: ($) =>
      choice(
        $.integer_literal,
        $.float_literal,
        $.boolean_literal,
        $.null_literal,
        $.ok_err_literal,
        $.multiline_string,
        $.string_literal,
        $.char_literal,
        $.identifier,
        $.type_identifier,
        $.copy_expr,
        $.primitive_type,
        $.call_expr,
        $.turbofish_call,
        $.struct_literal,
        $.list_literal,
        $.map_literal,
        $.tuple_expr,
        $.method_call_expr,
        $.field_expr,
        $.deref_expr,
        $.fan_out_expr,
        $.lambda_expr,
        $.binary_expr,
        $.unary_expr,
        $.capability_expr,
        $.try_expr,
        $.paren_expr,
        $.index_expr,
        $.range_expr,
        $.if_expr,
        $.marked_expr,
        $.generic_access,
        $.source_ref,
      ),

    // A `provider@target` source/dependency ref (U6, S59): `c@system`,
    // `path@./jet-pkgs`, `github@NixOS/nixpkgs/nixos-24.05`, `c@"vendor"`.
    source_ref: ($) =>
      seq(
        field("provider", $.identifier),
        token.immediate("@"),
        field("target", choice($.string_literal, $.ref_target)),
      ),

    ref_target: (_) => token.immediate(/[A-Za-z0-9_./:-]+/),

    // A value-fact marker riding an expression: `#Tainted input` (D-TAINT1), or
    // a bare marker value such as the typed hole `#Todo` (D-TOOL2).
    marked_expr: ($) => prec.right(seq($.attribute, optional($._expr))),

    copy_expr: ($) => prec.right(6, seq("copy", $._expr)),

    // A type-applied member access: `mem.Ptr<Int>.from_addr(…)` — a generic
    // applied to a path before a further `.method`. The `<` is adjacent.
    generic_access: ($) =>
      prec.left(4, seq($._expr, token.immediate("<"), commaSep1($._type), ">")),

    // `if` as a value (S/D-IF): `x :: if c { a } else { b }`. Both the plain and
    // the `== { … }` dispatch forms can produce a value.
    if_expr: ($) =>
      prec.right(
        choice(
          seq(
            "if",
            field("cond", $._expr),
            field("then", $.block),
            optional(seq("else", choice($.if_expr, $.block))),
          ),
          seq(
            "if",
            field("subject", $._expr),
            "==",
            "{",
            repeat($.dispatch_arm),
            optional($.dispatch_else),
            "}",
          ),
        ),
      ),

    integer_literal: (_) =>
      token(
        choice(
          /0x[0-9a-fA-F][0-9a-fA-F_]*/,
          /0o[0-7][0-7_]*/,
          /0b[01][01_]*/,
          /[0-9][0-9_]*/,
        ),
      ),
    float_literal: (_) => token(/[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9]+)?/),
    boolean_literal: (_) => choice("true", "false"),
    null_literal: (_) => "None",
    ok_err_literal: (_) => choice("ok", "err"),
    char_literal: (_) => token(seq("'", /[^'\\]|\\./, "'")),

    string_literal: ($) =>
      seq(
        '"',
        repeat(
          choice($.escape_sequence, $.string_interpolation, $._string_content),
        ),
        '"',
      ),

    // Triple-quoted multiline string `"""…"""` (D-SG5); interpolation stays live.
    multiline_string: ($) =>
      seq(
        '"""',
        repeat(
          choice(
            $.escape_sequence,
            $.string_interpolation,
            $._ml_string_content,
          ),
        ),
        '"""',
      ),

    _ml_string_content: (_) =>
      token.immediate(prec(1, /([^"\\{}]|"[^"]|""[^"])+/)),

    _string_content: (_) => token.immediate(prec(1, /[^"\\{}]+/)),

    escape_sequence: (_) =>
      token.immediate(/\\["\\/bfnrt0]|\\u\{[0-9a-fA-F]+\}|\{\{|\}\}/),

    string_interpolation: ($) => seq("{", $._expr, "}"),

    identifier: (_) => /[a-z_][a-zA-Z0-9_]*/,

    // `name(args)`, `Type(args)` constructor / distinct wrap, and `ok(x)`/`err(e)`
    // result constructors (S34) which also appear as dispatch-arm patterns.
    call_expr: ($) =>
      prec(
        2,
        seq(
          field(
            "name",
            choice(
              $.identifier,
              $.type_identifier,
              $.primitive_type,
              $.ok_err_literal,
            ),
          ),
          $.arg_list,
        ),
      ),

    // Turbofish: `decode<Order>(raw)`, `make_pair<Int>(…)`. The `<` is adjacent
    // (no space) so a spaced comparison `c < 3` stays a `binary_expr`.
    turbofish_call: ($) =>
      prec(
        3,
        seq(
          field("name", choice($.identifier, $.type_identifier)),
          token.immediate("<"),
          commaSep1($._type),
          ">",
          $.arg_list,
        ),
      ),

    // The opening `(` must be adjacent to the callee (no whitespace) so a
    // parenthesized expression on the next line is not read as a call.
    arg_list: ($) =>
      seq(token.immediate("("), commaSep(choice($.named_arg, $._expr)), ")"),

    // `Type { field: value, … }` / `Type<T> { … }` struct literal. Negative
    // precedence so that after `if`/`loop`/`comptime if` the trailing `{ … }` is
    // taken as the block, not a struct literal on a bare type-name condition.
    // Fields are separated by commas or newlines (S6-R), so the comma is
    // optional between them.
    struct_literal: ($) =>
      prec(
        -1,
        seq(
          field("name", choice($.type_identifier, $.generic_type)),
          $.record_body,
        ),
      ),

    // An anonymous record literal `{ key: value, … }` (used as a config/manifest
    // value, e.g. `payload: { name: "x", version: "1.0" }`).
    record_literal: ($) => $.record_body,

    record_body: ($) =>
      seq(
        "{",
        repeat(
          seq(
            field("field", $.identifier),
            ":",
            choice($._expr, $.record_literal),
            optional(","),
          ),
        ),
        "}",
      ),

    list_literal: ($) => seq("[", commaSep($._expr), "]"),

    // Map literal: `[:]` empty, or `[k: v, …]`.
    map_literal: ($) =>
      choice(
        seq("[", ":", "]"),
        seq(
          "[",
          commaSep1(seq(field("key", $._expr), ":", field("value", $._expr))),
          "]",
        ),
      ),

    // `(a, b)` tuple, or named `(min: 0, max: 10)` tuple literal.
    tuple_expr: ($) =>
      choice(
        seq("(", $.named_arg, repeat(seq(",", $.named_arg)), ")"),
        seq("(", $._expr, ",", commaSep1($._expr), ")"),
      ),

    // `recv.method(…)` and qualified constructor `Enum.Variant(…)` /
    // `mod.Type.new(…)` — the member may be lower (method) or Upper (variant).
    method_call_expr: ($) =>
      prec.left(
        4,
        seq(
          field("receiver", $._expr),
          choice(".", "?."),
          field("method", choice($.identifier, $.type_identifier)),
          $.arg_list,
        ),
      ),

    // `obj.field`, `Enum.Variant`, `Type.CONST`, `mod.Type` — member is lower or
    // Upper (qualified enum-variant / associated-const / nested-type access).
    field_expr: ($) =>
      prec.left(
        4,
        seq(
          field("object", $._expr),
          choice(".", "?."),
          field("field", choice($.identifier, $.type_identifier)),
        ),
      ),

    // Postfix deref `p.*` (D-CAP7).
    deref_expr: ($) => prec.left(4, seq($._expr, ".", "*")),

    // Fan-out `f.[a, b, c]` (S75) — `.[` is adjacency-detected (no space).
    fan_out_expr: ($) =>
      prec.left(
        4,
        seq(
          field("fn", $._expr),
          ".",
          token.immediate("["),
          commaSep1($._expr),
          "]",
        ),
      ),

    lambda_expr: ($) =>
      prec.right(
        seq(
          optional($.capture_clause),
          "(",
          commaSep($.lambda_param),
          ")",
          "=>",
          choice($.block, $._expr),
        ),
      ),

    // Move-capture prefix on a lambda: `take(sender, router) () => { … }`.
    capture_clause: ($) => seq("take", "(", commaSep($.identifier), ")"),

    lambda_param: ($) =>
      seq(field("name", $.identifier), optional(seq(":", $._type))),

    // Call-site capability sigils: `^x`, `&x`, `*x` (D-MEM1).
    capability_expr: ($) => prec.right(6, seq($.capability_sigil, $._expr)),

    binary_expr: ($) => {
      const table = [
        ["||", 1],
        ["&&", 2],
        ["??", 3],
        ["==", 4],
        ["!=", 4],
        ["<", 4],
        [">", 4],
        ["<=", 4],
        [">=", 4],
        ["|", 5],
        ["^", 5],
        ["&", 5],
        ["<<", 6],
        [">>", 6],
        ["+", 7],
        ["-", 7],
        ["*", 8],
        ["/", 8],
        ["%", 8],
      ];
      return choice(
        ...table.map(([op, prc]) =>
          prec.left(
            prc,
            seq(
              field("left", $._expr),
              field("op", op),
              field("right", $._expr),
            ),
          ),
        ),
      );
    },

    // `lo..hi` inclusive range (S22).
    range_expr: ($) =>
      prec.left(5, seq($._expr, "..", $._expr, optional(seq("step", $._expr)))),

    unary_expr: ($) => prec.right(9, seq(choice("-", "!"), $._expr)),

    // Postfix `?` propagation (S7).
    try_expr: ($) => prec.left(5, seq($._expr, "?")),

    paren_expr: ($) => seq("(", $._expr, ")"),

    // `xs[i]` — `[` must be adjacent (no whitespace), so a list literal on the
    // next line is not swallowed as an index (source has no `;` terminators).
    index_expr: ($) =>
      prec.left(4, seq($._expr, token.immediate("["), $._expr, "]")),

    // ── Comments (S5) ──────────────────────────────────────────────────────
    line_comment: (_) => token(seq("//", /[^/].*/)),
    doc_comment: (_) => token(seq("///", /.*/)),
    block_comment: (_) => token(seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}

function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}
