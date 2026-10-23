// tree-sitter grammar for Jet.
//
// Tracks the canonical syntax registry in Compiler/JetFoundation/Data/Syntax/
// (keyword/sigil/marker constants, each with a decision ID — invariant I7) plus the real code in
// Examples/features/*.jet. This grammar exists for editor syntax highlighting
// (Zed), not full compilation: it is permissive where the real parser is
// strict, but every keyword/sigil/marker it names is a *current* one. Retired
// spellings (`val`/`var`, `mut`/`take`/`view`, `when`/`switch`/`while`/
// `for`, bare `test`/`pure`/`todo`, and retired failure spellings are
// deliberately NOT recognized here.
//
// Run `tree-sitter generate` in this directory after editing, then rebuild the
// wasm via Tools/editors/zed/install.sh (FORCE=1).

// BEGIN GENERATED JET SYNTAX HIGHLIGHTS
const JET_HIGHLIGHT_KEYWORD_CONTROL = ["after", "break", "defer", "else", "if", "in", "loop", "next", "return", "task", "task.all", "task.any", "task.group", "task.race", "yield"];
const JET_HIGHLIGHT_KEYWORD_DECLARATION = ["Context", "Impure", "Reactive", "Scrub", "State", "Test", "Todo", "Transact", "Transition", "Unsafe", "add", "alias", "as", "change", "client", "derive", "distinct", "effect", "enum", "extern", "fact", "fn", "impl", "marker", "migration", "module", "policy", "priv", "protocol", "pub", "remove", "rename", "rust", "server", "state", "struct", "tag", "trait", "use", "validate", "via", "wrap"];
const JET_HIGHLIGHT_KEYWORD_OWNERSHIP = ["uninit"];
const JET_HIGHLIGHT_KEYWORD_OTHER = ["$build", "$package", "$phase", "$program", "@SOURCE", "@TYPES", "it", "prep", "self", "shared"];
const JET_HIGHLIGHT_LITERAL = ["Cancelled", "DeadlineBlown", "None", "Panicked", "Val", "false", "true"];
const JET_HIGHLIGHT_TYPE_BUILTIN = ["()", "BTreeMap", "Bits", "Bool", "Budget", "BudgetApplies", "Bytes", "CSV", "Cache", "Char", "Complex", "Computed", "Condition", "DBValue", "Decimal", "Derived", "Duration", "Dyn", "Effect", "EncodingError", "Err", "Event", "EventPolicy", "EventScope", "EventTrace", "F32", "F64", "Float", "HashMap", "Hook", "I16", "I32", "I64", "I8", "IOError", "Instant", "Int", "Iter", "JSON", "Key", "Measurement", "MemoStats", "PriorityQueue", "Ptr", "Queue", "Rank", "Receiver", "Sender", "Set", "Shared", "Shared.Weak", "SharedGuard", "SharedRevisionError", "SharedSnapshot", "Signal", "Stream", "String", "Subscription", "TOML", "Task", "TaskFailure", "U16", "U32", "U64", "U8", "UTF8Error", "ViewIter", "WatchEvent", "WatchHandle", "WatchSet", "YAML"];
const JET_HIGHLIGHT_BUILTIN = ["assert", "assert_eq", "channel", "check", "freeze", "input", "join", "print"];
const JET_HIGHLIGHT_MARKER_RULE = ["ABI", "Arithmetic", "Bindgen", "CLI", "Close", "Codable", "CodableAsBase", "Commutative", "Comparable", "Context", "Debug", "DebugOnly", "Decode", "DenyUnknownFields", "Deprecated", "DevPanel", "Discriminant", "Doc", "Encode", "Env", "Equatable", "Error", "Every", "FFI", "FX", "Flag", "Flatten", "HTML", "Impure", "Inline", "Interrupt", "Job", "Kernel", "Layout", "Live", "Local", "Memo", "Meta", "MustUse", "NoPrelude", "Nondeterministic", "Numeric", "Off", "Patchable", "Persist", "Policy", "Post", "Pre", "Printable", "PublishedSchema", "Reactive", "Receipt", "Redact", "Region", "Rename", "RenameAll", "Replayable", "Root", "SQL", "Scalar", "Scrub", "Shared", "Shield", "Short", "SingleUse", "Skip", "State", "Static", "Target", "Test", "Todo", "Track", "Transact", "Transition", "Undo", "UnitFamily", "Unsafe", "Untagged", "WasmExport", "allow", "wire"];
const JET_HIGHLIGHT_SIGIL = ["#", "$", "&", "...", "::", ":=", ":>", "<:", "^"];
const JET_HIGHLIGHT_OPERATOR = ["!", "!=", "%", "%%", "%%=", "%=", "&&", "*", "*=", "+", "+=", "-", "-=", "->", "..", "..<", ".[", "/", "/%", "/%=", "/=", "<", "<<", "<<=", "<=", "<=>", "==", ">", ">=", ">>", ">>=", "?", "?.", "??", "|", "||"];
// END GENERATED JET SYNTAX HIGHLIGHTS


// ── Design ────────────────────────────────────────────────────────────────────
// Editor grammar, not the compiler's parser. Jet source is a flat sequence of
// elements inside balanced (), [], {} and string/interpolation groups; only the
// shapes an editor colors, folds, indents or outlines get their own nodes
// (declarations, calls, members, bindings, labels, constructors, variants,
// markers, literals). Everything else is a loose element, so any current or
// future Jet syntax parses without ERROR nodes as long as its brackets balance,
// and the compiler stays the only authority on what is valid.
//
// Word keywords are split by the D-HL1 registry rules above:
// - lexer-reserved words named in the generated Zed query are anonymous
//   tokens (`fn`, `if`, `struct`, `true`, `Int`, ...);
// - the other reserved/statement words (`defer`, `next`, `task`, `as`, ...)
//   are the named `keyword` token, never anonymous, so the registry's
//   anonymous-literal guard keeps holding;
// - contextual declaration words (`state`, `protocol`, `marker`, ...) stay
//   identifiers and are colored only in declaration shape (`decl_head`:
//   a word directly followed by a type name).

const PREC = {
  head: 1,
  label: 2,
  member: 2,
  call: 3,
  binding: 4,
  handler: 5,
};

// Anonymous lowercase keywords that may still name a member after `.`.
const MEMBER_KEYWORDS = [
  "break", "else", "if", "in", "loop", "return", "derive", "enum", "extern",
  "fn", "impl", "module", "pub", "struct", "tag", "trait", "use", "self",
];

const OPERATORS = [
  "!", "!=", "%", "%%", "%%=", "%=", "&&", "&=", "*", "*=", "+", "+=", "-",
  "-=", "->", "..", "..<", "/", "/%", "/%=", "/=", "<", "<<", "<<=", "<=",
  "<=>", "==", ">", ">=", ">>", ">>=", "?.", "??", "^=", "|", "|=", "||",
  "~|", "~|=", "=", "=>", "&", "^", "~", "...", ":>", "<:", "#", "$", "@",
];

module.exports = grammar({
  name: "jet",

  word: ($) => $.identifier,

  extras: ($) => [/\s/, ";", $.line_comment, $.doc_comment, $.block_comment],

  conflicts: ($) => [
    // `[K: V]` is a map type; any other bracket group is a list/index/type list.
    [$._type, $._element],
    // `subject ? ok -> … ! error -> …` versus a lone postfix `?` (`T?`) that
    // happens to precede an identifier on the next line.
    [$._element, $.result_handler_expr],
  ],

  rules: {
    // D-ECO-INLINEPACKAGE1=A: one optional leading inline package record.
    source_file: ($) => seq(optional($.inline_package), repeat($._item)),

    // D-ECO-INLINEPACKAGE1=A: the carrier is contextual source structure.
    // PackageFacts owns its field grammar; tree-sitter only recognizes the
    // balanced record shell so editor spans remain intact.
    inline_package: ($) => seq("package", $.record_literal),

    record_literal: ($) => seq("{", repeat($._item), "}"),

    _item: ($) => $._element,

    _element: ($) =>
      choice(
        $.function_def,
        $.struct_def,
        $.enum_def,
        $.trait_def,
        $.impl_block,
        $.tag_def,
        $.module_def,
        $.decl_head,
        $.binding,
        $.label,
        $.call_expr,
        $.constructor,
        $.member_expr,
        $.method_call_expr,
        $.variant,
        $.result_handler_expr,
        $.paren_expr,
        $.bracket_expr,
        $.map_type,
        $.list_import,
        $.effect_row,
        $.attribute_list,
        $.block,
        $.attribute,
        $.lower_attribute,
        $.meta_name,
        $.keyword,
        $.control_keyword,
        $.self,
        $.boolean,
        $.primitive_type,
        $.type_identifier,
        $.identifier,
        $.string_literal,
        $.multiline_string,
        $.raw_string,
        $.char_literal,
        $.number_literal,
        $._keyword_word,
        $._operator,
        ",",
        ":",
        "::",
        ":=",
        ".",
        "?",
        $._unknown,
      ),

    // ── Declarations ─────────────────────────────────────────────────────────
    // `fn name<T>(params) -> Ret E! -[Effects]> { body }`; a signature with no
    // body (trait methods) or an extern `= "symbol"` ends at the first
    // non-signature element. `fn(Int) -> Int` is the anonymous function type.
    function_def: ($) =>
      prec.right(
        seq(
          "fn",
          optional(
            seq(
              // `fn Point.render(self)` defines a method of an existing type.
              optional(seq(field("owner", $.type_identifier), ".")),
              field("name", choice($.identifier, $.type_identifier, $.meta_name)),
            ),
          ),
          optional(field("type_parameters", $.type_parameters)),
          field("parameters", $.parameters),
          repeat($._signature_part),
          optional(field("body", $.block)),
        ),
      ),

    type_parameters: ($) => seq(choice("<", IMM("<")), repeat($._type_part), ">"),

    parameters: ($) => seq(LPAREN(), repeat($._element), ")"),

    _signature_part: ($) =>
      choice(
        $.primitive_type,
        $.type_identifier,
        $.identifier,
        $.effect_row,
        $.map_type,
        $.bracket_expr,
        $.paren_expr,
        $.attribute,
        "->",
        ".",
        "?",
        "!",
        "|",
        "&",
        "^",
        "~",
        ":",
        $.type_parameters,
      ),

    _type_part: ($) =>
      choice(
        $.primitive_type,
        $.type_identifier,
        $.identifier,
        $.map_type,
        $.bracket_expr,
        $.paren_expr,
        $.variant,
        $.type_parameters,
        $.number_literal,
        ",",
        ":",
        ".",
        "?",
        "!",
        "|",
        "&",
        "^",
        "+",
        "*",
        "/",
        "-",
        "->",
        "..",
        "=",
      ),

    struct_def: ($) => declaration($, "struct"),
    enum_def: ($) => declaration($, "enum"),
    trait_def: ($) => declaration($, "trait"),
    tag_def: ($) => declaration($, "tag"),

    // `impl Type.Trait { … }`, `impl Trait for Type { … }`.
    impl_block: ($) =>
      prec.right(
        seq(
          "impl",
          optional(field("type_parameters", $.type_parameters)),
          field("type", $._head_name),
          repeat($._head_part),
          optional(field("body", $.block)),
        ),
      ),

    // `module name { … }`, `module name<types>(values) { … }`.
    module_def: ($) =>
      prec.right(
        seq(
          "module",
          field("name", choice($.identifier, $.type_identifier)),
          repeat($._head_part),
          optional(field("body", $.block)),
        ),
      ),

    _head_name: ($) => choice($.type_identifier, $.primitive_type, $.identifier, $.meta_name),

    _head_part: ($) =>
      choice($.type_parameters, $.paren_expr, $.bracket_expr, $.identifier, $.type_identifier, $.primitive_type, $.attribute, ".", ":", ",", "|", "+", "="),

    // Contextual declaration words (`state`, `protocol`, `marker`, `alias`,
    // `migration`, `policy`, `client`, `server`, …) directly before a type name.
    decl_head: ($) =>
      prec(
        PREC.head,
        seq(field("keyword", $.identifier), field("name", $.type_identifier)),
      ),

    // ── Bindings, labels, calls, members ─────────────────────────────────────
    // `name :: value`, `name := value`, `LIMIT :: prep { … }`, `outer :: loop`.
    binding: ($) =>
      prec(
        PREC.binding,
        seq(
          field("name", choice($.identifier, $.type_identifier, $.lower_attribute, $.meta_name)),
          choice("::", ":="),
        ),
      ),

    // `field: Type`, `key: value`, named arguments and manifest keys.
    label: ($) => prec(PREC.label, seq(field("name", $._label_name), ":")),

    // Reserved words still name record fields and arguments (`tag: "x"`).
    _label_name: ($) =>
      prec(PREC.label, choice(
        $.identifier,
        alias($.keyword, $.identifier),
        alias($.control_keyword, $.identifier),
        ...MEMBER_KEYWORDS.map((word) => alias(word, $.identifier)),
      )),

    call_expr: ($) =>
      prec(
        PREC.call,
        seq(field("function", $.identifier), field("arguments", $.arguments)),
      ),

    arguments: ($) => seq(IMM("("), repeat($._element), ")"),

    // `Point(1, 2)`, `Val(x)`, `Point{x: 1}`, `[Int]{}` heads, typed literals
    // `Regex{"\d+"}`.
    constructor: ($) =>
      prec(
        PREC.call,
        seq(
          field("type", choice($.type_identifier, $.primitive_type)),
          choice(field("arguments", $.arguments), field("body", $.record_body)),
        ),
      ),

    record_body: ($) => seq(IMM("{"), repeat($._element), "}"),

    member_expr: ($) =>
      prec(PREC.member, seq(".", field("field", $._member_name))),

    method_call_expr: ($) =>
      prec(
        PREC.call,
        seq(".", field("method", $._member_name), field("arguments", $.arguments)),
      ),

    _member_name: ($) =>
      choice(
        $.identifier,
        alias($.keyword, $.identifier),
        alias($.control_keyword, $.identifier),
        alias($.lower_attribute, $.identifier),
        alias($.meta_name, $.identifier),
        ...MEMBER_KEYWORDS.map((word) => alias(word, $.identifier)),
      ),

    // `.Ok(x)`, `.Executable{…}`, `.None`, `Key.Enter`.
    variant: ($) =>
      prec.right(PREC.member, seq(".", field("name", $.type_identifier))),

    // D-RESULT-DECON2=B: one exhaustive Result handler. The contextual `?`/`!`
    // pair is told apart from postfix propagation, optional chaining, unary `!`
    // and ordinary arm arrows by the two payload-name + arrow slots; the
    // subject is the preceding element.
    result_handler_expr: ($) =>
      prec.dynamic(
        PREC.handler,
        seq(
          "?",
          field("ok", $.identifier),
          "->",
          field("success", choice($.block, $._handler_value)),
          "!",
          field("error", $.identifier),
          "->",
          field("failure", choice($.block, $._handler_value)),
        ),
      ),

    _handler_value: ($) =>
      choice(
        $.call_expr,
        $.constructor,
        $.identifier,
        $.string_literal,
        $.number_literal,
        $.boolean,
        $.paren_expr,
      ),

    // ── Groups ───────────────────────────────────────────────────────────────
    paren_expr: ($) => seq(LPAREN(), repeat($._element), ")"),

    bracket_expr: ($) => seq(LBRACKET(), repeat($._element), "]"),

    // `[String:Int]` / `[String: Int]` map types.
    map_type: ($) =>
      prec.dynamic(1, seq("[", $._type, ":", $._type, "]")),

    _type: ($) =>
      choice($.primitive_type, $.type_identifier, $.map_type, $.bracket_expr),

    // `use math.[double, triple]`.
    list_import: ($) => seq(".[", repeat($._element), "]"),

    // D-SHAPE8/D-ARROW-CONTROL1: the callable effect row `-[IO, FS]>`.
    effect_row: ($) => seq("-[", repeat($._element), "]>"),

    block: ($) => seq(LBRACE(), repeat($._element), "}"),


    // ── Markers (D-SHAPE2, D-ATTR1/2/3, D-CASING1) ──────────────────────────
    // `#Test`, `#Transition(A, B)` (arguments are the following group),
    // `#!Rule`, `#[Rule, Rule(args)]`, lowercase `@allow`, and `$sites` facts.
    attribute: (_) =>
      token(seq("#", optional("!"), /[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*/)),

    attribute_list: ($) => seq("#[", repeat($._element), "]"),

    lower_attribute: (_) => token(seq("@", /[A-Za-z_][A-Za-z0-9_]*/)),

    meta_name: (_) => token(seq("$", /[A-Za-z_][A-Za-z0-9_]*/)),

    // ── Words ────────────────────────────────────────────────────────────────
    // Reserved and statement words outside the generated anonymous set.
    control_keyword: (_) =>
      token(prec(1, choice("defer", "next", "task", "yield", "after"))),

    keyword: (_) =>
      token(
        prec(
          1,
          choice(
            "priv", "null", "const", "comptime", "copy", "move", "mutate", "it",
            "as", "distinct", "prep", "uninit", "freeze", "effect", "rust", "via",
          ),
        ),
      ),

    _keyword_word: ($) =>
      choice(
        "if", "else", "in", "loop", "break", "return",
        "derive", "extern", "pub", "use",
        // `module.id`, `tag.name`: a declaration word used as a value.
        prec(-1, alias("module", $.identifier)),
        prec(-1, alias("tag", $.identifier)),
      ),

    self: (_) => "self",

    boolean: (_) => choice("true", "false"),

    primitive_type: (_) =>
      choice(
        "Int", "Float", "Bool", "String", "Char", "Err",
        "I8", "I16", "I32", "I64", "U8", "U16", "U32", "U64", "F32", "F64",
      ),

    type_identifier: (_) => /[\p{Lu}][\p{L}\p{N}_]*/,

    identifier: (_) => /[_\p{Ll}\p{Lo}][\p{L}\p{N}_]*/,

    _operator: (_) => choice(...OPERATORS),

    // Any other single punctuation character still parses (never an ERROR).
    _unknown: (_) => token(prec(-1, /[^\s\p{L}\p{N}_"'(){}\[\]]/)),

    // ── Literals ─────────────────────────────────────────────────────────────
    // Integers, floats, radix literals and unit suffixes (`10ms`, `2.5km`).
    number_literal: (_) =>
      token(
        choice(
          /0[xX][0-9a-fA-F][0-9a-fA-F_]*/,
          /0[oO][0-7][0-7_]*/,
          /0[bB][01][01_]*/,
          /[0-9][0-9_]*(\.[0-9][0-9_]*)?([eE][+-]?[0-9]+)?([\p{L}µ°%_][\p{L}\p{N}_]*)?/,
        ),
      ),

    char_literal: (_) =>
      token(seq("'", choice(/[^'\\\n]/, /\\u\{[0-9a-fA-F]+\}/, /\\./), "'")),

    string_literal: ($) =>
      seq(
        '"',
        repeat(choice($.string_content, $.escape_sequence, $.interpolation)),
        '"',
      ),

    // Triple-quoted multiline string `"""…"""` (D-SG5); interpolation stays live.
    multiline_string: ($) =>
      seq(
        '"""',
        repeat(
          choice(
            alias($._multiline_content, $.string_content),
            alias($._multiline_quotes, $.string_content),
            $.escape_sequence,
            $.interpolation,
          ),
        ),
        '"""',
      ),

    // Raw text between backtick fences (D-RAW1): ``…`` raw strings and the
    // ```…``` inline foreign bodies (C/Rust/shell/... sources) are opaque.
    raw_string: (_) =>
      token(
        choice(
          seq("`", /[^`]*/, "`"),
          seq("``", repeat(choice(/[^`]/, /`[^`]/)), "``"),
          seq("```", repeat(choice(/[^`]/, /`[^`]/, /``[^`]/)), "```"),
          seq("````", repeat(choice(/[^`]/, /`[^`]/, /``[^`]/, /```[^`]/)), "````"),
        ),
      ),

    // Above the comment tokens: `"///"` and `"//"` are string text.
    string_content: (_) => token.immediate(prec(2, /[^"\\{]+/)),

    _multiline_content: (_) => token.immediate(prec(2, /[^"\\{]+/)),

    // Below the `"""` closer: a lone `"` or `""` inside the text.
    _multiline_quotes: (_) => token.immediate(/""?/),

    // Every backslash pair (typed literal bodies such as `Regex{"\d+"}` keep
    // theirs), `\u{…}`, and doubled braces.
    escape_sequence: (_) =>
      token.immediate(choice(/\\u\{[0-9a-fA-F]+\}/, /\\./, "{{")),

    // Inside an interpolation the enclosing string's `\"…\"` delimits a nested
    // string literal (`"{xs.contains(\"a\")}"`).
    interpolation: ($) =>
      seq(
        alias(token.immediate("{"), "{"),
        repeat(choice($._element, alias($._escaped_string, $.string_literal))),
        "}",
      ),

    _escaped_string: (_) =>
      token(seq('\\"', repeat(choice(/[^\\\n]/, /\\[^"\n]/)), '\\"')),

    line_comment: (_) => token(seq("//", /[^\n]*/)),
    doc_comment: (_) => token(prec(1, seq("///", /[^\n]*/))),
    block_comment: (_) => token(seq("/*", /[^*]*\*+([^/*][^*]*\*+)*/, "/")),
  },
});

// A delimiter that must touch the previous token (`f(`, `Type{`, `fn f<T>`),
// shown in the tree as the ordinary delimiter.
function IMM(text) {
  return alias(token.immediate(text), text);
}

function LPAREN() {
  return choice("(", IMM("("));
}

function LBRACKET() {
  return choice("[", IMM("["));
}

function LBRACE() {
  return choice("{", IMM("{"));
}

// `struct Name<T>: Bound { … }` — keyword, name, head parts, optional body.
function declaration($, keyword) {
  return prec.right(
    seq(
      keyword,
      field("name", $._head_name),
      repeat($._head_part),
      optional(field("body", $.block)),
    ),
  );
}
